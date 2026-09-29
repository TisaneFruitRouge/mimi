//! Signal: Mimi links to the user's own account as a device, like Signal Desktop, by the
//! user scanning a code with Signal on their phone. The conversation is Note to Self:
//! what the user writes there goes to the assistant, and the assistant answers there,
//! each message headed with its name (in Note to Self everything looks like the user's).
//!
//! As a linked device Mimi receives every chat on the account. Everything but Note to
//! Self is dropped unread (`classify`) and nothing about other people is stored
//! (`store`). Signal doesn't notify anyone about their own Note to Self, so Mimi's
//! messages there arrive silently.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mimi_protocol::{Action, Connection, ConnectionStatus, Delivery};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use self::classify::Incoming;
use self::format::{Style, Styled};
use self::worker::{Mode, Pieces, Report, Worker};
use super::LiveStatus;
use super::store::{self as rows, ConnectionRow};
use crate::api::error::AppError;
use crate::channels::{self, Channel, Outgoing, replies};
use crate::{AppState, now_ms};

pub mod classify;
pub mod format;
pub mod store;
pub mod worker;

pub const SIGNAL: &str = "signal";

/// The longest piece sent as one Signal message, in characters.
const MAX_MESSAGE: usize = 1_500;

// Passing states end with an ellipsis: the connect dialog relies on it.
const GETTING: &str = "Getting a code from Signal…";
const SCAN: &str = "Scan this code with Signal on your phone: Settings › Linked devices.";
const OFFLINE: &str = "Can't reach Signal right now. Retrying…";
const UNLINKED: &str = "Unlinked from your phone. Link again to keep using Signal.";
const PAUSED: &str = "Not linked yet. Show the code again to link Signal.";

/// A Signal connection's settings. Its keys and sessions are in the `signal_*` tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SignalConfig {
    /// The account, once linked.
    pub account: Option<Account>,
    /// Where messages from Note to Self go.
    pub conversation_id: Option<Uuid>,
    /// When the codes started showing, while linking.
    #[serde(default)]
    pub linking_since: Option<i64>,
    /// Removed from the phone: needs linking again.
    #[serde(default)]
    pub unlinked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub aci: Uuid,
    /// The user's Signal name, or their number.
    pub name: String,
}

/// The running Signal clients, by connection.
#[derive(Default)]
pub struct Workers(Mutex<HashMap<Uuid, Worker>>);

impl Workers {
    fn get(&self, id: Uuid) -> Option<Worker> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    pub(crate) fn set(&self, id: Uuid, worker: Worker) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, worker);
    }

    /// Forgets a stopped worker, unless another one took its place.
    fn forget(&self, id: Uuid, worker: &Worker) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if map.get(&id).is_some_and(|w| w.same(worker)) {
            map.remove(&id);
        }
    }
}

/// The status line a Signal connection shows when nothing live says otherwise.
pub fn describe(config: &SignalConfig) -> LiveStatus {
    match &config.account {
        _ if config.unlinked => (ConnectionStatus::Error, UNLINKED.to_owned(), None),
        Some(account) => (
            ConnectionStatus::Ok,
            format!(
                "Linked as {}. Write to your assistant in Note to Self; its messages come without a notification.",
                account.name
            ),
            None,
        ),
        None => (ConnectionStatus::NeedsAction, PAUSED.to_owned(), None),
    }
}

async fn load(state: &AppState, id: Uuid) -> Option<(ConnectionRow, SignalConfig)> {
    let row = rows::get(&state.db, id).await.ok().flatten()?;
    let config = serde_json::from_value(row.config.clone()).ok()?;
    Some((row, config))
}

/// Changes a Signal connection's settings.
async fn update(
    state: &AppState,
    id: Uuid,
    f: impl FnOnce(&mut SignalConfig),
) -> Option<SignalConfig> {
    let (mut row, mut config) = load(state, id).await?;
    f(&mut config);
    row.config = serde_json::to_value(&config).ok()?;
    rows::upsert(&state.db, row).await.ok()?;
    Some(config)
}

/// Adds Signal, or links the existing connection again: there is one at a time.
pub async fn connect(state: &Arc<AppState>) -> Result<Connection, AppError> {
    let existing = rows::list(&state.db)
        .await?
        .into_iter()
        .find(|r| r.integration == SIGNAL);
    let now = now_ms();
    let row = match existing {
        Some(mut row) => {
            let mut config: SignalConfig =
                serde_json::from_value(row.config.clone()).unwrap_or_default();
            if config.account.is_some() && !config.unlinked {
                return Err(AppError::bad_request(
                    "Signal is already linked. Disconnect it first to link another account.",
                ));
            }
            state.connections.stop(row.id);
            config.account = None;
            config.unlinked = false;
            config.linking_since = Some(now);
            row.config = serde_json::to_value(&config).map_err(AppError::internal)?;
            row
        }
        None => ConnectionRow {
            id: Uuid::now_v7(),
            integration: SIGNAL.to_owned(),
            name: "Signal".to_owned(),
            config: serde_json::to_value(SignalConfig {
                linking_since: Some(now),
                ..Default::default()
            })
            .map_err(AppError::internal)?,
            created_at: now,
        },
    };
    rows::upsert(&state.db, row.clone()).await?;
    // Until the first code arrives.
    state
        .connections
        .set_status(
            state,
            row.id,
            ConnectionStatus::NeedsAction,
            GETTING.to_owned(),
            None,
        )
        .await;
    super::start(state, &row);
    let (status, detail, action_url) = state
        .connections
        .live(row.id)
        .unwrap_or_else(|| super::describe(&row));
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

/// Runs a Signal connection: linking, then receiving, until cancelled or unlinked.
pub async fn run(state: Arc<AppState>, id: Uuid, cancel: CancellationToken) {
    let Some((_, config)) = load(&state, id).await else {
        return;
    };
    if config.unlinked {
        return;
    }
    let mode = match (&config.account, config.linking_since) {
        (Some(_), _) => Mode::Run,
        // Linking was under way (the daemon restarted meanwhile): carry on.
        (None, Some(since)) if now_ms() - since < 60 * 60 * 1000 => Mode::Link,
        (None, _) => return,
    };
    if mode == Mode::Link {
        state
            .connections
            .set_status(
                &state,
                id,
                ConnectionStatus::NeedsAction,
                GETTING.to_owned(),
                None,
            )
            .await;
    }
    let assistant = channels::assistant_name(&state).await;
    let (worker, mut reports) =
        worker::spawn(state.db.clone(), id, assistant, mode, cancel.clone());
    state.connections.signal.set(id, worker.clone());
    let mut welcome = false;
    loop {
        // A stopped connection says nothing more, even with reports still queued.
        let report = tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            r = reports.recv() => r,
        };
        let Some(report) = report else { break };
        let show = |(status, detail, url): LiveStatus| {
            state
                .connections
                .set_status(&state, id, status, detail, url)
        };
        match report {
            Report::Code(url) => {
                show((ConnectionStatus::NeedsAction, SCAN.to_owned(), Some(url))).await;
            }
            Report::CodeFailed | Report::Offline => {
                show((ConnectionStatus::Error, OFFLINE.to_owned(), None)).await;
            }
            Report::LinkPaused => {
                update(&state, id, |c| c.linking_since = None).await;
                show((ConnectionStatus::NeedsAction, PAUSED.to_owned(), None)).await;
            }
            Report::Linked(linked) => {
                tracing::info!(connection = %id, "linked to Signal");
                let config = update(&state, id, |c| {
                    c.account = Some(Account {
                        aci: linked.aci,
                        name: linked.name.clone(),
                    });
                    c.linking_since = None;
                    c.unlinked = false;
                })
                .await
                .unwrap_or_default();
                show(describe(&config)).await;
                welcome = true;
            }
            Report::Online => {
                if let Some((_, config)) = load(&state, id).await {
                    show(describe(&config)).await;
                    if std::mem::take(&mut welcome) {
                        greet(&state, id, &worker, &config).await;
                    }
                }
            }
            Report::Unlinked => {
                tracing::info!(connection = %id, "Signal device was unlinked");
                update(&state, id, |c| c.unlinked = true).await;
                show((ConnectionStatus::Error, UNLINKED.to_owned(), None)).await;
                break;
            }
            Report::Message(incoming) => on_message(&state, id, &worker, incoming).await,
        }
    }
    state.connections.signal.forget(id, &worker);
}

/// The first message in Note to Self, once linked.
async fn greet(state: &AppState, id: Uuid, worker: &Worker, config: &SignalConfig) {
    let assistant = channels::assistant_name(state).await;
    let hi = config
        .account
        .as_ref()
        .filter(|a| !a.name.starts_with('+'))
        .map(|a| format!(" {}", a.name))
        .unwrap_or_default();
    let text = format!(
        "Hi{hi}! I'm {assistant}, your assistant. Write to me here in Note to Self any time.\n\n\
         I only read Note to Self. Your other chats stay between you and the people in them.\n\n\
         *Signal doesn't notify you about Note to Self, so my messages and reminders arrive silently.* \
         Send /new to start a fresh conversation."
    );
    let channel = SignalChannel::new(state, id, worker.clone(), assistant);
    if let Err(e) = channel.send(&Outgoing::text(text)).await {
        tracing::warn!("couldn't greet on Signal: {e}");
    }
}

pub(crate) async fn on_message(
    state: &Arc<AppState>,
    id: Uuid,
    worker: &Worker,
    incoming: Incoming,
) {
    let channel = SignalChannel::new(
        state,
        id,
        worker.clone(),
        channels::assistant_name(state).await,
    );
    let answer = match incoming {
        Incoming::Note { text, quote, .. } => {
            if text == "/new" {
                update(state, id, |c| c.conversation_id = None).await;
                Some("Started a new conversation.".to_owned())
            } else {
                let quote = quote.map(|q| q.to_string());
                match replies::answer_text(state, id, quote.as_deref(), &text).await {
                    Some(line) => Some(line),
                    None => {
                        chat(state, id, channel, text).await;
                        return;
                    }
                }
            }
        }
        Incoming::Reaction { emoji, target } => {
            replies::answer_reaction(state, id, &target.to_string(), &emoji).await
        }
        Incoming::OwnEcho | Incoming::Ignore => None,
    };
    if let Some(line) = answer
        && let Err(e) = channel.send(&Outgoing::text(line)).await
    {
        tracing::warn!("couldn't answer on Signal: {e}");
    }
}

/// Hands the user's message to the assistant, in the Signal conversation.
async fn chat(state: &Arc<AppState>, id: Uuid, channel: SignalChannel, text: String) {
    let current = load(state, id).await.and_then(|(_, c)| c.conversation_id);
    let conversation = channels::ensure_conversation(state, current, "Signal").await;
    if conversation != current {
        update(state, id, |c| c.conversation_id = conversation).await;
    }
    let Some(conversation) = conversation else {
        let _ = channel
            .send(&Outgoing::text("Sorry, I couldn't open our conversation."))
            .await;
        return;
    };
    // Replies can take a while; don't hold up receiving.
    let state = state.clone();
    tokio::spawn(async move {
        channels::converse(&state, &channel, conversation, text).await;
    });
}

/// Every linked Signal account, for messages Mimi sends on its own.
pub async fn owners(state: &AppState) -> Vec<Arc<dyn Channel>> {
    let Ok(rows) = rows::list(&state.db).await else {
        return Vec::new();
    };
    let mut out: Vec<Arc<dyn Channel>> = Vec::new();
    for row in rows.into_iter().filter(|r| r.integration == SIGNAL) {
        let Ok(config) = serde_json::from_value::<SignalConfig>(row.config) else {
            continue;
        };
        if config.account.is_none() || config.unlinked {
            continue;
        }
        if let Some(worker) = state.connections.signal.get(row.id) {
            let assistant = channels::assistant_name(state).await;
            out.push(Arc::new(SignalChannel::new(
                state, row.id, worker, assistant,
            )));
        }
    }
    out
}

/// Note to Self on a linked account.
pub struct SignalChannel {
    connection: Uuid,
    worker: Worker,
    /// Heads every message, since in Note to Self they all look like the user's.
    assistant: String,
    prompts: Arc<replies::Prompts>,
}

impl SignalChannel {
    pub fn new(state: &AppState, connection: Uuid, worker: Worker, assistant: String) -> Self {
        Self {
            connection,
            worker,
            assistant,
            prompts: state.connections.prompts.clone(),
        }
    }

    /// Sends a message, headed with the assistant's name. Signal's ids for what was sent.
    async fn deliver(&self, body: &Styled) -> Result<Vec<u64>, String> {
        self.worker.send(pieces(&self.assistant, body)).await
    }

    /// Remembers what the messages asked, so a reply or reaction to any of them answers.
    fn remember(&self, stamps: &[u64], target: replies::Target) {
        for stamp in stamps {
            self.prompts
                .remember(self.connection, stamp.to_string(), target);
        }
    }
}

/// The message as Signal pieces: the assistant's name in bold on the first line of each.
pub fn pieces(assistant: &str, body: &Styled) -> Pieces {
    let mut header = Styled::default();
    header.styled(Style::Bold, |h| h.push(assistant));
    header.push("\n");
    let room = MAX_MESSAGE
        .saturating_sub(header.text.chars().count())
        .max(100);
    body.split(room)
        .into_iter()
        .map(|piece| {
            let mut message = header.clone();
            message.append(&piece);
            let ranges = message.body_ranges();
            (message.text, ranges)
        })
        .collect()
}

/// An outgoing message as Signal text.
pub fn body(message: &Outgoing) -> Styled {
    let mut out = Styled::default();
    if let Some(title) = &message.title {
        out.styled(Style::Bold, |o| o.push(title));
        out.push("\n\n");
    }
    out.append(&format::markdown(&message.markdown));
    for link in &message.links {
        out.push(&format!("\n\n{}: {}", link.label, link.url));
    }
    out
}

#[async_trait]
impl Channel for SignalChannel {
    fn kind(&self) -> &'static str {
        SIGNAL
    }

    async fn send(&self, message: &Outgoing) -> Result<(), String> {
        self.deliver(&body(message)).await.map(drop)
    }

    async fn ask_approval(&self, action: &Action) -> Result<(), String> {
        let mut text = Styled::default();
        text.styled(Style::Bold, |t| t.push("Waiting for you"));
        text.push("\n");
        text.push(&action.summary);
        text.push("\n\n");
        text.styled(Style::Italic, |t| t.push(replies::APPROVAL_HINT));
        let stamps = self.deliver(&text).await?;
        self.remember(&stamps, replies::Target::Approval(action.id));
        Ok(())
    }

    async fn remind(&self, delivery: &Delivery, late: Option<&str>) -> Result<(), String> {
        let mut text = Styled::default();
        text.push("⏰ ");
        text.styled(Style::Bold, |t| t.push(&delivery.title));
        if let Some(late) = late {
            text.push("\n");
            text.styled(Style::Italic, |t| t.push(late));
        }
        text.push("\n\n");
        text.styled(Style::Italic, |t| t.push(replies::REMINDER_HINT));
        let stamps = self.deliver(&text).await?;
        self.remember(&stamps, replies::Target::Reminder(delivery.id));
        Ok(())
    }
}
