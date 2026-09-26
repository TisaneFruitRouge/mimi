//! Tools the assistant can use while replying: reading a calendar, sending a message…
//!
//! Integrations contribute tools through a [`ToolSource`] registered on [`ToolSources`]
//! at startup. Each reply builds a fresh [`ToolRegistry`] from the sources, so a tool
//! only appears while its integration is connected.
//!
//! The approval rule: anything that sends, changes or deletes something on the user's
//! behalf returns `true` from [`Tool::needs_approval`] and waits for the user; reads
//! don't.

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
    /// One plain-language line for the approval card, e.g.
    /// "Create “Dentist” on Friday 10:00–10:45 in Personal".
    fn summary(&self, args: &Value) -> String;
    /// Short past-tense line once it ran, e.g. "read calendar". Defaults to the summary.
    fn result_label(&self, args: &Value, _output: &Value) -> String {
        self.summary(args)
    }
    fn run<'a>(&'a self, ctx: &'a ToolContext, args: Value)
    -> BoxFuture<'a, Result<Value, String>>;
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
pub struct Approvals(Mutex<HashMap<Uuid, oneshot::Sender<Decision>>>);

impl Approvals {
    pub fn wait(&self, action_id: Uuid) -> oneshot::Receiver<Decision> {
        let (tx, rx) = oneshot::channel();
        self.lock().insert(action_id, tx);
        rx
    }

    /// Delivers the user's decision. Returns false if nothing is waiting for it.
    pub fn decide(&self, action_id: Uuid, decision: Decision) -> bool {
        match self.lock().remove(&action_id) {
            Some(tx) => tx.send(decision).is_ok(),
            None => false,
        }
    }

    pub fn forget(&self, action_id: Uuid) {
        self.lock().remove(&action_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, oneshot::Sender<Decision>>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}
