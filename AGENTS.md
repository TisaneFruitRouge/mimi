# Hearth

A self-hosted, private personal AI assistant for everyday people, not only developers.
"Hearth" is a working name. Keep the name confined to identifiers and strings so a
rename is a find-and-replace.

## Principles (apply to every change)

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
- Frontend layout: `src/lib/api.ts` (typed calls + query keys), `src/lib/events.ts`
  (applies daemon events to the TanStack Query cache; prefer this over refetching),
  `src/features/<area>/` for screens, `src/components/` for shared pieces. Model output
  is rendered with react-markdown (no raw HTML); links open in the system browser.
- Every place a model is chosen or used shows its locality (`LocalityBadge`). Cloud use
  must always be visible to the user.

## Commands

```sh
pnpm dev                 # daemon + desktop app together, data in .dev/
pnpm dev:daemon          # daemon only (pnpm dev:app for the app only)
pnpm hearth <args>       # CLI against the .dev/ instance
pnpm check               # fmt, clippy, tests, typecheck (what CI runs)
HEARTH_HOME=/tmp/h1 ...  # any other isolated instance
```

When testing `pnpm dev` from an agent session, stop it by exact PID or by the
`setsid` process group. Never `pkill -f` with a pattern that could match other
projects' processes or the current shell's own command line.

The dev machine is the user's own desktop, in use while you work. Before sending input
(`wtype`) confirm the app window is focused (`hyprctl activewindow`), and only screenshot
it while it is visible: a window-region `grim` capture of a hidden window captures
whatever the user is looking at. Prefer verifying behaviour through the API/CLI.

Rust comes from mise on the dev machine (`eval "$(mise env -s bash)"` if `cargo` is not
on PATH).

## Conventions

- Rust 2024 edition, `unsafe_code` forbidden workspace-wide, clippy clean with `-D warnings`.
- Frontend: React 19, Tailwind v4, shadcn/ui (add components with
  `pnpm dlx shadcn@latest add <name>` from `apps/desktop`), lucide icons. Follow the
  system light/dark setting.
- License: AGPL-3.0-or-later. Check that new dependencies have compatible licenses.
