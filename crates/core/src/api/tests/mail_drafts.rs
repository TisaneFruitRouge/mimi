//! Saved drafts through the API (`mail::drafts`): saved as the user writes, listed,
//! opened, discarded; Send takes one out of Drafts and Undo puts it back. Mail is
//! `mail/fake.rs`.

use reqwest::Method;
use serde_json::{Value, json};

use super::Harness;
use super::mail_flow::{ME, connect};
use crate::mail::fake::FakeMail;

#[tokio::test]
async fn drafts_are_saved_listed_sent_and_given_back() {
    let fake = FakeMail::start(ME, "app-pass").await;
    let h = Harness::new().await;
    connect(&h, &fake, 0).await;
    let id = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    let draft = json!({
        "to": ["sam@example.com"], "subject": "Plans", "body": "Friday?",
        "html": "<div><b>Friday?</b></div>", "draft_id": id
    });

    let (status, _) = h
        .call(
            Method::PUT,
            &format!("/mail/drafts/{id}"),
            json!({"draft": draft}),
        )
        .await;
    assert_eq!(status, 200);
    let (_, list) = h.call(Method::GET, "/mail/drafts", Value::Null).await;
    assert_eq!(list[0]["id"], id, "{list}");
    assert_eq!(list[0]["subject"], "Plans");
    assert_eq!(list[0]["from_elsewhere"], false);
    let (status, one) = h
        .call(Method::GET, &format!("/mail/drafts/{id}"), Value::Null)
        .await;
    assert_eq!(status, 200);
    assert_eq!(one["html"], "<div><b>Friday?</b></div>");
    assert_eq!(one["draft_id"], id);

    // Send: out of Drafts once queued; Undo makes it a draft again.
    let (status, item) = h
        .call(Method::POST, "/mail/outbox", json!({"draft": draft}))
        .await;
    assert_eq!(status, 200, "{item}");
    let (_, list) = h.call(Method::GET, "/mail/drafts", Value::Null).await;
    assert_eq!(list, json!([]));
    let outgoing = item["id"].as_str().unwrap();
    let (status, back) = h
        .call(
            Method::DELETE,
            &format!("/mail/outbox/{outgoing}"),
            Value::Null,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(back["draft_id"], id);
    let (_, list) = h.call(Method::GET, "/mail/drafts", Value::Null).await;
    assert_eq!(list[0]["id"], id);

    // Closing the editor asks for the copy now; Discard removes it.
    let (status, _) = h
        .call(
            Method::POST,
            &format!("/mail/drafts/{id}/mirror"),
            Value::Null,
        )
        .await;
    assert_eq!(status, 200);
    let (status, _) = h
        .call(Method::DELETE, &format!("/mail/drafts/{id}"), Value::Null)
        .await;
    assert_eq!(status, 200);
    let (_, list) = h.call(Method::GET, "/mail/drafts", Value::Null).await;
    assert_eq!(list, json!([]));
    let (status, _) = h
        .call(Method::GET, &format!("/mail/drafts/{id}"), Value::Null)
        .await;
    assert_eq!(status, 404);
    assert!(fake.sent().is_empty());
}
