//! Reminders, routines, their delivery history and undo, in the encrypted database.

use mimi_protocol::{Delivery, DeliveryStatus, Schedule, ScheduleKind};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::{Db, DbError, enum_str, parse_enum, parse_uuid};

/// An item as stored, with the fields the scheduler maintains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: Uuid,
    pub kind: ScheduleKind,
    pub title: String,
    pub instruction: Option<String>,
    pub schedule: Schedule,
    pub paused: bool,
    pub next_at: Option<i64>,
    pub snoozed_until: Option<i64>,
    pub last_at: Option<i64>,
    pub ended: Option<String>,
    pub event_start: Option<i64>,
    pub conversation_id: Option<Uuid>,
    pub created_in: Option<Uuid>,
    pub anchor_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// Set up for someone the user trusts (`access`): it reaches them, not the user,
    /// and only their turns see it. Never changes.
    #[serde(default)]
    pub for_person: Option<Uuid>,
}

impl Item {
    /// When it next needs attention: a snooze or the schedule, whichever is first.
    pub fn wake_at(&self) -> Option<i64> {
        if self.paused || self.ended.is_some() {
            return None;
        }
        match (self.next_at, self.snoozed_until) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

const COLUMNS: &str = "id, kind, title, instruction, schedule, paused, next_at, snoozed_until, \
    last_at, ended, event_start, conversation_id, created_in, anchor_at, created_at, updated_at, \
    for_person";

fn opt_uuid(row: &Row, idx: usize) -> rusqlite::Result<Option<Uuid>> {
    let raw: Option<String> = row.get(idx)?;
    Ok(raw.and_then(|s| s.parse().ok()))
}

fn item(row: &Row) -> rusqlite::Result<Item> {
    let schedule: String = row.get(4)?;
    Ok(Item {
        id: parse_uuid(row, 0)?,
        kind: parse_enum(row, 1)?,
        title: row.get(2)?,
        instruction: row.get(3)?,
        schedule: serde_json::from_str(&schedule).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?,
        paused: row.get(5)?,
        next_at: row.get(6)?,
        snoozed_until: row.get(7)?,
        last_at: row.get(8)?,
        ended: row.get(9)?,
        event_start: row.get(10)?,
        conversation_id: opt_uuid(row, 11)?,
        created_in: opt_uuid(row, 12)?,
        anchor_at: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        for_person: opt_uuid(row, 16)?,
    })
}

pub async fn list(db: &Db) -> Result<Vec<Item>, DbError> {
    db.call(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {COLUMNS} FROM schedule_items ORDER BY COALESCE(next_at, 9e18), created_at"
        ))?;
        stmt.query_map([], item)?.collect()
    })
    .await
}

pub async fn get(db: &Db, id: Uuid) -> Result<Option<Item>, DbError> {
    db.call(move |c| {
        c.query_row(
            &format!("SELECT {COLUMNS} FROM schedule_items WHERE id = ?1"),
            [id.to_string()],
            item,
        )
        .optional()
    })
    .await
}

pub async fn upsert(db: &Db, i: Item) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            &format!(
                "INSERT INTO schedule_items ({COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, title = excluded.title,
                   instruction = excluded.instruction, schedule = excluded.schedule,
                   paused = excluded.paused, next_at = excluded.next_at,
                   snoozed_until = excluded.snoozed_until, last_at = excluded.last_at,
                   ended = excluded.ended, event_start = excluded.event_start,
                   conversation_id = excluded.conversation_id, anchor_at = excluded.anchor_at,
                   updated_at = excluded.updated_at"
            ),
            params![
                i.id.to_string(),
                enum_str(i.kind),
                i.title,
                i.instruction,
                serde_json::to_string(&i.schedule).expect("schedules serialize"),
                i.paused,
                i.next_at,
                i.snoozed_until,
                i.last_at,
                i.ended,
                i.event_start,
                i.conversation_id.map(|u| u.to_string()),
                i.created_in.map(|u| u.to_string()),
                i.anchor_at,
                i.created_at,
                i.updated_at,
                i.for_person.map(|u| u.to_string()),
            ],
        )?;
        Ok(())
    })
    .await
}

pub async fn delete(db: &Db, id: Uuid) -> Result<bool, DbError> {
    db.call(move |c| c.execute("DELETE FROM schedule_items WHERE id = ?1", [id.to_string()]))
        .await
        .map(|n| n > 0)
}

/// The earliest moment anything needs attention.
pub async fn next_wake(db: &Db) -> Result<Option<i64>, DbError> {
    db.call(|c| {
        c.query_row(
            "SELECT MIN(MIN(COALESCE(next_at, 9e18), COALESCE(snoozed_until, 9e18)))
             FROM schedule_items WHERE paused = 0 AND ended IS NULL",
            [],
            |r| r.get::<_, Option<f64>>(0),
        )
    })
    .await
    .map(|v| v.filter(|t| *t < 9e18).map(|t| t as i64))
}

// --- Deliveries ------------------------------------------------------------------------

const DELIVERY_COLUMNS: &str =
    "d.id, d.item_id, i.kind, i.title, d.due_at, d.at, d.status, d.detail, d.conversation_id";

fn delivery(row: &Row) -> rusqlite::Result<Delivery> {
    Ok(Delivery {
        id: parse_uuid(row, 0)?,
        item_id: parse_uuid(row, 1)?,
        kind: parse_enum(row, 2)?,
        title: row.get(3)?,
        due_at: row.get(4)?,
        at: row.get(5)?,
        status: parse_enum(row, 6)?,
        detail: row.get(7)?,
        conversation_id: opt_uuid(row, 8)?,
    })
}

pub async fn record(db: &Db, d: Delivery) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            "INSERT INTO schedule_deliveries (id, item_id, due_at, at, status, detail, conversation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET status = excluded.status, detail = excluded.detail,
               at = excluded.at, conversation_id = excluded.conversation_id",
            params![
                d.id.to_string(),
                d.item_id.to_string(),
                d.due_at,
                d.at,
                enum_str(d.status),
                d.detail,
                d.conversation_id.map(|u| u.to_string()),
            ],
        )?;
        Ok(())
    })
    .await
}

pub async fn delivery_by_id(db: &Db, id: Uuid) -> Result<Option<Delivery>, DbError> {
    db.call(move |c| {
        c.query_row(
            &format!(
                "SELECT {DELIVERY_COLUMNS} FROM schedule_deliveries d
                 JOIN schedule_items i ON i.id = d.item_id WHERE d.id = ?1"
            ),
            [id.to_string()],
            delivery,
        )
        .optional()
    })
    .await
}

/// The owner's latest deliveries (not those of people they trust).
pub async fn recent(db: &Db, limit: usize) -> Result<Vec<Delivery>, DbError> {
    db.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {DELIVERY_COLUMNS} FROM schedule_deliveries d
             JOIN schedule_items i ON i.id = d.item_id WHERE i.for_person IS NULL
             ORDER BY d.at DESC LIMIT ?1"
        ))?;
        stmt.query_map([limit as i64], delivery)?.collect()
    })
    .await
}

/// The owner's deliveries that were due in `[from, to)`, oldest first, at most `limit`.
pub async fn due_between(
    db: &Db,
    from: i64,
    to: i64,
    limit: usize,
) -> Result<Vec<Delivery>, DbError> {
    db.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {DELIVERY_COLUMNS} FROM schedule_deliveries d
             JOIN schedule_items i ON i.id = d.item_id
             WHERE d.due_at >= ?1 AND d.due_at < ?2 AND i.for_person IS NULL
             ORDER BY d.due_at, d.at LIMIT ?3"
        ))?;
        stmt.query_map(params![from, to, limit as i64], delivery)?
            .collect()
    })
    .await
}

pub async fn set_status(
    db: &Db,
    id: Uuid,
    status: DeliveryStatus,
    detail: Option<String>,
) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            "UPDATE schedule_deliveries SET status = ?2, detail = COALESCE(?3, detail) WHERE id = ?1",
            params![id.to_string(), enum_str(status), detail],
        )?;
        Ok(())
    })
    .await
}

/// Routine runs left "running" by a daemon that stopped.
pub async fn fail_unfinished(db: &Db) -> Result<usize, DbError> {
    db.call(|c| {
        c.execute(
            "UPDATE schedule_deliveries SET status = ?1, detail = 'Stopped when Mimi quit.' WHERE status = ?2",
            (
                enum_str(DeliveryStatus::Failed),
                enum_str(DeliveryStatus::Running),
            ),
        )
    })
    .await
}

// --- Undo ------------------------------------------------------------------------------

/// Records how an item was before a change the assistant made. Returns the revision id.
pub async fn remember_before(
    db: &Db,
    item_id: Uuid,
    before: Option<Item>,
    conversation_id: Uuid,
) -> Result<i64, DbError> {
    let before = before.map(|b| serde_json::to_string(&b).expect("items serialize"));
    db.call(move |c| {
        c.execute(
            "INSERT INTO schedule_revisions (item_id, before, conversation_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![item_id.to_string(), before, conversation_id.to_string(), crate::now_ms()],
        )?;
        Ok(c.last_insert_rowid())
    })
    .await
}

pub enum Undo {
    /// The item as it should be again (`None` = it shouldn't exist).
    Restore {
        item_id: Uuid,
        before: Option<Box<Item>>,
        conversation_id: Option<Uuid>,
    },
    AlreadyUndone,
    Unknown,
}

/// Takes a revision for undoing and marks it used.
pub async fn take_revision(db: &Db, id: i64) -> Result<Undo, DbError> {
    db.call(move |c| {
        let row: Option<(String, Option<String>, Option<String>, bool)> = c
            .query_row(
                "SELECT item_id, before, conversation_id, undone FROM schedule_revisions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((item_id, before, conversation, undone)) = row else {
            return Ok(Undo::Unknown);
        };
        if undone {
            return Ok(Undo::AlreadyUndone);
        }
        c.execute("UPDATE schedule_revisions SET undone = 1 WHERE id = ?1", [id])?;
        Ok(Undo::Restore {
            item_id: item_id.parse().unwrap_or_default(),
            before: before.and_then(|b| serde_json::from_str(&b).ok()).map(Box::new),
            conversation_id: conversation.and_then(|c| c.parse().ok()),
        })
    })
    .await
}
