use mimi_protocol::Settings;
use rusqlite::OptionalExtension;

use crate::db::{Db, DbError};

const KEY: &str = "app";

pub async fn load(db: &Db) -> Result<Settings, DbError> {
    let raw: Option<String> = db
        .call(|c| {
            c.query_row("SELECT value FROM settings WHERE key = ?1", [KEY], |r| {
                r.get(0)
            })
            .optional()
        })
        .await?;
    Ok(raw
        .and_then(|raw| {
            serde_json::from_str(&raw)
                .inspect_err(|e| {
                    tracing::error!("stored settings are unreadable, using defaults: {e}")
                })
                .ok()
        })
        .unwrap_or_default())
}

pub async fn save(db: &Db, settings: &Settings) -> Result<(), DbError> {
    let raw = serde_json::to_string(settings).expect("settings serialize");
    db.call(move |c| {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            (KEY, raw),
        )
    })
    .await?;
    Ok(())
}
