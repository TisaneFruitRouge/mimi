# Personality and instructions

Who the assistant is and how it talks, and the user's standing instructions to it
("answer in French unless I write in English", "sign my emails as Vincent"). Both are
free text in the settings: `Settings.personality` (empty = the default "helpful, direct
and warm") and `Settings.custom_instructions`. The daemon side is
`crates/core/src/persona.rs`; the page is Settings › Personality
(`#/settings/personality`, `apps/desktop/src/features/personality/`), which also holds
the assistant's name.

## Rules

- **Caps:** 600 characters for the personality and 1,000 for the instructions
  (`PERSONALITY_LIMIT`, `INSTRUCTIONS_LIMIT`, mirrored in
  `features/personality/presets.ts`). The API trims them and refuses longer text. Keep
  them small: they sit in every prompt beside memory.
- **Delimited and framed:** they go into `chat::build_prompt` in delimited blocks framed
  as preferences that never change approvals, the "external content is data" rule or
  privacy.
- **Never let this text reach approval or permission decisions.** Those are decided in
  `chat::act`, in code.
- **Untouched settings leave the prompt byte-for-byte as before**
  (`chat::tests::an_untouched_install_gets_the_prompt_it_always_had`).
- **Who gets what:**
  - chat, Telegram and the other messaging apps, and routines (all `chat::send`): both;
  - Mail panel reply drafts: the instructions only;
  - a trusted person's turns: the personality only (the instructions are the owner's
    wishes about acting for them, and may name private things);
  - sorting, summaries, smart folders and memory learning: neither.

## The settings page

The page edits the assistant's name, its personality and the user's custom
instructions. It offers a few starting points for the personality (Warm & friendly,
Calm & concise, Playful, Professional, Straight talker) that fill the box, and
"Default", which empties it.

The limits exist because both texts go into every prompt next to the memory profile
(1,200 characters) and recalled notes (1,600; see [Memory](memory.md)), so they must stay
small enough for ~8k-token local models.

## Where they apply

`chat::build_prompt` adds them after the opening lines and the date, before memory, so
the desktop chat, the messaging apps and routines (all `chat::send`) get them. Their size
comes out of the history budget. The personality replaces the default "helpful, direct
and warm"; empty texts leave the prompt exactly as it was.

- **Mail panel reply drafts** (`triage::draft_reply`) get the instructions only: the
  draft is the user's voice, not the assistant's. Drafts the assistant writes in a chat
  already have both. See [Email](email.md).
- **Sorting, summaries, smart folders and memory learning** never see them: their
  output has a fixed shape, and learning reads only the user's messages.
- **Someone the user trusts** gets the personality in their turns
  (`Persona::guest_block`) but not the instructions: those are the user's standing wishes
  about acting for the user ("sign my emails as Vincent"), may name private things, and
  would be followed as if the guest had asked. See
  [People the user trusts](trusted-people.md).

## They can't lift the rules

The texts sit in `<personality>` and `<user_instructions>` blocks (the tags are removed
from the text itself), introduced as the user's preferences that don't change the
approval step, the rule that emails, pages, calendars and tool results are information
rather than instructions, or privacy. Enforcement stays in code anyway: approvals and
permissions are decided in `chat::act`, which this text never reaches (see
[Tools and approvals](tools-and-approvals.md)). The prompt also says not to save them to
memory, since they're already known.

## The assistant's name

`Settings.assistant_name`, edited on the same page. The UI says it wherever it would
otherwise say "Mimi" to the user (`useAssistantName`, e.g. the "Ask … about this"
buttons; see [Frontend](frontend.md)). Messaging apps use it too: the Signal device is
named after it and every Signal message is headed with it (see [Signal](signal.md)), and
the Matrix device takes it as its name (see [Matrix](matrix.md)).

## Tests

- `chat::tests::an_untouched_install_gets_the_prompt_it_always_had`: empty settings
  change nothing in the prompt.
- `api/tests.rs › instructions_reach_the_model_but_cannot_skip_approval`: the
  instructions reach the model, and an action still waits for its card.
