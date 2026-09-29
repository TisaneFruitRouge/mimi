//! Messaging apps where the user talks to their assistant: Telegram, Signal, Matrix.
//!
//! Each connected and paired app is a [`Channel`]: a private line to the user. The
//! app-specific code (in `connections/`) receives the user's messages and hands them to
//! [`converse`], which runs the chat turn and relays the answer and any approval it
//! waits for. Mimi's own messages (reminders, routine results, approvals a routine asks
//! for) go to every paired channel through [`owners`].
//!
//! Apps without buttons (Signal, Matrix) answer prompts with a short reply or a
//! reaction; [`replies`] turns those into decisions.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mimi_protocol::{Action, ActionStatus, Delivery, Event, Message, MessageStatus};
use uuid::Uuid;

use crate::AppState;

pub mod replies;

/// A private line to the user in a messaging app.
#[async_trait]
pub trait Channel: Send + Sync {
    /// The integration id, for logs: `telegram`, `signal`, `matrix`.
    fn kind(&self) -> &'static str;

    /// Sends a message, formatted as well as the app allows.
    async fn send(&self, message: &Outgoing) -> Result<(), String>;

    /// Shows that the assistant is writing, where the app can.
    async fn typing(&self) {}

    /// Asks the user to approve an action.
    async fn ask_approval(&self, action: &Action) -> Result<(), String>;

    /// Delivers a due reminder, with a way to mark it done or snooze it. `late` says
    /// when it was due, if it's late.
    async fn remind(&self, delivery: &Delivery, late: Option<&str>) -> Result<(), String>;
}

/// Something to send: the assistant's Markdown, with an optional bold title and links
/// the user has to open themselves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outgoing {
    pub title: Option<String>,
    pub markdown: String,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub label: String,
    pub url: String,
}

impl Outgoing {
    pub fn text(markdown: impl Into<String>) -> Self {
        Self {
            markdown: markdown.into(),
            ..Self::default()
        }
    }

    /// A finished reply, with the pages its actions left for the user to open (like
    /// saving an event in Google Calendar).
    pub fn reply(title: Option<String>, message: &Message) -> Self {
        let markdown = match message.status {
            MessageStatus::Error => format!(
                "Sorry, something went wrong: {}",
                message.error.as_deref().unwrap_or("unknown error")
            ),
            MessageStatus::Cancelled if message.content.trim().is_empty() => {
                "(Stopped.)".to_owned()
            }
            _ if message.content.trim().is_empty() => "(No answer.)".to_owned(),
            _ => message.content.trim().to_owned(),
        };
        Self {
            title,
            markdown,
            links: open_links(&message.actions),
        }
    }
}

/// Pages the user has to open to finish something, from the actions' output.
pub fn open_links(actions: &[Action]) -> Vec<Link> {
    actions
        .iter()
        .filter_map(|a| a.output.as_ref()?["open_url"].as_str())
        .map(|url| Link {
            label: "Open in Google Calendar to save it".to_owned(),
            url: url.to_owned(),
        })
        .collect()
}

/// Every paired messaging app, for messages Mimi sends on its own.
pub async fn owners(state: &AppState) -> Vec<Arc<dyn Channel>> {
    let mut all: Vec<Arc<dyn Channel>> = Vec::new();
    all.extend(crate::connections::telegram::owners(state).await);
    all.extend(crate::connections::signal::owners(state).await);
    all.extend(crate::connections::matrix::owners(state).await);
    all
}

/// Sends a reminder to every paired app. The kinds it reached.
pub async fn remind_everywhere(
    state: &AppState,
    delivery: &Delivery,
    late: Option<&str>,
) -> Vec<&'static str> {
    let mut reached = Vec::new();
    for channel in owners(state).await {
        match channel.remind(delivery, late).await {
            Ok(()) => reached.push(channel.kind()),
            Err(e) => tracing::warn!("sending a reminder to {} failed: {e}", channel.kind()),
        }
    }
    reached
}

/// Sends a message to every channel given.
pub async fn send_all(channels: &[Arc<dyn Channel>], message: &Outgoing) {
    for channel in channels {
        if let Err(e) = channel.send(message).await {
            tracing::warn!("sending to {} failed: {e}", channel.kind());
        }
    }
}

/// Asks every channel given to approve an action.
pub async fn ask_all(channels: &[Arc<dyn Channel>], action: &Action) {
    for channel in channels {
        if let Err(e) = channel.ask_approval(action).await {
            tracing::warn!("asking {} for approval failed: {e}", channel.kind());
        }
    }
}

/// How long a reply from a messaging app may take, approvals included.
const REPLY_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Sends the user's message to the assistant in `conversation` and relays the answer,
/// asking on the same channel for any approval it needs. Spawn it: it returns only once
/// the reply is finished.
pub async fn converse(
    state: &Arc<AppState>,
    channel: &dyn Channel,
    conversation: Uuid,
    text: String,
) {
    let mut events = state.events.subscribe();
    channel.typing().await;
    let sent = match crate::chat::send(state.clone(), conversation, text, None, Vec::new()).await {
        Ok(sent) => sent,
        Err(e) => {
            let _ = channel.send(&Outgoing::text(e.message())).await;
            return;
        }
    };
    let assistant = sent.assistant_message.id;
    let mut typing = tokio::time::interval(Duration::from_secs(4));
    let deadline = tokio::time::sleep(REPLY_TIMEOUT);
    tokio::pin!(deadline);
    let mut announced = std::collections::HashSet::new();
    let message = loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(Event::MessageUpdated { message }) if message.id == assistant => {
                    for action in &message.actions {
                        if action.status == ActionStatus::PendingApproval
                            && announced.insert(action.id)
                            && let Err(e) = channel.ask_approval(action).await
                        {
                            tracing::warn!("asking {} for approval failed: {e}", channel.kind());
                        }
                    }
                    if message.status != MessageStatus::Streaming {
                        break message;
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            },
            _ = typing.tick() => channel.typing().await,
            _ = &mut deadline => return,
        }
    };
    if let Err(e) = channel.send(&Outgoing::reply(None, &message)).await {
        tracing::warn!("sending a reply to {} failed: {e}", channel.kind());
    }
}

/// Opens the channel's conversation, or a new one named after the app if it's gone.
/// `current` is the conversation the connection last used.
pub async fn ensure_conversation(
    state: &AppState,
    current: Option<Uuid>,
    title: &str,
) -> Option<Uuid> {
    if let Some(id) = current
        && crate::chat::store::get_conversation(&state.db, id)
            .await
            .ok()
            .flatten()
            .is_some()
    {
        return Some(id);
    }
    let conversation = crate::chat::new_conversation(Some(title.to_owned()));
    crate::chat::store::upsert_conversation(&state.db, conversation.clone())
        .await
        .ok()?;
    state.events.publish(Event::ConversationUpdated {
        conversation: conversation.clone(),
    });
    Some(conversation.id)
}

/// The assistant's name, for greetings.
pub async fn assistant_name(state: &AppState) -> String {
    crate::settings::load(&state.db)
        .await
        .map(|s| s.assistant_name)
        .unwrap_or_else(|_| "Mimi".to_owned())
}

/// Splits text into pieces of at most `max` characters, preferring line breaks.
pub fn chunks(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        if current.chars().count() + line.chars().count() > max && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        if line.chars().count() > max {
            // One very long line: cut it.
            let chars: Vec<char> = line.chars().collect();
            for piece in chars.chunks(max) {
                out.push(piece.iter().collect());
            }
            continue;
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_split() {
        let text = "a".repeat(3000) + "\n" + &*"b".repeat(3000);
        let parts = chunks(&text, 4000);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.chars().count() <= 4000));
        let one_line = "c".repeat(9000);
        let parts = chunks(&one_line, 4000);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts.concat(), one_line);
    }
}
