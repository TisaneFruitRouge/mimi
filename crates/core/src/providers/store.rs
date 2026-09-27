use mimi_protocol::Provider;
use rusqlite::{OptionalExtension, Row};
use uuid::Uuid;

use crate::db::{Db, DbError, enum_str, parse_enum, parse_uuid};

/// A provider as stored, including its secret.
#[derive(Clone)]
pub struct ProviderRecord {
    pub provider: Provider,
    pub api_key: Option<String>,
}

const COLUMNS: &str = "id, name, kind, base_url, locality, api_key, created_at";

fn from_row(row: &Row) -> rusqlite::Result<ProviderRecord> {
    let api_key: Option<String> = row.get(5)?;
    Ok(ProviderRecord {
        provider: Provider {
            id: parse_uuid(row, 0)?,
            name: row.get(1)?,
            kind: parse_enum(row, 2)?,
            base_url: row.get(3)?,
            locality: parse_enum(row, 4)?,
            has_api_key: api_key.is_some(),
            created_at: row.get(6)?,
        },
        api_key,
    })
}

pub async fn list(db: &Db) -> Result<Vec<Provider>, DbError> {
    db.call(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM providers ORDER BY created_at"
        ))?;
        stmt.query_map([], from_row)?
            .map(|r| r.map(|rec| rec.provider))
            .collect()
    })
    .await
}

pub async fn get(db: &Db, id: Uuid) -> Result<Option<ProviderRecord>, DbError> {
    db.call(move |c| {
        c.query_row(
            &format!("SELECT {COLUMNS} FROM providers WHERE id = ?1"),
            [id.to_string()],
            from_row,
        )
        .optional()
    })
    .await
}

/// Inserts the record, or updates its mutable fields.
pub async fn upsert(db: &Db, record: ProviderRecord) -> Result<(), DbError> {
    db.call(move |c| {
        let p = &record.provider;
        c.execute(
            &format!(
                "INSERT INTO providers ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name, base_url = excluded.base_url,
                     locality = excluded.locality, api_key = excluded.api_key"
            ),
            (
                p.id.to_string(),
                &p.name,
                enum_str(p.kind),
                &p.base_url,
                enum_str(p.locality),
                &record.api_key,
                p.created_at,
            ),
        )?;
        Ok(())
    })
    .await
}

pub async fn delete(db: &Db, id: Uuid) -> Result<bool, DbError> {
    db.call(move |c| c.execute("DELETE FROM providers WHERE id = ?1", [id.to_string()]))
        .await
        .map(|n| n > 0)
}
