//! A stand-in Matrix account for tests: a tiny server of its own, with accounts, groups,
//! a public directory, and a record of everything sent.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use uuid::Uuid;

use super::messenger::{Lookup, Messenger, RoomInfo};
use super::{MATRIX, MatrixConfig};
use crate::AppState;
use crate::connections::store::{self, ConnectionRow};

#[derive(Default)]
pub struct World {
    /// Accounts on the server, with their display names.
    pub users: HashMap<String, Option<String>>,
    /// Group addresses → room ids.
    pub aliases: HashMap<String, String>,
    /// Rooms the assistant is in.
    pub joined: Vec<RoomInfo>,
    /// Everyone in each room, joined or invited.
    pub members: HashMap<String, Vec<String>>,
    /// The server's public directory.
    pub directory: Vec<RoomInfo>,
    /// (room, Markdown) for every message sent.
    pub sent: Vec<(String, String)>,
    /// (room, user) for every direct chat opened.
    pub opened: Vec<(String, String)>,
    /// Rooms that are end-to-end encrypted.
    pub encrypted: Vec<String>,
    next: u32,
}

pub struct FakeMessenger {
    pub me: String,
    pub world: Mutex<World>,
}

impl FakeMessenger {
    pub fn new(me: &str) -> Arc<Self> {
        Arc::new(Self {
            me: me.to_owned(),
            world: Mutex::new(World::default()),
        })
    }

    pub fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().unwrap()
    }

    pub fn add_user(&self, id: &str, name: Option<&str>) {
        self.world()
            .users
            .insert(id.to_owned(), name.map(str::to_owned));
    }

    /// A group the assistant is in, with its members (the assistant included).
    pub fn add_group(&self, id: &str, name: &str, alias: Option<&str>, members: &[&str]) {
        let mut w = self.world();
        let mut everyone: Vec<String> = members.iter().map(|m| (*m).to_owned()).collect();
        everyone.push(self.me.clone());
        w.members.insert(id.to_owned(), everyone);
        if let Some(a) = alias {
            w.aliases.insert(a.to_owned(), id.to_owned());
        }
        w.joined.push(RoomInfo {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            alias: alias.map(str::to_owned),
            members: members.len() as u64 + 1,
            ..Default::default()
        });
    }

    /// A public group listed in the directory that the assistant isn't in.
    pub fn add_public(&self, id: &str, name: &str, alias: Option<&str>) {
        let mut w = self.world();
        if let Some(a) = alias {
            w.aliases.insert(a.to_owned(), id.to_owned());
        }
        w.directory.push(RoomInfo {
            id: id.to_owned(),
            name: Some(name.to_owned()),
            alias: alias.map(str::to_owned),
            members: 3,
            public: true,
            ..Default::default()
        });
    }

    pub fn sent(&self) -> Vec<(String, String)> {
        self.world().sent.clone()
    }

    /// What was sent to one room, in order.
    pub fn sent_to(&self, room: &str) -> Vec<String> {
        self.world()
            .sent
            .iter()
            .filter(|(r, _)| r == room)
            .map(|(_, text)| text.clone())
            .collect()
    }

    /// A direct chat someone opened with the assistant (they invited it, it joined).
    pub fn add_direct(&self, room: &str, user: &str, encrypted: bool) {
        let mut w = self.world();
        w.members
            .insert(room.to_owned(), vec![self.me.clone(), user.to_owned()]);
        w.joined.push(RoomInfo {
            id: room.to_owned(),
            members: 2,
            direct: true,
            creator: Some(user.to_owned()),
            ..Default::default()
        });
        if encrypted {
            w.encrypted.push(room.to_owned());
        }
    }

    fn server(&self) -> String {
        self.me
            .split_once(':')
            .map(|(_, s)| s)
            .unwrap_or("")
            .to_owned()
    }
}

#[async_trait]
impl Messenger for FakeMessenger {
    fn user_id(&self) -> String {
        self.me.clone()
    }

    async fn profile(&self, user: &str) -> Result<Option<String>, Lookup> {
        self.world().users.get(user).cloned().ok_or(Lookup::Missing)
    }

    async fn resolve_alias(&self, alias: &str) -> Result<Option<String>, String> {
        Ok(self.world().aliases.get(alias).cloned())
    }

    async fn joined(&self) -> Vec<RoomInfo> {
        self.world().joined.clone()
    }

    async fn directory(&self) -> Result<Vec<RoomInfo>, String> {
        Ok(self.world().directory.clone())
    }

    async fn members(&self, room: &str) -> Option<Vec<String>> {
        let w = self.world();
        w.joined
            .iter()
            .any(|r| r.id == room)
            .then(|| w.members.get(room).cloned().unwrap_or_default())
    }

    async fn create_dm(&self, user: &str) -> Result<String, String> {
        let server = self.server();
        let mut w = self.world();
        w.next += 1;
        let id = format!("!dm{}:{server}", w.next);
        w.joined.push(RoomInfo {
            id: id.clone(),
            members: 1,
            direct: true,
            creator: Some(self.me.clone()),
            ..Default::default()
        });
        w.members
            .insert(id.clone(), vec![self.me.clone(), user.to_owned()]);
        w.opened.push((id.clone(), user.to_owned()));
        Ok(id)
    }

    async fn join(&self, room: &str) -> Result<(), String> {
        let mut w = self.world();
        let info = w
            .directory
            .iter()
            .find(|r| r.id == room && r.public)
            .cloned()
            .ok_or("That group can't be joined.")?;
        w.members.insert(room.to_owned(), vec![self.me.clone()]);
        w.joined.push(info);
        Ok(())
    }

    async fn leave(&self, room: &str) {
        self.world().joined.retain(|r| r.id != room);
    }

    async fn send(&self, room: &str, markdown: &str) -> Result<(), String> {
        let mut w = self.world();
        if !w.joined.iter().any(|r| r.id == room) {
            return Err("Your assistant isn't in that chat.".to_owned());
        }
        w.sent.push((room.to_owned(), markdown.to_owned()));
        Ok(())
    }

    async fn send_html(&self, room: &str, body: &str, _html: &str) -> Result<String, String> {
        let mut w = self.world();
        if !w.joined.iter().any(|r| r.id == room) {
            return Err("Your assistant isn't in that chat.".to_owned());
        }
        w.next += 1;
        let id = format!("$event{}", w.next);
        w.sent.push((room.to_owned(), body.to_owned()));
        Ok(id)
    }

    async fn encrypted(&self, room: &str) -> bool {
        self.world().encrypted.iter().any(|r| r == room)
    }

    async fn download(&self, _photo: super::Photo) -> Result<crate::attachments::Upload, String> {
        Err("the fake has no media".to_owned())
    }
}

/// Saves a paired Matrix connection for `me`, owned by `owner` in the chat `owner_room`,
/// that sends through `fake`. Its id.
pub async fn paired_connection(
    state: &AppState,
    fake: Arc<FakeMessenger>,
    owner: &str,
    owner_room: &str,
) -> Uuid {
    let id = Uuid::now_v7();
    let config = MatrixConfig {
        user_id: fake.me.clone(),
        homeserver: "https://matrix.invalid/".into(),
        device_id: "DEVICE".into(),
        access_token: "secret".into(),
        store_passphrase: "pass".into(),
        pairing_code: None,
        owner: Some(owner.to_owned()),
        owner_name: Some("Vincent".into()),
        room_id: Some(owner_room.to_owned()),
        conversation_id: None,
        encrypted: true,
        cross_signed: true,
        signed_out: false,
        since: 0,
    };
    {
        let mut w = fake.world();
        w.members.insert(
            owner_room.to_owned(),
            vec![fake.me.clone(), owner.to_owned()],
        );
        w.joined.push(RoomInfo {
            id: owner_room.to_owned(),
            members: 2,
            direct: true,
            ..Default::default()
        });
        w.encrypted.push(owner_room.to_owned());
    }
    store::upsert(
        &state.db,
        ConnectionRow {
            id,
            integration: MATRIX.to_owned(),
            name: fake.me.clone(),
            config: serde_json::to_value(config).unwrap(),
            created_at: crate::now_ms(),
        },
    )
    .await
    .unwrap();
    state.connections.matrix.fake(id, fake);
    id
}
