use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use mimi_protocol::{
    DraftRequest, MailDraft, MailOverview, MailPreset, MailSummary, MailThread, MailThreadDetail,
    MarkRead,
};
use serde::Deserialize;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::mail::{self, store};

pub async fn presets() -> ApiResult<Vec<MailPreset>> {
    Ok(Json(mail::presets()))
}

pub async fn overview(State(state): State<Arc<AppState>>) -> ApiResult<MailOverview> {
    mail::overview(&state)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// needs_reply | important | other | inbox | sent | archive
    view: Option<String>,
    /// Words to search for.
    q: Option<String>,
    /// A person in the directory: conversations with any of their addresses.
    person: Option<Uuid>,
    /// Paging: conversations whose last message is older than this.
    before: Option<i64>,
    limit: Option<u32>,
}

pub async fn threads(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> ApiResult<Vec<MailThread>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    if let Some(person) = q.person {
        return mail::threads_with(&state, person, limit)
            .await
            .map(Json)
            .map_err(AppError::bad_request);
    }
    let view = match q.view.as_deref() {
        None | Some("") => None,
        Some(v) => {
            Some(mail::parse_view(v).ok_or_else(|| AppError::bad_request("Unknown mail view."))?)
        }
    };
    let query = store::Query {
        view,
        search: q.q.filter(|s| !s.trim().is_empty()),
        before: q.before,
        limit,
        ..Default::default()
    };
    mail::threads(&state, query)
        .await
        .map(Json)
        .map_err(AppError::internal)
}

pub async fn thread(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<MailThreadDetail> {
    mail::thread(&state, id)
        .await
        .map_err(AppError::internal)?
        .map(Json)
        .ok_or_else(|| AppError::not_found("Conversation"))
}

pub async fn read(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<MarkRead>,
) -> ApiResult<()> {
    mail::mark_read(&state, id, body.read)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(()))
}

pub async fn archive(State(state): State<Arc<AppState>>, Path(id): Path<i64>) -> ApiResult<()> {
    mail::archive(&state, id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(()))
}

pub async fn summarize(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<MailSummary> {
    let summary = mail::triage::summarize(&state, id)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(MailSummary { summary }))
}

pub async fn draft(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<DraftRequest>>,
) -> ApiResult<MailDraft> {
    let instructions = body.and_then(|Json(b)| b.instructions);
    mail::triage::draft_reply(&state, id, instructions)
        .await
        .map(Json)
        .map_err(AppError::bad_request)
}

/// Sends a message the user wrote and sent themselves, from the Mail panel or a draft
/// card: their click is the approval.
pub async fn send(
    State(state): State<Arc<AppState>>,
    Json(draft): Json<MailDraft>,
) -> ApiResult<()> {
    mail::send(&state, draft)
        .await
        .map_err(AppError::bad_request)?;
    Ok(Json(()))
}

/// Checks every account for new mail now.
pub async fn refresh(State(state): State<Arc<AppState>>) -> ApiResult<()> {
    for account in mail::accounts(&state).await {
        state.mail.poke(account.id);
    }
    Ok(Json(()))
}
