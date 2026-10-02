# People the user trusts

The owner can let someone in People ask their assistant things from that person's own
messaging app (Matrix today): "the assistant for Maya" on their card. Everything about
it is off for everyone until the owner turns it on, and only the owner's own API calls
change it. The code is `crates/core/src/access/`; the card is
`apps/desktop/src/features/people/assistant-access.tsx`, and approvals waiting for the
owner are `features/people/guest-approvals.tsx`.

It is meant to be frictionless and private: the owner sets it up once on the person's
card (the switch and the shared calendars), and from then on the trusted person uses the
assistant without the owner being asked about it ("X wrote to me, allow?") and without
their messages being forwarded to the owner. Their conversations are theirs, hidden from
the owner's app. Don't add owner notifications or visibility into a trusted person's
chats unless the user asks for it.

## Rules

- **Off for everyone by default.** Only the owner's own API calls change access
  (`GET|PUT /v1/people/{id}/access`): never a tool, never a guest.
- **Recognised only by an app address that's theirs in People** from an address book or
  added by hand, with access on, and exactly one such person. Never a card made from
  mail, never a name.
- **A guest's conversation belongs to the guest.** The owner's clients never get it:
  not listed, 404 on every `/conversations/{id}` route, and `Access::hides` drops its
  events from `/v1/events` (the ids are loaded before anything is served, and marked
  before a row is written).
- **Never forward a trusted person's messages to the owner**, not even replies in a
  chat the assistant opened.
- **The boundary is code, not prompt.** `access::principal_for` decides from the
  conversation, never from what's in it. A guest gets:
  - a guest prompt: who's talking, their shared calendars, the personality via
    `Persona::guest_block`, never the owner's custom instructions, profile or recalled
    memory;
  - no mentions and no learning;
  - only the explicit allow-list `access::tools::ALLOWED` (`registry`), checked again in
    `Turn::act`. An allow-list, not the owner's registry minus some.
- **`ToolContext.principal` travels into every tool.** No mail, memory, people,
  `matrix_send`, `message_me`.
- **Cards never offer "Don't ask again"** to or for a guest: that changes the owner's
  settings.
- **Turning access off or deleting the person** deletes their conversations, their
  reminders and routines, and leaves the chats they opened (`access::revoke`; `tidy`
  after syncs).
- `api/tests/access_flow.rs` proves the boundary with a scripted model that tries
  everything; keep it passing.

## Choices

`person_access` (migration 0026); `GET|PUT /v1/people/{id}/access`;
`PersonAccessChanged` events.

- Whether they may ask.
- The calendars shared with them (ids from `GET /v1/calendars`; unknown ones are
  refused).
- Who approves what they ask for: them, in their own chat (the default), or the owner.

The view adds where they write from (their usable Matrix addresses) and to (the
assistant's accounts), so the card can say what's missing.

## Recognising them

`access::find`: an app address that's theirs in People from an address book or added by
hand (a card Mimi made from mail doesn't count, a name never does), with access on. If
two people with access share it, nobody is recognised.

### Matrix

`matrix/guests.rs`. Their messages count in a direct chat the assistant keeps with them:
one it opened to message them for the owner, or one they open (their invitation, marked
direct or with just the two of them, is joined (`Invitation::KeepDirect`) and kept as `Why::Direct`;
it then tries to turn encryption on, as for the owner). The owner's encryption rules
apply: once a chat is encrypted, only encrypted messages count. Their groups are
declined; in the owner's groups they're like anyone else there: a mention gets an
answer from the group's messages alone, never as their guest turn (see
[Matrix](matrix.md#answering-in-groups)). Reactions answer only their
own prompts. An encrypted message from them it can't read is said to them in their own
chat. Everything there goes through `Messenger`, so `api/tests/access_flow.rs` drives
it with the fake. See [Matrix](matrix.md).

### Other apps

Telegram can plug in the same way later: recognise with `find`, keep a `Line`, give a
`Channel` back from `access::lines` (see [Messaging](messaging.md)). Signal is a linked
device on the owner's own account, so other people's Signal chats never can (see
[Signal](signal.md)).

## Their conversation is theirs

- Each line (app, assistant account, their address, the chat) has one conversation
  (`guest_conversations`, `access::line_conversation`), titled with their name; `/new`
  replaces it. Each routine of theirs has one too.
- They're stored because the chat engine needs the history, but the owner's clients
  never get them: `list_conversations` leaves them out, every `/conversations/{id}` route
  answers 404, and `/v1/events` drops every event about them (`Access::hides`, from a
  set of ids loaded at startup and marked before a conversation is saved).
- A trusted person's messages are never passed on to the owner, not even replies in a
  chat the assistant opened.
- Turning access off, or deleting the person, deletes their conversations and their
  reminders and routines, and the assistant leaves the chats they opened
  (`access::revoke`). After a sync that drops someone, `access::tidy` does the same.

## The guest boundary

Who a turn is for (`Principal`) comes from the conversation, never from what's in it
(`access::principal_for`). A conversation whose person is gone or turned off refuses new
turns.

For a guest, `chat::send`:

- builds a different prompt: who's talking, that the owner isn't there, the shared
  calendars by name or plainly that none is shared yet, the personality without the
  owner's instructions (`Persona::guest_block`; the instructions are the owner's wishes
  about acting for them and may name private things, see [Personality](personality.md)),
  and none of the owner's memory: no profile, no recall;
- drops mentions;
- never schedules learning (`learn_from` skips their conversations; see
  [Memory](memory.md));
- offers only `access::tools::registry`: an allow-list (`access::tools::ALLOWED`), not
  the owner's registry minus some.

`Turn::act` refuses anything outside the list again, and `ToolContext.principal` reaches
every tool:

- **Calendars:** the tools work on the accounts cut down to the shared calendars
  (`calendar::restrict`), so the others are never read. Every event and target is
  checked against the list again; an id from anywhere else is "not found", as if it
  didn't exist. Writes go only to calendars saved into directly (not Google by private
  address: its page would open in the guest's browser). Guests of an event only as email
  addresses (nothing looked up in the owner's contacts). Read problems say only that a
  shared calendar couldn't be read. Invitations are never offered for emailing (the owner
  can send them from the Calendar panel). On signed-in Google, their writes let Google
  notify guests only when the owner knows every guest (`tools::google_may_tell`); see
  [Calendar](calendar.md).
- **Reminders and routines:** created with `for_person` (`schedule::create_for`), listed,
  changed and cancelled only among their own (`schedule/tools.rs › belongs`); an event to
  time one to is looked for only in the shared calendars. They're delivered only to the
  person's chat (answered there by reply or reaction), with no notification, toast or
  history for the owner; their routines run as them, in a conversation of theirs. The
  owner's lists, history and calendar leave them out, and the API answers 404 for them.
  See [Reminders and routines](reminders-and-routines.md).
- **Nothing else:** no mail, memory, people, `matrix_send`, `message_me`, or anything
  else that reads or sends the owner's data.
- **Photos:** when the model can't see, they get `GUEST_UNSEEN_NOTE` (type it out
  instead): they can't change the owner's models. See [Photos](photos.md).

## Approvals

- They follow Settings › Permissions as for the owner, known-recipients net included
  (see [Tools and approvals](tools-and-approvals.md)).
- Theirs are asked in their own chat. Prompts are remembered per chat (`replies::line`),
  so their "yes" answers only their own and the owner's only the owner's.
- When their card says the owner approves, only the card reaches the owner
  (`access::ask_owner`: the owner's messaging apps, headed "For Maya:", a desktop
  notification, and a `GuestApproval` event the app shows as a card of its own, with
  `GET /v1/access/approvals` at launch), and they're told it waits for the owner. Their
  messages themselves are never sent along.
- Approval cards never offer "Don't ask again": that changes the owner's settings.

## People changes

- **Merging** combines everyone's access the strict way, as for permission exceptions:
  on only if all who had a choice had it on, the calendars they all had in common, the
  owner approving if anyone's card said so. Undoing the merge puts it back unless it was
  changed since. Their conversations and reminders keep the old ids, which resolve
  through `people_merged`. See [People](people.md#merging).
- **Deleting** someone deletes their access (and what `revoke` removes); bringing them
  back doesn't restore it.

## Tests

- `api/tests/access_flow.rs`: the owner turning it on, the prompt and tools a guest gets,
  shared calendars read and written, invented ids refused, a scripted model trying mail,
  memory, people and messaging, reminders and routines reaching them, approvals in their
  chat or the owner's, strangers unchanged, unreadable messages, turning it off, merges,
  undo and deletion.
- The live Matrix test's trusted friend (`live_matrix`; see [Matrix](matrix.md)): their
  requests, their approvals, the owner approving for them, their own chat, with a shared
  calendar.
