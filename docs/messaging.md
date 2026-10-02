# Messaging apps

The user can talk to their assistant from a messaging app on their phone: Telegram,
Signal or Matrix. Every app shares one layer, `crates/core/src/channels/`; each app's
own code lives under `crates/core/src/connections/` (`telegram.rs`, `signal/`,
`matrix/`). This page covers the shared layer and Telegram. The other apps have their
own pages:

- [Signal](signal.md): Mimi as a linked device on the user's own account, talking in
  Note to Self.
- [Matrix](matrix.md): an account the user makes for the assistant, including
  [messages to other people](matrix.md#messages-to-other-people).

How a connection is stored and checked before it's saved is in
[Connections](connections.md). Photos sent from an app are in [Photos](photos.md).
People the user trusts talking to the assistant from their own app are in
[People the user trusts](trusted-people.md).

## Rules

- A paired app is a private line to the user. Only the owner (and, on Matrix, people the
  owner trusts, each in their own chat) gives instructions; everything else is ignored
  or, at most, passed on as data, never to the model.
- Approvals from an app go through the same approval and permission system as the
  desktop chat (see [Tools and approvals](tools-and-approvals.md)). An app's "yes" only
  ever answers a prompt on its own line.
- Commands (`/new`) and answers to prompts ("yes", a reaction) are handled before
  anything else, so they never pick up held photos.
- Reminders and routine results go to every paired app through `channels::owners`.
  Sending runs in spawned tasks, so one slow channel never holds up the scheduler.
- `message_me` (`channels/tools.rs`) is the one sender without an approval card: it
  writes only to the user themselves, on a paired app's private line. Keep its
  destination fixed to `channels::owners`; never let it take an address. See
  [Tools and approvals](tools-and-approvals.md).
- Say honestly how private each app is: Telegram bot messages aren't end-to-end
  encrypted; Signal doesn't notify for Note to Self; a Matrix chat says whether it's
  encrypted.
- Never log tokens: a Telegram bot token is in every Bot API address, file addresses
  included, so errors are mapped without their text (see
  [Connections](connections.md)).

## The shared layer

- **`Channel`.** A paired app is a `Channel`: a private line to the user with `send`,
  `typing`, `ask_approval` and `remind`.
- **`channels::converse`.** The app's code receives the user's messages and hands them
  to `converse`, which runs the chat turn in the app's own conversation and relays the
  reply, and any approval it waits for, on the same channel.
- **`channels::owners`** lists every paired app, for what Mimi sends on its own:
  reminders, routine results and a routine's approvals (see
  [Reminders and routines](reminders-and-routines.md)), and `message_me`.
- **`channels::photos::deliver`.** Apps hand the user's messages over through it; it
  gathers photos with the words around them into one turn, per conversation, and holds
  photos until their words arrive (`state.connections.photos`). The timings and limits
  are in [Photos](photos.md). When the model couldn't see a photo, the app also gets
  `channels::UNSEEN_NOTE` (in a trusted person's chat, `GUEST_UNSEEN_NOTE`).
- **`channels::replies`.** Apps without buttons (Signal, Matrix) answer prompts in text.
  Approvals and reminders carry a hint (`replies::APPROVAL_HINT`,
  `replies::REMINDER_HINT`) and are remembered by what identifies them in the app (a
  timestamp in Signal, an event id in Matrix), in `state.connections.prompts`. Then:
  - a quoted reply or a reaction answers the prompt it points at;
  - a bare "yes"/"no" answers the only approval waiting on that channel;
  - a bare "done"/"snooze" answers the latest reminder of the hour.

  Prompts are keyed per chat (`replies::line`), so a trusted person's "yes" answers only
  their own prompts and the owner's only the owner's.
- **Personality.** Turns from an app go through `chat::send`, like the desktop chat, so
  they get the personality and custom instructions (see [Personality](personality.md)).

## Telegram

`crates/core/src/connections/telegram.rs`. The user creates a bot with @BotFather and
pastes its token; the bot becomes their private line to the assistant.

- **Checked before it's saved.** The token is checked with Telegram's `getMe`; a refused
  one says "Telegram didn't accept the bot token. Copy it again from @BotFather."
- **Long-polled.** The daemon asks Telegram for updates; nothing listens for inbound
  connections, so nothing has to reach this machine from outside. The last update
  handled is kept in the connection config (`offset`).
- **Pairing.** A one-time `/start <code>` pairs the bot with its owner (the owner's chat
  id and name are kept in the config). Every other chat is ignored.
- **Not end-to-end encrypted.** Bot messages aren't end-to-end encrypted: say so to the
  user.
- **Messages** go into a "Telegram" conversation.
- **Approvals** are sent as inline Approve / Don't buttons. Telegram cards don't offer "Don't ask again" (see
  [Tools and approvals](tools-and-approvals.md)).
- **Reminders** come with Done / Snooze 10 min / 1 hour buttons (`done:`, `snooze:`,
  `snooze60:` callbacks). Only the owner's taps count; a second tap on a settled one
  answers "Already handled".
- **Routines**: the answer is sent as Telegram HTML, split to fit; pending approvals are
  relayed as Approve / Don't buttons.
- **Photos**: the largest `photo` size, or a `document` whose type is a picture, fetched
  through `getFile` and the file endpoint of the Bot API base, only once the sender is
  known to be the owner and within 20 MB. The file address holds the token, so it's never
  logged and errors are mapped without their text. Details in [Photos](photos.md).
- **The Bot API base is configurable**: `MIMI_TELEGRAM_API` overrides it
  (`state.connections.telegram_api`), so tests and hand testing can point it at a fake.
- **People the user trusts** can't reach the assistant over Telegram yet; it can plug in
  the way Matrix does (recognise with `access::find`, keep a `Line`, give a `Channel`
  back from `access::lines`). See [People the user trusts](trusted-people.md).

## Tests

- `fake_telegram` in `api/tests.rs` is a fake Bot API server the Telegram tests drive.
- To try reminders for real, run a scratch daemon with `MIMI_NO_NOTIFICATIONS=1` and
  `MIMI_TELEGRAM_API` pointed at a fake bot server; "remind me in 2 minutes to …" fires
  on the minute. See [Reminders and routines](reminders-and-routines.md) and
  [Development](development.md) for scratch daemons.
- Signal and Matrix have their own tests: [Signal › Tests](signal.md#tests),
  [Matrix › Tests](matrix.md#tests).
