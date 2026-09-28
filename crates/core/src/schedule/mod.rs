//! Reminders and routines: Mimi telling the user something, or doing something for
//! them, at the right time, even with the app's window closed.
//!
//! Items store their schedule as a rule ([`rules`]); `next_at` is derived and recomputed
//! whenever the rule, the clock or the time zone changes. One loop ([`run`]) sleeps until
//! the next item is due (at most a minute at a time, so suspends and clock changes are
//! noticed), then [`tick`]s: due reminders are delivered (Telegram with Done / Snooze
//! buttons, a desktop notification, the app), due routines run as a chat turn in their
//! own conversation and their answer is delivered the same way. Occurrences missed while
//! the computer was off are sent once, marked late, if recent, and otherwise only
//! recorded as missed: never a flood.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use mimi_protocol::{
    ActionStatus, Delivery, DeliveryStatus, Event, MessageStatus, NewScheduleItem, Schedule,
    ScheduleItem, ScheduleKind, ScheduleOccurrence, ScheduleUpdate,
};
use tokio::sync::Notify;
use uuid::Uuid;

use self::rules::Fire;
use self::store::Item;
use crate::api::error::AppError;
use crate::connections::telegram;
use crate::{AppState, now_ms};

pub mod notify;
pub mod rules;
pub mod store;
pub mod tools;

/// How often an event-relative reminder checks whether its event moved.
const EVENT_REFRESH: Duration = Duration::from_secs(15 * 60);
/// Longest a routine may take (including waiting for an approval) before it's reported
/// as not finished.
const ROUTINE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The scheduler's live state: its wake-up signal and a few caches.
#[derive(Default)]
pub struct Scheduler {
    wake: Notify,
    /// When each event-relative item last looked its event up.
    event_checked: Mutex<HashMap<Uuid, Instant>>,
    /// The time zone `next_at` values were computed in.
    zone: Mutex<Option<String>>,
}

impl Scheduler {
    /// Re-plans now, e.g. after an item was added or changed.
    pub fn poke(&self) {
        self.wake.notify_one();
    }
}

pub fn install(state: &Arc<AppState>) {
    state.tool_sources.add(Arc::new(tools::ScheduleTools));
    tokio::spawn(run(state.clone()));
}

fn zone() -> TimeZone {
    TimeZone::system()
}

fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// Sleeps until something is due, then handles it; forever.
pub async fn run(state: Arc<AppState>) {
    if let Ok(n) = store::fail_unfinished(&state.db).await
        && n > 0
    {
        tracing::warn!(n, "routine runs were cut off by the last shutdown");
    }
    loop {
        let now = Timestamp::now();
        tick(&state, now).await;
        let wait = match store::next_wake(&state.db).await {
            Ok(Some(at)) => {
                Duration::from_millis((at - now.as_millisecond()).clamp(0, 60_000) as u64)
            }
            _ => Duration::from_secs(60),
        };
        // Capped at a minute: a monotonic sleep doesn't advance while the computer
        // sleeps, and the wall clock or time zone may change under us.
        tokio::select! {
            _ = tokio::time::sleep(wait.max(Duration::from_millis(50))) => {}
            _ = state.scheduler.wake.notified() => {}
        }
    }
}

/// Handles everything due at `now`. Takes the clock as an argument so tests can move it.
pub(crate) async fn tick(state: &Arc<AppState>, now: Timestamp) {
    let Ok(items) = store::list(&state.db).await else {
        return;
    };
    let now_ms = now.as_millisecond();
    let tz = zone();

    // The computer moved to another time zone: wall-clock rules now mean other instants.
    let zone_name = tz.iana_name().map(str::to_owned);
    let zone_changed = {
        let mut seen = state
            .scheduler
            .zone
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let changed = seen.is_some() && *seen != zone_name;
        *seen = zone_name;
        changed
    };

    let mut changed = false;
    for mut item in items {
        if item.paused || item.ended.is_some() {
            continue;
        }
        let before = item.clone();

        if zone_changed && item.next_at.is_some_and(|t| t > now_ms) {
            arm(state, &mut item, now, &tz).await;
        }
        // Event-relative items follow their event as it moves.
        if matches!(item.schedule, Schedule::BeforeEvent { .. })
            && item.next_at.is_some_and(|t| t > now_ms)
            && event_check_due(state, item.id)
        {
            arm(state, &mut item, now, &tz).await;
        }

        // A snoozed reminder comes back.
        if let Some(snoozed) = item.snoozed_until
            && snoozed <= now_ms
        {
            item.snoozed_until = None;
            match rules::classify(snoozed, now_ms) {
                Fire::Missed => record_missed(state, &item, snoozed, now_ms).await,
                fire => deliver_reminder(state, &item, snoozed, now_ms, fire),
            }
            item.last_at = Some(now_ms);
        }

        if let Some(due) = item.next_at
            && due <= now_ms
        {
            let mut due = due;
            if matches!(item.schedule, Schedule::BeforeEvent { .. }) {
                // Check the event right before firing: it may have moved or gone.
                arm(state, &mut item, ts(now_ms - rules::MISSED_AFTER_MS), &tz).await;
                match item.next_at {
                    Some(new) if new > now_ms + 30_000 => {
                        // It moved later: wait for the new time.
                        item.updated_at = now_ms;
                        changed = true;
                        let _ = store::upsert(&state.db, item).await;
                        continue;
                    }
                    Some(new) => due = new,
                    None => {}
                }
            }
            if item.ended.is_none() {
                let fire = rules::classify(due, now_ms);
                match (fire, item.kind) {
                    (Fire::Missed, _) => record_missed(state, &item, due, now_ms).await,
                    (_, ScheduleKind::Reminder) => {
                        deliver_reminder(state, &item, due, now_ms, fire)
                    }
                    (_, ScheduleKind::Routine) => {
                        if item.conversation_id.is_none() {
                            item.conversation_id = routine_conversation(state, &item).await;
                        }
                        start_routine(state, &item, due, now_ms, fire);
                    }
                }
                item.last_at = Some(now_ms);
            }
            // Next occurrence after both the due time and now: missed ones are skipped,
            // never replayed one by one.
            item.next_at = rules::next_after(
                &item.schedule,
                ts(due.max(now_ms)),
                ts(item.anchor_at),
                &tz,
                item.event_start.map(ts),
            )
            .map(|t| t.as_millisecond());
        }

        if item != before {
            item.updated_at = now_ms;
            changed = true;
            if let Err(e) = store::upsert(&state.db, item).await {
                tracing::error!("saving a reminder failed: {e}");
            }
        }
    }
    if changed {
        state.events.publish(Event::ScheduleChanged);
    }
}

fn event_check_due(state: &AppState, id: Uuid) -> bool {
    let mut checked = state
        .scheduler
        .event_checked
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match checked.get(&id) {
        Some(at) if at.elapsed() < EVENT_REFRESH => false,
        _ => {
            checked.insert(id, Instant::now());
            true
        }
    }
}

/// Recomputes `next_at` (looking the event up for event-relative items).
async fn arm(state: &AppState, item: &mut Item, after: Timestamp, tz: &TimeZone) {
    if let Schedule::BeforeEvent { event_id, .. } = &item.schedule {
        match find_event(state, event_id, item.event_start).await {
            EventLookup::Found { start } => item.event_start = Some(start),
            EventLookup::Gone => {
                item.ended = Some("The event was cancelled or removed.".to_owned());
                item.next_at = None;
                return;
            }
            // Offline or the calendar is disconnected: keep the last known time.
            EventLookup::Unknown => {}
        }
    }
    item.next_at = rules::next_after(
        &item.schedule,
        after,
        ts(item.anchor_at),
        tz,
        item.event_start.map(ts),
    )
    .map(|t| t.as_millisecond());
}

enum EventLookup {
    Found { start: i64 },
    Gone,
    Unknown,
}

/// Finds the event an event-relative item follows, even if it moved: same calendar and
/// uid, the occurrence closest to where it last was.
async fn find_event(state: &AppState, event_id: &str, last_known: Option<i64>) -> EventLookup {
    let Some((original, calendar, uid)) = crate::people::mentions::parse_event_id(event_id) else {
        return EventLookup::Gone;
    };
    let accounts = crate::connections::calendar_accounts(state).await;
    if accounts.is_empty() {
        return EventLookup::Unknown;
    }
    let center = last_known
        .and_then(chrono::DateTime::from_timestamp_millis)
        .unwrap_or(original);
    let (events, problems) = crate::connections::calendar::events_between(
        &state.http,
        &state.connections.feeds,
        &accounts,
        center - chrono::Duration::days(60),
        center + chrono::Duration::days(60),
    )
    .await;
    let found = events
        .iter()
        .filter(|e| e.uid == uid && e.calendar == calendar)
        .min_by_key(|e| (e.start - center).num_seconds().abs());
    match found {
        Some(e) => EventLookup::Found {
            start: e.start.timestamp_millis(),
        },
        None if problems.is_empty() => EventLookup::Gone,
        None => EventLookup::Unknown,
    }
}

// --- Delivery --------------------------------------------------------------------------

fn local_time(ms: i64) -> String {
    ts(ms).to_zoned(zone()).strftime("%-H:%M").to_string()
}

async fn record_missed(state: &AppState, item: &Item, due: i64, now: i64) {
    let delivery = Delivery {
        id: Uuid::now_v7(),
        item_id: item.id,
        kind: item.kind,
        title: item.title.clone(),
        due_at: due,
        at: now,
        status: DeliveryStatus::Missed,
        detail: Some("Mimi wasn't running at the time.".to_owned()),
        conversation_id: None,
    };
    tracing::info!(item = %item.id, "missed while not running");
    let _ = store::record(&state.db, delivery).await;
}

/// Sends a reminder everywhere it should go. Returns immediately; sending happens in
/// the background so one slow channel can't hold up the others or the scheduler.
fn deliver_reminder(state: &Arc<AppState>, item: &Item, due: i64, now: i64, fire: Fire) {
    let delivery = Delivery {
        id: Uuid::now_v7(),
        item_id: item.id,
        kind: ScheduleKind::Reminder,
        title: item.title.clone(),
        due_at: due,
        at: now,
        status: if fire == Fire::Late {
            DeliveryStatus::Late
        } else {
            DeliveryStatus::Delivered
        },
        detail: None,
        conversation_id: None,
    };
    let state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = store::record(&state.db, delivery.clone()).await {
            tracing::error!("recording a reminder failed: {e}");
        }
        state.events.publish(Event::ScheduleDelivered {
            delivery: delivery.clone(),
        });
        state.events.publish(Event::ScheduleChanged);
        let late = (delivery.status == DeliveryStatus::Late)
            .then(|| format!("It was due at {}.", local_time(delivery.due_at)));
        let mut channels = vec!["app"];

        if let Some((bot, chat)) = telegram::owner(&state).await {
            let html = format!(
                "⏰ <b>{}</b>{}",
                telegram::escape(&delivery.title),
                late.as_ref()
                    .map(|l| format!("\n<i>{l}</i>"))
                    .unwrap_or_default()
            );
            let id = delivery.id;
            let buttons = [
                ("Done", format!("done:{id}")),
                ("Snooze 10 min", format!("snooze:{id}")),
                ("1 hour", format!("snooze60:{id}")),
            ];
            match bot.send_with_buttons(chat, &html, &buttons).await {
                Ok(()) => channels.push("telegram"),
                Err(e) => tracing::warn!("sending a reminder to Telegram failed: {e}"),
            }
        }
        if desktop_notifications(&state).await {
            channels.push("desktop");
            notify::show(
                "Reminder".to_owned(),
                match &late {
                    Some(l) => format!("{}\n{l}", delivery.title),
                    None => delivery.title.clone(),
                },
            )
            .await;
        }
        tracing::info!(item = %delivery.item_id, ?channels, late = late.is_some(), "reminder delivered");
    });
}

async fn desktop_notifications(state: &AppState) -> bool {
    crate::settings::load(&state.db)
        .await
        .map(|s| s.desktop_notifications)
        .unwrap_or(true)
}

async fn routine_conversation(state: &AppState, item: &Item) -> Option<Uuid> {
    let conversation = crate::chat::new_conversation(Some(item.title.clone()));
    crate::chat::store::upsert_conversation(&state.db, conversation.clone())
        .await
        .ok()?;
    state.events.publish(Event::ConversationUpdated {
        conversation: conversation.clone(),
    });
    Some(conversation.id)
}

fn start_routine(state: &Arc<AppState>, item: &Item, due: i64, now: i64, fire: Fire) {
    let (state, item) = (state.clone(), item.clone());
    tokio::spawn(async move { run_routine(&state, item, due, now, fire).await });
}

/// Runs a routine as a chat turn in its conversation, relays approvals it asks for, and
/// delivers its answer.
async fn run_routine(state: &Arc<AppState>, item: Item, due: i64, now: i64, fire: Fire) {
    let Some(conversation) = item.conversation_id else {
        return;
    };
    let mut delivery = Delivery {
        id: Uuid::now_v7(),
        item_id: item.id,
        kind: ScheduleKind::Routine,
        title: item.title.clone(),
        due_at: due,
        at: now,
        status: DeliveryStatus::Running,
        detail: None,
        conversation_id: Some(conversation),
    };
    let _ = store::record(&state.db, delivery.clone()).await;
    state.events.publish(Event::ScheduleChanged);

    let finish = |mut delivery: Delivery, status: DeliveryStatus, detail: Option<String>| async move {
        delivery.status = status;
        delivery.detail = detail;
        let _ = store::record(&state.db, delivery.clone()).await;
        state.events.publish(Event::ScheduleDelivered { delivery });
        state.events.publish(Event::ScheduleChanged);
    };

    let mut events = state.events.subscribe();
    let instruction = item
        .instruction
        .clone()
        .unwrap_or_else(|| item.title.clone());
    let context = format!(
        "<routine>\nThis message was sent by a routine the user scheduled (\"{}\", {}), \
         automatically, at {}. They may not be at their computer: do it now and reply \
         with the result, written to be read later (for example on their phone). Don't \
         ask questions back unless you can't do it without an answer.\n</routine>",
        item.title,
        rules::describe(&item.schedule).to_lowercase(),
        local_time(now)
    );
    let sent = crate::chat::send_with_context(
        state.clone(),
        conversation,
        instruction,
        None,
        Vec::new(),
        Some(context),
    )
    .await;
    let sent = match sent {
        Ok(sent) => sent,
        Err(e) => {
            let busy = e.message().contains("still replying");
            let status = if busy {
                DeliveryStatus::Skipped
            } else {
                DeliveryStatus::Failed
            };
            let detail = if busy {
                "The previous run was still going.".to_owned()
            } else {
                e.message().to_owned()
            };
            tracing::warn!(item = %item.id, "routine didn't run: {detail}");
            finish(delivery, status, Some(detail)).await;
            return;
        }
    };
    let assistant = sent.assistant_message.id;
    let owner = telegram::owner(state).await;
    let mut announced = std::collections::HashSet::new();
    let deadline = tokio::time::sleep(ROUTINE_TIMEOUT);
    tokio::pin!(deadline);
    let message = loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(Event::MessageUpdated { message }) if message.id == assistant => {
                    // Approvals go where the user is: Telegram buttons and a desktop nudge.
                    for action in &message.actions {
                        if action.status == ActionStatus::PendingApproval && announced.insert(action.id) {
                            if let Some((bot, chat)) = &owner {
                                let _ = bot.ask_approval(*chat, action).await;
                            }
                            if desktop_notifications(state).await {
                                notify::show(format!("{} needs your OK", item.title), action.summary.clone()).await;
                            }
                        }
                    }
                    if message.status != MessageStatus::Streaming {
                        break message;
                    }
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return,
            },
            _ = &mut deadline => {
                finish(delivery, DeliveryStatus::Failed, Some("It took too long; it may still be waiting for your OK.".to_owned())).await;
                return;
            }
        }
    };

    match message.status {
        MessageStatus::Error => {
            let detail = message
                .error
                .clone()
                .unwrap_or_else(|| "Something went wrong.".to_owned());
            finish(delivery, DeliveryStatus::Failed, Some(detail)).await;
            return;
        }
        MessageStatus::Cancelled | MessageStatus::Interrupted => {
            finish(
                delivery,
                DeliveryStatus::Failed,
                Some("It was stopped.".to_owned()),
            )
            .await;
            return;
        }
        _ => {}
    }

    let content = message.content.trim();
    if let Some((bot, chat)) = &owner {
        let mut html = format!(
            "<b>{}</b>\n\n{}",
            telegram::escape(&item.title),
            if content.is_empty() {
                "(No answer.)".to_owned()
            } else {
                telegram::markdown_to_html(content)
            }
        );
        for action in &message.actions {
            if let Some(url) = action.output.as_ref().and_then(|o| o["open_url"].as_str()) {
                html.push_str(&format!(
                    "\n\n<a href=\"{}\">Open in Google Calendar to save it</a>",
                    telegram::escape(url).replace('"', "&quot;")
                ));
            }
        }
        if let Err(e) = bot.send(*chat, &html).await {
            tracing::warn!("sending a routine's result to Telegram failed: {e}");
        }
    }
    if desktop_notifications(state).await {
        notify::show(item.title.clone(), plain_preview(content, 180)).await;
    }
    delivery.at = crate::now_ms();
    let status = if fire == Fire::Late {
        DeliveryStatus::Late
    } else {
        DeliveryStatus::Delivered
    };
    tracing::info!(item = %item.id, "routine delivered");
    finish(delivery, status, None).await;
}

/// The start of a Markdown answer as plain text, for a notification.
fn plain_preview(md: &str, max: usize) -> String {
    let text: String = md
        .lines()
        .map(|l| {
            l.trim_start_matches(['#', '-', '*', '>', ' '])
                .replace("**", "")
                .replace('`', "")
        })
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > max {
        text.chars().take(max).collect::<String>() + "…"
    } else {
        text
    }
}

// --- Done and snooze -------------------------------------------------------------------

/// Marks a delivered reminder done. False if there's nothing to mark.
pub async fn mark_done(state: &AppState, delivery_id: Uuid) -> bool {
    let Ok(Some(d)) = store::delivery_by_id(&state.db, delivery_id).await else {
        return false;
    };
    if !matches!(
        d.status,
        DeliveryStatus::Delivered | DeliveryStatus::Late | DeliveryStatus::Snoozed
    ) {
        return false;
    }
    // Done also cancels a pending snooze of that reminder.
    if d.status == DeliveryStatus::Snoozed
        && let Ok(Some(mut item)) = store::get(&state.db, d.item_id).await
    {
        item.snoozed_until = None;
        let _ = store::upsert(&state.db, item).await;
    }
    let ok = store::set_status(&state.db, delivery_id, DeliveryStatus::Done, None)
        .await
        .is_ok();
    state.events.publish(Event::ScheduleChanged);
    ok
}

/// Brings a delivered reminder back in `minutes`. False if it can't be snoozed.
pub async fn snooze(state: &AppState, delivery_id: Uuid, minutes: u32) -> bool {
    let Ok(Some(d)) = store::delivery_by_id(&state.db, delivery_id).await else {
        return false;
    };
    if d.kind != ScheduleKind::Reminder
        || !matches!(d.status, DeliveryStatus::Delivered | DeliveryStatus::Late)
    {
        return false;
    }
    let Ok(Some(mut item)) = store::get(&state.db, d.item_id).await else {
        return false;
    };
    let now = now_ms();
    item.snoozed_until = Some(now + minutes.clamp(1, 24 * 60) as i64 * 60_000);
    item.updated_at = now;
    if store::upsert(&state.db, item).await.is_err() {
        return false;
    }
    let _ = store::set_status(&state.db, delivery_id, DeliveryStatus::Snoozed, None).await;
    state.events.publish(Event::ScheduleChanged);
    state.scheduler.poke();
    true
}

// --- Managing items --------------------------------------------------------------------

pub fn to_public(item: &Item) -> ScheduleItem {
    ScheduleItem {
        id: item.id,
        kind: item.kind,
        title: item.title.clone(),
        instruction: item.instruction.clone(),
        schedule: item.schedule.clone(),
        description: rules::describe(&item.schedule),
        paused: item.paused,
        next_at: item.wake_at(),
        last_at: item.last_at,
        ended: item.ended.clone(),
        conversation_id: item.conversation_id,
        created_at: item.created_at,
    }
}

/// Most entries one item adds to a calendar range (items that go off every few minutes
/// are already folded to one entry a day).
const MAX_OCCURRENCES_PER_ITEM: usize = 500;
/// Most past deliveries read for one calendar range.
const MAX_HISTORY: usize = 20_000;

fn occurrence(item_id: Uuid, at: i64, status: Option<DeliveryStatus>) -> ScheduleOccurrence {
    ScheduleOccurrence {
        item_id,
        at,
        until: at,
        count: 1,
        status,
        snoozed: false,
    }
}

/// Every time a reminder or routine goes off in `[from, to)`, for a calendar: what
/// already happened, from the history, and what's still to come, from each rule starting
/// at its next time. Paused and ended items have nothing to come; event-relative ones
/// come once, before their event.
pub async fn occurrences(
    state: &AppState,
    from: i64,
    to: i64,
) -> Result<Vec<ScheduleOccurrence>, AppError> {
    let tz = zone();
    let items = store::list(&state.db).await?;
    let history = store::due_between(&state.db, from, to, MAX_HISTORY).await?;
    let mut out = Vec::new();
    for item in &items {
        let folds = rules::folds(&item.schedule);
        let same_day =
            |a: i64, b: i64| rules::local_date(ts(a), &tz) == rules::local_date(ts(b), &tz);
        let mut mine: Vec<ScheduleOccurrence> = Vec::new();
        for d in history.iter().filter(|d| d.item_id == item.id) {
            match mine.last_mut() {
                Some(o) if folds && same_day(o.at, d.due_at) => {
                    o.until = d.due_at;
                    o.count += 1;
                    o.status = Some(d.status);
                }
                _ => mine.push(occurrence(item.id, d.due_at, Some(d.status))),
            }
        }
        let past = mine.len();
        if !item.paused && item.ended.is_none() {
            if let Some(next) = item.next_at
                && next < to
            {
                let spans = rules::occurrences(
                    &item.schedule,
                    ts(next.max(from)),
                    ts(to),
                    ts(item.anchor_at),
                    &tz,
                    item.event_start.map(ts),
                    MAX_OCCURRENCES_PER_ITEM,
                );
                for s in spans {
                    let at = s.first.as_millisecond();
                    // Already in the history (it's going off right now).
                    if !folds && mine[..past].iter().any(|o| o.at == at) {
                        continue;
                    }
                    mine.push(ScheduleOccurrence {
                        until: s.last.as_millisecond(),
                        count: s.count,
                        ..occurrence(item.id, at, None)
                    });
                }
            }
            if let Some(back) = item.snoozed_until
                && (from..to).contains(&back)
            {
                mine.push(ScheduleOccurrence {
                    snoozed: true,
                    ..occurrence(item.id, back, None)
                });
            }
        }
        mine.truncate(MAX_OCCURRENCES_PER_ITEM);
        out.extend(mine);
    }
    out.sort_by_key(|o| o.at);
    Ok(out)
}

pub async fn list(state: &AppState) -> Result<Vec<ScheduleItem>, AppError> {
    Ok(store::list(&state.db)
        .await?
        .iter()
        .map(to_public)
        .collect())
}

fn clean_title(title: &str) -> Result<String, AppError> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::bad_request(
            "Say what it's about, in up to 200 characters.",
        ));
    }
    Ok(title.to_owned())
}

fn clean_instruction(
    kind: ScheduleKind,
    instruction: Option<String>,
) -> Result<Option<String>, AppError> {
    let instruction = instruction
        .map(|i| i.trim().to_owned())
        .filter(|i| !i.is_empty());
    match kind {
        ScheduleKind::Routine if instruction.is_none() => Err(AppError::bad_request(
            "A routine needs to say what the assistant should do.",
        )),
        ScheduleKind::Routine => Ok(instruction),
        ScheduleKind::Reminder => Ok(None),
    }
}

pub async fn create(
    state: &AppState,
    new: NewScheduleItem,
    created_in: Option<Uuid>,
) -> Result<Item, AppError> {
    rules::validate(&new.schedule).map_err(AppError::bad_request)?;
    let now = now_ms();
    let mut item = Item {
        id: Uuid::now_v7(),
        kind: new.kind,
        title: clean_title(&new.title)?,
        instruction: clean_instruction(new.kind, new.instruction)?,
        schedule: new.schedule,
        paused: false,
        next_at: None,
        snoozed_until: None,
        last_at: None,
        ended: None,
        event_start: None,
        conversation_id: None,
        created_in,
        anchor_at: now,
        created_at: now,
        updated_at: now,
    };
    arm(state, &mut item, ts(now), &zone()).await;
    if item.ended.is_some() {
        return Err(AppError::bad_request(
            "That event can't be found in your calendars.",
        ));
    }
    if item.next_at.is_none() {
        return Err(AppError::bad_request(match item.schedule {
            Schedule::BeforeEvent { .. } => "That event has already started.",
            _ => "That time has already passed.",
        }));
    }
    store::upsert(&state.db, item.clone()).await?;
    state.events.publish(Event::ScheduleChanged);
    state.scheduler.poke();
    Ok(item)
}

pub async fn update(state: &AppState, id: Uuid, update: ScheduleUpdate) -> Result<Item, AppError> {
    let mut item = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Reminder"))?;
    let now = now_ms();
    let mut rearm = false;
    if let Some(title) = update.title {
        item.title = clean_title(&title)?;
    }
    if update.instruction.is_some() {
        item.instruction = clean_instruction(item.kind, update.instruction)?;
    }
    if let Some(schedule) = update.schedule {
        rules::validate(&schedule).map_err(AppError::bad_request)?;
        item.schedule = schedule;
        item.anchor_at = now;
        item.ended = None;
        item.event_start = None;
        rearm = true;
    }
    if let Some(paused) = update.paused {
        // Resuming plans from now: what came due while paused isn't delivered.
        rearm |= item.paused && !paused;
        item.paused = paused;
    }
    if rearm {
        item.snoozed_until = None;
        arm(state, &mut item, ts(now), &zone()).await;
        if item.next_at.is_none() && item.ended.is_none() && update.paused != Some(false) {
            return Err(AppError::bad_request("That time has already passed."));
        }
    }
    item.updated_at = now;
    store::upsert(&state.db, item.clone()).await?;
    state.events.publish(Event::ScheduleChanged);
    state.scheduler.poke();
    Ok(item)
}

pub async fn delete(state: &AppState, id: Uuid) -> Result<bool, AppError> {
    let removed = store::delete(&state.db, id).await?;
    if removed {
        state.events.publish(Event::ScheduleChanged);
        state.scheduler.poke();
    }
    Ok(removed)
}

/// Runs a routine right away, outside its schedule ("Run now").
pub async fn run_now(state: &Arc<AppState>, id: Uuid) -> Result<(), AppError> {
    let mut item = store::get(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("Routine"))?;
    if item.kind != ScheduleKind::Routine {
        return Err(AppError::bad_request("Only routines can be run."));
    }
    if item.conversation_id.is_none() {
        item.conversation_id = routine_conversation(state, &item).await;
        store::upsert(&state.db, item.clone()).await?;
    }
    let now = now_ms();
    start_routine(state, &item, now, now, Fire::OnTime);
    Ok(())
}

/// Takes back a change the assistant made; `revision` comes from its tool output.
pub async fn undo(state: &AppState, revision: i64) -> Result<(), AppError> {
    let (item_id, before, conversation) = match store::take_revision(&state.db, revision).await? {
        store::Undo::Restore {
            item_id,
            before,
            conversation_id,
        } => (item_id, before, conversation_id),
        store::Undo::AlreadyUndone => {
            return Err(AppError::new(
                axum::http::StatusCode::CONFLICT,
                "already_undone",
                "That was already undone.",
            ));
        }
        store::Undo::Unknown => return Err(AppError::not_found("Change")),
    };
    match before.map(|b| *b) {
        Some(mut item) => {
            // Plan from now, so restoring an old reminder doesn't fire it at once.
            if !item.paused && item.ended.is_none() {
                arm(state, &mut item, Timestamp::now(), &zone()).await;
            }
            item.updated_at = now_ms();
            store::upsert(&state.db, item).await?;
        }
        None => {
            store::delete(&state.db, item_id).await?;
        }
    }
    // Show it as undone in the chat line that made it.
    if let Some(conversation) = conversation {
        for mut message in crate::chat::store::messages(&state.db, conversation).await? {
            let mut touched = false;
            for a in &mut message.actions {
                if let Some(out) = a.output.as_mut()
                    && out["schedule_revision"].as_i64() == Some(revision)
                {
                    out["undone"] = serde_json::Value::Bool(true);
                    touched = true;
                }
            }
            if touched {
                crate::chat::store::upsert_message(&state.db, message.clone()).await?;
                state.events.publish(Event::MessageUpdated { message });
            }
        }
    }
    state.events.publish(Event::ScheduleChanged);
    state.scheduler.poke();
    Ok(())
}

#[cfg(test)]
mod tests;
