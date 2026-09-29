//! The encrypted SQLite database (SQLCipher). One connection, used from blocking tasks.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

/// Applied in order; `PRAGMA user_version` records how many have run. Never edit a
/// migration that has shipped: add a new one.
const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_settings.sql"),
    include_str!("migrations/0002_providers.sql"),
    include_str!("migrations/0003_conversations.sql"),
    include_str!("migrations/0004_web_sessions.sql"),
    include_str!("migrations/0005_message_actions.sql"),
    include_str!("migrations/0006_connections.sql"),
    include_str!("migrations/0007_memory.sql"),
    include_str!("migrations/0008_people.sql"),
    include_str!("migrations/0009_rename_to_mimi.sql"),
    include_str!("migrations/0010_onboarding.sql"),
    include_str!("migrations/0011_schedule.sql"),
    include_str!("migrations/0012_memory_vectors.sql"),
    include_str!("migrations/0013_mail.sql"),
    include_str!("migrations/0014_mail_received_on.sql"),
    include_str!("migrations/0015_mail_suspicious.sql"),
    include_str!("migrations/0016_mail_thread_ids.sql"),
    include_str!("migrations/0017_mail_folders.sql"),
    include_str!("migrations/0018_mail_html.sql"),
    include_str!("migrations/0019_mail_folder_looks.sql"),
    include_str!("migrations/0020_recheck_claimed_own_mail.sql"),
    include_str!("migrations/0021_people_removed.sql"),
    include_str!("migrations/0022_calendar_invitations.sql"),
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("the database could not be decrypted with the stored key")]
    WrongKey,
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("database task panicked")]
    Panicked,
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) the database at `path`, encrypted with `key`: 32 bytes
    /// as 64 hex characters.
    pub fn open(path: &Path, key: &str) -> Result<Self, DbError> {
        Self::init(Connection::open(path)?, key)
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?, &"00".repeat(32))
    }

    fn init(mut conn: Connection, key: &str) -> Result<Self, DbError> {
        debug_assert!(key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()));
        // A raw key skips SQLCipher's password KDF; the key is already random.
        conn.execute_batch(&format!("PRAGMA key = \"x'{key}'\";"))?;
        // The first read fails if the key is wrong.
        if conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .is_err()
        {
            return Err(DbError::WrongKey);
        }
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
        )?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` against the connection on the blocking thread pool.
    pub async fn call<R, F>(&self, f: F) -> Result<R, DbError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut conn)
        })
        .await
        .map_err(|_| DbError::Panicked)?
        .map_err(DbError::from)
    }
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in (1i64..).zip(MIGRATIONS).skip(applied as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i)?;
        tx.commit()?;
        tracing::info!(version = i, "applied database migration");
    }
    Ok(())
}

// Enums are stored as their serde names, so the database and the API agree.

pub fn enum_str<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => unreachable!("stored enums serialize to strings"),
    }
}

pub fn parse_enum<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row,
    idx: usize,
) -> rusqlite::Result<T> {
    let raw: String = row.get(idx)?;
    serde_json::from_value(serde_json::Value::String(raw)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub fn parse_uuid(row: &rusqlite::Row, idx: usize) -> rusqlite::Result<uuid::Uuid> {
    let raw: String = row.get(idx)?;
    raw.parse().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_key_is_rejected_and_data_is_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mimi.db");
        let key = "ab".repeat(32);
        {
            let db = Db::open(&path, &key).unwrap();
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('probe', '\"needle-in-plaintext\"')",
                [],
            )
            .unwrap();
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        assert!(matches!(
            Db::open(&path, &"cd".repeat(32)),
            Err(DbError::WrongKey)
        ));
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"needle"));
        assert!(Db::open(&path, &key).is_ok());
    }
}
