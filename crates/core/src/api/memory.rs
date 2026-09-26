use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hearth_protocol::{
    Event, MemoryLearning, MemoryNote, MemoryNoteEdit, MemoryOverview, MemoryProfileEdit,
    MemorySource,
};
use serde::Deserialize;

use super::error::{ApiResult, AppError};
use crate::memory::{self, NOTE_LIMIT, PROFILE_LIMIT, PROFILE_PATH, store};
use crate::{AppState, settings};

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

fn note_path(raw: &str) -> Result<String, AppError> {
    memory::normalize_path(raw)
        .filter(|p| p != PROFILE_PATH)
        .ok_or_else(|| AppError::bad_request("That isn't a valid note."))
}

fn no_secrets(text: &str) -> Result<(), AppError> {
    if memory::looks_secret(text) {
        return Err(AppError::bad_request(
            "This looks like a password, code or account number. To keep you safe, memory can't hold those.",
        ));
    }
    Ok(())
}

pub async fn overview(State(state): State<Arc<AppState>>) -> ApiResult<MemoryOverview> {
    let settings = settings::load(&state.db).await?;
    Ok(Json(MemoryOverview {
        profile: store::profile(&state.db).await?,
        profile_limit: PROFILE_LIMIT as u32,
        notes: store::list(&state.db).await?,
        learning: settings.memory_learning,
    }))
}

pub async fn get_note(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> ApiResult<MemoryNote> {
    let path = note_path(&q.path)?;
    store::get(&state.db, &path)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found("Note"))
}

pub async fn put_note(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
    Json(edit): Json<MemoryNoteEdit>,
) -> ApiResult<MemoryNote> {
    let path = note_path(&q.path)?;
    let body = edit.body.trim();
    if body.is_empty() {
        return Err(AppError::bad_request(
            "A note can't be empty. Delete it instead.",
        ));
    }
    if body.chars().count() > NOTE_LIMIT {
        return Err(AppError::bad_request(format!(
            "Notes can hold up to {NOTE_LIMIT} characters. Split this one into two."
        )));
    }
    no_secrets(body)?;
    store::put(
        &state.db,
        &path,
        edit.title.as_deref(),
        body,
        MemorySource::You,
        None,
    )
    .await?;
    state.events.publish(Event::MemoryChanged);
    store::get(&state.db, &path)
        .await?
        .map(Json)
        .ok_or_else(|| AppError::not_found("Note"))
}

pub async fn delete_note(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> ApiResult<()> {
    let path = note_path(&q.path)?;
    if store::delete(&state.db, &path, None).await?.is_none() {
        return Err(AppError::not_found("Note"));
    }
    state.events.publish(Event::MemoryChanged);
    Ok(Json(()))
}

pub async fn put_profile(
    State(state): State<Arc<AppState>>,
    Json(edit): Json<MemoryProfileEdit>,
) -> ApiResult<()> {
    let text = edit.text.trim();
    if text.chars().count() > PROFILE_LIMIT {
        return Err(AppError::bad_request(format!(
            "Keep this under {PROFILE_LIMIT} characters: it goes with every message. \
             Details belong in the notes below."
        )));
    }
    no_secrets(text)?;
    store::put(&state.db, PROFILE_PATH, None, text, MemorySource::You, None).await?;
    state.events.publish(Event::MemoryChanged);
    Ok(Json(()))
}

pub async fn put_learning(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MemoryLearning>,
) -> ApiResult<()> {
    let mut current = settings::load(&state.db).await?;
    current.memory_learning = req.learning;
    settings::save(&state.db, &current).await?;
    state
        .events
        .publish(Event::SettingsChanged { settings: current });
    state.events.publish(Event::MemoryChanged);
    Ok(Json(()))
}

/// Takes back one change, e.g. from the "Undo" next to "Remembered that…" in a chat.
pub async fn undo(State(state): State<Arc<AppState>>, Path(revision): Path<i64>) -> ApiResult<()> {
    match store::undo(&state.db, revision).await? {
        store::Undo::Done(_) => {}
        store::Undo::AlreadyUndone => {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "already_undone",
                "That was already undone.",
            ));
        }
        store::Undo::Unknown => return Err(AppError::not_found("Change")),
    }
    // Show it as undone in the chat line that made it.
    if let Some(conversation) = store::revision_conversation(&state.db, revision).await? {
        let messages = crate::chat::store::messages(&state.db, conversation).await?;
        for mut message in messages {
            let mut touched = false;
            for a in &mut message.actions {
                if let Some(out) = a.output.as_mut()
                    && out["revision"].as_i64() == Some(revision)
                {
                    out["undone"] = serde_json::Value::Bool(true);
                    touched = true;
                }
            }
            if touched {
                crate::chat::store::upsert_message(&state.db, message.clone()).await?;
                state.events.publish(Event::MessageUpdated { message });
            }
        }
    }
    state.events.publish(Event::MemoryChanged);
    Ok(Json(()))
}

pub async fn forget_all(State(state): State<Arc<AppState>>) -> ApiResult<()> {
    store::forget_all(&state.db).await?;
    state.events.publish(Event::MemoryChanged);
    Ok(Json(()))
}
