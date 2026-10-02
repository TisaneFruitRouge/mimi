//! Listening and speaking: `/v1/voice/…`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine;
use mimi_protocol::{SpeakRequest, Speech, TranscribeRequest, Transcript, VoiceStatus};

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::voice::{self, ListenError};

pub async fn status(State(state): State<Arc<AppState>>) -> ApiResult<VoiceStatus> {
    Ok(Json(voice::status(&state).await))
}

pub async fn transcribe(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TranscribeRequest>,
) -> ApiResult<Transcript> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.data.trim())
        .map_err(|_| AppError::bad_request("That recording couldn't be read."))?;
    match voice::transcribe(&state, bytes).await {
        Ok(transcript) => Ok(Json(transcript)),
        Err(e @ ListenError::NotReady(_)) => Err(AppError::new(
            StatusCode::CONFLICT,
            "voice_not_ready",
            e.message(),
        )),
        Err(e) => Err(AppError::bad_request(e.message())),
    }
}

/// Loads the recognizer while the user is still talking.
pub async fn prepare(State(state): State<Arc<AppState>>) -> ApiResult<()> {
    voice::prepare(&state);
    Ok(Json(()))
}

pub async fn speak(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SpeakRequest>,
) -> ApiResult<Speech> {
    voice::speak(&state, req)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<VoiceStatus> {
    voice::start_download(&state, &id).map_err(AppError::bad_request)?;
    Ok(Json(voice::status(&state).await))
}

pub async fn cancel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult<()> {
    voice::cancel_download(&state, &id);
    Ok(Json(()))
}

pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<VoiceStatus> {
    voice::remove(&state, &id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(voice::status(&state).await))
}
