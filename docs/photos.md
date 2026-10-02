# Photos

The user can send photos with a message, from the app, a browser or a messaging app
("📷 📷 add these dates to our calendar"). Only pictures for now: an attachment has a
`kind` (only `image` today) so videos can follow as a kind of their own (voice messages
are transcribed instead: [Voice](voice.md)).
Normalising is `crates/core/src/attachments.rs`, what each model can see is
`providers/vision.rs`, the messaging apps' side is `channels::photos`, and the UI is
`apps/desktop/src/features/chat/photos.tsx`.

## Rules

- **Normalise on arrival, whatever brought them:** decode (anything else, HEIC
  included, is refused in plain words), turn upright from EXIF, shrink, save again.
  Saving again drops all metadata (location, camera, time).
- **Keep them in the encrypted database** (`message_attachments`), never as files. They
  go with their message and conversation.
- **Serve only the re-encoded JPEG/PNG**, with `nosniff` and `CSP: sandbox`. The desktop
  app shows them as `data:` URLs: the CSPs allow no `blob:`. Only `transport.ts` knows
  how they're fetched (`attachmentUrl`).
- **Never drop photos silently.** A model that can't see gets a note instead, the
  message is marked `attachments_unseen`, the UI says so, and messaging apps get
  `channels::UNSEEN_NOTE` (a trusted person gets `GUEST_UNSEEN_NOTE`: they can't change
  the owner's models).
- **Unknown is no:** a source that says nothing about seeing counts as unable to.
- **Built-in models are refused as the model for photos**; removing its source clears it.
- **Internal calls (memory learning, mail) never get images**; learning reads only what
  the user wrote.
- **Messaging apps download a photo only once the sender is known to be the owner**,
  within 20 MB. Never log the Telegram token.
- **Commands and answers to prompts never pick up photos.**
- No model names outside Models in the UI.

## Protocol

- `SendMessage.attachments: Vec<NewAttachment>`: base64 `data`, optional `name` and
  `mime` (only a hint). At most 10 per message and 20 MB each; the send route alone
  accepts request bodies up to 64 MB. The app shrinks photos over 4 MB or 3,000 px to
  2,400 px JPEG before sending, when the browser can draw them. With photos, `content`
  may be empty.
- `Message.attachments: Vec<Attachment>` (`Attachment { id, kind, mime, name, size,
  width, height }`) and `Message.attachments_unseen` (the model couldn't see them).

## Normalising

`attachments.rs`, the `image` crate (MIT/Apache) with JPEG, PNG, GIF and WebP decoders.

- Every picture, whichever way it came, is decoded with limits (20,000 px a side,
  400 MB of memory), so a file that isn't a picture, or claims to be one, is refused
  with a plain sentence; HEIC gets its own ("share it as a JPEG").
- It's turned upright from its EXIF orientation and shrunk to 1,568 px on its long side
  (what cloud models work at; enough to read a ticket).
- It's saved again: PNG for PNGs and transparent pictures (screenshots stay sharp)
  unless that's over 3 MB, else JPEG at quality 85. Saving again keeps no metadata at
  all: no location, camera or time.
- Names are made safe and take the new extension.
- Decoding runs on a blocking thread.

## Storage

`message_attachments` (migration 0027) in the SQLCipher database: the normalised bytes
as a BLOB, with the message id (`ON DELETE CASCADE`, so deleting a conversation deletes
its photos) and position. Never loose files. `store::messages` fills `attachments` from
it; `attachments_unseen` is a column on `messages`.

## Serving

`GET /v1/attachments/{id}` (auth like every `/v1` route) returns the re-encoded bytes
with their own type (only `image/jpeg` or `image/png` ever),
`X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox`, and a private,
immutable cache header. A browser loads it directly (the session cookie covers it). The
desktop webview can't reach the daemon, so `transport.ts › attachmentUrl` asks the
`chat_attachment` command for the bytes (a `tauri::ipc::Response`) and shows them as a
`data:` URL; the page's CSP (Tauri's and the daemon's) allows `data:` pictures, not
`blob:`.

## Who can see

`providers/vision.rs`.

- **Anthropic:** every Claude model.
- **The built-in runtime:** no, since its catalog models run without their picture
  encoder (llama-server's `--mmproj` projector file, not downloaded; Gemma 3's ships
  next to the GGUF on Hugging Face, so adding it is a download of a second file). See
  [Models](models.md).
- **OpenAI-compatible sources are asked:**
  - on the user's machines, Ollama's `POST /api/show` (`capabilities` has `vision`, or a
    `projector_info` on older versions), LM Studio's `GET /api/v0/models/{id}`
    (`type: vlm`) and llama.cpp's `GET /props` (`modalities.vision`);
  - for any source, `/models` (OpenRouter's `architecture.input_modalities`, Mistral's
    `capabilities.vision`);
  - on `api.openai.com`, the families known to see (gpt-4o, gpt-4.1, gpt-5, o-series,
    without audio/realtime/TTS models).
- A source that says nothing counts as no.
- Answers are cached ten minutes per source and model (an unreachable one isn't cached).
- `GET /v1/models/vision[?provider_id&model]` answers for one model, or without one for
  what a new message gets (the default, else the model for photos), so the composer can
  say so before sending. `ModelInfo.sees_images` carries what model lists say, and for
  sources on the user's machines (which tell only when asked about one model) the
  models route asks for each model that lists leave unknown.

## Model for photos

`Settings.photo_model`: optional, `None` by default; Settings › Models.

- When the default model can't see, a message with photos, and the user's message right
  after it ("add the second one"), goes to this model instead (`chat::photo_model`,
  decided once the history is loaded): the whole turn, tool calls included, and the
  reply records it as its `model`. Everything else stays on the default, so a cheap text
  model can keep answering while photos cost a little more.
- It's skipped when a model was picked for the message, when the default sees, and when
  its source is gone or unusable (logged; the default answers and says it can't see).
- `PUT /v1/settings` refuses a built-in model for it (they can't see), and removing a
  source clears it.
- In Models the row is greyed out while the default sees; its picker
  (`ModelPickerDialog forPhotos`) lists only models known to see, and its "…" menu turns
  it off.

## Prompts

- `ChatMessage.images` (`ImagePart`: mime + bytes).
  - OpenAI-compatible requests turn a message with pictures into content parts:
    `image_url` parts with `data:` URLs, then the text (none when it's empty).
  - Anthropic gets `image` blocks (base64) before the text block.
- Pictures go with the latest two user messages that have some (the new one included),
  at most 10 in all; each takes 4,000 characters from the history budget. Older photos
  become a note in their message's text ("The user sent 2 photos here, no longer shown
  to you.").
- For a model that can't see, no picture is sent. The message gets a note (the user
  sent N photos; if it matters, say you can't see them and that a model that can, or
  one just for photos, is chosen in Models; for someone the user trusts, ask them to
  type out what matters instead, since they can't change the owner's models), older ones
  "which you couldn't see" (when they weren't seen when sent either), and the message is
  marked `attachments_unseen`.
- Internal calls (memory learning, mail) never carry pictures, and learning skips
  messages that are only photos.
- While an email account is connected, the owner's prompt names each message's photos
  with their ids (`chat:<id>`), so the assistant can attach one to an email written in
  the same chat; a trusted person's prompt never does. See
  [Email](email.md#the-assistants-bcc-and-files).

## Messaging apps

A picture is fetched only once its sender is known to be the owner, and only up to
20 MB. The caption is the message's words.

- **Telegram:** the largest `photo` size (or a `document` whose type is a picture),
  through `getFile` and the file endpoint of the configurable Bot API base. Errors are
  mapped without their text, which would hold the token. See [Messaging](messaging.md).
- **Signal:** image attachments of a Note to Self, fetched on the Signal thread. See
  [Signal](signal.md).
- **Matrix:** `m.image`, through matrix-sdk's media API, which decrypts it; caption per
  the spec. See [Matrix](matrix.md).

A photo that can't be fetched is said so ("Try sending it again").

`channels::photos::deliver` then hands turns to `channels::converse`, gathering them per
conversation, because people send photos first and words after, and apps deliver
several photos (a Telegram album, one Matrix event each) one by one:

- a message without photos takes whatever photos are held (and their captions) along;
- photos with words go once nothing more came for 3 seconds;
- photos without words wait 45 seconds for words, then go on their own;
- a batch that reaches 10 goes at once.

Commands (`/new`) and answers to prompts ("yes", a reaction) are handled before, so they
never pick up photos. When the model couldn't see them, the app also gets
`channels::UNSEEN_NOTE`, or, in the chat of someone the user trusts,
`GUEST_UNSEEN_NOTE` (type it out instead).

## UI

`features/chat/photos.tsx`: composer thumbnails, the grid in the bubble, the lightbox.

- The composer takes photos from `+` › Add photos, paste, and drops anywhere on the chat
  (with a quiet overlay). The Tauri windows set `dragDropEnabled: false`, so the webview
  gets HTML drops like a browser does.
- It shows removable thumbnails, and, when neither the default model nor the model for
  photos can see, "This model can't see photos. Choose one that can, or one just for
  photos, in Models."
- A sent message shows its photos above the bubble (one in its own shape, several as a
  grid), opening in a lightbox; a message the model couldn't see says so under it.
- No model names outside Models.

## Tests

`photo_flow › photos_go_to_the_model_for_photos_when_the_default_cant_see` covers the
model for photos.

## Later: video

The `kind` enum, the table (`kind`, `mime`, BLOB) and `channels::photos` are shaped for
it. Voice isn't an attachment: recordings are transcribed on this computer and only
their words are kept ([Voice](voice.md)).
