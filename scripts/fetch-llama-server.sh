#!/bin/sh
# Downloads the pinned llama.cpp release for a target and unpacks just what llama-server
# needs into apps/desktop/src-tauri/resources/llama/, which the desktop bundle ships as
# Mimi's built-in model runtime. Binaries are never committed; this runs before bundling.
#
#   scripts/fetch-llama-server.sh [target-triple]    (default: this machine)
#
# Linux uses the Vulkan build: it runs on AMD, Intel and NVIDIA GPUs and falls back to
# the CPU when there's no Vulkan driver (ggml loads its GPU backend as a plugin).
# macOS builds use Metal. To move to a newer llama.cpp, change RELEASE and the checksums
# (GitHub shows each asset's sha256 on the release page).
set -eu

RELEASE=b11211
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/apps/desktop/src-tauri/resources/llama"
CACHE="$ROOT/target/llama-cache"
TARGET=${1:-$(rustc -vV | sed -n 's/^host: //p')}

case "$TARGET" in
  x86_64-unknown-linux-gnu)
    ASSET=llama-$RELEASE-bin-ubuntu-vulkan-x64.tar.gz
    SHA256=39d0c77061e045b7138f441f399b6ecb47b7b019c1cf94d5eed6e7ef0973032d ;;
  aarch64-unknown-linux-gnu)
    ASSET=llama-$RELEASE-bin-ubuntu-vulkan-arm64.tar.gz
    SHA256=0e80659558d8a2ec3cc5ee1b912fbd4e8a1be6bdfe28b7c1db54d448aba4b937 ;;
  aarch64-apple-darwin)
    ASSET=llama-$RELEASE-bin-macos-arm64.tar.gz
    SHA256=2262cbe82440dfaa0b4ddeabded277a9fa0a91ef44ac56e3ab5ff6d820f5d4bc ;;
  x86_64-apple-darwin)
    ASSET=llama-$RELEASE-bin-macos-x64.tar.gz
    SHA256=e3525142255447f9bb0658b1a400ff2d58270817a6f9c07e877ce019389d80f3 ;;
  *)
    echo "No llama.cpp build for $TARGET" >&2
    exit 1 ;;
esac

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

mkdir -p "$CACHE"
ARCHIVE="$CACHE/$ASSET"
if [ ! -f "$ARCHIVE" ] || [ "$(sha256 "$ARCHIVE")" != "$SHA256" ]; then
  echo "Downloading $ASSET"
  curl -fL --retry 3 -o "$ARCHIVE.part" \
    "https://github.com/ggml-org/llama.cpp/releases/download/$RELEASE/$ASSET"
  mv "$ARCHIVE.part" "$ARCHIVE"
fi
ACTUAL=$(sha256 "$ARCHIVE")
if [ "$ACTUAL" != "$SHA256" ]; then
  echo "Checksum mismatch for $ASSET: expected $SHA256, got $ACTUAL" >&2
  rm -f "$ARCHIVE"
  exit 1
fi

WORK="$CACHE/$RELEASE-$TARGET"
rm -rf "$WORK" "$OUT"
mkdir -p "$WORK" "$OUT"
tar -xzf "$ARCHIVE" -C "$WORK"
SRC="$WORK/llama-$RELEASE"

# Keep llama-server and the libraries it loads, under the names it loads them by
# (lib*.so.0 / lib*.0.dylib, copied as real files), plus ggml's backend plugins. The
# other tools, their libraries and the RPC backend stay out.
for f in "$SRC"/*; do
  name=$(basename "$f")
  keep=no
  case "$name" in
    llama-server | LICENSE) keep=yes ;;
    *rpc* | libllama-*-impl.* | lib*.[0-9].[0-9]*.dylib) ;;
    lib*.so.[0-9] | lib*.[0-9].dylib) keep=yes ;;
    lib*.so | lib*.dylib) [ -L "$f" ] || keep=yes ;;
  esac
  case "$name" in libllama-server-impl.*) keep=yes ;; esac
  if [ "$keep" = yes ]; then cp -L "$f" "$OUT/$name"; fi
done
rm -rf "$WORK"

echo "llama-server $RELEASE for $TARGET in ${OUT#"$ROOT"/} ($(du -sh "$OUT" | cut -f1))"
