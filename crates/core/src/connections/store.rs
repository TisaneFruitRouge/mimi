use rusqlite::{OptionalExtension, Row};
use uuid::Uuid;

use crate::db::{Db, DbError, parse_uuid};

/// A connection as stored. `config` holds everything the integration needs, secrets
/// included; the database is encrypted and the config never leaves the daemon.
#[derive(Debug, Clone)]
pub struct ConnectionRow {
    pub id: Uuid,
    pub integration: String,
    pub name: String,
    pub config: serde_json::Value,
    pub created_at: i64,
}

fn from_row(row: &Row) -> rusqlite::Result<ConnectionRow> {
    let config: String = row.get(3)?;
    Ok(ConnectionRow {
        id: parse_uuid(row, 0)?,
        integration: row.get(1)?,
        name: row.get(2)?,
        config: serde_json::from_str(&config).unwrap_or(serde_json::Value::Null),
        created_at: row.get(4)?,
    })
}

pub async fn list(db: &Db) -> Result<Vec<ConnectionRow>, DbError> {
    db.call(|c| {
        let mut stmt = c.prepare(
            "SELECT id, integration, name, config, created_at FROM connections ORDER BY created_at",
        )?;
        stmt.query_map([], from_row)?.collect()
    })
    .await
}

pub async fn get(db: &Db, id: Uuid) -> Result<Option<ConnectionRow>, DbError> {
    db.call(move |c| {
        c.query_row(
            "SELECT id, integration, name, config, created_at FROM connections WHERE id = ?1",
            [id.to_string()],
            from_row,
        )
        .optional()
    })
    .await
}

pub async fn upsert(db: &Db, row: ConnectionRow) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            "INSERT INTO connections (id, integration, name, config, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, config = excluded.config",
            (
                row.id.to_string(),
                row.integration,
                row.name,
                row.config.to_string(),
                row.created_at,
            ),
        )?;
        Ok(())
    })
    .await
}

pub async fn delete(db: &Db, id: Uuid) -> Result<bool, DbError> {
    db.call(move |c| c.execute("DELETE FROM connections WHERE id = ?1", [id.to_string()]))
        .await
        .map(|n| n > 0)
}
