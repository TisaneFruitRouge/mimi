#!/bin/sh
# Gets everything the desktop installers ship besides the app itself: the daemon
# (`mimid`, a Tauri sidecar) and llama.cpp's `llama-server` (the built-in model runtime).
#
#   scripts/prepare-bundle.sh [target-triple]    (default: this machine)
#   pnpm --filter @mimi/desktop tauri build --config src-tauri/tauri.bundle.conf.json
#
# Or both at once: `pnpm bundle`. Plain `tauri build`/`tauri dev` without the extra
# config still work without any of this (development and CI).
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${1:-$(rustc -vV | sed -n 's/^host: //p')}
cd "$ROOT"

# mimid embeds the web UI in release builds, so the frontend comes first.
pnpm build
cargo build --release --locked -p mimi-core --bin mimid --target "$TARGET"
mkdir -p apps/desktop/src-tauri/binaries
cp "target/$TARGET/release/mimid" "apps/desktop/src-tauri/binaries/mimid-$TARGET"

scripts/fetch-llama-server.sh "$TARGET"
