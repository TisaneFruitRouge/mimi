//! Memory notes in the encrypted database, with a full-text index and undo history.

use mimi_protocol::{MemoryNote, MemoryNoteSummary, MemorySource};
use rusqlite::{OptionalExtension, Row, params};
use uuid::Uuid;

use super::{PROFILE_PATH, title_from_path};
use crate::db::{Db, DbError, enum_str, parse_enum};
use crate::now_ms;

/// Undo history kept per database; older changes can't be undone.
const REVISIONS_KEPT: i64 = 1000;

fn note(row: &Row) -> rusqlite::Result<MemoryNote> {
    Ok(MemoryNote {
        path: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        subject: row.get(3)?,
        source: parse_enum(row, 4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

const COLUMNS: &str = "path, title, body, subject, source, created_at, updated_at";

pub async fn get(db: &Db, path: &str) -> Result<Option<MemoryNote>, DbError> {
    let path = path.to_owned();
    db.call(move |c| {
        c.query_row(
            &format!("SELECT {COLUMNS} FROM memory_notes WHERE path = ?1"),
            [path],
            note,
        )
        .optional()
    })
    .await
}

/// The always-loaded summary, or an empty string.
pub async fn profile(db: &Db) -> Result<String, DbError> {
    Ok(get(db, PROFILE_PATH)
        .await?
        .map(|n| n.body)
        .unwrap_or_default())
}

/// Every library note (the profile excluded), by path.
pub async fn list(db: &Db) -> Result<Vec<MemoryNoteSummary>, DbError> {
    db.call(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM memory_notes WHERE path != ?1 ORDER BY path"
        ))?;
        stmt.query_map([PROFILE_PATH], note)?
            .map(|r| {
                r.map(|n| MemoryNoteSummary {
                    preview: preview(&n.body),
                    path: n.path,
                    title: n.title,
                    source: n.source,
                    updated_at: n.updated_at,
                })
            })
            .collect()
    })
    .await
}

fn preview(body: &str) -> String {
    let first = body
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '#']).trim())
        .find(|l| !l.is_empty())
        .unwrap_or("");
    first.chars().take(120).collect()
}

/// Writes a note (an empty body deletes it), keeping what it was before for undo.
/// Returns the revision id that undoes this change.
pub async fn put(
    db: &Db,
    path: &str,
    title: Option<&str>,
    body: &str,
    source: MemorySource,
    conversation_id: Option<Uuid>,
) -> Result<i64, DbError> {
    let path = path.to_owned();
    let body = body.trim().to_owned();
    // An explicit title wins; otherwise an existing note keeps its title and a new one
    // gets a title guessed from its content or path.
    let explicit = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let default = if path == PROFILE_PATH {
        "About you".to_owned()
    } else {
        super::guess_title(&path, &body).unwrap_or_else(|| title_from_path(&path))
    };
    db.call(move |c| {
        let tx = c.transaction()?;
        let revision = record_revision(&tx, &path, conversation_id)?;
        if body.is_empty() {
            tx.execute("DELETE FROM memory_notes WHERE path = ?1", [&path])?;
        } else {
            let now = now_ms();
            tx.execute(
                "INSERT INTO memory_notes (path, title, body, subject, source, created_at, updated_at)
                 VALUES (?1, COALESCE(?2, ?3), ?4, NULL, ?5, ?6, ?6)
                 ON CONFLICT (path) DO UPDATE SET title = COALESCE(?2, memory_notes.title),
                     body = excluded.body, source = excluded.source, updated_at = excluded.updated_at",
                params![path, explicit, default, body, enum_str(source), now],
            )?;
        }
        tx.commit()?;
        Ok(revision)
    })
    .await
}

pub async fn delete(
    db: &Db,
    path: &str,
    conversation_id: Option<Uuid>,
) -> Result<Option<i64>, DbError> {
    if get(db, path).await?.is_none() {
        return Ok(None);
    }
    put(db, path, None, "", MemorySource::You, conversation_id)
        .await
        .map(Some)
}

fn record_revision(
    tx: &rusqlite::Transaction,
    path: &str,
    conversation_id: Option<Uuid>,
) -> rusqlite::Result<i64> {
    let before: Option<(String, String, String)> = tx
        .query_row(
            "SELECT title, body, source FROM memory_notes WHERE path = ?1",
            [path],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (title, body, source) = match before {
        Some((t, b, s)) => (Some(t), Some(b), Some(s)),
        None => (None, None, None),
    };
    tx.execute(
        "INSERT INTO memory_revisions (path, before_title, before_body, before_source, conversation_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![path, title, body, source, conversation_id.map(|c| c.to_string()), now_ms()],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "DELETE FROM memory_revisions WHERE id <= ?1",
        [id - REVISIONS_KEPT],
    )?;
    Ok(id)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Undo {
    /// Restored; the note's path.
    Done(String),
    AlreadyUndone,
    Unknown,
}

/// A revision row: path, the note's previous title, body and source, and whether it
/// was already undone.
type RevisionRow = (String, Option<String>, Option<String>, Option<String>, bool);

/// Puts a note back the way it was before revision `id`. Only the latest change to a
/// note can be undone this way cleanly; undoing an older one restores that older state.
pub async fn undo(db: &Db, id: i64) -> Result<Undo, DbError> {
    db.call(move |c| {
        let tx = c.transaction()?;
        let row: Option<RevisionRow> = tx
            .query_row(
                "SELECT path, before_title, before_body, before_source, undone FROM memory_revisions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0)),
            )
            .optional()?;
        let Some((path, title, body, source, undone)) = row else {
            return Ok(Undo::Unknown);
        };
        if undone {
            return Ok(Undo::AlreadyUndone);
        }
        match (title, body) {
            (Some(title), Some(body)) => {
                let now = now_ms();
                tx.execute(
                    "INSERT INTO memory_notes (path, title, body, subject, source, created_at, updated_at)
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5)
                     ON CONFLICT (path) DO UPDATE SET title = excluded.title, body = excluded.body,
                         source = excluded.source, updated_at = excluded.updated_at",
                    params![path, title, body, source.unwrap_or_else(|| "you".to_owned()), now],
                )?;
            }
            _ => {
                tx.execute("DELETE FROM memory_notes WHERE path = ?1", [&path])?;
            }
        }
        tx.execute("UPDATE memory_revisions SET undone = 1 WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(Undo::Done(path))
    })
    .await
}

/// Where a revision was made, to update the chat line that shows it.
pub async fn revision_conversation(db: &Db, id: i64) -> Result<Option<Uuid>, DbError> {
    db.call(move |c| {
        c.query_row(
            "SELECT conversation_id FROM memory_revisions WHERE id = ?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .map(|o| o.flatten().and_then(|s| s.parse().ok()))
    })
    .await
}

/// Deletes every memory, the undo history and the learning progress.
pub async fn forget_all(db: &Db) -> Result<(), DbError> {
    db.call(|c| {
        c.execute_batch(
            "DELETE FROM memory_notes; DELETE FROM memory_revisions;
             INSERT INTO memory_fts (memory_fts) VALUES ('rebuild');",
        )
    })
    .await
}

/// A note found by full-text search.
#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub title: String,
    pub body: String,
}

/// Full-text search over the library (the profile excluded), best matches first.
/// `query` is an FTS5 expression; build it with [`super::recall::fts_query`].
pub async fn search(db: &Db, query: String, limit: usize) -> Result<Vec<Hit>, DbError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    db.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT n.path, n.title, n.body
             FROM memory_fts f JOIN memory_notes n ON n.rowid = f.rowid
             WHERE memory_fts MATCH ?1 AND n.path != ?2
             ORDER BY bm25(memory_fts, 2.0, 4.0, 1.0)
             LIMIT ?3",
        )?;
        stmt.query_map(params![query, PROFILE_PATH, limit as i64], |r| {
            Ok(Hit {
                path: r.get(0)?,
                title: r.get(1)?,
                body: r.get(2)?,
            })
        })?
        .collect()
    })
    .await
}

/// The newest message time the learning pass has read in a conversation.
pub async fn learned_until(db: &Db, conversation_id: Uuid) -> Result<i64, DbError> {
    db.call(move |c| {
        c.query_row(
            "SELECT until_ms FROM memory_learned WHERE conversation_id = ?1",
            [conversation_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map(|o| o.unwrap_or(0))
    })
    .await
}

pub async fn set_learned_until(
    db: &Db,
    conversation_id: Uuid,
    until_ms: i64,
) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            "INSERT INTO memory_learned (conversation_id, until_ms) VALUES (?1, ?2)
             ON CONFLICT (conversation_id) DO UPDATE SET until_ms = excluded.until_ms",
            params![conversation_id.to_string(), until_ms],
        )?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::recall::fts_query;

    #[tokio::test]
    async fn notes_are_searchable_and_undoable() {
        let db = Db::open_in_memory().unwrap();
        let rev1 = put(
            &db,
            "people/sam.md",
            None,
            "- Sam is the user's brother\n- Lives in Lyon",
            MemorySource::Assistant,
            None,
        )
        .await
        .unwrap();
        put(
            &db,
            "preferences/food.md",
            None,
            "- Vegetarian\n- Loves Thai curry",
            MemorySource::Learned,
            None,
        )
        .await
        .unwrap();
        put(
            &db,
            PROFILE_PATH,
            None,
            "Name: Vincent",
            MemorySource::You,
            None,
        )
        .await
        .unwrap();

        let hits = search(&db, fts_query("Where does my brother Sam live?"), 5)
            .await
            .unwrap();
        assert_eq!(hits[0].path, "people/sam.md");
        // Accents and case don't matter.
        let hits = search(&db, fts_query("curry thaï"), 5).await.unwrap();
        assert_eq!(hits[0].path, "preferences/food.md");
        // The profile is never a search result: it's always in the prompt anyway.
        assert!(
            search(&db, fts_query("Vincent"), 5)
                .await
                .unwrap()
                .is_empty()
        );

        assert_eq!(list(&db).await.unwrap().len(), 2);
        assert_eq!(
            get(&db, "people/sam.md").await.unwrap().unwrap().title,
            "Sam"
        );
        // A title the user chose survives later updates without one.
        put(
            &db,
            "preferences/food.md",
            Some("What I eat"),
            "- Vegetarian\n- Loves Thai curry",
            MemorySource::You,
            None,
        )
        .await
        .unwrap();
        put(
            &db,
            "preferences/food.md",
            None,
            "- Vegetarian\n- Loves Thai curry\n- Hates olives",
            MemorySource::Assistant,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            get(&db, "preferences/food.md")
                .await
                .unwrap()
                .unwrap()
                .title,
            "What I eat"
        );

        // Change, then undo back to the first version; undo the creation too.
        let rev2 = put(
            &db,
            "people/sam.md",
            None,
            "- Sam moved to Paris",
            MemorySource::Assistant,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            undo(&db, rev2).await.unwrap(),
            Undo::Done("people/sam.md".into())
        );
        assert!(
            get(&db, "people/sam.md")
                .await
                .unwrap()
                .unwrap()
                .body
                .contains("Lyon")
        );
        assert_eq!(undo(&db, rev2).await.unwrap(), Undo::AlreadyUndone);
        undo(&db, rev1).await.unwrap();
        assert!(get(&db, "people/sam.md").await.unwrap().is_none());
        assert!(
            search(&db, fts_query("brother"), 5)
                .await
                .unwrap()
                .is_empty()
        );

        forget_all(&db).await.unwrap();
        assert!(list(&db).await.unwrap().is_empty());
        assert_eq!(profile(&db).await.unwrap(), "");
    }
}
