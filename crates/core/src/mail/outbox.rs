//! Mail waiting to go: the few seconds Undo is offered after the user presses Send, and
//! messages scheduled for later ("Send later", or the assistant's `mail_send` with a
//! time, approved by the user).
//!
//! Each waits in `mail_outbox`, in the encrypted database, never in the app: closing
//! the window doesn't lose it, and a restart during the wait still sends it. One loop
//! ([`run`]) sleeps until the next one is due, at most a minute at a time like the
//! scheduler (a monotonic sleep doesn't advance while the computer is asleep), so
//! anything that came due while the computer was off or asleep goes as soon as Mimi
//! runs again, once.
//!
//! Everything that can be checked is checked when the message is queued (the account,
//! the From address, the recipients, the sizes), and the files it carries by reference
//! (a forward's originals, the assistant's email attachments and chat photos) are
//! fetched then and kept with it, so what goes is what was checked and a file moved
//! meanwhile can't stop it. A message that can't go is kept, marked with the reason, and
//! the user is told; only a server that couldn't be reached is tried again, a few times.
//! A message cut off mid-send by a crash is never sent again on its own: it may have
//! gone.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use mimi_protocol::{
    Event, MailDraft, NewMailAttachment, OutgoingKind, OutgoingMail, OutgoingStatus,
};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::{Undelivered, parse};
use crate::{AppState, now_ms};

/// The outbox loop's wake-up signal.
#[derive(Default)]
pub struct Outbox {
    wake: Notify,
}

impl Outbox {
    /// Re-plans now, e.g. after a message was queued or rescheduled.
    pub fn poke(&self) {
        self.wake.notify_one();
    }
}

const MINUTE: i64 = 60_000;
/// How far in the past a chosen time may be (a preset worked out a moment ago).
const GRACE_MS: i64 = MINUTE;
/// How far ahead a message may be scheduled.
const MAX_AHEAD_MS: i64 = 366 * 24 * 60 * MINUTE;
/// Waits before trying a server that couldn't be reached again, in minutes. After the
/// last, the message is marked as not sent and the user is told.
const RETRY_MINUTES: [i64; 4] = [1, 5, 15, 60];

const CUT_OFF: &str = "Mimi stopped while sending this, so it may or may not have gone. \
                       Check your Sent folder before sending it again.";
const ACCOUNT_GONE: &str = "The email account it was going from isn't connected any more.";

/// The files a queued message carries by reference, fetched when it was queued.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Files {
    /// A forward's original attachments.
    #[serde(default)]
    forwarded: Vec<NewMailAttachment>,
    /// Each attachment with a `source`, in the draft's order.
    #[serde(default)]
    referenced: Vec<NewMailAttachment>,
}

#[derive(Debug, Clone)]
struct Item {
    id: Uuid,
    kind: OutgoingKind,
    status: OutgoingStatus,
    draft: MailDraft,
    files: Files,
    connection_id: Uuid,
    from: String,
    send_at: i64,
    created_at: i64,
    error: Option<String>,
    attempts: i64,
    by_assistant: bool,
}

const COLUMNS: &str = "id, kind, status, draft, files, connection_id, from_address, send_at, \
                       created_at, error, attempts, by_assistant";

fn kind_str(k: OutgoingKind) -> &'static str {
    match k {
        OutgoingKind::Undo => "undo",
        OutgoingKind::Scheduled => "scheduled",
    }
}

fn read(r: &rusqlite::Row) -> rusqlite::Result<Item> {
    let text = |i: usize| r.get::<_, String>(i);
    let json_err = |i: usize, e: serde_json::Error| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e))
    };
    Ok(Item {
        id: text(0)?.parse().unwrap_or_default(),
        kind: match text(1)?.as_str() {
            "undo" => OutgoingKind::Undo,
            _ => OutgoingKind::Scheduled,
        },
        status: match text(2)?.as_str() {
            "waiting" => OutgoingStatus::Waiting,
            "sending" => OutgoingStatus::Sending,
            _ => OutgoingStatus::Failed,
        },
        draft: serde_json::from_str(&text(3)?).map_err(|e| json_err(3, e))?,
        files: serde_json::from_str(&text(4)?).map_err(|e| json_err(4, e))?,
        connection_id: text(5)?.parse().unwrap_or_default(),
        from: text(6)?,
        send_at: r.get(7)?,
        created_at: r.get(8)?,
        error: r.get(9)?,
        attempts: r.get(10)?,
        by_assistant: r.get(11)?,
    })
}

impl Item {
    /// As clients see it: the draft without its files' content.
    fn public(&self, status: OutgoingStatus, sent_at: Option<i64>) -> OutgoingMail {
        OutgoingMail {
            id: self.id,
            kind: self.kind,
            status,
            draft: without_content(&self.draft),
            connection_id: self.connection_id,
            from: self.from.clone(),
            send_at: self.send_at,
            created_at: self.created_at,
            sent_at,
            error: self.error.clone(),
            by_assistant: self.by_assistant,
        }
    }

    /// The message as it goes: from the account and address fixed when it was queued,
    /// with the files fetched then in place of their references.
    fn sendable(&self) -> MailDraft {
        MailDraft {
            connection_id: Some(self.connection_id),
            from: Some(self.from.clone()),
            ..with_files(&self.draft, &self.files)
        }
    }
}

fn without_content(draft: &MailDraft) -> MailDraft {
    let mut d = draft.clone();
    for a in &mut d.attachments {
        a.data.clear();
    }
    d
}

/// A draft with its referenced files (and a forward's) replaced by their content.
fn with_files(draft: &MailDraft, files: &Files) -> MailDraft {
    let mut referenced = files.referenced.iter();
    let mut attachments = files.forwarded.clone();
    for a in &draft.attachments {
        match &a.source {
            None => attachments.push(a.clone()),
            Some(_) => attachments.extend(referenced.next().cloned()),
        }
    }
    MailDraft {
        forward_of: None,
        attachments,
        ..draft.clone()
    }
}

fn as_new(a: parse::Attachment) -> NewMailAttachment {
    NewMailAttachment {
        name: a.name,
        mime: Some(a.content_type),
        data: base64::engine::general_purpose::STANDARD.encode(&a.data),
        content_id: None,
        source: None,
    }
}

/// Fetches the files a draft carries by reference: a forward's originals, and the
/// assistant's (an email's attachment, a chat photo), under the same rules as sending.
async fn fetch_files(state: &AppState, draft: &MailDraft) -> Result<Files, String> {
    let forwarded = match draft.forward_of {
        Some(message) => super::forwarded(state, message).await?,
        None => Vec::new(),
    };
    let refs: Vec<NewMailAttachment> = draft
        .attachments
        .iter()
        .filter(|a| a.source.is_some())
        .cloned()
        .collect();
    let referenced = super::attached(state, &refs).await?;
    Ok(Files {
        forwarded: forwarded.into_iter().map(as_new).collect(),
        referenced: referenced.into_iter().map(as_new).collect(),
    })
}

/// Refuses a time in the past, or more than a year ahead.
pub fn check_time(send_at: i64, now: i64) -> Result<i64, String> {
    if send_at < now - GRACE_MS {
        return Err("That time has already passed. Pick a time in the future.".to_owned());
    }
    if send_at > now + MAX_AHEAD_MS {
        return Err("Pick a time within the next year.".to_owned());
    }
    Ok(send_at.max(now))
}

/// A time the assistant gave: a local date and time ("2026-10-03T09:00", in the
/// computer's time zone) or an instant with its offset.
pub fn parse_when(raw: &str, tz: &TimeZone) -> Result<Timestamp, String> {
    if let Ok(t) = raw.trim().parse::<Timestamp>() {
        return Ok(t);
    }
    let local = crate::schedule::rules::parse_local(raw)?;
    tz.to_ambiguous_zoned(local)
        .compatible()
        .map(|z| z.timestamp())
        .map_err(|_| format!("“{raw}” isn't a time that exists here."))
}

fn publish(state: &AppState, item: OutgoingMail) {
    state.events.publish(Event::MailOutbox { item });
}

/// Queues a message: the user's Send (`send_at` `None`: it waits the seconds Undo is
/// offered, or goes at once when that's off) or a time chosen for it. Everything that
/// can be checked is checked first, so a mistake shows now, not when it's due.
///
/// The assistant's approved sends without a time go at once: the approval card was the
/// chance to stop them.
pub async fn queue(
    state: &Arc<AppState>,
    draft: MailDraft,
    send_at: Option<i64>,
    by_assistant: bool,
) -> Result<OutgoingMail, String> {
    let now = now_ms();
    let (kind, at) = match send_at {
        Some(t) => (OutgoingKind::Scheduled, check_time(t, now)?),
        None => {
            let wait = crate::settings::load(&state.db)
                .await
                .map_err(|e| e.to_string())?
                .undo_send_secs;
            if wait == 0 || by_assistant {
                return send_at_once(state, draft, by_assistant).await;
            }
            (OutgoingKind::Undo, now + i64::from(wait.min(60)) * 1000)
        }
    };
    let files = fetch_files(state, &draft).await?;
    let checked = super::prepare(state, &with_files(&draft, &files)).await?;
    let item = Item {
        id: Uuid::now_v7(),
        kind,
        status: OutgoingStatus::Waiting,
        draft,
        files,
        connection_id: checked.account.id,
        from: checked.from,
        send_at: at,
        created_at: now,
        error: None,
        attempts: 0,
        by_assistant,
    };
    let row = item.clone();
    let draft = serde_json::to_string(&row.draft).map_err(|e| e.to_string())?;
    let files = serde_json::to_string(&row.files).map_err(|e| e.to_string())?;
    state
        .db
        .call(move |c| {
            c.execute(
                &format!(
                    "INSERT INTO mail_outbox ({COLUMNS}) VALUES \
                     (?1, ?2, 'waiting', ?3, ?4, ?5, ?6, ?7, ?8, NULL, 0, ?9)"
                ),
                rusqlite::params![
                    row.id.to_string(),
                    kind_str(row.kind),
                    draft,
                    files,
                    row.connection_id.to_string(),
                    row.from,
                    row.send_at,
                    row.created_at,
                    row.by_assistant,
                ],
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(id = %item.id, kind = kind_str(kind), "an email is waiting to be sent");
    state.mail.outbox.poke();
    let public = item.public(OutgoingStatus::Waiting, None);
    publish(state, public.clone());
    Ok(public)
}

/// Sends without waiting (Undo send is off): as before the outbox existed.
async fn send_at_once(
    state: &Arc<AppState>,
    draft: MailDraft,
    by_assistant: bool,
) -> Result<OutgoingMail, String> {
    let ready = super::prepare(state, &draft).await?;
    let (connection_id, from) = (ready.account.id, ready.from.clone());
    super::deliver(state, ready).await.map_err(|e| e.message)?;
    let now = now_ms();
    Ok(OutgoingMail {
        id: Uuid::now_v7(),
        kind: OutgoingKind::Undo,
        status: OutgoingStatus::Sent,
        draft: without_content(&draft),
        connection_id,
        from,
        send_at: now,
        created_at: now,
        sent_at: Some(now),
        error: None,
        by_assistant,
    })
}

/// Everything waiting to go, or kept because it couldn't: those first, then by time.
pub async fn list(state: &AppState) -> Result<Vec<OutgoingMail>, String> {
    let items = state
        .db
        .call(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM mail_outbox
                 ORDER BY status = 'failed' DESC, send_at, created_at"
            ))?;
            let rows = stmt.query_map([], read)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(items.iter().map(|i| i.public(i.status, None)).collect())
}

/// Why an item can't be changed: it's going right now, or it's gone.
fn not_waiting(status: Option<OutgoingStatus>) -> String {
    match status {
        Some(OutgoingStatus::Sending) => {
            "It's being sent right now, so it can't be changed any more.".to_owned()
        }
        _ => "This email isn't waiting to be sent any more: it may already have gone.".to_owned(),
    }
}

/// Takes a message back before it goes (Undo, or Cancel in the Scheduled view) and
/// returns the draft as the user wrote it, files and all, to edit or drop.
pub async fn cancel(state: &AppState, id: Uuid) -> Result<MailDraft, String> {
    let taken = state
        .db
        .call(move |c| {
            let item = c
                .query_row(
                    &format!("SELECT {COLUMNS} FROM mail_outbox WHERE id = ?1"),
                    [id.to_string()],
                    read,
                )
                .optional()?;
            match item {
                Some(item) if item.status != OutgoingStatus::Sending => {
                    c.execute("DELETE FROM mail_outbox WHERE id = ?1", [id.to_string()])?;
                    Ok(Ok(item))
                }
                other => Ok(Err(other.map(|i| i.status))),
            }
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(not_waiting)?;
    tracing::info!(%id, "an email was taken back before it went");
    publish(state, taken.public(OutgoingStatus::Cancelled, None));
    Ok(taken.draft)
}

/// Sends a waiting (or failed) message at another time.
pub async fn reschedule(state: &AppState, id: Uuid, send_at: i64) -> Result<OutgoingMail, String> {
    let at = check_time(send_at, now_ms())?;
    let item = state
        .db
        .call(move |c| {
            c.query_row(
                &format!(
                    "UPDATE mail_outbox
                     SET send_at = ?2, kind = 'scheduled', status = 'waiting', error = NULL,
                         attempts = 0
                     WHERE id = ?1 AND status IN ('waiting', 'failed')
                     RETURNING {COLUMNS}"
                ),
                rusqlite::params![id.to_string(), at],
                read,
            )
            .optional()
        })
        .await
        .map_err(|e| e.to_string())?;
    let Some(item) = item else {
        return Err(not_waiting(status_of(state, id).await));
    };
    state.mail.outbox.poke();
    let public = item.public(item.status, None);
    publish(state, public.clone());
    Ok(public)
}

async fn status_of(state: &AppState, id: Uuid) -> Option<OutgoingStatus> {
    state
        .db
        .call(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM mail_outbox WHERE id = ?1"),
                [id.to_string()],
                read,
            )
            .optional()
        })
        .await
        .ok()
        .flatten()
        .map(|i| i.status)
}

/// Sends a waiting (or failed) message now, and says whether it went.
pub async fn send_now(state: &Arc<AppState>, id: Uuid) -> Result<(), String> {
    let claimed = state
        .db
        .call(move |c| {
            c.query_row(
                &format!(
                    "UPDATE mail_outbox SET status = 'sending'
                     WHERE id = ?1 AND status IN ('waiting', 'failed')
                     RETURNING {COLUMNS}"
                ),
                [id.to_string()],
                read,
            )
            .optional()
        })
        .await
        .map_err(|e| e.to_string())?;
    let Some(item) = claimed else {
        return Err(not_waiting(status_of(state, id).await));
    };
    attempt(state, item, false).await
}

/// Hands one claimed message (status `sending`) to the server, then records how it went.
/// `auto`: the loop sent it, so a server that couldn't be reached is tried again later;
/// the user's own "Send now" just says what happened.
async fn attempt(state: &Arc<AppState>, item: Item, auto: bool) -> Result<(), String> {
    publish(state, item.public(OutgoingStatus::Sending, None));
    let result = async {
        if super::account(state, item.connection_id).await.is_none() {
            return Err(Undelivered {
                message: ACCOUNT_GONE.to_owned(),
                retry: false,
            });
        }
        let ready = super::prepare(state, &item.sendable())
            .await
            .map_err(|message| Undelivered {
                message,
                retry: false,
            })?;
        super::deliver(state, ready).await
    }
    .await;
    let id = item.id.to_string();
    match result {
        Ok(()) => {
            let _ = state
                .db
                .call(move |c| c.execute("DELETE FROM mail_outbox WHERE id = ?1", [id]))
                .await
                .inspect_err(|e| tracing::error!("couldn't clear a sent email: {e}"));
            let late = now_ms() - item.send_at > crate::schedule::rules::LATE_AFTER_MS;
            tracing::info!(id = %item.id, late, "a waiting email was sent");
            publish(state, item.public(OutgoingStatus::Sent, Some(now_ms())));
            super::changed(state);
            Ok(())
        }
        Err(e) if auto && e.retry && (item.attempts as usize) < RETRY_MINUTES.len() => {
            let next = now_ms() + RETRY_MINUTES[item.attempts as usize] * MINUTE;
            let message = e.message.clone();
            let updated = state
                .db
                .call(move |c| {
                    c.query_row(
                        &format!(
                            "UPDATE mail_outbox
                             SET status = 'waiting', send_at = ?2, error = ?3,
                                 attempts = attempts + 1
                             WHERE id = ?1 RETURNING {COLUMNS}"
                        ),
                        rusqlite::params![id, next, message],
                        read,
                    )
                    .optional()
                })
                .await;
            tracing::warn!(id = %item.id, "couldn't reach the mail server; trying again later");
            if let Ok(Some(updated)) = updated {
                publish(state, updated.public(updated.status, None));
            }
            state.mail.outbox.poke();
            Err(e.message)
        }
        Err(e) => {
            let message = e.message.clone();
            let updated = state
                .db
                .call(move |c| {
                    c.query_row(
                        &format!(
                            "UPDATE mail_outbox SET status = 'failed', error = ?2
                             WHERE id = ?1 RETURNING {COLUMNS}"
                        ),
                        rusqlite::params![id, message],
                        read,
                    )
                    .optional()
                })
                .await;
            tracing::warn!(id = %item.id, "a waiting email couldn't be sent");
            if let Ok(Some(updated)) = updated {
                publish(state, updated.public(updated.status, None));
                if auto {
                    tell(state, &updated).await;
                }
            }
            Err(e.message)
        }
    }
}

/// Tells the user on the desktop that an email didn't go (the app shows it too, from
/// the event), when they have desktop notifications on.
async fn tell(state: &AppState, item: &Item) {
    let on = crate::settings::load(&state.db)
        .await
        .map(|s| s.desktop_notifications)
        .unwrap_or(false);
    if !on {
        return;
    }
    let subject = match item.draft.subject.trim() {
        "" => "(no subject)",
        s => s,
    };
    let body = format!(
        "“{}” to {}: {}",
        super::model::clip(subject, 60),
        item.draft.to.join(", "),
        item.error.as_deref().unwrap_or_default()
    );
    tokio::spawn(crate::schedule::notify::show(
        "An email wasn't sent".to_owned(),
        body,
    ));
}

/// Messages left mid-send by a daemon that stopped: they may have gone, so they're
/// marked as not sent rather than sent twice.
async fn recover(state: &AppState) {
    let cut_off = state
        .db
        .call(|c| {
            c.execute(
                "UPDATE mail_outbox SET status = 'failed', error = ?1 WHERE status = 'sending'",
                [CUT_OFF],
            )
        })
        .await;
    if let Ok(n) = cut_off
        && n > 0
    {
        tracing::warn!(n, "emails were cut off mid-send by the last shutdown");
    }
}

/// Sleeps until something is due, then sends it; forever.
pub async fn run(state: Arc<AppState>) {
    recover(&state).await;
    loop {
        let now = now_ms();
        // Sending runs in its own tasks: a slow server never holds up the loop.
        drop(tick(&state, now).await);
        let next: Option<i64> = state
            .db
            .call(|c| {
                c.query_row(
                    "SELECT MIN(send_at) FROM mail_outbox WHERE status = 'waiting'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .ok()
            .flatten();
        // Capped at a minute: a monotonic sleep doesn't advance while the computer
        // sleeps, and the wall clock may change under us.
        let wait = next.map_or(MINUTE, |at| (at - now).clamp(0, MINUTE));
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(wait.max(50) as u64)) => {}
            _ = state.mail.outbox.wake.notified() => {}
        }
    }
}

/// Starts sending everything due at `now` (the clock is an argument so tests can move
/// it). Returns the sends, which tests wait for.
pub(crate) async fn tick(state: &Arc<AppState>, now: i64) -> Vec<JoinHandle<()>> {
    let due = state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(&format!(
                "UPDATE mail_outbox SET status = 'sending'
                 WHERE status = 'waiting' AND send_at <= ?1
                 RETURNING {COLUMNS}"
            ))?;
            let rows = stmt.query_map([now], read)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .await;
    let due = match due {
        Ok(due) => due,
        Err(e) => {
            tracing::error!("couldn't read the outbox: {e}");
            return Vec::new();
        }
    };
    due.into_iter()
        .map(|item| {
            let state = state.clone();
            tokio::spawn(async move {
                let _ = attempt(&state, item, true).await;
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
