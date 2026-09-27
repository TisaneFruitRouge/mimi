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
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// The conversation this answers, so it threads correctly.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub reply_to: Option<i64>,
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
