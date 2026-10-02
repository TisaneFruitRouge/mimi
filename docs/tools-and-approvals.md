# Tools and approvals

What the assistant can do beyond writing text: read the calendar, draft an email, send a
Matrix message, remember a fact. Tools live in `crates/core/src/tools/` (the `Tool`
trait is in `mod.rs`, permissions in `tools/permissions.rs`); integrations add their own
through tool sources. The chat engine runs them in `chat::Turn` and decides on approvals
in `chat::act`. This is how Mimi keeps the principle that outbound actions go through the
capability and approval system, never straight from model output: everything the model
reads (incoming messages, web pages, emails, calendars, tool output) may contain prompt
injection.

## Rules

- **The approval rule.** Anything that sends, changes or deletes something on the user's
  behalf needs approval (`Tool::needs_approval` returns true); reads don't. Never weaken
  this for convenience: it is the main defence against prompt injection.
- **`message_me` is the one sender without a card** (`channels/tools.rs`). It writes only
  to the user themselves, on a paired app's private line, like a reminder, and shows in
  the chat. Keep its destination fixed to `channels::owners`; never let it take an
  address.
- **Local writes** (memory, the user's own reminders and routines) need no approval
  because they stay on the machine, but every write must show in the chat as a quiet
  line with Undo (see [Memory](memory.md) and
  [Reminders and routines](reminders-and-routines.md)).
- **Permissions change only through the user's own calls** (`PUT
  /v1/permissions/{kind}`, or "Don't ask again for …" on a card), never through a tool.
  `PUT /v1/settings` keeps the stored ones.
- **Keep the known-recipients net.** Automatic sending (email, invitations, events with
  guests, Matrix messages) still asks unless everyone it reaches is known, even for a
  person with an exception, and every blind copy counts. Don't add new permission kinds
  without the same care.
- **Files leaving the computer always ask.** An email the assistant attaches files to
  waits for approval whatever Permissions say (`Tool::always_asks`), and the assistant
  attaches only by reference (an email's attachment, a photo from the same chat), never
  a file from the computer (see [Email](email.md#the-assistants-bcc-and-files)).
- **The card shows what runs.** A tool whose arguments point at something looks it up in
  `Tool::resolve` and writes the real thing over whatever the model put there; the card,
  the permission decision and the run all see the same arguments.
- **Personality and custom instructions never reach approval or permission decisions**
  (see [Personality and instructions](personality.md)).
- **Tool output** is given back to the model verbatim (cut at 16k characters) and stored
  with the message, so keep it compact and free of secrets.
- **Someone the user trusts** gets an explicit allow-list of tools, checked again in
  `Turn::act`, and their cards never offer "Don't ask again" (see
  [People the user trusts](trusted-people.md)).
- Keep `api/tests.rs › mail_flow` passing: it proves a hostile email can't send mail
  without approval.

## How a reply uses tools

A reply is a loop of model rounds (`chat::Turn`). Each round streams from the model with
the available tools offered (`tools::ToolSources` → a per-reply `ToolRegistry`). When the
model asks for tools, each call becomes an `Action` on the assistant message:

- **Reads** run immediately.
- **Anything that sends, changes or deletes** waits as `pending_approval`, unless
  Settings › Permissions lets it run on its own (see [Permissions](#permissions)). The
  reply pauses until `POST /v1/actions/{id}/approve` (optionally with edited arguments)
  or `/reject`. A declined action doesn't run, and the model is told so.
- Results go back to the model as `role: "tool"` messages, and the next round starts,
  up to 8 rounds. After that the model is asked once more without tools, so it must
  answer in words.

Details:

- **State updates:** action changes reach clients as `message_updated` events.
- **Position in the text:** each action records `content_offset`, how far into the
  reply's text it happened, so clients show text and cards in order.
- **History:** history replays past tool calls and results, so follow-ups work.
- **Stopping:** cancelling a reply while it waits for approval fails the action without
  running it.
- **Restarts:** actions that were pending or running are marked failed. They never run
  later.
- **Models without tool support:** some models reject the `tools` parameter. The reply
  is retried once as plain chat.
- **Who is asked:** in the app, the card is in the chat. Messaging apps get the same
  approval on their own line: Telegram as inline Approve / Don't buttons, Signal and
  Matrix as text answered by a quoted reply or a reaction (`channels::replies`); see
  [Messaging apps](messaging.md). A routine's approvals are relayed to every paired app
  and announced on the desktop (see [Reminders and routines](reminders-and-routines.md)).
  A trusted person's approvals go to their own chat, or to the owner when their card
  says so (see [People the user trusts](trusted-people.md)).

## Approval cards

- **Looking things up first:** before anything is decided, `Tool::resolve` may look up
  what the arguments point at (the calendar tools read the event a change is about) and
  write it into the arguments, replacing whatever the model put under those keys. The
  card, the permission decision and the run all see the real thing.
- **Conforming the arguments:** then, before an approval card is shown, the call's
  arguments go through `Tool::prepare`. The default, `tools::conform`, applies the
  schema's types: plain mistakes like a string for a list are converted, anything else is
  refused. The stored action holds the result. Override `prepare` when the tool reads
  arguments in a further shape (e.g. `mail_send` splits recipients into one address
  each).
- **How a card reads:** optionally add a formatter for the tool's arguments in
  `apps/desktop/src/features/chat/action-formatters.tsx`; otherwise arguments show as a
  tidy key/value list. Any argument a formatter doesn't list in its `keys` is still
  shown after its rows. A row can show Markdown as it will look (`ArgRow.markdown`, e.g.
  a Matrix message). The `mail_send` card shows To, Cc, Bcc ("A hidden copy: the others
  won't see this"), Subject, the whole message, and each file with its name, size and
  where it comes from, looked up by `resolve` (see [Email](email.md)); a calendar write that Google will email guests about carries
  an `email_note` saying so beforehand (see [Calendar](calendar.md)).
- **Automatic actions** (allowed by Permissions) still show in the chat as a card with
  their details, and, for events with guests, the "Send invitations to …" button.

## Permissions

Settings › Permissions (`#/settings/permissions`) lets the user skip the card for some
kinds of action. The kinds are descriptors in one place, `tools::permissions::KINDS`: a
stable id (the settings key), title, the two plain-language details, an optional
safety-net note, an icon and a colour, a default `Autonomy` (`Ask | Automatic`), what an
exception can be about (people, calendars, people and Matrix groups, or nothing), and
optional switches (`Kind::switches`). `GET /v1/permissions` serves them with the user's
choices (`PermissionKind`), and the Permissions page renders only that, so a new kind is
a `Governs` variant, a descriptor, and `Tool::governed_by` on its tools.

| Kind | Default | Exceptions | Tools |
| --- | --- | --- | --- |
| `send_mail` | ask | people | `mail_send`, `calendar_send_invitations` |
| `send_messages` | ask | people and Matrix groups | `matrix_send` |
| `add_events` | ask | calendars | `calendar_add_event` |
| `change_events` | ask | calendars | `calendar_change_event`, `calendar_delete_event` |
| `schedule` | automatic | none | reminders and routines |

- **Choices** are `Settings.permissions`: per kind id, `{ autonomy, rules, switches }`
  (`KindPermission`), where a rule is `{ target: person(id) | calendar(id) |
  matrix_room(id), autonomy }` and `switches` lists the kind's switches turned on (only
  `send_messages` has one, `everyone_on_server`). Settings saved by the first version
  (`"send_mail": "automatic"`) still load, meaning the same. `PUT
  /v1/permissions/{kind}` is the only way to change them (it refuses unknown kinds,
  targets the kind can't have, and new exceptions about people or calendars that don't
  exist); `PUT /v1/settings` keeps what's stored, so a stale client can't undo them.
  The page offers people with a Matrix address and the groups from `GET
  /v1/matrix/groups` as exceptions for `send_messages`, and the switch under the kind.
- **Always asking:** `Tool::always_asks` marks a call no choice lets through (an email
  with files attached); its card offers no "Don't ask again", as an exception wouldn't
  skip it.
- **Deciding** (`requires_approval`, called from `chat::act`): a tool opts in with
  `Tool::governed_by` and says what a call is about with `Tool::call_targets` (email
  recipients, a calendar id, Matrix people and groups). Each target takes its
  exception's choice, else the kind's default; the most specific wins. A person
  exception covers every address of that person in People, and when two exceptions
  cover one address, asking wins. A call about several (an email to three people) runs
  on its own only if every target does.
- **The safety net** comes after, for the kinds marked `recipients_must_be_known`: every
  target must also be known, whatever the rules say.
  - `send_mail`: every recipient, Cc and Bcc included (and an email must have one), is
    known by `mail::known`: the user's own addresses, address-book or hand-added
    handles, or someone in the Sent folder; never Mimi's correspondent cards or mail
    that merely claims to be from the user. `calendar_send_invitations` is sending mail.
  - `add_events` and `change_events`: every guest (`CallTarget::Email` from
    `call_targets`) when a call adds guests, since the event then reaches them (Google
    shows it in their calendar without any email); nothing more for an event with nobody
    on it.
  - `send_messages`: every Matrix person and group (`CallTarget::MatrixUser` /
    `MatrixRoom`, checked by `matrix::rooms::all_known`), and a message must reach
    someone. Known means: the owner; a Matrix address in People from an address book or
    added by hand; groups the owner invited it into; people and groups it has messaged
    for the user before; and, only with the kind's switch "Everyone on <server> counts as
    someone you know" (off by default: anyone can sign up on public servers), anyone on
    its server and groups whose members all are. Details in
    [Matrix › Messages to other people](matrix.md#messages-to-other-people).
- **"Don't ask again for Sam"**: when a card could have been skipped by an exception
  (every target is one person, group or calendar with no exception of its own, and for
  email and messages everyone reached is known), the daemon offers it:
  `Action.always_allow` holds the text, `Approvals` keeps the exact exception, and `POST
  /actions/{id}/approve` with `always: true` approves and adds it. Never with edited
  arguments. Telegram cards don't offer it, and neither do a trusted person's cards
  (that would change the owner's settings).

## Adding a tool

- Implement `tools::Tool` (`crates/core/src/tools/mod.rs`): a stable `snake_case` name,
  a description written for the model, a JSON Schema for the arguments, `summary()` as
  one plain-language line for the approval card, and `result_label()` as a short
  past-tense line ("read calendar"). Return true from `needs_approval` for anything that
  sends, changes or deletes.
- If it should fall under Settings › Permissions, implement `governed_by` and
  `call_targets` (and `always_asks` for calls that must never run on their own); if its arguments point at something, `resolve`; if it reads arguments
  in a further shape, `prepare`.
- Integrations expose their tools through a `ToolSource` added to `state.tool_sources`
  at startup; the source returns no tools while the integration is disconnected. Each
  reply builds a fresh registry, so nothing else needs wiring.
- Optionally add a formatter in `action-formatters.tsx` (see
  [Approval cards](#approval-cards)).
- A tool a trusted person may use must also be in `access::tools::ALLOWED`; tools see who
  the turn is for in `ToolContext.principal` (see
  [People the user trusts](trusted-people.md)).

## Development tools

Debug builds have two fake tools, `dev_lookup` and `dev_send_note`, enabled with
`MIMI_DEV_TOOLS=1`, for exercising approval cards against a scripted model.

## Tests

- `api/tests.rs › mail_flow`: a hostile email and a model that obeys it; the send waits
  on the approval card and nothing is sent when it's declined. `mail_flow ›
  people_exceptions_and_dont_ask_again` covers exceptions and "Don't ask again".
- `api/tests/mail_files.rs`: a hostile email's Bcc and files wait for approval even when
  sending is automatic; only files the rules allow can be attached.
- `api/tests.rs › automatic_sending_only_writes_to_people_the_user_knows`,
  `permission_api`, `calendar_guests`, `matrix_flow`, and `matrix/send_tests.rs` cover
  permissions and the known-recipients net.
- `api/tests.rs › instructions_reach_the_model_but_cannot_skip_approval`: custom
  instructions can't lift approvals.
- `api/tests/access_flow.rs`: a scripted model tries everything as a trusted person.

## Later

Integrations through MCP: each tool declares its capabilities (hosts, paths, outbound
messaging) and the user approves them. Untrusted plugins run sandboxed.
