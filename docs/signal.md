# Signal

Mimi links to the user's own Signal account as a secondary device, like Signal Desktop,
and talks with them in **Note to Self**. The code is in
`crates/core/src/connections/signal/` (`worker.rs`, `classify.rs`, `format.rs`,
`store.rs`). It builds on the shared messaging layer: see [Messaging apps](messaging.md).

## Rules

- presage (<https://github.com/whisperfish/presage>) is AGPL-3.0-only, pinned to a
  revision, without its `cdsi` feature (which needs BoringSSL and a forked SQLite).
  Building needs `protoc`.
- One Signal connection at a time. `ConnectionSetup::Signal {}` (and "Link again") reuse
  it.
- The linking address (`sgnl://linkdevice…`) is the connection's `action_url`, drawn as a
  QR code on this computer (`SignalCode` in `connect-dialogs.tsx`), never opened as a
  link. Expired codes are replaced by themselves; linking pauses after an hour.
- The conversation is Note to Self: `classify.rs` keeps only what the user writes there
  (a "sent" transcript to their own account from another of their devices: text and
  photos) and reactions in it. Other files, other chats, groups, receipts, calls and
  stories are dropped unread and never logged.
- Every message Mimi sends is headed with the assistant's name in bold (in Note to Self
  everything looks like the user's), with Markdown as Signal text styles (`format.rs`).
- Signal doesn't notify for Note to Self: say so (the dialog, the connection's line and
  the welcome do).
- Approvals and reminders are text with `replies::APPROVAL_HINT` / `REMINDER_HINT`,
  answered by a quoted reply or a reaction (`channels::replies`).
- presage runs on its own thread (`worker.rs`: current-thread runtime + `LocalSet`,
  64 MB stack).
- The store (`store.rs`, migration 0024) is Mimi's SQLCipher database, every row keyed
  by connection (`ON DELETE CASCADE`). It keeps keys and sessions only: never messages,
  contacts, profiles or groups of other people (stand-ins stop presage from fetching
  them).
- Credentials refused three times in a row = unlinked from the phone.
- A linked device can't unlink itself: disconnecting deletes everything here and tells
  the user to remove it on the phone.
- presage and libsignal log only errors by default (the `main.rs` filter): at info and
  warning level they log linking codes and other chats' metadata.
- Signal can never carry people the user trusts: it's the owner's own account, so other
  people's Signal chats aren't Mimi's (see [People the user trusts](trusted-people.md)).
- Trying it for real needs a phone and a scratch daemon, never the user's own (see
  [Trying Signal for real](#trying-signal-for-real)).

## Linked device

presage's libraries generate code from `.proto` files, hence `protoc`. The device is
named after the assistant.

## Its own thread

presage uses `spawn_local` and libsignal's stores aren't `Send`, so each connection runs
presage on a dedicated thread (`worker.rs`) with a current-thread Tokio runtime and a
`LocalSet` (64 MB stack: libsignal's futures are several MB unoptimised). The daemon
holds a `Worker` handle (commands in: send these pieces to Note to Self, answered with
their timestamps) and reads `Report`s (a code, linked, online, offline, unlinked, a
message). The running workers are `state.connections.signal`.

## Linking

`ConnectionSetup::Signal {}` saves the one Signal connection (or reuses it: "Link again"
keeps its conversation) and starts linking. presage's provisioning address
(`sgnl://linkdevice?uuid=…&pub_key=…`) becomes the connection's `action_url` with status
`needs_action`, so `ConnectionsChanged` carries it to the dialog, which draws it as a QR
code with the `qrcode` package on this computer. It is never opened as a link.

- When Signal closes a code nobody scanned, a new one follows at once.
- Failing to reach Signal shows "Can't reach Signal right now. Retrying…" and backs off.
- After an hour without a scan linking stops ("Show a new code"); a daemon restart
  within that hour resumes it.
- Once linked, the connection says "Linked as <profile name, else number>", and after the
  first catch-up Mimi greets the user in Note to Self, saying how it works and that its
  messages arrive silently.

## Note to Self is the conversation

What the user writes in Note to Self reaches the linked device as a "sent" transcript
from another of their devices, addressed to their own account. `classify.rs` keeps
exactly that (and a plain message to themselves, which some apps may send) plus
reactions in Note to Self. Messages from this device, or with a timestamp Mimi used (the
last 256), are its own echoes.

Everything else (other people, groups, what the user sends others, receipts, typing,
calls, stories, edits, messages with neither text nor a picture) is dropped on the
Signal thread, unread and unlogged.

A note's pictures (image attachments within 20 MB, at most 10) are downloaded and
decrypted there too (`classify::pictures`, presage's `get_attachment`), only once it's
known to be a note; other files never are. Messages go to a "Signal" conversation through
`channels::photos::deliver` (see [Photos](photos.md)); `/new` starts another.

## Mimi's messages

Mimi's messages are sent to the account itself, which libsignal-service turns into a
"sent" transcript for the user's other devices: they show in Note to Self as the user's
own. So:

- Each starts with the assistant's name in bold on its own line.
- Markdown becomes plain text with Signal's style ranges (bold, italic, monospace,
  strikethrough, counted in UTF-16; `format.rs`), links as "label (address)".
- Long messages are split at 1,500 characters at line breaks, each piece headed again.
- Signal doesn't notify anyone about their own Note to Self, so these messages are
  silent: the dialog, the connection's line and the welcome say so.
- No typing indicator: in Note to Self it would show nowhere useful.

## Approvals and reminders

Approvals and reminders are text with `APPROVAL_HINT` / `REMINDER_HINT`, remembered by
the timestamps they were sent with. A reply quotes that timestamp (`quote.id`); a
reaction targets it (`target_sent_timestamp`). See [Messaging apps](messaging.md) for
the bare "yes"/"no" and "done"/"snooze" rules.

Reminders arrive in Note to Self (silently) and are answered by a reply ("done",
"snooze 1h") or a reaction (✅, 💤). A routine's answer is sent with Signal text styles,
split to fit, and its pending approvals arrive as a prompt to answer yes or no. See
[Reminders and routines](reminders-and-routines.md).

## Staying linked

Signal forgets linked devices that stay away about 30 days, so the receive loop always
reconnects: after a dropped connection in a second, after a failed one with backoff
(2 s → 5 min), still sending meanwhile.

Three refusals of the device's credentials in a row (HTTP 401/403, also inside a
websocket handshake error) mean the user removed it on their phone: the connection shows
"Unlinked from your phone. Link again to keep using Signal." and stops.

A linked device can't remove itself from the account, so disconnecting deletes
everything Mimi kept and tells the user to remove it under Linked devices too.

## Storage

`store.rs`, migration 0024. presage's `Store` traits are implemented on Mimi's SQLCipher
database (not presage-store-sqlite, whose forked libsqlite3-sys clashes with
rusqlite's), adapting presage-store-sqlite's protocol stores:

- `signal_kv`: registration data with the device's password, identity key pairs, sender
  certificate, master key and account entropy pool, the Note to Self timer;
- `signal_sessions`, `signal_identities`, `signal_pre_keys`, `signal_signed_pre_keys`,
  `signal_kyber_pre_keys` (+ `signal_base_keys_seen`), `signal_sender_keys`, each per
  ACI/PNI identity.

Every row has the connection id with `ON DELETE CASCADE`: removing the connection
removes them all, and a Signal thread still running can't write any more.

Nothing about messages or other people is kept: `save_message` stores nothing; contacts
other than the account's own, and all profiles, avatars, groups and sticker packs, are
dropped. presage would otherwise fetch every new sender's profile and every group from
Signal (its profile code even panics on a profile it can't decrypt, and release builds
abort on panics), so the store answers with stand-ins:

- every contact exists (nameless, a timer version that never updates);
- every group is at the latest revision;
- a sender's profile key is known from their message (in memory only);
- an unknown one is an error rather than "unknown", which stops the fetch.

## Logs

presage and libsignal log linking codes, and who wrote or deleted what in other chats, at
info and warning level, so `mimid`'s default filter (`main.rs`) keeps only their errors.

## Tests

There is no fake Signal server.

- `classify`, `format` and the store are unit-tested: every protocol store round-trips;
  removing the connection leaves no row; a message's text, a contact's name and a
  group's title are nowhere in the database.
- `api/tests.rs › signal_flow` drives the channel with a stand-in worker and a scripted
  model: a note becomes a chat turn, 👍 on the prompt approves, the reply comes back
  styled and headed, `/new`, reminders.
- Linking itself needs a phone: see below.

### Trying Signal for real

This needs a phone, and a scratch daemon, never the user's own (see
[Development](development.md) for scratch daemons):

1. In one shell:

   ```sh
   export MIMI_HOME=$(mktemp -d) MIMI_KEY_STORE=file MIMI_PORT=7493 MIMI_NO_NOTIFICATIONS=1
   target/debug/mimid
   ```

2. Then `pnpm web` in the same shell (it opens the web UI of the daemon in
   `$MIMI_HOME`) › Settings › Connections › Signal.
3. Scan the code (Signal › Settings › Linked devices › +), and write in Note to Self.
4. Then disconnect, remove the device on the phone, and delete `$MIMI_HOME`.

Without the UI: `POST /v1/connections {"integration":"signal"}` with the token in
`$MIMI_HOME/daemon.json`, and read the code from `GET /v1/connections`.
