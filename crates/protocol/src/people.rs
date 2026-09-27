use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// How a person can be reached. Phone numbers cover calls and SMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Channel {
    Phone,
    Email,
    Telegram,
    Signal,
    Whatsapp,
    Matrix,
    Other,
}

/// One way to reach a person, and where Mimi learned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Handle {
    pub id: Uuid,
    pub channel: Channel,
    /// As the user would write it, e.g. "+41 79 123 45 67".
    pub value: String,
    /// "mobile", "work"…, when the source says.
    pub label: Option<String>,
    /// Where it comes from: a connection's id, or `None` when the user added it.
    pub source_id: Option<String>,
    /// Plain-language source, e.g. "iCloud" or "Added by you".
    pub source_name: String,
}

/// A contact card from a source, attached to a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonSource {
    pub source_id: String,
    pub source_name: String,
    /// The card's id within its source.
    pub record: String,
    /// The name on that card.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Person {
    pub id: Uuid,
    pub name: String,
    pub nickname: Option<String>,
    pub handles: Vec<Handle>,
    /// The cards merged into this person. Several means it was unified.
    pub sources: Vec<PersonSource>,
    /// Added by the user rather than imported.
    pub manual: bool,
}

/// A person in lists and pickers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonSummary {
    pub id: Uuid,
    pub name: String,
    pub nickname: Option<String>,
    /// Distinct channels, in a stable order.
    pub channels: Vec<Channel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewHandle {
    pub channel: Channel,
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewPerson {
    pub name: String,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub handles: Vec<NewHandle>,
}

/// Fields left `null` are unchanged; an empty nickname clears it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct PersonUpdate {
    pub name: Option<String>,
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergePeople {
    /// Folded into the person in the URL, then removed.
    pub other: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SplitPerson {
    pub source_id: String,
    pub record: String,
}

/// Two people who may be the same (same name, different cards).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DuplicateSuggestion {
    pub a: PersonSummary,
    pub b: PersonSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DismissDuplicate {
    pub a: Uuid,
    pub b: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MentionKind {
    Person,
    Event,
}

/// Something the user tagged with @ in a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Mention {
    pub kind: MentionKind,
    /// A person's id, or an event's id from the mention search.
    pub id: String,
    /// The text shown after the @, e.g. "Sam Carter".
    pub label: String,
}

/// A suggestion in the @ picker.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MentionCandidate {
    pub kind: MentionKind,
    pub id: String,
    pub label: String,
    /// One quiet line: an event's day and time and calendar, a nickname…
    pub detail: Option<String>,
    /// For people: how they can be reached.
    pub channels: Vec<Channel>,
    /// For events: when they start (ms since the epoch).
    #[ts(type = "number | null")]
    pub starts_at: Option<i64>,
}
