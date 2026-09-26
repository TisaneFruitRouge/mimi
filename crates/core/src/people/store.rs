//! The people directory in the database. Everything here runs inside
//! `Db::call`, on a plain connection or transaction.

use std::collections::{BTreeSet, HashMap, HashSet};

use hearth_protocol::{Channel, Handle, Person, PersonSource, PersonSummary};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::normalize::{self, match_key};
use super::{CardHandle, ContactCard};
use crate::db::{enum_str, parse_enum, parse_uuid};

/// Plain-language names of sources, by source id.
pub type SourceNames = HashMap<String, String>;

pub const MANUAL_SOURCE_NAME: &str = "Added by you";

fn source_name(names: &SourceNames, source: Option<&str>) -> String {
    match source {
        None => MANUAL_SOURCE_NAME.to_owned(),
        Some(s) => names
            .get(s)
            .cloned()
            .unwrap_or_else(|| "A removed connection".to_owned()),
    }
}

fn insert_person(
    c: &Connection,
    name: &str,
    nickname: Option<&str>,
    manual: bool,
    now: i64,
) -> rusqlite::Result<Uuid> {
    let id = Uuid::now_v7();
    c.execute(
        "INSERT INTO people (id, name, nickname, manual, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id.to_string(), name, nickname, manual, now],
    )?;
    Ok(id)
}

fn insert_handle(
    c: &Connection,
    person: Uuid,
    h: &CardHandle,
    source: Option<&str>,
    record: Option<&str>,
    now: i64,
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO person_handles (id, person_id, channel, value, match_key, label, source, record, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            Uuid::now_v7().to_string(),
            person.to_string(),
            enum_str(h.channel),
            h.value,
            match_key(h.channel, &h.value),
            h.label,
            source,
            record,
            now
        ],
    )?;
    Ok(())
}

/// The person any of these handles already belongs to (the oldest, if several).
fn find_by_handles(c: &Connection, handles: &[CardHandle]) -> rusqlite::Result<Option<Uuid>> {
    let keys: Vec<String> = handles
        .iter()
        .filter_map(|h| match_key(h.channel, &h.value))
        .collect();
    for key in keys {
        let found: Option<String> = c
            .query_row(
                "SELECT h.person_id FROM person_handles h JOIN people p ON p.id = h.person_id
                 WHERE h.match_key = ?1 ORDER BY p.created_at LIMIT 1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = found.and_then(|s| s.parse().ok()) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// A card's handles, comparable across syncs.
fn fingerprint(card: &ContactCard) -> BTreeSet<(String, String, Option<String>)> {
    card.handles
        .iter()
        .map(|h| (enum_str(h.channel), h.value.clone(), h.label.clone()))
        .collect()
}

fn stored_fingerprint(
    c: &Connection,
    source: &str,
    record: &str,
) -> rusqlite::Result<BTreeSet<(String, String, Option<String>)>> {
    let mut stmt = c.prepare(
        "SELECT channel, value, label FROM person_handles WHERE source = ?1 AND record = ?2",
    )?;
    stmt.query_map([source, record], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect()
}

/// Brings one source's cards in. Returns whether anything changed.
///
/// A card seen before stays with its person (even if the user split it off), and only
/// its details refresh. A new card joins the person that already has one of its phone
/// numbers, emails or usernames, or becomes a new person. Names never unify people.
pub fn sync_source(
    c: &mut Connection,
    source: &str,
    cards: &[ContactCard],
    now: i64,
) -> rusqlite::Result<bool> {
    let tx = c.transaction()?;
    let existing: HashMap<String, (Uuid, String)> = {
        let mut stmt =
            tx.prepare("SELECT record, person_id, name FROM person_records WHERE source = ?1")?;
        stmt.query_map([source], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (parse_uuid(r, 1)?, r.get::<_, String>(2)?),
            ))
        })?
        .collect::<Result<_, _>>()?
    };
    let mut changed = false;
    let mut seen = HashSet::new();

    for card in cards {
        if !seen.insert(card.record.clone()) {
            continue;
        }
        let person = match existing.get(&card.record) {
            Some((person, old_name)) => {
                let same = *old_name == card.name
                    && stored_fingerprint(&tx, source, &card.record)? == fingerprint(card);
                if same {
                    continue;
                }
                tx.execute(
                    "UPDATE person_records SET name = ?1 WHERE source = ?2 AND record = ?3",
                    params![card.name, source, card.record],
                )?;
                *person
            }
            None => {
                let person = match find_by_handles(&tx, &card.handles)? {
                    Some(p) => p,
                    None => insert_person(&tx, &card.name, card.nickname.as_deref(), false, now)?,
                };
                tx.execute(
                    "INSERT INTO person_records (source, record, person_id, name) VALUES (?1, ?2, ?3, ?4)",
                    params![source, card.record, person.to_string(), card.name],
                )?;
                person
            }
        };
        changed = true;
        tx.execute(
            "DELETE FROM person_handles WHERE source = ?1 AND record = ?2",
            params![source, card.record],
        )?;
        for h in &card.handles {
            insert_handle(&tx, person, h, Some(source), Some(&card.record), now)?;
        }
        refresh_name(&tx, person, now)?;
        if let Some(nick) = &card.nickname {
            tx.execute(
                "UPDATE people SET nickname = ?1 WHERE id = ?2 AND nickname IS NULL",
                params![nick, person.to_string()],
            )?;
        }
    }

    for (record, (person, _)) in &existing {
        if seen.contains(record) {
            continue;
        }
        changed = true;
        tx.execute(
            "DELETE FROM person_handles WHERE source = ?1 AND record = ?2",
            params![source, record],
        )?;
        tx.execute(
            "DELETE FROM person_records WHERE source = ?1 AND record = ?2",
            params![source, record],
        )?;
        drop_if_empty(&tx, *person)?;
    }
    tx.commit()?;
    Ok(changed)
}

/// An imported person's name follows their first card, unless the user renamed them.
fn refresh_name(c: &Connection, person: Uuid, now: i64) -> rusqlite::Result<()> {
    // The fullest card names the person ("Sam Carter" with a phone and an email, not a
    // "Sam C." holding only a Signal number), whatever order the cards arrived in.
    let first: Option<String> = c
        .query_row(
            "SELECT r.name FROM person_records r WHERE r.person_id = ?1
             ORDER BY (SELECT COUNT(*) FROM person_handles h
                        WHERE h.source = r.source AND h.record = r.record) DESC,
                      length(r.name) DESC, r.rowid
             LIMIT 1",
            [person.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(name) = first {
        c.execute(
            "UPDATE people SET name = ?1, updated_at = ?2 WHERE id = ?3 AND name_locked = 0 AND manual = 0 AND name != ?1",
            params![name, now, person.to_string()],
        )?;
    }
    Ok(())
}

/// Removes an imported person once nothing is left of them.
fn drop_if_empty(c: &Connection, person: Uuid) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM people WHERE id = ?1 AND manual = 0
           AND NOT EXISTS (SELECT 1 FROM person_records WHERE person_id = ?1)
           AND NOT EXISTS (SELECT 1 FROM person_handles WHERE person_id = ?1)",
        [person.to_string()],
    )?;
    Ok(())
}

/// Drops everything imported from sources that no longer exist (removed connections).
pub fn purge_removed_sources(c: &mut Connection) -> rusqlite::Result<bool> {
    let tx = c.transaction()?;
    let orphans: Vec<Uuid> = {
        let mut stmt = tx.prepare(
            "SELECT DISTINCT person_id FROM person_records WHERE source NOT IN (SELECT id FROM connections)",
        )?;
        stmt.query_map([], |r| parse_uuid(r, 0))?
            .collect::<Result<_, _>>()?
    };
    tx.execute("DELETE FROM person_handles WHERE source IS NOT NULL AND source NOT IN (SELECT id FROM connections)", [])?;
    tx.execute(
        "DELETE FROM person_records WHERE source NOT IN (SELECT id FROM connections)",
        [],
    )?;
    for p in &orphans {
        drop_if_empty(&tx, *p)?;
    }
    tx.commit()?;
    Ok(!orphans.is_empty())
}

pub fn create_manual(
    c: &mut Connection,
    name: &str,
    nickname: Option<&str>,
    handles: &[CardHandle],
    now: i64,
) -> rusqlite::Result<Uuid> {
    let tx = c.transaction()?;
    let id = insert_person(&tx, name, nickname, true, now)?;
    for h in handles {
        insert_handle(&tx, id, h, None, None, now)?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn exists(c: &Connection, id: Uuid) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT 1 FROM people WHERE id = ?1",
        [id.to_string()],
        |_| Ok(()),
    )
    .optional()
    .map(|r| r.is_some())
}

pub fn rename(
    c: &Connection,
    id: Uuid,
    name: Option<&str>,
    nickname: Option<Option<&str>>,
    now: i64,
) -> rusqlite::Result<()> {
    if let Some(name) = name {
        c.execute(
            "UPDATE people SET name = ?1, name_locked = 1, updated_at = ?2 WHERE id = ?3",
            params![name, now, id.to_string()],
        )?;
    }
    if let Some(nick) = nickname {
        c.execute(
            "UPDATE people SET nickname = ?1, updated_at = ?2 WHERE id = ?3",
            params![nick, now, id.to_string()],
        )?;
    }
    Ok(())
}

pub fn add_handle(c: &Connection, person: Uuid, h: &CardHandle, now: i64) -> rusqlite::Result<()> {
    insert_handle(c, person, h, None, None, now)
}

/// Removes a handle the user added. Imported ones can't be removed here.
pub fn remove_manual_handle(c: &Connection, person: Uuid, handle: Uuid) -> rusqlite::Result<bool> {
    c.execute(
        "DELETE FROM person_handles WHERE id = ?1 AND person_id = ?2 AND source IS NULL",
        [handle.to_string(), person.to_string()],
    )
    .map(|n| n > 0)
}

/// Deletes a person the user added who has no imported cards.
pub fn delete_manual(c: &Connection, person: Uuid) -> rusqlite::Result<bool> {
    c.execute(
        "DELETE FROM people WHERE id = ?1 AND manual = 1
           AND NOT EXISTS (SELECT 1 FROM person_records WHERE person_id = ?1)",
        [person.to_string()],
    )
    .map(|n| n > 0)
}

/// Folds `other` into `keep`: every card and handle moves over.
pub fn merge(c: &mut Connection, keep: Uuid, other: Uuid, now: i64) -> rusqlite::Result<()> {
    let tx = c.transaction()?;
    let (k, o) = (keep.to_string(), other.to_string());
    tx.execute(
        "UPDATE person_records SET person_id = ?1 WHERE person_id = ?2",
        [&k, &o],
    )?;
    tx.execute(
        "UPDATE person_handles SET person_id = ?1 WHERE person_id = ?2",
        [&k, &o],
    )?;
    tx.execute(
        "UPDATE people SET
           manual = manual OR (SELECT manual FROM people WHERE id = ?2),
           nickname = COALESCE(nickname, (SELECT nickname FROM people WHERE id = ?2)),
           updated_at = ?3
         WHERE id = ?1",
        params![k, o, now],
    )?;
    tx.execute("DELETE FROM people_apart WHERE a = ?1 OR b = ?1", [&o])?;
    tx.execute("DELETE FROM people WHERE id = ?1", [&o])?;
    tx.commit()
}

/// Moves one card (and its handles) out into a person of its own. Returns the new
/// person, or `None` if that card isn't this person's.
pub fn split(
    c: &mut Connection,
    person: Uuid,
    source: &str,
    record: &str,
    now: i64,
) -> rusqlite::Result<Option<Uuid>> {
    let tx = c.transaction()?;
    let name: Option<String> = tx
        .query_row(
            "SELECT name FROM person_records WHERE person_id = ?1 AND source = ?2 AND record = ?3",
            params![person.to_string(), source, record],
            |r| r.get(0),
        )
        .optional()?;
    let Some(name) = name else { return Ok(None) };
    let fresh = insert_person(&tx, &name, None, false, now)?;
    tx.execute(
        "UPDATE person_records SET person_id = ?1 WHERE source = ?2 AND record = ?3",
        params![fresh.to_string(), source, record],
    )?;
    tx.execute(
        "UPDATE person_handles SET person_id = ?1 WHERE source = ?2 AND record = ?3",
        params![fresh.to_string(), source, record],
    )?;
    set_apart(&tx, person, fresh)?;
    refresh_name(&tx, person, now)?;
    drop_if_empty(&tx, person)?;
    tx.commit()?;
    Ok(Some(fresh))
}

pub fn set_apart(c: &Connection, a: Uuid, b: Uuid) -> rusqlite::Result<()> {
    let (a, b) = if a < b { (a, b) } else { (b, a) };
    c.execute(
        "INSERT OR IGNORE INTO people_apart (a, b) VALUES (?1, ?2)",
        [a.to_string(), b.to_string()],
    )?;
    Ok(())
}

pub fn get(c: &Connection, id: Uuid, names: &SourceNames) -> rusqlite::Result<Option<Person>> {
    let row: Option<(String, Option<String>, bool)> = c
        .query_row(
            "SELECT name, nickname, manual FROM people WHERE id = ?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((name, nickname, manual)) = row else {
        return Ok(None);
    };

    let mut stmt = c.prepare(
        "SELECT id, channel, value, label, source FROM person_handles WHERE person_id = ?1 ORDER BY channel, created_at",
    )?;
    let mut handles: Vec<Handle> = stmt
        .query_map([id.to_string()], |r| {
            let source: Option<String> = r.get(4)?;
            Ok(Handle {
                id: parse_uuid(r, 0)?,
                channel: parse_enum(r, 1)?,
                value: r.get(2)?,
                label: r.get(3)?,
                source_name: source_name(names, source.as_deref()),
                source_id: source,
            })
        })?
        .collect::<Result<_, _>>()?;
    // The same number from two address books is still one way to reach them.
    let mut seen = HashSet::new();
    handles.retain(|h| {
        let key = match_key(h.channel, &h.value).unwrap_or_else(|| h.value.to_lowercase());
        seen.insert((h.channel, key))
    });
    handles.sort_by_key(|h| h.channel);

    let mut stmt = c.prepare(
        "SELECT source, record, name FROM person_records WHERE person_id = ?1 ORDER BY rowid",
    )?;
    let sources = stmt
        .query_map([id.to_string()], |r| {
            let source: String = r.get(0)?;
            Ok(PersonSource {
                source_name: source_name(names, Some(&source)),
                source_id: source,
                record: r.get(1)?,
                name: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Some(Person {
        id,
        name,
        nickname,
        handles,
        sources,
        manual,
    }))
}

/// Everyone, with their channels and the text search matches against.
pub struct Indexed {
    pub summary: PersonSummary,
    /// Handle values, lowercased, for searching by number or address.
    pub values: Vec<String>,
}

pub fn all(c: &Connection) -> rusqlite::Result<Vec<Indexed>> {
    let mut stmt =
        c.prepare("SELECT id, name, nickname FROM people ORDER BY name COLLATE NOCASE")?;
    let people: Vec<(Uuid, String, Option<String>)> = stmt
        .query_map([], |r| Ok((parse_uuid(r, 0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut stmt = c.prepare("SELECT person_id, channel, value FROM person_handles")?;
    let mut handles: HashMap<Uuid, (BTreeSet<Channel>, Vec<String>)> = HashMap::new();
    for row in stmt.query_map([], |r| {
        Ok((
            parse_uuid(r, 0)?,
            parse_enum::<Channel>(r, 1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (person, channel, value) = row?;
        let entry = handles.entry(person).or_default();
        entry.0.insert(channel);
        entry.1.push(value.to_lowercase());
    }
    Ok(people
        .into_iter()
        .map(|(id, name, nickname)| {
            let (channels, values) = handles.remove(&id).unwrap_or_default();
            Indexed {
                summary: PersonSummary {
                    id,
                    name,
                    nickname,
                    channels: channels.into_iter().collect(),
                },
                values,
            }
        })
        .collect())
}

/// Pairs with the same name that the user hasn't already told apart.
pub fn duplicates(c: &Connection) -> rusqlite::Result<Vec<(PersonSummary, PersonSummary)>> {
    let everyone = all(c)?;
    let apart: HashSet<(Uuid, Uuid)> = {
        let mut stmt = c.prepare("SELECT a, b FROM people_apart")?;
        stmt.query_map([], |r| Ok((parse_uuid(r, 0)?, parse_uuid(r, 1)?)))?
            .collect::<Result<_, _>>()?
    };
    let mut by_name: HashMap<String, Vec<&PersonSummary>> = HashMap::new();
    for p in &everyone {
        let key = normalize::name_key(&p.summary.name);
        if !key.is_empty() {
            by_name.entry(key).or_default().push(&p.summary);
        }
    }
    let mut out = Vec::new();
    for group in by_name.values() {
        for (i, a) in group.iter().enumerate() {
            for b in &group[i + 1..] {
                let pair = if a.id < b.id {
                    (a.id, b.id)
                } else {
                    (b.id, a.id)
                };
                if !apart.contains(&pair) {
                    out.push(((*a).clone(), (*b).clone()));
                }
            }
        }
    }
    out.sort_by_key(|x| x.0.name.to_lowercase());
    Ok(out)
}
