use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use mimi_protocol::UpdateStatus;

use super::error::ApiResult;
use crate::AppState;

/// What the last check found (nothing is asked of GitHub here).
pub async fn status(State(state): State<Arc<AppState>>) -> ApiResult<UpdateStatus> {
    Ok(Json(crate::updates::status(&state).await))
}

/// The user's "Check now": asks GitHub straight away, whatever the setting says, since the
/// user asked for it.
pub async fn check(State(state): State<Arc<AppState>>) -> ApiResult<UpdateStatus> {
    Ok(Json(crate::updates::check(&state).await))
}
