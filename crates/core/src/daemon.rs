//! Process lifecycle: open the database, pick a port, publish the discovery file, serve
//! until signalled.

use std::fs::{self, DirBuilder};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hearth_protocol::{Discovery, Paths};
use tokio::net::TcpListener;

use crate::fsutil::{random_hex, write_private};
use crate::{AppState, api, db, keys};

pub async fn run(paths: Paths) -> anyhow::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&paths.data_dir)
        .with_context(|| format!("creating {}", paths.data_dir.display()))?;

    if let Some(existing) = running_instance(&paths) {
        bail!(
            "hearth is already running (pid {}, port {})",
            existing.pid,
            existing.port
        );
    }

    let (db, key_storage) = {
        let paths = paths.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let db_path = paths.data_dir.join("hearth.db");
            let key = keys::load_or_create(&paths, db_path.exists())?;
            let db = db::Db::open(&db_path, &key.hex)
                .with_context(|| format!("opening {}", db_path.display()))?;
            // SQLite gives the -wal/-shm files the same mode as the database file.
            fs::set_permissions(&db_path, fs::Permissions::from_mode(0o600))?;
            Ok((db, key.storage))
        })
        .await??
    };
    tracing::info!(?key_storage, "database opened");
    let interrupted = crate::chat::store::mark_interrupted(&db).await?;
    if interrupted > 0 {
        tracing::warn!(interrupted, "marked replies cut off by the last shutdown");
    }

    let listener = bind().await?;
    let port = listener.local_addr()?.port();
    let token = random_hex(32)?;

    let discovery_file = paths.discovery_file();
    write_private(
        &discovery_file,
        &serde_json::to_vec(&Discovery {
            pid: std::process::id(),
            port,
            token: token.clone(),
        })?,
    )?;

    let state = Arc::new(AppState {
        paths,
        token,
        started: Instant::now(),
        db,
        key_storage,
        events: crate::events::EventBus::new(),
        http: crate::http_client(),
        generations: Default::default(),
        port,
        login_codes: Default::default(),
        pulls: Default::default(),
        tool_sources: Default::default(),
        approvals: Default::default(),
        connections: Default::default(),
        learner: Default::default(),
        people: Default::default(),
    });
    #[cfg(debug_assertions)]
    if std::env::var(crate::tools::dev::ENV).is_ok_and(|v| v == "1") {
        tracing::warn!("development tools enabled");
        state
            .tool_sources
            .add(Arc::new(crate::tools::dev::DevTools));
    }
    tracing::info!(port, "hearth daemon listening on 127.0.0.1");

    state
        .tool_sources
        .add(Arc::new(crate::connections::calendar::tools::CalendarTools));
    state
        .tool_sources
        .add(Arc::new(crate::memory::tools::MemoryTools));
    crate::connections::start_all(&state).await;
    tokio::spawn(crate::memory::learn::run(state.clone()));
    crate::people::install(&state);

    let served = axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    let _ = fs::remove_file(&discovery_file);
    served?;
    Ok(())
}

/// Default port, so the web interface has a stable address.
pub const DEFAULT_PORT: u16 = 7437;
/// Overrides the port.
pub const PORT_ENV: &str = "HEARTH_PORT";

/// Binds loopback only; remote access will be a separate, explicitly enabled listener.
/// Falls back to a random port if the preferred one is taken.
async fn bind() -> anyhow::Result<TcpListener> {
    let preferred = match std::env::var(PORT_ENV) {
        Ok(v) => v
            .parse()
            .with_context(|| format!("{PORT_ENV}={v} is not a valid port"))?,
        Err(_) => DEFAULT_PORT,
    };
    match TcpListener::bind((Ipv4Addr::LOCALHOST, preferred)).await {
        Ok(listener) => Ok(listener),
        Err(e) => {
            tracing::warn!(
                "port {preferred} is unavailable ({e}); using a random port. The web \
                 interface address will change on every start."
            );
            Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?)
        }
    }
}

/// A previous daemon's discovery file, if that daemon still accepts connections.
fn running_instance(paths: &Paths) -> Option<Discovery> {
    let raw = fs::read(paths.discovery_file()).ok()?;
    let discovery: Discovery = serde_json::from_slice(&raw).ok()?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, discovery.port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    Some(discovery)
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("installing SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    tracing::info!("shutting down");
}
