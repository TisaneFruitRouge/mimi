//! Email: the user's own accounts over IMAP (read) and SMTP (send), with app passwords
//! and no registered app. Recent mail is copied into the encrypted database so it can be
//! searched, sorted and summarised locally; sending always goes through the user (an
//! approval card, or their own click in the Mail panel).
//!
//! Email is written by other people: everything read from it is data, never
//! instructions (see `tools.rs` and `triage.rs`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mimi_protocol::{
    Event, MailAccount, MailAttachmentSource, MailBox, MailDraft, MailOverview, MailPreset,
    MailReceivedAddress, MailSecurity, MailServers, MailThread, MailThreadDetail,
    NewMailAttachment,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::AppState;
use crate::connections::store as connection_store;

pub mod batch;
pub mod contacts;
pub mod discover;
pub mod drafts;
pub mod flags;
pub mod folders;
pub mod images;
pub mod jev;
pub mod known;
pub mod mentions;
pub mod model;
pub mod net;
pub mod notify;
pub mod older;
pub mod outbox;
pub mod parse;
pub mod render;
pub mod signature;
pub mod smtp;
pub mod store;
pub mod suspicious;
pub mod sync;
pub mod tools;
pub mod triage;
pub mod unsubscribe;

#[cfg(test)]
pub mod fake;
#[cfg(test)]
mod older_tests;
#[cfg(test)]
mod tests;

/// The integration id of email connections.
pub const EMAIL: &str = "email";

/// A healthy account's one-line description.
pub const DETAIL: &str = "Reads your mail. Sends as you allow in Permissions.";

#[derive(Debug, Clone, thiserror::Error)]
pub enum MailError {
    #[error(
        "The mail server didn't accept that address and password. Some services (iCloud, Gmail, Yahoo…) need an app password instead of your usual one."
    )]
    Login,
    #[error("Couldn't reach {0}. Check the server name and your internet connection.")]
    Unreachable(String),
    #[error("Couldn't set up a secure connection to the mail server ({0}).")]
    Tls(String),
    #[error("The mail server answered something unexpected ({0}).")]
    Protocol(String),
    #[error("{0}")]
    Refused(String),
}

/// A connected account, as stored (encrypted) in its connection's config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub email: String,
    pub password: String,
    pub preset: String,
    pub servers: MailServers,
}

impl EmailConfig {
    pub fn username(&self) -> &str {
        self.servers.username.as_deref().unwrap_or(&self.email)
    }
}

/// A connected account.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: Uuid,
    pub name: String,
    pub config: EmailConfig,
}

/// Live mail state: wake-ups for the sync loops and the sorting queue.
#[derive(Default)]
pub struct Mail {
    /// Where Jev requests go when not TypeSafe's endpoint (tests).
    pub jev_api: std::sync::Mutex<Option<String>>,
    pokes: Mutex<HashMap<Uuid, Arc<Notify>>>,
    /// Wakes the sorting queue when new mail arrives.
    pub triage_wake: Notify,
    /// Fetches an email's pictures when the user asks for them.
    pub images: images::Fetcher,
    /// Asks a mailing list's website to unsubscribe the user, on their click.
    pub unsubscribe: unsubscribe::Poster,
    /// Wakes the loop that sends mail waiting in the outbox (undo send, send later).
    pub outbox: outbox::Outbox,
    /// Copies saved drafts to the servers' Drafts folders.
    pub drafts: drafts::Drafts,
}

impl Mail {
    /// The wake-up signal of one account's sync loop.
    pub fn poke_handle(&self, id: Uuid) -> Arc<Notify> {
        self.pokes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(id)
            .or_default()
            .clone()
    }

    /// Asks an account's sync loop to check the server now.
    pub fn poke(&self, id: Uuid) {
        self.poke_handle(id).notify_one();
    }
}

// --- Services -------------------------------------------------------------------------

fn servers(
    imap_host: &str,
    imap_port: u16,
    imap_security: MailSecurity,
    smtp_host: &str,
    smtp_port: u16,
    smtp_security: MailSecurity,
) -> MailServers {
    MailServers {
        imap_host: imap_host.to_owned(),
        imap_port,
        imap_security,
        smtp_host: smtp_host.to_owned(),
        smtp_port,
        smtp_security,
        username: None,
    }
}

/// The services the connect dialog offers.
pub fn presets() -> Vec<MailPreset> {
    use MailSecurity::*;
    let preset =
        |id: &str, name: &str, help: &str, url: Option<&str>, servers, supported| MailPreset {
            id: id.to_owned(),
            name: name.to_owned(),
            help: help.to_owned(),
            help_url: url.map(str::to_owned),
            servers,
            supported,
        };
    vec![
        preset(
            "icloud",
            "iCloud Mail",
            "Use an app-specific password: sign in at account.apple.com, open Sign-In and Security, then App-Specific Passwords.",
            Some("https://support.apple.com/en-us/102654"),
            Some(servers(
                "imap.mail.me.com",
                993,
                Tls,
                "smtp.mail.me.com",
                587,
                StartTls,
            )),
            true,
        ),
        preset(
            "gmail",
            "Gmail",
            "Use an app password. Google only offers them once 2-Step Verification is on: turn it on, then create one on the App passwords page.",
            Some("https://myaccount.google.com/apppasswords"),
            Some(servers(
                "imap.gmail.com",
                993,
                Tls,
                "smtp.gmail.com",
                465,
                Tls,
            )),
            true,
        ),
        preset(
            "fastmail",
            "Fastmail",
            "Create an app password in Settings, Privacy & Security, Manage app passwords. Give it access to mail (IMAP and SMTP).",
            Some("https://www.fastmail.help/hc/en-us/articles/360058752854"),
            Some(servers(
                "imap.fastmail.com",
                993,
                Tls,
                "smtp.fastmail.com",
                465,
                Tls,
            )),
            true,
        ),
        preset(
            "proton",
            "Proton Mail",
            "Needs Proton Mail Bridge running on this computer (paid Proton plans). Use the password Bridge shows you, not your Proton password.",
            Some("https://proton.me/mail/bridge"),
            Some(servers(
                "127.0.0.1",
                1143,
                StartTls,
                "127.0.0.1",
                1025,
                StartTls,
            )),
            true,
        ),
        preset(
            "outlook",
            "Outlook.com or Hotmail",
            "Microsoft only lets apps in through a Microsoft sign-in, which Mimi doesn't support yet.",
            None,
            None,
            false,
        ),
        preset(
            "other",
            "Other",
            "Your mail provider's help pages list these settings. Use an app password if they offer one.",
            None,
            None,
            true,
        ),
    ]
}

/// The preset that fits an address, if one does.
pub fn guess_preset(email: &str) -> Option<&'static str> {
    let domain = email.rsplit('@').next()?.trim().to_ascii_lowercase();
    let is = |names: &[&str]| names.iter().any(|n| domain == *n);
    if is(&["gmail.com", "googlemail.com"]) {
        Some("gmail")
    } else if is(&["icloud.com", "me.com", "mac.com"]) {
        Some("icloud")
    } else if domain.starts_with("fastmail.") || is(&["sent.com", "fastmail.fm"]) {
        Some("fastmail")
    } else if is(&["proton.me", "protonmail.com", "protonmail.ch", "pm.me"]) {
        Some("proton")
    } else if domain.starts_with("outlook.")
        || domain.starts_with("hotmail.")
        || domain.starts_with("live.")
        || is(&["msn.com"])
    {
        Some("outlook")
    } else {
        None
    }
}

/// Works out an account's settings from what the user entered, then checks them against
/// the real servers: signs in over IMAP and SMTP. Returns the connection's name and config.
pub async fn connect(
    http: &reqwest::Client,
    email: String,
    password: String,
    preset: Option<String>,
    custom: Option<MailServers>,
) -> Result<(String, EmailConfig), String> {
    let email = email.trim().to_lowercase();
    let password: String = password.trim().to_owned();
    if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        return Err("Enter your full email address.".to_owned());
    }
    if password.is_empty() {
        return Err("Enter your password.".to_owned());
    }
    let explicit = preset.filter(|p| !p.is_empty() && p != "other");
    let (preset_id, servers) = match (explicit, custom) {
        // A service picked in the dialog.
        (Some(id), _) => {
            let chosen = presets()
                .into_iter()
                .find(|p| p.id == id)
                .ok_or("Unknown mail service.")?;
            if !chosen.supported {
                return Err(chosen.help);
            }
            let servers = chosen.servers.ok_or("Enter your mail server details.")?;
            (id, servers)
        }
        // Servers typed by the user.
        (None, Some(mut s)) => {
            s.imap_host = s.imap_host.trim().to_owned();
            s.smtp_host = s.smtp_host.trim().to_owned();
            s.username = s
                .username
                .map(|u| u.trim().to_owned())
                .filter(|u| !u.is_empty());
            if s.imap_host.is_empty() || s.smtp_host.is_empty() {
                return Err("Enter both server names.".to_owned());
            }
            ("other".to_owned(), s)
        }
        // Just an address: find the servers the way mail apps do.
        (None, None) => {
            let found = discover::discover(&email, http).await;
            if !found.supported {
                return Err(found
                    .help
                    .unwrap_or_else(|| "This mail service can't be connected.".to_owned()));
            }
            let servers = found.servers.ok_or(
                "Mimi couldn't find the mail servers for this address. Enter them under “Server settings”.",
            )?;
            (found.preset.unwrap_or_else(|| "other".to_owned()), servers)
        }
    };
    let mut config = EmailConfig {
        email: email.clone(),
        // Google shows app passwords in groups ("abcd efgh ijkl mnop"); the spaces
        // aren't part of them.
        password: if preset_id == "gmail" {
            password.replace(' ', "")
        } else {
            password
        },
        preset: preset_id.clone(),
        servers,
    };
    // Most services sign in with the full address; some (iCloud at times, older
    // hosting) want only the part before the @. A username from the settings goes
    // first, then the address, in case a name was typed there by mistake.
    let usernames = match config.servers.username.clone() {
        Some(name) => vec![Some(name), None],
        None => vec![None, email.split('@').next().map(str::to_owned)],
    };
    let mut last = MailError::Login;
    let mut signed_in = false;
    for username in usernames {
        config.servers.username = username;
        match sync::check_login(&config).await {
            Ok(()) => {
                signed_in = true;
                break;
            }
            Err(e @ MailError::Login) => last = e,
            Err(e) => return Err(e.to_string()),
        }
    }
    if !signed_in {
        return Err(last.to_string());
    }
    smtp::check(&config, smtp::user(&config))
        .await
        .map_err(|e| format!("Reading mail works, but sending doesn't: {e}"))?;
    Ok((email, config))
}

// --- Accounts -------------------------------------------------------------------------

pub async fn accounts(state: &AppState) -> Vec<Account> {
    let Ok(rows) = connection_store::list(&state.db).await else {
        return Vec::new();
    };
    rows.into_iter()
        .filter(|r| r.integration == EMAIL)
        .filter_map(|r| {
            serde_json::from_value(r.config).ok().map(|config| Account {
                id: r.id,
                name: r.name,
                config,
            })
        })
        .collect()
}

pub async fn account(state: &AppState, id: Uuid) -> Option<Account> {
    accounts(state).await.into_iter().find(|a| a.id == id)
}

/// The user's own addresses, to tell their messages apart: the accounts' addresses
/// only. Not the addresses mail arrived at: those come from headers anyone can write,
/// and mail from an address there must not pass as the user's. What the user sent from
/// an alias is still theirs, as it's filed as outgoing (Sent).
pub async fn my_addresses(state: &AppState) -> Vec<String> {
    accounts(state)
        .await
        .into_iter()
        .map(|a| a.config.email.to_lowercase())
        .collect()
}

/// Tells clients that mail changed.
pub fn changed(state: &AppState) {
    state.events.publish(Event::MailChanged);
}

/// Removes a disconnected account's mail.
pub async fn forget(state: &AppState, id: Uuid) {
    if let Err(e) = state
        .db
        .call(move |c| store::forget_connection(c, id))
        .await
    {
        tracing::error!("couldn't remove a disconnected account's mail: {e}");
    }
    drafts::forget(state, id).await;
    changed(state);
}

/// Registers the mail tools, the correspondents contact source, the sorting queue and
/// the new-mail notifier.
pub fn install(state: &Arc<AppState>) {
    state.tool_sources.add(Arc::new(tools::MailTools));
    state.people.sources.add(Arc::new(contacts::Correspondents));
    tokio::spawn(triage::run(state.clone()));
    tokio::spawn(notify::run(state.clone()));
    tokio::spawn(outbox::run(state.clone()));
    tokio::spawn(drafts::run(state.clone()));
}

// --- What the panel and the tools use -------------------------------------------------

pub async fn overview(state: &AppState, scope: store::Scope) -> Result<MailOverview, String> {
    let accounts = accounts(state).await;
    let settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    let ids: Vec<Uuid> = accounts.iter().map(|a| a.id).collect();
    let ((needs_reply, important, unread), addresses) = state
        .db
        .call(move |c| {
            let addresses = ids
                .iter()
                .map(|id| store::inbox_addresses(c, *id))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((store::counts(c, &scope)?, addresses))
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(MailOverview {
        accounts: accounts
            .into_iter()
            .zip(addresses)
            .map(|(a, found)| {
                let own = a.config.email.to_lowercase();
                // The account's own address first, even before mail arrives at it.
                let mut addresses: Vec<MailReceivedAddress> = found
                    .into_iter()
                    .map(|(email, threads, unread)| MailReceivedAddress {
                        email,
                        threads,
                        unread,
                    })
                    .collect();
                match addresses.iter().position(|x| x.email == own) {
                    Some(i) => {
                        let first = addresses.remove(i);
                        addresses.insert(0, first);
                    }
                    None => addresses.insert(
                        0,
                        MailReceivedAddress {
                            email: own,
                            threads: 0,
                            unread: 0,
                        },
                    ),
                }
                MailAccount {
                    connection_id: a.id,
                    email: a.config.email,
                    name: a.name,
                    addresses,
                }
            })
            .collect(),
        needs_reply,
        important,
        unread,
        sorting: settings.mail_sorting,
        model_locality: model::locality(state).await,
        sorter: settings.mail_sorter,
        sorter_locality: match settings.mail_sorter {
            mimi_protocol::MailSorter::Model => model::locality(state).await,
            mimi_protocol::MailSorter::Jev => Some(mimi_protocol::Locality::Cloud),
        },
        jev_connected: jev::key(&state.db).await.is_some(),
        folders: state
            .db
            .call(|c| folders::list(c))
            .await
            .map_err(|e| e.to_string())?,
    })
}

pub async fn threads(state: &AppState, query: store::Query) -> Result<Vec<MailThread>, String> {
    let me = my_addresses(state).await;
    state
        .db
        .call(move |c| store::threads(c, &query, &me))
        .await
        .map_err(|e| e.to_string())
}

pub async fn thread(state: &AppState, id: i64) -> Result<Option<MailThreadDetail>, String> {
    let me = my_addresses(state).await;
    state
        .db
        .call(move |c| store::detail(c, id, &me))
        .await
        .map_err(|e| e.to_string())
}

/// The email addresses of a person in the directory.
pub async fn person_addresses(state: &AppState, person: Uuid) -> Result<Vec<String>, String> {
    let p = crate::people::get(state, person)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("That person isn't in your contacts any more.")?;
    Ok(p.handles
        .into_iter()
        .filter(|h| h.channel == mimi_protocol::Channel::Email)
        .map(|h| h.value.to_lowercase())
        .collect())
}

/// Recent conversations with a person, for the People panel.
pub async fn threads_with(
    state: &AppState,
    person: Uuid,
    limit: u32,
) -> Result<Vec<MailThread>, String> {
    let with = person_addresses(state, person).await?;
    if with.is_empty() {
        return Ok(Vec::new());
    }
    threads(
        state,
        store::Query {
            with,
            limit,
            ..Default::default()
        },
    )
    .await
}

/// Marks a conversation read or unread, here and on the server.
pub async fn mark_read(state: &Arc<AppState>, id: i64, read: bool) -> Result<(), String> {
    let locations = state
        .db
        .call(move |c| {
            store::set_thread_seen(c, id, read)?;
            store::locations(c, id)
        })
        .await
        .map_err(|e| e.to_string())?;
    changed(state);
    sync::spawn_flag_change(state.clone(), locations, read);
    Ok(())
}

/// Moves a conversation out of the inbox, here and on the server.
pub async fn archive(state: &Arc<AppState>, id: i64) -> Result<(), String> {
    let locations = state
        .db
        .call(move |c| store::locations(c, id))
        .await
        .map_err(|e| e.to_string())?;
    let inbox: Vec<_> = locations
        .into_iter()
        .filter(|(_, _, folder, _)| folder == "inbox")
        .collect();
    if inbox.is_empty() {
        return Ok(());
    }
    sync::archive_on_server(state, &inbox).await?;
    state
        .db
        .call(move |c| store::archive_locally(c, id))
        .await
        .map_err(|e| e.to_string())?;
    for (conn, ..) in &inbox {
        state.mail.poke(*conn);
    }
    changed(state);
    Ok(())
}

/// Largest attachment Mimi fetches.
const MAX_ATTACHMENT: usize = 50 * 1024 * 1024;

/// One attachment of a stored message, fetched from the server on request.
pub async fn attachment(
    state: &AppState,
    message: i64,
    index: usize,
) -> Result<parse::Attachment, String> {
    let raw = source(state, message).await?;
    let found = parse::attachment(&raw, index).ok_or("That attachment couldn't be found.")?;
    if found.data.len() > MAX_ATTACHMENT {
        return Err(
            "That attachment is too large to open here. Open it in your usual mail app.".to_owned(),
        );
    }
    Ok(found)
}

/// A stored message's full source, fetched from the server.
async fn source(state: &AppState, message: i64) -> Result<Vec<u8>, String> {
    let (conn, mailbox, uid) = state
        .db
        .call(move |c| store::location(c, message))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("That email isn't here any more.")?;
    let account = account(state, conn)
        .await
        .ok_or("That account isn't connected any more.")?;
    sync::fetch_source(&account, &mailbox, uid)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "That email isn't on the server any more. It may have been moved or deleted.".to_owned()
        })
}

/// Moves a conversation to the Trash on the server (every copy: inbox, sent, archive)
/// and forgets it here.
pub async fn delete(state: &Arc<AppState>, id: i64) -> Result<(), String> {
    let locations = state
        .db
        .call(move |c| store::locations(c, id))
        .await
        .map_err(|e| e.to_string())?;
    if locations.is_empty() {
        return Err("That conversation isn't here any more.".to_owned());
    }
    sync::trash_on_server(state, &locations).await?;
    state
        .db
        .call(move |c| store::forget_thread(c, id))
        .await
        .map_err(|e| e.to_string())?;
    for (conn, ..) in &locations {
        state.mail.poke(*conn);
    }
    changed(state);
    Ok(())
}

/// Sends a message the user wrote or approved, and files it in Sent.
pub async fn send(state: &Arc<AppState>, draft: MailDraft) -> Result<(), String> {
    let ready = prepare(state, &draft).await?;
    deliver(state, ready).await.map_err(|e| e.message)
}

/// A message checked and built, ready to hand to the server.
pub struct Prepared {
    pub account: Account,
    /// The address it's from.
    pub from: String,
    main: String,
    built: smtp::Built,
}

/// Why a message didn't go. `retry`: the server couldn't be reached, so trying again
/// later may work; anything else needs the user.
#[derive(Debug, Clone)]
pub struct Undelivered {
    pub message: String,
    pub retry: bool,
}

/// Checks a draft and builds its message without sending anything: the account, the
/// From address, every file (fetched), the recipients and the sizes.
pub async fn prepare(state: &AppState, draft: &MailDraft) -> Result<Prepared, String> {
    let Sender {
        account,
        from,
        main,
        reply,
    } = sender(state, draft).await?;
    // A forward carries the original's attachments along.
    let mut attachments = match draft.forward_of {
        Some(message) => forwarded(state, message).await?,
        None => Vec::new(),
    };
    // Pictures placed in the text (with a content id) go inside the formatted message.
    let (pictures, files): (Vec<_>, Vec<_>) = draft
        .attachments
        .iter()
        .cloned()
        .partition(|a| a.content_id.is_some());
    attachments.extend(attached(state, &files).await?);
    let inline: Vec<smtp::Inline> = attached(state, &pictures)
        .await?
        .into_iter()
        .zip(pictures)
        .map(|(file, a)| smtp::Inline {
            content_id: a.content_id.unwrap_or_default(),
            file,
        })
        .collect();
    let size = attachments.iter().chain(inline.iter().map(|i| &i.file));
    if size.map(|f| f.data.len()).sum::<usize>() > smtp::MAX_ATTACHMENTS {
        return Err(match draft.forward_of {
            Some(_) if draft.attachments.is_empty() => {
                "The attachments are too large to forward from here.".to_owned()
            }
            _ => "Attachments can add up to 20 MB in one email.".to_owned(),
        });
    }
    let built = smtp::build(&from, draft, reply.as_ref(), &attachments, &inline)?;
    Ok(Prepared {
        account,
        from,
        main,
        built,
    })
}

/// Who a draft goes from: its account, the address, the account's own address, and the
/// conversation it answers.
pub(crate) struct Sender {
    pub account: Account,
    pub from: String,
    pub main: String,
    pub reply: Option<smtp::ReplyHeaders>,
}

/// Works out who a draft goes from: the account asked for (else the replied-to
/// conversation's, else the first), and the address asked for, else the one the
/// conversation arrived at, else the account's.
pub(crate) async fn sender(state: &AppState, draft: &MailDraft) -> Result<Sender, String> {
    let accounts = accounts(state).await;
    let reply = match draft.reply_to {
        Some(thread) => smtp::reply_headers(state, thread).await?,
        None => None,
    };
    let account = draft
        .connection_id
        .or(reply.as_ref().map(|r| r.connection_id))
        .and_then(|id| accounts.iter().find(|a| a.id == id))
        .or(accounts.first())
        .ok_or("No email account is connected.")?;
    // From the address asked for, else the one the conversation arrived at: only ever
    // one of this account's own, and only an alias that's plainly the user's (on their
    // own domain, or a +tag), whatever a message's headers claimed.
    let (conn, main) = (account.id, account.config.email.to_lowercase());
    let own = state
        .db
        .call(move |c| store::account_addresses(c, conn))
        .await
        .map_err(|e| e.to_string())?;
    let is_own =
        |a: &str| a == main || (own.iter().any(|o| o == a) && parse::is_own_address(a, &main));
    let from = match draft.from.as_deref().map(|f| f.trim().to_lowercase()) {
        Some(f) if !f.is_empty() && !is_own(&f) => {
            return Err(format!("{f} isn't one of this account's addresses."));
        }
        Some(f) if !f.is_empty() => f,
        _ => reply
            .as_ref()
            .and_then(|r| r.received_on.clone())
            .filter(|r| is_own(r))
            .unwrap_or_else(|| main.clone()),
    };
    Ok(Sender {
        account: account.clone(),
        from,
        main,
        reply,
    })
}

/// Hands a prepared message to the account's server and files it in Sent.
pub async fn deliver(state: &AppState, ready: Prepared) -> Result<(), Undelivered> {
    let Prepared {
        account,
        from,
        main,
        built,
    } = ready;
    smtp::deliver(&account.config, &built).await.map_err(|e| {
        let retry = matches!(e, MailError::Unreachable(_));
        let e = e.to_string();
        let message = if from == main {
            e
        } else {
            format!(
                "{e} Your mail service may not allow sending from {from}: add it as a sending \
                 address (an identity) in its settings, or send from {main}."
            )
        };
        Undelivered { message, retry }
    })?;
    tracing::info!(connection = %account.id, "sent an email");
    sync::file_sent(&account, built.formatted).await;
    state.mail.poke(account.id);
    Ok(())
}

/// The attachments of a message being forwarded, fetched from the server.
pub(crate) async fn forwarded(
    state: &AppState,
    message: i64,
) -> Result<Vec<parse::Attachment>, String> {
    Ok(parse::attachments(&source(state, message).await?))
}

/// The files attached to a draft: the user's own, decoded, and those attached by
/// reference (`NewMailAttachment::source`), fetched: an email's attachment from the
/// server, a chat photo from the encrypted database. Each goes under a plain file name.
pub(crate) async fn attached(
    state: &AppState,
    files: &[NewMailAttachment],
) -> Result<Vec<parse::Attachment>, String> {
    use base64::Engine;
    // Each email is fetched once, however many of its files go along.
    let mut fetched: HashMap<i64, Vec<parse::Attachment>> = HashMap::new();
    let mut out = Vec::new();
    for f in files {
        let (name, mime, data) = match &f.source {
            None => {
                let data = base64::engine::general_purpose::STANDARD
                    .decode(f.data.trim())
                    .map_err(|_| {
                        format!(
                            "“{}” couldn't be read. Try attaching it again.",
                            file_name(&f.name)
                        )
                    })?;
                (f.name.clone(), f.mime.clone().unwrap_or_default(), data)
            }
            Some(MailAttachmentSource::Email { message, index, .. }) => {
                if !fetched.contains_key(message) {
                    let files = parse::attachments(&source(state, *message).await?);
                    fetched.insert(*message, files);
                }
                let found = fetched[message].get(*index as usize).ok_or_else(|| {
                    format!("“{}” isn't in that email any more.", file_name(&f.name))
                })?;
                (
                    found.name.clone(),
                    found.content_type.clone(),
                    found.data.clone(),
                )
            }
            Some(MailAttachmentSource::Chat { attachment, .. }) => {
                let (_, meta, data) = chat_file(state, *attachment).await?.ok_or_else(|| {
                    format!(
                        "“{}” isn't in the chat any more, so it can't be attached.",
                        file_name(&f.name)
                    )
                })?;
                (meta.name, meta.mime, data)
            }
        };
        let content_type = Some(mime.trim())
            .filter(|m| m.parse::<mime::Mime>().is_ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        out.push(parse::Attachment {
            name: file_name(&name),
            content_type,
            data,
        });
    }
    Ok(out)
}

/// A file's name without any folder part or control characters.
fn file_name(raw: &str) -> String {
    let name: String = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    match name.trim() {
        "" => "Attachment".to_owned(),
        n => n.to_owned(),
    }
}

/// A photo sent in one of the owner's chats: the conversation it's in, what it is and
/// its content. `None` when it's gone, or was sent in a trusted person's conversation,
/// which is theirs and never the owner's to send on.
pub async fn chat_file(
    state: &AppState,
    id: Uuid,
) -> Result<Option<(Uuid, mimi_protocol::Attachment, Vec<u8>)>, String> {
    let conversation = crate::attachments::conversation_of(&state.db, id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(conversation) = conversation.filter(|c| !state.access.is_guest_conversation(*c))
    else {
        return Ok(None);
    };
    let found = crate::attachments::get(&state.db, id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(found.map(|(meta, data)| (conversation, meta, data)))
}

/// Parses a view name from a query string.
pub fn parse_view(raw: &str) -> Option<MailBox> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned())).ok()
}
