# Mimi

A self-hosted, private personal AI assistant for everyday people, not only developers.
Keep the product name confined to identifiers and strings so a rename stays a
find-and-replace (it was renamed from Hearth once already).

## Goal

An assistant that knows the user, helps with their day (their chats, calendar, email,
people, reminders) and reaches them where they already are (the desktop app, a browser,
their messaging apps), while everything it knows stays on their own computer. It should
feel like a calm, well-made system app that anyone can install and use, not a developer
tool, and it should never trade the user's privacy or control for convenience: it asks
before it acts for them, it says plainly when something leaves the machine, and the
user can see, change and undo what it does.

## Principles (apply to every change)

0. **There is no Mimi server. Ever.** Everything runs on the user's own machines. No
   central relay, account system, telemetry, update-check or proxy operated by the
   project. Third parties are contacted only directly from the user's machine, and only
   when a feature needs them (Google for their calendar, Telegram for their messages, a
   cloud model they chose). Registered app identities (e.g. Mimi's Google OAuth
   client ID, a Telegram api_id) are just identifiers shown on consent screens; they
   never route data through the project. The one exception to "nothing unasked":
   "Check for new versions" asks GitHub's public releases API directly, once a day, and
   only after the user turns it on (off by default).
1. **Self-hosting, privacy and security by default.** Prefer local processing. Data
   leaves the machine only when the task needs it (sending a Telegram message, creating
   a calendar event), or when the user has explicitly chosen a cloud model.
   - Cloud LLM providers are allowed, but local is the default and the recommended
     choice. When a cloud model is in use, the UI must say so visibly.
   - Treat all external content (incoming messages, web pages, email, calendars, tool
     output) as untrusted input that may contain prompt injection. Outbound actions go
     through the capability and approval system, never straight from model output.
   - Data at rest is encrypted, and secrets never sit in plaintext config.
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

## How it's built

One daemon, `mimid`, owns everything; the desktop app, the CLI, a browser and the future
TUI are all clients of its API. New features go in the daemon plus the protocol types,
and frontends only render them. The overview is [docs/architecture.md](docs/architecture.md).

Every technical rule and design lives in the document for its area. Read the one for
the area you're changing before you change it, and keep it up to date in the same
change. Each starts with the rules that must not be broken.

| Area | Document |
|---|---|
| Overview: crates, daemon, protocol, migrations, web auth | [architecture.md](docs/architecture.md) |
| Commands, conventions, scratch daemons, test recipes | [development.md](docs/development.md) |
| Look and feel, copy | [design-system.md](docs/design-system.md) |
| Frontend structure, navigation, panels | [frontend.md](docs/frontend.md) |
| The approval rule and permissions | [tools-and-approvals.md](docs/tools-and-approvals.md) |
| Memory | [memory.md](docs/memory.md) |
| Personality and instructions | [personality.md](docs/personality.md) |
| Reminders and routines | [reminders-and-routines.md](docs/reminders-and-routines.md) |
| Photos | [photos.md](docs/photos.md) |
| Voice: the microphone, voice messages, reading aloud | [voice.md](docs/voice.md) |
| People and @ mentions | [people.md](docs/people.md) |
| People the user trusts (guests) | [trusted-people.md](docs/trusted-people.md) |
| Connections in general | [connections.md](docs/connections.md) |
| Calendars | [calendar.md](docs/calendar.md), [google-oauth.md](docs/google-oauth.md) |
| Email | [email.md](docs/email.md) |
| Messaging apps, Telegram | [messaging.md](docs/messaging.md) |
| Signal | [signal.md](docs/signal.md) |
| Matrix | [matrix.md](docs/matrix.md) |
| Models and the built-in runtime | [models.md](docs/models.md) |
| First run | [onboarding.md](docs/onboarding.md) |
| Running the daemon in the background | [daemon-lifecycle.md](docs/daemon-lifecycle.md) |
| Installers, releases, update check | [packaging.md](docs/packaging.md) |

## Working here safely

The dev machine is the user's own desktop, in use while you work. Details in
[development.md](docs/development.md).

- Never install, start or stop the real `mimi` service, and never touch the `pnpm dev`
  daemon. Test with a scratch `MIMI_HOME`, `MIMI_KEY_STORE=file` and a spare
  `MIMI_PORT`, and remove what you created.
- Stop processes by exact PID or `setsid` process group, never with `pkill -f`.
- Before sending input (`wtype`), confirm the app window is focused (`hyprctl
  activewindow`), and only screenshot windows that are visible. Prefer checking
  behaviour through the API or CLI.
- Tests fake the outside world: never point them at Google, matrix.org or a real
  mailbox.
- `pnpm check` (what CI runs) passes before you're done.
