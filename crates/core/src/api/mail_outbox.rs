//! The outbox (`mail::outbox`): the user's Send with its Undo, and Send later. Every
//! handler here is the user's own click, which is the approval.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{MailDraft, NewOutgoingMail, OutgoingMail, RescheduleMail};
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::mail::outbox;

/// Queues a message: after the seconds Undo is offered, or at the time chosen.
pub async fn queue(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NewOutgoingMail>,
) -> ApiResult<OutgoingMail> {
    outbox::queue(&state, req.draft, req.send_at, false)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<OutgoingMail>> {
    outbox::list(&state)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

/// Takes a message back (Undo, Cancel) and returns its draft.
pub async fn cancel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<MailDraft> {
    outbox::cancel(&state, id)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

pub async fn reschedule(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<RescheduleMail>,
) -> ApiResult<OutgoingMail> {
    outbox::reschedule(&state, id, req.send_at)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

pub async fn send_now(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    outbox::send_now(&state, id)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}
