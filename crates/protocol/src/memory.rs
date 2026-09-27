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
    /// The person in the people directory this note is about, by id, if known.
    pub subject: Option<String>,
    /// That person's name, for showing the link.
    pub subject_name: Option<String>,
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
    /// The person this note is about, if it's linked to one.
    pub subject: Option<String>,
    pub subject_name: Option<String>,
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
    /// Finding memories by meaning.
    pub semantic: MemorySemantic,
}

/// Whether memories are found by meaning as well as by their words, and what that
/// needs. The language file behind it is a small embedding model that runs on this
/// computer (or in the user's Ollama); nothing leaves the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemorySemantic {
    /// The user's choice.
    pub enabled: bool,
    /// Whether this computer can do it: Mimi's built-in model runtime, or Ollama.
    pub available: bool,
    /// Whether the language file is on this computer.
    pub installed: bool,
    /// Size of the one-time download.
    #[ts(type = "number")]
    pub download_bytes: u64,
    /// The download to watch in `ModelPull` events, as source and model id.
    pub provider_id: Option<uuid::Uuid>,
    pub model: String,
    /// Notes understood so far, out of all notes.
    pub indexed: u32,
    pub total: u32,
    /// What went wrong last, in plain language.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MemorySemanticEdit {
    pub enabled: bool,
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
