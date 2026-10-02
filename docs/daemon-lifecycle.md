# Daemon lifecycle

How `mimid` gets started, kept running in the background and replaced after an update.
The desktop side lives in `apps/desktop/src-tauri/src/` (`daemon_process.rs`, `tray.rs`,
`relaunch.rs`, `main.rs`); finding the binary and the login service are `crates/service`
(`mimi-service`), shared by the app and the CLI. How clients find and authenticate to a
running daemon (`daemon.json`, the token) is in [Architecture](architecture.md).

## Rules

- **The user never starts `mimid`.** The app uses a running daemon, else starts the
  installed login service, else runs `mimid` as a supervised child and stops it on quit.
- **Finding `mimid` is `mimi_service::daemon_binary()` only.** Don't add other lookups
  elsewhere.
- **One daemon per data directory.** Service names derive from it, so dev and test
  installs can never replace a real one. Turning the background switch on first stops the
  app's child, then installs and starts the service.
- `pnpm dev` sets `MIMI_DAEMON=external` for the app: it only connects, never starts,
  stops or installs a daemon, and the background switch explains why it's unavailable.
- **Testing without disturbing the user's session:** never install, start or stop the
  plain `mimi` unit, and never touch the `pnpm dev` daemon. Use a scratch `MIMI_HOME`,
  `MIMI_KEY_STORE=file` and a spare `MIMI_PORT` (hashed unit name), and remove what you
  created (see [Development](development.md)).
- After an update the app must not keep talking to an old daemon: it compares versions
  and build ids at launch and restarts the service when they differ (not under
  `pnpm dev`).

## Who runs the daemon

| Owner | When | Stops when |
|---|---|---|
| The background service | "Keep Mimi running in the background" is on | The switch is turned off (the app then runs it itself) |
| The desktop app, as a child | The switch is off | The app quits |
| Someone else | `pnpm dev` (`MIMI_DAEMON=external`), or a daemon started by hand | Its owner stops it |

On launch the app (`apps/desktop/src-tauri/src/daemon_process.rs`):

1. uses a daemon that already answers;
2. else starts the installed service;
3. else spawns `mimid` as a child (logging to `daemon.log` in the data directory),
   restarts it if it crashes (at most 5 times a minute) and stops it with SIGTERM on
   quit.

With `MIMI_DAEMON=external` it only connects.

"Keep Mimi running in the background" in Settings installs or removes the login service
and hands the daemon over. `mimi service install|uninstall|status|start` does the same
headless.

## Finding `mimid`

`mimi_service::daemon_binary()`:

1. `MIMI_DAEMON_BIN`;
2. else `mimid` next to the running executable: bundles ship it as the Tauri sidecar
   `binaries/mimid`, which lands beside the app binary (see [Packaging](packaging.md));
   in development it's `target/<profile>/`;
3. else `PATH`.

## The service

One per data directory (`crates/service`):

- **Linux, systemd:** user unit `~/.config/systemd/user/mimi.service`,
  `Restart=on-failure`, enabled for login with `systemctl --user enable --now`.
- **Linux without systemd:** XDG autostart entry `~/.config/autostart/mimi.desktop`
  (starts at login, nothing restarts it after a crash; the switch's text says so).
- **macOS:** LaunchAgent `~/Library/LaunchAgents/dev.mimi.daemon.plist`, `RunAtLoad`,
  `KeepAlive` on unsuccessful exit, logs to `daemon.log`.
- **Names:** `mimi` / `dev.mimi.daemon` for the default install. A non-default data
  directory (any custom `MIMI_HOME`: development, tests) gets its own name,
  `mimi-<hash>` / `dev.mimi.daemon.<hash>`, so it can never replace the real install.
- `MIMI_HOME`, `MIMI_PORT` and `MIMI_KEY_STORE` are carried into the service only when
  set.

### AppImage

An AppImage runs from a temporary mount that vanishes when it quits, so a service
installed from one runs the AppImage *file* (`$APPIMAGE`) with `--daemon` instead
(`Spec::launched_from_app`); `main.rs` hands `--daemon` over to the bundled `mimid`.
Replacing the AppImage at the same path keeps the service working.

## Tray and closing the window

The tray (`tray.rs`) offers Open Mimi, a status line and Quit Mimi. In background mode,
closing the window hides it to the tray; quitting leaves a service-run daemon running.
Linux trays need libayatana-appindicator at runtime; without it there's no tray and
closing the window quits the app (the service, if on, keeps the assistant running).

Release builds are single-instance: launching again brings the window forward, unless
the program was replaced since this one started (see below).

## After an update

The login service may still run the old daemon after an update.

- **Version check.** At launch the app compares the daemon's `/health` version and build
  id (`MIMI_BUILD_ID`, set by `pnpm bundle` and the release workflow; absent in
  development) with its own, and restarts the service when they differ (not under
  `pnpm dev`). The build id means a same-version rebuild restarts the service too.
- **Hidden app.** An app hidden in the tray when an update is installed would otherwise
  show the old version forever. `relaunch.rs` remembers the file it was started from
  (`$APPIMAGE`, else the executable). When the user opens Mimi again after that file was
  replaced, the running app starts a small shell that waits for it to exit and launches
  the new program (`open` on the `.app` on macOS), and quits.
- **Older daemons.** Requests an older daemon can't read (unknown route, unreadable body)
  surface as `DaemonError.code === "outdated"` with a plain message.

Checking for new versions (opt-in) is in [Packaging › New versions](packaging.md#new-versions).

## Tests

The desktop lifecycle has an opt-in test that runs real processes without a window:

```sh
MIMI_HOME=… MIMI_PORT=… MIMI_KEY_STORE=file MIMI_DAEMON_BIN=$PWD/target/debug/mimid \
  cargo test -p mimi-desktop lifecycle -- --ignored
```
