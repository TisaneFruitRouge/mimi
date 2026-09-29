//! Answering Mimi's prompts without buttons. In Signal and Matrix the user answers an
//! approval or a reminder with a short reply ("yes", "no", "done", "snooze 1h") or a
//! reaction (👍, 👎, ✅, 💤). A quoted reply or a reaction answers the prompt it points
//! at; a bare "yes" or "no" answers the only approval still waiting on that channel, and
//! a bare "done" or "snooze" the latest reminder of the last hour. Anything else is a
//! message for the assistant.

use std::collections::VecDeque;
use std::sync::Mutex;

use uuid::Uuid;

use crate::{AppState, now_ms};

/// What a prompt was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Approval(Uuid),
    /// A reminder's delivery id.
    Reminder(Uuid),
}

/// A short answer, before knowing what it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    Yes,
    No,
    Done,
    /// Minutes.
    Snooze(u32),
}

/// The prompts a channel sent, so replies and reactions can be matched to them.
#[derive(Default)]
pub struct Prompts {
    sent: Mutex<VecDeque<Sent>>,
}

struct Sent {
    connection: Uuid,
    message_ref: String,
    target: Target,
    at: i64,
}

/// How many prompts are remembered, across channels.
const REMEMBERED: usize = 200;
/// How long a bare "done" or "snooze" still means the latest reminder.
const BARE_REMINDER_MS: i64 = 60 * 60 * 1000;

impl Prompts {
    /// Remembers that the message `message_ref` (the app's id for it) sent on
    /// `connection` asks about `target`.
    pub fn remember(&self, connection: Uuid, message_ref: impl Into<String>, target: Target) {
        let mut sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        sent.push_back(Sent {
            connection,
            message_ref: message_ref.into(),
            target,
            at: now_ms(),
        });
        while sent.len() > REMEMBERED {
            sent.pop_front();
        }
    }

    /// What a message sent on `connection` asked about, if it was a prompt.
    pub fn target_of(&self, connection: Uuid, message_ref: &str) -> Option<Target> {
        let sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        sent.iter()
            .rev()
            .find(|s| s.connection == connection && s.message_ref == message_ref)
            .map(|s| s.target)
    }

    fn approvals(&self, connection: Uuid) -> Vec<Uuid> {
        let sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        let mut ids: Vec<Uuid> = sent
            .iter()
            .filter(|s| s.connection == connection)
            .filter_map(|s| match s.target {
                Target::Approval(id) => Some(id),
                Target::Reminder(_) => None,
            })
            .collect();
        ids.dedup();
        ids
    }

    fn latest_reminder(&self, connection: Uuid, now: i64) -> Option<Uuid> {
        let sent = self.sent.lock().unwrap_or_else(|e| e.into_inner());
        sent.iter()
            .rev()
            .filter(|s| s.connection == connection && now - s.at < BARE_REMINDER_MS)
            .find_map(|s| match s.target {
                Target::Reminder(id) => Some(id),
                Target::Approval(_) => None,
            })
    }
}

/// Reads a short answer: a word or two, or a single emoji.
pub fn parse(text: &str) -> Option<Word> {
    let t = text
        .trim()
        .trim_end_matches(['.', '!'])
        .trim()
        .to_lowercase()
        .replace('’', "'");
    // Emoji may carry a skin tone or variation selector.
    let bare: String = t
        .chars()
        .filter(|c| !matches!(*c as u32, 0xFE0F | 0x1F3FB..=0x1F3FF))
        .collect();
    let word = match bare.as_str() {
        "yes" | "y" | "ok" | "okay" | "sure" | "approve" | "go" | "go ahead" | "do it" | "oui"
        | "👍" | "👌" | "✔" => Word::Yes,
        "no" | "n" | "don't" | "dont" | "do not" | "decline" | "cancel" | "stop" | "non" | "👎"
        | "❌" | "✖" => Word::No,
        "done" | "✅" | "fait" | "c'est fait" => Word::Done,
        "snooze" | "later" | "💤" | "snooze 10" | "snooze 10 min" | "snooze 10 minutes" => {
            Word::Snooze(10)
        }
        "snooze 1h" | "snooze 1 h" | "snooze 1 hour" | "snooze an hour" | "snooze 60"
        | "snooze 60 min" | "1h" | "1 hour" | "an hour" => Word::Snooze(60),
        _ => return None,
    };
    Some(word)
}

/// Handles a message from the user that may answer a prompt. `quoted` is the app's id of
/// the message it replies to, if any. Returns the line to send back when it was an
/// answer, or `None` when the message is for the assistant.
pub async fn answer_text(
    state: &AppState,
    connection: Uuid,
    quoted: Option<&str>,
    text: &str,
) -> Option<String> {
    let word = parse(text)?;
    let prompts = &state.connections.prompts;
    if let Some(target) = quoted.and_then(|q| prompts.target_of(connection, q)) {
        return apply(state, target, word).await;
    }
    match word {
        Word::Yes | Word::No => {
            let waiting: Vec<Uuid> = prompts
                .approvals(connection)
                .into_iter()
                .filter(|id| state.approvals.is_waiting(*id))
                .collect();
            match waiting.as_slice() {
                [] => None,
                [one] => apply(state, Target::Approval(*one), word).await,
                _ => Some(
                    "Several things are waiting for your OK. Reply to the one you mean.".to_owned(),
                ),
            }
        }
        Word::Done | Word::Snooze(_) => {
            let id = prompts.latest_reminder(connection, now_ms())?;
            apply(state, Target::Reminder(id), word).await
        }
    }
}

/// Handles a reaction to one of the channel's messages. The line to send back, if the
/// reaction answered a prompt.
pub async fn answer_reaction(
    state: &AppState,
    connection: Uuid,
    message_ref: &str,
    emoji: &str,
) -> Option<String> {
    let target = state
        .connections
        .prompts
        .target_of(connection, message_ref)?;
    apply(state, target, parse(emoji)?).await
}

/// Carries out an answer. The line to send back, or `None` if the word doesn't answer
/// that kind of prompt.
async fn apply(state: &AppState, target: Target, word: Word) -> Option<String> {
    const HANDLED: &str = "That was already handled.";
    match (target, word) {
        (Target::Approval(id), Word::Yes | Word::Done) => Some(
            if state
                .approvals
                .decide(id, crate::tools::Decision::Approve(None))
            {
                "✅ Approved".to_owned()
            } else {
                HANDLED.to_owned()
            },
        ),
        (Target::Approval(id), Word::No) => Some(
            if state.approvals.decide(id, crate::tools::Decision::Reject) {
                "✖️ Declined".to_owned()
            } else {
                HANDLED.to_owned()
            },
        ),
        (Target::Reminder(id), Word::Yes | Word::Done) => {
            Some(if crate::schedule::mark_done(state, id).await {
                "✅ Done".to_owned()
            } else {
                HANDLED.to_owned()
            })
        }
        (Target::Reminder(id), Word::Snooze(minutes)) => {
            Some(if crate::schedule::snooze(state, id, minutes).await {
                format!("💤 Snoozed for {}", snooze_label(minutes))
            } else {
                HANDLED.to_owned()
            })
        }
        (Target::Approval(_), Word::Snooze(_)) | (Target::Reminder(_), Word::No) => None,
    }
}

pub fn snooze_label(minutes: u32) -> String {
    if minutes == 60 {
        "an hour".to_owned()
    } else {
        format!("{minutes} minutes")
    }
}

/// The line under an approval prompt, for apps without buttons.
pub const APPROVAL_HINT: &str = "Reply yes or no (or react 👍 / 👎).";
/// The line under a reminder, for apps without buttons.
pub const REMINDER_HINT: &str = "Reply done, snooze, or snooze 1h (or react ✅ / 💤).";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_answers() {
        assert_eq!(parse(" Yes! "), Some(Word::Yes));
        assert_eq!(parse("👍🏽"), Some(Word::Yes));
        assert_eq!(parse("Don’t"), Some(Word::No));
        assert_eq!(parse("✅"), Some(Word::Done));
        assert_eq!(parse("snooze 1h"), Some(Word::Snooze(60)));
        assert_eq!(parse("💤"), Some(Word::Snooze(10)));
        assert_eq!(parse("yes, and add Sam too"), None);
        assert_eq!(parse("what's on today?"), None);
    }

    #[test]
    fn prompts_are_matched_per_connection() {
        let prompts = Prompts::default();
        let (a, b) = (Uuid::now_v7(), Uuid::now_v7());
        let approval = Uuid::now_v7();
        let reminder = Uuid::now_v7();
        prompts.remember(a, "m1", Target::Approval(approval));
        prompts.remember(a, "m2", Target::Reminder(reminder));
        assert_eq!(prompts.target_of(a, "m1"), Some(Target::Approval(approval)));
        assert_eq!(prompts.target_of(b, "m1"), None);
        assert_eq!(prompts.approvals(a), vec![approval]);
        assert_eq!(prompts.latest_reminder(a, now_ms()), Some(reminder));
        assert_eq!(
            prompts.latest_reminder(a, now_ms() + BARE_REMINDER_MS + 1),
            None
        );
    }

    #[tokio::test]
    async fn a_bare_yes_answers_the_only_waiting_approval() {
        let state = crate::AppState::for_tests("t");
        let connection = Uuid::now_v7();
        let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
        // Nothing waiting: "yes" is a message for the assistant.
        assert_eq!(answer_text(&state, connection, None, "yes").await, None);

        let decision = state.approvals.wait(first);
        state
            .connections
            .prompts
            .remember(connection, "p1", Target::Approval(first));
        assert_eq!(
            answer_text(&state, connection, None, "yes")
                .await
                .as_deref(),
            Some("✅ Approved")
        );
        assert!(matches!(
            decision.await,
            Ok(crate::tools::Decision::Approve(None))
        ));

        let _one = state.approvals.wait(first);
        let _two = state.approvals.wait(second);
        state
            .connections
            .prompts
            .remember(connection, "p2", Target::Approval(second));
        assert!(
            answer_text(&state, connection, None, "no")
                .await
                .is_some_and(|r| r.contains("Several"))
        );
        // Replying to the prompt settles it.
        assert_eq!(
            answer_text(&state, connection, Some("p2"), "no")
                .await
                .as_deref(),
            Some("✖️ Declined")
        );
        assert_eq!(
            answer_reaction(&state, connection, "p2", "👍")
                .await
                .as_deref(),
            Some("That was already handled.")
        );
    }
}
