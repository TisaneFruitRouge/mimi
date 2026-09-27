//! The menu-bar / system-tray icon: open Mimi, see whether it's running, quit.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

use crate::Connection;
use crate::daemon_process::{DaemonProcess, Owner};

struct Tray {
    status: MenuItem<Wry>,
}

/// Linux trays need libayatana-appindicator (or the older libappindicator) at runtime,
/// and creating one without it aborts the app. Check before trying.
fn supported() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    std::process::Command::new("ldconfig")
        .arg("-p")
        .output()
        .map(|o| {
            let libs = String::from_utf8_lossy(&o.stdout);
            libs.contains("libayatana-appindicator3.so") || libs.contains("libappindicator3.so")
        })
        .unwrap_or(false)
}

pub fn exists(app: &AppHandle) -> bool {
    app.try_state::<Tray>().is_some()
}

/// Creates the tray icon, if this desktop can show one. Without it, closing the window
/// quits the app (the background service, if on, keeps the assistant running).
pub fn create(app: &AppHandle) {
    if !supported() {
        eprintln!("mimi: no system tray available; closing the window quits the app");
        return;
    }
    let built = (|| -> tauri::Result<()> {
        let open = MenuItem::with_id(app, "open", "Open Mimi", true, None::<&str>)?;
        let status = MenuItem::with_id(app, "status", "Starting…", false, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "Quit Mimi", true, None::<&str>)?;
        let menu = Menu::with_items(
            app,
            &[
                &open,
                &PredefinedMenuItem::separator(app)?,
                &status,
                &PredefinedMenuItem::separator(app)?,
                &quit,
            ],
        )?;
        let mut tray = TrayIconBuilder::with_id("mimi")
            .tooltip("Mimi")
            .menu(&menu)
            .show_menu_on_left_click(true)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "open" => crate::show_main_window(app),
                "quit" => app.exit(0),
                _ => {}
            });
        if let Some(icon) = app.default_window_icon() {
            tray = tray.icon(icon.clone());
        }
        tray.build(app)?;
        app.manage(Tray { status });
        Ok(())
    })();
    if let Err(e) = built {
        eprintln!("mimi: couldn't create the tray icon: {e}");
        return;
    }
    // Keep the status line truthful as the daemon comes and goes.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app);
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

/// Updates the tray's status line.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.try_state::<Tray>() else {
        return;
    };
    let connected = app.state::<Connection>().0.load(Ordering::Relaxed);
    let owner = app.state::<Arc<DaemonProcess>>().owner();
    let text = match (connected, owner) {
        (false, Owner::Unknown) => "Starting…",
        (false, _) => "Not running",
        (true, Owner::Service) => "Running in the background",
        (true, _) => "Running while Mimi is open",
    };
    let _ = tray.status.set_text(text);
}
