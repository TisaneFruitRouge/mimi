//! Settings › Permissions: the kinds of action the assistant may do on its own, and the
//! user's choices. The only way (with an approval card's "Don't ask again") to change
//! them.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{KindPermission, PermissionKind};

use super::error::ApiResult;
use crate::AppState;
use crate::tools::permissions;

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<PermissionKind>> {
    Ok(Json(permissions::catalog(&state).await?))
}

/// Replaces one kind's default and exceptions. Returns the whole catalog.
pub async fn put(
    State(state): State<Arc<AppState>>,
    Path(kind): Path<String>,
    Json(choice): Json<KindPermission>,
) -> ApiResult<Vec<PermissionKind>> {
    permissions::set(&state, &kind, choice).await?;
    Ok(Json(permissions::catalog(&state).await?))
}
