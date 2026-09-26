//! Types shared between the hearth daemon and every client (desktop app, CLI, TUI).
//!
//! Anything that crosses the daemon's API boundary lives here, so clients can't drift
//! from what the daemon actually serves.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod paths;

pub use paths::Paths;

/// Prefix for all versioned API routes.
pub const API_PREFIX: &str = "/v1";

/// Unauthenticated liveness check. Deliberately reveals nothing beyond the version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub version: String,
}

/// Authenticated daemon status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub pid: u32,
    pub uptime_secs: u64,
    pub data_dir: PathBuf,
}

/// Written by the daemon at startup (mode 0600) so local clients can find it and
/// authenticate. Removed on clean shutdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    pub pid: u32,
    pub port: u16,
    pub token: String,
}
