//! Voice: the user talks to the assistant and hears it answer, all on this computer.
//!
//! - **Listening**: the composer's microphone and voice messages from the messaging
//!   apps are turned into words by a speech recognizer (NVIDIA's Parakeet for 25
//!   European languages, or Whisper for almost all). What was said becomes an ordinary
//!   message marked `spoken`; the recording itself is never kept.
//! - **Speaking**: replies are read aloud by a voice for their language: natural ones
//!   (Kokoro) on capable computers, light ones (Piper) on any.
//!
//! Both run in this process through sherpa-onnx (onnxruntime), from files downloaded
//! once from the catalog (`catalog.json`, see [`download`]). Nothing leaves the
//! computer. Loaded models are given back after [`IDLE_UNLOAD`] without use.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine;
use mimi_protocol::{
    Event, HardwareTier, PackState, SpeakRequest, Speech, Transcript, VoiceInfo,
    VoicePack as PackStatus, VoiceSettings, VoiceStatus, VoiceStyle,
};
use tokio_util::sync::CancellationToken;

use crate::AppState;

pub mod audio;
pub mod catalog;
pub mod download;
mod engine;
pub mod speakable;

/// Loaded recognizers and voices are unloaded after this long without use.
pub const IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);
/// Speech parts are joined with this much silence in voice messages.
const PAUSE_MS: u32 = 250;

/// What's loaded and what's downloading.
#[derive(Default)]
pub struct Voice {
    /// The loaded recognizer, by id (locked while it loads, so it loads once).
    listener: tokio::sync::Mutex<Option<(String, Arc<engine::Listener>)>>,
    /// The loaded voice model, by pack and language.
    speaker: tokio::sync::Mutex<Option<(String, Arc<engine::Speaker>)>>,
    last_used: Mutex<Option<Instant>>,
    /// Running downloads: their cancellation and the bytes so far.
    downloads: Mutex<HashMap<String, (CancellationToken, u64)>>,
    /// Why the last download of a pack failed.
    errors: Mutex<HashMap<String, String>>,
    /// Where downloads come from, instead of GitHub (tests).
    base: Mutex<Option<String>>,
    tier: tokio::sync::OnceCell<HardwareTier>,
    /// Tests: what every recording is heard as, with no model involved (and replies are
    /// read aloud as a tone).
    #[cfg(test)]
    pub fake_words: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Voice {
    pub(crate) fn downloads(&self) -> MutexGuard<'_, HashMap<String, (CancellationToken, u64)>> {
        lock(&self.downloads)
    }

    pub(crate) fn errors(&self) -> MutexGuard<'_, HashMap<String, String>> {
        lock(&self.errors)
    }

    pub(crate) fn base(&self) -> Option<String> {
        lock(&self.base).clone()
    }

    #[cfg(test)]
    pub fn set_base(&self, base: String) {
        *lock(&self.base) = Some(base);
    }

    #[cfg(test)]
    pub fn set_tier(&self, tier: HardwareTier) {
        let _ = self.tier.set(tier);
    }

    fn set_progress(&self, id: &str, done: u64) {
        if let Some(entry) = self.downloads().get_mut(id) {
            entry.1 = done;
        }
    }

    fn touch(&self) {
        *lock(&self.last_used) = Some(Instant::now());
    }

    /// The recognizer for `entry`, loading it if needed.
    async fn listener(
        &self,
        state: &AppState,
        entry: &'static catalog::Recognizer,
    ) -> Result<Arc<engine::Listener>, String> {
        self.touch();
        let mut loaded = self.listener.lock().await;
        if let Some((id, listener)) = loaded.as_ref()
            && *id == entry.id
        {
            return Ok(listener.clone());
        }
        *loaded = None;
        let dir = download::pack_dir(&state.paths, &entry.id);
        let started = Instant::now();
        let listener = tokio::task::spawn_blocking(move || engine::Listener::load(entry, &dir))
            .await
            .map_err(|e| e.to_string())??;
        tracing::info!(recognizer = %entry.id, ms = started.elapsed().as_millis(), "speech recognizer loaded");
        let listener = Arc::new(listener);
        *loaded = Some((entry.id.clone(), listener.clone()));
        Ok(listener)
    }

    /// The model behind `voice`, loading it if needed.
    async fn speaker(
        &self,
        state: &AppState,
        pack: &'static catalog::VoicePack,
        voice: &'static catalog::Voice,
    ) -> Result<Arc<engine::Speaker>, String> {
        self.touch();
        // Kokoro is set up per language; Piper voices are a language each.
        let key = format!(
            "{}:{}",
            pack.id,
            voice.lang.as_deref().unwrap_or(&voice.language)
        );
        let mut loaded = self.speaker.lock().await;
        if let Some((k, speaker)) = loaded.as_ref()
            && *k == key
        {
            return Ok(speaker.clone());
        }
        *loaded = None;
        let dir = download::pack_dir(&state.paths, &pack.id);
        let speaker = tokio::task::spawn_blocking(move || engine::Speaker::load(pack, voice, &dir))
            .await
            .map_err(|e| e.to_string())??;
        let speaker = Arc::new(speaker);
        *loaded = Some((key, speaker.clone()));
        Ok(speaker)
    }

    /// Unloads anything loaded from a pack (before removing it).
    async fn forget(&self, id: &str) {
        let mut listener = self.listener.lock().await;
        if listener.as_ref().is_some_and(|(l, _)| l == id) {
            *listener = None;
        }
        drop(listener);
        let mut speaker = self.speaker.lock().await;
        if speaker
            .as_ref()
            .is_some_and(|(k, _)| k.split(':').next() == Some(id))
        {
            *speaker = None;
        }
    }

    /// Gives the memory back after [`IDLE_UNLOAD`] without use. Called periodically.
    pub async fn unload_if_idle(&self) {
        let idle = lock(&self.last_used).is_some_and(|t| t.elapsed() >= IDLE_UNLOAD);
        if !idle {
            return;
        }
        let mut unloaded = false;
        if let Ok(mut listener) = self.listener.try_lock() {
            unloaded |= listener.take().is_some();
        }
        if let Ok(mut speaker) = self.speaker.try_lock() {
            unloaded |= speaker.take().is_some();
        }
        if unloaded {
            tracing::info!("voice models unloaded");
        }
    }
}

/// The computer's language and region (`fr`, `FR`), from its settings.
fn home_locale() -> (String, Option<String>) {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned());
    let mut parts = locale.split(['-', '_', '.', '@']);
    let language = parts
        .next()
        .filter(|l| l.len() == 2 || l.len() == 3)
        .unwrap_or("en")
        .to_lowercase();
    let region = parts.next().filter(|r| r.len() == 2).map(str::to_uppercase);
    // "C" and "POSIX" mean nothing was set.
    let language = if language == "c" || language == "posix" {
        "en".to_owned()
    } else {
        language
    };
    (language, region)
}

/// The recognizer the user chose, or the first that understands their language.
pub fn recognizer_for(settings: &VoiceSettings) -> &'static catalog::Recognizer {
    settings
        .recognizer
        .as_deref()
        .and_then(catalog::recognizer)
        .unwrap_or_else(|| recommended_recognizer(&home_locale().0))
}

fn recommended_recognizer(language: &str) -> &'static catalog::Recognizer {
    let recognizers = &catalog::get().recognizers;
    recognizers
        .iter()
        .find(|r| r.understands(language))
        .unwrap_or(&recognizers[0])
}

/// Natural voices for computers that read them faster than they speak.
async fn recommended_style(state: &AppState) -> VoiceStyle {
    let tier = *state
        .voice
        .tier
        .get_or_init(|| async {
            tokio::task::spawn_blocking(crate::hardware::detect)
                .await
                .map(|hw| crate::hardware::recommend::tier(crate::hardware::recommend::budget(&hw)))
                .unwrap_or(HardwareTier::Light)
        })
        .await;
    if tier >= HardwareTier::Standard {
        VoiceStyle::Natural
    } else {
        VoiceStyle::Light
    }
}

/// The voice for a language: the one the user chose for it, else one of the style
/// (the computer's region first: British English in the UK), else any.
pub fn choose(
    settings: &VoiceSettings,
    style: VoiceStyle,
    language: &str,
    region: Option<&str>,
) -> Option<(&'static catalog::VoicePack, &'static catalog::Voice)> {
    if let Some(chosen) = settings
        .voices
        .get(language)
        .and_then(|id| catalog::voice(id))
    {
        return Some(chosen);
    }
    let candidates: Vec<_> = catalog::get()
        .voice_packs
        .iter()
        .flat_map(|p| p.voices.iter().map(move |v| (p, v)))
        .filter(|(_, v)| v.base_language() == language)
        .collect();
    let in_region = |v: &catalog::Voice| {
        region.is_some_and(|r| v.language.eq_ignore_ascii_case(&format!("{language}-{r}")))
    };
    let rank = |(p, v): &(&catalog::VoicePack, &catalog::Voice)| (p.style != style, !in_region(v));
    candidates.into_iter().min_by_key(rank)
}

fn pack_status(state: &AppState, id: &str) -> PackStatus {
    let (label, detail, bytes) = match (catalog::recognizer(id), catalog::pack(id)) {
        (Some(r), _) => (r.label.clone(), r.detail.clone(), r.archive.bytes),
        (_, Some(p)) => (p.label.clone(), p.detail.clone(), p.archive.bytes),
        _ => (id.to_owned(), String::new(), 0),
    };
    let downloading = state.voice.downloads().get(id).map(|(_, done)| *done);
    let state_ = if downloading.is_some() {
        PackState::Downloading
    } else if download::is_ready(&state.paths, id) {
        PackState::Ready
    } else {
        PackState::Missing
    };
    PackStatus {
        id: id.to_owned(),
        label,
        detail,
        bytes,
        state: state_,
        done_bytes: downloading,
        error: state.voice.errors().get(id).cloned(),
    }
}

pub async fn status(state: &AppState) -> VoiceStatus {
    let settings = crate::settings::load(&state.db)
        .await
        .unwrap_or_default()
        .voice;
    let recommended_style = recommended_style(state).await;
    let catalog = catalog::get();
    VoiceStatus {
        listening: pack_status(state, &recognizer_for(&settings).id),
        recognizers: catalog
            .recognizers
            .iter()
            .map(|r| pack_status(state, &r.id))
            .collect(),
        style: settings.style.unwrap_or(recommended_style),
        recommended_style,
        voices: catalog
            .voice_packs
            .iter()
            .flat_map(|p| {
                p.voices.iter().map(move |v| VoiceInfo {
                    id: v.id.clone(),
                    name: v.name.clone(),
                    language: v.language.clone(),
                    language_name: v.language_name.clone(),
                    style: p.style,
                    pack: p.id.clone(),
                })
            })
            .collect(),
        packs: catalog
            .voice_packs
            .iter()
            .map(|p| pack_status(state, &p.id))
            .filter(|p| p.state != PackState::Missing || p.error.is_some())
            .collect(),
    }
}

/// Tells clients what changed (downloads, settings).
pub fn publish(state: &Arc<AppState>) {
    let state = state.clone();
    tokio::spawn(async move {
        let voice = status(&state).await;
        state.events.publish(Event::VoiceChanged { voice });
    });
}

/// Starts downloading a recognizer or voice pack.
pub fn start_download(state: &Arc<AppState>, id: &str) -> Result<(), String> {
    download::start(state, id)
}

pub fn cancel_download(state: &AppState, id: &str) -> bool {
    download::cancel(state, id)
}

/// Deletes a pack from this computer.
pub async fn remove(state: &Arc<AppState>, id: &str) -> Result<(), String> {
    if catalog::archive(id).is_none() {
        return Err("Mimi doesn't know those voice files.".to_owned());
    }
    download::cancel(state, id);
    state.voice.forget(id).await;
    let paths = state.paths.clone();
    let id_owned = id.to_owned();
    tokio::task::spawn_blocking(move || download::remove(&paths, &id_owned))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Couldn't remove the voice files: {e}"))?;
    state.voice.errors().remove(id);
    publish(state);
    Ok(())
}

/// Loads the recognizer ahead of time (the user started recording), if it's here.
pub fn prepare(state: &Arc<AppState>) {
    let state = state.clone();
    tokio::spawn(async move {
        let settings = crate::settings::load(&state.db)
            .await
            .unwrap_or_default()
            .voice;
        let entry = recognizer_for(&settings);
        if download::is_ready(&state.paths, &entry.id)
            && let Err(e) = state.voice.listener(&state, entry).await
        {
            tracing::warn!("loading the speech recognizer failed: {e}");
        }
    });
}

/// Why nothing was heard.
#[derive(Debug)]
pub enum ListenError {
    /// The recognizer isn't on this computer yet.
    NotReady(Box<PackStatus>),
    /// Something went wrong; the message is for the user.
    Failed(String),
}

impl ListenError {
    pub fn message(&self) -> String {
        match self {
            ListenError::NotReady(_) => {
                "Mimi needs to download what it uses to understand speech first.".to_owned()
            }
            ListenError::Failed(message) => message.clone(),
        }
    }
}

/// The words in a recording.
pub async fn transcribe(state: &AppState, bytes: Vec<u8>) -> Result<Transcript, ListenError> {
    let settings = crate::settings::load(&state.db)
        .await
        .unwrap_or_default()
        .voice;
    let entry = recognizer_for(&settings);
    #[cfg(test)]
    let fake = lock(&state.voice.fake_words).clone();
    #[cfg(not(test))]
    let fake: Option<String> = None;
    if fake.is_none() && !download::is_ready(&state.paths, &entry.id) {
        return Err(ListenError::NotReady(Box::new(pack_status(
            state, &entry.id,
        ))));
    }
    let failed = |e: String| ListenError::Failed(e);
    let sound = tokio::task::spawn_blocking(move || audio::decode(bytes))
        .await
        .map_err(|e| failed(e.to_string()))?
        .map_err(failed)?;
    let duration_ms = sound.duration_ms();
    if let Some(text) = fake {
        return Ok(Transcript { text, duration_ms });
    }
    if !audio::has_sound(&sound) {
        return Ok(Transcript {
            text: String::new(),
            duration_ms,
        });
    }
    let listener = state.voice.listener(state, entry).await.map_err(failed)?;
    let started = Instant::now();
    let text = tokio::task::spawn_blocking(move || listener.transcribe(sound))
        .await
        .map_err(|e| failed(e.to_string()))?;
    state.voice.touch();
    // How long, never what.
    tracing::debug!(
        audio_ms = duration_ms,
        ms = started.elapsed().as_millis(),
        "transcribed"
    );
    Ok(Transcript { text, duration_ms })
}

/// A short sentence in a voice's language, to hear it.
fn sample(language: &str) -> &'static str {
    match catalog::base_language(language) {
        "fr" => "Bonjour ! Voici ma voix quand je lis à voix haute.",
        "de" => "Hallo! So klinge ich, wenn ich vorlese.",
        "es" => "¡Hola! Así sueno cuando leo en voz alta.",
        "it" => "Ciao! Ecco come suono quando leggo ad alta voce.",
        "pt" => "Olá! É assim que eu soo quando leio em voz alta.",
        "nl" => "Hallo! Zo klink ik als ik voorlees.",
        "pl" => "Cześć! Tak brzmię, kiedy czytam na głos.",
        "ru" => "Привет! Так звучит мой голос, когда я читаю вслух.",
        "uk" => "Привіт! Так звучить мій голос, коли я читаю вголос.",
        "sv" => "Hej! Så här låter jag när jag läser högt.",
        "da" => "Hej! Sådan lyder jeg, når jeg læser højt.",
        "nb" => "Hei! Slik høres jeg ut når jeg leser høyt.",
        "fi" => "Hei! Tältä kuulostan, kun luen ääneen.",
        "cs" => "Ahoj! Takhle zním, když čtu nahlas.",
        "sk" => "Ahoj! Takto znejem, keď čítam nahlas.",
        "sl" => "Živjo! Tako zvenim, ko berem na glas.",
        "hu" => "Szia! Így hangzom, amikor felolvasok.",
        "ro" => "Bună! Așa sun când citesc cu voce tare.",
        "lv" => "Sveiki! Tā es skanu, kad lasu skaļi.",
        "el" => "Γεια σου! Έτσι ακούγομαι όταν διαβάζω δυνατά.",
        "vi" => "Xin chào! Đây là giọng của tôi khi đọc to.",
        "zh" => "你好！这是我朗读时的声音。",
        _ => "Hello! This is how I sound when I read aloud.",
    }
}

/// The voice for a text: the language it's in, else the computer's.
async fn voice_for(
    state: &AppState,
    settings: &VoiceSettings,
    plain: &str,
) -> Result<(&'static catalog::VoicePack, &'static catalog::Voice), String> {
    let style = settings.style.unwrap_or(recommended_style(state).await);
    let (home, region) = home_locale();
    let language = speakable::language(plain).unwrap_or(&home);
    choose(settings, style, language, region.as_deref())
        .ok_or_else(|| "There's no voice for this language yet.".to_owned())
}

fn speed(settings: &VoiceSettings) -> f32 {
    f32::from(settings.speed.clamp(75, 150)) / 100.0
}

/// One part of a text as speech (see [`speakable::parts`]).
pub async fn speak(state: &Arc<AppState>, req: SpeakRequest) -> Result<Speech, String> {
    let settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .voice;
    let forced = match req.voice.as_deref() {
        Some(id) => Some(catalog::voice(id).ok_or("Mimi doesn't know that voice.")?),
        None => None,
    };
    let text = match forced {
        Some((_, v)) if req.text.trim().is_empty() => sample(&v.language).to_owned(),
        _ => req.text,
    };
    let plain = speakable::plain(&text);
    let parts = speakable::parts(&plain);
    let Some(part) = parts.get(req.part as usize) else {
        return Ok(Speech::Nothing);
    };
    let (pack, voice) = match forced {
        Some(chosen) => chosen,
        None => voice_for(state, &settings, &plain).await?,
    };
    if !download::is_ready(&state.paths, &pack.id) {
        download::start(state, &pack.id)?;
        return Ok(Speech::Downloading {
            pack: pack_status(state, &pack.id),
        });
    }
    let speaker = state.voice.speaker(state, pack, voice).await?;
    let (part, sid, speed) = (part.clone(), voice.speaker, speed(&settings));
    let sound = tokio::task::spawn_blocking(move || speaker.speak(&part, sid, speed))
        .await
        .map_err(|e| e.to_string())??;
    state.voice.touch();
    Ok(Speech::Audio {
        audio: base64::engine::general_purpose::STANDARD.encode(audio::wav(&sound)),
        parts: parts.len() as u32,
    })
}

/// A whole reply as a voice message, if its voice is on this computer (else its
/// download starts, and this one goes without).
pub async fn voice_message(
    state: &Arc<AppState>,
    markdown: &str,
) -> Result<Option<audio::VoiceNote>, String> {
    let settings = crate::settings::load(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .voice;
    let plain = speakable::plain(markdown);
    let parts = speakable::parts(&plain);
    if parts.is_empty() {
        return Ok(None);
    }
    #[cfg(test)]
    if lock(&state.voice.fake_words).is_some() {
        let tone = audio::Sound {
            rate: 24_000,
            samples: (0..24_000).map(|i| (i as f32 / 10.0).sin() * 0.3).collect(),
        };
        return audio::voice_note(tone).map(Some);
    }
    let (pack, voice) = voice_for(state, &settings, &plain).await?;
    if !download::is_ready(&state.paths, &pack.id) {
        download::start(state, &pack.id)?;
        return Ok(None);
    }
    let speaker = state.voice.speaker(state, pack, voice).await?;
    let (sid, speed) = (voice.speaker, speed(&settings));
    let note = tokio::task::spawn_blocking(move || -> Result<audio::VoiceNote, String> {
        let mut all = audio::Sound {
            rate: 0,
            samples: Vec::new(),
        };
        for part in &parts {
            let sound = speaker.speak(part, sid, speed)?;
            if all.rate == 0 {
                all.rate = sound.rate;
            } else {
                let pause = (all.rate * PAUSE_MS / 1000) as usize;
                all.samples.extend(std::iter::repeat_n(0.0, pause));
            }
            all.samples.extend(sound.samples);
        }
        audio::voice_note(all)
    })
    .await
    .map_err(|e| e.to_string())??;
    state.voice.touch();
    Ok(Some(note))
}

/// Waits (up to `timeout`) for a pack to finish downloading. False if it failed or
/// stopped.
pub async fn wait_until_ready(state: &AppState, id: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if download::is_ready(&state.paths, id) {
            return true;
        }
        if !state.voice.downloads().contains_key(id) {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    false
}

/// Checks voice settings from a client before they're saved.
pub fn validate(settings: &mut VoiceSettings) -> Result<(), String> {
    settings.speed = settings.speed.clamp(75, 150);
    if let Some(id) = &settings.recognizer
        && catalog::recognizer(id).is_none()
    {
        return Err("Mimi doesn't know that speech recognizer.".to_owned());
    }
    for (language, id) in &settings.voices {
        match catalog::voice(id) {
            Some((_, v)) if v.base_language() == language => {}
            _ => return Err("Mimi doesn't know that voice.".to_owned()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
