//! Telegram: a bot the user creates with @BotFather becomes their private line to the
//! assistant. Long polling means nothing has to reach this machine from outside, and a
//! one-time pairing code ties the bot to its owner; everyone else is ignored.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mimi_protocol::{Action, ConnectionStatus, Delivery};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::store;
use crate::AppState;
use crate::channels::{self, Channel, Outgoing, replies};

pub const TELEGRAM: &str = "telegram";

/// Overrides the Bot API address, for tests.
pub const API_ENV: &str = "MIMI_TELEGRAM_API";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// A credential: never log it.
    pub bot_token: String,
    pub bot_username: String,
    /// Waiting for the owner to press Start with this code.
    pub pairing_code: Option<String>,
    pub owner_chat_id: Option<i64>,
    pub owner_name: Option<String>,
    /// Where messages from Telegram go.
    pub conversation_id: Option<Uuid>,
    /// Last Telegram update handled.
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum TgError {
    #[error("Telegram didn't accept the bot token. Copy it again from @BotFather.")]
    Unauthorized,
    #[error("Couldn't reach Telegram.")]
    Unreachable,
    #[error("Telegram said: {0}")]
    Api(String),
}

#[derive(Clone)]
pub struct Bot {
    http: reqwest::Client,
    base: String,
    /// Where files are downloaded from. Contains the token too: never log it.
    files: String,
}

#[derive(Debug, Deserialize)]
pub struct BotUser {
    pub username: Option<String>,
    pub first_name: String,
}

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    message: Option<TgMessage>,
    callback_query: Option<CallbackQuery>,
}

/// A tap on one of our inline buttons.
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    id: String,
    from: TgUser,
    message: Option<ButtonMessage>,
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgUser {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct ButtonMessage {
    message_id: i64,
    chat: Chat,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgMessage {
    chat: Chat,
    text: Option<String>,
    /// A photo: the same picture in several sizes, smallest first.
    photo: Option<Vec<PhotoSize>>,
    /// A file, which may be a picture sent uncompressed.
    document: Option<Document>,
    /// A voice message.
    voice: Option<Sound>,
    /// A sound file (a forwarded recording, a voice memo).
    audio: Option<Sound>,
    /// The words sent with a photo or file.
    caption: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Sound {
    file_id: String,
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PhotoSize {
    file_id: String,
    width: u32,
    height: u32,
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Document {
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
    file_size: Option<u64>,
}

/// What came with the words, to download once it's known to be from the owner.
#[derive(Debug, Default)]
struct Media {
    picture: Option<Picture>,
    /// The file id of a voice message or sound file.
    voice: Option<String>,
}

impl Media {
    fn is_empty(&self) -> bool {
        self.picture.is_none() && self.voice.is_none()
    }
}

/// A picture in a message, to download once it's known to be from the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Picture {
    file_id: String,
    name: Option<String>,
    mime: Option<String>,
}

impl TgMessage {
    /// The voice message it carries, or a sound file (within what may be downloaded).
    fn voice(&self) -> Option<String> {
        let limit = crate::attachments::MAX_UPLOAD_BYTES as u64;
        self.voice
            .as_ref()
            .or(self.audio.as_ref())
            .filter(|s| s.file_size.is_none_or(|n| n <= limit))
            .map(|s| s.file_id.clone())
    }

    /// The picture it carries: the largest size of a photo (within what may be
    /// downloaded), or a file that says it's a picture.
    fn picture(&self) -> Option<Picture> {
        let limit = crate::attachments::MAX_UPLOAD_BYTES as u64;
        if let Some(sizes) = &self.photo {
            return sizes
                .iter()
                .filter(|s| s.file_size.is_none_or(|n| n <= limit))
                .max_by_key(|s| u64::from(s.width) * u64::from(s.height))
                .map(|s| Picture {
                    file_id: s.file_id.clone(),
                    name: None,
                    mime: Some("image/jpeg".to_owned()),
                });
        }
        self.document
            .as_ref()
            .filter(|d| {
                d.mime_type
                    .as_deref()
                    .is_some_and(|m| m.starts_with("image/"))
                    && d.file_size.is_none_or(|n| n <= limit)
            })
            .map(|d| Picture {
                file_id: d.file_id.clone(),
                name: d.file_name.clone(),
                mime: d.mime_type.clone(),
            })
    }
}

#[derive(Debug, Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
    first_name: Option<String>,
    username: Option<String>,
}

/// The Bot API address: Telegram's, unless overridden for tests.
pub fn default_api() -> String {
    std::env::var(API_ENV).unwrap_or_else(|_| "https://api.telegram.org".to_owned())
}

impl Bot {
    pub fn new(http: reqwest::Client, api: &str, token: &str) -> Self {
        let api = api.trim_end_matches('/');
        Self {
            http,
            base: format!("{api}/bot{}", token.trim()),
            files: format!("{api}/file/bot{}", token.trim()),
        }
    }

    /// Downloads a file the owner sent (at most [`crate::attachments::MAX_UPLOAD_BYTES`]).
    async fn download(&self, file_id: &str) -> Result<Vec<u8>, TgError> {
        #[derive(Deserialize)]
        struct File {
            file_path: Option<String>,
            file_size: Option<u64>,
        }
        let file: File = self
            .call(
                "getFile",
                json!({ "file_id": file_id }),
                Duration::from_secs(20),
            )
            .await?;
        let too_large = || TgError::Api("the file is too large".to_owned());
        if file
            .file_size
            .is_some_and(|n| n > crate::attachments::MAX_UPLOAD_BYTES as u64)
        {
            return Err(too_large());
        }
        let path = file
            .file_path
            .ok_or_else(|| TgError::Api("no file".to_owned()))?;
        // As with every call: errors without their text, which would hold the token.
        let res = self
            .http
            .get(format!("{}/{}", self.files, path.trim_start_matches('/')))
            .timeout(Duration::from_secs(90))
            .send()
            .await
            .map_err(|_| TgError::Unreachable)?;
        if !res.status().is_success() {
            return Err(TgError::Api(format!(
                "download failed ({})",
                res.status().as_u16()
            )));
        }
        crate::attachments::read_capped(res)
            .await
            .ok_or_else(too_large)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<T, TgError> {
        #[derive(Deserialize)]
        struct Reply<T> {
            ok: bool,
            result: Option<T>,
            description: Option<String>,
            error_code: Option<u16>,
        }
        // Errors are mapped without their text: reqwest errors include the URL, which
        // contains the bot token.
        let res = self
            .http
            .post(format!("{}/{method}", self.base))
            .json(&body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| TgError::Unreachable)?;
        let reply: Reply<T> = res.json().await.map_err(|_| TgError::Unreachable)?;
        match reply {
            Reply {
                ok: true,
                result: Some(r),
                ..
            } => Ok(r),
            Reply {
                error_code: Some(401 | 404),
                ..
            } => Err(TgError::Unauthorized),
            Reply { description, .. } => Err(TgError::Api(
                description.unwrap_or_else(|| "unknown error".to_owned()),
            )),
        }
    }

    pub async fn get_me(&self) -> Result<BotUser, TgError> {
        self.call("getMe", json!({}), Duration::from_secs(15)).await
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<Update>, TgError> {
        self.call(
            "getUpdates",
            json!({ "offset": offset + 1, "timeout": 30, "allowed_updates": ["message", "callback_query"] }),
            Duration::from_secs(45),
        )
        .await
    }

    pub async fn send(&self, chat_id: i64, html: &str) -> Result<(), TgError> {
        for chunk in channels::chunks(html, 4000) {
            let sent: Result<serde_json::Value, _> = self
                .call(
                    "sendMessage",
                    json!({ "chat_id": chat_id, "text": chunk, "parse_mode": "HTML",
                            "link_preview_options": { "is_disabled": true } }),
                    Duration::from_secs(20),
                )
                .await;
            // Formatting Telegram rejects shouldn't lose the reply: resend as plain text.
            if let Err(TgError::Api(_)) = sent {
                self.call::<serde_json::Value>(
                    "sendMessage",
                    json!({ "chat_id": chat_id, "text": strip_tags(&chunk) }),
                    Duration::from_secs(20),
                )
                .await?;
            } else {
                sent?;
            }
        }
        Ok(())
    }

    /// Sends a message with a row of inline buttons: (label, callback data).
    pub(crate) async fn send_with_buttons(
        &self,
        chat_id: i64,
        html: &str,
        buttons: &[(&str, String)],
    ) -> Result<(), TgError> {
        let row: Vec<serde_json::Value> = buttons
            .iter()
            .map(|(text, data)| json!({ "text": text, "callback_data": data }))
            .collect();
        self.call::<serde_json::Value>(
            "sendMessage",
            json!({
                "chat_id": chat_id,
                "text": html,
                "parse_mode": "HTML",
                "link_preview_options": { "is_disabled": true },
                "reply_markup": { "inline_keyboard": [row] }
            }),
            Duration::from_secs(20),
        )
        .await
        .map(drop)
    }

    /// Asks the owner to approve an action, with buttons.
    pub(crate) async fn ask_approval(
        &self,
        chat_id: i64,
        action: &mimi_protocol::Action,
    ) -> Result<(), TgError> {
        let id = action.id;
        self.call::<serde_json::Value>(
            "sendMessage",
            json!({
                "chat_id": chat_id,
                "text": format!("<b>Waiting for you</b>\n{}", escape(&action.summary)),
                "parse_mode": "HTML",
                "reply_markup": { "inline_keyboard": [[
                    { "text": "Approve", "callback_data": format!("approve:{id}") },
                    { "text": "Don't", "callback_data": format!("reject:{id}") }
                ]] }
            }),
            Duration::from_secs(20),
        )
        .await
        .map(drop)
    }

    /// Answers a button tap and replaces the buttons with the outcome.
    async fn settle(&self, query: &CallbackQuery, toast: &str, outcome: Option<&str>) {
        let _: Result<bool, _> = self
            .call(
                "answerCallbackQuery",
                json!({ "callback_query_id": query.id, "text": toast }),
                Duration::from_secs(10),
            )
            .await;
        if let (Some(outcome), Some(message)) = (outcome, &query.message) {
            let original = message
                .text
                .as_deref()
                .unwrap_or("")
                .trim_start_matches("Waiting for you")
                .trim();
            let _: Result<serde_json::Value, _> = self
                .call(
                    "editMessageText",
                    json!({
                        "chat_id": message.chat.id,
                        "message_id": message.message_id,
                        "text": format!("{outcome}\n{}", escape(original)),
                        "parse_mode": "HTML"
                    }),
                    Duration::from_secs(10),
                )
                .await;
        }
    }

    /// Sends a voice message (Ogg/Opus).
    async fn send_voice(
        &self,
        chat_id: i64,
        note: &crate::voice::audio::VoiceNote,
    ) -> Result<(), TgError> {
        let file = reqwest::multipart::Part::bytes(note.ogg.clone())
            .file_name("voice.ogg")
            .mime_str("audio/ogg")
            .map_err(|_| TgError::Api("bad voice message".to_owned()))?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .text("duration", note.duration_ms.div_ceil(1000).to_string())
            .part("voice", file);
        // As with every call: errors without their text, which would hold the token.
        let res = self
            .http
            .post(format!("{}/sendVoice", self.base))
            .multipart(form)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|_| TgError::Unreachable)?;
        if res.status().is_success() {
            Ok(())
        } else {
            Err(TgError::Api(format!(
                "sending a voice message failed ({})",
                res.status().as_u16()
            )))
        }
    }

    async fn typing(&self, chat_id: i64) {
        let _: Result<bool, _> = self
            .call(
                "sendChatAction",
                json!({ "chat_id": chat_id, "action": "typing" }),
                Duration::from_secs(10),
            )
            .await;
    }
}

/// A fresh six-digit pairing code.
pub fn pairing_code() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).expect("OS randomness");
    format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000)
}

pub fn start_link(config: &TelegramConfig) -> Option<String> {
    config
        .pairing_code
        .as_ref()
        .map(|code| format!("https://t.me/{}?start={code}", config.bot_username))
}

/// The status line a Telegram connection shows.
pub fn describe(config: &TelegramConfig) -> (ConnectionStatus, String, Option<String>) {
    match (&config.owner_name, config.owner_chat_id) {
        (Some(name), Some(_)) => (
            ConnectionStatus::Ok,
            format!("Talking with {name} as @{}", config.bot_username),
            None,
        ),
        (None, Some(_)) => (
            ConnectionStatus::Ok,
            format!("Connected as @{}", config.bot_username),
            None,
        ),
        _ => (
            ConnectionStatus::NeedsAction,
            format!("Open @{} in Telegram and press Start", config.bot_username),
            start_link(config),
        ),
    }
}

/// Polls the bot until cancelled or the token stops working.
pub async fn run(state: Arc<AppState>, id: Uuid, cancel: CancellationToken) {
    let mut backoff = Duration::from_secs(2);
    loop {
        let Ok(Some(row)) = store::get(&state.db, id).await else {
            return;
        };
        let Ok(mut config) = serde_json::from_value::<TelegramConfig>(row.config.clone()) else {
            return;
        };
        let bot = Bot::new(
            state.http.clone(),
            &state.connections.telegram_api(),
            &config.bot_token,
        );
        let updates = tokio::select! {
            u = bot.get_updates(config.offset) => u,
            _ = cancel.cancelled() => return,
        };
        let updates = match updates {
            Ok(u) => {
                if backoff > Duration::from_secs(2) {
                    let (status, detail, url) = describe(&config);
                    state
                        .connections
                        .set_status(&state, id, status, detail, url)
                        .await;
                }
                backoff = Duration::from_secs(2);
                u
            }
            Err(TgError::Unauthorized) => {
                state
                    .connections
                    .set_status(
                        &state,
                        id,
                        ConnectionStatus::Error,
                        "The bot token stopped working. Remove this connection and add the bot again.".to_owned(),
                        None,
                    )
                    .await;
                return;
            }
            Err(_) => {
                state
                    .connections
                    .set_status(
                        &state,
                        id,
                        ConnectionStatus::Error,
                        "Can't reach Telegram right now. Retrying…".to_owned(),
                        None,
                    )
                    .await;
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {},
                    _ = cancel.cancelled() => return,
                }
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        if updates.is_empty() {
            continue;
        }
        for update in updates {
            config.offset = config.offset.max(update.update_id);
            if let Some(query) = update.callback_query {
                on_button(&state, &bot, &config, query).await;
                continue;
            }
            let Some(message) = update.message else {
                continue;
            };
            if message.chat.kind != "private" {
                continue;
            }
            let media = Media {
                picture: message.picture(),
                voice: message.voice(),
            };
            let Some(text) = message
                .text
                .or(message.caption)
                .or((!media.is_empty()).then(String::new))
            else {
                continue;
            };
            handle(&state, id, &bot, &mut config, message.chat, text, media).await;
        }
        let mut row = row;
        row.config = serde_json::to_value(&config).expect("config serializes");
        let _ = store::upsert(&state.db, row).await;
    }
}

async fn handle(
    state: &Arc<AppState>,
    id: Uuid,
    bot: &Bot,
    config: &mut TelegramConfig,
    chat: Chat,
    text: String,
    media: Media,
) {
    let text = text.trim().to_owned();
    match config.owner_chat_id {
        None => {
            // Only the person holding the pairing code can claim the bot.
            let code = text.strip_prefix("/start").map(str::trim);
            if code.is_some() && code == config.pairing_code.as_deref() {
                config.owner_chat_id = Some(chat.id);
                config.owner_name = chat.first_name.clone().or(chat.username.clone());
                config.pairing_code = None;
                let (status, detail, url) = describe(config);
                state
                    .connections
                    .set_status(state, id, status, detail, url)
                    .await;
                let name = channels::assistant_name(state).await;
                let _ = bot
                    .send(
                        chat.id,
                        &format!(
                            "Hi{}! I'm {}, your assistant. Message me here any time.\n\n\
                             <i>Telegram messages aren't end-to-end encrypted: Telegram can read what you send here.</i>",
                            config.owner_name.as_ref().map(|n| format!(" {}", escape(n))).unwrap_or_default(),
                            escape(&name)
                        ),
                    )
                    .await;
            }
        }
        Some(owner) if owner == chat.id => {
            if media.is_empty() && text == "/new" {
                config.conversation_id = None;
                let _ = bot.send(owner, "Started a new conversation.").await;
                return;
            }
            if media.is_empty() && text.starts_with("/start") {
                let _ = bot.send(owner, "I'm here. What can I do for you?").await;
                return;
            }
            let conversation =
                channels::ensure_conversation(state, config.conversation_id, "Telegram").await;
            config.conversation_id = conversation;
            let Some(conversation) = conversation else {
                let _ = bot
                    .send(owner, "Sorry, I couldn't open our conversation.")
                    .await;
                return;
            };
            let channel = Arc::new(TelegramChannel {
                bot: bot.clone(),
                chat: owner,
            });
            // Only now that it's known to be the owner's is anything downloaded.
            if let Some(voice) = media.voice {
                match bot.download(&voice).await {
                    Ok(audio) => channels::voice::hear(state, channel, conversation, audio, text),
                    Err(e) => {
                        tracing::warn!("downloading a voice message from Telegram failed: {e}");
                        let _ = bot
                            .send(
                                owner,
                                "I couldn't get that voice message from Telegram. Try sending it again.",
                            )
                            .await;
                    }
                }
                return;
            }
            let mut photos = Vec::new();
            if let Some(picture) = media.picture {
                match bot.download(&picture.file_id).await {
                    Ok(data) => photos.push(crate::attachments::Upload::new(
                        data,
                        picture.name,
                        picture.mime,
                    )),
                    Err(e) => {
                        tracing::warn!("downloading a photo from Telegram failed: {e}");
                        let _ = bot
                            .send(
                                owner,
                                "I couldn't get that photo from Telegram. Try sending it again.",
                            )
                            .await;
                        if text.is_empty() {
                            return;
                        }
                    }
                }
            }
            // Replies can take a while; this returns at once.
            channels::photos::deliver(state, channel, conversation, text, photos);
        }
        // Anyone else: ignore silently. The bot is private.
        Some(_) => {}
    }
}

/// Approve / Don't taps. Only the owner's taps count.
async fn on_button(state: &AppState, bot: &Bot, config: &TelegramConfig, query: CallbackQuery) {
    if Some(query.from.id) != config.owner_chat_id {
        bot.settle(&query, "This assistant is private.", None).await;
        return;
    }
    let parsed = query
        .data
        .as_deref()
        .and_then(|d| d.split_once(':'))
        .and_then(|(verb, id)| Some((verb.to_owned(), id.parse::<Uuid>().ok()?)));
    let Some((verb, id)) = parsed else {
        bot.settle(&query, "Unknown button.", None).await;
        return;
    };
    // Reminder buttons: Done / Snooze.
    let snooze = match verb.as_str() {
        "snooze" => Some(10),
        "snooze60" => Some(60),
        _ => None,
    };
    if verb == "done" || snooze.is_some() {
        let handled = match snooze {
            Some(minutes) => crate::schedule::snooze(state, id, minutes).await,
            None => crate::schedule::mark_done(state, id).await,
        };
        let (toast, outcome) = match (handled, snooze) {
            (false, _) => (
                "This was already handled.",
                "<i>Already handled</i>".to_owned(),
            ),
            (true, Some(m)) => (
                "Snoozed",
                format!("💤 <b>Snoozed for {}</b>", replies::snooze_label(m)),
            ),
            (true, None) => ("Done", "✅ <b>Done</b>".to_owned()),
        };
        bot.settle(&query, toast, Some(&outcome)).await;
        return;
    }
    let (decision, toast, outcome) = match verb.as_str() {
        "approve" => (
            crate::tools::Decision::Approve(None),
            "Approved",
            "✅ <b>Approved</b>",
        ),
        _ => (
            crate::tools::Decision::Reject,
            "Declined",
            "✖️ <b>Declined</b>",
        ),
    };
    if state.approvals.decide(id, decision) {
        bot.settle(&query, toast, Some(outcome)).await;
    } else {
        bot.settle(
            &query,
            "This was already handled.",
            Some("<i>Already handled</i>"),
        )
        .await;
    }
}

/// Every paired Telegram bot, for messages Mimi sends on its own (reminders, routine
/// results).
pub async fn owners(state: &AppState) -> Vec<Arc<dyn Channel>> {
    let Ok(rows) = super::store::list(&state.db).await else {
        return Vec::new();
    };
    rows.into_iter()
        .filter(|r| r.integration == TELEGRAM)
        .filter_map(|r| serde_json::from_value::<TelegramConfig>(r.config).ok())
        .filter_map(|c| {
            let chat = c.owner_chat_id?;
            let channel: Arc<dyn Channel> = Arc::new(TelegramChannel {
                bot: Bot::new(
                    state.http.clone(),
                    &state.connections.telegram_api(),
                    &c.bot_token,
                ),
                chat,
            });
            Some(channel)
        })
        .collect()
}

/// A paired bot's chat with its owner.
pub struct TelegramChannel {
    bot: Bot,
    chat: i64,
}

#[async_trait]
impl Channel for TelegramChannel {
    fn kind(&self) -> &'static str {
        TELEGRAM
    }

    async fn send(&self, message: &Outgoing) -> Result<(), String> {
        self.bot
            .send(self.chat, &to_html(message))
            .await
            .map_err(|e| e.to_string())
    }

    async fn typing(&self) {
        self.bot.typing(self.chat).await;
    }

    fn sends_voice(&self) -> bool {
        true
    }

    async fn send_voice(&self, note: &crate::voice::audio::VoiceNote) -> Result<(), String> {
        self.bot
            .send_voice(self.chat, note)
            .await
            .map_err(|e| e.to_string())
    }

    async fn ask_approval(&self, action: &Action) -> Result<(), String> {
        self.bot
            .ask_approval(self.chat, action)
            .await
            .map_err(|e| e.to_string())
    }

    async fn remind(&self, delivery: &Delivery, late: Option<&str>) -> Result<(), String> {
        let html = format!(
            "⏰ <b>{}</b>{}",
            escape(&delivery.title),
            late.map(|l| format!("\n<i>{}</i>", escape(l)))
                .unwrap_or_default()
        );
        let id = delivery.id;
        let buttons = [
            ("Done", format!("done:{id}")),
            ("Snooze 10 min", format!("snooze:{id}")),
            ("1 hour", format!("snooze60:{id}")),
        ];
        self.bot
            .send_with_buttons(self.chat, &html, &buttons)
            .await
            .map_err(|e| e.to_string())
    }
}

/// A message as Telegram HTML.
fn to_html(message: &Outgoing) -> String {
    let mut html = String::new();
    if let Some(title) = &message.title {
        html.push_str(&format!("<b>{}</b>\n\n", escape(title)));
    }
    html.push_str(&markdown_to_html(&message.markdown));
    for link in &message.links {
        html.push_str(&format!(
            "\n\n<a href=\"{}\">{}</a>",
            escape(&link.url).replace('"', "&quot;"),
            escape(&link.label)
        ));
    }
    html
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Converts the Markdown models write into the small HTML subset Telegram supports.
pub fn markdown_to_html(md: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            out.push_str(if in_code_block { "</pre>\n" } else { "<pre>" });
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            out.push_str(&escape(line));
            out.push('\n');
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(heading) = trimmed.strip_prefix('#') {
            out.push_str(&format!(
                "<b>{}</b>\n",
                inline(heading.trim_start_matches('#').trim())
            ));
            continue;
        }
        let line = match trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            Some(item) => format!("• {}", inline(item)),
            None => inline(line),
        };
        out.push_str(&line);
        out.push('\n');
    }
    if in_code_block {
        out.push_str("</pre>");
    }
    out.trim_end().to_owned()
}

/// Inline Markdown: `code`, **bold**, *italic*, [links](url).
fn inline(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let find = |from: usize, pat: &str| -> Option<usize> {
        let rest: String = chars[from..].iter().collect();
        rest.find(pat)
            .map(|byte| from + rest[..byte].chars().count())
    };
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        if chars[i] == '`'
            && let Some(end) = find(i + 1, "`")
        {
            out.push_str(&format!(
                "<code>{}</code>",
                escape(&chars[i + 1..end].iter().collect::<String>())
            ));
            i = end + 1;
        } else if rest.starts_with("**")
            && let Some(end) = find(i + 2, "**")
        {
            out.push_str(&format!(
                "<b>{}</b>",
                inline(&chars[i + 2..end].iter().collect::<String>())
            ));
            i = end + 2;
        } else if chars[i] == '['
            && let Some(close) = find(i + 1, "](")
            && let Some(end) = find(close + 2, ")")
        {
            let label: String = chars[i + 1..close].iter().collect();
            let url: String = chars[close + 2..end].iter().collect();
            if url.starts_with("https://") || url.starts_with("http://") {
                out.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    escape(&url).replace('"', "&quot;"),
                    escape(&label)
                ));
            } else {
                out.push_str(&escape(&label));
            }
            i = end + 1;
        } else if chars[i] == '*'
            && chars.get(i + 1).is_some_and(|c| !c.is_whitespace())
            && let Some(end) = find(i + 1, "*")
        {
            out.push_str(&format!(
                "<i>{}</i>",
                escape(&chars[i + 1..end].iter().collect::<String>())
            ));
            i = end + 1;
        } else {
            out.push_str(&escape(&chars[i].to_string()));
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_telegram_html() {
        let md = "# Plan\n\n**Monday**: call <Sam> & book `table`\n- one *thing*\n- [docs](https://example.com)\n```\nlet x = 1 < 2;\n```";
        assert_eq!(
            markdown_to_html(md),
            "<b>Plan</b>\n\n<b>Monday</b>: call &lt;Sam&gt; &amp; book <code>table</code>\n• one <i>thing</i>\n• <a href=\"https://example.com\">docs</a>\n<pre>let x = 1 &lt; 2;\n</pre>"
        );
    }

    #[test]
    fn pairing_codes_and_links() {
        let code = pairing_code();
        assert_eq!(code.len(), 6);
        let config = TelegramConfig {
            bot_token: "t".into(),
            bot_username: "my_mimi_bot".into(),
            pairing_code: Some("123456".into()),
            owner_chat_id: None,
            owner_name: None,
            conversation_id: None,
            offset: 0,
        };
        assert_eq!(
            start_link(&config).as_deref(),
            Some("https://t.me/my_mimi_bot?start=123456")
        );
        assert_eq!(describe(&config).0, ConnectionStatus::NeedsAction);
    }
}
