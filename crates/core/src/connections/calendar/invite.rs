//! Invitations sent through the user's own email, never by the calendar service.
//!
//! Saving, changing or removing an event with guests emails nobody (see `guests.rs`).
//! Instead Mimi records an *offer*: the event as it was then, and who could be told.
//! The user sends it with a click (the Calendar panel, a card in the chat), or asks the
//! assistant to (`calendar_send_invitations`, which follows the `send_mail` permission).
//!
//! What goes out is a standard iMIP message (RFC 6047): a plain-text part saying what,
//! when and where, and the event as `text/calendar; method=REQUEST` (or `CANCEL`), inline
//! and as `invite.ics`, so calendar apps offer Accept/Decline. One email goes to all the
//! guests of an offer, like Outlook and Thunderbird send it: every calendar app finds its
//! own guest line in it, the guests already see each other in the guest list, and the
//! user gets a single copy in Sent of exactly what everyone received.
//!
//! `SEQUENCE` is what the calendar has for the event, but always above the last one sent
//! for the same event (`calendar_invitations.sent_sequence`): an update or a cancellation
//! must be newer than what the guests already have, even when the calendar didn't count
//! the change (Google counts only new times).

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone, Utc};
use icalendar::{Calendar, Component, EventLike, Property};
use lettre::Message;
use lettre::message::header::{ContentTransferEncoding, ContentType};
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use mimi_protocol::{GuestResponse, InvitationGuest, InvitationKind, InvitationOffer};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::edit::Changes;
use super::guests::{self, Guest};
use super::{Created, EventRef, Located, NewEvent, Target};
use crate::AppState;

/// The event as it was when the offer was made: what the guests will be told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The UID other calendars know it by.
    pub ical_uid: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub location: Option<String>,
    pub notes: Option<String>,
    /// For one occurrence of a repeating event: its original start (`RECURRENCE-ID`).
    pub occurrence: Option<DateTime<Utc>>,
    pub sequence: u32,
    /// Every guest, without the user.
    pub guests: Vec<Guest>,
    /// Who organizes it, to send from the matching email account.
    pub organizer: Option<String>,
}

impl Snapshot {
    pub fn of(found: &Located, me: &HashSet<String>) -> Snapshot {
        let e = &found.event;
        Snapshot {
            ical_uid: found.ical_uid.clone(),
            title: e.title.clone(),
            start: e.start,
            end: e.end,
            all_day: e.all_day,
            location: e.location.clone(),
            notes: e.notes.clone(),
            occurrence: found.occurrence,
            sequence: found.sequence,
            guests: guests::guests_of(e, me),
            organizer: e.organizer.as_ref().map(|o| o.email.clone()),
        }
    }

    /// Whether what guests see (title, time, place, notes) is the same.
    fn same_details(&self, other: &Snapshot) -> bool {
        self.title == other.title
            && self.start == other.start
            && self.end == other.end
            && self.all_day == other.all_day
            && self.location == other.location
            && self.notes == other.notes
    }

    /// "on Friday 3 Oct, 19:00–21:00" in local time.
    pub fn when(&self) -> String {
        super::tools::describe_span(self.start, self.end, self.all_day)
    }
}

/// What to offer after an event changed, from how it was and how it is: the invitation
/// for new guests, the new details for guests who stay (when something they'd see
/// changed), and the invitation withdrawn for guests who were removed.
pub fn plan_after_change(before: &Snapshot, after: &Snapshot) -> Vec<(InvitationKind, Vec<Guest>)> {
    let had = |g: &Guest| before.guests.iter().any(|b| b.email == g.email);
    let has = |g: &Guest| after.guests.iter().any(|a| a.email == g.email);
    let added: Vec<Guest> = after.guests.iter().filter(|g| !had(g)).cloned().collect();
    let stayed: Vec<Guest> = after.guests.iter().filter(|g| had(g)).cloned().collect();
    let removed: Vec<Guest> = before.guests.iter().filter(|g| !has(g)).cloned().collect();
    let mut out = Vec::new();
    if !added.is_empty() {
        out.push((InvitationKind::Invite, added));
    }
    if !before.same_details(after) && !stayed.is_empty() {
        out.push((InvitationKind::Update, stayed));
    }
    if !removed.is_empty() {
        out.push((InvitationKind::Uninvite, removed));
    }
    out
}

/// What the rest of Mimi learns from a write: the offers made (nothing was emailed) and
/// anything the user should be told.
#[derive(Debug, Default)]
pub struct Outcome {
    pub offers: Vec<Uuid>,
    pub note: Option<String>,
}

/// Where an offer's event is, so the assistant can find the offer by the event's id.
struct Place<'a> {
    calendar_id: &'a str,
    calendar: &'a str,
    event_uid: &'a str,
    start: DateTime<Utc>,
}

// --- Writes that make offers ----------------------------------------------------------

/// Adds an event and, when it has guests, offers their invitations. A calendar that
/// can't take guests gets the event without them, and the note says why.
pub async fn add(
    state: &AppState,
    target: &Target,
    mut event: NewEvent,
) -> Result<(Created, Outcome), String> {
    let mut outcome = Outcome::default();
    if !event.guests.is_empty() {
        match guests::organizer_for(state, target).await {
            Ok(me) => event.organizer = Some(me),
            Err(why) => {
                outcome.note = Some(format!(
                    "{why} The event was saved without {}.",
                    guests::names(&event.guests)
                ));
                event.guests.clear();
            }
        }
    }
    let created = super::create(&state.http, &state.connections.feeds, target, &event).await?;
    if let Created::Saved { uid, ical_uid, .. } = &created
        && !event.guests.is_empty()
    {
        let accounts = crate::connections::calendar_accounts(state).await;
        let me = guests::my_addresses(state, &accounts).await;
        let guests: Vec<Guest> = event
            .guests
            .iter()
            .filter(|g| !me.contains(&g.email))
            .cloned()
            .collect();
        let snapshot = Snapshot {
            ical_uid: ical_uid.clone(),
            title: event.title.clone(),
            start: event.start,
            end: event.end,
            all_day: event.all_day,
            location: event.location.clone(),
            notes: event.notes.clone(),
            occurrence: None,
            sequence: 0,
            guests: guests.clone(),
            organizer: event.organizer.clone(),
        };
        let place = Place {
            calendar_id: target.id(),
            calendar: target.name(),
            event_uid: uid,
            start: event.start,
        };
        if !guests.is_empty() {
            outcome
                .offers
                .push(save_offer(state, InvitationKind::Invite, &place, &snapshot, &guests).await?);
        }
    }
    Ok((created, outcome))
}

/// Changes an event and offers to tell its guests. `before` is the event as found just
/// now. Changing guests is refused for events someone else organizes, and for every
/// occurrence of a repeating event at once.
pub async fn change(
    state: &AppState,
    r: &EventRef,
    before: &Located,
    whole: bool,
    mut changes: Changes,
) -> Result<(Option<Located>, Outcome), String> {
    let accounts = crate::connections::calendar_accounts(state).await;
    let me = guests::my_addresses(state, &accounts).await;
    let mut outcome = Outcome::default();
    let series = whole && before.repeats;
    if let Some(list) = changes.guests.take() {
        check_guests_may_change(before, &me, series)?;
        let target = super::target_by_id(&accounts, &r.calendar_id)
            .ok_or("That calendar isn't connected anymore.")?;
        let now = guests::guests_of(&before.event, &me);
        let same =
            now.len() == list.len() && now.iter().all(|g| list.iter().any(|l| l.email == g.email));
        if !same {
            changes.organizer = Some(guests::organizer_for(state, &target).await?);
            // The user's own entries (Google lists the organizer too) stay.
            let mut all = list;
            for a in &before.event.attendees {
                if (a.me || me.contains(&a.email)) && !all.iter().any(|g| g.email == a.email) {
                    all.push(Guest::from_attendee(a));
                }
            }
            changes.guests = Some(all);
        }
    }
    if !changes.details() && changes.guests.is_none() {
        return Err("Nothing to change.".to_owned());
    }
    super::change_event(
        &state.http,
        &state.connections.feeds,
        &accounts,
        r,
        whole,
        &changes,
        &me,
    )
    .await?;
    let moved = EventRef {
        start: match changes.when {
            Some(w) if !series => w.start,
            _ => r.start,
        },
        ..r.clone()
    };
    let after = super::locate(&state.http, &state.connections.feeds, &accounts, &moved)
        .await
        .ok();
    if !guests::is_mine(&before.event, &me) {
        return Ok((after, outcome));
    }
    let had_guests = !guests::guests_of(&before.event, &me).is_empty();
    match &after {
        Some(_) if series => {
            if had_guests && changes.details() {
                outcome.note = Some(
                    "The guests weren't told: Mimi sends news about one occurrence of a repeating event at a time. Tell them from your calendar app if they need to know."
                        .to_owned(),
                );
            }
        }
        Some(after) => {
            let (b, a) = (Snapshot::of(before, &me), Snapshot::of(after, &me));
            let place = Place {
                calendar_id: &after.event.calendar_id,
                calendar: &after.event.calendar,
                event_uid: &after.event.uid,
                start: after.event.start,
            };
            for (kind, recipients) in plan_after_change(&b, &a) {
                outcome
                    .offers
                    .push(save_offer(state, kind, &place, &a, &recipients).await?);
            }
        }
        None if had_guests => {
            outcome.note = Some(
                "The change was saved, but Mimi couldn't read the event back to offer telling the guests.".to_owned(),
            );
        }
        None => {}
    }
    Ok((after, outcome))
}

/// Guests change only on the user's own events, one occurrence of a series at a time.
pub fn check_guests_may_change(
    before: &Located,
    me: &HashSet<String>,
    series: bool,
) -> Result<(), String> {
    if !guests::is_mine(&before.event, me) {
        let who = before
            .event
            .organizer
            .as_ref()
            .map(|o| o.name.clone().unwrap_or_else(|| o.email.clone()))
            .unwrap_or_else(|| "Someone else".to_owned());
        return Err(format!(
            "{who} organizes this event, so only they can change its guests."
        ));
    }
    if series {
        return Err("Guests can be changed for one occurrence of a repeating event at a time here. To invite people to every occurrence, use the calendar app.".to_owned());
    }
    Ok(())
}

/// Removes an event and, when it had guests, offers to tell them it's cancelled.
pub async fn remove(
    state: &AppState,
    r: &EventRef,
    before: &Located,
    whole: bool,
) -> Result<Outcome, String> {
    let accounts = crate::connections::calendar_accounts(state).await;
    let me = guests::my_addresses(state, &accounts).await;
    super::remove_event(
        &state.http,
        &state.connections.feeds,
        &accounts,
        r,
        whole,
        &me,
    )
    .await?;
    let mut outcome = Outcome::default();
    let mut snapshot = Snapshot::of(before, &me);
    if guests::is_mine(&before.event, &me) && !snapshot.guests.is_empty() {
        if whole {
            // No RECURRENCE-ID: the whole event, every occurrence of it, is cancelled.
            snapshot.occurrence = None;
        }
        let place = Place {
            calendar_id: &r.calendar_id,
            calendar: &before.event.calendar,
            event_uid: &r.uid,
            start: r.start,
        };
        let recipients = snapshot.guests.clone();
        outcome.offers.push(
            save_offer(
                state,
                InvitationKind::Cancel,
                &place,
                &snapshot,
                &recipients,
            )
            .await?,
        );
    }
    Ok(outcome)
}

/// A fresh invitation to every guest of the event as it is now (the event sheet's "Send
/// invitations…", or the assistant asked for an event with no offer waiting).
pub async fn offer_current(
    state: &AppState,
    r: &EventRef,
    found: &Located,
) -> Result<Uuid, String> {
    let accounts = crate::connections::calendar_accounts(state).await;
    let me = guests::my_addresses(state, &accounts).await;
    if !guests::is_mine(&found.event, &me) {
        return Err(
            "Someone else organizes this event: invitations are theirs to send.".to_owned(),
        );
    }
    let snapshot = Snapshot::of(found, &me);
    if snapshot.guests.is_empty() {
        return Err("That event has no guests to invite.".to_owned());
    }
    let place = Place {
        calendar_id: &r.calendar_id,
        calendar: &found.event.calendar,
        event_uid: &r.uid,
        start: r.start,
    };
    let recipients = snapshot.guests.clone();
    save_offer(
        state,
        InvitationKind::Invite,
        &place,
        &snapshot,
        &recipients,
    )
    .await
}

// --- The offers table -----------------------------------------------------------------

/// One offer as stored.
#[derive(Debug, Clone)]
pub struct Stored {
    pub id: Uuid,
    pub kind: InvitationKind,
    pub calendar_id: String,
    pub calendar: String,
    pub event_uid: String,
    pub start: DateTime<Utc>,
    pub snapshot: Snapshot,
    pub recipients: Vec<Guest>,
    pub sent_at: Option<i64>,
    pub sent_from: Option<String>,
}

/// Offers never sent are forgotten after a month.
const KEEP_UNSENT_MS: i64 = 30 * 86_400_000;

async fn save_offer(
    state: &AppState,
    kind: InvitationKind,
    place: &Place<'_>,
    snapshot: &Snapshot,
    recipients: &[Guest],
) -> Result<Uuid, String> {
    let id = Uuid::now_v7();
    let row = (
        id.to_string(),
        crate::db::enum_str(kind),
        place.calendar_id.to_owned(),
        place.calendar.to_owned(),
        place.event_uid.to_owned(),
        place.start.timestamp_millis(),
        snapshot.ical_uid.clone(),
        snapshot.occurrence.map(|o| o.timestamp_millis()),
        serde_json::to_string(snapshot).map_err(|e| e.to_string())?,
        serde_json::to_string(recipients).map_err(|e| e.to_string())?,
    );
    let now = crate::now_ms();
    state
        .db
        .call(move |c| {
            c.execute(
                "DELETE FROM calendar_invitations WHERE sent_at IS NULL AND created_at < ?1",
                [now - KEEP_UNSENT_MS],
            )?;
            c.execute(
                "INSERT INTO calendar_invitations (id, kind, calendar_id, calendar, event_uid,
                     event_start, ical_uid, occurrence, snapshot, recipients, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, now
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(id)
}

const COLUMNS: &str = "id, kind, calendar_id, calendar, event_uid, event_start, snapshot, \
                       recipients, sent_at, sent_from";

fn stored(r: &rusqlite::Row) -> rusqlite::Result<Option<Stored>> {
    let id: String = r.get(0)?;
    let kind: String = r.get(1)?;
    let snapshot: String = r.get(6)?;
    let recipients: String = r.get(7)?;
    let start: i64 = r.get(5)?;
    let parsed = (|| {
        Some(Stored {
            id: id.parse().ok()?,
            kind: serde_json::from_value(serde_json::Value::String(kind)).ok()?,
            calendar_id: r.get(2).ok()?,
            calendar: r.get(3).ok()?,
            event_uid: r.get(4).ok()?,
            start: Utc.timestamp_millis_opt(start).single()?,
            snapshot: serde_json::from_str(&snapshot).ok()?,
            recipients: serde_json::from_str(&recipients).ok()?,
            sent_at: r.get(8).ok()?,
            sent_from: r.get(9).ok()?,
        })
    })();
    Ok(parsed)
}

pub async fn load(state: &AppState, id: Uuid) -> Result<Option<Stored>, String> {
    state
        .db
        .call(move |c| {
            Ok(c.query_row(
                &format!("SELECT {COLUMNS} FROM calendar_invitations WHERE id = ?1"),
                [id.to_string()],
                stored,
            )
            .optional()?
            .flatten())
        })
        .await
        .map_err(|e| e.to_string())
}

/// Offers not sent yet for occurrences starting at `start`, newest first.
pub async fn unsent_at(state: &AppState, start: DateTime<Utc>) -> Vec<Stored> {
    let ms = start.timestamp_millis();
    state
        .db
        .call(move |c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLUMNS} FROM calendar_invitations
                 WHERE event_start = ?1 AND sent_at IS NULL ORDER BY created_at DESC"
            ))?;
            let rows = stmt.query_map([ms], stored)?;
            Ok(rows.filter_map(|r| r.ok().flatten()).collect())
        })
        .await
        .unwrap_or_default()
}

/// The account an invitation goes out from: the one whose address organizes the event,
/// else the first one, and why when it's not the organizer's.
fn sender<'a>(
    accounts: &'a [crate::mail::Account],
    organizer: Option<&str>,
) -> Option<(&'a crate::mail::Account, Option<String>)> {
    if let Some(o) = organizer
        && let Some(a) = accounts
            .iter()
            .find(|a| a.config.email.eq_ignore_ascii_case(o))
    {
        return Some((a, None));
    }
    let first = accounts.first()?;
    let note = organizer.map(|o| {
        format!(
            "From {}: none of your email accounts is the event's organizer ({o}).",
            first.config.email
        )
    });
    Some((first, note))
}

/// An offer as clients show it.
pub async fn view(state: &AppState, s: &Stored) -> InvitationOffer {
    let accounts = crate::mail::accounts(state).await;
    let (from, from_note) = match &s.sent_from {
        Some(f) => (Some(f.clone()), None),
        None => match sender(&accounts, s.snapshot.organizer.as_deref()) {
            Some((a, note)) => (Some(a.config.email.clone()), note),
            None => (None, None),
        },
    };
    InvitationOffer {
        id: s.id,
        kind: s.kind,
        event_title: s.snapshot.title.clone(),
        event_when: s.snapshot.when(),
        guests: s
            .recipients
            .iter()
            .map(|g| InvitationGuest {
                name: g.name.clone(),
                email: g.email.clone(),
            })
            .collect(),
        from,
        from_note,
        sent_at: s.sent_at,
    }
}

pub async fn views(state: &AppState, ids: &[Uuid]) -> Vec<InvitationOffer> {
    let mut out = Vec::new();
    for id in ids {
        if let Ok(Some(s)) = load(state, *id).await {
            out.push(view(state, &s).await);
        }
    }
    out
}

// --- Sending --------------------------------------------------------------------------

/// Sends an offer from the user's email account: to all its guests, or only to `only`
/// (which must be among them). Each offer goes out once. The caller has the user's say:
/// their click, or an approved (or allowed) request.
pub async fn send(
    state: &Arc<AppState>,
    id: Uuid,
    only: Option<&[String]>,
) -> Result<InvitationOffer, String> {
    let offer = load(state, id)
        .await?
        .ok_or("Those invitations aren't available anymore.")?;
    if offer.sent_at.is_some() {
        return Err("Those invitations were already sent.".to_owned());
    }
    let recipients = pick(&offer.recipients, only)?;
    let accounts = crate::mail::accounts(state).await;
    let (account, _) = sender(&accounts, offer.snapshot.organizer.as_deref()).ok_or(
        "No email account is connected. Connect one in Settings › Connections to send invitations.",
    )?;
    let from = account.config.email.to_lowercase();
    let ical_uid = offer.snapshot.ical_uid.clone();
    let occurrence = offer.snapshot.occurrence.map(|o| o.timestamp_millis());
    let now = crate::now_ms();
    // Claimed first, so a second click can't send it twice.
    let (claimed, last) = state
        .db
        .call(move |c| {
            let claimed = c.execute(
                "UPDATE calendar_invitations SET sent_at = ?2 WHERE id = ?1 AND sent_at IS NULL",
                (id.to_string(), now),
            )? == 1;
            let last: Option<i64> = c.query_row(
                "SELECT MAX(sent_sequence) FROM calendar_invitations
                 WHERE ical_uid = ?1 AND occurrence IS ?2 AND sent_sequence IS NOT NULL",
                (&ical_uid, occurrence),
                |r| r.get(0),
            )?;
            Ok((claimed, last))
        })
        .await
        .map_err(|e| e.to_string())?;
    if !claimed {
        return Err("Those invitations were already sent.".to_owned());
    }
    let sequence = next_sequence(offer.snapshot.sequence, last);
    let sent = async {
        let built = email(
            &from,
            &recipients,
            &offer.snapshot,
            offer.kind,
            sequence,
            Utc::now(),
        )?;
        crate::mail::smtp::send(&account.config, &built).await?;
        Ok::<_, String>(built)
    }
    .await;
    let built = match sent {
        Ok(b) => b,
        Err(e) => {
            let _ = state
                .db
                .call(move |c| {
                    c.execute(
                        "UPDATE calendar_invitations SET sent_at = NULL WHERE id = ?1",
                        [id.to_string()],
                    )
                })
                .await;
            return Err(e);
        }
    };
    tracing::info!(connection = %account.id, guests = recipients.len(), "sent invitations");
    let sent_to = serde_json::to_string(&recipients).map_err(|e| e.to_string())?;
    let from_saved = from.clone();
    state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE calendar_invitations SET sent_sequence = ?2, sent_from = ?3, recipients = ?4
                 WHERE id = ?1",
                (id.to_string(), i64::from(sequence), from_saved, sent_to),
            )
        })
        .await
        .map_err(|e| e.to_string())?;
    crate::mail::sync::file_sent(account, built.formatted).await;
    state.mail.poke(account.id);
    let stored = load(state, id).await?.ok_or("The invitations were sent.")?;
    Ok(view(state, &stored).await)
}

/// The calendar's version, but newer than anything already sent for the event.
pub fn next_sequence(calendar: u32, last_sent: Option<i64>) -> u32 {
    match last_sent {
        Some(last) => calendar.max(u32::try_from(last + 1).unwrap_or(u32::MAX)),
        None => calendar,
    }
}

/// The offer's guests, or the ones named in `only` (addresses or names), each of which
/// must be one of them.
pub fn pick(all: &[Guest], only: Option<&[String]>) -> Result<Vec<Guest>, String> {
    let Some(only) = only.filter(|o| !o.is_empty()) else {
        return Ok(all.to_vec());
    };
    let mut out: Vec<Guest> = Vec::new();
    for raw in only {
        let raw = raw.trim();
        let wanted = Guest::parse(raw).map(|g| g.email);
        let matches: Vec<&Guest> = all
            .iter()
            .filter(|g| match &wanted {
                Some(email) => &g.email == email,
                None => {
                    let folded = crate::people::fold(raw);
                    g.name.as_deref().is_some_and(|n| {
                        let n = crate::people::fold(n);
                        n == folded || n.split_whitespace().next() == Some(folded.as_str())
                    })
                }
            })
            .collect();
        match matches.as_slice() {
            [one] => {
                if !out.iter().any(|o| o.email == one.email) {
                    out.push((*one).clone());
                }
            }
            [] => return Err(format!("{raw} isn't one of the event's guests.")),
            _ => {
                return Err(format!(
                    "Several guests match “{raw}”. Use their email address."
                ));
            }
        }
    }
    Ok(out)
}

// --- The message ----------------------------------------------------------------------

/// Text from a calendar, safe in a one-line header or an iCalendar value.
fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn multi_line(s: &str) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// The event as iCalendar data for an iMIP message: `METHOD:REQUEST` with every guest
/// (those it's sent to asked to answer, the others as they answered), or `METHOD:CANCEL`
/// naming only those it's sent to.
pub fn calendar_data(
    s: &Snapshot,
    kind: InvitationKind,
    organizer: &str,
    recipients: &[Guest],
    sequence: u32,
    now: DateTime<Utc>,
) -> String {
    let cancel = cancels(kind);
    let mut cal = Calendar::empty();
    cal.append_property(Property::new("PRODID", "-//Mimi//Mimi//EN"))
        .append_property(Property::new("VERSION", "2.0"))
        .append_property(Property::new("CALSCALE", "GREGORIAN"))
        .append_property(Property::new(
            "METHOD",
            if cancel { "CANCEL" } else { "REQUEST" },
        ));
    let mut e = icalendar::Event::new();
    e.uid(&s.ical_uid).sequence(sequence).timestamp(now);
    if s.all_day {
        e.starts(s.start.with_timezone(&Local).date_naive());
        e.ends(s.end.with_timezone(&Local).date_naive());
    } else {
        e.starts(s.start).ends(s.end);
    }
    if let Some(o) = s.occurrence {
        if s.all_day {
            e.recurrence_id(o.with_timezone(&Local).date_naive());
        } else {
            e.recurrence_id(o);
        }
    }
    e.summary(&one_line(&s.title));
    if let Some(l) = &s.location {
        e.location(&one_line(l));
    }
    if let Some(n) = &s.notes {
        e.description(&multi_line(n));
    }
    e.add_property("STATUS", if cancel { "CANCELLED" } else { "CONFIRMED" });
    e.append_property(Property::new("ORGANIZER", format!("mailto:{organizer}")));
    let listed: Vec<&Guest> = if cancel {
        recipients.iter().collect()
    } else {
        s.guests
            .iter()
            .chain(
                recipients
                    .iter()
                    .filter(|r| !s.guests.iter().any(|g| g.email == r.email)),
            )
            .collect()
    };
    for g in listed {
        let asked = recipients.iter().any(|r| r.email == g.email);
        let partstat = match (asked, g.response) {
            (false, Some(GuestResponse::Accepted)) => "ACCEPTED",
            (false, Some(GuestResponse::Declined)) => "DECLINED",
            (false, Some(GuestResponse::Tentative)) => "TENTATIVE",
            _ => "NEEDS-ACTION",
        };
        let mut p = Property::new("ATTENDEE", format!("mailto:{}", g.email));
        if let Some(name) = &g.name {
            p.add_parameter("CN", &format!("\"{}\"", one_line(name).replace('"', "")));
        }
        p.add_parameter("ROLE", "REQ-PARTICIPANT")
            .add_parameter("PARTSTAT", partstat);
        if asked && !cancel {
            p.add_parameter("RSVP", "TRUE");
        }
        e.append_multi_property(p.done());
    }
    cal.push(e.done());
    cal.to_string()
}

/// Whether the message takes the event away from its recipients (`METHOD:CANCEL`).
fn cancels(kind: InvitationKind) -> bool {
    matches!(kind, InvitationKind::Cancel | InvitationKind::Uninvite)
}

/// "Invitation: Dinner (Friday 3 Oct, 19:00–21:00)", and the plain-text body.
fn words(s: &Snapshot, kind: InvitationKind) -> (String, String) {
    let title = one_line(&s.title);
    let short: String = title.chars().take(100).collect();
    let when = s.when();
    let when_plain = when.trim_start_matches("on ").to_owned();
    let (subject, intro) = match kind {
        InvitationKind::Invite => (
            format!("Invitation: {short} ({when_plain})"),
            format!("You're invited to “{title}”."),
        ),
        InvitationKind::Update => (
            format!("Updated: {short} ({when_plain})"),
            format!("“{title}” has changed. Here are the new details."),
        ),
        InvitationKind::Uninvite => (
            format!("No longer invited: {short} ({when_plain})"),
            format!("You're no longer invited to “{title}”."),
        ),
        InvitationKind::Cancel => (
            format!("Cancelled: {short} ({when_plain})"),
            format!("“{title}” is cancelled."),
        ),
    };
    let zone = jiff::tz::TimeZone::system()
        .iana_name()
        .map(|z| format!(" ({z})"))
        .unwrap_or_default();
    let mut body = format!("{intro}\n\nWhen: {}{zone}\n", upper_first(&when_plain));
    if let Some(l) = &s.location {
        body.push_str(&format!("Where: {}\n", one_line(l)));
    }
    if !cancels(kind) && !s.guests.is_empty() {
        let names: Vec<String> = s.guests.iter().map(|g| one_line(g.label())).collect();
        body.push_str(&format!("Guests: {}\n", names.join(", ")));
    }
    if !cancels(kind)
        && let Some(n) = &s.notes
    {
        body.push_str(&format!("\n{}\n", multi_line(n)));
    }
    (subject, body)
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The whole email: plain text and the event (inline, and as `invite.ics`), from the
/// user's address to every recipient.
pub fn email(
    from: &str,
    recipients: &[Guest],
    s: &Snapshot,
    kind: InvitationKind,
    sequence: u32,
    now: DateTime<Utc>,
) -> Result<crate::mail::smtp::Built, String> {
    if recipients.is_empty() {
        return Err("Nobody to send the invitations to.".to_owned());
    }
    if recipients.len() > crate::mail::smtp::MAX_RECIPIENTS {
        return Err(format!(
            "That's more than {} guests for one email.",
            crate::mail::smtp::MAX_RECIPIENTS
        ));
    }
    let data = calendar_data(s, kind, from, recipients, sequence, now);
    let (subject, body) = words(s, kind);
    let method = if cancels(kind) { "CANCEL" } else { "REQUEST" };
    let domain = from.rsplit('@').next().unwrap_or("localhost").to_owned();
    let mailbox = |raw: &str| {
        raw.parse::<Mailbox>()
            .map_err(|_| format!("“{raw}” isn't an email address."))
    };
    let mut builder = Message::builder()
        .from(mailbox(from)?)
        .subject(subject)
        .date_now()
        .message_id(Some(format!("<{}@{domain}>", Uuid::now_v7().simple())));
    for r in recipients {
        let address = r
            .email
            .parse()
            .map_err(|_| format!("“{}” isn't an email address.", r.email))?;
        builder = builder.to(Mailbox::new(r.name.clone(), address));
    }
    let inline_type = ContentType::parse(&format!("text/calendar; charset=utf-8; method={method}"))
        .map_err(|e| e.to_string())?;
    let file_type =
        ContentType::parse("application/ics; name=\"invite.ics\"").map_err(|e| e.to_string())?;
    let parts = MultiPart::mixed()
        .multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(body))
                .singlepart(
                    SinglePart::builder()
                        .header(inline_type)
                        .header(ContentTransferEncoding::Base64)
                        .body(data.clone()),
                ),
        )
        .singlepart(Attachment::new("invite.ics".to_owned()).body(data, file_type));
    let message = builder.multipart(parts).map_err(|e| e.to_string())?;
    let formatted = message.formatted();
    Ok(crate::mail::smtp::Built { message, formatted })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn guest(name: &str, email: &str) -> Guest {
        Guest {
            name: Some(name.into()),
            email: email.into(),
            response: None,
        }
    }

    fn dinner() -> Snapshot {
        Snapshot {
            ical_uid: "dinner@mimi".into(),
            title: "Dinner".into(),
            start: Utc.with_ymd_and_hms(2026, 10, 2, 17, 0, 0).unwrap(),
            end: Utc.with_ymd_and_hms(2026, 10, 2, 19, 0, 0).unwrap(),
            all_day: false,
            location: Some("Café du Lac".into()),
            notes: None,
            occurrence: None,
            sequence: 0,
            guests: vec![
                guest("Sam", "sam@example.com"),
                guest("Léa", "lea@example.com"),
            ],
            organizer: Some("me@example.org".into()),
        }
    }

    #[test]
    fn a_change_offers_the_right_news_to_the_right_guests() {
        let before = dinner();
        // Only a guest added: only they get an invitation.
        let mut after = before.clone();
        after.guests.push(guest("Tom", "tom@example.com"));
        let plan = plan_after_change(&before, &after);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].0, InvitationKind::Invite);
        assert_eq!(plan[0].1[0].email, "tom@example.com");
        // A new time: those who stay get the new details; one removed is told.
        let mut moved = before.clone();
        moved.start += chrono::Duration::hours(1);
        moved.end += chrono::Duration::hours(1);
        moved.guests.retain(|g| g.email != "lea@example.com");
        let plan = plan_after_change(&before, &moved);
        let kinds: Vec<(InvitationKind, Vec<&str>)> = plan
            .iter()
            .map(|(k, g)| (*k, g.iter().map(|g| g.email.as_str()).collect()))
            .collect();
        assert_eq!(
            kinds,
            [
                (InvitationKind::Update, vec!["sam@example.com"]),
                (InvitationKind::Uninvite, vec!["lea@example.com"])
            ]
        );
        // Nothing they'd see changed: nothing to offer.
        assert!(plan_after_change(&before, &before.clone()).is_empty());
    }

    #[test]
    fn invitations_go_from_the_organizers_account_else_the_first_one_saying_so() {
        let account = |email: &str| crate::mail::Account {
            id: Uuid::now_v7(),
            name: email.into(),
            config: crate::mail::EmailConfig {
                email: email.into(),
                password: String::new(),
                preset: "other".into(),
                servers: mimi_protocol::MailServers {
                    imap_host: "127.0.0.1".into(),
                    imap_port: 1,
                    imap_security: mimi_protocol::MailSecurity::Plain,
                    smtp_host: "127.0.0.1".into(),
                    smtp_port: 1,
                    smtp_security: mimi_protocol::MailSecurity::Plain,
                    username: None,
                },
            },
        };
        let accounts = [account("home@example.org"), account("Work@example.com")];
        let (a, note) = sender(&accounts, Some("work@example.com")).unwrap();
        assert_eq!((a.config.email.as_str(), note), ("Work@example.com", None));
        let (a, note) = sender(&accounts, Some("family@group.calendar.google.com")).unwrap();
        assert_eq!(a.config.email, "home@example.org");
        assert!(note.unwrap().contains("family@group.calendar.google.com"));
        assert!(sender(&[], Some("work@example.com")).is_none());
    }

    #[test]
    fn sequences_only_go_up() {
        assert_eq!(next_sequence(0, None), 0);
        assert_eq!(next_sequence(0, Some(0)), 1);
        assert_eq!(next_sequence(3, Some(0)), 3);
        assert_eq!(next_sequence(1, Some(4)), 5);
    }

    #[test]
    fn guests_are_picked_by_address_or_name_and_only_among_the_guests() {
        let all = dinner().guests;
        assert_eq!(pick(&all, None).unwrap().len(), 2);
        let only = pick(&all, Some(&["lea".into()])).unwrap();
        assert_eq!(only[0].email, "lea@example.com");
        assert!(pick(&all, Some(&["mallory@example.net".into()])).is_err());
        assert!(pick(&all, Some(&["Mallory".into()])).is_err());
    }

    fn part_of(raw: &str, content_type: &str) -> String {
        use mail_parser::MimeHeaders;
        let parsed = mail_parser::MessageParser::default()
            .parse(raw.as_bytes())
            .unwrap();
        parsed
            .parts
            .iter()
            .find(|p| {
                p.content_type().is_some_and(|c| {
                    format!("{}/{}", c.ctype(), c.subtype().unwrap_or_default()) == content_type
                })
            })
            .map(|p| String::from_utf8_lossy(p.contents()).into_owned())
            .unwrap_or_default()
    }

    #[test]
    fn invitations_are_standard_imip_messages() {
        let s = dinner();
        let now = Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap();
        let built = email(
            "me@example.org",
            &s.guests,
            &s,
            InvitationKind::Invite,
            0,
            now,
        )
        .unwrap();
        let raw = String::from_utf8(built.formatted).unwrap();
        assert!(raw.contains("Subject: Invitation: Dinner"), "{raw}");
        assert!(raw.contains("method=REQUEST"), "{raw}");
        assert!(raw.contains("invite.ics"), "{raw}");
        let folded = part_of(&raw, "text/calendar");
        assert_eq!(
            folded,
            part_of(&raw, "application/ics"),
            "the file is the same event"
        );
        let ics = folded.replace("\r\n ", "");
        assert!(ics.contains("METHOD:REQUEST"), "{ics}");
        assert!(ics.contains("UID:dinner@mimi"));
        assert!(ics.contains("SEQUENCE:0"));
        assert!(ics.contains("ORGANIZER:mailto:me@example.org"));
        assert!(ics.contains("DTSTART:20261002T170000Z"));
        assert!(ics.contains("mailto:sam@example.com"));
        assert!(ics.contains("RSVP=TRUE"));
        let text = part_of(&raw, "text/plain");
        assert!(text.contains("You're invited to “Dinner”."), "{text}");
        assert!(text.contains("Where: Café du Lac"));
        assert!(text.contains("Guests: Sam, Léa"));

        // An update is a newer version; a cancellation names only those told, and one
        // occurrence of a series says which.
        let mut moved = s.clone();
        moved.occurrence = Some(s.start);
        let update = calendar_data(
            &moved,
            InvitationKind::Update,
            "me@example.org",
            &s.guests[..1],
            3,
            now,
        );
        assert!(update.contains("SEQUENCE:3") && update.contains("METHOD:REQUEST"));
        assert!(
            update.contains("RECURRENCE-ID:20261002T170000Z"),
            "{update}"
        );
        let cancel = calendar_data(
            &s,
            InvitationKind::Cancel,
            "me@example.org",
            &s.guests[1..],
            1,
            now,
        )
        .replace("\r\n ", "");
        assert!(cancel.contains("METHOD:CANCEL") && cancel.contains("STATUS:CANCELLED"));
        assert!(cancel.contains("mailto:lea@example.com"));
        assert!(!cancel.contains("mailto:sam@example.com"), "{cancel}");
        let (subject, body) = words(&s, InvitationKind::Cancel);
        assert!(subject.starts_with("Cancelled: Dinner"));
        assert!(body.contains("“Dinner” is cancelled."));
    }

    #[test]
    fn calendar_text_cannot_break_the_message() {
        let mut s = dinner();
        s.title = "Dinner\r\nBcc: mallory@example.net".into();
        s.notes = Some("Line one\nBEGIN:VEVENT\r\nEND:VCALENDAR".into());
        let now = Utc::now();
        let built = email(
            "me@example.org",
            &s.guests,
            &s,
            InvitationKind::Invite,
            0,
            now,
        )
        .unwrap();
        let raw = String::from_utf8(built.formatted).unwrap();
        assert!(!raw.to_lowercase().contains("\nbcc:"), "{raw}");
        let ics = part_of(&raw, "text/calendar");
        let parsed: Calendar = ics.parse().unwrap();
        assert_eq!(parsed.components.len(), 1, "{ics}");
        let e = parsed.components[0].as_event().unwrap();
        assert_eq!(e.get_summary(), Some("Dinner  Bcc: mallory@example.net"));
    }
}
