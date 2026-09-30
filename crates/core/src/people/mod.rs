//! The people the user knows, unified across the places they come from (address
//! books today; Telegram and other channels through [`ContactSource`] later), plus
//! the people they added themselves.
//!
//! One person has many handles (phone, email, Telegram…), each remembering its
//! source. Cards unify when they share a phone number, email or username; never on
//! name alone. Same-name people are offered as "possible duplicates" instead.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures::future::BoxFuture;
use mimi_protocol::{Channel, Event, Person, PersonSummary};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::db::DbError;
use crate::{AppState, now_ms};

pub mod carddav;
pub mod mentions;
pub mod merge;
pub mod normalize;
pub mod store;
pub mod tools;
pub mod vcard;

/// A contact card as a source provides it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactCard {
    /// Stable id of the card within its source (a vCard UID, a Telegram user id…).
    pub record: String,
    pub name: String,
    pub nickname: Option<String>,
    pub handles: Vec<CardHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardHandle {
    pub channel: Channel,
    pub value: String,
    pub label: Option<String>,
}

/// Everything one source currently holds, or why it couldn't be read. A failed read
/// keeps what was imported before rather than deleting it.
pub struct SourceBatch {
    /// The connection the cards come from.
    pub source: String,
    pub cards: Result<Vec<ContactCard>, String>,
}

/// Something that provides contacts: address books, a Telegram account…
///
/// Register one with `state.people.sources.add(...)` at startup. Each sync asks every
/// source for all its cards, per connection; the directory works out what changed.
pub trait ContactSource: Send + Sync {
    fn fetch<'a>(&'a self, state: &'a AppState) -> BoxFuture<'a, Vec<SourceBatch>>;
}

#[derive(Default)]
pub struct ContactSources(RwLock<Vec<Arc<dyn ContactSource>>>);

impl ContactSources {
    pub fn add(&self, source: Arc<dyn ContactSource>) {
        self.0
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(source);
    }

    fn all(&self) -> Vec<Arc<dyn ContactSource>> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The directory's live state: its sources and the sync trigger.
#[derive(Default)]
pub struct People {
    pub sources: ContactSources,
    wake: Notify,
}

impl People {
    /// Asks the background sync to run soon.
    pub fn sync_soon(&self) {
        self.wake.notify_one();
    }
}

/// How often contacts refresh on their own.
const SYNC_EVERY: Duration = Duration::from_secs(30 * 60);

/// Keeps the directory in step with its sources: at startup, periodically, when
/// connections change, and on request.
pub fn spawn_sync(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut events = state.events.subscribe();
        loop {
            sync_all(&state).await;
            let wait = tokio::time::sleep(SYNC_EVERY);
            tokio::pin!(wait);
            loop {
                tokio::select! {
                    _ = &mut wait => break,
                    _ = state.people.wake.notified() => break,
                    event = events.recv() => match event {
                        Ok(Event::ConnectionsChanged { .. }) => {
                            // Let a new connection settle, then read its address books.
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        _ => {}
                    },
                }
            }
        }
    });
}

/// One pass over every source. Publishes `PeopleChanged` if anything changed.
pub async fn sync_all(state: &AppState) {
    let mut changed = state
        .db
        .call(store::purge_removed_sources)
        .await
        .unwrap_or(false);
    for source in state.people.sources.all() {
        for batch in source.fetch(state).await {
            match batch.cards {
                Ok(cards) => {
                    let src = batch.source.clone();
                    match state
                        .db
                        .call(move |c| store::sync_source(c, &src, &cards, now_ms()))
                        .await
                    {
                        Ok(c) => changed |= c,
                        Err(e) => tracing::warn!("saving contacts failed: {e}"),
                    }
                }
                Err(e) => tracing::warn!(source = %batch.source, "reading contacts failed: {e}"),
            }
        }
    }
    if changed {
        state.events.publish(Event::PeopleChanged);
        // Someone a sync dropped takes what they had with the assistant along.
        crate::access::tidy(state).await;
    }
}

/// Plain-language names of every connection, for "where this came from".
pub async fn source_names(state: &AppState) -> Result<store::SourceNames, DbError> {
    Ok(crate::connections::store::list(&state.db)
        .await?
        .into_iter()
        .map(|c| (c.id.to_string(), c.name))
        .collect())
}

/// A person by id. An id merged into someone else finds that person (whose `id`
/// then differs from the one asked for).
pub async fn get(state: &AppState, id: uuid::Uuid) -> Result<Option<Person>, DbError> {
    let names = source_names(state).await?;
    state
        .db
        .call(move |c| match merge::resolve(c, id)? {
            Some(id) => store::get(c, id, &names),
            None => Ok(None),
        })
        .await
}

/// People matching `query` (name, nickname, number, address…), best first. An empty
/// query lists everyone alphabetically.
pub async fn search(
    state: &AppState,
    query: &str,
    limit: usize,
) -> Result<Vec<PersonSummary>, DbError> {
    let everyone = state.db.call(|c| store::all(c)).await?;
    Ok(rank(everyone, query, limit))
}

pub(crate) fn rank(everyone: Vec<store::Indexed>, query: &str, limit: usize) -> Vec<PersonSummary> {
    let query = fold(query.trim());
    if query.is_empty() {
        return everyone
            .into_iter()
            .take(limit)
            .map(|p| p.summary)
            .collect();
    }
    let digits: String = query.chars().filter(char::is_ascii_digit).collect();
    let mut scored: Vec<(u32, PersonSummary)> = everyone
        .into_iter()
        .filter_map(|p| {
            let by_name = score(&query, &fold(&p.summary.name));
            let by_nick = p
                .summary
                .nickname
                .as_deref()
                .and_then(|n| score(&query, &fold(n)));
            let by_handle = p
                .values
                .iter()
                .any(|v| {
                    v.contains(&query)
                        || (digits.len() >= 4
                            && v.chars()
                                .filter(char::is_ascii_digit)
                                .collect::<String>()
                                .contains(&digits))
                })
                .then_some(40);
            let best = by_name.max(by_nick).max(by_handle)?;
            Some((best, p.summary))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    scored.into_iter().take(limit).map(|(_, p)| p).collect()
}

/// How well `query` matches `text` (both folded): start of text, start of a word,
/// anywhere, or letters in order.
pub(crate) fn score(query: &str, text: &str) -> Option<u32> {
    if text.starts_with(query) {
        return Some(100);
    }
    if text.split_whitespace().any(|w| w.starts_with(query)) {
        return Some(80);
    }
    if text.contains(query) {
        return Some(60);
    }
    let mut chars = text.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|q| chars.any(|t| t == q))
        .then_some(20)
}

/// Lowercase without accents, for matching.
pub(crate) fn fold(s: &str) -> String {
    normalize::name_key(s)
}

/// Channels in the order people expect to see them.
pub fn channel_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Phone => "phone",
        Channel::Email => "email",
        Channel::Telegram => "Telegram",
        Channel::Signal => "Signal",
        Channel::Whatsapp => "WhatsApp",
        Channel::Matrix => "Matrix",
        Channel::Other => "other",
    }
}

/// A person as the model reads them: one line per way to reach them.
pub fn describe_for_model(p: &Person) -> String {
    let mut by_channel: Vec<(Channel, Vec<String>)> = Vec::new();
    for h in &p.handles {
        let text = match &h.label {
            Some(l) => format!("{} ({l})", h.value),
            None => h.value.clone(),
        };
        match by_channel.iter_mut().find(|(c, _)| *c == h.channel) {
            Some((_, values)) => values.push(text),
            None => by_channel.push((h.channel, vec![text])),
        }
    }
    let mut out = p.name.clone();
    if let Some(n) = &p.nickname {
        out.push_str(&format!(" (\"{n}\")"));
    }
    if by_channel.is_empty() {
        out.push_str(": no way to reach them is known");
    }
    for (channel, values) in by_channel {
        out.push_str(&format!(
            "; {}: {}",
            channel_label(channel),
            values.join(", ")
        ));
    }
    out
}

/// Registers the built-in contact sources and assistant tools.
pub fn install(state: &Arc<AppState>) {
    state.people.sources.add(Arc::new(carddav::AddressBooks));
    state.tool_sources.add(Arc::new(tools::PeopleTools));
    spawn_sync(state.clone());
}

/// Full details of several people, e.g. search hits for the assistant.
pub async fn details_many(state: &AppState, ids: Vec<uuid::Uuid>) -> Result<Vec<Person>, DbError> {
    let names = source_names(state).await?;
    state
        .db
        .call(move |c| {
            let mut out = Vec::new();
            for id in ids {
                if let Some(p) = store::get(c, id, &names)? {
                    out.push(p);
                }
            }
            Ok(out)
        })
        .await
}

#[cfg(test)]
mod tests;
