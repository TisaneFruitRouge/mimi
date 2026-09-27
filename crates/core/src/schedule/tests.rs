//! Scheduler behavior on a controlled clock. Delivery through Telegram, routines and the
//! assistant's tools are covered end to end in `api/tests.rs` (`schedule_flow`).

use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use mimi_protocol::{DeliveryStatus, NewScheduleItem, Schedule, ScheduleKind, ScheduleUpdate};

use super::rules;
use super::store::{self, Item};
use crate::AppState;

fn state() -> Arc<AppState> {
    Arc::new(AppState::for_tests("t"))
}

/// An instant from local wall-clock time, in this computer's zone (as the scheduler uses).
fn at(s: &str) -> Timestamp {
    super::zone()
        .to_ambiguous_zoned(rules::parse_local(s).unwrap())
        .compatible()
        .unwrap()
        .timestamp()
}

fn local(ms: i64) -> String {
    super::ts(ms)
        .to_zoned(super::zone())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// A daily reminder whose next occurrence is `due`, stored directly.
async fn daily_due(state: &AppState, title: &str, due: &str) -> Item {
    let now = at(due).as_millisecond();
    let item = Item {
        id: uuid::Uuid::now_v7(),
        kind: ScheduleKind::Reminder,
        title: title.to_owned(),
        instruction: None,
        schedule: Schedule::Daily {
            time: due[11..16].to_owned(),
        },
        paused: false,
        next_at: Some(now),
        snoozed_until: None,
        last_at: None,
        ended: None,
        event_start: None,
        conversation_id: None,
        created_in: None,
        anchor_at: now,
        created_at: now,
        updated_at: now,
    };
    store::upsert(&state.db, item.clone()).await.unwrap();
    item
}

/// Deliveries are recorded in the background: wait for them to settle.
async fn deliveries(state: &AppState, expected: usize) -> Vec<mimi_protocol::Delivery> {
    for _ in 0..100 {
        let d = store::recent(&state.db, 50).await.unwrap();
        if d.len() >= expected {
            return d;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    store::recent(&state.db, 50).await.unwrap()
}

#[tokio::test]
async fn on_time_late_and_missed() {
    let s = state();
    let on_time = daily_due(&s, "On time", "2026-10-05T07:00").await;
    super::tick(&s, at("2026-10-05T07:00")).await;
    let d = deliveries(&s, 1).await;
    assert_eq!(d[0].status, DeliveryStatus::Delivered);
    let item = store::get(&s.db, on_time.id).await.unwrap().unwrap();
    assert_eq!(local(item.next_at.unwrap()), "2026-10-06 07:00");

    // The computer slept through it: sent once when it wakes, marked late.
    let late = daily_due(&s, "Late", "2026-10-05T07:00").await;
    super::tick(&s, at("2026-10-05T07:40")).await;
    let d = deliveries(&s, 2).await;
    let mine: Vec<_> = d.iter().filter(|d| d.item_id == late.id).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].status, DeliveryStatus::Late);
}

#[tokio::test]
async fn long_outages_are_recorded_not_replayed() {
    let s = state();
    // Due three days ago; Mimi wasn't running since.
    let item = daily_due(&s, "Stretch", "2026-10-02T07:00").await;
    super::tick(&s, at("2026-10-05T12:00")).await;
    let d = deliveries(&s, 1).await;
    assert_eq!(d.len(), 1, "one record, not one per missed day");
    assert_eq!(d[0].status, DeliveryStatus::Missed);
    // Next is tomorrow morning, not a backlog.
    let item = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(local(item.next_at.unwrap()), "2026-10-06 07:00");
    // Nothing more happens until then.
    super::tick(&s, at("2026-10-05T12:01")).await;
    assert_eq!(deliveries(&s, 2).await.len(), 1);
}

#[tokio::test]
async fn intervals_skip_to_the_next_slot_after_a_gap() {
    let s = state();
    let mut item = daily_due(&s, "Drink water", "2026-10-05T09:00").await;
    item.schedule = Schedule::Interval { minutes: 30 };
    store::upsert(&s.db, item.clone()).await.unwrap();
    // Asleep from 09:00 to 11:10: one late reminder, then back on the 30-minute grid.
    super::tick(&s, at("2026-10-05T11:10")).await;
    assert_eq!(deliveries(&s, 1).await.len(), 1);
    let item = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(local(item.next_at.unwrap()), "2026-10-05 11:30");
}

#[tokio::test]
async fn paused_items_wait_and_resume_from_now() {
    let s = state();
    let item = super::create(
        &s,
        NewScheduleItem {
            kind: ScheduleKind::Reminder,
            title: "Vitamins".into(),
            instruction: None,
            schedule: Schedule::Daily {
                time: "08:00".into(),
            },
        },
        None,
    )
    .await
    .unwrap();
    super::update(
        &s,
        item.id,
        ScheduleUpdate {
            paused: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Far past its time while paused: nothing is sent or recorded.
    let paused = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert!(paused.wake_at().is_none());
    super::tick(&s, Timestamp::now() + jiff::SignedDuration::from_hours(48)).await;
    assert!(deliveries(&s, 1).await.is_empty());

    let resumed = super::update(
        &s,
        item.id,
        ScheduleUpdate {
            paused: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(resumed.next_at.unwrap() > crate::now_ms());
}

#[tokio::test]
async fn one_time_reminders_finish_and_past_times_are_refused() {
    let s = state();
    let past = super::create(
        &s,
        NewScheduleItem {
            kind: ScheduleKind::Reminder,
            title: "Too late".into(),
            instruction: None,
            schedule: Schedule::Once {
                at: "2020-01-01T09:00".into(),
            },
        },
        None,
    )
    .await;
    assert!(past.is_err());

    let routine_without_instruction = super::create(
        &s,
        NewScheduleItem {
            kind: ScheduleKind::Routine,
            title: "Briefing".into(),
            instruction: Some("  ".into()),
            schedule: Schedule::Daily {
                time: "07:00".into(),
            },
        },
        None,
    )
    .await;
    assert!(routine_without_instruction.is_err());

    let mut once = daily_due(&s, "Dentist papers", "2026-10-05T09:00").await;
    once.schedule = Schedule::Once {
        at: "2026-10-05T09:00".into(),
    };
    store::upsert(&s.db, once.clone()).await.unwrap();
    super::tick(&s, at("2026-10-05T09:00")).await;
    let done = store::get(&s.db, once.id).await.unwrap().unwrap();
    assert_eq!(done.next_at, None);
    assert!(done.wake_at().is_none());
}

#[tokio::test]
async fn undo_restores_the_previous_version() {
    let s = state();
    let item = daily_due(&s, "Bins", "2026-10-05T20:00").await;
    let conversation = uuid::Uuid::now_v7();
    let revision = store::remember_before(&s.db, item.id, Some(item.clone()), conversation)
        .await
        .unwrap();
    super::update(
        &s,
        item.id,
        ScheduleUpdate {
            title: Some("Recycling".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    super::undo(&s, revision).await.unwrap();
    let restored = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(restored.title, "Bins");
    // Restoring plans from now: it isn't fired for a time that already passed.
    assert!(restored.next_at.unwrap() > crate::now_ms());
    assert!(super::undo(&s, revision).await.is_err());
}

type Feed = Arc<std::sync::Mutex<(u16, String)>>;

/// A calendar feed whose events the test can change, served like a Google iCal address.
struct FakeFeed {
    ics: Feed,
    url: String,
}

impl FakeFeed {
    async fn start() -> Self {
        use axum::extract::State;
        let ics: Feed = Arc::new(std::sync::Mutex::new((200, String::new())));
        let app = axum::Router::new()
            .route(
                "/cal.ics",
                axum::routing::get(|State(ics): State<Feed>| async move {
                    let (status, body) = ics.lock().unwrap().clone();
                    (axum::http::StatusCode::from_u16(status).unwrap(), body)
                }),
            )
            .with_state(ics.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/cal.ics", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        FakeFeed { ics, url }
    }

    /// Replaces the calendar's content: the dentist at `start`, or no event at all.
    fn dentist_at(&self, start: Option<Timestamp>) {
        let fmt = |t: Timestamp| t.strftime("%Y%m%dT%H%M%SZ").to_string();
        let event = start.map(|s| {
            format!(
                "BEGIN:VEVENT\r\nUID:dentist-1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:{}\r\n\
                 DTEND:{}\r\nSUMMARY:Dentist\r\nEND:VEVENT\r\n",
                fmt(s),
                fmt(s + jiff::SignedDuration::from_hours(1))
            )
        });
        *self.ics.lock().unwrap() = (
            200,
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\n{}END:VCALENDAR\r\n",
                event.unwrap_or_default()
            ),
        );
    }

    fn fail(&self) {
        self.ics.lock().unwrap().0 = 500;
    }
}

/// The next check sees the calendar as it is now.
fn forget_checks(s: &AppState, feed: &FakeFeed) {
    s.connections.feeds.forget(&feed.url);
    s.scheduler.event_checked.lock().unwrap().clear();
}

#[tokio::test]
async fn event_reminders_follow_their_event() {
    let s = state();
    let feed = FakeFeed::start().await;
    let hour = jiff::SignedDuration::from_hours(1);
    let start = Timestamp::from_second(Timestamp::now().as_second() / 3600 * 3600).unwrap()
        + jiff::SignedDuration::from_hours(72);
    feed.dentist_at(Some(start));
    crate::connections::store::upsert(
        &s.db,
        crate::connections::store::ConnectionRow {
            id: uuid::Uuid::now_v7(),
            integration: crate::connections::calendar::GOOGLE.to_owned(),
            name: "Personal".to_owned(),
            config: serde_json::json!({ "ics_url": feed.url }),
            created_at: 0,
        },
    )
    .await
    .unwrap();

    // The id the assistant gets from an @ mention or a title search.
    let accounts = crate::connections::calendar_accounts(&s).await;
    let (events, _) = crate::connections::calendar::events_between(
        &s.http,
        &s.connections.feeds,
        &accounts,
        chrono::Utc::now(),
        chrono::Utc::now() + chrono::Duration::days(7),
    )
    .await;
    let event_id = crate::people::mentions::event_id(&events[0]);
    let before_dentist = |title: &str, minutes_before| NewScheduleItem {
        kind: ScheduleKind::Reminder,
        title: title.into(),
        instruction: None,
        schedule: Schedule::BeforeEvent {
            event_id: event_id.clone(),
            event_title: "Dentist".into(),
            minutes_before,
        },
    };

    let item = super::create(&s, before_dentist("Bring the insurance card", 60), None)
        .await
        .unwrap();
    assert_eq!(rules::describe(&item.schedule), "1 hour before “Dentist”");
    assert_eq!(item.next_at, Some((start - hour).as_millisecond()));

    // Moved two hours later: the reminder follows on the next check.
    let moved = start + jiff::SignedDuration::from_hours(2);
    feed.dentist_at(Some(moved));
    forget_checks(&s, &feed);
    super::tick(&s, Timestamp::now()).await;
    let item = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(item.next_at, Some((moved - hour).as_millisecond()));

    // The calendar can't be reached: the last known time stands.
    feed.fail();
    forget_checks(&s, &feed);
    super::tick(&s, Timestamp::now()).await;
    let item = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(item.next_at, Some((moved - hour).as_millisecond()));
    assert_eq!(item.ended, None);

    // Moved again just before the old time came: it isn't sent early, it waits.
    let later = moved + hour;
    feed.dentist_at(Some(later));
    forget_checks(&s, &feed);
    super::tick(&s, moved - hour).await;
    let item = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(item.next_at, Some((later - hour).as_millisecond()));
    assert!(deliveries(&s, 1).await.is_empty());

    // At the new time it goes off once; a reminder for a one-off event is then finished.
    super::tick(&s, later - hour).await;
    assert_eq!(deliveries(&s, 1).await.len(), 1);
    let item = store::get(&s.db, item.id).await.unwrap().unwrap();
    assert_eq!(item.next_at, None);

    // Cancelled events end their reminders, with a reason the user can read.
    let second = super::create(&s, before_dentist("Leave for the dentist", 30), None)
        .await
        .unwrap();
    feed.dentist_at(None);
    forget_checks(&s, &feed);
    super::tick(&s, Timestamp::now()).await;
    let second = store::get(&s.db, second.id).await.unwrap().unwrap();
    assert_eq!(
        second.ended.as_deref(),
        Some("The event was cancelled or removed.")
    );
    assert!(second.wake_at().is_none());
}
