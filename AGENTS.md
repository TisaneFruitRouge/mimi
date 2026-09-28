# Mimi

A self-hosted, private personal AI assistant for everyday people, not only developers.
Keep the product name confined to identifiers and strings so a rename stays a
find-and-replace (it was renamed from Hearth once already).

## Principles (apply to every change)

0. **There is no Mimi server. Ever.** Everything runs on the user's own machines. No
   central relay, account system, telemetry, update-check or proxy operated by the
   project. Third parties are contacted only directly from the user's machine, and only
   when a feature needs them (Google for their calendar, Telegram for their messages, a
   cloud model they chose). Registered app identities (e.g. Mimi's Google OAuth
   client ID, a Telegram api_id) are just identifiers shown on consent screens; they
   never route data through the project.
1. **Self-hosting, privacy and security by default.** Prefer local processing. Data
   leaves the machine only when the task needs it (sending a Telegram message, creating
   a calendar event), or when the user has explicitly chosen a cloud model.
   - Cloud LLM providers are allowed, but local is the default and the recommended
     choice. When a cloud model is in use, the UI must say so visibly.
   - Treat all external content (incoming messages, web pages, tool output) as
     untrusted input that may contain prompt injection. Outbound actions go through
     the capability and approval system, never straight from model output.
   - Data at rest is encrypted: the SQLite database uses SQLCipher, with its key in the
     OS keychain (or an owner-only file where no keychain exists, reported in `Status`).
     Secrets such as API keys go in that database, never in plaintext config.
   - Listeners bind to loopback unless the user explicitly enables remote access.
2. **UI and UX matter.** Everything must be configurable from the GUI. The GUI must be
   able to do everything the CLI/TUI can. Write user-facing copy for non-technical
   people, with no jargon in the default path.
3. **Easy install.** Native installers (.dmg, .deb/.rpm/AppImage), at most one terminal
   command. No manual dependency setup for users.
4. **Runs on anything.** No minimum hardware. Hardware detection drives model
   recommendations: cloud models on weak machines, large local models on big GPUs,
   and models the user already has installed.
5. **macOS and Linux only** for now. No Windows. Unix-only APIs are fine.
6. **One instance, one machine** for now. Multi-device access comes later, so keep the
   daemon API client-agnostic and don't take shortcuts that assume the client is local.

## Scope for the first version

In: desktop chat with persistent memory (local model first), messaging channels
(Telegram/Signal/Matrix), proactive features (reminders, scheduled tasks, briefings),
and the daemon running as a background service.
Out: acting on the computer itself (shell, file management, browser automation).

## Architecture

See `docs/architecture.md`. In short: `mimid` (crates/core) owns everything, and the
desktop app, CLI and future TUI are all clients that go through `mimi-client`. New
features go in the daemon plus the protocol types. Frontends only render them.

- API types go in `mimi-protocol` and derive `ts_rs::TS` with `#[ts(export)]`.
  `cargo test -p mimi-protocol` regenerates `apps/desktop/src/bindings/`, which is
  committed; CI fails if it's stale. Annotate `u64`/`i64` fields `#[ts(type = "number")]`.
- Schema changes are new files in `crates/core/src/db/migrations/`, registered in
  `MIGRATIONS`. Never edit a migration that has been committed.
- The desktop webview calls Tauri commands. It never talks to the daemon or holds the
  token directly: the `api` command proxies `/v1` requests, and the Rust side relays
  daemon events to the webview as `daemon-event` / `daemon-connection`.
- The same frontend also runs in a browser, served by the daemon (`rust-embed` of
  `apps/desktop/dist`: read from disk in debug builds, embedded in release). Only
  `src/lib/transport.ts` knows which it is: Tauri IPC in the app, same-origin
  `fetch`/WebSocket in a browser. Never call Tauri APIs from screens directly; use
  `openExternal` for links.
- Web auth: native clients use the bearer token; browsers use a `mimi_session`
  cookie (HttpOnly, SameSite=Strict) obtained from a single-use, 2-minute login link
  that only bearer clients can mint (`POST /v1/web/login-link`). Only a SHA-256 of each
  session token is stored. Cookie-authenticated requests must carry an allowed Host
  (`127.0.0.1:<port>`/`localhost:<port>`, against DNS rebinding) and, for anything but
  GET plus the event socket, a matching Origin (against CSRF). Keep these checks in
  `require_auth` when adding routes; new `/v1` routes get them automatically.
- Frontend layout: `src/lib/api.ts` (typed calls + query keys), `src/lib/events.ts`
  (applies daemon events to the TanStack Query cache; prefer this over refetching),
  `src/features/<area>/` for screens, `src/components/` for shared pieces. Model output
  is rendered with react-markdown (no raw HTML); links open in the system browser.
- Every place a model is chosen or used shows its locality (`LocalityBadge`). Cloud use
  must always be visible to the user.
- Look and feel: see "Design system" below. Copy is plain language for non-technical
  people: "model source" not "provider", no URLs unless the user typed one, no model
  names outside the Models page, no tool names or technical labels in chat.
- Navigation: the daily panels (Chat, Calendar, Mail, People) are a segmented control
  in the translucent top bar, ⌘/Ctrl 1–4. Everything set up once lives in the Settings
  window (gear button or ⌘/Ctrl ,): a System-Settings-style sidebar with General,
  Connections, Models, Memory, Reminders & notifications, Permissions, Privacy. Conversations are
  behind Search (⌘/Ctrl K), which also jumps to every panel and settings page. ⌘/Ctrl N
  is a new chat. The URL hash holds the route: `#/chat/<id>`, `#/calendar`, `#/mail`,
  `#/people/<id>`, `#/settings/<page>` (types `Tab`, `SettingsPage` in `top-bar.tsx`);
  old addresses (`#/models`, `#/reminders`, …) are rewritten to their new place. Chat
  is home. Reminders and routines live in Calendar (side list, bells in the grid);
  where they're delivered is Settings › Reminders & notifications. First run is the
  onboarding (see "Built-in model runtime, onboarding and installers"), which may name
  models like the Models page; the Models page in setup mode remains the fallback when
  a finished setup has lost its model.
- "Ask Mimi about this": every panel can open a new chat with the thing already
  @-mentioned. Build a `Draft` with `mentionDraft(kind, id, label)` (`lib/draft.ts`) and
  call the shell's `onAsk`; the chat takes it once on mount (`ComposerHandle.setDraft`).
  The button says the user's assistant name (`useAssistantName`), not "Mimi".
- The Mail panel is `features/mail/mail-view.tsx` with props `{ onSection, onAsk,
  onOpenPerson }`; the email feature owns that file.
- Integrations come from the daemon's catalog (`GET /v1/integrations`); list new ones
  there with status `coming_soon` until their connection flow exists, then add their id
  to `AVAILABLE` in `crates/core/src/integrations.rs`.

## Design system (frontend)

Calm, precise, macOS/iOS-system-app feel on a light canvas. Everything below lives in
`apps/desktop/src/index.css` and `src/components/`; new screens compose these rather
than inventing their own.

- **Colours** (tokens on `:root`, Tailwind names in `@theme`): `canvas` #f5f5f7 app
  background; `background` white for cards, popovers, composer; `foreground` #1d1d1f;
  `muted-foreground`/`faint` #6e6e73 secondary text; `fill` (translucent grey) for
  segmented tracks, chips, hovers and secondary buttons; `subtle` for quiet insets (user
  bubbles use #e9e9ee); `separator` for hairlines. Lime is the signature accent, used
  sparingly: send, Approve, best fit, progress, focus (`lime`, `lime-soft`, and
  `lime-deep` for lime-family text on white). Data-flow colours are fixed meanings:
  `private*` teal, `network*` blue, `cloud*` amber. No hard black borders.
- **Type scale** (utilities; use instead of ad-hoc sizes): `type-large-title` 30 (page
  titles), `type-title` 22, `type-headline` 17 semibold, `type-body` 15, `type-callout` 14,
  `type-subhead` 13, `type-footnote` 12. They're named `type-*`, not `text-*`, on purpose:
  the `cn()` merger treats unknown `text-*` classes as colours and silently drops a size
  combined with a text colour. Section headings use `section-label` (13, semibold, grey),
  never mono uppercase labels. Mono is only for things the user literally types (e.g.
  `/newbot`).
- **Surfaces**: `surface` (white, 18px radius, `--shadow-card`), `grouped` (iOS grouped
  list: white, 16px radius, hairline separators between children), `material` /
  `material-thick` (translucent + blur, for bars and floating controls). Elevation comes
  from `--shadow-card` < `--shadow-raised` < `--shadow-float` (popovers, sheets), never
  from borders. Radii: controls 10, small cards 14, cards 18, sheets 22, composer 24. To
  override a utility's background or shadow on the same element, use the important
  suffix (`bg-subtle!`).
- **Components** (`src/components/page.tsx`): `Page` (scroll container under the top bar;
  reports the scroll edge so the bar gains its material and hairline), `PageHeader`
  (large title + one-line subtitle), `Section` (label + optional action), `Grouped` +
  `Row` (icon tile, title, detail, trailing controls; pass `onClick` for a selectable
  row), `IconTile` (tinted rounded-square icon, sm/md/lg), `Pill` (small capsule label).
  Buttons (`components/ui/button.tsx`): `default` (dark, primary), `lime` (signature
  positive action), `secondary` (grey fill), `ghost`, `outline`, `destructive`; all press
  with a slight scale. Put destructive and rarely used actions in a "…" `DropdownMenu`,
  not in the row. Confirm destructive actions with the centred, macOS-style
  `AlertDialog` (`AlertDialogAction variant="destructive"`).
- **Spacing**: 4/8pt grid. Pages: max 880px wide, 24px side padding, 40px between
  sections, 10px between a section label and its content, 12px grid gaps.
- **Panels**: full-height screens under the top bar. Master–detail screens (People,
  Settings) use a left column on a translucent white (`bg-[rgb(255_255_255/0.45)]`,
  hairline on the right, starting 68–76px down so it clears the bar) and the detail as a
  normal `Page` beside it. Lists in a column are arrow-key navigable (`role="listbox"`
  / sidebar `nav`), with the selection in grey fill, not lime. Calendar colours come from
  the daemon (`CalendarInfo.color`: the server's own, else a stable palette pick); draw
  events as a tint of that colour with a 3px left bar, and the "now" line and today's
  date in red (#ff3b30), as calendar apps do. Anything read from a calendar (titles,
  places, notes, attendees) is plain text: never linkified, never markdown.
- **Right-click** (`components/app-context-menu.tsx`, `components/ui/context-menu.tsx`):
  every right-click opens one of Mimi's menus, never the webview's (Back, Reload…).
  Things with their own menu wrap themselves in `ContextMenu` (a mail conversation, a
  message, a smart folder, a chat message, a person; `ThingMenu` for "Open / Ask about
  this"; in Calendar an event: open, remind me before (upcoming only), remove its
  reminders, ask, copy details, hide its calendar; a reminder or routine: `ItemMenu`;
  an empty slot of the week grid: new event at that time, new reminder, navigation,
  view); the innermost wins, and `AppContextMenu` around the shell covers the
  rest (Copy / Ask about the selected text, New chat, Search, Settings). Text fields keep
  the system's editing menu. The email frame re-dispatches its right-clicks to the page
  (with the link or selection under them) so the message's menu opens there.
- **Per-viewer conveniences** (calendar view, hidden calendars) go in `localStorage`
  wrapped in try/catch; the page must work without them.
- **Motion** (`motion` library, `motion/react`): springs, not linear easing (typical
  stiffness 380–520, damping 30–38). Section changes fade and rise 6px; new messages and
  approval cards rise in; popovers and sheets scale in from 0.96–0.97. Don't wrap
  streaming content (message text) in layout animations. Everything respects
  `prefers-reduced-motion` (`MotionConfig reducedMotion="user"` plus a CSS override).
- **Chat**: no avatars; user messages are grey bubbles on the right; replies are plain
  text. The composer floats over the thread (the thread pads itself by the composer's
  measured height). Its toolbar keeps round icon buttons on the left (`@` mentions, `+`
  more/actions, also opened by typing `/`) and the privacy chip plus the round send/stop
  button on the right. Model switching lives in Models, not in the composer.
- **Desktop window**: on macOS the title bar is an overlay (hidden title); the top bar
  leaves room for the traffic lights (`macOverlayTitleBar` in `lib/platform.ts`) and is a
  `data-tauri-drag-region`. On Linux the window has no system title bar at all
  (`set_decorations(false)` at startup: GTK's would only say "Mimi" above our bar);
  the top bar draws minimize/maximize/close (`WindowControls`, from the `window_chrome`
  command), except under tiling window managers (Hyprland, Sway, i3, niri…), which
  manage windows themselves.

## Connections (the user's own accounts)

- `crates/core/src/connections/`: one row per connected account in the `connections`
  table, with its config (secrets included) as JSON in the encrypted database. The config
  never leaves the daemon; clients see `Connection` (name, status, one-line detail, an
  optional `action_url` for "finish setup"). `ConnectionsChanged` events keep them live.
- Every connection is checked against the real service before it's saved (fetch the
  feed, discover CalDAV calendars, Telegram `getMe`), so a saved connection works.
- Calendars: **Sign in with Google** (`calendar/google.rs`, integration `google`), the
  one registered app identity: OAuth for installed apps, owned by the daemon (it returns
  the consent URL, the client opens it with `openExternal`, a listener on 127.0.0.1
  exists only during sign-in; PKCE, `state` check, scopes `calendar.events` +
  `calendar.calendarlist.readonly`). Data goes only between the machine and Google; the
  refresh token stays in the connection config. Client ID from
  `MIMI_GOOGLE_CLIENT_ID`/`_SECRET` at build or run time; without one the dialog offers
  only Google's secret iCal address (read-only, new events through a pre-filled page).
  CalDAV with app-specific passwords (iCloud, Fastmail, Nextcloud, Radicale), written
  with ETags; one occurrence of a series is changed with an override or `EXDATE`
  (`calendar/edit.rs`). Recurrence is expanded locally (`calendar/ics.rs`) for iCal and
  CalDAV, by Google itself for signed-in Google. Setup notes: `docs/google-oauth.md`.
- Telegram is a bot the user creates with @BotFather, long-polled (nothing listens for
  inbound connections). A one-time `/start <code>` pairs it with its owner; every other
  chat is ignored. Messages go into a "Telegram" conversation; approval cards are sent as
  inline Approve / Don't buttons. Bot messages aren't end-to-end encrypted: say so.
- **Calendar panel APIs** (`api/calendar.rs`): `GET /v1/calendars` (id, name, colour,
  writable), `GET /v1/calendar/events?from&to` (ms, at most ~a year; events of every
  calendar merged, recurrence expanded, organizer/guests matched to People by email),
  `POST /v1/calendar/events` (CalDAV and signed-in Google: saved; Google by address:
  returns the pre-filled page to open). Calendar ids are stable
  (`calendar::calendar_id`: connection id, plus a hash of the collection or Google
  calendar id); every `CalEvent` carries its `calendar_id`. Adding an event from the
  panel is the user's own action, so it needs no approval card; the assistant's
  `calendar_add_event`, `calendar_change_event` and `calendar_delete_event` do, unless
  Permissions allow them.
- Never log feed URLs, tokens or passwords (reqwest errors include URLs: map them).
- Tests fake the outside world: Radicale (`uvx radicale --auth-type=none`) for CalDAV
  (`MIMI_TEST_CALDAV=http://127.0.0.1:5232/ cargo test -p mimi-core live_caldav --
  --ignored`), `calendar/google_fake.rs` for Google (sign-in, tokens, Calendar API),
  `fake_telegram` in `api/tests.rs` for the bot, public Google holiday feeds for iCal
  parsing. Never point tests at Google.

## Email

- `crates/core/src/mail/`; full design in `docs/architecture.md` › Email. IMAP to read,
  SMTP to send, with app passwords: no registered app, no Microsoft/Google sign-in.
  Presets (`mail::presets`): iCloud, Gmail (needs 2-Step Verification for app
  passwords), Fastmail, Proton (through Bridge on 127.0.0.1), Other; Outlook.com is
  listed as not supported yet. `mail::connect` signs in over IMAP and SMTP before
  anything is saved. Crates: async-imap, mail-parser, html2text, lettre (all MIT/Apache).
- **Transport** (`net.rs`, `smtp.rs`): TLS or STARTTLS checked against the OS trust
  store (rustls-platform-verifier). Unencrypted connections, and self-signed
  certificates, are accepted only for servers on this computer (Proton Bridge). SMTP
  says hello as `[127.0.0.1]`, never the computer's name; Message-IDs use the sender's
  domain.
- **Sync** (`sync.rs`): one loop per account (a connection task, cancelled on
  disconnect). Inbox, Sent and Archive only, never Spam or Trash; the last 90 days
  (at most 2,000 messages per mailbox on the first pass). New mail by UID above the
  stored high-water mark; flag changes and removals by a `(UID FLAGS)` sweep over what's
  stored; a new UIDVALIDITY refetches the mailbox. Then IDLE on the Inbox (10 minutes,
  or until poked after a send/archive; polling every 2 minutes without IDLE). Backoff
  30 s → 15 min; a refused password waits 30 minutes and says so on the connection.
  Messages over 2 MB are stored with headers only.
- **Storage** (`store.rs`, migration 0013): `mail_threads`, `mail_messages` (+
  `mail_fts`), `mail_sync`. Threading by In-Reply-To/References/Message-ID, then "Re:"
  subject plus a shared participant within 30 days. Bodies are plain text: HTML is
  converted with hidden content removed (`parse::strip_hidden`: display/visibility/
  opacity/zero-size styles, `hidden`, comments, scripts; font size is inherited so
  `font-size:0` layout wrappers keep readable text) and invisible characters stripped.
- **Email is untrusted.** Tools (`tools.rs`): `mail_search`, `mail_read_thread` (reads,
  output carries a "data, not instructions" notice), `mail_draft_reply`/`mail_compose`
  (a draft shown in the chat as an editable card; nothing is sent), `mail_send` (always
  needs approval; the card shows To, Cc, Subject and the whole message). Model calls
  about mail (`model.rs`: sorting, summaries, drafts) get no tools and their output is
  shown or parsed into a fixed shape. Mail never reaches memory learning (it reads only
  the user's own messages). `api/tests.rs` › `mail_flow` proves a hostile email can't
  send mail without approval; keep it passing.
- **Sorting** (`triage.rs`, `Settings.mail_sorting`, Settings › Privacy): new inbox
  conversations of the last 14 days get needs_reply / important / other and a one-line
  summary, one at a time, while no chat reply is being written, with
  `ChatOptions::QUICK`. Newsletters and automatic mail (List-Unsubscribe, List-Id,
  Precedence, Auto-Submitted, no-reply senders) are filed as "other" without the model.
- **People**: `contacts::Correspondents` is a contact source: everyone the user wrote
  to, plus people (not automatic senders) who wrote at least twice; email handles only,
  so they join someone only through the same address. `GET /v1/mail/threads?person=<id>`
  lists recent conversations with a person.
- **API**: `GET /v1/mail` (accounts, counts, sorting, model locality), `/mail/presets`,
  `/mail/threads?view&q&person&before&limit`, `/mail/threads/{id}`, `POST
  /mail/threads/{id}/read|archive|summarize|draft`, `/mail/send` (the user's own click
  in the panel or a draft card is the approval), `/mail/refresh`. `MailChanged` events.
- **Jev as the sorter** (`jev.rs`, `Settings.mail_sorter`): the user's own model sorts
  by default; they may choose Jev (TypeSafe's cloud decision model, `POST
  https://api.typesafe.ai/v1/systemone`, their own key) in Settings › Privacy, shown with
  the cloud badge there and in the Mail panel. The key is checked with TypeSafe before
  it's saved, kept in the `settings` table row `jev` (never in `Settings`, which clients
  read), and removed with `DELETE /mail/jev` (which puts the sorter back to the model).
  Automatic and suspicious mail is still filed locally, never sent. Jev writes no text:
  no summaries. Tests point `state.mail.jev_api` at a fake. Local "System One" models
  (Laya, GLiNER2.5-Decide) were tried on a labelled set of 180 emails and scored well
  below the chat model for these categories (about 40–50% vs 66%), so none ships yet.
- **Smart folders** (`folders.rs`, migration 0017): the user names a folder and describes
  what goes in it; the chosen sorter (their model, or Jev with one yes/no per folder)
  files conversations in the sorting queue after new mail is sorted (even with sorting
  off: making a folder asked for it), newest first, one conversation at a time. Rows in
  `mail_folder_threads` record each check (`source` auto | user); the user's choices are
  never overwritten, and a new description forgets only the sorter's. Suspicious mail
  is never filed. Folders are labels in Mimi only. `?folder=` on `/mail/threads`. Each
  folder has an icon and a colour from fixed sets (`folders::ICONS`/`COLORS`, mirrored
  in `features/mail/folder-looks.tsx`; migration 0019); the API refuses other names.
- **Delete and forward**: `DELETE /mail/threads/{id}` moves every copy to the account's
  Trash (found by `\Trash` or name, made if missing) and forgets it here; `forward_of`
  on a draft re-attaches the original's attachments (fetched from the server). Thread
  ids are never reused (`mail_thread_ids`, migration 0016), so a stale id can't reach
  another conversation.
- **Suspicious mail** (`suspicious.rs`, migration 0015): instructions addressed to an
  AI assistant (English and French phrasings, in visible or hidden HTML text) flag a
  message. Flagged conversations are never sorted or summarised by the model (filed as
  "other"), show a warning in the panel, and carry a `suspicious` note in tool output
  and # mentions. Keep the phrases specific: mail *about* AI must not be flagged
  (`mail_about_assistants_is_not`). Only the Sent folder is exempt: mail elsewhere is
  checked even when its From line names the user, since anyone can write that
  (`mail_that_only_claims_to_be_from_the_user_is_still_checked`, migration 0020).
- **Sending from aliases**: `MailDraft.from` picks one of the account's addresses (its
  own, or one mail arrived at that `parse::is_own_address` accepts); replies default to
  the conversation's `received_on`. Anything else is refused. SMTP signs in as
  `smtp::user` (the account's username).
- **Attachments** are not stored, only their names. `GET
  /mail/messages/{id}/attachments/{index}` fetches the message from the server and
  returns the file, always as an `application/octet-stream` download with `CSP:
  sandbox` (never rendered in the web UI's origin). The desktop app's
  `open_attachment` writes it to the app cache (emptied at launch) and opens it with
  the system's app; programs and scripts (`attachments::is_program`) are only saved to
  Downloads and revealed, never opened.
- **UI**: `features/mail/` (`mail-view.tsx` panel: views, list, reader, reply box;
  `message-body.tsx`: how a message is shown; `draft-editor.tsx`; `draft-card.tsx` for
  chat drafts). In Text mode email is plain text, never linkified. The connect form is
  `EmailAccount` in `connect-dialogs.tsx`.
- **Rendering** (`render.rs`, `images.rs`, migration 0018; details in
  `docs/architecture.md` › Email › Showing mail): Text · Formatted · Original, a
  per-viewer choice (localStorage, default Original). Only for display: models, tools,
  triage and mentions keep the plain-text `body`. The HTML is sanitized (ammonia
  allow-list, hidden text removed, no `url()` or positioning in CSS, no relative
  addresses) when kept at sync and again when served, with `cid:` pictures inlined and
  remote pictures left out until the user clicks "Load images"; then the daemon fetches
  only that message's pictures (public addresses only, checked after DNS and on every
  redirect; pictures only, size-capped) and inlines them as `data:`. The panel shows it
  in a `sandbox="allow-same-origin"` iframe (never add `allow-scripts`: with same origin
  it would lift the sandbox) with its own CSP, and opens links with `openExternal`.
  Formatted is Markdown made by the daemon with everything the sender wrote escaped.
  Crates: ammonia (its cssparser is MPL-2.0), markup5ever_rcdom (MIT/Apache).
- **Tests** fake the servers: `mail/fake.rs` is a small IMAP (IDLE, MOVE, APPEND…) and
  SMTP server; `mail/tests.rs` covers sync, UIDVALIDITY resets, flags, archive, send,
  hidden text, correspondents and sorting with a mock model. To try the app by hand,
  `MIMI_FAKE_MAIL_SEED=1 cargo test -p mimi-core fake_mail_server -- --ignored` serves
  seeded mail on 127.0.0.1:3143 (IMAP) / 3025 (SMTP) as `me@example.org` / `app-pass`
  (connect with "Other", security "None"). Never point tests at a real mailbox.

- **Received on** (migration 0014, `parse::received_on`): each incoming message records
  which of the user's addresses it arrived at: the top-most X-Original-To/Delivered-To
  that is plainly theirs (`parse::is_own_address`: the account's address, a +tag of it,
  or any address on its own domain, never a shared one like gmail.com), else a matching
  To/Cc, else the account's address. Anyone can write these headers, so an address on
  another domain never counts, even when the mail was addressed to it (aliases there
  show as the account's address). Mail stored before the column is filled from To/Cc
  at the next sync, and older values that fail the rule are reset. `store::Scope`
  (account or address) narrows lists and counts (`?account=` / `?address=` on `/mail`
  and `/mail/threads`); the Mail panel's "Received on" section shows each account's
  addresses once there's more than one. They are not "the user" elsewhere:
  `my_addresses` (from_me, "(the user)" in transcripts, participants) is the accounts'
  own addresses only, plus whatever was filed as outgoing (Sent).
- **Finding servers** (`mail/discover.rs`): users only type their address and password.
  Order: known consumer domains → the domain's MX mapped to known hosts (Migadu, Google
  Workspace, iCloud custom domains, Fastmail, mailbox.org, Posteo, Infomaniak, OVH,
  Gandi, Zoho…) → the domain's own autoconfig file (HTTPS) → RFC 6186 SRV → probing
  `imap.`/`mail.` hosts. Never a third-party lookup service. The password field only
  says "App password" for services that require one. Add new hosts to `known_mx` /
  `known_domain`; `cargo test -p mimi-core live_discovery -- --ignored` checks real domains.

## People and @ mentions

- `crates/core/src/people/`: one directory of people. A person has contact cards
  (`person_records`, one per card per source) and handles (`person_handles`, each with
  its channel, value, label and the card it came from; `source IS NULL` = added by the
  user). Keep person ids stable: other features (memory notes, messaging) link to them.
- **Adding a contact source** (e.g. Telegram contacts from a "send as me" account):
  implement `people::ContactSource` returning *all* current cards per connection, and
  register it with `state.people.sources.add(...)` at startup. Sync diffs by card id;
  never write to the people tables directly. Record ids must be stable per source.
- **Unification rules (don't loosen them):** cards merge only on a shared match key
  (`people::normalize::match_key`: email, phone incl. Signal/WhatsApp numbers, Telegram
  username). Never on names: those become "possible duplicates" for the user. Never
  guess a phone's country code. Once placed, a card stays with its person (so user
  splits survive syncs); imported handles are read-only, "change it in the address book".
- **Mentions:** `Mention { kind: person | event | mail_thread | mail_message, id, label }`,
  with `@label` in the message text for people and events and `#label` for email
  (`MentionKind::sigil`, `mentionSigil` in `lib/draft.ts`). The engine resolves them
  into a `<mentioned>` block (data, not instructions), stores it in
  `messages.mention_context` and replays it later. Event ids come from
  `people::mentions::event_id` (calendar + uid + occurrence start). `GET
  /mentions?kind=mail` gives the # suggestions (`mail/mentions.rs`; single-message
  conversations come as emails). A mentioned email is quoted with its angle brackets
  replaced, so it can't close the block, within a 3,000-character budget.
- **Composer:** the textarea stays the input. The @ and # logic lives in
  `apps/desktop/src/features/chat/mentions/` (`useMentions`, picker, highlight layer);
  the highlight layer must share the textarea's box and type (`fieldText` in
  composer.tsx) or the pills drift from the text.
- Channel icons and avatars: `src/components/people.tsx`.

## Tools (what the assistant can do)

- **Approval rule:** anything that sends, changes or deletes something on the user's
  behalf needs approval (`Tool::needs_approval` returns true); reads don't. Never
  weaken this for convenience; it is the main defence against prompt injection.
- **Permissions** (Settings › Permissions; design in `docs/architecture.md` › Tools
  and approvals › Permissions): the kinds of action the user may let happen without
  asking are descriptors in `tools::permissions::KINDS` (send email, add events, change
  or remove events, reminders & routines), served by `GET /v1/permissions`; the page
  renders only that. Each kind has a default (`Autonomy::Ask | Automatic`) plus
  exceptions for people or calendars; most specific wins, and a call about several
  (an email to three people) is automatic only if every one allows it. A tool opts in
  with `Tool::governed_by` and says who or what a call is about with
  `Tool::call_targets`; `requires_approval` decides, in `chat::act`. Choices change only
  through the user's own calls (`PUT /v1/permissions/{kind}`, or "Don't ask again for …"
  on a card: `always` on approve, taking the exception the daemon offered), never
  through a tool; `PUT /v1/settings` keeps the stored ones. Automatic email still asks
  unless every recipient is known (`mail::known`: the user's own addresses, address-book
  or hand-added handles, or someone in the Sent folder; never Mimi's correspondent cards
  or mail that merely claims to be from the user), even for a person with an exception.
  Keep that check, and don't add new kinds without the same care. Automatic actions
  show in the chat as a card with their details; `api/tests.rs ›
  automatic_sending_only_writes_to_people_the_user_knows`, `mail_flow ›
  people_exceptions_and_dont_ask_again` and `permission_api` cover it.
- **Adding a tool:** implement `tools::Tool` (crates/core/src/tools/mod.rs): a stable
  `snake_case` name, a description written for the model, a JSON Schema for the
  arguments, `summary()` as one plain-language line for the approval card, and
  `result_label()` as a short past-tense line ("read calendar"). Integrations expose
  their tools through a `ToolSource` added to `state.tool_sources` at startup; the
  source returns no tools while the integration is disconnected. Each reply builds a
  fresh registry, so nothing else needs wiring.
- Optionally add a formatter for the tool's arguments in
  `apps/desktop/src/features/chat/action-formatters.tsx` so its approval card reads
  well; otherwise arguments show as a tidy key/value list. Any argument a formatter
  doesn't list in its `keys` is still shown after its rows.
- **The card shows what runs:** a tool whose arguments point at something (an event to
  change) looks it up in `Tool::resolve` and writes the real thing into the arguments,
  over anything the model put there. Then, before an approval card is shown, the
  call's arguments go through `Tool::prepare` (default `tools::conform`: the schema's
  types, with plain mistakes like a string for a list converted and anything else
  refused), and the stored action holds the result. Override `prepare` when the tool reads arguments in
  a further shape (e.g. `mail_send` splits recipients into one address each).
- Tool output is given back to the model verbatim (cut at 16k characters) and stored
  with the message, so keep it compact and free of secrets.
- Debug builds have two fake tools (`dev_lookup`, `dev_send_note`) enabled with
  `MIMI_DEV_TOOLS=1`, for exercising approval cards against a scripted model.

## Memory (what the assistant knows about the user)

- `crates/core/src/memory/`; full design in `docs/architecture.md` › Memory. Two tiers:
  a capped **profile** (`profile.md`, 1,200 chars, always in the prompt) and a
  **library** of short notes by path (`people/sam.md`, `habits/…`, `preferences/…`,
  `places/…`, `work/…`, `interests/…`, `health/…`, `notes/…`), found through FTS5 and,
  when the user turns on "Understands meaning", by embeddings, then recalled into the
  prompt within a strict budget (`memory/recall.rs`). Keep the prompt small: local
  models have ~8k-token contexts.
- **Recall by meaning** (`memory/semantic.rs`) is optional and must stay so: words-only
  recall is the fallback whenever the embedding model is off, missing or slow. The
  model is the catalog's `embeddings` entry (downloaded like chat models), served by a
  second `llama-server --embedding` on the CPU or, without the built-in runtime, the
  user's Ollama; never a cloud source. Vectors live in `memory_vectors`; the background
  `semantic::run` keeps them current, so writers only publish `MemoryChanged`. Test with
  `semantic::tests::FakeEmbedder`, not a real model.
- **Notes about people** link to the people directory through `subject` (person id),
  only when it's unambiguous (`memory/link.rs`; never guess between two Léas). Recall
  puts notes about people the message @-mentions or names first.
- **Internal model calls** (learning, mail triage, anything the user doesn't read as it
  streams) use `OpenAiCompatible::complete(model, messages, ChatOptions::QUICK)`: thinking
  off on local sources, a harmless no-op elsewhere. Chats keep `stream_chat`.
- Memory tools don't need approval (memory is local), but writes must stay visible in the
  chat and undoable: return `revision` from every write so the UI can offer Undo.
- Only the user's own statements become memories. Never weaken `looks_secret`, the
  "don't remember this" opt-out (`learn.rs`), or the rule that external content (tool
  output, calendars, other people's messages) is context, not a source of facts.
- Schema: `memory_notes` + `memory_fts` (triggers keep them in step), `memory_revisions`
  (undo), `memory_learned` (learning progress per conversation), `memory_vectors`
  (embeddings), and `subject` on notes (the linked person's id).
- Try it for real with `MIMI_MEMORY_QUIET_SECS=15` so the background learning pass
  runs soon after a chat; it logs `learning pass done … notes_changed=N`.

## Reminders and routines (the scheduler)

- `crates/core/src/schedule/`; full design in `docs/architecture.md` › Reminders and
  routines. A **reminder** tells the user something; a **routine** runs an instruction
  as a chat turn in its own conversation and delivers the answer.
- **Store rules, not instants.** `Schedule` (protocol) holds local wall-clock rules
  (`once`, `daily`, `weekdays`, `weekly`, `monthly`, `yearly`, `interval`,
  `before_event`); `next_at` is only a cache, recomputed by `rules::next_after` in the
  computer's current zone. Put all date arithmetic in `rules.rs` (pure, tested with fixed
  zones: DST gaps move forward, repeated hours use the first, short months clamp).
- **The loop** (`schedule::run`) sleeps until the earliest `next_at`/`snoozed_until`,
  capped at 60 s (monotonic sleeps stop during suspend), and wakes early on
  `state.scheduler.poke()`. Anything that changes an item must go through
  `schedule::create/update/delete/undo` (they re-arm and poke); never write
  `schedule_items` elsewhere. `tick(state, now)` takes the clock so tests can move it.
- **Catch-up never floods:** a due item fires once, marked late after 2 minutes; past 12
  hours it's recorded as missed and not sent. The next occurrence is computed after
  `max(due, now)`, so a week offline is one record, not seven reminders.
- **Event-relative items** re-find their event (calendar + uid, closest occurrence)
  every 15 minutes and right before firing; gone → `ended` with a plain reason;
  calendar unreachable → keep the last known time.
- **Delivery:** reminders go to the Telegram owner (Done / Snooze 10 min / 1 hour
  buttons, `done:`/`snooze:`/`snooze60:` callbacks), desktop notifications
  (`notify.rs`, fail-soft, `Settings.desktop_notifications`; `MIMI_NO_NOTIFICATIONS=1`
  and tests never show one), and the app (`ScheduleDelivered` event → toast). Sending
  runs in spawned tasks so one slow channel never holds up the loop.
- **Routines keep the approval rule.** Their tool calls go through approvals like any
  chat; pending ones are relayed to Telegram and the desktop, and the run waits (up to
  30 minutes) in its own task, never blocking the scheduler. A run whose previous run
  is still going is skipped, not queued.
- **Tools** (`schedule/tools.rs`): `reminder_add`, `routine_add`, `schedule_list`,
  `schedule_change`, `schedule_cancel`. No approval (the user's own local schedule), but
  every write returns `schedule_revision` and shows as a quiet line with Undo, like
  memory. Changes merge with the existing rule ("make it 8:00" keeps "every weekday").
- Schema: `schedule_items`, `schedule_deliveries` (history), `schedule_revisions`
  (undo). Try it for real with a scratch daemon, `MIMI_NO_NOTIFICATIONS=1`, and
  `MIMI_TELEGRAM_API` pointed at a fake bot server; "remind me in 2 minutes to …" fires
  on the minute.

## Running in the background (daemon lifecycle)

- Full design in `docs/architecture.md` › Running the daemon. The user never starts
  `mimid`: the app uses a running daemon, else starts the installed login service, else
  runs `mimid` as a supervised child and stops it on quit
  (`apps/desktop/src-tauri/src/daemon_process.rs`). "Keep Mimi running in the
  background" in Settings installs/removes the login service and hands the daemon over;
  `mimi service install|uninstall|status|start` does the same headless.
- **After an update** the login service may still run the old daemon. At launch the app
  compares the daemon's `/health` version with its own and restarts the service when
  they differ (not under `pnpm dev`). Requests an older daemon can't read (unknown route,
  unreadable body) surface as `DaemonError.code === "outdated"` with a plain message.
- **Finding `mimid`** is `mimi_service::daemon_binary()` only: `MIMI_DAEMON_BIN`, else
  next to the running executable (bundles ship it as the Tauri sidecar `binaries/mimid`,
  which lands beside the app binary; `target/<profile>/` in dev), else `PATH`. Don't add
  other lookups elsewhere.
- **One daemon per data directory.** Service names derive from it: `mimi` /
  `dev.mimi.daemon` for the default install, `mimi-<hash>` for any custom `MIMI_HOME`.
  So dev and test installs can never replace a real one.
- `pnpm dev` sets `MIMI_DAEMON=external` for the app: it only connects, never starts,
  stops or installs a daemon, and the background switch explains why it's unavailable.
- **Testing without disturbing the user's session:** never install, start or stop the
  plain `mimi` unit, and never touch the `pnpm dev` daemon. Use a scratch `MIMI_HOME`,
  `MIMI_KEY_STORE=file` and a spare `MIMI_PORT` (hashed unit name), and remove what you
  created. The desktop lifecycle has an opt-in test that runs real processes without a
  window: `MIMI_HOME=… MIMI_PORT=… MIMI_KEY_STORE=file MIMI_DAEMON_BIN=$PWD/target/debug/mimid
  cargo test -p mimi-desktop lifecycle -- --ignored`.

- **AppImage:** an AppImage runs from a temporary mount that vanishes when it quits, so
  a service installed from one runs the AppImage *file* (`$APPIMAGE`) with `--daemon`
  instead (`Spec::launched_from_app`); `main.rs` hands `--daemon` over to the bundled
  `mimid`. Replacing the AppImage at the same path keeps the service working.

## Built-in model runtime, onboarding and installers

- **Built-in runtime** (`crates/core/src/runtime/`): Mimi ships llama.cpp's
  `llama-server` and runs models itself, so nobody needs Ollama. The daemon creates a
  "Built into Mimi" model source (`ProviderKind::Builtin`, device locality) at startup
  whenever it finds the binary; it can't be added, edited or removed through the API.
  `Runtime::ensure` starts `llama-server` on demand for the requested model (one at a
  time, restarted when the model changes or the process died), on a random loopback port
  with a per-start API key passed via `--api-key-file` (never on the command line), waits
  for `/health`, and unloads after 20 idle minutes. Context size follows the hardware
  tier (4k-32k); GPU layers use llama.cpp's automatic fit. Log:
  `<data>/logs/model-runtime.log`. Status: `GET /v1/runtime` + `RuntimeChanged` events.
- Always get a chat client through `providers::chat_client` (and list models through
  `providers::list_models`): they route the built-in source to the runtime. Don't call
  `providers::connect` for a record that might be built-in.
- **Finding `llama-server`**: `MIMI_LLAMA_SERVER`, then beside `mimid`
  (`llama/llama-server` or `llama-server`), then the bundle's resource folder
  (`../Resources/llama/` on macOS, `../lib/<app>/llama/` on Linux), then `PATH`.
  For development: `scripts/fetch-llama-server.sh`, then
  `MIMI_LLAMA_SERVER=$PWD/apps/desktop/src-tauri/resources/llama/llama-server`.
- **Downloads** (`runtime/download.rs`): GGUF files straight from Hugging Face, pinned
  per catalog entry in `hardware/catalog.json` (`gguf`: repo, file, bytes, sha256). Kept
  in `<data>/models/` as `<file>.part` until the SHA-256 matches; a stopped or failed
  download resumes with a Range request. Progress goes through the same `ModelPull`
  events as Ollama pulls. `POST /providers/{id}/pull/cancel` stops one; `DELETE
  /providers/{id}/models/{model}` deletes one (refused for the model in use). A new
  catalog model needs its `gguf` block too (single-file GGUFs only; the sha256 is on the
  Hugging Face file page).
- **Onboarding** (`apps/desktop/src/features/onboarding/`): shown while
  `settings.onboarding_done` is false; the step is saved in `settings.onboarding_step`,
  so closing the app resumes there. Welcome and privacy promise -> model (hardware
  summary, installed models, suggested downloads, detected servers, or a cloud service
  with an honest note) -> optional calendar/email/Telegram (the Connections dialogs) -> "about
  you" (appended to the memory profile) -> finish. A model chosen for download is saved as
  `settings.pending_model`; the daemon makes it the default when the download finishes
  (`settings::adopt_pending`), even with no window open. Settings › General › "Show the
  welcome again" reruns it. Migration 0010 marks existing setups as onboarded.
- **Installers**: `pnpm bundle` = `scripts/prepare-bundle.sh` (frontend, release
  `mimid` for the target as `src-tauri/binaries/mimid-<triple>`, llama-server into
  `src-tauri/resources/llama/`) + `tauri build --config src-tauri/tauri.bundle.conf.json`.
  The sidecar and resources are only in that extra config, so `tauri dev`, CI and plain
  builds need neither. Both folders are gitignored: never commit binaries. Linux uses
  llama.cpp's Vulkan build (the GPU backend is a plugin; without a Vulkan driver it runs
  on the CPU); macOS uses Metal. Bump llama.cpp by editing `RELEASE` and the checksums
  in `scripts/fetch-llama-server.sh`.
- **Releases**: pushing a `v*` tag that matches `tauri.conf.json`'s version runs
  `.github/workflows/release.yml` (deb/rpm/AppImage on Ubuntu 22.04, arm64 and x64 dmg
  on macOS) into a draft GitHub Release; publish it by hand. `scripts/install.sh` is the
  one-command installer (`curl ... | sh`). No automatic update checks, ever (principle
  0). macOS builds are unsigned for now (the install script clears the quarantine
  flag); Developer ID signing and notarization are a later step.

## Commands

```sh
pnpm dev                 # daemon (port 7438) + "Mimi Dev" app together, data in .dev/
pnpm web                 # build the frontend and open it in a browser (daemon running)
pnpm dev:daemon          # daemon only (pnpm dev:app for the app only)
pnpm mimi <args>       # CLI against the .dev/ instance
mimi service status      # background service (install | uninstall | status | start)
pnpm check               # fmt, clippy, tests, typecheck (what CI runs)
MIMI_HOME=/tmp/h1 ...  # any other isolated instance
```

`pnpm dev` runs next to an installed Mimi: its own data (`.dev/`), port (7438) and app
identity (`src-tauri/tauri.dev.conf.json`, "Mimi Dev"). To install the current code as
the real app: `pnpm bundle` (on Arch, `NO_STRIP=true` for the AppImage step), then
`scripts/install.sh --file target/release/bundle/appimage/Mimi_*.AppImage`.

When testing `pnpm dev` from an agent session, stop it by exact PID or by the
`setsid` process group. Never `pkill -f` with a pattern that could match other
projects' processes or the current shell's own command line.

To check UI changes visually, run a throwaway daemon (scratch `MIMI_HOME`,
`MIMI_KEY_STORE=file`, its own `MIMI_PORT`), `pnpm build`, and drive the web UI with
`node scripts/ui-check.mjs` (headless Chromium: screenshots, clicks, keys, computed
styles). Chromium's one-shot `--screenshot` flag can capture transitions mid-way; use the
script instead.

The dev machine is the user's own desktop, in use while you work. Before sending input
(`wtype`) confirm the app window is focused (`hyprctl activewindow`), and only screenshot
it while it is visible: a window-region `grim` capture of a hidden window captures
whatever the user is looking at. Prefer verifying behaviour through the API/CLI.

Rust comes from mise on the dev machine (`eval "$(mise env -s bash)"` if `cargo` is not
on PATH).

## Conventions

- Rust 2024 edition, `unsafe_code` forbidden workspace-wide, clippy clean with `-D warnings`.
- Frontend: React 19, Tailwind v4, shadcn/ui (add components with
  `pnpm dlx shadcn@latest add <name>` from `apps/desktop`), lucide icons. Light theme
  only for now.
- License: AGPL-3.0-or-later. Check that new dependencies have compatible licenses.
