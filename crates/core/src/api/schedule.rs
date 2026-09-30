use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use mimi_protocol::{
    Delivery, NewScheduleItem, ScheduleItem, ScheduleOccurrence, ScheduleUpdate, Snooze,
};
use serde::Deserialize;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::{AppState, schedule};

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Vec<ScheduleItem>> {
    Ok(Json(schedule::list(&state).await?))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(new): Json<NewScheduleItem>,
) -> ApiResult<ScheduleItem> {
    let item = schedule::create(&state, new, None).await?;
    Ok(Json(schedule::to_public(&item)))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(update): Json<ScheduleUpdate>,
) -> ApiResult<ScheduleItem> {
    schedule::owners_item(&state, id).await?;
    let item = schedule::update(&state, id, update).await?;
    Ok(Json(schedule::to_public(&item)))
}

pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    schedule::owners_item(&state, id).await?;
    if !schedule::delete(&state, id).await? {
        return Err(AppError::not_found("Reminder"));
    }
    Ok(Json(()))
}

/// Runs a routine now, outside its schedule.
pub async fn run(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    schedule::owners_item(&state, id).await?;
    schedule::run_now(&state, id).await?;
    Ok(Json(()))
}

#[derive(Deserialize)]
pub struct Recent {
    limit: Option<usize>,
}

pub async fn deliveries(
    State(state): State<Arc<AppState>>,
    Query(q): Query<Recent>,
) -> ApiResult<Vec<Delivery>> {
    let limit = q.limit.unwrap_or(30).clamp(1, 200);
    Ok(Json(schedule::store::recent(&state.db, limit).await?))
}

/// The longest span one request may ask for, as for calendar events.
const MAX_SPAN_MS: i64 = 400 * 24 * 60 * 60 * 1000;

#[derive(Deserialize)]
pub struct Range {
    from: Option<i64>,
    to: Option<i64>,
}

/// Every time reminders and routines go off between `from` and `to` (ms; the next week by
/// default), including what already happened, for the calendar.
pub async fn occurrences(
    State(state): State<Arc<AppState>>,
    Query(r): Query<Range>,
) -> ApiResult<Vec<ScheduleOccurrence>> {
    let from = r.from.unwrap_or_else(crate::now_ms);
    let to =
        r.to.unwrap_or_else(|| from.saturating_add(7 * 24 * 60 * 60 * 1000));
    if to <= from {
        return Err(AppError::bad_request("The end must be after the start."));
    }
    if to - from > MAX_SPAN_MS {
        return Err(AppError::bad_request("Ask for at most a year at a time."));
    }
    Ok(Json(schedule::occurrences(&state, from, to).await?))
}

fn already_handled() -> AppError {
    AppError::new(
        axum::http::StatusCode::CONFLICT,
        "already_handled",
        "This reminder was already dealt with.",
    )
}

pub async fn done(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    if !schedule::mark_done(&state, id).await {
        return Err(already_handled());
    }
    Ok(Json(()))
}

pub async fn snooze(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<Snooze>,
) -> ApiResult<()> {
    if !schedule::snooze(&state, id, req.minutes).await {
        return Err(already_handled());
    }
    Ok(Json(()))
}

/// Takes back a change the assistant made, from the "Undo" next to it in a chat.
pub async fn undo(State(state): State<Arc<AppState>>, Path(revision): Path<i64>) -> ApiResult<()> {
    schedule::undo(&state, revision).await?;
    Ok(Json(()))
}
