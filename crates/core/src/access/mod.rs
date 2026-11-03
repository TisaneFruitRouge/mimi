//! People the user trusts to ask their assistant things from their own messaging app.
//!
//! The user turns it on for someone on their card in People (`person_access`, migration
//! 0026), picks the calendars shared with them and who approves what they ask for. Off
//! for everyone until then. When that person writes to the assistant (on Matrix today,
//! recognised by an address from an address book or added by hand, never by a name or a
//! card Mimi made from mail), their message goes into a conversation of their own and
//! runs as a *guest*: [`Principal::Guest`] travels with the turn through `chat` into
//! every tool call. A guest gets none of the owner's memory, no custom instructions, and
//! an explicit allow-list of tools ([`tools`]): their shared calendars and their own
//! reminders and routines. Nothing else of the owner's can be reached from their turn.
//!
//! A guest's conversation belongs to the guest. It is stored (the chat engine needs its
//! history) but the owner's clients never list or receive it ([`Access::hides`]);
//! turning access off or deleting the person deletes it. When the owner approves for
//! them, only the approval itself reaches the owner ([`ask_owner`]).
//!
//! Each app plugs in by recognising the sender ([`find`]), keeping a conversation per
//! line ([`line_conversation`]) and giving a [`Channel`] back for a line ([`lines`]).

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use mimi_protocol::{
    Action, ActionStatus, Approver, Channel as HandleChannel, Event, GuestApproval, PersonAccess,
    PersonAccessUpdate,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::error::AppError;
use crate::channels::{self, Channel, Outgoing};
use crate::db::parse_uuid;
use crate::people::merge;
use crate::people::normalize::match_key;
use crate::{AppState, now_ms};

pub mod tools;

/// Most calendars shared with one person.
const MAX_CALENDARS: usize = 50;

/// Which conversations belong to trusted people, so nothing about them reaches the
/// owner's clients. Filled at startup, and before a guest conversation is first saved.
#[derive(Default)]
pub struct Access {
    hidden: RwLock<HashSet<Uuid>>,
}

impl Access {
    /// Whether a conversation is a trusted person's.
    pub fn is_guest_conversation(&self, id: Uuid) -> bool {
        self.hidden
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&id)
    }

    pub(crate) fn mark(&self, id: Uuid) {
        self.hidden
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id);
    }

    fn unmark(&self, id: Uuid) {
        self.hidden
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    /// Whether an event is about a trusted person's conversation, and so must not reach
    /// the owner's clients.
    pub fn hides(&self, event: &Event) -> bool {
        let conversation = match event {
            Event::ConversationUpdated { conversation } => Some(conversation.id),
            Event::ConversationDeleted { id } => Some(*id),
            Event::MessageUpdated { message } => Some(message.conversation_id),
            Event::MessageDelta {
                conversation_id, ..
            } => Some(*conversation_id),
            Event::ScheduleDelivered { delivery } => delivery.conversation_id,
            _ => None,
        };
        conversation.is_some_and(|id| self.is_guest_conversation(id))
    }
}

/// Loads which conversations are guests', before anything is served.
pub async fn load(state: &AppState) {
    let ids: Vec<Uuid> = state
        .db
        .call(|c| {
            let mut stmt = c.prepare("SELECT conversation_id FROM guest_conversations")?;
            stmt.query_map([], |r| parse_uuid(r, 0))?.collect()
        })
        .await
        .unwrap_or_default();
    for id in ids {
        state.access.mark(id);
    }
}

/// Someone the user trusts, as they are right now: who they are and what's shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guest {
    pub person_id: Uuid,
    /// What the user calls them in People: their nickname, else their name.
    pub name: String,
    /// The calendars shared with them: the only ones their turns can read or write.
    pub calendars: Vec<String>,
    pub approver: Approver,
    /// How the assistant refers to the user it works for, e.g. "Vincent".
    pub owner: String,
}

/// Who a chat turn is for. Decided from the conversation, never from what's in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Principal {
    /// The user who runs this assistant.
    #[default]
    Owner,
    /// Someone they trust, in their own conversation.
    Guest(Guest),
}

impl Principal {
    pub fn guest(&self) -> Option<&Guest> {
        match self {
            Principal::Owner => None,
            Principal::Guest(g) => Some(g),
        }
    }

    /// The calendars this turn may use: `None` for the owner (all of them).
    pub fn calendars(&self) -> Option<&[String]> {
        self.guest().map(|g| g.calendars.as_slice())
    }
}

/// A calendar shared with a trusted person, as their prompt names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedCalendar {
    pub name: String,
    pub writable: bool,
}

/// The shared calendars that are connected right now.
pub async fn shared_calendars(state: &AppState, guest: &Guest) -> Vec<SharedCalendar> {
    let accounts = crate::connections::calendar::restrict(
        crate::connections::calendar_accounts(state).await,
        &guest.calendars,
    );
    crate::connections::calendar::calendars(&accounts)
        .into_iter()
        .map(|c| SharedCalendar {
            name: c.name,
            writable: c.writable,
        })
        .collect()
}

// --- Stored access -----------------------------------------------------------------------

/// A `person_access` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub person_id: Uuid,
    pub enabled: bool,
    pub calendars: Vec<String>,
    pub approver: Approver,
    pub updated_at: i64,
}

fn approver_str(a: Approver) -> &'static str {
    match a {
        Approver::Guest => "guest",
        Approver::Owner => "owner",
    }
}

pub fn row(c: &Connection, person: Uuid) -> rusqlite::Result<Option<Row>> {
    c.query_row(
        "SELECT enabled, calendars, approver, updated_at FROM person_access WHERE person_id = ?1",
        [person.to_string()],
        |r| {
            let calendars: String = r.get(1)?;
            let approver: String = r.get(2)?;
            Ok(Row {
                person_id: person,
                enabled: r.get(0)?,
                calendars: serde_json::from_str(&calendars).unwrap_or_default(),
                approver: if approver == "owner" {
                    Approver::Owner
                } else {
                    Approver::Guest
                },
                updated_at: r.get(3)?,
            })
        },
    )
    .optional()
}

pub fn put_row(c: &Connection, row: &Row) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO person_access (person_id, enabled, calendars, approver, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (person_id) DO UPDATE SET enabled = excluded.enabled,
           calendars = excluded.calendars, approver = excluded.approver,
           updated_at = excluded.updated_at",
        params![
            row.person_id.to_string(),
            row.enabled,
            serde_json::to_string(&row.calendars).expect("calendar ids serialize"),
            approver_str(row.approver),
            row.updated_at,
        ],
    )?;
    Ok(())
}

pub fn delete_row(c: &Connection, person: Uuid) -> rusqlite::Result<()> {
    c.execute(
        "DELETE FROM person_access WHERE person_id = ?1",
        [person.to_string()],
    )?;
    Ok(())
}

/// Several people's access as one, when they're merged: the stricter choice, as for
/// permission exceptions. On only if everyone who had a choice had it on, the calendars
/// shared with all of them, and the owner approves if that was anyone's choice. `None`
/// when nobody had one.
pub fn combine(rows: &[Row], person: Uuid, now: i64) -> Option<Row> {
    let first = rows.first()?;
    let enabled = rows.iter().all(|r| r.enabled);
    let calendars = if enabled {
        first
            .calendars
            .iter()
            .filter(|c| rows.iter().all(|r| r.calendars.contains(c)))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let approver = if rows.iter().any(|r| r.approver == Approver::Owner) {
        Approver::Owner
    } else {
        Approver::Guest
    };
    Some(Row {
        person_id: person,
        enabled,
        calendars,
        approver,
        updated_at: now,
    })
}

/// The person and everyone merged into them over time: rows that point at any of them
/// (conversations, reminders) are theirs.
fn ids_of(c: &Connection, person: Uuid) -> rusqlite::Result<Vec<String>> {
    let mut ids = merge::merged_into(c, person)?;
    ids.push(person);
    Ok(ids.into_iter().map(|i| i.to_string()).collect())
}

// --- Viewing and changing -----------------------------------------------------------------

async fn mail_sources(state: &AppState) -> Vec<String> {
    crate::mail::accounts(state)
        .await
        .into_iter()
        .map(|a| a.id.to_string())
        .collect()
}

/// The Matrix addresses a person can write from: from an address book or added by hand,
/// never one Mimi only saw in mail.
async fn addresses_of(state: &AppState, person: &mimi_protocol::Person) -> Vec<String> {
    let mail = mail_sources(state).await;
    let mut out: Vec<String> = person
        .handles
        .iter()
        .filter(|h| h.channel == HandleChannel::Matrix)
        .filter(|h| h.source_id.as_ref().is_none_or(|s| !mail.contains(s)))
        .map(|h| h.value.trim().to_owned())
        .collect();
    out.sort_by_key(|a| a.to_lowercase());
    out.dedup_by_key(|a| a.to_lowercase());
    out
}

/// Someone's access, as their card shows it.
pub async fn view(state: &AppState, person: Uuid) -> Result<PersonAccess, AppError> {
    let found = crate::people::get(state, person)
        .await?
        .filter(|p| p.id == person)
        .ok_or_else(|| AppError::not_found("Person"))?;
    let stored = state.db.call(move |c| row(c, person)).await?;
    let stored = stored.unwrap_or(Row {
        person_id: person,
        enabled: false,
        calendars: Vec::new(),
        approver: Approver::Guest,
        updated_at: 0,
    });
    Ok(PersonAccess {
        person_id: person,
        enabled: stored.enabled,
        calendars: stored.calendars,
        approver: stored.approver,
        addresses: addresses_of(state, &found).await,
        assistant_addresses: crate::connections::matrix::guests::assistant_addresses(state).await,
    })
}

/// Changes someone's access: the user's own choice, through the API only. Turning it
/// off deletes their conversations and their reminders.
pub async fn update(
    state: &AppState,
    person: Uuid,
    change: PersonAccessUpdate,
) -> Result<PersonAccess, AppError> {
    crate::people::get(state, person)
        .await?
        .filter(|p| p.id == person)
        .ok_or_else(|| AppError::not_found("Person"))?;
    let before = state.db.call(move |c| row(c, person)).await?;
    let mut next = before.clone().unwrap_or(Row {
        person_id: person,
        enabled: false,
        calendars: Vec::new(),
        approver: Approver::Guest,
        updated_at: 0,
    });
    if let Some(enabled) = change.enabled {
        next.enabled = enabled;
    }
    if let Some(approver) = change.approver {
        next.approver = approver;
    }
    if let Some(calendars) = change.calendars {
        let known: Vec<String> = crate::connections::calendar::calendars(
            &crate::connections::calendar_accounts(state).await,
        )
        .into_iter()
        .map(|c| c.id)
        .collect();
        let mut picked: Vec<String> = Vec::new();
        for id in calendars {
            if !known.contains(&id) {
                return Err(AppError::bad_request(
                    "One of those calendars isn't connected anymore.",
                ));
            }
            if !picked.contains(&id) {
                picked.push(id);
            }
        }
        if picked.len() > MAX_CALENDARS {
            return Err(AppError::bad_request(format!(
                "Share at most {MAX_CALENDARS} calendars."
            )));
        }
        next.calendars = picked;
    }
    next.updated_at = now_ms();
    let saved = next.clone();
    state.db.call(move |c| put_row(c, &saved)).await?;
    let was_on = before.is_some_and(|b| b.enabled);
    if was_on && !next.enabled {
        revoke(state, person).await;
    }
    state
        .events
        .publish(Event::PersonAccessChanged { person_id: person });
    view(state, person).await
}

/// Takes away what someone had with the assistant: their conversations, their reminders
/// and routines, and the chats they opened with it.
pub async fn revoke(state: &AppState, person: Uuid) {
    // Their lines are known from their conversations: read them before those go.
    let lines = lines_rows(state, person).await;
    let conversations: Vec<Uuid> = state
        .db
        .call(move |c| {
            let ids = ids_of(c, person)?;
            let mut stmt =
                c.prepare("SELECT conversation_id FROM guest_conversations WHERE person_id = ?1")?;
            let mut out = Vec::new();
            for id in ids {
                for conv in stmt.query_map([id], |r| parse_uuid(r, 0))? {
                    out.push(conv?);
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default();
    for id in conversations {
        delete_conversation(state, id).await;
    }
    let items: Vec<Uuid> = state
        .db
        .call(move |c| {
            let ids = ids_of(c, person)?;
            let mut stmt = c.prepare("SELECT id FROM schedule_items WHERE for_person = ?1")?;
            let mut out = Vec::new();
            for id in ids {
                for item in stmt.query_map([id], |r| parse_uuid(r, 0))? {
                    out.push(item?);
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default();
    for id in items {
        let _ = crate::schedule::delete(state, id).await;
    }
    crate::connections::matrix::guests::leave_their_chats(state, &lines).await;
    tracing::info!(person = %person, "access taken back");
}

/// Deletes a guest conversation: nobody else ever saw it.
async fn delete_conversation(state: &AppState, id: Uuid) {
    state.generations.cancel(id);
    if let Err(e) = crate::chat::store::delete_conversation(&state.db, id).await {
        tracing::warn!("deleting a trusted person's conversation failed: {e}");
        return;
    }
    // Only once it's gone: until then it stays hidden.
    state.access.unmark(id);
}

/// After People changed (a sync dropped someone, a deletion): what belonged to people
/// who are gone, or who no longer have access, goes.
pub async fn tidy(state: &AppState) {
    let people: Vec<(String, Option<Uuid>, bool)> = state
        .db
        .call(|c| {
            let mut stmt = c.prepare(
                "SELECT person_id FROM guest_conversations
                 UNION SELECT for_person FROM schedule_items WHERE for_person IS NOT NULL",
            )?;
            let ids: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<_, _>>()?;
            let mut out = Vec::new();
            for raw in ids {
                let now = match raw.parse::<Uuid>() {
                    Ok(id) => merge::resolve(c, id)?,
                    Err(_) => None,
                };
                let enabled = match now {
                    Some(p) => row(c, p)?.is_some_and(|r| r.enabled),
                    None => false,
                };
                out.push((raw, now, enabled));
            }
            Ok(out)
        })
        .await
        .unwrap_or_default();
    for (raw, now, enabled) in people {
        if enabled {
            continue;
        }
        // Gone or turned off: everything under the old id goes.
        let Ok(id) = raw.parse::<Uuid>() else {
            continue;
        };
        revoke(state, now.unwrap_or(id)).await;
        if now.is_some_and(|n| n != id) {
            revoke(state, id).await;
        }
    }
}

// --- Recognising them -----------------------------------------------------------------------

async fn owner_name(state: &AppState) -> String {
    crate::connections::matrix::guests::owner_name(state)
        .await
        .unwrap_or_else(|| "the person who runs this assistant".to_owned())
}

/// Someone trusted, as they are right now. `None` if they're gone or access is off.
pub async fn guest(state: &AppState, person: Uuid) -> Option<Guest> {
    let found = crate::people::get(state, person).await.ok()??;
    let id = found.id;
    let stored = state.db.call(move |c| row(c, id)).await.ok()??;
    if !stored.enabled {
        return None;
    }
    Some(Guest {
        person_id: id,
        // What the user calls them: "Maya" rather than "Oumaya Laadhari".
        name: found
            .nickname
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(found.name),
        calendars: stored.calendars,
        approver: stored.approver,
        owner: owner_name(state).await,
    })
}

/// The trusted person who writes from this address in an app (`channel`: Matrix
/// today), if exactly one person in People with access has it from an address book or
/// added by hand. Never by name, never from a card Mimi made from mail.
pub async fn find(state: &AppState, channel: HandleChannel, address: &str) -> Option<Guest> {
    let key = match_key(channel, address)?;
    let kind = match channel {
        HandleChannel::Matrix => "matrix",
        HandleChannel::Telegram => "telegram",
        _ => return None,
    };
    let mail = mail_sources(state).await;
    let candidates: Vec<Uuid> = state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT h.person_id, h.source FROM person_handles h
                 JOIN person_access a ON a.person_id = h.person_id AND a.enabled = 1
                 WHERE h.channel = ?1 AND h.match_key = ?2",
            )?;
            let rows = stmt.query_map((kind, key), |r| {
                Ok((parse_uuid(r, 0)?, r.get::<_, Option<String>>(1)?))
            })?;
            let mut out: Vec<Uuid> = Vec::new();
            for row in rows {
                let (person, source) = row?;
                if source.is_none_or(|s| !mail.contains(&s)) && !out.contains(&person) {
                    out.push(person);
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default();
    match candidates.as_slice() {
        [one] => guest(state, *one).await,
        [] => None,
        _ => {
            tracing::warn!(
                "a Matrix address belongs to several trusted people; nobody is recognised by it"
            );
            None
        }
    }
}

/// Who a turn in this conversation is for: the owner, or the trusted person it belongs
/// to. Refused when that person is gone or their access is off.
pub async fn principal_for(state: &AppState, conversation: Uuid) -> Result<Principal, AppError> {
    let person: Option<Uuid> = state
        .db
        .call(move |c| {
            c.query_row(
                "SELECT person_id FROM guest_conversations WHERE conversation_id = ?1",
                [conversation.to_string()],
                |r| parse_uuid(r, 0),
            )
            .optional()
        })
        .await?;
    let Some(person) = person else {
        return Ok(Principal::Owner);
    };
    guest(state, person)
        .await
        .map(Principal::Guest)
        .ok_or_else(|| {
            AppError::new(
                axum::http::StatusCode::FORBIDDEN,
                "no_access",
                "They can't ask the assistant things anymore.",
            )
        })
}

// --- Their conversations and lines ------------------------------------------------------------

/// Where a trusted person talks to the assistant: an app, the assistant's account there,
/// their own address, and the chat (a Matrix room).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub app: String,
    pub connection: Uuid,
    pub address: String,
    pub chat: String,
}

/// The conversation of a trusted person's line, made on their first message. `fresh`
/// starts a new one (their `/new`): the old one is deleted, since nobody else can open
/// it.
pub async fn line_conversation(
    state: &AppState,
    guest: &Guest,
    line: &Line,
    fresh: bool,
) -> Option<Uuid> {
    let (person, l) = (guest.person_id, line.clone());
    let existing: Option<Uuid> = state
        .db
        .call(move |c| {
            let ids = ids_of(c, person)?;
            let mut stmt = c.prepare(
                "SELECT conversation_id FROM guest_conversations
                 WHERE person_id = ?1 AND app = ?2 AND connection_id = ?3 AND chat = ?4",
            )?;
            for id in ids {
                if let Some(found) = stmt
                    .query_row((id, &l.app, l.connection.to_string(), &l.chat), |r| {
                        parse_uuid(r, 0)
                    })
                    .optional()?
                {
                    return Ok(Some(found));
                }
            }
            Ok(None)
        })
        .await
        .ok()?;
    match existing {
        Some(id) if !fresh => return Some(id),
        Some(id) => delete_conversation(state, id).await,
        None => {}
    }
    new_conversation(state, guest, Some(line.clone()), &guest.name).await
}

/// A conversation for one of a trusted person's routines.
pub async fn routine_conversation(state: &AppState, person: Uuid, title: &str) -> Option<Uuid> {
    let guest = guest(state, person).await?;
    new_conversation(state, &guest, None, title).await
}

async fn new_conversation(
    state: &AppState,
    guest: &Guest,
    line: Option<Line>,
    title: &str,
) -> Option<Uuid> {
    let conversation = crate::chat::new_conversation(Some(title.to_owned()));
    let id = conversation.id;
    // Hidden before it exists, so no event about it can slip out.
    state.access.mark(id);
    let person = guest.person_id;
    let saved = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO conversations (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    id.to_string(),
                    conversation.title,
                    conversation.created_at,
                    conversation.updated_at
                ],
            )?;
            tx.execute(
                "INSERT INTO guest_conversations
                   (conversation_id, person_id, app, connection_id, address, chat, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id.to_string(),
                    person.to_string(),
                    line.as_ref().map(|l| l.app.clone()),
                    line.as_ref().map(|l| l.connection.to_string()),
                    line.as_ref().map(|l| l.address.clone()),
                    line.as_ref().map(|l| l.chat.clone()),
                    now_ms()
                ],
            )?;
            tx.commit()
        })
        .await;
    match saved {
        Ok(()) => Some(id),
        Err(e) => {
            tracing::warn!("saving a trusted person's conversation failed: {e}");
            state.access.unmark(id);
            None
        }
    }
}

/// A trusted person's lines, as recorded.
async fn lines_rows(state: &AppState, person: Uuid) -> Vec<Line> {
    state
        .db
        .call(move |c| {
            let ids = ids_of(c, person)?;
            let mut stmt = c.prepare(
                "SELECT app, connection_id, address, chat FROM guest_conversations
                 WHERE person_id = ?1 AND app IS NOT NULL",
            )?;
            let mut out: Vec<Line> = Vec::new();
            for id in ids {
                let rows = stmt.query_map([id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })?;
                for row in rows {
                    let (app, connection, address, chat) = row?;
                    let Ok(connection) = connection.parse() else {
                        continue;
                    };
                    let line = Line {
                        app,
                        connection,
                        address,
                        chat,
                    };
                    if !out.contains(&line) {
                        out.push(line);
                    }
                }
            }
            Ok(out)
        })
        .await
        .unwrap_or_default()
}

/// Where Mimi reaches a trusted person on its own (their reminders, their routines'
/// results): each of their lines whose app is running.
pub async fn lines(state: &Arc<AppState>, person: Uuid) -> Vec<Arc<dyn Channel>> {
    let mut out: Vec<Arc<dyn Channel>> = Vec::new();
    for line in lines_rows(state, person).await {
        let channel = match line.app.as_str() {
            crate::connections::matrix::MATRIX => {
                crate::connections::matrix::guests::channel(state, &line)
            }
            _ => None,
        };
        out.extend(channel);
    }
    out
}

// --- Approvals ----------------------------------------------------------------------------------

/// Who approves an action in this conversation, when the owner does for a trusted
/// person: that person. `None`: whoever the conversation's channel reaches.
pub async fn owner_approves_for(state: &AppState, conversation: Uuid) -> Option<Guest> {
    if !state.access.is_guest_conversation(conversation) {
        return None;
    }
    match principal_for(state, conversation).await {
        Ok(Principal::Guest(g)) if g.approver == Approver::Owner => Some(g),
        _ => None,
    }
}

/// Asks the owner to approve what a trusted person asked for: the card goes to their
/// messaging apps and their app (with a desktop nudge), and the person hears that it
/// waits. Only the action reaches the owner, never the conversation.
pub async fn ask_owner(state: &AppState, guest: &Guest, action: &Action, theirs: &[&dyn Channel]) {
    let mut asked = action.clone();
    asked.summary = format!("For {}: {}", guest.name, action.summary);
    channels::ask_all(&channels::owners(state).await, &asked).await;
    state.events.publish(Event::GuestApproval {
        approval: GuestApproval {
            person_id: guest.person_id,
            name: guest.name.clone(),
            action: action.clone(),
        },
    });
    if crate::settings::load(&state.db)
        .await
        .map(|s| s.desktop_notifications)
        .unwrap_or(true)
    {
        crate::schedule::notify::show(
            format!("{} asks for your OK", guest.name),
            action.summary.clone(),
        )
        .await;
    }
    let note = Outgoing::text(format!(
        "I've asked {} to OK this first: {}",
        guest.owner, action.summary
    ));
    for channel in theirs {
        if let Err(e) = channel.send(&note).await {
            tracing::warn!("telling a trusted person on {} failed: {e}", channel.kind());
        }
    }
}

/// Tells the owner's app that an approval they were asked for is settled.
pub fn settled(state: &AppState, guest: &Guest, action: &Action) {
    state.events.publish(Event::GuestApproval {
        approval: GuestApproval {
            person_id: guest.person_id,
            name: guest.name.clone(),
            action: action.clone(),
        },
    });
}

/// Approvals the owner was asked for that still wait: the app shows them at launch.
pub async fn waiting_for_owner(state: &AppState) -> Vec<GuestApproval> {
    let conversations: Vec<Uuid> = state
        .access
        .hidden
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .copied()
        .collect();
    let mut out = Vec::new();
    for conversation in conversations {
        let Some(guest) = owner_approves_for(state, conversation).await else {
            continue;
        };
        let Ok(messages) = crate::chat::store::messages(&state.db, conversation).await else {
            continue;
        };
        for message in messages {
            for action in message.actions {
                if action.status == ActionStatus::PendingApproval
                    && state.approvals.is_waiting(action.id)
                {
                    out.push(GuestApproval {
                        person_id: guest.person_id,
                        name: guest.name.clone(),
                        action,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(enabled: bool, calendars: &[&str], approver: Approver) -> Row {
        Row {
            person_id: Uuid::nil(),
            enabled,
            calendars: calendars.iter().map(|c| (*c).to_owned()).collect(),
            approver,
            updated_at: 0,
        }
    }

    #[test]
    fn merged_access_takes_the_stricter_choice() {
        let keep = Uuid::now_v7();
        assert_eq!(combine(&[], keep, 1), None);
        let one = combine(&[access(true, &["a", "b"], Approver::Guest)], keep, 1).unwrap();
        assert!(one.enabled);
        assert_eq!(one.calendars, ["a", "b"]);
        assert_eq!(one.person_id, keep);
        let both = combine(
            &[
                access(true, &["a", "b"], Approver::Guest),
                access(true, &["b", "c"], Approver::Owner),
            ],
            keep,
            1,
        )
        .unwrap();
        assert!(both.enabled);
        assert_eq!(both.calendars, ["b"]);
        assert_eq!(both.approver, Approver::Owner);
        let off = combine(
            &[
                access(true, &["a"], Approver::Guest),
                access(false, &["a"], Approver::Guest),
            ],
            keep,
            1,
        )
        .unwrap();
        assert!(!off.enabled);
        assert!(off.calendars.is_empty());
    }
}
