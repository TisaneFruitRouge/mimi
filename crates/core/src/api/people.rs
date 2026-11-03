use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use mimi_protocol::{
    DismissDuplicate, DuplicateSuggestion, EmailSuggestion, Event, GuestApproval, MentionCandidate,
    MergePeople, MergePreview, MergeRequest, MergeResult, NewHandle, NewPerson, Person,
    PersonAccess, PersonAccessUpdate, PersonSummary, PersonUpdate, RemovedPerson, SplitPerson,
};
use serde::Deserialize;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::people::{self, CardHandle, merge, normalize, store};
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

/// What someone may ask the assistant (their card's section).
pub async fn access(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<PersonAccess> {
    Ok(Json(crate::access::view(&state, id).await?))
}

/// The user's own choice for someone: whether they may ask the assistant things, the
/// calendars shared with them, and who approves. Turning it off deletes their
/// conversations and reminders.
pub async fn set_access(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(change): Json<PersonAccessUpdate>,
) -> ApiResult<PersonAccess> {
    Ok(Json(crate::access::update(&state, id, change).await?))
}

/// What people the user trusts asked for that waits for the user's own OK (only the
/// actions, never their conversations).
pub async fn guest_approvals(State(state): State<Arc<AppState>>) -> ApiResult<Vec<GuestApproval>> {
    Ok(Json(crate::access::waiting_for_owner(&state).await))
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

/// Deletes someone from Mimi only: their address books and mail are never touched, and
/// syncs no longer bring them back. Returns how to find them under "Removed contacts",
/// or `null` if nothing of them is left to bring back (someone the user added).
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Option<RemovedPerson>> {
    let restorable = state
        .db
        .call(move |c| store::delete(c, id, now_ms()))
        .await?
        .ok_or_else(|| AppError::not_found("Person"))?;
    changed(&state);
    // What they had with the assistant goes with them (their access went with the row).
    crate::access::revoke(&state, id).await;
    if !restorable {
        return Ok(Json(None));
    }
    Ok(Json(
        removed_people(&state)
            .await?
            .into_iter()
            .find(|r| r.id == id),
    ))
}

async fn removed_people(state: &AppState) -> Result<Vec<RemovedPerson>, AppError> {
    let names = people::source_names(state).await?;
    let removed = state.db.call(|c| store::removed(c)).await?;
    Ok(removed
        .into_iter()
        .map(|r| {
            let mut sources: Vec<String> = Vec::new();
            for s in &r.sources {
                let name = names
                    .get(s)
                    .cloned()
                    .unwrap_or_else(|| "A removed connection".to_owned());
                if !sources.contains(&name) {
                    sources.push(name);
                }
            }
            RemovedPerson {
                id: r.id,
                name: r.name,
                nickname: r.nickname,
                removed_at: r.removed_at,
                sources,
            }
        })
        .collect())
}

/// People the user deleted who can be brought back, most recent first.
pub async fn removed(State(state): State<Arc<AppState>>) -> ApiResult<Vec<RemovedPerson>> {
    Ok(Json(removed_people(&state).await?))
}

/// Brings a deleted person back, with the same id, and refreshes contacts so their
/// cards return to them.
pub async fn restore(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Person> {
    if !state
        .db
        .call(move |c| store::restore(c, id, now_ms()))
        .await?
    {
        return Err(AppError::not_found("Removed contact"));
    }
    changed(&state);
    people::sync_all(&state).await;
    found(people::get(&state, id).await?)
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

pub async fn update_handle(
    State(state): State<Arc<AppState>>,
    Path((id, handle)): Path<(Uuid, Uuid)>,
    Json(h): Json<NewHandle>,
) -> ApiResult<Person> {
    let new = valid_handle(&h)?;
    if !state
        .db
        .call(move |c| store::update_manual_handle(c, id, handle, &new))
        .await?
    {
        return Err(AppError::bad_request(
            "This comes from an address book. Change it there, and Mimi follows.",
        ));
    }
    changed(&state);
    found(people::get(&state, id).await?)
}

fn refused(r: &merge::Refusal) -> AppError {
    match r {
        merge::Refusal::Missing => AppError::not_found("Person"),
        other => AppError::bad_request(other.message()),
    }
}

/// Merges one person into the one in the URL (the "Possible duplicates" button of older
/// clients). `POST /people/merge` does several, with a name, and can be undone.
pub async fn merge(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<MergePeople>,
) -> ApiResult<Person> {
    let done = merge_people(
        &state,
        MergeRequest {
            keep: id,
            others: vec![req.other],
            name: None,
        },
    )
    .await?;
    Ok(Json(done.person))
}

/// What merging would give, for the confirmation: names to choose from and every way
/// to reach them.
pub async fn merge_preview(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MergeRequest>,
) -> ApiResult<MergePreview> {
    let names = people::source_names(&state).await?;
    state
        .db
        .call(move |c| merge::preview(c, req.keep, &req.others, &names))
        .await?
        .map(Json)
        .map_err(|r| refused(&r))
}

/// Merges the people the user picked into `keep`: their own choice, never a guess.
pub async fn merge_many(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MergeRequest>,
) -> ApiResult<MergeResult> {
    Ok(Json(merge_people(&state, req).await?))
}

async fn merge_people(state: &AppState, req: MergeRequest) -> Result<MergeResult, AppError> {
    let name = req.name.as_deref().map(valid_name).transpose()?;
    let MergeRequest { keep, others, .. } = req;
    let done = state
        .db
        .call(move |c| merge::merge(c, keep, &others, name.as_deref(), now_ms()))
        .await?
        .map_err(|r| refused(&r))?;
    after_merge(state, done.notes_changed, done.settings_changed).await;
    let person = people::get(state, keep)
        .await?
        .ok_or_else(|| AppError::not_found("Person"))?;
    Ok(MergeResult {
        person,
        merge_id: done.id,
    })
}

/// Tells every client what a merge (or its undo) changed.
async fn after_merge(state: &AppState, notes: bool, settings: bool) {
    changed(state);
    if notes {
        state.events.publish(Event::MemoryChanged);
    }
    if settings && let Ok(settings) = crate::settings::load(&state.db).await {
        state.events.publish(Event::SettingsChanged { settings });
    }
}

/// Undoes a merge exactly: everyone comes back under their own id. Returns the person
/// they had been merged into.
pub async fn undo_merge(
    State(state): State<Arc<AppState>>,
    Path(merge_id): Path<Uuid>,
) -> ApiResult<Person> {
    let undone = state
        .db
        .call(move |c| merge::undo(c, merge_id, now_ms()))
        .await?
        .map_err(|r| AppError::new(axum::http::StatusCode::CONFLICT, "conflict", r.message()))?;
    after_merge(&state, undone.notes_changed, undone.settings_changed).await;
    found(people::get(&state, undone.keep).await?)
}

pub async fn split(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(req): Json<SplitPerson>,
) -> ApiResult<Person> {
    let fresh = state
        .db
        .call(move |c| store::split(c, id, req.source_id.as_deref(), &req.record, now_ms()))
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

/// People's email addresses matching `q`, for an email's To or Cc and an event's Guests.
pub async fn emails(
    State(state): State<Arc<AppState>>,
    Query(s): Query<Search>,
) -> ApiResult<Vec<EmailSuggestion>> {
    Ok(Json(people::emails::suggest(&state, &s.q, 8).await?))
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
