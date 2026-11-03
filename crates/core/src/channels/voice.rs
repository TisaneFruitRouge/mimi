//! Voice messages in the messaging apps. What the user says in one becomes the words of
//! their message, marked spoken; Mimi first says back what it heard, so a misheard word
//! shows before anything is done about it. With "Answer voice messages with a voice
//! message" on (Settings › Voice), the reply also comes as a voice message where the app
//! can play one. The recording itself is never kept.

use std::sync::Arc;
use std::time::Duration;

use mimi_protocol::PackState;
use uuid::Uuid;

use super::{Channel, Outgoing, photos};
use crate::AppState;
use crate::voice::{self, ListenError};

/// How long a voice message waits for the speech recognizer to download.
const DOWNLOAD_WAIT: Duration = Duration::from_secs(60 * 60);
/// Longest echo of what was heard.
const ECHO_CHARS: usize = 600;

/// Listens to a voice message from the user (or someone they trust) and hands its words
/// to the assistant, with the caption it came with. Returns at once.
pub fn hear(
    state: &Arc<AppState>,
    channel: Arc<dyn Channel>,
    conversation: Uuid,
    audio: Vec<u8>,
    caption: String,
) {
    let state = state.clone();
    tokio::spawn(async move {
        let guest = state.access.is_guest_conversation(conversation);
        let Some(words) = listen(&state, &*channel, audio, guest).await else {
            return;
        };
        let text = if caption.trim().is_empty() {
            words
        } else {
            format!("{}\n\n{words}", caption.trim())
        };
        photos::deliver_spoken(&state, channel, conversation, text);
    });
}

async fn say(channel: &dyn Channel, text: String) {
    if let Err(e) = channel.send(&Outgoing::text(text)).await {
        tracing::warn!("sending to {} failed: {e}", channel.kind());
    }
}

/// The words in a voice message, after downloading the recognizer if it's the first.
/// Tells the user why when there are none.
async fn listen(
    state: &Arc<AppState>,
    channel: &dyn Channel,
    audio: Vec<u8>,
    guest: bool,
) -> Option<String> {
    channel.typing().await;
    let mut heard = voice::transcribe(state, audio.clone()).await;
    if let Err(ListenError::NotReady(pack)) = &heard {
        // Only the owner's messages start downloads on their computer.
        if guest {
            say(
                channel,
                "I can't listen to voice messages yet. Could you write it instead?".to_owned(),
            )
            .await;
            return None;
        }
        let already = pack.state == PackState::Downloading;
        if let Err(e) = voice::start_download(state, &pack.id) {
            say(
                channel,
                format!("I can't understand voice messages yet: {e}"),
            )
            .await;
            return None;
        }
        if !already {
            let mb = (pack.bytes + 500_000) / 1_000_000;
            say(
                channel,
                format!(
                    "I'm getting ready to understand voice messages: a one-time download of {mb} MB, kept on your computer. I'll answer this one as soon as it's done."
                ),
            )
            .await;
        }
        if !voice::wait_until_ready(state, &pack.id, DOWNLOAD_WAIT).await {
            let why = state
                .voice
                .errors()
                .get(&pack.id)
                .map(|e| format!(" ({})", e.trim_end_matches('.')))
                .unwrap_or_default();
            say(
                channel,
                format!(
                    "I couldn't get ready to understand voice messages{why}. You can try again in the app, in Settings › Voice."
                ),
            )
            .await;
            return None;
        }
        channel.typing().await;
        heard = voice::transcribe(state, audio).await;
    }
    match heard {
        Ok(t) if t.text.trim().is_empty() => {
            say(
                channel,
                "I couldn't make out any words in that voice message.".to_owned(),
            )
            .await;
            None
        }
        Ok(t) => {
            say(channel, echo(&t.text)).await;
            Some(t.text)
        }
        Err(e) => {
            say(
                channel,
                format!("I couldn't listen to that voice message: {}", e.message()),
            )
            .await;
            None
        }
    }
}

/// What was heard, said back.
fn echo(text: &str) -> String {
    let text = text.trim();
    let clipped: String = text.chars().take(ECHO_CHARS).collect();
    let more = if clipped.len() < text.len() {
        "…"
    } else {
        ""
    };
    format!("🎤 “{clipped}{more}”")
}

/// Sends a reply as a voice message too, if the user wants it and the app plays them.
pub async fn answer_aloud(state: &Arc<AppState>, channel: &dyn Channel, markdown: &str) {
    if !channel.sends_voice() {
        return;
    }
    let wanted = crate::settings::load(&state.db)
        .await
        .is_ok_and(|s| s.voice.reply_with_voice);
    if !wanted {
        return;
    }
    match voice::voice_message(state, markdown).await {
        Ok(Some(note)) => {
            if let Err(e) = channel.send_voice(&note).await {
                tracing::warn!("sending a voice message to {} failed: {e}", channel.kind());
            }
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("reading a reply aloud failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use mimi_protocol::{Action, Delivery};

    use super::*;

    #[derive(Default)]
    struct Said(Mutex<Vec<String>>);

    #[async_trait]
    impl Channel for Said {
        fn kind(&self) -> &'static str {
            "test"
        }
        async fn send(&self, message: &Outgoing) -> Result<(), String> {
            self.0.lock().unwrap().push(message.markdown.clone());
            Ok(())
        }
        async fn ask_approval(&self, _: &Action) -> Result<(), String> {
            Ok(())
        }
        async fn remind(&self, _: &Delivery, _: Option<&str>) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn someone_the_user_trusts_never_starts_a_download() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::for_tests("t");
        state.paths.data_dir = dir.path().to_path_buf();
        state.voice.set_base("http://127.0.0.1:9".to_owned());
        let state = Arc::new(state);
        let wav = crate::voice::audio::wav(&crate::voice::audio::Sound {
            rate: 16_000,
            samples: vec![0.2; 16_000],
        });
        let channel = Said::default();
        assert!(listen(&state, &channel, wav, true).await.is_none());
        assert!(state.voice.downloads().is_empty());
        let said = channel.0.lock().unwrap().clone();
        assert_eq!(
            said,
            ["I can't listen to voice messages yet. Could you write it instead?"]
        );
    }

    #[test]
    fn long_echoes_are_cut() {
        assert_eq!(echo("  add milk  "), "🎤 “add milk”");
        let long = "word ".repeat(200);
        let e = echo(&long);
        assert!(e.ends_with("…”"));
        assert!(e.chars().count() < ECHO_CHARS + 10);
    }
}
