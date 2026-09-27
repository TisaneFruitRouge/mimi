//! # mentions: suggesting conversations and emails in the chat's picker, and telling
//! the model what a mentioned one says. Email is written by others, so it's quoted as
//! data, with anything that could pass for markup taken out.

use chrono::{Local, TimeZone};
use mimi_protocol::{MailMessage, MailThread, MentionCandidate, MentionKind};

use super::{model::clip, parse::strip_quoted, store};
use crate::AppState;

/// Most text of a mentioned conversation the model gets (newest messages first).
const THREAD_BUDGET: usize = 3_000;
/// Most text of one message in a mentioned conversation.
const MESSAGE_CLIP: usize = 1_500;
/// Most text of a single mentioned email.
const EMAIL_CLIP: usize = 3_000;

/// Suggestions for `query`: recent mail when nothing is typed, else matching
/// conversations (single emails as emails), then matching emails inside longer
/// conversations.
pub async fn candidates(state: &AppState, query: &str, limit: usize) -> Vec<MentionCandidate> {
    let me = super::my_addresses(state).await;
    let query = query.trim().to_owned();
    state
        .db
        .call(move |c| {
            let q = store::Query {
                search: (!query.is_empty()).then(|| query.clone()),
                limit: limit as u32,
                ..Default::default()
            };
            let mut out = Vec::new();
            let mut shown = std::collections::HashSet::new();
            for t in store::threads(c, &q, &me)? {
                let messages = store::messages(c, t.id, &me)?;
                match messages.as_slice() {
                    [only] => {
                        shown.insert(only.id);
                        out.push(email_candidate(&t.subject, only));
                    }
                    _ => out.push(thread_candidate(&t)),
                }
            }
            if let Some(fts) = store::fts_query(&query) {
                for (id, _) in store::search_messages(c, &fts, 20)? {
                    if out.len() >= limit * 2 {
                        break;
                    }
                    if !shown.insert(id) {
                        continue;
                    }
                    if let Some((_, subject, m)) = store::message(c, id, &me)? {
                        out.push(email_candidate(&subject, &m));
                    }
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default()
}

fn thread_candidate(t: &MailThread) -> MentionCandidate {
    let who = t
        .participants
        .first()
        .map(|p| p.name.clone().unwrap_or_else(|| p.email.clone()));
    let mut detail = vec![format!("{} messages", t.message_count), day(t.last_at)];
    if let Some(who) = who {
        detail.insert(0, who);
    }
    MentionCandidate {
        kind: MentionKind::MailThread,
        id: t.id.to_string(),
        label: label(&t.subject),
        detail: Some(detail.join(" · ")),
        channels: Vec::new(),
        starts_at: None,
    }
}

fn email_candidate(subject: &str, m: &MailMessage) -> MentionCandidate {
    let who = if m.from_me {
        match m.to.first() {
            Some(to) => format!("To {}", to.name.as_deref().unwrap_or(&to.email)),
            None => "From you".to_owned(),
        }
    } else {
        format!("From {}", m.from.name.as_deref().unwrap_or(&m.from.email))
    };
    MentionCandidate {
        kind: MentionKind::MailMessage,
        id: m.id.to_string(),
        label: label(subject),
        detail: Some(format!("{who} · {}", day(m.date))),
        channels: Vec::new(),
        starts_at: None,
    }
}

/// A subject short enough to sit in a message as a tag.
fn label(subject: &str) -> String {
    let s = subject.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = if s.is_empty() {
        "(no subject)".to_owned()
    } else {
        s
    };
    clip(&s, 60)
}

fn day(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%-d %b").to_string())
        .unwrap_or_default()
}

/// Untrusted text for the model: angle brackets become look-alikes, so an email can't
/// close the block it's quoted in and pose as the user or the system.
fn quote(text: &str) -> String {
    text.replace('<', "‹").replace('>', "›")
}

fn from_line(m: &MailMessage) -> String {
    if m.from_me {
        "the user".to_owned()
    } else {
        quote(&super::tools::address(&m.from))
    }
}

/// What a mentioned conversation or email says, as lines for the `<mentioned>` block,
/// or `None` if it's gone (archived away, or older than what's kept).
pub async fn describe(
    state: &AppState,
    kind: MentionKind,
    id: &str,
    label: &str,
) -> Option<String> {
    let id: i64 = id.parse().ok()?;
    let me = super::my_addresses(state).await;
    let label = quote(label);
    match kind {
        MentionKind::MailThread => {
            let detail = state
                .db
                .call(move |c| store::detail(c, id, &me))
                .await
                .ok()??;
            let t = &detail.thread;
            let with: Vec<String> = t
                .participants
                .iter()
                .map(|p| quote(&super::tools::address(p)))
                .collect();
            // Newest first within the budget, shown oldest first.
            let mut parts = Vec::new();
            let mut used = 0;
            for m in detail.messages.iter().rev() {
                let text = quote(&clip(&strip_quoted(&m.body), MESSAGE_CLIP));
                if used + text.len() > THREAD_BUDGET && !parts.is_empty() {
                    break;
                }
                used += text.len();
                parts.push(format!(
                    "  [{} wrote on {}]\n  {}",
                    from_line(m),
                    super::tools::local_time(m.date),
                    text.replace('\n', "\n  ")
                ));
            }
            parts.reverse();
            let skipped = detail.messages.len() - parts.len();
            let mut out = format!(
                "- #{label} is an email conversation (conversation id: {id}, for the mail tools), \
                 subject \"{}\", with {}, {} message(s)",
                quote(&t.subject),
                if with.is_empty() {
                    "no one else".to_owned()
                } else {
                    with.join(", ")
                },
                t.message_count,
            );
            if let Some(on) = &t.received_on {
                out.push_str(&format!(", received at {on}"));
            }
            if skipped > 0 {
                out.push_str(&format!(" ({skipped} earlier not shown)"));
            }
            out.push_str(":\n");
            out.push_str(&parts.join("\n"));
            Some(out)
        }
        MentionKind::MailMessage => {
            let (thread, subject, m) = state
                .db
                .call(move |c| store::message(c, id, &me))
                .await
                .ok()??;
            let to: Vec<String> =
                m.to.iter()
                    .map(|a| quote(&super::tools::address(a)))
                    .collect();
            let warning = if m.suspicious {
                format!(" Warning: {}", super::tools::SUSPICIOUS)
            } else {
                String::new()
            };
            Some(format!(
                "- #{label} is one email (in conversation id: {thread}, for the mail tools), \
                 subject \"{}\", from {} to {}, sent {}.{warning}\n  {}",
                quote(&subject),
                from_line(&m),
                if to.is_empty() {
                    "undisclosed recipients".to_owned()
                } else {
                    to.join(", ")
                },
                super::tools::local_time(m.date),
                quote(&clip(&strip_quoted(&m.body), EMAIL_CLIP)).replace('\n', "\n  ")
            ))
        }
        MentionKind::Person | MentionKind::Event => None,
    }
}
