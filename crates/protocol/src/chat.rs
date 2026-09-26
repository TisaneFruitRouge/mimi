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
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SendMessageResult {
    pub user_message: Message,
    /// Starts out `streaming`.
    pub assistant_message: Message,
}
