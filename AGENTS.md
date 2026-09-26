# Hearth

A self-hosted, private personal AI assistant for everyday people, not only developers.
"Hearth" is a working name. Keep the name confined to identifiers and strings so a
rename is a find-and-replace.

## Principles (apply to every change)

0. **There is no Hearth server. Ever.** Everything runs on the user's own machines. No
   central relay, account system, telemetry, update-check or proxy operated by the
   project. Third parties are contacted only directly from the user's machine, and only
   when a feature needs them (Google for their calendar, Telegram for their messages, a
   cloud model they chose). Registered app identities (e.g. Hearth's Google OAuth
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

See `docs/architecture.md`. In short: `hearthd` (crates/core) owns everything, and the
desktop app, CLI and future TUI are all clients that go through `hearth-client`. New
features go in the daemon plus the protocol types. Frontends only render them.

- API types go in `hearth-protocol` and derive `ts_rs::TS` with `#[ts(export)]`.
  `cargo test -p hearth-protocol` regenerates `apps/desktop/src/bindings/`, which is
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
- Web auth: native clients use the bearer token; browsers use a `hearth_session`
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
- Look and feel ("Midnight", light edition): tokens live in `src/index.css` (`--canvas`,
  `--subtle`, `--faint`, `--lime*`, and the fixed data-flow colours `--private*` teal,
  `--network*` blue, `--cloud*` amber). Geist and Geist Mono are bundled via
  `@fontsource-variable`, never loaded from a CDN. Lime is the signature accent: use it
  sparingly (send, best-fit, progress). Mono uppercase labels for section headings. Copy
  is plain language: "model source" not "provider", no URLs unless the user typed one.
- Navigation: three sections (Chat, Connections, Models) in the top bar, conversations
  behind Ctrl/⌘K, settings in a dialog. The URL hash holds the route (`#/models`,
  `#/chat/<id>`). Setup is the Models page in setup mode until a model is chosen.
- Integrations come from the daemon's catalog (`GET /v1/integrations`); list new ones
  there with status `coming_soon` until their connection flow exists, then add their id
  to `AVAILABLE` in `crates/core/src/integrations.rs`.

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
  `HEARTH_DEV_TOOLS=1`, for exercising approval cards against a scripted model.

## Commands

```sh
pnpm dev                 # daemon + desktop app together, data in .dev/
pnpm web                 # build the frontend and open it in a browser (daemon running)
pnpm dev:daemon          # daemon only (pnpm dev:app for the app only)
pnpm hearth <args>       # CLI against the .dev/ instance
pnpm check               # fmt, clippy, tests, typecheck (what CI runs)
HEARTH_HOME=/tmp/h1 ...  # any other isolated instance
```

When testing `pnpm dev` from an agent session, stop it by exact PID or by the
`setsid` process group. Never `pkill -f` with a pattern that could match other
projects' processes or the current shell's own command line.

To check UI changes visually, run a throwaway daemon (scratch `HEARTH_HOME`,
`HEARTH_KEY_STORE=file`, its own `HEARTH_PORT`), `pnpm build`, and drive the web UI with
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
