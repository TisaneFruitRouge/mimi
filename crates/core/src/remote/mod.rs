//! Using Mimi from a phone. Paired phones reach the daemon peer to peer with iroh: QUIC
//! dialled by public key, straight to this computer when the networks allow it, else
//! through a relay that forwards encrypted packets it can't read. There is no Mimi
//! server: the relay and address lookup are iroh's public ones (number 0's), or the
//! user's own relay (Settings › Phone). Design: `docs/architecture.md` › Using Mimi
//! from a phone.
//!
//! The endpoint runs only while a phone is paired or a pairing code is waiting. Every
//! connection proves the phone's key; every request also carries the token issued to
//! that key at pairing. Requests then go through the normal router (`wire.rs`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, EndpointId, RelayMode, RelayUrl, SecretKey, Watcher};
use mimi_protocol::{Device, DevicePath, Event, PairingOffer, Relay, RemoteAccess};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::Db;
use crate::fsutil::random_hex;
use crate::{AppState, now_ms};

pub mod store;
pub mod wire;

/// The application protocol phones speak (`wire.rs`).
pub const ALPN: &[u8] = b"mimi/1";
const SETTINGS_ROW: &str = "remote";
/// Long enough to find the phone and open the camera.
const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);

/// Who sent a request that came in over iroh. Only `wire.rs` inserts it, so a local
/// request can never claim to be a phone, and a phone never gets local credentials.
#[derive(Debug, Clone)]
pub struct Peer {
    pub endpoint_id: String,
}

/// What's kept in the `settings` table (encrypted with the rest of the database).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    /// This computer's iroh key, hex. Its public half is what phones dial.
    secret_key: Option<String>,
    #[serde(default)]
    relay: Relay,
}

#[derive(Default)]
pub struct Remote {
    running: tokio::sync::Mutex<Option<Running>>,
    /// Unredeemed pairing codes, in memory only.
    codes: Mutex<HashMap<String, Instant>>,
    /// Open connections by phone key, to report how they're connected and to cut a
    /// removed phone off at once.
    live: Mutex<HashMap<EndpointId, Vec<Connection>>>,
    error: Mutex<Option<String>>,
    /// Tests: no relay, no address lookup, nothing leaves the machine.
    local_only: AtomicBool,
}

struct Running {
    endpoint: Endpoint,
    relay: Relay,
    accept: tokio::task::JoinHandle<()>,
}

impl Remote {
    #[cfg(test)]
    pub fn local_only_for_tests(&self) {
        self.local_only.store(true, Ordering::Relaxed);
    }

    fn pairing_open(&self) -> bool {
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        codes.retain(|_, expires| *expires > now);
        !codes.is_empty()
    }

    /// Consumes a pairing code. True if it existed and hadn't expired.
    pub fn redeem(&self, code: &str) -> bool {
        let mut codes = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        codes
            .remove(code)
            .is_some_and(|expires| Instant::now() < expires)
    }

    fn set_error(&self, error: Option<String>) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = error;
    }

    /// The endpoint, for tests that dial it.
    #[cfg(test)]
    pub async fn endpoint(&self) -> Option<Endpoint> {
        self.running
            .lock()
            .await
            .as_ref()
            .map(|r| r.endpoint.clone())
    }
}

/// Starts phone access at launch if a phone is paired.
pub fn install(state: &Arc<AppState>) {
    let state = state.clone();
    tokio::spawn(async move { sync(&state).await });
}

/// Starts or stops the endpoint to match what's needed: running while a phone is paired
/// or a code is waiting, with the chosen relay.
pub async fn sync(state: &Arc<AppState>) {
    let wanted = state.remote.pairing_open() || store::any(&state.db).await.unwrap_or(false);
    let config = load(&state.db).await;
    let mut running = state.remote.running.lock().await;
    let current = running.as_ref().map(|r| r.relay.clone());
    if wanted && current.as_ref() == Some(&config.relay) {
        return;
    }
    if let Some(old) = running.take() {
        stop_running(state, old).await;
    }
    if wanted {
        match start(state, config).await {
            Ok(r) => {
                state.remote.set_error(None);
                *running = Some(r);
            }
            Err(e) => {
                tracing::warn!("phone access couldn't start: {e:#}");
                state.remote.set_error(Some(
                    "Phones can't reach this computer right now. Check that it's online, \
                     then try again."
                        .to_owned(),
                ));
            }
        }
    }
    drop(running);
    publish(state).await;
}

/// Closes the endpoint at shutdown.
pub async fn stop(state: &Arc<AppState>) {
    if let Some(r) = state.remote.running.lock().await.take() {
        stop_running(state, r).await;
    }
}

async fn stop_running(state: &AppState, running: Running) {
    running.accept.abort();
    running.endpoint.close().await;
    state
        .remote
        .live
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

async fn start(state: &Arc<AppState>, mut config: Config) -> anyhow::Result<Running> {
    let key = match config.secret_key.as_deref().and_then(parse_secret) {
        Some(key) => key,
        None => {
            let key = SecretKey::generate();
            config.secret_key = Some(hex(&key.to_bytes()));
            save(&state.db, &config).await?;
            key
        }
    };
    let builder = if state.remote.local_only.load(Ordering::Relaxed) {
        Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled)
    } else {
        match &config.relay {
            Relay::Default => Endpoint::builder(presets::N0),
            // The user's own relay: nothing published to number 0's address lookup.
            Relay::Custom { url } => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::custom([url.parse::<RelayUrl>()?])),
        }
    };
    let endpoint = builder
        .secret_key(key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;
    tracing::info!(endpoint = %endpoint.id().fmt_short(), "phone access started");
    let accept = tokio::spawn(accept_loop(state.clone(), endpoint.clone()));

    // Report when the relay connects, so Settings can say phones can reach us from afar.
    {
        let state = Arc::downgrade(state);
        let mut relays = endpoint.home_relay_status().stream();
        tokio::spawn(async move {
            while relays.next().await.is_some() {
                let Some(state) = state.upgrade() else { return };
                publish(&state).await;
            }
        });
    }
    Ok(Running {
        endpoint,
        relay: config.relay,
        accept,
    })
}

async fn accept_loop(state: Arc<AppState>, endpoint: Endpoint) {
    let router = crate::api::router(state.clone());
    while let Some(incoming) = endpoint.accept().await {
        let state = state.clone();
        let router = router.clone();
        tokio::spawn(async move {
            let Ok(conn) = incoming.await else { return };
            handle(state, router, conn).await;
        });
    }
}

async fn handle(state: Arc<AppState>, router: axum::Router, conn: Connection) {
    let remote_id = conn.remote_id();
    let peer = Peer {
        endpoint_id: remote_id.to_string(),
    };
    let paired = store::is_paired(&state.db, &peer.endpoint_id)
        .await
        .unwrap_or(false);
    // Strangers only get in to pair, and only while a code is waiting.
    if !paired && !state.remote.pairing_open() {
        conn.close(1u32.into(), b"not paired");
        return;
    }
    state
        .remote
        .live
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(remote_id)
        .or_default()
        .push(conn.clone());
    let _ = store::touch(&state.db, &peer.endpoint_id).await;
    publish(&state).await;

    // Say when it switches between the relay and a direct path.
    let watcher = {
        let state = Arc::downgrade(&state);
        let conn = conn.clone();
        tokio::spawn(async move {
            let mut paths = conn.paths_stream();
            let mut last = None;
            while let Some(list) = paths.next().await {
                let now = list.iter().find(|p| p.is_selected()).map(|p| p.is_ip());
                if now != last {
                    last = now;
                    let Some(state) = state.upgrade() else { return };
                    publish(&state).await;
                }
            }
        })
    };

    while let Ok((send, recv)) = conn.accept_bi().await {
        tokio::spawn(wire::serve(router.clone(), peer.clone(), send, recv));
    }
    watcher.abort();

    {
        let mut live = state.remote.live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(conns) = live.get_mut(&remote_id) {
            conns.retain(|c| c.stable_id() != conn.stable_id());
            if conns.is_empty() {
                live.remove(&remote_id);
            }
        }
    }
    let _ = store::touch(&state.db, &peer.endpoint_id).await;
    publish(&state).await;
}

/// A fresh pairing code (starting phone access if needed). Older codes stay valid until
/// they expire or are used.
pub async fn offer(state: &Arc<AppState>) -> anyhow::Result<PairingOffer> {
    let code = random_hex(16)?;
    let expires = Instant::now() + PAIRING_TTL;
    state
        .remote
        .codes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(code.clone(), expires);
    sync(state).await;
    let (endpoint_id, relay) = {
        let running = state.remote.running.lock().await;
        let Some(r) = running.as_ref() else {
            anyhow::bail!("phone access isn't running");
        };
        (r.endpoint.id().to_string(), r.relay.clone())
    };
    // Let it lapse on its own if nobody scans it.
    {
        let state = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(PAIRING_TTL + Duration::from_secs(1)).await;
            sync(&state).await;
        });
    }
    let link = pairing_link(&endpoint_id, &code, &relay);
    Ok(PairingOffer {
        qr_svg: qr_svg(&link)?,
        link,
        expires_at: now_ms() + PAIRING_TTL.as_millis() as i64,
    })
}

/// Withdraws every waiting code (the user closed the pairing sheet).
pub async fn cancel_offers(state: &Arc<AppState>) {
    state
        .remote
        .codes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    sync(state).await;
}

pub fn pairing_link(endpoint_id: &str, code: &str, relay: &Relay) -> String {
    let mut url = url::Url::parse("mimi://pair").expect("valid");
    url.query_pairs_mut()
        .append_pair("id", endpoint_id)
        .append_pair("code", code);
    if let Relay::Custom { url: relay } = relay {
        url.query_pairs_mut().append_pair("relay", relay);
    }
    url.into()
}

fn qr_svg(link: &str) -> anyhow::Result<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::with_error_correction_level(link, qrcode::EcLevel::M)?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .dark_color(svg::Color("#1d1d1f"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

/// Forgets a phone and cuts its connections.
pub async fn remove(state: &Arc<AppState>, id: Uuid) -> anyhow::Result<bool> {
    let Some(endpoint_id) = store::delete(&state.db, id).await? else {
        return Ok(false);
    };
    tracing::info!(%id, "phone removed");
    let conns = endpoint_id
        .parse::<EndpointId>()
        .ok()
        .and_then(|key| {
            state
                .remote
                .live
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key)
        })
        .unwrap_or_default();
    publish(state).await;
    // Its token already fails. The moment's grace lets a phone that removed itself hear
    // that it worked before it's disconnected (and phone access stops, if it was the last).
    let state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        for conn in conns {
            conn.close(2u32.into(), b"removed");
        }
        sync(&state).await;
    });
    Ok(true)
}

/// Changes the relay; the endpoint restarts with it if it's running.
pub async fn set_relay(state: &Arc<AppState>, relay: Relay) -> anyhow::Result<()> {
    let mut config = load(&state.db).await;
    config.relay = relay;
    save(&state.db, &config).await?;
    sync(state).await;
    Ok(())
}

/// Checks a relay address the user typed.
pub fn check_relay(relay: &Relay) -> Result<Relay, String> {
    match relay {
        Relay::Default => Ok(Relay::Default),
        Relay::Custom { url } => {
            let url = url.trim();
            let parsed = url::Url::parse(url)
                .map_err(|_| "That doesn't look like a web address.".to_owned())?;
            let local = parsed
                .host_str()
                .is_some_and(|h| h == "localhost" || h == "127.0.0.1" || h == "[::1]");
            if parsed.scheme() != "https" && !(parsed.scheme() == "http" && local) {
                return Err("The relay's address must start with https://.".to_owned());
            }
            url.parse::<RelayUrl>()
                .map_err(|_| "That doesn't look like a relay address.".to_owned())?;
            Ok(Relay::Custom {
                url: url.to_owned(),
            })
        }
    }
}

pub async fn status(state: &AppState, this_device: Option<Uuid>) -> RemoteAccess {
    let config = load(&state.db).await;
    let running = state.remote.running.lock().await;
    let (endpoint_id, relay_connected) = match running.as_ref() {
        Some(r) => (
            Some(r.endpoint.id().to_string()),
            r.endpoint
                .home_relay_status()
                .get()
                .iter()
                .any(|s| s.is_connected()),
        ),
        None => (
            config
                .secret_key
                .as_deref()
                .and_then(parse_secret)
                .map(|k| k.public().to_string()),
            false,
        ),
    };
    let is_running = running.is_some();
    drop(running);
    let paths: HashMap<String, DevicePath> = {
        let live = state.remote.live.lock().unwrap_or_else(|e| e.into_inner());
        live.iter()
            .filter_map(|(key, conns)| {
                let direct = conns
                    .iter()
                    .any(|c| c.paths().iter().any(|p| p.is_selected() && p.is_ip()));
                (!conns.is_empty()).then(|| {
                    (
                        key.to_string(),
                        if direct {
                            DevicePath::Direct
                        } else {
                            DevicePath::Relayed
                        },
                    )
                })
            })
            .collect()
    };
    let devices = store::list(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|d| Device {
            connection: paths.get(&d.endpoint_id).copied(),
            this_device: Some(d.id) == this_device,
            id: d.id,
            name: d.name,
            created_at: d.created_at,
            last_seen_at: d.last_seen_at,
        })
        .collect();
    RemoteAccess {
        running: is_running,
        endpoint_id,
        relay: config.relay,
        relay_connected,
        devices,
        error: state
            .remote
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
    }
}

pub async fn publish(state: &AppState) {
    let remote = status(state, None).await;
    state.events.publish(Event::RemoteChanged { remote });
}

async fn load(db: &Db) -> Config {
    db.call(|c| {
        c.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [SETTINGS_ROW],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .ok()
    .flatten()
    .and_then(|raw| serde_json::from_str(&raw).ok())
    .unwrap_or_default()
}

async fn save(db: &Db, config: &Config) -> anyhow::Result<()> {
    let raw = serde_json::to_string(config)?;
    db.call(move |c| {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            (SETTINGS_ROW, raw),
        )
    })
    .await?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_secret(hex: &str) -> Option<SecretKey> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(SecretKey::from_bytes(&bytes))
}

#[cfg(test)]
mod tests;
