//! The outbox through the API (`mail::outbox`): the user's Send waits for Undo, Send
//! later, and the assistant's `mail_send` with a time, which still waits for the user's
//! OK and then waits in the outbox like the user's own. Mail is `mail/fake.rs`, the
//! model is scripted.

use std::sync::Arc;

use mimi_protocol::ActionStatus;
use serde_json::{Value, json};

use super::Harness;
use super::mail_flow::{ME, connect, pending, start};
use super::tool_use::{Reply, scripted_llm};
use crate::mail::fake::FakeMail;
use crate::mail::outbox;

fn draft() -> Value {
    json!({"to": ["sam@example.com"], "subject": "Plans", "body": "Friday?"})
}

#[tokio::test]
async fn send_undo_and_send_later_through_the_api() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let h = Harness::new().await;
    connect(&h, &fake, 0).await;
    use reqwest::Method;

    // Send: it waits the seconds Undo is offered; Undo gives the draft back.
    let (status, item) = h
        .call(Method::POST, "/mail/outbox", json!({"draft": draft()}))
        .await;
    assert_eq!(status, 200, "{item}");
    assert_eq!(item["kind"], "undo");
    assert_eq!(item["status"], "waiting");
    let id = item["id"].as_str().unwrap();
    let (status, back) = h
        .call(Method::DELETE, &format!("/mail/outbox/{id}"), Value::Null)
        .await;
    assert_eq!(status, 200, "{back}");
    assert_eq!(back["subject"], "Plans");
    assert_eq!(back["body"], "Friday?");
    let (_, list) = h.call(Method::GET, "/mail/outbox", Value::Null).await;
    assert_eq!(list, json!([]));

    // Send later: listed, moved, then sent now.
    let at = crate::now_ms() + 3_600_000;
    let (status, item) = h
        .call(
            Method::POST,
            "/mail/outbox",
            json!({"draft": draft(), "send_at": at}),
        )
        .await;
    assert_eq!(status, 200, "{item}");
    assert_eq!(item["kind"], "scheduled");
    assert_eq!(item["send_at"], at);
    let id = item["id"].as_str().unwrap();
    let (status, moved) = h
        .call(
            Method::PATCH,
            &format!("/mail/outbox/{id}"),
            json!({"send_at": at + 60_000}),
        )
        .await;
    assert_eq!(status, 200, "{moved}");
    assert_eq!(moved["send_at"], at + 60_000);
    let (_, list) = h.call(Method::GET, "/mail/outbox", Value::Null).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(fake.sent().is_empty());
    let (status, _) = h
        .call(
            Method::POST,
            &format!("/mail/outbox/{id}/send"),
            Value::Null,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(fake.sent().len(), 1);

    // A time in the past, or a wait Undo doesn't offer, is refused.
    let (status, err) = h
        .call(
            Method::POST,
            "/mail/outbox",
            json!({"draft": draft(), "send_at": crate::now_ms() - 3_600_000}),
        )
        .await;
    assert_eq!(status, 400);
    assert!(err["message"].as_str().unwrap().contains("already passed"));
    let (_, mut settings) = h.call(Method::GET, "/settings", Value::Null).await;
    assert_eq!(settings["undo_send_secs"], 10);
    settings["undo_send_secs"] = json!(7);
    let (status, _) = h.call(Method::PUT, "/settings", settings).await;
    assert_eq!(status, 400);
}

/// The assistant may send later, with the user's OK: the card says when, and once
/// approved the email waits in the outbox (where the user can still cancel it) and goes
/// at that time. A time that has passed is refused before any card.
#[tokio::test]
async fn the_assistant_schedules_an_email_only_with_the_users_ok() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let tz = jiff::tz::TimeZone::system();
    let tomorrow = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_hours(26))
        .unwrap()
        .to_zoned(tz)
        .datetime()
        .strftime("%Y-%m-%dT%H:%M")
        .to_string();
    let at = tomorrow.clone();
    let llm = scripted_llm(move |_, n| match n {
        0 => Reply::Call(
            "mail_send",
            json!({"to": ["sam@example.com"], "subject": "Plans", "body": "Friday?",
                   "send_at": "2001-01-01T09:00"}),
        ),
        1 => Reply::Call(
            "mail_send",
            json!({"to": ["sam@example.com"], "subject": "Plans", "body": "Friday?",
                   "send_at": at.clone()}),
        ),
        _ => Reply::Text("It's scheduled."),
    })
    .await;
    let mut h = Harness::new().await;
    h.use_mock(llm.port()).await;
    h.state
        .tool_sources
        .add(Arc::new(crate::mail::tools::MailTools));
    connect(&h, &fake, 0).await;

    let id = start(&h, "Send Sam my plans tomorrow").await;
    let action = pending(&mut h, &id).await;
    assert_eq!(action.arguments["send_at"], json!(tomorrow));
    assert!(action.summary.contains("to go on "), "{}", action.summary);
    let (status, _) = h
        .call(
            reqwest::Method::POST,
            &format!("/actions/{}/approve", action.id),
            json!({}),
        )
        .await;
    assert_eq!(status, 200);
    let (reply, _) = h.wait_for_reply(&id).await;
    let statuses: Vec<_> = reply.actions.iter().map(|a| a.status).collect();
    assert_eq!(statuses, [ActionStatus::Failed, ActionStatus::Done]);
    assert!(!reply.actions[0].requires_approval);
    assert!(
        reply.actions[0]
            .error
            .as_deref()
            .unwrap()
            .contains("already passed"),
        "{:?}",
        reply.actions[0]
    );

    // Waiting, not sent; it goes at its time.
    let (_, list) = h
        .call(reqwest::Method::GET, "/mail/outbox", Value::Null)
        .await;
    let item = &list.as_array().unwrap()[0];
    assert_eq!(item["by_assistant"], true);
    assert_eq!(item["kind"], "scheduled");
    assert!(fake.sent().is_empty());
    let send_at = item["send_at"].as_i64().unwrap();
    for s in outbox::tick(&h.state, send_at - 1).await {
        s.await.unwrap();
    }
    assert!(fake.sent().is_empty());
    for s in outbox::tick(&h.state, send_at).await {
        s.await.unwrap();
    }
    assert_eq!(fake.sent().len(), 1);
}
