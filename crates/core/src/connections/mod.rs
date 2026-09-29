//! Links to the user's own accounts: calendars, messaging and email. Every connection is
//! checked against the real service before it's saved, and its secrets stay in the
//! encrypted database.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mimi_protocol::{Connection, ConnectionSetup, ConnectionStatus, Event};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use self::calendar::google::GoogleAccountConfig;
use self::calendar::{Account, CalDavConfig, GoogleConfig, caldav::CalDav};
use self::matrix::MatrixConfig;
use self::store::ConnectionRow;
use self::telegram::{Bot, TelegramConfig};
use crate::api::error::AppError;
use crate::db::DbError;
use crate::{AppState, now_ms};

pub mod calendar;
pub mod matrix;
pub mod signal;
pub mod store;
pub mod telegram;

/// A connection's status, its one-line detail and where to finish setting it up.
type LiveStatus = (ConnectionStatus, String, Option<String>);

/// Live state of connections: status lines and background tasks.
pub struct Connections {
    status: Mutex<HashMap<Uuid, LiveStatus>>,
    tasks: Mutex<HashMap<Uuid, CancellationToken>>,
    pub feeds: calendar::FeedCache,
    /// Where the Telegram Bot API lives; tests point it at a fake.
    pub telegram_api: Mutex<String>,
    /// Prompts sent to apps without buttons, for matching replies and reactions.
    pub prompts: Arc<crate::channels::replies::Prompts>,
    /// The running Signal clients.
    pub signal: signal::Workers,
    /// Running Matrix clients.
    pub matrix: matrix::Clients,
}

impl Default for Connections {
    fn default() -> Self {
        Self {
            status: Default::default(),
            tasks: Default::default(),
            feeds: Default::default(),
            telegram_api: Mutex::new(telegram::default_api()),
            prompts: Default::default(),
            signal: Default::default(),
            matrix: Default::default(),
        }
    }
}

impl Connections {
    pub fn telegram_api(&self) -> String {
        self.telegram_api
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub async fn set_status(
        &self,
        state: &AppState,
        id: Uuid,
        status: ConnectionStatus,
        detail: String,
        action_url: Option<String>,
    ) {
        let changed = {
            let mut map = self.status.lock().unwrap_or_else(|e| e.into_inner());
            let next = (status, detail, action_url);
            map.insert(id, next.clone()) != Some(next)
        };
        if changed {
            publish(state).await;
        }
    }

    /// The live status of a connection, if something set one.
    fn live(&self, id: Uuid) -> Option<LiveStatus> {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    fn stop(&self, id: Uuid) {
        if let Some(token) = self
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
        {
            token.cancel();
        }
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }
}

/// Connections as clients see them.
pub async fn list(state: &AppState) -> Result<Vec<Connection>, DbError> {
    record_signed_out(state).await;
    let rows = store::list(&state.db).await?;
    let live = state
        .connections
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Ok(rows
        .into_iter()
        .map(|row| {
            let (status, detail, action_url) =
                live.get(&row.id).cloned().unwrap_or_else(|| describe(&row));
            Connection {
                id: row.id,
                integration: row.integration,
                name: row.name,
                status,
                detail,
                action_url,
                created_at: row.created_at,
            }
        })
        .collect())
}

fn describe(row: &ConnectionRow) -> LiveStatus {
    match row.integration.as_str() {
        calendar::GOOGLE => (
            ConnectionStatus::Ok,
            "Reads your events. New events open in Google Calendar for you to save.".to_owned(),
            None,
        ),
        calendar::CALDAV => {
            let n = serde_json::from_value::<CalDavConfig>(row.config.clone())
                .map(|c| c.calendars.len())
                .unwrap_or(0);
            (
                ConnectionStatus::Ok,
                format!("{n} calendar{}", if n == 1 { "" } else { "s" }),
                None,
            )
        }
        calendar::GOOGLE_ACCOUNT => {
            match serde_json::from_value::<GoogleAccountConfig>(row.config.clone()) {
                Ok(c) if c.signed_out => (
                    ConnectionStatus::Error,
                    "Google stopped accepting your assistant's access. Sign in again.".to_owned(),
                    None,
                ),
                Ok(c) => {
                    let n = c.calendars.len();
                    (
                        ConnectionStatus::Ok,
                        format!(
                            "{n} calendar{}. Reads, adds and changes events.",
                            if n == 1 { "" } else { "s" }
                        ),
                        None,
                    )
                }
                Err(_) => (
                    ConnectionStatus::Error,
                    "Unreadable settings".to_owned(),
                    None,
                ),
            }
        }
        telegram::TELEGRAM => serde_json::from_value::<TelegramConfig>(row.config.clone())
            .map(|c| telegram::describe(&c))
            .unwrap_or((
                ConnectionStatus::Error,
                "Unreadable settings".to_owned(),
                None,
            )),
        matrix::MATRIX => serde_json::from_value::<MatrixConfig>(row.config.clone())
            .map(|c| matrix::describe(&c))
            .unwrap_or((
                ConnectionStatus::Error,
                "Unreadable settings".to_owned(),
                None,
            )),
        crate::mail::EMAIL => (ConnectionStatus::Ok, crate::mail::DETAIL.to_owned(), None),
        signal::SIGNAL => serde_json::from_value::<signal::SignalConfig>(row.config.clone())
            .map(|c| signal::describe(&c))
            .unwrap_or((
                ConnectionStatus::Error,
                "Unreadable settings".to_owned(),
                None,
            )),
        _ => (
            ConnectionStatus::Error,
            "Unknown integration".to_owned(),
            None,
        ),
    }
}

/// Records the Google accounts that stopped accepting their access since last time, so
/// they say "sign in again" (and keep saying it after a restart). Whether any did.
async fn record_signed_out(state: &AppState) -> bool {
    let ids = state.connections.feeds.google.take_signed_out();
    let mut any = false;
    for id in ids {
        let Ok(Some(mut row)) = store::get(&state.db, id).await else {
            continue;
        };
        let Ok(mut config) = serde_json::from_value::<GoogleAccountConfig>(row.config.clone())
        else {
            continue;
        };
        config.signed_out = true;
        let Ok(value) = serde_json::to_value(config) else {
            continue;
        };
        row.config = value;
        if store::upsert(&state.db, row).await.is_ok() {
            tracing::info!(connection = %id, "Google stopped accepting a connection's access");
            any = true;
        }
    }
    any
}

/// Saves a Google account after sign-in: a new connection, the existing one for the
/// same account (new tokens, current calendars), or the one being signed in again.
pub async fn save_google_account(
    state: &Arc<AppState>,
    reconnect: Option<Uuid>,
    config: GoogleAccountConfig,
) -> Result<Connection, String> {
    let rows = store::list(&state.db)
        .await
        .map_err(|_| "Couldn't save the connection.".to_owned())?;
    let same_account = |r: &&ConnectionRow| {
        r.integration == calendar::GOOGLE_ACCOUNT
            && serde_json::from_value::<GoogleAccountConfig>(r.config.clone())
                .is_ok_and(|c| c.email.eq_ignore_ascii_case(&config.email))
    };
    let existing = match reconnect {
        Some(id) => {
            let row = rows
                .iter()
                .find(|r| r.id == id)
                .ok_or("That connection was removed meanwhile.")?;
            if !same_account(&row) {
                return Err(format!(
                    "That's a different Google account. Sign in as {} to reconnect it, or connect the other account on its own.",
                    row.name
                ));
            }
            Some(row.clone())
        }
        None => rows.iter().find(same_account).cloned(),
    };
    let row = ConnectionRow {
        id: existing.as_ref().map(|r| r.id).unwrap_or_else(Uuid::now_v7),
        integration: calendar::GOOGLE_ACCOUNT.to_owned(),
        name: config.email.clone(),
        config: serde_json::to_value(&config).map_err(|e| e.to_string())?,
        created_at: existing
            .as_ref()
            .map(|r| r.created_at)
            .unwrap_or_else(now_ms),
    };
    store::upsert(&state.db, row.clone())
        .await
        .map_err(|_| "Couldn't save the connection.".to_owned())?;
    state.connections.feeds.google.forget(row.id);
    publish(state).await;
    let (status, detail, action_url) = describe(&row);
    Ok(Connection {
        id: row.id,
        integration: row.integration,
        name: row.name,
        status,
        detail,
        action_url,
        created_at: row.created_at,
    })
}

async fn publish(state: &AppState) {
    if let Ok(connections) = list(state).await {
        state
            .events
            .publish(Event::ConnectionsChanged { connections });
    }
}

/// Checks what the user entered against the real service, then saves it.
pub async fn create(state: &Arc<AppState>, setup: ConnectionSetup) -> Result<Connection, AppError> {
    let id = Uuid::now_v7();
    let (integration, name, config) = match setup {
        ConnectionSetup::GoogleCalendar { ics_url } => {
            let url = calendar::parse_ics_url(&ics_url).map_err(AppError::bad_request)?;
            let body = state
                .connections
                .feeds
                .fetch(&state.http, url.as_str())
                .await
                .map_err(AppError::bad_request)?;
            let name =
                calendar::ics::calendar_name(&body).unwrap_or_else(|| "Google Calendar".to_owned());
            let config = GoogleConfig {
                ics_url: url.to_string(),
            };
            (calendar::GOOGLE, name, serde_json::to_value(config))
        }
        ConnectionSetup::Caldav {
            server_url,
            username,
            password,
        } => {
            let url =
                crate::providers::parse_base_url(&server_url).map_err(AppError::bad_request)?;
            if url.scheme() != "https"
                && crate::providers::locality_of(&url) == mimi_protocol::Locality::Cloud
            {
                return Err(AppError::bad_request(
                    "Use an https:// address, so your password isn't sent unencrypted.",
                ));
            }
            let calendars = CalDav::new(username.trim(), password.trim())
                .discover(&url)
                .await
                .map_err(|e| AppError::bad_request(e.to_string()))?;
            if calendars.is_empty() {
                return Err(AppError::bad_request("That account has no calendars."));
            }
            let name = url
                .host_str()
                .map(friendly_host)
                .unwrap_or_else(|| "Calendar".to_owned());
            let config = CalDavConfig {
                server_url: url.to_string(),
                username: username.trim().to_owned(),
                password: password.trim().to_owned(),
                calendars,
            };
            (calendar::CALDAV, name, serde_json::to_value(config))
        }
        ConnectionSetup::Telegram { bot_token } => {
            let bot = Bot::new(
                state.http.clone(),
                &state.connections.telegram_api(),
                &bot_token,
            );
            let me = bot
                .get_me()
                .await
                .map_err(|e| AppError::bad_request(e.to_string()))?;
            let username = me
                .username
                .ok_or_else(|| AppError::bad_request("That token doesn't belong to a bot."))?;
            let config = TelegramConfig {
                bot_token: bot_token.trim().to_owned(),
                bot_username: username.clone(),
                pairing_code: Some(telegram::pairing_code()),
                owner_chat_id: None,
                owner_name: None,
                conversation_id: None,
                offset: 0,
            };
            (
                telegram::TELEGRAM,
                format!("@{username}"),
                serde_json::to_value(config),
            )
        }
        ConnectionSetup::Matrix {
            user,
            password,
            homeserver,
        } => {
            let (name, config) =
                matrix::connect(state, id, &user, &password, homeserver.as_deref())
                    .await
                    .map_err(AppError::bad_request)?;
            (matrix::MATRIX, name, serde_json::to_value(config))
        }
        ConnectionSetup::Email {
            email,
            password,
            preset,
            servers,
        } => {
            let (name, config) =
                crate::mail::connect(&state.http, email, password, preset, servers)
                    .await
                    .map_err(AppError::bad_request)?;
            let taken = store::list(&state.db)
                .await?
                .into_iter()
                .any(|r| r.integration == crate::mail::EMAIL && r.name.eq_ignore_ascii_case(&name));
            if taken {
                return Err(AppError::bad_request(
                    "That email account is already connected.",
                ));
            }
            (crate::mail::EMAIL, name, serde_json::to_value(config))
        }
        // Linking happens after the row is saved: the user scans a code the daemon shows.
        ConnectionSetup::Signal {} => return signal::connect(state).await,
    };
    let row = ConnectionRow {
        id,
        integration: integration.to_owned(),
        name,
        config: config.map_err(AppError::internal)?,
        created_at: now_ms(),
    };
    store::upsert(&state.db, row.clone()).await?;
    start(state, &row);
    publish(state).await;
    let (status, detail, action_url) = describe(&row);
    Ok(Connection {
        id: row.id,
        integration: row.integration,
        name: row.name,
        status,
        detail,
        action_url,
        created_at: row.created_at,
    })
}

pub async fn delete(state: &AppState, id: Uuid) -> Result<bool, DbError> {
    state.connections.stop(id);
    let row = store::get(&state.db, id).await.ok().flatten();
    if let Some(row) = &row
        && let Ok(config) = serde_json::from_value::<GoogleConfig>(row.config.clone())
    {
        state.connections.feeds.forget(&config.ics_url);
    }
    let removed = store::delete(&state.db, id).await?;
    if let Some(r) = &row
        && r.integration == calendar::GOOGLE_ACCOUNT
    {
        state.connections.feeds.google.forget(id);
        // Tell Google to forget the access too, so nothing is left behind.
        if let (Ok(config), Some(endpoints)) = (
            serde_json::from_value::<GoogleAccountConfig>(r.config.clone()),
            state.connections.feeds.google.endpoints(),
        ) {
            let http = state.http.clone();
            tokio::spawn(async move {
                calendar::google::revoke(&http, &endpoints, &config.refresh_token).await;
            });
        }
    }
    if let Some(r) = &row
        && r.integration == matrix::MATRIX
    {
        matrix::forget(state, id, serde_json::from_value(r.config.clone()).ok()).await;
    }
    if row.is_some_and(|r| r.integration == crate::mail::EMAIL) {
        crate::mail::forget(state, id).await;
    }
    publish(state).await;
    Ok(removed)
}

/// Starts background work for every connection that needs it. Called at startup.
pub async fn start_all(state: &Arc<AppState>) {
    match store::list(&state.db).await {
        Ok(rows) => rows.iter().for_each(|row| start(state, row)),
        Err(e) => tracing::error!("couldn't load connections: {e}"),
    }
}

fn start(state: &Arc<AppState>, row: &ConnectionRow) {
    let token = CancellationToken::new();
    let (s, id, t) = (state.clone(), row.id, token.clone());
    match row.integration.as_str() {
        telegram::TELEGRAM => tokio::spawn(telegram::run(s, id, t)),
        signal::SIGNAL => tokio::spawn(signal::run(s, id, t)),
        matrix::MATRIX => tokio::spawn(matrix::run(s, id, t)),
        crate::mail::EMAIL => tokio::spawn(crate::mail::sync::run(s, id, t)),
        _ => return,
    };
    state
        .connections
        .tasks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(row.id, token);
}

/// Every connected calendar account.
pub async fn calendar_accounts(state: &AppState) -> Vec<Account> {
    if record_signed_out(state).await {
        publish(state).await;
    }
    let Ok(rows) = store::list(&state.db).await else {
        return Vec::new();
    };
    rows.into_iter()
        .filter_map(|row| match row.integration.as_str() {
            calendar::GOOGLE => {
                serde_json::from_value(row.config)
                    .ok()
                    .map(|config| Account::Google {
                        id: row.id,
                        name: row.name,
                        config,
                    })
            }
            calendar::CALDAV => serde_json::from_value(row.config)
                .ok()
                .map(|config| Account::CalDav { id: row.id, config }),
            calendar::GOOGLE_ACCOUNT => serde_json::from_value(row.config)
                .ok()
                .map(|config| Account::GoogleApi { id: row.id, config }),
            _ => None,
        })
        .collect()
}

/// "caldav.icloud.com" → "iCloud", otherwise the host itself.
fn friendly_host(host: &str) -> String {
    let known = [
        ("icloud.com", "iCloud"),
        ("fastmail.com", "Fastmail"),
        ("mailbox.org", "mailbox.org"),
        ("posteo.de", "Posteo"),
        ("gmx.", "GMX"),
    ];
    known
        .iter()
        .find(|(needle, _)| host.contains(needle))
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| host.to_owned())
}
