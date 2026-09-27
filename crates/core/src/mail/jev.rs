//! # jev: sorting mail with Jev, TypeSafe's cloud decision model, when the user chose it
//! in Settings › Privacy instead of their own model.
//!
//! Jev answers typed questions about a "state" with probabilities; it writes no text, so
//! mail it sorts gets a category but no summary. Only mail that needs a decision is sent:
//! newsletters, automatic mail and suspicious mail are filed locally first (`triage`).
//! The user's key lives in the encrypted database and never leaves the daemon except in
//! the `Authorization` header of requests to TypeSafe.

use std::time::Duration;

use mimi_protocol::{MailCategory, MailThreadDetail};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::AppState;
use crate::db::Db;

/// TypeSafe's evaluation endpoint. Tests point `Mail::jev_api` at a fake.
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MODEL: &str = "jev-latest";
const TIMEOUT: Duration = Duration::from_secs(30);
/// Row in the `settings` table holding the key (kept out of `Settings`, which clients read).
const KEY_ROW: &str = "jev";
/// Most of a conversation sent: the latest messages first, within this many characters.
const STATE_BUDGET: usize = 4000;

fn endpoint(state: &AppState) -> String {
    state
        .mail
        .jev_api
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| ENDPOINT.to_owned())
}

#[derive(Serialize, Deserialize)]
struct Stored {
    api_key: String,
}

/// The saved key, if the user added one.
pub async fn key(db: &Db) -> Option<String> {
    db.call(|c| {
        c.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [KEY_ROW],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .ok()
    .flatten()
    .and_then(|raw| serde_json::from_str::<Stored>(&raw).ok())
    .map(|s| s.api_key)
}

/// Checks the key with TypeSafe (one tiny question), then saves it.
pub async fn save_key(state: &AppState, api_key: &str) -> Result<(), String> {
    let api_key = api_key.trim().to_owned();
    if api_key.is_empty() {
        return Err("Paste your TypeSafe API key.".to_owned());
    }
    let questions =
        json!({ "check": { "type": "noul", "instructions": "Is `state` a greeting?" } });
    ask(state, &api_key, json!("Hello"), questions).await?;
    let raw = serde_json::to_string(&Stored { api_key }).expect("serializes");
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                (KEY_ROW, raw),
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Forgets the key.
pub async fn remove_key(db: &Db) -> Result<(), String> {
    db.call(|c| c.execute("DELETE FROM settings WHERE key = ?1", [KEY_ROW]))
        .await
        .map(drop)
        .map_err(|e| e.to_string())
}

/// One request to TypeSafe. Errors are plain sentences, never with the key or the URL.
async fn ask(
    app: &AppState,
    api_key: &str,
    state: Value,
    questions: Value,
) -> Result<Value, String> {
    let res = app
        .http
        .post(endpoint(app))
        .bearer_auth(api_key)
        .timeout(TIMEOUT)
        .json(&json!({ "model": MODEL, "state": state, "questions": questions }))
        .send()
        .await
        .map_err(|_| "Couldn't reach TypeSafe. Check your internet connection.".to_owned())?;
    match res.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err("TypeSafe didn't accept that API key.".to_owned()),
        402 => return Err("Your TypeSafe account needs credit or a plan.".to_owned()),
        429 => return Err("TypeSafe is busy right now; sorting will try again later.".to_owned()),
        code => return Err(format!("TypeSafe couldn't answer (error {code}).")),
    }
    res.json()
        .await
        .map_err(|_| "TypeSafe's answer wasn't readable.".to_owned())
}

/// The conversation as Jev's state: subject, who wrote, and the latest messages' text.
fn state(detail: &MailThreadDetail) -> Value {
    let mut messages = Vec::new();
    let mut used = 0;
    for m in detail.messages.iter().rev() {
        let text = super::model::clip(&super::parse::strip_quoted(&m.body), 2000);
        if used + text.len() > STATE_BUDGET && !messages.is_empty() {
            break;
        }
        used += text.len();
        let from = if m.from_me {
            "the user".to_owned()
        } else {
            match &m.from.name {
                Some(n) => format!("{n} <{}>", m.from.email),
                None => m.from.email.clone(),
            }
        };
        messages.push(json!({ "from": from, "text": text }));
    }
    messages.reverse();
    json!({ "subject": detail.thread.subject, "messages": messages })
}

/// The same categories and definitions the user's model gets (`triage::INSTRUCTIONS`).
fn questions() -> Value {
    json!({
        "category": {
            "type": "choice",
            "instructions": "How should the user's assistant file this email conversation (`messages`, \
                             oldest first; \"the user\" is the recipient)?",
            "criteria": {
                "needs_reply": "A real person asks the user something or is waiting for the user's answer or decision.",
                "important": "Worth the user's attention soon (bills due, security alerts, travel, appointments, \
                              changes to plans, messages from people they know) but no reply is expected.",
                "other": "Everything else: newsletters, promotions, notifications, receipts, FYI."
            }
        }
    })
}

/// Which of `folders` a conversation belongs in, by Jev (one yes/no each).
pub async fn folders(
    state_: &AppState,
    detail: &MailThreadDetail,
    folders: &[super::folders::FolderSpec],
) -> Result<Vec<i64>, String> {
    let api_key = key(&state_.db)
        .await
        .ok_or("Add your TypeSafe API key in Settings › Privacy, or file with your own model.")?;
    let answer = ask(
        state_,
        &api_key,
        state(detail),
        super::folders::jev_questions(folders),
    )
    .await?;
    Ok(folders
        .iter()
        .filter(|(id, _, _)| {
            answer["answers"][format!("folder_{id}")]["noul"]
                .as_f64()
                .is_some_and(|p| p >= 0.5)
        })
        .map(|(id, _, _)| *id)
        .collect())
}

/// Sorts one conversation with Jev.
pub async fn sort(state_: &AppState, detail: &MailThreadDetail) -> Result<MailCategory, String> {
    let api_key = key(&state_.db)
        .await
        .ok_or("Add your TypeSafe API key in Settings › Privacy, or sort with your own model.")?;
    let answer = ask(state_, &api_key, state(detail), questions()).await?;
    match answer["answers"]["category"]["choice"].as_str() {
        Some("needs_reply") => Ok(MailCategory::NeedsReply),
        Some("important") => Ok(MailCategory::Important),
        Some("other") => Ok(MailCategory::Other),
        _ => Err("TypeSafe's answer wasn't usable.".to_owned()),
    }
}
