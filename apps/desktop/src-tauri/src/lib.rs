use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;
use hearth_client::{Client, Method};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// Errors cross into the webview as a tagged value the UI can branch on.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CommandError {
    NotRunning,
    Api { code: String, message: String },
    Other { message: String },
}

impl From<hearth_client::Error> for CommandError {
    fn from(err: hearth_client::Error) -> Self {
        match err {
            hearth_client::Error::NotRunning => Self::NotRunning,
            hearth_client::Error::Api { code, message, .. } => Self::Api { code, message },
            other => Self::Other {
                message: other.to_string(),
            },
        }
    }
}

/// Forwards a request from the webview to the daemon's `/v1` API. The webview never
/// sees the daemon token; the Rust side attaches it.
#[tauri::command]
async fn api(
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, CommandError> {
    let method = match method.as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        other => {
            return Err(CommandError::Other {
                message: format!("unsupported method {other}"),
            });
        }
    };
    if !path.starts_with('/') || path.contains("..") {
        return Err(CommandError::Other {
            message: format!("invalid API path {path}"),
        });
    }
    Ok(Client::local()?.request_json(method, &path, body).await?)
}

/// Opens the web interface in the default browser, signed in via a single-use link.
#[tauri::command]
async fn open_in_browser(app: AppHandle) -> Result<(), CommandError> {
    let link = Client::local()?.web_login_link().await?;
    app.opener()
        .open_url(link.url, None::<&str>)
        .map_err(|e| CommandError::Other {
            message: e.to_string(),
        })
}

#[derive(Default)]
struct Connection(Arc<AtomicBool>);

/// Whether the event relay is currently connected to the daemon.
#[tauri::command]
fn daemon_connected(conn: tauri::State<'_, Connection>) -> bool {
    conn.0.load(Ordering::Relaxed)
}

/// Relays daemon events to the webview as `daemon-event`, and connection changes as
/// `daemon-connection`. Reconnects forever, re-reading the discovery file each time so
/// daemon restarts (which change the port and token) are picked up.
fn spawn_event_relay(app: AppHandle, connected: Arc<AtomicBool>) {
    let set = move |app: &AppHandle, value: bool| {
        if connected.swap(value, Ordering::Relaxed) != value {
            let _ = app.emit("daemon-connection", value);
        }
    };
    tauri::async_runtime::spawn(async move {
        loop {
            let stream = match Client::local() {
                Ok(client) => client.events().await.ok(),
                Err(_) => None,
            };
            if let Some(mut stream) = stream {
                set(&app, true);
                while let Some(Ok(event)) = stream.next().await {
                    let _ = app.emit("daemon-event", &event);
                }
            }
            set(&app, false);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Connection::default())
        .setup(|app| {
            let connected = app.state::<Connection>().0.clone();
            spawn_event_relay(app.handle().clone(), connected);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            api,
            daemon_connected,
            open_in_browser
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
