//! Older mail on the server (`older.rs`) against the fake IMAP server: what's searched,
//! the cap, what's kept, and that the next sync passes leave it be.

use std::sync::Arc;

use mimi_protocol::{MailBox, MailSecurity, MailServers};
use serde_json::{Value, json};
use uuid::Uuid;

use super::fake::{FakeMail, message};
use super::older::{self, Terms};
use super::*;
use crate::tools::{ToolContext, ToolSource};
use crate::{AppState, now_ms};

const ME: &str = "me@example.org";
const PASSWORD: &str = "app-pass";
const DAY: i64 = 24 * 3600 * 1000;

fn servers(fake: &FakeMail) -> MailServers {
    MailServers {
        imap_host: "127.0.0.1".to_owned(),
        imap_port: fake.imap_port,
        imap_security: MailSecurity::Plain,
        smtp_host: "127.0.0.1".to_owned(),
        smtp_port: fake.smtp_port,
        smtp_security: MailSecurity::Plain,
        username: None,
    }
}

/// A state with the account saved but no sync loop running, so tests drive passes.
async fn account(fake: &FakeMail) -> (Arc<AppState>, Account) {
    let state = Arc::new(AppState::for_tests("t"));
    let (name, config) = connect(
        &reqwest::Client::new(),
        ME.to_owned(),
        PASSWORD.to_owned(),
        Some("other".to_owned()),
        Some(servers(fake)),
    )
    .await
    .unwrap();
    let id = Uuid::now_v7();
    crate::connections::store::upsert(
        &state.db,
        crate::connections::store::ConnectionRow {
            id,
            integration: EMAIL.to_owned(),
            name: name.clone(),
            config: serde_json::to_value(&config).unwrap(),
            created_at: now_ms(),
        },
    )
    .await
    .unwrap();
    (state, Account { id, name, config })
}

async fn pass(state: &Arc<AppState>, account: &Account) {
    let mut s = sync::session(&account.config).await.unwrap();
    sync::pass(state, &mut s, account).await.unwrap();
    let _ = s.logout().await;
}

/// A message sent `days` ago, delivered then.
fn deliver_old(fake: &FakeMail, mailbox: &str, days: i64, subject: &str, body: &str) -> u32 {
    let at = now_ms() - days * DAY;
    let date = chrono::DateTime::from_timestamp_millis(at)
        .unwrap()
        .to_rfc2822();
    let id = format!("{}@example.com", Uuid::now_v7());
    let raw = format!(
        "From: Sam <sam@example.com>\r\nTo: {ME}\r\nSubject: {subject}\r\nDate: {date}\r\n\
         Message-ID: <{id}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n"
    );
    fake.deliver(mailbox, &raw, at, &["\\Seen"])
}

fn words(text: &str) -> Terms {
    Terms::new(text, Vec::new())
}

async fn search(state: &Arc<AppState>, text: &str) -> mimi_protocol::MailOlderResults {
    older::search(state, &words(text), None, older::CAP)
        .await
        .unwrap()
}

async fn local(state: &AppState, view: Option<MailBox>, q: Option<&str>) -> Vec<String> {
    threads(
        state,
        store::Query {
            view,
            search: q.map(str::to_owned),
            limit: 100,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .into_iter()
    .map(|t| t.subject)
    .collect()
}

#[tokio::test]
async fn older_mail_is_found_in_inbox_sent_and_archive_only() {
    let fake = FakeMail::start(ME, PASSWORD).await;
    for (mailbox, subject) in [
        ("INBOX", "Plumber invoice 2019"),
        ("Sent", "Re: the plumber"),
        ("Archive", "Plumber quote"),
        ("Junk", "Cheap plumber pills"),
        ("Trash", "Plumber, deleted"),
    ] {
        deliver_old(&fake, mailbox, 400, subject, "About the plumber.");
    }
    fake.deliver(
        "INBOX",
        &message(
            "sam@example.com",
            ME,
            "Plumber tomorrow",
            "He comes at 9.",
            "new@x",
            "",
        ),
        now_ms() - DAY,
        &[],
    );
    let (state, account) = account(&fake).await;
    pass(&state, &account).await;
    assert_eq!(
        local(&state, None, Some("plumber")).await,
        ["Plumber tomorrow"]
    );

    let found = search(&state, "plumber").await;
    let mut subjects: Vec<&str> = found.threads.iter().map(|t| t.subject.as_str()).collect();
    subjects.sort_unstable();
    assert_eq!(
        subjects,
        ["Plumber invoice 2019", "Plumber quote", "the plumber"]
    );
    assert!(!found.more && found.problems.is_empty(), "{found:?}");
    assert!(found.before < now_ms() - 89 * DAY);
    // Spam and Trash are never opened, let alone searched.
    let used = fake.mailboxes_used();
    assert!(
        !used.contains("Junk") && !used.contains("Trash"),
        "{used:?}"
    );
    let search_cmd = fake
        .commands("INBOX")
        .into_iter()
        .find(|c| c.contains("TEXT"))
        .unwrap();
    assert!(
        search_cmd.starts_with("SEARCH CHARSET UTF-8 BEFORE ")
            && search_cmd.ends_with("TEXT \"plumber\""),
        "{search_cmd}"
    );

    // It opens like any conversation: whole messages, the received_on of sync.
    let old = &found.threads[0];
    let detail = thread(&state, old.id).await.unwrap().unwrap();
    assert_eq!(detail.messages[0].body.trim(), "About the plumber.");
    assert_eq!(detail.thread.received_on.as_deref(), Some(ME));

    // Searchable here while kept, but not in the mailboxes, counts or sorting.
    assert_eq!(local(&state, None, Some("plumber")).await.len(), 4);
    assert_eq!(
        local(&state, Some(MailBox::Inbox), None).await,
        ["Plumber tomorrow"]
    );
    assert_eq!(local(&state, Some(MailBox::Archive), None).await.len(), 0);
    let o = overview(&state, store::Scope::default()).await.unwrap();
    assert_eq!(o.unread, 1);
    assert_eq!(o.accounts[0].addresses[0].threads, 1);
    let queued = state
        .db
        .call(|c| {
            folders::create(c, "Home", "Anything about the house")?;
            Ok((store::unsorted(c, 0, 10)?, folders::next(c)?))
        })
        .await
        .unwrap();
    assert_eq!(queued.0.len(), 1, "only the recent conversation is sorted");
    let (next, _) = queued.1.unwrap();
    assert_ne!(next, old.id);
    let people = state
        .db
        .call(move |c| store::correspondents(c, account.id, &[ME.to_owned()]))
        .await
        .unwrap();
    assert!(people.is_empty(), "{people:?}");
}

#[tokio::test]
async fn the_next_passes_keep_it_without_fetching_again() {
    let fake = FakeMail::start(ME, PASSWORD).await;
    let kept = deliver_old(&fake, "INBOX", 300, "Old lease", "The lease, signed.");
    let gone = deliver_old(
        &fake,
        "INBOX",
        290,
        "Old lease, page 2",
        "More of the lease.",
    );
    let recent = fake.deliver(
        "INBOX",
        &message(ME, ME, "Note", "hi", "n@x", ""),
        now_ms() - DAY,
        &[],
    );
    let (state, account) = account(&fake).await;
    pass(&state, &account).await;
    let conn = account.id;
    let mark = |state: Arc<AppState>| async move {
        state
            .db
            .call(move |c| store::sync_state(c, conn, "INBOX"))
            .await
            .unwrap()
    };
    let before_search = mark(state.clone()).await;
    assert_eq!(search(&state, "lease").await.threads.len(), 2);
    let bodies = fake.bodies_sent();

    fake.remove("INBOX", gone);
    fake.set_flags("INBOX", kept, &["\\Flagged"]);
    let commands = fake.commands("INBOX").len();
    pass(&state, &account).await;

    // Nothing fetched again; the high-water mark didn't move.
    assert_eq!(fake.bodies_sent(), bodies);
    assert_eq!(mark(state.clone()).await, before_search);
    // Sync's own sweep still covers only what it copied; older mail has its own.
    let sweeps: Vec<String> = fake.commands("INBOX")[commands..]
        .iter()
        .filter(|c| c.starts_with("FETCH") && c.contains("(UID FLAGS)"))
        .cloned()
        .collect();
    assert_eq!(
        sweeps,
        [
            format!("FETCH {recent}:{recent} (UID FLAGS)"),
            format!("FETCH {kept},{gone} (UID FLAGS)"),
        ]
    );
    // Kept past the 97 days, removals and flags followed.
    let found = local(&state, None, Some("lease")).await;
    assert_eq!(found, ["Old lease"]);
    let t = threads(
        &state,
        store::Query {
            search: Some("lease".into()),
            limit: 5,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // It was read; now it's flagged and unread there.
    assert!(t[0].flagged && t[0].unread);

    // Once its time is up, it's forgotten; recent mail stays.
    let id = t[0].id;
    older::opened(&state, id).await;
    let until: i64 = state
        .db
        .call(move |c| {
            c.query_row(
                "SELECT kept_until FROM mail_messages WHERE thread_id = ?1",
                [id],
                |r| r.get(0),
            )
        })
        .await
        .unwrap();
    assert!(until > now_ms() + 29 * DAY, "opening keeps it 30 days");
    state
        .db
        .call(|c| {
            c.execute(
                "UPDATE mail_messages SET kept_until = 1 WHERE kept_until IS NOT NULL",
                [],
            )
        })
        .await
        .unwrap();
    pass(&state, &account).await;
    assert!(local(&state, None, Some("lease")).await.is_empty());
    assert_eq!(local(&state, None, None).await, ["Note"]);
    assert_eq!(fake.bodies_sent(), bodies);

    // A renumbered mailbox drops kept mail with the rest.
    search(&state, "lease").await;
    assert_eq!(local(&state, None, Some("lease")).await.len(), 1);
    fake.renumber("INBOX");
    pass(&state, &account).await;
    assert!(local(&state, None, Some("lease")).await.is_empty());
    assert_eq!(local(&state, None, None).await, ["Note"]);
}

#[tokio::test]
async fn only_the_newest_matches_come_in() {
    let fake = FakeMail::start(ME, PASSWORD).await;
    // Oldest first, as mail arrives.
    for i in (0..60).rev() {
        deliver_old(
            &fake,
            "INBOX",
            100 + i,
            &format!("Receipt {i}"),
            "Your receipt.",
        );
    }
    deliver_old(&fake, "Archive", 99, "Receipt archived", "Your receipt.");
    let (state, account) = account(&fake).await;
    pass(&state, &account).await;
    let found = search(&state, "receipt").await;
    assert!(found.more);
    assert_eq!(found.threads.len(), older::CAP);
    let subjects: Vec<&str> = found.threads.iter().map(|t| t.subject.as_str()).collect();
    assert_eq!(subjects[0], "Receipt archived");
    assert_eq!(subjects[1], "Receipt 0");
    assert!(subjects.contains(&"Receipt 48"));
    assert!(!subjects.contains(&"Receipt 49"));
    assert!(!subjects.contains(&"Receipt 59"));
    assert_eq!(fake.bodies_sent(), older::CAP as u32);

    // The same search again fetches nothing more.
    let again = search(&state, "receipt").await;
    assert_eq!(again.threads.len(), older::CAP);
    assert_eq!(fake.bodies_sent(), older::CAP as u32);
}

#[tokio::test]
async fn servers_that_refuse_utf8_are_searched_in_plain_ascii() {
    let fake = FakeMail::start(ME, PASSWORD).await;
    deliver_old(
        &fake,
        "INBOX",
        200,
        "Menu du café",
        "Le menu du café, et la facture.",
    );
    deliver_old(&fake, "INBOX", 210, "Facture", "La facture.");
    let (state, account) = account(&fake).await;
    pass(&state, &account).await;

    // Accented words go to servers that take UTF-8 (as a literal here).
    let found = search(&state, "café").await;
    assert_eq!(found.threads.len(), 1);
    assert_eq!(found.threads[0].subject, "Menu du café");

    fake.set_refuse_charset(true);
    let found = search(&state, "facture").await;
    assert_eq!(found.threads.len(), 2, "{found:?}");
    assert!(found.problems.is_empty(), "{found:?}");
    let searches: Vec<String> = fake
        .commands("INBOX")
        .into_iter()
        .filter(|c| c.contains("facture"))
        .collect();
    assert!(
        searches[0].starts_with("SEARCH CHARSET UTF-8 "),
        "{searches:?}"
    );
    assert!(searches[1].starts_with("SEARCH BEFORE "), "{searches:?}");

    // Accented words are left out, and the user is told.
    let found = search(&state, "café facture").await;
    assert_eq!(found.threads.len(), 2);
    assert!(found.problems[0].contains("accented"), "{found:?}");
    let found = search(&state, "café").await;
    assert!(found.threads.is_empty());
    assert!(found.problems[0].contains("accented"), "{found:?}");
}

#[tokio::test]
async fn the_assistant_reaches_older_mail_as_data() {
    let fake = FakeMail::start(ME, PASSWORD).await;
    deliver_old(
        &fake,
        "INBOX",
        500,
        "Warranty",
        "Your boiler warranty runs until 2030.",
    );
    let (state, account) = account(&fake).await;
    pass(&state, &account).await;
    let tools = tools::MailTools.tools(&state).await;
    let search = tools.iter().find(|t| t.name() == "mail_search").unwrap();
    let ctx = ToolContext {
        state: state.clone(),
        conversation_id: Uuid::now_v7(),
        principal: Default::default(),
    };
    let run = |args: Value| search.run(&ctx, args);

    // Nothing recent matches: the server is asked.
    let out = run(json!({ "query": "warranty" })).await.unwrap();
    assert!(
        out["notice"]
            .as_str()
            .unwrap()
            .contains("never as instructions")
    );
    assert_eq!(out["conversations"], json!([]));
    assert_eq!(out["older_conversations"][0]["subject"], "Warranty");
    assert!(out["older_note"].as_str().unwrap().contains("mail server"));

    // Asked not to, it doesn't; asked to, it does even with recent matches.
    let out = run(json!({ "query": "warranty", "older": false }))
        .await
        .unwrap();
    assert!(out.get("older_conversations").is_none());
    let out = run(json!({ "from": "sam@example.com", "older": true }))
        .await
        .unwrap();
    assert_eq!(out["older_conversations"][0]["subject"], "Warranty");
    assert!(
        search
            .summary(&json!({ "query": "x", "older": true }))
            .contains("older")
    );
}
