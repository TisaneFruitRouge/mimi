# Using Mimi from a phone

Phones reach the daemon peer to peer with [iroh](https://iroh.computer) (QUIC dialled by
public key; `crates/core/src/remote/`), and the iPhone app (`apps/ios/`) is a client of
the same `/v1` API.

## Rules

- There is no Mimi server: the phone connects straight to the computer when the networks
  allow it, else through a relay that forwards encrypted QUIC packets it can't read. By
  default the relays and address lookup are number 0's public ones (iroh's makers), the
  one third party this feature adds; it's disclosed in Settings › Phone, which can point
  at the user's own `iroh-relay` instead.
- The endpoint runs only while a phone is paired or a pairing code waits. The loopback
  listener is unchanged: phones never use it.
- Requests over iroh accept only a phone token issued to that very key (`Auth::Device`),
  never the local token or cookies. Keep anything that changes how phones reach the
  computer (relay, codes, login links) refused to `Auth::Device`.
- The phone's event feed hides trusted people's conversations exactly like the
  WebSocket does (`api/events.rs › next_event`).
- Tests: `remote/tests.rs` (real endpoint, fake phones, relays off). Never point tests at
  number 0's relays except the ignored `live_pairing`.

## How it works

- **When it runs**: the endpoint exists only while a phone is paired or a pairing code is
  waiting (`remote::sync`). Its secret key is in the `settings` row `remote` (encrypted
  with the database), so the computer keeps its address across restarts.
- **Pairing**: `POST /v1/remote/pairing` (this computer only: bearer or browser session)
  makes a one-time code, valid 10 minutes, and returns
  `mimi://pair?id=<endpoint id>&code=<code>[&relay=<url>]` with its QR code (SVG). The
  phone dials the id with ALPN `mimi/1` and calls `POST /v1/remote/pair {code, name}`; the
  daemon stores the phone's endpoint id with a SHA-256 of a fresh token (`devices`,
  migration 0034) and returns the token once. While no code waits, connections from
  unknown keys are closed at once.
- **Auth**: requests over iroh carry a `remote::Peer` extension (only `wire.rs` sets it).
  `require_auth` then accepts only a phone token issued to that endpoint id and marks the
  request `Auth::Device(id)`. Pairing codes, the relay and login links are this
  computer's business (403 from a phone); a phone may rename or remove only itself.
  Removing a phone cuts its connections.
- **Wire format** (`remote/wire.rs`): one bidirectional QUIC stream per request. The client
  writes a head (4-byte big-endian length, then JSON `{method, path, headers}`), the body,
  and finishes; the daemon answers `{status, headers}` and the body the same way. Only
  `/v1/*` and `/health` are served. Requests then go through the normal router, so every
  route keeps its own checks.
- **Events**: `GET /v1/events` with `Accept: application/x-ndjson` streams one JSON event
  per line instead of a WebSocket, with a blank line every 25 s so a phone can tell a quiet
  feed from a dead connection.
- **Status**: `GET /v1/remote` (paired phones, whether each is connected directly or
  through the relay, whether the relay is reachable) and `RemoteChanged` events.
- `live_pairing` (ignored) pairs through number 0's real infrastructure with a running
  daemon, like the iPhone app does.

## The iPhone app

- `apps/ios/` (SwiftUI, iOS 26; `apps/ios/README.md`). A client like the desktop app: it
  uses the same `/v1` API over iroh (`DaemonLink`) and renders; features live in the
  daemon. Same look (`Mimi/Design/`: the desktop's tokens, lime used sparingly, the
  mochi), same copy rules, same visible cloud use (`LocalityBadge`; on the phone a
  local model is "On your computer").
- Swift models in `Mimi/Models/` mirror `crates/protocol` by hand: when a protocol type
  the app reads changes, change its mirror (explicit `CodingKeys`, decode only what's
  read, tolerate unknown fields and events). Approval cards use `ActionArguments`, a
  port of `action-formatters.tsx`: keep them in step.
- `project.yml` → `Mimi.xcodeproj` with XcodeGen; commit both. Personal signing lives in
  the ignored `Config/Local.xcconfig`.
- Push notifications aren't there yet (Apple's push needs a key per developer account,
  which doesn't fit "no Mimi server" for everyone). Telegram, Signal and Matrix cover
  reminders away from the app meanwhile.
