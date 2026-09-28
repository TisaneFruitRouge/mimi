use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::Permissions;

/// User-editable settings. Every field has a default so settings saved by an older
/// version keep loading after new fields are added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    /// What the assistant calls itself.
    pub assistant_name: String,
    /// Model used for new messages. `None` until the user finishes setup.
    pub default_model: Option<ModelRef>,
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
}

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
            default_model: None,
            memory_learning: true,
            pending_model: None,
            onboarding_done: false,
            onboarding_step: 0,
            desktop_notifications: true,
            memory_semantic: false,
            mail_sorting: true,
            mail_sorter: MailSorter::Model,
            permissions: Permissions::default(),
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
