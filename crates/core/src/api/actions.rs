use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hearth_protocol::ApproveAction;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::tools::Decision;

fn not_waiting() -> AppError {
    AppError::new(
        StatusCode::CONFLICT,
        "not_waiting",
        "This action isn't waiting for approval anymore.",
    )
}

/// Lets a pending action run, optionally with arguments the user edited.
pub async fn approve(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    body: Option<Json<ApproveAction>>,
) -> ApiResult<()> {
    let arguments = body.and_then(|Json(b)| b.arguments);
    if arguments.as_ref().is_some_and(|a| !a.is_object()) {
        return Err(AppError::bad_request("Edited arguments must be an object."));
    }
    if !state.approvals.decide(id, Decision::Approve(arguments)) {
        return Err(not_waiting());
    }
    Ok(Json(()))
}

/// Declines a pending action. It won't run, and the assistant is told so.
pub async fn reject(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    if !state.approvals.decide(id, Decision::Reject) {
        return Err(not_waiting());
    }
    Ok(Json(()))
}
