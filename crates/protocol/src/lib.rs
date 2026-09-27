//! Types shared between the mimi daemon and every client (desktop app, CLI, TUI).
//!
//! Anything that crosses the daemon's API boundary lives here, so clients can't drift
//! from what the daemon actually serves. Types deriving `TS` are exported to
//! `apps/desktop/src/bindings` by `cargo test -p mimi-protocol`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub mod calendar;
pub mod chat;
pub mod connections;
pub mod events;
pub mod hardware;
pub mod integrations;
pub mod memory;
pub mod paths;
pub mod people;
pub mod providers;
pub mod schedule;
pub mod settings;

pub use calendar::*;
pub use chat::*;
pub use connections::*;
pub use events::Event;
pub use hardware::*;
pub use integrations::*;
pub use memory::*;
pub use paths::Paths;
pub use people::*;
pub use providers::*;
pub use schedule::*;
pub use settings::*;

/// Prefix for all versioned API routes.
pub const API_PREFIX: &str = "/v1";

/// Unauthenticated liveness check. Deliberately reveals nothing beyond the version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Health {
    pub version: String,
}

/// Authenticated daemon status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Status {
    pub version: String,
    pub pid: u32,
    #[ts(type = "number")]
    pub uptime_secs: u64,
    #[ts(type = "string")]
    pub data_dir: PathBuf,
    pub key_storage: KeyStorage,
}

/// Where the database encryption key is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum KeyStorage {
    /// The OS keychain (macOS Keychain, Secret Service on Linux).
    Keychain,
    /// A file in the data directory readable only by this user. Used when no keychain is
    /// available, e.g. on a headless server.
    File,
}

/// Body of every non-2xx API response.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApiError {
    /// Stable, machine-readable code, e.g. `not_found`.
    pub code: String,
    /// Human-readable explanation, suitable for showing to the user.
    pub message: String,
}

/// Written by the daemon at startup (mode 0600) so local clients can find it and
/// authenticate. Removed on clean shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    pub pid: u32,
    pub port: u16,
    pub token: String,
}

/// A single-use link that signs a browser in to the web interface.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WebLoginLink {
    pub url: String,
}
