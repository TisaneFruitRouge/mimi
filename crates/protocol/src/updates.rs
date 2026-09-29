use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Whether a newer version of Mimi has been published (Settings › General › "Check for new
/// versions", off by default). Checked by asking GitHub, where Mimi is published, directly
/// from this computer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct UpdateStatus {
    /// The version running now.
    pub current: String,
    /// The newest published version, when it's newer than `current`.
    pub available: Option<Release>,
    /// When GitHub last answered (ms since the epoch).
    #[ts(type = "number | null")]
    pub checked_at: Option<i64>,
    /// Why the last check failed, in plain words.
    pub error: Option<String>,
}

/// A published version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Release {
    /// e.g. "0.2.0".
    pub version: String,
    /// Its release page (what's new, and the downloads).
    pub url: String,
}
