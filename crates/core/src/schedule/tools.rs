//! Reminder and routine tools for the model. Setting something up for the user themselves
//! is private and local, so none needs an approval card; every change shows as one quiet
//! line in the chat, with Undo. (What a routine *does* when it runs still goes through
//! the approval rule, at that time.)

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use mimi_protocol::{NewScheduleItem, Schedule, ScheduleKind, ScheduleUpdate, Weekday};
use serde_json::{Value, json};

use super::{rules, store};
use crate::AppState;
use crate::tools::{Tool, ToolContext, ToolSource};

pub struct ScheduleTools;

impl ToolSource for ScheduleTools {
    fn tools<'a>(&'a self, _state: &'a AppState) -> BoxFuture<'a, Vec<Arc<dyn Tool>>> {
        async move { all() }.boxed()
    }
}

/// Every reminder and routine tool. They work for whoever the turn is for: the user's
/// own items, or, in a trusted person's turn, only that person's (`access`).
pub fn all() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(AddReminder) as Arc<dyn Tool>,
        Arc::new(AddRoutine),
        Arc::new(List),
        Arc::new(Change),
        Arc::new(Cancel),
    ]
}

/// Whether an item is this turn's to see and change: the user's own for the user, and
/// only their own for someone they trust.
async fn belongs(ctx: &ToolContext, item: &store::Item) -> bool {
    match (ctx.principal.guest(), item.for_person) {
        (None, None) => true,
        (Some(guest), Some(person)) => {
            person == guest.person_id
                || crate::people::get(&ctx.state, person)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|p| p.id == guest.person_id)
        }
        _ => false,
    }
}

/// Arguments that say when, shared by adding and changing.
fn when_properties() -> Value {
    json!({
        "at": { "type": "string", "description": "Local date and time for a one-time reminder, YYYY-MM-DDTHH:MM" },
        "in_minutes": { "type": "integer", "description": "For \"in 20 minutes\"" },
        "repeat": { "type": "string", "enum": ["daily", "weekdays", "weekly", "monthly", "yearly", "interval"], "description": "For something that repeats" },
        "time": { "type": "string", "description": "HH:MM, for repeats" },
        "days": { "type": "array", "items": { "type": "string" }, "description": "For weekly: mon, tue, wed, thu, fri, sat, sun" },
        "day_of_month": { "type": "integer", "description": "For monthly" },
        "date": { "type": "string", "description": "For yearly: MM-DD" },
        "every_minutes": { "type": "integer", "description": "For interval" },
        "event": { "type": "string", "description": "To time it relative to a calendar event: the event's title, or its event id if the user tagged it" },
        "minutes_before": { "type": "integer", "description": "With event: how long before it starts (default 30)" }
    })
}

fn has_when(args: &Value) -> bool {
    [
        "at",
        "in_minutes",
        "repeat",
        "time",
        "days",
        "day_of_month",
        "date",
        "every_minutes",
        "event",
    ]
    .iter()
    .any(|k| !args[*k].is_null())
}

fn int(args: &Value, key: &str) -> Option<i64> {
    args[key]
        .as_i64()
        .or_else(|| args[key].as_str().and_then(|s| s.trim().parse().ok()))
}

fn text(args: &Value, key: &str) -> Option<String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// A title as the user will see it in lists and notifications: "Call Léa", not "call Léa".
fn title(args: &Value, key: &str) -> Option<String> {
    text(args, key).map(|t| {
        let mut chars = t.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default()
    })
}

fn parse_day(s: &str) -> Option<Weekday> {
    let s = s.trim().to_lowercase();
    Some(match s.get(..3)? {
        "mon" => Weekday::Mon,
        "tue" => Weekday::Tue,
        "wed" => Weekday::Wed,
        "thu" => Weekday::Thu,
        "fri" => Weekday::Fri,
        "sat" => Weekday::Sat,
        "sun" => Weekday::Sun,
        _ => return None,
    })
}

fn from_jiff(w: jiff::civil::Weekday) -> Weekday {
    use jiff::civil::Weekday as J;
    match w {
        J::Monday => Weekday::Mon,
        J::Tuesday => Weekday::Tue,
        J::Wednesday => Weekday::Wed,
        J::Thursday => Weekday::Thu,
        J::Friday => Weekday::Fri,
        J::Saturday => Weekday::Sat,
        J::Sunday => Weekday::Sun,
    }
}

fn local_string(t: Timestamp, tz: &TimeZone) -> String {
    t.to_zoned(tz.clone())
        .strftime("%Y-%m-%dT%H:%M")
        .to_string()
}

/// Turns the model's "when" arguments into a schedule. `None` if it didn't say when.
/// Events are looked for only in `calendars` when given (a trusted person's shared
/// calendars).
async fn schedule_from_args(
    state: &AppState,
    args: &Value,
    now: Timestamp,
    tz: &TimeZone,
    calendars: Option<&[String]>,
) -> Result<Option<Schedule>, String> {
    if !has_when(args) {
        return Ok(None);
    }
    if let Some(event) = text(args, "event") {
        let minutes = int(args, "minutes_before")
            .unwrap_or(30)
            .clamp(0, 7 * 24 * 60) as u32;
        let (event_id, event_title) = resolve_event(state, &event, now, calendars).await?;
        return Ok(Some(Schedule::BeforeEvent {
            event_id,
            event_title,
            minutes_before: minutes,
        }));
    }
    let at = text(args, "at")
        .map(|a| rules::parse_local(&a))
        .transpose()?;
    let today = now.to_zoned(tz.clone()).datetime();
    let time = match text(args, "time") {
        Some(t) => {
            let t = rules::parse_time(&t)?;
            format!("{:02}:{:02}", t.hour(), t.minute())
        }
        None => at
            .map(|a| format!("{:02}:{:02}", a.hour(), a.minute()))
            .unwrap_or_else(|| "09:00".to_owned()),
    };
    let base_date = at.map(|a| a.date()).unwrap_or(today.date());

    if let Some(repeat) = text(args, "repeat") {
        let schedule = match repeat.to_lowercase().as_str() {
            "daily" | "every_day" | "everyday" => Schedule::Daily { time },
            "weekdays" | "workdays" => Schedule::Weekdays { time },
            "weekly" => {
                let days: Vec<Weekday> = match &args["days"] {
                    Value::Array(d) => d
                        .iter()
                        .filter_map(|v| v.as_str())
                        .filter_map(parse_day)
                        .collect(),
                    Value::String(d) => d.split([',', ' ']).filter_map(parse_day).collect(),
                    _ => Vec::new(),
                };
                let days = if days.is_empty() {
                    vec![from_jiff(base_date.weekday())]
                } else {
                    days
                };
                Schedule::Weekly { days, time }
            }
            "monthly" => Schedule::Monthly {
                day: int(args, "day_of_month")
                    .unwrap_or(base_date.day() as i64)
                    .clamp(1, 31) as u8,
                time,
            },
            "yearly" | "annually" | "every_year" => {
                let (month, day) = match text(args, "date") {
                    Some(d) => {
                        let d = d.rsplit_once('-').map(|(m, d)| {
                            (m.rsplit('-').next().unwrap_or(m).to_owned(), d.to_owned())
                        });
                        match d {
                            Some((m, d)) => (
                                m.parse::<u8>()
                                    .map_err(|_| "`date` should look like 03-12".to_owned())?,
                                d.parse::<u8>()
                                    .map_err(|_| "`date` should look like 03-12".to_owned())?,
                            ),
                            None => return Err("`date` should look like 03-12".to_owned()),
                        }
                    }
                    None => (base_date.month() as u8, base_date.day() as u8),
                };
                Schedule::Yearly { month, day, time }
            }
            "interval" | "hourly" => {
                let minutes =
                    int(args, "every_minutes").unwrap_or(if repeat == "hourly" { 60 } else { 0 });
                if minutes <= 0 {
                    return Err("For an interval, say every how many minutes.".to_owned());
                }
                Schedule::Interval {
                    minutes: minutes as u32,
                }
            }
            other => return Err(format!("“{other}” isn't a kind of repeat I know.")),
        };
        return Ok(Some(schedule));
    }
    if let Some(minutes) = int(args, "in_minutes") {
        if minutes <= 0 {
            return Err("`in_minutes` must be positive.".to_owned());
        }
        // Rounded up to the next whole minute, so "in 1 minute" is never in the past.
        let at = now
            .checked_add((minutes * 60 + 59).seconds())
            .map_err(|e| e.to_string())?;
        return Ok(Some(Schedule::Once {
            at: local_string(at, tz),
        }));
    }
    if let Some(at) = at {
        return Ok(Some(Schedule::Once {
            at: format!("{}T{:02}:{:02}", at.date(), at.hour(), at.minute()),
        }));
    }
    // Only a time: the next time it comes round.
    let t = rules::parse_time(&time)?;
    let mut date = today.date();
    if date.to_datetime(t) <= today {
        date = date.tomorrow().map_err(|e| e.to_string())?;
    }
    Ok(Some(Schedule::Once {
        at: format!("{}T{:02}:{:02}", date, t.hour(), t.minute()),
    }))
}

/// Finds a calendar event by id (from an @ mention) or by title: the next one to start.
async fn resolve_event(
    state: &AppState,
    query: &str,
    now: Timestamp,
    calendars: Option<&[String]>,
) -> Result<(String, String), String> {
    let accounts = crate::connections::calendar_accounts(state).await;
    let accounts = match calendars {
        Some(allowed) => crate::connections::calendar::restrict(accounts, allowed),
        None => accounts,
    };
    if accounts.is_empty() {
        return Err("No calendar is connected, so I can't time this to an event.".to_owned());
    }
    let now = chrono::DateTime::from_timestamp_millis(now.as_millisecond()).unwrap_or_default();
    let (events, _) = crate::connections::calendar::events_between(
        &state.http,
        &state.connections.feeds,
        &accounts,
        now - chrono::Duration::hours(1),
        now + chrono::Duration::days(60),
    )
    .await;
    if query.starts_with("ev:") {
        return events
            .iter()
            .find(|e| crate::people::mentions::event_id(e) == query)
            .map(|e| (query.to_owned(), e.title.clone()))
            .ok_or_else(|| "That event can't be found in your calendars.".to_owned());
    }
    let q = query.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    events
        .iter()
        .filter(|e| e.start > now)
        .find(|e| {
            let title = e.title.to_lowercase();
            title.contains(&q) || q.contains(&title) || words.iter().all(|w| title.contains(w))
        })
        .map(|e| (crate::people::mentions::event_id(e), e.title.clone()))
        .ok_or_else(|| format!("I couldn't find an upcoming event called “{query}”."))
}

/// "today at 18:30", "tomorrow at 9:00", "on Friday at 7:00", "on 12 Oct at 9:00".
pub fn human_when(at_ms: i64, now: Timestamp, tz: &TimeZone) -> String {
    let Ok(at) = Timestamp::from_millisecond(at_ms) else {
        return String::new();
    };
    let at = at.to_zoned(tz.clone());
    let today = now.to_zoned(tz.clone()).date();
    let days = (at.date() - today).get_days();
    let time = at.strftime("%-H:%M");
    match days {
        0 => format!("today at {time}"),
        1 => format!("tomorrow at {time}"),
        2..=6 => format!("on {} at {time}", at.strftime("%A")),
        _ => format!("on {} at {time}", at.strftime("%-d %b")),
    }
}

fn describe_item(item: &store::Item, now: Timestamp, tz: &TimeZone) -> String {
    match (&item.schedule, item.next_at) {
        (Schedule::Once { .. }, Some(next)) => human_when(next, now, tz),
        (s, _) => lower_first(&rules::describe(s)),
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_lowercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn item_json(item: &store::Item, now: Timestamp, tz: &TimeZone) -> Value {
    json!({
        "id": item.id,
        "kind": item.kind,
        "title": item.title,
        "instruction": item.instruction,
        "when": rules::describe(&item.schedule),
        "next": item.wake_at().map(|n| human_when(n, now, tz)),
        "paused": item.paused,
        "ended": item.ended,
    })
}

/// The item an id refers to. Small models sometimes pass a name instead of the id: a name
/// that matches exactly one item is accepted; otherwise the error lists what exists, so
/// the model can try again with a real id.
async fn find_item(ctx: &ToolContext, args: &Value) -> Result<store::Item, String> {
    let id = text(args, "id").ok_or("`id` is required; list reminders first to get it.")?;
    let mut items = Vec::new();
    for item in store::list(&ctx.state.db)
        .await
        .map_err(|e| e.to_string())?
    {
        if belongs(ctx, &item).await {
            items.push(item);
        }
    }
    if let Some(item) = items
        .iter()
        .find(|i| i.id.to_string() == id || (id.len() >= 8 && i.id.to_string().starts_with(&id)))
    {
        return Ok(item.clone());
    }
    let wanted = id.to_lowercase().replace(['-', '_'], " ");
    let by_name: Vec<&store::Item> = items
        .iter()
        .filter(|i| {
            let title = i.title.to_lowercase();
            !title.is_empty() && (wanted.contains(&title) || title.contains(&wanted))
        })
        .collect();
    if let [one] = by_name.as_slice() {
        return Ok((*one).clone());
    }
    if items.is_empty() {
        return Err("The user has no reminders or routines.".to_owned());
    }
    let list: Vec<String> = items
        .iter()
        .map(|i| format!("{} “{}”", i.id, i.title))
        .collect();
    Err(format!(
        "There's no reminder or routine with that id. These exist: {}",
        list.join("; ")
    ))
}

/// The first clock time in some text: "every weekday at 8:00" → "8:00".
fn time_in(s: &str) -> Option<String> {
    s.split(|c: char| !(c.is_ascii_digit() || c == ':'))
        .find(|w| rules::parse_time(w).is_ok() && w.contains(':'))
        .map(str::to_owned)
}

/// A change that only says part of when ("make it 8:00") keeps the rest of the schedule:
/// a weekday reminder moved to 8:00 still repeats on weekdays.
fn merged_when(before: &Schedule, args: &Value) -> Value {
    let restates = ["repeat", "at", "in_minutes", "event"]
        .iter()
        .any(|k| !args[*k].is_null());
    if restates {
        return args.clone();
    }
    let mut merged = match before {
        Schedule::Once { at } => {
            // Same day, new time.
            let time = text(args, "time")
                .and_then(|t| rules::parse_time(&t).ok())
                .map(|t| format!("{:02}:{:02}", t.hour(), t.minute()));
            return match (at.get(..10), time) {
                (Some(day), Some(time)) => json!({ "at": format!("{day}T{time}") }),
                _ => json!({ "at": at }),
            };
        }
        Schedule::Daily { time } => json!({ "repeat": "daily", "time": time }),
        Schedule::Weekdays { time } => json!({ "repeat": "weekdays", "time": time }),
        Schedule::Weekly { days, time } => {
            json!({ "repeat": "weekly", "days": days, "time": time })
        }
        Schedule::Monthly { day, time } => {
            json!({ "repeat": "monthly", "day_of_month": day, "time": time })
        }
        Schedule::Yearly { month, day, time } => {
            json!({ "repeat": "yearly", "date": format!("{month:02}-{day:02}"), "time": time })
        }
        Schedule::Interval { minutes } => json!({ "repeat": "interval", "every_minutes": minutes }),
        Schedule::BeforeEvent {
            event_id,
            minutes_before,
            ..
        } => json!({ "event": event_id, "minutes_before": minutes_before }),
    };
    for key in [
        "time",
        "days",
        "day_of_month",
        "date",
        "every_minutes",
        "minutes_before",
    ] {
        if !args[key].is_null() {
            merged[key] = args[key].clone();
        }
    }
    merged
}

struct AddReminder;

impl Tool for AddReminder {
    fn name(&self) -> &str {
        "reminder_add"
    }
    fn description(&self) -> &str {
        "Set a reminder for the user. Use this whenever they say \"remind me\", \"don't let me \
         forget\" or similar, never calendar_add_event: a reminder is a nudge to them, not an \
         event in their calendar. Anything they ask to be reminded of, once \
         (\"tomorrow at 9\", \"in 20 minutes\"), repeating (\"every Monday at 8\"), or relative \
         to a calendar event (\"an hour before the \
         dentist\"). They get it on their phone (Telegram, if connected) and computer. Times \
         are the user's local time. Afterwards, tell the user the time exactly as the result's \
         `when` says; don't show them the id."
    }
    fn parameters(&self) -> Value {
        let mut props = when_properties();
        props["text"] = json!({ "type": "string", "description": "What to remind them of, short, e.g. \"Call Léa\"" });
        json!({ "type": "object", "properties": props, "required": ["text"] })
    }
    fn needs_approval(&self, _: &Value) -> bool {
        false
    }
    fn governed_by(&self) -> Option<crate::tools::Governs> {
        Some(crate::tools::Governs::Schedule)
    }
    fn summary(&self, _: &Value) -> String {
        "Set a reminder".to_owned()
    }
    fn result_label(&self, args: &Value, output: &Value) -> String {
        format!(
            "reminder set {}: {}",
            output["when"].as_str().unwrap_or_default(),
            output["title"]
                .as_str()
                .or(args["text"].as_str())
                .unwrap_or_default()
        )
    }
    fn run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        async move { add(ctx, &args, ScheduleKind::Reminder).await }.boxed()
    }
}

struct AddRoutine;

impl Tool for AddRoutine {
    fn name(&self) -> &str {
        "routine_add"
    }
    fn description(&self) -> &str {
        "Set up a routine: work you do for the user on a schedule and report back, e.g. \
         \"every morning at 7, send me my day\" or \"Sunday at 18:00, look at my week\". Each \
         run is a message to you with the instruction; the answer goes to the user's phone \
         (Telegram, if connected) and computer. Only when you have to do something each time \
         (look things up, sum up, write); for \"remind me to …\", even every day, use \
         reminder_add. Don't show the user the id."
    }
    fn parameters(&self) -> Value {
        let mut props = when_properties();
        props["name"] =
            json!({ "type": "string", "description": "A short name, e.g. \"Morning briefing\"" });
        props["instruction"] = json!({ "type": "string", "description": "What to do each time, written as a request to you" });
        json!({ "type": "object", "properties": props, "required": ["name", "instruction"] })
    }
    fn needs_approval(&self, _: &Value) -> bool {
        false
    }
    fn governed_by(&self) -> Option<crate::tools::Governs> {
        Some(crate::tools::Governs::Schedule)
    }
    fn summary(&self, _: &Value) -> String {
        "Set up a routine".to_owned()
    }
    fn result_label(&self, args: &Value, output: &Value) -> String {
        format!(
            "routine set up: {}, {}",
            output["title"]
                .as_str()
                .or(args["name"].as_str())
                .unwrap_or_default(),
            output["when"].as_str().unwrap_or_default()
        )
    }
    fn run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        async move { add(ctx, &args, ScheduleKind::Routine).await }.boxed()
    }
}

async fn add(ctx: &ToolContext, args: &Value, kind: ScheduleKind) -> Result<Value, String> {
    let state = &ctx.state;
    let tz = TimeZone::system();
    let now = Timestamp::now();
    let schedule = schedule_from_args(state, args, now, &tz, ctx.principal.calendars())
        .await?
        .ok_or("Say when: `at`, `in_minutes`, `repeat` with `time`, or `event`.")?;
    let (title, instruction) = match kind {
        ScheduleKind::Reminder => (title(args, "text").ok_or("`text` is required")?, None),
        ScheduleKind::Routine => (
            title(args, "name").ok_or("`name` is required")?,
            Some(text(args, "instruction").ok_or("`instruction` is required")?),
        ),
    };
    let item = super::create_for(
        state,
        NewScheduleItem {
            kind,
            title,
            instruction,
            schedule,
        },
        Some(ctx.conversation_id),
        ctx.principal.guest().map(|g| g.person_id),
    )
    .await
    .map_err(|e| e.message().to_owned())?;
    let revision = store::remember_before(&state.db, item.id, None, ctx.conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = item_json(&item, now, &tz);
    out["when"] = json!(describe_item(&item, now, &tz));
    out["schedule_revision"] = json!(revision);
    Ok(out)
}

struct List;

impl Tool for List {
    fn name(&self) -> &str {
        "schedule_list"
    }
    fn description(&self) -> &str {
        "List the user's reminders and routines, with their ids, to answer questions about \
         them or before changing or cancelling one."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn needs_approval(&self, _: &Value) -> bool {
        false
    }
    fn summary(&self, _: &Value) -> String {
        "Check your reminders".to_owned()
    }
    fn result_label(&self, _: &Value, _: &Value) -> String {
        "checked your reminders".to_owned()
    }
    fn run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        _args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        async move {
            let tz = TimeZone::system();
            let now = Timestamp::now();
            let mut active: Vec<Value> = Vec::new();
            for item in store::list(&ctx.state.db)
                .await
                .map_err(|e| e.to_string())?
            {
                if (item.wake_at().is_some() || item.paused)
                    && active.len() < 40
                    && belongs(ctx, &item).await
                {
                    active.push(item_json(&item, now, &tz));
                }
            }
            Ok(json!({ "items": active }))
        }
        .boxed()
    }
}

struct Change;

impl Tool for Change {
    fn name(&self) -> &str {
        "schedule_change"
    }
    fn description(&self) -> &str {
        "Change a reminder or routine by id: its text or name, its instruction, when it \
         happens (same fields as when adding), or pause/resume it."
    }
    fn parameters(&self) -> Value {
        let mut props = when_properties();
        props["id"] = json!({ "type": "string" });
        props["text"] =
            json!({ "type": "string", "description": "New reminder text or routine name" });
        props["instruction"] = json!({ "type": "string" });
        props["paused"] = json!({ "type": "boolean" });
        json!({ "type": "object", "properties": props, "required": ["id"] })
    }
    fn needs_approval(&self, _: &Value) -> bool {
        false
    }
    fn governed_by(&self) -> Option<crate::tools::Governs> {
        Some(crate::tools::Governs::Schedule)
    }
    fn summary(&self, _: &Value) -> String {
        "Change a reminder".to_owned()
    }
    fn result_label(&self, _: &Value, output: &Value) -> String {
        let title = output["title"].as_str().unwrap_or_default();
        if output["paused"].as_bool() == Some(true) {
            format!("paused “{title}”")
        } else {
            format!(
                "changed “{title}”: {}",
                output["when"].as_str().unwrap_or_default()
            )
        }
    }
    fn run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        async move {
            let state = &ctx.state;
            let tz = TimeZone::system();
            let now = Timestamp::now();
            let before = find_item(ctx, &args).await?;
            let mut args = args;
            // Small models sometimes write the new time in words ("every weekday at 8:00").
            if args["time"].is_null()
                && let Some(time) = args["when"].as_str().and_then(time_in)
            {
                args["time"] = json!(time);
            }
            let update = ScheduleUpdate {
                title: title(&args, "text"),
                instruction: text(&args, "instruction"),
                schedule: if has_when(&args) || !args["minutes_before"].is_null() {
                    schedule_from_args(
                        state,
                        &merged_when(&before.schedule, &args),
                        now,
                        &tz,
                        ctx.principal.calendars(),
                    )
                    .await?
                } else {
                    None
                },
                paused: args["paused"].as_bool(),
            };
            if update.title.is_none()
                && update.instruction.is_none()
                && update.schedule.is_none()
                && update.paused.is_none()
            {
                return Err(
                    "Nothing to change. Give `text`, `instruction`, `paused`, or when: \
                            `time` (HH:MM), `days`, `repeat`, `at`, `in_minutes`, `event`."
                        .to_owned(),
                );
            }
            let item = super::update(state, before.id, update)
                .await
                .map_err(|e| e.message().to_owned())?;
            let revision =
                store::remember_before(&state.db, item.id, Some(before), ctx.conversation_id)
                    .await
                    .map_err(|e| e.to_string())?;
            let mut out = item_json(&item, now, &tz);
            out["when"] = json!(describe_item(&item, now, &tz));
            out["schedule_revision"] = json!(revision);
            Ok(out)
        }
        .boxed()
    }
}

struct Cancel;

impl Tool for Cancel {
    fn name(&self) -> &str {
        "schedule_cancel"
    }
    fn description(&self) -> &str {
        "Cancel (delete) a reminder or routine by id."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] })
    }
    fn needs_approval(&self, _: &Value) -> bool {
        false
    }
    fn governed_by(&self) -> Option<crate::tools::Governs> {
        Some(crate::tools::Governs::Schedule)
    }
    fn summary(&self, _: &Value) -> String {
        "Cancel a reminder".to_owned()
    }
    fn result_label(&self, _: &Value, output: &Value) -> String {
        format!(
            "cancelled “{}”",
            output["title"].as_str().unwrap_or_default()
        )
    }
    fn run<'a>(
        &'a self,
        ctx: &'a ToolContext,
        args: Value,
    ) -> BoxFuture<'a, Result<Value, String>> {
        async move {
            let state = &ctx.state;
            let item = find_item(ctx, &args).await?;
            let (id, title) = (item.id, item.title.clone());
            let revision = store::remember_before(&state.db, id, Some(item), ctx.conversation_id)
                .await
                .map_err(|e| e.to_string())?;
            super::delete(state, id).await.map_err(|e| e.message().to_owned())?;
            Ok(json!({ "id": id, "title": title, "cancelled": true, "schedule_revision": revision }))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zurich() -> TimeZone {
        TimeZone::get("Europe/Zurich").unwrap()
    }

    fn now() -> Timestamp {
        // Friday 2 October 2026, 18:15 in Zurich.
        "2026-10-02T16:15:00Z".parse().unwrap()
    }

    async fn parse(args: Value) -> Result<Option<Schedule>, String> {
        let state = AppState::for_tests("t");
        schedule_from_args(&state, &args, now(), &zurich(), None).await
    }

    #[tokio::test]
    async fn one_time_reminders() {
        assert_eq!(
            parse(json!({"at": "2026-10-03T09:00"})).await.unwrap(),
            Some(Schedule::Once {
                at: "2026-10-03T09:00".into()
            })
        );
        assert_eq!(
            parse(json!({"in_minutes": 20})).await.unwrap(),
            Some(Schedule::Once {
                at: "2026-10-02T18:35".into()
            })
        );
        // Just a time: later today if it's still ahead, else tomorrow.
        assert_eq!(
            parse(json!({"time": "20:00"})).await.unwrap(),
            Some(Schedule::Once {
                at: "2026-10-02T20:00".into()
            })
        );
        assert_eq!(
            parse(json!({"time": "08:00"})).await.unwrap(),
            Some(Schedule::Once {
                at: "2026-10-03T08:00".into()
            })
        );
        assert_eq!(parse(json!({})).await.unwrap(), None);
    }

    #[tokio::test]
    async fn changes_keep_what_they_dont_mention() {
        let weekdays = Schedule::Weekdays {
            time: "07:30".into(),
        };
        // "Make it 8:00": still every weekday.
        assert_eq!(
            parse(merged_when(&weekdays, &json!({"id": "x", "time": "8:00"})))
                .await
                .unwrap(),
            Some(Schedule::Weekdays {
                time: "08:00".into()
            })
        );
        let weekly = Schedule::Weekly {
            days: vec![Weekday::Mon, Weekday::Thu],
            time: "20:00".into(),
        };
        assert_eq!(
            parse(merged_when(&weekly, &json!({"time": "19:00"})))
                .await
                .unwrap(),
            Some(Schedule::Weekly {
                days: vec![Weekday::Mon, Weekday::Thu],
                time: "19:00".into()
            })
        );
        // A one-time reminder moved to another time keeps its day.
        let once = Schedule::Once {
            at: "2026-10-05T09:00".into(),
        };
        assert_eq!(
            parse(merged_when(&once, &json!({"time": "10:30"})))
                .await
                .unwrap(),
            Some(Schedule::Once {
                at: "2026-10-05T10:30".into()
            })
        );
        assert_eq!(time_in("Every weekday at 8:00").as_deref(), Some("8:00"));
        assert_eq!(time_in("every weekday"), None);
        // Saying the whole thing again replaces it.
        assert_eq!(
            parse(merged_when(
                &weekdays,
                &json!({"repeat": "daily", "time": "06:00"})
            ))
            .await
            .unwrap(),
            Some(Schedule::Daily {
                time: "06:00".into()
            })
        );
    }

    #[tokio::test]
    async fn repeats() {
        assert_eq!(
            parse(json!({"repeat": "weekly", "days": ["Monday"], "time": "8:00"}))
                .await
                .unwrap(),
            Some(Schedule::Weekly {
                days: vec![Weekday::Mon],
                time: "08:00".into()
            })
        );
        assert_eq!(
            parse(json!({"repeat": "weekdays", "time": "07:00"}))
                .await
                .unwrap(),
            Some(Schedule::Weekdays {
                time: "07:00".into()
            })
        );
        assert_eq!(
            parse(json!({"repeat": "yearly", "date": "03-12"}))
                .await
                .unwrap(),
            Some(Schedule::Yearly {
                month: 3,
                day: 12,
                time: "09:00".into()
            })
        );
        assert_eq!(
            parse(json!({"repeat": "interval", "every_minutes": 45}))
                .await
                .unwrap(),
            Some(Schedule::Interval { minutes: 45 })
        );
        assert!(parse(json!({"repeat": "fortnightly"})).await.is_err());
        // An event needs a connected calendar.
        assert!(parse(json!({"event": "Dentist"})).await.is_err());
    }

    #[test]
    fn human_times() {
        let tz = zurich();
        let at = |s: &str| {
            tz.to_ambiguous_zoned(rules::parse_local(s).unwrap())
                .compatible()
                .unwrap()
                .timestamp()
                .as_millisecond()
        };
        assert_eq!(
            human_when(at("2026-10-02T20:00"), now(), &tz),
            "today at 20:00"
        );
        assert_eq!(
            human_when(at("2026-10-03T09:00"), now(), &tz),
            "tomorrow at 9:00"
        );
        assert_eq!(
            human_when(at("2026-10-05T07:00"), now(), &tz),
            "on Monday at 7:00"
        );
        assert_eq!(
            human_when(at("2026-10-20T07:00"), now(), &tz),
            "on 20 Oct at 7:00"
        );
    }
}
