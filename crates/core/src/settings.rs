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

/// Makes a model chosen during setup the default once its download has finished.
pub async fn adopt_pending(state: &crate::AppState, provider_id: uuid::Uuid, model: &str) {
    let Ok(mut current) = load(&state.db).await else {
        return;
    };
    let matches = current
        .pending_model
        .as_ref()
        .is_some_and(|m| m.provider_id == provider_id && m.model == model);
    if !matches {
        return;
    }
    current.default_model = current.pending_model.take();
    if save(&state.db, &current).await.is_ok() {
        tracing::info!(model, "the model chosen during setup is ready");
        state
            .events
            .publish(mimi_protocol::Event::SettingsChanged { settings: current });
    }
}

#[cfg(test)]
mod tests {
    use mimi_protocol::ModelRef;

    use super::*;

    #[tokio::test]
    async fn a_pending_model_becomes_the_default_when_ready() {
        let state = crate::AppState::for_tests("t");
        let provider_id = uuid::Uuid::now_v7();
        let pending = ModelRef {
            provider_id,
            model: "qwen3:4b".into(),
        };
        save(
            &state.db,
            &Settings {
                pending_model: Some(pending.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Another download finishing changes nothing.
        adopt_pending(&state, provider_id, "gemma3:4b").await;
        assert_eq!(load(&state.db).await.unwrap().default_model, None);

        adopt_pending(&state, provider_id, "qwen3:4b").await;
        let s = load(&state.db).await.unwrap();
        assert_eq!(s.default_model, Some(pending));
        assert_eq!(s.pending_model, None);
    }
}
