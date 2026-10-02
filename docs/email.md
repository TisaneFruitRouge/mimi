# Email

Mail is read over IMAP and sent over SMTP with the account's app password, in
`crates/core/src/mail/`. There is no registered app, no Microsoft or Google sign-in, and
nothing between the user's computer and their mail service. The Mail panel is
`apps/desktop/src/features/mail/`. Crates: async-imap, mail-parser, html2text, lettre
(all MIT/Apache), ammonia (its cssparser is MPL-2.0) and markup5ever_rcdom (MIT/Apache)
for showing mail. Connections in general: [Connections](connections.md).

## Rules

- **Email is untrusted.** Tools label mail as data. The only tool that acts,
  `mail_send`, asks for approval unless Settings › Permissions lets it go and every
  recipient is known (`mail::known`); its card shows To, Cc, Subject and the whole
  message. `api/tests.rs › mail_flow` proves a hostile email can't send mail without
  approval; keep it passing.
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
- Bcc and attached files come only from the user: the assistant's tools (`draft_from`)
  have neither, so a model can't add a hidden recipient or send a file. Bcc counts
  towards `MAX_RECIPIENTS`. The message sent never carries the Bcc line; only the copy
  filed in Sent does (`smtp::build`).
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
- **Unsubscribing is only ever the user's click** (`POST
  /v1/mail/threads/{id}/unsubscribe`). There is no assistant tool for it; keep
  `api/tests/unsubscribe_flow.rs` passing. A one-click address is fetched over https
  only, from the internet only (checked after DNS, on the address connected to, and on
  every redirect), with nothing of the user's in the request.

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
   - Mail older than 97 days is forgotten; the high-water mark becomes
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

## Storage and threading

`store.rs`, migration 0013:

- `mail_threads`: subject, last activity, category, summary, `sorted_at`.
- `mail_messages`: per mailbox and UID: headers, plain-text body, snippet, flags,
  attachment names; with `mail_fts` for search.
- `mail_sync`: UIDVALIDITY and high-water mark per mailbox.
- `mail_thread_ids` (migration 0016): thread ids are never reused, so a stale id can't
  reach another conversation.

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
  instructions" notice.
- `mail_draft_reply` and `mail_compose` make a draft, shown in the chat as an editable
  card; nothing is sent.
- `mail_send` asks for approval (the card shows To, Cc, Subject and the whole
  message) unless the user made sending automatic and every recipient is known. Its `prepare` splits recipients into one address each. It's governed by the
  `send_mail` permission (see [Tools and approvals](tools-and-approvals.md)).

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
  the user attached with the paperclip, pasted into the message (a pasted picture is
  attached, as "Pasted image.png" when it has no name) or dropped on the draft. Sent as
  they are; the name loses any folder part. Together with forwarded attachments they
  can add up to `smtp::MAX_ATTACHMENTS` (20 MB); `/mail/send` takes requests up to
  `smtp::MAX_REQUEST_BYTES`.
- Messages are plain text, so pictures go as attachments, not inline.

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

All under `/v1`. Changes publish `MailChanged` (`mail_changed`).

- `GET /v1/mail`: accounts, counts, whether sorting is on, the model's locality.
  `?account=` / `?address=` narrow the counts.
- `GET /v1/mail/presets`.
- `GET /v1/mail/threads?view=&q=&person=&folder=&account=&address=&before=&limit=`
  (`view=flagged`: flagged conversations, wherever they are).
- `GET /v1/mail/threads/{id}`, `DELETE /v1/mail/threads/{id}` (to Trash).
- `POST /v1/mail/threads/{id}/read` (`{read}`), `/archive`, `/flag` (`{flagged}`, see
  [Flags](#flags)), `/summarize`, `/draft` (`{instructions}`).
- `POST /v1/mail/send` (a `MailDraft`): the panel's or a draft card's Send button, which
  is the user's own action and so the approval.
- `POST /v1/mail/refresh`.
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
- `draft-editor.tsx`: the draft editor; `draft-attachments.tsx`, its files. To, Cc and
  Bcc are chips that suggest people from People by name, address or `@name` (see
  [People](people.md#addresses-in-to-cc-and-guests)).
- `draft-card.tsx`: drafts the assistant writes in chat, as editable cards with their
  own Send button. `mail_send` approval cards show every field.
- `folder-looks.tsx`: the smart folders' icons and colours.
- `unsubscribe.tsx`: the reader's Unsubscribe button and its sheet (see
  [Unsubscribing](#unsubscribing)).

Mail panel reply drafts get the user's custom instructions only, not the personality
(see [Personality](personality.md)).

## Tests

- `mail/fake.rs` is a small IMAP (IDLE, MOVE, APPEND…) and SMTP server. `mail/tests.rs`
  covers sync, UIDVALIDITY resets, flags (and flagging from Mimi), archive, send, hidden text, correspondents and
  sorting with a mock model; also `dropped_idle_connections_are_not_errors`,
  `mail_about_assistants_is_not` and
  `mail_that_only_claims_to_be_from_the_user_is_still_checked`.
- `api/tests.rs › mail_flow` has a hostile email and a model that obeys it: the send
  waits on the approval card, and nothing is sent when it's declined. `mail_flow ›
  people_exceptions_and_dont_ask_again` covers permissions.
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
- Never point tests at a real mailbox.
