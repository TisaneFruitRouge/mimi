//! Keeps the local copy of an account's recent mail in step with its IMAP server.
//!
//! One loop per account: a full pass over Inbox, Sent and Archive (new messages by UID,
//! flag changes and removals for what's already here; a new UIDVALIDITY means the
//! server renumbered the mailbox, so it's fetched again), then IDLE on the Inbox until
//! the server reports something, the app pokes the loop (after sending or archiving), or
//! ten minutes pass. Servers without IDLE are polled every two minutes. Errors back off
//! from 30 seconds to 15 minutes; a refused password waits 30 minutes, so a revoked app
//! password doesn't lock the account.
//!
//! Spam and Trash are never read.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_imap::types::{Fetch, Flag, NameAttribute};
use futures::{StreamExt, TryStreamExt};
use mimi_protocol::ConnectionStatus;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::net::{self, ImapSession};
use super::store::{self, NewMessage};
use super::{Account, EmailConfig, MailError, parse};
use crate::{AppState, now_ms};

/// How far back mail is copied.
pub const WINDOW_DAYS: i64 = 90;
/// At most this many messages per mailbox on the first pass, newest first.
const FIRST_PASS_CAP: usize = 2000;
/// Messages bigger than this are stored with their headers only.
const MAX_FETCH_BYTES: u32 = 2_000_000;
const CHUNK: usize = 25;
const IDLE_FOR: Duration = Duration::from_secs(10 * 60);
const POLL_EVERY: Duration = Duration::from_secs(2 * 60);
/// Longest a full pass may take before the connection is considered stuck.
const PASS_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Longest reading one message may take before it's stored unread.
const PARSE_TIMEOUT: Duration = Duration::from_secs(20);
const DAY_MS: i64 = 24 * 3600 * 1000;

fn proto(e: async_imap::error::Error) -> MailError {
    MailError::Protocol(e.to_string())
}

pub async fn session(config: &EmailConfig) -> Result<ImapSession, MailError> {
    let s = &config.servers;
    net::imap_login(
        &s.imap_host,
        s.imap_port,
        s.imap_security,
        config.username(),
        &config.password,
    )
    .await
}

/// Signs in and out again: the check before an account is saved.
pub async fn check_login(config: &EmailConfig) -> Result<(), MailError> {
    let mut s = session(config).await?;
    s.select("INBOX").await.map_err(proto)?;
    let _ = s.logout().await;
    Ok(())
}

/// The mailboxes Mimi reads, by what they're for.
#[derive(Debug, Default, Clone)]
pub struct Folders {
    pub sent: Option<String>,
    pub archive: Option<String>,
    /// Gmail's "All Mail": where archived Gmail messages live. Not read (it holds
    /// everything, the inbox included).
    pub all: Option<String>,
    /// Where deleted mail goes. Not read.
    pub trash: Option<String>,
}

pub async fn folders(session: &mut ImapSession) -> Result<Folders, MailError> {
    let names: Vec<_> = session
        .list(Some(""), Some("*"))
        .await
        .map_err(proto)?
        .try_collect()
        .await
        .map_err(proto)?;
    let mut f = Folders::default();
    let mut sent_by_name = None;
    let mut archive_by_name = None;
    let mut trash_by_name = None;
    for n in &names {
        let attrs = n.attributes();
        if attrs.contains(&NameAttribute::NoSelect) {
            continue;
        }
        let name = n.name().to_owned();
        if attrs.contains(&NameAttribute::Sent) {
            f.sent.get_or_insert(name.clone());
        } else if attrs.contains(&NameAttribute::Archive) {
            f.archive.get_or_insert(name.clone());
        } else if attrs.contains(&NameAttribute::All) {
            f.all.get_or_insert(name.clone());
        } else if attrs.contains(&NameAttribute::Trash) {
            f.trash.get_or_insert(name.clone());
        }
        let leaf = n
            .delimiter()
            .and_then(|d| name.rsplit(d).next())
            .unwrap_or(&name)
            .to_lowercase();
        if matches!(
            leaf.as_str(),
            "sent" | "sent messages" | "sent items" | "sent mail"
        ) {
            sent_by_name.get_or_insert(name.clone());
        }
        if matches!(leaf.as_str(), "archive" | "archives") {
            archive_by_name.get_or_insert(name.clone());
        }
        if matches!(
            leaf.as_str(),
            "trash"
                | "deleted"
                | "deleted items"
                | "deleted messages"
                | "bin"
                | "corbeille"
                | "papierkorb"
                | "papelera"
        ) {
            trash_by_name.get_or_insert(name.clone());
        }
    }
    f.sent = f.sent.or(sent_by_name);
    f.archive = f.archive.or(archive_by_name);
    f.trash = f.trash.or(trash_by_name);
    Ok(f)
}

/// The account's sync loop. Runs until `token` is cancelled.
pub async fn run(state: Arc<AppState>, id: Uuid, token: CancellationToken) {
    let poke = state.mail.poke_handle(id);
    // Connections in a row that failed before syncing anything.
    let mut failures = 0u32;
    let mut first = true;
    // Whether the account shows something other than "fine" (starting, or an error), to
    // put back once a pass works.
    let mut needs_ok = true;
    loop {
        let Some(account) = super::account(&state, id).await else {
            return;
        };
        if std::mem::take(&mut first) {
            set_status(&state, id, ConnectionStatus::Ok, "Checking your mail…").await;
        }
        let mut synced = false;
        let outcome = tokio::select! {
            r = connected(&state, &account, &poke, &token, &mut needs_ok, &mut synced) => r,
            _ = token.cancelled() => return,
        };
        let wait = match outcome {
            Ok(()) => return,
            Err(MailError::Login) => {
                needs_ok = true;
                set_status(
                    &state,
                    id,
                    ConnectionStatus::Error,
                    "Can't sign in any more. The app password may have been removed: disconnect this account and connect it again.",
                )
                .await;
                Duration::from_secs(30 * 60)
            }
            // Servers and home routers close connections that sit idle waiting for mail.
            // One that had synced and then dropped isn't failing: reconnect without
            // counting it, so the account doesn't end up showing an error while mail
            // arrives fine.
            Err(e) if synced => {
                failures = 0;
                tracing::info!(connection = %id, "mail connection dropped, reconnecting: {e}");
                Duration::from_secs(30)
            }
            Err(e) => {
                failures += 1;
                tracing::warn!(connection = %id, "mail sync failed: {e}");
                if failures >= 3 {
                    needs_ok = true;
                    set_status(
                        &state,
                        id,
                        ConnectionStatus::Error,
                        &format!("Can't check mail right now. {e}"),
                    )
                    .await;
                }
                Duration::from_secs((30u64 << failures.min(5)).min(15 * 60))
            }
        };
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = poke.notified() => {}
            _ = token.cancelled() => return,
        }
    }
}

enum Wake {
    Server,
    Poke,
    Stop,
}

/// One connection's life: sync, wait for news, sync again. Returns `Ok` only when
/// stopped; any error drops the connection and the caller reconnects.
async fn connected(
    state: &Arc<AppState>,
    account: &Account,
    poke: &tokio::sync::Notify,
    token: &CancellationToken,
    needs_ok: &mut bool,
    synced: &mut bool,
) -> Result<(), MailError> {
    let mut session = session(&account.config).await?;
    let idle = session.capabilities().await.map_err(proto)?.has_str("IDLE");
    loop {
        tokio::time::timeout(PASS_TIMEOUT, pass(state, &mut session, account))
            .await
            .map_err(|_| MailError::Unreachable(account.config.servers.imap_host.clone()))??;
        *synced = true;
        if std::mem::take(needs_ok) {
            set_status(state, account.id, ConnectionStatus::Ok, super::DETAIL).await;
        }
        let wake = if idle {
            let inbox = session.select("INBOX").await.map_err(proto)?;
            // Mail that arrived since the pass read the inbox: IDLE wouldn't mention it.
            if arrived_since_pass(state, account.id, &inbox).await {
                continue;
            }
            let mut handle = session.idle();
            handle.init().await.map_err(proto)?;
            let wake = {
                let (fut, _stop) = handle.wait_with_timeout(IDLE_FOR);
                tokio::select! {
                    r = fut => r.map(|_| Wake::Server).map_err(proto),
                    _ = poke.notified() => Ok(Wake::Poke),
                    _ = token.cancelled() => Ok(Wake::Stop),
                }
            };
            session = handle.done().await.map_err(proto)?;
            wake?
        } else {
            tokio::select! {
                _ = tokio::time::sleep(POLL_EVERY) => Wake::Server,
                _ = poke.notified() => Wake::Poke,
                _ = token.cancelled() => Wake::Stop,
            }
        };
        if matches!(wake, Wake::Stop) {
            let _ = session.logout().await;
            return Ok(());
        }
        // Drop notifications nobody reads, so the channel never fills up.
        while session.unsolicited_responses.try_recv().is_ok() {}
    }
}

async fn arrived_since_pass(
    state: &AppState,
    conn: Uuid,
    inbox: &async_imap::types::Mailbox,
) -> bool {
    let stored = state
        .db
        .call(move |c| store::sync_state(c, conn, "INBOX"))
        .await
        .ok()
        .flatten();
    match stored {
        Some((validity, last)) => {
            inbox.uid_validity != Some(validity) || inbox.uid_next.is_some_and(|n| n > last + 1)
        }
        None => true,
    }
}

async fn set_status(state: &AppState, id: Uuid, status: ConnectionStatus, detail: &str) {
    state
        .connections
        .set_status(state, id, status, detail.to_owned(), None)
        .await;
}

/// One full pass over the account's mailboxes.
pub async fn pass(
    state: &Arc<AppState>,
    session: &mut ImapSession,
    account: &Account,
) -> Result<(), MailError> {
    let f = folders(session).await?;
    let (conn, me) = (account.id, account.config.email.clone());
    let mut changed = state
        .db
        .call(move |c| {
            Ok(store::backfill_received_on(c, conn, &me)?
                + store::forget_foreign_received_on(c, conn, &me)?
                + store::backfill_suspicious(c, conn)?)
        })
        .await
        .map_err(|e| MailError::Protocol(e.to_string()))?
        > 0;
    changed |= sync_mailbox(state, session, account, "INBOX", "inbox").await?;
    if let Some(sent) = &f.sent {
        changed |= sync_mailbox(state, session, account, sent, "sent").await?;
    }
    if let Some(archive) = &f.archive {
        changed |= sync_mailbox(state, session, account, archive, "archive").await?;
    }
    if changed {
        super::changed(state);
        state.mail.triage_wake.notify_one();
    }
    Ok(())
}

/// IMAP's date format for SEARCH: 1-Jan-2026.
fn imap_date(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .format("%-d-%b-%Y")
        .to_string()
}

/// Brings one mailbox up to date. Returns whether anything changed.
async fn sync_mailbox(
    state: &Arc<AppState>,
    session: &mut ImapSession,
    account: &Account,
    mailbox: &str,
    folder: &'static str,
) -> Result<bool, MailError> {
    let conn = account.id;
    let selected = session.select(mailbox).await.map_err(proto)?;
    let validity = selected.uid_validity.unwrap_or(0);
    let name = mailbox.to_owned();
    let stored = state
        .db
        .call(move |c| store::sync_state(c, conn, &name))
        .await
        .map_err(|e| MailError::Protocol(e.to_string()))?;
    let mut changed = false;
    let last_uid = match stored {
        Some((v, last)) if v == validity => Some(last),
        Some(_) => {
            tracing::info!(connection = %conn, folder, "mailbox was renumbered; fetching it again");
            let name = mailbox.to_owned();
            state
                .db
                .call(move |c| store::reset_mailbox(c, conn, &name))
                .await
                .map_err(|e| MailError::Protocol(e.to_string()))?;
            changed = true;
            None
        }
        None => None,
    };
    let window_start = now_ms() - WINDOW_DAYS * DAY_MS;

    // 1. New messages.
    let mut new_uids: Vec<u32> = match last_uid {
        Some(last) if selected.uid_next.is_some_and(|next| next <= last + 1) => Vec::new(),
        Some(last) => session
            .uid_search(format!("UID {}:*", last + 1))
            .await
            .map_err(proto)?
            .into_iter()
            .filter(|u| *u > last)
            .collect(),
        None if selected.exists == 0 => Vec::new(),
        None => session
            .uid_search(format!("SINCE {}", imap_date(window_start)))
            .await
            .map_err(proto)?
            .into_iter()
            .collect(),
    };
    new_uids.sort_unstable();
    if last_uid.is_none() && new_uids.len() > FIRST_PASS_CAP {
        new_uids.drain(..new_uids.len() - FIRST_PASS_CAP);
    }
    // Everything below the UIDNEXT seen at SELECT has been considered (fetched, or left
    // out on purpose), even where expunged messages leave gaps.
    let high = new_uids
        .last()
        .copied()
        .or(last_uid)
        .unwrap_or(0)
        .max(selected.uid_next.unwrap_or(1).saturating_sub(1));
    // Each batch is stored with the high-water mark it reached, so a pass that fails or
    // runs out of time part-way carries on from there next time instead of starting over.
    // The mark only moves over messages actually fetched, in order: a server may end a
    // fetch early without an error, and what it left out must be fetched next time.
    let mut in_order = true;
    for chunk in new_uids.chunks(CHUNK) {
        let fetched = fetch_messages(session, chunk).await?;
        let got: std::collections::HashSet<u32> = fetched.iter().map(|f| f.uid).collect();
        let prefix = chunk.iter().take_while(|u| got.contains(u)).last().copied();
        let reached = if in_order { prefix } else { None };
        in_order &= prefix == chunk.last().copied();
        let mut messages: Vec<NewMessage> = Vec::with_capacity(fetched.len());
        for m in fetched {
            messages.push(m.read(conn, mailbox, folder, &account.config.email).await);
        }
        let (name, now) = (mailbox.to_owned(), now_ms());
        let added = state
            .db
            .call(move |c| {
                let tx = c.transaction()?;
                let mut added = 0;
                for m in &messages {
                    added += usize::from(store::insert(&tx, m)?.is_some());
                }
                if let Some(reached) = reached {
                    store::set_sync_state(&tx, conn, &name, validity, reached, now)?;
                }
                tx.commit()?;
                Ok(added)
            })
            .await
            .map_err(|e| MailError::Protocol(e.to_string()))?;
        changed |= added > 0;
    }

    // 2. Flag changes and removals among what's already here.
    let name = mailbox.to_owned();
    let known = state
        .db
        .call(move |c| store::known(c, conn, &name))
        .await
        .map_err(|e| MailError::Protocol(e.to_string()))?;
    let known: BTreeMap<u32, (bool, bool)> = known
        .into_iter()
        .map(|(uid, seen, flagged)| (uid, (seen, flagged)))
        .collect();
    if let (Some(min), Some(max)) = (known.keys().next(), known.keys().next_back()) {
        let current: HashMap<u32, (bool, bool)> = session
            .uid_fetch(format!("{min}:{max}"), "(UID FLAGS)")
            .await
            .map_err(proto)?
            .try_filter_map(|f| async move {
                let deleted = f.flags().any(|fl| fl == Flag::Deleted);
                Ok(f.uid.filter(|_| !deleted).map(|uid| (uid, flags_of(&f))))
            })
            .try_collect()
            .await
            .map_err(proto)?;
        let gone: Vec<u32> = known
            .keys()
            .filter(|u| !current.contains_key(u))
            .copied()
            .collect();
        let updates: Vec<(u32, bool, bool)> = known
            .iter()
            .filter_map(|(uid, was)| {
                current
                    .get(uid)
                    .filter(|now| *now != was)
                    .map(|(seen, flagged)| (*uid, *seen, *flagged))
            })
            .collect();
        if !gone.is_empty() || !updates.is_empty() {
            changed = true;
            let name = mailbox.to_owned();
            state
                .db
                .call(move |c| {
                    let tx = c.transaction()?;
                    for (uid, seen, flagged) in updates {
                        store::set_flags(&tx, conn, &name, uid, seen, flagged)?;
                    }
                    store::remove(&tx, conn, &name, &gone)?;
                    tx.commit()
                })
                .await
                .map_err(|e| MailError::Protocol(e.to_string()))?;
        }
    }

    // 3. Forget what has aged out of the window.
    let name = mailbox.to_owned();
    let now = now_ms();
    let pruned = state
        .db
        .call(move |c| {
            let n = store::prune(c, conn, &name, window_start - 7 * DAY_MS)?;
            store::set_sync_state(c, conn, &name, validity, high, now)?;
            Ok(n)
        })
        .await
        .map_err(|e| MailError::Protocol(e.to_string()))?;
    Ok(changed || pruned > 0)
}

fn flags_of(f: &Fetch) -> (bool, bool) {
    let mut seen = false;
    let mut flagged = false;
    for fl in f.flags() {
        match fl {
            Flag::Seen => seen = true,
            Flag::Flagged => flagged = true,
            _ => {}
        }
    }
    (seen, flagged)
}

/// A message as it came off the wire.
struct Fetched {
    uid: u32,
    seen: bool,
    flagged: bool,
    received: Option<i64>,
    raw: Vec<u8>,
    /// Only the headers were fetched (the message is very large).
    headers_only: bool,
}

impl Fetched {
    /// Reads the message, away from the async threads. One that can't be read, or takes
    /// too long, is still stored (its headers, with a note for its body), so it's never
    /// fetched and tried again.
    async fn read(self, conn: Uuid, mailbox: &str, folder: &'static str, me: &str) -> NewMessage {
        let raw: Arc<[u8]> = self.raw.as_slice().into();
        let reading = tokio::task::spawn_blocking({
            let raw = raw.clone();
            move || parse::parse(&raw)
        });
        let parsed = match tokio::time::timeout(PARSE_TIMEOUT, reading).await {
            Ok(Ok(Some(mut parsed))) => {
                if self.headers_only {
                    parsed.body = "(This message is too large to copy. Open it in your usual \
                                   mail app to read it.)"
                        .to_owned();
                    // Only the headers were fetched; the HTML comes from the server on
                    // first view (`mail::content`).
                    parsed.html = None;
                }
                parsed
            }
            _ => {
                tracing::warn!(connection = %conn, folder, uid = self.uid, "couldn't read a message");
                unreadable(&raw)
            }
        };
        self.into_new(parsed, conn, mailbox, folder, me)
    }

    fn into_new(
        self,
        parsed: parse::Parsed,
        conn: Uuid,
        mailbox: &str,
        folder: &'static str,
        me: &str,
    ) -> NewMessage {
        let outgoing = folder == "sent" || parsed.from.email.eq_ignore_ascii_case(me);
        NewMessage {
            connection_id: conn,
            mailbox: mailbox.to_owned(),
            folder,
            uid: self.uid,
            received: self.received.unwrap_or_else(now_ms),
            seen: self.seen || outgoing,
            flagged: self.flagged,
            outgoing,
            received_on: (!outgoing)
                .then(|| parse::received_on(&parsed.delivered_to, &parsed.to, &parsed.cc, me)),
            parsed,
        }
    }
}

/// A message that couldn't be read whole: its headers if those can be, with a note for
/// its body.
fn unreadable(raw: &[u8]) -> parse::Parsed {
    let headers_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(raw.len(), |p| p + 4)
        .min(64 * 1024);
    let mut parsed = parse::parse(&raw[..headers_end]).unwrap_or_else(|| parse::Parsed {
        message_id: None,
        in_reply_to: None,
        refs: Vec::new(),
        from: mimi_protocol::MailAddress {
            name: None,
            email: String::new(),
        },
        to: Vec::new(),
        cc: Vec::new(),
        subject: String::new(),
        date: None,
        body: String::new(),
        attachments: Vec::new(),
        automated: false,
        suspicious: false,
        delivered_to: Vec::new(),
        html: None,
    });
    parsed.body =
        "(This message couldn't be read here. Open it in your usual mail app to read it.)"
            .to_owned();
    // "No HTML", not "unknown": showing it must not fetch and parse it again.
    parsed.html = Some(String::new());
    parsed
}

async fn fetch_messages(
    session: &mut ImapSession,
    uids: &[u32],
) -> Result<Vec<Fetched>, MailError> {
    let set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let sizes: HashMap<u32, u32> = session
        .uid_fetch(&set, "(UID RFC822.SIZE)")
        .await
        .map_err(proto)?
        .try_filter_map(|f| async move { Ok(f.uid.map(|u| (u, f.size.unwrap_or(0)))) })
        .try_collect()
        .await
        .map_err(proto)?;
    let (small, big): (Vec<u32>, Vec<u32>) = uids
        .iter()
        .filter(|u| sizes.contains_key(u))
        .partition(|u| sizes[u] <= MAX_FETCH_BYTES);
    let mut out = Vec::new();
    for (list, query, headers_only) in [
        (small, "(UID FLAGS INTERNALDATE BODY.PEEK[])", false),
        (big, "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER])", true),
    ] {
        if list.is_empty() {
            continue;
        }
        let set = list
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut stream = session.uid_fetch(&set, query).await.map_err(proto)?;
        while let Some(f) = stream.next().await {
            let f = f.map_err(proto)?;
            let Some(uid) = f.uid else { continue };
            if f.flags().any(|fl| fl == Flag::Deleted) {
                continue;
            }
            let raw = if headers_only { f.header() } else { f.body() };
            let Some(raw) = raw else { continue };
            let (seen, flagged) = flags_of(&f);
            out.push(Fetched {
                uid,
                seen,
                flagged,
                received: f.internal_date().map(|d| d.timestamp_millis()),
                raw: raw.to_vec(),
                headers_only,
            });
        }
    }
    Ok(out)
}

// --- Changes made from Mimi -----------------------------------------------------------

fn group(locations: &[(Uuid, String, String, u32)]) -> HashMap<(Uuid, String), Vec<u32>> {
    let mut by: HashMap<(Uuid, String), Vec<u32>> = HashMap::new();
    for (conn, mailbox, _, uid) in locations {
        by.entry((*conn, mailbox.clone())).or_default().push(*uid);
    }
    by
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Sets or clears \Seen on the server, in the background (the local copy already has it).
pub fn spawn_flag_change(
    state: Arc<AppState>,
    locations: Vec<(Uuid, String, String, u32)>,
    read: bool,
) {
    tokio::spawn(async move {
        let accounts = super::accounts(&state).await;
        for ((conn, mailbox), uids) in group(&locations) {
            let Some(account) = accounts.iter().find(|a| a.id == conn) else {
                continue;
            };
            let result = async {
                let mut s = session(&account.config).await?;
                s.select(&mailbox).await.map_err(proto)?;
                let op = if read {
                    "+FLAGS.SILENT (\\Seen)"
                } else {
                    "-FLAGS.SILENT (\\Seen)"
                };
                s.uid_store(uid_set(&uids), op)
                    .await
                    .map_err(proto)?
                    .try_collect::<Vec<_>>()
                    .await
                    .map_err(proto)?;
                let _ = s.logout().await;
                Ok::<_, MailError>(())
            }
            .await;
            if let Err(e) = result {
                tracing::warn!(connection = %conn, "couldn't update read state on the server: {e}");
            }
        }
    });
}

/// Moves messages out of the inbox on the server: to Archive, or on Gmail, out of the
/// Inbox label (they stay in All Mail).
/// One message's full source, fetched from the server (only names of attachments are
/// kept locally). Leaves it unread if it was.
pub async fn fetch_source(
    account: &Account,
    mailbox: &str,
    uid: u32,
) -> Result<Option<Vec<u8>>, MailError> {
    let mut s = session(&account.config).await?;
    s.select(mailbox).await.map_err(proto)?;
    let mut raw = None;
    {
        let mut stream = s
            .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
            .await
            .map_err(proto)?;
        while let Some(f) = stream.next().await {
            let f = f.map_err(proto)?;
            if f.uid == Some(uid) {
                raw = f.body().map(<[u8]>::to_vec);
            }
        }
    }
    let _ = s.logout().await;
    Ok(raw)
}

pub async fn archive_on_server(
    state: &AppState,
    locations: &[(Uuid, String, String, u32)],
) -> Result<(), String> {
    move_on_server(state, locations, Destination::Archive)
        .await
        .map_err(|e| format!("Couldn't archive it: {e}"))
}

/// Moves messages to the account's Trash (made if missing). Messages already there
/// stay put.
pub async fn trash_on_server(
    state: &AppState,
    locations: &[(Uuid, String, String, u32)],
) -> Result<(), String> {
    move_on_server(state, locations, Destination::Trash)
        .await
        .map_err(|e| format!("Couldn't delete it: {e}"))
}

#[derive(Clone, Copy)]
enum Destination {
    Archive,
    Trash,
}

async fn move_on_server(
    state: &AppState,
    locations: &[(Uuid, String, String, u32)],
    to: Destination,
) -> Result<(), MailError> {
    let accounts = super::accounts(state).await;
    for ((conn, mailbox), uids) in group(locations) {
        let account = accounts.iter().find(|a| a.id == conn).ok_or_else(|| {
            MailError::Protocol("that account isn't connected any more".to_owned())
        })?;
        let mut s = session(&account.config).await?;
        let f = folders(&mut s).await?;
        let (found, made) = match to {
            Destination::Archive => (f.archive.or(f.all), "Archive"),
            Destination::Trash => (f.trash, "Trash"),
        };
        let target = match found {
            Some(t) => t,
            None => {
                s.create(made).await.map_err(proto)?;
                made.to_owned()
            }
        };
        if target == mailbox {
            let _ = s.logout().await;
            continue;
        }
        let can_move = s.capabilities().await.map_err(proto)?.has_str("MOVE");
        s.select(&mailbox).await.map_err(proto)?;
        let set = uid_set(&uids);
        if can_move {
            s.uid_mv(&set, &target).await.map_err(proto)?;
        } else {
            s.uid_copy(&set, &target).await.map_err(proto)?;
            s.uid_store(&set, "+FLAGS.SILENT (\\Deleted)")
                .await
                .map_err(proto)?
                .try_collect::<Vec<_>>()
                .await
                .map_err(proto)?;
            s.expunge()
                .await
                .map_err(proto)?
                .try_collect::<Vec<_>>()
                .await
                .map_err(proto)?;
        }
        let _ = s.logout().await;
    }
    Ok(())
}

/// Services that file sent mail themselves; appending would make a second copy.
const FILES_SENT_ITSELF: &[&str] = &["gmail", "proton"];

/// Puts a copy of a sent message in the account's Sent mailbox. Failing here doesn't
/// undo the send, so it's only logged.
pub async fn file_sent(account: &Account, raw: Vec<u8>) {
    if FILES_SENT_ITSELF.contains(&account.config.preset.as_str()) {
        return;
    }
    let result = async {
        let mut s = session(&account.config).await?;
        let sent = match folders(&mut s).await?.sent {
            Some(sent) => sent,
            None => {
                s.create("Sent").await.map_err(proto)?;
                "Sent".to_owned()
            }
        };
        s.append(&sent, Some("(\\Seen)"), None, &raw)
            .await
            .map_err(proto)?;
        let _ = s.logout().await;
        Ok::<_, MailError>(())
    }
    .await;
    if let Err(e) = result {
        tracing::warn!(connection = %account.id, "sent, but couldn't file a copy in Sent: {e}");
    }
}

/// Every account's UIDs seen so far, for tests.
#[cfg(test)]
pub async fn uids(state: &AppState, conn: Uuid, mailbox: &str) -> std::collections::HashSet<u32> {
    let name = mailbox.to_owned();
    state
        .db
        .call(move |c| store::known(c, conn, &name))
        .await
        .unwrap()
        .into_iter()
        .map(|(u, ..)| u)
        .collect()
}
