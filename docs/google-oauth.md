# Google sign-in: setting up the app identity

"Sign in with Google" (Settings › Connections › Google Calendar) lets the assistant
read the user's Google calendars and add, move and remove events. It needs an OAuth
client registered with Google: an identifier Google shows on its consent screen. It
routes nothing through the project. The daemon on the user's computer talks to Google
directly, and the tokens stay in its encrypted database (principle 0 in `AGENTS.md`).

Without a client, everything still works: the dialog explains that signing in isn't
available in this build and offers the calendar's private (iCal) address instead,
which is read-only.

## What the app asks for

- **Application type:** Desktop app. The daemon uses a loopback redirect
  (`http://127.0.0.1:<random port>/`), which Google allows for desktop clients on any
  port, with PKCE (S256) and a random `state`.
- **Scopes**, and nothing else:
  - `https://www.googleapis.com/auth/calendar.events`: view and edit events on all
    the user's calendars.
  - `https://www.googleapis.com/auth/calendar.calendarlist.readonly`: see the list of
    calendars they're subscribed to (names, colours, which ones they can write to).

  Both are "sensitive" scopes (not "restricted"): Google verifies the app before it
  can be used freely, but no security assessment is required.

## Creating the client (maintainer, once)

1. In the [Google Cloud console](https://console.cloud.google.com/), create a project
   (e.g. "Mimi").
2. **APIs & Services › Library:** enable the **Google Calendar API**.
3. **Google Auth Platform › Branding:** app name ("Mimi"), a support email, the logo,
   the home page and the privacy policy link. The privacy policy should say plainly
   that calendar data goes only between the user's computer and Google.
4. **Audience:** user type **External**. While the app is in **Testing**, only the
   test users listed here can sign in (up to 100), and Google ends their sign-in after
   7 days (they'll see "Sign in again" in Connections). Add yourself and your testers.
5. **Data access:** add the two scopes above.
6. **Clients › Create client:** type **Desktop app**. Keep the **client ID** and the
   **client secret**. Google says a desktop client's secret isn't confidential (it
   ships inside every copy of the app); PKCE is what protects the exchange. Store it as
   a CI secret all the same, so it isn't committed.

## Before verification

Until Google verifies the app, it can be put **In production** but:

- users see **"Google hasn't verified this app"** and must choose *Advanced › Go to
  Mimi (unsafe)* to continue;
- at most **100 users** can sign in in total (Google's cap for unverified apps with
  sensitive scopes).

Verification (Branding › Verification) asks for the home page and privacy policy on a
domain you've verified in Search Console, a short video of the sign-in and of how each
scope is used, and a sentence per scope. It takes days to weeks.

## Building with it

The daemon reads `MIMI_GOOGLE_CLIENT_ID` and `MIMI_GOOGLE_CLIENT_SECRET` at build time
(`option_env!`), and the same variables at run time override them.

```sh
# Installers with sign-in built in:
MIMI_GOOGLE_CLIENT_ID=1234-abc.apps.googleusercontent.com \
MIMI_GOOGLE_CLIENT_SECRET=GOCSPX-… pnpm bundle

# Development, without rebuilding:
MIMI_GOOGLE_CLIENT_ID=… MIMI_GOOGLE_CLIENT_SECRET=… pnpm dev
```

Release builds (`.github/workflows/release.yml`) take them from the repository secrets
`MIMI_GOOGLE_CLIENT_ID` and `MIMI_GOOGLE_CLIENT_SECRET`; without them the release
simply has no sign-in.

## How it behaves

- The client asks the daemon to start (`POST /v1/google/sign-in`), opens the consent
  page in the system browser, and polls until it's done. The daemon listens on
  127.0.0.1 only for that sign-in (10 minutes at most) and turns away anything without
  the right `state`.
- The refresh token is stored in the connection's config (encrypted database); access
  tokens stay in memory. None of them reaches a client or a log.
- If Google stops accepting the sign-in (the user removed access at
  myaccount.google.com › Security › Third-party access, or a test user's 7 days ran
  out), the connection says "Sign in again"; signing in again keeps the same
  connection, so reminders and permission exceptions on its calendars keep working.
- Disconnecting in Mimi also asks Google to revoke the access.
- Tests never talk to Google: `crates/core/src/connections/calendar/google_fake.rs`
  plays Google, and `api/tests.rs › google_flow` covers the whole flow.
