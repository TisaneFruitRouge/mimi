//! Makes sure a daemon is available while the app runs, so the user never starts
//! anything by hand.
//!
//! On launch: a daemon that's already reachable is used as is (the background service,
//! or `pnpm dev`'s). Otherwise the background service is started if it's installed, and
//! failing that the app runs `mimid` itself as a child, restarts it if it crashes, and
//! stops it on quit. With `MIMI_DAEMON=external` (set by `pnpm dev`) the app only ever
//! connects.

use std::fs::OpenOptions;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mimi_client::Client;
use mimi_protocol::Paths;
use mimi_service::Spec;

/// Who keeps the daemon running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// Not decided yet (still starting).
    Unknown,
    /// Someone else: `pnpm dev`, or a daemon the user started themselves.
    External,
    /// The login service (systemd / launchd / autostart).
    Service,
    /// This app, as a child process.
    App,
}

/// The background switch's state, for Settings.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackgroundStatus {
    /// Whether the switch can be used here.
    pub available: bool,
    /// Plain-language reason when it can't.
    pub unavailable_reason: Option<String>,
    /// Starts at login and keeps running with no window open.
    pub enabled: bool,
    /// Nothing restarts it after a crash (XDG autostart fallback).
    pub no_restart: bool,
}

pub struct DaemonProcess {
    owner: Mutex<Owner>,
    child: Mutex<Option<Child>>,
    quitting: Mutex<bool>,
}

impl Default for DaemonProcess {
    fn default() -> Self {
        Self {
            owner: Mutex::new(Owner::Unknown),
            child: Mutex::new(None),
            quitting: Mutex::new(false),
        }
    }
}

async fn reachable() -> bool {
    match Client::local() {
        Ok(client) => client.health().await.is_ok(),
        Err(_) => false,
    }
}

async fn wait_until(up: bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if reachable().await == up {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

fn spec() -> Result<Spec, String> {
    let binary =
        mimi_service::daemon_binary().ok_or_else(|| mimi_service::Error::NoBinary.to_string())?;
    Spec::current(binary).map_err(|e| e.to_string())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl DaemonProcess {
    pub fn owner(&self) -> Owner {
        *lock(&self.owner)
    }

    /// Brings a daemon up if none is running. Call once at launch.
    pub async fn ensure_running(&self) {
        if reachable().await {
            let service = spec()
                .ok()
                .and_then(|s| mimi_service::status(&s).ok())
                .is_some_and(|s| s.installed);
            *lock(&self.owner) = if service {
                Owner::Service
            } else {
                Owner::External
            };
            return;
        }
        if mimi_service::daemon_is_external() {
            *lock(&self.owner) = Owner::External;
            return;
        }
        match spec() {
            Ok(spec) => {
                let installed = mimi_service::status(&spec).is_ok_and(|s| s.installed);
                if installed
                    && mimi_service::start(&spec).is_ok()
                    && wait_until(true, Duration::from_secs(15)).await
                {
                    *lock(&self.owner) = Owner::Service;
                    return;
                }
                self.spawn_child(&spec);
                *lock(&self.owner) = Owner::App;
            }
            Err(e) => eprintln!("mimi: can't start the assistant: {e}"),
        }
    }

    /// Runs the daemon as a child, logging to `daemon.log` in its data directory.
    fn spawn_child(&self, spec: &Spec) {
        let log = Paths::resolve().ok().and_then(|p| {
            let _ = std::fs::create_dir_all(&p.data_dir);
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(p.data_dir.join("daemon.log"))
                .ok()
        });
        let (out, err) = match log.and_then(|f| f.try_clone().ok().map(|g| (f, g))) {
            Some((f, g)) => (Stdio::from(f), Stdio::from(g)),
            None => (Stdio::null(), Stdio::null()),
        };
        match Command::new(&spec.binary)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .spawn()
        {
            Ok(child) => *lock(&self.child) = Some(child),
            Err(e) => eprintln!("mimi: couldn't start {}: {e}", spec.binary.display()),
        }
    }

    /// Restarts the app's own daemon if it dies, with a small backoff. Runs forever.
    pub async fn supervise(&self) {
        let mut recent: Vec<Instant> = Vec::new();
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if *lock(&self.quitting) || self.owner() != Owner::App {
                continue;
            }
            let exited = lock(&self.child)
                .as_mut()
                .is_some_and(|c| c.try_wait().ok().flatten().is_some());
            if !exited {
                continue;
            }
            recent.retain(|t| t.elapsed() < Duration::from_secs(60));
            if recent.len() >= 5 {
                // Crashing in a loop: stop trying until the next launch.
                continue;
            }
            recent.push(Instant::now());
            if let Ok(spec) = spec() {
                self.spawn_child(&spec);
            }
        }
    }

    /// Stops the app's own daemon, cleanly if it can. Leaves others alone.
    pub fn stop_child(&self) {
        let Some(mut child) = lock(&self.child).take() else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        mimi_service::terminate(child.id());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Called when the app is quitting for good.
    pub fn quit(&self) {
        *lock(&self.quitting) = true;
        if self.owner() == Owner::App {
            self.stop_child();
        }
    }

    pub fn background_status(&self) -> BackgroundStatus {
        let unavailable = |reason: &str| BackgroundStatus {
            available: false,
            unavailable_reason: Some(reason.to_owned()),
            enabled: false,
            no_restart: false,
        };
        if mimi_service::daemon_is_external() {
            return unavailable("While developing, the assistant is started by pnpm dev.");
        }
        let spec = match spec() {
            Ok(s) => s,
            Err(_) => return unavailable("Mimi couldn't find its assistant program."),
        };
        match mimi_service::status(&spec) {
            Ok(status) => BackgroundStatus {
                available: true,
                unavailable_reason: None,
                enabled: status.installed,
                no_restart: status.manager == mimi_service::Manager::Autostart,
            },
            Err(e) => unavailable(&e.to_string()),
        }
    }

    /// Turns background mode on (install the login service and hand the daemon to it)
    /// or off (remove it, and let the app run the daemon again).
    pub async fn set_background(&self, enabled: bool) -> Result<BackgroundStatus, String> {
        if mimi_service::daemon_is_external() {
            return Err("While developing, the assistant is started by pnpm dev.".to_owned());
        }
        let spec = spec()?;
        if enabled {
            // Only one daemon per data directory: hand over from whoever runs it now.
            match self.owner() {
                Owner::App => self.stop_child(),
                Owner::External => {
                    if let Ok(paths) = Paths::resolve() {
                        mimi_service::stop_daemon(&paths);
                    }
                }
                Owner::Service | Owner::Unknown => {}
            }
            wait_until(false, Duration::from_secs(10)).await;
            let installed = tokio::task::spawn_blocking({
                let spec = spec.clone();
                move || mimi_service::install(&spec)
            })
            .await
            .map_err(|e| e.to_string())?;
            if let Err(e) = installed {
                // Don't leave the user without an assistant.
                self.spawn_child(&spec);
                *lock(&self.owner) = Owner::App;
                return Err(e.to_string());
            }
            *lock(&self.owner) = Owner::Service;
            wait_until(true, Duration::from_secs(15)).await;
        } else {
            tokio::task::spawn_blocking({
                let spec = spec.clone();
                move || mimi_service::uninstall(&spec)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            wait_until(false, Duration::from_secs(10)).await;
            self.spawn_child(&spec);
            *lock(&self.owner) = Owner::App;
            wait_until(true, Duration::from_secs(15)).await;
        }
        Ok(self.background_status())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole lifecycle against real processes and the real service manager, without
    /// a window. Needs a throwaway environment, so it only runs on request:
    ///
    /// ```sh
    /// MIMI_HOME=/tmp/x MIMI_PORT=7492 MIMI_KEY_STORE=file \
    /// MIMI_DAEMON_BIN=$PWD/target/debug/mimid \
    ///   cargo test -p mimi-desktop lifecycle -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn lifecycle() {
        assert!(
            std::env::var("MIMI_HOME").is_ok(),
            "set a throwaway MIMI_HOME"
        );
        let daemon = DaemonProcess::default();
        assert!(!reachable().await, "start with nothing running");

        // No daemon, no service: the app runs one itself.
        daemon.ensure_running().await;
        assert_eq!(daemon.owner(), Owner::App);
        assert!(
            wait_until(true, Duration::from_secs(15)).await,
            "child came up"
        );
        assert!(!daemon.background_status().enabled);

        // It's restarted if it dies.
        let pid = lock(&daemon.child).as_ref().unwrap().id();
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
        let supervisor = daemon.supervise();
        let check = async {
            wait_until(false, Duration::from_secs(5)).await;
            wait_until(true, Duration::from_secs(15)).await
        };
        let restarted = tokio::select! { r = check => r, _ = supervisor => false };
        assert!(restarted, "supervisor restarted the child");
        assert_ne!(lock(&daemon.child).as_ref().unwrap().id(), pid);

        // Background on: the service takes over.
        let status = daemon
            .set_background(true)
            .await
            .expect("enable background");
        assert!(status.enabled);
        assert_eq!(daemon.owner(), Owner::Service);
        assert!(lock(&daemon.child).is_none());
        assert!(reachable().await);

        // A fresh app launch uses the running service.
        let second = DaemonProcess::default();
        second.ensure_running().await;
        assert_eq!(second.owner(), Owner::Service);
        second.quit();
        assert!(
            reachable().await,
            "quitting the app leaves the service running"
        );

        // Background off: back to the app.
        let status = daemon
            .set_background(false)
            .await
            .expect("disable background");
        assert!(!status.enabled);
        assert_eq!(daemon.owner(), Owner::App);
        assert!(reachable().await);

        // Quitting stops the app's own daemon, cleanly.
        daemon.quit();
        assert!(
            wait_until(false, Duration::from_secs(10)).await,
            "child stopped on quit"
        );
        let paths = Paths::resolve().unwrap();
        assert!(
            !paths.discovery_file().exists(),
            "clean shutdown removed the discovery file"
        );
    }
}
