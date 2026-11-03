//! Merging people the user says are the same, and undoing it.
//!
//! Contacts only ever unify on their own through a shared number, address or username
//! (see `store::sync_source`). This is the other way: the user's explicit choice, for
//! any people they pick. Everything that points at a merged-away person follows them to
//! the kept one: cards, hand-added handles, memory notes, permission exceptions, pairs
//! the user told apart, cards on their way back after a restore, and old links (through
//! `people_merged`). What each merge changed is kept for a week in `people_merges`, so
//! it can be undone exactly while nothing has come to depend on it; after that, "Not
//! the same person" separates any card again, including what the user had added by
//! hand (kept as a card of its own in `person_own_cards`).
//!
//! Everything here runs inside `Db::call`, in one transaction per merge or undo.

use std::collections::HashSet;

use mimi_protocol::{Autonomy, Handle, MergePreview, PermissionRule, PermissionTarget, Settings};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::normalize::match_key;
use super::store::{self, SourceNames};
use crate::db::parse_uuid;

/// The most people merged at once.
pub const MAX_MERGE: usize = 50;

/// How long a merge can be undone exactly.
const UNDO_FOR_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Why a merge can't happen, in words for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Someone merged with themselves.
    Same,
    /// No one to merge.
    Nobody,
    TooMany,
    /// Not in People at all.
    Missing,
    /// Deleted from Mimi (by name): bring them back first.
    Removed(String),
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Self::Same => "That's the same person.".to_owned(),
            Self::Nobody => "Choose at least one other person to merge.".to_owned(),
            Self::TooMany => format!("Merge at most {MAX_MERGE} people at once."),
            Self::Missing => "One of these people isn't in your contacts anymore.".to_owned(),
            Self::Removed(name) => format!(
                "{name} was removed from Mimi. Bring them back from Removed contacts first."
            ),
        }
    }
}

/// Why a merge can't be undone exactly anymore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoRefusal {
    /// Too old, already undone, or never was.
    Gone,
    /// Something happened since that an undo would lose (another merge, a split, a
    /// deletion).
    Moved,
}

impl UndoRefusal {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Gone => "This merge can't be undone anymore.",
            Self::Moved => {
                "This merge can't be undone anymore: the contact changed since. Use “Not the \
                 same person” on it to separate a card."
            }
        }
    }
}

/// The person that `id` is now: themselves, or whoever they were merged into (and so
/// on, if that person was merged too). `None` if they're gone.
pub fn resolve(c: &Connection, id: Uuid) -> rusqlite::Result<Option<Uuid>> {
    let mut id = id;
    for _ in 0..16 {
        if store::exists(c, id)? {
            return Ok(Some(id));
        }
        let next: Option<Uuid> = c
            .query_row(
                "SELECT into_id FROM people_merged WHERE id = ?1",
                [id.to_string()],
                |r| parse_uuid(r, 0),
            )
            .optional()?;
        match next {
            Some(next) => id = next,
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Everyone merged into this person over time (directly or through someone merged
/// into them), for finding what still points at their old ids.
pub fn merged_into(c: &Connection, id: Uuid) -> rusqlite::Result<Vec<Uuid>> {
    let mut stmt = c.prepare("SELECT id FROM people_merged WHERE into_id = ?1")?;
    let mut out: Vec<Uuid> = Vec::new();
    let mut queue = vec![id];
    while let Some(next) = queue.pop() {
        for found in stmt.query_map([next.to_string()], |r| parse_uuid(r, 0))? {
            let found = found?;
            if found != id && !out.contains(&found) && out.len() < 256 {
                out.push(found);
                queue.push(found);
            }
        }
    }
    Ok(out)
}

fn check(c: &Connection, keep: Uuid, others: &[Uuid]) -> rusqlite::Result<Result<(), Refusal>> {
    if others.is_empty() {
        return Ok(Err(Refusal::Nobody));
    }
    if others.len() >= MAX_MERGE {
        return Ok(Err(Refusal::TooMany));
    }
    let mut seen = HashSet::from([keep]);
    for id in others {
        if !seen.insert(*id) {
            return Ok(Err(Refusal::Same));
        }
    }
    for id in std::iter::once(&keep).chain(others) {
        if store::exists(c, *id)? {
            continue;
        }
        let removed: Option<String> = c
            .query_row(
                "SELECT name FROM people_removed WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        return Ok(Err(match removed {
            Some(name) => Refusal::Removed(name),
            None => Refusal::Missing,
        }));
    }
    Ok(Ok(()))
}

/// What merging these people would give: who, the names to choose from, and every way
/// to reach them (the same number or address once).
pub fn preview(
    c: &Connection,
    keep: Uuid,
    others: &[Uuid],
    names: &SourceNames,
) -> rusqlite::Result<Result<MergePreview, Refusal>> {
    if let Err(r) = check(c, keep, others)? {
        return Ok(Err(r));
    }
    let order: Vec<Uuid> = std::iter::once(keep)
        .chain(others.iter().copied())
        .collect();
    let everyone = store::all(c)?;
    let mut people = Vec::new();
    let mut choices: Vec<String> = Vec::new();
    let mut handles: Vec<Handle> = Vec::new();
    let mut seen = HashSet::new();
    for id in &order {
        if let Some(p) = everyone.iter().find(|p| p.summary.id == *id) {
            people.push(p.summary.clone());
        }
        let Some(person) = store::get(c, *id, names)? else {
            continue;
        };
        if !choices.contains(&person.name) {
            choices.push(person.name.clone());
        }
        for h in person.handles {
            let key = match_key(h.channel, &h.value).unwrap_or_else(|| h.value.to_lowercase());
            if seen.insert((h.channel, key)) {
                handles.push(h);
            }
        }
    }
    handles.sort_by_key(|h| h.channel);
    Ok(Ok(MergePreview {
        people,
        names: choices,
        handles,
    }))
}

/// A person's own row, as it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Row {
    id: Uuid,
    name: String,
    nickname: Option<String>,
    name_locked: bool,
    manual: bool,
    created_at: i64,
    updated_at: i64,
}

fn row(c: &Connection, id: Uuid) -> rusqlite::Result<Option<Row>> {
    c.query_row(
        "SELECT name, nickname, name_locked, manual, created_at, updated_at FROM people WHERE id = ?1",
        [id.to_string()],
        |r| {
            Ok(Row {
                id,
                name: r.get(0)?,
                nickname: r.get(1)?,
                name_locked: r.get(2)?,
                manual: r.get(3)?,
                created_at: r.get(4)?,
                updated_at: r.get(5)?,
            })
        },
    )
    .optional()
}

/// Everything a merge changed, to put back on undo.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Snapshot {
    keep: Option<Row>,
    /// The kept person's name, nickname and lock right after the merge: an undo puts
    /// the old ones back only if the user hasn't changed them since.
    after: (String, Option<String>, bool),
    absorbed: Vec<Row>,
    /// Source cards (source, record) and who had them.
    cards: Vec<(String, String, Uuid)>,
    /// Hand-added cards the merged-away people already had, and who had them.
    own_moved: Vec<(String, Uuid)>,
    /// Hand-added cards made by this merge (their record is the person's old id).
    own_created: Vec<Uuid>,
    /// Cards on their way back after a restore (source, record), and who to.
    marks: Vec<(String, String, Uuid)>,
    apart_removed: Vec<(Uuid, Uuid)>,
    apart_added: Vec<(Uuid, Uuid)>,
    /// Memory notes relinked (path, the subject they had).
    notes: Vec<(String, Uuid)>,
    /// Permission exceptions about anyone merged, by kind, before and after.
    rules_before: Vec<(String, PermissionRule)>,
    rules_after: Vec<(String, PermissionRule)>,
    /// What everyone merged could ask the assistant (`access`), before and after.
    #[serde(default)]
    access_before: Vec<crate::access::Row>,
    #[serde(default)]
    access_after: Option<crate::access::Row>,
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// Undoes it: [`undo`].
    pub id: Uuid,
    /// Memory notes followed the merged-away people.
    pub notes_changed: bool,
    /// Permission exceptions followed them.
    pub settings_changed: bool,
}

fn apart_pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}

/// Folds `others` into `keep`, the user's own choice. `name`, if given, becomes the
/// merged person's name (and stays, whatever address books say later).
pub fn merge(
    c: &mut Connection,
    keep: Uuid,
    others: &[Uuid],
    name: Option<&str>,
    now: i64,
) -> rusqlite::Result<Result<Merged, Refusal>> {
    let tx = c.transaction()?;
    if let Err(r) = check(&tx, keep, others)? {
        return Ok(Err(r));
    }
    let merge_id = Uuid::now_v7();
    let k = keep.to_string();
    let mut snap = Snapshot {
        keep: row(&tx, keep)?,
        ..Snapshot::default()
    };
    // Before anyone's row goes (their access would go with it).
    merge_access(&tx, keep, others, &mut snap, now)?;

    for &other in others {
        let Some(gone) = row(&tx, other)? else {
            continue;
        };
        let o = other.to_string();
        // Cards from address books and other sources.
        {
            let mut stmt =
                tx.prepare("SELECT source, record FROM person_records WHERE person_id = ?1")?;
            for card in stmt.query_map([&o], |r| Ok((r.get(0)?, r.get(1)?)))? {
                let (source, record) = card?;
                snap.cards.push((source, record, other));
            }
        }
        tx.execute(
            "UPDATE person_records SET person_id = ?1 WHERE person_id = ?2",
            [&k, &o],
        )?;
        // What the user had added to people merged into them before.
        {
            let mut stmt =
                tx.prepare("SELECT record FROM person_own_cards WHERE person_id = ?1")?;
            for record in stmt.query_map([&o], |r| r.get::<_, String>(0))? {
                snap.own_moved.push((record?, other));
            }
        }
        tx.execute(
            "UPDATE person_own_cards SET person_id = ?1 WHERE person_id = ?2",
            [&k, &o],
        )?;
        // What the user added to them directly becomes a card of its own, so it can be
        // separated again: someone added by hand has nothing else to be told apart by.
        let direct: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM person_handles
                            WHERE person_id = ?1 AND source IS NULL AND record IS NULL)",
            [&o],
            |r| r.get(0),
        )?;
        if gone.manual || direct {
            tx.execute(
                "INSERT INTO person_own_cards (record, person_id, name, nickname, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![o, k, gone.name, gone.nickname, gone.created_at],
            )?;
            tx.execute(
                "UPDATE person_handles SET record = ?1
                 WHERE person_id = ?1 AND source IS NULL AND record IS NULL",
                [&o],
            )?;
            snap.own_created.push(other);
        }
        tx.execute(
            "UPDATE person_handles SET person_id = ?1 WHERE person_id = ?2",
            [&k, &o],
        )?;
        // Cards still on their way back after a restore come to the merged person.
        {
            let mut stmt = tx.prepare(
                "SELECT source, record FROM person_records_removed WHERE person_id = ?1",
            )?;
            for card in stmt.query_map([&o], |r| Ok((r.get(0)?, r.get(1)?)))? {
                let (source, record) = card?;
                snap.marks.push((source, record, other));
            }
        }
        tx.execute(
            "UPDATE person_records_removed SET person_id = ?1 WHERE person_id = ?2",
            [&k, &o],
        )?;
        // Memory notes about them are about the merged person now.
        {
            let mut stmt = tx.prepare("SELECT path FROM memory_notes WHERE subject = ?1")?;
            for path in stmt.query_map([&o], |r| r.get::<_, String>(0))? {
                snap.notes.push((path?, other));
            }
        }
        tx.execute(
            "UPDATE memory_notes SET subject = ?1 WHERE subject = ?2",
            [&k, &o],
        )?;
        tx.execute(
            "UPDATE people SET nickname = COALESCE(nickname, ?2) WHERE id = ?1",
            params![k, gone.nickname],
        )?;
        // Old links (a page address, a mention in a chat) find the merged person.
        tx.execute(
            "INSERT OR REPLACE INTO people_merged (id, into_id, merge_id, merged_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![o, k, merge_id.to_string(), now],
        )?;
        tx.execute("DELETE FROM people WHERE id = ?1", [&o])?;
        snap.absorbed.push(gone);
    }

    // Told apart from someone: the merged person is still not them.
    let everyone: HashSet<Uuid> = std::iter::once(keep)
        .chain(others.iter().copied())
        .collect();
    let absorbed: HashSet<Uuid> = others.iter().copied().collect();
    let pairs: Vec<(Uuid, Uuid)> = {
        let mut stmt = tx.prepare("SELECT a, b FROM people_apart")?;
        stmt.query_map([], |r| Ok((parse_uuid(r, 0)?, parse_uuid(r, 1)?)))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(a, b)| absorbed.contains(a) || absorbed.contains(b))
            .collect()
    };
    for (a, b) in pairs {
        tx.execute(
            "DELETE FROM people_apart WHERE a = ?1 AND b = ?2",
            [a.to_string(), b.to_string()],
        )?;
        snap.apart_removed.push((a, b));
        let follow = |id: Uuid| if everyone.contains(&id) { keep } else { id };
        let (x, y) = (follow(a), follow(b));
        if x != y {
            let (x, y) = apart_pair(x, y);
            let added = tx.execute(
                "INSERT OR IGNORE INTO people_apart (a, b) VALUES (?1, ?2)",
                [x.to_string(), y.to_string()],
            )?;
            if added > 0 {
                snap.apart_added.push((x, y));
            }
        }
    }

    let settings_changed = merge_permissions(&tx, keep, others, &mut snap)?;

    if let Some(name) = name {
        tx.execute(
            "UPDATE people SET name = ?1, name_locked = 1 WHERE id = ?2",
            params![name, k],
        )?;
    }
    store::refresh_name(&tx, keep, now)?;
    tx.execute(
        "UPDATE people SET updated_at = ?1 WHERE id = ?2",
        params![now, k],
    )?;
    snap.after = tx.query_row(
        "SELECT name, nickname, name_locked FROM people WHERE id = ?1",
        [&k],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    tx.execute(
        "DELETE FROM people_merges WHERE created_at < ?1",
        [now - UNDO_FOR_MS],
    )?;
    let raw = serde_json::to_string(&snap)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    tx.execute(
        "INSERT INTO people_merges (id, keep, snapshot, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![merge_id.to_string(), k, raw, now],
    )?;
    let notes_changed = !snap.notes.is_empty();
    tx.commit()?;
    Ok(Ok(Merged {
        id: merge_id,
        notes_changed,
        settings_changed,
    }))
}

fn load_settings(c: &Connection) -> rusqlite::Result<Option<Settings>> {
    let raw: Option<String> = c
        .query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(raw.and_then(|raw| serde_json::from_str(&raw).ok()))
}

fn save_settings(c: &Connection, settings: &Settings) -> rusqlite::Result<()> {
    let raw = serde_json::to_string(settings)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    c.execute("UPDATE settings SET value = ?1 WHERE key = 'app'", [raw])?;
    Ok(())
}

fn about(rule: &PermissionRule, people: &HashSet<Uuid>) -> bool {
    matches!(rule.target, PermissionTarget::Person(p) if people.contains(&p))
}

/// Permission exceptions for anyone merged become one for the merged person: automatic
/// only if every one of them was (by exception or by default), else ask. Returns
/// whether any changed.
fn merge_permissions(
    c: &Connection,
    keep: Uuid,
    others: &[Uuid],
    snap: &mut Snapshot,
) -> rusqlite::Result<bool> {
    let Some(mut settings) = load_settings(c)? else {
        return Ok(false);
    };
    let everyone: HashSet<Uuid> = std::iter::once(keep)
        .chain(others.iter().copied())
        .collect();
    let mut changed = false;
    for (kind, choice) in settings.permissions.0.iter_mut() {
        let involved: Vec<PermissionRule> = choice
            .rules
            .iter()
            .filter(|r| about(r, &everyone))
            .cloned()
            .collect();
        if involved.is_empty() {
            continue;
        }
        let all_automatic = everyone.iter().all(|p| {
            involved
                .iter()
                .find(|r| r.target == PermissionTarget::Person(*p))
                .map_or(choice.autonomy, |r| r.autonomy)
                == Autonomy::Automatic
        });
        let rule = PermissionRule {
            target: PermissionTarget::Person(keep),
            autonomy: if all_automatic {
                Autonomy::Automatic
            } else {
                Autonomy::Ask
            },
        };
        choice.rules.retain(|r| !about(r, &everyone));
        choice.rules.push(rule.clone());
        snap.rules_before
            .extend(involved.into_iter().map(|r| (kind.clone(), r)));
        snap.rules_after.push((kind.clone(), rule));
        changed = true;
    }
    if changed {
        save_settings(c, &settings)?;
    }
    Ok(changed)
}

/// What everyone merged could ask the assistant becomes the merged person's: the
/// stricter choice (`access::combine`).
fn merge_access(
    c: &Connection,
    keep: Uuid,
    others: &[Uuid],
    snap: &mut Snapshot,
    now: i64,
) -> rusqlite::Result<()> {
    for id in std::iter::once(keep).chain(others.iter().copied()) {
        if let Some(r) = crate::access::row(c, id)? {
            snap.access_before.push(r);
        }
    }
    if snap.access_before.is_empty() {
        return Ok(());
    }
    for other in others {
        crate::access::delete_row(c, *other)?;
    }
    let merged = crate::access::combine(&snap.access_before, keep, now);
    if let Some(merged) = &merged {
        crate::access::put_row(c, merged)?;
    }
    snap.access_after = merged;
    Ok(())
}

/// Puts back everyone's access as it was before the merge, unless the user changed the
/// merged person's since.
fn unmerge_access(c: &Connection, keep: Uuid, snap: &Snapshot) -> rusqlite::Result<()> {
    if snap.access_before.is_empty() || crate::access::row(c, keep)? != snap.access_after {
        return Ok(());
    }
    crate::access::delete_row(c, keep)?;
    for r in &snap.access_before {
        crate::access::put_row(c, r)?;
    }
    Ok(())
}

/// Puts back the exceptions a merge replaced, unless the user changed them since.
fn unmerge_permissions(c: &Connection, snap: &Snapshot) -> rusqlite::Result<bool> {
    if snap.rules_after.is_empty() {
        return Ok(false);
    }
    let Some(mut settings) = load_settings(c)? else {
        return Ok(false);
    };
    for (kind, rule) in &snap.rules_after {
        if let Some(choice) = settings.permissions.0.get_mut(kind) {
            choice.rules.retain(|r| r != rule);
        }
    }
    for (kind, rule) in &snap.rules_before {
        if let Some(choice) = settings.permissions.0.get_mut(kind)
            && !choice.rules.iter().any(|r| r.target == rule.target)
        {
            choice.rules.push(rule.clone());
        }
    }
    save_settings(c, &settings)?;
    Ok(true)
}

/// What an undo did: who is back, and what else followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub keep: Uuid,
    pub restored: Vec<Uuid>,
    pub notes_changed: bool,
    pub settings_changed: bool,
}

/// Undoes a merge exactly: everyone merged away comes back under their own id, with
/// their name, cards, handles, notes and exceptions. Only the latest merge into a
/// person can be undone, and only while its people haven't been separated, merged or
/// deleted since. Cards that joined the merged person after it stay with them.
pub fn undo(
    c: &mut Connection,
    merge_id: Uuid,
    now: i64,
) -> rusqlite::Result<Result<Undone, UndoRefusal>> {
    let tx = c.transaction()?;
    let found: Option<(Uuid, String)> = tx
        .query_row(
            "SELECT keep, snapshot FROM people_merges WHERE id = ?1",
            [merge_id.to_string()],
            |r| Ok((parse_uuid(r, 0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((keep, raw)) = found else {
        return Ok(Err(UndoRefusal::Gone));
    };
    let Ok(snap) = serde_json::from_str::<Snapshot>(&raw) else {
        return Ok(Err(UndoRefusal::Gone));
    };
    let latest: String = tx.query_row(
        "SELECT id FROM people_merges WHERE keep = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
        [keep.to_string()],
        |r| r.get(0),
    )?;
    if latest != merge_id.to_string() || !store::exists(&tx, keep)? {
        return Ok(Err(UndoRefusal::Moved));
    }
    for gone in &snap.absorbed {
        let taken: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM people WHERE id = ?1)
                 OR EXISTS (SELECT 1 FROM people_removed WHERE id = ?1)",
            [gone.id.to_string()],
            |r| r.get(0),
        )?;
        if taken {
            return Ok(Err(UndoRefusal::Moved));
        }
    }

    let k = keep.to_string();
    for p in &snap.absorbed {
        tx.execute(
            "INSERT INTO people (id, name, nickname, name_locked, manual, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                p.id.to_string(),
                p.name,
                p.nickname,
                p.name_locked,
                p.manual,
                p.created_at,
                p.updated_at
            ],
        )?;
    }
    // Only what's still with the merged person goes back: a card split off since
    // stays where the user put it.
    for (source, record, owner) in &snap.cards {
        let moved = tx.execute(
            "UPDATE person_records SET person_id = ?1 WHERE source = ?2 AND record = ?3 AND person_id = ?4",
            params![owner.to_string(), source, record, k],
        )?;
        if moved > 0 {
            tx.execute(
                "UPDATE person_handles SET person_id = ?1 WHERE source = ?2 AND record = ?3",
                params![owner.to_string(), source, record],
            )?;
        }
    }
    for (record, owner) in &snap.own_moved {
        let moved = tx.execute(
            "UPDATE person_own_cards SET person_id = ?1 WHERE record = ?2 AND person_id = ?3",
            params![owner.to_string(), record, k],
        )?;
        if moved > 0 {
            tx.execute(
                "UPDATE person_handles SET person_id = ?1
                 WHERE source IS NULL AND record = ?2 AND person_id = ?3",
                params![owner.to_string(), record, k],
            )?;
        }
    }
    for owner in &snap.own_created {
        let o = owner.to_string();
        tx.execute(
            "DELETE FROM person_own_cards WHERE record = ?1 AND person_id = ?2",
            [&o, &k],
        )?;
        tx.execute(
            "UPDATE person_handles SET person_id = ?1, record = NULL
             WHERE source IS NULL AND record = ?1 AND person_id = ?2",
            [&o, &k],
        )?;
    }
    for (source, record, owner) in &snap.marks {
        tx.execute(
            "UPDATE person_records_removed SET person_id = ?1
             WHERE source = ?2 AND record = ?3 AND person_id = ?4",
            params![owner.to_string(), source, record, k],
        )?;
    }
    if let Some(before) = &snap.keep {
        let current: (String, Option<String>, bool) = tx.query_row(
            "SELECT name, nickname, name_locked FROM people WHERE id = ?1",
            [&k],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if current == snap.after {
            tx.execute(
                "UPDATE people SET name = ?1, nickname = ?2, name_locked = ?3, updated_at = ?4 WHERE id = ?5",
                params![before.name, before.nickname, before.name_locked, now, k],
            )?;
        }
    }
    for (a, b) in &snap.apart_added {
        tx.execute(
            "DELETE FROM people_apart WHERE a = ?1 AND b = ?2",
            [a.to_string(), b.to_string()],
        )?;
    }
    for (a, b) in &snap.apart_removed {
        tx.execute(
            "INSERT OR IGNORE INTO people_apart (a, b) VALUES (?1, ?2)",
            [a.to_string(), b.to_string()],
        )?;
    }
    for (path, subject) in &snap.notes {
        tx.execute(
            "UPDATE memory_notes SET subject = ?1 WHERE path = ?2 AND subject = ?3",
            params![subject.to_string(), path, k],
        )?;
    }
    let settings_changed = unmerge_permissions(&tx, &snap)?;
    unmerge_access(&tx, keep, &snap)?;
    tx.execute(
        "DELETE FROM people_merged WHERE merge_id = ?1",
        [merge_id.to_string()],
    )?;
    tx.execute(
        "DELETE FROM people_merges WHERE id = ?1",
        [merge_id.to_string()],
    )?;
    // Someone whose cards all left their address book since has nothing to come back as.
    let mut restored = Vec::new();
    for p in &snap.absorbed {
        store::refresh_name(&tx, p.id, now)?;
        store::drop_if_empty(&tx, p.id)?;
        if store::exists(&tx, p.id)? {
            restored.push(p.id);
        }
    }
    store::refresh_name(&tx, keep, now)?;
    tx.commit()?;
    Ok(Ok(Undone {
        keep,
        restored,
        notes_changed: !snap.notes.is_empty(),
        settings_changed,
    }))
}
