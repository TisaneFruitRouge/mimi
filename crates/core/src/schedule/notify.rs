//! Desktop notifications sent by the daemon itself, so they appear even when the app's
//! window is closed. Best effort: a machine without a notification service (a headless
//! server, CI) simply doesn't get them.

/// Set to `1` to never show desktop notifications, e.g. in tests.
pub const DISABLE_ENV: &str = "MIMI_NO_NOTIFICATIONS";

pub async fn show(summary: String, body: String) {
    // Tests run on developers' desktops: never pop real notifications there.
    if cfg!(test) || std::env::var(DISABLE_ENV).is_ok_and(|v| v == "1") {
        tracing::info!(%summary, "desktop notification skipped (disabled)");
        return;
    }
    let shown = tokio::task::spawn_blocking(move || {
        notify_rust::Notification::new()
            .appname("Mimi")
            .summary(&summary)
            .body(&body)
            .timeout(notify_rust::Timeout::Milliseconds(12_000))
            .show()
            .map(drop)
    })
    .await;
    match shown {
        Ok(Ok(())) => tracing::debug!("desktop notification shown"),
        Ok(Err(e)) => tracing::debug!("no desktop notification: {e}"),
        Err(e) => tracing::debug!("no desktop notification: {e}"),
    }
}
