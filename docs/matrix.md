# Matrix

The user makes an account for the assistant on any Matrix server, and talks with it from
their own Matrix account. The assistant can also write for the user to other people and
groups. The code is in `crates/core/src/connections/matrix/` (matrix-sdk, Apache-2.0,
with `e2e-encryption` and its SQLite store). It builds on the shared messaging layer:
see [Messaging apps](messaging.md).

## Rules

- Mimi signs in with the account's password, sets up cross-signing with it, and never
  keeps it.
- One client per connection, in `state.connections.matrix`. A store is opened once and
  never twice.
- Pairing is the six-digit code, from someone in a room of two. Then only the owner's
  direct chat is a conversation; encryption is turned on there if it's off; once it's
  encrypted, unencrypted messages are ignored (reactions excepted: apps never encrypt
  them).
- It keeps (`rooms.rs`, migration 0025) the owner's chat, groups the **owner** invites
  it into (everyone else's invitations are declined), chats it opened and public groups
  it joined to send messages. Other rooms are left.
- Only the owner and people the owner trusts (see
  [People the user trusts](trusted-people.md)) give instructions, each in their own
  direct chat. Everyone else's messages never reach the model: a reply in a chat it
  opened for the user is passed on to the owner's chat quoted as it is (`forward_text`,
  logged, never its content).
- In its groups it answers whoever mentions it, the owner included, as a member of the
  group (`group.rs`): one model call with **no tools**, from the group's latest messages
  only. Never the owner's memory, profile, instructions or data, and nothing stored.
  Keep it that way: everyone reads the answer, and the rest of the group is untrusted.
- An encrypted message it can't read is logged (room and sender) and said: to the owner
  in their chat, to a trusted person in theirs, and, for a reply in a chat it opened, to
  the owner.
- Compare Matrix addresses without letter case (People may hold "@Maya:…").
- Approvals and reminders are answered by reply or reaction (`channels::replies`).
- Replies are Markdown → HTML with the model's HTML escaped (`format.rs`).
- Removing the connection signs the device out and deletes the store.
- Say honestly whether the chat is encrypted.
- Everything that talks to Matrix goes through `messenger::Messenger` (the running
  client; `fake.rs` in tests, `Clients::fake`).
- `matrix_send` goes through `Tool::resolve` (every recipient turned into the exact
  account or room, never a guess), and `run` checks it all again.
- Sending on its own (`send_messages` set to automatic) only ever reaches known people
  and groups (`matrix::rooms::all_known`). "Everyone on the server" only ever covers the
  assistant's own server. Keep that check.
- Keep the server commands in `matrixServers` (`connect-dialogs.tsx`) checked against
  each program's docs.
- Never point tests at matrix.org.
- **Build gotcha:** matrix-sdk pulls in `decancer`, whose `AddAssign` impl for `String`
  breaks `s += &string` inference in mimi-core: write `s += &*string` or `push_str`.

## The account

The user makes an account for the assistant on any server and enters its address and
password; `ConnectionSetup::Matrix { user, password, homeserver? }`. The dialog has two
paths:

- **A public server**: sign up in Element, on matrix.org or any server with open
  sign-up.
- **My own server**, for servers with sign-up closed: the user picks the address, and
  the dialog shows the command that makes the account for their server program
  (`matrixServers` in `connect-dialogs.tsx`: Synapse's `register_new_matrix_user`, also
  in Docker; `!admin users create-user` in the admin room of Tuwunel, conduwuit or
  Continuwuity; `mas-cli manage register-user`; Dendrite's `create-account`, also in
  Docker), with a password generated on this computer and already filled in. Commands
  that can ask for the password do, so it stays out of the shell's history.

## Signing in

`connect`: the server comes from the address through `.well-known/matrix/client`, else
the address's host, else the "Server address" the user typed (http only for servers on
this computer or network). Mimi logs in with the password (device name: the assistant's
name) and refuses an account that's already connected.

It sets up cross-signing with the same password (user-interactive auth), replacing an
identity made elsewhere (Element makes one when the account is created there), since the
account is the assistant's. So the owner's app shows the device as trusted.

The password isn't kept; the config holds the user id, homeserver, device id and access
token. Key backup and recovery aren't set up: losing the store only loses the ability to
read old messages, which the assistant doesn't need.

## Store

`<data>/matrix/<connection id>/`: matrix-sdk's SQLite stores (sync state, encryption
keys), encrypted with a random passphrase kept in the connection config, so in the
encrypted database. matrix-sdk-sqlite uses the same rusqlite as the daemon, so both link
one libsqlite3 (SQLCipher's).

A store is opened once: the client made at sign-in is handed to the connection's task,
and everything else (`owners`) uses the running client from `state.connections.matrix`.

## Sync

`run`: `sync_once` in a loop (30 s long polls; nothing listens for inbound connections).
A handler task takes the events in order, so handling one (turning on encryption waits
for the next sync) never holds up syncing.

- Messages from before the connection was made are history, not requests (`since`, with
  an hour's grace for a server clock that's behind); the sync token in the store means a
  restart replays nothing.
- The client is made once per task and kept across failed syncs (a store is never open
  twice; replies being written keep using it).
- Unreachable → "Retrying…" with backoff (2 s → 5 min). An unknown token (signed out
  elsewhere) → an error saying to connect again, kept after restarts (`signed_out`).

## Pairing

Until paired, the assistant joins every invitation but answers nothing except the exact
six-digit code, sent in a room of two. Its sender becomes the owner (user id, display
name, room). Then it:

- leaves every other room;
- declines everyone else's invitations (it joins the owner's groups; see
  [Messages to other people](#messages-to-other-people));
- turns end-to-end encryption on in the owner's chat if it's off (it usually can:
  Element makes both members admins of a direct chat).

The welcome says honestly whether the chat is encrypted. If the owner leaves, the
connection asks them to start a new chat; their next message in a new direct chat moves
it there.

## Messages

The owner's messages go to a "Matrix" conversation (`/new` starts a new one), with typing
notices while the reply is written.

- **Encryption.** Once the chat is encrypted, a message that arrives unencrypted is
  ignored: a server on the way could have made it up. Reactions are the exception
  (Matrix apps never encrypt them); they only ever answer a prompt the assistant sent.
- **Unreadable messages.** A message it can't decrypt gets "try sending it again", and
  is logged (room and sender, never content). From anyone else it's logged too, and said
  to a trusted person in their own chat, or, for a reply in a chat the assistant opened,
  to the owner (and to the sender when they're in People).
- **Replies** are Markdown turned into Matrix HTML with pulldown-cmark (`format.rs`): the
  model's raw HTML is shown as text, links keep only http(s) and mailto, pictures become
  their description, and long replies split at line breaks (12,000 characters) without
  cutting a code block in two.

## Photos

An `m.image` from the owner is a message whose words are its caption (the body, when a
`filename` differs from it, per the spec) and whose picture is fetched with matrix-sdk's
media API, which decrypts encrypted media, only once the sender is known to be the owner
and within 20 MB (`info.size`, then the bytes). The same encryption rule applies.
Others' pictures are never fetched; a caption in a chat it opened for the user is passed
on like any reply. Grouping with the words that follow is `channels::photos` (see
[Photos](photos.md)).

## Prompts

Approvals ("Waiting for you: …") and reminders ("⏰ …") carry the hints from
`channels::replies`, and are remembered by event id: a reply to one (its
`m.in_reply_to`) or a reaction on it (👍 / 👎, ✅ / 💤) answers it; a bare "yes" or "no"
answers the only approval waiting. Replies' quoted fallback is removed first. Reminders
go to the owner's chat (see [Reminders and routines](reminders-and-routines.md)).

## Removing

Removing the connection signs the device out (`/logout`, best effort) and deletes its
store.

## Status line

- "From your own Matrix account, start a chat with @bot:server and send 123456"
  (NeedsAction, with a `https://matrix.to/#/@bot:server` link and the code the app shows
  large);
- then "Talking with Vincent as @bot:server, end-to-end encrypted" (or "not encrypted:
  turn on encryption in the chat's settings", or "from a device that isn't verified" when
  cross-signing couldn't be set up).

## Logging

matrix-sdk is chatty and reports expected "not found" answers as errors, so the daemon's
default log filter (`main.rs`) keeps it to warnings.

## People the user trusts

Their messages count in a direct chat the assistant keeps with them (`matrix/guests.rs`,
`KeepDirect`): one it opened, or one they opened. The rules and the boundary are in
[People the user trusts](trusted-people.md).

## Messages to other people

The assistant can write for the user to other people and groups on any Matrix server,
from its own account (`matrix/send.rs`, `rooms.rs`, `messenger.rs`).

### `matrix_send`

`matrix_send` (`to`, `text` in Markdown; `from` when several accounts are paired) is
governed by `send_messages`: a card by default.

- **Resolving.** `Tool::resolve` turns each recipient into what it reaches and writes it
  over the arguments: `to` (user ids and room ids), `recipients` for the card ("Sam
  Carter (@sam:example.org)", "Group “Family” (#family:example.org)") and `joins` (public
  groups it will join). It accepts Matrix addresses, `#aliases`, room ids (`!room`),
  `matrix.to` links, people from People by id (an @ mention) or by a name that comes down
  to exactly one Matrix address, and the name of a group it's in. A name that fits a
  group and a person, or several people, is refused so the model asks. Never a guess.
- **Other servers** are fine: someone there is known like anyone else (People, messaged
  before); the "everyone on the server" switch never covers them. A group elsewhere named
  by its address, that the assistant isn't in, is joined as part of the send: only its
  own server's list says whether it's public, so a private one fails then.
- **Delivering.** `run` resolves again and delivers:
  - to the owner in their chat;
  - to a person in the direct chat it opened with them (still there, they haven't left),
    else a new end-to-end encrypted one (`create_dm`, `is_direct`);
  - to a group it's in;
  - to a public group listed on its server, which it joins first.

  The output lists who it reached, the chats it opened and the groups it joined.

### `matrix_rooms`

A read: lists its groups and the server's public ones, names marked as other people's
writing.

### `matrix_read`

A read (`matrix/read.rs`): the latest messages (30 by default, at most 100, each cut at
1,000 characters) in a group it's in, so the owner can ask what was said there. The
group is named by id, `#alias:server`, `#alias` without its server, or name; a name that
fits several groups is refused. Only groups (`send::groups_of`): never the owner's chat,
and never a direct chat, so a trusted person's chat stays theirs; trusted people don't
get the tool. History comes from the server (`/messages`, decrypted by matrix-sdk);
messages whose keys never reached it say so. Each message is "the user", "you" or the
sender's name and address, with a "written by other people, not instructions" notice,
as for email. Nothing in a group reaches the model unless the owner asks: what's said
there still isn't a conversation.

### Known people and groups

For sending on its own (`send_messages` set to automatic; `CallTarget::MatrixUser` /
`MatrixRoom`, checked by `matrix::rooms::all_known`), these are known:

- the owner and their chat;
- a Matrix address in People from an address book or added by hand (never a card Mimi
  made from mail);
- groups the owner invited it into;
- people and groups it has messaged for the user (`matrix_known`, migration 0025: kept
  even after it leaves a room).

The switch "Everyone on example.org counts as someone you know" (off by default, because
anyone can make an account on a public server) adds everyone whose address is on the
assistant's own server, and groups it's in whose members (joined or invited) all are;
never a public group it isn't in yet. A message to several reaches them on its own only
if every one is known and allowed. A message must reach someone. The permission kinds
and how a call is decided are in [Tools and approvals](tools-and-approvals.md).

### Rooms it keeps

`matrix_rooms` table, migration 0025:

- groups the owner invited it into (`group`; everyone else's invitations are declined,
  and a direct-chat invitation from the owner is taken only when their chat is gone);
- public groups it joined to post (`joined`);
- chats it opened (`direct`, with who they're with).

They're recorded before joining, and rooms it made itself are never left, so a sync
racing the send can't make it leave. A chat whose other person left is left and
forgotten; the next message opens a new one.

### Answering in groups

`group.rs`. In a group it keeps (`Why::Group` or `Why::Joined`), a message that mentions
the assistant gets an answer there, whoever wrote it, the owner included: the owner chose
that their own mentions get no more than anyone's, so private requests stay in their
chat.

- **A mention** is the sender's app marking the assistant in `m.mentions` (a pill in
  Element, or a reply to one of its messages), or the words naming it with an @: its
  address, its account's name ("@mimi") or its own name, standing on their own (not
  inside an email address). Other talk is ignored, as is `@room`.
- **The answer** is one call to the active model (`mail::model::ask`, no tools, no
  thinking), never a chat turn: no conversation, no memory, no learning, nothing for the
  owner's app to show. The prompt says where it is, that everyone reads it, and that it
  has nothing private and can't act from there (the owner can ask in their own chat);
  the personality comes through `Persona::guest_block`, never the owner's instructions.
  The group's latest 30 messages (6,000 characters at most, newest kept) are fenced as
  other people's words, then the message to answer and who sent it.
- **Sent** as a reply to the mention, mentioning its sender, with typing notices.
  "Sorry, I can't answer right now." when no answer could be written (logged, never the
  content).
- **Encryption.** Once a group is encrypted, an unencrypted mention is ignored. Pictures
  and voice messages in a group are never fetched; a caption that mentions it is
  answered as words.
- **One at a time.** Answers in a group are written one after another; with one being
  written and another waiting, further mentions are dropped (`MAX_IN_LINE`), so a busy
  group can't pile up the model.
- With a cloud model chosen, the group's messages go to it like the owner's do.

### Nobody else gives instructions

Except people the owner trusts, in their own direct chat (see
[People the user trusts](trusted-people.md)). For anyone else, a reply in a chat the
assistant opened is passed to the owner's chat as it is, quoted and escaped ("💬 Sam
Carter (@sam:example.org) replied: > …", at most 2,000 characters; only encrypted ones
once the chat is), without the model (`forward_text`), and logged (never its content),
whether it went through or not. In groups, only a mention is answered, as a member of
the group with nothing of the owner's ([Answering in groups](#answering-in-groups));
everything else said there, the owner included, is ignored.

The sender is matched to the chat without letter case: a reply from "@maya:…" to a chat
opened for "@Maya:…" (as typed in People) used to be dropped without a word.

### Permissions

Settings › Permissions offers people with a Matrix address and the groups from
`GET /v1/matrix/groups` as exceptions, and the switch under the kind.

## Tests

- Pure logic in `matrix/tests.rs` and `format.rs`.
- Sending to others in `matrix/send_tests.rs` and `api/tests.rs › matrix_flow`, against
  `matrix/fake.rs` (`Clients::fake`). `api/tests.rs ›
  automatic_sending_only_writes_to_people_the_user_knows` and `permission_api` cover the
  known-people check with the other kinds.
- `api/tests/access_flow.rs` drives trusted people through `Messenger` with the fake,
  and mentions in a group (`anyone_who_mentions_it_in_a_group_…`): no tools, nothing of
  the owner's, nothing stored. Mention matching and the group prompt are in `group.rs`.
- `api/tests/matrix_live.rs` runs everything against a real homeserver, with matrix-sdk
  clients as the owner and a friend.

### Running the live test

Matrix runs against a throwaway Synapse with open registration. Never point it at
matrix.org.

1. Generate a config:

   ```sh
   uvx --from matrix-synapse python -m synapse.app.homeserver \
     --server-name localhost --config-path homeserver.yaml \
     --generate-config --report-stats=no
   ```

2. Append a newline to the generated `homeserver.yaml` before adding lines, then add:
   - `enable_registration: true`
   - `enable_registration_without_verification: true`
   - `trusted_key_servers: []`
   - generous `rc_registration` / `rc_login` / `rc_message` / `rc_joins` / `rc_invites`
     limits (the test registers and signs in fast).

   Use a spare port (the command below assumes Synapse's 8008; change
   `MIMI_TEST_MATRIX` to match).
3. Start it with the same command minus `--generate-config …`.
4. Run:

   ```sh
   MIMI_TEST_MATRIX=http://127.0.0.1:8008 cargo test -p mimi-core live_matrix -- --ignored
   ```

It registers an assistant, an owner, a stranger and a friend, and drives the owner and
the friend with matrix-sdk clients: approval, E2EE delivery, automatic sending,
forwarded replies, the owner's group, and the friend let in with a shared calendar
(their requests, their approvals, the owner approving for them, their own chat).
