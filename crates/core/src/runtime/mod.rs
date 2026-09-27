//! The built-in model runtime: llama.cpp's `llama-server`, shipped with Mimi and managed
//! by the daemon, so a model runs on this computer without installing anything else.
//!
//! One model is loaded at a time. It starts on demand (the first message that needs it),
//! restarts if it crashed, and is unloaded after a while without use to give the memory
//! back. Models are GGUF files downloaded straight from Hugging Face into `models/` in
//! the data directory (see [`download`]).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mimi_protocol::{
    Event, HardwareTier, Locality, Provider, ProviderKind, RuntimeState, RuntimeStatus,
};
use reqwest::Url;
use tokio::process::{Child, Command};
use uuid::Uuid;

use crate::providers::store::{self, ProviderRecord};
use crate::{AppState, now_ms};

pub mod download;

/// Points at a `llama-server` executable, overriding the search (useful in development).
pub const BINARY_ENV: &str = "MIMI_LLAMA_SERVER";

/// A loaded model is unloaded after this long without use.
pub const IDLE_UNLOAD: Duration = Duration::from_secs(20 * 60);

/// Loading a large model from a slow disk can take a while.
const START_TIMEOUT: Duration = Duration::from_secs(300);

/// The source's placeholder address; the real one changes every time the server starts.
const PLACEHOLDER_URL: &str = "http://127.0.0.1";

pub struct Runtime {
    binary: Option<PathBuf>,
    running: tokio::sync::Mutex<Option<Running>>,
    status: Mutex<RuntimeStatus>,
    last_used: Mutex<Instant>,
}

struct Running {
    model: String,
    port: u16,
    /// Required on every request, so other local programs and websites (which can reach
    /// loopback ports from a browser) can't use the model.
    key: String,
    child: Child,
}

/// Where a running model answers, and the key it requires.
pub struct Endpoint {
    pub url: Url,
    pub key: String,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::with_binary(find_binary())
    }
}

impl Runtime {
    pub fn with_binary(binary: Option<PathBuf>) -> Self {
        Self {
            status: Mutex::new(RuntimeStatus {
                available: binary.is_some(),
                state: RuntimeState::Idle,
                model: None,
                error: None,
                models_bytes: 0,
            }),
            binary,
            running: tokio::sync::Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
        }
    }

    pub fn available(&self) -> bool {
        self.binary.is_some()
    }

    pub fn status(&self, state: &AppState) -> RuntimeStatus {
        let mut s = self
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        s.models_bytes = download::models_bytes(&state.paths);
        s
    }

    fn set_status(&self, state: &AppState, update: impl FnOnce(&mut RuntimeStatus)) {
        {
            let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner());
            update(&mut s);
        }
        state.events.publish(Event::RuntimeChanged {
            runtime: self.status(state),
        });
    }

    /// The address of a server running `model`, starting (or restarting) it if needed.
    pub async fn ensure(&self, state: &AppState, model: &str) -> Result<Endpoint, String> {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        let mut running = self.running.lock().await;
        if let Some(r) = running.as_mut() {
            let alive = matches!(r.child.try_wait(), Ok(None));
            if alive && r.model == model {
                return Ok(Endpoint {
                    url: api_url(r.port),
                    key: r.key.clone(),
                });
            }
            if !alive {
                tracing::warn!(model = %r.model, "the model runtime had stopped; restarting it");
            }
        }
        if let Some(r) = running.take() {
            shut_down(r).await;
        }

        let binary = self
            .binary
            .clone()
            .ok_or("Mimi's built-in model runtime isn't installed on this computer.")?;
        let path = download::installed_path(&state.paths, model)
            .ok_or("That model isn't on this computer yet. Download it in Models.")?;
        let context = context_size().await;
        let port = free_port().map_err(|e| format!("couldn't find a free port: {e}"))?;
        let key = crate::fsutil::random_hex(24).map_err(|e| e.to_string())?;
        // In an owner-only file rather than on the command line, where other users of
        // this computer could read it.
        let key_file = state.paths.data_dir.join("model-runtime.key");
        crate::fsutil::write_private(&key_file, key.as_bytes()).map_err(|e| e.to_string())?;
        let log =
            open_log(&state.paths).map_err(|e| format!("couldn't open the runtime log: {e}"))?;

        self.set_status(state, |s| {
            s.state = RuntimeState::Starting;
            s.model = Some(model.to_owned());
            s.error = None;
        });
        tracing::info!(model, context, port, "starting the built-in model runtime");
        let spawned = Command::new(&binary)
            .arg("--model")
            .arg(&path)
            .args(["--host", "127.0.0.1", "--port", &port.to_string()])
            .args(["--ctx-size", &context.to_string()])
            // One conversation at a time gets the whole context.
            .args(["--parallel", "1"])
            .args(["--alias", model])
            .arg("--api-key-file")
            .arg(&key_file)
            .args(["--no-webui", "--offline", "--no-slots"])
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .kill_on_drop(true)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                let message = format!("The built-in model runtime couldn't start: {e}");
                self.set_status(state, |s| {
                    s.state = RuntimeState::Failed;
                    s.error = Some(message.clone());
                });
                return Err(message);
            }
        };

        match wait_ready(&state.http, port, &mut child).await {
            Ok(()) => {
                self.set_status(state, |s| s.state = RuntimeState::Ready);
                *running = Some(Running {
                    model: model.to_owned(),
                    port,
                    key: key.clone(),
                    child,
                });
                Ok(Endpoint {
                    url: api_url(port),
                    key,
                })
            }
            Err(reason) => {
                let _ = child.kill().await;
                tracing::warn!(
                    model,
                    "the model runtime failed to start: {reason}\n{}",
                    log_tail(&state.paths)
                );
                let message = friendly_start_error(&reason, &log_tail(&state.paths));
                self.set_status(state, |s| {
                    s.state = RuntimeState::Failed;
                    s.error = Some(message.clone());
                });
                Err(message)
            }
        }
    }

    /// Unloads the model, if any.
    pub async fn stop(&self, state: &AppState) {
        if let Some(r) = self.running.lock().await.take() {
            tracing::info!(model = %r.model, "unloading the built-in model");
            shut_down(r).await;
            self.set_status(state, |s| {
                s.state = RuntimeState::Idle;
                s.model = None;
            });
        }
    }

    /// Unloads `model` if it's the one loaded (e.g. before deleting its file).
    pub async fn stop_model(&self, state: &AppState, model: &str) {
        let loaded = self
            .running
            .lock()
            .await
            .as_ref()
            .is_some_and(|r| r.model == model);
        if loaded {
            self.stop(state).await;
        }
    }

    /// Unloads the model after [`IDLE_UNLOAD`] without use. Called periodically.
    pub async fn unload_if_idle(&self, state: &AppState) {
        let idle = self
            .last_used
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed();
        // Never unload while a reply may be streaming: generations keep `last_used`
        // fresh only at their start, so also check nothing is being written.
        if idle >= IDLE_UNLOAD && !state.generations.any() {
            self.stop(state).await;
        }
    }
}

fn api_url(port: u16) -> Url {
    Url::parse(&format!("http://127.0.0.1:{port}/v1")).expect("valid URL")
}

async fn shut_down(mut r: Running) {
    // llama-server exits cleanly on SIGTERM; kill() sends SIGKILL, which is fine too
    // since it holds nothing that needs flushing.
    let _ = r.child.kill().await;
    let _ = r.child.wait().await;
}

async fn wait_ready(http: &reqwest::Client, port: u16, child: &mut Child) -> Result<(), String> {
    let health = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + START_TIMEOUT;
    while Instant::now() < deadline {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("it exited ({status})"));
        }
        if let Ok(res) = http
            .get(&health)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            && res.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Err("it took too long to load the model".to_owned())
}

fn friendly_start_error(reason: &str, log: &str) -> String {
    let log = log.to_lowercase();
    if log.contains("out of memory")
        || log.contains("failed to allocate")
        || log.contains("cannot allocate")
    {
        "This model needs more memory than this computer has free. Try a smaller model, or close some apps.".to_owned()
    } else if log.contains("failed to load model")
        || log.contains("invalid magic")
        || log.contains("gguf") && log.contains("error")
    {
        "The model file couldn't be loaded. Remove it in Models and download it again.".to_owned()
    } else if reason.contains("too long") {
        "The model took too long to load. Try again, or pick a smaller model.".to_owned()
    } else {
        "The built-in model runtime stopped unexpectedly. Try again in a moment.".to_owned()
    }
}

/// A context window that suits the machine: small machines keep memory for the model.
async fn context_size() -> u32 {
    let hw = tokio::task::spawn_blocking(crate::hardware::detect).await;
    let tier = hw
        .map(|hw| crate::hardware::recommend::tier(crate::hardware::recommend::budget(&hw)))
        .unwrap_or(HardwareTier::Light);
    match tier {
        HardwareTier::Minimal => 4096,
        HardwareTier::Light => 8192,
        HardwareTier::Standard => 16384,
        HardwareTier::Strong | HardwareTier::Workstation => 32768,
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn log_path(paths: &mimi_protocol::Paths) -> PathBuf {
    paths.data_dir.join("logs").join("model-runtime.log")
}

/// The runtime's log, started afresh when it grows past a few megabytes.
fn open_log(paths: &mimi_protocol::Paths) -> std::io::Result<fs::File> {
    let path = log_path(paths);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let too_big = fs::metadata(&path).is_ok_and(|m| m.len() > 5 * 1024 * 1024);
    fs::OpenOptions::new()
        .create(true)
        .append(!too_big)
        .write(true)
        .truncate(too_big)
        .open(path)
}

/// The last lines of the runtime's log, for diagnostics.
fn log_tail(paths: &mimi_protocol::Paths) -> String {
    let text = fs::read_to_string(log_path(paths)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(20)..].join("\n")
}

/// Finds `llama-server`: an explicit override, then next to this executable in the
/// places installers put it, then `PATH`.
pub fn find_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(BINARY_ENV) {
        let p = PathBuf::from(p);
        return is_executable(&p).then_some(p);
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    candidates(&exe_dir)
        .into_iter()
        .chain(path_candidates())
        .find(|p| is_executable(p))
}

/// Where bundles put the runtime relative to the directory holding `mimid`:
/// - macOS app: `Mimi.app/Contents/MacOS/mimid` → `Contents/Resources/llama/`
/// - Linux .deb/.rpm/AppImage: `…/bin/mimid` → `…/lib/Mimi/llama/` (the product name)
/// - development: a `llama/` folder or a `llama-server` next to the binary.
fn candidates(exe_dir: &Path) -> Vec<PathBuf> {
    vec![
        exe_dir.join("llama/llama-server"),
        exe_dir.join("llama-server"),
        exe_dir.join("../Resources/llama/llama-server"),
        exe_dir.join("../lib/Mimi/llama/llama-server"),
    ]
}

fn path_candidates() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("llama-server"))
                .collect()
        })
        .unwrap_or_default()
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The name users see for the built-in source.
pub const SOURCE_NAME: &str = "Built into Mimi";

/// Makes sure the built-in source exists when the runtime is present. Called at startup.
pub async fn ensure_source(state: &AppState) -> Option<Uuid> {
    let existing = builtin_source(state).await;
    if existing.is_some() || !state.runtime.available() {
        return existing;
    }
    let record = ProviderRecord {
        provider: Provider {
            id: Uuid::now_v7(),
            name: SOURCE_NAME.to_owned(),
            kind: ProviderKind::Builtin,
            base_url: PLACEHOLDER_URL.to_owned(),
            locality: Locality::Device,
            has_api_key: false,
            created_at: now_ms(),
        },
        api_key: None,
    };
    let id = record.provider.id;
    match store::upsert(&state.db, record).await {
        Ok(()) => {
            if let Ok(providers) = store::list(&state.db).await {
                state.events.publish(Event::ProvidersChanged { providers });
            }
            Some(id)
        }
        Err(e) => {
            tracing::error!("couldn't add the built-in model source: {e}");
            None
        }
    }
}

/// The built-in source's id, if it exists.
pub async fn builtin_source(state: &AppState) -> Option<Uuid> {
    store::list(&state.db)
        .await
        .ok()?
        .into_iter()
        .find(|p| p.kind == ProviderKind::Builtin)
        .map(|p| p.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_for_the_runtime_where_bundles_put_it() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("usr/bin");
        let lib = dir.path().join("usr/lib/Mimi/llama");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&lib).unwrap();
        let server = lib.join("llama-server");
        fs::write(&server, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&server, fs::Permissions::from_mode(0o755)).unwrap();
        let found = candidates(&bin)
            .into_iter()
            .find(|p| is_executable(p))
            .unwrap();
        assert_eq!(
            found.canonicalize().unwrap(),
            server.canonicalize().unwrap()
        );
    }

    #[test]
    fn start_errors_are_explained() {
        assert!(
            friendly_start_error("it exited", "ggml_vulkan: failed to allocate buffer")
                .contains("memory")
        );
        assert!(
            friendly_start_error(
                "it exited",
                "llama_model_load: error loading model: failed to load model"
            )
            .contains("Remove it")
        );
        assert!(
            friendly_start_error("it took too long to load the model", "").contains("too long")
        );
    }
}
