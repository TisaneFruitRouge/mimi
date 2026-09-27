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
- Navigation: three sections (Chat, Connections, Models) in a segmented control in the
  translucent top bar; conversations behind the Search button (⌘/Ctrl K); settings in a
  sheet (⌘/Ctrl ,). Shortcuts: ⌘/Ctrl N new chat, ⌘/Ctrl 1–3 sections. The URL hash
  holds the route (`#/models`, `#/chat/<id>`). First run is the onboarding (see "Built-in
  model runtime, onboarding and installers"), which may name models like the Models
  page; the Models page in setup mode remains the fallback when a finished setup has
  lost its model. Reminders (`#/reminders`) is reached from the bell in the top bar and
  the ⌘K palette, not from the segmented control.
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
  `data-tauri-drag-region`. Linux keeps native decorations.

## Connections (the user's own accounts)

- `crates/core/src/connections/`: one row per connected account in the `connections`
  table, with its config (secrets included) as JSON in the encrypted database. The config
  never leaves the daemon; clients see `Connection` (name, status, one-line detail, an
  optional `action_url` for "finish setup"). `ConnectionsChanged` events keep them live.
- Every connection is checked against the real service before it's saved (fetch the
  feed, discover CalDAV calendars, Telegram `getMe`), so a saved connection works.
- No registered app identities: calendars use Google's secret iCal address (read) plus
  pre-filled "Add to Google Calendar" pages the user saves (write), and CalDAV with
  app-specific passwords (iCloud, Fastmail, Nextcloud, Radicale). Recurrence is
  expanded locally (`calendar/ics.rs`), for Google and CalDAV alike.
- Telegram is a bot the user creates with @BotFather, long-polled (nothing listens for
  inbound connections). A one-time `/start <code>` pairs it with its owner; every other
  chat is ignored. Messages go into a "Telegram" conversation; approval cards are sent as
  inline Approve / Don't buttons. Bot messages aren't end-to-end encrypted: say so.
- Never log feed URLs, tokens or passwords (reqwest errors include URLs: map them).
- Tests fake the outside world: Radicale (`uvx radicale --auth-type=none`) for CalDAV by
  hand, `fake_telegram` in `api/tests.rs` for the bot, public Google holiday feeds for
  iCal parsing.

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
- **Mentions:** `Mention { kind: person | event, id, label }`, with `@label` in the
  message text. The engine resolves them into a `<mentioned>` block (data, not
  instructions), stores it in `messages.mention_context` and replays it later. Event
  ids come from `people::mentions::event_id` (calendar + uid + occurrence start).
- **Composer:** the textarea stays the input. The @ logic lives in
  `apps/desktop/src/features/chat/mentions/` (`useMentions`, picker, highlight layer);
  the highlight layer must share the textarea's box and type (`fieldText` in
  composer.tsx) or the pills drift from the text.
- Channel icons and avatars: `src/components/people.tsx`.

## Tools (what the assistant can do)

- **Approval rule:** anything that sends, changes or deletes something on the user's
  behalf needs approval (`Tool::needs_approval` returns true); reads don't. Never
  weaken this for convenience; it is the main defence against prompt injection.
- **Adding a tool:** implement `tools::Tool` (crates/core/src/tools/mod.rs): a stable
  `snake_case` name, a description written for the model, a JSON Schema for the
  arguments, `summary()` as one plain-language line for the approval card, and
  `result_label()` as a short past-tense line ("read calendar"). Integrations expose
  their tools through a `ToolSource` added to `state.tool_sources` at startup; the
  source returns no tools while the integration is disconnected. Each reply builds a
  fresh registry, so nothing else needs wiring.
- Optionally add a formatter for the tool's arguments in
  `apps/desktop/src/features/chat/action-formatters.tsx` so its approval card reads
  well; otherwise arguments show as a tidy key/value list.
- Tool output is given back to the model verbatim (cut at 16k characters) and stored
  with the message, so keep it compact and free of secrets.
- Debug builds have two fake tools (`dev_lookup`, `dev_send_note`) enabled with
  `MIMI_DEV_TOOLS=1`, for exercising approval cards against a scripted model.

## Memory (what the assistant knows about the user)

- `crates/core/src/memory/`; full design in `docs/architecture.md` › Memory. Two tiers:
  a capped **profile** (`profile.md`, 1,200 chars, always in the prompt) and a
  **library** of short notes by path (`people/sam.md`, `habits/…`, `preferences/…`,
  `places/…`, `work/…`, `interests/…`, `health/…`, `notes/…`), found through FTS5 and
  recalled into the prompt within a strict budget (`memory/recall.rs`). Keep the prompt
  small: local models have ~8k-token contexts.
- Memory tools don't need approval (memory is local), but writes must stay visible in the
  chat and undoable: return `revision` from every write so the UI can offer Undo.
- Only the user's own statements become memories. Never weaken `looks_secret`, the
  "don't remember this" opt-out (`learn.rs`), or the rule that external content (tool
  output, calendars, other people's messages) is context, not a source of facts.
- Schema: `memory_notes` + `memory_fts` (triggers keep them in step), `memory_revisions`
  (undo), `memory_learned` (learning progress per conversation). `subject` on notes is
  the seam for linking a note to a contact id.
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
  with an honest note) -> optional calendar/Telegram (the Connections dialogs) -> "about
  you" (appended to the memory profile) -> finish. A model chosen for download is saved as
  `settings.pending_model`; the daemon makes it the default when the download finishes
  (`settings::adopt_pending`), even with no window open. Settings > "Show the welcome
  again" reruns it. Migration 0010 marks existing setups as onboarded.
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
pnpm dev                 # daemon + desktop app together, data in .dev/
pnpm web                 # build the frontend and open it in a browser (daemon running)
pnpm dev:daemon          # daemon only (pnpm dev:app for the app only)
pnpm mimi <args>       # CLI against the .dev/ instance
mimi service status      # background service (install | uninstall | status | start)
pnpm check               # fmt, clippy, tests, typecheck (what CI runs)
MIMI_HOME=/tmp/h1 ...  # any other isolated instance
```

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
