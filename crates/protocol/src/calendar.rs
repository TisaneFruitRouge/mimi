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
    /// People can be invited to events saved in it (guests). False for calendars read
    /// through a private address, and for CalDAV accounts Mimi has no email address for.
    #[serde(default)]
    pub guests: bool,
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
    /// A guest's answer, when the calendar knows it.
    #[serde(default)]
    pub response: Option<GuestResponse>,
}

/// A guest's answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum GuestResponse {
    Accepted,
    Declined,
    Tentative,
    /// Invited, no answer yet.
    Pending,
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
    /// One occurrence of a repeating event.
    #[serde(default)]
    pub repeats: bool,
    /// The user organizes it (or nobody does): they may change its guests and send
    /// invitations. False for events someone else invited them to.
    #[serde(default)]
    pub mine: bool,
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
    /// People to invite: email addresses ("Sam <sam@example.com>" or bare). The calendar
    /// service emails nobody; the answer offers to send the invitations.
    #[serde(default)]
    pub guests: Vec<String>,
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
    /// Invitations the user may send now (Mimi emailed nothing; on Google calendars Google
    /// tells the guests itself, and `note` says so).
    #[serde(default)]
    pub invitations: Vec<InvitationOffer>,
    /// Something to tell the user, e.g. whom Google invited, or why the guests weren't added.
    #[serde(default)]
    pub note: Option<String>,
}

/// A change the user makes to one occurrence of an event in the Calendar panel. Fields
/// left out stay as they are; an empty place or notes removes it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventChange {
    #[serde(default)]
    pub title: Option<String>,
    /// Milliseconds, like `NewCalendarEvent`. Start and end go together.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub start: Option<i64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub end: Option<i64>,
    #[serde(default)]
    pub all_day: Option<bool>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Everyone who should be a guest afterwards (email addresses). Absent: unchanged.
    #[serde(default)]
    pub guests: Option<Vec<String>>,
}

/// What happened to a changed or removed event.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EventChanged {
    /// The occurrence's id afterwards (it changes when the event moves); None once
    /// removed.
    pub event_id: Option<String>,
    /// Messages the user may send the guests now (Mimi emailed nothing; on Google calendars
    /// Google tells the guests itself, and `note` says so).
    pub invitations: Vec<InvitationOffer>,
    /// Something to tell the user, e.g. whom Google told, or why the guests weren't told.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum InvitationKind {
    /// The invitation itself.
    Invite,
    /// The new details of an event the guests were invited to.
    Update,
    /// The event is cancelled.
    Cancel,
    /// These guests were taken off the event: their invitation is withdrawn.
    Uninvite,
}

/// Invitation emails Mimi can send for an event, from the user's own email account. Only
/// ever sent by the user's click, or by the assistant as Permissions allow.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InvitationOffer {
    pub id: Uuid,
    pub kind: InvitationKind,
    pub event_title: String,
    /// "on Friday 3 Oct, 19:00–21:00", in the computer's time zone.
    pub event_when: String,
    /// Who it goes to.
    pub guests: Vec<InvitationGuest>,
    /// The address it's sent from; None while no email account is connected.
    pub from: Option<String>,
    /// Why it's sent from that address, when it isn't the event's organizer.
    pub from_note: Option<String>,
    /// When it was sent, if it was.
    #[ts(type = "number | null")]
    pub sent_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InvitationGuest {
    pub name: Option<String>,
    pub email: String,
}

/// Sends an offer's emails, to all of its guests or only these addresses.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SendInvitations {
    #[serde(default)]
    pub guests: Option<Vec<String>>,
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
