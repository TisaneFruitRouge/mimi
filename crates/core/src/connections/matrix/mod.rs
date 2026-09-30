//! Matrix: an account the user creates for their assistant, on any server, becomes their
//! private line to it. Mimi signs in as that account (never the user's own), keeps its
//! end-to-end encryption keys in a store of its own, and syncs from the server, so
//! nothing has to reach this machine from outside. A one-time code sent from the user's
//! own account pairs it with its owner. Only the owner gives it instructions, in their
//! direct chat; it also stays in groups the owner invites it into and in chats it opens
//! to message people for the user (`send.rs`, `rooms.rs`), and leaves everything else.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::config::SyncSettings;
use matrix_sdk::encryption::EncryptionSettings;
use matrix_sdk::ruma::api::client::uiaa;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::events::relation::Thread;
use matrix_sdk::ruma::events::room::member::MembershipState;
use matrix_sdk::ruma::events::room::message::{MessageType, Relation, RoomMessageEventContent};
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent, AnySyncStateEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
    SyncStateEvent,
};
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk::{Client, Room, RoomMemberships, RoomState, SessionMeta, SessionTokens};
use mimi_protocol::{Action, ConnectionStatus, Delivery};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::store;
use crate::channels::replies::{self, Target};
use crate::channels::{self, Channel, Outgoing};
use crate::{AppState, now_ms};

#[cfg(test)]
pub mod fake;
pub mod format;
pub mod guests;
pub mod messenger;
pub mod rooms;
pub mod send;

use self::messenger::Messenger;
use self::rooms::Why;

pub const MATRIX: &str = "matrix";

/// What a Matrix connection keeps, in the encrypted database. Never log it.
#[derive(Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    /// The assistant's account, `@name:server`.
    pub user_id: String,
    /// Where its server answers, found at sign-in.
    pub homeserver: String,
    pub device_id: String,
    /// A credential.
    pub access_token: String,
    /// Unlocks the local store of encryption keys. A credential.
    pub store_passphrase: String,
    /// Waiting for the owner to send this code.
    pub pairing_code: Option<String>,
    /// The owner's Matrix address, once paired.
    pub owner: Option<String>,
    pub owner_name: Option<String>,
    /// The owner's direct chat with the assistant.
    pub room_id: Option<String>,
    /// Where messages from Matrix go.
    pub conversation_id: Option<Uuid>,
    /// Whether the owner's chat is end-to-end encrypted.
    #[serde(default)]
    pub encrypted: bool,
    /// Whether the assistant's device is signed by its own account (cross-signing), so
    /// the owner's app doesn't warn about an unverified device.
    #[serde(default)]
    pub cross_signed: bool,
    /// The server stopped accepting the access token.
    #[serde(default)]
    pub signed_out: bool,
    /// When the connection was made: older messages are history, not requests.
    pub since: i64,
}

/// A running connection: its client and a way back to the daemon's state.
pub struct Live {
    connection: Uuid,
    client: Client,
    state: Weak<AppState>,
}

/// Running Matrix clients, one per connection. A store can only be open once, so
/// everything that talks to Matrix for a connection goes through its client here.
#[derive(Default)]
pub struct Clients {
    live: Mutex<HashMap<Uuid, Arc<Live>>>,
    /// Clients signed in while connecting, waiting for their connection's task.
    handoff: Mutex<HashMap<Uuid, Client>>,
    /// Stand-ins for running clients, so tests can send without a homeserver.
    #[cfg(test)]
    fakes: Mutex<HashMap<Uuid, Arc<dyn Messenger>>>,
}

impl Clients {
    /// The account a connection sends from, while it's running.
    pub fn messenger(&self, id: Uuid) -> Option<Arc<dyn Messenger>> {
        #[cfg(test)]
        if let Some(fake) = self
            .fakes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            return Some(fake.clone());
        }
        self.get(id).map(|live| live as Arc<dyn Messenger>)
    }

    /// Makes a connection send through `fake` instead of a homeserver.
    #[cfg(test)]
    pub fn fake(&self, id: Uuid, fake: Arc<dyn Messenger>) {
        self.fakes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, fake);
    }

    fn get(&self, id: Uuid) -> Option<Arc<Live>> {
        self.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    fn insert(&self, live: Arc<Live>) {
        self.live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(live.connection, live);
    }

    /// Forgets a connection's client; with `only`, only if it's still that one.
    fn remove(&self, id: Uuid, only: Option<&Arc<Live>>) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if only.is_none_or(|l| live.get(&id).is_some_and(|current| Arc::ptr_eq(l, current))) {
            live.remove(&id);
        }
        drop(live);
        self.handoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    fn hand_over(&self, id: Uuid, client: Client) {
        self.handoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, client);
    }

    fn take_handoff(&self, id: Uuid) -> Option<Client> {
        self.handoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
    }
}

/// Where a connection keeps its encryption keys and sync state.
fn store_dir(state: &AppState, id: Uuid) -> PathBuf {
    state.paths.data_dir.join("matrix").join(id.to_string())
}

/// Where the account lives, as the user entered it.
#[derive(Debug, Clone, PartialEq)]
pub enum Server {
    /// A server name (`example.org`): its homeserver is found through `.well-known`.
    Name(String),
    Url(Url),
}

/// The account to sign in as.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    /// What to sign in with: the full address, or just the name.
    pub login: String,
    pub server: Server,
}

/// Reads what the user typed: `@name:server`, or a name plus the server's address.
pub fn parse_account(user: &str, homeserver: Option<&str>) -> Result<Account, String> {
    let user = user.trim();
    let (name, server_name) = match user.trim_start_matches('@').split_once(':') {
        Some((name, server)) => (name, Some(server.trim())),
        None => (user.trim_start_matches('@'), None),
    };
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(
            "Enter your assistant's Matrix address, like @my-assistant:matrix.org.".to_owned(),
        );
    }
    let homeserver = homeserver.map(str::trim).filter(|h| !h.is_empty());
    let server = match (homeserver, server_name) {
        (Some(h), _) if h.contains("://") => {
            let url = crate::providers::parse_base_url(h)?;
            if url.scheme() != "https"
                && crate::providers::locality_of(&url) == mimi_protocol::Locality::Cloud
            {
                return Err(
                    "Use an https:// address, so your password isn't sent unencrypted.".to_owned(),
                );
            }
            Server::Url(url)
        }
        (Some(h), _) => Server::Name(h.trim_end_matches('/').to_owned()),
        (None, Some(s)) if !s.is_empty() => Server::Name(s.to_owned()),
        _ => {
            return Err(
                "Enter the whole address, with its server, like @my-assistant:matrix.org."
                    .to_owned(),
            );
        }
    };
    let login = match server_name {
        Some(s) if !s.is_empty() => format!("@{name}:{s}"),
        _ => name.to_owned(),
    };
    Ok(Account { login, server })
}

/// A fresh random secret, for the store's passphrase.
fn secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn builder(dir: &std::path::Path, passphrase: &str) -> matrix_sdk::ClientBuilder {
    Client::builder()
        .sqlite_store(dir, Some(passphrase))
        .with_encryption_settings(EncryptionSettings {
            // Done by hand at sign-in, with the password.
            auto_enable_cross_signing: false,
            ..Default::default()
        })
}

/// A plain sentence for a failed request. Never includes addresses or tokens.
fn plain_error(e: &matrix_sdk::Error) -> String {
    match e.client_api_error_kind() {
        Some(ErrorKind::LimitExceeded(_)) => {
            "Your Matrix server asked to slow down. Try again in a minute.".to_owned()
        }
        Some(_) => "Your Matrix server refused the request.".to_owned(),
        None => "Couldn't reach your Matrix server.".to_owned(),
    }
}

/// A plain sentence for a refused sign-in.
fn login_error(e: &matrix_sdk::Error) -> String {
    match e.client_api_error_kind() {
        Some(ErrorKind::Forbidden) => {
            "Your Matrix server didn't accept that address and password.".to_owned()
        }
        Some(ErrorKind::UserDeactivated) => "That Matrix account was deactivated.".to_owned(),
        _ => plain_error(e),
    }
}

fn signed_out(e: &matrix_sdk::Error) -> bool {
    matches!(e.client_api_error_kind(), Some(ErrorKind::UnknownToken(_)))
}

/// Signs in as the assistant's account, sets up its encryption and returns the
/// connection's name and config. The client stays open for the connection's task.
pub async fn connect(
    state: &AppState,
    id: Uuid,
    user: &str,
    password: &str,
    homeserver: Option<&str>,
) -> Result<(String, MatrixConfig), String> {
    let account = parse_account(user, homeserver)?;
    if password.is_empty() {
        return Err("Enter the account's password.".to_owned());
    }
    let dir = store_dir(state, id);
    let passphrase = secret();
    let result = sign_in(state, id, &dir, &passphrase, &account, password).await;
    if result.is_err() {
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    result
}

async fn sign_in(
    state: &AppState,
    id: Uuid,
    dir: &std::path::Path,
    passphrase: &str,
    account: &Account,
    password: &str,
) -> Result<(String, MatrixConfig), String> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|_| "Couldn't make a place to keep the assistant's keys.".to_owned())?;
    let builder = builder(dir, passphrase);
    let builder = match &account.server {
        Server::Url(url) => builder.homeserver_url(url.as_str()),
        Server::Name(name) => builder.server_name_or_homeserver_url(name),
    };
    let client = builder.build().await.map_err(|_| match &account.server {
        Server::Name(name) => format!(
            "Couldn't find a Matrix server for {name}. Check the address, or enter the server's address under Server settings."
        ),
        Server::Url(_) => "Couldn't reach that Matrix server.".to_owned(),
    })?;
    let name = channels::assistant_name(state).await;
    let login = client
        .matrix_auth()
        .login_username(&account.login, password)
        .initial_device_display_name(&name)
        .send()
        .await
        .map_err(|e| login_error(&e))?;
    let user_id = login.user_id.to_string();

    let taken = store::list(&state.db)
        .await
        .map_err(|_| "Couldn't check your connections.".to_owned())?
        .into_iter()
        .any(|r| r.integration == MATRIX && r.name == user_id);
    if taken {
        let _ = client.logout().await;
        return Err("That Matrix account is already connected.".to_owned());
    }

    let cross_signed = cross_sign(&client, password).await;
    let config = MatrixConfig {
        user_id: user_id.clone(),
        homeserver: client.homeserver().to_string(),
        device_id: login.device_id.to_string(),
        access_token: login.access_token,
        store_passphrase: passphrase.to_owned(),
        pairing_code: Some(super::telegram::pairing_code()),
        owner: None,
        owner_name: None,
        room_id: None,
        conversation_id: None,
        encrypted: false,
        cross_signed,
        signed_out: false,
        // With an hour's grace, in case the server's clock is behind this computer's.
        since: now_ms() - 60 * 60 * 1000,
    };
    state.connections.matrix.hand_over(id, client);
    Ok((user_id, config))
}

/// Gives the account a cross-signing identity that signs this device, so the owner's
/// app trusts it without a warning. An identity made elsewhere (Element sets one up when
/// the account is created there) is replaced: the account belongs to the assistant.
/// Whether the device ended up signed.
async fn cross_sign(client: &Client, password: &str) -> bool {
    let encryption = client.encryption();
    let Some(user) = client.user_id().map(ToString::to_string) else {
        return false;
    };
    let auth = |session: Option<String>| {
        let mut p = uiaa::Password::new(
            uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user.clone())),
            password.to_owned(),
        );
        p.session = session;
        uiaa::AuthData::Password(p)
    };
    let done = |r: matrix_sdk::Result<()>| match r {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Matrix cross-signing setup failed: {}", plain_error(&e));
            false
        }
    };
    if let Err(e) = encryption.bootstrap_cross_signing_if_needed(None).await {
        match e.as_uiaa_response() {
            Some(info) => {
                let session = info.session.clone();
                done(
                    encryption
                        .bootstrap_cross_signing(Some(auth(session)))
                        .await,
                );
            }
            None => return done(Err(e)),
        }
    }
    if encryption
        .cross_signing_status()
        .await
        .is_some_and(|s| s.is_complete())
    {
        return true;
    }
    // The account already had an identity whose keys we don't hold: replace it.
    match encryption.bootstrap_cross_signing(None).await {
        Ok(()) => {}
        Err(e) => match e.as_uiaa_response() {
            Some(info) => {
                let session = info.session.clone();
                if !done(
                    encryption
                        .bootstrap_cross_signing(Some(auth(session)))
                        .await,
                ) {
                    return false;
                }
            }
            None => return done(Err(e)),
        },
    }
    encryption
        .cross_signing_status()
        .await
        .is_some_and(|s| s.is_complete())
}

/// The Matrix link that opens a chat with the assistant.
pub fn chat_link(config: &MatrixConfig) -> String {
    format!("https://matrix.to/#/{}", config.user_id)
}

/// The status line a Matrix connection shows.
pub fn describe(config: &MatrixConfig) -> (ConnectionStatus, String, Option<String>) {
    if config.signed_out {
        return (
            ConnectionStatus::Error,
            "Your Matrix server signed the assistant out. Remove this connection and connect it again.".to_owned(),
            None,
        );
    }
    let Some(owner) = &config.owner else {
        let code = config.pairing_code.as_deref().unwrap_or("the code");
        return (
            ConnectionStatus::NeedsAction,
            format!(
                "From your own Matrix account, start a chat with {} and send {code}",
                config.user_id
            ),
            Some(chat_link(config)),
        );
    };
    let who = config.owner_name.as_deref().unwrap_or(owner);
    if config.room_id.is_none() {
        return (
            ConnectionStatus::NeedsAction,
            format!(
                "{who} left the chat. Start a new one with {} to keep talking.",
                config.user_id
            ),
            Some(chat_link(config)),
        );
    }
    let privacy = match (config.encrypted, config.cross_signed) {
        (true, true) => "end-to-end encrypted",
        (true, false) => "end-to-end encrypted, from a device that isn't verified",
        (false, _) => "not encrypted: turn on encryption in the chat's settings",
    };
    (
        ConnectionStatus::Ok,
        format!("Talking with {who} as {}, {privacy}", config.user_id),
        None,
    )
}

/// Who a message is from, as far as the assistant is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    /// Not paired yet: anyone may be about to send the code.
    Claimant,
    /// The owner, in their chat with the assistant.
    Owner,
    /// The owner, in another room.
    OwnerElsewhere,
    Stranger,
}

pub fn classify(config: &MatrixConfig, room: &str, sender: &str) -> Sender {
    match &config.owner {
        None => Sender::Claimant,
        Some(owner) if owner != sender => Sender::Stranger,
        Some(_) if config.room_id.as_deref() == Some(room) => Sender::Owner,
        Some(_) => Sender::OwnerElsewhere,
    }
}

/// Whether a message is the pairing code.
pub fn is_pairing_code(config: &MatrixConfig, text: &str) -> bool {
    config
        .pairing_code
        .as_deref()
        .is_some_and(|code| text.trim() == code)
}

/// Whether a message from the owner can be believed. Once their chat is encrypted, only
/// encrypted messages count: anything else could have been made up by a server along
/// the way. (Reactions are the exception: Matrix apps send them unencrypted.)
pub fn trusted(config: &MatrixConfig, sealed: bool) -> bool {
    sealed || !config.encrypted
}

/// What to do with an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invitation {
    Decline,
    /// Join: someone may be about to pair, or the owner opens a new chat after leaving
    /// theirs.
    Join,
    /// The owner invites the assistant into a group: join and stay.
    KeepGroup,
    /// Someone the owner lets ask it things (`access`) opens a direct chat with it:
    /// join and keep it as their chat.
    KeepDirect,
}

/// Anyone's invitation is accepted until paired (the code still has to come); then only
/// the owner's: to a group, or to a new direct chat after they left theirs. Everyone
/// else's is declined.
pub fn answer_invite(config: &MatrixConfig, inviter: Option<&str>, direct: bool) -> Invitation {
    answer_invite_from(config, inviter, direct, false)
}

/// [`answer_invite`], knowing whether the invitation is a direct chat from someone the
/// owner lets ask the assistant things (`trusted_direct`: the inviter is trusted and
/// it's just the two of them): it's joined and kept as their chat. Their groups aren't:
/// only the owner brings the assistant into groups.
pub fn answer_invite_from(
    config: &MatrixConfig,
    inviter: Option<&str>,
    direct: bool,
    trusted_direct: bool,
) -> Invitation {
    if trusted_direct && config.owner.is_some() && inviter != config.owner.as_deref() {
        return Invitation::KeepDirect;
    }
    match &config.owner {
        None => Invitation::Join,
        Some(owner) if inviter != Some(owner.as_str()) => Invitation::Decline,
        Some(_) if !direct => Invitation::KeepGroup,
        Some(_) if config.room_id.is_none() => Invitation::Join,
        Some(_) => Invitation::Decline,
    }
}

/// Something that happened in a sync, in the order it came.
#[derive(Debug)]
enum Incoming {
    Invite {
        room: OwnedRoomId,
    },
    /// Activity in a room the assistant is in.
    Joined {
        room: OwnedRoomId,
    },
    Text {
        room: OwnedRoomId,
        sender: OwnedUserId,
        text: String,
        /// The message it replies to.
        quoted: Option<OwnedEventId>,
        /// Whether it came end-to-end encrypted.
        sealed: bool,
    },
    Reaction {
        room: OwnedRoomId,
        sender: OwnedUserId,
        target: OwnedEventId,
        key: String,
    },
    /// An encrypted message the assistant couldn't read.
    Unreadable {
        room: OwnedRoomId,
        sender: OwnedUserId,
    },
    Left {
        room: OwnedRoomId,
        user: OwnedUserId,
    },
    /// The assistant is no longer in a room (it left, or was removed).
    Gone {
        room: OwnedRoomId,
    },
}

/// How a connection's sync ended.
enum Ended {
    Cancelled,
    SignedOut,
    Failed,
}

/// Syncs the account until cancelled or signed out, reconnecting with backoff.
pub async fn run(state: Arc<AppState>, id: Uuid, cancel: CancellationToken) {
    let mut backoff = Duration::from_secs(2);
    // One client for the connection's whole life: its store must never be open twice,
    // and replies still being written keep using it.
    let client = loop {
        let Some(config) = load(&state, id).await else {
            return;
        };
        if config.signed_out {
            return;
        }
        let opened = match state.connections.matrix.take_handoff(id) {
            Some(client) => Ok(client),
            None => open(&state, id, &config).await,
        };
        match opened {
            Ok(client) => break client,
            Err(e) if signed_out(&e) => return mark_signed_out(&state, id).await,
            Err(_) => {
                if !retry_later(&state, id, &cancel, &mut backoff).await {
                    return;
                }
            }
        }
    };
    let live = Arc::new(Live {
        connection: id,
        client,
        state: Arc::downgrade(&state),
    });
    state.connections.matrix.insert(live.clone());
    loop {
        // The handler saved whatever changed during the last sync.
        let Some(config) = load(&state, id).await else {
            break;
        };
        match sync(&state, &live, config, &cancel, &mut backoff).await {
            Ended::Cancelled => break,
            Ended::SignedOut => {
                mark_signed_out(&state, id).await;
                break;
            }
            Ended::Failed => {
                if !retry_later(&state, id, &cancel, &mut backoff).await {
                    break;
                }
            }
        }
    }
    state.connections.matrix.remove(id, Some(&live));
}

async fn load(state: &AppState, id: Uuid) -> Option<MatrixConfig> {
    let row = store::get(&state.db, id).await.ok()??;
    serde_json::from_value(row.config).ok()
}

/// Says the server is unreachable and waits before the next try. False if cancelled.
async fn retry_later(
    state: &AppState,
    id: Uuid,
    cancel: &CancellationToken,
    backoff: &mut Duration,
) -> bool {
    state
        .connections
        .set_status(
            state,
            id,
            ConnectionStatus::Error,
            "Can't reach your Matrix server right now. Retrying…".to_owned(),
            None,
        )
        .await;
    tokio::select! {
        _ = tokio::time::sleep(*backoff) => {},
        _ = cancel.cancelled() => return false,
    }
    *backoff = (*backoff * 2).min(Duration::from_secs(300));
    true
}

/// The server stopped accepting the access token: say so, for good.
async fn mark_signed_out(state: &AppState, id: Uuid) {
    if let Ok(Some(mut row)) = store::get(&state.db, id).await
        && let Ok(mut config) = serde_json::from_value::<MatrixConfig>(row.config)
    {
        config.signed_out = true;
        let (status, detail, url) = describe(&config);
        row.config = serde_json::to_value(&config).expect("config serializes");
        let _ = store::upsert(&state.db, row).await;
        state
            .connections
            .set_status(state, id, status, detail, url)
            .await;
    }
    tracing::info!(connection = %id, "Matrix server stopped accepting the assistant's access");
}

/// Opens the connection's client from its store and session.
async fn open(state: &AppState, id: Uuid, config: &MatrixConfig) -> matrix_sdk::Result<Client> {
    let dir = store_dir(state, id);
    let client = builder(&dir, &config.store_passphrase)
        .homeserver_url(&config.homeserver)
        .build()
        .await
        .map_err(|e| matrix_sdk::Error::UnknownError(Box::new(e)))?;
    let session = MatrixSession {
        meta: SessionMeta {
            user_id: config
                .user_id
                .as_str()
                .try_into()
                .map_err(|e| matrix_sdk::Error::UnknownError(Box::new(e)))?,
            device_id: config.device_id.as_str().into(),
        },
        tokens: SessionTokens {
            access_token: config.access_token.clone(),
            refresh_token: None,
        },
    };
    client.restore_session(session).await?;
    Ok(client)
}

/// Syncs until something ends it. Events go to a handler task in order, so handling
/// one (turning on encryption waits for the next sync) never holds up syncing.
async fn sync(
    state: &Arc<AppState>,
    live: &Arc<Live>,
    config: MatrixConfig,
    cancel: &CancellationToken,
    backoff: &mut Duration,
) -> Ended {
    let (tx, rx) = mpsc::unbounded_channel();
    let since = config.since;
    let handler = tokio::spawn(handle_all(state.clone(), live.clone(), config, rx));
    let client = &live.client;
    let mut first = true;
    let ended = loop {
        // The first sync returns at once; later ones wait for news.
        let settings = if first {
            SyncSettings::default().timeout(Duration::ZERO)
        } else {
            SyncSettings::default().timeout(Duration::from_secs(30))
        };
        let response = tokio::select! {
            r = client.sync_once(settings) => r,
            _ = cancel.cancelled() => break Ended::Cancelled,
        };
        let response = match response {
            Ok(r) => r,
            Err(e) if signed_out(&e) => break Ended::SignedOut,
            Err(e) => {
                tracing::warn!(connection = %live.connection, "Matrix sync failed: {}", plain_error(&e));
                break Ended::Failed;
            }
        };
        if first {
            first = false;
            *backoff = Duration::from_secs(2);
            // Refresh the status line: it may still say the server was unreachable.
            let _ = tx.send(None);
        }
        for item in incoming(response, client.user_id(), since) {
            if tx.send(Some(item)).is_err() {
                break;
            }
        }
    };
    drop(tx);
    let _ = handler.await;
    ended
}

/// The events of a sync worth handling, oldest first.
fn incoming(
    response: matrix_sdk::sync::SyncResponse,
    me: Option<&UserId>,
    since: i64,
) -> Vec<Incoming> {
    let mut out = Vec::new();
    for room in response.rooms.invited.into_keys() {
        out.push(Incoming::Invite { room });
    }
    for room in response.rooms.left.into_keys() {
        out.push(Incoming::Gone { room });
    }
    for (room, update) in response.rooms.joined {
        out.push(Incoming::Joined { room: room.clone() });
        for event in update.timeline.events {
            let sealed = event.encryption_info().is_some();
            let Ok(event) = event.raw().deserialize() else {
                continue;
            };
            let sender = event.sender().to_owned();
            // History from before the connection isn't a request.
            if Some(sender.as_ref()) == me || i64::from(event.origin_server_ts().0) < since {
                continue;
            }
            let room = room.clone();
            match event {
                AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                    SyncMessageLikeEvent::Original(message),
                )) => {
                    let content = message.content;
                    let quoted = match content.relates_to {
                        // Edits aren't new requests.
                        Some(Relation::Replacement(_)) => continue,
                        Some(Relation::Reply(reply)) => Some(reply.in_reply_to.event_id),
                        Some(Relation::Thread(Thread {
                            in_reply_to: Some(reply),
                            is_falling_back: false,
                            ..
                        })) => Some(reply.event_id),
                        _ => None,
                    };
                    let text = match content.msgtype {
                        MessageType::Text(t) => t.body,
                        MessageType::Emote(t) => t.body,
                        _ => continue,
                    };
                    let text = if quoted.is_some() {
                        format::strip_reply_fallback(&text).to_owned()
                    } else {
                        text
                    };
                    out.push(Incoming::Text {
                        room,
                        sender,
                        text,
                        quoted,
                        sealed,
                    });
                }
                AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::Reaction(
                    SyncMessageLikeEvent::Original(reaction),
                )) => out.push(Incoming::Reaction {
                    room,
                    sender,
                    target: reaction.content.relates_to.event_id,
                    key: reaction.content.relates_to.key,
                }),
                AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomEncrypted(_)) => {
                    out.push(Incoming::Unreadable { room, sender });
                }
                AnySyncTimelineEvent::State(AnySyncStateEvent::RoomMember(
                    SyncStateEvent::Original(member),
                )) if matches!(
                    member.content.membership,
                    MembershipState::Leave | MembershipState::Ban
                ) =>
                {
                    if let Ok(user) = UserId::parse(member.state_key.as_str()) {
                        out.push(Incoming::Left { room, user });
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Handles a connection's events one at a time, saving its config as it changes.
/// `None` asks for the status line to be refreshed.
async fn handle_all(
    state: Arc<AppState>,
    live: Arc<Live>,
    mut config: MatrixConfig,
    mut rx: mpsc::UnboundedReceiver<Option<Incoming>>,
) {
    let id = live.connection;
    while let Some(item) = rx.recv().await {
        let Some(item) = item else {
            let (status, detail, url) = describe(&config);
            state
                .connections
                .set_status(&state, id, status, detail, url)
                .await;
            continue;
        };
        let before = serde_json::to_value(&config).ok();
        handle(&state, &live, &mut config, item).await;
        let after = serde_json::to_value(&config).ok();
        if before != after
            && let (Some(after), Ok(Some(mut row))) = (after, store::get(&state.db, id).await)
        {
            row.config = after;
            let _ = store::upsert(&state.db, row).await;
            let (status, detail, url) = describe(&config);
            state
                .connections
                .set_status(&state, id, status, detail, url)
                .await;
        }
    }
}

async fn handle(
    state: &Arc<AppState>,
    live: &Arc<Live>,
    config: &mut MatrixConfig,
    item: Incoming,
) {
    let client = &live.client;
    match item {
        Incoming::Invite { room } => {
            let Some(room) = client.get_room(&room) else {
                return;
            };
            let inviter = room
                .invite_details()
                .await
                .ok()
                .and_then(|i| i.inviter)
                .map(|m| m.user_id().to_string());
            let direct = room.is_direct().await.unwrap_or(false);
            // Someone trusted opening a chat of two: marked direct, or just them and it.
            let trusted_direct = match inviter.as_deref() {
                Some(who) if config.owner.as_deref() != Some(who) => {
                    (direct || room.active_members_count() <= 2)
                        && crate::access::find(state, mimi_protocol::Channel::Matrix, who)
                            .await
                            .is_some()
                }
                _ => false,
            };
            match answer_invite_from(config, inviter.as_deref(), direct, trusted_direct) {
                Invitation::KeepDirect => {
                    let id = room.room_id().to_string();
                    // Recorded first, so the sync doesn't leave the chat it's joining.
                    let _ = rooms::keep(
                        state,
                        live.connection,
                        &id,
                        Why::Direct,
                        inviter.as_deref(),
                        None,
                    )
                    .await;
                    match room.join().await {
                        Ok(()) => {
                            tracing::info!(connection = %live.connection, room = %id, "joined a chat someone the owner trusts opened");
                            // Like the owner's chat: end-to-end encrypted if it can be.
                            tokio::spawn(async move {
                                let _ = ensure_encrypted(&room).await;
                            });
                        }
                        Err(e) => {
                            tracing::warn!(
                                "joining a trusted person's chat failed: {}",
                                plain_error(&e)
                            );
                            let _ = rooms::forget(state, live.connection, &id).await;
                        }
                    }
                }
                Invitation::Join => {
                    if let Err(e) = room.join().await {
                        tracing::warn!("joining a Matrix room failed: {}", plain_error(&e));
                    }
                }
                Invitation::KeepGroup => {
                    let id = room.room_id().to_string();
                    let name = room.name();
                    // Recorded first, so the sync doesn't leave the group it's joining.
                    let _ = rooms::keep(
                        state,
                        live.connection,
                        &id,
                        Why::Group,
                        None,
                        name.as_deref(),
                    )
                    .await;
                    let _ =
                        rooms::remember(state, live.connection, &id, rooms::Known::Invited).await;
                    if let Err(e) = room.join().await {
                        tracing::warn!("joining a Matrix group failed: {}", plain_error(&e));
                        let _ = rooms::forget(state, live.connection, &id).await;
                    }
                }
                // Declining is leaving.
                Invitation::Decline => {
                    // Said in the log: someone replying by opening a chat of their own
                    // would otherwise vanish without a trace.
                    tracing::info!(
                        connection = %live.connection,
                        room = %room.room_id(),
                        inviter = inviter.as_deref().unwrap_or("unknown"),
                        "declined an invitation from someone who isn't the owner or trusted"
                    );
                    let _ = room.leave().await;
                }
            }
        }
        Incoming::Joined { room } => {
            // Once paired, the assistant stays only in its owner's chat and the rooms it
            // keeps: groups the owner invited it into, and chats it opened or groups it
            // joined to send messages (the rooms it made itself are its own, even before
            // they're recorded).
            if config.owner.is_some()
                && config.room_id.is_some()
                && config.room_id.as_deref() != Some(room.as_str())
                && let Some(room) = client.get_room(&room)
                && room.state() == RoomState::Joined
                && rooms::kept_room(state, live.connection, room.room_id().as_str())
                    .await
                    .is_none()
                && !room
                    .creators()
                    .is_some_and(|c| c.iter().any(|u| Some(u.as_ref()) == client.user_id()))
            {
                let _ = room.leave().await;
            }
        }
        Incoming::Gone { room } => {
            let _ = rooms::forget(state, live.connection, room.as_str()).await;
        }
        Incoming::Text {
            room: room_id,
            sender,
            text,
            quoted,
            sealed,
        } => {
            let Some(room) = client.get_room(&room_id) else {
                return;
            };
            let from = classify(config, room_id.as_str(), sender.as_str());
            if from == Sender::Owner && !trusted(config, sealed) {
                tracing::warn!(connection = %live.connection, "ignored an unencrypted message in the owner's encrypted Matrix chat");
                return;
            }
            if from == Sender::Owner && sealed {
                // The owner may have turned encryption on themselves.
                config.encrypted = true;
            }
            match from {
                Sender::Claimant => {
                    if is_pairing_code(config, &text) {
                        pair(state, live, config, &room, &sender).await;
                    }
                }
                Sender::OwnerElsewhere => {
                    // The owner left their chat and opened a new one: move there. (What
                    // they write in groups isn't for the assistant.)
                    if config.room_id.is_none()
                        && is_direct(&room).await
                        && rooms::kept_room(state, live.connection, room_id.as_str())
                            .await
                            .is_none()
                    {
                        config.room_id = Some(room_id.to_string());
                        config.encrypted = ensure_encrypted(&room).await != Privacy::Plain;
                        on_owner_text(state, live, config, &room, text, quoted).await;
                    }
                }
                Sender::Owner => on_owner_text(state, live, config, &room, text, quoted).await,
                // Someone the owner lets ask it things talks with it in their own chat;
                // anyone else's reply in a chat it opened is passed on to the owner as it
                // is, never to the model (`guests.rs`).
                Sender::Stranger => {
                    let Some(messenger) = state.connections.matrix.messenger(live.connection)
                    else {
                        return;
                    };
                    guests::on_text(
                        state,
                        live.connection,
                        config,
                        messenger,
                        guests::Inbound {
                            room: room_id.to_string(),
                            sender: sender.to_string(),
                            text,
                            quoted: quoted.map(|q| q.to_string()),
                            sealed,
                        },
                    )
                    .await;
                }
            }
        }
        Incoming::Reaction {
            room,
            sender,
            target,
            key,
        } => {
            // Matrix apps never encrypt reactions, so these count even in an encrypted
            // chat. They only ever answer a prompt the assistant sent.
            if classify(config, room.as_str(), sender.as_str()) != Sender::Owner {
                if let Some(messenger) = state.connections.matrix.messenger(live.connection) {
                    guests::on_reaction(
                        state,
                        live.connection,
                        messenger,
                        room.as_str(),
                        sender.as_str(),
                        target.as_str(),
                        &key,
                    )
                    .await;
                }
                return;
            }
            if let Some(line) =
                replies::answer_reaction(state, live.connection, target.as_str(), &key).await
                && let Some(room) = client.get_room(&room)
            {
                let _ = send_plain(&room, &line).await;
            }
        }
        Incoming::Unreadable { room, sender } => {
            if classify(config, room.as_str(), sender.as_str()) == Sender::Owner {
                tracing::warn!(connection = %live.connection, room = %room, "couldn't decrypt a message from the owner");
                if let Some(room) = client.get_room(&room) {
                    let _ = send_plain(&room, UNREADABLE).await;
                }
            } else if let Some(messenger) = state.connections.matrix.messenger(live.connection) {
                guests::on_unreadable(
                    state,
                    live.connection,
                    config,
                    messenger,
                    room.as_str(),
                    sender.as_str(),
                )
                .await;
            }
        }
        Incoming::Left { room, user } => {
            if config.owner.as_deref() == Some(user.as_str())
                && config.room_id.as_deref() == Some(room.as_str())
            {
                config.room_id = None;
                if let Some(room) = client.get_room(&room) {
                    let _ = room.leave().await;
                }
            } else if let Some(kept) = rooms::kept_room(state, live.connection, room.as_str()).await
                && kept.why == Why::Direct
                && kept.user_id.as_deref() == Some(user.as_str())
            {
                // They left the chat the assistant opened with them: the next message
                // opens a new one.
                let _ = rooms::forget(state, live.connection, room.as_str()).await;
                if let Some(room) = client.get_room(&room) {
                    let _ = room.leave().await;
                }
            }
        }
    }
}

/// Said when an encrypted message couldn't be read.
pub const UNREADABLE: &str =
    "I couldn't read your last message: its encryption keys didn't reach me. Try sending it again.";

/// Most characters of someone's reply passed on to the owner.
const FORWARD_LIMIT: usize = 2_000;

/// "Sam Carter (@sam:example.org) replied:" and their message, quoted, as plain text and
/// as Matrix HTML with everything they wrote escaped.
pub fn forward_text(name: Option<&str>, user: &str, text: &str) -> (String, String) {
    let text = text.trim();
    let text = if text.chars().count() > FORWARD_LIMIT {
        format!("{}…", text.chars().take(FORWARD_LIMIT).collect::<String>())
    } else {
        text.to_owned()
    };
    let who = match name {
        Some(n) => format!("{n} ({user})"),
        None => user.to_owned(),
    };
    let quoted: Vec<String> = text.lines().map(|l| format!("> {l}")).collect();
    let body = format!("💬 {who} replied:\n{}", quoted.join("\n"));
    let html = format!(
        "💬 <b>{}</b> replied:<blockquote>{}</blockquote>",
        format::escape_html(&who),
        text.lines()
            .map(format::escape_html)
            .collect::<Vec<_>>()
            .join("<br>")
    );
    (body, html)
}

/// Whether a room is a chat between two people (the assistant and one other).
async fn is_direct(room: &Room) -> bool {
    room.members(RoomMemberships::ACTIVE)
        .await
        .is_ok_and(|members| members.len() <= 2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Privacy {
    Encrypted,
    TurnedOn,
    Plain,
}

/// Turns on end-to-end encryption in the owner's chat if it's off and the assistant may.
async fn ensure_encrypted(room: &Room) -> Privacy {
    if room
        .latest_encryption_state()
        .await
        .is_ok_and(|s| s.is_encrypted())
    {
        return Privacy::Encrypted;
    }
    match tokio::time::timeout(Duration::from_secs(20), room.enable_encryption()).await {
        Ok(Ok(())) => Privacy::TurnedOn,
        _ => Privacy::Plain,
    }
}

/// The code arrived: its sender becomes the owner, in this chat.
async fn pair(
    state: &Arc<AppState>,
    live: &Arc<Live>,
    config: &mut MatrixConfig,
    room: &Room,
    sender: &UserId,
) {
    if !is_direct(room).await {
        let _ = send_plain(
            room,
            "Send me the code in a direct chat, just the two of us.",
        )
        .await;
        return;
    }
    let display_name = room
        .get_member_no_sync(sender)
        .await
        .ok()
        .flatten()
        .and_then(|m| m.display_name().map(str::to_owned));
    config.owner = Some(sender.to_string());
    config.owner_name = display_name;
    config.room_id = Some(room.room_id().to_string());
    config.pairing_code = None;
    tracing::info!(connection = %live.connection, "Matrix assistant paired with its owner");

    // Other rooms the account was in (other people's invitations while unpaired).
    for other in live.client.joined_rooms() {
        if other.room_id() != room.room_id() {
            let _ = other.leave().await;
        }
    }

    let privacy = ensure_encrypted(room).await;
    config.encrypted = privacy != Privacy::Plain;
    let name = channels::assistant_name(state).await;
    let hello = format!(
        "Hi{}! I'm {}, your assistant. Message me here any time, and send /new to start a fresh conversation.",
        config
            .owner_name
            .as_ref()
            .map(|n| format!(" {}", format::escape_markdown(n)))
            .unwrap_or_default(),
        format::escape_markdown(&name)
    );
    let note = match privacy {
        Privacy::Encrypted => {
            "This chat is end-to-end encrypted: only your devices and this computer can read it."
        }
        Privacy::TurnedOn => {
            "I turned on end-to-end encryption for this chat, so only your devices and this computer can read it."
        }
        Privacy::Plain => {
            "This chat isn't end-to-end encrypted, and I couldn't turn it on: your Matrix server can read it. You can turn encryption on in the chat's settings."
        }
    };
    let channel = MatrixChannel {
        live: live.clone(),
        room: room.clone(),
    };
    let _ = channel
        .send(&Outgoing::text(format!("{hello}\n\n*{note}*")))
        .await;
}

/// A message from the owner in their chat: an answer to a prompt, a command, or a
/// request for the assistant.
async fn on_owner_text(
    state: &Arc<AppState>,
    live: &Arc<Live>,
    config: &mut MatrixConfig,
    room: &Room,
    text: String,
    quoted: Option<OwnedEventId>,
) {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return;
    }
    if let Some(line) = replies::answer_text(
        state,
        live.connection,
        quoted.as_ref().map(|q| q.as_str()),
        &text,
    )
    .await
    {
        let _ = send_plain(room, &line).await;
        return;
    }
    if text == "/new" {
        config.conversation_id = None;
        let _ = send_plain(room, "Started a new conversation.").await;
        return;
    }
    let conversation = channels::ensure_conversation(state, config.conversation_id, "Matrix").await;
    config.conversation_id = conversation;
    let Some(conversation) = conversation else {
        let _ = send_plain(room, "Sorry, I couldn't open our conversation.").await;
        return;
    };
    let (state, channel) = (
        state.clone(),
        MatrixChannel {
            live: live.clone(),
            room: room.clone(),
        },
    );
    // Replies can take a while; don't hold up the next messages.
    tokio::spawn(async move {
        channels::converse(&state, &channel, conversation, text).await;
        let _ = channel.room.typing_notice(false).await;
    });
}

async fn send_plain(room: &Room, text: &str) -> Result<OwnedEventId, String> {
    room.send(RoomMessageEventContent::text_plain(text))
        .await
        .map(|r| r.response.event_id)
        .map_err(|e| plain_error(&e))
}

/// Every paired Matrix account that's running, for messages Mimi sends on its own
/// (reminders, routine results).
pub async fn owners(state: &AppState) -> Vec<Arc<dyn Channel>> {
    let Ok(rows) = store::list(&state.db).await else {
        return Vec::new();
    };
    rows.into_iter()
        .filter(|r| r.integration == MATRIX)
        .filter_map(|r| {
            let config = serde_json::from_value::<MatrixConfig>(r.config).ok()?;
            let room_id = OwnedRoomId::try_from(config.room_id?).ok()?;
            config.owner.as_ref()?;
            let live = state.connections.matrix.get(r.id)?;
            let room = live.client.get_room(&room_id)?;
            let channel: Arc<dyn Channel> = Arc::new(MatrixChannel { live, room });
            Some(channel)
        })
        .collect()
}

/// A paired Matrix account that's running: what the messaging tools send from.
pub struct Paired {
    pub id: Uuid,
    pub config: MatrixConfig,
    pub messenger: Arc<dyn Messenger>,
}

/// Every paired, running Matrix account.
pub async fn paired(state: &AppState) -> Vec<Paired> {
    let Ok(rows) = store::list(&state.db).await else {
        return Vec::new();
    };
    rows.into_iter()
        .filter(|r| r.integration == MATRIX)
        .filter_map(|r| {
            let config = serde_json::from_value::<MatrixConfig>(r.config).ok()?;
            if config.owner.is_none() || config.signed_out {
                return None;
            }
            let messenger = state.connections.matrix.messenger(r.id)?;
            Some(Paired {
                id: r.id,
                config,
                messenger,
            })
        })
        .collect()
}

/// The paired connection whose assistant account is `user_id`, running or not.
pub async fn paired_config(state: &AppState, user_id: &str) -> Option<(Uuid, MatrixConfig)> {
    store::list(&state.db)
        .await
        .ok()?
        .into_iter()
        .filter(|r| r.integration == MATRIX)
        .filter_map(|r| Some((r.id, serde_json::from_value::<MatrixConfig>(r.config).ok()?)))
        .find(|(_, c)| c.owner.is_some() && c.user_id.eq_ignore_ascii_case(user_id))
}

/// The servers the paired Matrix accounts live on, e.g. for "Everyone on example.org".
pub async fn servers(state: &AppState) -> Vec<String> {
    let mut out: Vec<String> = store::list(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.integration == MATRIX)
        .filter_map(|r| serde_json::from_value::<MatrixConfig>(r.config).ok())
        .filter_map(|c| rooms::server_of_user(&c.user_id))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Signs the assistant's device out (best effort) and removes its local store.
pub async fn forget(state: &AppState, id: Uuid, config: Option<MatrixConfig>) {
    state.connections.matrix.remove(id, None);
    let dir = store_dir(state, id);
    let http = state.http.clone();
    tokio::spawn(async move {
        if let Some(config) = config
            && !config.signed_out
        {
            // Errors are dropped unread: they would carry the server's address.
            let _ = http
                .post(format!(
                    "{}/_matrix/client/v3/logout",
                    config.homeserver.trim_end_matches('/')
                ))
                .bearer_auth(&config.access_token)
                .json(&serde_json::json!({}))
                .timeout(Duration::from_secs(15))
                .send()
                .await;
        }
        // Give the stopped sync a moment to let go of its files.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = tokio::fs::remove_dir_all(&dir).await;
    });
}

/// Stops a connection's task and starts it again from its store, as a restarted daemon
/// would.
#[cfg(test)]
pub async fn restart(state: &Arc<AppState>, id: Uuid) {
    state.connections.stop(id);
    for _ in 0..600 {
        if state.connections.matrix.get(id).is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let row = store::get(&state.db, id).await.unwrap().unwrap();
    super::start(state, &row);
}

/// The owner's chat with the assistant.
pub struct MatrixChannel {
    live: Arc<Live>,
    room: Room,
}

impl MatrixChannel {
    /// Sends a prompt and remembers what it asks, so a reply or reaction answers it.
    async fn prompt(&self, body: String, html: String, target: Target) -> Result<(), String> {
        let event = self
            .room
            .send(RoomMessageEventContent::text_html(body, html))
            .await
            .map_err(|e| plain_error(&e))?
            .response
            .event_id;
        if let Some(state) = self.live.state.upgrade() {
            state
                .connections
                .prompts
                .remember(self.live.connection, event.to_string(), target);
        }
        Ok(())
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn kind(&self) -> &'static str {
        MATRIX
    }

    async fn send(&self, message: &Outgoing) -> Result<(), String> {
        for part in format::outgoing(message) {
            self.room
                .send(RoomMessageEventContent::text_html(part.body, part.html))
                .await
                .map_err(|e| plain_error(&e))?;
        }
        Ok(())
    }

    async fn typing(&self) {
        let _ = self.room.typing_notice(true).await;
    }

    async fn ask_approval(&self, action: &Action) -> Result<(), String> {
        let (body, html) = approval_prompt(action);
        self.prompt(body, html, Target::Approval(action.id)).await
    }

    async fn remind(&self, delivery: &Delivery, late: Option<&str>) -> Result<(), String> {
        let (body, html) = reminder_prompt(delivery, late);
        self.prompt(body, html, Target::Reminder(delivery.id)).await
    }
}

/// An approval prompt, as plain text and Matrix HTML.
pub fn approval_prompt(action: &Action) -> (String, String) {
    let body = format!(
        "Waiting for you: {}\n{}",
        action.summary,
        replies::APPROVAL_HINT
    );
    let html = format!(
        "<b>Waiting for you</b><br>{}<br><i>{}</i>",
        format::escape_html(&action.summary),
        format::escape_html(replies::APPROVAL_HINT)
    );
    (body, html)
}

/// A due reminder, as plain text and Matrix HTML.
pub fn reminder_prompt(delivery: &Delivery, late: Option<&str>) -> (String, String) {
    let body = format!(
        "⏰ {}{}\n{}",
        delivery.title,
        late.map(|l| format!("\n{l}")).unwrap_or_default(),
        replies::REMINDER_HINT
    );
    let html = format!(
        "⏰ <b>{}</b>{}<br><i>{}</i>",
        format::escape_html(&delivery.title),
        late.map(|l| format!("<br><i>{}</i>", format::escape_html(l)))
            .unwrap_or_default(),
        format::escape_html(replies::REMINDER_HINT)
    );
    (body, html)
}

#[cfg(test)]
mod tests;
