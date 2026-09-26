use hearth_client::Client;
use hearth_protocol::Status;

/// Errors cross into the webview as a tagged value the UI can branch on.
#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
enum CommandError {
    NotRunning,
    Other(String),
}

impl From<hearth_client::Error> for CommandError {
    fn from(err: hearth_client::Error) -> Self {
        match err {
            hearth_client::Error::NotRunning => Self::NotRunning,
            other => Self::Other(other.to_string()),
        }
    }
}

// The webview never sees the daemon token: it calls these commands, and the Rust side
// talks to the daemon.
#[tauri::command]
async fn daemon_status() -> Result<Status, CommandError> {
    Ok(Client::local()?.status().await?)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![daemon_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
