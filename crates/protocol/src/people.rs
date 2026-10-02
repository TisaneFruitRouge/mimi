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

/// A contact card attached to a person: from a source, or what the user had added by
/// hand to someone they later merged into this person. Each can be separated again
/// ("Not the same person", `POST /people/{id}/split`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonSource {
    /// The connection the card comes from, or `None` for what the user added by hand.
    pub source_id: Option<String>,
    pub source_name: String,
    /// The card's id within its source; for what the user added, the id that contact
    /// had before the merge.
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
    /// Added by the user rather than imported. (People merged into them become cards
    /// in `sources` instead.)
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
    /// A few of their numbers and addresses, for telling people apart in pickers.
    #[serde(default)]
    pub reach: Vec<String>,
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

/// `POST /people/{id}/merge`: one person into another. `POST /people/merge` takes
/// several at once, and a name.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergePeople {
    /// Folded into the person in the URL, then removed.
    pub other: Uuid,
}

/// The user's choice to merge people (`POST /people/merge`; `POST
/// /people/merge/preview` shows the result first). Never automatic: contacts only
/// unify on their own through a shared number, address or username.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeRequest {
    /// Whose id stays. Old links to the others find this person.
    pub keep: Uuid,
    /// Folded into `keep`, then gone.
    pub others: Vec<Uuid>,
    /// The merged person's name; `None` keeps `keep`'s.
    #[serde(default)]
    pub name: Option<String>,
}

/// What a merge would give, for the confirmation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergePreview {
    /// Everyone being merged, `keep` first.
    pub people: Vec<PersonSummary>,
    /// Their different names, `keep`'s first: the names to choose from.
    pub names: Vec<String>,
    /// Every way to reach the merged person; the same number or address counts once.
    pub handles: Vec<Handle>,
}

/// A merge that happened.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MergeResult {
    pub person: Person,
    /// Undoes it exactly, while nothing has come to depend on it:
    /// `POST /people/merges/{merge_id}/undo`.
    pub merge_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SplitPerson {
    /// The card's `PersonSource.source_id`: `None` for what the user added by hand.
    #[serde(default)]
    pub source_id: Option<String>,
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

/// Someone the user deleted from Mimi (not from their address book), who can be
/// brought back. `DELETE /people/{id}` returns this, or `null` when nothing is left to
/// bring back (someone the user had added themselves).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemovedPerson {
    /// The id they had, and get back when restored.
    pub id: Uuid,
    pub name: String,
    pub nickname: Option<String>,
    /// When they were deleted (ms since the epoch).
    #[ts(type = "number")]
    pub removed_at: i64,
    /// Plain-language names of where their cards come from, e.g. "iCloud".
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MentionKind {
    Person,
    Event,
    /// An email conversation (tagged with #).
    MailThread,
    /// One email (tagged with #).
    MailMessage,
}

impl MentionKind {
    /// What the tag starts with in the message text: `#` for email, `@` otherwise.
    pub fn sigil(self) -> char {
        match self {
            Self::MailThread | Self::MailMessage => '#',
            Self::Person | Self::Event => '@',
        }
    }
}

/// Something the user tagged with @ (people, events) or # (email) in a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Mention {
    pub kind: MentionKind,
    /// A person's id, an event's id from the mention search, or an email
    /// conversation's or message's number.
    pub id: String,
    /// The text shown after the @ or #, e.g. "Sam Carter".
    pub label: String,
}

/// Someone to write to or invite, as the user types in an email's To or Cc or an
/// event's Guests (`GET /v1/people/emails?q=`): a person in People, by one of their
/// email addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EmailSuggestion {
    pub person_id: Uuid,
    pub name: String,
    pub email: String,
}

/// A suggestion in the @ picker.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MentionCandidate {
    pub kind: MentionKind,
    pub id: String,
    pub label: String,
    /// One quiet line: an event's day and time and calendar, a nickname, who an email
    /// is from…
    pub detail: Option<String>,
    /// For people: how they can be reached.
    pub channels: Vec<Channel>,
    /// For events: when they start (ms since the epoch).
    #[ts(type = "number | null")]
    pub starts_at: Option<i64>,
}
