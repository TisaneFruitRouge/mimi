//! Everyone on Matrix other than the owner. Someone the owner lets ask the assistant
//! things (`access`) talks with it in a direct chat of their own: their messages go to
//! their own conversation, as them, and never to the owner. Anyone else is never heard
//! by the model: a reply in a chat the assistant opened for the owner is passed on to
//! the owner quoted as it is, and everything else is ignored.
//!
//! Everything here goes through [`Messenger`], so tests drive it with the fake.

use std::sync::Arc;

use async_trait::async_trait;
use mimi_protocol::{Action, Channel as HandleChannel, Delivery};
use uuid::Uuid;

use super::messenger::Messenger;
use super::rooms::{self, Kept, Why};
use super::{MATRIX, MatrixConfig, format, forward_text, reminder_prompt};
use crate::AppState;
use crate::access::{self, Guest, Line};
use crate::channels::replies::{self, Target};
use crate::channels::{self, Channel, Outgoing};

/// A text message from someone other than the owner (or from the owner outside their
/// own chat and groups), in a room the assistant is in.
#[derive(Debug, Clone)]
pub struct Inbound {
    pub room: String,
    pub sender: String,
    pub text: String,
    /// The event it replies to.
    pub quoted: Option<String>,
    /// Whether it came end-to-end encrypted.
    pub sealed: bool,
    /// The picture it carries, fetched only if the sender is someone the owner trusts.
    pub photo: Option<super::Photo>,
}

/// The direct chat with `sender` this room is, if the assistant keeps it as one: a chat
/// it opened to message them, or one they opened as someone the owner trusts.
async fn direct_with(state: &AppState, connection: Uuid, room: &str, sender: &str) -> Option<Kept> {
    rooms::kept_room(state, connection, room)
        .await
        // Addresses are compared without case: one typed into People as "@Maya:…" is
        // the same account as the "@maya:…" that writes.
        .filter(|k| {
            k.why == Why::Direct
                && k.user_id
                    .as_deref()
                    .is_some_and(|u| u.eq_ignore_ascii_case(sender))
        })
}

/// Handles a message from anyone but the owner.
pub async fn on_text(
    state: &Arc<AppState>,
    connection: Uuid,
    config: &MatrixConfig,
    messenger: Arc<dyn Messenger>,
    msg: Inbound,
) {
    if direct_with(state, connection, &msg.room, &msg.sender)
        .await
        .is_none()
    {
        // A room it doesn't keep: nobody there gives it instructions. (Mentions in its
        // groups are answered as a member of the group, before this: `group.rs`.)
        tracing::debug!(connection = %connection, room = %msg.room, "ignored a message outside the owner's chat");
        return;
    }
    // Once a chat is encrypted, only encrypted messages count: anything else could have
    // been made up by a server along the way.
    if !msg.sealed && messenger.encrypted(&msg.room).await {
        tracing::warn!(connection = %connection, room = %msg.room, sender = %msg.sender, "ignored an unencrypted message in an encrypted chat");
        return;
    }
    match access::find(state, HandleChannel::Matrix, &msg.sender).await {
        Some(guest) => on_guest_text(state, connection, messenger, guest, msg).await,
        None => forward(state, connection, config, messenger.as_ref(), &msg).await,
    }
}

/// Passes a reply from someone the assistant messaged on to the owner's chat, quoted as
/// it is: the model never sees it, so it can't give instructions.
async fn forward(
    state: &AppState,
    connection: Uuid,
    config: &MatrixConfig,
    messenger: &dyn Messenger,
    msg: &Inbound,
) {
    let Some(owner_room) = config.room_id.as_deref() else {
        tracing::warn!(connection = %connection, room = %msg.room, "a reply couldn't be passed on: the owner's chat is closed");
        return;
    };
    let name = match super::send::people_name(state, &msg.sender).await {
        Some(name) => Some(name),
        None => messenger
            .profile(&msg.sender)
            .await
            .ok()
            .flatten()
            .and_then(|n| super::send::clean_name(&n)),
    };
    let (body, html) = forward_text(name.as_deref(), &msg.sender, &msg.text);
    match messenger.send_html(owner_room, &body, &html).await {
        Ok(_) => {
            tracing::info!(connection = %connection, room = %msg.room, "passed a reply on to the owner")
        }
        Err(e) => {
            tracing::warn!(connection = %connection, room = %msg.room, "passing a reply on to the owner failed: {e}")
        }
    }
}

/// A trusted person's line: this chat, on this connection.
fn line_of(connection: Uuid, msg: &Inbound) -> Line {
    Line {
        app: MATRIX.to_owned(),
        connection,
        address: msg.sender.clone(),
        chat: msg.room.clone(),
    }
}

/// A message from someone the owner trusts: an answer to a prompt, `/new`, or a request
/// for the assistant, in their own conversation.
async fn on_guest_text(
    state: &Arc<AppState>,
    connection: Uuid,
    messenger: Arc<dyn Messenger>,
    guest: Guest,
    msg: Inbound,
) {
    let channel = GuestChannel::new(state, connection, messenger, &msg.room);
    let text = msg.text.trim().to_owned();
    if text.is_empty() && msg.photo.is_none() {
        return;
    }
    // A photo's caption is never an answer to a prompt or a command.
    if msg.photo.is_none()
        && let Some(answer) =
            replies::answer_text(state, channel.key, msg.quoted.as_deref(), &text).await
    {
        let _ = channel.send(&Outgoing::text(answer)).await;
        return;
    }
    let line = line_of(connection, &msg);
    if msg.photo.is_none() && text == "/new" {
        let reply = match access::line_conversation(state, &guest, &line, true).await {
            Some(_) => "Started a new conversation.",
            None => "Sorry, I couldn't start a new conversation.",
        };
        let _ = channel.send(&Outgoing::text(reply)).await;
        return;
    }
    let Some(conversation) = access::line_conversation(state, &guest, &line, false).await else {
        let _ = channel
            .send(&Outgoing::text("Sorry, I couldn't open our conversation."))
            .await;
        return;
    };
    tracing::info!(connection = %connection, room = %msg.room, "a message from someone the owner trusts");
    let mut photo = msg.photo;
    if let Some(recording) = photo.take_if(|p| p.voice) {
        match channel.messenger.download(recording).await {
            Ok(upload) => {
                channels::voice::hear(state, Arc::new(channel), conversation, upload.data, text)
            }
            Err(e) => {
                tracing::warn!(connection = %connection, room = %msg.room, "downloading a Matrix voice message failed: {e}");
                let _ = channel
                    .send(&Outgoing::text(
                        "I couldn't get that voice message from the server. Try sending it again.",
                    ))
                    .await;
            }
        }
        return;
    }
    let mut photos = Vec::new();
    if let Some(photo) = photo {
        match channel.messenger.download(photo).await {
            Ok(upload) => photos.push(upload),
            Err(e) => {
                tracing::warn!(connection = %connection, room = %msg.room, "downloading a Matrix photo failed: {e}");
                let _ = channel
                    .send(&Outgoing::text(
                        "I couldn't get that photo from the server. Try sending it again.",
                    ))
                    .await;
                if text.is_empty() {
                    return;
                }
            }
        }
    }
    // Photos wait for the words that come with them, as the owner's do; replies can
    // take a while, so this never holds up the next messages.
    channels::photos::deliver(state, Arc::new(channel), conversation, text, photos);
}

/// A reaction from anyone but the owner: it only ever answers a prompt the assistant
/// sent a trusted person in their own chat.
pub async fn on_reaction(
    state: &Arc<AppState>,
    connection: Uuid,
    messenger: Arc<dyn Messenger>,
    room: &str,
    sender: &str,
    target: &str,
    key: &str,
) {
    if direct_with(state, connection, room, sender).await.is_none()
        || access::find(state, HandleChannel::Matrix, sender)
            .await
            .is_none()
    {
        return;
    }
    let channel = GuestChannel::new(state, connection, messenger, room);
    if let Some(answer) = replies::answer_reaction(state, channel.key, target, key).await {
        let _ = channel.send(&Outgoing::text(answer)).await;
    }
}

/// An encrypted message from anyone but the owner that couldn't be read. Logged (never
/// its content), and said where it helps: to someone the owner trusts, in their own
/// chat; for a reply in a chat the assistant opened for the owner, to the owner, and to
/// the sender if they're in People.
pub async fn on_unreadable(
    state: &Arc<AppState>,
    connection: Uuid,
    config: &MatrixConfig,
    messenger: Arc<dyn Messenger>,
    room: &str,
    sender: &str,
) {
    tracing::warn!(connection = %connection, room = %room, sender = %sender, "couldn't decrypt a message");
    if direct_with(state, connection, room, sender).await.is_none() {
        return;
    }
    let sorry = Outgoing::text(super::UNREADABLE);
    if access::find(state, HandleChannel::Matrix, sender)
        .await
        .is_some()
    {
        let channel = GuestChannel::new(state, connection, messenger, room);
        let _ = channel.send(&sorry).await;
        return;
    }
    let name = super::send::people_name(state, sender).await;
    if name.is_some()
        && let Err(e) = messenger.send(room, &sorry.markdown).await
    {
        tracing::warn!(connection = %connection, room = %room, "telling someone their message couldn't be read failed: {e}");
    }
    if let Some(owner_room) = config.room_id.as_deref() {
        let who = match &name {
            Some(n) => format!("{n} ({sender})"),
            None => sender.to_owned(),
        };
        let body = format!(
            "💬 {who} replied, but I couldn't read it: its encryption keys didn't reach me."
        );
        let html = format!(
            "💬 <b>{}</b> replied, but I couldn't read it: its encryption keys didn't reach me.",
            format::escape_html(&who)
        );
        if let Err(e) = messenger.send_html(owner_room, &body, &html).await {
            tracing::warn!(connection = %connection, "telling the owner about an unreadable reply failed: {e}");
        }
    }
}

/// A trusted person's own chat with the assistant, as a [`Channel`]: replies, approval
/// prompts and their reminders. Its prompts are kept under the chat's own key
/// ([`replies::line`]), so only they can answer them, and they can answer nothing else.
pub struct GuestChannel {
    state: Arc<AppState>,
    messenger: Arc<dyn Messenger>,
    room: String,
    key: Uuid,
}

impl GuestChannel {
    fn new(
        state: &Arc<AppState>,
        connection: Uuid,
        messenger: Arc<dyn Messenger>,
        room: &str,
    ) -> Self {
        Self {
            state: state.clone(),
            messenger,
            room: room.to_owned(),
            key: replies::line(connection, room),
        }
    }

    async fn prompt(&self, body: String, html: String, target: Target) -> Result<(), String> {
        let event = self.messenger.send_html(&self.room, &body, &html).await?;
        self.state
            .connections
            .prompts
            .remember(self.key, event, target);
        Ok(())
    }
}

#[async_trait]
impl Channel for GuestChannel {
    fn kind(&self) -> &'static str {
        MATRIX
    }

    async fn send(&self, message: &Outgoing) -> Result<(), String> {
        for part in format::outgoing(message) {
            self.messenger
                .send_html(&self.room, &part.body, &part.html)
                .await?;
        }
        self.messenger.typing(&self.room, false).await;
        Ok(())
    }

    async fn typing(&self) {
        self.messenger.typing(&self.room, true).await;
    }

    async fn ask_approval(&self, action: &Action) -> Result<(), String> {
        let (body, html) = super::approval_prompt(action);
        self.prompt(body, html, Target::Approval(action.id)).await
    }

    async fn remind(&self, delivery: &Delivery, late: Option<&str>) -> Result<(), String> {
        let (body, html) = reminder_prompt(delivery, late);
        self.prompt(body, html, Target::Reminder(delivery.id)).await
    }
}

/// A trusted person's line, as a channel, while its connection runs.
pub fn channel(state: &Arc<AppState>, line: &Line) -> Option<Arc<dyn Channel>> {
    let messenger = state.connections.matrix.messenger(line.connection)?;
    Some(Arc::new(GuestChannel::new(
        state,
        line.connection,
        messenger,
        &line.chat,
    )))
}

/// When access is taken away: the chats they opened with the assistant are left (the
/// ones it opened for the owner stay, and replies there are passed on as before).
pub async fn leave_their_chats(state: &AppState, lines: &[Line]) {
    for line in lines.iter().filter(|l| l.app == MATRIX) {
        let Some(messenger) = state.connections.matrix.messenger(line.connection) else {
            continue;
        };
        let me = messenger.user_id();
        let opened_by_them = messenger
            .joined()
            .await
            .into_iter()
            .find(|r| r.id == line.chat)
            .is_some_and(|r| r.creator.as_deref().is_some_and(|c| c != me));
        if opened_by_them {
            let _ = rooms::forget(state, line.connection, &line.chat).await;
            messenger.leave(&line.chat).await;
        }
    }
}

/// The assistant's Matrix accounts that are paired, for "writes to @mimi:example.org".
pub async fn assistant_addresses(state: &AppState) -> Vec<String> {
    let mut out: Vec<String> = super::store::list(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.integration == MATRIX)
        .filter_map(|r| serde_json::from_value::<MatrixConfig>(r.config).ok())
        .filter(|c| c.owner.is_some() && !c.signed_out)
        .map(|c| c.user_id)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// What the owner calls themselves on Matrix, for "I've asked Vincent".
pub async fn owner_name(state: &AppState) -> Option<String> {
    super::store::list(&state.db)
        .await
        .ok()?
        .into_iter()
        .filter(|r| r.integration == MATRIX)
        .filter_map(|r| serde_json::from_value::<MatrixConfig>(r.config).ok())
        .find_map(|c| c.owner_name.and_then(|n| super::send::clean_name(&n)))
}
