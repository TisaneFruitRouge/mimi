//! Finding memories by meaning, not only by their words: "what's my sibling's job?"
//! finds the note about a sister, in any of the user's languages.
//!
//! Each library note gets an embedding (a vector of numbers standing for its meaning)
//! from a small multilingual model that runs on this computer: a second `llama-server`
//! in embedding mode, started on demand and unloaded when idle, or the user's Ollama when
//! there's no built-in runtime. Vectors live in the encrypted database next to the notes
//! and are compared by brute force, which is instant for a personal memory. The whole
//! thing is optional: without it, recall works on words alone.

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::future::BoxFuture;
use mimi_protocol::{Event, Locality, MemorySemantic, ProviderKind, PullState};
use rusqlite::params;
use sha2::{Digest, Sha256};
use tokio::process::{Child, Command};
use uuid::Uuid;

use super::PROFILE_PATH;
use crate::db::{Db, DbError};
use crate::hardware::recommend::embedding_model;
use crate::providers::{self, OpenAiCompatible};
use crate::runtime::{self, Endpoint, download};
use crate::{AppState, now_ms};

/// Most characters of a note that are embedded: the model reads about 512 tokens.
const EMBED_CHARS: usize = 1200;
/// Notes embedded per request.
const BATCH: usize = 16;
/// How long recall waits for a message's embedding (starting the model included)
/// before going on with words alone.
const QUERY_TIMEOUT: Duration = Duration::from_secs(6);
/// Loading the embedding model takes a second or two; more means something's wrong.
const START_TIMEOUT: Duration = Duration::from_secs(60);

/// Similarity (cosine) below which a note isn't about the message at all. Measured
/// with the multilingual Granite model: related notes score 0.62-0.72, unrelated
/// ones 0.45-0.59.
pub const MIN_SIMILARITY: f32 = 0.6;
/// Notes much less similar than the best match are left out.
pub const NEAR_BEST: f32 = 0.08;

/// Turns texts into vectors. The real one talks to a local model; tests use a fake.
pub trait Embedder: Send + Sync {
    /// Which model makes the vectors, as stored with them: vectors from another model
    /// aren't comparable and get remade.
    fn model(&self) -> &str;
    fn embed<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<Vec<f32>>, String>>;
}

struct Remote {
    client: OpenAiCompatible,
    /// The model's name for the server.
    name: String,
    /// The name stored with vectors: says which server made them too.
    model: String,
}

impl Embedder for Remote {
    fn model(&self) -> &str {
        &self.model
    }

    fn embed<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<Vec<f32>>, String>> {
        async move {
            self.client
                .embed(&self.name, texts)
                .await
                .map_err(|e| e.to_string())
        }
        .boxed()
    }
}

/// The embedding server, the indexing trigger and the last problem.
#[derive(Default)]
pub struct Semantic {
    server: tokio::sync::Mutex<Option<Server>>,
    last_used: Mutex<Option<Instant>>,
    wake: tokio::sync::Notify,
    error: Mutex<Option<String>>,
}

struct Server {
    port: u16,
    key: String,
    child: Child,
}

impl Semantic {
    /// Asks the background indexer to look for notes to embed.
    pub fn poke(&self) {
        self.wake.notify_one();
    }

    fn set_error(&self, error: Option<String>) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = error;
    }

    fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The built-in embedding server's address, starting it if needed.
    async fn ensure_server(&self, state: &AppState, model: &str) -> Result<Endpoint, String> {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        let mut server = self.server.lock().await;
        if let Some(s) = server.as_mut() {
            if matches!(s.child.try_wait(), Ok(None)) {
                return Ok(Endpoint {
                    url: runtime::api_url(s.port),
                    key: s.key.clone(),
                });
            }
            tracing::warn!("the embedding model had stopped; restarting it");
        }
        if let Some(mut s) = server.take() {
            let _ = s.child.kill().await;
        }

        let binary = state
            .runtime
            .binary()
            .ok_or("Mimi's built-in model runtime isn't installed on this computer.")?
            .to_path_buf();
        let path = download::installed_path(&state.paths, model)
            .ok_or("The language file for finding memories by meaning isn't downloaded yet.")?;
        let port = runtime::free_port().map_err(|e| format!("couldn't find a free port: {e}"))?;
        let key = crate::fsutil::random_hex(24).map_err(|e| e.to_string())?;
        let key_file = state.paths.data_dir.join("embedding-runtime.key");
        crate::fsutil::write_private(&key_file, key.as_bytes()).map_err(|e| e.to_string())?;
        let log = runtime::open_log(&state.paths)
            .map_err(|e| format!("couldn't open the runtime log: {e}"))?;
        tracing::info!(port, "starting the embedding model");
        let mut child = spawn(&binary, &path, port, &key_file, model, log)
            .map_err(|e| format!("The embedding model couldn't start: {e}"))?;
        match tokio::time::timeout(
            START_TIMEOUT,
            runtime::wait_ready(&state.http, port, &mut child),
        )
        .await
        {
            Ok(Ok(())) => {
                *server = Some(Server {
                    port,
                    key: key.clone(),
                    child,
                });
                Ok(Endpoint {
                    url: runtime::api_url(port),
                    key,
                })
            }
            outcome => {
                let _ = child.kill().await;
                let reason = match outcome {
                    Ok(Err(reason)) => reason,
                    _ => "it took too long to start".to_owned(),
                };
                tracing::warn!(
                    "the embedding model failed to start: {reason}\n{}",
                    runtime::log_tail(&state.paths)
                );
                Err("Finding memories by meaning couldn't start. Mimi will try again with the next change.".to_owned())
            }
        }
    }

    /// Stops the embedding server, if it runs.
    pub async fn stop(&self) {
        if let Some(mut s) = self.server.lock().await.take() {
            tracing::info!("stopping the embedding model");
            let _ = s.child.kill().await;
            let _ = s.child.wait().await;
        }
    }

    /// Stops the embedding server after [`runtime::IDLE_UNLOAD`] without use.
    pub async fn unload_if_idle(&self) {
        let idle = self
            .last_used
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|t| t.elapsed() >= runtime::IDLE_UNLOAD);
        if idle {
            self.stop().await;
        }
    }
}

fn spawn(
    binary: &std::path::Path,
    model_path: &std::path::Path,
    port: u16,
    key_file: &std::path::Path,
    alias: &str,
    log: std::fs::File,
) -> std::io::Result<Child> {
    Command::new(binary)
        .arg("--model")
        .arg(model_path)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .arg("--embedding")
        // Each input must fit in one batch; four slots of 512 tokens, the model's
        // window, let a batch of notes go through together.
        .args([
            "--ctx-size",
            "2048",
            "--batch-size",
            "2048",
            "--ubatch-size",
            "2048",
        ])
        .args(["--parallel", "4"])
        // On the CPU: it's small and fast there, and leaves the GPU to the chat model.
        .args(["--n-gpu-layers", "0"])
        .args(["--alias", alias])
        .arg("--api-key-file")
        .arg(key_file)
        .args(["--no-webui", "--offline"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .kill_on_drop(true)
        .spawn()
}

/// Where embeddings can be made on this computer.
enum Backend {
    /// Mimi's own runtime; the built-in source's id, which downloads the model.
    Builtin(Uuid),
    /// The user's Ollama (on this computer or their network, never a cloud service).
    Ollama(providers::store::ProviderRecord),
}

impl Backend {
    fn provider_id(&self) -> Uuid {
        match self {
            Backend::Builtin(id) => *id,
            Backend::Ollama(record) => record.provider.id,
        }
    }

    /// The model name stored with vectors.
    fn stored_model(&self, model: &str) -> String {
        match self {
            Backend::Builtin(_) => format!("builtin/{model}"),
            Backend::Ollama(_) => format!("ollama/{model}"),
        }
    }
}

async fn backend(state: &AppState) -> Option<Backend> {
    if state.runtime.available()
        && let Some(id) = runtime::builtin_source(state).await
    {
        return Some(Backend::Builtin(id));
    }
    let sources = providers::store::list(&state.db).await.ok()?;
    for source in sources {
        if source.kind == ProviderKind::Builtin || source.locality == Locality::Cloud {
            continue;
        }
        let Ok(Some(record)) = providers::store::get(&state.db, source.id).await else {
            continue;
        };
        let Ok(client) = providers::connect(&state.http, &record) else {
            continue;
        };
        if client.is_ollama().await {
            return Some(Backend::Ollama(record));
        }
    }
    None
}

async fn installed(state: &AppState, backend: &Backend, model: &str) -> bool {
    match backend {
        Backend::Builtin(_) => download::installed_path(&state.paths, model).is_some(),
        Backend::Ollama(record) => match providers::connect(&state.http, record) {
            Ok(client) => client.list_models().await.is_ok_and(|models| {
                models
                    .iter()
                    .any(|m| m.id == model || m.id == format!("{model}:latest"))
            }),
            Err(_) => false,
        },
    }
}

/// The embedder to use now, if finding by meaning is on and ready.
pub async fn embedder(state: &AppState) -> Result<Option<Arc<dyn Embedder>>, String> {
    let settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if !settings.memory_semantic {
        return Ok(None);
    }
    let Some(backend) = backend(state).await else {
        return Ok(None);
    };
    let (model, _) = embedding_model();
    if !installed(state, &backend, &model).await {
        return Ok(None);
    }
    let client = match &backend {
        Backend::Builtin(_) => {
            let endpoint = state.semantic.ensure_server(state, &model).await?;
            OpenAiCompatible::new(state.http.clone(), endpoint.url, Some(endpoint.key)).local(true)
        }
        Backend::Ollama(record) => providers::connect(&state.http, record)?.local(true),
    };
    Ok(Some(Arc::new(Remote {
        client,
        name: model.clone(),
        model: backend.stored_model(&model),
    })))
}

/// What the Memory screen shows about finding by meaning.
pub async fn status(state: &AppState) -> MemorySemantic {
    let enabled = crate::settings::load(&state.db)
        .await
        .map(|s| s.memory_semantic)
        .unwrap_or(false);
    let (model, gguf) = embedding_model();
    let backend = backend(state).await;
    let installed = match &backend {
        Some(b) => installed(state, b, &model).await,
        None => false,
    };
    let stored = backend.as_ref().map(|b| b.stored_model(&model));
    let (indexed, total) = counts(&state.db, stored).await.unwrap_or((0, 0));
    MemorySemantic {
        enabled,
        available: backend.is_some(),
        installed,
        download_bytes: gguf.bytes,
        provider_id: backend.as_ref().map(Backend::provider_id),
        model,
        indexed,
        total,
        error: if enabled {
            state.semantic.error()
        } else {
            None
        },
    }
}

/// Turns finding by meaning on or off. Turning it on downloads the model if needed.
pub async fn set_enabled(state: &Arc<AppState>, enabled: bool) -> Result<MemorySemantic, String> {
    let (model, _) = embedding_model();
    let backend = backend(state).await;
    if enabled {
        let Some(backend) = &backend else {
            return Err("Finding memories by meaning needs Mimi's built-in model runtime or Ollama on this computer.".to_owned());
        };
        if !installed(state, backend, &model).await {
            match backend {
                Backend::Builtin(id) => {
                    download::start(state.clone(), *id, model.clone())?;
                }
                Backend::Ollama(record) => {
                    let client = providers::connect(&state.http, record)?;
                    providers::pull::start(
                        state.clone(),
                        client,
                        record.provider.id,
                        model.clone(),
                    );
                }
            }
        }
    }
    let mut settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    settings.memory_semantic = enabled;
    crate::settings::save(&state.db, &settings)
        .await
        .map_err(|e| e.to_string())?;
    state.events.publish(Event::SettingsChanged { settings });
    state.semantic.set_error(None);
    if !enabled {
        state.semantic.stop().await;
    }
    state.semantic.poke();
    state.events.publish(Event::MemoryChanged);
    Ok(status(state).await)
}

/// The message's embedding, for recall, or `None` when finding by meaning is off, not
/// ready, or too slow right now.
pub async fn query_vector(state: &AppState, text: &str) -> Option<(String, Vec<f32>)> {
    let text = clip(text.trim(), EMBED_CHARS);
    if text.is_empty() {
        return None;
    }
    let find = async {
        let embedder = match embedder(state).await {
            Ok(e) => e?,
            Err(e) => {
                tracing::warn!("finding memories by meaning is unavailable: {e}");
                return None;
            }
        };
        match embedder.embed(std::slice::from_ref(&text)).await {
            Ok(mut v) => Some((embedder.model().to_owned(), v.pop()?)),
            Err(e) => {
                tracing::warn!("embedding a message failed: {e}");
                None
            }
        }
    };
    let found = tokio::time::timeout(QUERY_TIMEOUT, find).await;
    if found.is_err() {
        tracing::warn!("embedding a message took too long; recalling by words only");
    }
    found.ok().flatten()
}

/// Runs forever: keeps every note's embedding current and notes linked to the people
/// they're about. Wakes on memory, people and settings changes, and when the embedding
/// model finishes downloading.
pub async fn run(state: Arc<AppState>) {
    let mut events = state.events.subscribe();
    let (model, _) = embedding_model();
    let mut announce = false;
    loop {
        let mut changed = false;
        match super::link::relink(&state, &[]).await {
            Ok(n) => changed |= n > 0,
            Err(e) => tracing::warn!("linking notes to people failed: {e}"),
        }
        match embedder(&state).await {
            Ok(Some(embedder)) => match index(&state.db, embedder.as_ref()).await {
                Ok(n) => {
                    if n > 0 {
                        tracing::info!(notes = n, "embedded memory notes");
                    }
                    changed |= n > 0;
                    if state.semantic.error().is_some() {
                        state.semantic.set_error(None);
                        changed = true;
                    }
                }
                Err(e) => {
                    tracing::warn!("embedding memory notes failed: {e}");
                    state.semantic.set_error(Some(
                        "Some notes couldn't be read for meaning yet. Mimi will try again."
                            .to_owned(),
                    ));
                    changed = true;
                }
            },
            Ok(None) => {}
            Err(e) => {
                state.semantic.set_error(Some(e));
                changed = true;
            }
        }
        if changed || announce {
            state.events.publish(Event::MemoryChanged);
        }
        announce = false;

        // Wait for something that may need work.
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Ok(Event::MemoryChanged | Event::PeopleChanged | Event::SettingsChanged { .. }) => break,
                    Ok(Event::ModelPull { pull }) if pull.model == model => {
                        if pull.state == PullState::Done {
                            announce = true;
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                _ = state.semantic.wake.notified() => break,
            }
        }
        // Let a burst of changes settle, then take them all at once.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        while let Ok(event) = events.try_recv() {
            if let Event::ModelPull { pull } = event
                && pull.model == model
                && pull.state == PullState::Done
            {
                announce = true;
            }
        }
    }
}

/// The text of a note that's embedded, and its hash.
fn note_text(title: &str, body: &str) -> (String, String) {
    let text = clip(&format!("{title}\n{body}"), EMBED_CHARS);
    let hash = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    (text, hash)
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Embeds every note that has no vector from `embedder`'s model, or whose text changed
/// since. Returns how many were embedded.
pub async fn index(db: &Db, embedder: &dyn Embedder) -> Result<usize, String> {
    let model = embedder.model().to_owned();
    let wanted = model.clone();
    let stale: Vec<(String, String, String)> = db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT n.path, n.title, n.body, v.model, v.hash
                 FROM memory_notes n LEFT JOIN memory_vectors v ON v.path = n.path
                 WHERE n.path != ?1 ORDER BY n.updated_at DESC",
            )?;
            let rows = stmt.query_map([PROFILE_PATH], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (path, title, body, model, hash) = row?;
                let (text, new_hash) = note_text(&title, &body);
                if model.as_deref() != Some(wanted.as_str()) || hash.as_deref() != Some(&new_hash) {
                    out.push((path, text, new_hash));
                }
            }
            Ok(out)
        })
        .await
        .map_err(|e| e.to_string())?;

    let mut done = 0;
    for chunk in stale.chunks(BATCH) {
        let texts: Vec<String> = chunk.iter().map(|(_, text, _)| text.clone()).collect();
        let vectors = embedder.embed(&texts).await?;
        if vectors.len() != chunk.len() || vectors.iter().any(Vec::is_empty) {
            return Err("the embedding model gave back the wrong number of vectors".to_owned());
        }
        let rows: Vec<(String, String, Vec<u8>)> = chunk
            .iter()
            .zip(vectors)
            .map(|((path, _, hash), v)| (path.clone(), hash.clone(), to_blob(&normalized(v))))
            .collect();
        let model = model.clone();
        done += db
            .call(move |c| {
                let tx = c.transaction()?;
                let mut n = 0;
                for (path, hash, blob) in rows {
                    // The note may have been deleted meanwhile.
                    n += tx.execute(
                        "INSERT INTO memory_vectors (path, model, hash, vector, updated_at)
                         SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM memory_notes WHERE path = ?1)
                         ON CONFLICT (path) DO UPDATE SET model = excluded.model, hash = excluded.hash,
                             vector = excluded.vector, updated_at = excluded.updated_at",
                        params![path, model, hash, blob, now_ms()],
                    )?;
                }
                tx.commit()?;
                Ok(n)
            })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(done)
}

/// Notes embedded with `model` (current or not), and all notes.
async fn counts(db: &Db, model: Option<String>) -> Result<(u32, u32), DbError> {
    db.call(move |c| {
        let total: u32 = c.query_row(
            "SELECT COUNT(*) FROM memory_notes WHERE path != ?1",
            [PROFILE_PATH],
            |r| r.get(0),
        )?;
        let indexed: u32 = match model {
            Some(m) => c.query_row(
                "SELECT COUNT(*) FROM memory_vectors WHERE model = ?1",
                [m],
                |r| r.get(0),
            )?,
            None => 0,
        };
        Ok((indexed, total))
    })
    .await
}

/// Notes similar in meaning to `query`, best first: at least [`MIN_SIMILARITY`] and
/// within [`NEAR_BEST`] of the best match.
pub async fn similar(
    db: &Db,
    model: &str,
    query: &[f32],
    limit: usize,
) -> Result<Vec<(String, f32)>, DbError> {
    let model = model.to_owned();
    let query = normalized(query.to_vec());
    let vectors: Vec<(String, Vec<u8>)> = db
        .call(move |c| {
            let mut stmt = c.prepare("SELECT path, vector FROM memory_vectors WHERE model = ?1")?;
            stmt.query_map([model], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect()
        })
        .await?;
    let mut scored: Vec<(String, f32)> = vectors
        .into_iter()
        .filter_map(|(path, blob)| {
            let v = from_blob(&blob);
            (v.len() == query.len()).then(|| (path, dot(&query, &v)))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let best = scored.first().map_or(0.0, |s| s.1);
    Ok(scored
        .into_iter()
        .filter(|(_, s)| *s >= MIN_SIMILARITY && *s >= best - NEAR_BEST)
        .take(limit)
        .collect())
}

fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use mimi_protocol::MemorySource;

    use super::*;
    use crate::memory::store;

    /// Places each text on a few "meaning" axes by keywords, in English and French, so
    /// tests can check recall by meaning without a model.
    pub(crate) struct FakeEmbedder {
        pub calls: AtomicUsize,
        pub texts: AtomicUsize,
    }

    impl FakeEmbedder {
        pub(crate) fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                texts: AtomicUsize::new(0),
            }
        }
    }

    const AXES: &[&[&str]] = &[
        &["sister", "sibling", "brother", "sœur", "soeur", "frère"],
        &["job", "nurse", "work", "métier", "travail", "infirmière"],
        &[
            "food",
            "eat",
            "peanut",
            "satay",
            "vegetarian",
            "manger",
            "cacahuète",
        ],
        &["tea", "coffee", "drink", "boire", "thé", "morning", "matin"],
    ];

    impl Embedder for FakeEmbedder {
        fn model(&self) -> &str {
            "fake"
        }

        fn embed<'a>(
            &'a self,
            texts: &'a [String],
        ) -> BoxFuture<'a, Result<Vec<Vec<f32>>, String>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.texts.fetch_add(texts.len(), Ordering::SeqCst);
            let out = texts
                .iter()
                .map(|t| {
                    let t = t.to_lowercase();
                    let tokens: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).collect();
                    // A shared axis, so unrelated texts aren't orthogonal.
                    let mut v = vec![0.0f32; AXES.len() + 1];
                    v[AXES.len()] = 1.0;
                    for (i, words) in AXES.iter().enumerate() {
                        v[i] += words
                            .iter()
                            .filter(|w| tokens.iter().any(|t| t.starts_with(*w)))
                            .count() as f32;
                    }
                    v
                })
                .collect();
            async move { Ok(out) }.boxed()
        }
    }

    async fn note(db: &Db, path: &str, body: &str) {
        store::put(db, path, None, body, MemorySource::You, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn notes_are_embedded_once_and_again_when_they_change() {
        let db = Db::open_in_memory().unwrap();
        note(
            &db,
            "people/léa.md",
            "- Léa is the user's sister\n- Nurse in Geneva",
        )
        .await;
        note(
            &db,
            "preferences/food.md",
            "- Vegetarian\n- Allergic to peanuts",
        )
        .await;
        note(&db, super::super::PROFILE_PATH, "- Name: Vincent").await;
        let fake = FakeEmbedder::new();

        // Backfill: every library note, never the profile.
        assert_eq!(index(&db, &fake).await.unwrap(), 2);
        assert_eq!(counts(&db, Some("fake".into())).await.unwrap(), (2, 2));
        // Nothing changed: nothing to do.
        assert_eq!(index(&db, &fake).await.unwrap(), 0);
        // A changed note is embedded again, alone.
        note(
            &db,
            "preferences/food.md",
            "- Vegetarian\n- Loves Thai curry",
        )
        .await;
        let before = fake.texts.load(Ordering::SeqCst);
        assert_eq!(index(&db, &fake).await.unwrap(), 1);
        assert_eq!(fake.texts.load(Ordering::SeqCst), before + 1);
        // A deleted note takes its vector with it.
        store::delete(&db, "preferences/food.md", None)
            .await
            .unwrap();
        assert_eq!(counts(&db, Some("fake".into())).await.unwrap(), (1, 1));
        store::forget_all(&db).await.unwrap();
        assert_eq!(counts(&db, Some("fake".into())).await.unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn similar_notes_are_found_across_languages() {
        let db = Db::open_in_memory().unwrap();
        note(
            &db,
            "people/léa.md",
            "- Léa is the user's sister\n- Nurse in Geneva",
        )
        .await;
        note(
            &db,
            "preferences/food.md",
            "- Vegetarian\n- Allergic to peanuts",
        )
        .await;
        note(&db, "habits/mornings.md", "- Drinks tea every morning").await;
        let fake = FakeEmbedder::new();
        index(&db, &fake).await.unwrap();

        let query = |q: &str| {
            let q = q.to_owned();
            let fake = &fake;
            let db = &db;
            async move {
                let v = fake.embed(&[q]).await.unwrap().pop().unwrap();
                similar(db, "fake", &v, 4).await.unwrap()
            }
        };
        let hits = query("Quel est le métier de ma sœur ?").await;
        assert_eq!(hits[0].0, "people/léa.md");
        assert_eq!(hits.len(), 1, "{hits:?}");
        let hits = query("Can I eat satay sauce?").await;
        assert_eq!(hits[0].0, "preferences/food.md");
        // Nothing about the weather: nothing found.
        assert!(query("What's the weather tomorrow?").await.is_empty());
        // Vectors from another model are never compared.
        let v = fake.embed(&["sister".into()]).await.unwrap().pop().unwrap();
        assert!(similar(&db, "other", &v, 4).await.unwrap().is_empty());
    }

    #[test]
    fn vectors_survive_the_database() {
        let v = vec![0.25f32, -1.5, 3.0];
        assert_eq!(from_blob(&to_blob(&v)), v);
        let n = normalized(vec![3.0, 4.0]);
        assert!((dot(&n, &n) - 1.0).abs() < 1e-6);
    }
}
