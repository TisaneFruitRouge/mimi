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
    /// Conversations the user flagged (starred), wherever they are.
    Flagged,
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
    /// How it looks: an icon and a colour, by name (see `MailFolderInput`).
    pub icon: String,
    pub color: String,
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
    /// One of the daemon's icon names (`sparkles`, `receipt`, `plane`…).
    pub icon: Option<String>,
    /// One of the daemon's colour names (`violet`, `blue`, `green`…).
    pub color: Option<String>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
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
    /// Blind copies: they get the message, and nobody else sees that they did. The
    /// assistant may add them too; its emails then wait for the user's OK unless every
    /// blind copy, like every other recipient, is someone the user knows.
    #[serde(default)]
    pub bcc: Vec<String>,
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
    /// Files the user attached (or pasted), sent as they are, and files the assistant
    /// attached by reference (`NewMailAttachment::source`).
    #[serde(default)]
    pub attachments: Vec<NewMailAttachment>,
    /// The message with its formatting, when the user wrote it in the editor: HTML that
    /// the daemon cleans before sending, with pictures in the text as `cid:` addresses of
    /// attachments that have a `content_id`. `body` is always the plain-text version, and
    /// it's what models, tools, approval cards and checks read. Without it the message
    /// goes as plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub html: Option<String>,
    /// The saved draft this is (`/v1/mail/drafts/{id}`): sending it removes the draft
    /// once it's queued, and taking it back (Undo, Cancel) saves it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub draft_id: Option<Uuid>,
}

/// A file the user attaches to an email they write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewMailAttachment {
    pub name: String,
    /// Its content type as the app read it, e.g. "application/pdf". Unknown or
    /// unreadable types go as "application/octet-stream".
    #[serde(default)]
    pub mime: Option<String>,
    /// The file's content, base64-encoded. Empty when `source` says where it is.
    pub data: String,
    /// Where the file is, when the assistant attached it: the daemon fetches it when
    /// the email is sent, so the model never handles the file itself.
    #[serde(default)]
    pub source: Option<MailAttachmentSource>,
    /// Set for a picture placed in the text: the `html` shows it as `cid:<content_id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content_id: Option<String>,
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

/// Body of `POST /v1/mail/threads/{id}/flag`: flag (star) a conversation or take the
/// flag off.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FlagThread {
    pub flagged: bool,
}

/// How a mailing list lets people unsubscribe, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailUnsubscribeMethod {
    /// Mimi asks the sender's website directly (RFC 8058 one-click).
    OneClick,
    /// Mimi sends the short email the list asks for, from the user's account.
    Email,
    /// Only a web page: it opens in the user's browser.
    Website,
}

/// `GET /v1/mail/threads/{id}/unsubscribe`: how to unsubscribe from the list a
/// conversation's latest message came from (`null` when it doesn't say).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailUnsubscribe {
    pub method: MailUnsubscribeMethod,
    /// Who is asked: the domain of the web or email address ("example.com").
    pub domain: String,
    /// The sender's name, else their address, for "Archive all from …".
    pub sender: String,
    /// `website` only: the page to open in the browser.
    pub url: Option<String>,
    /// Set once the user unsubscribed from this list (remembered per list).
    pub done: Option<MailUnsubscribed>,
    /// Conversations from this list still in the Inbox, this one included.
    pub in_inbox: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailUnsubscribed {
    pub method: MailUnsubscribeMethod,
    #[ts(type = "number")]
    pub at: i64,
}

/// Answer of `POST /v1/mail/threads/{id}/unsubscribe/archive`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailArchivedCount {
    pub archived: u32,
}

/// A file the assistant attached to an email by reference, fetched only when it's sent.
/// Never a file from the computer: only these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum MailAttachmentSource {
    /// An attachment of an email in the user's mail (fetched from the server).
    Email {
        /// The message (`MailMessage.id`).
        #[ts(type = "number")]
        message: i64,
        /// Which of its attachments, in the order `MailMessage.attachments` lists them.
        index: u32,
        /// Its size in bytes when it was attached, for showing.
        #[serde(default)]
        #[ts(type = "number | null")]
        size: Option<u64>,
    },
    /// A photo the user sent in a chat (`Attachment.id`). The assistant may only attach
    /// one from the conversation it's working in.
    Chat {
        attachment: Uuid,
        /// Its size in bytes, for showing.
        #[serde(default)]
        #[ts(type = "number | null")]
        size: Option<u64>,
    },
}

/// Body of `POST /v1/mail/older`: look on the mail servers for mail older than what's
/// kept here (the last 90 days).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailOlderSearch {
    /// Words to look for, as in the search field.
    pub q: String,
    /// Only this account's server (every account's when missing).
    #[serde(default)]
    pub account: Option<Uuid>,
}

/// What a search of the mail servers found, older than what's kept here.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailOlderResults {
    /// Conversations with a match, newest first. They're kept here for a while (a week,
    /// or 30 days after they were last opened) and open like any other.
    pub threads: Vec<MailThread>,
    /// The servers were searched for mail from before this moment.
    #[ts(type = "number")]
    pub before: i64,
    /// More matched than were brought in: only the newest are.
    pub more: bool,
    /// Accounts that couldn't be searched, or only partly, in plain words.
    pub problems: Vec<String>,
}

/// Body of `POST /v1/mail/outbox`: the user's Send, or Send later.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewOutgoingMail {
    pub draft: MailDraft,
    /// When to send it (ms since the epoch). `None`: now, after the few seconds Undo is
    /// offered (`Settings.undo_send_secs`; straight away when that's off).
    #[serde(default)]
    #[ts(type = "number | null")]
    pub send_at: Option<i64>,
}

/// Why a message waits in the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OutgoingKind {
    /// The user pressed Send: it waits out the seconds Undo is offered.
    Undo,
    /// The user (or the assistant, with their OK) chose a time to send it.
    Scheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OutgoingStatus {
    /// Waiting for `send_at`. With an `error`, an earlier try couldn't reach the server
    /// and this is the next one.
    Waiting,
    /// Being handed to the mail server right now.
    Sending,
    /// Gone (only in events: sent mail leaves the outbox and is filed in Sent).
    Sent,
    /// Not sent: `error` says why. It stays until the user sends, reschedules or
    /// cancels it.
    Failed,
    /// Taken back by the user (only in events).
    Cancelled,
}

/// A message in the outbox. Lists and events leave the attachments' content out
/// (`data` is empty); cancelling one gives the whole draft back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutgoingMail {
    pub id: Uuid,
    pub kind: OutgoingKind,
    pub status: OutgoingStatus,
    /// The draft as the user wrote it.
    pub draft: MailDraft,
    /// The account it goes from, and the address.
    pub connection_id: Uuid,
    pub from: String,
    /// When it goes (ms). For a late one, when it was due.
    #[ts(type = "number")]
    pub send_at: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    /// When it actually went, for `sent`: later than `send_at` when the computer was
    /// off or asleep at that time.
    #[ts(type = "number | null")]
    pub sent_at: Option<i64>,
    /// What went wrong, in plain words.
    pub error: Option<String>,
    /// Scheduled by the assistant, with the user's OK.
    pub by_assistant: bool,
}

/// Body of `PATCH /v1/mail/outbox/{id}`: send it at another time.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RescheduleMail {
    #[ts(type = "number")]
    pub send_at: i64,
}

/// Body of `POST /v1/mail/threads/batch`: one action on several conversations at once,
/// chosen together in the Mail panel. The user's own click, like the single actions.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailBatch {
    #[ts(type = "number[]")]
    pub ids: Vec<i64>,
    pub action: MailBatchAction,
}

/// What a batch does to each conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum MailBatchAction {
    /// Out of the Inbox, to the account's Archive.
    Archive,
    /// Every copy to the account's Trash.
    Delete,
    /// Mark read (`true`) or unread.
    Read { read: bool },
    /// Flag (`true`) or remove the flag.
    Flag { flagged: bool },
    /// Into a smart folder (`member`), or out of it. Labels in Mimi only.
    Folder {
        #[ts(type = "number")]
        folder: i64,
        member: bool,
    },
}

/// Answer of `POST /v1/mail/threads/batch`. Some conversations can fail while others
/// work (one account's server can't be reached): `failed` says which, and `message` says
/// so in plain words.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailBatchResult {
    /// Conversations it was done to (or that needed nothing).
    #[ts(type = "number[]")]
    pub done: Vec<i64>,
    pub failed: Vec<MailBatchFailure>,
    /// What went wrong, for showing as it is; `None` when nothing did.
    pub message: Option<String>,
}

/// Conversations a batch couldn't change, and why.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailBatchFailure {
    #[ts(type = "number[]")]
    pub ids: Vec<i64>,
    pub reason: String,
}

/// The user's email signatures (Settings › General › Signature), `GET|PUT
/// /v1/mail/signatures`. A signature goes at the end of each email the user writes (below
/// their text, above anything forwarded), after a `-- ` line, and the emails the assistant
/// writes for them get it too. It's part of the draft: the user can change or delete it
/// in any one email.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct MailSignatures {
    /// Every address signs with `all`. Off: an address in `addresses` signs with its own,
    /// and any other with `all`.
    pub same_for_all: bool,
    pub all: MailSignature,
    /// Signatures of single addresses (the account's own, or an alias), lowercase.
    pub addresses: Vec<AddressSignature>,
    /// Whether replies and forwards get it too. Some people leave it out of replies, as
    /// some mail apps do.
    pub in_replies: bool,
}

impl Default for MailSignatures {
    fn default() -> Self {
        Self {
            same_for_all: true,
            all: MailSignature::default(),
            addresses: Vec::new(),
            in_replies: true,
        }
    }
}

/// One signature, written in the mail editor: like a draft's text, a plain-text version
/// and, with formatting, HTML whose pictures are `cid:` addresses of `pictures`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct MailSignature {
    /// The plain text, one line per line (a picture reads "[image: name]"). Empty: none.
    pub text: String,
    /// With its formatting, cleaned like the HTML of an email when saved. `None`: plain.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub html: Option<String>,
    /// Small pictures in it (a logo), kept with the setting and sent in the email's text.
    /// Each has a `content_id` and its data; at most a few, small (see `mail::signature`).
    pub pictures: Vec<NewMailAttachment>,
}

/// The signature of one of the user's addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddressSignature {
    pub address: String,
    pub signature: MailSignature,
}

/// A saved draft, as the Drafts view lists it (`GET /v1/mail/drafts`): an email the user
/// started here, or one found in an account's Drafts folder on the server (written in
/// another mail app). Drafts are never sent unless the user opens one and presses Send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailDraftInfo {
    pub id: Uuid,
    /// The account it belongs to, and is copied to (`None` until there is one).
    pub connection_id: Option<Uuid>,
    pub subject: String,
    pub to: Vec<String>,
    /// The start of its text.
    pub snippet: String,
    /// When it last changed (ms).
    #[ts(type = "number")]
    pub updated_at: i64,
    /// The conversation it answers, when that conversation is here: it opens there, in
    /// the reply box.
    #[ts(type = "number | null")]
    pub reply_to: Option<i64>,
    /// How many files and pictures it carries.
    pub files: u32,
    /// Written in another mail app (found in the server's Drafts folder) and not changed
    /// here since.
    pub from_elsewhere: bool,
}

/// Body of `PUT /v1/mail/drafts/{id}`: the draft as it is now, saved as the user writes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SaveMailDraft {
    pub draft: MailDraft,
    /// The editor closed: copy it to the server's Drafts folder now rather than after a
    /// pause.
    #[serde(default)]
    pub now: bool,
}
