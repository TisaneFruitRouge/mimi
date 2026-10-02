//! Mail older than what sync keeps, found on the server when the user (or the assistant)
//! searches for it.
//!
//! Sync copies the last `sync::WINDOW_DAYS` and forgets what's older, so a search here
//! can't see years-old mail. This asks the server: `UID SEARCH CHARSET UTF-8 BEFORE
//! <window start> TEXT …` in Inbox, Sent and Archive (never Spam or Trash), falling back
//! to a plain search when a server refuses UTF-8. The newest `CAP` matches across those
//! mailboxes are fetched and stored through sync's own pipeline (`sync::fetch_messages`,
//! `Fetched::read`: parsing, safe HTML, the suspicious check, received_on), so they open,
//! reply, forward and mention like any conversation.
//!
//! What's fetched is *kept* (`mail_messages.kept_until`, migration 0030), not synced:
//! - a week after it was found, or 30 days after it was last opened, then forgotten;
//! - left out of the mailboxes, counts, sorting, smart folders and correspondents, and of
//!   sync's flag sweep (whose UID range it would widen); `sweep` checks it instead;
//! - never announced as new mail, so it wakes no sorting, notification or memory.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_imap::types::Flag;
use futures::TryStreamExt;
use mimi_protocol::{MailOlderResults, MailThread};
use rusqlite::{Connection, OptionalExtension, params};
use tokio::time::Instant;
use uuid::Uuid;

use super::net::ImapSession;
use super::store::{self, NewMessage};
use super::sync::{self, proto};
use super::{Account, MailError};
use crate::{AppState, now_ms};

/// At most this many older messages are brought in per search and account, newest first.
pub const CAP: usize = 50;
const DAY_MS: i64 = 24 * 3600 * 1000;
/// How long mail found by a search is kept when it isn't opened.
const KEEP_FOUND: i64 = 7 * DAY_MS;
/// How long it's kept after it was last opened.
const KEEP_OPENED: i64 = 30 * DAY_MS;
/// Longest a search of the servers may take; what came in by then is shown.
const TIME_LIMIT: Duration = Duration::from_secs(45);
const CHUNK: usize = 25;
const MAX_WORDS: usize = 8;

/// The mailboxes searched, by what they're for. Spam and Trash are never among them.
const MAILBOXES: [&str; 3] = ["inbox", "sent", "archive"];

/// Where older mail starts: the start of what sync copies.
pub fn window_start(now: i64) -> i64 {
    now - sync::WINDOW_DAYS * DAY_MS
}

// --- What to look for -----------------------------------------------------------------

/// Search terms: words (all must match) and people (any of them).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Terms {
    pub words: Vec<String>,
    /// Names or addresses, matched against From, To and Cc.
    pub people: Vec<String>,
}

impl Terms {
    /// The words of what the user typed, as the local search splits them.
    pub fn new(text: &str, people: Vec<String>) -> Self {
        let words = text
            .split(|c: char| !c.is_alphanumeric() && c != '@' && c != '.')
            .map(|w| w.trim_matches('.').to_owned())
            .filter(|w| w.chars().count() >= 2)
            .take(MAX_WORDS)
            .collect();
        let people = people
            .into_iter()
            .map(|p| clean(&p))
            .filter(|p| !p.is_empty())
            .take(MAX_WORDS)
            .collect();
        Terms { words, people }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.people.is_empty()
    }
}

/// Without control characters (which would end the command line) or surrounding space.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// One string argument of a SEARCH. Non-ASCII text goes as a non-synchronising literal
/// when the server takes them (LITERAL+), else quoted as UTF-8, which most servers accept.
fn string_arg(s: &str, literal_plus: bool) -> String {
    let s = clean(s);
    if !s.is_ascii() && literal_plus {
        format!("{{{}+}}\r\n{s}", s.len())
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// `OR a OR b c`: any of the keys.
fn any(mut keys: Vec<String>) -> Option<String> {
    let last = keys.pop()?;
    Some(
        keys.into_iter()
            .rev()
            .fold(last, |rest, k| format!("OR {k} {rest}")),
    )
}

/// The criteria of `UID SEARCH`: mail from before `before` matching every word and any
/// of the people. Without `utf8` (the server refused it), terms that aren't plain ASCII
/// are left out; `None` when nothing is left to look for.
pub fn criteria(before: i64, terms: &Terms, utf8: bool, literal_plus: bool) -> Option<String> {
    let keep = |w: &&String| utf8 || w.is_ascii();
    let words: Vec<&String> = terms.words.iter().filter(keep).collect();
    let people: Vec<&String> = terms.people.iter().filter(keep).collect();
    if words.is_empty() && people.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if utf8 {
        parts.push("CHARSET UTF-8".to_owned());
    }
    parts.push(format!("BEFORE {}", sync::imap_date(before)));
    for w in words {
        parts.push(format!("TEXT {}", string_arg(w, literal_plus)));
    }
    let people: Vec<String> = people
        .into_iter()
        .map(|p| {
            let a = string_arg(p, literal_plus);
            format!("OR FROM {a} OR TO {a} CC {a}")
        })
        .collect();
    parts.extend(any(people).map(|p| format!("({p})")));
    Some(parts.join(" "))
}

// --- Searching ------------------------------------------------------------------------

/// Searches the servers of every account (or only `account`) for older mail matching
/// `terms`, brings in the newest `cap` matches of each, and returns their conversations,
/// newest first. Accounts that can't be searched are named in `problems`.
pub async fn search(
    state: &Arc<AppState>,
    terms: &Terms,
    account: Option<Uuid>,
    cap: usize,
) -> Result<MailOlderResults, String> {
    if terms.is_empty() {
        return Err("Type something to search for.".to_owned());
    }
    let now = now_ms();
    let before = window_start(now);
    let accounts: Vec<Account> = super::accounts(state)
        .await
        .into_iter()
        .filter(|a| account.is_none_or(|id| id == a.id))
        .collect();
    if accounts.is_empty() {
        return Err("No email account is connected.".to_owned());
    }
    // What has had its time is gone before anything new comes in.
    let _ = state.db.call(move |c| forget_expired(c, None, now)).await;
    let deadline = Instant::now() + TIME_LIMIT;
    let found = futures::future::join_all(
        accounts
            .iter()
            .map(|a| search_account(state, a, terms, before, cap, deadline)),
    )
    .await;
    let mut hits = Vec::new();
    let mut more = false;
    let mut problems = Vec::new();
    for (a, result) in accounts.iter().zip(found) {
        match result {
            Ok(f) => {
                more |= f.more;
                if f.unfinished {
                    problems.push(format!(
                        "{}: the mail server was slow, so only some of the older mail came in.",
                        a.config.email
                    ));
                }
                if let Some(e) = &f.failed {
                    problems.push(format!("{}: {e}", a.config.email));
                }
                if f.narrowed {
                    problems.push(format!(
                        "{}: the mail server can't search for accented or non-Latin letters, so \
                         those words were left out.",
                        a.config.email
                    ));
                }
                hits.extend(
                    f.hits
                        .into_iter()
                        .map(|(mailbox, uid)| (a.id, mailbox, uid)),
                );
            }
            Err(e) => problems.push(format!("{}: {e}", a.config.email)),
        }
    }
    let me = super::my_addresses(state).await;
    let threads = state
        .db
        .call(move |c| threads_of(c, &hits, &me))
        .await
        .map_err(|e| e.to_string())?;
    Ok(MailOlderResults {
        threads,
        before,
        more,
        problems,
    })
}

/// What one account's search found.
#[derive(Default)]
struct Found {
    /// The matches stored here (just fetched, or already here): (mailbox, uid).
    hits: Vec<(String, u32)>,
    /// More matched than `cap`.
    more: bool,
    /// Time ran out before everything came in.
    unfinished: bool,
    /// The server refused UTF-8, so some words were left out.
    narrowed: bool,
    /// Fetching failed part-way (what came in before is still shown).
    failed: Option<MailError>,
}

async fn search_account(
    state: &Arc<AppState>,
    account: &Account,
    terms: &Terms,
    before: i64,
    cap: usize,
    deadline: Instant,
) -> Result<Found, MailError> {
    let slow = || MailError::Refused("the mail server took too long to answer.".to_owned());
    let mut session = tokio::time::timeout_at(deadline, sync::session(&account.config))
        .await
        .map_err(|_| slow())??;
    let result = tokio::time::timeout_at(
        deadline,
        find(state, &mut session, account, terms, before, cap),
    )
    .await;
    let (mut found, wanted) = match result {
        Ok(r) => r?,
        Err(_) => return Err(slow()),
    };
    // Bring in what isn't here yet, a chunk at a time, until time runs out.
    for (mailbox, folder, uids) in wanted {
        if found.unfinished {
            break;
        }
        let fetched = tokio::time::timeout_at(
            deadline,
            bring_in(
                state,
                &mut session,
                account,
                (&mailbox, folder),
                &uids,
                &mut found.hits,
            ),
        )
        .await;
        match fetched {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                found.failed = Some(e);
                break;
            }
            Err(_) => found.unfinished = true,
        }
    }
    if !found.unfinished && found.failed.is_none() {
        let _ = session.logout().await;
    }
    Ok(found)
}

/// Mailboxes, with the UIDs to fetch in each: (mailbox, folder, uids).
type Wanted = Vec<(String, &'static str, Vec<u32>)>;

/// Searches each mailbox, and picks the newest `cap` matches across them. Those already
/// stored here go straight into `hits`; the rest are returned to fetch.
async fn find(
    state: &Arc<AppState>,
    session: &mut ImapSession,
    account: &Account,
    terms: &Terms,
    before: i64,
    cap: usize,
) -> Result<(Found, Wanted), MailError> {
    let conn = account.id;
    let literal_plus = {
        let caps = session.capabilities().await.map_err(proto)?;
        caps.has_str("LITERAL+") || caps.has_str("LITERAL-")
    };
    let folders = sync::folders(session).await?;
    let names = [Some("INBOX".to_owned()), folders.sent, folders.archive];
    let mut found = Found::default();
    let mut utf8 = true;
    // (arrived, mailbox index, uid) of every candidate.
    let mut candidates: Vec<(i64, usize, u32)> = Vec::new();
    let mut boxes: Vec<(String, &'static str)> = Vec::new();
    for (name, folder) in names.into_iter().zip(MAILBOXES) {
        let Some(name) = name else { continue };
        let selected = session.examine(&name).await.map_err(proto)?;
        // Renumbered since the last pass: sync fetches it again first.
        let key = name.clone();
        let stored = state
            .db
            .call(move |c| store::sync_state(c, conn, &key))
            .await
            .map_err(|e| MailError::Protocol(e.to_string()))?;
        if stored.is_some_and(|(v, _)| Some(v) != selected.uid_validity) {
            continue;
        }
        if selected.exists == 0 {
            continue;
        }
        let mut uids = None;
        while uids.is_none() {
            let Some(query) = criteria(before, terms, utf8, literal_plus) else {
                break;
            };
            match uid_search(session, &query).await? {
                Ok(found) => uids = Some(found),
                // BADCHARSET (or a server that doesn't know CHARSET at all): search again
                // without it, in plain ASCII.
                Err(_) if utf8 => {
                    utf8 = false;
                    found.narrowed = criteria(before, terms, false, literal_plus)
                        .is_some_and(|_| terms_dropped(terms));
                }
                Err(refused) => {
                    return Err(MailError::Refused(format!(
                        "the mail server refused the search ({refused})."
                    )));
                }
            }
        }
        let Some(uids) = uids else {
            return Err(MailError::Refused(
                "the mail server can't search for accented or non-Latin letters.".to_owned(),
            ));
        };
        let mut uids = uids;
        // Highest UIDs first: the newest arrivals, as a rule.
        uids.sort_unstable_by(|a, b| b.cmp(a));
        found.more |= uids.len() > cap;
        uids.truncate(cap);
        if uids.is_empty() {
            continue;
        }
        let index = boxes.len();
        boxes.push((name, folder));
        let dates: Vec<(i64, usize, u32)> = session
            .uid_fetch(uid_set(&uids), "(UID INTERNALDATE)")
            .await
            .map_err(proto)?
            .try_filter_map(|f| async move {
                Ok(f.uid.map(|u| {
                    let at = f.internal_date().map_or(0, |d| d.timestamp_millis());
                    (at, index, u)
                }))
            })
            .try_collect()
            .await
            .map_err(proto)?;
        candidates.extend(dates);
    }
    candidates.sort_unstable_by(|a, b| b.cmp(a));
    found.more |= candidates.len() > cap;
    candidates.truncate(cap);
    let mut by_box: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for (_, index, uid) in candidates {
        by_box.entry(index).or_default().push(uid);
    }
    let mut wanted = Vec::new();
    for (index, uids) in by_box {
        let (name, folder) = boxes[index].clone();
        let key = name.clone();
        let here = state
            .db
            .call(move |c| stored_uids(c, conn, &key))
            .await
            .map_err(|e| MailError::Protocol(e.to_string()))?;
        let (have, need): (Vec<u32>, Vec<u32>) = uids.into_iter().partition(|u| here.contains(u));
        found
            .hits
            .extend(have.into_iter().map(|u| (name.clone(), u)));
        if !need.is_empty() {
            wanted.push((name, folder, need));
        }
    }
    Ok((found, wanted))
}

/// `UID SEARCH`, telling a refusal (NO or BAD, with the server's words) from no matches:
/// async-imap's own `uid_search` reads both as an empty result.
async fn uid_search(
    session: &mut ImapSession,
    query: &str,
) -> Result<Result<Vec<u32>, String>, MailError> {
    use async_imap::imap_proto::{MailboxDatum, Response, Status};
    let tag = session
        .run_command(format!("UID SEARCH {query}"))
        .await
        .map_err(proto)?;
    let mut uids = Vec::new();
    loop {
        let response = session
            .read_response()
            .await
            .map_err(|e| MailError::Protocol(e.to_string()))?
            .ok_or_else(|| MailError::Protocol("the connection closed".to_owned()))?;
        match response.parsed() {
            Response::MailboxData(MailboxDatum::Search(found)) => uids.extend(found),
            Response::Done {
                tag: done,
                status,
                information,
                ..
            } if *done == tag => {
                return Ok(match status {
                    Status::Ok => Ok(uids),
                    _ => Err(information
                        .as_deref()
                        .unwrap_or("no reason given")
                        .to_owned()),
                });
            }
            // Anything else the server mentions on the way (new mail, flags) is left for
            // the next sync pass.
            _ => {}
        }
    }
}

/// Whether searching without UTF-8 leaves some terms out.
fn terms_dropped(terms: &Terms) -> bool {
    terms
        .words
        .iter()
        .chain(&terms.people)
        .any(|w| !w.is_ascii())
}

/// Fetches messages and keeps them for a while, through sync's own pipeline, a chunk at
/// a time. Adds each stored message to `hits` as it goes, so a search that runs out of
/// time still shows what came in.
async fn bring_in(
    state: &Arc<AppState>,
    session: &mut ImapSession,
    account: &Account,
    (mailbox, folder): (&str, &'static str),
    uids: &[u32],
    hits: &mut Vec<(String, u32)>,
) -> Result<(), MailError> {
    let conn = account.id;
    session.examine(mailbox).await.map_err(proto)?;
    let mut sorted = uids.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    for chunk in sorted.chunks(CHUNK) {
        let fetched = sync::fetch_messages(session, chunk).await?;
        let mut messages: Vec<NewMessage> = Vec::with_capacity(fetched.len());
        for m in fetched {
            messages.push(m.read(conn, mailbox, folder, &account.config.email).await);
        }
        let until = now_ms() + KEEP_FOUND;
        let got = state
            .db
            .call(move |c| {
                let tx = c.transaction()?;
                let mut got = Vec::new();
                for m in &messages {
                    if store::insert(&tx, m)?.is_some() {
                        tx.execute(
                            "UPDATE mail_messages SET kept_until = ?4
                             WHERE connection_id = ?1 AND mailbox = ?2 AND uid = ?3",
                            params![m.connection_id.to_string(), m.mailbox, m.uid, until],
                        )?;
                    }
                    got.push(m.uid);
                }
                tx.commit()?;
                Ok(got)
            })
            .await
            .map_err(|e| MailError::Protocol(e.to_string()))?;
        hits.extend(got.into_iter().map(|u| (mailbox.to_owned(), u)));
    }
    Ok(())
}

fn uid_set(uids: &[u32]) -> String {
    uids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Every UID of a mailbox stored here, synced or kept.
fn stored_uids(c: &Connection, conn: Uuid, mailbox: &str) -> rusqlite::Result<HashSet<u32>> {
    c.prepare("SELECT uid FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2")?
        .query_map(params![conn.to_string(), mailbox], |r| r.get(0))?
        .collect()
}

/// The conversations of the messages found, newest first, each once.
fn threads_of(
    c: &Connection,
    hits: &[(Uuid, String, u32)],
    me: &[String],
) -> rusqlite::Result<Vec<MailThread>> {
    let mut ids = Vec::new();
    for (conn, mailbox, uid) in hits {
        let id: Option<i64> = c
            .query_row(
                "SELECT thread_id FROM mail_messages
                 WHERE connection_id = ?1 AND mailbox = ?2 AND uid = ?3",
                params![conn.to_string(), mailbox, uid],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = id.filter(|id| !ids.contains(id)) {
            ids.push(id);
        }
    }
    let mut threads = Vec::new();
    for id in ids {
        threads.extend(store::thread(c, id, me)?);
    }
    threads.sort_by_key(|t| std::cmp::Reverse(t.last_at));
    Ok(threads)
}

// --- Keeping --------------------------------------------------------------------------

/// The user opened a conversation: older mail in it stays 30 days from now.
pub async fn opened(state: &AppState, thread: i64) {
    let until = now_ms() + KEEP_OPENED;
    let result = state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE mail_messages SET kept_until = max(kept_until, ?2)
                 WHERE thread_id = ?1 AND kept_until IS NOT NULL",
                params![thread, until],
            )
        })
        .await;
    if let Err(e) = result {
        tracing::warn!("couldn't keep older mail that was opened: {e}");
    }
}

/// Forgets older mail whose time is up (one account's mailbox, or everything). Returns
/// how many messages went.
pub fn forget_expired(
    c: &Connection,
    mailbox: Option<(Uuid, &str)>,
    now: i64,
) -> rusqlite::Result<usize> {
    let n = match mailbox {
        Some((conn, name)) => c.execute(
            "DELETE FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2
               AND kept_until IS NOT NULL AND kept_until < ?3",
            params![conn.to_string(), name, now],
        )?,
        None => c.execute(
            "DELETE FROM mail_messages WHERE kept_until IS NOT NULL AND kept_until < ?1",
            [now],
        )?,
    };
    if n > 0 {
        store::drop_empty_threads(c)?;
    }
    Ok(n)
}

/// Older mail kept from one mailbox: uid → (seen, flagged).
fn kept(c: &Connection, conn: Uuid, mailbox: &str) -> rusqlite::Result<HashMap<u32, (bool, bool)>> {
    c.prepare(
        "SELECT uid, seen, flagged FROM mail_messages
         WHERE connection_id = ?1 AND mailbox = ?2 AND kept_until IS NOT NULL",
    )?
    .query_map(params![conn.to_string(), mailbox], |r| {
        Ok((r.get(0)?, (r.get(1)?, r.get(2)?)))
    })?
    .collect()
}

/// Part of each sync pass, with `mailbox` selected: forgets kept mail whose time is up,
/// then brings flag changes and removals of the rest, by their own UIDs (not a range, so
/// a years-old UID doesn't make sync sweep the whole mailbox). Returns whether anything
/// changed.
pub(super) async fn sweep(
    state: &Arc<AppState>,
    session: &mut ImapSession,
    conn: Uuid,
    mailbox: &str,
) -> Result<bool, MailError> {
    let (name, now) = (mailbox.to_owned(), now_ms());
    let (expired, kept) = state
        .db
        .call(move |c| {
            let n = forget_expired(c, Some((conn, &name)), now)?;
            Ok((n, kept(c, conn, &name)?))
        })
        .await
        .map_err(|e| MailError::Protocol(e.to_string()))?;
    if kept.is_empty() {
        return Ok(expired > 0);
    }
    let mut uids: Vec<u32> = kept.keys().copied().collect();
    uids.sort_unstable();
    let current: HashMap<u32, (bool, bool)> = session
        .uid_fetch(uid_set(&uids), "(UID FLAGS)")
        .await
        .map_err(proto)?
        .try_filter_map(|f| async move {
            let deleted = f.flags().any(|fl| fl == Flag::Deleted);
            Ok(f.uid
                .filter(|_| !deleted)
                .map(|uid| (uid, sync::flags_of(&f))))
        })
        .try_collect()
        .await
        .map_err(proto)?;
    let gone: Vec<u32> = uids
        .iter()
        .filter(|u| !current.contains_key(u))
        .copied()
        .collect();
    let updates: Vec<(u32, bool, bool)> = kept
        .iter()
        .filter_map(|(uid, was)| {
            current
                .get(uid)
                .filter(|now| *now != was)
                .map(|(seen, flagged)| (*uid, *seen, *flagged))
        })
        .collect();
    if gone.is_empty() && updates.is_empty() {
        return Ok(expired > 0);
    }
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
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criteria_ask_for_every_word_before_the_window() {
        let before = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        let terms = Terms::new("Invoice, \"plumber\" 2019", vec![]);
        assert_eq!(terms.words, ["Invoice", "plumber", "2019"]);
        assert_eq!(
            criteria(before, &terms, true, false).unwrap(),
            "CHARSET UTF-8 BEFORE 4-Mar-2026 TEXT \"Invoice\" TEXT \"plumber\" TEXT \"2019\""
        );
        assert_eq!(
            criteria(before, &terms, false, false).unwrap(),
            "BEFORE 4-Mar-2026 TEXT \"Invoice\" TEXT \"plumber\" TEXT \"2019\""
        );
    }

    #[test]
    fn people_match_any_of_from_to_and_cc() {
        let terms = Terms::new("", vec!["sam@example.com".into(), "Jo \"J\" Doe".into()]);
        let c = criteria(0, &terms, false, false).unwrap();
        assert_eq!(
            c,
            "BEFORE 1-Jan-1970 (OR OR FROM \"sam@example.com\" OR TO \"sam@example.com\" CC \"sam@example.com\" \
             OR FROM \"Jo \\\"J\\\" Doe\" OR TO \"Jo \\\"J\\\" Doe\" CC \"Jo \\\"J\\\" Doe\")"
        );
    }

    #[test]
    fn non_ascii_words_go_as_literals_or_are_left_out_without_utf8() {
        let terms = Terms::new("facture café", vec![]);
        assert_eq!(
            criteria(0, &terms, true, true).unwrap(),
            "CHARSET UTF-8 BEFORE 1-Jan-1970 TEXT \"facture\" TEXT {5+}\r\ncafé"
        );
        assert_eq!(
            criteria(0, &terms, true, false).unwrap(),
            "CHARSET UTF-8 BEFORE 1-Jan-1970 TEXT \"facture\" TEXT \"café\""
        );
        assert_eq!(
            criteria(0, &terms, false, true).unwrap(),
            "BEFORE 1-Jan-1970 TEXT \"facture\""
        );
        assert!(terms_dropped(&terms));
        assert_eq!(criteria(0, &Terms::new("café", vec![]), false, true), None);
    }

    #[test]
    fn nothing_can_end_the_command_line() {
        let terms = Terms::new("", vec!["evil\r\nA1 DELETE INBOX".into()]);
        let c = criteria(0, &terms, false, false).unwrap();
        assert!(!c.contains('\r') && !c.contains('\n'), "{c}");
        assert!(Terms::new("a ! ?", vec![" ".into()]).is_empty());
    }
}
