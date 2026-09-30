//! The rooms a Matrix assistant keeps besides its owner's chat, and who it may write to
//! on its own (Settings › Permissions › Send messages).
//!
//! Once paired, the assistant stays in its owner's chat, in groups the owner invited it
//! into, in public groups it joined to post there with the user's OK, and in chats it
//! opened to message someone. Everything else is left. Who it has messaged, and which
//! groups the owner brought it into, are remembered as *known* (migration 0025).

use std::collections::HashSet;

use matrix_sdk::ruma::{RoomId, UserId};
use mimi_protocol::Channel;
use uuid::Uuid;

use super::MatrixConfig;
use crate::AppState;
use crate::db::DbError;
use crate::people::normalize::match_key;

/// Why the assistant stays in a room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The owner invited it in.
    Group,
    /// A public group it joined to post there.
    Joined,
    /// A chat it opened with one person, to message them.
    Direct,
}

impl Why {
    fn as_str(self) -> &'static str {
        match self {
            Why::Group => "group",
            Why::Joined => "joined",
            Why::Direct => "direct",
        }
    }

    fn parse(s: &str) -> Option<Why> {
        match s {
            "group" => Some(Why::Group),
            "joined" => Some(Why::Joined),
            "direct" => Some(Why::Direct),
            _ => None,
        }
    }
}

/// A room the assistant keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub room_id: String,
    pub why: Why,
    /// For a direct chat: who it's with.
    pub user_id: Option<String>,
    /// Its name when it was recorded, for when the running client can't say.
    pub name: Option<String>,
}

/// Records a room the assistant keeps. A room keeps the reason it was first recorded
/// with; a new name replaces the old one.
pub async fn keep(
    state: &AppState,
    connection: Uuid,
    room_id: &str,
    why: Why,
    user_id: Option<&str>,
    name: Option<&str>,
) -> Result<(), DbError> {
    let (room_id, user_id, name) = (
        room_id.to_owned(),
        user_id.map(str::to_owned),
        name.map(str::to_owned),
    );
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO matrix_rooms (connection_id, room_id, why, user_id, name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (connection_id, room_id)
                 DO UPDATE SET name = COALESCE(excluded.name, name)",
                (
                    connection.to_string(),
                    room_id,
                    why.as_str(),
                    user_id,
                    name,
                    crate::now_ms(),
                ),
            )?;
            Ok(())
        })
        .await
}

/// Forgets a room the assistant no longer keeps (it left, or was removed).
pub async fn forget(state: &AppState, connection: Uuid, room_id: &str) -> Result<(), DbError> {
    let room_id = room_id.to_owned();
    state
        .db
        .call(move |c| {
            c.execute(
                "DELETE FROM matrix_rooms WHERE connection_id = ?1 AND room_id = ?2",
                (connection.to_string(), room_id),
            )?;
            Ok(())
        })
        .await
}

/// Every room a connection keeps besides its owner's chat.
pub async fn kept(state: &AppState, connection: Uuid) -> Vec<Kept> {
    state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT room_id, why, user_id, name FROM matrix_rooms
                 WHERE connection_id = ?1 ORDER BY created_at DESC, rowid DESC",
            )?;
            let rows = stmt.query_map([connection.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (room_id, why, user_id, name) = row?;
                if let Some(why) = Why::parse(&why) {
                    out.push(Kept {
                        room_id,
                        why,
                        user_id,
                        name,
                    });
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default()
}

/// The room a connection keeps with this id, if it does.
pub async fn kept_room(state: &AppState, connection: Uuid, room_id: &str) -> Option<Kept> {
    kept(state, connection)
        .await
        .into_iter()
        .find(|k| k.room_id == room_id)
}

/// Why someone or a group is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// A group the owner invited the assistant into.
    Invited,
    /// Someone or a group the assistant messaged for the user.
    Messaged,
}

/// Remembers a user id or room id as known.
pub async fn remember(
    state: &AppState,
    connection: Uuid,
    target: &str,
    why: Known,
) -> Result<(), DbError> {
    let target = target.to_owned();
    let why = match why {
        Known::Invited => "invited",
        Known::Messaged => "messaged",
    };
    state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO matrix_known (connection_id, target, why, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (connection_id, target) DO NOTHING",
                (connection.to_string(), target, why, crate::now_ms()),
            )?;
            Ok(())
        })
        .await
}

async fn remembered(state: &AppState, connection: Uuid) -> HashSet<String> {
    state
        .db
        .call(move |c| {
            let mut stmt = c.prepare("SELECT target FROM matrix_known WHERE connection_id = ?1")?;
            stmt.query_map([connection.to_string()], |r| r.get::<_, String>(0))?
                .collect()
        })
        .await
        .unwrap_or_default()
}

/// The server part of a user id (`@sam:example.org` → `example.org`), lowercased.
pub fn server_of_user(user: &str) -> Option<String> {
    UserId::parse(user)
        .ok()
        .map(|u| u.server_name().as_str().to_lowercase())
}

/// The server part of a room id, when it has one (room versions from 12 on don't).
pub fn server_of_room(room: &str) -> Option<String> {
    RoomId::parse(room)
        .ok()?
        .server_name()
        .map(|s| s.as_str().to_lowercase())
}

/// Something a message reaches, as `Tool::call_targets` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    User(String),
    Room(String),
}

/// Whether every one of `targets`, written to from the assistant account `from`, is
/// someone the user knows, so a message may go out without asking:
///
/// - a person: the owner; someone whose Matrix address is in People from an address
///   book or added by hand; someone the assistant has messaged for the user before; or,
///   with `everyone_on_server`, anyone on the assistant's own server;
/// - a group: the owner's chat; a group the owner invited the assistant into; a group it
///   has posted in for the user before; or, with `everyone_on_server`, a group it's in
///   whose members are all on its server.
///
/// Nobody, an account that isn't a paired connection, or anything else isn't.
pub async fn all_known(
    state: &AppState,
    from: &str,
    targets: &[Target],
    everyone_on_server: bool,
) -> bool {
    if targets.is_empty() {
        return false;
    }
    let Some((connection, config)) = super::paired_config(state, from).await else {
        return false;
    };
    let Some(server) = server_of_user(&config.user_id) else {
        return false;
    };
    let remembered = remembered(state, connection).await;
    let mail_sources: Vec<String> = crate::mail::accounts(state)
        .await
        .into_iter()
        .map(|a| a.id.to_string())
        .collect();
    for target in targets {
        let known = match target {
            Target::User(user) => {
                config.owner.as_deref() == Some(user.as_str())
                    || remembered.contains(user)
                    || in_contacts(state, user, &mail_sources).await
                    || (everyone_on_server && server_of_user(user).as_deref() == Some(&server))
            }
            Target::Room(room) => {
                config.room_id.as_deref() == Some(room.as_str())
                    || remembered.contains(room)
                    || (everyone_on_server
                        && all_members_on(state, connection, room, &server).await)
            }
        };
        if !known {
            return false;
        }
    }
    true
}

/// Whether this Matrix address is in People from an address book or added by hand:
/// never a card Mimi made itself from mail.
async fn in_contacts(state: &AppState, user: &str, mail_sources: &[String]) -> bool {
    let Some(key) = match_key(Channel::Matrix, user) else {
        return false;
    };
    let mail_sources = mail_sources.to_vec();
    state
        .db
        .call(move |c| crate::mail::known::in_contacts(c, "matrix", &key, &mail_sources))
        .await
        .unwrap_or(false)
}

/// Whether everyone in a room the assistant is in is on `server`.
async fn all_members_on(state: &AppState, connection: Uuid, room: &str, server: &str) -> bool {
    let Some(messenger) = state.connections.matrix.messenger(connection) else {
        return false;
    };
    match messenger.members(room).await {
        Some(members) if !members.is_empty() => members
            .iter()
            .all(|m| server_of_user(m).as_deref() == Some(server)),
        _ => false,
    }
}

/// The names of the groups the assistant keeps, by room id, for exceptions in Settings ›
/// Permissions: as the running client has them, else as recorded.
pub async fn group_labels(state: &AppState) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for paired in super::paired(state).await {
        for group in super::send::groups_of(state, &paired).await {
            if !out.iter().any(|(id, _)| id == &group.id) {
                out.push((group.id.clone(), super::send::group_name(&group)));
            }
        }
        for kept in kept(state, paired.id).await {
            if kept.why != Why::Direct && !out.iter().any(|(id, _)| id == &kept.room_id) {
                out.push((
                    kept.room_id.clone(),
                    kept.name.clone().unwrap_or_else(|| "A group".to_owned()),
                ));
            }
        }
    }
    out
}

/// Whether the config says this is the owner's own chat.
pub fn is_owner_room(config: &MatrixConfig, room: &str) -> bool {
    config.room_id.as_deref() == Some(room)
}
