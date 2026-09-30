use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use mimi_protocol::{
    Conversation, ConversationDetail, ConversationUpdate, Event, ModelRef, NewConversation,
    SendMessage, SendMessageResult, VisionSupport,
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

/// A trusted person's conversation belongs to them: to the owner's clients it doesn't
/// exist (`access`).
fn theirs(state: &AppState, id: Uuid) -> Result<(), AppError> {
    if state.access.is_guest_conversation(id) {
        return Err(AppError::not_found("Conversation"));
    }
    Ok(())
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<ConversationDetail> {
    theirs(&state, id)?;
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
    theirs(&state, id)?;
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
    theirs(&state, id)?;
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
    theirs(&state, id)?;
    if req.attachments.len() > crate::attachments::MAX_PER_MESSAGE {
        return Err(AppError::bad_request(format!(
            "At most {} photos can go with one message.",
            crate::attachments::MAX_PER_MESSAGE
        )));
    }
    let attachments = req
        .attachments
        .into_iter()
        .map(crate::attachments::Upload::from_api)
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::bad_request)?;
    Ok(Json(
        chat::send_with_attachments(
            state,
            id,
            req.content,
            req.model,
            req.mentions,
            None,
            attachments,
        )
        .await?,
    ))
}

/// A photo sent with a message, for showing it. Only what the daemon made itself
/// (re-encoded JPEG or PNG) is ever served, with its own type, never sniffed, and
/// sandboxed if opened on its own.
pub async fn attachment(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<axum::response::Response, AppError> {
    // A guest's photos are theirs, like their conversation.
    match crate::attachments::conversation_of(&state.db, id).await? {
        Some(conversation) if !state.access.is_guest_conversation(conversation) => {}
        _ => return Err(AppError::not_found("Photo")),
    }
    let (meta, data) = crate::attachments::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Photo"))?;
    let mime = match meta.mime.as_str() {
        "image/jpeg" | "image/png" => meta.mime.as_str(),
        _ => "application/octet-stream",
    };
    axum::response::Response::builder()
        .header("content-type", mime)
        .header("content-length", data.len())
        .header("x-content-type-options", "nosniff")
        .header("content-security-policy", "sandbox; default-src 'none'")
        .header("cache-control", "private, max-age=31536000, immutable")
        .header("cross-origin-resource-policy", "same-origin")
        .body(axum::body::Body::from(data))
        .map_err(AppError::internal)
}

/// Which model `GET /v1/models/vision` asks about: the default one unless given.
#[derive(Debug, serde::Deserialize)]
pub struct VisionQuery {
    provider_id: Option<uuid::Uuid>,
    model: Option<String>,
}

/// Whether a model (the default one, unless given) can see photos, so the composer can
/// say so before they're sent.
pub async fn vision(
    State(state): State<Arc<AppState>>,
    Query(q): Query<VisionQuery>,
) -> ApiResult<VisionSupport> {
    let model = match (q.provider_id, q.model) {
        (Some(provider_id), Some(model)) => Some(ModelRef { provider_id, model }),
        _ => crate::settings::load(&state.db).await?.default_model,
    };
    let Some(model) = model else {
        return Ok(Json(VisionSupport { sees_images: false }));
    };
    let sees_images = match crate::providers::store::get(&state.db, model.provider_id).await? {
        Some(record) => crate::providers::vision::sees_images(&state, &record, &model.model).await,
        None => false,
    };
    Ok(Json(VisionSupport { sees_images }))
}

/// Stops the reply being written in this conversation, keeping what was written so far.
pub async fn cancel(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    theirs(&state, id)?;
    state.generations.cancel(id);
    Ok(Json(()))
}
