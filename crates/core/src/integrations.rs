//! The integrations Hearth offers. Connection flows arrive with the tools and channels
//! work; until then everything is listed as coming soon, so the UI can be honest.

use hearth_protocol::{Integration, IntegrationCategory, IntegrationStatus};

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
            "See your schedule and add events.",
            &[
                "Read your events",
                "Create and move events, after you approve",
            ],
        ),
        item(
            "caldav",
            "iCloud & other calendars",
            Calendar,
            "Apple iCloud, Fastmail, Nextcloud or any CalDAV calendar.",
            &[
                "Read your events",
                "Create and move events, after you approve",
            ],
        ),
        item(
            "google_contacts",
            "Google Contacts",
            Contacts,
            "So Hearth knows who people are and how to reach them.",
            &["Look up names, numbers and addresses"],
        ),
        item(
            "carddav",
            "iCloud & other contacts",
            Contacts,
            "Apple iCloud, Nextcloud or any CardDAV address book.",
            &["Look up names, numbers and addresses"],
        ),
        item(
            "telegram",
            "Telegram",
            Messaging,
            "Chat with Hearth from your phone.",
            &[
                "Receive your messages",
                "Reply to you",
                "Message people, after you approve",
            ],
        ),
        item(
            "signal",
            "Signal",
            Messaging,
            "End-to-end encrypted chat with Hearth.",
            &["Receive your messages", "Reply to you"],
        ),
        item(
            "matrix",
            "Matrix",
            Messaging,
            "Chat with Hearth on your own Matrix server.",
            &["Receive your messages", "Reply to you"],
        ),
        item(
            "whatsapp",
            "WhatsApp",
            Messaging,
            "Chat with Hearth on WhatsApp.",
            &["Receive your messages", "Reply to you"],
        ),
        item(
            "email",
            "Email",
            Email,
            "Gmail or any IMAP inbox.",
            &[
                "Summarize what matters",
                "Draft replies you approve before sending",
            ],
        ),
    ]
}
