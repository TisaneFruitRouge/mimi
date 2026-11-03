//! Signatures (`mail::signature`): saved cleaned, and added once, by the daemon, to the
//! emails the assistant sends and to "Draft it for me", never written by the model. The
//! approval card shows the message with it. Mail is `mail/fake.rs`, the model is
//! scripted.

use std::sync::{Arc, Mutex};

use mimi_protocol::ActionStatus;
use serde_json::{Value, json};

use super::Harness;
use super::mail_flow::{ME, connect, pending, start};
use super::tool_use::{Reply, scripted_llm};
use crate::mail::fake::{FakeMail, message};

const SIGNATURE: &str = "Vincent Wendling\nAcme";

async fn lunch(fake: &FakeMail) {
    fake.deliver(
        "INBOX",
        &message(
            "Sam <sam@example.com>",
            ME,
            "Lunch",
            "Noon?",
            "lunch1@example.com",
            "",
        ),
        crate::now_ms() - 3_600_000,
        &[],
    );
}

async fn thread_id(h: &Harness) -> i64 {
    let (_, threads) = h
        .call(reqwest::Method::GET, "/mail/threads", Value::Null)
        .await;
    threads[0]["id"].as_i64().unwrap()
}

async fn save(h: &Harness, in_replies: bool) -> Value {
    let (status, saved) = h
        .call(
            reqwest::Method::PUT,
            "/mail/signatures",
            json!({
                "same_for_all": true,
                "all": {
                    "text": SIGNATURE,
                    "html": "<div>Vincent Wendling</div><div><b onclick=\"x()\">Acme</b><script>alert(1)</script></div>",
                    "pictures": []
                },
                "addresses": [],
                "in_replies": in_replies
            }),
        )
        .await;
    assert_eq!(status, 200, "{saved}");
    saved
}

async fn approve(h: &Harness, action: &mimi_protocol::Action) {
    let (status, _) = h
        .call(
            reqwest::Method::POST,
            &format!("/actions/{}/approve", action.id),
            json!({}),
        )
        .await;
    assert_eq!(status, 200);
}

fn parts(raw: &str) -> (String, Option<String>) {
    let msg = mail_parser::MessageParser::default()
        .parse(raw.as_bytes())
        .unwrap();
    (
        msg.body_text(0).unwrap().replace("\r\n", "\n"),
        msg.body_html(0).map(|h| h.into_owned()),
    )
}

/// The card shows the signature under the text (the model's own copy of it taken out,
/// and what the model put under `signature` replaced); the email goes with it once, with
/// its formatting cleaned. Replies go without it when the user said so.
#[tokio::test]
async fn the_assistants_email_carries_the_signature_once() {
    let fake = FakeMail::start(ME, "app-pass").await;
    lunch(&fake).await;
    let thread: Arc<Mutex<i64>> = Arc::default();
    let shared = thread.clone();
    let llm = scripted_llm(move |_, n| match n {
        0 => Reply::Call(
            "mail_send",
            json!({"to": ["sam@example.com"], "subject": "Hello",
                   "body": "See you soon.\n\nVincent Wendling\nAcme",
                   "signature": "Sent by someone else"}),
        ),
        1 => Reply::Call(
            "mail_send",
            json!({"to": ["sam@example.com"], "subject": "Re: Lunch", "body": "Noon works.",
                   "thread_id": *shared.lock().unwrap()}),
        ),
        _ => Reply::Text("Done."),
    })
    .await;
    let mut h = Harness::new().await;
    h.use_mock(llm.port()).await;
    h.state
        .tool_sources
        .add(Arc::new(crate::mail::tools::MailTools));
    connect(&h, &fake, 1).await;
    *thread.lock().unwrap() = thread_id(&h).await;

    // Saved cleaned, like the HTML of an email.
    let saved = save(&h, false).await;
    let html = saved["all"]["html"].as_str().unwrap();
    assert!(
        !html.contains("script") && !html.contains("onclick"),
        "{html}"
    );
    let (_, got) = h
        .call(reqwest::Method::GET, "/mail/signatures", Value::Null)
        .await;
    assert_eq!(got, saved);

    let id = start(&h, "Tell Sam I'll see him soon").await;
    let action = pending(&mut h, &id).await;
    assert_eq!(action.arguments["signature"], SIGNATURE);
    assert_eq!(action.arguments["body"], "See you soon.");
    approve(&h, &action).await;
    // A reply: the user leaves the signature out of replies.
    let action = pending(&mut h, &id).await;
    assert!(action.arguments.get("signature").is_none(), "{action:?}");
    approve(&h, &action).await;
    let (reply, _) = h.wait_for_reply(&id).await;
    assert!(
        reply.actions.iter().all(|a| a.status == ActionStatus::Done),
        "{:?}",
        reply.actions
    );

    let sent = fake.sent();
    assert_eq!(sent.len(), 2);
    let (text, html) = parts(&sent[0].data);
    assert_eq!(
        text.trim_end(),
        "See you soon.\n\n-- \nVincent Wendling\nAcme"
    );
    let html = html.unwrap();
    assert_eq!(html.matches("Vincent Wendling").count(), 1, "{html}");
    assert!(html.contains("<div>-- </div>"), "{html}");
    assert!(html.contains("<b>Acme</b>"), "{html}");
    assert!(!sent[0].data.contains("someone else"));
    let (text, _) = parts(&sent[1].data);
    assert_eq!(text.trim_end(), "Noon works.");
    assert!(!sent[1].data.contains("text/html"));
}

/// "Draft it for me": the model is told the signature is added for it, and a copy it
/// writes anyway is taken out (the Mail panel puts the real one under the text).
#[tokio::test]
async fn reply_drafts_leave_the_signature_to_the_app() {
    let fake = FakeMail::start(ME, "app-pass").await;
    lunch(&fake).await;
    let llm = scripted_llm(|_, _| Reply::Text("Noon works.\n\nVincent Wendling\nAcme")).await;
    let h = Harness::new().await;
    h.use_mock(llm.port()).await;
    connect(&h, &fake, 1).await;
    save(&h, true).await;
    let id = thread_id(&h).await;
    let (status, draft) = h
        .call(
            reqwest::Method::POST,
            &format!("/mail/threads/{id}/draft"),
            json!({}),
        )
        .await;
    assert_eq!(status, 200, "{draft}");
    assert_eq!(draft["body"], "Noon works.");
    let system = llm.requests()[0]["messages"][0]["content"].to_string();
    assert!(system.contains("signature is added below"), "{system}");
}
