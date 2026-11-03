//! Notifications on this computer when new mail lands in an Inbox
//! (`Settings.mail_notifications`).
//!
//! The sync queues each new Inbox message as it stores it (`queue`, in the same
//! transaction), except on a mailbox's first pass, for the user's own mail, mail already
//! read, and mail sent or received more than an hour ago (old mail that merely arrived
//! late). The notifier (`run`) wakes on mail and settings changes, decides what each
//! queued message deserves (`verdict`), and shows what's ready as one notification
//! (`compose`): "3 new emails" rather than three. A row leaves the queue before its
//! notification shows, and each Message-ID is considered once (`mail_notify_seen`), so
//! nothing is announced twice, across restarts too.
//!
//! "Important only" waits for the sorter (`triage`) to sort the conversation, for up to
//! [`SORT_WAIT_MS`]; if it's slow or failing, the email is announced anyway, unless it's
//! automatic mail. With nothing to sort (sorting off, no model, no Jev key) it means
//! every new email except newsletters and automatic mail. Suspicious mail never shows
//! its subject.

use std::sync::Arc;
use std::time::Duration;

use mimi_protocol::{Event, MailCategory, NewMailNotify, Settings};
use rusqlite::{Connection, params};
use tokio::sync::broadcast::error::RecvError;

use super::store::{self, NewMessage};
use crate::{AppState, now_ms};

/// Mail sent or received longer ago than this is old news: never announced.
pub const RECENT_MS: i64 = 60 * 60 * 1000;
/// How long "important only" waits for the sorter before announcing anyway.
pub const SORT_WAIT_MS: i64 = 3 * 60 * 1000;
/// How long a Message-ID is remembered as announced (or passed over).
const SEEN_FOR_MS: i64 = 7 * 24 * 3600 * 1000;
/// Longest single sleep: monotonic sleeps don't advance while the computer is asleep.
const MAX_SLEEP_MS: i64 = 60 * 1000;
/// Messages listed by name in one notification; the rest are counted.
const LISTED: usize = 3;

// --- Queueing, from the sync ----------------------------------------------------------

/// Queues a message the sync just stored, if it could deserve a notification. `watch`
/// is false on a mailbox's first pass (a new account, or a renumbered mailbox): that's
/// old mail being copied, not mail arriving.
pub fn queue(c: &Connection, m: &NewMessage, watch: bool, now: i64) -> rusqlite::Result<()> {
    let sent = m.parsed.date.unwrap_or(m.received);
    if !watch
        || m.folder != "inbox"
        || m.outgoing
        || m.seen
        || now - m.received.min(sent) > RECENT_MS
    {
        return Ok(());
    }
    // Once per Message-ID: a copy back with a new UID, or the same mail in a second
    // account, isn't news.
    if let Some(id) = &m.parsed.message_id {
        let fresh = c.execute(
            "INSERT OR IGNORE INTO mail_notify_seen (message_id, at) VALUES (?1, ?2)",
            params![id, now],
        )? > 0;
        if !fresh {
            return Ok(());
        }
    }
    c.execute(
        "INSERT OR IGNORE INTO mail_notify_queue (message_id, queued_at)
         SELECT id, ?4 FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2 AND uid = ?3",
        params![m.connection_id.to_string(), m.mailbox, m.uid, now],
    )?;
    Ok(())
}

// --- Deciding -------------------------------------------------------------------------

/// A queued message, as the notifier sees it.
#[derive(Debug, Clone)]
pub struct Pending {
    pub message: i64,
    pub thread: i64,
    pub queued_at: i64,
    pub sender: String,
    pub subject: String,
    pub seen: bool,
    pub automated: bool,
    pub suspicious: bool,
    /// The conversation's category, once sorted since this message arrived.
    pub sorted: Option<MailCategory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Never announced: leaves the queue.
    Drop,
    Announce,
    /// Waits for the sorter until then (ms).
    Wait(i64),
}

/// What one queued message deserves. `sorting`: whether something sorts new mail now.
pub fn verdict(p: &Pending, notify: NewMailNotify, sorting: bool, now: i64) -> Verdict {
    // Read meanwhile (here or on another device), or held so long it's old news.
    if p.seen || now - p.queued_at > RECENT_MS {
        return Verdict::Drop;
    }
    match notify {
        NewMailNotify::Off => Verdict::Drop,
        NewMailNotify::All => Verdict::Announce,
        // Newsletters and automatic mail only with "all".
        NewMailNotify::Important if p.automated => Verdict::Drop,
        // Nothing sorts, so nothing can tell: everything but automatic mail.
        NewMailNotify::Important if !sorting => Verdict::Announce,
        // Never sorted (filed as "everything else"), so never important.
        NewMailNotify::Important if p.suspicious => Verdict::Drop,
        NewMailNotify::Important => match p.sorted {
            Some(MailCategory::NeedsReply | MailCategory::Important) => Verdict::Announce,
            Some(MailCategory::Other) => Verdict::Drop,
            // The sorter is slow or failing: better a notification than a missed email.
            None if now >= p.queued_at + SORT_WAIT_MS => Verdict::Announce,
            None => Verdict::Wait(p.queued_at + SORT_WAIT_MS),
        },
    }
}

/// What to do with the queue now.
#[derive(Debug, Default)]
pub struct Plan {
    /// Leave the queue now: announced or dropped.
    pub done: Vec<i64>,
    /// Announced together, newest first.
    pub announce: Vec<Pending>,
    /// When to look again, if something waits.
    pub next: Option<i64>,
}

/// Decides over the whole queue (newest first). What's ready is held while others still
/// wait for the sorter, so they arrive as one notification, but never past
/// [`SORT_WAIT_MS`] after it was queued.
pub fn plan(pending: &[Pending], notify: NewMailNotify, sorting: bool, now: i64) -> Plan {
    let mut out = Plan::default();
    let mut ready = Vec::new();
    let mut waiting_until: Option<i64> = None;
    for p in pending {
        match verdict(p, notify, sorting, now) {
            Verdict::Drop => out.done.push(p.message),
            Verdict::Announce => ready.push(p.clone()),
            Verdict::Wait(until) => {
                waiting_until = Some(waiting_until.map_or(until, |w| w.min(until)));
            }
        }
    }
    let held_until = ready
        .iter()
        .map(|p| p.queued_at + SORT_WAIT_MS)
        .min()
        .unwrap_or(i64::MAX);
    if !ready.is_empty() && (waiting_until.is_none() || held_until <= now) {
        out.done.extend(ready.iter().map(|p| p.message));
        out.announce = ready;
        out.next = waiting_until;
    } else {
        out.next = match (waiting_until, ready.is_empty()) {
            (w, true) => w,
            (Some(w), false) => Some(w.min(held_until)),
            (None, false) => Some(held_until),
        };
    }
    out
}

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// The conversation a click opens; `None` when they're from several.
    pub thread: Option<i64>,
    /// The messages it announces.
    pub messages: Vec<i64>,
}

/// One notification for everything in `items` (newest first). Without `details`, it
/// doesn't say who wrote or what about; suspicious mail never shows its subject.
pub fn compose(items: &[Pending], details: bool) -> Option<Notice> {
    let first = items.first()?;
    let n = items.len();
    let subject = |p: &Pending| {
        if p.suspicious {
            "This email looks suspicious, so its subject is hidden.".to_owned()
        } else if p.subject.is_empty() {
            "(no subject)".to_owned()
        } else {
            p.subject.clone()
        }
    };
    let (title, body) = match (n, details) {
        (1, true) => (format!("New email from {}", first.sender), subject(first)),
        (1, false) => ("New email".to_owned(), String::new()),
        (_, false) => (format!("{n} new emails"), String::new()),
        (_, true) => {
            let mut lines: Vec<String> = items
                .iter()
                .take(LISTED)
                .map(|p| {
                    let s = if p.suspicious {
                        "(subject hidden: this email looks suspicious)".to_owned()
                    } else {
                        subject(p)
                    };
                    format!("{}: {s}", p.sender)
                })
                .collect();
            if n > LISTED {
                lines.push(format!("and {} more", n - LISTED));
            }
            (format!("{n} new emails"), lines.join("\n"))
        }
    };
    let thread = items
        .iter()
        .all(|p| p.thread == first.thread)
        .then_some(first.thread);
    Some(Notice {
        title,
        body,
        thread,
        messages: items.iter().map(|p| p.message).collect(),
    })
}

/// Text from an email made fit for one line of a notification.
fn one_line(text: &str, max: usize) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    let line = words.join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

fn pending(c: &Connection) -> rusqlite::Result<Vec<Pending>> {
    let mut stmt = c.prepare(
        "SELECT q.message_id, m.thread_id, q.queued_at, m.from_name, m.from_email, m.subject,
                m.seen, m.automated, coalesce(m.suspicious, 0), t.category,
                t.sorted_at IS NOT NULL AND t.sorted_at >= t.last_at
         FROM mail_notify_queue q
         JOIN mail_messages m ON m.id = q.message_id
         JOIN mail_threads t ON t.id = m.thread_id
         ORDER BY m.date DESC, m.id DESC",
    )?;
    stmt.query_map([], |r| {
        let name: Option<String> = r.get(3)?;
        let email: String = r.get(4)?;
        let sender = name
            .map(|n| one_line(&n, 60))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| one_line(&email, 60));
        let category: Option<String> = r.get(9)?;
        let sorted: bool = r.get(10)?;
        Ok(Pending {
            message: r.get(0)?,
            thread: r.get(1)?,
            queued_at: r.get(2)?,
            sender,
            subject: one_line(&r.get::<_, String>(5)?, 120),
            seen: r.get(6)?,
            automated: r.get(7)?,
            suspicious: r.get(8)?,
            sorted: category
                .as_deref()
                .and_then(store::category_from)
                .filter(|_| sorted),
        })
    })?
    .collect()
}

/// Whether something sorts new mail right now (as `triage::drain` decides).
async fn sorting(state: &AppState, settings: &Settings) -> bool {
    settings.mail_sorting
        && match settings.mail_sorter {
            mimi_protocol::MailSorter::Model => settings.default_model.is_some(),
            mimi_protocol::MailSorter::Jev => super::jev::key(&state.db).await.is_some(),
        }
}

/// What one look at the queue decided.
#[derive(Debug, Default)]
pub struct Tick {
    pub notice: Option<Notice>,
    /// When to look again, if something waits for the sorter.
    pub next: Option<i64>,
}

/// Looks at the queue once: takes out what's decided and returns the notification to
/// show, if any. Tests call it with their own clock.
pub async fn tick(state: &AppState, now: i64) -> Result<Tick, String> {
    let settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    let sorting = sorting(state, &settings).await;
    let choice = settings.mail_notifications;
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "DELETE FROM mail_notify_seen WHERE at < ?1",
                [now - SEEN_FOR_MS],
            )?;
            let plan = plan(&pending(&tx)?, choice.notify, sorting, now);
            for id in &plan.done {
                tx.execute("DELETE FROM mail_notify_queue WHERE message_id = ?1", [id])?;
            }
            tx.commit()?;
            Ok(Tick {
                notice: compose(&plan.announce, choice.show_details),
                next: plan.next,
            })
        })
        .await
        .map_err(|e| e.to_string())
}

// --- The loop -------------------------------------------------------------------------

/// The notifier. Runs for the daemon's lifetime.
pub async fn run(state: Arc<AppState>) {
    let mut events = state.events.subscribe();
    let mut next: Option<i64> = None;
    let mut first = true;
    loop {
        if !std::mem::take(&mut first) {
            let sleep = next.map(|at| (at - now_ms()).clamp(0, MAX_SLEEP_MS) as u64);
            let woken = tokio::select! {
                e = events.recv() => match e {
                    Ok(Event::MailChanged | Event::SettingsChanged { .. } | Event::Resync)
                    | Err(RecvError::Lagged(_)) => true,
                    Ok(_) => false,
                    Err(RecvError::Closed) => return,
                },
                _ = sleep_for(sleep) => true,
            };
            if !woken {
                continue;
            }
        }
        match tick(&state, now_ms()).await {
            Ok(t) => {
                next = t.next;
                if let Some(notice) = t.notice {
                    announce(&state, notice);
                }
            }
            Err(e) => {
                tracing::warn!("couldn't check for new mail to announce: {e}");
                next = Some(now_ms() + MAX_SLEEP_MS);
            }
        }
    }
}

async fn sleep_for(ms: Option<u64>) {
    match ms {
        Some(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
        None => std::future::pending().await,
    }
}

/// Shows it; a click brings the app forward on the conversation.
fn announce(state: &AppState, notice: Notice) {
    tracing::info!(count = notice.messages.len(), "announcing new mail");
    let events = state.events.clone();
    let thread_id = notice.thread;
    tokio::spawn(crate::schedule::notify::show_clickable(
        notice.title,
        notice.body,
        move || events.publish(Event::OpenMail { thread_id }),
    ));
}

#[cfg(test)]
#[path = "notify_tests.rs"]
mod tests;
