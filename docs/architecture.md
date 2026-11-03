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

`mimid` (crates/core) owns everything: conversations, models, memory, connections,
tools, the scheduler. The desktop app, the CLI and the future TUI are all clients that go
through `mimi-client`; the browser is a client too. New features go in the daemon plus
the protocol types, and frontends only render them. This page is the overview; each
area has its own document (see [Areas](#areas)).

## Rules

- **Protocol types.** API types go in `mimi-protocol` and derive `ts_rs::TS` with
  `#[ts(export)]`. `cargo test -p mimi-protocol` regenerates
  `apps/desktop/src/bindings/`, which is committed; CI fails if it's stale. Annotate
  `u64`/`i64` fields `#[ts(type = "number")]`.
- **Migrations.** Schema changes are new files in `crates/core/src/db/migrations/`,
  registered in `MIGRATIONS`. Never edit a migration that has been committed.
- **Data at rest** is encrypted: the SQLite database uses SQLCipher, with its key in the
  OS keychain (or an owner-only file where no keychain exists, reported in `Status`).
  Secrets such as API keys go in that database, never in plaintext config.
- **Loopback.** Listeners bind to loopback unless the user explicitly enables remote
  access.
- **Client-agnostic API.** Multi-device access comes later: don't take shortcuts that
  assume the client is on the same machine.
- **Web auth checks** live in `require_auth`; new `/v1` routes get them automatically.
  Keep them there when adding routes.

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

## Daemon discovery and auth

On start, `mimid`:

1. Creates the data dir with mode `0700`.
2. Refuses to start if another instance's discovery file points at a live port.
3. Binds `127.0.0.1:7437` (`MIMI_PORT` overrides; a random port if it's taken) and
   generates a 256-bit token.
4. Writes `daemon.json` (`pid`, `port`, `token`) to the data dir with mode `0600`, atomically.
5. Deletes `daemon.json` on SIGINT/SIGTERM.

Clients read `daemon.json` and send `Authorization: Bearer <token>`. Outside `/v1`,
only `GET /health` (the version and build id, nothing else), `GET /login` and the static
frontend files are public. Everything under `/v1` needs the token or a browser session
(see below).

The desktop webview never sees the token: the frontend calls Tauri commands, and the
Rust side makes the request (see [Frontend](frontend.md)).

`MIMI_HOME` overrides all paths, which lets you run isolated instances in development.

## Web interface

The daemon serves the built frontend (`apps/desktop/dist`, via `rust-embed`: read from
disk in debug builds, embedded in release) at `/`, with a fallback to `index.html` for
client-side routes. Static files are public; everything under `/v1` requires auth. The
frontend picks its transport at runtime (`src/lib/transport.ts`): Tauri IPC inside the
desktop app, same-origin `fetch` and a WebSocket in a browser.

Browsers can't read the discovery file, so they sign in with a link:

1. A native client (bearer token) calls `POST /v1/web/login-link` and opens the URL
   (`mimi open`, or the desktop app's `open_in_browser` command). Only bearer clients
   can mint links; sessions can't.
2. `GET /login?code=…` redeems the single-use code (valid two minutes, kept in memory),
   creates a session, sets `mimi_session` (HttpOnly, SameSite=Strict, 30 days) and
   redirects to `/`. The database stores only the SHA-256 of the session token.
3. Cookie-authenticated requests must have Host `127.0.0.1:<port>` or
   `localhost:<port>` (DNS-rebinding defense). Non-GET requests and the `/v1/events`
   upgrade must also send a matching Origin (CSRF defense). `POST /v1/web/logout` ends
   the session.

Known limit: browsers share cookies across ports of the same host, so another web app
on `127.0.0.1` could receive the cookie. Such software already runs as the user, so
this doesn't widen what it can reach, but remote access will need a proper origin and
TLS.

## Areas

Read the document for the area you're changing before changing it. Each starts with
the rules that must not be broken, then the design.

**Building and running**

- [Development](development.md): commands, conventions, scratch daemons, UI checks,
  test recipes, environment variables.
- [Running the daemon](daemon-lifecycle.md): who runs `mimid`, the login service, the
  tray, updates of a running install.
- [Models](models.md): model sources, Anthropic, the built-in llama.cpp runtime,
  downloads, internal calls without thinking.
- [Packaging and releases](packaging.md): installers, the release workflow, the install
  script, the opt-in update check.

**The app**

- [Frontend](frontend.md): transport, code layout, navigation and routes, panels.
- [Design system](design-system.md): colours, type, surfaces, components, motion, copy.
- [First run](onboarding.md): the onboarding.

**What the assistant does**

- [Tools and approvals](tools-and-approvals.md): the approval rule, permissions, adding a
  tool.
- [Memory](memory.md): profile, library, recall, learning.
- [Personality and instructions](personality.md).
- [Reminders and routines](reminders-and-routines.md): the scheduler.
- [Photos](photos.md): attachments, which models see them.
- [Voice](voice.md): the microphone, voice messages, reading aloud, on this computer.

**The user's world**

- [Connections](connections.md): the user's own accounts, in general.
- [Calendars](calendar.md), with [Google sign-in](google-oauth.md).
- [Email](email.md).
- [Messaging apps](messaging.md) (the shared layer and Telegram), [Signal](signal.md),
  [Matrix](matrix.md).
- [People and @ mentions](people.md).
- [People the user trusts](trusted-people.md): guests using the assistant.

## Planned

- **Integrations**: MCP. Each tool declares its capabilities (hosts, paths, outbound
  messaging) and the user approves them. Untrusted plugins run sandboxed.
- **Remote access**: a separate, opt-in listener with its own device pairing and
  authentication. The loopback listener stays loopback-only.
