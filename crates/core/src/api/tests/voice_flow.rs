//! Talking to the assistant: the microphone's recordings (`/voice/transcribe`), spoken
//! messages and what the model is told about them, voice messages from Telegram (the
//! first one downloading the recognizer, later ones heard and answered aloud), and
//! reading aloud. The models themselves are replaced by `Voice::fake_words`; the real
//! ones are covered by `voice::tests::live_voice_round_trip`.

use base64::Engine;
use serde_json::{Value, json};

use super::Harness;
use super::fake_telegram::FakeTelegram;
use super::schedule_flow::{OWNER, pair, wait_for};
use super::tool_use::{Reply, scripted_llm};
use crate::voice::audio;

/// Two seconds of a tone, as the composer sends it.
fn recording() -> Vec<u8> {
    let samples = (0..32_000)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin() * 0.4)
        .collect();
    audio::wav(&audio::Sound {
        rate: 16_000,
        samples,
    })
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn unreachable_downloads(h: &Harness) {
    // Nothing listens there: no test ever reaches GitHub.
    h.state.voice.set_base("http://127.0.0.1:9".to_owned());
}

#[tokio::test]
async fn the_microphone_explains_what_it_still_needs() {
    let h = Harness::new().await;
    unreachable_downloads(&h);
    let (status, voice) = h.call(reqwest::Method::GET, "/voice", Value::Null).await;
    assert_eq!(status, 200, "{voice}");
    assert_eq!(voice["listening"]["state"], "missing");
    assert!(voice["listening"]["bytes"].as_u64().unwrap() > 100_000_000);

    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/voice/transcribe",
            json!({ "data": b64(&recording()) }),
        )
        .await;
    assert_eq!(status, 409, "{err}");
    assert_eq!(err["code"], "voice_not_ready");

    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/voice/transcribe",
            json!({ "data": "not base64!" }),
        )
        .await;
    assert_eq!(status, 400, "{err}");

    // Downloads are only what the catalog lists.
    let (status, _) = h
        .call(
            reqwest::Method::POST,
            "/voice/packs/nope/download",
            Value::Null,
        )
        .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn what_the_user_says_is_heard_and_sent_marked_as_spoken() {
    let llm = scripted_llm(|_, _| Reply::Text("Noted.")).await;
    let mut h = Harness::new().await;
    h.use_mock(llm.port()).await;
    *h.state.voice.fake_words.lock().unwrap() = Some("Remind me to call Sam".to_owned());

    let (status, heard) = h
        .call(
            reqwest::Method::POST,
            "/voice/transcribe",
            json!({ "data": b64(&recording()) }),
        )
        .await;
    assert_eq!(status, 200, "{heard}");
    assert_eq!(heard["text"], "Remind me to call Sam");
    assert_eq!(heard["duration_ms"], 2000);

    // What isn't a recording is refused in plain words.
    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/voice/transcribe",
            json!({ "data": b64(b"hello") }),
        )
        .await;
    assert_eq!(status, 400);
    assert_eq!(err["message"], "That recording couldn't be read.");

    let (_, conv) = h
        .call(reqwest::Method::POST, "/conversations", json!({}))
        .await;
    let conv_id = conv["id"].as_str().unwrap().to_owned();
    let (status, sent) = h
        .call(
            reqwest::Method::POST,
            &format!("/conversations/{conv_id}/messages"),
            json!({ "content": "Remind me to call Sam", "spoken": true }),
        )
        .await;
    assert_eq!(status, 200, "{sent}");
    assert_eq!(sent["user_message"]["spoken"], true);
    let reply = sent["assistant_message"]["id"].as_str().unwrap().to_owned();
    h.wait_for_reply(&reply).await;

    // The model knows a word may be misheard; typed messages say nothing of the kind.
    let request = &llm.requests()[0];
    let last = request["messages"].as_array().unwrap().last().unwrap();
    let text = last["content"].as_str().unwrap();
    assert!(text.starts_with("Remind me to call Sam"), "{text}");
    assert!(text.contains("transcribed automatically"), "{text}");

    let (_, detail) = h
        .call(
            reqwest::Method::GET,
            &format!("/conversations/{conv_id}"),
            Value::Null,
        )
        .await;
    assert_eq!(detail["messages"][0]["spoken"], true);
    assert_eq!(detail["messages"][1]["spoken"], false);
}

#[tokio::test]
async fn telegram_voice_messages_are_heard_and_answered_aloud() {
    let llm = scripted_llm(|_, _| Reply::Text("I'll remind you at six.")).await;
    let h = Harness::new().await;
    h.use_mock(llm.port()).await;
    *h.state.voice.fake_words.lock().unwrap() = Some("Remind me to buy milk at six".to_owned());
    let mut settings = crate::settings::load(&h.state.db).await.unwrap();
    settings.voice.reply_with_voice = true;
    crate::settings::save(&h.state.db, &settings).await.unwrap();
    let tg: FakeTelegram = pair(&h).await;

    // A note from a phone: Ogg/Opus, as Telegram records them.
    let note = audio::voice_note(audio::decode(recording()).unwrap()).unwrap();
    tg.voice(OWNER, note.ogg);
    wait_for(|| tg.voices.lock().unwrap().len() == 1).await;
    let said = tg.sent_to(OWNER);
    assert!(
        said.iter()
            .any(|m| m == "🎤 “Remind me to buy milk at six”"),
        "{said:?}"
    );
    assert!(
        said.iter().any(|m| m.contains("remind you at six")),
        "{said:?}"
    );

    // In the app, it's the user's message, marked spoken.
    let request = &llm.requests()[0];
    let last = request["messages"].as_array().unwrap().last().unwrap();
    assert!(
        last["content"]
            .as_str()
            .unwrap()
            .starts_with("Remind me to buy milk at six")
    );
}

#[tokio::test]
async fn the_first_voice_message_downloads_the_recognizer() {
    let llm = scripted_llm(|_, _| Reply::Text("Hi.")).await;
    let h = Harness::new().await;
    h.use_mock(llm.port()).await;
    unreachable_downloads(&h);
    let tg = pair(&h).await;

    tg.voice(OWNER, recording());
    // It says what it's doing, then (the download can't reach anything here) why it
    // couldn't, and how to try again. Nothing reaches the model.
    wait_for(|| {
        tg.sent_to(OWNER)
            .iter()
            .any(|m| m.contains("couldn't get ready"))
    })
    .await;
    let said = tg.sent_to(OWNER);
    assert!(
        said.iter()
            .any(|m| m.contains("getting ready to understand voice messages")),
        "{said:?}"
    );
    assert!(
        said.iter().any(|m| m.contains("Settings › Voice")),
        "{said:?}"
    );
    assert!(llm.requests().is_empty());
}

#[tokio::test]
async fn reading_aloud_needs_something_to_read() {
    let h = Harness::new().await;
    unreachable_downloads(&h);
    let (status, speech) = h
        .call(
            reqwest::Method::POST,
            "/voice/speak",
            json!({ "text": "```\nlet x = 1;\n```" }),
        )
        .await;
    assert_eq!(status, 200, "{speech}");
    assert_eq!(speech["kind"], "nothing");

    let (status, err) = h
        .call(
            reqwest::Method::POST,
            "/voice/speak",
            json!({ "text": "", "voice": "nope" }),
        )
        .await;
    assert_eq!(status, 400, "{err}");

    // A voice that isn't here starts downloading, and says so.
    let (_, speech) = h
        .call(
            reqwest::Method::POST,
            "/voice/speak",
            json!({ "text": "", "voice": "piper-de_DE-thorsten-medium" }),
        )
        .await;
    assert_eq!(speech["kind"], "downloading", "{speech}");
    assert_eq!(speech["pack"]["id"], "piper-de_DE-thorsten-medium");
}

#[tokio::test]
async fn voice_settings_are_checked() {
    let h = Harness::new().await;
    let mut settings: Value = h
        .call(reqwest::Method::GET, "/settings", Value::Null)
        .await
        .1;
    settings["voice"]["voices"] = json!({ "fr": "piper-de_DE-thorsten-medium" });
    let (status, _) = h
        .call(reqwest::Method::PUT, "/settings", settings.clone())
        .await;
    assert_eq!(status, 400);
    settings["voice"]["voices"] = json!({ "fr": "kokoro-siwis" });
    settings["voice"]["speed"] = json!(10);
    let (status, saved) = h.call(reqwest::Method::PUT, "/settings", settings).await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["voice"]["speed"], 75);
}
