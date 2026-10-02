//! sherpa-onnx, which runs the recognizers and voices on this computer. Everything here
//! blocks (loading takes a second or more, speaking a little less than the speech
//! lasts), so callers run it in `spawn_blocking`.

use std::path::Path;

use sherpa_onnx::{
    GenerationConfig, OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig,
    OfflineTransducerModelConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig,
    OfflineTtsModelConfig, OfflineTtsVitsModelConfig, OfflineWhisperModelConfig,
};

use super::audio::{self, RECOGNIZER_RATE, Sound};
use super::catalog::{Recognizer, RecognizerEngine, Voice, VoiceEngine, VoicePack};

/// Threads for one recognition or one sentence: enough to be quick, not so many that the
/// rest of the computer stalls.
fn threads() -> i32 {
    std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4)) as i32
}

fn file(
    dir: &Path,
    files: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Option<String> {
    files
        .get(key)
        .map(|f| dir.join(f).to_string_lossy().into_owned())
}

/// A loaded speech recognizer.
pub struct Listener {
    recognizer: OfflineRecognizer,
    /// Longest piece it takes at once.
    max_seconds: u32,
}

impl Listener {
    pub fn load(entry: &Recognizer, dir: &Path) -> Result<Self, String> {
        let f = |key| file(dir, &entry.files, key);
        let mut model = OfflineModelConfig {
            tokens: f("tokens"),
            num_threads: threads(),
            ..Default::default()
        };
        let max_seconds = match entry.engine {
            RecognizerEngine::NemoTransducer => {
                model.transducer = OfflineTransducerModelConfig {
                    encoder: f("encoder"),
                    decoder: f("decoder"),
                    joiner: f("joiner"),
                };
                model.model_type = Some("nemo_transducer".to_owned());
                // It takes long recordings, but memory grows with them.
                60
            }
            RecognizerEngine::Whisper => {
                model.whisper = OfflineWhisperModelConfig {
                    encoder: f("encoder"),
                    decoder: f("decoder"),
                    // Empty: it finds the language itself.
                    language: Some(String::new()),
                    task: Some("transcribe".to_owned()),
                    tail_paddings: -1,
                    ..Default::default()
                };
                // Whisper hears 30 seconds at a time.
                28
            }
        };
        let config = OfflineRecognizerConfig {
            model_config: model,
            ..Default::default()
        };
        let recognizer = OfflineRecognizer::create(&config).ok_or_else(damaged)?;
        Ok(Self {
            recognizer,
            max_seconds,
        })
    }

    /// The words in a recording.
    pub fn transcribe(&self, sound: Sound) -> String {
        let sound = sound.resampled(RECOGNIZER_RATE);
        let mut words: Vec<String> = Vec::new();
        for piece in audio::pieces(&sound, self.max_seconds) {
            let stream = self.recognizer.create_stream();
            stream.accept_waveform(RECOGNIZER_RATE as i32, piece);
            self.recognizer.decode(&stream);
            if let Some(result) = stream.get_result() {
                let text = result.text.trim();
                if !text.is_empty() {
                    words.push(text.to_owned());
                }
            }
        }
        words.join(" ")
    }
}

fn damaged() -> String {
    "The voice files couldn't be loaded. Remove them in Settings › Voice and download them again."
        .to_owned()
}

/// A loaded voice: one model, set up for one language.
pub struct Speaker {
    tts: OfflineTts,
}

impl Speaker {
    pub fn load(pack: &VoicePack, voice: &Voice, dir: &Path) -> Result<Self, String> {
        let f = |key| file(dir, &pack.files, key);
        let mut model = OfflineTtsModelConfig {
            num_threads: threads(),
            ..Default::default()
        };
        match pack.engine {
            VoiceEngine::Kokoro => {
                let lexicon = voice
                    .lexicon
                    .iter()
                    .map(|l| dir.join(l).to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(",");
                model.kokoro = OfflineTtsKokoroModelConfig {
                    model: f("model"),
                    voices: f("voices"),
                    tokens: f("tokens"),
                    data_dir: f("data_dir"),
                    dict_dir: f("dict_dir"),
                    lexicon: (!lexicon.is_empty()).then_some(lexicon),
                    lang: voice.lang.clone(),
                    ..Default::default()
                };
            }
            VoiceEngine::Vits => {
                model.vits = OfflineTtsVitsModelConfig {
                    model: f("model"),
                    tokens: f("tokens"),
                    data_dir: f("data_dir"),
                    ..Default::default()
                };
            }
        }
        let config = OfflineTtsConfig {
            model,
            ..Default::default()
        };
        let tts = OfflineTts::create(&config).ok_or_else(damaged)?;
        Ok(Self { tts })
    }

    /// Speech for a few sentences. `speed` is 1.0 for normal.
    pub fn speak(&self, text: &str, speaker: i32, speed: f32) -> Result<Sound, String> {
        // The library can't take text with a NUL in it.
        let text = text.replace('\0', " ");
        let config = GenerationConfig {
            sid: speaker,
            speed,
            ..Default::default()
        };
        let audio = self
            .tts
            .generate_with_config(&text, &config, None::<fn(&[f32], f32) -> bool>)
            .ok_or("That couldn't be read aloud.")?;
        Ok(Sound {
            rate: audio.sample_rate() as u32,
            samples: audio.samples().to_vec(),
        })
    }
}
