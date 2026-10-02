//! The user's email signatures: `GET|PUT /v1/mail/signatures` (see `mail::signature`).

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use mimi_protocol::MailSignatures;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::mail;

pub async fn get(State(state): State<Arc<AppState>>) -> ApiResult<MailSignatures> {
    Ok(Json(mail::signature::load(&state.db).await))
}

/// Saves them, cleaned (formatting as in an email, small pictures only); returns them as
/// saved.
pub async fn put(
    State(state): State<Arc<AppState>>,
    Json(input): Json<MailSignatures>,
) -> ApiResult<MailSignatures> {
    mail::signature::save(&state, input)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}
