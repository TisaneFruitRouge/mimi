use std::sync::Arc;

use mimi_protocol::{Autonomy, KindPermission, PermissionRule, PermissionTarget, Permissions};
use serde_json::json;

use super::*;
use crate::connections::matrix::fake::{FakeMessenger, paired_connection};
use crate::tools::permissions::{EVERYONE_ON_SERVER, always_offer, requires_approval};

const ME: &str = "@mimi:home.org";
const OWNER: &str = "@vincent:home.org";
const OWNER_ROOM: &str = "!owner:home.org";
const SAM: &str = "@sam:home.org";
const LEA: &str = "@lea:home.org";
const FAMILY: &str = "!family:home.org";
const MIXED: &str = "!mixed:home.org";
const LOCALS: &str = "!locals:home.org";
const BOOKS: &str = "!books:home.org";

async fn setup() -> (Arc<AppState>, Arc<FakeMessenger>, Uuid) {
    let state = Arc::new(AppState::for_tests("t"));
    let fake = FakeMessenger::new(ME);
    fake.add_user(SAM, Some("Sammy"));
    fake.add_user(LEA, None);
    fake.add_user("@alex1:home.org", None);
    fake.add_user("@alex2:home.org", None);
    fake.add_group(FAMILY, "Family", Some("#family:home.org"), &[OWNER, SAM]);
    fake.add_group(MIXED, "Mixed", None, &[OWNER, "@eve:elsewhere.net"]);
    fake.add_group(LOCALS, "Locals", None, &[OWNER, LEA]);
    fake.add_public(BOOKS, "Book club", Some("#books:home.org"));
    let id = paired_connection(&state, fake.clone(), OWNER, OWNER_ROOM).await;
    (state, fake, id)
}

/// Someone the user added to People by hand, with a Matrix address.
async fn person(state: &AppState, name: &str, matrix: Option<&str>) -> Uuid {
    let id = Uuid::now_v7();
    let (name, matrix) = (name.to_owned(), matrix.map(str::to_owned));
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO people (id, name, created_at, updated_at) VALUES (?1, ?2, 0, 0)",
                (id.to_string(), &name),
            )?;
            if let Some(m) = matrix {
                let key = crate::people::normalize::match_key(mimi_protocol::Channel::Matrix, &m);
                c.execute(
                    "INSERT INTO person_handles (id, person_id, channel, value, match_key, created_at)
                     VALUES (?1, ?2, 'matrix', ?3, ?4, 0)",
                    (Uuid::now_v7().to_string(), id.to_string(), &m, key),
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
    id
}

async fn resolve(state: &AppState, inputs: &[&str]) -> Result<Vec<Recipient>, String> {
    let accounts = paired(state).await;
    let inputs: Vec<String> = inputs.iter().map(|s| (*s).to_owned()).collect();
    resolve_all(state, &accounts[0], &inputs).await
}

fn user(id: &str, name: Option<&str>) -> Recipient {
    Recipient::Person {
        id: id.to_owned(),
        name: name.map(str::to_owned),
    }
}

#[tokio::test]
async fn recipients_become_exactly_who_and_what_is_reached() {
    let (state, _, _) = setup().await;
    // An address: its display name, until People has a name for it.
    assert_eq!(
        resolve(&state, &[SAM]).await.unwrap(),
        [user(SAM, Some("Sammy"))]
    );
    let sam = person(&state, "Sam Carter", Some(SAM)).await;
    assert_eq!(
        resolve(&state, &[SAM]).await.unwrap(),
        [user(SAM, Some("Sam Carter"))]
    );
    // From People: by name (first name too) or by id (an @ mention), once each.
    assert_eq!(
        resolve(&state, &["sam", "Sam Carter", &sam.to_string(), SAM])
            .await
            .unwrap(),
        [user(SAM, Some("Sam Carter"))]
    );
    // matrix.to links, no display name.
    assert_eq!(
        resolve(&state, &["https://matrix.to/#/@lea:home.org"])
            .await
            .unwrap(),
        [user(LEA, None)]
    );
    // The owner, by address or by their chat.
    assert_eq!(
        resolve(&state, &[OWNER, OWNER_ROOM]).await.unwrap(),
        [user(OWNER, Some("Vincent"))]
    );
    // A group it's in: by address, id or name.
    let family = Recipient::Group {
        id: FAMILY.into(),
        name: Some("Family".into()),
        alias: Some("#family:home.org".into()),
        join: false,
    };
    for input in ["#family:home.org", FAMILY, "family", "Family"] {
        assert_eq!(
            resolve(&state, &[input]).await.unwrap(),
            std::slice::from_ref(&family)
        );
    }
    assert_eq!(family.label(), "Group “Family” (#family:home.org)");
    // A public group on its server: joined as part of sending.
    for input in ["#books:home.org", BOOKS] {
        let [Recipient::Group { join, name, .. }] = &resolve(&state, &[input]).await.unwrap()[..]
        else {
            panic!("one group");
        };
        assert!(join);
        assert_eq!(name.as_deref(), Some("Book club"));
    }
    // Written as one line, separated by commas.
    let to = entries(&json!(["@sam:home.org, Family", "@lea:home.org"]));
    assert_eq!(to, [SAM, "Family", LEA]);
}

#[tokio::test]
async fn only_the_assistants_own_server_and_never_a_guess() {
    let (state, _, _) = setup().await;
    let err = |inputs: &'static [&'static str]| {
        let state = state.clone();
        async move { resolve(&state, inputs).await.unwrap_err() }
    };
    let e = err(&["@someone:matrix.org"]).await;
    assert!(
        e.contains("another Matrix server (matrix.org)") && e.contains("home.org"),
        "{e}"
    );
    assert!(
        err(&["#room:matrix.org"])
            .await
            .contains("another Matrix server")
    );
    assert!(
        err(&["!room:matrix.org"])
            .await
            .contains("another Matrix server")
    );
    // Addresses on its server that lead nowhere.
    assert!(err(&["@nobody:home.org"]).await.contains("no one"));
    assert!(err(&["#nothing:home.org"]).await.contains("No group"));
    // A group it isn't in and can't join.
    assert!(err(&["!private:home.org"]).await.contains("Invite it"));
    assert!(err(&[ME]).await.contains("own address"));
    assert!(
        err(&["not an address"])
            .await
            .contains("Nobody and no group")
    );
    assert!(err(&[]).await.contains("Who is it for"));
    // People without one Matrix address to use.
    person(&state, "Tom", None).await;
    assert!(err(&["Tom"]).await.contains("no Matrix address"));
    person(&state, "Alex One", Some("@alex1:home.org")).await;
    person(&state, "Alex Two", Some("@alex2:home.org")).await;
    assert!(err(&["Alex"]).await.contains("Several people"));
    person(&state, "Rémi", Some("@remi:matrix.org")).await;
    assert!(err(&["Rémi"]).await.contains("another Matrix server"));
    // A group and a person with the same name: ask.
    person(&state, "Family", Some(SAM)).await;
    assert!(err(&["Family"]).await.contains("group or a person"));
}

#[tokio::test]
async fn known_means_the_owner_contacts_messaged_or_invited_or_the_server_when_chosen() {
    let (state, _, id) = setup().await;
    let known = |targets: Vec<Target>, everyone: bool| {
        let state = state.clone();
        async move { rooms::all_known(&state, ME, &targets, everyone).await }
    };
    let u = |s: &str| Target::User(s.to_owned());
    let r = |s: &str| Target::Room(s.to_owned());

    assert!(known(vec![u(OWNER)], false).await);
    assert!(known(vec![r(OWNER_ROOM)], false).await);
    assert!(!known(vec![], false).await, "nobody isn't known");
    assert!(!known(vec![u(SAM)], false).await);
    person(&state, "Sam", Some(SAM)).await;
    assert!(known(vec![u(SAM)], false).await, "in People");
    assert!(!known(vec![u(SAM), u(LEA)], false).await, "one unknown");
    rooms::remember(&state, id, LEA, rooms::Known::Messaged)
        .await
        .unwrap();
    assert!(known(vec![u(SAM), u(LEA)], false).await, "messaged before");

    // Groups: the owner's invitation or an earlier message makes them known.
    assert!(!known(vec![r(FAMILY)], false).await);
    rooms::remember(&state, id, FAMILY, rooms::Known::Invited)
        .await
        .unwrap();
    assert!(known(vec![r(FAMILY)], false).await);

    // Everyone on the server, when the user says so: people there, and groups whose
    // members all are. Never anyone elsewhere, nor a group it isn't in.
    let bob = "@bob:home.org";
    assert!(!known(vec![u(bob)], false).await);
    assert!(known(vec![u(bob)], true).await);
    assert!(!known(vec![u("@bob:matrix.org")], true).await);
    assert!(!known(vec![r(LOCALS)], false).await);
    assert!(known(vec![r(LOCALS)], true).await);
    assert!(!known(vec![r(MIXED)], true).await, "someone from elsewhere");
    assert!(!known(vec![r(BOOKS)], true).await, "not in it yet");

    // Only from a paired account.
    assert!(!rooms::all_known(&state, "@other:home.org", &[u(OWNER)], true).await);
}

fn perms(autonomy: Autonomy, rules: Vec<PermissionRule>, switches: &[&str]) -> Permissions {
    Permissions(
        [(
            "send_messages".to_owned(),
            KindPermission {
                autonomy,
                rules,
                switches: switches.iter().map(|s| (*s).to_owned()).collect(),
            },
        )]
        .into(),
    )
}

#[tokio::test]
async fn permissions_decide_for_every_recipient_and_strangers_always_ask() {
    let (state, _, id) = setup().await;
    let tool = SendMessage {
        accounts: vec![ME.into()],
    };
    let ctx = ToolContext {
        state: state.clone(),
        conversation_id: Uuid::now_v7(),
    };
    let prepared = |to: serde_json::Value| {
        let (tool, ctx) = (&tool, &ctx);
        async move {
            let resolved = tool
                .resolve(ctx, json!({"to": to, "text": "Hi!"}))
                .await
                .unwrap();
            tool.prepare(resolved).unwrap()
        }
    };
    let asks = |to: serde_json::Value, p: Permissions| {
        let (state, tool) = (&state, &tool);
        let prepared = &prepared;
        async move {
            let args = prepared(to).await;
            requires_approval(state, tool, &args, &p).await
        }
    };
    let sam = person(&state, "Sam", Some(SAM)).await;

    // The card shows what runs: exact addresses, names, and the account it's from.
    let args = prepared(json!(["Sam", "#family:home.org"])).await;
    assert_eq!(args["to"], json!([SAM, FAMILY]));
    assert_eq!(args["from"], ME);
    assert_eq!(
        args["recipients"],
        json!(["Sam (@sam:home.org)", "Group “Family” (#family:home.org)"])
    );
    assert_eq!(
        tool.summary(&args),
        "Send a Matrix message to Sam (@sam:home.org) and Group “Family” (#family:home.org)"
    );
    assert!(args.get("joins").is_none());
    assert_eq!(
        prepared(json!([BOOKS])).await["joins"],
        json!(["Book club"])
    );
    assert_eq!(
        tool.call_targets(&args),
        [
            CallTarget::MatrixUser {
                from: ME.into(),
                user: SAM.into()
            },
            CallTarget::MatrixRoom {
                from: ME.into(),
                room: FAMILY.into()
            }
        ]
    );

    // Asking by default, even for the owner.
    let default = Permissions::default();
    assert!(asks(json!([OWNER]), default.clone()).await);
    // Automatic: only for people and groups the user knows, every one of them.
    let auto = perms(Autonomy::Automatic, vec![], &[]);
    assert!(!asks(json!([OWNER]), auto.clone()).await);
    assert!(!asks(json!(["Sam"]), auto.clone()).await);
    assert!(asks(json!(["Sam", LEA]), auto.clone()).await, "Léa is new");
    assert!(asks(json!([FAMILY]), auto.clone()).await);
    // The server switch makes everyone there known.
    let server = perms(Autonomy::Automatic, vec![], &[EVERYONE_ON_SERVER]);
    assert!(!asks(json!(["Sam", LEA]), server.clone()).await);
    assert!(
        asks(json!([MIXED]), server.clone()).await,
        "Eve is elsewhere"
    );

    // Exceptions: a person (through their Matrix address) or a group. A group's
    // exception still needs the group to be known.
    let sam_ok = perms(
        Autonomy::Ask,
        vec![PermissionRule {
            target: PermissionTarget::Person(sam),
            autonomy: Autonomy::Automatic,
        }],
        &[],
    );
    assert!(!asks(json!([SAM]), sam_ok.clone()).await);
    assert!(
        asks(json!([SAM, OWNER]), sam_ok.clone()).await,
        "the owner asks"
    );
    let family_ok = perms(
        Autonomy::Ask,
        vec![PermissionRule {
            target: PermissionTarget::MatrixRoom(FAMILY.into()),
            autonomy: Autonomy::Automatic,
        }],
        &[],
    );
    assert!(
        asks(json!(["Family"]), family_ok.clone()).await,
        "not known yet"
    );
    rooms::remember(&state, id, FAMILY, rooms::Known::Invited)
        .await
        .unwrap();
    assert!(!asks(json!(["Family"]), family_ok.clone()).await);
    let family_asks = perms(
        Autonomy::Automatic,
        vec![PermissionRule {
            target: PermissionTarget::MatrixRoom(FAMILY.into()),
            autonomy: Autonomy::Ask,
        }],
        &[],
    );
    assert!(asks(json!(["Family"]), family_asks).await);

    // "Don't ask again for …", for known people in People and known groups only.
    let offer = |to: serde_json::Value| {
        let (state, tool) = (&state, &tool);
        let prepared = &prepared;
        async move {
            let args = prepared(to).await;
            always_offer(state, tool, &args, &Permissions::default()).await
        }
    };
    let (text, targets) = offer(json!(["Sam", "Family"])).await.unwrap();
    assert_eq!(text, "Don't ask again for Sam and Family");
    assert_eq!(
        targets,
        [
            PermissionTarget::Person(sam),
            PermissionTarget::MatrixRoom(FAMILY.into())
        ]
    );
    assert_eq!(offer(json!([LEA])).await, None, "not in People");
    person(&state, "Léa", Some(LEA)).await;
    assert!(offer(json!([LEA])).await.is_some());
    assert_eq!(offer(json!([MIXED])).await, None, "not known");
}

#[tokio::test]
async fn sending_opens_one_chat_per_person_and_joins_public_groups() {
    let (state, fake, id) = setup().await;
    let tool = SendMessage {
        accounts: vec![ME.into()],
    };
    let ctx = ToolContext {
        state: state.clone(),
        conversation_id: Uuid::now_v7(),
    };
    let send = |to: serde_json::Value| {
        let (tool, ctx) = (&tool, &ctx);
        async move {
            let args = tool
                .resolve(ctx, json!({"to": to, "text": "**Dinner** at 8?"}))
                .await?;
            let args = tool.prepare(args)?;
            let out = tool.run(ctx, args.clone()).await?;
            Ok::<_, String>((tool.result_label(&args, &out), out))
        }
    };

    let (label, out) = send(json!([SAM, "Family", OWNER])).await.unwrap();
    assert_eq!(label, "sent a Matrix message to Sammy, Family and Vincent");
    assert_eq!(out["new_chats_with"], json!(["Sammy"]));
    let sent = fake.sent();
    assert_eq!(
        sent,
        [
            ("!dm1:home.org".to_owned(), "**Dinner** at 8?".to_owned()),
            (FAMILY.to_owned(), "**Dinner** at 8?".to_owned()),
            (OWNER_ROOM.to_owned(), "**Dinner** at 8?".to_owned()),
        ]
    );
    // The chat is kept, and everyone it reached is now known.
    let kept = rooms::kept(&state, id).await;
    assert_eq!(kept[0].room_id, "!dm1:home.org");
    assert_eq!(kept[0].why, Why::Direct);
    assert!(
        rooms::all_known(
            &state,
            ME,
            &[Target::User(SAM.into()), Target::Room(FAMILY.into())],
            false
        )
        .await
    );

    // The next message to Sam uses the same chat; once they left it, a new one.
    send(json!([SAM])).await.unwrap();
    assert_eq!(fake.sent()[3].0, "!dm1:home.org");
    fake.world()
        .members
        .insert("!dm1:home.org".into(), vec![ME.into()]);
    send(json!([SAM])).await.unwrap();
    assert_eq!(fake.sent()[4].0, "!dm2:home.org");
    assert!(
        !fake.world().joined.iter().any(|r| r.id == "!dm1:home.org"),
        "the abandoned chat is left"
    );

    // A public group is joined to post, and kept.
    let (_, out) = send(json!(["#books:home.org"])).await.unwrap();
    assert_eq!(out["joined_groups"], json!(["Book club"]));
    assert_eq!(fake.sent()[5].0, BOOKS);
    assert!(
        rooms::kept_room(&state, id, BOOKS)
            .await
            .is_some_and(|k| k.why == Why::Joined)
    );

    // What runs is checked again: nothing goes elsewhere, whatever the arguments say.
    let bad = json!({"to": ["@x:matrix.org"], "text": "hi", "from": ME, "recipients": ["Sam"]});
    assert!(tool.run(&ctx, bad).await.is_err());
    assert_eq!(fake.sent().len(), 6);
}
