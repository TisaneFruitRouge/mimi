//! "Check for new versions" (Settings › General): off unless the user turns it on. Then,
//! about once a day, this computer asks GitHub (where Mimi is published) for the latest
//! release: one anonymous request, straight to GitHub, nothing about the user in it.
//! Nothing is downloaded or installed; the app only says a new version exists.

use std::sync::Mutex;
use std::time::Duration;

use mimi_protocol::{Event, Release, UpdateStatus};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use tokio::sync::Notify;

use crate::{AppState, VERSION, now_ms};

/// Where Mimi is published (the workspace's `repository`).
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const API: &str = "https://api.github.com";
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(20);
/// Row in the `settings` table with the last answer, so restarts don't ask again.
const ROW: &str = "updates";

#[derive(Default)]
pub struct Updates {
    /// Where release lookups go when not GitHub's API (tests).
    pub api: Mutex<Option<String>>,
    /// Wakes the loop, e.g. when the setting is turned on.
    pub wake: Notify,
}

/// Checks once a day while the setting is on.
pub async fn run(state: std::sync::Arc<AppState>) {
    loop {
        let on = crate::settings::load(&state.db)
            .await
            .is_ok_and(|s| s.update_check);
        if on {
            let last = status(&state).await.checked_at.unwrap_or(0);
            if now_ms() - last >= EVERY.as_millis() as i64 {
                check(&state).await;
            }
        }
        tokio::select! {
            _ = state.updates.wake.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(60 * 60)) => {}
        }
    }
}

/// What's known now, from the last check.
pub async fn status(state: &AppState) -> UpdateStatus {
    let stored: Option<String> = state
        .db
        .call(|c| {
            c.query_row("SELECT value FROM settings WHERE key = ?1", [ROW], |r| {
                r.get(0)
            })
            .optional()
        })
        .await
        .ok()
        .flatten();
    let mut status: UpdateStatus = stored
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    // After an update, what was "available" may be what's running now.
    status.current = VERSION.to_owned();
    status.available = status.available.filter(|r| newer(&r.version, VERSION));
    status
}

/// Asks GitHub now, stores and publishes the answer. The user's own "Check now" and the
/// daily check both come here.
pub async fn check(state: &AppState) -> UpdateStatus {
    let mut status = status(state).await;
    match latest(state).await {
        Ok(release) => {
            status.available = release.filter(|r| newer(&r.version, VERSION));
            status.checked_at = Some(now_ms());
            status.error = None;
        }
        Err(e) => {
            tracing::info!("checking for a new version failed: {e}");
            status.error = Some("Couldn't reach GitHub to check. It'll try again later.".into());
        }
    }
    let raw = serde_json::to_string(&status).expect("update status serializes");
    let _ = state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                (ROW, raw),
            )
        })
        .await;
    state.events.publish(Event::UpdateChanged {
        update: status.clone(),
    });
    status
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The latest published release, if there is one. Errors carry no URL.
async fn latest(state: &AppState) -> Result<Option<Release>, String> {
    let api = state
        .updates
        .api
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| API.to_owned());
    let repo = REPOSITORY.trim_start_matches("https://github.com/");
    let res = state
        .http
        .get(format!("{api}/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    // No release published yet.
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("GitHub answered {}", res.status()));
    }
    let release: GithubRelease = res.json().await.map_err(|e| e.without_url().to_string())?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let Some(version) = parse(&release.tag_name).map(|(a, b, c)| format!("{a}.{b}.{c}")) else {
        return Ok(None);
    };
    // The page is built from the version, not taken from the answer.
    let url = format!("{REPOSITORY}/releases/tag/v{version}");
    Ok(Some(Release { version, url }))
}

/// "v1.2.3" or "1.2.3"; pre-releases ("1.3.0-beta") and anything else are ignored.
fn parse(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v').unwrap_or(tag).split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

fn newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("v1.0.0", "0.9.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
        assert!(!newer("0.2.0-beta", "0.1.0"));
        assert!(!newer("nightly", "0.1.0"));
        assert!(!newer("1.2.3.4", "0.1.0"));
    }

    /// A fake GitHub answering `/repos/<repo>/releases/latest` with whatever `answer` holds.
    async fn fake_github(answer: std::sync::Arc<Mutex<(u16, String)>>) -> String {
        use axum::{Router, http::StatusCode, routing::get};
        let app = Router::new().route(
            "/repos/{owner}/{repo}/releases/latest",
            get(move || {
                let answer = answer.clone();
                async move {
                    let (code, body) = answer.lock().unwrap().clone();
                    (StatusCode::from_u16(code).unwrap(), body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn a_newer_release_is_found_and_remembered() {
        let state = AppState::for_tests("t");
        let answer = std::sync::Arc::new(Mutex::new((404, String::new())));
        *state.updates.api.lock().unwrap() = Some(fake_github(answer.clone()).await);
        let set = |code: u16, body: &str| *answer.lock().unwrap() = (code, body.to_owned());

        // Nothing published yet.
        let s = check(&state).await;
        assert_eq!((s.available, s.error.is_none()), (None, true));
        assert!(s.checked_at.is_some());

        // The same version: nothing to say.
        set(200, &format!(r#"{{"tag_name":"v{VERSION}"}}"#));
        assert_eq!(check(&state).await.available, None);

        // A pre-release doesn't count.
        set(200, r#"{"tag_name":"v99.0.0","prerelease":true}"#);
        assert_eq!(check(&state).await.available, None);

        // A newer one does, with its page built from the version, not from the answer.
        set(
            200,
            r#"{"tag_name":"v99.1.0","html_url":"https://evil.example/"}"#,
        );
        let found = check(&state).await.available.unwrap();
        assert_eq!(found.version, "99.1.0");
        assert_eq!(found.url, format!("{REPOSITORY}/releases/tag/v99.1.0"));

        // GitHub down: the last answer is kept, with a plain note.
        set(500, "");
        let s = check(&state).await;
        assert_eq!(s.available.map(|r| r.version).as_deref(), Some("99.1.0"));
        assert!(s.error.unwrap().contains("GitHub"));
        assert_eq!(status(&state).await.current, VERSION);
    }
}
