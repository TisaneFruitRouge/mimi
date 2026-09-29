//! Guests on events: who they are (an address, or someone from People), how they're
//! written into iCalendar data, and whose events the user may invite people to.
//!
//! Calendars never email guests on Mimi's behalf: Google writes are made with
//! `sendUpdates=none`, and CalDAV attendees carry `SCHEDULE-AGENT=CLIENT` (RFC 6638), which
//! tells servers such as iCloud, Fastmail and Nextcloud that the client does the
//! scheduling. Invitations go out only through Mimi's own mail (`invite.rs`), when the
//! user says so.

use std::collections::HashSet;

use icalendar::Property;
use lettre::message::Mailbox;
use mimi_protocol::GuestResponse;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ics::{Attendee, CalEvent};
use super::{Account, Target};
use crate::AppState;

/// Most guests Mimi adds or invites at once, like the recipients of one email.
pub const MAX_GUESTS: usize = crate::mail::smtp::MAX_RECIPIENTS;

/// Someone invited to an event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guest {
    pub name: Option<String>,
    /// Lowercased.
    pub email: String,
    /// Their answer, when the calendar knows it.
    #[serde(default)]
    pub response: Option<GuestResponse>,
}

impl Guest {
    /// "Sam Carter <sam@example.com>" or a bare address. `None` for anything that isn't
    /// one plain address.
    pub fn parse(raw: &str) -> Option<Guest> {
        let m = raw.trim().parse::<Mailbox>().ok()?;
        let email = m.email.to_string().to_lowercase();
        if !plain_address(&email) {
            return None;
        }
        Some(Guest {
            name: m.name.map(|n| clean_name(&n)).filter(|n| !n.is_empty()),
            email,
            response: None,
        })
    }

    /// Their name, else their address.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.email)
    }

    /// "Sam Carter <sam@example.com>", as cards and email headers show it.
    pub fn mailbox(&self) -> String {
        match &self.name {
            Some(name)
                if name.contains([',', ';', ':', '<', '>', '@', '(', ')', '[', ']', '\\']) =>
            {
                match self.email.parse() {
                    Ok(address) => Mailbox::new(Some(name.clone()), address).to_string(),
                    Err(_) => self.email.clone(),
                }
            }
            Some(name) => format!("{name} <{}>", self.email),
            None => self.email.clone(),
        }
    }

    pub fn from_attendee(a: &Attendee) -> Guest {
        Guest {
            name: a.name.as_deref().map(clean_name).filter(|n| !n.is_empty()),
            email: a.email.to_lowercase(),
            response: a.response,
        }
    }
}

/// An address with nothing that could break out of an iCalendar value or a mail header.
fn plain_address(email: &str) -> bool {
    let Some((local, domain)) = email.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && email
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '@' | '\''))
}

/// A display name without quotes or control characters.
fn clean_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Parses guests as tool arguments and the API carry them ("Name <address>" or bare).
pub fn parse_all(raw: &[String]) -> Result<Vec<Guest>, String> {
    let mut out: Vec<Guest> = Vec::new();
    for r in raw.iter().filter(|r| !r.trim().is_empty()) {
        let g = Guest::parse(r).ok_or_else(|| format!("“{}” isn't an email address.", r.trim()))?;
        if !out.iter().any(|o| o.email == g.email) {
            out.push(g);
        }
    }
    if out.len() > MAX_GUESTS {
        return Err(format!(
            "That's more than {MAX_GUESTS} guests. Add the others in the calendar app."
        ));
    }
    Ok(out)
}

/// "Sam", "Sam and Léa", "Sam, Léa and Tom".
pub fn join(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub fn names(guests: &[Guest]) -> String {
    join(&guests.iter().map(Guest::label).collect::<Vec<_>>())
}

/// Turns what the user or the model named into guests: email addresses, people from
/// People by id (an @ mention), or by name when that's unambiguous: exactly one person
/// with that name who has exactly one email address (or several entries in People that
/// all come down to that one address). Anything else is refused with a plain message, so
/// the assistant asks instead of guessing an address.
pub async fn resolve(state: &AppState, inputs: &[String]) -> Result<Vec<Guest>, String> {
    let mut out: Vec<Guest> = Vec::new();
    for raw in inputs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let guest = if let Ok(id) = raw.parse::<Uuid>() {
            person_guest(state, id).await?
        } else if raw.contains('@') {
            let mut g =
                Guest::parse(raw).ok_or_else(|| format!("“{raw}” isn't an email address."))?;
            if g.name.is_none() {
                g.name = name_for(state, &g.email).await;
            }
            g
        } else {
            by_name(state, raw).await?
        };
        if !out.iter().any(|o| o.email == guest.email) {
            out.push(guest);
        }
    }
    if out.len() > MAX_GUESTS {
        return Err(format!(
            "That's more than {MAX_GUESTS} guests. Add the others in the calendar app."
        ));
    }
    Ok(out)
}

async fn person_guest(state: &AppState, id: Uuid) -> Result<Guest, String> {
    let person = crate::people::get(state, id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("That person isn't in People anymore.")?;
    let addresses = crate::mail::person_addresses(state, id).await?;
    one_address(&person.name, addresses)
}

fn one_address(name: &str, addresses: Vec<String>) -> Result<Guest, String> {
    let mut addresses: Vec<String> = addresses
        .into_iter()
        .filter(|a| plain_address(a))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    match addresses.len() {
        0 => Err(format!(
            "{name} has no email address in People. Ask the user for it."
        )),
        1 => Ok(Guest {
            name: Some(clean_name(name)).filter(|n| !n.is_empty()),
            email: addresses.remove(0),
            response: None,
        }),
        _ => Err(format!(
            "{name} has several email addresses ({}). Ask the user which one to invite.",
            addresses.join(", ")
        )),
    }
}

/// Exactly one person called this (full name, nickname or first name).
async fn by_name(state: &AppState, name: &str) -> Result<Guest, String> {
    let wanted = crate::people::fold(name);
    let everyone = state
        .db
        .call(|c| crate::people::store::all(c))
        .await
        .map_err(|e| e.to_string())?;
    let found: Vec<_> = everyone
        .into_iter()
        .filter(|p| {
            let full = crate::people::fold(&p.summary.name);
            full == wanted
                || full.split_whitespace().next() == Some(wanted.as_str())
                || p.summary
                    .nickname
                    .as_deref()
                    .is_some_and(|n| crate::people::fold(n) == wanted)
        })
        .collect();
    // Two entries for one person (the same single address) aren't a choice to make.
    let mut addresses = std::collections::BTreeSet::new();
    for p in &found {
        addresses.extend(crate::mail::person_addresses(state, p.summary.id).await?);
    }
    match found.as_slice() {
        [] => Err(format!(
            "Nobody called “{name}” is in People. Ask the user for their email address."
        )),
        [one] => person_guest(state, one.summary.id).await,
        [first, ..] if addresses.len() == 1 => {
            one_address(&first.summary.name, addresses.into_iter().collect())
        }
        several => Err(format!(
            "Several people are called “{name}” ({}). Ask the user which one they mean.",
            several
                .iter()
                .map(|p| p.summary.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The name People has for an address, when exactly one person has it.
async fn name_for(state: &AppState, email: &str) -> Option<String> {
    let key = crate::people::normalize::match_key(mimi_protocol::Channel::Email, email)?;
    let names: Vec<String> = state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT p.name FROM person_handles h JOIN people p ON p.id = h.person_id
                 WHERE h.channel = 'email' AND h.match_key = ?1 LIMIT 2",
            )?;
            stmt.query_map([key], |r| r.get(0))?.collect()
        })
        .await
        .ok()?;
    match names.as_slice() {
        [one] => Some(clean_name(one)).filter(|n| !n.is_empty()),
        _ => None,
    }
}

/// Every address that is the user's: their email accounts, the Google accounts they
/// signed in with, and CalDAV usernames that are addresses. Used to tell their own
/// events (they organize them) and to leave them out of their own invitations.
pub async fn my_addresses(state: &AppState, accounts: &[Account]) -> HashSet<String> {
    let mut me: HashSet<String> = crate::mail::my_addresses(state).await.into_iter().collect();
    for account in accounts {
        match account {
            Account::GoogleApi { config, .. } => {
                me.insert(config.email.to_lowercase());
            }
            Account::CalDav { config, .. } if plain_address(&config.username.to_lowercase()) => {
                me.insert(config.username.to_lowercase());
            }
            _ => {}
        }
    }
    me
}

/// Whether the user may change the event's guests and send its invitations: they
/// organize it, or nobody does.
pub fn is_mine(e: &CalEvent, me: &HashSet<String>) -> bool {
    match &e.organizer {
        None => true,
        Some(o) => o.me || me.contains(&o.email.to_lowercase()),
    }
}

/// The event's guests, without the user themselves.
pub fn guests_of(e: &CalEvent, me: &HashSet<String>) -> Vec<Guest> {
    let mut out: Vec<Guest> = Vec::new();
    for a in &e.attendees {
        let g = Guest::from_attendee(a);
        if a.me || me.contains(&g.email) || out.iter().any(|o| o.email == g.email) {
            continue;
        }
        out.push(g);
    }
    out
}

/// The address that organizes events the user adds guests to in this calendar: the
/// Google account itself, or for CalDAV the username when it's an address, else the
/// user's (first) email account. Otherwise why guests can't be added there.
pub async fn organizer_for(state: &AppState, target: &Target) -> Result<String, String> {
    match target {
        Target::GoogleApi { config, .. } => Ok(config.email.to_lowercase()),
        Target::Google { name, .. } => Err(format!(
            "Guests can't be added to {name} from Mimi: it's connected through its private address. Connect it with \"Sign in with Google\" in Settings › Connections."
        )),
        Target::CalDav {
            config, calendar, ..
        } => {
            let username = config.username.trim().to_lowercase();
            if plain_address(&username) {
                return Ok(username);
            }
            crate::mail::my_addresses(state)
                .await
                .into_iter()
                .next()
                .ok_or_else(|| {
                    format!(
                        "Guests can't be added to {} yet: Mimi needs your email address to invite people. Connect your email account in Settings › Connections.",
                        calendar.name
                    )
                })
        }
    }
}

// --- iCalendar ------------------------------------------------------------------------

/// A parameter value, always quoted (names may hold commas or colons), without anything
/// that could end it early.
fn param_text(s: &str) -> String {
    format!("\"{}\"", clean_name(s))
}

/// `ATTENDEE` for a new guest: a required participant, asked to answer, whom the
/// calendar server must not email (`SCHEDULE-AGENT=CLIENT`).
pub(crate) fn attendee_property(g: &Guest) -> Property {
    let mut p = Property::new("ATTENDEE", format!("mailto:{}", g.email));
    if let Some(name) = &g.name {
        p.add_parameter("CN", &param_text(name));
    }
    p.add_parameter("ROLE", "REQ-PARTICIPANT")
        .add_parameter("PARTSTAT", "NEEDS-ACTION")
        .add_parameter("RSVP", "TRUE")
        .add_parameter("SCHEDULE-AGENT", "CLIENT");
    p.done()
}

/// `ORGANIZER`, also handled by the client.
pub(crate) fn organizer_property(email: &str) -> Property {
    Property::new("ORGANIZER", format!("mailto:{email}"))
        .add_parameter("SCHEDULE-AGENT", "CLIENT")
        .done()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_parsed_plainly_or_refused() {
        let g = Guest::parse("Sam Carter <Sam@Example.com>").unwrap();
        assert_eq!(g.name.as_deref(), Some("Sam Carter"));
        assert_eq!(g.email, "sam@example.com");
        assert_eq!(g.mailbox(), "Sam Carter <sam@example.com>");
        assert_eq!(
            Guest::parse("lea@example.org").unwrap().label(),
            "lea@example.org"
        );
        // What a card shows reads back as the same guest.
        for name in ["Léa Martin", "Carter, Sam"] {
            let g = Guest {
                name: Some(name.into()),
                email: "lea@example.org".into(),
                response: None,
            };
            assert_eq!(
                Guest::parse(&g.mailbox()),
                Some(g.clone()),
                "{}",
                g.mailbox()
            );
        }
        for bad in [
            "not an address",
            "sam@localhost",
            "\"a:b\"@example.com",
            "x@example.com\r\nBcc: y@example.com",
        ] {
            assert!(Guest::parse(bad).is_none(), "{bad}");
        }
        assert_eq!(
            parse_all(&["sam@example.com".into(), "Sam <SAM@example.com>".into()])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(join(&["Sam", "Léa", "Tom"]), "Sam, Léa and Tom");
    }

    async fn person(state: &AppState, name: &str, email: &str) -> Uuid {
        let id = Uuid::now_v7();
        let (name, email) = (name.to_owned(), email.to_owned());
        let key =
            crate::people::normalize::match_key(mimi_protocol::Channel::Email, &email).unwrap();
        state
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO people (id, name, created_at, updated_at) VALUES (?1, ?2, 0, 0)",
                    (id.to_string(), &name),
                )?;
                c.execute(
                    "INSERT INTO person_handles (id, person_id, channel, value, match_key, created_at)
                     VALUES (?1, ?2, 'email', ?3, ?4, 0)",
                    (Uuid::now_v7().to_string(), id.to_string(), &email, key),
                )?;
                Ok(())
            })
            .await
            .unwrap();
        id
    }

    /// Names become addresses only when there's no choice to make; never a guess.
    #[tokio::test]
    async fn names_resolve_only_when_unambiguous() {
        let state = AppState::for_tests("t");
        let sam = person(&state, "Sam Carter", "sam@example.com").await;
        person(&state, "Léa Martin", "lea@example.com").await;
        person(&state, "Léa Martin", "lea@example.com").await;
        person(&state, "Tom Blanc", "tom@example.com").await;
        person(&state, "Tom Rouge", "tom.rouge@example.com").await;
        let one = |raw: &str| {
            let state = &state;
            let raw = raw.to_owned();
            async move { resolve(state, &[raw]).await }
        };
        assert_eq!(one("Sam").await.unwrap()[0].email, "sam@example.com");
        assert_eq!(
            one(&sam.to_string()).await.unwrap()[0].name.as_deref(),
            Some("Sam Carter")
        );
        // Two entries, one address: that address.
        assert_eq!(one("léa").await.unwrap()[0].email, "lea@example.com");
        // An address People knows gets its name.
        assert_eq!(
            one("tom@example.com").await.unwrap()[0].name.as_deref(),
            Some("Tom Blanc")
        );
        // Two different Toms, and nobody called Zoé: the assistant must ask.
        assert!(
            one("Tom")
                .await
                .unwrap_err()
                .contains("Tom Blanc, Tom Rouge")
        );
        assert!(one("Zoé").await.unwrap_err().contains("Ask the user"));
        assert!(one("not@an address").await.is_err());
    }

    #[test]
    fn names_with_awkward_characters_stay_inside_their_parameter() {
        let p = attendee_property(&Guest {
            name: Some("Carter, Sam \"The\" ;boss:".into()),
            email: "sam@example.com".into(),
            response: None,
        });
        let mut cal = icalendar::Calendar::new();
        let mut e = icalendar::Event::new();
        icalendar::Component::append_multi_property(&mut e, p);
        cal.push(e.done());
        let text = cal.to_string();
        assert!(text.contains("CN=\"Carter, Sam The ;boss:\""), "{text}");
        assert!(text.contains("SCHEDULE-AGENT=CLIENT"));
        // It reads back as the same guest.
        let back: icalendar::Calendar = text.parse().unwrap();
        let event = back.components[0].as_event().unwrap();
        let a = super::super::ics::attendee(
            &icalendar::Component::multi_properties(event)["ATTENDEE"][0],
        )
        .unwrap();
        assert_eq!(a.email, "sam@example.com");
        assert_eq!(a.name.as_deref(), Some("Carter, Sam The ;boss:"));
        assert_eq!(a.response, Some(GuestResponse::Pending));
    }
}
