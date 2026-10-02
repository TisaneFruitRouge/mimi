# Reminders and routines

The scheduler, in `crates/core/src/schedule/`. A **reminder** tells the user something
at the right time; a **routine** has the assistant do something on a schedule and report
back ("every morning at 7, send me my day"): it runs an instruction as a chat turn in its
own conversation and delivers the answer.

## Rules

- **Store rules, not instants.** `Schedule` (protocol) holds local wall-clock rules
  (`once`, `daily`, `weekdays`, `weekly`, `monthly`, `yearly`, `interval`,
  `before_event`); `next_at` is only a cache, recomputed by `rules::next_after` in the
  computer's current zone.
- **All date arithmetic goes in `rules.rs`**: pure, tested with fixed zones (DST gaps
  move forward, repeated hours use the first, short months clamp).
- **Go through the module.** Anything that changes an item must go through
  `schedule::create/update/delete/undo` (they re-arm and poke the loop); never write
  `schedule_items` elsewhere.
- **The loop takes the clock:** `tick(state, now)`, so tests can move it.
- **Catch-up never floods:** a due item fires once, marked late after 2 minutes; past 12
  hours it's recorded as missed and not sent. The next occurrence is computed after
  `max(due, now)`, so a week offline is one record, not seven reminders.
- **One slow channel never holds up the loop:** sending runs in spawned tasks; failures
  are logged, never fatal.
- **Routines keep the approval rule.** Their tool calls go through approvals like any
  chat (see [Tools and approvals](tools-and-approvals.md)). The run waits for them in its
  own task, never blocking the scheduler. A run whose previous run is still going is
  skipped, not queued.
- **Schedule tools need no approval** (the user's own local schedule), but every write
  returns `schedule_revision` and shows as a quiet line with Undo, like memory.
- **Tests never show a desktop notification** (`MIMI_NO_NOTIFICATIONS=1`).
- **Items for someone the user trusts** (`for_person`) reach only that person's chat,
  and their routines run as them.

## Schedules

An item stores a `Schedule` rule in local wall-clock terms:

- once at a date and time (`once`);
- `daily`, `weekdays`, `weekly` on chosen days;
- `monthly` (short months use their last day);
- `yearly` (29 February falls back to the 28th);
- every N minutes (`interval`, anchored to when it was set up);
- N minutes before a calendar event (`before_event`).

`next_at` is a cache computed by `rules::next_after` in the computer's current time zone,
so times stay right across DST (a skipped 02:30 moves to 03:30; a repeated hour uses the
first) and when the user travels (a zone change re-arms every future item).

## The loop

`schedule::run` is one task that sleeps until the earliest `next_at` or `snoozed_until`
(SQL `MIN` over active items), at most 60 s at a time because monotonic sleeps don't
advance while the computer is suspended. Creating, changing, pausing, snoozing or undoing
pokes it (`state.scheduler.poke()`, a `Notify`) to re-plan. No polling of the database
otherwise.

## Catch-up

When an item comes due, `rules::classify` compares the due time with now: on time, late
(more than 2 minutes: sent once, marked late), or missed (more than 12 hours: recorded in
the history, not sent). The next occurrence is computed after whichever is later, the due
time or now, so downtime never produces a burst of reminders. Routine runs left
"running" by a daemon that stopped are marked failed at startup.

## Following events

A `before_event` item keeps the event's id (calendar + uid + original start, as for @
mentions; see [People › mentions](people.md)) and its last known start. Every 15 minutes,
and again right before firing, it looks the event up (the occurrence closest to the last
known start, ±60 days):

- moved → the reminder moves (if it moved later just before firing, it waits);
- gone from a calendar that answered → the item ends with "The event was cancelled or
  removed.";
- calendar unreachable → the last known time stands.

## Delivering a reminder

Each channel is sent to in the background (spawned tasks; failures logged, never fatal):

- **Telegram**, to the paired owner, with Done / Snooze 10 min / 1 hour buttons
  (`done:` / `snooze:` / `snooze60:` callbacks). Only the owner's taps count; a second tap
  on a settled one answers "Already handled".
- **Signal**, in Note to Self (silently: Signal doesn't notify for Note to Self),
  answered by a reply ("done", "snooze 1h") or a reaction (✅, 💤).
- **Matrix**, in the owner's chat, answered by a reply or a reaction (✅, 💤).
- **A desktop notification** from the daemon (`notify.rs`, fail-soft: notify-rust over
  D-Bus on Linux, the notification center on macOS), when
  `Settings.desktop_notifications` is on. `MIMI_NO_NOTIFICATIONS=1` and tests never show
  one.
- **The app:** a `schedule_delivered` event (`ScheduleDelivered`: a toast with Done /
  Snooze) and the history.

Messaging apps are reached through `channels::owners`; prompts answered in text are
matched by `channels::replies` (see [Messaging apps](messaging.md)). Snoozing sets
`snoozed_until`; the snooze comes back through the same catch-up rules.

Where reminders reach the user is set in Settings › Reminders & notifications
(`#/settings/notifications`). The same page holds the new-mail notifications, which also
use `notify.rs` (`show_clickable`: text from outside, escaped for servers that read
markup, and a click heard on Linux); see [Email](email.md#new-mail-notifications).

## Routines

- **Their conversation:** each routine has its own, created on first run and titled
  with the routine's name.
- **A run** is a normal chat turn (`chat::send_with_context`) whose instruction is the
  user message, plus a hidden `<routine>` note telling the model it's a scheduled run.
  It gets the personality and custom instructions like any chat (see
  [Personality and instructions](personality.md)).
- **The answer** goes to every paired messaging app (Markdown as Telegram HTML, Signal
  text styles or Matrix HTML, split to fit) and to a desktop notification.
- **Approvals:** anything the model wants to send, change or delete still needs approval.
  Pending approvals are relayed to the messaging apps (Telegram as Approve / Don't
  buttons, Signal and Matrix as a prompt to answer yes or no by reply or reaction) and
  announced on the desktop. The run waits for them in its own task (up to 30 minutes),
  so reminders keep going meanwhile; a run that comes due while the previous one is
  still going is skipped.

## Items for someone the user trusts

An item set up for someone the user trusts (`for_person`, migration 0026, created with
`schedule::create_for`) goes only to that person's chat (answered there by reply or
reaction), with no notification, toast or history for the user. Their routines run as
them, in a conversation of theirs, and their approvals follow their card. The schedule
tools give a trusted person only their own items (`schedule/tools.rs › belongs`), and an
event to time one to is looked for only in the calendars shared with them. The owner's
lists, history and calendar leave them out, and the API answers 404 for them. Turning
their access off or deleting the person deletes them. See
[People the user trusts](trusted-people.md).

## Tools

For the model (`schedule/tools.rs`):

- `reminder_add` and `routine_add`. When: `at`, `in_minutes`, `repeat` with
  `time`/`days`/`day_of_month`/`date`/`every_minutes`, or `event` with
  `minutes_before`.
- `schedule_list`.
- `schedule_change`: partial changes merge with the existing rule ("make it 8:00" keeps
  "every weekday").
- `schedule_cancel`.

Setting up the user's own schedule is local and private, so there's no approval card by
default (the `schedule` permission kind is automatic; see
[Tools and approvals › Permissions](tools-and-approvals.md#permissions)). Each write
returns `schedule_revision` and shows as one quiet line with Undo (`schedule_revisions`
keeps the previous state; `POST /v1/schedule/undo/{revision}`).

## In the app

Reminders and routines live in the Calendar panel (see [Calendar](calendar.md) and
[Frontend](frontend.md)); old addresses such as `#/reminders` are rewritten to their new
place.

- **Beside the grid**, a side list: what's coming up in plain words ("Tomorrow at 9:00 ·
  Every weekday at 9:00"), with edit, pause, run a routine now and delete.
- **Inside the grid**: timed items as a chip at every time they go off in view (past ones
  greyed, missed ones struck through, frequent ones once a day), event-relative ones as a
  bell on their event.
- **Menus:** an event's sheet has "Remind me before…" (a `before_event` reminder on that
  occurrence); its right-click menu has "remind me before" (upcoming events only) and
  "remove its reminders"; a reminder or routine has `ItemMenu`; an empty slot of the week
  grid offers "new reminder".
- Settings › Reminders & notifications (`#/settings/notifications`): where they're
  delivered.

## In a calendar: occurrences

`GET /v1/schedule/occurrences?from&to` gives every time items go off in a range (ms, at
most 400 days, a week from now by default), sorted by time:

- what already happened comes from the history (`schedule_deliveries` with `due_at` in
  range, with its delivery status);
- what's to come is expanded by `rules::occurrences` (repeated `next_after`, so the same
  DST and short-month rules) from the item's `next_at` onwards, so nothing before it
  (created later, paused meanwhile) is invented;
- paused and ended items have nothing to come;
- a pending snooze is one more entry (`snoozed`).

Intervals under two hours are folded to one entry per local day (`at`, `until`,
`count`), and each item gives at most 500 entries, so "every minute" over a year is 365
entries, not half a million.

## API

`GET|POST /v1/schedule`, `PATCH|DELETE /v1/schedule/{id}`, `POST /v1/schedule/{id}/run`,
`GET /v1/schedule/deliveries`, `POST /v1/schedule/deliveries/{id}/done`, `POST
/v1/schedule/deliveries/{id}/snooze`, `POST /v1/schedule/undo/{revision}`, `GET
/v1/schedule/occurrences?from&to`. Changes publish `schedule_changed`; deliveries publish
`schedule_delivered`.

## Schema

`schedule_items` (with `for_person`, migration 0026), `schedule_deliveries` (history),
`schedule_revisions` (undo).

## Tests

- `rules.rs` is pure and tested with fixed zones (DST gaps, repeated hours, short
  months).
- `tick(state, now)` lets tests move the clock.
- To try it for real: a scratch daemon (see [Development](development.md)) with
  `MIMI_NO_NOTIFICATIONS=1` and `MIMI_TELEGRAM_API` pointed at a fake bot server; "remind
  me in 2 minutes to …" fires on the minute.
