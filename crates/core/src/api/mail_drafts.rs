//! Saved drafts (`mail::drafts`): the editor saves as the user writes; the Drafts view
//! lists them, here and from the servers' Drafts folders. Nothing here sends: sending is
//! the outbox, on the user's click.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{MailDraft, MailDraftInfo, SaveMailDraft};
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::mail::drafts;

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<MailDraftInfo>> {
    drafts::list(&state)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

/// One draft, to continue it (a draft from another app is read from the server).
pub async fn get(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<MailDraft> {
    match drafts::get(&state, id).await {
        Ok(Some(draft)) => Ok(Json(draft)),
        Ok(None) => Err(AppError::not_found("draft")),
        Err(e) => Err(AppError::bad_request(e)),
    }
}

pub async fn save(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<SaveMailDraft>,
) -> ApiResult<()> {
    drafts::save(&state, id, req.draft, req.now)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

/// Discard: gone here at once, and from the server's Drafts folder soon after.
pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    drafts::delete(&state, id)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

/// The editor closed: copy the draft to the server now.
pub async fn mirror(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    drafts::mirror_soon(&state, id)
        .await
        .map(Json)
        .map_err(AppError::internal)
}
