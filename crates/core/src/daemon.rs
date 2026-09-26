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

    // Loopback only; remote access will be a separate, explicitly enabled listener.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
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
    });
    tracing::info!(port, "hearth daemon listening on 127.0.0.1");

    let served = axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    let _ = fs::remove_file(&discovery_file);
    served?;
    Ok(())
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
