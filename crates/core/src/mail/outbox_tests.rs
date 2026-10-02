//! The outbox against the fake mail server: the undo window, sending at a time,
//! catching up after the computer was off, changing or taking back a waiting message,
//! and problems kept on the message instead of retried forever.

use std::sync::Arc;

use base64::Engine;
use mimi_protocol::{
    Event, MailAttachmentSource, MailDraft, NewMailAttachment, OutgoingKind, OutgoingStatus,
    Settings,
};

use super::*;
use crate::mail::fake::FakeMail;
use crate::mail::tests::account_without_loop;
use crate::mail::{Account, EMAIL};

const ME: &str = "me@example.org";
const HOUR: i64 = 60 * MINUTE;

fn draft(subject: &str) -> MailDraft {
    MailDraft {
        to: vec!["sam@example.com".to_owned()],
        subject: subject.to_owned(),
        body: "See you then.".to_owned(),
        ..Default::default()
    }
}

async fn send_all(state: &Arc<AppState>, now: i64) -> usize {
    let sends = tick(state, now).await;
    let n = sends.len();
    for s in sends {
        s.await.unwrap();
    }
    n
}

async fn stored(state: &AppState) -> Vec<OutgoingMail> {
    list(state).await.unwrap()
}

async fn set_undo(state: &AppState, secs: u32) {
    crate::settings::save(
        &state.db,
        &Settings {
            undo_send_secs: secs,
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

/// Saves the account again with other settings (a changed password, a server gone).
async fn change_account(
    state: &AppState,
    account: &Account,
    f: impl FnOnce(&mut super::super::EmailConfig),
) {
    let mut config = account.config.clone();
    f(&mut config);
    crate::connections::store::upsert(
        &state.db,
        crate::connections::store::ConnectionRow {
            id: account.id,
            integration: EMAIL.to_owned(),
            name: account.name.clone(),
            config: serde_json::to_value(&config).unwrap(),
            created_at: now_ms(),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn send_waits_out_the_undo_window_and_undo_takes_it_back() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, account) = account_without_loop(&fake).await;
    let mut events = state.events.subscribe();

    // Undo send is on by default, for 10 seconds.
    let now = now_ms();
    let item = queue(&state, draft("Dinner"), None, false).await.unwrap();
    assert_eq!(item.kind, OutgoingKind::Undo);
    assert_eq!(item.status, OutgoingStatus::Waiting);
    assert_eq!(item.connection_id, account.id);
    assert_eq!(item.from, ME);
    assert!(
        (item.send_at - now - 10_000).abs() < 2_000,
        "{}",
        item.send_at - now
    );
    assert!(matches!(
        events.recv().await.unwrap(),
        Event::MailOutbox { item: ref i } if i.id == item.id && i.status == OutgoingStatus::Waiting
    ));

    // Nothing goes during the window; Undo gives the draft back, exactly.
    assert_eq!(send_all(&state, now + 5_000).await, 0);
    let back = cancel(&state, item.id).await.unwrap();
    assert_eq!(back, draft("Dinner"));
    assert!(stored(&state).await.is_empty());
    assert_eq!(send_all(&state, now + HOUR).await, 0);
    assert!(fake.sent().is_empty());

    // Left alone, it goes once the window is over, once, and is filed in Sent.
    let item = queue(&state, draft("Dinner"), None, false).await.unwrap();
    assert_eq!(send_all(&state, item.send_at - 1).await, 0);
    assert_eq!(send_all(&state, item.send_at).await, 1);
    assert_eq!(send_all(&state, item.send_at + HOUR).await, 0);
    let sent = fake.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, ["sam@example.com"]);
    assert_eq!(fake.count("Sent"), 1);
    assert!(stored(&state).await.is_empty());
    // Too late to take back.
    let err = cancel(&state, item.id).await.unwrap_err();
    assert!(err.contains("may already have gone"), "{err}");
}

#[tokio::test]
async fn with_undo_send_off_it_goes_at_once() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, _) = account_without_loop(&fake).await;
    set_undo(&state, 0).await;
    let item = queue(&state, draft("Now"), None, false).await.unwrap();
    assert_eq!(item.status, OutgoingStatus::Sent);
    assert_eq!(fake.sent().len(), 1);
    assert!(stored(&state).await.is_empty());
}

#[tokio::test]
async fn a_scheduled_email_goes_at_its_time_or_as_soon_as_mimi_runs_again() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, _) = account_without_loop(&fake).await;
    let now = now_ms();
    let item = queue(&state, draft("Tomorrow"), Some(now + 20 * HOUR), false)
        .await
        .unwrap();
    assert_eq!(item.kind, OutgoingKind::Scheduled);
    assert_eq!(item.send_at, now + 20 * HOUR);
    assert_eq!(stored(&state).await, std::slice::from_ref(&item));
    assert_eq!(send_all(&state, now + HOUR).await, 0);

    // The computer was off for days: it goes once when Mimi runs again, marked late.
    let mut events = state.events.subscribe();
    let back = now + 3 * 24 * HOUR;
    assert_eq!(send_all(&state, back).await, 1);
    assert_eq!(send_all(&state, back + MINUTE).await, 0);
    assert_eq!(fake.sent().len(), 1);
    loop {
        if let Event::MailOutbox { item: i } = events.recv().await.unwrap()
            && i.status == OutgoingStatus::Sent
        {
            assert_eq!(i.send_at, now + 20 * HOUR);
            assert!(i.sent_at.is_some());
            break;
        }
    }
}

#[tokio::test]
async fn the_loop_sends_what_came_due_while_it_was_stopped() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, _) = account_without_loop(&fake).await;
    let item = queue(&state, draft("Due"), Some(now_ms() + HOUR), false)
        .await
        .unwrap();
    // As if the computer slept through the time.
    let id = item.id.to_string();
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE mail_outbox SET send_at = send_at - 2 * 3600000 WHERE id = ?1",
                [id],
            )
        })
        .await
        .unwrap();
    let running = tokio::spawn(run(state.clone()));
    for _ in 0..100 {
        if !fake.sent().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    running.abort();
    assert_eq!(fake.sent().len(), 1);
}

#[tokio::test]
async fn a_send_cut_off_by_a_crash_is_never_repeated_on_its_own() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, _) = account_without_loop(&fake).await;
    let item = queue(&state, draft("Maybe"), Some(now_ms() + MINUTE), false)
        .await
        .unwrap();
    let id = item.id.to_string();
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE mail_outbox SET status = 'sending' WHERE id = ?1",
                [id],
            )
        })
        .await
        .unwrap();
    recover(&state).await;
    assert_eq!(send_all(&state, now_ms() + HOUR).await, 0);
    let kept = &stored(&state).await[0];
    assert_eq!(kept.status, OutgoingStatus::Failed);
    assert!(kept.error.as_deref().unwrap().contains("Sent folder"));
    assert!(fake.sent().is_empty());
    // The user decides: here, send it.
    send_now(&state, item.id).await.unwrap();
    assert_eq!(fake.sent().len(), 1);
}

#[tokio::test]
async fn a_waiting_email_can_be_rescheduled_sent_now_or_cancelled() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, _) = account_without_loop(&fake).await;
    let now = now_ms();

    let item = queue(&state, draft("Later"), Some(now + HOUR), false)
        .await
        .unwrap();
    let moved = reschedule(&state, item.id, now + 3 * HOUR).await.unwrap();
    assert_eq!(moved.send_at, now + 3 * HOUR);
    assert_eq!(send_all(&state, now + 2 * HOUR).await, 0);
    let err = reschedule(&state, item.id, now - HOUR).await.unwrap_err();
    assert!(err.contains("already passed"), "{err}");
    send_now(&state, item.id).await.unwrap();
    assert_eq!(fake.sent().len(), 1);
    assert!(stored(&state).await.is_empty());
    assert!(send_now(&state, item.id).await.is_err());
    assert!(reschedule(&state, item.id, now + HOUR).await.is_err());

    // An undo-window send moved to a time becomes a scheduled one.
    let item = queue(&state, draft("Undo"), None, false).await.unwrap();
    let moved = reschedule(&state, item.id, now + HOUR).await.unwrap();
    assert_eq!(moved.kind, OutgoingKind::Scheduled);

    // Cancelled: the draft comes back to edit or drop, and nothing goes.
    let back = cancel(&state, item.id).await.unwrap();
    assert_eq!(back.subject, "Undo");
    assert_eq!(send_all(&state, now + 2 * HOUR).await, 0);
    assert_eq!(fake.sent().len(), 1);
}

#[tokio::test]
async fn mistakes_show_when_scheduling_not_hours_later() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, _) = account_without_loop(&fake).await;
    let later = Some(now_ms() + HOUR);
    let refused = |d: MailDraft| {
        let state = state.clone();
        async move { queue(&state, d, later, false).await.unwrap_err() }
    };
    let err = refused(MailDraft {
        to: vec!["not an address".to_owned()],
        ..draft("x")
    })
    .await;
    assert!(err.contains("isn't an email address"), "{err}");
    let err = refused(MailDraft {
        to: vec![],
        ..draft("x")
    })
    .await;
    assert!(err.contains("at least one recipient"), "{err}");
    let err = refused(MailDraft {
        from: Some("ceo@other.example".to_owned()),
        ..draft("x")
    })
    .await;
    assert!(
        err.contains("isn't one of this account's addresses"),
        "{err}"
    );
    let err = refused(MailDraft {
        attachments: vec![NewMailAttachment {
            name: "big.bin".to_owned(),
            mime: None,
            data: base64::engine::general_purpose::STANDARD.encode(vec![
                0u8;
                super::super::smtp::MAX_ATTACHMENTS
                    + 1
            ]),
            content_id: None,
            source: None,
        }],
        ..draft("x")
    })
    .await;
    assert!(err.contains("20 MB"), "{err}");
    // A file that isn't there any more.
    let err = refused(MailDraft {
        attachments: vec![NewMailAttachment {
            name: "gone.pdf".to_owned(),
            mime: None,
            data: String::new(),
            content_id: None,
            source: Some(MailAttachmentSource::Email {
                message: 999_999,
                index: 0,
                size: None,
            }),
        }],
        ..draft("x")
    })
    .await;
    assert!(err.contains("isn't here any more"), "{err}");
    let err = queue(&state, draft("x"), Some(now_ms() - HOUR), false)
        .await
        .unwrap_err();
    assert!(err.contains("already passed"), "{err}");
    let err = queue(&state, draft("x"), Some(now_ms() + 400 * 24 * HOUR), false)
        .await
        .unwrap_err();
    assert!(err.contains("next year"), "{err}");
    assert!(stored(&state).await.is_empty());
}

#[tokio::test]
async fn a_refused_password_is_kept_on_the_email_not_retried() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, account) = account_without_loop(&fake).await;
    let now = now_ms();
    let item = queue(&state, draft("Report"), Some(now + HOUR), false)
        .await
        .unwrap();
    change_account(&state, &account, |c| c.password = "revoked".to_owned()).await;

    assert_eq!(send_all(&state, now + HOUR).await, 1);
    let kept = &stored(&state).await[0];
    assert_eq!(kept.status, OutgoingStatus::Failed);
    assert!(
        kept.error.as_deref().unwrap().contains("password"),
        "{:?}",
        kept.error
    );
    // Not tried again on its own.
    assert_eq!(send_all(&state, now + 30 * HOUR).await, 0);
    assert!(fake.sent().is_empty());

    // Fixed, the user sends it.
    change_account(&state, &account, |c| c.password = "app-pass".to_owned()).await;
    send_now(&state, item.id).await.unwrap();
    assert_eq!(fake.sent().len(), 1);
}

#[tokio::test]
async fn an_unreachable_server_is_tried_a_few_times_then_the_user_is_told() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, account) = account_without_loop(&fake).await;
    let now = now_ms();
    queue(&state, draft("Offline"), Some(now + MINUTE), false)
        .await
        .unwrap();
    // A port nothing listens on.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    change_account(&state, &account, |c| c.servers.smtp_port = closed).await;

    let mut at = now + MINUTE;
    for attempt in 0..RETRY_MINUTES.len() {
        assert_eq!(send_all(&state, at).await, 1);
        let kept = &stored(&state).await[0];
        assert_eq!(kept.status, OutgoingStatus::Waiting, "attempt {attempt}");
        assert!(kept.error.is_some());
        assert!(kept.send_at > at);
        at = kept.send_at;
    }
    assert_eq!(send_all(&state, at).await, 1);
    let kept = &stored(&state).await[0];
    assert_eq!(kept.status, OutgoingStatus::Failed);
    assert!(
        kept.error.as_deref().unwrap().contains("Couldn't reach"),
        "{:?}",
        kept.error
    );
    assert_eq!(send_all(&state, at + 24 * HOUR).await, 0);
}

#[tokio::test]
async fn a_disconnected_account_never_sends_from_another() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let (state, account) = account_without_loop(&fake).await;
    let now = now_ms();
    queue(&state, draft("Gone"), Some(now + MINUTE), false)
        .await
        .unwrap();
    crate::connections::store::delete(&state.db, account.id)
        .await
        .unwrap();
    assert_eq!(send_all(&state, now + MINUTE).await, 1);
    let kept = &stored(&state).await[0];
    assert_eq!(kept.status, OutgoingStatus::Failed);
    assert_eq!(kept.error.as_deref(), Some(ACCOUNT_GONE));
    assert!(fake.sent().is_empty());
}

/// A forward's originals are fetched when it's scheduled, so the email can go even if
/// the original is gone from the server by then. The list leaves files' content out.
#[tokio::test]
async fn files_are_kept_with_a_scheduled_email() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let raw = format!(
        "From: Bank <bank@example.com>\r\nTo: {ME}\r\nSubject: Statement\r\n\
         Message-ID: <st1@example.com>\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
         --b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nAttached.\r\n\
         --b\r\nContent-Type: application/pdf; name=\"statement.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"statement.pdf\"\r\n\r\n\
         Balance: 1200\r\n--b--\r\n"
    );
    let uid = fake.deliver("INBOX", &raw, now_ms() - HOUR, &[]);
    let (state, account) = account_without_loop(&fake).await;
    crate::mail::tests::pass(&state, &account).await;
    let thread = crate::mail::tests::all_threads(&state).await[0].id;
    let detail = crate::mail::thread(&state, thread).await.unwrap().unwrap();
    let original = detail.messages[0].id;

    let mine = NewMailAttachment {
        name: "note.txt".to_owned(),
        mime: Some("text/plain".to_owned()),
        data: base64::engine::general_purpose::STANDARD.encode("hello"),
        content_id: None,
        source: None,
    };
    let forward = MailDraft {
        subject: "Fwd: Statement".to_owned(),
        forward_of: Some(original),
        attachments: vec![mine.clone()],
        ..draft("Fwd: Statement")
    };
    let now = now_ms();
    let item = queue(&state, forward.clone(), Some(now + HOUR), false)
        .await
        .unwrap();
    assert_eq!(item.draft.attachments[0].data, "");
    fake.remove("INBOX", uid);

    assert_eq!(send_all(&state, now + HOUR).await, 1);
    let sent = fake.sent();
    assert_eq!(sent.len(), 1, "{:?}", stored(&state).await);
    let files = crate::mail::parse::attachments(sent[0].data.as_bytes());
    let names: Vec<_> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["statement.pdf", "note.txt"]);
    assert_eq!(files[0].data, b"Balance: 1200");

    let again = queue(&state, forward.clone(), Some(now + HOUR), false)
        .await
        .unwrap_err();
    // The original is gone now: that shows straight away.
    assert!(again.contains("isn't"), "{again}");
}

#[test]
fn the_assistants_times_are_local_unless_they_say_otherwise() {
    let tz = TimeZone::get("Europe/Paris").unwrap();
    let t = parse_when("2026-10-03T09:00", &tz).unwrap();
    assert_eq!(t.to_string(), "2026-10-03T07:00:00Z");
    let t = parse_when("2026-10-03T09:00:00Z", &tz).unwrap();
    assert_eq!(t.to_string(), "2026-10-03T09:00:00Z");
    // A time skipped by the clocks going forward moves forward.
    let t = parse_when("2026-03-29T02:30", &tz).unwrap();
    assert_eq!(t.to_string(), "2026-03-29T01:30:00Z");
    assert!(parse_when("next tuesday", &tz).is_err());
}
