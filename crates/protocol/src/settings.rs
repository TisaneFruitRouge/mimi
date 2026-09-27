use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            assistant_name: "Mimi".to_owned(),
            default_model: None,
            memory_learning: true,
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
