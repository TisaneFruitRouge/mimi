# Packaging and releases

How Mimi is turned into native installers (.dmg, .deb, .rpm, AppImage), released on
GitHub, installed with one command, and how the opt-in check for new versions works.
Installers are Tauri bundles (`apps/desktop/src-tauri/`), built by `pnpm bundle` and
`.github/workflows/release.yml`; the installer script is `scripts/install.sh`; the
update check is `crates/core/src/updates.rs`.

## Rules

- **Easy install.** Native installers, at most one terminal command, no manual
  dependency setup for users: Mimi brings its own model runtime.
- **The extra pieces are only in `tauri.bundle.conf.json`.** The `mimid` sidecar and
  `llama-server` resources are configured only in
  `apps/desktop/src-tauri/tauri.bundle.conf.json`, so `tauri dev`, CI and plain builds
  need neither.
- **Never commit binaries.** `src-tauri/binaries/` and `src-tauri/resources/llama/` are
  gitignored.
- **llama.cpp is pinned.** `scripts/fetch-llama-server.sh` downloads a pinned llama.cpp
  release with SHA-256 checks; bump it by editing `RELEASE` and the checksums there.
- **A release tag matches the app's version.** Pushing a `v*` tag that matches
  `tauri.conf.json`'s version runs the release workflow into a *draft* GitHub Release;
  publish it by hand.
- **Update checks are opt-in only** (principle 0: there is no Mimi server). With
  "Check for new versions" on (off by default), the daemon asks GitHub's public releases
  API directly, once a day. It only tells the user; it never downloads or installs
  anything. Running the installer again updates.
- **`MIMI_BUILD_ID`** is set by `pnpm bundle` and the release workflow (absent in
  development), so the app can restart a service still running an older build (see
  [Daemon lifecycle › After an update](daemon-lifecycle.md#after-an-update)).

## What an installer contains

A Tauri bundle with two extra pieces:

- `mimid` as a sidecar (`externalBin: binaries/mimid`), built in release mode for the
  target triple by `scripts/prepare-bundle.sh` as `src-tauri/binaries/mimid-<triple>`.
  It lands beside the app's executable, where `mimi_service::daemon_binary` looks (see
  [Daemon lifecycle](daemon-lifecycle.md#finding-mimid)).
- `llama-server` and its libraries as resources (`resources/llama/` → `llama/`), fetched
  by `scripts/fetch-llama-server.sh` from the pinned llama.cpp release with SHA-256
  checks, and trimmed to what `llama-server` loads. The runtime finds them there (see
  [Models › Finding `llama-server`](models.md#finding-llama-server)).

| Platform | Installers | llama.cpp build |
|---|---|---|
| Linux x86_64 | .deb, .rpm, AppImage | Vulkan (CPU fallback) |
| macOS arm64 / x86_64 | .dmg (unsigned for now) | Metal |

Linux uses llama.cpp's Vulkan build: the GPU backend is a plugin, and without a Vulkan
driver it runs on the CPU. macOS uses Metal.

An AppImage runs from a temporary mount, so a background service installed from one runs
the AppImage file itself; see [Daemon lifecycle › AppImage](daemon-lifecycle.md#appimage).

## Building locally

`pnpm bundle` = `scripts/prepare-bundle.sh` (the frontend, release `mimid` for the target
as `src-tauri/binaries/mimid-<triple>`, `llama-server` into `src-tauri/resources/llama/`)
+ `tauri build --config src-tauri/tauri.bundle.conf.json`. The installers land in
`target/release/bundle/`.

- On Arch, set `NO_STRIP=true` for the AppImage step.
- Google sign-in is baked in only if `.env.google` or `MIMI_GOOGLE_CLIENT_*` is set; see
  [Google OAuth](google-oauth.md) and [Calendar](calendar.md).

To install the current code as the real app:

```sh
pnpm bundle
scripts/install.sh --file target/release/bundle/appimage/Mimi_*.AppImage
```

`pnpm dev` runs next to an installed Mimi without touching it (see
[Development](development.md)).

## Releases

Pushing a version tag (`v0.2.0`, matching `tauri.conf.json`'s version) runs
`.github/workflows/release.yml`, which builds all installers (deb/rpm/AppImage on Ubuntu
22.04, arm64 and x64 .dmg on macOS) and attaches them to a draft GitHub Release. Publish
it by hand.

## The install script

`scripts/install.sh` is the one-command installer:

```sh
curl -fsSL https://raw.githubusercontent.com/TisaneFruitRouge/mimi/main/scripts/install.sh | sh
```

It picks the right file for the machine from GitHub Releases, checks its SHA-256 and
installs it:

- **Linux** (x86_64 only): the .deb or .rpm on apt or dnf/zypper systems (asks for the
  password once), else the AppImage in `~/Applications` with a menu entry.
  `… | sh -s -- --appimage` skips the system package.
- **macOS**: `Mimi.app` in `/Applications` (or `~/Applications`). Builds are unsigned for
  now, so the script clears the quarantine flag; a .dmg opened by hand needs
  right-click › Open the first time.
- `MIMI_VERSION=v0.2.0` picks a release (default: the latest); `MIMI_REPO` another
  repository; `--file PATH` installs a package already on disk (e.g. a local
  `pnpm bundle` build) instead of downloading one, its kind taken from the file name.

Nothing is sent anywhere but GitHub. Running it again updates.

## New versions

Settings › General › "Check for new versions" (`Settings.update_check`, off by default).
When it's on, the daemon (`crates/core/src/updates.rs`) asks GitHub's releases API for the
latest release (`releases/latest` for the workspace's `repository`) once a day; the last
answer is kept in the `settings` table row `updates`, so restarts don't ask again.

- Drafts and pre-releases don't count.
- The release page's address is built from the version, not taken from the answer.
- The app shows a one-time notice per new version, and the status in Settings.
- Nothing is downloaded or installed for the user.
- API: `GET /v1/updates`, `POST /v1/updates/check`; changes publish `UpdateChanged`.

## Later

- Developer ID signing and notarization for macOS (until then the install script removes
  the quarantine flag).
- Linux arm64 packages.
