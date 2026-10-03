//! Paired phones (`devices`, migration 0024). A token is shown once, at pairing; only its
//! SHA-256 is kept, together with the endpoint id it's bound to.

use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::db::{Db, DbError};
use crate::fsutil::random_hex;
use crate::now_ms;
use crate::web::hash_token;

#[derive(Debug, Clone)]
pub struct DeviceRow {
    pub id: Uuid,
    pub name: String,
    pub endpoint_id: String,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceRow> {
    let id: String = r.get(0)?;
    Ok(DeviceRow {
        id: Uuid::parse_str(&id).unwrap_or_default(),
        name: r.get(1)?,
        endpoint_id: r.get(2)?,
        created_at: r.get(3)?,
        last_seen_at: r.get(4)?,
    })
}

const COLUMNS: &str = "id, name, endpoint_id, created_at, last_seen_at";

pub async fn list(db: &Db) -> Result<Vec<DeviceRow>, DbError> {
    db.call(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM devices ORDER BY created_at"
        ))?;
        stmt.query_map([], row)?.collect()
    })
    .await
}

pub async fn any(db: &Db) -> Result<bool, DbError> {
    db.call(|c| {
        c.query_row("SELECT 1 FROM devices LIMIT 1", [], |_| Ok(()))
            .optional()
    })
    .await
    .map(|found| found.is_some())
}

pub async fn is_paired(db: &Db, endpoint_id: &str) -> Result<bool, DbError> {
    let endpoint_id = endpoint_id.to_owned();
    db.call(move |c| {
        c.query_row(
            "SELECT 1 FROM devices WHERE endpoint_id = ?1",
            [endpoint_id],
            |_| Ok(()),
        )
        .optional()
    })
    .await
    .map(|found| found.is_some())
}

/// Pairs a phone and returns it with its token (to be sent once, never stored). A phone
/// pairing again with the same key replaces its old entry.
pub async fn create(
    db: &Db,
    name: String,
    endpoint_id: String,
) -> anyhow::Result<(DeviceRow, String)> {
    let token = random_hex(32)?;
    let device = DeviceRow {
        id: Uuid::now_v7(),
        name,
        endpoint_id,
        created_at: now_ms(),
        last_seen_at: Some(now_ms()),
    };
    let hash = hash_token(&token);
    let d = device.clone();
    db.call(move |c| {
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM devices WHERE endpoint_id = ?1",
            [&d.endpoint_id],
        )?;
        tx.execute(
            "INSERT INTO devices (id, name, endpoint_id, token_hash, created_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                d.id.to_string(),
                &d.name,
                &d.endpoint_id,
                hash,
                d.created_at,
                d.last_seen_at,
            ),
        )?;
        tx.commit()
    })
    .await?;
    Ok((device, token))
}

/// The device a token belongs to, if it was issued to this endpoint. A token copied off
/// a phone is useless from anywhere else.
pub async fn authenticate(
    db: &Db,
    token: &str,
    endpoint_id: &str,
) -> Result<Option<Uuid>, DbError> {
    let hash = hash_token(token);
    let endpoint_id = endpoint_id.to_owned();
    db.call(move |c| {
        c.query_row(
            "SELECT id FROM devices WHERE token_hash = ?1 AND endpoint_id = ?2",
            (hash, endpoint_id),
            |r| r.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map(|id| id.and_then(|id| Uuid::parse_str(&id).ok()))
}

/// Removes a phone; returns its endpoint id so its connections can be closed.
pub async fn delete(db: &Db, id: Uuid) -> Result<Option<String>, DbError> {
    db.call(move |c| {
        c.query_row(
            "DELETE FROM devices WHERE id = ?1 RETURNING endpoint_id",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()
    })
    .await
}

pub async fn rename(db: &Db, id: Uuid, name: String) -> Result<bool, DbError> {
    db.call(move |c| {
        c.execute(
            "UPDATE devices SET name = ?2 WHERE id = ?1",
            (id.to_string(), name),
        )
    })
    .await
    .map(|n| n > 0)
}

pub async fn touch(db: &Db, endpoint_id: &str) -> Result<(), DbError> {
    let endpoint_id = endpoint_id.to_owned();
    let now = now_ms();
    db.call(move |c| {
        c.execute(
            "UPDATE devices SET last_seen_at = ?2 WHERE endpoint_id = ?1",
            (endpoint_id, now),
        )
    })
    .await
    .map(drop)
}
