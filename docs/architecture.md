# Architecture

```
 ┌──────────────┐  ┌──────────┐  ┌──────────┐
 │ Desktop app  │  │   CLI    │  │ TUI (tbd)│      clients
 │ Tauri + React│  │ `hearth` │  │          │
 └──────┬───────┘  └────┬─────┘  └────┬─────┘
        │   hearth-client (typed Rust client)
        └───────────────┼─────────────┘
                        │ HTTP on 127.0.0.1, bearer token
                ┌───────┴────────┐
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

## Daemon discovery and auth

On start, `hearthd`:

1. Creates the data dir with mode `0700`.
2. Refuses to start if another instance's discovery file points at a live port.
3. Binds `127.0.0.1` on a random port and generates a 256-bit token.
4. Writes `daemon.json` (`pid`, `port`, `token`) to the data dir with mode `0600`, atomically.
5. Deletes `daemon.json` on SIGINT/SIGTERM.

Clients read `daemon.json` and send `Authorization: Bearer <token>`. Only `GET /health`
is unauthenticated, and it returns nothing but the version. Everything else lives under
`/v1` behind the token.

The desktop webview never sees the token: the frontend calls Tauri commands, and the
Rust side makes the request.

`HEARTH_HOME` overrides all paths, which lets you run isolated instances in development.

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
