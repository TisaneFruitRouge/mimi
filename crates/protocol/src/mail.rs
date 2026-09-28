use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// How a mail server connection is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailSecurity {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (IMAP 143, SMTP 587).
    StartTls,
    /// No encryption. Only accepted for servers on this computer (e.g. Proton Bridge).
    Plain,
}

/// Where an account's mail lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailServers {
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: MailSecurity,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: MailSecurity,
    /// Login name, when it isn't the email address.
    #[serde(default)]
    pub username: Option<String>,
}

/// A mail service the user can pick when connecting.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailPreset {
    pub id: String,
    pub name: String,
    /// How to get the app password, in plain language.
    pub help: String,
    pub help_url: Option<String>,
    /// `None` means the user types the server details ("Other").
    pub servers: Option<MailServers>,
    /// False for services that need a sign-in Mimi doesn't support yet.
    pub supported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailCategory {
    /// Someone is waiting for the user's answer.
    NeedsReply,
    /// Worth reading soon, no reply expected.
    Important,
    /// Everything else: newsletters, notifications, receipts…
    Other,
}

/// What part of the mail to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailBox {
    NeedsReply,
    Important,
    Other,
    Inbox,
    Sent,
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailAddress {
    pub name: Option<String>,
    pub email: String,
}

/// One conversation, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailThread {
    #[ts(type = "number")]
    pub id: i64,
    pub connection_id: Uuid,
    /// Which of the user's addresses its latest incoming message arrived at.
    pub received_on: Option<String>,
    /// A message in it contains instructions aimed at an AI assistant.
    pub suspicious: bool,
    /// The smart folders it's in.
    #[ts(type = "number[]")]
    pub folders: Vec<i64>,
    pub subject: String,
    /// Everyone in the conversation except the user, most recent first.
    pub participants: Vec<MailAddress>,
    #[ts(type = "number")]
    pub last_at: i64,
    pub message_count: u32,
    pub unread: bool,
    pub flagged: bool,
    /// `None` until it has been sorted.
    pub category: Option<MailCategory>,
    /// One line written by the assistant.
    pub summary: Option<String>,
    /// The start of the latest message.
    pub snippet: String,
    /// Newsletters, notifications and other automatic mail.
    pub automated: bool,
    /// Whether the latest message was sent by the user.
    pub last_from_me: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailMessage {
    #[ts(type = "number")]
    pub id: i64,
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    #[ts(type = "number")]
    pub date: i64,
    /// Plain text. HTML mail is converted, with hidden text removed.
    pub body: String,
    pub seen: bool,
    pub from_me: bool,
    pub attachments: Vec<String>,
    /// Contains instructions aimed at an AI assistant.
    pub suspicious: bool,
    /// Whether it has an HTML version to show as it was sent. `None` until known (mail
    /// copied before Mimi kept it, found out the first time it's shown).
    pub has_html: Option<bool>,
}

/// One email ready to show: as it was sent (its HTML, made safe) and as formatted text.
/// See `mail::render` in the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailContent {
    /// The email's own HTML, made safe: no scripts, forms, hidden text or anything loaded
    /// from elsewhere. Pictures on other servers are left out unless `images_loaded`.
    /// `None` when it has no HTML version (or it couldn't be fetched).
    pub html: Option<String>,
    /// Markdown made from the HTML, or from the plain text: headings, emphasis, lists,
    /// links, quotes and simple tables. No raw HTML, no pictures.
    pub formatted: String,
    /// How many pictures in `html` come from other servers.
    pub remote_images: u32,
    /// Whether those pictures are in `html` (fetched by the daemon when the user asked).
    pub images_loaded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailThreadDetail {
    pub thread: MailThread,
    pub messages: Vec<MailMessage>,
}

/// Counts for the sidebar (for the whole mail, or the account or address asked for),
/// plus the connected accounts.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailOverview {
    pub accounts: Vec<MailAccount>,
    pub needs_reply: u32,
    pub important: u32,
    pub unread: u32,
    /// Whether new mail is sorted in the background.
    pub sorting: bool,
    /// Where the model that sorts, summarises and drafts runs; `None` without one.
    pub model_locality: Option<crate::Locality>,
    /// What sorts new mail (`Settings.mail_sorter`), and where that runs.
    pub sorter: crate::MailSorter,
    pub sorter_locality: Option<crate::Locality>,
    /// Whether a TypeSafe key is saved, so Jev can be chosen.
    pub jev_connected: bool,
    /// The user's smart folders.
    pub folders: Vec<MailFolder>,
}

/// A smart folder: the user's sorter files conversations that fit its description.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailFolder {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    /// What goes in it, in the user's words.
    pub description: String,
    /// Conversations in it, and how many of those are unread.
    pub threads: u32,
    pub unread: u32,
    /// Conversations not yet checked against it (it's still being filled).
    pub to_check: u32,
}

/// Body of `POST /v1/mail/folders`, and of `PATCH` (fields left out stay as they are).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct MailFolderInput {
    pub name: Option<String>,
    pub description: Option<String>,
}

/// Body of `POST /v1/mail/threads/{id}/folders`: the user puts a conversation in a
/// folder or takes it out. Their choice is kept over the sorter's.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderMembership {
    #[ts(type = "number")]
    pub folder: i64,
    pub member: bool,
}

/// Body of `PUT /v1/mail/jev`: the user's TypeSafe API key.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct JevKey {
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailAccount {
    pub connection_id: Uuid,
    pub email: String,
    pub name: String,
    /// The addresses its inbox mail arrived at (aliases, catch-all addresses, +tags),
    /// the account's own first.
    pub addresses: Vec<MailReceivedAddress>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailReceivedAddress {
    pub email: String,
    /// Conversations in the inbox that arrived at it.
    pub threads: u32,
    pub unread: u32,
}

/// A message to send, written by the user (or drafted for them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailDraft {
    /// Which account sends it. Defaults to the thread's account, else the first one.
    #[serde(default)]
    pub connection_id: Option<Uuid>,
    /// Which of that account's addresses it's from: its own, or an alias mail has
    /// arrived at. Defaults to the address a replied-to conversation arrived at, else
    /// the account's.
    #[serde(default)]
    pub from: Option<String>,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// The conversation this answers, so it threads correctly.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub reply_to: Option<i64>,
    /// A message being forwarded: its attachments go along (fetched from the server).
    #[serde(default)]
    #[ts(type = "number | null")]
    pub forward_of: Option<i64>,
}

/// Body of `POST /v1/mail/threads/{id}/draft`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct DraftRequest {
    /// What the reply should say, in the user's words ("say yes, but next week").
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailSummary {
    pub summary: String,
}

/// Body of `POST /v1/mail/threads/{id}/read`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MarkRead {
    pub read: bool,
}

/// What Mimi worked out about a mailbox from its address alone.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailDiscovery {
    /// The provider's name when it's a service Mimi recognises ("Migadu", "Gmail").
    pub provider: Option<String>,
    /// The connect dialog preset it corresponds to, if any.
    pub preset: Option<String>,
    /// The servers to use; `None` when nothing was found and the user must enter them.
    pub servers: Option<MailServers>,
    /// Whether this service wants an app-specific password rather than the usual one.
    pub needs_app_password: bool,
    pub help: Option<String>,
    pub help_url: Option<String>,
    /// False for services that don't let other apps read mail (e.g. no IMAP).
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailDiscoverRequest {
    pub email: String,
}
