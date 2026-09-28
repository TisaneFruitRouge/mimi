use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use mimi_protocol::ApproveAction;
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

/// Lets a pending action run, optionally with arguments the user edited. With `always`,
/// the user also chose the card's "Don't ask again for …": the exception the daemon
/// offered on that card is added to Settings › Permissions.
pub async fn approve(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    body: Option<Json<ApproveAction>>,
) -> ApiResult<()> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let arguments = body.arguments;
    if arguments.as_ref().is_some_and(|a| !a.is_object()) {
        return Err(AppError::bad_request("Edited arguments must be an object."));
    }
    // The offer was worked out for what the card showed; an edit may be about others.
    let offer = if body.always {
        if arguments.is_some() {
            return Err(AppError::bad_request(
                "\"Don't ask again\" can't be used with changes. Approve this one, then change Settings › Permissions.",
            ));
        }
        Some(
            state.approvals.offer_of(id).ok_or_else(|| {
                AppError::bad_request("There's nothing to remember for this one.")
            })?,
        )
    } else {
        None
    };
    if !state.approvals.decide(id, Decision::Approve(arguments)) {
        return Err(not_waiting());
    }
    if let Some(offer) = offer {
        crate::tools::permissions::allow_always(&state, offer.kind, &offer.targets).await?;
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
