//! Whether email addresses belong to people the user knows, for sending mail on its own
//! (Settings › Permissions): the user's own addresses, anyone in their contacts, and
//! anyone they have written to.

use lettre::message::Mailbox;
use mimi_protocol::{Channel, MailAddress};
use rusqlite::Connection;

use crate::AppState;
use crate::people::normalize::match_key;

/// Whether every one of `recipients` ("Sam <sam@x.org>" or a bare address) is known.
/// Nobody, or anything that isn't an address, isn't.
///
/// Contacts means address books and people the user added by hand, not the cards Mimi
/// makes from mail (anyone who wrote twice would count). "Written to" means found in
/// the Sent folder, not merely `outgoing`: that also counts mail whose From line names
/// the user, which anyone can forge.
pub async fn all_known(state: &AppState, recipients: &[String]) -> bool {
    let Some(emails) = recipients
        .iter()
        .map(|raw| address_of(raw))
        .collect::<Option<Vec<String>>>()
    else {
        return false;
    };
    if emails.is_empty() {
        return false;
    }
    let me = super::my_addresses(state).await;
    let mail_sources: Vec<String> = super::accounts(state)
        .await
        .into_iter()
        .map(|a| a.id.to_string())
        .collect();
    state
        .db
        .call(move |c| {
            for email in &emails {
                let in_book = match match_key(Channel::Email, email) {
                    Some(key) => in_contacts(c, "email", &key, &mail_sources)?,
                    None => false,
                };
                if !(me.contains(email) || in_book || written_to(c, email)?) {
                    return Ok(false);
                }
            }
            Ok(true)
        })
        .await
        .unwrap_or(false)
}

/// The address in "Sam <sam@x.org>" or a bare address, lowercased. `None` for anything
/// else.
pub fn address_of(raw: &str) -> Option<String> {
    raw.trim()
        .parse::<Mailbox>()
        .ok()
        .map(|m| m.email.to_string().to_lowercase())
}

/// Whether a handle with this match key is in People from an address book or added by
/// hand (`channel`: `email`, `matrix`…), not from a card Mimi made itself from mail
/// (`mail_sources`: the email accounts' connection ids).
pub(crate) fn in_contacts(
    c: &Connection,
    channel: &str,
    key: &str,
    mail_sources: &[String],
) -> rusqlite::Result<bool> {
    let mut stmt =
        c.prepare("SELECT source FROM person_handles WHERE channel = ?1 AND match_key = ?2")?;
    let sources = stmt.query_map((channel, key), |r| r.get::<_, Option<String>>(0))?;
    for source in sources {
        match source? {
            None => return Ok(true),
            Some(s) if !mail_sources.contains(&s) => return Ok(true),
            Some(_) => {}
        }
    }
    Ok(false)
}

fn written_to(c: &Connection, email: &str) -> rusqlite::Result<bool> {
    // A cheap text match narrows it down; the parsed recipients decide.
    let pattern = format!(
        "%{}%",
        email
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let mut stmt = c.prepare(
        "SELECT to_json, cc_json FROM mail_messages
         WHERE folder = 'sent' AND (to_json LIKE ?1 ESCAPE '\\' OR cc_json LIKE ?1 ESCAPE '\\')",
    )?;
    let rows = stmt.query_map([pattern], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (to, cc) = row?;
        let parse = |json: &str| serde_json::from_str::<Vec<MailAddress>>(json).unwrap_or_default();
        if parse(&to)
            .iter()
            .chain(parse(&cc).iter())
            .any(|a| a.email.eq_ignore_ascii_case(email))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
