use mimi_protocol::Channel;

use super::store::{self, SourceNames};
use super::{CardHandle, ContactCard};
use crate::db::Db;

fn card(record: &str, name: &str, handles: &[(Channel, &str)]) -> ContactCard {
    ContactCard {
        record: record.into(),
        name: name.into(),
        nickname: None,
        handles: handles
            .iter()
            .map(|(c, v)| CardHandle {
                channel: *c,
                value: v.to_string(),
                label: None,
            })
            .collect(),
    }
}

async fn db_with_sources(sources: &[&str]) -> Db {
    let db = Db::open_in_memory().unwrap();
    let sources: Vec<String> = sources.iter().map(|s| s.to_string()).collect();
    db.call(move |c| {
        for s in &sources {
            c.execute(
                "INSERT INTO connections (id, integration, name, config, created_at) VALUES (?1, 'caldav', ?1, '{}', 0)",
                [s],
            )?;
        }
        Ok(())
    })
    .await
    .unwrap();
    db
}

async fn sync(db: &Db, source: &str, cards: Vec<ContactCard>) -> bool {
    let source = source.to_owned();
    db.call(move |c| store::sync_source(c, &source, &cards, 1))
        .await
        .unwrap()
}

async fn everyone(db: &Db) -> Vec<(String, Vec<Channel>)> {
    db.call(|c| store::all(c))
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.summary.name, p.summary.channels))
        .collect()
}

#[tokio::test]
async fn unifies_on_shared_numbers_but_never_on_names() {
    let db = db_with_sources(&["icloud", "work"]).await;
    sync(
        &db,
        "icloud",
        vec![
            card("1", "Sam Carter", &[(Channel::Phone, "+41 79 123 45 67")]),
            card("2", "Alex Kim", &[(Channel::Email, "alex@home.example")]),
        ],
    )
    .await;
    sync(
        &db,
        "work",
        vec![
            // Same number written differently: the same Sam.
            card(
                "a",
                "Samuel Carter",
                &[
                    (Channel::Phone, "0041791234567"),
                    (Channel::Email, "sam@work.example"),
                ],
            ),
            // Same name, nothing in common: a different Alex, suggested as a duplicate.
            card("b", "Alex Kim", &[(Channel::Telegram, "@alexkim")]),
            // Signal on the same number as Sam's phone joins Sam.
            card("c", "S.", &[(Channel::Signal, "+41791234567")]),
        ],
    )
    .await;
    // Named after the fullest card (the one with the most ways to reach him).
    assert_eq!(
        everyone(&db).await,
        vec![
            ("Alex Kim".to_owned(), vec![Channel::Email]),
            ("Alex Kim".to_owned(), vec![Channel::Telegram]),
            (
                "Samuel Carter".to_owned(),
                vec![Channel::Phone, Channel::Email, Channel::Signal]
            ),
        ]
    );
    let dupes = db.call(|c| store::duplicates(c)).await.unwrap();
    assert_eq!(dupes.len(), 1);
    assert_eq!(dupes[0].0.name, "Alex Kim");
}

#[tokio::test]
async fn the_name_does_not_depend_on_card_order() {
    for flip in [false, true] {
        let db = db_with_sources(&["icloud"]).await;
        let mut cards = vec![
            card("signal", "Sam C.", &[(Channel::Signal, "0041791234567")]),
            card(
                "main",
                "Sam Carter",
                &[
                    (Channel::Phone, "+41 79 123 45 67"),
                    (Channel::Email, "sam@work.example"),
                ],
            ),
        ];
        if flip {
            cards.reverse();
        }
        sync(&db, "icloud", cards).await;
        let people = everyone(&db).await;
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].0, "Sam Carter", "flip={flip}");
    }
}

#[tokio::test]
async fn syncs_refresh_and_remove_cards() {
    let db = db_with_sources(&["icloud"]).await;
    sync(
        &db,
        "icloud",
        vec![card("1", "Sam", &[(Channel::Phone, "+41791234567")])],
    )
    .await;
    // Unchanged cards change nothing.
    assert!(
        !sync(
            &db,
            "icloud",
            vec![card("1", "Sam", &[(Channel::Phone, "+41791234567")])]
        )
        .await
    );
    // A renamed card renames the person; a new number replaces the old one.
    assert!(
        sync(
            &db,
            "icloud",
            vec![card("1", "Sam Carter", &[(Channel::Phone, "+41790000000")])]
        )
        .await
    );
    let people = db.call(|c| store::all(c)).await.unwrap();
    assert_eq!(people[0].summary.name, "Sam Carter");
    assert_eq!(people[0].values, vec!["+41790000000".to_owned()]);
    // A card gone from the source takes the person with it.
    assert!(sync(&db, "icloud", vec![]).await);
    assert!(everyone(&db).await.is_empty());
}

#[tokio::test]
async fn split_merge_and_manual_people() {
    let db = db_with_sources(&["icloud", "work"]).await;
    sync(
        &db,
        "icloud",
        vec![card("1", "Sam Carter", &[(Channel::Phone, "+41791234567")])],
    )
    .await;
    sync(
        &db,
        "work",
        vec![card("a", "Sam C.", &[(Channel::Phone, "+41 79 123 45 67")])],
    )
    .await;
    let sam = db.call(|c| store::all(c)).await.unwrap()[0].summary.id;

    // Wrongly unified? Split the work card off; later syncs keep it apart.
    let other = db
        .call(move |c| store::split(c, sam, "work", "a", 2))
        .await
        .unwrap()
        .expect("split");
    sync(
        &db,
        "work",
        vec![card("a", "Sam C.", &[(Channel::Phone, "+41 79 123 45 67")])],
    )
    .await;
    assert_eq!(everyone(&db).await.len(), 2);

    // Merge them back, and the second person disappears.
    db.call(move |c| store::merge(c, sam, other, 3))
        .await
        .unwrap();
    let names = SourceNames::from([
        ("icloud".into(), "iCloud".into()),
        ("work".into(), "Work".into()),
    ]);
    let person = db
        .call(move |c| store::get(c, sam, &names))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(person.sources.len(), 2);
    // Two cards with the same number still show it once.
    assert_eq!(person.handles.len(), 1);
    assert_eq!(everyone(&db).await.len(), 1);

    // Manual people survive syncs and can carry their own handles.
    let grandma = db
        .call(|c| {
            store::create_manual(
                c,
                "Grandma",
                Some("Nana"),
                &[CardHandle {
                    channel: Channel::Phone,
                    value: "022 555 00 00".into(),
                    label: Some("home".into()),
                }],
                4,
            )
        })
        .await
        .unwrap();
    sync(&db, "icloud", vec![]).await;
    let names = SourceNames::new();
    let g = db
        .call(move |c| store::get(c, grandma, &names))
        .await
        .unwrap()
        .unwrap();
    assert!(g.manual);
    assert_eq!(g.handles[0].source_name, store::MANUAL_SOURCE_NAME);
}

#[tokio::test]
async fn removed_connections_take_their_contacts_along() {
    let db = db_with_sources(&["icloud"]).await;
    sync(
        &db,
        "icloud",
        vec![card("1", "Sam", &[(Channel::Phone, "+41791234567")])],
    )
    .await;
    db.call(|c| c.execute("DELETE FROM connections", []).map(drop))
        .await
        .unwrap();
    assert!(db.call(store::purge_removed_sources).await.unwrap());
    assert!(everyone(&db).await.is_empty());
}

#[test]
fn search_ranks_names_nicknames_and_numbers() {
    use mimi_protocol::PersonSummary;
    let p = |name: &str, nick: Option<&str>, values: &[&str]| store::Indexed {
        summary: PersonSummary {
            id: uuid::Uuid::now_v7(),
            name: name.into(),
            nickname: nick.map(Into::into),
            channels: vec![],
        },
        values: values.iter().map(|v| v.to_string()).collect(),
    };
    let everyone = || {
        vec![
            p("Samantha Jones", None, &[]),
            p("Alex Kim", Some("Sam"), &[]),
            p("Bob Samson", None, &["+41 79 555 12 34"]),
        ]
    };
    let names = |q: &str| {
        super::rank(everyone(), q, 10)
            .into_iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names("sam"), ["Alex Kim", "Samantha Jones", "Bob Samson"]);
    assert_eq!(names("5551234"), ["Bob Samson"]);
    assert_eq!(names("zzz"), Vec::<String>::new());
}
