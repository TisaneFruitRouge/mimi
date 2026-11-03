//! Whether a model can see pictures. Photos only go to models that can; the others are
//! told that the user sent some, and the user is told the model can't see them.
//!
//! - Anthropic: every Claude model can.
//! - The built-in runtime: no. Its catalog models are run without their picture encoder
//!   (llama-server's `--mmproj` file), which isn't downloaded.
//! - OpenAI-compatible sources are asked, in their own way: Ollama's `/api/show`
//!   (`capabilities` has `vision`), LM Studio's `/api/v0/models/{id}` (`type: vlm`) and
//!   llama.cpp's `/props` (`modalities.vision`) for sources on the user's machines;
//!   `/models` for all (OpenRouter's `architecture.input_modalities`, Mistral's
//!   `capabilities.vision`); and OpenAI's own model families on its API. Anything else
//!   can't be told, which counts as no.
//!
//! Answers are kept for ten minutes, per source and model.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use mimi_protocol::ProviderKind;
use uuid::Uuid;

use super::store::ProviderRecord;

const FRESH: Duration = Duration::from_secs(10 * 60);

/// What each source said about each model, and when: (source, model) → (sees, asked).
type Answers = HashMap<(Uuid, String), (bool, Instant)>;

static KNOWN: LazyLock<Mutex<Answers>> = LazyLock::new(Default::default);

/// Whether `model` from this source can see pictures.
pub async fn sees_images(state: &crate::AppState, record: &ProviderRecord, model: &str) -> bool {
    match record.provider.kind {
        ProviderKind::Anthropic => return true,
        ProviderKind::Builtin => return false,
        ProviderKind::OpenaiCompatible => {}
    }
    let key = (record.provider.id, model.to_owned());
    if let Some((sees, at)) = KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .copied()
        && at.elapsed() < FRESH
    {
        return sees;
    }
    let Ok(client) = super::connect_openai(&state.http, record) else {
        return false;
    };
    // An unreachable source is asked again next time.
    let Some(sees) = client.sees_images(model).await else {
        return false;
    };
    KNOWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, (sees, Instant::now()));
    sees
}

/// Model families on OpenAI's own API that see pictures (its `/models` doesn't say).
pub(super) fn openai_family_sees(model: &str) -> bool {
    const FAMILIES: &[&str] = &[
        "gpt-4o",
        "chatgpt-4o",
        "gpt-4.1",
        "gpt-4.5",
        "gpt-5",
        "o1",
        "o3",
        "o4",
    ];
    let model = model.to_ascii_lowercase();
    FAMILIES.iter().any(|f| {
        model == *f
            || model
                .strip_prefix(f)
                .is_some_and(|rest| rest.starts_with(['-', '.']))
    }) && !model.contains("audio")
        && !model.contains("realtime")
        && !model.contains("tts")
        && !model.contains("transcribe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_families() {
        for yes in [
            "gpt-4o",
            "gpt-4o-mini",
            "gpt-4.1-nano",
            "gpt-5",
            "gpt-5.2-mini",
            "o3",
            "o4-mini",
        ] {
            assert!(openai_family_sees(yes), "{yes}");
        }
        for no in [
            "gpt-3.5-turbo",
            "gpt-4o-audio-preview",
            "gpt-4o-mini-tts",
            "o1x",
            "davinci-002",
            "gpt-4",
        ] {
            assert!(!openai_family_sees(no), "{no}");
        }
    }
}
