//! Model downloads for the built-in runtime: GGUF files straight from Hugging Face to
//! this computer. Resumable (a `.part` file plus an HTTP range request), verified against
//! the SHA-256 in the catalog, and reported as `ModelPull` events like Ollama downloads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use mimi_protocol::{Event, ModelInfo, ModelPull, Paths, PullState};
use reqwest::StatusCode;
use reqwest::header::RANGE;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::AppState;
use crate::hardware::recommend::{GgufSource, gguf_source, gguf_sources};

/// Where models are downloaded from. Tests point [`Downloads::base`] at a fake.
const HUGGING_FACE: &str = "https://huggingface.co";

/// Free space kept on top of the model itself.
const SPACE_MARGIN: u64 = 512 * 1024 * 1024;

pub fn models_dir(paths: &Paths) -> PathBuf {
    paths.data_dir.join("models")
}

/// The finished file of a catalog model, if it's been downloaded.
pub fn installed_path(paths: &Paths, model: &str) -> Option<PathBuf> {
    let source = gguf_source(model)?;
    let path = models_dir(paths).join(&source.file);
    path.is_file().then_some(path)
}

/// Downloaded models, as the built-in source lists them.
pub fn installed(paths: &Paths) -> Vec<ModelInfo> {
    let dir = models_dir(paths);
    let mut models: Vec<ModelInfo> = gguf_sources()
        .into_iter()
        .filter(|(_, s)| dir.join(&s.file).is_file())
        .map(|(id, s)| ModelInfo {
            id,
            name: None,
            size_bytes: Some(s.bytes),
            supports_tools: None,
            price: None,
            // The built-in runtime runs models without their picture encoder.
            sees_images: Some(false),
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models
}

/// Space taken by downloaded (and partly downloaded) models.
pub fn models_bytes(paths: &Paths) -> u64 {
    std::fs::read_dir(models_dir(paths))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Deletes a model's file and any partial download of it.
pub fn remove(paths: &Paths, model: &str) -> std::io::Result<bool> {
    let Some(source) = gguf_source(model) else {
        return Ok(false);
    };
    let dir = models_dir(paths);
    let mut removed = false;
    for path in [dir.join(&source.file), part_path(&dir, &source)] {
        match std::fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(removed)
}

fn part_path(dir: &Path, source: &GgufSource) -> PathBuf {
    dir.join(format!("{}.part", source.file))
}

/// Running downloads' cancellation handles, by model, and where downloads come from.
pub struct Downloads {
    cancels: Mutex<HashMap<String, CancellationToken>>,
    pub base: Mutex<String>,
}

impl Default for Downloads {
    fn default() -> Self {
        Self {
            cancels: Default::default(),
            base: Mutex::new(HUGGING_FACE.to_owned()),
        }
    }
}

impl Downloads {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CancellationToken>> {
        self.cancels.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn base(&self) -> String {
        self.base.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Stops a running download. Returns whether one was running.
    pub fn cancel(&self, model: &str) -> bool {
        self.lock()
            .get(model)
            .map(CancellationToken::cancel)
            .is_some()
    }
}

/// Starts downloading a catalog model, unless it's already downloading or downloaded.
pub fn start(state: Arc<AppState>, provider_id: Uuid, model: String) -> Result<ModelPull, String> {
    let source =
        gguf_source(&model).ok_or("Mimi doesn't know where to download that model from.")?;
    let key = (provider_id, model.clone());
    if let Some(existing) = state.pulls.get(&key) {
        return Ok(existing);
    }
    if installed_path(&state.paths, &model).is_some() {
        let done_model = model.clone();
        let done = ModelPull {
            provider_id,
            model,
            state: PullState::Done,
            status: "Ready".to_owned(),
            completed_bytes: Some(source.bytes),
            total_bytes: Some(source.bytes),
            error: None,
        };
        state
            .events
            .publish(Event::ModelPull { pull: done.clone() });
        tokio::spawn(async move {
            crate::settings::adopt_pending(&state, provider_id, &done_model).await;
        });
        return Ok(done);
    }
    let pull = ModelPull {
        provider_id,
        model: model.clone(),
        state: PullState::Running,
        status: "Starting download".to_owned(),
        completed_bytes: None,
        total_bytes: Some(source.bytes),
        error: None,
    };
    state.pulls.set(key, pull.clone());
    let cancel = CancellationToken::new();
    state.downloads.lock().insert(model, cancel.clone());
    state
        .events
        .publish(Event::ModelPull { pull: pull.clone() });
    tokio::spawn(run(state, pull.clone(), source, cancel));
    Ok(pull)
}

async fn run(
    state: Arc<AppState>,
    mut pull: ModelPull,
    source: GgufSource,
    cancel: CancellationToken,
) {
    let key = (pull.provider_id, pull.model.clone());
    let outcome = download(&state, &mut pull, &source, &cancel).await;
    match outcome {
        Ok(()) => {
            pull.state = PullState::Done;
            pull.status = "Ready".to_owned();
            pull.completed_bytes = Some(source.bytes);
            tracing::info!(model = %pull.model, "model downloaded");
            crate::settings::adopt_pending(&state, pull.provider_id, &pull.model).await;
        }
        Err(_) if cancel.is_cancelled() => {
            pull.state = PullState::Cancelled;
            pull.status = "Stopped".to_owned();
        }
        Err(e) => {
            tracing::warn!(model = %pull.model, "model download failed: {e}");
            pull.state = PullState::Failed;
            pull.error = Some(e);
        }
    }
    state.pulls.remove(&key);
    state.downloads.lock().remove(&pull.model);
    state.events.publish(Event::ModelPull { pull });
}

async fn download(
    state: &AppState,
    pull: &mut ModelPull,
    source: &GgufSource,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let dir = models_dir(&state.paths);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("Couldn't create the models folder: {e}"))?;
    let part = part_path(&dir, source);
    let mut have = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if have > source.bytes {
        let _ = tokio::fs::remove_file(&part).await;
        have = 0;
    }
    check_space(&dir, source.bytes - have)?;

    // Resuming: the hash has to cover what's already there.
    let mut hasher = Sha256::new();
    if have > 0 {
        publish(
            state,
            pull,
            "Checking what's already downloaded",
            Some(have),
        );
        let mut file = tokio::fs::File::open(&part)
            .await
            .map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
    }

    let base = state.downloads.base();
    let url = format!(
        "{}/{}/resolve/main/{}",
        base.trim_end_matches('/'),
        source.repo,
        source.file
    );
    let mut request = state.http.get(&url);
    if have > 0 {
        request = request.header(RANGE, format!("bytes={have}-"));
    }
    let res = tokio::select! {
        r = request.send() => r.map_err(|_| "Couldn't reach Hugging Face. Check your internet connection.".to_owned())?,
        _ = cancel.cancelled() => return Err("cancelled".to_owned()),
    };
    let append = match res.status() {
        StatusCode::PARTIAL_CONTENT => true,
        StatusCode::OK => {
            // The server ignored the range: start over.
            have = 0;
            hasher = Sha256::new();
            false
        }
        StatusCode::RANGE_NOT_SATISFIABLE if have == source.bytes => true,
        StatusCode::NOT_FOUND => {
            return Err("This model isn't available for download anymore.".to_owned());
        }
        s => {
            return Err(format!(
                "The download didn't start (Hugging Face answered {s}). Try again later."
            ));
        }
    };
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&part)
        .await
        .map_err(|e| format!("Couldn't write the model file: {e}"))?;

    let mut stream = res.bytes_stream();
    let mut last_sent = Instant::now() - Duration::from_secs(1);
    publish(state, pull, "Downloading", Some(have));
    loop {
        let chunk = tokio::select! {
            c = stream.next() => c,
            _ = cancel.cancelled() => {
                let _ = file.flush().await;
                return Err("cancelled".to_owned());
            }
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|_| {
            "The download was interrupted. Start it again to continue where it stopped.".to_owned()
        })?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Couldn't write the model file: {e}"))?;
        hasher.update(&chunk);
        have += chunk.len() as u64;
        if last_sent.elapsed() >= Duration::from_millis(250) {
            last_sent = Instant::now();
            publish(state, pull, "Downloading", Some(have));
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    file.sync_all().await.map_err(|e| e.to_string())?;
    drop(file);

    if have != source.bytes {
        return Err(
            "The download was interrupted. Start it again to continue where it stopped.".to_owned(),
        );
    }
    publish(state, pull, "Checking the download", Some(have));
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !digest.eq_ignore_ascii_case(&source.sha256) {
        let _ = tokio::fs::remove_file(&part).await;
        return Err("The downloaded file was damaged, so it was deleted. Try again.".to_owned());
    }
    tokio::fs::rename(&part, dir.join(&source.file))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn publish(state: &AppState, pull: &mut ModelPull, status: &str, completed: Option<u64>) {
    pull.status = status.to_owned();
    pull.completed_bytes = completed;
    state
        .pulls
        .set((pull.provider_id, pull.model.clone()), pull.clone());
    state
        .events
        .publish(Event::ModelPull { pull: pull.clone() });
}

/// Refuses to start a download that can't fit on the disk.
fn check_space(dir: &Path, needed: u64) -> Result<(), String> {
    let Some(free) = free_space(dir) else {
        return Ok(());
    };
    if free < needed + SPACE_MARGIN {
        return Err(format!(
            "There isn't enough free space: this model needs {:.1} GB and {:.1} GB is free.",
            needed as f64 / 1e9,
            free as f64 / 1e9
        ));
    }
    Ok(())
}

fn free_space(dir: &Path) -> Option<u64> {
    let dir = dir.canonicalize().ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode as Code};
    use axum::response::Response;
    use axum::routing::get;

    use super::*;

    const PAYLOAD_LEN: usize = 300_000;

    fn payload() -> Vec<u8> {
        (0..PAYLOAD_LEN).map(|i| (i * 7 % 251) as u8).collect()
    }

    fn source(sha256: String) -> GgufSource {
        GgufSource {
            repo: "test/model".into(),
            file: "model.gguf".into(),
            bytes: PAYLOAD_LEN as u64,
            sha256,
            quant: "Q4_K_M".into(),
        }
    }

    fn sha(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Serves the payload at /test/model/resolve/main/model.gguf, honouring `Range`.
    async fn fake_hub(data: Arc<Vec<u8>>) -> String {
        async fn serve(State(data): State<Arc<Vec<u8>>>, headers: HeaderMap) -> Response {
            let start = headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("bytes="))
                .and_then(|v| v.trim_end_matches('-').parse::<usize>().ok());
            match start {
                Some(s) => Response::builder()
                    .status(Code::PARTIAL_CONTENT)
                    .body(Body::from(data[s..].to_vec()))
                    .unwrap(),
                None => Response::new(Body::from(data.to_vec())),
            }
        }
        let app = Router::new()
            .route("/test/model/resolve/main/model.gguf", get(serve))
            .with_state(data);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        url
    }

    fn state_in(dir: &Path, hub: String) -> AppState {
        let mut state = AppState::for_tests("t");
        state.paths.data_dir = dir.to_path_buf();
        *state.downloads.base.lock().unwrap() = hub;
        state
    }

    fn pull() -> ModelPull {
        ModelPull {
            provider_id: Uuid::nil(),
            model: "test".into(),
            state: PullState::Running,
            status: String::new(),
            completed_bytes: None,
            total_bytes: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn downloads_verifies_and_resumes() {
        let data = Arc::new(payload());
        let hub = fake_hub(data.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path(), hub);
        let src = source(sha(&data));

        // A previous attempt stopped a third of the way through.
        let models = models_dir(&state.paths);
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(part_path(&models, &src), &data[..PAYLOAD_LEN / 3]).unwrap();

        download(&state, &mut pull(), &src, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(std::fs::read(models.join("model.gguf")).unwrap(), *data);
        assert!(!part_path(&models, &src).exists());
    }

    #[tokio::test]
    async fn damaged_downloads_are_thrown_away() {
        let data = Arc::new(payload());
        let hub = fake_hub(data.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path(), hub);
        let src = source("0".repeat(64));

        let err = download(&state, &mut pull(), &src, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(err.contains("damaged"), "{err}");
        let models = models_dir(&state.paths);
        assert!(!models.join("model.gguf").exists());
        assert!(!part_path(&models, &src).exists());
    }

    #[tokio::test]
    async fn missing_models_are_explained() {
        let hub = fake_hub(Arc::new(payload())).await;
        let dir = tempfile::tempdir().unwrap();
        let state = state_in(dir.path(), hub);
        let mut src = source("0".repeat(64));
        src.repo = "gone/model".into();
        let err = download(&state, &mut pull(), &src, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(err.contains("isn't available"), "{err}");
    }

    #[test]
    fn every_catalog_download_is_pinned() {
        let sources = gguf_sources();
        assert!(sources.len() >= 10);
        for (id, s) in sources {
            assert_eq!(s.sha256.len(), 64, "{id}");
            assert!(s.file.ends_with(".gguf"), "{id}");
            assert!(s.bytes > 100_000_000, "{id}");
        }
    }
}
