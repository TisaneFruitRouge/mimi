//! Acting on several conversations at once: the conversations the user chose together in
//! the Mail panel (⌘/Ctrl-click, Shift-click, Select all), archived, deleted, marked read
//! or unread, flagged or put in a smart folder in one request.
//!
//! The work is the single actions' (`archive`, `delete`, `mark_read`, `flags`), done in
//! one IMAP session per account rather than one per conversation. Accounts are told one
//! after the other; when one can't be reached, its conversations fail and the others
//! still go through, and the answer says which failed, in plain words.
//!
//! Read and flag changes show here at once and are undone for an account whose server
//! doesn't take them, as single flags are. Archive and delete wait for the server, then
//! change the local copy of what it moved.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use futures::TryStreamExt;
use mimi_protocol::{MailBatchAction, MailBatchFailure, MailBatchResult};
use rusqlite::{Connection, params};
use uuid::Uuid;

use super::{Account, MailError, changed, flags, folders, store, sync};
use crate::AppState;

/// Most conversations one request may change.
pub const MAX_BATCH: usize = 500;

/// Longest one account's server may take to move a batch.
const MOVE_TIMEOUT: Duration = Duration::from_secs(120);

/// Where a message is: account, mailbox, folder (inbox | sent | archive) and UID.
type Location = (Uuid, String, String, u32);

/// Does `action` to every conversation in `ids`. Fails as a whole only when the request
/// itself can't be done (no conversations, too many, a folder that's gone).
pub async fn run(
    state: &Arc<AppState>,
    ids: Vec<i64>,
    action: MailBatchAction,
) -> Result<MailBatchResult, String> {
    let mut seen = BTreeSet::new();
    let ids: Vec<i64> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
    if ids.is_empty() {
        return Err("Choose at least one conversation.".to_owned());
    }
    if ids.len() > MAX_BATCH {
        return Err(format!(
            "That's more than {MAX_BATCH} conversations at once. Choose fewer and try again."
        ));
    }
    let total = ids.len();
    let outcome = match action {
        MailBatchAction::Archive => move_threads(state, ids, sync::Destination::Archive).await?,
        MailBatchAction::Delete => move_threads(state, ids, sync::Destination::Trash).await?,
        MailBatchAction::Read { read } => set_read(state, ids, read).await?,
        MailBatchAction::Flag { flagged } => set_flagged(state, ids, flagged).await?,
        MailBatchAction::Folder { folder, member } => {
            set_folder(state, ids, folder, member).await?
        }
    };
    Ok(outcome.result(state, action, total).await)
}

/// Which conversations went through, and the accounts whose server refused.
#[derive(Default)]
struct Outcome {
    done: Vec<i64>,
    /// Per failed account: its conversations and the error.
    failed: BTreeMap<Uuid, (Vec<i64>, MailError)>,
}

impl Outcome {
    /// Sorts conversations by their accounts: done unless one of them failed.
    fn sort(threads: &BTreeMap<i64, BTreeSet<Uuid>>, errors: &HashMap<Uuid, MailError>) -> Self {
        let mut out = Outcome::default();
        for (id, accounts) in threads {
            match accounts.iter().find_map(|a| errors.get(a).map(|e| (a, e))) {
                Some((account, e)) => {
                    out.failed
                        .entry(*account)
                        .or_insert_with(|| (Vec::new(), e.clone()))
                        .0
                        .push(*id);
                }
                None => out.done.push(*id),
            }
        }
        out
    }

    fn failed_ids(&self) -> BTreeSet<i64> {
        self.failed
            .values()
            .flat_map(|(ids, _)| ids)
            .copied()
            .collect()
    }

    /// The answer, with what went wrong said in plain words.
    async fn result(
        self,
        state: &AppState,
        action: MailBatchAction,
        total: usize,
    ) -> MailBatchResult {
        if self.failed.is_empty() {
            return MailBatchResult {
                done: self.done,
                ..Default::default()
            };
        }
        let accounts = super::accounts(state).await;
        let name = |id: &Uuid| {
            accounts
                .iter()
                .find(|a| a.id == *id)
                .map(|a| a.config.email.clone())
        };
        // The account is named only when there's more than one to tell apart.
        let several = accounts.len() > 1;
        let failed: Vec<MailBatchFailure> = self
            .failed
            .iter()
            .map(|(account, (ids, e))| MailBatchFailure {
                ids: ids.clone(),
                reason: match name(account) {
                    Some(email) if several => format!("{email}: {e}"),
                    _ => e.to_string(),
                },
            })
            .collect();
        let count: usize = failed.iter().map(|f| f.ids.len()).sum();
        let verb = verb(action);
        let head = if total == 1 {
            format!("Couldn't {verb} it.")
        } else if count == total {
            format!("Couldn't {verb} any of the {total} conversations.")
        } else {
            format!("Couldn't {verb} {count} of the {total} conversations.")
        };
        let reasons: Vec<&str> = failed.iter().map(|f| f.reason.as_str()).collect();
        MailBatchResult {
            message: Some(format!("{head} {}", reasons.join(" "))),
            done: self.done,
            failed,
        }
    }
}

/// What a batch does, as in "Couldn't … it".
fn verb(action: MailBatchAction) -> &'static str {
    match action {
        MailBatchAction::Archive => "archive",
        MailBatchAction::Delete => "delete",
        MailBatchAction::Read { read: true } => "mark as read",
        MailBatchAction::Read { read: false } => "mark as unread",
        MailBatchAction::Flag { flagged: true } => "flag",
        MailBatchAction::Flag { flagged: false } => "remove the flag from",
        MailBatchAction::Folder { member: true, .. } => "add to the folder",
        MailBatchAction::Folder { member: false, .. } => "take out of the folder",
    }
}

/// Each conversation's stored copies, by conversation. Gone ones are left out.
fn locations_of(c: &Connection, ids: &[i64]) -> rusqlite::Result<BTreeMap<i64, Vec<Location>>> {
    let mut out = BTreeMap::new();
    for id in ids {
        let found = store::locations(c, *id)?;
        if !found.is_empty() {
            out.insert(*id, found);
        }
    }
    Ok(out)
}

/// The accounts each conversation's messages are in.
fn accounts_of(locations: &BTreeMap<i64, Vec<Location>>) -> BTreeMap<i64, BTreeSet<Uuid>> {
    locations
        .iter()
        .map(|(id, l)| (*id, l.iter().map(|x| x.0).collect()))
        .collect()
}

fn by_account(locations: impl IntoIterator<Item = Location>) -> BTreeMap<Uuid, Vec<Location>> {
    let mut out: BTreeMap<Uuid, Vec<Location>> = BTreeMap::new();
    for l in locations {
        out.entry(l.0).or_default().push(l);
    }
    out
}

/// Archive (the Inbox copies) or delete (every copy) on the server, one session per
/// account, then here for what the server took.
async fn move_threads(
    state: &Arc<AppState>,
    ids: Vec<i64>,
    to: sync::Destination,
) -> Result<Outcome, String> {
    let wanted = ids.clone();
    let mut found = state
        .db
        .call(move |c| locations_of(c, &wanted))
        .await
        .map_err(|e| e.to_string())?;
    let archiving = matches!(to, sync::Destination::Archive);
    if archiving {
        for l in found.values_mut() {
            l.retain(|(_, _, folder, _)| folder == "inbox");
        }
        // Already out of the Inbox: nothing to do, as for a single archive.
        found.retain(|_, l| !l.is_empty());
    }
    let threads = accounts_of(&found);
    let mut errors = HashMap::new();
    let moving = by_account(found.into_values().flatten());
    for (account, locations) in &moving {
        let result = tokio::time::timeout(MOVE_TIMEOUT, sync::move_on_server(state, locations, to))
            .await
            .unwrap_or_else(|_| Err(too_slow()));
        if let Err(e) = result {
            tracing::warn!(connection = %account, "couldn't move conversations on the server: {e}");
            errors.insert(*account, e);
        }
    }
    let outcome = Outcome::sort(&threads, &errors);
    let moved = outcome.done.clone();
    state
        .db
        .call(move |c| {
            for id in &moved {
                if archiving {
                    store::archive_locally(c, *id)?;
                } else {
                    store::forget_thread(c, *id)?;
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
    for account in moving.keys().filter(|a| !errors.contains_key(a)) {
        state.mail.poke(*account);
    }
    changed(state);
    // Conversations that were already gone, or out of the Inbox, needed nothing.
    Ok(with_gone(outcome, &ids))
}

/// The error for a server that took too long.
fn too_slow() -> MailError {
    MailError::Refused("The mail server took too long to answer. Try again in a moment.".to_owned())
}

/// One message's read state before a change.
struct Seen {
    id: i64,
    conn: Uuid,
    seen: bool,
}

fn seen_before(c: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<Seen>> {
    let mut stmt = c.prepare(
        "SELECT id, connection_id, seen FROM mail_messages WHERE thread_id = ?1 AND NOT outgoing",
    )?;
    let mut out = Vec::new();
    for thread in ids {
        let rows = stmt.query_map([thread], |r| {
            Ok(Seen {
                id: r.get(0)?,
                conn: r.get::<_, String>(1)?.parse().unwrap_or_default(),
                seen: r.get(2)?,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
    }
    Ok(out)
}

/// Marks conversations read or unread: here at once, then on each account's server in
/// one session; undone here for an account whose server didn't take it.
async fn set_read(state: &Arc<AppState>, ids: Vec<i64>, read: bool) -> Result<Outcome, String> {
    let wanted = ids.clone();
    let (found, before) = state
        .db
        .call(move |c| {
            let found = locations_of(c, &wanted)?;
            let before = seen_before(c, &wanted)?;
            for id in found.keys() {
                store::set_thread_seen(c, *id, read)?;
            }
            Ok((found, before))
        })
        .await
        .map_err(|e| e.to_string())?;
    changed(state);
    let threads = accounts_of(&found);
    let mut errors = HashMap::new();
    let accounts = super::accounts(state).await;
    for (conn, locations) in by_account(found.into_values().flatten()) {
        let result = match accounts.iter().find(|a| a.id == conn) {
            Some(account) => tokio::time::timeout(
                flags::SERVER_TIMEOUT,
                seen_on_server(account, &locations, read),
            )
            .await
            .unwrap_or_else(|_| Err(too_slow())),
            None => Err(not_connected()),
        };
        if let Err(e) = result {
            tracing::warn!(connection = %conn, "couldn't change read state on the server: {e}");
            errors.insert(conn, e);
        }
    }
    if !errors.is_empty() {
        let undo: Vec<(i64, bool)> = before
            .iter()
            .filter(|m| errors.contains_key(&m.conn) && m.seen != read)
            .map(|m| (m.id, m.seen))
            .collect();
        state
            .db
            .call(move |c| {
                for (id, seen) in &undo {
                    c.execute(
                        "UPDATE mail_messages SET seen = ?2 WHERE id = ?1 AND seen = ?3",
                        params![id, seen, read],
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
        changed(state);
    }
    Ok(with_gone(Outcome::sort(&threads, &errors), &ids))
}

/// Gone conversations count as done: there's nothing left to change.
fn with_gone(mut outcome: Outcome, ids: &[i64]) -> Outcome {
    let failed = outcome.failed_ids();
    outcome.done = ids
        .iter()
        .copied()
        .filter(|id| !failed.contains(id))
        .collect();
    outcome
}

fn not_connected() -> MailError {
    MailError::Refused("That account isn't connected any more.".to_owned())
}

/// Sets or clears `\Seen` on one account's messages, in one session.
async fn seen_on_server(
    account: &Account,
    locations: &[Location],
    read: bool,
) -> Result<(), MailError> {
    let proto = |e: async_imap::error::Error| MailError::Protocol(e.to_string());
    let mut by_mailbox: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (_, mailbox, _, uid) in locations {
        by_mailbox
            .entry(mailbox.as_str())
            .or_default()
            .push(uid.to_string());
    }
    let op = if read {
        "+FLAGS.SILENT (\\Seen)"
    } else {
        "-FLAGS.SILENT (\\Seen)"
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

/// Flags conversations or takes their flags off, as `flags::set_flagged` does for one:
/// here at once, then each account's server in one session, undone here for an account
/// whose server didn't take it.
async fn set_flagged(
    state: &Arc<AppState>,
    ids: Vec<i64>,
    flagged: bool,
) -> Result<Outcome, String> {
    let _turn = flags::ONE_AT_A_TIME.lock().await;
    let wanted = ids.clone();
    let targets = state
        .db
        .call(move |c| {
            let mut out: BTreeMap<i64, Vec<flags::Copy>> = BTreeMap::new();
            for id in &wanted {
                let all = flags::copies(c, *id)?;
                // Gone, or already so: nothing to do.
                if all.is_empty() || all.iter().any(|m| m.flagged) == flagged {
                    continue;
                }
                let targets = flags::targets(all, flagged);
                flags::set_local(c, &targets, flagged)?;
                out.insert(*id, targets);
            }
            Ok(out)
        })
        .await
        .map_err(|e| e.to_string())?;
    if targets.is_empty() {
        return Ok(with_gone(Outcome::default(), &ids));
    }
    changed(state);

    let threads: BTreeMap<i64, BTreeSet<Uuid>> = targets
        .iter()
        .map(|(id, copies)| (*id, copies.iter().map(|m| m.conn).collect()))
        .collect();
    let mut per_account: BTreeMap<Uuid, Vec<flags::Copy>> = BTreeMap::new();
    for copy in targets.values().flatten() {
        per_account.entry(copy.conn).or_default().push(copy.clone());
    }
    let accounts = super::accounts(state).await;
    let mut errors = HashMap::new();
    for (conn, copies) in &per_account {
        let result = match accounts.iter().find(|a| a.id == *conn) {
            Some(account) => tokio::time::timeout(
                flags::SERVER_TIMEOUT,
                flags::on_server(account, copies, flagged),
            )
            .await
            .unwrap_or_else(|_| Err(too_slow())),
            None => Err(not_connected()),
        };
        if let Err(e) = result {
            tracing::warn!(connection = %conn, "couldn't change flags on the server: {e}");
            errors.insert(*conn, e);
        }
    }
    let (kept, undone): (Vec<_>, Vec<_>) = per_account
        .into_iter()
        .partition(|(conn, _)| !errors.contains_key(conn));
    let kept: Vec<flags::Copy> = kept.into_iter().flat_map(|(_, c)| c).collect();
    let undone: Vec<flags::Copy> = undone.into_iter().flat_map(|(_, c)| c).collect();
    let again = state
        .db
        .call(move |c| {
            flags::undo_local(c, &undone, flagged)?;
            // A sync pass that read the server just before the change may have put the
            // old flag back here meanwhile.
            flags::set_local(c, &kept, flagged)
        })
        .await
        .map_err(|e| e.to_string())?;
    if again > 0 || !errors.is_empty() {
        changed(state);
    }
    Ok(with_gone(Outcome::sort(&threads, &errors), &ids))
}

/// Puts conversations in a smart folder or takes them out: the user's choice, kept over
/// the sorter's. Labels in Mimi only, so nothing on the server.
async fn set_folder(
    state: &Arc<AppState>,
    ids: Vec<i64>,
    folder: i64,
    member: bool,
) -> Result<Outcome, String> {
    let wanted = ids.clone();
    let found = state
        .db
        .call(move |c| {
            if !folders::exists(c, folder)? {
                return Ok(false);
            }
            for id in &wanted {
                let thread = c
                    .query_row("SELECT 1 FROM mail_threads WHERE id = ?1", [id], |_| Ok(()))
                    .map(|_| true)
                    .or_else(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => Ok(false),
                        e => Err(e),
                    })?;
                if thread {
                    folders::set(c, folder, *id, member, true)?;
                }
            }
            Ok(true)
        })
        .await
        .map_err(|e| e.to_string())?;
    if !found {
        return Err("That folder isn't there any more.".to_owned());
    }
    changed(state);
    Ok(Outcome {
        done: ids,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests;
