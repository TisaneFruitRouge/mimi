# Connections

Connections are the user's own accounts that Mimi talks to: calendars, email, and
messaging apps. The shared part lives in `crates/core/src/connections/` (one row per
connected account) and `crates/core/src/integrations.rs` (the catalog of what can be
connected). Each kind of account has its own doc, listed below.

## Rules

- One row per connected account in the `connections` table. Its config (secrets
  included) is JSON in the encrypted database. The config never leaves the daemon:
  clients only see `Connection` (name, status, one-line detail, an optional `action_url`
  for "finish setup"). `ConnectionsChanged` events keep them live.
- Every connection is checked against the real service before it's saved (fetch the
  feed, discover CalDAV calendars, Telegram `getMe`, sign in over IMAP and SMTP), so a
  saved connection works.
- Never log feed URLs, tokens or passwords. reqwest errors include URLs: map them before
  logging or returning them.
- Integrations come from the daemon's catalog (`GET /v1/integrations`). List a new one
  there with status `coming_soon` until its connection flow exists, then add its id to
  `AVAILABLE` in `crates/core/src/integrations.rs`.
- Data goes only between the user's machine and the service itself; nothing is routed
  through the project. Registered app identities (Mimi's Google OAuth client ID) are
  just identifiers shown on consent screens.
- Tests fake the outside world. Never point tests at Google, at matrix.org, or at a real
  mailbox (see [Tests](#tests)).

## Kinds of connection

| Kind | Doc | In short |
|---|---|---|
| Calendars | [Calendar](calendar.md) | Sign in with Google (OAuth, the one registered app identity; setup in [Google OAuth](google-oauth.md)), Google's private iCal address (read-only), CalDAV with app-specific passwords (iCloud, Fastmail, Nextcloud, Radicale). |
| Email | [Email](email.md) | IMAP to read, SMTP to send, with app passwords. Servers found from the address. |
| Telegram | [Messaging apps](messaging.md) | A bot the user creates with @BotFather, long-polled, paired with its owner by a one-time `/start <code>`. |
| Signal | [Signal](signal.md) | Mimi linked to the user's own account as a device; the conversation is Note to Self. |
| Matrix | [Matrix](matrix.md) | An account the user makes for the assistant on any server; can also message other people and groups ([Matrix › Messages to other people](matrix.md#messages-to-other-people)). |

Connected CalDAV accounts also bring their address books (CardDAV) into People: see
[People](people.md). The connect forms are the Connections dialogs
(`apps/desktop/src/features/connections/connect-dialogs.tsx`), used both in Settings ›
Connections and in the [onboarding](onboarding.md).

## Tests

Tests fake every outside service:

- **CalDAV**: Radicale, run locally; see [Calendar › Tests](calendar.md#tests) for the
  command and `live_caldav`.
- **Google** (sign-in, tokens, Calendar API): `calendar/google_fake.rs`; see
  [Calendar › Tests](calendar.md#tests).
- **Telegram**: `fake_telegram` in `api/tests.rs` stands in for the Bot API; see
  [Messaging apps](messaging.md).
- **iCal parsing**: public Google holiday feeds.
- **Email**: `mail/fake.rs`, a small IMAP and SMTP server; see
  [Email › Tests](email.md#tests).
- **Matrix**: a throwaway Synapse with open registration, on a spare port; the full
  recipe and `live_matrix` are in [Matrix › Tests](matrix.md#tests).
- **Signal**: no fake server; linking needs a phone and a scratch daemon; see
  [Signal](signal.md).
