//! Signal's keys and sessions, kept in Mimi's encrypted database for presage.
//!
//! The protocol stores (sessions, identities, pre-keys, sender keys) are adapted from
//! presage-store-sqlite (<https://github.com/whisperfish/presage>, AGPL-3.0-only), moved
//! onto Mimi's SQLCipher database and scoped to one connection (migration 0024).
//!
//! Unlike presage's own stores, this one keeps nothing about messages or other people.
//! A linked device receives every chat on the account, so presage offers each message,
//! contact and group to the store; they are dropped here. What is kept is what the
//! protocol needs to keep working: the account's keys, sessions with other devices, the
//! Note to Self disappearing-message timer. Other people are answered with stand-ins
//! that stop presage from fetching their profiles or groups from Signal.

use std::collections::HashMap;
use std::ops::RangeBounds;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use presage::AvatarBytes;
use presage::libsignal_service::Profile;
use presage::libsignal_service::content::ContentBody;
use presage::libsignal_service::libsignal_account_keys::AccountEntropyPool;
use presage::libsignal_service::pre_keys::{KyberPreKeyStoreExt, PreKeysStore};
use presage::libsignal_service::prelude::{Content, MasterKey, ProfileKey, SessionStoreExt, Uuid};
use presage::libsignal_service::proto::sync_message;
use presage::libsignal_service::protocol::{
    CiphertextMessageType, DeviceId, Direction, GenericSignedPreKey, IdentityChange, IdentityKey,
    IdentityKeyPair, IdentityKeyStore, KyberPreKeyId, KyberPreKeyRecord, KyberPreKeyStore,
    PreKeyId, PreKeyRecord, PreKeyStore, ProtocolAddress, ProtocolStore, PublicKey,
    SenderCertificate, SenderKeyRecord, SenderKeyStore, ServiceId, SessionRecord, SessionStore,
    SignalProtocolError, SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore,
};
use presage::libsignal_service::zkgroup::GroupMasterKeyBytes;
use presage::manager::RegistrationData;
use presage::model::contacts::Contact;
use presage::model::groups::Group;
use presage::store::{ContentsStore, StateStore, StickerPack, Store, StoreError, Thread};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError};

#[derive(Debug, thiserror::Error)]
pub enum SignalStoreError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("unreadable Signal data")]
    Unreadable,
    #[error(transparent)]
    Protocol(#[from] SignalProtocolError),
    /// Asked about someone else: Mimi keeps nothing about other people.
    #[error("not kept: Mimi only keeps what Note to Self needs")]
    NotKept,
}

impl From<serde_json::Error> for SignalStoreError {
    fn from(_: serde_json::Error) -> Self {
        Self::Unreadable
    }
}

impl StoreError for SignalStoreError {}

fn protocol_error(e: impl std::fmt::Display) -> SignalProtocolError {
    SignalProtocolError::InvalidState("mimi signal store", e.to_string())
}

/// The account itself, cached once linked.
#[derive(Clone, Copy)]
struct Me {
    aci: Uuid,
    profile_key: ProfileKey,
}

/// The timer of Note to Self's disappearing messages, so Mimi's messages follow it.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
struct NoteToSelf {
    expire_timer: u32,
    expire_timer_version: u32,
}

/// Profile keys seen in other people's messages, kept in memory only and forgotten past
/// this many. They let presage see it already knows a sender's key, so it doesn't fetch
/// their profile.
const SEEN_KEYS: usize = 2_000;

const REGISTRATION: &str = "registration";
const SENDER_CERTIFICATE: &str = "sender_certificate";
const MASTER_KEY: &str = "master_key";
const ACCOUNT_ENTROPY_POOL: &str = "account_entropy_pool";
const NOTE_TO_SELF: &str = "note_to_self";

/// presage's store for one Signal connection.
#[derive(Clone)]
pub struct SignalStore {
    db: Db,
    connection: String,
    me: Arc<Mutex<Option<Me>>>,
    seen: Arc<Mutex<HashMap<Uuid, [u8; 32]>>>,
}

impl SignalStore {
    pub fn new(db: Db, connection: uuid::Uuid) -> Self {
        Self {
            db,
            connection: connection.to_string(),
            me: Default::default(),
            seen: Default::default(),
        }
    }

    async fn kv_get(&self, key: &'static str) -> Result<Option<Vec<u8>>, DbError> {
        let connection = self.connection.clone();
        self.db
            .call(move |c| {
                c.query_row(
                    "SELECT value FROM signal_kv WHERE connection_id = ?1 AND key = ?2",
                    params![connection, key],
                    |r| r.get(0),
                )
                .optional()
            })
            .await
    }

    async fn kv_set(&self, key: &'static str, value: Option<Vec<u8>>) -> Result<(), DbError> {
        let connection = self.connection.clone();
        self.db
            .call(move |c| {
                match value {
                    Some(value) => c.execute(
                        "INSERT INTO signal_kv (connection_id, key, value) VALUES (?1, ?2, ?3)
                         ON CONFLICT (connection_id, key) DO UPDATE SET value = excluded.value",
                        params![connection, key, value],
                    )?,
                    None => c.execute(
                        "DELETE FROM signal_kv WHERE connection_id = ?1 AND key = ?2",
                        params![connection, key],
                    )?,
                };
                Ok(())
            })
            .await
    }

    /// Deletes the keys and sessions (`everything` also the settings kept beside them).
    async fn wipe(&self, everything: bool) -> Result<(), DbError> {
        let connection = self.connection.clone();
        *self.me.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.db
            .call(move |c| {
                let tx = c.transaction()?;
                for table in [
                    "signal_sessions",
                    "signal_identities",
                    "signal_pre_keys",
                    "signal_base_keys_seen",
                    "signal_signed_pre_keys",
                    "signal_kyber_pre_keys",
                    "signal_sender_keys",
                ] {
                    tx.execute(
                        &format!("DELETE FROM {table} WHERE connection_id = ?1"),
                        [&connection],
                    )?;
                }
                if everything {
                    tx.execute(
                        "DELETE FROM signal_kv WHERE connection_id = ?1",
                        [&connection],
                    )?;
                } else {
                    tx.execute(
                        "DELETE FROM signal_kv WHERE connection_id = ?1 AND key IN
                         ('registration', 'identity_keypair_aci', 'identity_keypair_pni',
                          'sender_certificate')",
                        [&connection],
                    )?;
                }
                tx.commit()
            })
            .await
    }

    fn remember_me(&self, data: &RegistrationData) {
        *self.me.lock().unwrap_or_else(|e| e.into_inner()) = Some(Me {
            aci: data.service_ids.aci,
            profile_key: data.profile_key(),
        });
    }

    /// The account, from the cache (filled when the registration is loaded or saved).
    async fn me(&self) -> Option<Me> {
        if let Some(me) = *self.me.lock().unwrap_or_else(|e| e.into_inner()) {
            return Some(me);
        }
        let data = self.load_registration_data().await.ok().flatten()?;
        self.remember_me(&data);
        Some(Me {
            aci: data.service_ids.aci,
            profile_key: data.profile_key(),
        })
    }

    fn is_me(me: Option<Me>, id: &ServiceId) -> bool {
        matches!(id, ServiceId::Aci(_)) && me.is_some_and(|m| m.aci == id.raw_uuid())
    }

    async fn note_to_self(&self) -> Result<NoteToSelf, SignalStoreError> {
        Ok(match self.kv_get(NOTE_TO_SELF).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => NoteToSelf::default(),
        })
    }

    fn protocol(&self, identity: Identity) -> SignalProtocolStore {
        SignalProtocolStore {
            store: self.clone(),
            identity,
        }
    }
}

/// A stand-in for someone Mimi keeps nothing about. Its timer version is the highest
/// there is, so presage never tries to update it.
fn stand_in(uuid: Uuid, timer: NoteToSelf) -> Contact {
    Contact {
        uuid,
        phone_number: None,
        name: String::new(),
        verified: Default::default(),
        profile_key: Vec::new(),
        expire_timer: timer.expire_timer,
        expire_timer_version: timer.expire_timer_version,
        inbox_position: 0,
        avatar: None,
    }
}

impl Store for SignalStore {
    type Error = SignalStoreError;
    type AciStore = SignalProtocolStore;
    type PniStore = SignalProtocolStore;

    async fn clear(&mut self) -> Result<(), SignalStoreError> {
        Ok(self.wipe(true).await?)
    }

    fn aci_protocol_store(&self) -> Self::AciStore {
        self.protocol(Identity::Aci)
    }

    fn pni_protocol_store(&self) -> Self::PniStore {
        self.protocol(Identity::Pni)
    }
}

impl StateStore for SignalStore {
    type StateStoreError = SignalStoreError;

    async fn load_registration_data(&self) -> Result<Option<RegistrationData>, SignalStoreError> {
        let Some(bytes) = self.kv_get(REGISTRATION).await? else {
            return Ok(None);
        };
        let data: RegistrationData = serde_json::from_slice(&bytes)?;
        self.remember_me(&data);
        Ok(Some(data))
    }

    async fn set_aci_identity_key_pair(
        &self,
        key_pair: IdentityKeyPair,
    ) -> Result<(), SignalStoreError> {
        let value = key_pair.serialize().to_vec();
        Ok(self.kv_set(Identity::Aci.key_pair(), Some(value)).await?)
    }

    async fn set_pni_identity_key_pair(
        &self,
        key_pair: IdentityKeyPair,
    ) -> Result<(), SignalStoreError> {
        let value = key_pair.serialize().to_vec();
        Ok(self.kv_set(Identity::Pni.key_pair(), Some(value)).await?)
    }

    async fn save_registration_data(
        &mut self,
        state: &RegistrationData,
    ) -> Result<(), SignalStoreError> {
        let value = serde_json::to_vec(state)?;
        self.kv_set(REGISTRATION, Some(value)).await?;
        self.remember_me(state);
        Ok(())
    }

    async fn sender_certificate(&self) -> Result<Option<SenderCertificate>, SignalStoreError> {
        self.kv_get(SENDER_CERTIFICATE)
            .await?
            .map(|bytes| SenderCertificate::deserialize(&bytes))
            .transpose()
            .map_err(From::from)
    }

    async fn save_sender_certificate(
        &self,
        certificate: &SenderCertificate,
    ) -> Result<(), SignalStoreError> {
        let value = certificate.serialized()?.to_vec();
        Ok(self.kv_set(SENDER_CERTIFICATE, Some(value)).await?)
    }

    async fn is_registered(&self) -> bool {
        self.load_registration_data().await.ok().flatten().is_some()
    }

    async fn clear_registration(&mut self) -> Result<(), SignalStoreError> {
        Ok(self.wipe(false).await?)
    }

    async fn fetch_master_key(&self) -> Result<Option<MasterKey>, SignalStoreError> {
        self.kv_get(MASTER_KEY)
            .await?
            .map(|bytes| MasterKey::from_slice(&bytes).map_err(|_| SignalStoreError::Unreadable))
            .transpose()
    }

    async fn store_master_key(
        &self,
        master_key: Option<&MasterKey>,
    ) -> Result<(), SignalStoreError> {
        let value = master_key.map(|k| k.inner.to_vec());
        Ok(self.kv_set(MASTER_KEY, value).await?)
    }

    async fn fetch_account_entropy_pool(
        &self,
    ) -> Result<Option<AccountEntropyPool>, SignalStoreError> {
        self.kv_get(ACCOUNT_ENTROPY_POOL)
            .await?
            .map(|bytes| {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .ok_or(SignalStoreError::Unreadable)
            })
            .transpose()
    }

    async fn store_account_entropy_pool(
        &self,
        aep: Option<&AccountEntropyPool>,
    ) -> Result<(), SignalStoreError> {
        let value = aep.map(|k| k.to_string().into_bytes());
        Ok(self.kv_set(ACCOUNT_ENTROPY_POOL, value).await?)
    }
}

impl ContentsStore for SignalStore {
    type ContentsStoreError = SignalStoreError;
    type ContactsIter = std::vec::IntoIter<Result<Contact, SignalStoreError>>;
    type GroupsIter = std::iter::Empty<Result<(GroupMasterKeyBytes, Group), SignalStoreError>>;
    type MessagesIter = std::iter::Empty<Result<Content, SignalStoreError>>;
    type StickerPacksIter = std::iter::Empty<Result<StickerPack, SignalStoreError>>;

    async fn clear_profiles(&mut self) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn clear_contents(&mut self) -> Result<(), SignalStoreError> {
        Ok(self.kv_set(NOTE_TO_SELF, None).await?)
    }

    async fn clear_messages(&mut self) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn clear_thread(&mut self, _thread: &Thread) -> Result<(), SignalStoreError> {
        Ok(())
    }

    /// Messages are never stored. Other people's profile keys are noted in memory
    /// (before anything is awaited), so presage sees them as known.
    async fn save_message(
        &self,
        _thread: &Thread,
        message: Content,
    ) -> Result<(), SignalStoreError> {
        let data = match &message.body {
            ContentBody::DataMessage(m) => Some(m),
            ContentBody::SynchronizeMessage(s) => match &s.content {
                Some(sync_message::Content::Sent(sent)) => sent.message.as_ref(),
                _ => None,
            },
            _ => None,
        };
        let key = data
            .and_then(|m| m.profile_key.as_deref())
            .and_then(|k| <[u8; 32]>::try_from(k).ok());
        if let (Some(key), ServiceId::Aci(_)) = (key, message.metadata.sender) {
            let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() >= SEEN_KEYS {
                seen.clear();
            }
            seen.insert(message.metadata.sender.raw_uuid(), key);
        }
        Ok(())
    }

    async fn delete_message(
        &mut self,
        _thread: &Thread,
        _ts: u64,
    ) -> Result<bool, SignalStoreError> {
        Ok(false)
    }

    async fn message(
        &self,
        _thread: &Thread,
        _ts: u64,
    ) -> Result<Option<Content>, SignalStoreError> {
        Ok(None)
    }

    async fn thread_for_sender_and_timestamp(
        &self,
        _sender: &ServiceId,
        _timestamp: u64,
    ) -> Result<Option<Thread>, SignalStoreError> {
        Ok(None)
    }

    async fn messages(
        &self,
        _thread: &Thread,
        _range: impl RangeBounds<u64>,
    ) -> Result<Self::MessagesIter, SignalStoreError> {
        Ok(std::iter::empty())
    }

    async fn clear_contacts(&mut self) -> Result<(), SignalStoreError> {
        Ok(())
    }

    /// Only the account's own entry is kept (for Note to Self's timer).
    async fn save_contact(&mut self, contact: &Contact) -> Result<(), SignalStoreError> {
        let me = self.me().await;
        if me.is_none_or(|m| m.aci != contact.uuid) {
            return Ok(());
        }
        let timer = NoteToSelf {
            expire_timer: contact.expire_timer,
            expire_timer_version: contact.expire_timer_version,
        };
        let value = serde_json::to_vec(&timer)?;
        Ok(self.kv_set(NOTE_TO_SELF, Some(value)).await?)
    }

    async fn contacts(&self) -> Result<Self::ContactsIter, SignalStoreError> {
        let Some(me) = self.me().await else {
            return Ok(Vec::new().into_iter());
        };
        let timer = self.note_to_self().await?;
        Ok(vec![Ok(stand_in(me.aci, timer))].into_iter())
    }

    async fn contact_by_id(&self, id: &ServiceId) -> Result<Option<Contact>, SignalStoreError> {
        let me = self.me().await;
        if Self::is_me(me, id) {
            let timer = self.note_to_self().await?;
            return Ok(Some(stand_in(id.raw_uuid(), timer)));
        }
        let never = NoteToSelf {
            expire_timer: 0,
            expire_timer_version: u32::MAX,
        };
        Ok(Some(stand_in(id.raw_uuid(), never)))
    }

    async fn clear_groups(&mut self) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn save_group(
        &self,
        _master_key: GroupMasterKeyBytes,
        _group: impl Into<Group>,
    ) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn groups(&self) -> Result<Self::GroupsIter, SignalStoreError> {
        Ok(std::iter::empty())
    }

    /// A stand-in at the highest revision, so presage never fetches a group from Signal.
    async fn group(
        &self,
        _master_key: GroupMasterKeyBytes,
    ) -> Result<Option<Group>, SignalStoreError> {
        Ok(Some(Group {
            title: String::new(),
            avatar: String::new(),
            disappearing_messages_timer: None,
            access_control: None,
            revision: u32::MAX,
            members: Vec::new(),
            pending_members: Vec::new(),
            requesting_members: Vec::new(),
            invite_link_password: Vec::new(),
            description: None,
        }))
    }

    async fn save_group_avatar(
        &self,
        _master_key: GroupMasterKeyBytes,
        _avatar: &AvatarBytes,
    ) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn group_avatar(
        &self,
        _master_key: GroupMasterKeyBytes,
    ) -> Result<Option<AvatarBytes>, SignalStoreError> {
        Ok(None)
    }

    async fn upsert_profile_key(
        &mut self,
        uuid: &Uuid,
        key: ProfileKey,
    ) -> Result<bool, SignalStoreError> {
        if self.me().await.is_some_and(|m| m.aci == *uuid) {
            return Ok(false);
        }
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        Ok(seen.insert(*uuid, key.get_bytes()) != Some(key.get_bytes()))
    }

    /// The account's own key; someone else's only if seen in this session. Unknown keys
    /// are an error rather than `None`, which would make presage fetch their profile.
    async fn profile_key(
        &self,
        service_id: &ServiceId,
    ) -> Result<Option<ProfileKey>, SignalStoreError> {
        let me = self.me().await;
        if Self::is_me(me, service_id) {
            return Ok(me.map(|m| m.profile_key));
        }
        let seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        match seen.get(&service_id.raw_uuid()) {
            Some(bytes) => Ok(Some(ProfileKey::create(*bytes))),
            None => Err(SignalStoreError::NotKept),
        }
    }

    async fn save_profile(
        &mut self,
        _uuid: Uuid,
        _key: ProfileKey,
        _profile: Profile,
    ) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn profile(
        &self,
        _uuid: Uuid,
        _key: ProfileKey,
    ) -> Result<Option<Profile>, SignalStoreError> {
        Ok(None)
    }

    async fn save_profile_avatar(
        &mut self,
        _uuid: Uuid,
        _key: ProfileKey,
        _profile: &AvatarBytes,
    ) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn profile_avatar(
        &self,
        _uuid: Uuid,
        _key: ProfileKey,
    ) -> Result<Option<AvatarBytes>, SignalStoreError> {
        Ok(None)
    }

    async fn add_sticker_pack(&mut self, _pack: &StickerPack) -> Result<(), SignalStoreError> {
        Ok(())
    }

    async fn sticker_pack(&self, _id: &[u8]) -> Result<Option<StickerPack>, SignalStoreError> {
        Ok(None)
    }

    async fn remove_sticker_pack(&mut self, _id: &[u8]) -> Result<bool, SignalStoreError> {
        Ok(false)
    }

    async fn sticker_packs(&self) -> Result<Self::StickerPacksIter, SignalStoreError> {
        Ok(std::iter::empty())
    }
}

/// Which of the account's two identities a protocol store is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    Aci,
    Pni,
}

impl Identity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Aci => "aci",
            Self::Pni => "pni",
        }
    }

    fn key_pair(self) -> &'static str {
        match self {
            Self::Aci => "identity_keypair_aci",
            Self::Pni => "identity_keypair_pni",
        }
    }
}

/// The Signal protocol's stores for one identity of the account.
#[derive(Clone)]
pub struct SignalProtocolStore {
    store: SignalStore,
    identity: Identity,
}

fn device_number(address: &ProtocolAddress) -> u32 {
    u32::from(u8::from(address.device_id()))
}

impl SignalProtocolStore {
    /// Runs a query with the connection id and identity as ?1 and ?2.
    async fn call<R, F>(&self, f: F) -> Result<R, SignalProtocolError>
    where
        R: Send + 'static,
        F: FnOnce(&mut rusqlite::Connection, &str, &str) -> rusqlite::Result<R> + Send + 'static,
    {
        let connection = self.store.connection.clone();
        let identity = self.identity.as_str();
        self.store
            .db
            .call(move |c| f(c, &connection, identity))
            .await
            .map_err(protocol_error)
    }

    async fn record(
        &self,
        sql: &'static str,
        id: u32,
    ) -> Result<Option<Vec<u8>>, SignalProtocolError> {
        self.call(move |c, conn, ident| {
            c.query_row(sql, params![conn, ident, id], |r| r.get(0))
                .optional()
        })
        .await
    }

    async fn max_id(&self, table: &'static str) -> Result<Option<u32>, SignalProtocolError> {
        self.call(move |c, conn, ident| {
            c.query_row(
                &format!("SELECT MAX(id) FROM {table} WHERE connection_id = ?1 AND identity = ?2"),
                params![conn, ident],
                |r| r.get(0),
            )
        })
        .await
    }

    async fn count(&self, table: &'static str) -> Result<usize, SignalProtocolError> {
        self.call(move |c, conn, ident| {
            c.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE connection_id = ?1 AND identity = ?2"),
                params![conn, ident],
                |r| r.get::<_, i64>(0),
            )
        })
        .await
        .map(|n| usize::try_from(n).unwrap_or(0))
    }
}

impl ProtocolStore for SignalProtocolStore {}

#[async_trait(?Send)]
impl SessionStore for SignalProtocolStore {
    async fn load_session(
        &self,
        address: &ProtocolAddress,
    ) -> Result<Option<SessionRecord>, SignalProtocolError> {
        let (name, device) = (address.name().to_owned(), device_number(address));
        self.call(move |c, conn, ident| {
            c.query_row(
                "SELECT record FROM signal_sessions
                 WHERE connection_id = ?1 AND identity = ?2 AND address = ?3 AND device_id = ?4",
                params![conn, ident, name, device],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
        })
        .await?
        .map(|bytes| SessionRecord::deserialize(&bytes))
        .transpose()
    }

    async fn store_session(
        &mut self,
        address: &ProtocolAddress,
        record: &SessionRecord,
    ) -> Result<(), SignalProtocolError> {
        let (name, device) = (address.name().to_owned(), device_number(address));
        let record = record.serialize()?;
        self.call(move |c, conn, ident| {
            c.execute(
                "INSERT INTO signal_sessions (connection_id, identity, address, device_id, record)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (connection_id, identity, address, device_id)
                 DO UPDATE SET record = excluded.record",
                params![conn, ident, name, device, record],
            )
            .map(drop)
        })
        .await
    }
}

#[async_trait(?Send)]
impl SessionStoreExt for SignalProtocolStore {
    async fn get_sub_device_sessions(
        &self,
        name: &ServiceId,
    ) -> Result<Vec<DeviceId>, SignalProtocolError> {
        let name = name.service_id_string();
        let ids: Vec<u32> = self
            .call(move |c, conn, ident| {
                let mut stmt = c.prepare(
                    "SELECT device_id FROM signal_sessions
                     WHERE connection_id = ?1 AND identity = ?2 AND address = ?3 AND device_id != 1",
                )?;
                stmt.query_map(params![conn, ident, name], |r| r.get(0))?
                    .collect()
            })
            .await?;
        Ok(ids
            .into_iter()
            .filter_map(|id| id.try_into().ok())
            .collect())
    }

    async fn delete_session(&self, address: &ProtocolAddress) -> Result<(), SignalProtocolError> {
        let (name, device) = (address.name().to_owned(), device_number(address));
        self.call(move |c, conn, ident| {
            c.execute(
                "DELETE FROM signal_sessions
                 WHERE connection_id = ?1 AND identity = ?2 AND address = ?3 AND device_id = ?4",
                params![conn, ident, name, device],
            )
            .map(drop)
        })
        .await
    }

    async fn delete_all_sessions(&self, name: &ServiceId) -> Result<usize, SignalProtocolError> {
        let name = name.service_id_string();
        self.call(move |c, conn, ident| {
            c.execute(
                "DELETE FROM signal_sessions WHERE connection_id = ?1 AND identity = ?2 AND address = ?3",
                params![conn, ident, name],
            )
        })
        .await
    }
}

#[async_trait(?Send)]
impl PreKeyStore for SignalProtocolStore {
    async fn get_pre_key(&self, prekey_id: PreKeyId) -> Result<PreKeyRecord, SignalProtocolError> {
        let bytes = self
            .record(
                "SELECT record FROM signal_pre_keys
                 WHERE connection_id = ?1 AND identity = ?2 AND id = ?3",
                prekey_id.into(),
            )
            .await?
            .ok_or(SignalProtocolError::InvalidPreKeyId)?;
        PreKeyRecord::deserialize(&bytes)
    }

    async fn save_pre_key(
        &mut self,
        prekey_id: PreKeyId,
        record: &PreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let id: u32 = prekey_id.into();
        let record = record.serialize()?;
        self.call(move |c, conn, ident| {
            c.execute(
                "INSERT INTO signal_pre_keys (connection_id, identity, id, record)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (connection_id, identity, id) DO UPDATE SET record = excluded.record",
                params![conn, ident, id, record],
            )
            .map(drop)
        })
        .await
    }

    async fn remove_pre_key(&mut self, prekey_id: PreKeyId) -> Result<(), SignalProtocolError> {
        let id: u32 = prekey_id.into();
        self.call(move |c, conn, ident| {
            c.execute(
                "DELETE FROM signal_pre_keys WHERE connection_id = ?1 AND identity = ?2 AND id = ?3",
                params![conn, ident, id],
            )
            .map(drop)
        })
        .await
    }
}

#[async_trait(?Send)]
impl PreKeysStore for SignalProtocolStore {
    async fn next_pre_key_id(&self) -> Result<u32, SignalProtocolError> {
        Ok(self.max_id("signal_pre_keys").await?.map_or(1, |id| id + 1))
    }

    async fn next_signed_pre_key_id(&self) -> Result<u32, SignalProtocolError> {
        Ok(self
            .max_id("signal_signed_pre_keys")
            .await?
            .map_or(1, |id| id + 1))
    }

    async fn next_pq_pre_key_id(&self) -> Result<u32, SignalProtocolError> {
        Ok(self
            .max_id("signal_kyber_pre_keys")
            .await?
            .map_or(1, |id| id + 1))
    }

    async fn signed_pre_keys_count(&self) -> Result<usize, SignalProtocolError> {
        self.count("signal_signed_pre_keys").await
    }

    async fn kyber_pre_keys_count(&self, _last_resort: bool) -> Result<usize, SignalProtocolError> {
        self.count("signal_kyber_pre_keys").await
    }

    async fn signed_prekey_id(&self) -> Result<Option<SignedPreKeyId>, SignalProtocolError> {
        Ok(self.max_id("signal_signed_pre_keys").await?.map(From::from))
    }

    async fn last_resort_kyber_prekey_id(
        &self,
    ) -> Result<Option<KyberPreKeyId>, SignalProtocolError> {
        self.call(|c, conn, ident| {
            c.query_row(
                "SELECT MAX(id) FROM signal_kyber_pre_keys
                 WHERE connection_id = ?1 AND identity = ?2 AND is_last_resort = 1",
                params![conn, ident],
                |r| r.get::<_, Option<u32>>(0),
            )
        })
        .await
        .map(|id| id.map(From::from))
    }
}

#[async_trait(?Send)]
impl SignedPreKeyStore for SignalProtocolStore {
    async fn get_signed_pre_key(
        &self,
        signed_prekey_id: SignedPreKeyId,
    ) -> Result<SignedPreKeyRecord, SignalProtocolError> {
        let bytes = self
            .record(
                "SELECT record FROM signal_signed_pre_keys
                 WHERE connection_id = ?1 AND identity = ?2 AND id = ?3",
                signed_prekey_id.into(),
            )
            .await?
            .ok_or(SignalProtocolError::InvalidSignedPreKeyId)?;
        SignedPreKeyRecord::deserialize(&bytes)
    }

    async fn save_signed_pre_key(
        &mut self,
        signed_prekey_id: SignedPreKeyId,
        record: &SignedPreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let id: u32 = signed_prekey_id.into();
        let record = record.serialize()?;
        self.call(move |c, conn, ident| {
            c.execute(
                "INSERT INTO signal_signed_pre_keys (connection_id, identity, id, record)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (connection_id, identity, id) DO UPDATE SET record = excluded.record",
                params![conn, ident, id, record],
            )
            .map(drop)
        })
        .await
    }
}

#[async_trait(?Send)]
impl KyberPreKeyStore for SignalProtocolStore {
    async fn get_kyber_pre_key(
        &self,
        kyber_prekey_id: KyberPreKeyId,
    ) -> Result<KyberPreKeyRecord, SignalProtocolError> {
        let bytes = self
            .record(
                "SELECT record FROM signal_kyber_pre_keys
                 WHERE connection_id = ?1 AND identity = ?2 AND id = ?3",
                kyber_prekey_id.into(),
            )
            .await?
            .ok_or(SignalProtocolError::InvalidKyberPreKeyId)?;
        KyberPreKeyRecord::deserialize(&bytes)
    }

    async fn save_kyber_pre_key(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        record: &KyberPreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let id: u32 = kyber_prekey_id.into();
        let record = record.serialize()?;
        self.call(move |c, conn, ident| {
            c.execute(
                "INSERT INTO signal_kyber_pre_keys (connection_id, identity, id, record)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (connection_id, identity, id) DO UPDATE SET record = excluded.record",
                params![conn, ident, id, record],
            )
            .map(drop)
        })
        .await
    }

    /// One-time keys are deleted once used; a last-resort key stays, and remembers the
    /// base key it was used with so the same one is refused next time.
    async fn mark_kyber_pre_key_used(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        ec_prekey_id: SignedPreKeyId,
        base_key: &PublicKey,
    ) -> Result<(), SignalProtocolError> {
        let id: u32 = kyber_prekey_id.into();
        let ec_id: u32 = ec_prekey_id.into();
        let base_key = base_key.serialize().to_vec();
        let fresh = self
            .call(move |c, conn, ident| {
                let tx = c.transaction()?;
                let last_resort: bool = tx
                    .query_row(
                        "SELECT is_last_resort FROM signal_kyber_pre_keys
                         WHERE connection_id = ?1 AND identity = ?2 AND id = ?3",
                        params![conn, ident, id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .unwrap_or(false);
                let fresh = if last_resort {
                    tx.execute(
                        "INSERT OR IGNORE INTO signal_base_keys_seen
                         (connection_id, identity, kyber_pre_key_id, signed_pre_key_id, base_key)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![conn, ident, id, ec_id, base_key],
                    )? == 1
                } else {
                    tx.execute(
                        "DELETE FROM signal_kyber_pre_keys
                         WHERE connection_id = ?1 AND identity = ?2 AND id = ?3 AND is_last_resort = 0",
                        params![conn, ident, id],
                    )?;
                    true
                };
                tx.commit()?;
                Ok(fresh)
            })
            .await?;
        if fresh {
            Ok(())
        } else {
            Err(SignalProtocolError::InvalidMessage(
                CiphertextMessageType::PreKey,
                "reused base key".to_owned(),
            ))
        }
    }
}

#[async_trait(?Send)]
impl KyberPreKeyStoreExt for SignalProtocolStore {
    async fn store_last_resort_kyber_pre_key(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        record: &KyberPreKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let id: u32 = kyber_prekey_id.into();
        let record = record.serialize()?;
        self.call(move |c, conn, ident| {
            c.execute(
                "INSERT INTO signal_kyber_pre_keys (connection_id, identity, id, record, is_last_resort)
                 VALUES (?1, ?2, ?3, ?4, 1)
                 ON CONFLICT (connection_id, identity, id)
                 DO UPDATE SET record = excluded.record, is_last_resort = 1",
                params![conn, ident, id, record],
            )
            .map(drop)
        })
        .await
    }

    async fn load_last_resort_kyber_pre_keys(
        &self,
    ) -> Result<Vec<KyberPreKeyRecord>, SignalProtocolError> {
        let records: Vec<Vec<u8>> = self
            .call(|c, conn, ident| {
                let mut stmt = c.prepare(
                    "SELECT record FROM signal_kyber_pre_keys
                     WHERE connection_id = ?1 AND identity = ?2 AND is_last_resort = 1",
                )?;
                stmt.query_map(params![conn, ident], |r| r.get(0))?
                    .collect()
            })
            .await?;
        records
            .iter()
            .map(|bytes| KyberPreKeyRecord::deserialize(bytes))
            .collect()
    }

    async fn remove_kyber_pre_key(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
    ) -> Result<(), SignalProtocolError> {
        let id: u32 = kyber_prekey_id.into();
        self.call(move |c, conn, ident| {
            c.execute(
                "DELETE FROM signal_kyber_pre_keys WHERE connection_id = ?1 AND identity = ?2 AND id = ?3",
                params![conn, ident, id],
            )
            .map(drop)
        })
        .await
    }

    // libsignal-service doesn't call these two yet; presage's own store panics in them.
    // Keeping every key is the safe answer.
    async fn mark_all_one_time_kyber_pre_keys_stale_if_necessary(
        &mut self,
        _stale_time: DateTime<Utc>,
    ) -> Result<(), SignalProtocolError> {
        Ok(())
    }

    async fn delete_all_stale_one_time_kyber_pre_keys(
        &mut self,
        _threshold: DateTime<Utc>,
        _min_count: usize,
    ) -> Result<(), SignalProtocolError> {
        Ok(())
    }
}

#[async_trait(?Send)]
impl IdentityKeyStore for SignalProtocolStore {
    async fn get_identity_key_pair(&self) -> Result<IdentityKeyPair, SignalProtocolError> {
        let bytes = self
            .store
            .kv_get(self.identity.key_pair())
            .await
            .map_err(protocol_error)?
            .ok_or_else(|| protocol_error("no identity key pair"))?;
        IdentityKeyPair::try_from(bytes.as_slice())
    }

    async fn get_local_registration_id(&self) -> Result<u32, SignalProtocolError> {
        let data = self
            .store
            .load_registration_data()
            .await
            .map_err(protocol_error)?
            .ok_or_else(|| protocol_error("not linked"))?;
        Ok(match self.identity {
            Identity::Aci => data.registration_id,
            Identity::Pni => data.pni_registration_id.unwrap_or(data.registration_id),
        })
    }

    async fn save_identity(
        &mut self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
    ) -> Result<IdentityChange, SignalProtocolError> {
        let name = address.name().to_owned();
        let bytes = identity.serialize().to_vec();
        let replaced = self
            .call(move |c, conn, ident| {
                let tx = c.transaction()?;
                let old: Option<Vec<u8>> = tx
                    .query_row(
                        "SELECT record FROM signal_identities
                         WHERE connection_id = ?1 AND identity = ?2 AND address = ?3",
                        params![conn, ident, name],
                        |r| r.get(0),
                    )
                    .optional()?;
                tx.execute(
                    "INSERT INTO signal_identities (connection_id, identity, address, record)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (connection_id, identity, address) DO UPDATE SET record = excluded.record",
                    params![conn, ident, name, bytes],
                )?;
                tx.commit()?;
                Ok(old.is_some_and(|old| old != bytes))
            })
            .await?;
        Ok(IdentityChange::from_changed(replaced))
    }

    /// Mimi only ever writes to the account's own devices, so it trusts what Signal
    /// says, like presage's stores do by default.
    async fn is_trusted_identity(
        &self,
        _address: &ProtocolAddress,
        _identity: &IdentityKey,
        _direction: Direction,
    ) -> Result<bool, SignalProtocolError> {
        Ok(true)
    }

    async fn get_identity(
        &self,
        address: &ProtocolAddress,
    ) -> Result<Option<IdentityKey>, SignalProtocolError> {
        let name = address.name().to_owned();
        self.call(move |c, conn, ident| {
            c.query_row(
                "SELECT record FROM signal_identities
                 WHERE connection_id = ?1 AND identity = ?2 AND address = ?3",
                params![conn, ident, name],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
        })
        .await?
        .map(|bytes| IdentityKey::decode(&bytes))
        .transpose()
    }
}

#[async_trait(?Send)]
impl SenderKeyStore for SignalProtocolStore {
    async fn store_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: Uuid,
        record: &SenderKeyRecord,
    ) -> Result<(), SignalProtocolError> {
        let (name, device) = (sender.name().to_owned(), device_number(sender));
        let distribution = distribution_id.to_string();
        let record = record.serialize()?;
        self.call(move |c, conn, ident| {
            c.execute(
                "INSERT INTO signal_sender_keys
                 (connection_id, identity, address, device_id, distribution_id, record)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (connection_id, identity, address, device_id, distribution_id)
                 DO UPDATE SET record = excluded.record",
                params![conn, ident, name, device, distribution, record],
            )
            .map(drop)
        })
        .await
    }

    async fn load_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: Uuid,
    ) -> Result<Option<SenderKeyRecord>, SignalProtocolError> {
        let (name, device) = (sender.name().to_owned(), device_number(sender));
        let distribution = distribution_id.to_string();
        self.call(move |c, conn, ident| {
            c.query_row(
                "SELECT record FROM signal_sender_keys
                 WHERE connection_id = ?1 AND identity = ?2 AND address = ?3 AND device_id = ?4
                   AND distribution_id = ?5",
                params![conn, ident, name, device, distribution],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
        })
        .await?
        .map(|bytes| SenderKeyRecord::deserialize(&bytes))
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use presage::libsignal_service::content::Metadata;
    use presage::libsignal_service::prelude::phonenumber;
    use presage::libsignal_service::proto::DataMessage;
    use presage::libsignal_service::protocol::{
        Aci, KeyPair, Timestamp, create_sender_key_distribution_message, kem,
    };

    use super::*;
    use crate::connections::store::{self as rows, ConnectionRow};

    const ME: Uuid = Uuid::from_u128(0xabc);
    const FRIEND: Uuid = Uuid::from_u128(0xdef);

    async fn setup() -> (Db, uuid::Uuid, SignalStore) {
        let db = Db::open_in_memory().unwrap();
        let id = uuid::Uuid::now_v7();
        rows::upsert(
            &db,
            ConnectionRow {
                id,
                integration: "signal".into(),
                name: "Signal".into(),
                config: serde_json::json!({}),
                created_at: 0,
            },
        )
        .await
        .unwrap();
        (db.clone(), id, SignalStore::new(db, id))
    }

    fn registration() -> RegistrationData {
        let phone = phonenumber::parse(None, "+33612345678").unwrap();
        // Parsed from text, as the store does: the profile key is read as a borrowed str.
        let json = serde_json::json!({
            "signal_servers": "Production",
            "device_name": "Mimi",
            "phone_number": phone,
            "uuid": ME,
            "pni": Uuid::from_u128(0x123),
            "password": "pw",
            "device_id": 3,
            "registration_id": 11,
            "pni_registration_id": 12,
            "profile_key": "a2FpanBxeGR2YWlhZWF1bG1zcm96Y2tqa2dicGpvd2M=",
        });
        serde_json::from_str(&json.to_string()).unwrap()
    }

    fn aci(uuid: Uuid) -> ServiceId {
        ServiceId::Aci(Aci::from(uuid))
    }

    fn device(n: u32) -> DeviceId {
        DeviceId::try_from(n).unwrap()
    }

    async fn rows_left(db: &Db, id: uuid::Uuid) -> i64 {
        let id = id.to_string();
        db.call(move |c| {
            let mut total = 0;
            for table in [
                "signal_kv",
                "signal_sessions",
                "signal_identities",
                "signal_pre_keys",
                "signal_signed_pre_keys",
                "signal_kyber_pre_keys",
                "signal_base_keys_seen",
                "signal_sender_keys",
            ] {
                total += c.query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE connection_id = ?1"),
                    [&id],
                    |r| r.get::<_, i64>(0),
                )?;
            }
            Ok(total)
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn protocol_stores_round_trip_per_identity() {
        let (db, id, mut store) = setup().await;
        let mut rng = rand::rng();
        store.save_registration_data(&registration()).await.unwrap();
        assert!(store.is_registered().await);
        let aci_pair = IdentityKeyPair::generate(&mut rng);
        let pni_pair = IdentityKeyPair::generate(&mut rng);
        store.set_aci_identity_key_pair(aci_pair).await.unwrap();
        store.set_pni_identity_key_pair(pni_pair).await.unwrap();
        let mut aci_store = store.aci_protocol_store();
        let mut pni_store = store.pni_protocol_store();
        assert_eq!(
            aci_store.get_identity_key_pair().await.unwrap().serialize(),
            aci_pair.serialize()
        );
        assert_eq!(
            pni_store.get_identity_key_pair().await.unwrap().serialize(),
            pni_pair.serialize()
        );
        assert_eq!(aci_store.get_local_registration_id().await.unwrap(), 11);
        assert_eq!(pni_store.get_local_registration_id().await.unwrap(), 12);

        // Sessions, per identity and device.
        let phone = ProtocolAddress::new(ME.to_string(), device(1));
        let laptop = ProtocolAddress::new(ME.to_string(), device(2));
        let session = SessionRecord::new_fresh();
        aci_store.store_session(&phone, &session).await.unwrap();
        aci_store.store_session(&laptop, &session).await.unwrap();
        aci_store.store_session(&laptop, &session).await.unwrap();
        assert!(aci_store.load_session(&phone).await.unwrap().is_some());
        assert!(pni_store.load_session(&phone).await.unwrap().is_none());
        let others = aci_store.get_sub_device_sessions(&aci(ME)).await.unwrap();
        assert_eq!(others, vec![device(2)]);
        aci_store.delete_session(&laptop).await.unwrap();
        assert!(aci_store.load_session(&laptop).await.unwrap().is_none());
        assert_eq!(aci_store.delete_all_sessions(&aci(ME)).await.unwrap(), 1);

        // Identities: new, unchanged, replaced.
        let first = *IdentityKeyPair::generate(&mut rng).identity_key();
        let second = *IdentityKeyPair::generate(&mut rng).identity_key();
        assert_eq!(
            aci_store.save_identity(&phone, &first).await.unwrap(),
            IdentityChange::NewOrUnchanged
        );
        assert_eq!(
            aci_store.save_identity(&phone, &first).await.unwrap(),
            IdentityChange::NewOrUnchanged
        );
        assert_eq!(
            aci_store.save_identity(&phone, &second).await.unwrap(),
            IdentityChange::ReplacedExisting
        );
        assert_eq!(aci_store.get_identity(&phone).await.unwrap(), Some(second));
        assert_eq!(pni_store.get_identity(&phone).await.unwrap(), None);

        // One-time pre-keys.
        let key = KeyPair::generate(&mut rng);
        aci_store
            .save_pre_key(
                PreKeyId::from(7),
                &PreKeyRecord::new(PreKeyId::from(7), &key),
            )
            .await
            .unwrap();
        assert_eq!(aci_store.next_pre_key_id().await.unwrap(), 8);
        assert_eq!(pni_store.next_pre_key_id().await.unwrap(), 1);
        assert!(aci_store.get_pre_key(PreKeyId::from(7)).await.is_ok());
        aci_store.remove_pre_key(PreKeyId::from(7)).await.unwrap();
        assert!(matches!(
            aci_store.get_pre_key(PreKeyId::from(7)).await,
            Err(SignalProtocolError::InvalidPreKeyId)
        ));

        // Signed pre-keys.
        let signed_id = SignedPreKeyId::from(3);
        let signature = aci_pair
            .private_key()
            .calculate_signature(&key.public_key.serialize(), &mut rng)
            .unwrap();
        let signed = SignedPreKeyRecord::new(
            signed_id,
            Timestamp::from_epoch_millis(1_000),
            &key,
            &signature,
        );
        aci_store
            .save_signed_pre_key(signed_id, &signed)
            .await
            .unwrap();
        assert!(aci_store.get_signed_pre_key(signed_id).await.is_ok());
        assert_eq!(aci_store.signed_pre_keys_count().await.unwrap(), 1);
        assert_eq!(aci_store.signed_prekey_id().await.unwrap(), Some(signed_id));
        assert_eq!(aci_store.next_signed_pre_key_id().await.unwrap(), 4);

        // Kyber pre-keys: a one-time key goes once used; a last-resort key stays but
        // refuses the same base key twice.
        let kyber = |n: u32| {
            KyberPreKeyRecord::generate(
                kem::KeyType::Kyber1024,
                KyberPreKeyId::from(n),
                aci_pair.private_key(),
            )
            .unwrap()
        };
        aci_store
            .save_kyber_pre_key(KyberPreKeyId::from(1), &kyber(1))
            .await
            .unwrap();
        aci_store
            .store_last_resort_kyber_pre_key(KyberPreKeyId::from(2), &kyber(2))
            .await
            .unwrap();
        assert_eq!(aci_store.kyber_pre_keys_count(false).await.unwrap(), 2);
        assert_eq!(
            aci_store.last_resort_kyber_prekey_id().await.unwrap(),
            Some(KyberPreKeyId::from(2))
        );
        assert_eq!(
            aci_store
                .load_last_resort_kyber_pre_keys()
                .await
                .unwrap()
                .len(),
            1
        );
        aci_store
            .mark_kyber_pre_key_used(KyberPreKeyId::from(1), signed_id, &key.public_key)
            .await
            .unwrap();
        assert!(
            aci_store
                .get_kyber_pre_key(KyberPreKeyId::from(1))
                .await
                .is_err()
        );
        aci_store
            .mark_kyber_pre_key_used(KyberPreKeyId::from(2), signed_id, &key.public_key)
            .await
            .unwrap();
        assert!(
            aci_store
                .mark_kyber_pre_key_used(KyberPreKeyId::from(2), signed_id, &key.public_key)
                .await
                .is_err()
        );
        assert!(
            aci_store
                .get_kyber_pre_key(KyberPreKeyId::from(2))
                .await
                .is_ok()
        );

        // Sender keys, through libsignal itself.
        let distribution = Uuid::from_u128(0x55);
        create_sender_key_distribution_message(&phone, distribution, &mut aci_store, &mut rng)
            .await
            .unwrap();
        assert!(
            aci_store
                .load_sender_key(&phone, distribution)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            pni_store
                .load_sender_key(&phone, distribution)
                .await
                .unwrap()
                .is_none()
        );

        // Settings kept beside them.
        let master = MasterKey { inner: [9; 32] };
        store.store_master_key(Some(&master)).await.unwrap();
        assert_eq!(store.fetch_master_key().await.unwrap(), Some(master));
        store.store_master_key(None).await.unwrap();
        assert_eq!(store.fetch_master_key().await.unwrap(), None);

        // Linking again clears the keys; removing the connection removes everything.
        store.clear_registration().await.unwrap();
        assert!(!store.is_registered().await);
        assert!(aci_store.load_session(&phone).await.unwrap().is_none());
        store.set_aci_identity_key_pair(aci_pair).await.unwrap();
        aci_store.store_session(&phone, &session).await.unwrap();
        assert!(rows_left(&db, id).await > 0);
        rows::delete(&db, id).await.unwrap();
        assert_eq!(rows_left(&db, id).await, 0);
        // A worker still running can't write for a removed connection.
        assert!(aci_store.store_session(&phone, &session).await.is_err());
    }

    fn message(sender: Uuid, body: DataMessage) -> Content {
        Content {
            metadata: Metadata {
                sender: aci(sender),
                destination: aci(ME),
                sender_device: device(1),
                pni_verified: None,
                client_timestamp: chrono::Utc::now(),
                server_timestamp: chrono::Utc::now(),
                needs_receipt: false,
                unidentified_sender: false,
                was_plaintext: false,
                server_guid: None,
            },
            body: body.into(),
        }
    }

    fn hex(text: &str) -> String {
        text.bytes().map(|b| format!("{b:02X}")).collect()
    }

    #[tokio::test]
    async fn nothing_about_messages_or_other_people_is_kept() {
        let (db, _, mut store) = setup().await;
        store.save_registration_data(&registration()).await.unwrap();
        let secret = "the-secret-plans-for-saturday";
        let from_friend = message(
            FRIEND,
            DataMessage {
                body: Some(secret.into()),
                profile_key: Some(vec![7; 32]),
                ..Default::default()
            },
        );
        let thread = Thread::Contact(aci(FRIEND));
        store.save_message(&thread, from_friend).await.unwrap();
        let friend = Contact {
            name: "Sam Secret".into(),
            ..stand_in(FRIEND, NoteToSelf::default())
        };
        store.save_contact(&friend).await.unwrap();
        let group = Group {
            title: "Secret club".into(),
            revision: 1,
            ..store.group([1; 32]).await.unwrap().unwrap()
        };
        store.save_group([1; 32], group).await.unwrap();

        // Nothing readable back, and nothing in the database.
        assert!(store.message(&thread, 1).await.unwrap().is_none());
        assert_eq!(store.messages(&thread, ..).await.unwrap().count(), 0);
        let dump: String = db
            .call(|c| {
                let mut stmt = c.prepare("SELECT key || '=' || hex(value) FROM signal_kv")?;
                let rows: Vec<String> = stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<_, _>>()?;
                Ok(rows.join(","))
            })
            .await
            .unwrap();
        assert!(!dump.contains(&hex(secret)), "{dump}");
        assert!(!dump.contains(&hex("Sam Secret")), "{dump}");
        assert!(!dump.contains(&hex("Secret club")), "{dump}");
        assert!(!dump.contains("note_to_self"), "{dump}");

        // Stand-ins stop presage from fetching other people's profiles and groups: the
        // sender's key counts as known (it came with the message), a stranger's is an
        // error rather than "unknown", and every group is already up to date.
        let known = store.profile_key(&aci(FRIEND)).await.unwrap().unwrap();
        assert_eq!(known.get_bytes(), [7; 32]);
        assert!(
            store
                .profile_key(&aci(Uuid::from_u128(0x999)))
                .await
                .is_err()
        );
        let contact = store.contact_by_id(&aci(FRIEND)).await.unwrap().unwrap();
        assert!(contact.name.is_empty());
        assert_eq!(contact.expire_timer_version, u32::MAX);
        assert_eq!(
            store.group([1; 32]).await.unwrap().unwrap().revision,
            u32::MAX
        );
        assert_eq!(store.groups().await.unwrap().count(), 0);

        // Note to Self's timer is kept, so Mimi's messages disappear like the user's.
        let note_to_self = Thread::Contact(aci(ME));
        store
            .update_expire_timer(&note_to_self, 3600, 2)
            .await
            .unwrap();
        let me = store.contact_by_id(&aci(ME)).await.unwrap().unwrap();
        assert_eq!((me.expire_timer, me.expire_timer_version), (3600, 2));
        assert_eq!(
            store.expire_timer(&note_to_self).await.unwrap(),
            Some((3600, 2))
        );
        assert_eq!(
            store
                .profile_key(&aci(ME))
                .await
                .unwrap()
                .unwrap()
                .get_bytes(),
            registration().profile_key().get_bytes()
        );
    }
}
