use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Who last wrote a memory note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MemorySource {
    /// The user, in the Memory screen.
    You,
    /// The assistant, during a conversation (it shows in the chat).
    Assistant,
    /// Learned in the background from past conversations.
    Learned,
}

/// One note in the memory library, e.g. `people/sam.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryNote {
    /// Where it lives: `folder/name.md`, lowercase.
    pub path: String,
    pub title: String,
    /// Markdown, usually a short list of facts.
    pub body: String,
    /// Optional link to something the note is about, e.g. a contact's id. Unused for now.
    pub subject: Option<String>,
    pub source: MemorySource,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// A note without its body, for listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryNoteSummary {
    pub path: String,
    pub title: String,
    /// The first line or so of the body.
    pub preview: String,
    pub source: MemorySource,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// Everything the Memory screen shows.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryOverview {
    /// The short summary that's always given to the assistant.
    pub profile: String,
    /// Most characters the profile may hold.
    pub profile_limit: u32,
    pub notes: Vec<MemoryNoteSummary>,
    /// Whether the assistant learns from conversations. Paused means nothing new is
    /// remembered; what's known is still used.
    pub learning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryNoteEdit {
    #[serde(default)]
    pub title: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryProfileEdit {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemoryLearning {
    pub learning: bool,
}
