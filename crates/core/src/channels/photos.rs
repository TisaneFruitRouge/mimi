//! Photos from messaging apps, gathered into one turn.
//!
//! People send photos first and say what they want after ("📷 📷 add these dates to our
//! calendar"), or several photos at once that the app delivers one by one (a Telegram
//! album, one Matrix event per picture), with the caption on any of them. So a message
//! with photos isn't answered at once: it's held for the conversation, and the turn goes
//! to the assistant when
//!
//! - a message without photos arrives: it takes the held photos (and any caption) along;
//! - nothing more came for [`SETTLE`] after photos that came with words: the photos and
//!   their words go on their own;
//! - nothing more came for [`WAIT_FOR_WORDS`] after photos without words: they go on
//!   their own, and the assistant says what it sees;
//! - there are as many as one message can take.
//!
//! Commands and answers to prompts ("/new", "yes") are handled before this, so they
//! never pick up photos.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;

use super::Channel;
use crate::AppState;
use crate::attachments::{MAX_PER_MESSAGE, Upload};

/// How long photos that came with words wait for the rest of their batch.
pub const SETTLE: Duration = Duration::from_secs(3);
/// How long photos without words wait for the user to say what they're for.
pub const WAIT_FOR_WORDS: Duration = Duration::from_secs(45);

/// Photos held per conversation.
pub struct Held {
    pending: Mutex<HashMap<Uuid, Pending>>,
    /// (settle, wait for words): shortened in tests.
    windows: Mutex<(Duration, Duration)>,
}

impl Default for Held {
    fn default() -> Self {
        Self {
            pending: Default::default(),
            windows: Mutex::new((SETTLE, WAIT_FOR_WORDS)),
        }
    }
}

#[derive(Default)]
struct Pending {
    text: String,
    photos: Vec<Upload>,
    /// Bumped by every addition, so only the latest timer sends them.
    generation: u64,
}

fn join(a: String, b: String) -> String {
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, _) => b,
        (_, true) => a,
        _ => format!("{a}\n\n{b}"),
    }
}

impl Held {
    #[cfg(test)]
    pub fn set_windows(&self, settle: Duration, wait_for_words: Duration) {
        *self.windows.lock().unwrap_or_else(|e| e.into_inner()) = (settle, wait_for_words);
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whatever is held for a conversation, taken.
    fn take(&self, conversation: Uuid) -> Option<Pending> {
        self.map().remove(&conversation)
    }

    /// What's held, if nothing was added since `generation`.
    fn take_if(&self, conversation: Uuid, generation: u64) -> Option<Pending> {
        let mut map = self.map();
        if map.get(&conversation)?.generation == generation {
            map.remove(&conversation)
        } else {
            None
        }
    }

    /// Adds a message with photos. Returns what must go now (a full batch), the
    /// generation to wait on, and how long.
    fn add(
        &self,
        conversation: Uuid,
        text: String,
        photos: Vec<Upload>,
    ) -> (Option<Pending>, u64, Duration) {
        let (settle, wait_for_words) = *self.windows.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.map();
        let mut now = None;
        if map
            .get(&conversation)
            .is_some_and(|p| p.photos.len() + photos.len() > MAX_PER_MESSAGE)
        {
            now = map.remove(&conversation);
        }
        let pending = map.entry(conversation).or_default();
        pending.text = join(std::mem::take(&mut pending.text), text);
        pending.photos.extend(photos);
        pending.generation += 1;
        let generation = pending.generation;
        let wait = if pending.text.trim().is_empty() {
            wait_for_words
        } else {
            settle
        };
        if now.is_none() && pending.photos.len() >= MAX_PER_MESSAGE {
            now = map.remove(&conversation);
        }
        (now, generation, wait)
    }
}

/// Hands a message from a messaging app to the assistant (see the module's notes on
/// photos). Returns at once: the turn runs in its own task, through
/// [`super::converse`].
pub fn deliver(
    state: &Arc<AppState>,
    channel: Arc<dyn Channel>,
    conversation: Uuid,
    text: String,
    photos: Vec<Upload>,
) {
    let held = &state.connections.photos;
    let run = |text: String, photos: Vec<Upload>| {
        let (state, channel) = (state.clone(), channel.clone());
        tokio::spawn(async move {
            super::converse(&state, &*channel, conversation, text, photos).await;
        });
    };
    if photos.is_empty() {
        match held.take(conversation) {
            Some(p) => run(join(p.text, text), p.photos),
            None => run(text, Vec::new()),
        }
        return;
    }
    let (now, generation, wait) = held.add(conversation, text, photos);
    if let Some(p) = now {
        run(p.text, p.photos);
    }
    let state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(wait).await;
        if let Some(p) = state.connections.photos.take_if(conversation, generation) {
            super::converse(&state, &*channel, conversation, p.text, p.photos).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(n: u8) -> Upload {
        Upload::new(vec![n], None, None)
    }

    #[test]
    fn photos_wait_for_words_and_captions_settle() {
        let held = Held::default();
        let conv = Uuid::now_v7();
        let (now, g1, wait) = held.add(conv, String::new(), vec![photo(1)]);
        assert!(now.is_none());
        assert_eq!(wait, WAIT_FOR_WORDS);
        let (_, g2, wait) = held.add(conv, "Add these dates".into(), vec![photo(2)]);
        assert_eq!(wait, SETTLE);
        // The first timer finds a newer batch and leaves it.
        assert!(held.take_if(conv, g1).is_none());
        let p = held.take_if(conv, g2).unwrap();
        assert_eq!(p.text, "Add these dates");
        assert_eq!(p.photos, vec![photo(1), photo(2)]);
        assert!(held.take(conv).is_none());
    }

    #[test]
    fn a_full_batch_goes_at_once() {
        let held = Held::default();
        let conv = Uuid::now_v7();
        let (now, ..) = held.add(conv, String::new(), (0..8).map(photo).collect());
        assert!(now.is_none());
        // 8 + 3 is more than a message takes: the 8 go now, the 3 wait.
        let (now, ..) = held.add(conv, String::new(), (8..11).map(photo).collect());
        assert_eq!(now.unwrap().photos.len(), 8);
        let (now, ..) = held.add(conv, String::new(), (11..18).map(photo).collect());
        assert_eq!(now.unwrap().photos.len(), 10);
        assert!(held.take(conv).is_none());
    }
}
