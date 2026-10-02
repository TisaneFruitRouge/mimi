# Email

Mail is read over IMAP and sent over SMTP with the account's app password, in
`crates/core/src/mail/`. There is no registered app, no Microsoft or Google sign-in, and
nothing between the user's computer and their mail service. The Mail panel is
`apps/desktop/src/features/mail/`. Crates: async-imap, mail-parser, html2text, lettre
(all MIT/Apache), ammonia (its cssparser is MPL-2.0) and markup5ever_rcdom (MIT/Apache)
for showing mail and cleaning what's sent; TipTap (MIT) for writing it. Connections in
general: [Connections](connections.md).

## Rules

- **Email is untrusted.** Tools label mail as data. The only tool that acts,
  `mail_send`, asks for approval unless Settings › Permissions lets it go and every
  recipient, Bcc included, is known (`mail::known`); its card shows To, Cc, Bcc (said to
  be hidden from the others), Subject, the whole message and every file with its size
  and where it comes from. `api/tests.rs › mail_flow` and `api/tests/mail_files.rs`
  prove a hostile email can't send mail, a blind copy or a file without approval; keep
  them passing.
- Model calls about mail (`model.rs`: sorting, summaries, drafts) get no tools; their
  output is shown to the user or parsed into a fixed shape.
- Mail never reaches memory learning: learning reads only the user's own messages.
- `mail::connect` signs in over IMAP and SMTP before anything is saved.
- TLS or STARTTLS, checked against the OS trust store (rustls-platform-verifier).
  Unencrypted connections, and self-signed certificates, are accepted only for servers
  on this computer (Proton Bridge).
- SMTP says hello as `[127.0.0.1]`, never the computer's name. Message-IDs use the
  sender's domain.
- Inbox, Sent and Archive only: Spam and Trash are never read.
- Finding servers never uses a third-party lookup service.
- Suspicious mail is never sorted or summarised by the model, and never filed into a
  smart folder. Keep the phrases specific: mail *about* AI must not be flagged. Only
  the Sent folder is exempt from the check.
- The user's own smart folder choices are never overwritten by a sorter.
- The Jev key is never in `Settings` (clients read that). Automatic and suspicious mail
  is still filed locally, never sent to Jev.
- A draft's `from` must be one of the account's addresses; anything else is refused.
- **The assistant's Bcc and files.** Its tools may add blind copies and attach files,
  under these rules (`tools.rs`, see [The assistant's Bcc and files](#the-assistants-bcc-and-files)):
  - Bcc addresses are recipients like any other: `call_targets` lists them, so automatic
    sending still asks unless every one is known, and the card and the summary line
    (all a Telegram approval shows) name them as a hidden copy.
  - An email with files attached always asks, even when sending is automatic
    (`Tool::always_asks`), and its card offers no "Don't ask again".
  - Files only by reference, never bytes from the model: an attachment of an email in
    the user's mail (`email:<message>:<index>`, never from mail flagged suspicious) or a
    photo the user sent **in the same conversation** (`chat:<id>`). Never a file from
    the computer (acting on the computer is out of scope), never a photo from another
    chat, never a trusted person's.
  - Bcc counts towards `MAX_RECIPIENTS`. The message sent never carries the Bcc line;
    only the copy filed in Sent does (`smtp::build`).
- HTML the user writes is cleaned against a strict allow-list before it's sent
  (`smtp::outgoing_html`): no styles, scripts, event handlers, forms or remote pictures;
  pictures only `cid:` ones attached to the message. `body` stays the plain text that
  models and checks read.
- Anyone can write delivery headers: an address on another domain never counts as the
  user's. `my_addresses` is the accounts' own addresses only, plus whatever was filed as
  outgoing (Sent).
- Attachments are not stored, only their names. A served attachment is always an
  `application/octet-stream` download with `CSP: sandbox`, never rendered in the web
  UI's origin; programs and scripts are never opened.
- Showing mail is only for display: models, tools, triage and mentions keep the
  plain-text `body`. The HTML is sanitized at sync and again when served; remote
  pictures load only when the user clicks "Load images". Never add `allow-scripts` to
  the frame's sandbox: with same origin it would lift the sandbox.
- In Text mode email is plain text, never linkified.
- Thread ids are never reused, so a stale id can't reach another conversation.
- Sending from the panel or from a draft card is the user's own click, which is the
  approval. Automatic email (Settings › Permissions) still asks unless every recipient
  is known (`mail::known`): see [Tools and approvals](tools-and-approvals.md).
- Never log passwords, and never point tests at a real mailbox.
- **Waiting mail lives in the daemon** (`mail/outbox.rs`, `mail_outbox` in the encrypted
  database), never in the app: closing the window never loses or double-sends it. A
  message cut off mid-send by a crash is marked as not sent, never sent again on its own.
  Only a server that couldn't be reached is retried, a few times; any other problem stays
  on the message and the user is told. See [Undo send and send later](#undo-send-and-send-later).
- **Unsubscribing is only ever the user's click** (`POST
  /v1/mail/threads/{id}/unsubscribe`). There is no assistant tool for it; keep
  `api/tests/unsubscribe_flow.rs` passing. A one-click address is fetched over https
  only, from the internet only (checked after DNS, on the address connected to, and on
  every redirect), with nothing of the user's in the request.
- New-mail notifications never announce an account's first sync, old mail that arrives
  late, the user's own mail or the same message twice, and never show a suspicious
  email's subject (`mail::notify`; see [New mail notifications](#new-mail-notifications)).
  Tests never show a real notification.

## Connecting

`ConnectionSetup::Email { email, password, preset, servers }`. Users only type their
address and password. The preset (`mail::presets`: iCloud, Gmail, Fastmail, Proton
through Bridge on 127.0.0.1, Other) is guessed from the address when not given. Gmail
needs 2-Step Verification for app passwords. Outlook.com needs a Microsoft sign-in and
is listed as not supported yet: the connect form says so rather than failing.

`mail::connect` signs in to IMAP (selecting INBOX) and to SMTP before the connection is
saved; iCloud's IMAP login falls back to the part before the @. The config (address,
password, servers) lives in the connection row, in the encrypted database. The password
field only says "App password" for services that require one. The connect form is
`EmailAccount` in `connect-dialogs.tsx`.

### Finding servers

`mail/discover.rs`, in this order:

1. known consumer domains;
2. the domain's MX mapped to known hosts (Migadu, Google Workspace, iCloud custom
   domains, Fastmail, mailbox.org, Posteo, Infomaniak, OVH, Gandi, Zoho…);
3. the domain's own autoconfig file (HTTPS);
4. RFC 6186 SRV records;
5. probing `imap.`/`mail.` hosts.

Never a third-party lookup service. Add new hosts to `known_mx` / `known_domain`.

## Transport

`net.rs`, `smtp.rs`. TLS or STARTTLS checked against the OS trust store
(rustls-platform-verifier); plain connections and self-signed certificates only for
servers on this computer (Proton Bridge). SMTP says hello as `[127.0.0.1]` and signs in
as `smtp::user` (the account's username). Message-IDs use the sender's domain.

## Sync

Each email connection runs `sync::run` (`sync.rs`) as its connection task, one loop per
account, cancelled on disconnect.

1. Sign in, then a *pass* over Inbox, Sent and Archive (found by special-use attributes,
   else by name; Spam and Trash are never read). Per mailbox:
   - `SELECT`; a UIDVALIDITY different from the stored one drops the mailbox's local
     copy and refetches it.
   - New messages, by UID above the stored high-water mark: the first pass does
     `UID SEARCH SINCE <90 days ago>` (newest 2,000 per mailbox), later ones
     `UID SEARCH UID <last+1>:*`. Sizes first, then bodies in chunks of 25
     (`BODY.PEEK[]`, so reading doesn't mark mail read); messages over 2 MB are stored
     with headers only. Each chunk is stored with the high-water mark it reached (only
     over UIDs actually fetched, in order), so a pass that fails or times out part-way
     carries on from there. A message that can't be read, or takes over 20 s to, is
     stored with its headers and a note as its body.
   - Flag changes and removals: a `UID FETCH <min>:<max> (UID FLAGS)` sweep over what's
     stored.
   - Mail older than 97 days is forgotten (not older mail a search brought in: see
     [Older mail on the server](#older-mail-on-the-server)); the high-water mark becomes
     `max(highest UID seen, UIDNEXT-1)`.
2. `IDLE` on the Inbox for up to 10 minutes, until the server reports news or the app
   pokes the loop (after sending, archiving or "Check for new mail"). Before idling, a
   `UIDNEXT` beyond the stored mark (mail that arrived during the pass) triggers another
   pass instead. Servers without IDLE are polled every 2 minutes.
3. Failures:
   - A connection that synced and then dropped (servers and routers close idle ones)
     just reconnects after 30 s: it isn't a failure
     (`dropped_idle_connections_are_not_errors`).
   - Connections that fail before syncing back off from 30 s, doubling to 15 min, and
     show an error on the connection from the third failure in a row, cleared by the
     next pass that works.
   - A refused password marks the connection as needing attention, says so, and waits
     30 minutes, so a revoked password can't lock the account.

Actions (mark read, archive, filing a sent copy) open a short second session. Archive
uses `MOVE` (or COPY + `\Deleted` + EXPUNGE) to the Archive mailbox, or Gmail's All
Mail. Sent mail is appended to Sent except on Gmail and Proton, which file it
themselves.

## Older mail on the server

`older.rs`, migration 0030. Only the last 90 days are copied, so searching here can't
find older mail; the server can.

- **Asking.** The Mail panel's search shows local results, then a "Search older mail"
  button under them; a server search takes seconds, so it runs only when asked, never
  while typing, and the results stay for the same words. They show below the local
  results as "Older mail on the server" (conversations already above left out). The
  scope's account narrows it; an address scope doesn't (the whole account is searched).
- **Searching.** `UID SEARCH CHARSET UTF-8 BEFORE <window start> TEXT "word" …` (every
  word, split like the local search) in Inbox, Sent and Archive, never Spam or Trash
  (nor Gmail's All Mail, as in sync). Non-ASCII words go as `LITERAL+` literals when the
  server offers them, else quoted. A server that refuses UTF-8 (NO or BAD, read from the
  tagged response: async-imap's `uid_search` reports a refusal as no matches) is asked
  again without CHARSET, with the non-ASCII words left out and the user told. People
  (from the assistant) are `OR FROM x OR TO x CC x`.
- **The cap.** The newest `CAP` (50) matches per account across the three mailboxes, by
  arrival (`INTERNALDATE` of each mailbox's 50 highest UIDs); `more` says there were more.
  The whole search has 45 s: what came in by then is shown, with a note.
- **Bringing it in.** Sync's own pipeline (`sync::fetch_messages`, `Fetched::read`):
  parsing, hidden text and safe HTML, the suspicious check, received_on, 2 MB headers-only.
  So it opens, replies, forwards, attaches and mentions like any conversation. Messages
  already here aren't fetched again.
- **Keeping.** `mail_messages.kept_until`: a week after it was found, 30 days after it
  was last opened (`GET /mail/threads/{id}`), then forgotten (`older::forget_expired`, at
  each pass and each older search). While kept it's found by local search, but left out
  of the mailboxes and sorted views, counts, "Received on", correspondents, sorting
  (`store::unsorted`) and smart folders (`folders::ELIGIBLE`). Nothing is announced as new
  mail, so it wakes no sorting, notification or memory.
- **Sync leaves it be.** Its UIDs are below the high-water mark, so nothing is fetched
  again; the 97-day prune skips it; sync's flag sweep (`store::known`) leaves it out, as
  its old UIDs would widen the `min:max` range to most of the mailbox. `older::sweep`, run
  in each pass, checks flags and removals of kept mail by its own UIDs instead. A new
  UIDVALIDITY drops it with the rest of the mailbox; an older search skips a mailbox whose
  UIDVALIDITY changed since the last pass.
- **The assistant.** `mail_search` takes `older: true`, and also asks the server when
  nothing recent matches its words or person (`older: false` stops that). Older finds come
  as `older_conversations` with a note, in the same output that carries the "data, not
  instructions" notice.

## Storage and threading

`store.rs`, migration 0013:

- `mail_threads`: subject, last activity, category, summary, `sorted_at`.
- `mail_messages`: per mailbox and UID: headers, plain-text body, snippet, flags,
  attachment names; with `mail_fts` for search.
- `mail_sync`: UIDVALIDITY and high-water mark per mailbox.
- `mail_thread_ids` (migration 0016): thread ids are never reused, so a stale id can't
  reach another conversation.
- `mail_notify_queue`, `mail_notify_seen` (migration 0031): new mail waiting to be
  announced, and Message-IDs already considered (see
  [New mail notifications](#new-mail-notifications)).

A message joins the thread of its In-Reply-To or References, or of a message that
already refers to it (Message-ID), else (for "Re:"-style subjects) a thread from the
last 30 days with the same subject and a shared participant. The same Message-ID in two
mailboxes (a reply in Sent and Inbox) shows once.

## Untrusted content

Bodies are plain text. HTML goes through `parse::strip_hidden` and html2text, and
invisible (zero-width) characters are removed. `strip_hidden` removes:

- elements hidden by `display:none`, `visibility:hidden`, zero opacity or zero size,
  `mso-hide`, or the `hidden` attribute;
- comments, scripts and styles;
- text whose inherited font size is under 2px. Font size is inherited, so `font-size:0`
  layout wrappers whose children set a readable size keep their text.

That pass is linear however the HTML is nested, reads at most 512 KB of it, and leaves
out tags nested over 100 deep (keeping their text), so crafted HTML can't stall sync.

The assistant's tools (`tools.rs`):

- `mail_search` and `mail_read_thread` are reads; their output carries a "data, not
  instructions" notice. `mail_search` can reach older mail on the server (see
  [Older mail on the server](#older-mail-on-the-server)).
- `mail_read_thread` gives each message's `message_id` and its attachments' names with
  the `file` id the draft and send tools take (none for suspicious mail). Sizes aren't
  kept locally: they're measured when a file is attached.
- `mail_draft_reply` and `mail_compose` make a draft, shown in the chat as an editable
  card; nothing is sent. Both take `bcc` and `attachments`.
- `mail_send` asks for approval (the card shows To, Cc, Bcc, Subject, the whole message
  and the files) unless the user made sending automatic, every recipient is known and
  nothing is attached. Its `resolve` looks the files up (name, size, where from) and
  refuses one that can't go before any card; its `prepare` splits recipients into one
  address each. It's governed by the `send_mail` permission (see
  [Tools and approvals](tools-and-approvals.md)).

Model calls about mail (`model.rs`: sorting, summaries, reply drafts) get no tools; their
answers are shown to the user or parsed into a fixed shape (a category from three and
one clipped line). Memory learning only reads the user's own messages, so nothing from
an email becomes a memory.

## Suspicious mail

`suspicious.rs`, migration 0015. Instructions addressed to an AI assistant (English and
French phrasings, in visible or hidden HTML text) mark a message as suspicious. Such
conversations:

- are never sorted or summarised by the model (filed as "other"), and never filed into
  smart folders;
- show a warning in the panel;
- carry a `suspicious` note in tool output and # mentions.

Keep the phrases specific: mail *about* AI must not be flagged
(`mail_about_assistants_is_not`). Only the Sent folder is exempt: mail elsewhere is
checked even when its From line names the user, since anyone can write that
(`mail_that_only_claims_to_be_from_the_user_is_still_checked`, migration 0020).

## Received on

Migration 0014, `parse::received_on`. Each incoming message records which of the
user's addresses it arrived at:

1. the top-most X-Original-To/Delivered-To that is plainly theirs
   (`parse::is_own_address`: the account's address, a +tag of it, or any address on its
   own domain, never a shared one like gmail.com);
2. else a matching To/Cc;
3. else the account's address.

Anyone can write these headers, so an address on another domain never counts, even when
the mail was addressed to it (aliases there show as the account's address). Mail stored
before the column existed is filled from To/Cc at the next sync, and older values that
fail the rule are reset.

`store::Scope` (account or address) narrows lists and counts (`?account=` /
`?address=` on `/mail` and `/mail/threads`). The Mail panel's "Received on" section
shows each account's addresses once there's more than one.

These addresses are not "the user" elsewhere: `my_addresses` (from_me, "(the user)" in
transcripts, participants) is the accounts' own addresses only, plus whatever was filed
as outgoing (Sent).

## Sending from aliases

`MailDraft.from` picks one of the account's addresses: its own, or one that mail arrived
at and that `parse::is_own_address` accepts. Replies default to the conversation's
`received_on`. Anything else is refused. SMTP signs in as `smtp::user` (the account's
username).

## Sorting

`triage.rs`, `Settings.mail_sorting` (Settings › Privacy). `triage::run` wakes after a
pass that changed something (and every 5 minutes). While sorting is on and a model is
set up, it takes the newest unsorted Inbox conversation from the last 14 days, one at a
time, waiting while a chat reply is being written.

- Newsletters and automatic mail (List-Unsubscribe, List-Id, Precedence bulk/list,
  Auto-Submitted, no-reply-style senders) are filed as "other" without the model.
- Anything else goes to the model with `ChatOptions::QUICK` (no thinking; see
  [Models](models.md)) and gets needs_reply, important or other plus a one-line
  summary.
- A thread with a newer message is sorted again.
- If the model fails, the queue stops until the next wake-up.

## Jev as the sorter

`jev.rs`, `Settings.mail_sorter`. The user's own model sorts by default. In Settings ›
Privacy they may choose Jev instead: TypeSafe's cloud decision model
(`POST https://api.typesafe.ai/v1/systemone`), with their own key. It's shown with the
cloud badge there and in the Mail panel.

- The key is checked with TypeSafe before it's saved, and kept in the `settings` table
  row `jev`, never in `Settings`, which clients read.
- `DELETE /v1/mail/jev` removes the key and puts the sorter back to the model.
- Automatic and suspicious mail is still filed locally, never sent.
- Jev writes no text: no summaries.
- Tests point `state.mail.jev_api` at a fake.

Local "System One" models (Laya, GLiNER2.5-Decide) were tried on a labelled set of 180
emails and scored well below the chat model for these categories (about 40–50% vs 66%),
so none ships yet.

## Smart folders

`folders.rs`, migration 0017. The user names a folder and describes what goes in it.
The chosen sorter (their model, or Jev with one yes/no per folder) files conversations
in the sorting queue after new mail is sorted (even with sorting off: making a folder
asked for it), newest first, one conversation at a time.

- Rows in `mail_folder_threads` record each check (`source` auto | user). The user's
  choices are never overwritten, and a new description forgets only the sorter's.
- Suspicious mail is never filed.
- Folders are labels in Mimi only; nothing changes on the mail server.
- `?folder=` on `/mail/threads` lists a folder.
- Each folder has an icon and a colour from fixed sets (`folders::ICONS`/`COLORS`,
  migration 0019, mirrored in `features/mail/folder-looks.tsx`); the API refuses other
  names.

## Delete and forward

- `DELETE /v1/mail/threads/{id}` moves every copy to the account's Trash (found by
  `\Trash` or by name, made if missing) and forgets it here.
- `forward_of` on a draft re-attaches the original's attachments, fetched from the
  server.

## Writing: Bcc and attachments

- `MailDraft.bcc`: blind copies, in the envelope and in the Sent copy only.
- `MailDraft.attachments` (`NewMailAttachment`: name, content type, base64 data): files
  the user attached with the paperclip, pasted into the message or dropped on the draft
  (outside the text). Sent as they are; the name loses any folder part. Together with
  forwarded attachments and pictures in the text they can add up to
  `smtp::MAX_ATTACHMENTS` (20 MB); `/mail/send` takes requests up to
  `smtp::MAX_REQUEST_BYTES`.
- Pictures pasted or dropped into the text stay there: see
  [Writing with formatting](#writing-with-formatting).

## Writing with formatting

The text of a message is written in a rich text editor (`features/mail/body-editor.tsx`,
TipTap on ProseMirror, MIT): bold, italic, underline, links (⌘/Ctrl K, web and email
addresses only), bulleted and numbered lists, quotes, and pictures where they were
pasted or dropped. Enter starts a new line, as in a mail app.

- **Two versions.** `MailDraft.body` is always the plain text (one line per line, "- "
  and "1. " for lists, "> " for quotes, "words (address)" for links, "[image: name]"
  for pictures): models, tools, approval cards, triage and `mail::known` read only that.
  `MailDraft.html` is set once the text has any formatting; a message without any goes
  as plain text, exactly as before. `features/mail/mail-text.ts` turns one into the
  other.
- **Pictures in the text** are draft attachments with a `content_id`, shown in the HTML
  as `cid:<content_id>`. The draft lists only the other files under the text; a picture
  taken out of the text leaves the draft. Text pasted with pictures pastes as text, and
  pictures in pasted HTML (from a web page) are left out: they'd load from elsewhere.
- **Drafts the assistant writes** have a `body` and no `html`: they open as lines of
  text. When the text is replaced without the formatting (the reply box's "Write
  again"), the editor sees the HTML no longer says what the text does, and the text wins.
- **Sending** (`smtp::build`): without `html`, `text/plain` (in `multipart/mixed` with
  files), as before. With it, `multipart/alternative` (text, then HTML); the HTML inside
  `multipart/related` with the pictures it shows; inside `multipart/mixed` when files are
  attached. An inline picture the HTML doesn't show goes as an ordinary attachment.
- **The HTML is cleaned before it goes** (`smtp::outgoing_html`, ammonia): only the
  editor's tags (`div`, `p`, `br`, `strong`/`b`, `em`/`i`, `u`, `s`, `a`, `ul`, `ol`,
  `li`, `blockquote`, `img`); links only http(s) and mailto; pictures only `cid:` ones
  attached to this message, as png, jpeg, gif or webp, under a content id of plain
  characters; `width` and `start` only as numbers. No styles, classes, ids, event
  handlers, scripts, forms, frames, SVG or remote pictures. Quotes get a fixed left
  border. Tests: `smtp::tests` (MIME shapes, hostile HTML) and
  `tests.rs › formatted_mail_goes_with_its_pictures_in_the_text`.

## The assistant's Bcc and files

The draft and send tools take `bcc` (addresses) and `attachments` (`file` ids). A file
is a reference the daemon resolves, so the model never handles its content:

- `email:<message>:<index>`: an email's attachment, as `mail_read_thread` lists it.
  Refused for mail flagged suspicious: a hostile email mustn't get its own attachment
  passed on from the user's address.
- `chat:<id>`: a photo the user sent in this conversation. While an email account is
  connected, the owner's prompt names the photos of each message with their ids
  (`chat::files_note`); a trusted person's never does.

At most 10 files, 20 MB together. `find_files` checks each one (and, for an email's,
fetches it from the server to measure it); the error says plainly why a file can't go.
In a draft each becomes a `NewMailAttachment` with empty `data` and a `source`
(`MailAttachmentSource::Email { message, index, size }` or `Chat { attachment, size }`;
`size` only for showing). `mail::send` (`attached`) fetches them when the email is sent,
whether the assistant sends it or the user does from the draft card: an email's from the
server, a photo from the encrypted database, never one from a trusted person's
conversation (theirs, not the owner's). The draft card shows them with their name, size,
origin and, for photos, a thumbnail; the user can take any of them out before sending.

## Attachments

Attachments are not stored, only their names. `GET
/v1/mail/messages/{id}/attachments/{index}` fetches the message from the server and
returns the file, always as an `application/octet-stream` download with `CSP: sandbox`,
never rendered in the web UI's origin.

The desktop app's `open_attachment` command writes it to the app cache (emptied at
launch) and opens it with the system's app. Programs and scripts
(`attachments::is_program`) are only saved to Downloads and revealed, never opened.

## Showing mail

`mail/render.rs`, `mail/images.rs`, migration 0018. The Mail panel shows a message three
ways, chosen per viewer (Text · Formatted · Original, kept in localStorage, default
Original):

- **Text**: the plain-text body, never linkified.
- **Formatted**: Markdown the daemon makes from the HTML (or the text), with everything
  the sender wrote escaped.
- **Original**: the email's own HTML, made safe.

None of it ever reaches a model: models, tools, triage and mentions read only the
plain-text `body`.

**Kept at sync.** The safe HTML is stored with the message (`mail_messages.html`: `''`
without an HTML part; `NULL` for mail copied earlier or too large, fetched from the
server the first time it's shown, then kept), so mail opens offline.

**Making it safe:** `parse::strip_hidden`, then ammonia with an allow-list of tags,
attributes (no ids, classes, event handlers) and inline CSS (no `url()`, escapes,
comments, positioning, or functions other than colours and `calc()`); links only
http(s), mailto and tel; relative addresses dropped (they'd point at the app); inline
`cid:` pictures put in as `data:` addresses. It runs again each time the HTML is served.

**Remote pictures** are left out when served: they tell the sender when and where the
mail was opened. "Load images" (`POST /v1/mail/messages/{id}/images`) has the daemon
fetch only that message's picture addresses:

- http(s) only; names resolved, and every address on this computer, the local network or
  reserved ranges refused, also after DNS and on every redirect;
- no proxy, cookies or referrer;
- pictures only (no SVG), 5 MB each, 20 MB and 30 s per message.

They come back as `data:` addresses, so the app's CSP (`img-src 'self' data:`) stays as
is. `GET /v1/mail/messages/{id}/content` returns `MailContent` (safe HTML, Markdown, how
many pictures are hidden).

**The frame.** The panel shows the HTML in an iframe with `sandbox="allow-same-origin
allow-popups allow-popups-to-escape-sandbox"` (never `allow-scripts`: with same origin
it would lift the sandbox) and its own CSP (`default-src 'none'; img-src data:;
style-src 'unsafe-inline'`). The page sizes the frame to its content and opens clicked
links with `openExternal`. WebKit (the desktop app, Safari) runs no event listener at
all in a frame without scripts, so there that click handler never fires: the frame's
`<base target="_blank">` makes each link a new-window request instead, which the desktop
app's main window (`on_new_window` in `create_main_window`) opens in the system browser
and denies; a browser opens a tab. The frame also re-dispatches its right-clicks to the
page (with the link or selection under them), so the message's menu opens there (see
[Design system](design-system.md)).

## Flags

`mail/flags.rs`. A conversation is flagged (starred) when any of its messages has
`\Flagged` on the server; sync reads flags in its sweep. The user flags from the star in
a row's margin (shown on hover, filled when flagged), the reader's Flag button or the
right-click menu; `features/mail/flag.tsx` holds `useFlagThread`, which a keyboard
shortcut can call.

- `POST /v1/mail/threads/{id}/flag` (`FlagThread { flagged }`). Flagging sets `\Flagged`
  on the latest message (each stored copy of it: a reply can be in Sent and the Inbox),
  over `UID STORE +FLAGS.SILENT`; unflagging clears it from every message, so no older
  flag keeps the star on. Other flags are left alone. Asking for what's already so does
  nothing.
- The local copy changes first and `MailChanged` goes out, so the star moves at once;
  the request then waits for the server (30 s at most). If the server refuses or can't
  be reached, the local change is undone, `MailChanged` goes out again and the request
  fails with a message the panel shows: the star never claims something the mail
  account doesn't have (`a_flag_the_server_refuses_is_undone`). After success the change
  is written again, in case a sync pass read the server just before it. Changes run one
  at a time, so a quick flag-unflag can't cross over.
- The panel is optimistic too: `useFlagThread` changes the cached lists and conversation,
  and puts them back if the request fails.
- The Flagged view (`MailBox::Flagged`) lists flagged conversations in Inbox, Sent and
  Archive alike: archiving keeps the flag.
- The assistant has no flag tool: flagging changes the mail account, so by the approval
  rule it would need a card, too heavy for a star.

## Unsubscribing

`mail/unsubscribe.rs`, migration 0029. Newsletters and mailing lists say how to leave
them in `List-Unsubscribe` (RFC 2369) and, for one-click, `List-Unsubscribe-Post:
List-Unsubscribe=One-Click` (RFC 8058).

- **Kept at sync.** `mail_messages.list_unsubscribe`, `list_unsubscribe_post` (unfolded,
  capped at 2 KB) and `list_id` (the bare List-Id, lowercased): `''` when the message
  has none, `NULL` for mail stored before the columns existed. For those, the first
  time a conversation whose latest message is automatic is opened, its source is read
  from the server once (`mail::source`) and the headers kept. A person's email is never
  fetched for this.
- **The offer** comes from the conversation's latest message from someone else. Ways,
  best first:
  1. *one-click*: the Post header and an https address. The daemon POSTs
     `List-Unsubscribe=One-Click` (form-encoded) and nothing else: no cookies,
     referrer or proxy, 10 s timeout, at most 3 redirects. The address is refused
     unless it's https with no password; names are resolved and only internet
     addresses kept (`images::PublicOnly`, so the connection goes to the address
     checked and DNS rebinding can't swap it); literal addresses on this computer,
     the local network, link-local, CGNAT, NAT64/6to4 forms of those and other
     reserved ranges are refused (`images::is_public`), and every redirect is checked
     the same way. A 2xx answer is success.
  2. *email*: the first `mailto:` address (one address only; cc, bcc and extra
     recipients are ignored), with its subject and body percent-decoded, control
     characters removed and capped (200 / 2,000 characters; "Unsubscribe" and a
     one-line request when missing). Sent through `mail::send` from the address the
     mail arrived at, so it's filed in Sent like any other.
  3. *website*: an http(s) page (https first), opened in the user's browser by the
     app; Mimi only remembers that it was opened.
- **Remembered per list** in `mail_unsubscribed`, by account: `id:` and the List-Id,
  else `from:` and the sender's address. Later mail from that list shows
  "Unsubscribed".
- **Archiving afterwards**: "Archive this conversation", or every Inbox conversation
  from the list (`unsubscribe::archive_list`, at most 500, one IMAP session).
- **Never automatic.** No tool, no sorting rule and no routine unsubscribes; viewing
  the offer changes nothing. `api/tests/unsubscribe_flow.rs` has a newsletter telling
  the assistant to unsubscribe and a model that tries: the assistant has no such tool,
  nothing is POSTed or sent until the user's `POST`.
- Tests reach a list server on this computer through
  `state.mail.unsubscribe.allow_local_for_tests()` (http and loopback, tests only);
  nothing else turns that on.

In the panel, `features/mail/unsubscribe.tsx` adds "Unsubscribe" to the reader's
toolbar. Its sheet says what will happen in plain words ("Mimi will ask example.com to
stop sending you these emails."); website lists say it opens in the browser. Afterwards
the button reads "Unsubscribed" and the sheet offers to archive this conversation or
all of them from that sender.

## New mail notifications

`mail/notify.rs`, migration 0031, `Settings.mail_notifications` (Settings › Reminders &
notifications › New email). New mail in an Inbox shows as a notification on this
computer, from the daemon, so it appears with the app closed.

**Which mail** (`MailNotifications.notify`, `NewMailNotify`):

- **Off**.
- **Important** (the default): conversations the sorter files as needs a reply or
  important. With nothing to sort (sorting off, no model, Jev chosen without a key) it
  can't tell, so it means every new email except newsletters and automatic mail; the
  setting says so.
- **All**: every new email in the Inbox, newsletters and automatic mail included.

**Never announced:**

- anything found by a mailbox's first pass (a new account, or a renumbered mailbox): the
  sync only queues messages when the mailbox already had a high-water mark;
- mail sent or received more than an hour ago (`RECENT_MS`: old mail that merely
  arrived late, or mail moved back into the Inbox, which keeps its received date);
- the user's own mail (`outgoing`), mail already read elsewhere, and mail read or gone
  before its notification is due;
- a Message-ID already considered (`mail_notify_seen`, kept a week): a copy back with a
  new UID, or the same message in a second account;
- with Important: automatic mail, and suspicious mail while sorting is on (it's never
  sorted, so never important).

**How:** the sync queues each new Inbox message in the transaction that stores it
(`notify::queue` into `mail_notify_queue`). The notifier (`notify::run`, started by
`mail::install`) listens to `mail_changed` and `settings_changed` on the event bus (the
sync and the sorter already publish them), so it hears each pass and each conversation
the sorter finishes without hooks of its own. Each look (`notify::tick`, with the clock
passed in for tests) decides every queued message (`verdict`) and removes it from the
queue before anything shows, so a crash or restart can't announce it twice.

**Waiting for the sorter:** with Important and something sorting, a message waits until
its conversation is sorted (`sorted_at >= last_at`). If the sorter is slow (a local
model on a small computer, the user chatting meanwhile) or failing, it's announced
anyway after `SORT_WAIT_MS` (3 minutes), unless it's automatic: a late notification
beats a missed email.

**Together:** what's ready is held while other new mail still waits for the sorter, so
they arrive as one notification ("3 new emails", up to three "Sender: Subject" lines
and "and 2 more"), but never past `SORT_WAIT_MS` after it was queued. One email says
"New email from Sam Carter" with its subject.

**Privacy:** notifications can show on a lock screen. `MailNotifications.show_details`
(on by default, "Show who it's from and the subject") turns them into a plain "New
email" / "3 new emails". Suspicious mail never shows its subject, even with details on.
On Linux, servers that read markup get the body escaped (`<b>` in a subject stays text).

**A click** (Linux: the notification's default action over D-Bus, heard for an hour by
`schedule::notify::show_clickable`) publishes `open_mail` (`Event::OpenMail
{ thread_id }`, `None` for several conversations). The desktop app's relay brings its
window forward and the page opens Mail on that conversation (`showMailThread`);
browsers ignore the event. Nothing happens if the app isn't running. On macOS,
notifications from the background daemon don't report clicks, so it's a plain one.

Mail notifications are separate from the reminders' "Notifications on this computer"
switch; `MIMI_NO_NOTIFICATIONS=1` and tests silence both.

## Undo send and send later

`mail/outbox.rs`, migration 0032 (`mail_outbox`), `api/mail_outbox.rs`. The panel's and
the draft cards' Send buttons call `POST /v1/mail/outbox` (a `NewOutgoingMail`: the
`MailDraft` and an optional `send_at`).

- **Undo send.** Without `send_at`, the message waits `Settings.undo_send_secs` (Off, 5,
  10, 20 or 30 seconds; default 10; Settings › General › Email) in the outbox
  (`kind: undo`), then goes. With it off, it's sent at once as before. The app shows
  "Sending…" with Undo; Undo (`DELETE /v1/mail/outbox/{id}`) returns the draft, which
  goes back into the editor it came from (`useDraftHome`: the reply box of that
  conversation, the draft card) or, if that's gone, into a new compose.
- **Send later.** The arrow beside Send offers Tomorrow morning (8:00), Tomorrow afternoon
  (13:00), Monday morning (8:00) and any date and time; each says plainly that if the
  computer is off or asleep then, it goes as soon as Mimi runs again. `send_at` is an
  instant (ms): a later time-zone change doesn't move it. At most a year ahead; a time
  more than a minute past is refused.
- **Checked when queued.** Everything `mail::prepare` checks happens before it's stored:
  the account, the From address, the recipients (count, addresses), the body and
  attachment sizes. Mistakes show at once, not when it's due.
- **Files are fetched when queued.** A forward's originals and the files the assistant
  attached by reference (`source`) are fetched then (under the same rules as sending) and
  kept with the message (`files`), so what goes is what was checked, and a file moved or
  deleted meanwhile can't stop it. The draft itself is kept as the user wrote it (the
  whole `MailDraft` as JSON, so new fields come along), and that's what Undo or Cancel
  give back.
- **The account and address** are fixed when queued: a message never goes from another
  account, even if its own was disconnected meanwhile (it fails, saying so).
- **The loop** (`outbox::run`) works like the scheduler's: it sleeps until the earliest
  `send_at`, at most a minute at a time (a monotonic sleep doesn't advance while the
  computer sleeps), and is poked when something is queued or rescheduled.
  `tick(state, now)` takes the clock for tests. Each due message is claimed (`status`
  `sending`) and sent in its own task, so a slow server never holds up the others; what
  came due while the computer was off goes once when Mimi runs again, with `sent_at`
  telling the app it was late. Sent messages leave the outbox and are filed in Sent like
  any other.
- **Problems.** A server that couldn't be reached is tried again after 1, 5, 15 and 60
  minutes (the message says so meanwhile), then marked not sent. Anything else (password
  refused, account disconnected, a recipient refused) marks it not sent at once, with
  the reason, a desktop notification (when those are on) and a toast with "Open". It
  waits there until the user sends it now, picks another time, edits or cancels it. A
  message left `sending` by a daemon that stopped is marked not sent ("check your Sent
  folder"): it may have gone, and sending twice is worse.
- **The Scheduled view** in the Mail sidebar (`features/mail/send-later.tsx`) lists
  scheduled and unsent messages (not the few seconds of Undo), problems first. Each
  opens read-only with Send now (`POST /v1/mail/outbox/{id}/send`), Change time (`PATCH`
  with `RescheduleMail`), Edit (taken out of the outbox into a compose) and Cancel sending,
  which offers to keep it as a draft (a compose) or delete it.
- **The assistant.** `mail_send` takes an optional `send_at` (local `YYYY-MM-DDTHH:MM`,
  or an instant with an offset). It still goes through approval; `prepare` refuses a
  time that has passed (before any card) and writes the local time the card shows under
  "When" (and the summary line, all a Telegram approval shows). Approved, it waits in
  the outbox like the user's own (`by_assistant`), where the user can still cancel it;
  approved after that time, it goes at once. Undo send applies to the user's own Send
  only: the assistant's approved sends without a time go at once, the approval card
  having been the chance to stop them.
- Events: `mail_outbox` (`OutgoingMail`, attachments without their content) whenever a
  message is queued, rescheduled, sent, cancelled or fails.

## Invitations

Event invitations, updates and cancellations are sent through this mail's SMTP path and
filed in Sent, only when the user says so: see
[Calendar › Guests and invitations](calendar.md#guests-and-invitations).

## People

`contacts::Correspondents` (`mail::contacts`) is a contact source: per account, everyone
the user wrote to, plus human senders (not automatic ones) with at least two messages,
as cards with one email handle (record id = the address). Email handles only, so they
join someone in the directory only through the same address (see [People](people.md)).
They never count as known recipients for `mail::known`. `GET
/v1/mail/threads?person=<id>` lists recent conversations with a person.

## API

All under `/v1`. Changes publish `MailChanged` (`mail_changed`); a click on a new-mail
notification publishes `open_mail`.

- `GET /v1/mail`: accounts, counts, whether sorting is on, the model's locality.
  `?account=` / `?address=` narrow the counts.
- `GET /v1/mail/presets`.
- `GET /v1/mail/threads?view=&q=&person=&folder=&account=&address=&before=&limit=`
  (`view=flagged`: flagged conversations, wherever they are).
- `GET /v1/mail/threads/{id}`, `DELETE /v1/mail/threads/{id}` (to Trash).
- `POST /v1/mail/threads/{id}/read` (`{read}`), `/archive`, `/flag` (`{flagged}`, see
  [Flags](#flags)), `/summarize`, `/draft` (`{instructions}`).
- `POST /v1/mail/send` (a `MailDraft`): sends at once. The user's own action, and so the
  approval.
- `GET|POST /v1/mail/outbox` (list; queue a `NewOutgoingMail`: the panel's and draft
  cards' Send and Send later), `PATCH|DELETE /v1/mail/outbox/{id}` (reschedule; take back,
  returning the draft), `POST /v1/mail/outbox/{id}/send` (now). See
  [Undo send and send later](#undo-send-and-send-later).
- `POST /v1/mail/refresh`.
- `POST /v1/mail/older` (`MailOlderSearch { q, account }` → `MailOlderResults { threads,
  before, more, problems }`): searches the servers for older mail and brings in the
  newest matches.
- `GET /v1/mail/messages/{id}/content`, `POST /v1/mail/messages/{id}/images`,
  `GET /v1/mail/messages/{id}/attachments/{index}`.
- `DELETE /v1/mail/jev`.
- `GET /v1/mail/threads/{id}/unsubscribe` (a `MailUnsubscribe`, or `null`), `POST` the
  same to unsubscribe, `POST /v1/mail/threads/{id}/unsubscribe/archive` (every Inbox
  conversation from that list). See [Unsubscribing](#unsubscribing).

## UI

`features/mail/`:

- `mail-view.tsx`: the Mail panel, with props `{ onSection, onAsk, onOpenPerson }`; the
  email feature owns that file. Three columns: views ("Sorted for you": Needs a reply,
  Important, Everything else; smart folders; mailboxes: Inbox, Sent, Archive; "Received
  on" addresses; with the sorting model's locality), the conversation list with
  one-line summaries and search across all mail, and the reader (Reply, Summarize,
  Archive, Flag, Mark as unread, Ask the assistant; a reply box that can draft the answer
  with the model; Send only by the user). Mailboxes also lists Flagged.
- `flag.tsx`: flagging (`useFlagThread`, the row's star); see [Flags](#flags).
- `message-body.tsx`: how a message is shown (Text · Formatted · Original).
- `older-mail.tsx`: "Search older mail" under search results, and what the servers found.
- `draft-editor.tsx`: the draft editor; `draft-attachments.tsx`, its files;
  `body-editor.tsx` and `mail-text.ts`, its formatted text. To, Cc and Bcc are chips
  that suggest people from People by name, address or `@name` (see
  [People](people.md#addresses-in-to-cc-and-guests)).
- `draft-card.tsx`: drafts the assistant writes in chat, as editable cards with their
  own Send button, showing the Bcc and files it added (removable). `mail_send` approval
  cards show every field: Bcc as a hidden copy, each file's name, size and origin.
- `folder-looks.tsx`: the smart folders' icons and colours.
- `unsubscribe.tsx`: the reader's Unsubscribe button and its sheet (see
  [Unsubscribing](#unsubscribing)).
- `mail-notifications.tsx`: the New email group of Settings › Reminders & notifications
  (which mail, and whether to show who and what).
- `send-later.tsx`: the Send later menu and time picker, the "Sending…" toast with Undo,
  the Scheduled view, the Undo send setting (see
  [Undo send and send later](#undo-send-and-send-later)).

Mail panel reply drafts get the user's custom instructions only, not the personality
(see [Personality](personality.md)).

## Tests

- `mail/fake.rs` is a small IMAP (IDLE, MOVE, APPEND…) and SMTP server. `mail/tests.rs`
  covers sync, UIDVALIDITY resets, flags (and flagging from Mimi), archive, send, hidden text, correspondents and
  sorting with a mock model; also `dropped_idle_connections_are_not_errors`,
  `mail_about_assistants_is_not` and
  `mail_that_only_claims_to_be_from_the_user_is_still_checked`.
- `mail/older_tests.rs`: older mail found only in Inbox, Sent and Archive, the search
  criteria, the cap, the UTF-8 fallback, kept mail left out of views and sorting, the
  next passes fetching nothing again (and sweeping it by its own UIDs), expiry, and
  `mail_search` reaching it. The fake server evaluates SEARCH keys (`BEFORE`, `TEXT`,
  `FROM`, `OR`…), takes `LITERAL+`, can refuse CHARSET, and records UID commands.
- `api/tests.rs › mail_flow` has a hostile email and a model that obeys it: the send
  waits on the approval card, and nothing is sent when it's declined. `mail_flow ›
  people_exceptions_and_dont_ask_again` covers permissions.
- `api/tests/mail_files.rs`: with sending automatic, a hostile email's Bcc to a stranger
  and a file to a known person both wait, a path on the computer is refused, a photo
  from another chat can't be attached, and a draft card's photo is sent (but not once
  its chat is a trusted person's).
- To try the app by hand:
  `MIMI_FAKE_MAIL_SEED=1 cargo test -p mimi-core fake_mail_server -- --ignored` serves
  seeded mail on 127.0.0.1:3143 (IMAP) / 3025 (SMTP) as `me@example.org` / `app-pass`
  (connect with "Other", security "None").
- `cargo test -p mimi-core live_discovery -- --ignored` checks server discovery on real
  domains.
- Jev: tests point `state.mail.jev_api` at a fake.
- Unsubscribing: `mail/unsubscribe.rs` tests parsing (several addresses, missing angle
  brackets, mailto subject and body), the refused addresses and redirects, one-click
  against a list server on this computer and the email through the fake SMTP;
  `api/tests/unsubscribe_flow.rs` proves only the user's click unsubscribes.
- `mail/notify_tests.rs`: which mail is announced (each choice, the sorter's wait and
  fallback), batching, first-sync silence and no duplicates, against the fake server.
  Tests look at what `notify::tick` would show; nothing is ever shown.
- Undo send and send later: `mail/outbox_tests.rs` (the undo window, sending at a time,
  catching up after downtime, a send cut off by a crash, reschedule / send now / cancel,
  mistakes refused when queued, a refused password and an unreachable server, a
  disconnected account, files kept with a scheduled forward) and
  `api/tests/mail_outbox.rs` (the API, and the assistant's scheduled send waiting for
  approval).
- Never point tests at a real mailbox.
