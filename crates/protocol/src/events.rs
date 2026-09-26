use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Provider, Settings};

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
    /// This client fell behind and missed events. Refetch any state you display.
    Resync,
}
