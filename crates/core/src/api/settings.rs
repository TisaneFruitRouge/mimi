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
    crate::persona::validate(&mut new).map_err(AppError::bad_request)?;
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
    if new.mail_sorter == mimi_protocol::MailSorter::Jev
        && crate::mail::jev::key(&state.db).await.is_none()
    {
        return Err(AppError::bad_request(
            "Add your TypeSafe API key before sorting mail with Jev.",
        ));
    }
    let current = settings::load(&state.db).await?;
    let sorter_changed = current.mail_sorter != new.mail_sorter;
    let update_check_on = new.update_check && !current.update_check;
    // Permissions change only through `/permissions`, where they're checked; a client
    // saving other settings can't widen them, even with a stale copy.
    new.permissions = current.permissions;
    settings::save(&state.db, &new).await?;
    if sorter_changed {
        // Where mail is sorted shows in the Mail panel.
        crate::mail::changed(&state);
        state.mail.triage_wake.notify_one();
    }
    if update_check_on {
        state.updates.wake.notify_one();
    }
    state.events.publish(Event::SettingsChanged {
        settings: new.clone(),
    });
    Ok(Json(new))
}
