//! Reminders and routines: things Mimi tells the user, or does for them, at set times.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScheduleKind {
    /// Tells the user something at the right time.
    Reminder,
    /// The assistant carries out an instruction on a schedule and reports back.
    Routine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

/// When something happens. Stored as a rule, not as instants, so times stay right
/// across daylight-saving changes and follow the computer's time zone. Times are
/// local, `HH:MM`; dates are local, `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Schedule {
    /// Once, at a local date and time (`YYYY-MM-DDTHH:MM`).
    Once {
        at: String,
    },
    Daily {
        time: String,
    },
    /// Monday to Friday.
    Weekdays {
        time: String,
    },
    Weekly {
        days: Vec<Weekday>,
        time: String,
    },
    /// On a day of the month; months that are too short use their last day.
    Monthly {
        day: u8,
        time: String,
    },
    /// Every year on a date, e.g. birthdays. 29 February falls back to the 28th.
    Yearly {
        month: u8,
        day: u8,
        time: String,
    },
    /// Every so many minutes, counted from when it was set up.
    Interval {
        minutes: u32,
    },
    /// Some time before a calendar event, following it if it moves.
    BeforeEvent {
        /// An event id as used by @ mentions (`ev:…`).
        event_id: String,
        event_title: String,
        minutes_before: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScheduleItem {
    pub id: Uuid,
    pub kind: ScheduleKind,
    /// What to remind about, or the routine's name.
    pub title: String,
    /// For routines: what the assistant does each time.
    pub instruction: Option<String>,
    pub schedule: Schedule,
    /// The schedule in words, e.g. "Every weekday at 07:00".
    pub description: String,
    pub paused: bool,
    /// When it happens next, if ever. Includes snoozes.
    #[ts(type = "number | null")]
    pub next_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_at: Option<i64>,
    /// Why it won't happen again, e.g. "The event was cancelled". `None` while active.
    pub ended: Option<String>,
    /// For routines: the conversation their results go to.
    pub conversation_id: Option<Uuid>,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DeliveryStatus {
    /// Sent on time.
    Delivered,
    /// Sent after its time, e.g. because the computer was asleep.
    Late,
    /// Too long ago to still be useful: recorded, not sent.
    Missed,
    /// The user marked it done.
    Done,
    Snoozed,
    /// A routine that is still running (or waiting for an approval).
    Running,
    Failed,
    /// A routine run that couldn't start, e.g. because the previous one was still busy.
    Skipped,
}

/// One time a reminder went off or a routine ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Delivery {
    pub id: Uuid,
    pub item_id: Uuid,
    pub kind: ScheduleKind,
    pub title: String,
    /// The time it was due.
    #[ts(type = "number")]
    pub due_at: i64,
    /// When it actually happened.
    #[ts(type = "number")]
    pub at: i64,
    pub status: DeliveryStatus,
    pub detail: Option<String>,
    /// For routines: where the result is.
    pub conversation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewScheduleItem {
    pub kind: ScheduleKind,
    pub title: String,
    #[serde(default)]
    pub instruction: Option<String>,
    pub schedule: Schedule,
}

/// Fields left `null` are unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ScheduleUpdate {
    pub title: Option<String>,
    pub instruction: Option<String>,
    pub schedule: Option<Schedule>,
    pub paused: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Snooze {
    pub minutes: u32,
}
