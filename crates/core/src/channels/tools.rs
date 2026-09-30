//! Letting the assistant message the user in their messaging apps ("send me that on
//! Signal"). It only ever reaches the user themselves, on the private line of a paired
//! app, like a reminder does, so it needs no approval; the chat still shows it.

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde_json::{Value, json};

use super::{Outgoing, owners};
use crate::AppState;
use crate::tools::{Tool, ToolContext, ToolSource};

/// Offers `message_me` while at least one messaging app is paired.
pub struct MessagingTools;

impl ToolSource for MessagingTools {
    fn tools<'a>(&'a self, state: &'a AppState) -> BoxFuture<'a, Vec<Arc<dyn Tool>>> {
        async move {
            let mut apps: Vec<&'static str> =
                owners(state).await.iter().map(|c| c.kind()).collect();
            apps.dedup();
            if apps.is_empty() {
                return Vec::new();
            }
            vec![Arc::new(MessageMe { apps }) as Arc<dyn Tool>]
        }
        .boxed()
    }
}

/// The name people know an app by.
pub fn app_name(kind: &str) -> &str {
    match kind {
        "telegram" => "Telegram",
        "signal" => "Signal",
        "matrix" => "Matrix",
        other => other,
    }
}

struct MessageMe {
    /// The paired apps, by integration id.
    apps: Vec<&'static str>,
}

impl MessageMe {
    fn names(&self, args: &Value) -> String {
        let chosen: Vec<&str> = match args["app"].as_str() {
            Some(app) => vec![app_name(app)],
            None => self.apps.iter().map(|a| app_name(a)).collect(),
        };
        match chosen.as_slice() {
            [one] => (*one).to_owned(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
            [] => "your messaging apps".to_owned(),
        }
    }
}

impl Tool for MessageMe {
    fn name(&self) -> &str {
        "message_me"
    }

    fn description(&self) -> &str {
        "Send the user a message in a messaging app they've connected, where they chat with \
         you (their own private chat: it reaches nobody else). Use it when they ask you to send \
         them something there, e.g. \"send me that list on my phone\". Write the whole message \
         in Markdown. Leave out `app` to use every connected app."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The message, in Markdown" },
                "app": { "type": "string", "enum": self.apps, "description": "Which app; omit for all of them" }
            },
            "required": ["text"]
        })
    }

    fn needs_approval(&self, _args: &Value) -> bool {
        false
    }

    fn summary(&self, args: &Value) -> String {
        format!("send you a message on {}", self.names(args))
    }

    fn result_label(&self, args: &Value, _output: &Value) -> String {
        format!("sent you a message on {}", self.names(args))
    }

    fn run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        async move {
            let text = args["text"].as_str().unwrap_or_default().trim();
            if text.is_empty() {
                return Err("The message is empty.".to_owned());
            }
            let app = args["app"].as_str();
            if let Some(app) = app
                && !self.apps.contains(&app)
            {
                return Err(format!("{} isn't connected.", app_name(app)));
            }
            let message = Outgoing::text(text);
            let mut sent = Vec::new();
            let mut failed = Vec::new();
            for channel in owners(&ctx.state).await {
                if app.is_some_and(|a| a != channel.kind()) {
                    continue;
                }
                match channel.send(&message).await {
                    Ok(()) => sent.push(app_name(channel.kind())),
                    Err(e) => {
                        tracing::warn!("sending a message to {} failed: {e}", channel.kind());
                        failed.push(app_name(channel.kind()));
                    }
                }
            }
            if sent.is_empty() {
                return Err(match failed.as_slice() {
                    [] => "No messaging app is connected any more.".to_owned(),
                    _ => format!("Couldn't reach {} right now.", failed.join(" or ")),
                });
            }
            Ok(json!({ "sent_to": sent, "failed": failed }))
        }
        .boxed()
    }
}
