# Memory

What the assistant knows about the user, built to stay small in the prompt however much
accumulates. The code is in `crates/core/src/memory/`. It works like a tiny file system:
a capped **profile** always in the prompt, and a **library** of short notes by path,
found through FTS5 and, when the user turns on "Understands meaning", by embeddings, then
recalled into the prompt within a strict budget (`memory/recall.rs`).

## Rules

- **Keep the prompt small:** local models have ~8k-token contexts. The profile is capped
  at 1,200 characters; recall adds at most 4 notes and 1,600 characters.
- **Only the user's own statements become memories.** Never weaken `looks_secret`, the
  "don't remember this" opt-out (`learn.rs`), or the rule that external content (tool
  output, calendars, emails, other people's messages) is context, not a source of facts.
  Mail never reaches memory learning (it reads only the user's own messages).
- **Someone the user trusts:** their conversation is never learned from (`learn_from`
  skips it), and their turns never recall or show the owner's memory (see
  [People the user trusts](trusted-people.md)).
- **Memory tools need no approval** (memory is local), but writes must stay visible in
  the chat and undoable: return `revision` from every write so the UI can offer Undo.
- **Recall by meaning is optional and must stay so:** words-only recall is the fallback
  whenever the embedding model is off, missing or slow. The embedding model is never a
  cloud source. Writers only publish `MemoryChanged`; the background `semantic::run`
  keeps the vectors current.
- **Notes about people** link to the people directory through `subject` (person id) only
  when it's unambiguous (`memory/link.rs`): never guess between two Léas.
- **Internal model calls** (learning, mail triage, anything the user doesn't read as it
  streams) use `OpenAiCompatible::complete(model, messages, ChatOptions::QUICK)`
  (`ChatClient::complete`): thinking off on local sources, a harmless no-op elsewhere.
  Chats keep `stream_chat`. See [Models](models.md). Internal calls never carry pictures.
- The recalled `<memory>` block is labelled as data, not instructions.

## Two tiers

| Tier | What | In the prompt |
|---|---|---|
| Profile (`profile.md`) | The few key facts about the user: name, city, language, work, household. Capped at 1,200 characters. | Always |
| Library (`people/sam.md`, `habits/mornings.md`, `preferences/food.md`, `places/…`, `work/…`, `interests/…`, `health/…`, `notes/…`) | Short markdown notes, usually bullet facts, one per person or topic (4,000 characters max each). | Only what's relevant |

The onboarding's "About you" step appends what the user writes (name, place, free text)
to the profile (see [Onboarding](onboarding.md)).

## Storage

- `memory_notes` in the encrypted database, with an FTS5 index (`memory_fts`,
  `unicode61 remove_diacritics`) kept in step by triggers, and `subject` on notes (the
  linked person's id).
- `memory_revisions`: every change first records the note's previous state here, so any
  change can be undone.
- `memory_learned`: learning progress per conversation.
- `memory_vectors`: embeddings (see [Finding by meaning](#finding-by-meaning)).

## Recall, every message

`recall.rs` takes notes from three places, merged best first:

1. **People:** notes linked to anyone the message @-mentions or names (full name,
   nickname, or a first name only one person has) come first.
2. **Words:** the user's message (plus their previous one, for follow-ups) becomes an FTS
   query (stopwords dropped, longer words matched as prefixes).
3. **Meaning** (when the user turned it on): the message's embedding against every
   note's, by cosine similarity; a note counts at 0.6 or more and within 0.08 of the best
   match.

Words and meaning are merged by reciprocal rank fusion (`1/(60 + rank)` per list), so a
note found both ways ranks highest. The profile and up to 4 notes, 1,600 characters at
most, go into the system prompt inside a delimited `<memory>` block that's labelled as
data, not instructions.

## Finding by meaning

`semantic.rs`; optional, off by default (`Settings.memory_semantic`).

- **The model:** a small multilingual embedding model (IBM Granite Embedding 278M, Q6_K
  GGUF, 236 MB, 768 dimensions, Apache-2.0; English, French, German, Spanish and eight
  more languages), pinned in `catalog.json` under `embeddings` and downloaded like chat
  models (see [Models](models.md)).
- **Serving it:** a second `llama-server --embedding` on the CPU (loopback, random port,
  its own key file), started on demand and stopped after 20 idle minutes. With no
  built-in runtime, the user's Ollama (`granite-embedding:278m`) serves `/v1/embeddings`
  instead. Never a cloud source.
- **Vectors** are little-endian f32 blobs in `memory_vectors` (one per note, with the
  model and a hash of the embedded text), compared by brute force in Rust (no loadable
  SQLite extensions: the workspace forbids unsafe code).
- **Keeping them current:** a background task (`semantic::run`) embeds new and changed
  notes on `memory_changed` (backfill included) and re-embeds everything if the model
  changes; deleted notes take their vector along (foreign key).
- **Fallback:** if the model is off, missing or slow (6 s for a message, start
  included), recall carries on with words alone.
- Settings › Memory shows it as "Understands meaning" with the download size and
  progress.

## Tools

`memory_search`, `memory_read` and `memory_list` for anything not already recalled, and
`memory_write` (add facts, de-duplicated), `memory_update` (rewrite a note) and
`memory_forget` (remove facts or a note). None needs approval, since memory is local and
private. Writes show in the chat as one quiet line ("Remembered that Sam is your
brother") with Undo (`POST /v1/memory/undo/{revision}`). Reads aren't shown.

## Learning in the background

After a reply, a conversation is scheduled to be learned from once it's been quiet for 2
minutes (`MIMI_MEMORY_QUIET_SECS` changes that).

- **The pass:** the active model gets the current profile, the list of notes, the notes
  related to the conversation, and the new messages since the last pass
  (`memory_learned`). It replies with a JSON plan of facts to add or remove per note,
  which is applied with de-duplication and the profile cap.
- **No thinking:** the pass asks with `ChatOptions::QUICK` (see [Models](models.md)), and
  the plan starts with a short `facts` list, which keeps non-thinking models from
  skipping people or details. With Qwen3 8B through Ollama a pass takes 15-25 s instead
  of 40 s to over 2 minutes.
- **Sources:** only the user's own messages count as sources; the assistant's replies
  are context. Messages that are only photos are skipped, and no pictures are sent (see
  [Photos](photos.md)). Conversations of people the user trusts are never learned from.
- **Exclusions:** a message where the user asks not to remember something is left out
  before the model sees it.
- **Restarts:** unread conversations are picked up after a restart.
- Personality and custom instructions never reach the pass (see
  [Personality and instructions](personality.md)).

## Never stored

Anything that looks like a secret (passwords, codes, card or account numbers, keys):
`looks_secret` is checked on every write path, including Settings › Memory.

## The user in control

- Settings › Memory (`#/settings/memory`) shows and edits the profile and every note,
  with the linked person on each note.
- "Learn from conversations" can be paused (`Settings.memory_learning`). Paused, the
  writing tools disappear and nothing said meanwhile is learned later.
- "Forget everything" deletes it all.
- When a cloud model is active, the page says that relevant memories are sent with
  messages.

## People

Knowledge about people lives in `people/<name>.md`. `link.rs` links a note to someone in
the people directory (`subject` = their id) when it's clear who:

- the file name or title matches their full name or nickname, or
- only one person has that first name, or
- the user @-mentioned them in the conversation the note was learned from (which
  settles two Léas), or
- the note spells out the full name.

Ambiguous notes stay unlinked rather than guess. Links are made after each learning pass
and on every memory or people change, and redone when a person is deleted or merged
away. Deleting a person keeps their notes: the relink after `PeopleChanged` clears their
`subject`, and links them again after a restore when the name is clear. Merging moves
`subject` to the kept person in the same transaction, and undoing the merge puts it
back (see [People](people.md)). `GET /v1/people/{id}/memory` lists a person's notes; a
person's page in People shows them.

## API

`GET /v1/memory`, `GET|PUT|DELETE /v1/memory/note?path=`, `PUT /v1/memory/profile`, `PUT
/v1/memory/learning`, `PUT /v1/memory/semantic` (turning it on starts the download),
`POST /v1/memory/undo/{revision}`, `POST /v1/memory/forget-all`, `GET
/v1/people/{id}/memory`. Changes publish `memory_changed` (`MemoryChanged`).

## Tests

- Test recall by meaning with `semantic::tests::FakeEmbedder`, not a real model.
- To try learning for real, run with `MIMI_MEMORY_QUIET_SECS=15` so the background pass
  runs soon after a chat; it logs `learning pass done … notes_changed=N`. Use a scratch
  daemon (see [Development](development.md)).
