//! What the assistant may do without asking first, as the user chose in Settings ›
//! Permissions ([`mimi_protocol::Permissions`]).

use mimi_protocol::{Autonomy, Permissions};
use serde_json::Value;

use super::Tool;
use crate::AppState;

/// The kinds of action a permission setting covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Governs {
    SendMail,
    AddEvents,
    Schedule,
}

fn setting(permissions: &Permissions, kind: Governs) -> Autonomy {
    match kind {
        Governs::SendMail => permissions.send_mail,
        Governs::AddEvents => permissions.add_events,
        Governs::Schedule => permissions.schedule,
    }
}

/// Whether a call may end up on an approval card, so its arguments must be prepared in
/// the shape the card shows (see [`Tool::prepare`]).
pub fn may_ask(tool: &dyn Tool, args: &Value, permissions: &Permissions) -> bool {
    tool.needs_approval(args)
        || tool
            .governed_by()
            .is_some_and(|kind| setting(permissions, kind) == Autonomy::Ask)
}

/// Whether this call waits for the user's approval. `args` are the prepared arguments.
///
/// Automatic email still asks unless every recipient is someone the user knows: this
/// keeps the defence against a hostile email that tells the assistant to write to its
/// sender's accomplice.
pub async fn requires_approval(
    state: &AppState,
    tool: &dyn Tool,
    args: &Value,
    permissions: &Permissions,
) -> bool {
    let Some(kind) = tool.governed_by() else {
        return tool.needs_approval(args);
    };
    match (setting(permissions, kind), kind) {
        (Autonomy::Ask, _) => true,
        (Autonomy::Automatic, Governs::SendMail) => {
            let recipients: Vec<String> = ["to", "cc"]
                .iter()
                .flat_map(|key| args[*key].as_array().cloned().unwrap_or_default())
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            !crate::mail::known::all_known(state, &recipients).await
        }
        (Autonomy::Automatic, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use futures::future::BoxFuture;
    use serde_json::json;

    use super::*;
    use crate::tools::ToolContext;

    struct Fake(bool, Option<Governs>);

    impl Tool for Fake {
        fn name(&self) -> &str {
            "fake"
        }
        fn description(&self) -> &str {
            ""
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }
        fn needs_approval(&self, _: &Value) -> bool {
            self.0
        }
        fn governed_by(&self) -> Option<Governs> {
            self.1
        }
        fn summary(&self, _: &Value) -> String {
            String::new()
        }
        fn run<'a>(&'a self, _: &'a ToolContext, _: Value) -> BoxFuture<'a, Result<Value, String>> {
            Box::pin(async { Ok(Value::Null) })
        }
    }

    #[tokio::test]
    async fn settings_decide_only_for_the_actions_they_cover() {
        let state = AppState::for_tests("t");
        let args = json!({});
        let defaults = Permissions::default();
        let all_automatic = Permissions {
            send_mail: Autonomy::Automatic,
            add_events: Autonomy::Automatic,
            schedule: Autonomy::Automatic,
        };
        let all_ask = Permissions {
            send_mail: Autonomy::Ask,
            add_events: Autonomy::Ask,
            schedule: Autonomy::Ask,
        };
        let ask = |tool: Fake, p: Permissions| {
            let state = &state;
            let args = &args;
            async move { requires_approval(state, &tool, args, &p).await }
        };

        // Tools no setting covers keep their own rule, whatever the settings say.
        assert!(ask(Fake(true, None), all_automatic).await);
        assert!(!ask(Fake(false, None), all_ask).await);

        // Events: ask by default, automatic when allowed.
        let event = || Fake(true, Some(Governs::AddEvents));
        assert!(ask(event(), defaults).await);
        assert!(!ask(event(), all_automatic).await);

        // Reminders: automatic by default, as before; asking when the user wants that,
        // with the arguments prepared for the card.
        let reminder = || Fake(false, Some(Governs::Schedule));
        assert!(!ask(reminder(), defaults).await);
        assert!(ask(reminder(), all_ask).await);
        assert!(may_ask(&reminder(), &args, &all_ask));
        assert!(!may_ask(&reminder(), &args, &defaults));

        // Automatic email still asks for strangers, for nobody, and for non-addresses.
        let send = |to: Value| {
            let state = &state;
            async move {
                requires_approval(
                    state,
                    &Fake(true, Some(Governs::SendMail)),
                    &json!({"to": to}),
                    &all_automatic,
                )
                .await
            }
        };
        assert!(send(json!(["stranger@example.net"])).await);
        assert!(send(json!([])).await);
        assert!(send(json!(["not an address"])).await);
    }
}
