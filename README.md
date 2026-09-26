# Hearth

> Working name.

A private personal AI assistant that you host yourself. It runs on your own computer,
uses local models by default, and sends data to outside services only when a task
requires it, such as sending a Telegram message or creating a calendar event.

**Status:** early scaffolding. Nothing useful yet.

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
cargo run -p hearth-core --bin hearthd   # start the daemon
pnpm dev                                  # desktop app, in another terminal
cargo run -p hearth-cli -- status         # CLI
```

To run an isolated instance, set `HEARTH_HOME=/some/dir`.

Checks (same as CI):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck
```

See [docs/architecture.md](docs/architecture.md) for how the pieces fit together.

## License

[AGPL-3.0-or-later](LICENSE).
