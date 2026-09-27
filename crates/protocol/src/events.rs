use serde::{Deserialize, Serialize};
use ts_rs::TS;

use uuid::Uuid;

use crate::{
    Connection, Conversation, Delivery, Message, ModelPull, Provider, RuntimeStatus, Settings,
};

/// Pushed by the daemon to every client connected to `GET /v1/events` (WebSocket, one
/// JSON event per text frame).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    SettingsChanged {
        settings: Settings,
    },
    ProvidersChanged {
        providers: Vec<Provider>,
    },
    /// A conversation was created or changed (title, last activity).
    ConversationUpdated {
        conversation: Conversation,
    },
    ConversationDeleted {
        id: Uuid,
    },
    /// A message was created or changed state (e.g. finished streaming).
    MessageUpdated {
        message: Message,
    },
    /// New text for a streaming message, to append to what the client has.
    MessageDelta {
        conversation_id: Uuid,
        message_id: Uuid,
        content: String,
        reasoning: String,
    },
    /// The people directory changed (sync, edit, merge). Refetch what you show.
    PeopleChanged,
    /// Mail changed (new mail, sorting, read state). Refetch what you show.
    MailChanged,
    ConnectionsChanged {
        connections: Vec<Connection>,
    },
    /// A model download started, progressed or finished.
    ModelPull {
        pull: ModelPull,
    },
    /// The built-in runtime started or stopped a model, or failed to.
    RuntimeChanged {
        runtime: RuntimeStatus,
    },
    /// Something in the memory changed: refetch the Memory screen.
    MemoryChanged,
    /// Reminders or routines were added, changed, removed, or ran.
    ScheduleChanged,
    /// A reminder went off, or a routine finished: show it now.
    ScheduleDelivered {
        delivery: Delivery,
    },
    /// This client fell behind and missed events. Refetch any state you display.
    Resync,
}
