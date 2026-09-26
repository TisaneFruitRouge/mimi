use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use hearth_protocol::{
    Conversation, ConversationDetail, ConversationUpdate, Event, NewConversation, SendMessage,
    SendMessageResult,
};
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::chat::{self, store};
use crate::{AppState, now_ms};

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Conversation>> {
    Ok(Json(store::list_conversations(&state.db).await?))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    body: Option<Json<NewConversation>>,
) -> ApiResult<Conversation> {
    let title = body.and_then(|Json(b)| b.title);
    let conversation = chat::new_conversation(title);
    store::upsert_conversation(&state.db, conversation.clone()).await?;
    state.events.publish(Event::ConversationUpdated {
        conversation: conversation.clone(),
    });
    Ok(Json(conversation))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<ConversationDetail> {
    let conversation = store::get_conversation(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Conversation"))?;
    let messages = store::messages(&state.db, id).await?;
    Ok(Json(ConversationDetail {
        conversation,
        messages,
    }))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(update): Json<ConversationUpdate>,
) -> ApiResult<Conversation> {
    let mut conversation = store::get_conversation(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Conversation"))?;
    let title = update.title.trim();
    if title.is_empty() || title.chars().count() > 120 {
        return Err(AppError::bad_request(
            "The title must be between 1 and 120 characters.",
        ));
    }
    conversation.title = title.to_owned();
    conversation.updated_at = now_ms();
    store::upsert_conversation(&state.db, conversation.clone()).await?;
    state.events.publish(Event::ConversationUpdated {
        conversation: conversation.clone(),
    });
    Ok(Json(conversation))
}

pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    state.generations.cancel(id);
    if !store::delete_conversation(&state.db, id).await? {
        return Err(AppError::not_found("Conversation"));
    }
    state.events.publish(Event::ConversationDeleted { id });
    Ok(Json(()))
}

pub async fn send(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<SendMessage>,
) -> ApiResult<SendMessageResult> {
    Ok(Json(chat::send(state, id, req.content, req.model).await?))
}

/// Stops the reply being written in this conversation, keeping what was written so far.
pub async fn cancel(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    state.generations.cancel(id);
    Ok(Json(()))
}
