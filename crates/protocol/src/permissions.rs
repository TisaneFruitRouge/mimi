//! What the assistant may do without asking first (Settings › Permissions).
//!
//! The daemon defines the kinds of action (send email, add events…) and serves them as
//! [`PermissionKind`]s; clients render whatever it serves. The user's choices are stored
//! as [`Permissions`]: per kind, a default plus exceptions for particular people or
//! calendars.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Autonomy {
    /// Shown as a card the user approves or declines.
    Ask,
    /// Done straight away, and shown in the chat afterwards.
    Automatic,
}

/// The user's choices, by kind id (`send_mail`, `add_events`…). A kind that isn't
/// listed uses its default. Changed only through `PUT /v1/permissions/{kind}` (and the
/// "don't ask again" choice on an approval card): never by the assistant.
///
/// Settings saved before exceptions existed (`{"send_mail": "automatic", …}`) still load
/// with the same meaning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Permissions(pub BTreeMap<String, KindPermission>);

/// One kind's choice: what happens by default, and the exceptions to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct KindPermission {
    pub autonomy: Autonomy,
    /// Exceptions for particular people or calendars. The most specific wins.
    pub rules: Vec<PermissionRule>,
}

impl<'de> Deserialize<'de> for KindPermission {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        /// Either the old bare choice or the full shape.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Bare(Autonomy),
            Full {
                autonomy: Autonomy,
                #[serde(default)]
                rules: Vec<PermissionRule>,
            },
        }
        Ok(match Repr::deserialize(d)? {
            Repr::Bare(autonomy) => Self {
                autonomy,
                rules: Vec::new(),
            },
            Repr::Full { autonomy, rules } => Self { autonomy, rules },
        })
    }
}

/// An exception: this person or calendar gets `autonomy` instead of the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PermissionRule {
    pub target: PermissionTarget,
    pub autonomy: Autonomy,
}

/// What an exception is about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionTarget {
    /// Someone in People, by id: matched through their email addresses.
    Person(Uuid),
    /// One calendar, by its stable id (`CalendarInfo.id`).
    Calendar(String),
}

/// What a kind's exceptions can be about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionTargetKind {
    /// No exceptions: one choice for the whole kind.
    None,
    Person,
    Calendar,
}

/// A kind of action, as `GET /v1/permissions` serves it: how to show it, and the user's
/// current choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PermissionKind {
    /// Stable, e.g. `send_mail`.
    pub id: String,
    /// "Send emails".
    pub title: String,
    /// What asking first means for this kind, one plain sentence.
    pub ask_detail: String,
    /// What automatic means, one plain sentence.
    pub automatic_detail: String,
    /// A safety net that holds whatever the choice, in one plain sentence.
    pub note: Option<String>,
    /// A lucide icon name, e.g. `send`.
    pub icon: String,
    /// `#rrggbb` for the icon tile.
    pub color: String,
    pub default_autonomy: Autonomy,
    pub targets: PermissionTargetKind,
    /// The user's choice for everything without an exception.
    pub autonomy: Autonomy,
    pub rules: Vec<PermissionRuleView>,
}

/// An exception as clients show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PermissionRuleView {
    pub target: PermissionTarget,
    pub autonomy: Autonomy,
    /// The person's or calendar's name.
    pub label: String,
    /// The person or calendar no longer exists; the exception does nothing.
    pub missing: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_exceptions_keep_their_meaning() {
        let old: Permissions = serde_json::from_str(
            r#"{"send_mail": "automatic", "add_events": "ask", "schedule": "automatic"}"#,
        )
        .unwrap();
        assert_eq!(old.0["send_mail"].autonomy, Autonomy::Automatic);
        assert_eq!(old.0["add_events"].autonomy, Autonomy::Ask);
        assert!(old.0["schedule"].rules.is_empty());

        let person = Uuid::from_u128(7);
        let new = Permissions(BTreeMap::from([(
            "send_mail".to_owned(),
            KindPermission {
                autonomy: Autonomy::Ask,
                rules: vec![PermissionRule {
                    target: PermissionTarget::Person(person),
                    autonomy: Autonomy::Automatic,
                }],
            },
        )]));
        let json = serde_json::to_value(&new).unwrap();
        assert_eq!(
            json["send_mail"]["rules"][0]["target"],
            serde_json::json!({"kind": "person", "id": person})
        );
        assert_eq!(serde_json::from_value::<Permissions>(json).unwrap(), new);
    }
}
