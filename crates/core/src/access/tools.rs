//! What a trusted person's turn can use: an explicit allow-list, never the owner's
//! registry minus some. Their shared calendars (read, add, change, remove; each tool
//! checks the calendar again against [`super::Principal`]) and their own reminders and
//! routines. No mail, memory, people, messaging, or anything else of the owner's.

use std::sync::Arc;

use super::Guest;
use crate::AppState;
use crate::tools::{Tool, ToolRegistry};

/// Every tool a guest turn may ever call. `chat` refuses anything else, whatever the
/// registry holds.
pub const ALLOWED: &[&str] = &[
    "calendar_events",
    "calendar_add_event",
    "calendar_change_event",
    "calendar_delete_event",
    "reminder_add",
    "routine_add",
    "schedule_list",
    "schedule_change",
    "schedule_cancel",
];

pub fn allowed(name: &str) -> bool {
    ALLOWED.contains(&name)
}

/// The tools for one of a trusted person's replies.
pub async fn registry(state: &AppState, guest: &Guest) -> ToolRegistry {
    let mut registry = ToolRegistry::default();
    let tools: Vec<Arc<dyn Tool>> =
        crate::connections::calendar::tools::for_guest(state, &guest.calendars)
            .await
            .into_iter()
            .chain(crate::schedule::tools::all())
            .collect();
    for tool in tools {
        if allowed(tool.name()) {
            registry.register(tool);
        }
    }
    registry
}
