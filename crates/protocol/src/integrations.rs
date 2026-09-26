use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IntegrationCategory {
    Calendar,
    Contacts,
    Messaging,
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IntegrationStatus {
    Connected,
    Available,
    /// Listed so people know it's planned; can't be connected yet.
    ComingSoon,
}

/// Something Hearth can connect to on the user's behalf.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub category: IntegrationCategory,
    pub description: String,
    /// What connecting it lets Hearth do, in plain language.
    pub abilities: Vec<String>,
    pub status: IntegrationStatus,
}
