use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::{Locality, ModelRef};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Conversation {
    pub id: Uuid,
    pub title: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MessageStatus {
    Complete,
    /// The assistant is still writing it; `message_delta` events carry the new text.
    Streaming,
    /// Generation failed; `error` says why. Partial content is kept.
    Error,
    /// The user stopped it.
    Cancelled,
    /// The daemon stopped while it was being written.
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Message {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: MessageRole,
    pub content: String,
    /// A reasoning model's thinking, kept apart from the answer. Empty if none.
    pub reasoning: String,
    pub status: MessageStatus,
    /// The model that wrote an assistant message.
    pub model: Option<ModelRef>,
    /// Where that model ran, as it was when the message was written.
    pub locality: Option<Locality>,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    /// Tools the assistant used, or asked permission to use, while writing this reply.
    #[serde(default)]
    pub actions: Vec<Action>,
    /// People and events the user tagged with @ in this message.
    #[serde(default)]
    pub mentions: Vec<crate::Mention>,
    /// Photos the user sent with this message (only user messages have them).
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// The model this message went to couldn't see its photos, so it was only told that
    /// the user sent some. Clients say so under the message.
    #[serde(default)]
    pub attachments_unseen: bool,
}

/// What kind of file is attached to a message. Only pictures for now; voice notes and
/// videos will be kinds of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AttachmentKind {
    Image,
}

/// A file the user sent with a message, as the daemon keeps it: pictures are decoded,
/// shrunk, stripped of their metadata (location, camera…) and saved again as JPEG or
/// PNG. Its content is served at `GET /v1/attachments/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Attachment {
    pub id: Uuid,
    pub kind: AttachmentKind,
    /// The content type as kept: `image/jpeg` or `image/png`.
    pub mime: String,
    /// The name it came with, or a plain one ("Photo 1.jpg").
    pub name: String,
    /// Bytes, as kept.
    #[ts(type = "number")]
    pub size: u64,
    /// Pixels, for pictures.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// A file sent with a new message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewAttachment {
    /// The file's content, base64-encoded (standard alphabet, padding optional).
    pub data: String,
    /// Its file name, if it has one.
    #[serde(default)]
    pub name: Option<String>,
    /// The content type the client believes it has. Only a hint: the daemon reads the
    /// file itself and refuses what isn't a picture.
    #[serde(default)]
    pub mime: Option<String>,
}

/// Whether a model can see the photos sent to it (`GET /v1/models/vision`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VisionSupport {
    /// False also when it can't be told (the model source doesn't say).
    pub sees_images: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ActionStatus {
    /// Waiting for the user to approve or decline it.
    PendingApproval,
    /// Approved; about to run.
    Approved,
    /// The user declined it. It did not run.
    Rejected,
    Running,
    Done,
    /// It failed, or never ran (stopped, interrupted). `error` says why.
    Failed,
}

/// One use of a tool by the assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Action {
    pub id: Uuid,
    /// The tool's name, e.g. `calendar_create_event`.
    pub tool: String,
    /// One plain-language line saying what it does, for the approval card.
    pub summary: String,
    /// The arguments the model chose (after any edit by the user).
    #[ts(type = "Record<string, unknown>")]
    pub arguments: serde_json::Value,
    /// Anything that sends, changes or deletes on the user's behalf needs approval.
    pub requires_approval: bool,
    pub status: ActionStatus,
    /// Short past-tense description once done, e.g. "read calendar".
    pub result: Option<String>,
    pub error: Option<String>,
    /// What the tool returned, as given back to the model.
    #[ts(type = "unknown")]
    pub output: Option<serde_json::Value>,
    /// The model's id for this tool call, needed to replay the conversation to it.
    pub call_id: String,
    /// Which model round of the reply asked for it (0-based).
    pub round: u32,
    /// How much of the reply's text, in Unicode characters, came before this action,
    /// so clients can show it in place.
    #[serde(default)]
    pub content_offset: u32,
    /// While it waits for approval: the text of a second choice that also stops asking
    /// for this person or calendar from now on (e.g. "Don't ask again for Sam"), when
    /// that would make a difference.
    #[serde(default)]
    pub always_allow: Option<String>,
}

/// Body of `POST /v1/actions/{id}/approve`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ApproveAction {
    /// Replaces the model's arguments, when the user edited them.
    #[ts(type = "Record<string, unknown> | null")]
    pub arguments: Option<serde_json::Value>,
    /// The user chose the card's `always_allow`: approve, and add the matching exception
    /// in Settings › Permissions. Not with edited arguments.
    pub always: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct NewConversation {
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConversationUpdate {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SendMessage {
    pub content: String,
    /// Overrides the default model for this reply.
    #[serde(default)]
    pub model: Option<ModelRef>,
    /// People and events tagged with @, email tagged with #. Each label appears in
    /// `content` as `@label` (or `#label` for email).
    #[serde(default)]
    pub mentions: Vec<crate::Mention>,
    /// Photos sent with the message (at most 10). With photos, `content` may be empty.
    #[serde(default)]
    pub attachments: Vec<NewAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SendMessageResult {
    pub user_message: Message,
    /// Starts out `streaming`.
    pub assistant_message: Message,
}
