//! Model downloads through Ollama, reported to clients as `ModelPull` events.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use mimi_protocol::{Event, ModelPull, PullState};
use serde::Deserialize;
use uuid::Uuid;

use super::OpenAiCompatible;
use crate::AppState;

/// Downloads in progress, by source and model.
#[derive(Default)]
pub struct Pulls(Mutex<HashMap<(Uuid, String), ModelPull>>);

impl Pulls {
    pub fn running(&self) -> Vec<ModelPull> {
        self.lock().values().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(Uuid, String), ModelPull>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Model names as Ollama writes them: `name`, `name:tag`, `namespace/name:tag`.
pub fn valid_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':' | '/'))
}

/// Starts a download unless the same one is already running. Returns its state.
pub fn start(
    state: Arc<AppState>,
    client: OpenAiCompatible,
    provider_id: Uuid,
    model: String,
) -> ModelPull {
    let key = (provider_id, model.clone());
    let mut pulls = state.pulls.lock();
    if let Some(existing) = pulls.get(&key) {
        return existing.clone();
    }
    let pull = ModelPull {
        provider_id,
        model,
        state: PullState::Running,
        status: "Starting download".to_owned(),
        completed_bytes: None,
        total_bytes: None,
        error: None,
    };
    pulls.insert(key, pull.clone());
    drop(pulls);
    state
        .events
        .publish(Event::ModelPull { pull: pull.clone() });
    tokio::spawn(run(state, client, pull.clone()));
    pull
}

#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    status: String,
    total: Option<u64>,
    completed: Option<u64>,
    error: Option<String>,
}

async fn run(state: Arc<AppState>, client: OpenAiCompatible, mut pull: ModelPull) {
    let key = (pull.provider_id, pull.model.clone());
    let outcome: Result<(), String> = async {
        let res = client
            .start_pull(&pull.model)
            .await
            .map_err(|e| e.to_string())?;
        let mut bytes = res.bytes_stream();
        let mut buf = Vec::new();
        let mut last_sent = Instant::now() - Duration::from_secs(1);
        while let Some(chunk) = bytes.next().await {
            buf.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
            while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let Ok(line) = serde_json::from_slice::<Line>(&line) else {
                    continue;
                };
                if let Some(err) = line.error {
                    return Err(err);
                }
                pull.status = friendly_status(&line.status);
                if line.total.is_some() {
                    pull.total_bytes = line.total;
                    pull.completed_bytes = line.completed.or(Some(0));
                }
                // Progress lines arrive many times a second; a few updates are plenty.
                if last_sent.elapsed() >= Duration::from_millis(250) {
                    last_sent = Instant::now();
                    state.pulls.lock().insert(key.clone(), pull.clone());
                    state
                        .events
                        .publish(Event::ModelPull { pull: pull.clone() });
                }
            }
        }
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            pull.state = PullState::Done;
            pull.status = "Ready".to_owned();
            pull.completed_bytes = pull.total_bytes;
        }
        Err(e) => {
            tracing::warn!(model = %pull.model, "model download failed: {e}");
            pull.state = PullState::Failed;
            pull.error = Some(friendly_error(&e));
        }
    }
    state.pulls.lock().remove(&key);
    state.events.publish(Event::ModelPull { pull });
}

fn friendly_error(raw: &str) -> String {
    if raw.contains("file does not exist") || raw.contains("not found") {
        "That model wasn't found. Check its name and try again.".to_owned()
    } else if raw.contains("no space left") {
        "There isn't enough disk space for this model.".to_owned()
    } else if raw.contains("connection") || raw.contains("dial tcp") || raw.contains("timeout") {
        "The download was interrupted. Check your internet connection and try again.".to_owned()
    } else {
        format!("The download failed: {raw}")
    }
}

fn friendly_status(raw: &str) -> String {
    if raw.starts_with("pulling") && raw != "pulling manifest" {
        "Downloading".to_owned()
    } else if raw.starts_with("verifying") {
        "Checking the download".to_owned()
    } else if raw == "pulling manifest" {
        "Starting download".to_owned()
    } else if raw.is_empty() {
        "Working".to_owned()
    } else {
        let mut s = raw.to_owned();
        if let Some(first) = s.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names() {
        assert!(valid_model_name("qwen3:8b"));
        assert!(valid_model_name("hf.co/org/model:Q4_K_M"));
        assert!(!valid_model_name(""));
        assert!(!valid_model_name("rm -rf /"));
        assert!(!valid_model_name("a;b"));
    }
}
