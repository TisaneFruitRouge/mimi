//! The Calendar panel's reads and "New event", plus the calendar and conversation
//! sides of a person's page.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Duration, TimeZone, Utc};
use mimi_protocol::{
    CalendarEvent, CalendarEvents, CalendarInfo, Channel, CreatedEvent, EventPerson,
    NewCalendarEvent, PersonConversation,
};
use serde::Deserialize;
use uuid::Uuid;

use super::error::{ApiResult, AppError};
use crate::AppState;
use crate::connections::calendar::{self, Created, NewEvent, ics::CalEvent};
use crate::people::{self, normalize::match_key, store};

/// The longest span one request may read, so a stray query can't expand years of
/// recurring events.
const MAX_SPAN_DAYS: i64 = 400;

pub async fn calendars(State(state): State<Arc<AppState>>) -> ApiResult<Vec<CalendarInfo>> {
    let accounts = crate::connections::calendar_accounts(&state).await;
    Ok(Json(
        calendar::calendars(&accounts)
            .into_iter()
            .map(|c| CalendarInfo {
                id: c.id,
                name: c.name,
                color: c.color,
                writable: c.writable,
                google: c.google,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct Range {
    from: Option<i64>,
    to: Option<i64>,
}

fn range(r: &Range, default_days: i64) -> Result<(DateTime<Utc>, DateTime<Utc>), AppError> {
    let ms = |v: i64| {
        Utc.timestamp_millis_opt(v)
            .single()
            .ok_or_else(|| AppError::bad_request("That date is out of range."))
    };
    let from = r.from.map(ms).transpose()?.unwrap_or_else(Utc::now);
    let to = match r.to {
        Some(to) => ms(to)?,
        None => from
            .checked_add_signed(Duration::days(default_days))
            .ok_or_else(|| AppError::bad_request("That date is out of range."))?,
    };
    if to <= from {
        return Err(AppError::bad_request("The end must be after the start."));
    }
    if to - from > Duration::days(MAX_SPAN_DAYS) {
        return Err(AppError::bad_request("Ask for at most a year at a time."));
    }
    Ok((from, to))
}

pub async fn events(
    State(state): State<Arc<AppState>>,
    Query(r): Query<Range>,
) -> ApiResult<CalendarEvents> {
    let (from, to) = range(&r, 7)?;
    let (events, unavailable) = read(&state, from, to).await;
    Ok(Json(CalendarEvents {
        events: with_people(&state, events).await?,
        unavailable,
    }))
}

async fn read(
    state: &AppState,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> (Vec<CalEvent>, Vec<String>) {
    let accounts = crate::connections::calendar_accounts(state).await;
    calendar::events_between(&state.http, &state.connections.feeds, &accounts, from, to).await
}

fn email_key(email: &str) -> Option<String> {
    match_key(Channel::Email, email)
}

/// Events as clients see them, with organizers and guests matched to People by email.
async fn with_people(
    state: &AppState,
    events: Vec<CalEvent>,
) -> Result<Vec<CalendarEvent>, AppError> {
    let keys: Vec<String> = events
        .iter()
        .flat_map(|e| e.organizer.iter().chain(&e.attendees))
        .filter_map(|a| email_key(&a.email))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let people: HashMap<String, (Uuid, String)> = if keys.is_empty() {
        HashMap::new()
    } else {
        state
            .db
            .call(move |c| store::by_match_keys(c, &keys))
            .await?
    };
    let person = |a: &calendar::ics::Attendee| {
        let found = email_key(&a.email).and_then(|k| people.get(&k));
        EventPerson {
            name: a.name.clone(),
            email: a.email.clone(),
            person_id: found.map(|(id, _)| *id),
            person_name: found.map(|(_, name)| name.clone()),
        }
    };
    Ok(events
        .into_iter()
        .map(|e| CalendarEvent {
            id: people::mentions::event_id(&e),
            organizer: e.organizer.as_ref().map(&person),
            attendees: e.attendees.iter().map(&person).collect(),
            calendar_id: e.calendar_id,
            calendar: e.calendar,
            title: e.title,
            start: e.start.timestamp_millis(),
            end: e.end.timestamp_millis(),
            all_day: e.all_day,
            location: e.location,
            notes: e.notes,
        })
        .collect())
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(new): Json<NewCalendarEvent>,
) -> ApiResult<CreatedEvent> {
    let title = new.title.trim().to_owned();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::bad_request("Give the event a name."));
    }
    let ms = |v: i64| {
        Utc.timestamp_millis_opt(v)
            .single()
            .ok_or_else(|| AppError::bad_request("That date is out of range."))
    };
    let (start, end) = (ms(new.start)?, ms(new.end)?);
    if end <= start {
        return Err(AppError::bad_request("The event must end after it starts."));
    }
    let tidy = |s: Option<String>| s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let accounts = crate::connections::calendar_accounts(&state).await;
    let target = calendar::target_by_id(&accounts, &new.calendar_id)
        .ok_or_else(|| AppError::not_found("Calendar"))?;
    let event = NewEvent {
        title,
        start,
        end,
        all_day: new.all_day,
        location: tidy(new.location),
        notes: tidy(new.notes),
    };
    // The user pressed "Add" themselves: this is their own action, not the assistant's.
    let created = calendar::create(&target, &event)
        .await
        .map_err(|e| AppError::bad_request(format!("Couldn't add the event: {e}")))?;
    Ok(Json(match created {
        Created::Saved { calendar } => CreatedEvent {
            saved: true,
            calendar,
            open_url: None,
        },
        Created::OpenToSave { url } => CreatedEvent {
            saved: false,
            calendar: target.name().to_owned(),
            open_url: Some(url),
        },
    }))
}

/// Events with this person: they're invited (by email), or their name is in the title.
pub async fn person_events(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(r): Query<Range>,
) -> ApiResult<Vec<CalendarEvent>> {
    let person = people::get(&state, id)
        .await?
        .ok_or_else(|| AppError::not_found("Person"))?;
    let (from, to) = range(&r, 60)?;
    let keys: HashSet<String> = state
        .db
        .call(move |c| store::match_keys_of(c, id))
        .await?
        .into_iter()
        .collect();
    let names: Vec<String> = std::iter::once(person.name.as_str())
        .chain(person.nickname.as_deref())
        .map(people::fold)
        .filter(|n| n.chars().count() >= 3)
        .collect();
    let (events, _) = read(&state, from, to).await;
    let mine: Vec<CalEvent> = events
        .into_iter()
        .filter(|e| involves(e, &keys, &names))
        .collect();
    Ok(Json(with_people(&state, mine).await?))
}

fn involves(e: &CalEvent, keys: &HashSet<String>, names: &[String]) -> bool {
    let invited = e
        .organizer
        .iter()
        .chain(&e.attendees)
        .filter_map(|a| email_key(&a.email))
        .any(|k| keys.contains(&k));
    if invited {
        return true;
    }
    let title = format!(" {} ", people::fold(&e.title));
    // "Sam's birthday" folds to "sams birthday".
    names
        .iter()
        .any(|n| title.contains(&format!(" {n} ")) || title.contains(&format!(" {n}s ")))
}

pub async fn person_conversations(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<PersonConversation>> {
    let found = crate::chat::store::conversations_mentioning(&state.db, id, 20).await?;
    Ok(Json(
        found
            .into_iter()
            .map(|c| PersonConversation {
                id: c.id,
                title: c.title,
                updated_at: c.updated_at,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::calendar::ics::Attendee;

    fn event(title: &str, attendees: &[&str]) -> CalEvent {
        CalEvent {
            uid: "u".into(),
            title: title.into(),
            start: Utc::now(),
            end: Utc::now(),
            all_day: false,
            location: None,
            notes: None,
            calendar: "Home".into(),
            calendar_id: "c".into(),
            attendees: attendees
                .iter()
                .map(|e| Attendee {
                    name: None,
                    email: (*e).into(),
                })
                .collect(),
            organizer: None,
        }
    }

    #[test]
    fn events_involve_people_by_email_or_whole_name() {
        let keys: HashSet<String> = [email_key("Sam@Example.com").unwrap()].into();
        let names = vec![people::fold("Sam Carter"), people::fold("Sammy")];
        assert!(involves(
            &event("Standup", &["sam@example.com"]),
            &keys,
            &names
        ));
        assert!(involves(
            &event("Dinner with Sam Carter", &[]),
            &keys,
            &names
        ));
        assert!(involves(&event("Sammy's birthday", &[]), &keys, &names));
        assert!(involves(&event("sammy: drinks", &[]), &keys, &names));
        assert!(!involves(&event("Samantha Carter", &[]), &keys, &names));
        assert!(!involves(
            &event("Standup", &["lea@example.com"]),
            &keys,
            &names
        ));
    }
}
