//! Conversations and the reply loop: persist the user's message, stream the model's
//! answer to clients as it arrives, and save it when done.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use hearth_protocol::{
    Conversation, Event, Message, MessageRole, MessageStatus, ModelRef, SendMessageResult,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::api::error::AppError;
use crate::providers::{self, ChatChunk, ChatMessage, Role, store as provider_store};
use crate::{AppState, now_ms, settings};

pub mod store;

pub const DEFAULT_TITLE: &str = "New conversation";

/// Roughly how much history (in characters) goes to the model with each message.
/// About 8k tokens, which every model we suggest can handle.
const HISTORY_BUDGET_CHARS: usize = 32_000;

/// Replies being written right now, by conversation. One at a time per conversation.
#[derive(Default)]
pub struct Generations(Mutex<HashMap<Uuid, CancellationToken>>);

impl Generations {
    fn start(&self, conversation_id: Uuid) -> Option<CancellationToken> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if map.contains_key(&conversation_id) {
            return None;
        }
        let token = CancellationToken::new();
        map.insert(conversation_id, token.clone());
        Some(token)
    }

    fn finish(&self, conversation_id: Uuid) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&conversation_id);
    }

    /// Returns whether a reply was in progress.
    pub fn cancel(&self, conversation_id: Uuid) -> bool {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&conversation_id)
            .map(CancellationToken::cancel)
            .is_some()
    }
}

pub async fn send(
    state: Arc<AppState>,
    conversation_id: Uuid,
    content: String,
    model: Option<ModelRef>,
) -> Result<SendMessageResult, AppError> {
    let content = content.trim().to_owned();
    if content.is_empty() {
        return Err(AppError::bad_request("The message is empty."));
    }
    let mut conversation = store::get_conversation(&state.db, conversation_id)
        .await?
        .ok_or_else(|| AppError::not_found("Conversation"))?;
    let settings = settings::load(&state.db).await?;
    let model = model.or(settings.default_model).ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "no_model",
            "Choose a model before sending messages.",
        )
    })?;
    let provider = provider_store::get(&state.db, model.provider_id)
        .await?
        .ok_or_else(|| {
            AppError::new(
                axum::http::StatusCode::BAD_REQUEST,
                "no_model",
                "The chosen model's provider was removed. Choose another model.",
            )
        })?;
    let client = providers::connect(&state.http, &provider).map_err(AppError::bad_request)?;

    let cancel = state.generations.start(conversation_id).ok_or_else(|| {
        AppError::new(
            axum::http::StatusCode::CONFLICT,
            "busy",
            "The assistant is still replying in this conversation.",
        )
    })?;

    // Build the model's view of the conversation before adding the new messages.
    let history = store::messages(&state.db, conversation_id).await;
    let history = match history {
        Ok(h) => h,
        Err(e) => {
            state.generations.finish(conversation_id);
            return Err(e.into());
        }
    };
    let prompt = build_prompt(&settings.assistant_name, &history, &content);
    tracing::debug!(
        %conversation_id,
        prompt_messages = prompt.len(),
        model = %model.model,
        "sending to model"
    );

    let now = now_ms();
    let user_message = Message {
        id: Uuid::now_v7(),
        conversation_id,
        role: MessageRole::User,
        content: content.clone(),
        reasoning: String::new(),
        status: MessageStatus::Complete,
        model: None,
        locality: None,
        error: None,
        created_at: now,
    };
    let assistant_message = Message {
        id: Uuid::now_v7(),
        role: MessageRole::Assistant,
        content: String::new(),
        status: MessageStatus::Streaming,
        model: Some(model.clone()),
        locality: Some(provider.provider.locality),
        ..user_message.clone()
    };
    if conversation.title == DEFAULT_TITLE && history.is_empty() {
        conversation.title = title_from(&content);
    }
    conversation.updated_at = now;

    let saved = async {
        store::upsert_message(&state.db, user_message.clone()).await?;
        store::upsert_message(&state.db, assistant_message.clone()).await?;
        store::upsert_conversation(&state.db, conversation.clone()).await
    }
    .await;
    if let Err(e) = saved {
        state.generations.finish(conversation_id);
        return Err(e.into());
    }
    state.events.publish(Event::MessageUpdated {
        message: user_message.clone(),
    });
    state.events.publish(Event::MessageUpdated {
        message: assistant_message.clone(),
    });
    state
        .events
        .publish(Event::ConversationUpdated { conversation });

    tokio::spawn(generate(
        state.clone(),
        client,
        model.model,
        prompt,
        assistant_message.clone(),
        cancel,
    ));

    Ok(SendMessageResult {
        user_message,
        assistant_message,
    })
}

async fn generate(
    state: Arc<AppState>,
    client: providers::OpenAiCompatible,
    model: String,
    prompt: Vec<ChatMessage>,
    mut message: Message,
    cancel: CancellationToken,
) {
    let (conversation_id, message_id) = (message.conversation_id, message.id);
    let outcome: Result<(), String> = async {
        let mut stream = tokio::select! {
            s = client.stream_chat(&model, &prompt) => s.map_err(|e| e.to_string())?,
            _ = cancel.cancelled() => return Ok(()),
        };
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = cancel.cancelled() => return Ok(()),
            };
            let (content, reasoning) = match chunk {
                None => return Ok(()),
                Some(Err(e)) => return Err(e.to_string()),
                Some(Ok(ChatChunk::Content(c))) => (c, String::new()),
                Some(Ok(ChatChunk::Reasoning(r))) => (String::new(), r),
            };
            message.content.push_str(&content);
            message.reasoning.push_str(&reasoning);
            state.events.publish(Event::MessageDelta {
                conversation_id,
                message_id,
                content,
                reasoning,
            });
        }
    }
    .await;

    message.status = match &outcome {
        Err(_) => MessageStatus::Error,
        Ok(()) if cancel.is_cancelled() => MessageStatus::Cancelled,
        Ok(()) => MessageStatus::Complete,
    };
    message.error = outcome.err();
    message.content = message.content.trim().to_owned();
    message.reasoning = message.reasoning.trim().to_owned();

    if let Err(e) = store::upsert_message(&state.db, message.clone()).await {
        tracing::error!("saving the assistant's reply failed: {e}");
    }
    if let Ok(Some(mut conversation)) = store::get_conversation(&state.db, conversation_id).await {
        conversation.updated_at = now_ms();
        if store::upsert_conversation(&state.db, conversation.clone())
            .await
            .is_ok()
        {
            state
                .events
                .publish(Event::ConversationUpdated { conversation });
        }
    }
    state.generations.finish(conversation_id);
    state.events.publish(Event::MessageUpdated { message });
}

fn build_prompt(assistant_name: &str, history: &[Message], new_message: &str) -> Vec<ChatMessage> {
    let now = jiff::Zoned::now();
    let system = format!(
        "You are {assistant_name}, a personal assistant. You run on the user's own \
         computer, and their conversations stay private. Be helpful, direct and warm. \
         Answer in the user's language. Use Markdown when it helps readability.\n\n\
         Current date and time: {}.",
        now.strftime("%A, %B %-d, %Y, %H:%M (%Z)")
    );

    // Newest history first until the budget runs out, then back in order.
    let mut budget = HISTORY_BUDGET_CHARS.saturating_sub(new_message.len());
    let mut kept: Vec<ChatMessage> = Vec::new();
    for m in history.iter().rev() {
        if m.content.is_empty() || m.status == MessageStatus::Streaming {
            continue;
        }
        if m.content.len() > budget {
            break;
        }
        budget -= m.content.len();
        kept.push(ChatMessage {
            role: match m.role {
                MessageRole::User => Role::User,
                MessageRole::Assistant => Role::Assistant,
            },
            content: m.content.clone(),
        });
    }
    kept.reverse();

    let mut prompt = vec![ChatMessage {
        role: Role::System,
        content: system,
    }];
    prompt.extend(kept);
    prompt.push(ChatMessage {
        role: Role::User,
        content: new_message.to_owned(),
    });
    prompt
}

/// A short title from the first message: its first line, cut at a word boundary.
pub fn title_from(content: &str) -> String {
    const MAX: usize = 48;
    let line = content.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut title = String::new();
    for word in words {
        let next_len =
            title.chars().count() + word.chars().count() + usize::from(!title.is_empty());
        if next_len > MAX {
            if title.is_empty() {
                title = word.chars().take(MAX).collect();
            }
            title.push('…');
            return title;
        }
        if !title.is_empty() {
            title.push(' ');
        }
        title.push_str(word);
    }
    if title.is_empty() {
        DEFAULT_TITLE.to_owned()
    } else {
        title
    }
}

pub fn new_conversation(title: Option<String>) -> Conversation {
    let now = now_ms();
    Conversation {
        id: Uuid::now_v7(),
        title: title
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| DEFAULT_TITLE.to_owned()),
        created_at: now,
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            title_from("What's the weather like?"),
            "What's the weather like?"
        );
        assert_eq!(
            title_from(
                "\n  Can you help me plan a trip to Lisbon next month with my family and our dog?"
            ),
            "Can you help me plan a trip to Lisbon next month…"
        );
        assert_eq!(title_from("   "), DEFAULT_TITLE);
    }
}
