//! Learning in the background: once a conversation goes quiet, the active model reads
//! what the user said since last time and files lasting facts into the profile and the
//! library, merging with what's already known.
//!
//! Only the user's own messages are a source of facts; the assistant's replies are given
//! as context only. Messages where the user asks not to remember something are left out
//! entirely, and anything that looks like a secret is dropped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mimi_protocol::{Event, MemorySource, MessageRole, MessageStatus};
use serde::Deserialize;
use uuid::Uuid;

use super::recall::fts_query;
use super::{
    PROFILE_LIMIT, PROFILE_PATH, add_facts, cap_profile, looks_secret, normalize_path,
    remove_facts, store,
};
use crate::AppState;
use crate::providers::{self, ChatMessage, ChatOptions, Role};

/// How long a conversation must be quiet before it's learned from.
pub const QUIET_ENV: &str = "MIMI_MEMORY_QUIET_SECS";
const DEFAULT_QUIET: Duration = Duration::from_secs(120);

/// Most characters of conversation given to one learning pass.
const EXCERPT_BUDGET: usize = 6000;
/// Longest fact accepted from a learning pass.
const MAX_FACT: usize = 240;

/// Conversations waiting to be learned from, and when.
#[derive(Default)]
pub struct Learner {
    due: Mutex<HashMap<Uuid, Instant>>,
    wake: tokio::sync::Notify,
}

fn quiet() -> Duration {
    std::env::var(QUIET_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_QUIET)
}

impl Learner {
    /// (Re)starts the quiet timer for a conversation.
    pub fn schedule(&self, conversation_id: Uuid) {
        self.due
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(conversation_id, Instant::now() + quiet());
        self.wake.notify_one();
    }

    fn take_due(&self) -> (Vec<Uuid>, Option<Instant>) {
        let mut due = self.due.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let ready: Vec<Uuid> = due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(id, _)| *id)
            .collect();
        for id in &ready {
            due.remove(id);
        }
        (ready, due.values().min().copied())
    }
}

/// Runs forever: learns from conversations as they go quiet. Conversations with
/// unread messages from before a restart are picked up at start.
pub async fn run(state: Arc<AppState>) {
    if let Ok(conversations) = crate::chat::store::list_conversations(&state.db).await {
        for c in conversations {
            let until = store::learned_until(&state.db, c.id).await.unwrap_or(0);
            if c.updated_at > until {
                state.learner.schedule(c.id);
            }
        }
    }
    loop {
        let (ready, next) = state.learner.take_due();
        for id in ready {
            if state.generations.is_running(id) {
                state.learner.schedule(id);
                continue;
            }
            match learn_from(&state, id).await {
                Ok(n) => {
                    tracing::info!(conversation = %id, notes_changed = n, "learning pass done")
                }
                Err(e) => {
                    tracing::warn!(conversation = %id, "learning from a conversation failed: {e}")
                }
            }
        }
        let sleep = next
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        tokio::select! {
            _ = tokio::time::sleep(sleep) => {}
            _ = state.learner.wake.notified() => {}
        }
    }
}

/// What the model proposes to change.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Plan {
    profile: Changes,
    notes: Vec<NotePlan>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Changes {
    add: Vec<String>,
    remove: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NotePlan {
    path: String,
    add: Vec<String>,
    remove: Vec<String>,
}

/// Learns from what's new in a conversation. Returns how many notes changed.
pub async fn learn_from(state: &Arc<AppState>, conversation_id: Uuid) -> Result<usize, String> {
    let db = &state.db;
    let settings = crate::settings::load(db).await.map_err(|e| e.to_string())?;
    let messages = crate::chat::store::messages(db, conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let since = store::learned_until(db, conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let newest = messages.iter().map(|m| m.created_at).max().unwrap_or(since);
    let fresh: Vec<_> = messages
        .iter()
        .filter(|m| m.created_at > since && m.status != MessageStatus::Streaming)
        .collect();
    let mark_read = || store::set_learned_until(db, conversation_id, newest);

    // Paused learning: nothing said meanwhile is remembered later either.
    if !settings.memory_learning || !fresh.iter().any(|m| m.role == MessageRole::User) {
        mark_read().await.map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let Some(excerpt) = excerpt(&fresh) else {
        mark_read().await.map_err(|e| e.to_string())?;
        return Ok(0);
    };

    let Some(model) = settings.default_model else {
        return Err("no model is set up".to_owned());
    };
    let provider = providers::store::get(db, model.provider_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("the model's source is gone")?;
    let client = providers::chat_client(state, &provider, &model.model).await?;

    let profile = store::profile(db).await.map_err(|e| e.to_string())?;
    let known = store::list(db).await.map_err(|e| e.to_string())?;
    let user_text: String = fresh
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let related = store::search(db, fts_query(&user_text), 3)
        .await
        .map_err(|e| e.to_string())?;

    let mut context = String::new();
    context.push_str("## Current profile (profile.md)\n");
    context.push_str(if profile.trim().is_empty() {
        "(empty)"
    } else {
        profile.trim()
    });
    context.push_str("\n\n## Existing notes\n");
    if known.is_empty() {
        context.push_str("(none)\n");
    }
    for n in known.iter().take(60) {
        context.push_str(&format!("- {}\n", n.path));
    }
    for n in &related {
        context.push_str(&format!("\n## {}\n{}\n", n.path, n.body.trim()));
    }
    context.push_str("\n## New conversation\n");
    context.push_str(&excerpt);

    let prompt = vec![
        ChatMessage::text(Role::System, INSTRUCTIONS),
        ChatMessage::text(Role::User, context),
    ];
    // No thinking: a learning pass is bookkeeping, and reasoning models otherwise spend
    // minutes deliberating before a small JSON answer.
    let reply = tokio::time::timeout(
        Duration::from_secs(300),
        client.complete(&model.model, &prompt, ChatOptions::QUICK),
    )
    .await
    .map_err(|_| "the model took too long".to_owned())?
    .map_err(|e| e.to_string())?;
    let plan = parse_plan(&reply).ok_or("the model's answer wasn't a usable plan")?;
    let changes = apply(state, conversation_id, plan).await?;
    if changes > 0 {
        // People the user @-mentioned settle who a note is about ("Léa" when there
        // are two).
        let hints: Vec<Uuid> = fresh
            .iter()
            .flat_map(|m| super::link::mentioned(&m.mentions))
            .collect();
        if let Err(e) = super::link::relink(state, &hints).await {
            tracing::warn!("linking notes to people failed: {e}");
        }
    }
    mark_read().await.map_err(|e| e.to_string())?;
    if changes > 0 {
        state.events.publish(Event::MemoryChanged);
    }
    Ok(changes)
}

/// The learning pass runs without the model's thinking (much faster), so the answer
/// starts with a short `facts` list instead: writing the facts down first stops models
/// like Qwen3 8B from skipping people or details. `facts` itself isn't used.
const INSTRUCTIONS: &str = "You maintain a private memory about the user of a personal assistant. \
Read the new conversation and decide what lasting facts to remember.\n\
Rules:\n\
- Only facts the USER stated themselves, in their own messages. The assistant's replies are context only.\n\
- Only things still true in a few weeks: family, friends, colleagues, birthdays, where they live or work, \
routines, likes and dislikes, health they chose to share. Not one-off tasks, questions, today's plans or small talk.\n\
- Never passwords, codes, account or card numbers.\n\
- Short facts: \"The user ...\" or \"<Name> ...\".\n\
- First list in \"facts\" every lasting fact the user stated in the new conversation (not what is already \
known), then file each one:\n\
  - profile: only the few key facts about the user (name, city, language, job, household).\n\
  - notes: one note per other person, people/<first-name>.md (never one named after the user), and one per \
topic for the user's own details: preferences/food.md, preferences/drinks.md, habits/mornings.md, \
habits/commute.md, places/home.md, work/job.md, interests/music.md, health/health.md. Reuse existing paths.\n\
  - If a new fact contradicts a known one, put the known one, word for word, in \"remove\". Keep known facts \
that are still true.\n\
- If there is nothing worth remembering, return empty lists.\n\
Example: \"I'm Ana, a teacher in Porto. My brother Rui loves chess and moved to Lisbon. I cycle to school.\" gives\n\
{\"facts\": [\"The user's name is Ana\", \"The user is a teacher in Porto\", \"Rui is the user's brother\", \
\"Rui loves chess\", \"Rui lives in Lisbon\", \"The user cycles to school\"], \
\"profile\": {\"add\": [\"The user's name is Ana\", \"The user is a teacher in Porto\"], \"remove\": []}, \
\"notes\": [{\"path\": \"people/rui.md\", \"add\": [\"Rui is the user's brother\", \"Rui loves chess\", \
\"Rui lives in Lisbon\"], \"remove\": [\"Rui lives in Porto\"]}, \
{\"path\": \"habits/commute.md\", \"add\": [\"The user cycles to school\"], \"remove\": []}]}\n\
Reply with JSON only, in that shape.";

/// The conversation as the learning pass sees it, or `None` if nothing is left once
/// messages the user asked not to be remembered are dropped.
fn excerpt(messages: &[&mimi_protocol::Message]) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut skip_reply = false;
    for m in messages {
        match m.role {
            MessageRole::User => {
                skip_reply = asks_not_to_remember(&m.content);
                if !skip_reply {
                    parts.push(format!("USER: {}", clip(&m.content, 800)));
                }
            }
            MessageRole::Assistant if !skip_reply && !m.content.is_empty() => {
                parts.push(format!(
                    "ASSISTANT (context only): {}",
                    clip(&m.content, 200)
                ));
            }
            _ => {}
        }
    }
    if !parts.iter().any(|p| p.starts_with("USER:")) {
        return None;
    }
    // Keep the newest parts that fit.
    let mut kept = Vec::new();
    let mut used = 0;
    for p in parts.iter().rev() {
        if used + p.len() > EXCERPT_BUDGET {
            break;
        }
        used += p.len();
        kept.push(p.clone());
    }
    kept.reverse();
    Some(kept.join("\n"))
}

fn asks_not_to_remember(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "don't remember",
        "do not remember",
        "dont remember",
        "don't save",
        "do not save",
        "off the record",
        "keep this between us",
        "forget i said",
        "ne retiens pas",
        "ne mémorise pas",
        "oublie ça",
        "nicht merken",
        "merk dir das nicht",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

/// The first JSON object in the reply, tolerating code fences and chatter around it.
fn parse_plan(reply: &str) -> Option<Plan> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    serde_json::from_str(reply.get(start..=end)?).ok()
}

fn usable(facts: Vec<String>) -> Vec<String> {
    facts
        .into_iter()
        .map(|f| f.trim().trim_start_matches("- ").trim().to_owned())
        .filter(|f| !f.is_empty() && f.chars().count() <= MAX_FACT && !looks_secret(f))
        .collect()
}

/// Writes the plan's changes. Returns how many notes changed.
async fn apply(state: &Arc<AppState>, conversation_id: Uuid, plan: Plan) -> Result<usize, String> {
    let db = &state.db;
    let mut changes = 0;

    let mut edits: Vec<(String, Vec<String>, Vec<String>)> = vec![(
        PROFILE_PATH.to_owned(),
        usable(plan.profile.add),
        plan.profile.remove,
    )];
    for n in plan.notes.into_iter().take(12) {
        let Some(path) = normalize_path(&n.path) else {
            continue;
        };
        edits.push((path, usable(n.add), n.remove));
    }

    for (path, add, remove) in edits {
        if add.is_empty() && remove.is_empty() {
            continue;
        }
        let current = store::get(db, &path).await.map_err(|e| e.to_string())?;
        let before = current.as_ref().map(|n| n.body.clone()).unwrap_or_default();
        let (mut body, _) = remove_facts(&before, &remove);
        if path == PROFILE_PATH {
            // Add what fits; the profile never grows past its limit.
            for fact in add {
                let (next, added) = add_facts(&body, std::slice::from_ref(&fact));
                if added > 0 && next.chars().count() <= PROFILE_LIMIT {
                    body = next;
                }
            }
            body = cap_profile(&body);
        } else {
            body = add_facts(&body, &add).0;
            if body.chars().count() > super::NOTE_LIMIT {
                continue;
            }
        }
        if body.trim() == before.trim() {
            continue;
        }
        let title = current.as_ref().map(|n| n.title.clone());
        store::put(
            db,
            &path,
            title.as_deref(),
            &body,
            MemorySource::Learned,
            Some(conversation_id),
        )
        .await
        .map_err(|e| e.to_string())?;
        changes += 1;
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_are_read_leniently() {
        let reply = "Sure! ```json\n{\"profile\": {\"add\": [\"The user's name is Vincent\"]}, \"notes\": [{\"path\": \"People/Sam\", \"add\": [\"Sam is the user's brother\"]}]}\n```";
        let plan = parse_plan(reply).unwrap();
        assert_eq!(plan.profile.add, ["The user's name is Vincent"]);
        assert!(plan.profile.remove.is_empty());
        assert_eq!(plan.notes[0].path, "People/Sam");
        assert!(parse_plan("nothing to remember").is_none());
    }

    #[test]
    fn opt_outs_are_respected() {
        assert!(asks_not_to_remember(
            "Don't remember this, but my ex is called Alex"
        ));
        assert!(asks_not_to_remember("Ne retiens pas ça : je suis malade"));
        assert!(!asks_not_to_remember("Remember that I like tea"));
        let secret = usable(vec!["My PIN code is 1234".into(), "Likes tea".into()]);
        assert_eq!(secret, ["Likes tea"]);
    }
}
