//! Sending over SMTP (lettre). Only ever called for a message the user wrote or
//! approved: the Mail panel's Send button, or an approved `mail_send`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::time::Duration;

use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, Mailboxes, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use mimi_protocol::{MailDraft, MailSecurity};
use rusqlite::OptionalExtension;
use uuid::Uuid;

use super::{EmailConfig, MailError, net};
use crate::AppState;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Recipients per message, so a bad draft can't mail a whole address book.
pub const MAX_RECIPIENTS: usize = 20;
const MAX_BODY: usize = 100_000;
/// Total size of the attachments of one message (attached, or forwarded along).
pub const MAX_ATTACHMENTS: usize = 20 * 1024 * 1024;
/// Largest request to send a message: attachments come base64-encoded (4/3 larger).
pub const MAX_REQUEST_BYTES: usize = MAX_ATTACHMENTS / 3 * 4 + 2 * 1024 * 1024;

fn transport(
    config: &EmailConfig,
    user: Option<&str>,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailError> {
    let s = &config.servers;
    net::check_security(&s.smtp_host, s.smtp_security)?;
    let params = || {
        TlsParameters::builder(s.smtp_host.clone())
            // Servers on this computer (Proton Bridge) use their own certificate.
            .dangerous_accept_invalid_certs(net::is_loopback(&s.smtp_host))
            .build_rustls()
            .map_err(|e| MailError::Tls(e.to_string()))
    };
    let tls = match s.smtp_security {
        MailSecurity::Tls => Tls::Wrapper(params()?),
        MailSecurity::StartTls => Tls::Required(params()?),
        MailSecurity::Plain => Tls::None,
    };
    let user = user.unwrap_or(&config.email).to_owned();
    Ok(
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(s.smtp_host.clone())
            .port(s.smtp_port)
            .tls(tls)
            .credentials(Credentials::new(user, config.password.clone()))
            // Not the computer's name: it would tell every recipient what it's called.
            .hello_name(ClientId::Ipv4(Ipv4Addr::LOCALHOST))
            .timeout(Some(TIMEOUT))
            .build(),
    )
}

fn smtp_error(host: &str, e: lettre::transport::smtp::Error) -> MailError {
    let code = e.status().map(|c| c.to_string());
    if code
        .as_deref()
        .is_some_and(|c| c.starts_with("535") || c.starts_with("534"))
    {
        MailError::Login
    } else if e.is_tls() {
        MailError::Tls(e.to_string())
    } else if e.is_timeout()
        || (e.status().is_none() && !e.is_client() && std::error::Error::source(&e).is_some())
    {
        // Connection refused, DNS failures and the like.
        MailError::Unreachable(host.to_owned())
    } else {
        MailError::Protocol(e.to_string())
    }
}

/// Signs in to the SMTP server without sending anything.
pub async fn check(config: &EmailConfig, user: Option<&str>) -> Result<(), MailError> {
    let t = transport(config, user)?;
    match t.test_connection().await {
        Ok(true) => Ok(()),
        Ok(false) => Err(MailError::Protocol(
            "the server closed the connection".to_owned(),
        )),
        Err(e) => Err(smtp_error(&config.servers.smtp_host, e)),
    }
}

/// What a reply needs to thread correctly.
#[derive(Debug, Clone)]
pub struct ReplyHeaders {
    pub connection_id: Uuid,
    /// The address the conversation's latest incoming mail arrived at.
    pub received_on: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

pub async fn reply_headers(state: &AppState, thread: i64) -> Result<Option<ReplyHeaders>, String> {
    state
        .db
        .call(move |c| {
            let received_on: Option<String> = c
                .query_row(
                    "SELECT received_on FROM mail_messages
                     WHERE thread_id = ?1 AND received_on IS NOT NULL ORDER BY date DESC LIMIT 1",
                    [thread],
                    |r| r.get(0),
                )
                .optional()?;
            c.query_row(
                "SELECT connection_id, message_id, refs FROM mail_messages
                 WHERE thread_id = ?1 ORDER BY date DESC, id DESC LIMIT 1",
                [thread],
                |r| {
                    let conn: String = r.get(0)?;
                    let mid: Option<String> = r.get(1)?;
                    let refs: String = r.get(2)?;
                    let mut references: Vec<String> =
                        refs.split_whitespace().map(str::to_owned).collect();
                    references.extend(mid.clone());
                    // Long threads: the first and the latest references are what matter.
                    if references.len() > 12 {
                        let tail = references.split_off(references.len() - 11);
                        references.truncate(1);
                        references.extend(tail);
                    }
                    Ok(ReplyHeaders {
                        connection_id: conn.parse().unwrap_or_default(),
                        received_on: received_on.clone(),
                        in_reply_to: mid,
                        references,
                    })
                },
            )
            .optional()
        })
        .await
        .map_err(|e| e.to_string())
}

/// A message ready to go.
pub struct Built {
    /// What goes out: without its Bcc header, so blind copies stay blind.
    pub message: Message,
    /// The copy filed in the user's Sent folder, which keeps the Bcc line.
    pub formatted: Vec<u8>,
}

fn mailbox(raw: &str) -> Result<Mailbox, String> {
    raw.trim()
        .parse::<Mailbox>()
        .map_err(|_| format!("“{}” isn't an email address.", raw.trim()))
}

/// A picture placed in the text of a formatted message: its HTML shows it as
/// `cid:<content_id>`.
pub struct Inline {
    pub content_id: String,
    pub file: super::parse::Attachment,
}

/// Builds the message from a draft, from `from` (the account's address or one of its
/// aliases, checked by the caller):
///
/// - without `html`: plain text, as it always was (`multipart/mixed` with files);
/// - with it: `multipart/alternative` (the plain-text `body`, and the cleaned HTML),
///   the HTML inside `multipart/related` with the pictures it shows, all inside
///   `multipart/mixed` when files are attached. Inline pictures the HTML doesn't show go
///   as ordinary attachments, so nothing the user added is lost.
pub fn build(
    from: &str,
    draft: &MailDraft,
    reply: Option<&ReplyHeaders>,
    attachments: &[super::parse::Attachment],
    inline: &[Inline],
) -> Result<Built, String> {
    // Each address once: a Cc that repeats a To (models do that) would get it twice.
    let mut seen = std::collections::HashSet::new();
    let mut unique = |list: &[String]| -> Result<Vec<Mailbox>, String> {
        let mut out = Vec::new();
        for raw in list.iter().filter(|t| !t.trim().is_empty()) {
            let m = mailbox(raw)?;
            if seen.insert(m.email.to_string().to_lowercase()) {
                out.push(m);
            }
        }
        Ok(out)
    };
    let to = unique(&draft.to)?;
    let cc = unique(&draft.cc)?;
    let bcc = unique(&draft.bcc)?;
    if to.is_empty() {
        return Err("Add at least one recipient.".to_owned());
    }
    if to.len() + cc.len() + bcc.len() > MAX_RECIPIENTS {
        return Err(format!("That's more than {MAX_RECIPIENTS} recipients."));
    }
    let html = draft.html.as_deref().filter(|h| !h.trim().is_empty());
    if draft.body.len() > MAX_BODY || html.is_some_and(|h| h.len() > MAX_HTML) {
        return Err("That message is too long.".to_owned());
    }
    let domain = from.rsplit('@').next().unwrap_or("localhost").to_owned();
    let from = mailbox(from)?;
    let mut builder = Message::builder()
        .from(from)
        .subject(draft.subject.trim())
        .date_now()
        .message_id(Some(format!("<{}@{domain}>", Uuid::now_v7().simple())));
    for m in to {
        builder = builder.to(m);
    }
    for m in cc {
        builder = builder.cc(m);
    }
    for m in bcc {
        builder = builder.bcc(m);
    }
    if let Some(r) = reply {
        if let Some(id) = &r.in_reply_to {
            builder = builder.in_reply_to(format!("<{id}>"));
        }
        if !r.references.is_empty() {
            builder = builder.references(
                r.references
                    .iter()
                    .map(|id| format!("<{id}>"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
    }
    // The HTML, cleaned, and which of the pictures it shows.
    let html = html.map(|h| {
        let pictures: HashSet<String> = inline
            .iter()
            .filter(|i| is_inline_picture(i))
            .map(|i| i.content_id.clone())
            .collect();
        outgoing_html(h, &pictures)
    });
    let (in_text, loose): (Vec<&Inline>, Vec<&Inline>) = inline.iter().partition(|i| {
        html.as_ref()
            .is_some_and(|(_, shown)| shown.contains(&i.content_id))
    });
    let files: Vec<&super::parse::Attachment> = attachments
        .iter()
        .chain(loose.iter().map(|i| &i.file))
        .collect();
    let mut body = draft.body.replace("\r\n", "\n");
    if let Some((html, _)) = &html
        && body.trim().is_empty()
    {
        // Clients always send the text; this is only so the text part is never blank.
        body = html2text::from_read(html.as_bytes(), 78).unwrap_or_default();
    }
    let finish = |builder: lettre::message::MessageBuilder| {
        let Some((html, _)) = &html else {
            return if files.is_empty() {
                builder.header(ContentType::TEXT_PLAIN).body(body.clone())
            } else {
                builder.multipart(with_files(SinglePart::plain(body.clone()).into(), &files))
            }
            .map_err(|e| e.to_string());
        };
        let alternative = if in_text.is_empty() {
            MultiPart::alternative_plain_html(body.clone(), html.clone())
        } else {
            let mut related = MultiPart::related().singlepart(SinglePart::html(html.clone()));
            for i in &in_text {
                related = related.singlepart(
                    Attachment::new_inline_with_name(i.content_id.clone(), i.file.name.clone())
                        .body(i.file.data.clone(), content_type(&i.file.content_type)),
                );
            }
            MultiPart::alternative()
                .singlepart(SinglePart::plain(body.clone()))
                .multipart(related)
        };
        if files.is_empty() {
            builder.multipart(alternative)
        } else {
            builder.multipart(with_files(alternative.into(), &files))
        }
        .map_err(|e| e.to_string())
    };
    // The same message twice: once to send, once (with its Bcc line) for Sent.
    let filed = finish(builder.clone().keep_bcc())?;
    let message = finish(builder)?;
    Ok(Built {
        message,
        formatted: filed.formatted(),
    })
}

/// The first part of a message, or its text and HTML, followed by attached files.
enum Content {
    Single(SinglePart),
    Multi(MultiPart),
}

impl From<SinglePart> for Content {
    fn from(p: SinglePart) -> Self {
        Content::Single(p)
    }
}

impl From<MultiPart> for Content {
    fn from(p: MultiPart) -> Self {
        Content::Multi(p)
    }
}

fn with_files(first: Content, files: &[&super::parse::Attachment]) -> MultiPart {
    let mut parts = match first {
        Content::Single(p) => MultiPart::mixed().singlepart(p),
        Content::Multi(p) => MultiPart::mixed().multipart(p),
    };
    for a in files {
        parts = parts.singlepart(
            Attachment::new(a.name.clone()).body(a.data.clone(), content_type(&a.content_type)),
        );
    }
    parts
}

fn content_type(raw: &str) -> ContentType {
    ContentType::parse(raw)
        .unwrap_or_else(|_| ContentType::parse("application/octet-stream").unwrap())
}

// --- Formatted messages -----------------------------------------------------------------

/// Most HTML in one message (its pictures are attachments, not in here).
const MAX_HTML: usize = 1_000_000;

/// Pictures that can be shown in the text. No SVG: it's a document, not a picture.
const INLINE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// An inline picture the HTML may show: a picture, under a content id that is safe in a
/// header and in an address (the app makes them from a UUID).
fn is_inline_picture(i: &Inline) -> bool {
    let id = &i.content_id;
    (1..=200).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._@+=".contains(&b))
        && INLINE_TYPES.contains(&i.file.content_type.to_ascii_lowercase().as_str())
}

/// Formatting the editor makes, and nothing else.
const HTML_TAGS: &[&str] = &[
    "p",
    "div",
    "br",
    "strong",
    "b",
    "em",
    "i",
    "u",
    "s",
    "a",
    "ul",
    "ol",
    "li",
    "blockquote",
    "img",
];

/// Dropped with everything in them.
const HTML_DROPPED: &[&str] = &[
    "script", "style", "template", "textarea", "select", "option", "iframe", "object", "embed",
    "noscript", "title", "svg", "math", "head",
];

/// How quoted text looks in the recipient's mail app (as Gmail and Apple Mail quote).
const QUOTE_STYLE: &str = "margin:0 0 0 0.8ex;border-left:1px solid #ccc;padding-left:1ex";

/// The user's HTML made safe to send, as a whole document, and the content ids of the
/// pictures it shows. Only formatting is kept: links to the web or an email address, and
/// pictures that are attached to this message (`pictures`). No styles, classes, event
/// handlers, scripts, forms, frames or remote pictures, so the message can't run or
/// load anything, or tell anyone when it's read.
pub fn outgoing_html(html: &str, pictures: &HashSet<String>) -> (String, HashSet<String>) {
    let allowed = pictures.clone();
    let clean = ammonia::Builder::empty()
        .tags(HTML_TAGS.iter().copied().collect())
        .clean_content_tags(HTML_DROPPED.iter().copied().collect())
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href"])),
            ("img", HashSet::from(["src", "alt", "width"])),
            ("ol", HashSet::from(["start"])),
        ]))
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid"]))
        .url_relative(ammonia::UrlRelative::Deny)
        .link_rel(None)
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            let v = value.trim();
            let lower = v.to_ascii_lowercase();
            let number = || {
                (!v.is_empty() && v.len() <= 4 && v.bytes().all(|b| b.is_ascii_digit()))
                    .then_some(Cow::Borrowed(v))
            };
            match (element, attribute) {
                ("a", "href") => ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|s| lower.starts_with(s))
                    .then_some(Cow::Borrowed(v)),
                ("img", "src") => v
                    .get(4..)
                    .filter(|id| lower.starts_with("cid:") && allowed.contains(*id))
                    .map(|_| Cow::Borrowed(v)),
                ("img", "width") | ("ol", "start") => number(),
                _ => Some(Cow::Borrowed(value)),
            }
        })
        .clean(html)
        .to_string();
    let shown = pictures
        .iter()
        .filter(|id| clean.contains(&format!("src=\"cid:{id}\"")))
        .cloned()
        .collect();
    // Allowed tags carry no attributes but those above, so this is exactly how ammonia
    // writes a quote.
    let clean = clean.replace(
        "<blockquote>",
        &format!("<blockquote style=\"{QUOTE_STYLE}\">"),
    );
    (
        format!(
            "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"></head><body>{clean}</body></html>\n"
        ),
        shown,
    )
}

/// Who to sign in to SMTP as: the account's username when it has one (iCloud always
/// wants the full address for SMTP), else the address.
pub fn user(config: &EmailConfig) -> Option<&str> {
    if config.preset == "icloud" {
        None
    } else {
        config.servers.username.as_deref()
    }
}

/// Hands the message to the account's SMTP server.
pub async fn send(config: &EmailConfig, built: &Built) -> Result<(), String> {
    let t = transport(config, user(config)).map_err(|e| e.to_string())?;
    t.send(built.message.clone())
        .await
        .map(|_| ())
        .map_err(|e| smtp_error(&config.servers.smtp_host, e).to_string())
}

/// Everyone the message goes to, for display.
pub fn recipients(draft: &MailDraft) -> Vec<String> {
    let mut all: Vec<String> = draft
        .to
        .iter()
        .chain(&draft.cc)
        .chain(&draft.bcc)
        .map(|s| s.trim().to_owned())
        .collect();
    all.retain(|s| !s.is_empty());
    all
}

/// Parses "Sam <sam@example.com>, bo@example.com" into a list of addresses.
pub fn split_addresses(raw: &str) -> Vec<String> {
    raw.parse::<Mailboxes>()
        .map(|m| m.into_iter().map(|m| m.to_string()).collect())
        .unwrap_or_else(|_| {
            raw.split([',', ';'])
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use mail_parser::{MessageParser, MimeHeaders};

    use super::super::parse::Attachment as File;
    use super::*;

    fn draft(html: Option<&str>) -> MailDraft {
        MailDraft {
            connection_id: None,
            from: None,
            to: vec!["sam@example.com".to_owned()],
            cc: vec![],
            bcc: vec!["bo@example.net".to_owned()],
            subject: "Photos".to_owned(),
            body: "Here they are.".to_owned(),
            reply_to: None,
            forward_of: None,
            attachments: vec![],
            html: html.map(str::to_owned),
        }
    }

    fn picture(id: &str) -> Inline {
        Inline {
            content_id: id.to_owned(),
            file: File {
                name: format!("{id}.png"),
                content_type: "image/png".to_owned(),
                data: b"\x89PNG not really".to_vec(),
            },
        }
    }

    fn pdf() -> File {
        File {
            name: "plan.pdf".to_owned(),
            content_type: "application/pdf".to_owned(),
            data: b"%PDF-1.7".to_vec(),
        }
    }

    /// Each part's content type, in order (depth first).
    fn shape(raw: &[u8]) -> Vec<String> {
        let msg = MessageParser::default().parse(raw).unwrap();
        msg.parts
            .iter()
            .map(|p| {
                let ct = p.content_type().unwrap();
                format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or_default())
            })
            .collect()
    }

    #[test]
    fn without_html_the_message_stays_plain_text() {
        let built = build("me@example.org", &draft(None), None, &[], &[]).unwrap();
        assert_eq!(shape(&built.formatted), ["text/plain"]);
        // An inline picture without HTML to show it is just attached.
        let built = build("me@example.org", &draft(None), None, &[], &[picture("a")]).unwrap();
        assert_eq!(
            shape(&built.formatted),
            ["multipart/mixed", "text/plain", "image/png"]
        );
    }

    #[test]
    fn formatted_text_goes_with_its_plain_version() {
        let built = build(
            "me@example.org",
            &draft(Some("<div><strong>Here</strong> they are.</div>")),
            None,
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(
            shape(&built.formatted),
            ["multipart/alternative", "text/plain", "text/html"]
        );
        let msg = MessageParser::default().parse(&built.formatted).unwrap();
        assert_eq!(msg.body_text(0).unwrap().trim(), "Here they are.");
        assert!(
            msg.body_html(0)
                .unwrap()
                .contains("<div><strong>Here</strong> they are.</div>")
        );
        // Blind copies stay blind with formatting too.
        let sent = String::from_utf8(built.message.formatted()).unwrap();
        assert!(!sent.contains("bo@example.net"), "{sent}");
        let filed = String::from_utf8(built.formatted).unwrap();
        assert!(filed.contains("Bcc: bo@example.net"), "{filed}");
    }

    #[test]
    fn pictures_in_the_text_go_inline_and_files_go_after() {
        let html = r#"<div>Look:</div><div><img src="cid:a@inline" alt="a.png" width="600"></div>"#;
        let built = build(
            "me@example.org",
            &draft(Some(html)),
            None,
            &[pdf()],
            // "b" isn't in the text (taken out of it, say): it still goes, attached.
            &[picture("a@inline"), picture("b@inline")],
        )
        .unwrap();
        assert_eq!(
            shape(&built.formatted),
            [
                "multipart/mixed",
                "multipart/alternative",
                "text/plain",
                "multipart/related",
                "text/html",
                "image/png",
                "application/pdf",
                "image/png",
            ]
        );
        let msg = MessageParser::default().parse(&built.formatted).unwrap();
        assert_eq!(msg.parts[5].content_id(), Some("a@inline"));
        assert!(
            msg.parts[5]
                .content_disposition()
                .is_some_and(|d| d.ctype() == "inline")
        );
        assert_eq!(msg.parts[7].attachment_name(), Some("b@inline.png"));
        let shown = msg.body_html(0).unwrap();
        assert!(
            shown.contains(r#"<img src="cid:a@inline" alt="a.png" width="600">"#),
            "{shown}"
        );

        // Without other files there's no multipart/mixed around it.
        let built = build(
            "me@example.org",
            &draft(Some(html)),
            None,
            &[],
            &[picture("a@inline")],
        )
        .unwrap();
        assert_eq!(
            shape(&built.formatted),
            [
                "multipart/alternative",
                "text/plain",
                "multipart/related",
                "text/html",
                "image/png",
            ]
        );
    }

    #[test]
    fn hostile_html_is_cleaned_before_it_goes() {
        let pictures = HashSet::from(["ok@inline".to_owned()]);
        let hostile = r#"
            <script>alert(1)</script><style>@import url(https://evil.example/x.css)</style>
            <div style="background:url(https://evil.example/t.gif)" onclick="steal()" class="x" id="y">
              <b onmouseover="x()">bold</b> <span style="color:red">span</span>
              <a href="javascript:alert(1)">js</a> <a href="https://ok.example/" target="_blank" rel="opener">web</a>
              <a href="mailto:sam@example.com">mail</a> <a href="/relative">rel</a>
              <a href="data:text/html,<script>x</script>">data</a>
              <img src="https://tracker.example/pixel.gif" width="1">
              <img src="data:image/png;base64,AAAA">
              <img src="cid:missing@inline">
              <img src="cid:ok@inline" onerror="x()" width="100%">
              <iframe src="https://evil.example/"></iframe>
              <form action="https://evil.example/"><input name="p"><button>Go</button></form>
              <svg><image href="https://evil.example/i.png"/></svg>
              <!-- a comment -->
              <blockquote>quoted</blockquote>
              <ol start="3"><li>three</li></ol>
            </div>
        "#;
        let (html, shown) = outgoing_html(hostile, &pictures);
        for gone in [
            "script",
            "alert",
            "style=\"background",
            "@import",
            "onclick",
            "class=",
            "id=",
            "onmouseover",
            "<span",
            "javascript",
            "/relative",
            "data:",
            "tracker",
            "missing",
            "onerror",
            "100%",
            "iframe",
            "<form",
            "<input",
            "<button",
            "<svg",
            "evil",
            "comment",
            "target=",
            "rel=",
        ] {
            assert!(!html.contains(gone), "{gone} in {html}");
        }
        for kept in [
            "<b>bold</b>",
            "span",
            "<a>js</a>",
            r#"<a href="https://ok.example/">web</a>"#,
            r#"<a href="mailto:sam@example.com">mail</a>"#,
            r#"<img src="cid:ok@inline">"#,
            r#"<blockquote style="margin:0 0 0 0.8ex;border-left:1px solid #ccc;padding-left:1ex">quoted</blockquote>"#,
            r#"<ol start="3"><li>three</li></ol>"#,
            "<!DOCTYPE html>",
        ] {
            assert!(html.contains(kept), "{kept} not in {html}");
        }
        assert_eq!(shown, pictures);
        // A picture whose id could break out of a header or an address isn't shown.
        let mut odd = picture("a>b");
        assert!(!is_inline_picture(&odd));
        odd.content_id = "fine@inline".to_owned();
        odd.file.content_type = "image/svg+xml".to_owned();
        assert!(!is_inline_picture(&odd));
    }

    #[test]
    fn a_formatted_message_is_never_without_its_text() {
        let mut d = draft(Some("<div>Only <em>formatted</em></div>"));
        d.body = String::new();
        let built = build("me@example.org", &d, None, &[], &[]).unwrap();
        let msg = MessageParser::default().parse(&built.formatted).unwrap();
        assert!(msg.body_text(0).unwrap().contains("formatted"));
        d.html = Some("x".repeat(MAX_HTML + 1));
        assert!(build("me@example.org", &d, None, &[], &[]).is_err());
    }
}
