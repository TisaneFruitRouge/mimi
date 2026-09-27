//! The Calendar panel: every connected calendar and its events, merged.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// One calendar the user can see, from any account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CalendarInfo {
    /// Stable across restarts; used to show, hide and add to it.
    pub id: String,
    pub name: String,
    /// `#rrggbb`: the calendar's own colour when it has one, else a stable pick.
    pub color: String,
    /// Events are saved straight into it. Google calendars aren't: new events open in
    /// Google Calendar for the user to save.
    pub writable: bool,
    pub google: bool,
}

/// Someone on an event (organizer or guest), matched to People when possible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventPerson {
    /// As the invitation names them. Written by whoever sent it.
    pub name: Option<String>,
    pub email: String,
    /// The person in People with this address, if any.
    pub person_id: Option<Uuid>,
    pub person_name: Option<String>,
}

/// One occurrence of an event. Titles, places and notes come from calendars, which
/// other people can write into: show them as plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CalendarEvent {
    /// Stable per occurrence; the same id @ mentions and "remind me before" use.
    pub id: String,
    pub calendar_id: String,
    pub calendar: String,
    pub title: String,
    #[ts(type = "number")]
    pub start: i64,
    #[ts(type = "number")]
    pub end: i64,
    pub all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub organizer: Option<EventPerson>,
    pub attendees: Vec<EventPerson>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CalendarEvents {
    pub events: Vec<CalendarEvent>,
    /// Calendars that couldn't be read just now, with why, in plain words.
    pub unavailable: Vec<String>,
}

/// An event the user adds from the Calendar panel.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewCalendarEvent {
    pub calendar_id: String,
    pub title: String,
    /// Milliseconds; for all-day events, local midnight of the first day.
    #[ts(type = "number")]
    pub start: i64,
    /// Milliseconds; for all-day events, local midnight after the last day.
    #[ts(type = "number")]
    pub end: i64,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// What happened to a new event.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreatedEvent {
    /// Saved into the calendar.
    pub saved: bool,
    pub calendar: String,
    /// For Google calendars: the pre-filled page where the user presses Save.
    pub open_url: Option<String>,
}

/// A conversation where the user @-mentioned someone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonConversation {
    pub id: Uuid,
    pub title: String,
    #[ts(type = "number")]
    pub updated_at: i64,
}
