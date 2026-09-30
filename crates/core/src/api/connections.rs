use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{
    Connection, ConnectionSetup, GoogleSignIn, GoogleSignInInfo, GoogleSignInStatus, MatrixGroup,
    StartGoogleSignIn,
};
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::connections::calendar::google;
use crate::{AppState, connections};

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<Connection>> {
    Ok(Json(connections::list(&state).await?))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(setup): Json<ConnectionSetup>,
) -> ApiResult<Connection> {
    Ok(Json(connections::create(&state, setup).await?))
}

pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    if !connections::delete(&state, id).await? {
        return Err(AppError::not_found("Connection"));
    }
    Ok(Json(()))
}

/// The Matrix groups the assistant is in, for exceptions in Settings › Permissions.
pub async fn matrix_groups(State(state): State<Arc<AppState>>) -> ApiResult<Vec<MatrixGroup>> {
    let mut out: Vec<MatrixGroup> = Vec::new();
    for paired in connections::matrix::paired(&state).await {
        for g in connections::matrix::send::groups_of(&state, &paired).await {
            if !out.iter().any(|o| o.id == g.id) {
                out.push(MatrixGroup {
                    name: connections::matrix::send::group_name(&g),
                    id: g.id,
                    alias: g.alias,
                    members: g.members,
                });
            }
        }
    }
    out.sort_by_key(|g| g.name.to_lowercase());
    Ok(Json(out))
}

/// Whether "Sign in with Google" works in this build.
pub async fn google_info(State(state): State<Arc<AppState>>) -> ApiResult<GoogleSignInInfo> {
    Ok(Json(GoogleSignInInfo {
        available: state.connections.feeds.google.endpoints().is_some(),
    }))
}

/// Starts signing in with Google. The client opens the returned page in the browser,
/// on this computer: Google sends the browser back to a listener on 127.0.0.1.
pub async fn google_start(
    State(state): State<Arc<AppState>>,
    body: Option<Json<StartGoogleSignIn>>,
) -> ApiResult<GoogleSignIn> {
    let reconnect = body.and_then(|Json(b)| b.reconnect);
    Ok(Json(google::start_sign_in(&state, reconnect).await?))
}

pub async fn google_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<GoogleSignInStatus> {
    google::sign_in_status(&state, id)
        .map(Json)
        .ok_or_else(|| AppError::not_found("Sign-in"))
}

pub async fn google_cancel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<()> {
    if !google::cancel_sign_in(&state, id) {
        return Err(AppError::not_found("Sign-in"));
    }
    Ok(Json(()))
}
