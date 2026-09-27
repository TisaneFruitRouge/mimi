# Mimi

> Working name.

A private personal AI assistant that you host yourself. It runs on your own computer,
uses local models by default, and sends data to outside services only when a task
requires it, such as sending a Telegram message or creating a calendar event.

**Status:** early. What works today:

- Chat with streaming replies, saved and encrypted on disk (SQLCipher, key in the OS
  keychain).
- Models run inside Mimi (llama.cpp, with Metal on macOS and Vulkan on Linux): pick one
  that fits your computer and it downloads, verified, from Hugging Face. Ollama, LM
  Studio, a server on your network or a cloud service work too. Every model is labeled
  *on this device*, *on your network* or *cloud*.
- A first-run welcome that looks at your hardware, recommends a model, and optionally
  connects your calendar and Telegram.
- A desktop app and a `mimi` CLI with the same capabilities.

## Goals

- **Private and secure by default.** Local models first. Cloud models are optional and
  clearly labeled. Every integration asks for exactly the access it needs.
- **For everyone, not just developers.** A polished desktop app that can configure
  everything, with no config files required. Installs like any other app.
- **Runs on anything.** Model recommendations adapt to your hardware, from an old laptop
  (where a cloud model is the realistic option) to a workstation with a large GPU.
- **macOS and Linux.**

## Install

On Linux (x86_64) or macOS, in a terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/TisaneFruitRouge/mimi/main/scripts/install.sh | sh
```

It downloads the right installer from
[GitHub Releases](https://github.com/TisaneFruitRouge/mimi/releases) and checks it: the
.deb or .rpm on Debian/Ubuntu and Fedora/openSUSE (asks for your password once), the
AppImage elsewhere (`… | sh -s -- --appimage` to prefer it), `Mimi.app` on macOS. Or
download an installer from the releases page yourself. Nothing else to set up: Mimi
brings its own model runtime. Mimi never checks for updates by itself; run the command
again to update.

macOS builds aren't notarized yet. The install command takes care of that; if you open
the .dmg yourself, right-click Mimi and choose Open the first time.

## Development

Requirements: Rust (stable), Node 22+, pnpm. On Linux, also the
[Tauri system dependencies](https://v2.tauri.app/start/prerequisites/#linux).

```sh
pnpm install
pnpm dev              # daemon + desktop app; closing the app or Ctrl+C stops both
pnpm mimi status    # CLI, talking to the dev daemon
pnpm check            # fmt, clippy, tests, typecheck (same as CI)
```

`pnpm dev` keeps its data in `.dev/` at the repo root, separate from a real install.

### Web interface

The daemon also serves the app to your browser at `http://127.0.0.1:7437` (loopback
only; `MIMI_PORT` changes the port). With the daemon running:

```sh
pnpm web              # build the frontend, then open it in your browser, signed in
pnpm mimi open      # just open it (after a build)
```

Browsers sign in with a single-use link from the desktop app or `mimi open`, which
sets a session cookie.
To run the pieces on their own: `pnpm dev:daemon`, `pnpm dev:app`. Set
`MIMI_HOME=/some/dir` for any other isolated instance.

### Built-in model runtime and installers

```sh
scripts/fetch-llama-server.sh   # llama-server for this machine, into apps/desktop/src-tauri/resources/llama/
MIMI_LLAMA_SERVER=$PWD/apps/desktop/src-tauri/resources/llama/llama-server pnpm dev
pnpm bundle                     # this platform's installers, in target/release/bundle/
```

Without `llama-server` the built-in source simply doesn't appear, and Mimi suggests
Ollama instead. Releases are built by `.github/workflows/release.yml` when a version tag
is pushed.

See [docs/architecture.md](docs/architecture.md) for how the pieces fit together.

## License

[AGPL-3.0-or-later](LICENSE).
