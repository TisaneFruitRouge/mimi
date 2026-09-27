use mimi_protocol::{Action, ActionStatus, Conversation, Message, MessageStatus, ModelRef};
use rusqlite::{OptionalExtension, Row};
use uuid::Uuid;

use crate::db::{Db, DbError, enum_str, parse_enum, parse_uuid};

fn conversation(row: &Row) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: parse_uuid(row, 0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

const MESSAGE_COLUMNS: &str = "id, conversation_id, role, content, reasoning, status, provider_id, model, locality, error, created_at, actions, mentions";

fn message(row: &Row) -> rusqlite::Result<Message> {
    let provider_id: Option<String> = row.get(6)?;
    let model: Option<String> = row.get(7)?;
    let locality: Option<String> = row.get(8)?;
    Ok(Message {
        id: parse_uuid(row, 0)?,
        conversation_id: parse_uuid(row, 1)?,
        role: parse_enum(row, 2)?,
        content: row.get(3)?,
        reasoning: row.get(4)?,
        status: parse_enum(row, 5)?,
        model: match (provider_id, model) {
            (Some(p), Some(model)) => Some(ModelRef {
                provider_id: p.parse().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?,
                model,
            }),
            _ => None,
        },
        locality: match locality {
            Some(_) => Some(parse_enum(row, 8)?),
            None => None,
        },
        error: row.get(9)?,
        created_at: row.get(10)?,
        actions: {
            let raw: String = row.get(11)?;
            serde_json::from_str(&raw).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    11,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
        },
        // Unreadable mentions only lose their pills, never the message.
        mentions: row
            .get::<_, String>(12)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default(),
    })
}

pub async fn list_conversations(db: &Db) -> Result<Vec<Conversation>, DbError> {
    db.call(|c| {
        let mut stmt = c.prepare(
            "SELECT id, title, created_at, updated_at FROM conversations ORDER BY updated_at DESC",
        )?;
        stmt.query_map([], conversation)?.collect()
    })
    .await
}

pub async fn get_conversation(db: &Db, id: Uuid) -> Result<Option<Conversation>, DbError> {
    db.call(move |c| {
        c.query_row(
            "SELECT id, title, created_at, updated_at FROM conversations WHERE id = ?1",
            [id.to_string()],
            conversation,
        )
        .optional()
    })
    .await
}

pub async fn upsert_conversation(db: &Db, conv: Conversation) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            // Not INSERT OR REPLACE: that deletes the row first, cascading to messages.
            "INSERT INTO conversations (id, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET title = excluded.title, updated_at = excluded.updated_at",
            (conv.id.to_string(), conv.title, conv.created_at, conv.updated_at),
        )?;
        Ok(())
    })
    .await
}

pub async fn delete_conversation(db: &Db, id: Uuid) -> Result<bool, DbError> {
    db.call(move |c| c.execute("DELETE FROM conversations WHERE id = ?1", [id.to_string()]))
        .await
        .map(|n| n > 0)
}

pub async fn messages(db: &Db, conversation_id: Uuid) -> Result<Vec<Message>, DbError> {
    db.call(move |c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1
             ORDER BY created_at, id"
        ))?;
        stmt.query_map([conversation_id.to_string()], message)?
            .collect()
    })
    .await
}

pub async fn upsert_message(db: &Db, m: Message) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            &format!(
                "INSERT INTO messages ({MESSAGE_COLUMNS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT (id) DO UPDATE SET content = excluded.content,
                     reasoning = excluded.reasoning, status = excluded.status,
                     error = excluded.error, actions = excluded.actions"
            ),
            rusqlite::params![
                m.id.to_string(),
                m.conversation_id.to_string(),
                enum_str(m.role),
                m.content,
                m.reasoning,
                enum_str(m.status),
                m.model.as_ref().map(|r| r.provider_id.to_string()),
                m.model.as_ref().map(|r| r.model.clone()),
                m.locality.map(enum_str),
                m.error,
                m.created_at,
                serde_json::to_string(&m.actions).expect("actions serialize"),
                serde_json::to_string(&m.mentions).expect("mentions serialize"),
            ],
        )?;
        Ok(())
    })
    .await
}

/// Messages still marked as streaming belong to a daemon that stopped mid-reply. Their
/// unfinished actions (including ones awaiting approval) are marked failed: nothing is
/// waiting for them anymore, and they must not run later.
pub async fn mark_interrupted(db: &Db) -> Result<usize, DbError> {
    db.call(|c| {
        let tx = c.transaction()?;
        let rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT id, actions FROM messages WHERE status = ?1")?;
            stmt.query_map([enum_str(MessageStatus::Streaming)], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<Result<_, _>>()?
        };
        for (id, raw) in &rows {
            let mut actions: Vec<Action> = serde_json::from_str(raw).unwrap_or_default();
            for a in &mut actions {
                if matches!(
                    a.status,
                    ActionStatus::PendingApproval | ActionStatus::Approved | ActionStatus::Running
                ) {
                    a.status = ActionStatus::Failed;
                    a.error = Some("Mimi stopped before this finished.".to_owned());
                }
            }
            tx.execute(
                "UPDATE messages SET status = ?1, actions = ?2 WHERE id = ?3",
                (
                    enum_str(MessageStatus::Interrupted),
                    serde_json::to_string(&actions).expect("actions serialize"),
                    id,
                ),
            )?;
        }
        tx.commit()?;
        Ok(rows.len())
    })
    .await
}

/// Stores what the model was told about a message's @ mentions, so later turns replay
/// the same context even if the contact or event changes or disappears.
pub async fn set_mention_context(
    db: &Db,
    message_id: Uuid,
    context: String,
) -> Result<(), DbError> {
    db.call(move |c| {
        c.execute(
            "UPDATE messages SET mention_context = ?1 WHERE id = ?2",
            (context, message_id.to_string()),
        )?;
        Ok(())
    })
    .await
}

/// Mention context of every message in a conversation that has one.
pub async fn mention_contexts(
    db: &Db,
    conversation_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, String>, DbError> {
    db.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT id, mention_context FROM messages
             WHERE conversation_id = ?1 AND mention_context IS NOT NULL",
        )?;
        let rows = stmt.query_map([conversation_id.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, context) = row?;
            if let Ok(id) = id.parse() {
                out.insert(id, context);
            }
        }
        Ok(out)
    })
    .await
}
