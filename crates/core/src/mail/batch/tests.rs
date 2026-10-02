//! Batches against the fake IMAP server: one session per account, and an account that
//! can't be reached failing only its own conversations.

use std::sync::Arc;

use mimi_protocol::{MailBatchAction, MailBox, MailSecurity, MailServers};
use uuid::Uuid;

use super::run;
use crate::AppState;
use crate::mail::fake::{FakeMail, message};
use crate::mail::tests::{account_without_loop, all_threads, pass};
use crate::mail::{Account, EMAIL, connect, folders, store, threads};
use crate::now_ms;

const ME: &str = "me@example.org";
const PASSWORD: &str = "app-pass";
const DAY: i64 = 24 * 3600 * 1000;

/// Puts `n` separate conversations in a mailbox.
fn deliver(fake: &FakeMail, mailbox: &str, to: &str, subjects: &[&str]) -> Vec<u32> {
    subjects
        .iter()
        .enumerate()
        .map(|(i, subject)| {
            fake.deliver(
                mailbox,
                &message(
                    "Sam <sam@example.com>",
                    to,
                    subject,
                    "Hello",
                    &format!("{subject}-{i}@example.com"),
                    "",
                ),
                now_ms() - DAY + i as i64,
                &[],
            )
        })
        .collect()
}

/// The conversations' ids by subject.
async fn ids(state: &AppState, subjects: &[&str]) -> Vec<i64> {
    let all = all_threads(state).await;
    subjects
        .iter()
        .map(|s| all.iter().find(|t| t.subject == *s).unwrap().id)
        .collect()
}

async fn view(state: &AppState, view: MailBox) -> Vec<String> {
    threads(
        state,
        store::Query {
            view: Some(view),
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .into_iter()
    .map(|t| t.subject)
    .collect()
}

/// A second account in the same state.
async fn add_account(state: &Arc<AppState>, fake: &FakeMail, email: &str) -> Account {
    let (name, config) = connect(
        &reqwest::Client::new(),
        email.to_owned(),
        PASSWORD.to_owned(),
        Some("other".to_owned()),
        Some(MailServers {
            imap_host: "127.0.0.1".to_owned(),
            imap_port: fake.imap_port,
            imap_security: MailSecurity::Plain,
            smtp_host: "127.0.0.1".to_owned(),
            smtp_port: fake.smtp_port,
            smtp_security: MailSecurity::Plain,
            username: None,
        }),
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
    Account { id, name, config }
}

#[tokio::test]
async fn every_action_works_on_several_conversations_in_one_session() {
    let fake = FakeMail::start(ME, PASSWORD).await;
    let uids = deliver(&fake, "INBOX", ME, &["One", "Two", "Three", "Four"]);
    let (state, account) = account_without_loop(&fake).await;
    pass(&state, &account).await;
    let all = ids(&state, &["One", "Two", "Three", "Four"]).await;

    // Read, then unread: every message on the server, one session for all of them.
    let logins = fake.logins();
    let r = run(&state, all.clone(), MailBatchAction::Read { read: true })
        .await
        .unwrap();
    assert_eq!(r.done.len(), 4);
    assert!(r.failed.is_empty() && r.message.is_none(), "{r:?}");
    assert_eq!(fake.logins(), logins + 1);
    assert!(all_threads(&state).await.iter().all(|t| !t.unread));
    for uid in &uids {
        assert!(fake.flags("INBOX", *uid).contains("\\Seen"));
    }
    run(
        &state,
        all[..2].to_vec(),
        MailBatchAction::Read { read: false },
    )
    .await
    .unwrap();
    assert!(!fake.flags("INBOX", uids[0]).contains("\\Seen"));
    assert!(fake.flags("INBOX", uids[2]).contains("\\Seen"));

    // Flag: one session, and conversations already flagged need nothing.
    let logins = fake.logins();
    run(
        &state,
        all[..1].to_vec(),
        MailBatchAction::Flag { flagged: true },
    )
    .await
    .unwrap();
    let r = run(&state, all.clone(), MailBatchAction::Flag { flagged: true })
        .await
        .unwrap();
    assert_eq!(r.done.len(), 4);
    assert_eq!(fake.logins(), logins + 2);
    for uid in &uids {
        assert!(fake.flags("INBOX", *uid).contains("\\Flagged"));
    }
    assert_eq!(view(&state, MailBox::Flagged).await.len(), 4);
    run(
        &state,
        all[2..].to_vec(),
        MailBatchAction::Flag { flagged: false },
    )
    .await
    .unwrap();
    assert_eq!(view(&state, MailBox::Flagged).await.len(), 2);
    assert!(!fake.flags("INBOX", uids[3]).contains("\\Flagged"));

    // A smart folder: labels here only, nothing on the server.
    let folder = state
        .db
        .call(|c| folders::create_with(c, "Trips", "Travel plans", "folder", "gray"))
        .await
        .unwrap();
    let logins = fake.logins();
    run(
        &state,
        all[..3].to_vec(),
        MailBatchAction::Folder {
            folder,
            member: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(fake.logins(), logins);
    let in_folder = threads(
        &state,
        store::Query {
            folder: Some(folder),
            limit: 50,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(in_folder.len(), 3);

    // Archive: one session moves them all.
    let logins = fake.logins();
    let r = run(&state, all[..3].to_vec(), MailBatchAction::Archive)
        .await
        .unwrap();
    assert_eq!(r.done.len(), 3);
    assert_eq!(fake.logins(), logins + 1);
    assert_eq!(fake.count("INBOX"), 1);
    assert_eq!(fake.count("Archive"), 3);
    assert_eq!(view(&state, MailBox::Inbox).await, vec!["Four"]);

    // Delete, across the Inbox and the Archive: still one session.
    // (Archived mail comes back from the Archive as new conversations.)
    pass(&state, &account).await;
    let pair = ids(&state, &["One", "Four"]).await;
    let logins = fake.logins();
    let r = run(&state, pair.clone(), MailBatchAction::Delete)
        .await
        .unwrap();
    assert_eq!(r.done, pair);
    assert_eq!(fake.logins(), logins + 1);
    assert_eq!(fake.count("INBOX"), 0);
    assert_eq!(fake.count("Archive"), 2);
    assert_eq!(fake.count("Trash"), 2);
    pass(&state, &account).await;
    let mut left = view(&state, MailBox::Archive).await;
    left.sort();
    assert_eq!(left, vec!["Three", "Two"]);

    // Gone conversations need nothing; the request itself is checked.
    let r = run(&state, vec![pair[0]], MailBatchAction::Archive)
        .await
        .unwrap();
    assert_eq!(r.done, vec![pair[0]]);
    assert!(run(&state, vec![], MailBatchAction::Archive).await.is_err());
    assert!(
        run(
            &state,
            vec![1; 1],
            MailBatchAction::Folder {
                folder: 999,
                member: true
            }
        )
        .await
        .unwrap_err()
        .contains("folder")
    );
    let many: Vec<i64> = (1..=(super::MAX_BATCH as i64 + 1)).collect();
    assert!(run(&state, many, MailBatchAction::Delete).await.is_err());
}

#[tokio::test]
async fn an_account_that_fails_keeps_its_conversations_and_says_why() {
    let work = FakeMail::start(ME, PASSWORD).await;
    let home = FakeMail::start("you@example.net", PASSWORD).await;
    deliver(&work, "INBOX", ME, &["Report", "Budget"]);
    deliver(&home, "INBOX", "you@example.net", &["Garden"]);
    let (state, account) = account_without_loop(&work).await;
    let second = add_account(&state, &home, "you@example.net").await;
    pass(&state, &account).await;
    pass(&state, &second).await;
    let all = ids(&state, &["Report", "Budget", "Garden"]).await;

    // The second account's password was revoked.
    home.state.lock().unwrap().password = "revoked".to_owned();

    // Flags: the first account's go through; the second's are put back here.
    let r = run(&state, all.clone(), MailBatchAction::Flag { flagged: true })
        .await
        .unwrap();
    assert_eq!(r.done, all[..2].to_vec());
    assert_eq!(r.failed.len(), 1);
    assert_eq!(r.failed[0].ids, vec![all[2]]);
    assert!(r.failed[0].reason.starts_with("you@example.net: "), "{r:?}");
    let message = r.message.unwrap();
    assert!(
        message.starts_with("Couldn't flag 1 of the 3 conversations."),
        "{message}"
    );
    assert!(message.contains("app password"), "{message}");
    let flagged = view(&state, MailBox::Flagged).await;
    assert!(!flagged.contains(&"Garden".to_owned()), "{flagged:?}");
    assert_eq!(flagged.len(), 2);

    // Read state is put back the same way.
    let r = run(&state, all.clone(), MailBatchAction::Read { read: true })
        .await
        .unwrap();
    assert_eq!(r.done.len(), 2);
    let garden = all_threads(&state)
        .await
        .into_iter()
        .find(|t| t.subject == "Garden")
        .unwrap();
    assert!(garden.unread);

    // Archive: the first account's move; the second's stay where they are.
    let r = run(&state, all.clone(), MailBatchAction::Archive)
        .await
        .unwrap();
    assert_eq!(r.done, all[..2].to_vec());
    assert_eq!(r.failed[0].ids, vec![all[2]]);
    assert!(
        r.message
            .unwrap()
            .starts_with("Couldn't archive 1 of the 3 conversations."),
    );
    assert_eq!(work.count("Archive"), 2);
    assert_eq!(home.count("INBOX"), 1);
    assert_eq!(view(&state, MailBox::Inbox).await, vec!["Garden"]);

    // Only one conversation, and it fails: said simply.
    let r = run(&state, vec![all[2]], MailBatchAction::Delete)
        .await
        .unwrap();
    assert!(r.done.is_empty());
    assert!(r.message.unwrap().starts_with("Couldn't delete it."));
    assert_eq!(home.count("Trash"), 0);
}
