//! Flagging (starring) conversations from Mimi.
//!
//! A conversation is flagged when any of its messages carries `\Flagged` on the server
//! (that's how sync reads it). Flagging sets `\Flagged` on the conversation's latest
//! message, every stored copy of it (a reply can sit in Sent and the Inbox); taking the
//! flag off clears it from every message, so no older flag keeps the star on.
//!
//! The local copy changes first, so the panel shows it at once, then the server is told
//! while the request waits. If the server refuses or can't be reached, the local change
//! is undone and the request fails: the star never claims something the mail account
//! doesn't have. The next sync pass reconciles anything changed elsewhere meanwhile.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use futures::TryStreamExt;
use rusqlite::{Connection, params};
use uuid::Uuid;

use super::{Account, MailError, changed, sync};
use crate::AppState;

/// SQL condition on a thread `t`: one of its messages is flagged.
pub const FLAGGED_FILTER: &str =
    "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND x.flagged)";

/// Longest the server may take to apply a flag before the change is undone here.
pub(super) const SERVER_TIMEOUT: Duration = Duration::from_secs(30);

/// One change at a time, so a quick flag-unflag can't have the first change's undo or
/// rewrite land over the second.
pub(super) static ONE_AT_A_TIME: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// One stored copy of a message, where it is on the server, and its flag before.
#[derive(Debug, Clone)]
pub(super) struct Copy {
    id: i64,
    pub(super) conn: Uuid,
    mailbox: String,
    uid: u32,
    message_id: Option<String>,
    pub(super) flagged: bool,
}

/// A conversation's stored copies, oldest first.
pub(super) fn copies(c: &Connection, thread: i64) -> rusqlite::Result<Vec<Copy>> {
    let mut stmt = c.prepare(
        "SELECT id, connection_id, mailbox, uid, message_id, flagged FROM mail_messages
         WHERE thread_id = ?1 ORDER BY date, id",
    )?;
    stmt.query_map([thread], |r| {
        Ok(Copy {
            id: r.get(0)?,
            conn: r.get::<_, String>(1)?.parse().unwrap_or_default(),
            mailbox: r.get(2)?,
            uid: r.get(3)?,
            message_id: r.get(4)?,
            flagged: r.get(5)?,
        })
    })?
    .collect()
}

/// What a change touches: flagging, every copy of the latest message; unflagging,
/// every message.
pub(super) fn targets(all: Vec<Copy>, flagged: bool) -> Vec<Copy> {
    if !flagged {
        return all;
    }
    let Some(latest) = all.last().cloned() else {
        return Vec::new();
    };
    match &latest.message_id {
        Some(mid) => all
            .into_iter()
            .filter(|m| m.message_id.as_ref() == Some(mid))
            .collect(),
        None => vec![latest],
    }
}

pub(super) fn set_local(c: &Connection, copies: &[Copy], flagged: bool) -> rusqlite::Result<usize> {
    let mut n = 0;
    for m in copies {
        n += c.execute(
            "UPDATE mail_messages SET flagged = ?2 WHERE id = ?1 AND flagged != ?2",
            params![m.id, flagged],
        )?;
    }
    Ok(n)
}

/// Puts back what a failed change altered, unless sync has changed it since.
pub(super) fn undo_local(c: &Connection, copies: &[Copy], flagged: bool) -> rusqlite::Result<()> {
    for m in copies.iter().filter(|m| m.flagged != flagged) {
        c.execute(
            "UPDATE mail_messages SET flagged = ?2 WHERE id = ?1 AND flagged = ?3",
            params![m.id, m.flagged, flagged],
        )?;
    }
    Ok(())
}

/// Flags a conversation or takes its flag off, here at once and then on the server.
/// Undone here if the server doesn't take it.
pub async fn set_flagged(state: &Arc<AppState>, thread: i64, flagged: bool) -> Result<(), String> {
    let _turn = ONE_AT_A_TIME.lock().await;
    let found = state
        .db
        .call(move |c| {
            let all = copies(c, thread)?;
            if all.is_empty() {
                return Ok(None);
            }
            if all.iter().any(|m| m.flagged) == flagged {
                return Ok(Some(Vec::new()));
            }
            let targets = targets(all, flagged);
            set_local(c, &targets, flagged)?;
            Ok(Some(targets))
        })
        .await
        .map_err(|e| e.to_string())?;
    let targets = found.ok_or("That conversation isn't here any more.")?;
    if targets.is_empty() {
        return Ok(());
    }
    changed(state);

    let account = super::account(state, targets[0].conn).await;
    let result = match &account {
        Some(account) => {
            tokio::time::timeout(SERVER_TIMEOUT, on_server(account, &targets, flagged))
                .await
                .unwrap_or_else(|_| {
                    Err(MailError::Unreachable(
                        account.config.servers.imap_host.clone(),
                    ))
                })
        }
        None => Err(MailError::Refused(
            "That account isn't connected any more.".to_owned(),
        )),
    };
    let restore = targets.clone();
    match result {
        Ok(()) => {
            // A sync pass that read the server just before the change may have put the
            // old flag back here meanwhile.
            let again = state
                .db
                .call(move |c| set_local(c, &restore, flagged))
                .await
                .map_err(|e| e.to_string())?;
            if again > 0 {
                changed(state);
            }
            Ok(())
        }
        Err(e) => {
            tracing::warn!(thread, "couldn't change a flag on the server: {e}");
            state
                .db
                .call(move |c| undo_local(c, &restore, flagged))
                .await
                .map_err(|e| e.to_string())?;
            changed(state);
            Err(if flagged {
                format!("Couldn't flag it: {e}")
            } else {
                format!("Couldn't remove the flag: {e}")
            })
        }
    }
}

/// Sets or clears `\Flagged` on the given copies, one session for the account.
pub(super) async fn on_server(
    account: &Account,
    copies: &[Copy],
    flagged: bool,
) -> Result<(), MailError> {
    let proto = |e: async_imap::error::Error| MailError::Protocol(e.to_string());
    let mut by_mailbox: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for m in copies.iter().filter(|m| m.conn == account.id) {
        by_mailbox
            .entry(m.mailbox.as_str())
            .or_default()
            .push(m.uid.to_string());
    }
    let op = if flagged {
        "+FLAGS.SILENT (\\Flagged)"
    } else {
        "-FLAGS.SILENT (\\Flagged)"
    };
    let mut s = sync::session(&account.config).await?;
    for (mailbox, uids) in by_mailbox {
        s.select(mailbox).await.map_err(proto)?;
        s.uid_store(uids.join(","), op)
            .await
            .map_err(proto)?
            .try_collect::<Vec<_>>()
            .await
            .map_err(proto)?;
    }
    let _ = s.logout().await;
    Ok(())
}
