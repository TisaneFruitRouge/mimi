# Mimi

> Working name.

A private personal AI assistant that you host yourself. It runs on your own computer,
uses local models by default, and sends data to outside services only when a task
requires it, such as sending a Telegram message or creating a calendar event.

**Status:** early. What works today:

- Chat with streaming replies, saved and encrypted on disk (SQLCipher, key in the OS
  keychain).
- Any OpenAI-compatible model provider: Ollama, LM Studio and llama.cpp on your
  computer, a server on your network, or a cloud service. Every model is labeled
  *on this device*, *on your network* or *cloud*.
- First-run setup that looks at your hardware, finds local model servers, and
  recommends models that fit.
- A desktop app and a `mimi` CLI with the same capabilities.

## Goals

- **Private and secure by default.** Local models first. Cloud models are optional and
  clearly labeled. Every integration asks for exactly the access it needs.
- **For everyone, not just developers.** A polished desktop app that can configure
  everything, with no config files required. Installs like any other app.
- **Runs on anything.** Model recommendations adapt to your hardware, from an old laptop
  (where a cloud model is the realistic option) to a workstation with a large GPU.
- **macOS and Linux.**

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

See [docs/architecture.md](docs/architecture.md) for how the pieces fit together.

## License

[AGPL-3.0-or-later](LICENSE).
