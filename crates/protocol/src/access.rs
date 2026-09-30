//! People the user trusts to ask their assistant things from their own messaging app
//! (Matrix today), set on the person's card in People.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::Action;

/// Who approves what a trusted person asks for, when it needs approval.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Approver {
    /// They do, in their own chat with the assistant.
    #[default]
    Guest,
    /// The user does: only the card itself (what will run) reaches them, never the
    /// person's conversation.
    Owner,
}

/// What someone in People may do with the assistant (`GET /people/{id}/access`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PersonAccess {
    pub person_id: Uuid,
    /// They may ask the assistant things. Off for everyone until the user turns it on.
    pub enabled: bool,
    /// The calendars shared with them (`GET /calendars` ids): the only ones the
    /// assistant reads or writes for them.
    pub calendars: Vec<String>,
    pub approver: Approver,
    /// The addresses they can write from: their Matrix addresses from an address book
    /// or added by hand (never one Mimi only saw in mail).
    pub addresses: Vec<String>,
    /// The assistant's own Matrix accounts they can write to.
    pub assistant_addresses: Vec<String>,
}

/// A change to someone's access (`PUT /people/{id}/access`); what's left out stays.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct PersonAccessUpdate {
    #[ts(optional)]
    pub enabled: Option<bool>,
    #[ts(optional)]
    pub calendars: Option<Vec<String>>,
    #[ts(optional)]
    pub approver: Option<Approver>,
}

/// Something a trusted person asked for that waits for the user's OK, because their
/// card says the user approves. Only the action: never their conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GuestApproval {
    pub person_id: Uuid,
    /// Their name in People.
    pub name: String,
    pub action: Action,
}
