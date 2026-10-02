# Frontend

The React frontend (`apps/desktop/src`, package `@mimi/desktop`: React 19, Tailwind v4,
shadcn/ui, lucide icons) runs in two places: inside the Tauri desktop app
(`apps/desktop/src-tauri`, crate `mimi-desktop`) and in a browser, served by the daemon.
Like every client it only renders what the daemon owns: new features go in the daemon
plus the protocol types (see [Architecture](architecture.md)). How it looks is in
[Design system](design-system.md).

## Rules

- The desktop webview calls Tauri commands. It never talks to the daemon or holds the
  token directly: the `api` command proxies `/v1` requests, and the Rust side relays
  daemon events to the webview as `daemon-event` / `daemon-connection`.
- Only `src/lib/transport.ts` knows whether it runs in the app or a browser. Never call
  Tauri APIs from screens directly; use `openExternal` for links.
- API types come from `mimi-protocol` through the generated `src/bindings/` (see
  [Architecture](architecture.md)); don't hand-write them.
- Apply daemon events to the TanStack Query cache in `src/lib/events.ts`; prefer that
  over refetching.
- Model output is rendered with react-markdown (no raw HTML); links open in the system
  browser.
- "Ask … about this" buttons build a `Draft` with `mentionDraft` and call the shell's
  `onAsk`; they say the user's assistant name (`useAssistantName`), not "Mimi".
- `features/mail/mail-view.tsx` belongs to the email feature; the shell only passes it
  `{ onSection, onAsk, onOpenPerson }`.
- Per-viewer conveniences go in `localStorage` wrapped in try/catch; the page must work
  without them.
- The GUI must be able to do everything the CLI/TUI can.

## Desktop app and browser

The same frontend runs in both:

- **Desktop app**: the webview calls Tauri commands. The `api` command proxies `/v1`
  requests through `mimi-client` with the bearer token, which the webview never sees;
  the Rust side relays daemon events as `daemon-event` and the connection state as
  `daemon-connection`. Other commands cover what a webview can't do itself
  (`open_in_browser`, `chat_attachment`, `open_attachment`, `window_chrome`, …).
- **Browser**: the daemon serves the built frontend (`rust-embed` of
  `apps/desktop/dist`: read from disk in debug builds, embedded in release) and the page
  uses same-origin `fetch` and a WebSocket, authenticated by the `mimi_session` cookie
  (see [Architecture › Web interface](architecture.md#web-interface)).

`src/lib/transport.ts` is the only place that knows which: Tauri IPC in the app,
same-origin `fetch`/WebSocket in a browser. Everything that differs goes through it:
requests, events, `openExternal` for links, window controls (`windowAction`,
`windowChrome`), and photos (`attachmentUrl`; see [Photos](photos.md)). Screens never
call Tauri APIs directly.

Requests an older daemon can't read surface as `DaemonError.code === "outdated"` with a
plain message (see [Running in the background](daemon-lifecycle.md)).

## Code layout

- `src/lib/api.ts`: typed calls and query keys.
- `src/lib/events.ts`: applies daemon events to the TanStack Query cache. Prefer this
  over refetching.
- `src/lib/transport.ts`: the app or the browser (above).
- `src/lib/draft.ts`: drafts handed to a new chat ("Ask … about this").
- `src/features/<area>/`: screens (`chat`, `calendar`, `mail`, `people`, `personality`,
  `onboarding`, `shell`, …).
- `src/components/`: shared pieces (see [Design system](design-system.md)).
- `src/bindings/`: TypeScript types generated from `mimi-protocol`, committed.

Model output is rendered with react-markdown, with no raw HTML; links open in the system
browser.

## Navigation

Types `Tab`, `SettingsPage` and `Section` are in `src/features/shell/top-bar.tsx`.

- **Panels**: the daily panels are a segmented control in the translucent top bar:
  **Chat** (home), **Calendar**, **Mail** and **People**, ⌘/Ctrl 1–4.
- **Settings**: everything set up once lives in the Settings window (gear button or
  ⌘/Ctrl ,): a System-Settings-style sidebar with General, Personality, Connections,
  Models, Voice, Memory, Reminders & notifications, Permissions, Privacy (`SettingsPage`:
  `general`, `personality`, `connections`, `models`, `voice`, `memory`,
  `notifications`, `permissions`, `privacy`).
- **Search** (⌘/Ctrl K): conversations are behind it, and it also jumps to every panel
  and settings page.
- **New chat**: ⌘/Ctrl N.
- **Routes**: the URL hash holds the route: `#/chat/<id>`, `#/calendar`, `#/mail`,
  `#/people/<id>`, `#/settings/<page>`. Old addresses (`#/models`, `#/reminders`, …) are
  rewritten to their new place.
- **Chat is home.**
- **Reminders and routines** live in Calendar (side list, bells in the grid); where
  they're delivered is Settings › Reminders & notifications (see
  [Reminders and routines](reminders-and-routines.md)), which also has New email
  (`features/mail/mail-notifications.tsx`; see
  [Email](email.md#new-mail-notifications)).
- **A click on a new-mail notification** arrives as the `open_mail` event: the app's
  relay shows the window, and `lib/events.ts` opens Mail on the conversation
  (`showMailThread`, which also reaches a Mail panel that's already showing). The
  browser transport drops the event: it's for the app on the daemon's computer.
- **First run** is the onboarding (see [Onboarding](onboarding.md)), which may name
  models like the Models page. The Models page in setup mode remains the fallback when a
  finished setup has lost its model.

## Ask about this

Every panel can hand something to Chat with "Ask … about this": a new chat opens with
the thing already @-mentioned.

- Build a `Draft` (text plus its `Mention`s) with `mentionDraft(kind, id, label)`
  (`lib/draft.ts`) and call the shell's `onAsk`.
- The shell opens a new chat, and the chat takes the draft once, on mount
  (`ComposerHandle.setDraft`), so the thing arrives as an @ pill the engine resolves like
  any other mention (see [People › Mentions](people.md)).
- The button says the user's assistant name (`useAssistantName`), not "Mimi".

## Panels

### Calendar

`features/calendar/`. The calendar APIs it uses are in [Calendar](calendar.md); how
events are drawn is in [Design system › Panels](design-system.md#panels).

- A week grid (all-day row, overlapping events side by side, the "now" line) or a
  four-week list, every calendar merged and coloured, with calendars shown or hidden per
  viewer.
- Reminders and routines sit beside it (edit, pause, run now, delete) and inside it:
  timed ones as a chip at every time they go off in view (`GET
  /v1/schedule/occurrences`; past ones greyed, missed ones struck through, frequent ones
  once a day), event-relative ones as a bell on their event.
- An event's sheet shows when, where, who (matched to People), the invitation's notes as
  plain text, "Remind me before…" (a `before_event` reminder on that occurrence) and
  "Ask … about this".
- New events go straight into CalDAV calendars and Google calendars signed in with
  Google; for Google calendars read through their private address, the pre-filled Google
  page opens. Adding, changing or deleting an event from the panel is the user's own
  action, so it needs no approval card.
- Its right-click menus are listed in
  [Design system › Right-click menus](design-system.md#right-click-menus).

### People

`features/people/`: master–detail (see [People](people.md) for the directory and its
API).

- The list (search, possible duplicates) on the left.
- A person's page shows how to reach them, what memory holds about them (`GET
  /v1/people/{id}/memory`), what's coming up with them (`GET /v1/people/{id}/events`:
  events they're invited to by email, or whose title names them; recurring ones once),
  recent email (`GET /v1/mail/threads?person=<id>`), and chats where they were
  @-mentioned (`GET /v1/people/{id}/conversations`). Sections whose API isn't there
  (older daemon, no mailbox) say so quietly or stay hidden.
- "Delete contact…" is in the person's "…" menu, their right-click menu and on the
  Delete key (`features/people/delete-person.tsx`): a confirmation that says it's from
  Mimi only, then the next person in the list is selected and a toast offers Undo. The
  list's "…" menu opens "Removed contacts", where each can be brought back.
- Merging, and the card that lets someone use the assistant
  (`features/people/assistant-access.tsx`), are described in [People](people.md) and
  [People the user trusts](trusted-people.md).

### Mail

`features/mail/mail-view.tsx`, with props `{ onSection, onAsk, onOpenPerson }`. The
email feature owns that file; see [Email](email.md).

- Several conversations are chosen as in the People list (⌘/Ctrl-click, Shift-click) and
  acted on together (`features/mail/selection.tsx`).
- Mail has its own keyboard shortcuts while it's shown (`features/mail/shortcuts.tsx`, a
  window `keydown` listener that steps aside while typing or with a dialog or menu open,
  and leaves the app's ⌘/Ctrl shortcuts alone); `?` lists them. See
  [Email › Keyboard shortcuts](email.md#keyboard-shortcuts).

### Chat

The chat screen, the composer and its @ / # mentions are in `features/chat/`; see
[People › Mentions](people.md) for the mention logic and [Photos](photos.md) for photos
in the composer (the Tauri windows set `dragDropEnabled: false`, so drops reach the
webview as in a browser).

## Per-viewer conveniences

Things that belong to one viewer rather than the account (the calendar view, hidden
calendars, how an email is shown) go in `localStorage`, wrapped in try/catch. The page
must work without them.
