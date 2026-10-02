//! # emails: suggesting people by their email addresses, as the user types in an email's
//! To or Cc or an event's Guests.

use std::collections::{HashMap, HashSet};

use mimi_protocol::{Channel, EmailSuggestion};
use rusqlite::Connection;
use uuid::Uuid;

use super::{fold, score, store};
use crate::AppState;
use crate::db::{DbError, enum_str, parse_uuid};

/// People with an email address matching `query` by name, nickname or address, best
/// first, one suggestion per address: all of theirs when the name matches, else the
/// addresses that do. Nothing for an empty query.
pub async fn suggest(
    state: &AppState,
    query: &str,
    limit: usize,
) -> Result<Vec<EmailSuggestion>, DbError> {
    let query = query.trim().to_owned();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    state
        .db
        .call(move |c| {
            let mut emails = emails(c)?;
            let folded = fold(&query);
            let needle = query.to_lowercase();
            let mut ranked: Vec<(u32, String, Uuid, Vec<String>)> = store::all(c)?
                .into_iter()
                .filter_map(|p| {
                    let theirs = emails.remove(&p.summary.id)?;
                    let by_name = score(&folded, &fold(&p.summary.name));
                    let by_nick = p
                        .summary
                        .nickname
                        .as_deref()
                        .and_then(|n| score(&folded, &fold(n)));
                    // Typing part of an address shows the addresses it's in; a name, all.
                    let matching: Vec<String> = theirs
                        .iter()
                        .filter(|e| e.contains(&needle))
                        .cloned()
                        .collect();
                    let by_email = (!matching.is_empty()).then(|| {
                        if matching.iter().any(|e| e.starts_with(&needle)) {
                            90
                        } else {
                            50
                        }
                    });
                    let best = by_name.max(by_nick).max(by_email)?;
                    let shown = if by_name.max(by_nick).is_none() {
                        matching
                    } else {
                        theirs
                    };
                    Some((best, p.summary.name, p.summary.id, shown))
                })
                .collect();
            ranked.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
            });
            let mut out = Vec::new();
            // One address listed once, even when two people have it.
            let mut listed = HashSet::new();
            for (_, name, id, shown) in ranked {
                out.extend(
                    shown
                        .into_iter()
                        .filter(|email| listed.insert(email.clone()))
                        .map(|email| EmailSuggestion {
                            person_id: id,
                            name: name.clone(),
                            email,
                        }),
                );
                if out.len() >= limit {
                    break;
                }
            }
            out.truncate(limit);
            Ok(out)
        })
        .await
}

/// Everyone's email addresses, lowercased, each once, in the order they were added.
fn emails(c: &Connection) -> rusqlite::Result<HashMap<Uuid, Vec<String>>> {
    let mut stmt = c.prepare(
        "SELECT person_id, value FROM person_handles WHERE channel = ?1 ORDER BY created_at, rowid",
    )?;
    let mut out: HashMap<Uuid, Vec<String>> = HashMap::new();
    let mut seen = HashSet::new();
    for row in stmt.query_map([enum_str(Channel::Email)], |r| {
        Ok((parse_uuid(r, 0)?, r.get::<_, String>(1)?))
    })? {
        let (person, email) = row?;
        let email = email.trim().to_lowercase();
        if seen.insert((person, email.clone())) {
            out.entry(person).or_default().push(email);
        }
    }
    Ok(out)
}
