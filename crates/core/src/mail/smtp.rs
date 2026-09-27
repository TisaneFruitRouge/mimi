//! Sending over SMTP (lettre). Only ever called for a message the user wrote or
//! approved: the Mail panel's Send button, or an approved `mail_send`.

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
/// Total size of attachments forwarded in one message.
pub const MAX_ATTACHMENTS: usize = 20 * 1024 * 1024;

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
    pub message: Message,
    pub formatted: Vec<u8>,
}

fn mailbox(raw: &str) -> Result<Mailbox, String> {
    raw.trim()
        .parse::<Mailbox>()
        .map_err(|_| format!("“{}” isn't an email address.", raw.trim()))
}

/// Builds the message from a draft: plain text, from `from` (the account's address or
/// one of its aliases, checked by the caller).
pub fn build(
    from: &str,
    draft: &MailDraft,
    reply: Option<&ReplyHeaders>,
    attachments: &[super::parse::Attachment],
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
    if to.is_empty() {
        return Err("Add at least one recipient.".to_owned());
    }
    if to.len() + cc.len() > MAX_RECIPIENTS {
        return Err(format!("That's more than {MAX_RECIPIENTS} recipients."));
    }
    if draft.body.len() > MAX_BODY {
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
    let body = draft.body.replace("\r\n", "\n");
    let message = if attachments.is_empty() {
        builder.header(ContentType::TEXT_PLAIN).body(body)
    } else {
        let mut parts = MultiPart::mixed().singlepart(SinglePart::plain(body));
        for a in attachments {
            let kind = ContentType::parse(&a.content_type).unwrap_or(ContentType::TEXT_PLAIN);
            parts = parts.singlepart(Attachment::new(a.name.clone()).body(a.data.clone(), kind));
        }
        builder.multipart(parts)
    }
    .map_err(|e| e.to_string())?;
    let formatted = message.formatted();
    Ok(Built { message, formatted })
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
