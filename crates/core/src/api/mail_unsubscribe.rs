//! Unsubscribing from a mailing list (`mail::unsubscribe`). Only these handlers, the
//! user's own clicks in the Mail panel, ever unsubscribe: there is no assistant tool.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{MailArchivedCount, MailUnsubscribe};

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::mail::unsubscribe;

/// How to unsubscribe from a conversation's list (`null` when it doesn't say). For
/// older automatic mail this reads the message's headers from the server once.
pub async fn offer(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Option<MailUnsubscribe>> {
    unsubscribe::offer(&state, id)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

/// Unsubscribes: the user's click on "Unsubscribe", which is the approval.
pub async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<MailUnsubscribe> {
    unsubscribe::unsubscribe(&state, id)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

/// Archives every Inbox conversation from the same list.
pub async fn archive_all(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<MailArchivedCount> {
    unsubscribe::archive_list(&state, id)
        .await
        .map(|archived| Json(MailArchivedCount { archived }))
        .map_err(AppError::bad_request)
}
