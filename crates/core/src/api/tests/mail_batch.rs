//! `POST /v1/mail/threads/batch`: several conversations at once, through the API.

use std::time::Duration;

use serde_json::{Value, json};

use super::Harness;
use crate::mail::fake::{FakeMail, message};

const ME: &str = "me@example.org";

#[tokio::test]
async fn several_conversations_change_in_one_request() {
    let fake = FakeMail::start(ME, "app-pass").await;
    for (i, subject) in ["Lunch", "Tickets"].iter().enumerate() {
        fake.deliver(
            "INBOX",
            &message(
                "Sam <sam@example.com>",
                ME,
                subject,
                "Hi",
                &format!("b{i}@example.com"),
                "",
            ),
            crate::now_ms() - 3_600_000,
            &[],
        );
    }
    let h = Harness::new().await;
    let (status, conn) = h
        .call(
            reqwest::Method::POST,
            "/connections",
            json!({
                "integration": "email",
                "email": ME,
                "password": "app-pass",
                "preset": "other",
                "servers": {
                    "imap_host": "127.0.0.1", "imap_port": fake.imap_port, "imap_security": "plain",
                    "smtp_host": "127.0.0.1", "smtp_port": fake.smtp_port, "smtp_security": "plain"
                }
            }),
        )
        .await;
    assert_eq!(status, 200, "{conn}");
    let mut ids = Vec::new();
    for _ in 0..100 {
        let (_, threads) = h
            .call(reqwest::Method::GET, "/mail/threads", Value::Null)
            .await;
        ids = threads
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["id"].as_i64())
            .collect();
        if ids.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(ids.len(), 2, "the mail never arrived");

    let (status, result) = h
        .call(
            reqwest::Method::POST,
            "/mail/threads/batch",
            json!({ "ids": ids, "action": { "kind": "flag", "flagged": true } }),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["done"].as_array().unwrap().len(), 2, "{result}");
    assert!(result["message"].is_null(), "{result}");
    let (_, flagged) = h
        .call(
            reqwest::Method::GET,
            "/mail/threads?view=flagged",
            Value::Null,
        )
        .await;
    assert_eq!(flagged.as_array().unwrap().len(), 2, "{flagged}");

    let (status, result) = h
        .call(
            reqwest::Method::POST,
            "/mail/threads/batch",
            json!({ "ids": ids, "action": { "kind": "archive" } }),
        )
        .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(fake.count("Archive"), 2);

    // Nothing chosen is refused in plain words.
    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/mail/threads/batch",
            json!({ "ids": [], "action": { "kind": "delete" } }),
        )
        .await;
    assert_eq!(status, 400, "{err}");
}
