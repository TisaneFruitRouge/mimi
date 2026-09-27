use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// A configured link to one of the user's accounts (a calendar, a Telegram bot, …).
/// Secrets stay in the daemon; clients only see this summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Connection {
    pub id: Uuid,
    /// The catalog id of the integration, e.g. `telegram`.
    pub integration: String,
    pub name: String,
    pub status: ConnectionStatus,
    /// One plain-language line about its state, e.g. "3 calendars".
    pub detail: String,
    /// Where the user should go next, when `status` is `needs_action` (e.g. the
    /// Telegram link to press Start).
    pub action_url: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConnectionStatus {
    Ok,
    /// Set up, but waiting for the user to finish a step elsewhere.
    NeedsAction,
    Error,
}

/// What the user entered to connect an integration.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "integration", rename_all = "snake_case")]
#[ts(export)]
pub enum ConnectionSetup {
    /// A Google calendar read through its secret iCal address. No Google sign-in.
    GoogleCalendar { ics_url: String },
    /// Any CalDAV account: iCloud, Fastmail, Nextcloud, Radicale…
    Caldav {
        server_url: String,
        username: String,
        password: String,
    },
    /// A Telegram bot created by the user with @BotFather.
    Telegram { bot_token: String },
    /// An email account, read over IMAP and sent through SMTP with an app password.
    Email {
        email: String,
        password: String,
        /// A preset id (`icloud`, `gmail`…); `None` picks one from the address.
        #[serde(default)]
        preset: Option<String>,
        /// Server details for "Other"; ignored when a preset applies.
        #[serde(default)]
        servers: Option<crate::MailServers>,
    },
}
