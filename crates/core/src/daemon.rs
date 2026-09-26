//! Process lifecycle: pick a port, publish the discovery file, serve until signalled.

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use hearth_protocol::{Discovery, Paths};
use tokio::net::TcpListener;

use crate::{AppState, router};

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

    // Loopback only; remote access will be a separate, explicitly enabled listener.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let token = new_token()?;

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
    });
    tracing::info!(port, "hearth daemon listening on 127.0.0.1");

    let served = axum::serve(listener, router(state))
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

fn new_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("generating token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes via a temp file + rename so readers never see a partial file, with the
/// file created owner-only from the start.
fn write_private(path: &std::path::Path, contents: &[u8]) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
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
