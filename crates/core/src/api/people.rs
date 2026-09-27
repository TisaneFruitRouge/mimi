use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use mimi_protocol::{
    DismissDuplicate, DuplicateSuggestion, Event, MentionCandidate, MergePeople, NewHandle,
    NewPerson, Person, PersonSummary, PersonUpdate, SplitPerson,
};
use serde::Deserialize;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::people::{self, CardHandle, normalize, store};
use crate::{AppState, now_ms};

#[derive(Deserialize)]
pub struct Search {
    #[serde(default)]
    q: String,
    /// For mentions: `mail` suggests conversations and emails (the # picker) instead
    /// of people and events.
    #[serde(default)]
    kind: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(s): Query<Search>,
) -> ApiResult<Vec<PersonSummary>> {
    Ok(Json(people::search(&state, &s.q, 500).await?))
}

pub async fn get(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<Person> {
    found(people::get(&state, id).await?)
}

fn found(p: Option<Person>) -> ApiResult<Person> {
    p.map(Json).ok_or_else(|| AppError::not_found("Person"))
}

fn valid_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::bad_request(
            "The name must be between 1 and 120 characters.",
        ));
    }
    Ok(name.to_owned())
}

fn valid_handle(h: &NewHandle) -> Result<CardHandle, AppError> {
    let value = normalize::clean(h.channel, &h.value);
    if value.is_empty() || value.chars().count() > 200 {
        return Err(AppError::bad_request(
            "Enter a phone number, address or username.",
        ));
    }
    if matches!(
        h.channel,
        mimi_protocol::Channel::Email
            | mimi_protocol::Channel::Phone
            | mimi_protocol::Channel::Telegram
    ) && normalize::match_key(h.channel, &value).is_none()
    {
        return Err(AppError::bad_request(match h.channel {
            mimi_protocol::Channel::Email => "That doesn't look like an email address.",
            mimi_protocol::Channel::Telegram => "That doesn't look like a Telegram username.",
            _ => "That doesn't look like a phone number.",
        }));
    }
    Ok(CardHandle {
        channel: h.channel,
        value,
        label: h
            .label
            .as_ref()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty()),
    })
}

fn changed(state: &AppState) {
    state.events.publish(Event::PeopleChanged);
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(new): Json<NewPerson>,
) -> ApiResult<Person> {
    let name = valid_name(&new.name)?;
    let nickname = new
        .nickname
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty());
    let handles = new
        .handles
        .iter()
        .map(valid_handle)
        .collect::<Result<Vec<_>, _>>()?;
    let id = state
        .db
        .call(move |c| store::create_manual(c, &name, nickname.as_deref(), &handles, now_ms()))
        .await?;
    changed(&state);
    found(people::get(&state, id).await?)
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(update): Json<PersonUpdate>,
) -> ApiResult<Person> {
    let name = update.name.as_deref().map(valid_name).transpose()?;
    let nickname = update.nickname.map(|n| n.trim().to_owned());
    let exists = state
        .db
        .call(move |c| {
            if !store::exists(c, id)? {
                return Ok(false);
            }
            let nick = nickname.as_deref().map(|n| (!n.is_empty()).then_some(n));
            store::rename(c, id, name.as_deref(), nick, now_ms())?;
            Ok(true)
        })
        .await?;
    if !exists {
        return Err(AppError::not_found("Person"));
    }
    changed(&state);
    found(people::get(&state, id).await?)
}

pub async fn delete(State(state): State<Arc<AppState>>, Path(id): Path<Uuid>) -> ApiResult<()> {
    if !state.db.call(move |c| store::delete_manual(c, id)).await? {
        return Err(AppError::bad_request(
            "Only people you added yourself can be removed. Others come from your address books.",
        ));
    }
    changed(&state);
    Ok(Json(()))
}

pub async fn add_handle(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(h): Json<NewHandle>,
) -> ApiResult<Person> {
    let handle = valid_handle(&h)?;
    let exists = state
        .db
        .call(move |c| {
            if !store::exists(c, id)? {
                return Ok(false);
            }
            store::add_handle(c, id, &handle, now_ms())?;
            Ok(true)
        })
        .await?;
    if !exists {
        return Err(AppError::not_found("Person"));
    }
    changed(&state);
    found(people::get(&state, id).await?)
}

pub async fn remove_handle(
    State(state): State<Arc<AppState>>,
    Path((id, handle)): Path<(Uuid, Uuid)>,
) -> ApiResult<Person> {
    if !state
        .db
        .call(move |c| store::remove_manual_handle(c, id, handle))
        .await?
    {
        return Err(AppError::bad_request(
            "This comes from an address book. Change it there, and Mimi follows.",
        ));
    }
    changed(&state);
    found(people::get(&state, id).await?)
}

pub async fn merge(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<MergePeople>,
) -> ApiResult<Person> {
    let other = req.other;
    if other == id {
        return Err(AppError::bad_request("That's the same person."));
    }
    let both = state
        .db
        .call(move |c| Ok(store::exists(c, id)? && store::exists(c, other)?))
        .await?;
    if !both {
        return Err(AppError::not_found("Person"));
    }
    state
        .db
        .call(move |c| store::merge(c, id, other, now_ms()))
        .await?;
    changed(&state);
    found(people::get(&state, id).await?)
}

pub async fn split(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<SplitPerson>,
) -> ApiResult<Person> {
    let fresh = state
        .db
        .call(move |c| store::split(c, id, &req.source_id, &req.record, now_ms()))
        .await?
        .ok_or_else(|| AppError::not_found("Card"))?;
    changed(&state);
    found(people::get(&state, fresh).await?)
}

pub async fn duplicates(State(state): State<Arc<AppState>>) -> ApiResult<Vec<DuplicateSuggestion>> {
    let pairs = state.db.call(|c| store::duplicates(c)).await?;
    Ok(Json(
        pairs
            .into_iter()
            .map(|(a, b)| DuplicateSuggestion { a, b })
            .collect(),
    ))
}

pub async fn dismiss_duplicate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DismissDuplicate>,
) -> ApiResult<()> {
    state
        .db
        .call(move |c| store::set_apart(c, req.a, req.b))
        .await?;
    changed(&state);
    Ok(Json(()))
}

/// Refreshes contacts from every source now.
pub async fn sync(State(state): State<Arc<AppState>>) -> ApiResult<()> {
    people::sync_all(&state).await;
    Ok(Json(()))
}

pub async fn mentions(
    State(state): State<Arc<AppState>>,
    Query(s): Query<Search>,
) -> ApiResult<Vec<MentionCandidate>> {
    if s.kind.as_deref() == Some("mail") {
        return Ok(Json(
            crate::mail::mentions::candidates(&state, &s.q, 6).await,
        ));
    }
    Ok(Json(people::mentions::candidates(&state, &s.q, 6).await))
}
