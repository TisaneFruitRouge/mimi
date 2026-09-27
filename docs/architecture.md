# Architecture

```
 ┌──────────────┐  ┌──────────┐  ┌──────────┐   ┌─────────────┐
 │ Desktop app  │  │   CLI    │  │ TUI (tbd)│   │  Browser    │  clients
 │ Tauri + React│  │ `mimi` │  │          │   │ same React  │
 └──────┬───────┘  └────┬─────┘  └────┬─────┘   └──────┬──────┘
        │   mimi-client (typed Rust client)          │ fetch + WebSocket,
        └───────────────┼─────────────┘                │ session cookie
                        │ HTTP on 127.0.0.1:7437,      │
                        │ bearer token                 │
                ┌───────┴──────────────────────────────┴┐
                │    mimid     │      mimi-core
                │ agent · memory │
                │ models · tools │
                │ scheduler      │
                └────────────────┘
```

## Crates

| Path | Crate | Role |
|---|---|---|
| `crates/protocol` | `mimi-protocol` | API types and on-disk paths shared by the daemon and every client. |
| `crates/core` | `mimi-core` | The daemon (`mimid` binary) and all assistant logic. |
| `crates/client` | `mimi-client` | Typed client for the daemon API. Every frontend uses it. |
| `crates/cli` | `mimi-cli` | The `mimi` command. The TUI will live here too. |
| `crates/service` | `mimi-service` | Finds `mimid` and runs it as a login service (systemd, launchd, XDG autostart). Shared by the app and the CLI. |
| `apps/desktop/src-tauri` | `mimi-desktop` | Tauri shell. Exposes Tauri commands that call `mimi-client`. |
| `apps/desktop/src` | `@mimi/desktop` | React + Tailwind + shadcn/ui frontend. |

## Web interface

The daemon serves the built frontend (`apps/desktop/dist`, via `rust-embed`) at `/`,
with a fallback to `index.html` for client-side routes. Static files are public;
everything under `/v1` requires auth. The frontend picks its transport at runtime
(`src/lib/transport.ts`): Tauri IPC inside the desktop app, same-origin `fetch` and a
WebSocket in a browser.

Browsers can't read the discovery file, so they sign in with a link:

1. A native client (bearer token) calls `POST /v1/web/login-link` and opens the URL
   (`mimi open`, or the desktop app's `open_in_browser` command).
2. `GET /login?code=…` redeems the single-use code (valid two minutes, kept in memory),
   creates a session, sets `mimi_session` (HttpOnly, SameSite=Strict, 30 days) and
   redirects to `/`. The database stores only the SHA-256 of the session token.
3. Cookie-authenticated requests must have Host `127.0.0.1:<port>` or
   `localhost:<port>` (DNS-rebinding defense). Non-GET requests and the `/v1/events`
   upgrade must also send a matching Origin (CSRF defense). Sessions can't mint login
   links. `POST /v1/web/logout` ends the session.

Known limit: browsers share cookies across ports of the same host, so another web app
on `127.0.0.1` could receive the cookie. Such software already runs as the user, so
this doesn't widen what it can reach, but remote access will need a proper origin and
TLS.

## Daemon discovery and auth

On start, `mimid`:

1. Creates the data dir with mode `0700`.
2. Refuses to start if another instance's discovery file points at a live port.
3. Binds `127.0.0.1:7437` (`MIMI_PORT` overrides; a random port if it's taken) and
   generates a 256-bit token.
4. Writes `daemon.json` (`pid`, `port`, `token`) to the data dir with mode `0600`, atomically.
5. Deletes `daemon.json` on SIGINT/SIGTERM.

Clients read `daemon.json` and send `Authorization: Bearer <token>`. Outside `/v1`,
only `GET /health` (the version, nothing else), `GET /login` and the static frontend
files are public. Everything under `/v1` needs the token or a browser session (see
above).

The desktop webview never sees the token: the frontend calls Tauri commands, and the
Rust side makes the request.

`MIMI_HOME` overrides all paths, which lets you run isolated instances in development.

## Tools and approvals

A reply is a loop of model rounds (`chat::Turn`). Each round streams from the model
with the available tools offered (`tools::ToolSources` → a per-reply `ToolRegistry`).
When the model asks for tools, each call becomes an `Action` on the assistant message:

- **Reads** run immediately.
- **Anything that sends, changes or deletes** waits as `pending_approval`. The reply
  pauses until `POST /v1/actions/{id}/approve` (optionally with edited arguments) or
  `/reject`. A declined action doesn't run, and the model is told so.
- Results go back to the model as `role: "tool"` messages, and the next round starts,
  up to 8 rounds. After that the model is asked once more without tools, so it must
  answer in words.

Details:

- **State updates:** action changes reach clients as `message_updated` events.
- **Position in the text:** each action records `content_offset`, how far into the
  reply's text it happened, so clients show text and cards in order.
- **History:** history replays past tool calls and results, so follow-ups work.
- **Stopping:** cancelling a reply while it waits for approval fails the action
  without running it.
- **Restarts:** actions that were pending or running are marked failed. They never
  run later.
- **Models without tool support:** some models reject the `tools` parameter. The
  reply is retried once as plain chat.

## Memory

`crates/core/src/memory/`. What the assistant knows about the user, built to stay
small in the prompt however much accumulates. It works like a tiny file system:

| Tier | What | In the prompt |
|---|---|---|
| Profile (`profile.md`) | The few key facts about the user: name, city, language, work, household. Capped at 1,200 characters. | Always |
| Library (`people/sam.md`, `habits/mornings.md`, `preferences/food.md`, `places/…`, `work/…`, `interests/…`, `health/…`, `notes/…`) | Short markdown notes, usually bullet facts, one per person or topic (4,000 characters max each). | Only what's relevant |

- **Storage:** the `memory_notes` table in the encrypted database, with an FTS5 index
  (`memory_fts`, `unicode61 remove_diacritics`) kept in step by triggers. Every change
  first records the note's previous state in `memory_revisions`, so any change can be
  undone.
- **Recall, every message** (`recall.rs`): notes come from three places, merged
  best first:
  1. **People:** notes linked to anyone the message @-mentions or names (full name,
     nickname, or a first name only one person has) come first.
  2. **Words:** the user's message (plus their previous one, for follow-ups) becomes an
     FTS query (stopwords dropped, longer words matched as prefixes).
  3. **Meaning** (when the user turned it on): the message's embedding against every
     note's, by cosine similarity; a note counts at 0.6 or more and within 0.08 of the
     best match.

  Words and meaning are merged by reciprocal rank fusion (`1/(60 + rank)` per list), so a
  note found both ways ranks highest. The profile and up to 4 notes, 1,600 characters at
  most, go into the system prompt inside a delimited `<memory>` block that's labelled as
  data, not instructions.
- **Finding by meaning** (`semantic.rs`, optional, off by default,
  `Settings.memory_semantic`): a small multilingual embedding model (IBM Granite
  Embedding 278M, Q6_K GGUF, 236 MB, 768 dimensions, Apache-2.0; English, French,
  German, Spanish and eight more languages) is pinned in `catalog.json` under
  `embeddings` and downloaded like chat models. It runs as a second `llama-server
  --embedding` on the CPU (loopback, random port, its own key file), started on demand
  and stopped after 20 idle minutes; with no built-in runtime, the user's Ollama
  (`granite-embedding:278m`, never a cloud source) serves `/v1/embeddings` instead.
  Vectors are little-endian f32 blobs in `memory_vectors` (one per note, with the model
  and a hash of the embedded text), compared by brute force. A background task
  (`semantic::run`) embeds new and changed notes on `memory_changed` (backfill included)
  and re-embeds everything if the model changes; deleted notes take their vector along
  (foreign key). If the model is off, missing or slow (6 s for a message, start
  included), recall carries on with words alone. The Memory screen shows it as
  "Understands meaning" with the download size and progress.
- **Tools:** `memory_search`, `memory_read` and `memory_list` for anything not already
  recalled, and `memory_write` (add facts, de-duplicated), `memory_update` (rewrite a
  note) and `memory_forget` (remove facts or a note). None needs approval, since memory is
  local and private. Writes show in the chat as one quiet line ("Remembered that Sam is
  your brother") with Undo (`POST /v1/memory/undo/{revision}`). Reads aren't shown.
- **Learning in the background:** after a reply, a conversation is scheduled to be
  learned from once it's been quiet for 2 minutes (`MIMI_MEMORY_QUIET_SECS`).
  - **The pass:** the active model gets the current profile, the list of notes, the
    notes related to the conversation, and the new messages since the last pass
    (`memory_learned`). It replies with a JSON plan of facts to add or remove per note,
    which is applied with de-duplication and the profile cap.
  - **No thinking:** the pass asks with `ChatOptions::QUICK` (see Models), and the plan
    starts with a short `facts` list, which keeps non-thinking models from skipping
    people or details. With Qwen3 8B through Ollama a pass takes 15-25 s instead of
    40 s to over 2 minutes.
  - **Sources:** only the user's own messages count as sources; the assistant's replies
    are context.
  - **Exclusions:** a message where the user asks not to remember something is left out
    before the model sees it.
  - **Restarts:** unread conversations are picked up after a restart.
- **Never stored:** anything that looks like a secret (passwords, codes, card or
  account numbers, keys). This is checked on every write path, including the Memory
  screen.
- **The user in control:**
  - The Memory screen (`#/memory`, from Settings) shows and edits the profile and every
    note.
  - "Learn from conversations" can be paused (`Settings.memory_learning`). Paused, the
    writing tools disappear and nothing said meanwhile is learned later.
  - "Forget everything" deletes it all.
  - When a cloud model is active, the screen says that relevant memories are sent with
    messages.
- **People** (`link.rs`): knowledge about people lives in `people/<name>.md`. A note is
  linked to someone in the people directory (`subject` = their id) when it's clear who:
  the file name or title matches their full name or nickname, or only one person has
  that first name, or the user @-mentioned them in the conversation the note was learned
  from (which settles two Léas), or the note spells out the full name. Ambiguous notes
  stay unlinked rather than guess. Links are made after each learning pass and on every
  memory or people change, and redone when a person is deleted or merged away.
  `GET /v1/people/{id}/memory` lists a person's notes; the Memory screen shows the
  linked person on each note.
- **API:** `GET /v1/memory`, `GET|PUT|DELETE /v1/memory/note?path=`,
  `PUT /v1/memory/profile`, `PUT /v1/memory/learning`, `PUT /v1/memory/semantic`
  (turning it on starts the download), `POST /v1/memory/undo/{revision}`,
  `POST /v1/memory/forget-all`, `GET /v1/people/{id}/memory`. Changes publish
  `memory_changed`.

## Reminders and routines

`crates/core/src/schedule/`. Reminders tell the user something at the right time;
routines have the assistant do something on a schedule and report back ("every morning
at 7, send me my day").

- **Rules, not instants.** An item stores a `Schedule` rule in local wall-clock terms:
  once at a date and time, daily, weekdays, weekly on chosen days, monthly (short months
  use their last day), yearly (29 February falls back to the 28th), every N minutes
  (anchored to when it was set up), or N minutes before a calendar event. `next_at` is a
  cache computed by `rules::next_after` in the computer's current time zone, so times
  stay right across DST (a skipped 02:30 moves to 03:30; a repeated hour uses the first)
  and when the user travels (a zone change re-arms every future item).
- **The loop.** One task sleeps until the earliest `next_at` or `snoozed_until` (SQL
  `MIN` over active items), at most 60 s at a time because monotonic sleeps don't advance
  while the computer is suspended. Creating, changing, pausing, snoozing or undoing
  pokes it (`Notify`) to re-plan. No polling of the database otherwise.
- **Catch-up.** When an item comes due, `rules::classify` compares the due time with now:
  on time, late (more than 2 minutes: sent once, marked late), or missed (more than 12
  hours: recorded in the history, not sent). The next occurrence is computed after
  whichever is later, the due time or now, so downtime never produces a burst of
  reminders. Routine runs left "running" by a daemon that stopped are marked failed at
  startup.
- **Following events.** A `before_event` item keeps the event's id (calendar + uid +
  original start, as for @ mentions) and its last known start. Every 15 minutes, and
  again right before firing, it looks the event up (the occurrence closest to the last
  known start, ±60 days): moved → the reminder moves (if it moved later just before
  firing, it waits); gone from a calendar that answered → the item ends with "The event
  was cancelled or removed."; calendar unreachable → the last known time stands.
- **Delivery of a reminder** (each channel in the background, failures logged, never
  fatal):
  - Telegram, to the paired owner, with Done / Snooze 10 min / 1 hour buttons. Only the
    owner's taps count; a second tap on a settled one answers "Already handled".
  - A desktop notification from the daemon (notify-rust over D-Bus on Linux, the
    notification center on macOS), when `Settings.desktop_notifications` is on.
  - The app: a `schedule_delivered` event (toast with Done / Snooze) and the history.
  Snoozing sets `snoozed_until`; the snooze comes back through the same catch-up rules.
- **Routines.** Each has its own conversation, created on first run and titled with the
  routine's name. A run is a normal chat turn (`chat::send_with_context`) whose
  instruction is the user message, plus a hidden `<routine>` note telling the model it's
  a scheduled run. The answer goes to Telegram (Markdown converted to Telegram HTML, split
  to fit) and to a desktop notification. Anything the model wants to send, change or
  delete still needs approval: pending approvals are relayed to Telegram as Approve /
  Don't buttons and announced on the desktop. The run waits for them in its own task (up
  to 30 minutes), so reminders keep going meanwhile; a run that comes due while the
  previous one is still going is skipped.
- **Tools** for the model: `reminder_add` and `routine_add` (when: `at`, `in_minutes`,
  `repeat` with `time`/`days`/`day_of_month`/`date`/`every_minutes`, or `event` with
  `minutes_before`), `schedule_list`, `schedule_change` (partial changes keep the rest of
  the rule) and `schedule_cancel`. Setting up the user's own schedule is local and
  private, so no approval card: each write shows as one quiet line with Undo
  (`schedule_revisions` keeps the previous state; `POST /v1/schedule/undo/{revision}`).
- **Screen:** `#/reminders`, from the bell in the top bar or ⌘K: what's coming up in
  plain words ("Tomorrow at 9:00 · Every weekday at 9:00"), pause, edit, delete, run a
  routine now, add by hand, recent deliveries, and where they reach the user.
- **API:** `GET|POST /v1/schedule`, `PATCH|DELETE /v1/schedule/{id}`,
  `POST /v1/schedule/{id}/run`, `GET /v1/schedule/deliveries`,
  `POST /v1/schedule/deliveries/{id}/done`, `POST /v1/schedule/deliveries/{id}/snooze`,
  `POST /v1/schedule/undo/{revision}`. Changes publish `schedule_changed`; deliveries
  publish `schedule_delivered`.

## Email

Mail is read over IMAP and sent over SMTP with the account's app password
(`crates/core/src/mail/`). There is no registered app and nothing between the user's
computer and their mail service. Outlook.com needs a Microsoft sign-in and isn't
supported yet; the connect form says so rather than failing.

**Connecting.** `ConnectionSetup::Email { email, password, preset, servers }`. The preset
(iCloud, Gmail, Fastmail, Proton via Bridge, Other) is guessed from the address when
not given. `mail::connect` signs in to IMAP (selecting INBOX) and to SMTP before the
connection is saved; iCloud's IMAP login falls back to the part before the @. The
config (address, password, servers) lives in the connection row, in the encrypted
database. TLS is verified against the OS trust store; plain connections and
self-signed certificates are only accepted for loopback servers (Proton Bridge).

**Sync.** Each email connection runs `sync::run` as its connection task:

1. Sign in, then a *pass* over Inbox, Sent and Archive (found by special-use
   attributes, else by name; Spam and Trash are never read). Per mailbox:
   - `SELECT`; a UIDVALIDITY different from the stored one drops the mailbox's local
     copy and starts over.
   - New messages: first pass `UID SEARCH SINCE <90 days ago>` (newest 2,000), later
     `UID SEARCH UID <last+1>:*`. Sizes first, then bodies in chunks of 25 (`BODY.PEEK[]`,
     so reading doesn't mark mail read); over 2 MB only the headers.
   - Flags and removals: `UID FETCH <min>:<max> (UID FLAGS)` over what's stored.
   - Mail older than 97 days is forgotten; the high-water mark becomes
     `max(highest UID seen, UIDNEXT-1)`.
2. `IDLE` on the Inbox for up to 10 minutes, until the server reports news or the app
   pokes the loop (after sending, archiving or "Check for new mail"). Before idling, a
   `UIDNEXT` beyond the stored mark (mail that arrived during the pass) triggers another
   pass instead. Servers without IDLE are polled every 2 minutes.
3. Errors drop the connection and retry with backoff (30 s doubling to 15 min). A
   refused login marks the connection as needing attention and waits 30 minutes, so a
   revoked password can't lock the account.

Actions (mark read, archive, filing a sent copy) open a short second session. Archive
uses `MOVE` (or COPY + `\Deleted` + EXPUNGE) to the Archive mailbox, or Gmail's All
Mail. Sent mail is appended to Sent except on Gmail and Proton, which file it
themselves.

**Storage and threading.** `mail_threads` (subject, last activity, category, summary,
`sorted_at`), `mail_messages` (per mailbox and UID: headers, plain-text body, snippet,
flags, attachment names) with `mail_fts` for search, and `mail_sync` (UIDVALIDITY and
high-water mark per mailbox). A message joins the thread of its In-Reply-To or
References, or of a message that already refers to it, else (for "Re:"-style subjects)
a thread from the last 30 days with the same subject and a shared participant. The
same Message-ID in two mailboxes (a reply in Sent and Inbox) shows once.

**Untrusted content.** Bodies are plain text: HTML goes through `parse::strip_hidden`
(elements hidden by `display:none`, `visibility:hidden`, zero opacity or size,
`mso-hide`, the `hidden` attribute; comments, scripts, styles; text whose inherited font
size is under 2px) and html2text, and zero-width characters are removed. Tools label
mail as data; the only tool that acts, `mail_send`, always needs approval and its card
shows the whole message. Model calls about mail (sorting, summaries, reply drafts) get
no tools; their answers are shown to the user or parsed into a fixed shape (a category
from three and one clipped line). Memory learning only reads the user's own messages,
so nothing from an email becomes a memory. `api/tests.rs` › `mail_flow` has a hostile
email and a model that obeys it: the send waits on the approval card and nothing is sent
when it's declined.

**Sorting.** `triage::run` wakes after a pass that changed something (and every 5
minutes). While `Settings.mail_sorting` is on and a model is set up, it takes the newest
unsorted Inbox thread from the last 14 days, one at a time, waiting while a chat reply
is being written. Automatic mail (List-Unsubscribe, List-Id, Precedence bulk/list,
Auto-Submitted, no-reply-style senders) is filed as "other" without the model; anything
else is sent to the model with `ChatOptions::QUICK` (no thinking) and gets needs_reply,
important or other plus a one-line summary. A thread with a newer message is sorted
again. If the model fails, the queue stops until the next wake-up.

**People.** `mail::contacts::Correspondents` offers, per account, everyone the user
wrote to and every human sender with at least two messages, as cards with one email
handle (record id = the address). The directory merges them only on that address.

**API.** `GET /v1/mail` (accounts, counts, whether sorting is on, the model's locality),
`GET /v1/mail/presets`, `GET /v1/mail/threads?view=&q=&person=&before=&limit=`,
`GET /v1/mail/threads/{id}`, `POST /v1/mail/threads/{id}/read` (`{read}`),
`/archive`, `/summarize`, `/draft` (`{instructions}`), `POST /v1/mail/send` (a
`MailDraft`; the panel's Send button, which is the user's own action), `POST
/v1/mail/refresh`. Changes publish `mail_changed`.

**UI.** The Mail panel (`features/mail/mail-view.tsx`) has three columns: views ("Sorted
for you": Needs a reply, Important, Everything else; mailboxes: Inbox, Sent, Archive,
with the sorting model's locality), the conversation list with one-line summaries and
search across all mail, and the reader (Reply, Summarize, Archive, Mark as unread, Ask
the assistant; a reply box that can draft the answer with the model; Send only by the
user). Drafts the assistant writes in chat appear as editable cards with their own Send
button (`draft-card.tsx`); `mail_send` approval cards show every field.

## People and @ mentions

`crates/core/src/people/` keeps one directory of people, unified across sources.

- **Model:** a person (`people`) has contact cards from sources (`person_records`:
  source = connection id, record = the card's id there) and handles
  (`person_handles`: channel phone/email/telegram/signal/whatsapp/matrix, value, label,
  and the card it came from, or none when the user added it). People the user adds are
  `manual` and survive syncs. Person ids are stable (merges keep the first id), so
  other features can link to them, such as memory notes' `subject`.
- **Sources:** `ContactSource` implementations return every card, per connection;
  `people::sync_all` works out what changed. Today: address books (CardDAV) on connected
  CalDAV accounts (iCloud → contacts.icloud.com, Fastmail → carddav.fastmail.com,
  others on the same server). Syncs run at startup, every 30 minutes, when connections
  change, and on `POST /v1/people/sync`. A removed connection takes its cards along.
- **Unification:** a new card joins the person who already has one of its match keys
  (email lowercased; phone digits with `+`/`00` as international, national numbers kept
  as they are; Signal/WhatsApp numbers are phones; Telegram usernames). Never on name:
  same-name people become "possible duplicates" (`GET /v1/people/duplicates`), which
  the user merges or dismisses. A card, once placed, stays with its person, so a split
  (`POST /v1/people/{id}/split`) survives later syncs.
- **Mentions:** `GET /v1/mentions?q=` suggests people and events (upcoming 30 days;
  with a query, the past month to six months ahead). Event ids encode calendar, uid and
  start (`ev:<ms>:<hex calendar>:<hex uid>`). `SendMessage.mentions` keeps those whose
  `@label` is still in the text; the engine resolves them into a `<mentioned>` block
  appended to the user's message for the model (who they are and every way to reach
  them; the event's time, calendar and place), marked as data rather than
  instructions. The block is stored with the message (`mention_context`) and replayed
  in later turns.
- **Assistant tools:** `people_search` and `person_details`, reads without approval.
- **UI:** the composer is still a textarea. Typing `@` opens the picker
  (`features/chat/mentions/`), picked items become `@label` text tracked as tokens and
  drawn as pills behind the text, and a token deletes as a whole. People is a top-bar
  panel (`#/people/<id>`); see below.

## Panels and navigation

The top bar holds the panels used every day: **Chat** (home), **Calendar**, **Mail**
and **People** (⌘/Ctrl 1–4). What is set up once is in the **Settings** window
(`#/settings/<page>`: General, Connections, Models, Memory, Reminders & notifications,
Privacy), a sidebar like the system's own settings. Every panel can hand something to
Chat with "Ask … about this": `lib/draft.ts` holds a pending draft (text plus its
`Mention`s), the shell opens a new chat, and the composer takes the draft on mount, so
the thing arrives as an @ pill the engine resolves like any other mention.

- **Calendar** (`features/calendar/`): a week grid (all-day row, overlapping events side
  by side, the "now" line) or a four-week list, every calendar merged and coloured, with
  calendars shown or hidden per viewer. Reminders and routines sit beside it (edit,
  pause, run now, delete) and inside it: timed ones as chips at their time,
  event-relative ones as a bell on their event. An event's sheet shows when, where, who
  (matched to People), the invitation's notes as plain text, "Remind me before…"
  (a `before_event` reminder on that occurrence) and "Ask … about this". New events go
  straight into CalDAV calendars; for Google ones the pre-filled Google page opens.
  - `GET /v1/calendars` → `CalendarInfo` (stable id, name, colour, writable). Ids are
    the connection id (Google) or connection id + hash of the collection URL (CalDAV).
    Colours are the server's `calendar-color` when it has one, else picked from a fixed
    palette by id, so they never change between runs.
  - `GET /v1/calendar/events?from&to` → `CalendarEvents` (events plus the calendars that
    couldn't be read). ICS `ORGANIZER`/`ATTENDEE` are read (`mailto:` or the `EMAIL`
    parameter, `CN` as the name) and matched to People through email match keys.
  - `POST /v1/calendar/events` (`NewCalendarEvent`) → `CreatedEvent { saved, open_url }`.
- **People** (`features/people/`): master–detail. The list (search, possible
  duplicates) on the left; a person's page shows how to reach them, what memory holds
  about them (`GET /v1/people/{id}/memory`), what's coming up with them
  (`GET /v1/people/{id}/events`: events they're invited to by email, or whose title names
  them; recurring ones once), recent email (`GET /v1/mail/threads?person=<id>`), and
  chats where they were @-mentioned (`GET /v1/people/{id}/conversations`). Sections
  whose API isn't there (older daemon, no mailbox) say so quietly or stay hidden.
- **Mail** (`features/mail/mail-view.tsx`): owned by the email feature.

## Running the daemon

The user never starts `mimid` by hand. Who runs it:

| Owner | When | Stops when |
|---|---|---|
| The background service | "Keep Mimi running in the background" is on | The switch is turned off (the app then runs it itself) |
| The desktop app, as a child | The switch is off | The app quits |
| Someone else | `pnpm dev` (`MIMI_DAEMON=external`), or a daemon started by hand | Its owner stops it |

On launch the app (`apps/desktop/src-tauri/src/daemon_process.rs`): uses a daemon that
already answers; else starts the installed service; else spawns `mimid` as a child
(logging to `daemon.log` in the data directory), restarts it if it crashes (at most 5
times a minute) and stops it with SIGTERM on quit. With `MIMI_DAEMON=external` it only
connects. Only one daemon runs per data directory, so turning the switch on first stops
the app's child, then installs and starts the service.

Finding the binary (`mimi_service::daemon_binary`): `MIMI_DAEMON_BIN`, else `mimid`
next to the running executable (the bundled Tauri sidecar, or `target/<profile>/` in
development), else `PATH`.

The service (`crates/service`), one per data directory:

- **Linux, systemd:** user unit `~/.config/systemd/user/mimi.service`,
  `Restart=on-failure`, enabled for login with `systemctl --user enable --now`.
- **Linux without systemd:** XDG autostart entry `~/.config/autostart/mimi.desktop`
  (starts at login, nothing restarts it after a crash; the switch's text says so).
- **macOS:** LaunchAgent `~/Library/LaunchAgents/dev.mimi.daemon.plist`, `RunAtLoad`,
  `KeepAlive` on unsuccessful exit, logs to `daemon.log`.
- A non-default data directory (`MIMI_HOME`: development, tests) gets its own name,
  `mimi-<hash>` / `dev.mimi.daemon.<hash>`, so it can never replace the real install.
  `MIMI_HOME`, `MIMI_PORT` and `MIMI_KEY_STORE` are carried into the service only when set.

The tray (`tray.rs`) offers Open Mimi, a status line and Quit Mimi. In background mode,
closing the window hides it to the tray; quitting leaves a service-run daemon running.
Linux trays need libayatana-appindicator at runtime; without it there's no tray and
closing the window quits the app (the service, if on, keeps the assistant running).
Release builds are single-instance: launching again brings the window forward.

## Models and the built-in runtime

Model sources are OpenAI-compatible endpoints (Ollama, LM Studio, a server on the
network, a cloud service) plus one that is part of Mimi: **Built into Mimi**, backed by
llama.cpp's `llama-server`, which ships with the installers. Nobody has to install a
model runner.

```
 chat / memory learning
        │ providers::chat_client(source, model)
        ├── OpenAI-compatible source ──► its URL (Ollama, LM Studio, cloud, …)
        └── built-in source ──► Runtime::ensure(model)
                                  │ start or reuse llama-server:
                                  │ 127.0.0.1:<random port>, per-start API key file,
                                  │ context by hardware tier, GPU layers auto
                                  └► http://127.0.0.1:<port>/v1
```

- **Internal jobs don't think.** `OpenAiCompatible::complete(model, messages,
  ChatOptions::QUICK)` returns a whole answer without the model's reasoning, for
  background work (memory learning, mail triage); `stream_chat_with` takes the same
  `ChatOptions { thinking }` for streaming. With thinking off, local sources (built-in,
  device or network) get `reasoning_effort: "none"` (what Ollama's OpenAI endpoint
  honours; `think` is ignored there) and `chat_template_kwargs.enable_thinking: false`
  (llama.cpp with Qwen3-style templates); a server that rejects them gets the request
  again without. Cloud sources never get the fields. Chats keep thinking.
- **One model at a time.** `ensure` reuses the running server when it serves the same
  model and is alive, else stops it and starts a new one, then waits for `/health`
  (large models can take a while to load; the chat shows the reply as pending). It's
  started lazily on the first message, stopped after 20 idle minutes (never during a
  reply), when its model is deleted, and when the daemon exits. A crashed server is
  restarted by the next message. Output goes to `logs/model-runtime.log`.
- **Security.** It listens on loopback only, on a random port, and requires a random
  key generated at every start, written to an owner-only file (`--api-key-file`, so the
  key isn't visible in the process list). The web UI is disabled (`--no-webui`) and it
  never fetches anything itself (`--offline`).
- **Context size** follows the hardware tier: 4k (minimal), 8k (light), 16k (standard),
  32k (strong, workstation).
- **GPU.** macOS builds use Metal. Linux builds use Vulkan, which covers AMD, Intel and
  NVIDIA; ggml loads the Vulkan backend as a plugin and falls back to its CPU backends
  (picked for the CPU's instruction set) when there's no Vulkan driver.
- **Downloads.** Every catalog model pins a single GGUF file on Hugging Face (repo,
  file, size, SHA-256). The daemon downloads it directly from huggingface.co into
  `models/`, as `<file>.part` until complete. Stopping or a failure keeps the part;
  starting again resumes with an HTTP Range request and re-hashes what's there. The
  file only takes its real name once its SHA-256 matches, and there must be room for
  the rest plus a margin before a download starts. Progress, stop and resume use the
  same `ModelPull` events as Ollama pulls. Deleting a model removes the file (refused
  while it's the default).
- **Finding the binary** (`runtime::find_binary`): `MIMI_LLAMA_SERVER`, beside `mimid`
  (`llama/llama-server`, `llama-server`), the bundle's resources (`Mimi.app/Contents/
  Resources/llama/`, `/usr/lib/Mimi/llama/`, `$APPDIR/usr/lib/Mimi/llama/`), then
  `PATH`. Without one there's no built-in source and Mimi recommends Ollama instead.

## First run

The onboarding (`apps/desktop/src/features/onboarding/`) replaces the old setup mode:

1. **Welcome**: what Mimi is and the privacy promise.
2. **Model**: a plain-language summary of this computer, models already on it, the
   recommended downloads for it (built-in runtime, else Ollama), local servers found
   running, or a cloud service (with the trade-off said plainly; recommended first on
   weak machines).
3. **Connections** (optional): calendar and Telegram, through the Connections dialogs.
4. **About you** (optional): name, place and free text, appended to the memory profile.
5. **Finish**: waits for the download if one is running; "Start chatting".

Progress is in the settings (`onboarding_step`, `onboarding_done`), so closing the app
midway resumes there, and it never shows again once finished (Settings has "Show the
welcome again"). A model picked for download is saved as `pending_model`; whichever
download finishes it, the daemon makes it the default model on its own, so the user
can keep going (or close the app) while it downloads.

## Packaging and releases

Installers are Tauri bundles with two extra pieces, configured only in
`apps/desktop/src-tauri/tauri.bundle.conf.json` so development and CI builds don't need
them:

- `mimid` as a sidecar (`externalBin: binaries/mimid`, built for the target triple by
  `scripts/prepare-bundle.sh`). It lands beside the app's executable, where
  `mimi_service::daemon_binary` looks.
- `llama-server` and its libraries as resources (`resources/llama/` → `llama/`),
  fetched by `scripts/fetch-llama-server.sh` from a pinned llama.cpp release with
  SHA-256 checks, and trimmed to what `llama-server` loads.

| Platform | Installers | llama.cpp build |
|---|---|---|
| Linux x86_64 | .deb, .rpm, AppImage | Vulkan (CPU fallback) |
| macOS arm64 / x86_64 | .dmg (unsigned for now) | Metal |

`pnpm bundle` builds them locally. Pushing a version tag (`v0.2.0`, matching
`tauri.conf.json`) runs `.github/workflows/release.yml`, which builds all of them and
attaches them to a draft GitHub Release. `scripts/install.sh` (`curl … | sh`) picks the
right file for the machine from GitHub Releases, checks its SHA-256 and installs it: the
.deb/.rpm on apt/dnf/zypper systems, else the AppImage in `~/Applications` with a menu
entry, and on macOS `Mimi.app` in `/Applications`. There is no automatic update check
(principle 0); running the installer again updates.

Later: Developer ID signing and notarization for macOS (until then the install script
removes the quarantine flag, and a .dmg opened by hand needs right-click › Open), Linux
arm64 packages.

## Planned

- **Semantic memory search**: optional local embeddings (e.g. an Ollama embedding model)
  alongside FTS5, stored as plain vectors and compared in Rust (no loadable SQLite
  extensions: the workspace forbids unsafe code).
- **Integrations**: MCP. Each tool declares its capabilities (hosts, paths, outbound
  messaging) and the user approves them. Untrusted plugins run sandboxed.
- **Remote access**: a separate, opt-in listener with its own device pairing and
  authentication. The loopback listener stays loopback-only.
