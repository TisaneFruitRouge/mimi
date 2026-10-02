use std::path::Path;
use std::sync::Arc;

use mimi_protocol::{PackState, SpeakRequest, Speech, VoiceSettings, VoiceStyle};

use super::*;

fn state_in(dir: &Path) -> Arc<AppState> {
    let mut state = AppState::for_tests("t");
    state.paths.data_dir = dir.to_path_buf();
    // Nothing listens there: a download that starts fails at once, never reaching GitHub.
    state.voice.set_base("http://127.0.0.1:9".to_owned());
    state.voice.set_tier(HardwareTier::Strong);
    Arc::new(state)
}

#[test]
fn each_language_gets_a_voice_of_the_chosen_style() {
    let settings = VoiceSettings::default();
    let (pack, voice) = choose(&settings, VoiceStyle::Natural, "fr", Some("FR")).unwrap();
    assert_eq!(
        (pack.id.as_str(), voice.id.as_str()),
        ("kokoro", "kokoro-siwis")
    );
    let (pack, _) = choose(&settings, VoiceStyle::Light, "fr", Some("FR")).unwrap();
    assert_eq!(pack.id, "piper-fr_FR-siwis-medium");
    // German has light voices only: the style is a preference, not a requirement.
    let (pack, _) = choose(&settings, VoiceStyle::Natural, "de", None).unwrap();
    assert_eq!(pack.style, VoiceStyle::Light);
    // Italian has natural voices only.
    let (pack, _) = choose(&settings, VoiceStyle::Light, "it", None).unwrap();
    assert_eq!(pack.style, VoiceStyle::Natural);
    assert!(choose(&settings, VoiceStyle::Natural, "ja", None).is_none());
}

#[test]
fn english_follows_the_computers_region_and_the_users_choice() {
    let mut settings = VoiceSettings::default();
    let (_, voice) = choose(&settings, VoiceStyle::Natural, "en", Some("GB")).unwrap();
    assert_eq!(voice.language, "en-GB");
    let (_, voice) = choose(&settings, VoiceStyle::Natural, "en", Some("US")).unwrap();
    assert_eq!(voice.language, "en-US");
    let (_, voice) = choose(&settings, VoiceStyle::Light, "en", None).unwrap();
    assert_eq!(voice.language, "en-US");

    settings
        .voices
        .insert("en".to_owned(), "kokoro-george".to_owned());
    let (_, voice) = choose(&settings, VoiceStyle::Light, "en", Some("US")).unwrap();
    assert_eq!(voice.id, "kokoro-george");
}

#[test]
fn voice_settings_are_checked() {
    let mut ok = VoiceSettings {
        speed: 500,
        recognizer: Some("whisper-turbo".to_owned()),
        ..Default::default()
    };
    ok.voices.insert("fr".to_owned(), "kokoro-siwis".to_owned());
    validate(&mut ok).unwrap();
    assert_eq!(ok.speed, 150);

    let mut unknown = VoiceSettings {
        recognizer: Some("nope".to_owned()),
        ..Default::default()
    };
    assert!(validate(&mut unknown).is_err());
    // A voice must be filed under its own language.
    let mut misfiled = VoiceSettings::default();
    misfiled
        .voices
        .insert("de".to_owned(), "kokoro-siwis".to_owned());
    assert!(validate(&mut misfiled).is_err());
}

#[test]
fn the_recommended_recognizer_understands_the_users_language() {
    assert_eq!(recommended_recognizer("fr").id, "parakeet-v3");
    assert_eq!(recommended_recognizer("ja").id, "whisper-turbo");
    let chosen = VoiceSettings {
        recognizer: Some("whisper-turbo".to_owned()),
        ..Default::default()
    };
    assert_eq!(recognizer_for(&chosen).id, "whisper-turbo");
}

#[tokio::test]
async fn nothing_is_heard_before_the_recognizer_is_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_in(dir.path());
    match transcribe(&state, audio::wav(&silence())).await {
        Err(ListenError::NotReady(pack)) => {
            assert_eq!(pack.state, PackState::Missing);
            assert!(pack.bytes > 100_000_000);
        }
        other => panic!("{other:?}"),
    }
}

fn silence() -> audio::Sound {
    audio::Sound {
        rate: 16_000,
        samples: vec![0.0; 16_000],
    }
}

#[tokio::test]
async fn reading_aloud_starts_the_voices_download() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_in(dir.path());
    let speech = speak(
        &state,
        SpeakRequest {
            text: "Bonjour ! Ton rendez-vous avec Sam est demain à quinze heures.".to_owned(),
            part: 0,
            voice: None,
        },
    )
    .await
    .unwrap();
    let Speech::Downloading { pack } = speech else {
        panic!("{speech:?}");
    };
    // A strong computer, French: the natural voices.
    assert_eq!(pack.id, "kokoro");
    assert_eq!(pack.state, PackState::Downloading);
    // The fake address fails it soon after, and says why.
    for _ in 0..100 {
        if !state.voice.downloads().contains_key("kokoro") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let status = status(&state).await;
    let kokoro = status.packs.iter().find(|p| p.id == "kokoro").unwrap();
    assert_eq!(kokoro.state, PackState::Missing);
    assert!(kokoro.error.as_deref().unwrap().contains("GitHub"));
}

#[tokio::test]
async fn code_alone_is_nothing_to_read() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_in(dir.path());
    let speech = speak(
        &state,
        SpeakRequest {
            text: "```\nlet x = 1;\n```".to_owned(),
            part: 0,
            voice: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(speech, Speech::Nothing);
}

#[tokio::test]
async fn the_status_lists_every_recognizer_and_voice() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_in(dir.path());
    let status = status(&state).await;
    assert_eq!(status.recognizers.len(), 2);
    assert_eq!(status.recommended_style, VoiceStyle::Natural);
    assert!(status.voices.iter().any(|v| v.language == "de"));
    assert!(status.packs.is_empty());
    // A pack that's there is listed, and removing it removes it.
    std::fs::create_dir_all(download::pack_dir(
        &state.paths,
        "piper-de_DE-thorsten-medium",
    ))
    .unwrap();
    let status_now = super::status(&state).await;
    assert_eq!(status_now.packs.len(), 1);
    assert_eq!(status_now.packs[0].state, PackState::Ready);
    remove(&state, "piper-de_DE-thorsten-medium").await.unwrap();
    assert!(!download::is_ready(
        &state.paths,
        "piper-de_DE-thorsten-medium"
    ));
}

/// Listens to Mimi's own voice: reads a sentence aloud and transcribes it again, with the
/// real models. Point `MIMI_TEST_VOICE` at a folder holding the unpacked `kokoro` and
/// `parakeet-v3` packs (as in `<data>/voice/`):
/// `MIMI_TEST_VOICE=~/.local/share/mimi/voice cargo test -p mimi-core live_voice -- --ignored`
#[tokio::test]
#[ignore]
async fn live_voice_round_trip() {
    let source = std::path::PathBuf::from(std::env::var("MIMI_TEST_VOICE").unwrap());
    let dir = tempfile::tempdir().unwrap();
    let state = state_in(dir.path());
    let voice_dir = download::voice_dir(&state.paths);
    std::fs::create_dir_all(&voice_dir).unwrap();
    for pack in ["kokoro", "parakeet-v3"] {
        std::os::unix::fs::symlink(source.join(pack), voice_dir.join(pack)).unwrap();
    }
    let said = "Bonjour ! Ton rendez-vous avec Sam est demain à quinze heures trente.";
    let Speech::Audio { audio, parts } = speak(
        &state,
        SpeakRequest {
            text: said.to_owned(),
            part: 0,
            voice: None,
        },
    )
    .await
    .unwrap() else {
        panic!("no audio");
    };
    assert_eq!(parts, 1);
    let wav = base64::engine::general_purpose::STANDARD
        .decode(audio)
        .unwrap();
    let heard = transcribe(&state, wav).await.unwrap();
    let text = heard.text.to_lowercase();
    assert!(
        text.contains("rendez-vous") && text.contains("sam"),
        "{text}"
    );
    assert!(heard.duration_ms > 2_000);

    // And as a voice message, which decodes and is heard the same.
    let note = voice_message(&state, said).await.unwrap().unwrap();
    let heard = transcribe(&state, note.ogg).await.unwrap();
    assert!(
        heard.text.to_lowercase().contains("demain"),
        "{}",
        heard.text
    );
}
