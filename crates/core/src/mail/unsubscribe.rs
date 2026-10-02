//! Unsubscribing from newsletters and mailing lists, only ever on the user's click.
//!
//! A list says how to leave it in its `List-Unsubscribe` header (RFC 2369): web
//! addresses and `mailto:` addresses, and with `List-Unsubscribe-Post:
//! List-Unsubscribe=One-Click` (RFC 8058) a web address that takes a single POST. Best
//! first:
//!
//! - **one-click**: the daemon POSTs `List-Unsubscribe=One-Click` to the https address;
//! - **email**: it sends the short email the list asks for, from the address the mail
//!   arrived at, through the normal send path (filed in Sent);
//! - **website**: the app opens the page in the user's browser.
//!
//! Everything in those headers was written by the sender. The web address is fetched
//! only over https, only from the internet (names are resolved and checked, and the
//! connection goes to the address that was checked; see `images::PublicOnly`), with no
//! cookies, referrer or proxy, nothing from the user's data, a short timeout and at most
//! a few redirects, each checked the same way. The email's subject and body are cleaned
//! and capped, and it goes to that one address only.
//!
//! There is no assistant tool for this: only `POST /v1/mail/threads/{id}/unsubscribe`,
//! the user's click, unsubscribes.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use mimi_protocol::{MailDraft, MailUnsubscribe, MailUnsubscribeMethod, MailUnsubscribed};
use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use super::images::{PublicOnly, is_public};
use crate::AppState;

/// Longest header kept.
const MAX_HEADER: usize = 2048;
/// Most addresses read from one header.
const MAX_URIS: usize = 8;
const MAX_SUBJECT: usize = 200;
const MAX_BODY: usize = 2000;
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 3;
/// Most conversations "Archive all" moves at once.
const MAX_ARCHIVE: u32 = 500;

/// A message's list headers, as stored with it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListHeaders {
    /// `List-Unsubscribe`, unfolded.
    pub unsubscribe: Option<String>,
    /// `List-Unsubscribe-Post`, unfolded.
    pub post: Option<String>,
    /// `List-Id`'s bare id, lowercased ("news.example.com").
    pub id: Option<String>,
}

impl ListHeaders {
    pub fn read(msg: &mail_parser::Message) -> Self {
        let header = |name: &str| msg.header_raw(name).map(unfold).filter(|v| !v.is_empty());
        Self {
            unsubscribe: header("List-Unsubscribe"),
            post: header("List-Unsubscribe-Post"),
            id: header("List-Id").and_then(|v| list_id(&v)),
        }
    }

    /// The key a list is remembered by: its List-Id, else the sender's address.
    fn key(&self, from: &str) -> String {
        match &self.id {
            Some(id) => format!("id:{id}"),
            None => format!("from:{}", from.to_lowercase()),
        }
    }
}

/// A header's value on one line, capped.
fn unfold(raw: &str) -> String {
    let flat: String = raw.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    let flat = flat.trim();
    match flat.char_indices().nth(MAX_HEADER) {
        Some((i, _)) => flat[..i].to_owned(),
        None => flat.to_owned(),
    }
}

/// "Weekly News <news.example.com>" → "news.example.com".
fn list_id(v: &str) -> Option<String> {
    let id = match (v.rfind('<'), v.rfind('>')) {
        (Some(a), Some(b)) if a < b => &v[a + 1..b],
        _ => v,
    };
    let id = id.trim().to_lowercase();
    (!id.is_empty() && id.len() <= 255 && !id.chars().any(|c| c.is_whitespace())).then_some(id)
}

// --- Choosing a way -------------------------------------------------------------------

/// How to unsubscribe, best first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Way {
    OneClick(Url),
    Email {
        to: String,
        subject: String,
        body: String,
    },
    Website(Url),
}

impl Way {
    pub fn method(&self) -> MailUnsubscribeMethod {
        match self {
            Way::OneClick(_) => MailUnsubscribeMethod::OneClick,
            Way::Email { .. } => MailUnsubscribeMethod::Email,
            Way::Website(_) => MailUnsubscribeMethod::Website,
        }
    }

    /// Who is asked, without "www.".
    pub fn domain(&self) -> String {
        let host = match self {
            Way::OneClick(url) | Way::Website(url) => url.host_str().unwrap_or_default(),
            Way::Email { to, .. } => to.rsplit('@').next().unwrap_or_default(),
        };
        let host = host.to_lowercase();
        host.strip_prefix("www.").map(str::to_owned).unwrap_or(host)
    }
}

/// The best way a list's headers offer, if any.
pub fn choose(unsubscribe: &str, post: Option<&str>) -> Option<Way> {
    choose_in(unsubscribe, post, false)
}

/// `local`: tests only, whose list server is on this computer, over http.
fn choose_in(unsubscribe: &str, post: Option<&str>, local: bool) -> Option<Way> {
    let uris = uris(unsubscribe);
    let one_click = post.is_some_and(|p| {
        let p: String = p.chars().filter(|c| !c.is_whitespace()).collect();
        p.eq_ignore_ascii_case("List-Unsubscribe=One-Click")
    });
    let web: Vec<Url> = uris.iter().filter_map(|u| web_url(u, local)).collect();
    if one_click && let Some(url) = web.iter().find(|u| u.scheme() == "https" || local) {
        return Some(Way::OneClick(url.clone()));
    }
    if let Some(way) = uris.iter().find_map(|u| mailto(u)) {
        return Some(way);
    }
    // https first, for the browser too.
    let mut web = web;
    web.sort_by_key(|u| u.scheme() != "https");
    web.into_iter().next().map(Way::Website)
}

/// The addresses in a `List-Unsubscribe` header: each between angle brackets (spaces
/// inside are dropped, as RFC 2369 asks), or comma-separated when a sender left the
/// brackets out.
pub fn uris(header: &str) -> Vec<String> {
    let squash = |s: &str| -> String { s.chars().filter(|c| !c.is_whitespace()).collect() };
    let found: Vec<String> = if header.contains('<') {
        header
            .split('<')
            .skip(1)
            .filter_map(|part| part.split_once('>').map(|(uri, _)| squash(uri)))
            .collect()
    } else {
        header.split(',').map(squash).collect()
    };
    found
        .into_iter()
        .filter(|u| !u.is_empty())
        .take(MAX_URIS)
        .collect()
}

/// A web address that may be offered: http(s), no password, a host that isn't written
/// as an address on this computer or the local network.
fn web_url(raw: &str, local: bool) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    allowed_url(&url, &["https", "http"], local).then_some(url)
}

fn allowed_url(url: &Url, schemes: &[&str], local: bool) -> bool {
    if !schemes.contains(&url.scheme()) || !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let ip_ok = |ip: IpAddr| is_public(ip) || (local && ip.is_loopback());
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            local || !(d == "localhost" || d.ends_with(".localhost"))
        }
        Some(url::Host::Ipv4(ip)) => ip_ok(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => ip_ok(IpAddr::V6(ip)),
        None => false,
    }
}

/// `mailto:leave@example.com?subject=…&body=…`: one address, with the subject and body
/// cleaned. Other fields (cc, bcc, more recipients) are ignored.
fn mailto(uri: &str) -> Option<Way> {
    let rest = uri
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("mailto:"))
        .map(|_| &uri[7..])?;
    let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
    let to = decode(to).split(',').next()?.trim().to_owned();
    let to = to.parse::<lettre::Address>().ok()?.to_string();
    let (mut subject, mut body) = (String::new(), String::new());
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match decode(k).to_ascii_lowercase().as_str() {
            "subject" => subject = decode(v),
            "body" => body = decode(v),
            _ => {}
        }
    }
    let subject = clean_line(&subject, MAX_SUBJECT);
    let body = clean_body(&body, MAX_BODY);
    Some(Way::Email {
        to,
        subject: if subject.is_empty() {
            "Unsubscribe".to_owned()
        } else {
            subject
        },
        body: if body.is_empty() {
            "Please unsubscribe me from this list.".to_owned()
        } else {
            body
        },
    })
}

/// Percent-decoding (a `+` stays a `+` in mailto addresses).
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One line: control characters and runs of spaces become one space, capped.
fn clean_line(s: &str, max: usize) -> String {
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    words.join(" ").chars().take(max).collect()
}

/// Text: newlines kept, other control characters dropped, capped.
fn clean_body(s: &str, max: usize) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

// --- One-click ------------------------------------------------------------------------

/// Sends one-click requests.
#[derive(Default)]
pub struct Poster {
    /// Tests only: their list server runs on this computer, over plain http. Only
    /// `allow_local_for_tests` sets it.
    local: std::sync::atomic::AtomicBool,
}

impl Poster {
    #[cfg(test)]
    pub fn allow_local_for_tests(&self) {
        self.local.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn local(&self) -> bool {
        self.local.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn schemes(local: bool) -> &'static [&'static str] {
        if local {
            &["https", "http"]
        } else {
            &["https"]
        }
    }

    /// POSTs `List-Unsubscribe=One-Click` to a list's address (RFC 8058).
    pub async fn one_click(&self, url: &Url) -> Result<(), String> {
        let local = self.local();
        let domain = Way::OneClick(url.clone()).domain();
        if !allowed_url(url, Self::schemes(local), local) {
            return Err(refused(&domain));
        }
        let client = reqwest::Client::builder()
            // A proxy would resolve names itself, past the checks.
            .no_proxy()
            .referer(false)
            .user_agent("Mozilla/5.0")
            .dns_resolver(PublicOnly {
                allow_loopback: local,
            })
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if allowed_url(attempt.url(), Self::schemes(local), local) {
                    attempt.follow()
                } else {
                    attempt.error("redirected to a refused address")
                }
            }))
            .connect_timeout(Duration::from_secs(5))
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        let res = client
            .post(url.clone())
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body("List-Unsubscribe=One-Click")
            .send()
            .await
            .map_err(|e| {
                // Never the address itself: it identifies the user.
                tracing::debug!("a one-click unsubscribe failed: {e}");
                if e.is_redirect() {
                    refused(&domain)
                } else {
                    format!(
                        "Couldn't reach {domain}. Check your internet connection and try again."
                    )
                }
            })?;
        if res.status().is_success() {
            Ok(())
        } else {
            tracing::debug!(status = %res.status(), "a one-click unsubscribe was refused");
            Err(format!(
                "{domain} didn't accept the request. Try again later, or look for an unsubscribe link at the bottom of the email."
            ))
        }
    }
}

fn refused(domain: &str) -> String {
    format!(
        "The unsubscribe address from {domain} points somewhere Mimi doesn't go (your own network). Look for an unsubscribe link at the bottom of the email instead."
    )
}

// --- Storage --------------------------------------------------------------------------

/// Keeps a stored message's list headers ('' when it has none).
pub fn store_headers(c: &Connection, message: i64, list: &ListHeaders) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE mail_messages SET list_unsubscribe = ?2, list_unsubscribe_post = ?3, list_id = ?4
         WHERE id = ?1",
        params![
            message,
            list.unsubscribe.as_deref().unwrap_or(""),
            list.post.as_deref().unwrap_or(""),
            list.id.as_deref().unwrap_or(""),
        ],
    )?;
    Ok(())
}

/// A conversation's latest message from someone else, with its list headers.
struct Latest {
    message: i64,
    connection_id: Uuid,
    sender: String,
    from_email: String,
    received_on: Option<String>,
    automated: bool,
    /// `None` when the headers weren't read yet (mail stored before they were kept).
    list: Option<ListHeaders>,
}

fn latest(c: &Connection, thread: i64) -> rusqlite::Result<Option<Latest>> {
    c.query_row(
        "SELECT id, connection_id, from_name, from_email, received_on, automated,
                list_unsubscribe, list_unsubscribe_post, list_id
         FROM mail_messages WHERE thread_id = ?1 AND NOT outgoing AND folder <> 'sent'
         ORDER BY date DESC, id DESC LIMIT 1",
        [thread],
        |r| {
            let from_email: String = r.get(3)?;
            let name: Option<String> = r.get(2)?;
            let unsubscribe: Option<String> = r.get(6)?;
            let some = |v: Option<String>| v.filter(|v| !v.is_empty());
            Ok(Latest {
                message: r.get(0)?,
                connection_id: r.get::<_, String>(1)?.parse().unwrap_or_default(),
                sender: some(name.map(|n| n.trim().to_owned()))
                    .unwrap_or_else(|| from_email.clone()),
                received_on: r.get(4)?,
                automated: r.get(5)?,
                list: unsubscribe.map(|u| ListHeaders {
                    unsubscribe: some(Some(u)),
                    post: some(r.get(7).ok().flatten()),
                    id: some(r.get(8).ok().flatten()),
                }),
                from_email,
            })
        },
    )
    .optional()
}

/// Matches the messages of a list (`?2`: its id or NULL, `?3`: the sender's address).
/// Mail stored before List-Id was kept joins an id's list by its automatic sender.
const SAME_LIST: &str = "((?2 IS NOT NULL AND (list_id = ?2
        OR (list_id IS NULL AND automated AND lower(from_email) = ?3)))
    OR (?2 IS NULL AND coalesce(list_id, '') = '' AND lower(from_email) = ?3))";

fn inbox_threads(
    c: &Connection,
    conn: Uuid,
    list: &ListHeaders,
    from: &str,
    limit: u32,
) -> rusqlite::Result<Vec<i64>> {
    c.prepare(&format!(
        "SELECT DISTINCT thread_id FROM mail_messages
         WHERE connection_id = ?1 AND folder = 'inbox' AND NOT outgoing AND {SAME_LIST}
         LIMIT ?4"
    ))?
    .query_map(
        params![conn.to_string(), list.id, from.to_lowercase(), limit],
        |r| r.get(0),
    )?
    .collect()
}

fn done(c: &Connection, conn: Uuid, key: &str) -> rusqlite::Result<Option<MailUnsubscribed>> {
    c.query_row(
        "SELECT method, at FROM mail_unsubscribed WHERE connection_id = ?1 AND list = ?2",
        params![conn.to_string(), key],
        |r| {
            let method: String = r.get(0)?;
            Ok((method, r.get::<_, i64>(1)?))
        },
    )
    .optional()
    .map(|found| {
        found.and_then(|(method, at)| {
            serde_json::from_value(serde_json::Value::String(method))
                .ok()
                .map(|method| MailUnsubscribed { method, at })
        })
    })
}

fn remember(
    c: &Connection,
    conn: Uuid,
    key: &str,
    method: MailUnsubscribeMethod,
) -> rusqlite::Result<()> {
    c.execute(
        "INSERT OR REPLACE INTO mail_unsubscribed (connection_id, list, method, at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            conn.to_string(),
            key,
            crate::db::enum_str(method),
            crate::now_ms()
        ],
    )?;
    Ok(())
}

// --- What the panel uses --------------------------------------------------------------

/// A conversation's list: its latest message and how to leave it.
struct Found {
    latest: Latest,
    list: ListHeaders,
    way: Way,
}

async fn find(state: &AppState, thread: i64) -> Result<Option<Found>, String> {
    let Some(mut latest) = state
        .db
        .call(move |c| latest(c, thread))
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    // Mail stored before the headers were kept: read them from the server, once, for
    // automatic mail only (a person's email doesn't come from a list).
    if latest.list.is_none() && latest.automated {
        let message = latest.message;
        match super::source(state, message).await {
            Ok(raw) => {
                let list = mail_parser::MessageParser::default()
                    .parse(&raw)
                    .map(|m| ListHeaders::read(&m))
                    .unwrap_or_default();
                let kept = list.clone();
                state
                    .db
                    .call(move |c| store_headers(c, message, &kept))
                    .await
                    .map_err(|e| e.to_string())?;
                latest.list = Some(list);
            }
            Err(e) => tracing::debug!("couldn't read an older email's list headers: {e}"),
        }
    }
    let Some(list) = latest.list.clone() else {
        return Ok(None);
    };
    let Some(way) = list
        .unsubscribe
        .as_deref()
        .and_then(|u| choose_in(u, list.post.as_deref(), state.mail.unsubscribe.local()))
    else {
        return Ok(None);
    };
    Ok(Some(Found { latest, list, way }))
}

async fn describe(state: &AppState, found: &Found) -> Result<MailUnsubscribe, String> {
    let (conn, key, list, from) = (
        found.latest.connection_id,
        found.list.key(&found.latest.from_email),
        found.list.clone(),
        found.latest.from_email.clone(),
    );
    let (done, in_inbox) = state
        .db
        .call(move |c| {
            Ok((
                done(c, conn, &key)?,
                inbox_threads(c, conn, &list, &from, MAX_ARCHIVE)?.len() as u32,
            ))
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(MailUnsubscribe {
        method: found.way.method(),
        domain: found.way.domain(),
        sender: found.latest.sender.clone(),
        url: match &found.way {
            Way::Website(url) => Some(url.to_string()),
            _ => None,
        },
        done,
        in_inbox,
    })
}

/// How to unsubscribe from a conversation's list, if its latest message says.
pub async fn offer(state: &AppState, thread: i64) -> Result<Option<MailUnsubscribe>, String> {
    match find(state, thread).await? {
        Some(found) => describe(state, &found).await.map(Some),
        None => Ok(None),
    }
}

/// Unsubscribes from a conversation's list: the user's click. A website is only
/// remembered here; the app opens it.
pub async fn unsubscribe(state: &Arc<AppState>, thread: i64) -> Result<MailUnsubscribe, String> {
    let found = find(state, thread)
        .await?
        .ok_or("This email doesn't say how to unsubscribe.")?;
    match &found.way {
        Way::OneClick(url) => state.mail.unsubscribe.one_click(url).await?,
        Way::Email { to, subject, body } => {
            super::send(
                state,
                MailDraft {
                    connection_id: Some(found.latest.connection_id),
                    from: found.latest.received_on.clone(),
                    to: vec![to.clone()],
                    subject: subject.clone(),
                    body: body.clone(),
                    ..Default::default()
                },
            )
            .await?
        }
        Way::Website(_) => {}
    }
    tracing::info!(method = ?found.way.method(), "unsubscribed from a mailing list");
    let (conn, key, method) = (
        found.latest.connection_id,
        found.list.key(&found.latest.from_email),
        found.way.method(),
    );
    state
        .db
        .call(move |c| remember(c, conn, &key, method))
        .await
        .map_err(|e| e.to_string())?;
    super::changed(state);
    describe(state, &found).await
}

/// Archives every Inbox conversation from a conversation's list. Returns how many.
pub async fn archive_list(state: &Arc<AppState>, thread: i64) -> Result<u32, String> {
    let found = find(state, thread)
        .await?
        .ok_or("This email isn't from a mailing list.")?;
    let (conn, list, from) = (
        found.latest.connection_id,
        found.list.clone(),
        found.latest.from_email.clone(),
    );
    let (threads, locations) = state
        .db
        .call(move |c| {
            let threads = inbox_threads(c, conn, &list, &from, MAX_ARCHIVE)?;
            let mut locations = Vec::new();
            for t in &threads {
                locations.extend(
                    super::store::locations(c, *t)?
                        .into_iter()
                        .filter(|(_, _, folder, _)| folder == "inbox"),
                );
            }
            Ok((threads, locations))
        })
        .await
        .map_err(|e| e.to_string())?;
    if threads.is_empty() {
        return Ok(0);
    }
    super::sync::archive_on_server(state, &locations).await?;
    let archived = threads.clone();
    state
        .db
        .call(move |c| {
            for t in &archived {
                super::store::archive_locally(c, *t)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
    let mut accounts: Vec<Uuid> = locations.iter().map(|l| l.0).collect();
    accounts.sort();
    accounts.dedup();
    for a in accounts {
        state.mail.poke(a);
    }
    super::changed(state);
    Ok(threads.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(to: &str, subject: &str, body: &str) -> Way {
        Way::Email {
            to: to.to_owned(),
            subject: subject.to_owned(),
            body: body.to_owned(),
        }
    }

    #[test]
    fn list_headers_are_read_and_unfolded() {
        let raw = b"From: News <news@shop.example>\r\nTo: me@example.org\r\nSubject: Sale\r\n\
List-Id: Shop News <News.Shop.Example>\r\n\
List-Unsubscribe: <mailto:leave@shop.example?subject=unsub>,\r\n <https://shop.example/u/\r\n abc>\r\n\
List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\nBuy now\r\n";
        let parsed = crate::mail::parse::parse(raw).unwrap();
        assert_eq!(
            parsed.list,
            ListHeaders {
                unsubscribe: Some(
                    "<mailto:leave@shop.example?subject=unsub>, <https://shop.example/u/ abc>"
                        .to_owned()
                ),
                post: Some("List-Unsubscribe=One-Click".to_owned()),
                id: Some("news.shop.example".to_owned()),
            }
        );
        assert_eq!(
            choose(
                parsed.list.unsubscribe.as_deref().unwrap(),
                parsed.list.post.as_deref()
            ),
            Some(Way::OneClick(
                Url::parse("https://shop.example/u/abc").unwrap()
            ))
        );

        let plain = crate::mail::parse::parse(
            b"From: sam@example.com\r\nTo: me@example.org\r\nSubject: Hi\r\n\r\nHello\r\n",
        )
        .unwrap();
        assert_eq!(plain.list, ListHeaders::default());
    }

    #[test]
    fn the_best_way_is_chosen() {
        let both =
            "<mailto:u@list.example?subject=remove%20me>, <https://www.list.example/out?id=1>";
        // One-click needs the Post header and an https address.
        let one = choose(both, Some("List-Unsubscribe=One-Click")).unwrap();
        assert_eq!(one.method(), MailUnsubscribeMethod::OneClick);
        assert_eq!(one.domain(), "list.example");
        assert_eq!(
            choose(both, Some("  list-unsubscribe = one-click ")).map(|w| w.method()),
            Some(MailUnsubscribeMethod::OneClick)
        );
        assert_eq!(
            choose(both, None),
            Some(email(
                "u@list.example",
                "remove me",
                "Please unsubscribe me from this list."
            ))
        );
        assert_eq!(
            choose(both, Some("something else")).map(|w| w.method()),
            Some(MailUnsubscribeMethod::Email)
        );
        // An http address never gets the one-click POST: it's opened in the browser.
        assert_eq!(
            choose(
                "<http://list.example/out>",
                Some("List-Unsubscribe=One-Click")
            ),
            Some(Way::Website(Url::parse("http://list.example/out").unwrap()))
        );
        // https before http for the browser.
        assert_eq!(
            choose("<http://a.example/x>, <https://b.example/y>", None),
            Some(Way::Website(Url::parse("https://b.example/y").unwrap()))
        );
        // Without angle brackets.
        assert_eq!(
            choose("mailto:out@list.example", None).map(|w| w.domain()),
            Some("list.example".to_owned())
        );
        assert_eq!(
            choose("https://list.example/u/1, mailto:x@y.example", None).map(|w| w.method()),
            Some(MailUnsubscribeMethod::Email)
        );
        // Comments and junk around the addresses are ignored.
        assert_eq!(
            uris("(Use this) <https://a.example/1> (or) <mailto:b@a.example>"),
            vec!["https://a.example/1", "mailto:b@a.example"]
        );
        // Nothing usable.
        for header in [
            "",
            "<>",
            "<javascript:alert(1)>",
            "<file:///etc/passwd>",
            "<ftp://list.example/x>",
            "<https://user:pw@list.example/x>",
            "<mailto:not-an-address>",
            "<https://127.0.0.1/out>",
            "<https://localhost/out>",
            "<http://192.168.1.1/out>",
            "<https://[::1]/out>",
        ] {
            assert_eq!(
                choose(header, Some("List-Unsubscribe=One-Click")),
                None,
                "{header}"
            );
        }
    }

    #[test]
    fn mailto_subject_and_body_are_cleaned() {
        assert_eq!(
            mailto(
                "mailto:leave+abc123@list.example?subject=unsubscribe+abc123&body=Remove%0D%0Ame%20please"
            ),
            Some(email(
                "leave+abc123@list.example",
                "unsubscribe+abc123",
                "Remove\nme please"
            ))
        );
        assert_eq!(
            mailto(
                "MAILTO:a%40b.example?Subject=Hi%0D%0ABcc:%20evil@x.example&cc=more@x.example&to=other@x.example"
            ),
            Some(email(
                "a@b.example",
                "Hi Bcc: evil@x.example",
                "Please unsubscribe me from this list."
            ))
        );
        // Several addresses: the first only.
        assert_eq!(
            mailto("mailto:one@a.example,two@b.example").map(|w| w.domain()),
            Some("a.example".to_owned())
        );
        let Some(Way::Email { subject, body, .. }) = mailto(&format!(
            "mailto:a@b.example?subject={}&body={}%07%1B[31m",
            "x".repeat(500),
            "y".repeat(5000)
        )) else {
            panic!("not an email");
        };
        assert_eq!(subject.len(), MAX_SUBJECT);
        assert_eq!(body.len(), MAX_BODY);
        assert!(!body.contains('\u{7}'));
        assert_eq!(clean_body("a%07\u{1b}[31mb\r\nc", 100), "a%07[31mb\nc");
        assert_eq!(mailto("mailto:a@b.example <x>"), None);
        assert_eq!(mailto("https://a.example"), None);
    }

    #[test]
    fn list_ids_are_bare() {
        assert_eq!(
            list_id("Weekly <Weekly.Example.COM>"),
            Some("weekly.example.com".to_owned())
        );
        assert_eq!(list_id("plain.example"), Some("plain.example".to_owned()));
        assert_eq!(list_id("<>"), None);
        assert_eq!(list_id("two words"), None);
    }

    /// A list server on this computer, recording what it was sent.
    async fn list_server() -> (String, Arc<std::sync::Mutex<Vec<(String, String, String)>>>) {
        use axum::Router;
        use axum::response::Redirect;
        use axum::routing::post;
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = seen.clone();
        let app = Router::new()
            .route("/u/ok", post(|| async { "You're unsubscribed." }))
            .route(
                "/u/no",
                post(|| async { (axum::http::StatusCode::NOT_FOUND, "no") }),
            )
            .route(
                "/u/to-router",
                post(|| async { Redirect::temporary("http://192.168.1.1/admin") }),
            )
            .route(
                "/u/to-metadata",
                post(|| async { Redirect::temporary("http://169.254.169.254/latest") }),
            )
            .route("/u/to-ok", post(|| async { Redirect::temporary("/u/ok") }))
            .layer(axum::middleware::from_fn(
                move |req: axum::extract::Request, next: axum::middleware::Next| {
                    let record = record.clone();
                    async move {
                        let (parts, body) = req.into_parts();
                        let body = axum::body::to_bytes(body, 1 << 16).await.unwrap();
                        let header = |n: &str| {
                            parts
                                .headers
                                .get(n)
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("")
                                .to_owned()
                        };
                        assert_eq!(header("cookie"), "");
                        assert_eq!(header("referer"), "");
                        record.lock().unwrap().push((
                            parts.uri.path().to_owned(),
                            header("content-type"),
                            String::from_utf8_lossy(&body).into_owned(),
                        ));
                        next.run(axum::extract::Request::from_parts(parts, body.into()))
                            .await
                    }
                },
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    #[tokio::test]
    async fn one_click_posts_only_the_one_click_body() {
        let (base, seen) = list_server().await;
        let poster = Poster::default();
        poster.allow_local_for_tests();
        let url = |p: &str| Url::parse(&format!("{base}{p}")).unwrap();
        poster.one_click(&url("/u/ok")).await.unwrap();
        assert_eq!(
            seen.lock().unwrap()[0],
            (
                "/u/ok".to_owned(),
                "application/x-www-form-urlencoded".to_owned(),
                "List-Unsubscribe=One-Click".to_owned()
            )
        );
        let err = poster.one_click(&url("/u/no")).await.unwrap_err();
        assert!(err.contains("didn't accept"), "{err}");
        // A redirect to a public-looking place on this computer is followed (tests
        // allow loopback); into the local network it never is.
        poster.one_click(&url("/u/to-ok")).await.unwrap();
        for path in ["/u/to-router", "/u/to-metadata"] {
            let err = poster.one_click(&url(path)).await.unwrap_err();
            assert!(err.contains("doesn't go"), "{path}: {err}");
        }
    }

    #[tokio::test]
    async fn one_click_never_reaches_this_computer_or_the_local_network() {
        let (base, seen) = list_server().await;
        let port = base.rsplit(':').next().unwrap();
        // Not allowing local: the real thing.
        let poster = Poster::default();
        for url in [
            format!("{base}/u/ok"),
            format!("https://127.0.0.1:{port}/u/ok"),
            format!("https://localhost:{port}/u/ok"),
            format!("https://[::1]:{port}/u/ok"),
            format!("https://[::ffff:127.0.0.1]:{port}/u/ok"),
            format!("https://0.0.0.0:{port}/u/ok"),
            format!("https://2130706433:{port}/u/ok"),
            "https://10.0.0.1/u".to_owned(),
            "https://192.168.1.1/u".to_owned(),
            "https://172.16.5.4/u".to_owned(),
            "https://169.254.169.254/latest".to_owned(),
            "https://100.64.0.1/u".to_owned(),
            "https://[fd00::1]/u".to_owned(),
            "https://[fe80::1]/u".to_owned(),
            "https://[64:ff9b::7f00:1]/u".to_owned(),
            "https://user:pass@example.com/u".to_owned(),
            "http://example.com/u".to_owned(),
        ] {
            let err = poster
                .one_click(&Url::parse(&url).unwrap())
                .await
                .unwrap_err();
            assert!(err.contains("doesn't go"), "{url}: {err}");
        }
        // However it's spelled.
        assert!(
            poster
                .one_click(&Url::parse(&format!("https://localhost.:{port}/u/ok")).unwrap())
                .await
                .is_err()
        );
        assert!(seen.lock().unwrap().is_empty());
    }

    // --- Through the mailbox ----------------------------------------------------------

    use crate::mail::fake::{FakeMail, message};
    use crate::mail::tests::{account_without_loop, all_threads, pass};

    const ME: &str = "me@example.org";

    fn newsletter(n: u32, unsubscribe: &str, extra: &str) -> String {
        message(
            "Weekly News <news@list.example>",
            ME,
            &format!("Issue {n}"),
            "This week's news.",
            &format!("issue{n}@list.example"),
            &format!("List-Unsubscribe: {unsubscribe}\r\n{extra}"),
        )
    }

    #[tokio::test]
    async fn the_lists_email_is_sent_only_when_asked_and_filed_in_sent() {
        let fake = FakeMail::start(ME, "app-pass").await;
        let now = crate::now_ms();
        let header = "<mailto:leave@list.example?subject=unsubscribe%20abc&body=Remove%20me>";
        fake.deliver("INBOX", &newsletter(1, header, ""), now - 7_200_000, &[]);
        fake.deliver(
            "INBOX",
            &message(
                "Sam <sam@example.com>",
                ME,
                "Lunch",
                "Noon?",
                "l1@example.com",
                "",
            ),
            now - 3_600_000,
            &[],
        );
        let (state, account) = account_without_loop(&fake).await;
        pass(&state, &account).await;
        let threads = all_threads(&state).await;
        let news = threads.iter().find(|t| t.subject == "Issue 1").unwrap().id;
        let lunch = threads.iter().find(|t| t.subject == "Lunch").unwrap().id;

        // A person's email offers nothing.
        assert_eq!(offer(&state, lunch).await.unwrap(), None);
        let o = offer(&state, news).await.unwrap().unwrap();
        assert_eq!(o.method, MailUnsubscribeMethod::Email);
        assert_eq!(
            (o.domain.as_str(), o.sender.as_str()),
            ("list.example", "Weekly News")
        );
        assert_eq!((o.done, o.in_inbox, o.url), (None, 1, None));
        // Looking is not unsubscribing.
        assert!(fake.sent().is_empty());

        let o = unsubscribe(&state, news).await.unwrap();
        assert_eq!(o.done.unwrap().method, MailUnsubscribeMethod::Email);
        let sent = fake.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].from, ME);
        assert_eq!(sent[0].to, vec!["leave@list.example"]);
        assert!(
            sent[0].data.contains("Subject: unsubscribe abc"),
            "{}",
            sent[0].data
        );
        assert!(sent[0].data.contains("Remove me"), "{}", sent[0].data);
        assert_eq!(fake.count("Sent"), 1);

        // The next issue shows it's done (remembered per list), and both can be
        // archived in one go; the person's email stays.
        fake.deliver("INBOX", &newsletter(2, header, ""), now - 600_000, &[]);
        pass(&state, &account).await;
        let threads = all_threads(&state).await;
        let second = threads.iter().find(|t| t.subject == "Issue 2").unwrap().id;
        let o = offer(&state, second).await.unwrap().unwrap();
        assert!(o.done.is_some());
        assert_eq!(o.in_inbox, 2);
        assert_eq!(archive_list(&state, second).await.unwrap(), 2);
        assert_eq!(fake.count("INBOX"), 1);
        let left: Vec<_> = all_threads(&state)
            .await
            .into_iter()
            .filter(|t| t.subject != "unsubscribe abc")
            .map(|t| t.subject)
            .collect();
        assert_eq!(left, vec!["Lunch"]);
        assert_eq!(fake.sent().len(), 1);
    }

    #[tokio::test]
    async fn one_click_reaches_the_list_and_older_mail_reads_its_headers_once() {
        let (base, seen) = list_server().await;
        let fake = FakeMail::start(ME, "app-pass").await;
        fake.deliver(
            "INBOX",
            &newsletter(
                1,
                &format!("<mailto:leave@list.example>, <{base}/u/ok>"),
                "List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\nList-Id: Weekly <weekly.list.example>\r\n",
            ),
            crate::now_ms() - 3_600_000,
            &[],
        );
        let (state, account) = account_without_loop(&fake).await;
        state.mail.unsubscribe.allow_local_for_tests();
        pass(&state, &account).await;
        let news = all_threads(&state).await[0].id;

        // As if stored before the headers were kept.
        state
            .db
            .call(|c| {
                c.execute(
                    "UPDATE mail_messages SET list_unsubscribe = NULL, list_unsubscribe_post = NULL,
                        list_id = NULL",
                    [],
                )
            })
            .await
            .unwrap();
        let o = offer(&state, news).await.unwrap().unwrap();
        assert_eq!(o.method, MailUnsubscribeMethod::OneClick);
        assert_eq!(o.domain, "127.0.0.1");
        let kept: String = state
            .db
            .call(|c| c.query_row("SELECT list_id FROM mail_messages", [], |r| r.get(0)))
            .await
            .unwrap();
        assert_eq!(kept, "weekly.list.example");
        assert!(seen.lock().unwrap().is_empty());

        let o = unsubscribe(&state, news).await.unwrap();
        assert_eq!(o.done.unwrap().method, MailUnsubscribeMethod::OneClick);
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(seen.lock().unwrap()[0].2, "List-Unsubscribe=One-Click");
        // No email when one-click worked.
        assert!(fake.sent().is_empty());
    }
}
