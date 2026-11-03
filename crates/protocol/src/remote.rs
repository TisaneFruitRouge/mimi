use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Using Mimi from a phone (Settings › Phone): the phones paired with this computer and
/// how they reach it. Phones connect peer to peer (iroh), straight to this computer when
/// they can, else through a relay that passes on encrypted traffic it can't read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemoteAccess {
    /// True while phones can reach this computer: once one is paired, or while a pairing
    /// code is waiting to be scanned.
    pub running: bool,
    /// This computer's address for phones, which is also its public key: a phone checks
    /// it's talking to this computer and nothing else. Absent until first started.
    pub endpoint_id: Option<String>,
    pub relay: Relay,
    /// Whether this computer is reachable through its relay right now, so phones on
    /// other networks can find it.
    pub relay_connected: bool,
    pub devices: Vec<Device>,
    /// Why phones can't reach this computer, in plain words.
    pub error: Option<String>,
}

/// Which relay helps phones reach this computer when a direct connection isn't possible.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Relay {
    /// The public relays of iroh's makers (number 0), and their address lookup.
    #[default]
    Default,
    /// The user's own relay (`iroh-relay`). Nothing is published anywhere else: phones
    /// learn its address when they pair.
    Custom { url: String },
}

/// A paired phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Device {
    pub id: Uuid,
    /// What the phone calls itself, e.g. "Vincent's iPhone".
    pub name: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub last_seen_at: Option<i64>,
    /// How it's connected right now; absent while it isn't.
    pub connection: Option<DevicePath>,
    /// True for the phone making this request.
    #[serde(default)]
    pub this_device: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DevicePath {
    /// Straight to this computer.
    Direct,
    /// Through the relay (still end-to-end encrypted).
    Relayed,
}

/// A one-time pairing code, shown as a QR code for the phone to scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairingOffer {
    /// `mimi://pair?id=<endpoint id>&code=<code>[&relay=<url>]`: what the QR code holds.
    pub link: String,
    /// The QR code, as an SVG document.
    pub qr_svg: String,
    #[ts(type = "number")]
    pub expires_at: i64,
}

/// Sent by a phone over its first connection, with the code from the QR code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairRequest {
    pub code: String,
    pub name: String,
}

/// The phone's credentials: keep the token in the Keychain and send it as
/// `Authorization: Bearer <token>` on every request over this connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Paired {
    pub device: Device,
    pub token: String,
}

/// `PATCH /remote/devices/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeviceUpdate {
    pub name: String,
}
