//! What sending messages for the user needs from a Matrix account, behind a small trait:
//! the running client ([`super::Live`]) implements it, and tests use a fake instead of a
//! homeserver.

use async_trait::async_trait;
use matrix_sdk::RoomMemberships;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::room::JoinRuleKind;
use matrix_sdk::ruma::{RoomAliasId, RoomId, UserId};
use matrix_sdk::{Room, RoomState};

use super::{Live, format, plain_error};
use crate::channels::Outgoing;

/// A room as the assistant sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomInfo {
    pub id: String,
    /// Its name, as its members set it (or as Matrix works it out from who's in it).
    pub name: Option<String>,
    /// Its address, `#name:server`.
    pub alias: Option<String>,
    pub members: u64,
    /// Marked as a direct chat.
    pub direct: bool,
    /// Anyone may join it.
    pub public: bool,
    /// Who made it.
    pub creator: Option<String>,
}

/// Why looking someone up failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The server has no such account.
    Missing,
    /// The server couldn't be asked; a plain sentence.
    Failed(String),
}

/// A Matrix account the assistant sends from.
#[async_trait]
pub trait Messenger: Send + Sync {
    /// The assistant's own address, `@name:server`.
    fn user_id(&self) -> String;
    /// Someone's display name (`None` when they haven't set one).
    async fn profile(&self, user: &str) -> Result<Option<String>, Lookup>;
    /// The room an address (`#name:server`) points at; `None` when it points nowhere.
    async fn resolve_alias(&self, alias: &str) -> Result<Option<String>, String>;
    /// Every room the assistant is in.
    async fn joined(&self) -> Vec<RoomInfo>;
    /// The public rooms listed in its server's directory.
    async fn directory(&self) -> Result<Vec<RoomInfo>, String>;
    /// Everyone in a room the assistant is in, joined or invited; `None` when it isn't in
    /// that room.
    async fn members(&self, room: &str) -> Option<Vec<String>>;
    /// Opens an end-to-end encrypted direct chat with someone, inviting them. Its id.
    async fn create_dm(&self, user: &str) -> Result<String, String>;
    /// Joins a public room.
    async fn join(&self, room: &str) -> Result<(), String>;
    /// Leaves a room, best effort.
    async fn leave(&self, room: &str);
    /// Sends Markdown to a room the assistant is in, formatted as Matrix HTML.
    async fn send(&self, room: &str, markdown: &str) -> Result<(), String>;
    /// Sends one message as plain text and Matrix HTML to a room the assistant is in.
    /// Its event id, so a reply or reaction to it can be matched.
    async fn send_html(&self, room: &str, body: &str, html: &str) -> Result<String, String>;
    /// Shows, or stops showing, that the assistant is writing in a room.
    async fn typing(&self, _room: &str, _on: bool) {}
    /// Whether a room the assistant is in is end-to-end encrypted.
    async fn encrypted(&self, room: &str) -> bool;
    /// Downloads (and decrypts) a picture someone sent, within the size limit.
    async fn download(&self, photo: super::Photo) -> Result<crate::attachments::Upload, String>;
}

/// A readable room of the running client, by id.
fn joined_room(live: &Live, room: &str) -> Option<Room> {
    let id = RoomId::parse(room).ok()?;
    live.client
        .get_room(&id)
        .filter(|r| r.state() == RoomState::Joined)
}

async fn info_of(room: &Room) -> RoomInfo {
    let name = match room.name().filter(|n| !n.trim().is_empty()) {
        Some(name) => Some(name),
        None => room.display_name().await.ok().map(|n| n.to_string()),
    };
    // The count comes with the room's summary, which the first syncs may not carry yet.
    let members = match room.joined_members_count() {
        0 => room
            .members(RoomMemberships::JOIN)
            .await
            .map(|m| m.len() as u64)
            .unwrap_or(0),
        n => n,
    };
    RoomInfo {
        id: room.room_id().to_string(),
        name,
        alias: room.canonical_alias().map(|a| a.to_string()),
        members,
        direct: room.is_direct().await.unwrap_or(false),
        public: room.is_public().unwrap_or(false),
        creator: room
            .creators()
            .and_then(|c| c.first().map(ToString::to_string)),
    }
}

#[async_trait]
impl Messenger for Live {
    fn user_id(&self) -> String {
        self.client
            .user_id()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    async fn profile(&self, user: &str) -> Result<Option<String>, Lookup> {
        let id = UserId::parse(user).map_err(|_| Lookup::Missing)?;
        match self.client.account().fetch_user_profile_of(&id).await {
            Ok(profile) => Ok(profile
                .get("displayname")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .filter(|n| !n.trim().is_empty())),
            Err(e) if matches!(e.client_api_error_kind(), Some(ErrorKind::NotFound)) => {
                Err(Lookup::Missing)
            }
            Err(e) => Err(Lookup::Failed(plain_error(&e))),
        }
    }

    async fn resolve_alias(&self, alias: &str) -> Result<Option<String>, String> {
        let Ok(alias) = RoomAliasId::parse(alias) else {
            return Ok(None);
        };
        match self.client.resolve_room_alias(&alias).await {
            Ok(r) => Ok(Some(r.room_id.to_string())),
            Err(e) if matches!(e.client_api_error_kind(), Some(ErrorKind::NotFound)) => Ok(None),
            Err(_) => Err("Couldn't reach your assistant's Matrix server.".to_owned()),
        }
    }

    async fn joined(&self) -> Vec<RoomInfo> {
        let mut out = Vec::new();
        for room in self.client.joined_rooms() {
            out.push(info_of(&room).await);
        }
        out
    }

    async fn directory(&self) -> Result<Vec<RoomInfo>, String> {
        let response = self
            .client
            .public_rooms(Some(200), None, None)
            .await
            .map_err(|_| "Couldn't read your assistant's server's list of groups.".to_owned())?;
        Ok(response
            .chunk
            .into_iter()
            .map(|c| RoomInfo {
                id: c.room_id.to_string(),
                name: c.name.filter(|n| !n.trim().is_empty()),
                alias: c.canonical_alias.map(|a| a.to_string()),
                members: c.num_joined_members.into(),
                direct: false,
                public: c.join_rule == JoinRuleKind::Public,
                creator: None,
            })
            .collect())
    }

    async fn members(&self, room: &str) -> Option<Vec<String>> {
        let room = joined_room(self, room)?;
        let members = room.members(RoomMemberships::ACTIVE).await.ok()?;
        Some(members.iter().map(|m| m.user_id().to_string()).collect())
    }

    async fn create_dm(&self, user: &str) -> Result<String, String> {
        let id = UserId::parse(user).map_err(|_| format!("“{user}” isn't a Matrix address."))?;
        let room = self
            .client
            .create_dm(&id)
            .await
            .map_err(|e| plain_error(&e))?;
        Ok(room.room_id().to_string())
    }

    async fn join(&self, room: &str) -> Result<(), String> {
        let id = RoomId::parse(room).map_err(|_| "That isn't a Matrix group.".to_owned())?;
        self.client
            .join_room_by_id(&id)
            .await
            .map(|_| ())
            .map_err(|e| plain_error(&e))
    }

    async fn leave(&self, room: &str) {
        if let Some(room) = joined_room(self, room) {
            let _ = room.leave().await;
        }
    }

    async fn send(&self, room: &str, markdown: &str) -> Result<(), String> {
        let room = joined_room(self, room)
            .ok_or_else(|| "Your assistant isn't in that chat.".to_owned())?;
        for part in format::outgoing(&Outgoing::text(markdown)) {
            room.send(RoomMessageEventContent::text_html(part.body, part.html))
                .await
                .map_err(|e| plain_error(&e))?;
        }
        Ok(())
    }

    async fn send_html(&self, room: &str, body: &str, html: &str) -> Result<String, String> {
        let room = joined_room(self, room)
            .ok_or_else(|| "Your assistant isn't in that chat.".to_owned())?;
        room.send(RoomMessageEventContent::text_html(body, html))
            .await
            .map(|r| r.response.event_id.to_string())
            .map_err(|e| plain_error(&e))
    }

    async fn typing(&self, room: &str, on: bool) {
        if let Some(room) = joined_room(self, room) {
            let _ = room.typing_notice(on).await;
        }
    }

    async fn encrypted(&self, room: &str) -> bool {
        match joined_room(self, room) {
            Some(room) => room
                .latest_encryption_state()
                .await
                .is_ok_and(|s| s.is_encrypted()),
            None => false,
        }
    }

    async fn download(&self, photo: super::Photo) -> Result<crate::attachments::Upload, String> {
        super::download(&self.client, photo).await
    }
}
