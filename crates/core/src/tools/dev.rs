//! Development-only tools for exercising the tool engine and approval UI end to end.
//! Compiled into debug builds only, and registered only when `HEARTH_DEV_TOOLS=1`.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::{Value, json};

use super::{Tool, ToolContext, ToolSource};
use crate::AppState;

pub const ENV: &str = "HEARTH_DEV_TOOLS";

pub struct DevTools;

impl ToolSource for DevTools {
    fn tools<'a>(&'a self, _state: &'a AppState) -> BoxFuture<'a, Vec<Arc<dyn Tool>>> {
        Box::pin(async { vec![Arc::new(Lookup) as Arc<dyn Tool>, Arc::new(Note)] })
    }
}

struct Lookup;

impl Tool for Lookup {
    fn name(&self) -> &str {
        "dev_lookup"
    }
    fn description(&self) -> &str {
        "Look up a fact by keyword (development tool; returns canned data)."
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]})
    }
    fn needs_approval(&self, _: &Value) -> bool {
        false
    }
    fn summary(&self, args: &Value) -> String {
        format!("Look up “{}”", args["query"].as_str().unwrap_or(""))
    }
    fn result_label(&self, args: &Value, _: &Value) -> String {
        format!(
            "looked up {}",
            args["query"].as_str().unwrap_or("something")
        )
    }
    fn run<'a>(&'a self, _: &'a ToolContext, args: Value) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async move {
            Ok(json!({"query": args["query"], "answer": "Sam's birthday is on 12 October."}))
        })
    }
}

struct Note;

impl Tool for Note {
    fn name(&self) -> &str {
        "dev_send_note"
    }
    fn description(&self) -> &str {
        "Send a short note to someone (development tool; sends nothing)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"to": {"type": "string"}, "text": {"type": "string"}},
            "required": ["to", "text"]
        })
    }
    fn needs_approval(&self, _: &Value) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "Send a note to {}",
            args["to"].as_str().unwrap_or("someone")
        )
    }
    fn result_label(&self, args: &Value, _: &Value) -> String {
        format!(
            "sent a note to {}",
            args["to"].as_str().unwrap_or("someone")
        )
    }
    fn run<'a>(&'a self, _: &'a ToolContext, _: Value) -> BoxFuture<'a, Result<Value, String>> {
        Box::pin(async { Ok(json!({"sent": true})) })
    }
}
