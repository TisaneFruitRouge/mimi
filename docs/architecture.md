# Architecture

```
 ┌──────────────┐  ┌──────────┐  ┌──────────┐   ┌─────────────┐
 │ Desktop app  │  │   CLI    │  │ TUI (tbd)│   │  Browser    │  clients
 │ Tauri + React│  │ `hearth` │  │          │   │ same React  │
 └──────┬───────┘  └────┬─────┘  └────┬─────┘   └──────┬──────┘
        │   hearth-client (typed Rust client)          │ fetch + WebSocket,
        └───────────────┼─────────────┘                │ session cookie
                        │ HTTP on 127.0.0.1:7437,      │
                        │ bearer token                 │
                ┌───────┴──────────────────────────────┴┐
                │    hearthd     │      hearth-core
                │ agent · memory │
                │ models · tools │
                │ scheduler      │
                └────────────────┘
```

## Crates

| Path | Crate | Role |
|---|---|---|
| `crates/protocol` | `hearth-protocol` | API types and on-disk paths shared by the daemon and every client. |
| `crates/core` | `hearth-core` | The daemon (`hearthd` binary) and all assistant logic. |
| `crates/client` | `hearth-client` | Typed client for the daemon API. Every frontend uses it. |
| `crates/cli` | `hearth-cli` | The `hearth` command. The TUI will live here too. |
| `apps/desktop/src-tauri` | `hearth-desktop` | Tauri shell. Exposes Tauri commands that call `hearth-client`. |
| `apps/desktop/src` | `@hearth/desktop` | React + Tailwind + shadcn/ui frontend. |

## Web interface

The daemon serves the built frontend (`apps/desktop/dist`, via `rust-embed`) at `/`,
with a fallback to `index.html` for client-side routes. Static files are public;
everything under `/v1` requires auth. The frontend picks its transport at runtime
(`src/lib/transport.ts`): Tauri IPC inside the desktop app, same-origin `fetch` and a
WebSocket in a browser.

Browsers can't read the discovery file, so they sign in with a link:

1. A native client (bearer token) calls `POST /v1/web/login-link` and opens the URL
   (`hearth open`, or the desktop app's `open_in_browser` command).
2. `GET /login?code=…` redeems the single-use code (valid two minutes, kept in memory),
   creates a session, sets `hearth_session` (HttpOnly, SameSite=Strict, 30 days) and
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

On start, `hearthd`:

1. Creates the data dir with mode `0700`.
2. Refuses to start if another instance's discovery file points at a live port.
3. Binds `127.0.0.1:7437` (`HEARTH_PORT` overrides; a random port if it's taken) and
   generates a 256-bit token.
4. Writes `daemon.json` (`pid`, `port`, `token`) to the data dir with mode `0600`, atomically.
5. Deletes `daemon.json` on SIGINT/SIGTERM.

Clients read `daemon.json` and send `Authorization: Bearer <token>`. Outside `/v1`,
only `GET /health` (the version, nothing else), `GET /login` and the static frontend
files are public. Everything under `/v1` needs the token or a browser session (see
above).

The desktop webview never sees the token: the frontend calls Tauri commands, and the
Rust side makes the request.

`HEARTH_HOME` overrides all paths, which lets you run isolated instances in development.

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

## Planned

- **Background service**: install `hearthd` as a systemd user unit (Linux) or a launchd
  agent (macOS) from the GUI, so messaging channels and scheduled tasks work with the
  window closed.
- **Models**: bundle and manage `llama-server` (Metal on macOS, Vulkan on Linux); also
  connect to Ollama or any OpenAI-compatible endpoint. Cloud providers are allowed but
  never the default, and are labeled wherever they're in use. Recommendations come from
  detected hardware: cloud models on weak machines, large local models on big GPUs, and
  models the user already has installed.
- **Storage**: SQLite with SQLCipher, `sqlite-vec` for semantic memory, FTS5 for keyword
  search. Secrets in the OS keychain.
- **Integrations**: MCP. Each tool declares its capabilities (hosts, paths, outbound
  messaging) and the user approves them. Untrusted plugins run sandboxed.
- **Remote access**: a separate, opt-in listener with its own device pairing and
  authentication. The loopback listener stays loopback-only.
