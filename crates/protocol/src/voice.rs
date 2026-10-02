use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How the assistant talks and listens (Settings › Voice). Everything happens on this
/// computer: speech is turned into words, and replies into speech, by files Mimi
/// downloads once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct VoiceSettings {
    /// Send what the user said as soon as they stop talking, instead of putting the
    /// words in the message box to check first.
    pub send_when_done: bool,
    /// The speech recognizer that understands the user (a `VoiceStatus.recognizers`
    /// id). `None`: the one that suits their language.
    pub recognizer: Option<String>,
    /// How replies sound when read aloud. `None`: what suits this computer.
    pub style: Option<VoiceStyle>,
    /// The voice chosen for a language, by the language without its region (`en`, `fr`)
    /// → `VoiceInfo.id`. Languages without one get a voice of the chosen style.
    pub voices: BTreeMap<String, String>,
    /// Reading speed, in percent of normal (75–150).
    pub speed: u16,
    /// In messaging apps, answer a voice message with a voice message too, after the
    /// written reply (Telegram and Matrix: Signal can't play what Mimi records).
    pub reply_with_voice: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            send_when_done: false,
            recognizer: None,
            style: None,
            voices: BTreeMap::new(),
            speed: 100,
            reply_with_voice: false,
        }
    }
}

/// How voices sound, and what they ask of the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VoiceStyle {
    /// Lifelike, for capable computers: one larger download for several languages.
    Natural,
    /// Quick on any computer: a small download per language.
    Light,
}

/// What's needed to listen and speak, and how far along it is (`GET /v1/voice`,
/// `VoiceChanged` events).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VoiceStatus {
    /// The recognizer in use: the chosen one, or the one that suits the user's language.
    pub listening: VoicePack,
    /// Every recognizer the user can choose.
    pub recognizers: Vec<VoicePack>,
    /// How replies sound now: the chosen style, or the recommended one.
    pub style: VoiceStyle,
    /// What suits this computer.
    pub recommended_style: VoiceStyle,
    /// Every voice, by language, whichever style it is.
    pub voices: Vec<VoiceInfo>,
    /// Voice files on this computer or downloading, to show and remove.
    pub packs: Vec<VoicePack>,
}

/// Something Mimi downloads to listen or speak: a speech recognizer, or a set of voices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VoicePack {
    pub id: String,
    /// What it is, in plain words ("Understands 25 European languages").
    pub label: String,
    /// One line more: languages, speed.
    pub detail: String,
    /// Download size.
    #[ts(type = "number")]
    pub bytes: u64,
    pub state: PackState,
    /// While downloading: how much has arrived.
    #[ts(type = "number | null")]
    pub done_bytes: Option<u64>,
    /// Why the last download failed, if it did.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PackState {
    Missing,
    Downloading,
    /// Downloaded and unpacked: ready to use.
    Ready,
}

/// A voice for reading aloud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct VoiceInfo {
    pub id: String,
    /// Its name ("Siwis").
    pub name: String,
    /// The language it speaks, as a tag ("fr", "en-US").
    pub language: String,
    /// That language, in English ("French", "English (US)").
    pub language_name: String,
    pub style: VoiceStyle,
    /// The pack it comes in (`VoicePack.id`).
    pub pack: String,
}

/// Body of `POST /v1/voice/transcribe`: something the user said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TranscribeRequest {
    /// The recording, base64-encoded: WAV, Ogg/Opus, WebM, MP4/AAC, MP3 or FLAC.
    pub data: String,
}

/// What was understood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transcript {
    /// The words, empty if none were heard.
    pub text: String,
    /// How long the recording was.
    pub duration_ms: u32,
}

/// Body of `POST /v1/voice/speak`: text to read aloud, one part at a time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpeakRequest {
    /// Markdown, as the assistant wrote it; it's turned into what one would say.
    pub text: String,
    /// Which part (0-based): long text is read in parts, so it starts sooner.
    #[serde(default)]
    pub part: u32,
    /// A voice to use instead of the chosen one (to hear it in Settings).
    #[serde(default)]
    pub voice: Option<String>,
}

/// One part of the text as speech, or the voice it needs still downloading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Speech {
    Audio {
        /// A WAV file, base64-encoded.
        audio: String,
        /// How many parts the text has.
        parts: u32,
    },
    /// The voice for this text isn't on this computer yet: its download has started.
    /// Ask again once the pack is ready (`VoiceChanged`).
    Downloading { pack: VoicePack },
    /// Nothing to read (only code, or empty).
    Nothing,
}
