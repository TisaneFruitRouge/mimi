//! The integrations Mimi offers, and which of them can be connected today.

use mimi_protocol::{Integration, IntegrationCategory, IntegrationStatus};

/// Integrations whose connection flow exists.
const AVAILABLE: &[&str] = &[
    "google_calendar",
    "caldav",
    "carddav",
    "telegram",
    "signal",
    "email",
];

/// The kinds of connection that make an integration connected. Address books come with
/// the same account connection as calendars (CalDAV and CardDAV share the app password),
/// so connecting one connects both; Google Calendar is connected by signing in with
/// Google or through a calendar's private address.
fn connected_as(id: &str) -> Vec<&str> {
    match id {
        "carddav" => vec!["caldav"],
        "google_calendar" => vec!["google_calendar", "google"],
        other => vec![other],
    }
}

/// The catalog, with each entry's status for this user.
pub fn catalog_for(connected: &[String]) -> Vec<Integration> {
    let mut all = catalog();
    for i in &mut all {
        i.status = if connected
            .iter()
            .any(|c| connected_as(&i.id).contains(&c.as_str()))
        {
            IntegrationStatus::Connected
        } else if AVAILABLE.contains(&i.id.as_str()) {
            IntegrationStatus::Available
        } else {
            IntegrationStatus::ComingSoon
        };
    }
    all
}

pub fn catalog() -> Vec<Integration> {
    let item =
        |id: &str, name: &str, category, description: &str, abilities: &[&str]| Integration {
            id: id.to_owned(),
            name: name.to_owned(),
            category,
            description: description.to_owned(),
            abilities: abilities.iter().map(|a| (*a).to_owned()).collect(),
            status: IntegrationStatus::ComingSoon,
        };
    use IntegrationCategory::*;
    vec![
        item(
            "google_calendar",
            "Google Calendar",
            Calendar,
            "Your schedule from Google: sign in with Google, or use a calendar's private address.",
            &[
                "Read your events",
                "Add, move and remove events, after you approve (when signed in)",
            ],
        ),
        item(
            "caldav",
            "iCloud & other calendars",
            Calendar,
            "Apple iCloud, Fastmail, Nextcloud or any CalDAV calendar.",
            &[
                "Read your events",
                "Add, move and remove events, after you approve",
            ],
        ),
        item(
            "google_contacts",
            "Google Contacts",
            Contacts,
            "So Mimi knows who people are and how to reach them.",
            &["Look up names, numbers and addresses"],
        ),
        item(
            "carddav",
            "iCloud & other contacts",
            Contacts,
            "Your iCloud, Fastmail or Nextcloud address book. Connects with the calendar account.",
            &[
                "Know who people are and every way to reach them",
                "Let you mention them with @",
            ],
        ),
        item(
            "telegram",
            "Telegram",
            Messaging,
            "Chat with your assistant from your phone, through a bot only you can use.",
            &[
                "Chat with you, and only you",
                "Ask you to approve actions, right in Telegram",
            ],
        ),
        item(
            "signal",
            "Signal",
            Messaging,
            "Chat with your assistant in Note to Self, end-to-end encrypted.",
            &[
                "Chat with you in Note to Self, and only there",
                "Ask you to approve actions: reply yes or no",
                "Send your reminders, without a notification",
            ],
        ),
        item(
            "matrix",
            "Matrix",
            Messaging,
            "Chat with Mimi on your own Matrix server.",
            &["Receive your messages", "Reply to you"],
        ),
        item(
            "whatsapp",
            "WhatsApp",
            Messaging,
            "Chat with Mimi on WhatsApp.",
            &["Receive your messages", "Reply to you"],
        ),
        item(
            "email",
            "Email",
            Email,
            "Any mailbox: iCloud, Gmail, Migadu, Fastmail, your own domain…",
            &[
                "Sort new mail: needs a reply, important, everything else",
                "Summarize what matters",
                "Draft replies you approve before sending",
            ],
        ),
    ]
}
