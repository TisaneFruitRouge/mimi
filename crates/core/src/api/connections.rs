use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use mimi_protocol::{Connection, ConnectionSetup};
use uuid::Uuid;

use super::error::{ApiResult, AppError};
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
