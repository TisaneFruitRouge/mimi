//! Drafts: an email being written is never lost, and drafts are shared with the user's
//! other mail apps through the account's Drafts folder on the server.
//!
//! - **Saved as it's written.** The app saves a compose or reply as the user writes
//!   (`PUT /v1/mail/drafts/{id}`, debounced), with its HTML, pictures and files, in
//!   `mail_drafts` in the encrypted database. Sending removes it once the message is
//!   queued (`outbox::queue`); taking it back (Undo, Cancel) saves it again.
//! - **Copied to the server.** Each saved draft is mirrored to the account's Drafts
//!   folder (`\Drafts`, else by name, made if missing) as a proper message with the
//!   `\Draft` flag and an `X-Mimi-Draft` header naming it. A change appends the new copy,
//!   then deletes the old one (and any other copy carrying its header), so there's one
//!   copy. Copies wait for a pause in the writing ([`PAUSE_MS`]), at most [`MAX_WAIT_MS`]
//!   while the user keeps writing, at once when the editor closes, and before the daemon
//!   stops. Mirroring touches the Drafts folder only.
//! - **Drafts from elsewhere.** Each sync pass reads the Drafts folder ([`read_server`]):
//!   a draft another app wrote shows in the Drafts view; its text is kept here, its files
//!   are fetched from the server when it's opened. Its HTML is cleaned like mail that's
//!   shown (hidden text removed), then down to the editor's own formatting. Changed here,
//!   it becomes Mimi's: its server copy is replaced by Mimi's.
//! - **Conflicts.** Mimi never merges and never drops text written here. If its copy is
//!   gone from the server when the draft wasn't changed here since it was copied, it was
//!   sent, deleted or taken over in another app, and it leaves Drafts here too (another
//!   app's version shows as a draft from elsewhere). If it was changed here meanwhile, it
//!   stays and is copied again; another app's version stays beside it.
//! - **Never sent by itself, never read by the model.** Nothing here sends: only the
//!   user's Send (the outbox) does. Drafts are kept apart from `mail_messages`, so they
//!   never reach the model, memory, sorting, notifications, # mentions or smart folders.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures::{StreamExt, TryStreamExt};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use mail_parser::{MessageParser, MimeHeaders, PartType};
use mimi_protocol::{Event, MailDraft, MailDraftInfo, NewMailAttachment};
use rusqlite::{OptionalExtension, params};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use super::net::ImapSession;
use super::sync::{self, proto};
use super::{Account, MailError, parse, smtp};
use crate::{AppState, now_ms};

/// Live drafts state: one change to the servers' Drafts folders at a time (the sync's
/// read and the copies), and the copier's wake-up.
#[derive(Default)]
pub struct Drafts {
    lock: Mutex<()>,
    wake: Notify,
}

impl Drafts {
    fn poke(&self) {
        self.wake.notify_one();
    }
}

/// The header that marks Mimi's copy of a draft on the server, with the draft's id.
pub const HEADER: &str = "X-Mimi-Draft";
/// A draft is copied to the server once it hasn't changed for this long…
pub const PAUSE_MS: i64 = 3_000;
/// …or this long after its first change not yet there, however the writing goes.
pub const MAX_WAIT_MS: i64 = 30_000;
/// A server that couldn't be reached is tried again after this long.
const RETRY_MS: i64 = 2 * 60_000;
/// A deleted or sent draft's row stays this long after its server copy is gone, so a
/// late save from an editor can't bring a sent draft back.
const KEEP_REMOVED_MS: i64 = 10 * 60_000;
/// A draft from elsewhere bigger than this is read from the server when it's opened.
const MAX_FETCH: u32 = 2_000_000;
/// The longest the daemon waits at shutdown for drafts to reach the server.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(8);

fn publish(state: &AppState) {
    state.events.publish(Event::MailDrafts);
}

/// Where a draft's copy is on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Copy {
    conn: Uuid,
    mailbox: String,
    validity: u32,
    uid: u32,
}

#[derive(Debug, Clone)]
struct Row {
    connection_id: Option<Uuid>,
    draft: String,
    partial: bool,
    rev: i64,
    copy: Option<Copy>,
    removed: Option<String>,
}

const COLUMNS: &str = "connection_id, draft, partial, rev, server_conn, server_mailbox, \
                       server_validity, server_uid, removed";

fn read_row(r: &rusqlite::Row) -> rusqlite::Result<Row> {
    let uuid = |s: Option<String>| s.and_then(|s| s.parse::<Uuid>().ok());
    let copy = match (
        uuid(r.get(4)?),
        r.get::<_, Option<String>>(5)?,
        r.get::<_, Option<i64>>(6)?,
        r.get::<_, Option<i64>>(7)?,
    ) {
        (Some(conn), Some(mailbox), Some(validity), Some(uid)) => Some(Copy {
            conn,
            mailbox,
            validity: validity as u32,
            uid: uid as u32,
        }),
        _ => None,
    };
    Ok(Row {
        connection_id: uuid(r.get(0)?),
        draft: r.get(1)?,
        partial: r.get(2)?,
        rev: r.get(3)?,
        copy,
        removed: r.get(8)?,
    })
}

async fn load(state: &AppState, id: Uuid) -> Result<Option<Row>, String> {
    state
        .db
        .call(move |c| {
            c.query_row(
                &format!("SELECT {COLUMNS} FROM mail_drafts WHERE id = ?1"),
                [id.to_string()],
                read_row,
            )
            .optional()
        })
        .await
        .map_err(|e| e.to_string())
}

// --- What the app uses -----------------------------------------------------------------

/// Every saved draft, latest first.
pub async fn list(state: &AppState) -> Result<Vec<MailDraftInfo>, String> {
    state
        .db
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT d.id, d.connection_id, d.subject, d.recipients, d.snippet, d.updated_at,
                        (SELECT t.id FROM mail_threads t WHERE t.id = d.reply_to),
                        d.files, d.origin
                 FROM mail_drafts d WHERE d.removed IS NULL
                 ORDER BY d.updated_at DESC",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(MailDraftInfo {
                    id: r.get::<_, String>(0)?.parse().unwrap_or_default(),
                    connection_id: r.get::<_, Option<String>>(1)?.and_then(|s| s.parse().ok()),
                    subject: r.get(2)?,
                    to: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                    snippet: r.get(4)?,
                    updated_at: r.get(5)?,
                    reply_to: r.get(6)?,
                    files: r.get(7)?,
                    from_elsewhere: r.get::<_, String>(8)? == "server",
                })
            })?;
            rows.collect()
        })
        .await
        .map_err(|e| e.to_string())
}

/// One draft, to continue it. A draft from elsewhere with files (or too large to keep
/// here) is read from the server, files and all.
pub async fn get(state: &AppState, id: Uuid) -> Result<Option<MailDraft>, String> {
    let Some(row) = load(state, id).await?.filter(|r| r.removed.is_none()) else {
        return Ok(None);
    };
    if row.partial
        && let Some(copy) = &row.copy
    {
        let account = super::account(state, copy.conn)
            .await
            .ok_or("That draft's account isn't connected any more.")?;
        let raw = sync::fetch_source(&account, &copy.mailbox, copy.uid)
            .await
            .map_err(|e| format!("Couldn't get this draft from the mail server. {e}"))?
            .ok_or("This draft isn't on the mail server any more.")?;
        let read = read_draft(&raw, &account).ok_or("This draft couldn't be read.")?;
        let mut draft = read.draft;
        draft.reply_to = serde_json::from_str::<MailDraft>(&row.draft)
            .ok()
            .and_then(|d| d.reply_to);
        draft.draft_id = Some(id);
        return Ok(Some(draft));
    }
    let mut draft: MailDraft = serde_json::from_str(&row.draft).map_err(|e| e.to_string())?;
    draft.draft_id = Some(id);
    Ok(Some(draft))
}

/// What the list shows of a draft.
struct Summary {
    subject: String,
    recipients: String,
    snippet: String,
    files: u32,
}

fn summary(draft: &MailDraft) -> Summary {
    Summary {
        subject: draft.subject.trim().to_owned(),
        recipients: serde_json::to_string(&draft.to).unwrap_or_else(|_| "[]".to_owned()),
        snippet: parse::snippet(&draft.body),
        files: draft.attachments.len() as u32 + u32::from(draft.forward_of.is_some()),
    }
}

/// The account a draft belongs to: the one it says, else its conversation's, else the
/// first one.
async fn account_of(state: &AppState, draft: &MailDraft) -> Option<Uuid> {
    let accounts = super::accounts(state).await;
    if let Some(id) = draft
        .connection_id
        .filter(|c| accounts.iter().any(|a| a.id == *c))
    {
        return Some(id);
    }
    if let Some(thread) = draft.reply_to {
        let conn: Option<String> = state
            .db
            .call(move |c| {
                c.query_row(
                    "SELECT connection_id FROM mail_threads WHERE id = ?1",
                    [thread],
                    |r| r.get(0),
                )
                .optional()
            })
            .await
            .ok()
            .flatten();
        if let Some(id) = conn.and_then(|c| c.parse::<Uuid>().ok())
            && accounts.iter().any(|a| a.id == id)
        {
            return Some(id);
        }
    }
    accounts.first().map(|a| a.id)
}

/// Saves a draft as it is now. `now`: the editor closed, so it's copied to the server
/// without waiting for a pause. A save of a draft that was just sent is ignored: an
/// editor's last save can arrive after its Send.
pub async fn save(state: &AppState, id: Uuid, draft: MailDraft, now: bool) -> Result<(), String> {
    store(state, id, draft, now, false).await
}

/// Saves a draft taken back from the outbox (Undo, Cancel): it's a draft again, even
/// though it was sent a moment ago.
pub(crate) async fn restore(state: &AppState, draft: &MailDraft) {
    let Some(id) = draft.draft_id else { return };
    if let Err(e) = store(state, id, draft.clone(), false, true).await {
        tracing::error!("couldn't keep a draft taken back from the outbox: {e}");
    }
}

async fn store(
    state: &AppState,
    id: Uuid,
    mut draft: MailDraft,
    now: bool,
    revive: bool,
) -> Result<(), String> {
    draft.draft_id = Some(id);
    let conn = account_of(state, &draft).await.map(|c| c.to_string());
    let s = summary(&draft);
    let json = serde_json::to_string(&draft).map_err(|e| e.to_string())?;
    let at = now_ms();
    let saved = state
        .db
        .call(move |c| {
            let removed: Option<Option<String>> = c
                .query_row(
                    "SELECT removed FROM mail_drafts WHERE id = ?1",
                    [id.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            if !revive && removed.flatten().as_deref() == Some("sent") {
                return Ok(false);
            }
            c.execute(
                "INSERT INTO mail_drafts (id, connection_id, draft, subject, recipients, snippet,
                     reply_to, files, origin, partial, rev, mirrored_rev, dirty_since, asap,
                     created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'here', 0, 1, 0, ?9, ?10, ?9, ?9)
                 ON CONFLICT (id) DO UPDATE SET
                     connection_id = excluded.connection_id, draft = excluded.draft,
                     subject = excluded.subject, recipients = excluded.recipients,
                     snippet = excluded.snippet, reply_to = excluded.reply_to,
                     files = excluded.files, origin = 'here', partial = 0, rev = rev + 1,
                     dirty_since = CASE WHEN removed IS NULL AND dirty_since IS NOT NULL
                                   THEN dirty_since ELSE excluded.dirty_since END,
                     asap = CASE WHEN excluded.asap = 1 THEN 1 ELSE asap END,
                     retry_at = CASE WHEN excluded.asap = 1 THEN NULL ELSE retry_at END,
                     removed = NULL, updated_at = excluded.updated_at",
                params![
                    id.to_string(),
                    conn,
                    json,
                    s.subject,
                    s.recipients,
                    s.snippet,
                    draft.reply_to,
                    s.files,
                    at,
                    now,
                ],
            )?;
            Ok(true)
        })
        .await
        .map_err(|e| e.to_string())?;
    if saved {
        publish(state);
        state.mail.drafts.poke();
    }
    Ok(())
}

/// Copies a draft to the server now (its editor closed with nothing new to save).
pub async fn mirror_soon(state: &AppState, id: Uuid) -> Result<(), String> {
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE mail_drafts SET asap = 1, retry_at = NULL
                 WHERE id = ?1 AND removed IS NULL AND rev > mirrored_rev",
                [id.to_string()],
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    state.mail.drafts.poke();
    Ok(())
}

/// Deletes a draft (Discard): here at once, from the server in the background.
pub async fn delete(state: &AppState, id: Uuid) -> Result<(), String> {
    remove(state, id, "deleted").await
}

/// The draft was queued to be sent: it leaves Drafts, here and on the server.
pub(crate) async fn sent(state: &AppState, id: Uuid) {
    if let Err(e) = remove(state, id, "sent").await {
        tracing::error!("couldn't take a sent email out of Drafts: {e}");
    }
}

async fn remove(state: &AppState, id: Uuid, why: &'static str) -> Result<(), String> {
    let at = now_ms();
    state
        .db
        .call(move |c| {
            // A sent draft that was never saved still gets its row: a save on its way
            // from the editor mustn't bring it back.
            c.execute(
                "INSERT INTO mail_drafts (id, removed, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT (id) DO UPDATE SET removed = excluded.removed, draft = '{}',
                     dirty_since = NULL, asap = 0, retry_at = NULL, updated_at = excluded.updated_at",
                params![id.to_string(), why, at],
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    publish(state);
    state.mail.drafts.poke();
    Ok(())
}

/// A disconnected account's drafts: those already on its server, or found there, go
/// with its mail; ones with changes that never reached it stay here, so no writing is
/// lost (they go to whichever account is connected next).
pub async fn forget(state: &AppState, conn: Uuid) {
    let id = conn.to_string();
    let done = state
        .db
        .call(move |c| {
            c.execute(
                "DELETE FROM mail_drafts WHERE connection_id = ?1
                     AND (origin = 'server' OR removed IS NOT NULL OR rev <= mirrored_rev)",
                [&id],
            )?;
            c.execute(
                "UPDATE mail_drafts SET connection_id = NULL WHERE connection_id = ?1",
                [&id],
            )?;
            c.execute(
                "UPDATE mail_drafts SET server_conn = NULL, server_mailbox = NULL,
                     server_validity = NULL, server_uid = NULL
                 WHERE server_conn = ?1",
                [&id],
            )
        })
        .await;
    if let Err(e) = done {
        tracing::error!("couldn't forget a disconnected account's drafts: {e}");
    }
    publish(state);
}

// --- Copying to the server -------------------------------------------------------------

/// The copier: sends drafts to the servers as they become due; forever.
pub async fn run(state: Arc<AppState>) {
    loop {
        tick(&state, now_ms()).await;
        let waiting = state
            .db
            .call(|c| {
                c.query_row(
                    "SELECT EXISTS (SELECT 1 FROM mail_drafts
                     WHERE (removed IS NULL AND rev > mirrored_rev AND connection_id IS NOT NULL)
                        OR removed IS NOT NULL)",
                    [],
                    |r| r.get::<_, bool>(0),
                )
            })
            .await
            .unwrap_or(false);
        let wait = if waiting { 1 } else { 60 };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
            _ = state.mail.drafts.wake.notified() => {}
        }
    }
}

/// Copies what's due at `now` and deletes the server copies of drafts that went
/// (the clock is an argument so tests can move it). Returns how many it handled.
pub(crate) async fn tick(state: &AppState, now: i64) -> usize {
    let due = state
        .db
        .call(move |c| {
            let ids = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> rusqlite::Result<Vec<String>> {
                let mut stmt = c.prepare(sql)?;
                let rows = stmt.query_map(p, |r| r.get(0))?;
                rows.collect()
            };
            let copy = ids(
                "SELECT id FROM mail_drafts
                 WHERE removed IS NULL AND rev > mirrored_rev AND connection_id IS NOT NULL
                   AND (retry_at IS NULL OR retry_at <= ?1)
                   AND (asap = 1 OR updated_at <= ?1 - ?2 OR dirty_since <= ?1 - ?3)
                 ORDER BY updated_at",
                params![now, PAUSE_MS, MAX_WAIT_MS],
            )?;
            let delete = ids(
                "SELECT id FROM mail_drafts
                 WHERE removed IS NOT NULL AND server_uid IS NOT NULL
                   AND (retry_at IS NULL OR retry_at <= ?1)",
                params![now],
            )?;
            c.execute(
                "DELETE FROM mail_drafts
                 WHERE removed IS NOT NULL AND server_uid IS NULL AND updated_at <= ?1",
                [now - KEEP_REMOVED_MS],
            )?;
            Ok((copy, delete))
        })
        .await;
    let (copy, delete) = match due {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("couldn't read the drafts: {e}");
            return 0;
        }
    };
    let mut handled = 0;
    for (id, copying) in copy
        .into_iter()
        .map(|i| (i, true))
        .chain(delete.into_iter().map(|i| (i, false)))
    {
        let Ok(id) = id.parse::<Uuid>() else { continue };
        let result = if copying {
            mirror(state, id).await
        } else {
            delete_copy(state, id).await
        };
        handled += 1;
        if let Err(e) = result {
            tracing::warn!(draft = %id, "couldn't update the Drafts folder on the server: {e}");
            let retry = now_ms() + RETRY_MS;
            let _ = state
                .db
                .call(move |c| {
                    c.execute(
                        "UPDATE mail_drafts SET retry_at = ?2 WHERE id = ?1",
                        params![id.to_string(), retry],
                    )
                })
                .await;
        }
    }
    handled
}

/// Everything not yet on the servers, now: the daemon is stopping.
pub async fn flush(state: &AppState) {
    let _ = state
        .db
        .call(|c| {
            c.execute(
                "UPDATE mail_drafts SET asap = 1, retry_at = NULL
                 WHERE removed IS NULL AND rev > mirrored_rev",
                [],
            )
        })
        .await;
    if tokio::time::timeout(FLUSH_TIMEOUT, tick(state, now_ms()))
        .await
        .is_err()
    {
        tracing::warn!("some drafts didn't reach the server before stopping; they will next time");
    }
}

/// The account's Drafts folder: found by its special use, else by name; made when
/// missing and `make`.
async fn drafts_folder(s: &mut ImapSession, make: bool) -> Result<Option<String>, MailError> {
    match sync::folders(s).await?.drafts {
        Some(name) => Ok(Some(name)),
        None if make => {
            s.create("Drafts").await.map_err(proto)?;
            Ok(Some("Drafts".to_owned()))
        }
        None => Ok(None),
    }
}

/// Flags messages deleted and expunges them: only those UIDs where the server allows
/// (UIDPLUS), so nobody else's deleted mail goes with them.
async fn delete_uids(s: &mut ImapSession, uids: &[u32]) -> Result<(), MailError> {
    if uids.is_empty() {
        return Ok(());
    }
    let set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    s.uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
        .await
        .map_err(proto)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(proto)?;
    if s.capabilities().await.map_err(proto)?.has_str("UIDPLUS") {
        s.uid_expunge(&set)
            .await
            .map_err(proto)?
            .try_collect::<Vec<_>>()
            .await
            .map_err(proto)?;
    } else {
        s.expunge()
            .await
            .map_err(proto)?
            .try_collect::<Vec<_>>()
            .await
            .map_err(proto)?;
    }
    Ok(())
}

/// Mimi's copies of a draft in the selected mailbox. A server that can't search that
/// header finds none: the copy Mimi last made is still known by its UID.
async fn copies_of(s: &mut ImapSession, id: Uuid) -> Result<BTreeSet<u32>, MailError> {
    match s.uid_search(format!("HEADER {HEADER} \"{id}\"")).await {
        Ok(found) => Ok(found.into_iter().collect()),
        Err(async_imap::error::Error::No(_) | async_imap::error::Error::Bad(_)) => {
            Ok(BTreeSet::new())
        }
        Err(e) => Err(proto(e)),
    }
}

/// Copies one draft to its account's Drafts folder, replacing its previous copy.
async fn mirror(state: &AppState, id: Uuid) -> Result<(), String> {
    let _one_at_a_time = state.mail.drafts.lock.lock().await;
    let Some(row) = load(state, id).await? else {
        return Ok(());
    };
    if row.removed.is_some() {
        return Ok(());
    }
    let account = match row.connection_id {
        Some(conn) => super::account(state, conn).await,
        None => None,
    };
    let Some(account) = account else {
        return Ok(());
    };
    let draft: MailDraft = serde_json::from_str(&row.draft).map_err(|e| e.to_string())?;
    let raw = message(state, &account, &draft, id).await?;
    let previous = row.copy.clone();
    let placed = async {
        let mut s = sync::session(&account.config).await?;
        let mailbox = drafts_folder(&mut s, true).await?.unwrap_or_default();
        let selected = s.select(&mailbox).await.map_err(proto)?;
        let validity = selected.uid_validity.unwrap_or(0);
        let next = selected.uid_next.unwrap_or(1);
        s.append(&mailbox, Some("(\\Draft \\Seen)"), None, &raw)
            .await
            .map_err(proto)?;
        let ours = copies_of(&mut s, id).await?;
        // Servers that can't search a header: the newest message since the append.
        let new = match ours.range(next..).next_back() {
            Some(u) => Some(*u),
            None => s
                .uid_search(format!("UID {next}:*"))
                .await
                .map_err(proto)?
                .into_iter()
                .filter(|u| *u >= next)
                .max(),
        };
        let mut old: Vec<u32> = ours.into_iter().filter(|u| Some(*u) != new).collect();
        if let Some(p) = &previous
            && p.conn == account.id
            && p.mailbox == mailbox
            && p.validity == validity
            && Some(p.uid) != new
        {
            old.push(p.uid);
        }
        delete_uids(&mut s, &old).await?;
        let _ = s.logout().await;
        Ok::<_, MailError>((mailbox, validity, new))
    }
    .await
    .map_err(|e| e.to_string())?;
    let (mailbox, validity, new) = placed;
    // The draft moved to another account (its From changed): its old copy goes.
    if let Some(p) = previous.filter(|p| p.conn != account.id || p.mailbox != mailbox)
        && let Err(e) = delete_at(state, &p, id).await
    {
        tracing::warn!(draft = %id, "couldn't delete a draft's copy in its old account: {e}");
    }
    if new.is_none() {
        tracing::warn!(draft = %id, "copied a draft, but the server didn't say where");
    }
    let (rev, conn) = (row.rev, account.id.to_string());
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE mail_drafts SET mirrored_rev = ?2,
                     dirty_since = CASE WHEN rev > ?2 THEN dirty_since END,
                     asap = CASE WHEN rev > ?2 THEN asap ELSE 0 END, retry_at = NULL,
                     server_conn = ?3, server_mailbox = ?4, server_validity = ?5,
                     server_uid = ?6
                 WHERE id = ?1",
                params![id.to_string(), rev, conn, mailbox, validity, new],
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(draft = %id, "copied a draft to the server");
    Ok(())
}

/// Deletes the server copy of a draft that was deleted or sent.
async fn delete_copy(state: &AppState, id: Uuid) -> Result<(), String> {
    let _one_at_a_time = state.mail.drafts.lock.lock().await;
    let Some(row) = load(state, id).await? else {
        return Ok(());
    };
    if row.removed.is_none() {
        return Ok(());
    }
    if let Some(copy) = &row.copy {
        delete_at(state, copy, id).await?;
    }
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE mail_drafts SET server_conn = NULL, server_mailbox = NULL,
                     server_validity = NULL, server_uid = NULL, retry_at = NULL
                 WHERE id = ?1",
                [id.to_string()],
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Deletes a copy on the server, and any other copy of Mimi's carrying the draft's id.
/// Nothing to do when its account is gone or the mailbox was renumbered.
async fn delete_at(state: &AppState, copy: &Copy, id: Uuid) -> Result<(), String> {
    let Some(account) = super::account(state, copy.conn).await else {
        return Ok(());
    };
    async {
        let mut s = sync::session(&account.config).await?;
        let selected = s.select(&copy.mailbox).await.map_err(proto)?;
        let mut uids = copies_of(&mut s, id).await?;
        if selected.uid_validity.unwrap_or(0) == copy.validity {
            uids.insert(copy.uid);
        }
        delete_uids(&mut s, &uids.into_iter().collect::<Vec<_>>()).await?;
        let _ = s.logout().await;
        Ok::<_, MailError>(())
    }
    .await
    .map_err(|e| e.to_string())
}

/// The draft as a message for the Drafts folder: like the one that would be sent (its
/// HTML cleaned the same way, its pictures and files along, threaded as a reply), with
/// its Bcc, the `\Draft` flag given at append, and Mimi's header. Addresses still being
/// typed are left out, and a draft with no recipient is still a message.
async fn message(
    state: &AppState,
    account: &Account,
    draft: &MailDraft,
    id: Uuid,
) -> Result<Vec<u8>, String> {
    let main = account.config.email.to_lowercase();
    let from = draft
        .from
        .as_deref()
        .map(|f| f.trim().to_lowercase())
        .filter(|f| !f.is_empty())
        .unwrap_or(main);
    let from: Mailbox = from
        .parse()
        .map_err(|_| format!("{from} isn't an address."))?;
    let reply = match draft.reply_to {
        Some(thread) => smtp::reply_headers(state, thread).await.unwrap_or(None),
        None => None,
    };
    // Files: the user's own, and those the assistant attached or a forward carries,
    // when they can still be fetched (they're Mimi's to send either way).
    let mut files = match draft.forward_of {
        Some(message) => super::forwarded(state, message).await.unwrap_or_default(),
        None => Vec::new(),
    };
    let mut pictures = Vec::new();
    for a in &draft.attachments {
        let Ok(mut got) = super::attached(state, std::slice::from_ref(a)).await else {
            continue;
        };
        let Some(file) = got.pop() else { continue };
        match &a.content_id {
            Some(cid) => pictures.push((cid.clone(), file)),
            None => files.push(file),
        }
    }
    let mut builder = lettre::Message::builder()
        .from(from.clone())
        .subject(draft.subject.trim())
        .date_now()
        .message_id(Some(format!(
            "<{}@{}>",
            Uuid::now_v7().simple(),
            from.email.domain()
        )))
        .envelope(
            lettre::address::Envelope::new(Some(from.email.clone()), vec![from.email.clone()])
                .map_err(|e| e.to_string())?,
        )
        .keep_bcc();
    for raw in &draft.to {
        if let Ok(m) = raw.trim().parse::<Mailbox>() {
            builder = builder.to(m);
        }
    }
    for raw in &draft.cc {
        if let Ok(m) = raw.trim().parse::<Mailbox>() {
            builder = builder.cc(m);
        }
    }
    for raw in &draft.bcc {
        if let Ok(m) = raw.trim().parse::<Mailbox>() {
            builder = builder.bcc(m);
        }
    }
    if let Some(r) = &reply {
        if let Some(mid) = &r.in_reply_to {
            builder = builder.in_reply_to(format!("<{mid}>"));
        }
        if !r.references.is_empty() {
            let refs: Vec<String> = r.references.iter().map(|m| format!("<{m}>")).collect();
            builder = builder.references(refs.join(" "));
        }
    }
    let body = draft.body.replace("\r\n", "\n");
    // The text and its HTML (with the pictures it shows), as when it's sent.
    let alternative = draft
        .html
        .as_deref()
        .filter(|h| !h.trim().is_empty())
        .map(|h| {
            let cids = pictures.iter().map(|(c, _)| c.clone()).collect();
            let (html, shown) = smtp::outgoing_html(h, &cids);
            let shown: Vec<&(String, parse::Attachment)> =
                pictures.iter().filter(|(c, _)| shown.contains(c)).collect();
            if shown.is_empty() {
                return MultiPart::alternative_plain_html(body.clone(), html);
            }
            let mut related = MultiPart::related().singlepart(SinglePart::html(html));
            for (cid, f) in shown {
                related = related.singlepart(
                    Attachment::new_inline_with_name(cid.clone(), f.name.clone())
                        .body(f.data.clone(), content_type(&f.content_type)),
                );
            }
            MultiPart::alternative()
                .singlepart(SinglePart::plain(body.clone()))
                .multipart(related)
        });
    let with_files = |mut mixed: MultiPart| {
        for f in &files {
            mixed = mixed.singlepart(
                Attachment::new(f.name.clone()).body(f.data.clone(), content_type(&f.content_type)),
            );
        }
        mixed
    };
    let built = match (alternative, files.is_empty()) {
        (None, true) => builder.header(ContentType::TEXT_PLAIN).body(body),
        (None, false) => builder.multipart(with_files(
            MultiPart::mixed().singlepart(SinglePart::plain(body)),
        )),
        (Some(alternative), true) => builder.multipart(alternative),
        (Some(alternative), false) => {
            builder.multipart(with_files(MultiPart::mixed().multipart(alternative)))
        }
    }
    .map_err(|e| e.to_string())?;
    let mut raw = format!("{HEADER}: {id}\r\n").into_bytes();
    raw.extend(built.formatted());
    Ok(raw)
}

fn content_type(raw: &str) -> ContentType {
    ContentType::parse(raw)
        .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("valid"))
}

// --- Reading the server's Drafts folder ------------------------------------------------

/// A draft read from the server.
pub(crate) struct ReadDraft {
    pub draft: MailDraft,
    /// The message it answers (In-Reply-To), to find its conversation here.
    pub in_reply_to: Option<String>,
    pub date: Option<i64>,
}

/// An address as the editor writes it: "Name <address>", or the bare address.
fn address_text(a: &mimi_protocol::MailAddress) -> String {
    let name = a
        .name
        .as_deref()
        .map(|n| {
            n.chars()
                .filter(|c| !"<>\",;".contains(*c))
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|n| !n.is_empty());
    match name {
        Some(n) => format!("{n} <{}>", a.email),
        None => a.email.clone(),
    }
}

/// Reads a draft written in another mail app into what the editor takes. The HTML is cleaned as mail that's shown is (hidden text removed), then down
/// to the editor's formatting, keeping only pictures that come with the draft. Files
/// come with their content: callers keeping it in the database drop that.
pub(crate) fn read_draft(raw: &[u8], account: &Account) -> Option<ReadDraft> {
    let parsed = parse::parse(raw)?;
    let msg = MessageParser::default().parse(raw)?;
    let main = account.config.email.to_lowercase();
    let from = Some(parsed.from.email.to_lowercase())
        .filter(|f| *f == main || parse::is_own_address(f, &main));
    let bcc: Vec<String> = msg
        .bcc()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.address.as_deref())
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let raw_html = match msg.html_part(0).map(|p| &p.body) {
        Some(PartType::Html(h)) => Some(h.to_string()),
        _ => None,
    };
    // Pictures the HTML shows by content id stay in the text; everything else is a file.
    let mut attachments = Vec::new();
    for part in msg.attachments() {
        let mime = part
            .content_type()
            .map(|c| match c.subtype() {
                Some(sub) => format!("{}/{sub}", c.ctype()),
                None => c.ctype().to_owned(),
            })
            .unwrap_or_else(|| "application/octet-stream".to_owned())
            .to_ascii_lowercase();
        let cid = part
            .content_id()
            .map(|c| {
                c.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_owned()
            })
            .filter(|c| {
                mime.starts_with("image/")
                    && raw_html
                        .as_deref()
                        .is_some_and(|h| h.contains(&format!("cid:{c}")))
            });
        if part.attachment_name().is_none() && cid.is_none() {
            continue;
        }
        attachments.push(NewMailAttachment {
            name: part
                .attachment_name()
                .map(str::to_owned)
                .unwrap_or_else(|| "picture".to_owned()),
            mime: Some(mime),
            data: base64::engine::general_purpose::STANDARD.encode(part.contents()),
            source: None,
            content_id: cid,
        });
    }
    let html = raw_html.map(|h| {
        let cids = attachments
            .iter()
            .filter_map(|a| a.content_id.clone())
            .collect();
        let (doc, _) = smtp::outgoing_html(&parse::strip_hidden(&h), &cids);
        // Just what's inside the body: the editor reads it as a fragment.
        doc.split_once("<body>")
            .and_then(|(_, rest)| rest.rsplit_once("</body>"))
            .map(|(inner, _)| inner.to_owned())
            .unwrap_or(doc)
    });
    Some(ReadDraft {
        draft: MailDraft {
            connection_id: Some(account.id),
            from,
            to: parsed.to.iter().map(address_text).collect(),
            cc: parsed.cc.iter().map(address_text).collect(),
            bcc,
            subject: parsed.subject,
            body: parsed.body,
            reply_to: None,
            forward_of: None,
            attachments,
            html: html.filter(|h| !h.trim().is_empty()),
            draft_id: None,
        },
        in_reply_to: parsed.in_reply_to,
        date: parsed.date,
    })
}

/// A draft whose copy is (or was) in a Drafts folder being read.
struct Known {
    id: String,
    /// Mimi's own (written or changed here), not another app's.
    here: bool,
    rev: i64,
    mirrored: i64,
    removed: bool,
    validity: u32,
    uid: u32,
}

/// A message in the Drafts folder, as fetched.
struct Found {
    uid: u32,
    size: u32,
    header: Vec<u8>,
    received: Option<i64>,
}

/// Brings the Drafts view in step with an account's Drafts folder, during a sync pass
/// (`mailbox`: the folder, `None` when the account has none). Drafts another app wrote
/// appear; drafts gone from the server leave unless they changed here since. Returns
/// whether anything changed.
pub(crate) async fn read_server(
    state: &AppState,
    s: &mut ImapSession,
    account: &Account,
    mailbox: Option<&str>,
) -> Result<bool, MailError> {
    let _one_at_a_time = state.mail.drafts.lock.lock().await;
    let conn = account.id.to_string();
    let db = |e: crate::db::DbError| MailError::Protocol(e.to_string());
    let Some(mailbox) = mailbox else {
        // No Drafts folder: nothing from elsewhere, and no copies of Mimi's.
        let gone = state
            .db
            .call(move |c| {
                let n = c.execute(
                    "DELETE FROM mail_drafts WHERE server_conn = ?1 AND origin = 'server'",
                    [&conn],
                )?;
                c.execute(
                    "UPDATE mail_drafts SET server_conn = NULL, server_mailbox = NULL,
                         server_validity = NULL, server_uid = NULL
                     WHERE server_conn = ?1",
                    [&conn],
                )?;
                Ok(n)
            })
            .await
            .map_err(db)?;
        if gone > 0 {
            publish(state);
        }
        return Ok(gone > 0);
    };
    let selected = s.select(mailbox).await.map_err(proto)?;
    let validity = selected.uid_validity.unwrap_or(0);
    let current: BTreeSet<u32> = if selected.exists == 0 {
        BTreeSet::new()
    } else {
        s.uid_fetch("1:*", "(UID FLAGS)")
            .await
            .map_err(proto)?
            .try_filter_map(|f| async move {
                let deleted = f.flags().any(|fl| fl == async_imap::types::Flag::Deleted);
                Ok(f.uid.filter(|_| !deleted))
            })
            .try_collect()
            .await
            .map_err(proto)?
    };

    // What's known about this folder, and what's no longer there.
    let (name, known_conn, now) = (mailbox.to_owned(), conn.clone(), now_ms());
    let seen = current.clone();
    let (known, mut changed) = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let rows: Vec<Known> = {
                let mut stmt = tx.prepare(
                    "SELECT id, origin, rev, mirrored_rev, removed, server_validity, server_uid
                     FROM mail_drafts WHERE server_conn = ?1 AND server_mailbox = ?2",
                )?;
                let rows = stmt.query_map(params![known_conn, name], |r| {
                    Ok(Known {
                        id: r.get(0)?,
                        here: r.get::<_, String>(1)? == "here",
                        rev: r.get(2)?,
                        mirrored: r.get(3)?,
                        removed: r.get::<_, Option<String>>(4)?.is_some(),
                        validity: r.get::<_, i64>(5)? as u32,
                        uid: r.get::<_, i64>(6)? as u32,
                    })
                })?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut known = BTreeSet::new();
            let mut changed = false;
            let clear = "UPDATE mail_drafts SET server_conn = NULL, server_mailbox = NULL,
                             server_validity = NULL, server_uid = NULL WHERE id = ?1";
            for Known {
                id,
                here,
                rev,
                mirrored,
                removed,
                validity: row_validity,
                uid,
            } in rows
            {
                let renumbered = row_validity != validity;
                if !renumbered && seen.contains(&uid) {
                    known.insert(uid);
                    continue;
                }
                if removed {
                    // Its copy is gone already. A renumbered one is still deleted: by
                    // its header.
                    if !renumbered {
                        tx.execute(clear, [&id])?;
                    }
                } else if !here {
                    tx.execute("DELETE FROM mail_drafts WHERE id = ?1", [&id])?;
                    changed = true;
                } else if renumbered {
                    // The server renumbered the folder: copy it again (the copy there,
                    // found by its header, is replaced).
                    tx.execute(clear, [&id])?;
                    tx.execute(
                        "UPDATE mail_drafts SET mirrored_rev = 0,
                             dirty_since = COALESCE(dirty_since, ?2) WHERE id = ?1",
                        params![id, now - MAX_WAIT_MS],
                    )?;
                } else if rev <= mirrored {
                    // Sent, deleted or taken over in another app, and not changed here.
                    tx.execute("DELETE FROM mail_drafts WHERE id = ?1", [&id])?;
                    changed = true;
                } else {
                    // Changed here since: it stays, and goes to the server again.
                    tx.execute(clear, [&id])?;
                }
            }
            tx.commit()?;
            Ok((known, changed))
        })
        .await
        .map_err(db)?;

    // Messages not known yet: Mimi's own copies are skipped, other apps' drafts come in.
    let new: Vec<u32> = current.difference(&known).copied().collect();
    if !new.is_empty() {
        let found = fetch_headers(s, &new).await?;
        let mut foreign = Vec::new();
        for f in found {
            let ours = MessageParser::default()
                .parse(&f.header)
                .and_then(|m| m.header_raw(HEADER).map(|v| v.trim().to_owned()))
                .and_then(|v| v.parse::<Uuid>().ok());
            let mine = match ours {
                Some(id) => load(state, id)
                    .await
                    .map_err(MailError::Protocol)?
                    .is_some(),
                None => false,
            };
            if !mine {
                foreign.push(f);
            }
        }
        let small: Vec<u32> = foreign
            .iter()
            .filter(|f| f.size <= MAX_FETCH)
            .map(|f| f.uid)
            .collect();
        let mut bodies = fetch_bodies(s, &small).await?;
        let mut rows = Vec::new();
        for f in foreign {
            let (raw, whole) = match bodies.remove(&f.uid) {
                Some(raw) => (raw, true),
                None => (f.header.clone(), false),
            };
            let Some(read) = read_draft(&raw, account) else {
                continue;
            };
            rows.push((f.uid, f.received, read, whole));
        }
        if !rows.is_empty() {
            let (conn, name) = (conn.clone(), mailbox.to_owned());
            state
                .db
                .call(move |c| {
                    let tx = c.transaction()?;
                    for (uid, received, read, whole) in rows {
                        let mut draft = read.draft;
                        if let Some(mid) = &read.in_reply_to {
                            draft.reply_to = tx
                                .query_row(
                                    "SELECT thread_id FROM mail_messages
                                     WHERE connection_id = ?1 AND message_id = ?2 LIMIT 1",
                                    params![conn, mid],
                                    |r| r.get(0),
                                )
                                .optional()?;
                        }
                        let partial = !whole || !draft.attachments.is_empty();
                        // Files are read from the server when the draft is opened.
                        for a in &mut draft.attachments {
                            a.data.clear();
                        }
                        let s = summary(&draft);
                        let when = read.date.or(received).unwrap_or(now);
                        tx.execute(
                            "INSERT INTO mail_drafts (id, connection_id, draft, subject,
                                 recipients, snippet, reply_to, files, origin, partial, rev,
                                 mirrored_rev, server_conn, server_mailbox, server_validity,
                                 server_uid, created_at, updated_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'server', ?9, 0, 0, ?2,
                                 ?10, ?11, ?12, ?13, ?14)",
                            params![
                                Uuid::now_v7().to_string(),
                                conn,
                                serde_json::to_string(&draft).unwrap_or_default(),
                                s.subject,
                                s.recipients,
                                s.snippet,
                                draft.reply_to,
                                s.files,
                                partial,
                                name,
                                validity,
                                uid,
                                now,
                                when,
                            ],
                        )?;
                    }
                    tx.commit()
                })
                .await
                .map_err(db)?;
            changed = true;
        }
    }
    if changed {
        publish(state);
    }
    Ok(changed)
}

/// The headers and sizes of messages in the selected mailbox.
async fn fetch_headers(s: &mut ImapSession, uids: &[u32]) -> Result<Vec<Found>, MailError> {
    let set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut out = Vec::new();
    let mut stream = s
        .uid_fetch(
            &set,
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])",
        )
        .await
        .map_err(proto)?;
    while let Some(f) = stream.next().await {
        let f = f.map_err(proto)?;
        let Some(uid) = f.uid else { continue };
        if f.flags().any(|fl| fl == async_imap::types::Flag::Deleted) {
            continue;
        }
        out.push(Found {
            uid,
            size: f.size.unwrap_or(0),
            header: f.header().map(<[u8]>::to_vec).unwrap_or_default(),
            received: f.internal_date().map(|d| d.timestamp_millis()),
        });
    }
    Ok(out)
}

/// Whole messages of the selected mailbox, by UID.
async fn fetch_bodies(
    s: &mut ImapSession,
    uids: &[u32],
) -> Result<HashMap<u32, Vec<u8>>, MailError> {
    if uids.is_empty() {
        return Ok(HashMap::new());
    }
    let set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let mut out = HashMap::new();
    let mut stream = s
        .uid_fetch(&set, "(UID BODY.PEEK[])")
        .await
        .map_err(proto)?;
    while let Some(f) = stream.next().await {
        let f = f.map_err(proto)?;
        if let (Some(uid), Some(body)) = (f.uid, f.body()) {
            out.insert(uid, body.to_vec());
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "drafts_tests.rs"]
mod tests;
