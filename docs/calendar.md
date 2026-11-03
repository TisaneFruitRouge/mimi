# Calendar

The user's calendars: read, merged, and written to by the user from the Calendar panel
and by the assistant through its tools. The code lives in
`crates/core/src/connections/calendar/` (accounts, recurrence, guests and invitations,
the assistant's tools) and `crates/core/src/api/calendar.rs` (the panel's API). How the
panel looks and behaves is in [Frontend](frontend.md); reminders on events are in
[Reminders and routines](reminders-and-routines.md). Connections in general:
[Connections](connections.md).

## Rules

- **Sign in with Google** is the one registered app identity. The daemon owns the OAuth
  flow; a listener on 127.0.0.1 exists only during sign-in. Data goes only between the
  machine and Google; the refresh token stays in the connection config. Setup notes:
  [Google OAuth](google-oauth.md).
- Recurrence is expanded locally (`calendar/ics.rs`) for iCal and CalDAV, by Google
  itself for signed-in Google.
- CalDAV objects are written with ETags. One occurrence of a series is changed with an
  override or an `EXDATE` (`calendar/edit.rs`).
- **CalDAV calendars never email anyone.** Every guest is written with
  `SCHEDULE-AGENT=CLIENT`, and `edit::quiet` adds that to existing guests of the user's
  own events before any write or delete.
- **Only Google emails guests, and only for the user's own events.** Writes of the
  user's own events with guests pass `sendUpdates=all`; everything else passes
  `sendUpdates=none`. A trusted person approves their own requests, so their writes
  notify only when the user knows every guest (`tools::google_may_tell`,
  `mail::known`).
- Guests come as addresses, person ids, or a name only when it comes down to one address
  (`guests::resolve`): never a guess.
- Guests change only on events the user organizes (`CalendarEvent.mine`), one
  occurrence of a series at a time.
- Nothing is emailed by Mimi itself until the user clicks Send on an offer, or the
  assistant's `calendar_send_invitations` runs under the `send_mail` permission. Each
  offer is sent once. Writes answer with the offers they made.
- Adding, changing or deleting an event from the panel is the user's own action, so it
  needs no approval card. The assistant's `calendar_add_event`, `calendar_change_event`
  and `calendar_delete_event` do, unless Permissions allow them (see
  [Tools and approvals](tools-and-approvals.md)).
- Calendar ids are stable (`calendar::calendar_id`), and every `CalEvent` carries its
  `calendar_id`. Other features (permission exceptions, shared calendars, @ mentions,
  event reminders) keep them.
- Anything read from a calendar (titles, places, notes, attendees) is plain text in the
  UI: never linkified, never Markdown (see [Design system](design-system.md)).
- RSVPs aren't processed.
- Never log feed URLs, tokens or passwords, and never point tests at Google.

## Accounts

Three kinds of account (`Account`):

- **Google, signed in** (`google.rs`, integration `google`): OAuth 2.0 for installed
  apps. `POST /v1/google/sign-in` binds a listener on 127.0.0.1 (random port) for that
  sign-in only (10 minutes at most) and returns Google's consent page, with PKCE (S256),
  a random `state`, `access_type=offline` and two scopes: `calendar.events` and
  `calendar.calendarlist.readonly`. The client opens it with `openExternal`; the browser
  comes back to the listener, which turns away anything without the right `state`; the
  daemon exchanges the code with Google, lists the calendars and saves (or refreshes)
  the connection. Clients poll `GET /v1/google/sign-in/{id}` and may `DELETE` it.
  - The refresh token is in the connection's config; access tokens (an hour) are kept in
    memory (`FeedCache.google`). A refused refresh marks the connection `signed_out`
    ("Sign in again" in Connections, which reconnects the same row with `reconnect`,
    keeping its calendar ids). Disconnecting revokes the token with Google.
  - Events come from Google's own expansion (`singleEvents`), with the series' id as
    `uid`. Writes are insert/patch/delete on an occurrence's id (one) or the series'
    (all).
  - The app identity is `MIMI_GOOGLE_CLIENT_ID`/`_SECRET`, at build time
    (`option_env!`) or run time. Without one, `GET /v1/google/sign-in` says
    `available: false` and the dialog offers only the private address. See
    [Google OAuth](google-oauth.md).
- **Google through its private address** (`google_calendar`): Google's secret iCal
  address, read-only. New events open a pre-filled Google page; changes are refused with
  a plain explanation.
- **CalDAV** (`caldav.rs`, `edit.rs`), with app-specific passwords (iCloud, Fastmail,
  Nextcloud, Radicale). Calendars are discovered before the connection is saved. Objects
  are read with their ETag and written back with `If-Match`. A change or removal of one
  occurrence of a repeating event is an override (`RECURRENCE-ID`) or an `EXDATE`,
  written in the series' own form (date, UTC, named zone or floating); the whole event
  or series is changed in place or deleted. Moving every occurrence at once is refused
  (for Google too).

## Calendar ids and colours

Ids (`calendar::calendar_id`) are the connection id (Google through its address), or the
connection id plus a hash of the collection URL (CalDAV) or of Google's calendar id
(signed in). Colours (`CalendarInfo.color`) are the server's own (`calendar-color`) when
it has one, else picked from a fixed palette by id, so they never change between runs.

## The assistant's tools

`tools.rs`:

- `calendar_events` (a read): each event gets an `id`, its start plus a hash of
  calendar and uid; an @-mentioned event's `ev:` id works too (see
  [People › Mentions](people.md)).
- `calendar_add_event`, `calendar_change_event` and `calendar_delete_event` (`which`:
  this occurrence or all). Change and delete resolve the event before the card
  (`Tool::resolve`), so the card shows its real title, time and calendar, and they
  re-check that it's the same calendar when they run. They're governed by the
  `add_events` and `change_events` permission kinds; an event write that adds guests
  runs on its own only if every guest is known (see
  [Tools and approvals](tools-and-approvals.md)).
- `calendar_send_invitations`: sends an offer's email; it is sending mail (`send_mail`).

For someone the user trusts, the tools work on the accounts cut down to the calendars
shared with them (`calendar::restrict`): see [Trusted people](trusted-people.md).

## Guests and invitations

Events can have guests (`guests.rs`, `invite.rs`). Only Google ever emails them, on the
user's own Google events, so that an event shows in a guest's calendar as soon as
they're added.

### Google

`attendees` on insert and patch. A write of the user's own event that has guests
(before or after) passes `sendUpdates=all`: Google emails the invitation, the change or
the cancellation and puts the event in Google guests' calendars (as their "Add
invitations to my calendar" setting allows), exactly as saving with "Send" in Google
Calendar does. No offer is made; the answer's `note` says whom Google told
(`invite::told_note`), and the approval card's `email_note` says so beforehand.

Every other write passes `sendUpdates=none`, including a trusted person's when any guest
it reaches is someone the user doesn't know (`tools::google_may_tell` with
`mail::known`): they approve their own requests, so they mustn't make Google write to
strangers in the user's name. Adding guests the user doesn't know still asks first (the
known-recipients net in [Tools and approvals](tools-and-approvals.md)). Google replaces
the whole list on a patch, so guests who stay are given back exactly as Google had them
(answers included). The organizer is the calendar itself.

### CalDAV

`ORGANIZER` is the user's address for the account: the CalDAV username when it's an
address, else the first email account. Without either, an event is saved without its
guests and the answer says why. Each guest is an `ATTENDEE` with `CN`,
`ROLE=REQ-PARTICIPANT`, `PARTSTAT=NEEDS-ACTION`, `RSVP=TRUE` and
`SCHEDULE-AGENT=CLIENT` (RFC 6638: iCloud, Fastmail and Nextcloud then leave the
scheduling to the client). Guests who stay keep their line untouched (answer included).
Because a server would email guests it isn't told to leave alone, `edit::quiet` gives
`SCHEDULE-AGENT=CLIENT` to the other attendees of the user's own events on every write,
and before a delete (written first, then deleted).

### Who

Guests are given as addresses, person ids (from @ mentions) or names. A name counts only
when it comes down to one address (`guests::resolve`); otherwise the tool says so and
the assistant asks. Guests change only on events the user organizes (`guests::is_mine`,
`CalendarEvent.mine`: no organizer, or one of their addresses, or Google's `self`), and
one occurrence of a repeating event at a time (an override keeps the series' guests). A
trusted person gives guests only as email addresses.

### Offers

Every quiet write that could concern guests (CalDAV, or Google without `sendUpdates`)
leaves *offers* (`invite.rs`, table `calendar_invitations`, migration 0022): a snapshot
of the event and who could be told. The kinds are invite, update, cancel and uninvite:

- adding gives an invitation;
- a change gives the invitation to new guests, the new details to guests who stay (only
  if the title, time, place or notes changed) and a withdrawn invitation to guests taken
  off;
- removing gives a cancellation (without `RECURRENCE-ID` for a whole series);
- a change to every occurrence of a series makes no offer (its answer says so): the
  message would need the series' rules.

Nothing is sent until the user clicks (the Calendar panel's dialogs and toasts, or the
button on the chat's card; `POST /v1/calendar/invitations/{id}/send`, where the click is
the approval, like `/mail/send`) or the assistant's `calendar_send_invitations` runs
under the `send_mail` permission. The tools' results say plainly that nothing was
emailed, name the guests and tell the model to ask. "Send invitations…" on any event,
Google ones included, makes a fresh offer (`POST /v1/calendar/events/{id}/invitations`),
for sending Mimi's own email anyway. Trusted people are never offered invitations to
email (the owner can send them from the Calendar panel).

### Sending

Invitations go out through the user's own mail ([Email](email.md)): one iMIP message
(RFC 6047) per offer, to all its recipients. That keeps one copy in Sent of exactly what
everyone got, and calendar apps find their own `ATTENDEE` in it (Outlook and
Thunderbird send invitations this way).

- **From** the email account whose address is the organizer, else the first one (the
  offer's `from_note` says so).
- **Content**: plain text (what, when with the time zone, where, guests, notes) and the
  event as `text/calendar; method=REQUEST` or `CANCEL`, inline and again as
  `invite.ics` (`application/ics`, as Gmail sends it, so apps don't show it twice).
- **`SEQUENCE`** is the calendar's, but always above the last one sent for that UID and
  occurrence (`sent_sequence`): Google counts only new times, and guests must see every
  update as newer.
- **Once**: an offer is claimed before it's sent, so it goes out once. Filed in Sent like
  any other email.
- **Answers**: guests' replies to the invitation arrive as ordinary email and aren't
  processed; Google records answers from Google accounts itself.

## API

`api/calendar.rs`. All under `/v1`.

- `GET /v1/calendars` → `CalendarInfo` (stable id, name, colour, writable).
- `GET /v1/calendar/events?from&to` (ms, at most ~a year) → `CalendarEvents`: events of
  every calendar merged, recurrence expanded, plus the calendars that couldn't be read.
  ICS `ORGANIZER`/`ATTENDEE` are read (`mailto:` or the `EMAIL` parameter, `CN` as the
  name); organizer and guests are matched to People through email match keys.
- `POST /v1/calendar/events` (`NewCalendarEvent`) → `CreatedEvent { saved, open_url }`:
  CalDAV and signed-in Google calendars are saved; Google by address returns the
  pre-filled page to open.
- `PATCH|DELETE /v1/calendar/events/{id}`: one occurrence; `?which=all` deletes a
  series.
- `POST /v1/calendar/events/{id}/invitations`: a fresh offer for its guests.
- `GET /v1/calendar/invitations/{id}`, `POST /v1/calendar/invitations/{id}/send`.
- The Guests field suggests people from People by name or address
  (`GET /v1/people/emails?q=`, see [People](people.md#addresses-in-to-cc-and-guests)).
- `POST /v1/google/sign-in`, `GET /v1/google/sign-in` (`available`),
  `GET|DELETE /v1/google/sign-in/{id}`.

Writes answer with the offers they made; nothing is emailed.

## Tests

- **CalDAV**: Radicale (`uvx radicale --auth-type=none`), then
  `MIMI_TEST_CALDAV=http://127.0.0.1:5232/ cargo test -p mimi-core live_caldav --
  --ignored`. `live_caldav_adds_changes_and_removes_events` runs all of the CalDAV
  reading and writing against the real server.
- **Google**: `calendar/google_fake.rs` fakes sign-in, tokens and the Calendar API;
  `api/tests.rs › google_flow` covers sign-in, reads, writes, rules and revocation
  against it. Never point tests at Google.
- **iCal parsing**: public Google holiday feeds.
- **Guests and permissions**: `calendar_guests` (with the other permission tests listed
  in [Tools and approvals](tools-and-approvals.md)).
