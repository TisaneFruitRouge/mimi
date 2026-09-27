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

## Crates

| Path | Crate | Role |
|---|---|---|
| `crates/protocol` | `mimi-protocol` | API types and on-disk paths shared by the daemon and every client. |
| `crates/core` | `mimi-core` | The daemon (`mimid` binary) and all assistant logic. |
| `crates/client` | `mimi-client` | Typed client for the daemon API. Every frontend uses it. |
| `crates/cli` | `mimi-cli` | The `mimi` command. The TUI will live here too. |
| `apps/desktop/src-tauri` | `mimi-desktop` | Tauri shell. Exposes Tauri commands that call `mimi-client`. |
| `apps/desktop/src` | `@mimi/desktop` | React + Tailwind + shadcn/ui frontend. |

## Web interface

The daemon serves the built frontend (`apps/desktop/dist`, via `rust-embed`) at `/`,
with a fallback to `index.html` for client-side routes. Static files are public;
everything under `/v1` requires auth. The frontend picks its transport at runtime
(`src/lib/transport.ts`): Tauri IPC inside the desktop app, same-origin `fetch` and a
WebSocket in a browser.

Browsers can't read the discovery file, so they sign in with a link:

1. A native client (bearer token) calls `POST /v1/web/login-link` and opens the URL
   (`mimi open`, or the desktop app's `open_in_browser` command).
2. `GET /login?code=…` redeems the single-use code (valid two minutes, kept in memory),
   creates a session, sets `mimi_session` (HttpOnly, SameSite=Strict, 30 days) and
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

On start, `mimid`:

1. Creates the data dir with mode `0700`.
2. Refuses to start if another instance's discovery file points at a live port.
3. Binds `127.0.0.1:7437` (`MIMI_PORT` overrides; a random port if it's taken) and
   generates a 256-bit token.
4. Writes `daemon.json` (`pid`, `port`, `token`) to the data dir with mode `0600`, atomically.
5. Deletes `daemon.json` on SIGINT/SIGTERM.

Clients read `daemon.json` and send `Authorization: Bearer <token>`. Outside `/v1`,
only `GET /health` (the version, nothing else), `GET /login` and the static frontend
files are public. Everything under `/v1` needs the token or a browser session (see
above).

The desktop webview never sees the token: the frontend calls Tauri commands, and the
Rust side makes the request.

`MIMI_HOME` overrides all paths, which lets you run isolated instances in development.

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

## Memory

`crates/core/src/memory/`. What the assistant knows about the user, built to stay
small in the prompt however much accumulates. It works like a tiny file system:

| Tier | What | In the prompt |
|---|---|---|
| Profile (`profile.md`) | The few key facts about the user: name, city, language, work, household. Capped at 1,200 characters. | Always |
| Library (`people/sam.md`, `habits/mornings.md`, `preferences/food.md`, `places/…`, `work/…`, `interests/…`, `health/…`, `notes/…`) | Short markdown notes, usually bullet facts, one per person or topic (4,000 characters max each). | Only what's relevant |

- **Storage:** the `memory_notes` table in the encrypted database, with an FTS5 index
  (`memory_fts`, `unicode61 remove_diacritics`) kept in step by triggers. Every change
  first records the note's previous state in `memory_revisions`, so any change can be
  undone.
- **Recall, every message:** the user's message (plus their previous one, for
  follow-ups) becomes an FTS query (stopwords dropped, longer words matched as
  prefixes). The profile and up to 4 matching notes, 1,600 characters at most, go into
  the system prompt inside a delimited `<memory>` block that's labelled as data, not
  instructions.
- **Tools:** `memory_search`, `memory_read` and `memory_list` for anything not already
  recalled, and `memory_write` (add facts, de-duplicated), `memory_update` (rewrite a
  note) and `memory_forget` (remove facts or a note). None needs approval, since memory is
  local and private. Writes show in the chat as one quiet line ("Remembered that Sam is
  your brother") with Undo (`POST /v1/memory/undo/{revision}`). Reads aren't shown.
- **Learning in the background:** after a reply, a conversation is scheduled to be
  learned from once it's been quiet for 2 minutes (`MIMI_MEMORY_QUIET_SECS`).
  - **The pass:** the active model gets the current profile, the list of notes, the
    notes related to the conversation, and the new messages since the last pass
    (`memory_learned`). It replies with a JSON plan of facts to add or remove per note,
    which is applied with de-duplication and the profile cap.
  - **Sources:** only the user's own messages count as sources; the assistant's replies
    are context.
  - **Exclusions:** a message where the user asks not to remember something is left out
    before the model sees it.
  - **Restarts:** unread conversations are picked up after a restart.
- **Never stored:** anything that looks like a secret (passwords, codes, card or
  account numbers, keys). This is checked on every write path, including the Memory
  screen.
- **The user in control:**
  - The Memory screen (`#/memory`, from Settings) shows and edits the profile and every
    note.
  - "Learn from conversations" can be paused (`Settings.memory_learning`). Paused, the
    writing tools disappear and nothing said meanwhile is learned later.
  - "Forget everything" deletes it all.
  - When a cloud model is active, the screen says that relevant memories are sent with
    messages.
- **People:** knowledge about people lives in `people/<name>.md`. Notes have an optional
  `subject` column, unused for now, as the seam for linking them to contact ids from the
  contacts directory.
- **API:** `GET /v1/memory`, `GET|PUT|DELETE /v1/memory/note?path=`,
  `PUT /v1/memory/profile`, `PUT /v1/memory/learning`, `POST /v1/memory/undo/{revision}`,
  `POST /v1/memory/forget-all`. Changes publish `memory_changed`.

## People and @ mentions

`crates/core/src/people/` keeps one directory of people, unified across sources.

- **Model:** a person (`people`) has contact cards from sources (`person_records`:
  source = connection id, record = the card's id there) and handles
  (`person_handles`: channel phone/email/telegram/signal/whatsapp/matrix, value, label,
  and the card it came from, or none when the user added it). People the user adds are
  `manual` and survive syncs. Person ids are stable (merges keep the first id), so
  other features can link to them, such as memory notes' `subject`.
- **Sources:** `ContactSource` implementations return every card, per connection;
  `people::sync_all` works out what changed. Today: address books (CardDAV) on connected
  CalDAV accounts (iCloud → contacts.icloud.com, Fastmail → carddav.fastmail.com,
  others on the same server). Syncs run at startup, every 30 minutes, when connections
  change, and on `POST /v1/people/sync`. A removed connection takes its cards along.
- **Unification:** a new card joins the person who already has one of its match keys
  (email lowercased; phone digits with `+`/`00` as international, national numbers kept
  as they are; Signal/WhatsApp numbers are phones; Telegram usernames). Never on name:
  same-name people become "possible duplicates" (`GET /v1/people/duplicates`), which
  the user merges or dismisses. A card, once placed, stays with its person, so a split
  (`POST /v1/people/{id}/split`) survives later syncs.
- **Mentions:** `GET /v1/mentions?q=` suggests people and events (upcoming 30 days;
  with a query, the past month to six months ahead). Event ids encode calendar, uid and
  start (`ev:<ms>:<hex calendar>:<hex uid>`). `SendMessage.mentions` keeps those whose
  `@label` is still in the text; the engine resolves them into a `<mentioned>` block
  appended to the user's message for the model (who they are and every way to reach
  them; the event's time, calendar and place), marked as data rather than
  instructions. The block is stored with the message (`mention_context`) and replayed
  in later turns.
- **Assistant tools:** `people_search` and `person_details`, reads without approval.
- **UI:** the composer is still a textarea. Typing `@` opens the picker
  (`features/chat/mentions/`), picked items become `@label` text tracked as tokens and
  drawn as pills behind the text, and a token deletes as a whole. People live at
  `#/people`, reached from Connections.

## Planned

- **Background service**: install `mimid` as a systemd user unit (Linux) or a launchd
  agent (macOS) from the GUI, so messaging channels and scheduled tasks work with the
  window closed.
- **Models**: bundle and manage `llama-server` (Metal on macOS, Vulkan on Linux); also
  connect to Ollama or any OpenAI-compatible endpoint. Cloud providers are allowed but
  never the default, and are labeled wherever they're in use. Recommendations come from
  detected hardware: cloud models on weak machines, large local models on big GPUs, and
  models the user already has installed.
- **Semantic memory search**: optional local embeddings (e.g. an Ollama embedding model)
  alongside FTS5, stored as plain vectors and compared in Rust (no loadable SQLite
  extensions: the workspace forbids unsafe code).
- **Integrations**: MCP. Each tool declares its capabilities (hosts, paths, outbound
  messaging) and the user approves them. Untrusted plugins run sandboxed.
- **Remote access**: a separate, opt-in listener with its own device pairing and
  authentication. The loopback listener stays loopback-only.
