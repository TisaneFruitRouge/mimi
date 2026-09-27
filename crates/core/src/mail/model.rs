//! Asking the active model about mail: sorting, summaries, reply drafts. These calls
//! never offer tools, so nothing in an email can make the model act; its answer is only
//! ever shown to the user or parsed into a fixed shape.

use std::time::Duration;

use mimi_protocol::{Locality, MailMessage};

use crate::AppState;
use crate::providers::{self, ChatMessage, ChatOptions, Role};

/// Longest a single background answer may take.
const TIMEOUT: Duration = Duration::from_secs(180);

/// Where the active model runs, if one is set up.
pub async fn locality(state: &AppState) -> Option<Locality> {
    let settings = crate::settings::load(&state.db).await.ok()?;
    let model = settings.default_model?;
    let record = providers::store::get(&state.db, model.provider_id)
        .await
        .ok()??;
    Some(record.provider.locality)
}

/// Asks the active model one question, with no tools, and returns its answer.
pub async fn ask(state: &AppState, system: &str, user: String) -> Result<String, String> {
    let settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    let model = settings
        .default_model
        .ok_or("Set up a model first, in Models.")?;
    let record = providers::store::get(&state.db, model.provider_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("The model's source is gone. Pick another model in Models.")?;
    let client = providers::chat_client(state, &record, &model.model).await?;
    let prompt = [
        ChatMessage::text(Role::System, system),
        ChatMessage::text(Role::User, user),
    ];
    // No thinking: these are short, structured answers, and reasoning models would
    // spend most of their time on it.
    tokio::time::timeout(
        TIMEOUT,
        client.complete(&model.model, &prompt, ChatOptions::QUICK),
    )
    .await
    .map_err(|_| "The model took too long.".to_owned())?
    .map_err(|e| e.to_string())
}

/// A conversation laid out for the model, fenced as data. Quoted history is trimmed
/// from each message (the earlier messages are there already), and the whole thing is
/// capped so it fits small local contexts.
pub fn transcript(subject: &str, messages: &[MailMessage], budget: usize) -> String {
    let mut parts: Vec<String> = Vec::new();
    for m in messages.iter().rev() {
        let who = match &m.from.name {
            Some(n) => format!("{n} <{}>", m.from.email),
            None => m.from.email.clone(),
        };
        let who = if m.from_me {
            format!("{who} (the user)")
        } else {
            who
        };
        let date = chrono::DateTime::from_timestamp_millis(m.date)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%a %-d %b %Y %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        let body = super::parse::strip_quoted(&m.body);
        let body = clip(&body, 2500);
        let part = format!("From: {who}\nDate: {date}\n\n{body}");
        let used: usize = parts.iter().map(String::len).sum();
        if used + part.len() > budget && !parts.is_empty() {
            break;
        }
        parts.push(clip(&part, budget));
    }
    parts.reverse();
    format!(
        "<email_thread subject=\"{}\">\n{}\n</email_thread>",
        subject.replace('"', "'"),
        parts.join("\n\n---\n\n").replace("</email_thread>", "")
    )
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

/// One tidy line: no line breaks, no markdown emphasis, capped.
pub fn one_line(s: &str, max: usize) -> String {
    let flat = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '"' || c == '*' || c == '`')
        .to_owned();
    clip(&flat, max)
}
