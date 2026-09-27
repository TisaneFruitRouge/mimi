use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use mimi_protocol::{Event, Settings};

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
    for model in [&new.default_model, &new.pending_model]
        .into_iter()
        .flatten()
    {
        if crate::providers::store::get(&state.db, model.provider_id)
            .await?
            .is_none()
        {
            return Err(AppError::bad_request(
                "The chosen model's provider doesn't exist.",
            ));
        }
    }
    settings::save(&state.db, &new).await?;
    state.events.publish(Event::SettingsChanged {
        settings: new.clone(),
    });
    Ok(Json(new))
}
