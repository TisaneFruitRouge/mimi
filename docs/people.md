# People and @ mentions

One directory of the people the user knows, unified across sources (address books, mail
correspondents, people the user adds), and the @ and # mentions that bring a person, an
event or an email into a chat. The daemon side is `crates/core/src/people/`; the
composer side is `apps/desktop/src/features/chat/mentions/`. The People panel is
described in [Frontend](frontend.md); what it reads is listed under
[API](#api) below.

## Rules

- **Person ids are stable.** Other features link to them (memory notes' `subject`,
  permission exceptions, messaging, access for [people the user trusts](trusted-people.md)).
  Merges keep the id of the person kept; old ids resolve through `people_merged`.
- **Adding a contact source** (e.g. Telegram contacts from a "send as me" account):
  implement `people::ContactSource` returning *all* current cards per connection, and
  register it with `state.people.sources.add(...)` at startup. Sync diffs by card id;
  never write to the people tables directly. Record ids must be stable per source.
- **Unification rules (don't loosen them):**
  - Cards merge only on a shared match key (`people::normalize::match_key`: email,
    phone incl. Signal/WhatsApp numbers, Telegram username).
  - Never on names: those become "possible duplicates" for the user.
  - Never guess a phone's country code.
  - Once placed, a card stays with its person, so user splits survive syncs.
  - Imported handles are read-only: "change it in the address book".
- **Deleting is from Mimi only.** Sources (address books, mail accounts) are never
  written to. A deleted card must never resurrect anyone, however it changes later;
  `people/tests.rs` covers the edge cases.
- **Merging is only ever the user's choice**, never Mimi's on its own. Removed people
  can't be merged.
- **Mentions are data, not instructions.** The `<mentioned>` block is labelled so, and a
  mentioned email is quoted with its angle brackets replaced so it can't close the block.
- **The composer's highlight layer must share the textarea's box and type**
  (`fieldText` in `composer.tsx`) or the pills drift from the text.

## Model

- A person (`people`) has contact cards from sources (`person_records`: source =
  connection id, record = the card's id there; one per card per source) and handles
  (`person_handles`: channel phone/email/telegram/signal/whatsapp/matrix, value, label,
  and the card it came from; `source IS NULL` = added by the user).
- People the user adds are `manual` and survive syncs. Imported handles are read-only
  ("change it in the address book").

## Sources

- `ContactSource` implementations return every card, per connection;
  `people::sync_all` works out what changed (diffing by card id).
- Today:
  - Address books (CardDAV) on connected CalDAV accounts (iCloud →
    contacts.icloud.com, Fastmail → carddav.fastmail.com, others on the same server).
  - Mail correspondents (`mail::contacts::Correspondents`): per account, everyone the
    user wrote to and every human sender (not automatic senders) with at least two
    messages, as cards with one email handle (record id = the address). They join
    someone only through that same address. See [Email](email.md).
- Syncs run at startup, every 30 minutes, when connections change, and on
  `POST /v1/people/sync`. A removed connection takes its cards along.

## Unification

- A new card joins the person who already has one of its match keys
  (`people::normalize::match_key`):
  - email, lowercased;
  - phone digits, with `+`/`00` as international and national numbers kept as they are
    (never a guessed country code);
  - Signal/WhatsApp numbers count as phones;
  - Telegram usernames.
- Never on name: same-name people become "possible duplicates"
  (`GET /v1/people/duplicates`), which the user merges or dismisses.
- A card, once placed, stays with its person, so a split (`POST /v1/people/{id}/split`)
  survives later syncs.

## Deleting

`store::delete` / `store::restore`, migration 0021. Anyone can be deleted, from Mimi
only; address books and mail accounts are never changed.

- What the user added (the person row, hand-added handles) goes: hand-added people and
  handles are simply deleted.
- Each imported card is remembered in `person_records_removed` (source + record, the ids
  sync knows it by), and the person's name, nickname, renamed flag and `created_at` in
  `people_removed`, under their id.
- `sync_source` skips removed cards, however they change later, so a deleted card can't
  resurrect the person or join someone else through a new match key. A *new* card
  sharing their old number is a new person.
- `POST /v1/people/removed/{id}/restore` puts the person back with the same id and marks
  their cards `restoring`: the next sync (run right away) attaches each one to that id
  rather than unifying it anew, and the person isn't dropped as empty while cards are
  still on their way.
- Marks for cards a source no longer has, and for removed connections
  (`purge_removed_sources`), are forgotten, and so is a removal with no cards left (with
  its `people_apart` pairs, which are otherwise kept for a restore).
- Memory notes are kept: the relink after `PeopleChanged` clears their `subject`, and
  links them again after a restore when the name is clear (see [Memory](memory.md)).
- Chat mentions keep their label and say the person "is no longer in their contacts".
- `DELETE /v1/people/{id}` returns a `RemovedPerson`, or `null` for someone added by
  hand (gone for good). `GET /v1/people/removed` lists the removed ("Removed contacts").
- Deleting someone also deletes their access as a trusted person (see
  [People the user trusts](trusted-people.md)).

## Merging

`people/merge.rs`, migration 0023. The user can merge any people they pick: "Merge
with…" (a person's "…" and right-click menus, a searchable picker), or several chosen in
the list (⌘/Ctrl- or Shift-click), then a sheet with the name and every way to reach
them.

- `POST /v1/people/merge/preview` (`{keep, others}`) gives the names to choose from and
  every way to reach them, the same number or address once.
- `POST /v1/people/merge` (`{keep, others, name}`) does it in one transaction.
  `/people/{id}/merge` still takes one.
- Everything pointing at a merged-away id follows the kept one in the same transaction:
  - cards, handles and restore marks move to `keep`;
  - memory notes' `subject` follows;
  - permission exceptions follow: an exception becomes automatic only if it was for
    everyone merged, else ask;
  - `people_apart` pairs are carried over;
  - access for trusted people is combined the strict way (see
    [People the user trusts](trusted-people.md#people-changes)).
- Each merged-away id is recorded in `people_merged`, so `people::get` (page addresses,
  @ mentions in old chats) and "Chats about them" still find the person.
- A chosen name is locked against address-book renames.
- Someone added by hand has no card to be told apart by, so what a merged-away person
  had added by hand (their handles, their name) becomes an own card
  (`person_own_cards`, a `PersonSource` with `source_id: null`). "Not the same person"
  (splitting it) recreates them, under their old id if it's free, so anyone can be
  separated again.
- **Undo.** What each merge changed is kept in `people_merges` for a week.
  `POST /v1/people/merges/{id}/undo` (the toast's Undo) recreates everyone under their
  own id and puts back their cards, notes, exceptions and pairs exactly, leaving what
  happened since (new cards, a rename). Only the latest merge into a person can be
  undone, and not once someone involved was separated, merged or deleted again (409).
- **Removed people and pending restores:** a removed person can't be merged (bring them
  back first); cards on their way back after a restore go to the merged person.

## Mentions

- `Mention { kind: person | event | mail_thread | mail_message, id, label }`. The text
  carries `@label` for people and events and `#label` for email (`MentionKind::sigil`,
  `mentionSigil` in `lib/draft.ts`).
- **Suggestions:** `GET /v1/mentions?q=` suggests people and events (upcoming 30 days;
  with a query, the past month to six months ahead). `GET /v1/mentions?kind=mail` gives
  the # suggestions (`mail/mentions.rs`); single-message conversations come as emails.
- **Event ids** come from `people::mentions::event_id`: calendar + uid + occurrence
  start, encoded as `ev:<ms>:<hex calendar>:<hex uid>`.
- **Resolving:** `SendMessage.mentions` keeps those whose label is still in the text.
  The engine resolves them into a `<mentioned>` block appended to the user's message
  for the model (who they are and every way to reach them; the event's time, calendar
  and place; the email), marked as data rather than instructions. The block is stored
  with the message (`messages.mention_context`) and replayed in later turns.
- **Email** is quoted with its angle brackets replaced, so it can't close the block,
  within a 3,000-character budget. A suspicious email carries its `suspicious` note
  (see [Email](email.md)).
- Recall puts memory notes about people the message @-mentions or names first (see
  [Memory](memory.md)).
- Trusted people's turns get no mentions (see [People the user trusts](trusted-people.md)).
- "Ask … about this" in every panel builds a `Draft` with
  `mentionDraft(kind, id, label)`, so the thing arrives as a mention (see
  [Frontend](frontend.md)).

## Composer

- The textarea stays the input. The @ and # logic lives in
  `apps/desktop/src/features/chat/mentions/` (`useMentions`, the picker, the highlight
  layer).
- Typing `@` (or `#` for email) opens the picker; picked items become label text tracked
  as tokens and drawn as pills behind the text, and a token deletes as a whole.
- The highlight layer shares the textarea's box and type (`fieldText` in
  `composer.tsx`).

## Assistant tools

`people_search` and `person_details`: reads, without approval. Trusted people don't get
them.

## API

- `POST /v1/people/sync`.
- `GET /v1/people/duplicates`, `POST /v1/people/{id}/split`.
- `DELETE /v1/people/{id}`, `GET /v1/people/removed`,
  `POST /v1/people/removed/{id}/restore`.
- `POST /v1/people/merge/preview`, `POST /v1/people/merge`, `/people/{id}/merge`,
  `POST /v1/people/merges/{id}/undo`.
- What a person's page shows (UI in [Frontend](frontend.md)):
  - `GET /v1/people/{id}/memory`: what memory holds about them;
  - `GET /v1/people/{id}/events`: what's coming up with them (events they're invited to
    by email, or whose title names them; recurring ones once);
  - `GET /v1/mail/threads?person=<id>`: recent email with them;
  - `GET /v1/people/{id}/conversations`: chats where they were @-mentioned.
- `GET|PUT /v1/people/{id}/access`: see [People the user trusts](trusted-people.md).
- `GET /v1/mentions?q=`, `GET /v1/mentions?kind=mail`.
- Changes publish `PeopleChanged`.

## UI pieces

- Channel icons and avatars: `src/components/people.tsx`.
- The People panel (`features/people/`), its delete, merge and "Removed contacts" flows:
  [Frontend](frontend.md).

## Tests

`people/tests.rs` covers the edge cases of deleting and restoring; keep a deleted card
from resurrecting anyone.
