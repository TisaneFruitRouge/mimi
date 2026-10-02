use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::{Permissions, VoiceSettings};

/// User-editable settings. Every field has a default so settings saved by an older
/// version keep loading after new fields are added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    /// What the assistant calls itself.
    pub assistant_name: String,
    /// Who the assistant is and how it talks, in the user's own words (Settings ›
    /// Personality). Empty means the default voice: helpful, direct and warm. At most
    /// [`PERSONALITY_LIMIT`] characters.
    pub personality: String,
    /// Standing instructions the user gives for every conversation ("answer in French
    /// unless I write in English"). At most [`INSTRUCTIONS_LIMIT`] characters.
    pub custom_instructions: String,
    /// Model used for new messages. `None` until the user finishes setup.
    pub default_model: Option<ModelRef>,
    /// Optional model that answers messages with photos (and the message right after)
    /// when the default model can't see pictures. `None`: the default answers everything.
    pub photo_model: Option<ModelRef>,
    /// Whether the assistant learns new things about the user from conversations.
    pub memory_learning: bool,
    /// The model chosen during setup while it's still downloading. When its download
    /// finishes, the daemon makes it the default model, even if no window is open.
    pub pending_model: Option<ModelRef>,
    /// Whether the first-run welcome has been completed.
    pub onboarding_done: bool,
    /// Where the welcome was left, so closing the app midway resumes there.
    pub onboarding_step: u8,
    /// Whether reminders and routine results also show as notifications on this computer.
    pub desktop_notifications: bool,
    /// Whether memories are also found by meaning, not only by their words ("my sibling"
    /// finds the note about a sister). Needs a small language file on this computer.
    pub memory_semantic: bool,
    /// Whether new mail is sorted (needs a reply / important / everything else) and
    /// summarised in the background by the active model.
    pub mail_sorting: bool,
    /// What sorts it: the user's own model (the default), or a cloud decision model the
    /// user chose.
    pub mail_sorter: MailSorter,
    /// What the assistant may do without asking first.
    pub permissions: Permissions,
    /// Whether this computer asks GitHub, about once a day, if a newer version of Mimi
    /// has been published. Off unless the user turns it on.
    pub update_check: bool,
    /// Talking to the assistant and hearing it (Settings › Voice).
    pub voice: VoiceSettings,
}

/// Longest personality, in characters. It goes into every prompt, next to the memory
/// profile (1,200) and recalled notes (1,600), so it stays small for ~8k-token models.
pub const PERSONALITY_LIMIT: usize = 600;
/// Longest set of custom instructions, in characters. Same reasoning as
/// [`PERSONALITY_LIMIT`]; instructions tend to be a list, so they get a little more.
pub const INSTRUCTIONS_LIMIT: usize = 1000;

/// What sorts new mail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MailSorter {
    /// The model chosen in Models (on this computer by default).
    #[default]
    Model,
    /// Jev by TypeSafe: a cloud service that answers typed questions. Mail it sorts is
    /// sent to TypeSafe. Needs the user's own API key.
    Jev,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            assistant_name: "Mimi".to_owned(),
            personality: String::new(),
            custom_instructions: String::new(),
            default_model: None,
            photo_model: None,
            memory_learning: true,
            pending_model: None,
            onboarding_done: false,
            onboarding_step: 0,
            desktop_notifications: true,
            memory_semantic: false,
            mail_sorting: true,
            mail_sorter: MailSorter::Model,
            permissions: Permissions::default(),
            update_check: false,
            voice: VoiceSettings::default(),
        }
    }
}

/// A model offered by a configured provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelRef {
    pub provider_id: Uuid,
    pub model: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_personality_still_load() {
        let old = r#"{"assistant_name":"Ember","default_model":null,"memory_learning":false}"#;
        let settings: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.assistant_name, "Ember");
        assert!(!settings.memory_learning);
        assert_eq!(settings.personality, "");
        assert_eq!(settings.custom_instructions, "");

        let json = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }
}
