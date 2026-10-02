use serde::{Deserialize, Serialize};
use ts_rs::TS;

use uuid::Uuid;

use crate::{
    Connection, Conversation, Delivery, GuestApproval, Message, ModelPull, Provider, RuntimeStatus,
    Settings, UpdateStatus, VoiceStatus,
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
    /// A check for new versions finished (or its setting changed).
    UpdateChanged {
        update: UpdateStatus,
    },
    /// Something in the memory changed: refetch the Memory screen.
    MemoryChanged,
    /// Reminders or routines were added, changed, removed, or ran.
    ScheduleChanged,
    /// A reminder went off, or a routine finished: show it now.
    ScheduleDelivered {
        delivery: Delivery,
    },
    /// What someone in People may ask the assistant changed: refetch their access.
    PersonAccessChanged {
        person_id: Uuid,
    },
    /// Something a trusted person asked for waits for the user's OK, or was settled
    /// (then `action.status` isn't `pending_approval` anymore). Never their conversation.
    GuestApproval {
        approval: GuestApproval,
    },
    /// A voice download started, progressed or finished, or voice settings changed.
    VoiceChanged {
        voice: VoiceStatus,
    },
    /// This client fell behind and missed events. Refetch any state you display.
    Resync,
    /// The user clicked a new-mail notification on this computer: the desktop app comes
    /// forward and opens Mail, on that conversation when there's one (`None`: several).
    /// Browsers ignore it.
    OpenMail {
        #[ts(type = "number | null")]
        thread_id: Option<i64>,
    },
}
