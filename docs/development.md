# Development

How to build, run and test Mimi while working on it, and the conventions every change
follows. The short, user-facing version of the commands is in the
[README](../README.md#development); how the pieces fit together is in
[Architecture](architecture.md).

## Rules

- `pnpm check` (fmt, clippy, tests, typecheck) is what CI runs; keep it passing. It also
  fails when the committed TypeScript bindings are stale (see
  [Architecture](architecture.md)).
- **Never disturb the user's own session.** Never install, start or stop the plain `mimi`
  unit, and never touch the `pnpm dev` daemon. Test with a scratch `MIMI_HOME`,
  `MIMI_KEY_STORE=file` and a spare `MIMI_PORT` (which also gives the service a hashed
  unit name), and remove what you created. See
  [Daemon lifecycle](daemon-lifecycle.md).
- When testing `pnpm dev` from an agent session, stop it by exact PID or by the `setsid`
  process group. Never `pkill -f` with a pattern that could match other projects'
  processes or the current shell's own command line.
- The dev machine is the user's own desktop, in use while you work. Before sending input
  (`wtype`), confirm the app window is focused (`hyprctl activewindow`), and only
  screenshot it while it is visible: a window-region `grim` capture of a hidden window
  captures whatever the user is looking at. Prefer verifying behaviour through the
  API/CLI.
- Tests fake the outside world. Never point tests at Google, at matrix.org or at a real
  mailbox (see [Area test recipes](#area-test-recipes)).
- Check that new dependencies have licenses compatible with AGPL-3.0-or-later.
- Never commit binaries: the bundle's `binaries/` and `resources/llama/` folders are
  gitignored (see [Packaging](packaging.md)).

## Requirements

Rust (stable), Node 22+, pnpm, and `protoc` (the Protocol Buffers compiler, for Signal's
libraries: `protobuf-compiler` on Debian/Ubuntu, `protobuf` on Homebrew and Arch). On
Linux, also the [Tauri system dependencies](https://v2.tauri.app/start/prerequisites/#linux).

On the dev machine Rust comes from mise: `eval "$(mise env -s bash)"` if `cargo` is not on
`PATH`.

## Commands

```sh
pnpm install
pnpm dev                 # daemon (port 7438) + "Mimi Dev" app together, data in .dev/
pnpm web                 # build the frontend and open it in a browser (daemon running)
pnpm dev:daemon          # daemon only (pnpm dev:app for the app only)
pnpm mimi <args>         # CLI against the .dev/ instance (e.g. pnpm mimi status)
pnpm mimi open           # open the web UI in a browser, signed in (after a build)
mimi service status      # background service (install | uninstall | status | start | restart)
pnpm check               # fmt, clippy, tests, typecheck (what CI runs)
pnpm bundle              # this platform's installers, in target/release/bundle/
MIMI_HOME=/tmp/h1 ...    # any other isolated instance
```

- `pnpm dev` runs next to an installed Mimi: its own data (`.dev/` at the repo root), port
  (7438) and app identity (`src-tauri/tauri.dev.conf.json`, "Mimi Dev"). Closing the app or
  Ctrl+C stops both. It sets `MIMI_DAEMON=external` for the app, so the app only connects
  and never starts, stops or installs a daemon (see
  [Daemon lifecycle](daemon-lifecycle.md)).
- The built-in model runtime needs `llama-server`: `scripts/fetch-llama-server.sh`, then
  `MIMI_LLAMA_SERVER=$PWD/apps/desktop/src-tauri/resources/llama/llama-server pnpm dev`.
  Without it the built-in source doesn't appear and Mimi suggests Ollama instead (see
  [Models](models.md)).
- To install the current code as the real app (`pnpm bundle`, then
  `scripts/install.sh --file …`), see [Packaging › Building locally](packaging.md#building-locally).

## Scratch daemons

`MIMI_HOME` overrides all paths, so any directory makes an isolated instance. A
throwaway daemon that can't touch the user's own:

```sh
export MIMI_HOME=$(mktemp -d) MIMI_KEY_STORE=file MIMI_PORT=7493 MIMI_NO_NOTIFICATIONS=1
target/debug/mimid
pnpm web          # in the same shell: opens the web UI of the daemon in $MIMI_HOME
```

- `MIMI_KEY_STORE=file` keeps the database key in an owner-only file instead of the OS
  keychain; `MIMI_PORT` is any spare port; `MIMI_NO_NOTIFICATIONS=1` keeps it from showing
  desktop notifications.
- Without the UI, call the API with the bearer token from `$MIMI_HOME/daemon.json`.
- When done, stop the daemon and delete `$MIMI_HOME`.

## Checking UI changes

To check UI changes visually, run a throwaway daemon (above), `pnpm build`, and drive the
web UI with `node scripts/ui-check.mjs` (headless Chromium: screenshots, clicks, keys,
computed styles). Chromium's one-shot `--screenshot` flag can capture transitions
mid-way; use the script instead.

## Environment variables

| Variable | What it does | More |
|---|---|---|
| `MIMI_HOME` | Data directory; overrides all paths | [Architecture](architecture.md) |
| `MIMI_PORT` | Daemon port (default 7437; `pnpm dev` uses 7438) | [Architecture](architecture.md) |
| `MIMI_KEY_STORE=file` | Database key in an owner-only file, not the keychain | |
| `MIMI_DAEMON=external` | The app only connects to a daemon (set by `pnpm dev`) | [Daemon lifecycle](daemon-lifecycle.md) |
| `MIMI_DAEMON_BIN` | Which `mimid` to run | [Daemon lifecycle](daemon-lifecycle.md#finding-mimid) |
| `MIMI_BUILD_ID` | Build id reported by `/health` (set by `pnpm bundle` and the release workflow) | [Daemon lifecycle](daemon-lifecycle.md#after-an-update) |
| `MIMI_LLAMA_SERVER` | Which `llama-server` the built-in runtime uses | [Models](models.md#finding-llama-server) |
| `MIMI_GOOGLE_CLIENT_ID` / `_SECRET` | Google sign-in app identity, at build or run time | [Google OAuth](google-oauth.md), [Calendar](calendar.md) |
| `MIMI_NO_NOTIFICATIONS=1` | No desktop notifications (tests never show one) | [Reminders and routines](reminders-and-routines.md), [Email](email.md#new-mail-notifications) |
| `MIMI_DEV_TOOLS=1` | Debug builds: the fake tools `dev_lookup` and `dev_send_note` | [Tools and approvals](tools-and-approvals.md) |
| `MIMI_MEMORY_QUIET_SECS` | Quiet time before a conversation is learned from (default 2 minutes) | [Memory](memory.md) |
| `MIMI_TELEGRAM_API` | Telegram Bot API base, e.g. a fake bot server | [Messaging](messaging.md) |
| `MIMI_TEST_CALDAV` | CalDAV server for the live CalDAV test | [Connections › Tests](connections.md#tests) |
| `MIMI_TEST_MATRIX` | Homeserver for the live Matrix test | [Matrix](matrix.md) |
| `MIMI_FAKE_MAIL_SEED=1` | Seeded mail for the fake mail server | [Email](email.md) |

## Conventions

- Rust 2024 edition, `unsafe_code` forbidden workspace-wide, clippy clean with
  `-D warnings`.
- matrix-sdk pulls in `decancer`, whose `AddAssign` impl for `String` breaks `s += &string`
  inference in mimi-core: write `s += &*string` or `push_str` (see [Matrix](matrix.md)).
- Frontend: React 19, Tailwind v4, shadcn/ui (add components with
  `pnpm dlx shadcn@latest add <name>` from `apps/desktop`), lucide icons. Light theme only
  for now. See [Frontend](frontend.md) and [Design system](design-system.md).
- API types, the generated TypeScript bindings and database migrations follow the rules in
  [Architecture](architecture.md).
- Keep the product name confined to identifiers and strings, so a rename stays a
  find-and-replace (it was renamed from Hearth once already).
- License: AGPL-3.0-or-later. Check that new dependencies have compatible licenses.

## Area test recipes

Each area's own doc has the details; these are the ones that need setup or env vars.

- **Daemon lifecycle**: the opt-in test that runs real processes without a window
  (`cargo test -p mimi-desktop lifecycle -- --ignored` with a scratch `MIMI_HOME`,
  `MIMI_PORT`, `MIMI_KEY_STORE=file` and `MIMI_DAEMON_BIN`). See
  [Daemon lifecycle › Tests](daemon-lifecycle.md#tests).
- **CalDAV** against Radicale (`uvx radicale --auth-type=none`,
  `MIMI_TEST_CALDAV=… cargo test -p mimi-core live_caldav -- --ignored`), Google against
  `calendar/google_fake.rs`, Telegram against `fake_telegram`, iCal parsing with public
  Google holiday feeds. See [Connections › Tests](connections.md#tests) and
  [Calendar](calendar.md).
- **Matrix** against a throwaway Synapse with open registration
  (`MIMI_TEST_MATRIX=… cargo test -p mimi-core live_matrix -- --ignored`). See
  [Matrix](matrix.md) and [Connections › Tests](connections.md#tests).
- **Signal** for real needs a phone and a scratch daemon, never the user's own. See
  [Signal](signal.md).
- **Email**: the fake IMAP/SMTP server (`mail/fake.rs`), seeded mail to try the app by hand
  (`MIMI_FAKE_MAIL_SEED=1 cargo test -p mimi-core fake_mail_server -- --ignored`), server
  discovery against real domains (`live_discovery`), Jev against a fake. See
  [Email](email.md).
- **Memory**: `MIMI_MEMORY_QUIET_SECS=15` so learning runs soon after a chat;
  `semantic::tests::FakeEmbedder` instead of a real embedding model. See
  [Memory](memory.md).
- **Reminders and routines**: a scratch daemon with `MIMI_NO_NOTIFICATIONS=1` and
  `MIMI_TELEGRAM_API` pointed at a fake bot server; `tick(state, now)` takes the clock. See
  [Reminders and routines](reminders-and-routines.md).
- **Approval cards** against a scripted model with `MIMI_DEV_TOOLS=1` (debug builds). See
  [Tools and approvals](tools-and-approvals.md).
- **Guest boundary**: `api/tests/access_flow.rs`. See [Trusted people](trusted-people.md).
