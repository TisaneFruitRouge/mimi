use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use hearth_protocol::Settings;

use super::error::{ApiResult, AppError};
use crate::{AppState, settings};

pub async fn get(State(state): State<Arc<AppState>>) -> ApiResult<Settings> {
    Ok(Json(settings::load(&state.db).await?))
}

pub async fn put(
    State(state): State<Arc<AppState>>,
    Json(mut new): Json<Settings>,
) -> ApiResult<Settings> {
    new.assistant_name = new.assistant_name.trim().to_owned();
    if new.assistant_name.is_empty() || new.assistant_name.chars().count() > 40 {
        return Err(AppError::bad_request(
            "The assistant's name must be between 1 and 40 characters.",
        ));
    }
    settings::save(&state.db, &new).await?;
    Ok(Json(new))
}
