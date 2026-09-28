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

// Deleting people from Mimi (never from their sources), and bringing them back.

async fn id_of(db: &Db, name: &'static str) -> uuid::Uuid {
    db.call(move |c| store::all(c))
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.summary.name == name)
        .unwrap_or_else(|| panic!("{name} not found"))
        .summary
        .id
}

async fn delete(db: &Db, id: uuid::Uuid) -> Option<bool> {
    db.call(move |c| store::delete(c, id, 10)).await.unwrap()
}

async fn restore(db: &Db, id: uuid::Uuid) -> bool {
    db.call(move |c| store::restore(c, id, 20)).await.unwrap()
}

async fn removed(db: &Db) -> Vec<store::Removed> {
    db.call(|c| store::removed(c)).await.unwrap()
}

async fn person(db: &Db, id: uuid::Uuid) -> Option<mimi_protocol::Person> {
    let names = SourceNames::new();
    db.call(move |c| store::get(c, id, &names)).await.unwrap()
}

/// Rows in the people tables that mention this person, anywhere.
async fn traces(db: &Db, id: uuid::Uuid) -> i64 {
    db.call(move |c| {
        c.query_row(
            "SELECT (SELECT COUNT(*) FROM people WHERE id = ?1)
                  + (SELECT COUNT(*) FROM person_records WHERE person_id = ?1)
                  + (SELECT COUNT(*) FROM person_handles WHERE person_id = ?1)
                  + (SELECT COUNT(*) FROM people_apart WHERE a = ?1 OR b = ?1)
                  + (SELECT COUNT(*) FROM people_removed WHERE id = ?1)
                  + (SELECT COUNT(*) FROM person_records_removed WHERE person_id = ?1)",
            [id.to_string()],
            |r| r.get(0),
        )
    })
    .await
    .unwrap()
}

fn sam_cards() -> (Vec<ContactCard>, Vec<ContactCard>) {
    (
        vec![card(
            "1",
            "Sam Carter",
            &[
                (Channel::Phone, "+41791234567"),
                (Channel::Email, "sam@home.example"),
            ],
        )],
        vec![card("a", "Sam C.", &[(Channel::Phone, "+41 79 123 45 67")])],
    )
}

#[tokio::test]
async fn a_deleted_person_stays_deleted_and_comes_back_whole() {
    let db = db_with_sources(&["icloud", "work"]).await;
    let (icloud, work) = sam_cards();
    sync(&db, "icloud", icloud.clone()).await;
    sync(&db, "work", work.clone()).await;
    let sam = id_of(&db, "Sam Carter").await;
    // A way to reach Sam the user added by hand.
    db.call(move |c| {
        store::add_handle(
            c,
            sam,
            &CardHandle {
                channel: Channel::Telegram,
                value: "@samc".into(),
                label: None,
            },
            2,
        )
    })
    .await
    .unwrap();

    // Both cards, from both sources, go at once; the hand-added handle with them.
    assert_eq!(delete(&db, sam).await, Some(true));
    assert!(everyone(&db).await.is_empty());
    let r = removed(&db).await;
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].id, r[0].name.as_str()), (sam, "Sam Carter"));
    assert_eq!(r[0].sources, ["icloud", "work"]);
    let keys = db
        .call(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM person_handles WHERE match_key IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0),
            )
        })
        .await
        .unwrap();
    assert_eq!(keys, 0, "nothing of Sam counts as a contact any more");

    // Syncs skip the cards, even as they change or gain new numbers.
    assert!(!sync(&db, "icloud", icloud.clone()).await);
    let mut changed = card(
        "a",
        "Samuel Carter",
        &[
            (Channel::Phone, "+41 79 123 45 67"),
            (Channel::Email, "samuel@work.example"),
        ],
    );
    changed.nickname = Some("Sammy".into());
    assert!(!sync(&db, "work", vec![changed.clone()]).await);
    assert!(everyone(&db).await.is_empty());

    // Brought back: the same person, with every card from every source.
    assert!(restore(&db, sam).await);
    assert!(removed(&db).await.is_empty());
    assert!(sync(&db, "icloud", icloud).await);
    assert!(sync(&db, "work", vec![changed]).await);
    let back = person(&db, sam).await.expect("Sam is back");
    assert_eq!(back.sources.len(), 2);
    assert_eq!(back.name, "Samuel Carter");
    // What the user added by hand is gone for good.
    assert!(back.handles.iter().all(|h| h.source_id.is_some()));
    assert_eq!(everyone(&db).await.len(), 1);
    // Nothing of the removal is left behind.
    assert!(!restore(&db, sam).await);
}

#[tokio::test]
async fn deleting_never_swallows_or_resurrects_anyone_else() {
    let db = db_with_sources(&["icloud", "work"]).await;
    let (icloud, _) = sam_cards();
    sync(&db, "icloud", icloud.clone()).await;
    sync(
        &db,
        "work",
        vec![card(
            "b",
            "Alex Kim",
            &[(Channel::Email, "alex@work.example")],
        )],
    )
    .await;
    let sam = id_of(&db, "Sam Carter").await;
    let alex = id_of(&db, "Alex Kim").await;
    delete(&db, sam).await;

    // The deleted card gains Alex's address: it doesn't join Alex, and Sam stays gone.
    let mut sams = icloud.clone();
    sams[0].handles.push(CardHandle {
        channel: Channel::Email,
        value: "alex@work.example".into(),
        label: None,
    });
    assert!(!sync(&db, "icloud", sams).await);
    let a = person(&db, alex).await.unwrap();
    assert_eq!(a.sources.len(), 1);
    assert_eq!(a.handles.len(), 1);

    // A new card elsewhere with Sam's old number is someone new, not the deleted Sam.
    sync(
        &db,
        "work",
        vec![
            card("b", "Alex Kim", &[(Channel::Email, "alex@work.example")]),
            card("c", "Samantha", &[(Channel::Phone, "+41791234567")]),
        ],
    )
    .await;
    let samantha = id_of(&db, "Samantha").await;
    assert_ne!(samantha, sam);
    assert!(person(&db, sam).await.is_none());

    // Restored, Sam gets exactly their own card back: not Samantha's, though it shares
    // the number, and Samantha stays who she is.
    restore(&db, sam).await;
    sync(&db, "icloud", icloud).await;
    let back = person(&db, sam).await.unwrap();
    assert_eq!(back.sources.len(), 1);
    assert_eq!(back.sources[0].source_id, "icloud");
    assert_eq!(person(&db, samantha).await.unwrap().sources.len(), 1);
}

#[tokio::test]
async fn people_added_by_hand_are_simply_deleted() {
    let db = db_with_sources(&["icloud"]).await;
    let grandma = db
        .call(|c| {
            store::create_manual(
                c,
                "Grandma",
                None,
                &[CardHandle {
                    channel: Channel::Phone,
                    value: "+41225550000".into(),
                    label: None,
                }],
                1,
            )
        })
        .await
        .unwrap();
    // Nothing to bring back.
    assert_eq!(delete(&db, grandma).await, Some(false));
    assert!(removed(&db).await.is_empty());
    assert_eq!(traces(&db, grandma).await, 0);
    assert_eq!(delete(&db, grandma).await, None);

    // Someone added by hand who later got a card: the card is remembered, and the name
    // the user gave comes back with them.
    let gran = db
        .call(|c| {
            store::create_manual(
                c,
                "Gran",
                None,
                &[CardHandle {
                    channel: Channel::Phone,
                    value: "+41225550001".into(),
                    label: None,
                }],
                1,
            )
        })
        .await
        .unwrap();
    let margaret = || {
        vec![card(
            "g",
            "Margaret Smith",
            &[(Channel::Phone, "+41225550001")],
        )]
    };
    sync(&db, "icloud", margaret()).await;
    assert_eq!(person(&db, gran).await.unwrap().sources.len(), 1);
    assert_eq!(delete(&db, gran).await, Some(true));
    restore(&db, gran).await;
    sync(&db, "icloud", margaret()).await;
    let back = person(&db, gran).await.unwrap();
    assert_eq!(back.name, "Gran");
    assert_eq!(back.handles.len(), 1);
}

#[tokio::test]
async fn removals_are_forgotten_with_their_cards_and_sources() {
    let db = db_with_sources(&["icloud", "work"]).await;
    let (icloud, work) = sam_cards();
    sync(&db, "icloud", icloud.clone()).await;
    sync(&db, "work", work.clone()).await;
    let sam = id_of(&db, "Sam Carter").await;
    delete(&db, sam).await;

    // The work card is deleted from its address book: only iCloud's is left to restore.
    sync(&db, "work", vec![]).await;
    assert_eq!(removed(&db).await[0].sources, ["icloud"]);

    // The iCloud connection is removed: nothing is left of Sam at all.
    db.call(|c| {
        c.execute("DELETE FROM connections WHERE id = 'icloud'", [])
            .map(drop)
    })
    .await
    .unwrap();
    assert!(db.call(store::purge_removed_sources).await.unwrap());
    assert!(removed(&db).await.is_empty());
    assert_eq!(traces(&db, sam).await, 0);
    assert!(!db.call(store::purge_removed_sources).await.unwrap());
}

#[tokio::test]
async fn a_restore_waits_for_its_cards_and_gives_up_when_they_are_gone() {
    let db = db_with_sources(&["icloud", "work"]).await;
    let (icloud, work) = sam_cards();
    sync(&db, "icloud", icloud.clone()).await;
    sync(&db, "work", work.clone()).await;
    let sam = id_of(&db, "Sam Carter").await;
    delete(&db, sam).await;
    restore(&db, sam).await;

    // Back in the list right away, before any source has been read again; one source
    // syncing doesn't drop Sam while the other's card is still on its way.
    assert!(person(&db, sam).await.is_some());
    sync(&db, "icloud", icloud.clone()).await;
    assert_eq!(person(&db, sam).await.unwrap().sources.len(), 1);

    // Deleted again before the work card came back: both are removed again.
    assert_eq!(delete(&db, sam).await, Some(true));
    assert_eq!(removed(&db).await[0].sources, ["icloud", "work"]);
    assert!(!sync(&db, "work", work).await);

    // Restored, but meanwhile the cards were deleted at their sources: Sam doesn't
    // linger as an empty person.
    restore(&db, sam).await;
    assert!(sync(&db, "icloud", vec![]).await);
    assert!(
        person(&db, sam).await.is_some(),
        "the work card may still come"
    );
    db.call(|c| {
        c.execute("DELETE FROM connections WHERE id = 'work'", [])
            .map(drop)
    })
    .await
    .unwrap();
    assert!(db.call(store::purge_removed_sources).await.unwrap());
    assert_eq!(traces(&db, sam).await, 0);
}

#[tokio::test]
async fn told_apart_pairs_survive_a_restore_but_not_a_final_deletion() {
    let db = db_with_sources(&["icloud", "work"]).await;
    let home_card = || {
        vec![card(
            "1",
            "Alex Kim",
            &[(Channel::Email, "alex@home.example")],
        )]
    };
    sync(&db, "icloud", home_card()).await;
    sync(
        &db,
        "work",
        vec![card("b", "Alex Kim", &[(Channel::Telegram, "@alexkim")])],
    )
    .await;
    let dupes = db.call(|c| store::duplicates(c)).await.unwrap();
    assert_eq!(dupes.len(), 1);
    let (a, b) = (dupes[0].0.id, dupes[0].1.id);
    let (home, other) = if person(&db, a).await.unwrap().sources[0].source_id == "icloud" {
        (a, b)
    } else {
        (b, a)
    };

    // A deleted person is never offered as a duplicate.
    delete(&db, home).await;
    assert!(db.call(|c| store::duplicates(c)).await.unwrap().is_empty());
    restore(&db, home).await;
    sync(&db, "icloud", home_card()).await;
    assert_eq!(db.call(|c| store::duplicates(c)).await.unwrap().len(), 1);

    // "Not the same person" is remembered across a delete and restore…
    db.call(move |c| store::set_apart(c, home, other))
        .await
        .unwrap();
    delete(&db, home).await;
    restore(&db, home).await;
    sync(&db, "icloud", home_card()).await;
    assert!(db.call(|c| store::duplicates(c)).await.unwrap().is_empty());

    // …and forgotten once nothing of them can come back.
    delete(&db, home).await;
    sync(&db, "icloud", vec![]).await;
    assert_eq!(traces(&db, home).await, 0);
}
