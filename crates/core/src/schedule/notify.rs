//! Desktop notifications sent by the daemon itself, so they appear even when the app's
//! window is closed. Best effort: a machine without a notification service (a headless
//! server, CI) simply doesn't get them.

/// Set to `1` to never show desktop notifications, e.g. in tests.
pub const DISABLE_ENV: &str = "MIMI_NO_NOTIFICATIONS";

/// Tests run on developers' desktops: never pop real notifications there.
fn disabled(summary: &str) -> bool {
    let off = cfg!(test) || std::env::var(DISABLE_ENV).is_ok_and(|v| v == "1");
    if off {
        tracing::info!(%summary, "desktop notification skipped (disabled)");
    }
    off
}

pub async fn show(summary: String, body: String) {
    if disabled(&summary) {
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

/// A notification whose text comes from outside (an email's sender and subject), with
/// `on_click` run when the user clicks it. Clicks are heard on Linux (the notification's
/// "default" action over D-Bus, for up to an hour); elsewhere it's a plain notification.
/// Returns once the notification is gone or clicked, so callers spawn it.
pub async fn show_clickable(summary: String, body: String, on_click: impl FnOnce() + Send) {
    if disabled(&summary) {
        return;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Err(e) = dbus::show(&summary, &body, on_click).await {
        tracing::debug!("no desktop notification: {e}");
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = on_click;
        show(summary, body).await;
    }
}

/// The freedesktop notification service, spoken to directly so a click can be heard
/// without notify-rust's action helper (which panics, and so would abort the daemon, if
/// the bus refuses a match rule).
#[cfg(all(unix, not(target_os = "macos")))]
mod dbus {
    use std::collections::HashMap;
    use std::time::Duration;

    use futures::StreamExt;
    use zbus::zvariant::Value;

    /// How long a click on a notification is listened for.
    const CLICKABLE_FOR: Duration = Duration::from_secs(60 * 60);

    #[zbus::proxy(
        interface = "org.freedesktop.Notifications",
        default_service = "org.freedesktop.Notifications",
        default_path = "/org/freedesktop/Notifications",
        gen_blocking = false
    )]
    trait Notifications {
        fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

        #[allow(clippy::too_many_arguments)]
        fn notify(
            &self,
            app_name: &str,
            replaces_id: u32,
            app_icon: &str,
            summary: &str,
            body: &str,
            actions: &[&str],
            hints: HashMap<&str, Value<'_>>,
            expire_timeout: i32,
        ) -> zbus::Result<u32>;

        #[zbus(signal)]
        fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

        #[zbus(signal)]
        fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
    }

    pub async fn show(summary: &str, body: &str, on_click: impl FnOnce()) -> zbus::Result<()> {
        let bus = zbus::Connection::session().await?;
        let proxy = NotificationsProxy::new(&bus).await?;
        // Servers that read markup in the body would otherwise take a sender's "<b>" or
        // "<a href>" as theirs.
        let markup = proxy
            .get_capabilities()
            .await
            .is_ok_and(|c| c.iter().any(|c| c == "body-markup"));
        let body = if markup {
            escape(body)
        } else {
            body.to_owned()
        };
        // Listen before showing it, so a quick click isn't missed.
        let mut clicks = proxy.receive_action_invoked().await?;
        let mut closes = proxy.receive_notification_closed().await?;
        let hints = HashMap::from([("category", Value::from("email.arrived"))]);
        let id = proxy
            .notify(
                "Mimi",
                0,
                "",
                summary,
                &body,
                &["default", "Open"],
                hints,
                12_000,
            )
            .await?;
        tracing::debug!("desktop notification shown");
        let wait = async {
            loop {
                tokio::select! {
                    Some(signal) = clicks.next() => {
                        if let Ok(args) = signal.args()
                            && args.id == id
                        {
                            return args.action_key == "default";
                        }
                    }
                    Some(signal) = closes.next() => {
                        if signal.args().is_ok_and(|a| a.id == id) {
                            return false;
                        }
                    }
                    else => return false,
                }
            }
        };
        if tokio::time::timeout(CLICKABLE_FOR, wait)
            .await
            .unwrap_or(false)
        {
            on_click();
        }
        Ok(())
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn markup_from_mail_is_shown_as_text() {
            assert_eq!(
                super::escape("Q&A <b>now</b>"),
                "Q&amp;A &lt;b&gt;now&lt;/b&gt;"
            );
        }
    }
}
