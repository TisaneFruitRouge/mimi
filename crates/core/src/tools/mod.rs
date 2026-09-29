//! Tools the assistant can use while replying: reading a calendar, sending a message…
//!
//! Integrations contribute tools through a [`ToolSource`] registered on [`ToolSources`]
//! at startup. Each reply builds a fresh [`ToolRegistry`] from the sources, so a tool
//! only appears while its integration is connected.
//!
//! The approval rule: anything that sends, changes or deletes something on the user's
//! behalf returns `true` from [`Tool::needs_approval`] and waits for the user; reads
//! don't. The user may let a few kinds of action happen on their own (Settings ›
//! Permissions): a tool opts in with [`Tool::governed_by`] (and says who or what the call
//! is about with [`Tool::call_targets`]), and [`permissions::requires_approval`] makes
//! the final call.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::AppState;
use crate::providers::{FunctionSpec, ToolSpec};

#[cfg(debug_assertions)]
pub mod dev;
pub mod permissions;

pub use permissions::{CallTarget, Governs};

/// What a tool gets to work with.
pub struct ToolContext {
    pub state: Arc<AppState>,
    pub conversation_id: Uuid,
}

pub trait Tool: Send + Sync {
    /// Unique, stable, `snake_case`: the model calls it by this name.
    fn name(&self) -> &str;
    /// Tells the model when and how to use it.
    fn description(&self) -> &str;
    /// JSON Schema of the arguments object.
    fn parameters(&self) -> Value;
    /// Whether this call sends, changes or deletes something, so the user must approve.
    fn needs_approval(&self, args: &Value) -> bool;
    /// The permission setting that decides instead of [`Tool::needs_approval`], for the
    /// few kinds of action the user may let happen on their own.
    fn governed_by(&self) -> Option<Governs> {
        None
    }
    /// Who or what this call is about, for the exceptions of its permission ("email Sam
    /// without asking", "always ask before writing to the Family calendar"). Read from
    /// the prepared (and resolved) arguments.
    fn call_targets(&self, _args: &Value) -> Vec<CallTarget> {
        Vec::new()
    }
    /// Looks up what the arguments refer to before anything is decided, e.g. the event a
    /// change is about, and writes it into the arguments so the card (and the permission)
    /// see the real thing, not the model's description of it. Values it writes replace
    /// anything the model put under the same keys.
    fn resolve<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(std::future::ready(Ok(args)))
    }
    /// One plain-language line for the approval card, e.g.
    /// "Create “Dentist” on Friday 10:00–10:45 in Personal".
    fn summary(&self, args: &Value) -> String;
    /// Short past-tense line once it ran, e.g. "read calendar". Defaults to the summary.
    fn result_label(&self, args: &Value, _output: &Value) -> String {
        self.summary(args)
    }
    /// Puts the arguments of a call that needs approval in the shape the tool reads,
    /// before the card is shown, so the user approves exactly what will run. Defaults
    /// to [`conform`] with [`Tool::parameters`].
    fn prepare(&self, args: Value) -> Result<Value, String> {
        conform(&self.parameters(), args)
    }
    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value)
    -> BoxFuture<'a, Result<Value, String>>;
}

/// Checks arguments against a JSON Schema's declared types and converts the unambiguous
/// mismatches models make (`"cc": "a@x"` becomes `["a@x"]`, `"3"` an integer, `null`
/// dropped). Anything else that doesn't fit is refused, so a tool never reads a value in
/// a shape the approval card didn't show. Arguments the schema doesn't list are kept as
/// they are: the card shows them too.
pub fn conform(schema: &Value, args: Value) -> Result<Value, String> {
    let Value::Object(args) = args else {
        return Err("The arguments should be a JSON object.".to_owned());
    };
    let mut out = serde_json::Map::new();
    for (key, value) in args {
        if value.is_null() {
            continue;
        }
        let value = match &schema["properties"][&key] {
            Value::Null => value,
            property => conform_value(property, value).map_err(|expected| {
                format!("`{key}` should be {expected}. Call the tool again with that.")
            })?,
        };
        out.insert(key, value);
    }
    Ok(Value::Object(out))
}

/// One value against its schema; on a mismatch, what was expected ("a list").
fn conform_value(schema: &Value, value: Value) -> Result<Value, &'static str> {
    match (schema["type"].as_str(), value) {
        (Some("string"), v @ Value::String(_)) => Ok(v),
        (Some("string"), Value::Number(n)) => Ok(Value::String(n.to_string())),
        (Some("string"), Value::Bool(b)) => Ok(Value::String(b.to_string())),
        (Some("string"), _) => Err("text"),
        (Some("integer"), Value::Number(n)) => n
            .as_i64()
            .or_else(|| {
                n.as_f64()
                    .filter(|f| f.fract() == 0.0 && f.abs() < 9e15)
                    .map(|f| f as i64)
            })
            .map(Value::from)
            .ok_or("a whole number"),
        (Some("integer"), Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| "a whole number"),
        (Some("integer"), _) => Err("a whole number"),
        (Some("number"), v @ Value::Number(_)) => Ok(v),
        (Some("number"), Value::String(s)) => s
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or("a number"),
        (Some("number"), _) => Err("a number"),
        (Some("boolean"), v @ Value::Bool(_)) => Ok(v),
        (Some("boolean"), Value::String(s)) => match s.trim() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("true or false"),
        },
        (Some("boolean"), _) => Err("true or false"),
        (Some("array"), Value::Array(items)) => items
            .into_iter()
            .filter(|v| !v.is_null())
            .map(|v| conform_value(&schema["items"], v))
            .collect::<Result<_, _>>()
            .map(Value::Array)
            .map_err(|_| "a list"),
        (Some("array"), Value::Object(_)) => Err("a list"),
        (Some("array"), v) => conform_value(&schema["items"], v)
            .map(|v| Value::Array(vec![v]))
            .map_err(|_| "a list"),
        (Some("object"), v @ Value::Object(_)) => conform(schema, v).map_err(|_| "an object"),
        (Some("object"), _) => Err("an object"),
        // No type declared, or one this doesn't know: taken as it is.
        (_, v) => Ok(v),
    }
}

/// Something that offers tools, typically one per integration.
pub trait ToolSource: Send + Sync {
    /// The tools usable right now, e.g. none while the integration is disconnected.
    fn tools<'a>(&'a self, state: &'a AppState) -> BoxFuture<'a, Vec<Arc<dyn Tool>>>;
}

/// Every registered tool source. Integrations add theirs once, at startup.
#[derive(Default)]
pub struct ToolSources(RwLock<Vec<Arc<dyn ToolSource>>>);

impl ToolSources {
    pub fn add(&self, source: Arc<dyn ToolSource>) {
        self.0
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(source);
    }

    /// The tools available for one reply.
    pub async fn registry(&self, state: &AppState) -> ToolRegistry {
        let sources = self.0.read().unwrap_or_else(|e| e.into_inner()).clone();
        let mut registry = ToolRegistry::default();
        for source in sources {
            for tool in source.tools(state).await {
                registry.register(tool);
            }
        }
        registry
    }
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Adds a tool. A later tool with the same name replaces the earlier one.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.retain(|t| t.name() != tool.name());
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .iter()
            .map(|t| ToolSpec {
                kind: "function",
                function: FunctionSpec {
                    name: t.name().to_owned(),
                    description: t.description().to_owned(),
                    parameters: t.parameters(),
                },
            })
            .collect()
    }
}

/// The user's answer to an approval card.
#[derive(Debug)]
pub enum Decision {
    /// Go ahead, optionally with arguments the user edited.
    Approve(Option<Value>),
    Reject,
}

/// Approval cards waiting for an answer, by action id.
#[derive(Default)]
pub struct Approvals {
    waiting: Mutex<HashMap<Uuid, oneshot::Sender<Decision>>>,
    /// The exceptions a card's "Don't ask again for …" would add, decided by the daemon
    /// when the card was shown (never taken from the request that approves it).
    offers: Mutex<HashMap<Uuid, AlwaysOffer>>,
}

/// What choosing a card's "Don't ask again for …" adds to Settings › Permissions.
#[derive(Debug, Clone)]
pub struct AlwaysOffer {
    pub kind: &'static str,
    pub targets: Vec<mimi_protocol::PermissionTarget>,
}

impl Approvals {
    pub fn wait(&self, action_id: Uuid) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        lock(&self.waiting).insert(action_id, tx);
        rx
    }

    /// Remembers what the card's second choice would allow from now on.
    pub fn offer(&self, action_id: Uuid, offer: AlwaysOffer) {
        lock(&self.offers).insert(action_id, offer);
    }

    /// The card's offer, if it had one.
    pub fn offer_of(&self, action_id: Uuid) -> Option<AlwaysOffer> {
        lock(&self.offers).get(&action_id).cloned()
    }

    /// Delivers the user's decision. Returns false if nothing is waiting for it.
    pub fn decide(&self, action_id: Uuid, decision: Decision) -> bool {
        lock(&self.offers).remove(&action_id);
        match lock(&self.waiting).remove(&action_id) {
            Some(tx) => tx.send(decision).is_ok(),
            None => false,
        }
    }

    /// Whether a card is still waiting for an answer.
    pub fn is_waiting(&self, action_id: Uuid) -> bool {
        lock(&self.waiting).contains_key(&action_id)
    }

    pub fn forget(&self, action_id: Uuid) {
        lock(&self.waiting).remove(&action_id);
        lock(&self.offers).remove(&action_id);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::conform;

    fn schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "to": { "type": "array", "items": { "type": "string" } },
                "subject": { "type": "string" },
                "thread_id": { "type": "integer" },
                "urgent": { "type": "boolean" }
            }
        })
    }

    #[test]
    fn plain_mistakes_are_put_in_the_declared_shape() {
        let args = json!({"to": "a@example.com", "subject": 42, "thread_id": "7",
                          "urgent": "true", "cc": null, "extra": "kept"});
        assert_eq!(
            conform(&schema(), args).unwrap(),
            json!({"to": ["a@example.com"], "subject": "42", "thread_id": 7,
                   "urgent": true, "extra": "kept"})
        );
    }

    #[test]
    fn values_that_dont_fit_are_refused() {
        for args in [
            json!({"to": {"hidden": "x@evil.example"}}),
            json!({"to": [["x@evil.example"]]}),
            json!({"subject": ["a", "b"]}),
            json!({"thread_id": "seven"}),
            json!({"urgent": "yes"}),
            json!(["not", "an", "object"]),
        ] {
            assert!(conform(&schema(), args.clone()).is_err(), "{args}");
        }
    }
}
