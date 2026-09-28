//! The local copy of mail: threads, messages, search and sync positions. All functions
//! take a connection so callers can batch them in one `db.call`.

use mimi_protocol::{
    MailAddress, MailBox, MailCategory, MailMessage, MailThread, MailThreadDetail,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::parse::{self, Parsed};

const DAY_MS: i64 = 24 * 3600 * 1000;

/// A message as fetched from a mailbox, ready to store.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub connection_id: Uuid,
    pub mailbox: String,
    /// inbox | sent | archive
    pub folder: &'static str,
    pub uid: u32,
    pub parsed: Parsed,
    /// When the server received it; used when the Date header is missing or absurd.
    pub received: i64,
    pub seen: bool,
    pub flagged: bool,
    pub outgoing: bool,
    /// Which of the user's addresses it arrived at (`None` for sent mail).
    pub received_on: Option<String>,
}

/// Stores a message, threading it. Returns its thread, or `None` if it was already there.
pub fn insert(c: &Connection, m: &NewMessage) -> rusqlite::Result<Option<i64>> {
    let conn = m.connection_id.to_string();
    let exists: bool = c
        .query_row(
            "SELECT 1 FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2 AND uid = ?3",
            params![conn, m.mailbox, m.uid],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if exists {
        return Ok(None);
    }
    let p = &m.parsed;
    // Trust the Date header unless it's missing or more than a day in the future.
    let date = p
        .date
        .filter(|d| *d > 0 && *d < m.received + DAY_MS)
        .unwrap_or(m.received);
    let thread = match find_thread(c, &conn, p, date)? {
        Some(id) => {
            c.execute(
                "UPDATE mail_threads SET last_at = max(last_at, ?2), automated = automated AND ?3
                 WHERE id = ?1",
                params![id, date, p.automated],
            )?;
            id
        }
        None => {
            // A number never used before (see migration 0016).
            c.execute("INSERT INTO mail_thread_ids DEFAULT VALUES", [])?;
            let id = c.last_insert_rowid();
            c.execute("DELETE FROM mail_thread_ids WHERE id = ?1", [id])?;
            c.execute(
                "INSERT INTO mail_threads (id, connection_id, subject, last_at, automated)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, conn, display_subject(&p.subject), date, p.automated],
            )?;
            id
        }
    };
    c.execute(
        "INSERT INTO mail_messages (connection_id, mailbox, folder, uid, thread_id, message_id,
            in_reply_to, refs, from_name, from_email, to_json, cc_json, subject, date, body,
            snippet, attachments, seen, flagged, outgoing, automated, received_on, suspicious)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23)",
        params![
            conn,
            m.mailbox,
            m.folder,
            m.uid,
            thread,
            p.message_id,
            p.in_reply_to,
            p.refs.join(" "),
            p.from.name,
            p.from.email,
            serde_json::to_string(&p.to).expect("addresses serialize"),
            serde_json::to_string(&p.cc).expect("addresses serialize"),
            p.subject,
            date,
            p.body,
            parse::snippet(&p.body),
            serde_json::to_string(&p.attachments).expect("names serialize"),
            m.seen,
            m.flagged,
            m.outgoing,
            p.automated,
            m.received_on,
            p.suspicious && !m.outgoing,
        ],
    )?;
    Ok(Some(thread))
}

/// Which conversation a new message belongs to: the one it replies to, the one holding
/// another copy of it (or a reply to it), or, for "Re:" subjects, a recent conversation
/// with the same subject and a shared participant.
fn find_thread(c: &Connection, conn: &str, p: &Parsed, date: i64) -> rusqlite::Result<Option<i64>> {
    let mut keys: Vec<&str> = p.refs.iter().rev().take(20).map(String::as_str).collect();
    if let Some(r) = &p.in_reply_to {
        keys.insert(0, r);
    }
    for key in &keys {
        let found = c
            .query_row(
                "SELECT thread_id FROM mail_messages WHERE connection_id = ?1 AND message_id = ?2",
                params![conn, key],
                |r| r.get(0),
            )
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    if let Some(mid) = &p.message_id {
        let found = c
            .query_row(
                "SELECT thread_id FROM mail_messages WHERE connection_id = ?1
                   AND (message_id = ?2 OR in_reply_to = ?2 OR instr(' ' || refs || ' ', ' ' || ?2 || ' ') > 0)
                 LIMIT 1",
                params![conn, mid],
                |r| r.get(0),
            )
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    if !parse::is_reply_subject(&p.subject) {
        return Ok(None);
    }
    let wanted = parse::normalize_subject(&p.subject);
    let mut stmt = c.prepare(
        "SELECT id, subject FROM mail_threads WHERE connection_id = ?1 AND last_at BETWEEN ?2 AND ?3
         ORDER BY last_at DESC LIMIT 50",
    )?;
    let candidates: Vec<(i64, String)> = stmt
        .query_map(params![conn, date - 30 * DAY_MS, date + DAY_MS], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut people: Vec<&str> = vec![p.from.email.as_str()];
    people.extend(p.to.iter().chain(&p.cc).map(|a| a.email.as_str()));
    for (id, subject) in candidates {
        if parse::normalize_subject(&subject) != wanted {
            continue;
        }
        for email in &people {
            let shared: bool = c
                .query_row(
                    "SELECT 1 FROM mail_messages WHERE thread_id = ?1 AND (from_email = ?2
                       OR instr(to_json, '\"email\":\"' || ?2 || '\"') > 0
                       OR instr(cc_json, '\"email\":\"' || ?2 || '\"') > 0) LIMIT 1",
                    params![id, email],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if shared {
                return Ok(Some(id));
            }
        }
    }
    Ok(None)
}

/// A subject for display: without "Re:"-style prefixes, as written.
fn display_subject(subject: &str) -> String {
    let normalized_len = parse::normalize_subject(subject).chars().count();
    let chars: Vec<char> = subject.trim().chars().collect();
    let s: String = chars[chars.len().saturating_sub(normalized_len)..]
        .iter()
        .collect();
    if s.trim().is_empty() {
        "(no subject)".to_owned()
    } else {
        s.trim().to_owned()
    }
}

// --- Sync positions -------------------------------------------------------------------

pub fn sync_state(
    c: &Connection,
    conn: Uuid,
    mailbox: &str,
) -> rusqlite::Result<Option<(u32, u32)>> {
    c.query_row(
        "SELECT uidvalidity, last_uid FROM mail_sync WHERE connection_id = ?1 AND mailbox = ?2",
        params![conn.to_string(), mailbox],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

pub fn set_sync_state(
    c: &Connection,
    conn: Uuid,
    mailbox: &str,
    uidvalidity: u32,
    last_uid: u32,
    now: i64,
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO mail_sync (connection_id, mailbox, uidvalidity, last_uid, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (connection_id, mailbox) DO UPDATE SET uidvalidity = excluded.uidvalidity,
             last_uid = excluded.last_uid, synced_at = excluded.synced_at",
        params![conn.to_string(), mailbox, uidvalidity, last_uid, now],
    )?;
    Ok(())
}

/// Forgets a mailbox's messages (the server renumbered it).
pub fn reset_mailbox(c: &Connection, conn: Uuid, mailbox: &str) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2",
        params![conn.to_string(), mailbox],
    )?;
    c.execute(
        "DELETE FROM mail_sync WHERE connection_id = ?1 AND mailbox = ?2",
        params![conn.to_string(), mailbox],
    )?;
    drop_empty_threads(c)
}

/// Every stored message of a mailbox: uid, seen, flagged.
pub fn known(
    c: &Connection,
    conn: Uuid,
    mailbox: &str,
) -> rusqlite::Result<Vec<(u32, bool, bool)>> {
    let mut stmt = c.prepare(
        "SELECT uid, seen, flagged FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2",
    )?;
    stmt.query_map(params![conn.to_string(), mailbox], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?
    .collect()
}

pub fn set_flags(
    c: &Connection,
    conn: Uuid,
    mailbox: &str,
    uid: u32,
    seen: bool,
    flagged: bool,
) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE mail_messages SET seen = ?4, flagged = ?5
         WHERE connection_id = ?1 AND mailbox = ?2 AND uid = ?3",
        params![conn.to_string(), mailbox, uid, seen, flagged],
    )?;
    Ok(())
}

/// Removes messages that left the mailbox on the server (deleted or moved).
pub fn remove(c: &Connection, conn: Uuid, mailbox: &str, uids: &[u32]) -> rusqlite::Result<()> {
    for uid in uids {
        c.execute(
            "DELETE FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2 AND uid = ?3",
            params![conn.to_string(), mailbox, uid],
        )?;
    }
    drop_empty_threads(c)
}

/// Forgets a mailbox's messages older than `before`. Returns how many went.
pub fn prune(c: &Connection, conn: Uuid, mailbox: &str, before: i64) -> rusqlite::Result<usize> {
    let n = c.execute(
        "DELETE FROM mail_messages WHERE connection_id = ?1 AND mailbox = ?2 AND date < ?3",
        params![conn.to_string(), mailbox, before],
    )?;
    if n > 0 {
        drop_empty_threads(c)?;
    }
    Ok(n)
}

fn drop_empty_threads(c: &Connection) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM mail_threads WHERE NOT EXISTS
            (SELECT 1 FROM mail_messages m WHERE m.thread_id = mail_threads.id)",
        [],
    )?;
    c.execute(
        "UPDATE mail_threads SET last_at = (SELECT max(date) FROM mail_messages m WHERE m.thread_id = mail_threads.id)",
        [],
    )?;
    Ok(())
}

/// Removes all mail of a disconnected account.
pub fn forget_connection(c: &Connection, conn: Uuid) -> rusqlite::Result<()> {
    let id = conn.to_string();
    c.execute("DELETE FROM mail_messages WHERE connection_id = ?1", [&id])?;
    c.execute("DELETE FROM mail_threads WHERE connection_id = ?1", [&id])?;
    c.execute("DELETE FROM mail_sync WHERE connection_id = ?1", [&id])?;
    Ok(())
}

/// Works out where mail stored before `received_on` existed arrived, from its visible
/// recipients (the delivery headers weren't kept). Returns how many were filled in.
pub fn backfill_received_on(c: &Connection, conn: Uuid, account: &str) -> rusqlite::Result<usize> {
    let rows: Vec<(i64, String, String)> = c
        .prepare(
            "SELECT id, to_json, cc_json FROM mail_messages
             WHERE connection_id = ?1 AND received_on IS NULL AND NOT outgoing",
        )?
        .query_map([conn.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    for (id, to, cc) in &rows {
        let to: Vec<MailAddress> = serde_json::from_str(to).unwrap_or_default();
        let cc: Vec<MailAddress> = serde_json::from_str(cc).unwrap_or_default();
        c.execute(
            "UPDATE mail_messages SET received_on = ?2 WHERE id = ?1",
            params![id, parse::received_on(&[], &to, &cc, account)],
        )?;
    }
    Ok(rows.len())
}

/// Checks mail stored before the `suspicious` column existed (visible text only: hidden
/// HTML wasn't kept). Flagged conversations leave "Needs a reply". Returns how many
/// messages were checked.
pub fn backfill_suspicious(c: &Connection, conn: Uuid) -> rusqlite::Result<usize> {
    let rows: Vec<(i64, i64, String, bool)> = c
        .prepare(
            "SELECT id, thread_id, body, outgoing FROM mail_messages
             WHERE connection_id = ?1 AND suspicious IS NULL",
        )?
        .query_map([conn.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    for (id, thread, body, outgoing) in &rows {
        let flagged = !outgoing && super::suspicious::aimed_at_assistants(body);
        c.execute(
            "UPDATE mail_messages SET suspicious = ?2 WHERE id = ?1",
            params![id, flagged],
        )?;
        if flagged {
            set_sorted(c, *thread, MailCategory::Other, None)?;
            c.execute(
                "UPDATE mail_threads SET summary = NULL WHERE id = ?1",
                [thread],
            )?;
        }
    }
    Ok(rows.len())
}

/// Every address one account's mail arrived at.
pub fn account_addresses(c: &Connection, conn: Uuid) -> rusqlite::Result<Vec<String>> {
    c.prepare(
        "SELECT DISTINCT received_on FROM mail_messages
         WHERE connection_id = ?1 AND received_on IS NOT NULL",
    )?
    .query_map([conn.to_string()], |r| r.get(0))?
    .collect()
}

/// Puts mail recorded as arriving at an address that isn't plainly the user's (believed
/// from its headers before `parse::is_own_address` was the rule) back on the account's
/// address. Returns how many messages changed.
pub fn forget_foreign_received_on(
    c: &Connection,
    conn: Uuid,
    account: &str,
) -> rusqlite::Result<usize> {
    let mut changed = 0;
    for address in account_addresses(c, conn)? {
        if !parse::is_own_address(&address, account) {
            changed += c.execute(
                "UPDATE mail_messages SET received_on = ?3 WHERE connection_id = ?1 AND received_on = ?2",
                params![conn.to_string(), address, account.to_lowercase()],
            )?;
        }
    }
    Ok(changed)
}

/// An account's addresses that have mail in the inbox: (address, conversations, unread
/// conversations), most used first.
pub fn inbox_addresses(c: &Connection, conn: Uuid) -> rusqlite::Result<Vec<(String, u32, u32)>> {
    c.prepare(
        "SELECT received_on, count(DISTINCT thread_id),
                count(DISTINCT CASE WHEN NOT seen THEN thread_id END)
         FROM mail_messages WHERE connection_id = ?1 AND folder = 'inbox' AND received_on IS NOT NULL
         GROUP BY received_on ORDER BY 2 DESC, 1",
    )?
    .query_map([conn.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
    .collect()
}

// --- Reading --------------------------------------------------------------------------

/// What to list.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub view: Option<MailBox>,
    /// Words to search for (all must match).
    pub search: Option<String>,
    /// Only conversations with one of these addresses.
    pub with: Vec<String>,
    /// Only conversations with a message from someone whose name or address matches.
    pub from_text: Option<String>,
    /// Only conversations whose last message is older than this (paging).
    pub before: Option<i64>,
    /// Only conversations newer than this.
    pub since: Option<i64>,
    pub unread_only: bool,
    /// Only conversations in this smart folder (the view is ignored).
    pub folder: Option<i64>,
    /// Only this account's conversations, and, within it, only those that arrived at
    /// (or were sent from) one address.
    pub scope: Scope,
    pub limit: u32,
}

/// Part of the user's mail: one account, or one address.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub account: Option<Uuid>,
    pub address: Option<String>,
}

impl Scope {
    /// SQL conditions on a thread `t`, with their arguments.
    fn filters(&self) -> (Vec<String>, Vec<rusqlite::types::Value>) {
        let mut filters = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(account) = self.account {
            filters.push("t.connection_id = ?".to_owned());
            args.push(account.to_string().into());
        }
        if let Some(address) = &self.address {
            filters.push(
                "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id
                   AND (x.received_on = ? OR (x.outgoing AND x.from_email = ?)))"
                    .to_owned(),
            );
            args.push(address.to_lowercase().into());
            args.push(address.to_lowercase().into());
        }
        (filters, args)
    }
}

/// Turns what the user typed into an FTS5 query: every word must match (as a prefix).
pub fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '@' && c != '.')
        .map(|w| w.trim_matches('.').replace('"', ""))
        .filter(|w| w.chars().count() >= 2)
        .take(8)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

pub fn threads(c: &Connection, q: &Query, me: &[String]) -> rusqlite::Result<Vec<MailThread>> {
    let in_folder = |f: &str| {
        format!(
            "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND x.folder = '{f}')"
        )
    };
    let (mut filters, mut args) = q.scope.filters();
    let view = if let Some(folder) = q.folder {
        filters.push(
            "EXISTS (SELECT 1 FROM mail_folder_threads ft WHERE ft.thread_id = t.id
               AND ft.folder_id = ? AND ft.member = 1)"
                .to_owned(),
        );
        args.push(folder.into());
        None
    } else {
        q.view
    };
    match view {
        Some(MailBox::Inbox) => filters.push(in_folder("inbox")),
        Some(MailBox::Sent) => filters.push(in_folder("sent")),
        Some(MailBox::Archive) => {
            filters.push(in_folder("archive"));
            filters.push(format!("NOT {}", in_folder("inbox")));
        }
        Some(MailBox::NeedsReply) => {
            filters.push(in_folder("inbox"));
            filters.push("t.category = 'needs_reply'".to_owned());
        }
        Some(MailBox::Important) => {
            filters.push(in_folder("inbox"));
            filters.push("t.category = 'important'".to_owned());
        }
        Some(MailBox::Other) => {
            filters.push(in_folder("inbox"));
            filters
                .push("(t.category = 'other' OR (t.category IS NULL AND t.automated))".to_owned());
        }
        None => {}
    }
    if let Some(fts) = q.search.as_deref().and_then(fts_query) {
        filters.push(
            "t.id IN (SELECT m2.thread_id FROM mail_fts JOIN mail_messages m2 ON m2.id = mail_fts.rowid
                       WHERE mail_fts MATCH ?)"
                .to_owned(),
        );
        args.push(fts.into());
    }
    if let Some(fts) = q.from_text.as_deref().and_then(fts_query) {
        filters.push(
            "t.id IN (SELECT m2.thread_id FROM mail_fts JOIN mail_messages m2 ON m2.id = mail_fts.rowid
                       WHERE mail_fts MATCH ?)"
                .to_owned(),
        );
        args.push(format!("{{from_name from_email}} : ({fts})").into());
    }
    if !q.with.is_empty() {
        let mut ors = Vec::new();
        for email in &q.with {
            ors.push(
                "(x.from_email = ? OR instr(x.to_json, '\"email\":\"' || ? || '\"') > 0
                  OR instr(x.cc_json, '\"email\":\"' || ? || '\"') > 0)"
                    .to_owned(),
            );
            for _ in 0..3 {
                args.push(email.to_lowercase().into());
            }
        }
        filters.push(format!(
            "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND ({}))",
            ors.join(" OR ")
        ));
    }
    if let Some(before) = q.before {
        filters.push("t.last_at < ?".to_owned());
        args.push(before.into());
    }
    if let Some(since) = q.since {
        filters.push("t.last_at >= ?".to_owned());
        args.push(since.into());
    }
    if q.unread_only {
        filters.push(
            "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND NOT x.seen AND NOT x.outgoing)"
                .to_owned(),
        );
    }
    let sql = format!(
        "SELECT t.id FROM mail_threads t {} ORDER BY t.last_at DESC LIMIT {}",
        if filters.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", filters.join(" AND "))
        },
        q.limit.clamp(1, 200)
    );
    let ids: Vec<i64> = c
        .prepare(&sql)?
        .query_map(rusqlite::params_from_iter(args), |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    ids.into_iter()
        .filter_map(|id| thread(c, id, me).transpose())
        .collect()
}

/// One conversation's summary line.
pub fn thread(c: &Connection, id: i64, me: &[String]) -> rusqlite::Result<Option<MailThread>> {
    let head = c
        .query_row(
            "SELECT connection_id, subject, last_at, category, summary, automated FROM mail_threads WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((conn, subject, last_at, category, summary, automated)) = head else {
        return Ok(None);
    };
    let messages = messages(c, id, me)?;
    let Some(last) = messages.last() else {
        return Ok(None);
    };
    let folders = super::folders::of_thread(c, id)?;
    let suspicious: bool = c.query_row(
        "SELECT coalesce(max(suspicious), 0) FROM mail_messages WHERE thread_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let received_on = c
        .query_row(
            "SELECT received_on FROM mail_messages WHERE thread_id = ?1 AND received_on IS NOT NULL
             ORDER BY date DESC LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(Some(MailThread {
        id,
        connection_id: conn.parse().unwrap_or_default(),
        received_on: received_on.clone(),
        suspicious,
        folders,
        subject,
        // The alias it arrived at is the user's, not someone they're writing with.
        participants: participants(&messages, &[me, received_on.as_slice()].concat()),
        last_at,
        message_count: messages.len() as u32,
        unread: messages.iter().any(|m| !m.seen && !m.from_me),
        flagged: flagged(c, id)?,
        category: category.as_deref().and_then(category_from),
        summary,
        snippet: parse::snippet(&last.body),
        automated,
        last_from_me: last.from_me,
    }))
}

fn flagged(c: &Connection, thread: i64) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT coalesce(max(flagged), 0) FROM mail_messages WHERE thread_id = ?1",
        [thread],
        |r| r.get(0),
    )
}

pub fn detail(
    c: &Connection,
    id: i64,
    me: &[String],
) -> rusqlite::Result<Option<MailThreadDetail>> {
    let Some(thread) = thread(c, id, me)? else {
        return Ok(None);
    };
    Ok(Some(MailThreadDetail {
        messages: messages(c, id, me)?,
        thread,
    }))
}

/// A thread's messages, oldest first, one per Message-ID (a sent reply can be stored
/// in both Sent and Inbox).
pub fn messages(c: &Connection, thread: i64, me: &[String]) -> rusqlite::Result<Vec<MailMessage>> {
    let mut stmt = c.prepare(
        "SELECT id, from_name, from_email, to_json, cc_json, date, body, seen, outgoing, attachments,
                message_id, coalesce(suspicious, 0)
         FROM mail_messages WHERE thread_id = ?1 ORDER BY date, id",
    )?;
    let rows = stmt.query_map([thread], |r| {
        let from_email: String = r.get(2)?;
        let outgoing: bool = r.get(8)?;
        Ok((
            r.get::<_, Option<String>>(10)?,
            MailMessage {
                id: r.get(0)?,
                from: MailAddress {
                    name: r.get(1)?,
                    email: from_email.clone(),
                },
                to: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                cc: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                date: r.get(5)?,
                body: r.get(6)?,
                seen: r.get(7)?,
                from_me: outgoing || me.iter().any(|e| e.eq_ignore_ascii_case(&from_email)),
                attachments: serde_json::from_str(&r.get::<_, String>(9)?).unwrap_or_default(),
                suspicious: r.get(11)?,
            },
        ))
    })?;
    let mut seen_ids = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let (mid, msg) = row?;
        if let Some(mid) = mid
            && !seen_ids.insert(mid)
        {
            continue;
        }
        out.push(msg);
    }
    Ok(out)
}

/// One message, with its conversation and subject.
pub fn message(
    c: &Connection,
    id: i64,
    me: &[String],
) -> rusqlite::Result<Option<(i64, String, MailMessage)>> {
    let Some((thread, subject)) = c
        .query_row(
            "SELECT thread_id, subject FROM mail_messages WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let found = messages(c, thread, me)?.into_iter().find(|m| m.id == id);
    Ok(found.map(|m| (thread, subject, m)))
}

/// Messages matching an FTS query, newest first: (message, conversation).
pub fn search_messages(c: &Connection, fts: &str, limit: u32) -> rusqlite::Result<Vec<(i64, i64)>> {
    c.prepare(
        "SELECT m.id, m.thread_id FROM mail_fts JOIN mail_messages m ON m.id = mail_fts.rowid
         WHERE mail_fts MATCH ?1 ORDER BY m.date DESC LIMIT ?2",
    )?
    .query_map(params![fts, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
    .collect()
}

fn participants(messages: &[MailMessage], me: &[String]) -> Vec<MailAddress> {
    let mut out: Vec<MailAddress> = Vec::new();
    for m in messages.iter().rev() {
        for a in std::iter::once(&m.from).chain(&m.to).chain(&m.cc) {
            let mine = me.iter().any(|e| e.eq_ignore_ascii_case(&a.email));
            if mine || a.email.is_empty() {
                continue;
            }
            match out.iter_mut().find(|o| o.email == a.email) {
                // A bare address in one message, a name in another: keep the name.
                Some(o) if o.name.is_none() => o.name = a.name.clone(),
                Some(_) => {}
                None => out.push(a.clone()),
            }
        }
    }
    out.truncate(6);
    out
}

pub fn category_from(s: &str) -> Option<MailCategory> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

pub fn category_str(c: MailCategory) -> &'static str {
    match c {
        MailCategory::NeedsReply => "needs_reply",
        MailCategory::Important => "important",
        MailCategory::Other => "other",
    }
}

/// Counts for the sidebar: needs a reply, important, unread (all in the inbox).
pub fn counts(c: &Connection, scope: &Scope) -> rusqlite::Result<(u32, u32, u32)> {
    let (filters, args) = scope.filters();
    let mut inbox = vec![
        "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND x.folder = 'inbox')"
            .to_owned(),
    ];
    inbox.extend(filters);
    let inbox = inbox.join(" AND ");
    let count = |extra: &str| -> rusqlite::Result<u32> {
        c.query_row(
            &format!("SELECT count(*) FROM mail_threads t WHERE {inbox} AND {extra}"),
            rusqlite::params_from_iter(args.iter()),
            |r| r.get(0),
        )
    };
    Ok((
        count("t.category = 'needs_reply'")?,
        count("t.category = 'important'")?,
        count(
            "EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND NOT x.seen AND NOT x.outgoing)",
        )?,
    ))
}

/// Where one message lives on the server: (connection, mailbox, uid).
pub fn location(c: &Connection, message: i64) -> rusqlite::Result<Option<(Uuid, String, u32)>> {
    c.query_row(
        "SELECT connection_id, mailbox, uid FROM mail_messages WHERE id = ?1",
        [message],
        |r| {
            Ok((
                r.get::<_, String>(0)?.parse().unwrap_or_default(),
                r.get(1)?,
                r.get(2)?,
            ))
        },
    )
    .optional()
}

/// Where a thread's messages live on the server: (connection, mailbox, uid).
pub fn locations(
    c: &Connection,
    thread: i64,
) -> rusqlite::Result<Vec<(Uuid, String, String, u32)>> {
    let mut stmt = c.prepare(
        "SELECT connection_id, mailbox, folder, uid FROM mail_messages WHERE thread_id = ?1",
    )?;
    stmt.query_map([thread], |r| {
        Ok((
            r.get::<_, String>(0)?.parse().unwrap_or_default(),
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
        ))
    })?
    .collect()
}

pub fn set_thread_seen(c: &Connection, thread: i64, seen: bool) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE mail_messages SET seen = ?2 WHERE thread_id = ?1 AND NOT outgoing",
        params![thread, seen],
    )?;
    Ok(())
}

/// Forgets a whole conversation locally (it was deleted on the server).
pub fn forget_thread(c: &Connection, thread: i64) -> rusqlite::Result<()> {
    c.execute("DELETE FROM mail_messages WHERE thread_id = ?1", [thread])?;
    drop_empty_threads(c)
}

/// Takes a thread out of the inbox locally (the server move happens separately).
pub fn archive_locally(c: &Connection, thread: i64) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM mail_messages WHERE thread_id = ?1 AND folder = 'inbox'",
        [thread],
    )?;
    drop_empty_threads(c)
}

pub fn set_sorted(
    c: &Connection,
    thread: i64,
    category: MailCategory,
    summary: Option<&str>,
) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE mail_threads SET category = ?2, summary = coalesce(?3, summary), sorted_at = last_at WHERE id = ?1",
        params![thread, category_str(category), summary],
    )?;
    Ok(())
}

/// Inbox threads that arrived or changed since they were last sorted, newest first.
pub fn unsorted(c: &Connection, since: i64, limit: u32) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = c.prepare(
        "SELECT t.id FROM mail_threads t
         WHERE (t.sorted_at IS NULL OR t.sorted_at < t.last_at) AND t.last_at >= ?1
           AND EXISTS (SELECT 1 FROM mail_messages x WHERE x.thread_id = t.id AND x.folder = 'inbox')
         ORDER BY t.last_at DESC LIMIT ?2",
    )?;
    stmt.query_map(params![since, limit], |r| r.get(0))?
        .collect()
}

/// People the user corresponds with: everyone they wrote to, and people (not
/// automated senders) who wrote to them at least twice. (email, best name, count).
pub fn correspondents(
    c: &Connection,
    conn: Uuid,
    me: &[String],
) -> rusqlite::Result<Vec<(String, Option<String>, u32)>> {
    use std::collections::HashMap;
    let mut tally: HashMap<String, (HashMap<String, u32>, u32, bool)> = HashMap::new();
    let mut stmt = c.prepare(
        "SELECT from_name, from_email, to_json, cc_json, outgoing, automated FROM mail_messages
         WHERE connection_id = ?1",
    )?;
    let rows = stmt.query_map([conn.to_string()], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, bool>(5)?,
        ))
    })?;
    let is_me = |e: &str| me.iter().any(|m| m.eq_ignore_ascii_case(e));
    for row in rows {
        let (name, from, to, cc, outgoing, automated) = row?;
        let mut add = |email: &str, name: Option<String>, wrote_to: bool| {
            if email.is_empty() || is_me(email) {
                return;
            }
            let entry = tally.entry(email.to_owned()).or_default();
            if let Some(n) = name {
                *entry.0.entry(n).or_default() += 1;
            }
            entry.1 += 1;
            entry.2 |= wrote_to;
        };
        if outgoing || is_me(&from) {
            let to: Vec<MailAddress> = serde_json::from_str(&to).unwrap_or_default();
            let cc: Vec<MailAddress> = serde_json::from_str(&cc).unwrap_or_default();
            for a in to.into_iter().chain(cc) {
                add(&a.email, a.name, true);
            }
        } else if !automated {
            add(&from, name, false);
        }
    }
    let mut out: Vec<(String, Option<String>, u32)> = tally
        .into_iter()
        .filter(|(_, (_, n, wrote_to))| *wrote_to || *n >= 2)
        .map(|(email, (names, n, _))| {
            let name = names.into_iter().max_by_key(|(_, k)| *k).map(|(n, _)| n);
            (email, name, n)
        })
        .collect();
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    out.truncate(500);
    Ok(out)
}
