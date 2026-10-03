# Mimi for iPhone

A native SwiftUI client for the Mimi on your computer. It reaches the daemon peer to
peer with [iroh](https://iroh.computer) (see `docs/architecture.md` › Using Mimi from a
phone): pair it once by scanning the code in the desktop app's Settings › Phone.

## Build and run

Needs Xcode 26 and iOS 26.

1. Copy `Config/Local.example.xcconfig` to `Config/Local.xcconfig` and set your team and a
   bundle id of your own. (A free Apple account works, but what it installs stops opening
   after 7 days; reinstall from Xcode.)
2. Open `Mimi.xcodeproj`, pick your iPhone, Run. The iroh package (a prebuilt
   xcframework) is fetched by Xcode on first build.
3. On your computer: Mimi › Settings › Phone › Pair a phone, and scan the code with the
   app (or with the Camera app, which opens Mimi).

The project is generated from `project.yml` with [XcodeGen](https://github.com/yonaskolb/XcodeGen)
(`brew install xcodegen`, then `xcodegen generate` here). Regenerate after adding files,
targets or settings, and commit both.

## Tests

- Unit tests (`MimiTests`): `xcodebuild -scheme Mimi -destination 'platform=iOS Simulator,name=iPhone 17 Pro' test`.
- Walkthrough (`MimiUITests`, scheme `Walkthrough`): drives the whole app against a running
  daemon and attaches screenshots. Run a scratch daemon (see the repository's CLAUDE.md:
  scratch `MIMI_HOME`, `MIMI_KEY_STORE=file`, a spare `MIMI_PORT`; `MIMI_DEV_TOOLS=1` for
  the approval card), make a link with `POST /v1/remote/pairing`, then
  `TEST_RUNNER_MIMI_PAIRING_LINK='mimi://pair?…' xcodebuild -scheme Walkthrough … test`.
  It needs a model that streams a reply and asks for `dev_send_note` when told to send
  a note.

## Layout

- `Mimi/Connection/`: `DaemonLink` (iroh, the `mimi/1` wire format), `MimiAPI` (typed
  `/v1` calls), the Keychain (this phone's key and token, never synced).
- `Mimi/Models/`: Codable mirrors of `crates/protocol` types. Explicit `CodingKeys`, no
  key strategy (it would rewrite keys inside free-form JSON). Decode only what the app
  reads; unknown fields and events are ignored, so an older app keeps working.
- `Mimi/Store/AppModel.swift`: the connection lifecycle and the event feed, applied to
  cached data like the desktop's `lib/events.ts`. Screens that show something else
  refetch on `model.revision("mail_changed")` and the like.
- `Mimi/Design/`: the desktop's colour tokens, `LocalityBadge`, `AssistantAvatar` (the
  same mochi as the desktop and the app icon), Markdown for replies.
- `Mimi/Features/<area>/`: screens.
- `Icon/`: the app icon (`icon.svg`, `regenerate.sh`), the desktop icon's character on a
  full-bleed tile.
