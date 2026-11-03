//! What can be downloaded to listen and speak (`catalog.json`): speech recognizers and
//! packs of voices, each one archive pinned by size and SHA-256.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use mimi_protocol::VoiceStyle;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub recognizers: Vec<Recognizer>,
    pub voice_packs: Vec<VoicePack>,
}

/// A downloadable archive, unpacked without its top folder.
#[derive(Debug, Clone, Deserialize)]
pub struct Archive {
    pub url: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecognizerEngine {
    /// NVIDIA's Parakeet: a NeMo transducer.
    NemoTransducer,
    /// OpenAI's Whisper: at most 30 seconds at a time.
    Whisper,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Recognizer {
    pub id: String,
    pub engine: RecognizerEngine,
    pub label: String,
    pub detail: String,
    /// ISO 639-1 codes, or `*` for all.
    pub languages: Vec<String>,
    pub archive: Archive,
    /// Paths inside the unpacked archive: `encoder`, `decoder`, `joiner`, `tokens`.
    pub files: BTreeMap<String, String>,
}

impl Recognizer {
    pub fn understands(&self, language: &str) -> bool {
        self.languages.iter().any(|l| l == "*" || l == language)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceEngine {
    Kokoro,
    /// Piper's VITS voices.
    Vits,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoicePack {
    pub id: String,
    pub style: VoiceStyle,
    pub engine: VoiceEngine,
    pub label: String,
    pub detail: String,
    pub archive: Archive,
    /// Paths inside the unpacked archive: `model`, `tokens`, `data_dir`, and for Kokoro
    /// `voices` and `dict_dir`.
    pub files: BTreeMap<String, String>,
    pub voices: Vec<Voice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// A BCP-47 tag: `fr`, `en-US`.
    pub language: String,
    pub language_name: String,
    /// The speaker within the model.
    pub speaker: i32,
    /// Kokoro: the language it reads (`en-us`, `fr`…).
    #[serde(default)]
    pub lang: Option<String>,
    /// Kokoro: its pronunciation dictionaries.
    #[serde(default)]
    pub lexicon: Vec<String>,
}

impl Voice {
    /// The language without its region: `en` for `en-US`.
    pub fn base_language(&self) -> &str {
        base_language(&self.language)
    }
}

/// `en` for `en-US` or `en_GB`.
pub fn base_language(tag: &str) -> &str {
    tag.split(['-', '_']).next().unwrap_or(tag)
}

pub fn get() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("catalog.json")).expect("the voice catalog is valid")
    })
}

pub fn recognizer(id: &str) -> Option<&'static Recognizer> {
    get().recognizers.iter().find(|r| r.id == id)
}

pub fn pack(id: &str) -> Option<&'static VoicePack> {
    get().voice_packs.iter().find(|p| p.id == id)
}

/// A voice and the pack it comes in.
pub fn voice(id: &str) -> Option<(&'static VoicePack, &'static Voice)> {
    get()
        .voice_packs
        .iter()
        .find_map(|p| p.voices.iter().find(|v| v.id == id).map(|v| (p, v)))
}

/// The archive behind a recognizer or voice pack id.
pub fn archive(id: &str) -> Option<&'static Archive> {
    recognizer(id)
        .map(|r| &r.archive)
        .or_else(|| pack(id).map(|p| &p.archive))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_download_is_pinned_and_every_id_unique() {
        let catalog = get();
        // Packs and recognizers share one namespace (`archive`), voices another.
        let mut ids = HashSet::new();
        let mut voices = HashSet::new();
        for r in &catalog.recognizers {
            assert!(ids.insert(r.id.clone()), "{}", r.id);
            assert_eq!(r.archive.sha256.len(), 64, "{}", r.id);
            assert!(
                r.archive
                    .url
                    .starts_with("https://github.com/k2-fsa/sherpa-onnx/")
            );
            assert!(r.files.contains_key("encoder") && r.files.contains_key("tokens"));
        }
        for p in &catalog.voice_packs {
            assert!(ids.insert(p.id.clone()), "{}", p.id);
            assert_eq!(p.archive.sha256.len(), 64, "{}", p.id);
            assert!(
                p.archive
                    .url
                    .starts_with("https://github.com/k2-fsa/sherpa-onnx/")
            );
            assert!(p.files.contains_key("model") && p.files.contains_key("tokens"));
            assert!(!p.voices.is_empty(), "{}", p.id);
            for v in &p.voices {
                assert!(voices.insert(v.id.clone()), "{}", v.id);
                if p.engine == VoiceEngine::Kokoro {
                    assert!(v.lang.is_some(), "{}", v.id);
                }
            }
        }
    }

    #[test]
    fn every_light_language_has_one_pack_of_its_own() {
        // A light voice is downloaded per language, so a language must not need two.
        let mut seen = HashSet::new();
        for p in get()
            .voice_packs
            .iter()
            .filter(|p| p.style == VoiceStyle::Light)
        {
            let languages: HashSet<&str> = p.voices.iter().map(|v| v.language.as_str()).collect();
            assert_eq!(languages.len(), 1, "{}", p.id);
            for l in languages {
                assert!(seen.insert(l.to_owned()), "{l}");
            }
        }
    }

    #[test]
    fn languages_without_regions_match_tags_with_them() {
        assert_eq!(base_language("en-US"), "en");
        assert_eq!(base_language("pt_BR"), "pt");
        assert_eq!(base_language("fr"), "fr");
        assert!(recognizer("parakeet-v3").unwrap().understands("fr"));
        assert!(!recognizer("parakeet-v3").unwrap().understands("ja"));
        assert!(recognizer("whisper-turbo").unwrap().understands("ja"));
    }
}
