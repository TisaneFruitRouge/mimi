# Mimi

A self-hosted, private personal AI assistant for everyday people, not only developers.
"Mimi" is a working name. Keep the name confined to identifiers and strings so a
rename is a find-and-replace.

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
  holds the route (`#/models`, `#/chat/<id>`). Setup is the Models page in setup mode
  until a model is chosen.
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

## Commands

```sh
pnpm dev                 # daemon + desktop app together, data in .dev/
pnpm web                 # build the frontend and open it in a browser (daemon running)
pnpm dev:daemon          # daemon only (pnpm dev:app for the app only)
pnpm mimi <args>       # CLI against the .dev/ instance
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
