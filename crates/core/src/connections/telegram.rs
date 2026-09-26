//! Telegram: a bot the user creates with @BotFather becomes their private line to the
//! assistant. Long polling means nothing has to reach this machine from outside, and a
//! one-time pairing code ties the bot to its owner; everyone else is ignored.

use std::sync::Arc;
use std::time::Duration;

use hearth_protocol::{ConnectionStatus, Event, MessageStatus};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::store;
use crate::AppState;

pub const TELEGRAM: &str = "telegram";

/// Overrides the Bot API address, for tests.
pub const API_ENV: &str = "HEARTH_TELEGRAM_API";

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
        Self {
            http,
            base: format!("{}/bot{}", api.trim_end_matches('/'), token.trim()),
        }
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
        for chunk in chunks(html, 4000) {
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

    /// Asks the owner to approve an action, with buttons.
    async fn ask_approval(
        &self,
        chat_id: i64,
        action: &hearth_protocol::Action,
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
            let Some(text) = message.text else { continue };
            if message.chat.kind != "private" {
                continue;
            }
            handle(&state, id, &bot, &mut config, message.chat, text).await;
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
                let name = crate::settings::load(&state.db)
                    .await
                    .map(|s| s.assistant_name)
                    .unwrap_or_else(|_| "Hearth".to_owned());
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
            if text == "/new" {
                config.conversation_id = None;
                let _ = bot.send(owner, "Started a new conversation.").await;
                return;
            }
            if text.starts_with("/start") {
                let _ = bot.send(owner, "I'm here. What can I do for you?").await;
                return;
            }
            let conversation = ensure_conversation(state, config).await;
            let Some(conversation) = conversation else {
                let _ = bot
                    .send(owner, "Sorry, I couldn't open our conversation.")
                    .await;
                return;
            };
            // Replies can take a while; don't hold up polling.
            let (state, bot) = (state.clone(), bot.clone());
            tokio::spawn(async move { reply(&state, &bot, owner, conversation, text).await });
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

async fn ensure_conversation(state: &Arc<AppState>, config: &mut TelegramConfig) -> Option<Uuid> {
    if let Some(id) = config.conversation_id
        && crate::chat::store::get_conversation(&state.db, id)
            .await
            .ok()
            .flatten()
            .is_some()
    {
        return Some(id);
    }
    let conversation = crate::chat::new_conversation(Some("Telegram".to_owned()));
    crate::chat::store::upsert_conversation(&state.db, conversation.clone())
        .await
        .ok()?;
    state.events.publish(Event::ConversationUpdated {
        conversation: conversation.clone(),
    });
    config.conversation_id = Some(conversation.id);
    Some(conversation.id)
}

/// Sends the user's message to the assistant and relays the answer.
async fn reply(state: &Arc<AppState>, bot: &Bot, chat_id: i64, conversation: Uuid, text: String) {
    let mut events = state.events.subscribe();
    bot.typing(chat_id).await;
    let sent = match crate::chat::send(state.clone(), conversation, text, None).await {
        Ok(sent) => sent,
        Err(e) => {
            let _ = bot.send(chat_id, &escape(e.message())).await;
            return;
        }
    };
    let assistant = sent.assistant_message.id;
    let mut typing = tokio::time::interval(Duration::from_secs(4));
    let deadline = tokio::time::sleep(Duration::from_secs(15 * 60));
    tokio::pin!(deadline);
    let mut announced = std::collections::HashSet::new();
    let message = loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(Event::MessageUpdated { message }) if message.id == assistant => {
                    for action in &message.actions {
                        if action.status == hearth_protocol::ActionStatus::PendingApproval && announced.insert(action.id) {
                            let _ = bot.ask_approval(chat_id, action).await;
                        }
                    }
                    if message.status != MessageStatus::Streaming {
                        break message;
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            },
            _ = typing.tick() => bot.typing(chat_id).await,
            _ = &mut deadline => return,
        }
    };
    let mut text = match message.status {
        MessageStatus::Error => format!(
            "Sorry, something went wrong: {}",
            escape(message.error.as_deref().unwrap_or("unknown error"))
        ),
        MessageStatus::Cancelled if message.content.is_empty() => "(Stopped.)".to_owned(),
        _ if message.content.is_empty() => "(No answer.)".to_owned(),
        _ => markdown_to_html(&message.content),
    };
    // Things the user has to finish themselves, like saving an event in Google Calendar.
    for action in &message.actions {
        if let Some(url) = action.output.as_ref().and_then(|o| o["open_url"].as_str()) {
            text.push_str(&format!(
                "\n\n<a href=\"{}\">Open in Google Calendar to save it</a>",
                escape(url).replace('"', "&quot;")
            ));
        }
    }
    if let Err(e) = bot.send(chat_id, &text).await {
        tracing::warn!("sending a reply to Telegram failed: {e}");
    }
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

/// Splits text into pieces Telegram accepts, preferring line breaks.
fn chunks(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        if current.chars().count() + line.chars().count() > max && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        out.push(current);
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
            bot_username: "my_hearth_bot".into(),
            pairing_code: Some("123456".into()),
            owner_chat_id: None,
            owner_name: None,
            conversation_id: None,
            offset: 0,
        };
        assert_eq!(
            start_link(&config).as_deref(),
            Some("https://t.me/my_hearth_bot?start=123456")
        );
        assert_eq!(describe(&config).0, ConnectionStatus::NeedsAction);
    }

    #[test]
    fn long_replies_are_split() {
        let text = "a".repeat(3000) + "\n" + &"b".repeat(3000);
        let parts = chunks(&text, 4000);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.chars().count() <= 4000));
    }
}
