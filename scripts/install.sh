#!/bin/sh
# Installs Mimi from its GitHub Releases, on Linux or macOS:
#
#   curl -fsSL https://raw.githubusercontent.com/TisaneFruitRouge/mimi/main/scripts/install.sh | sh
#
# Linux: the .deb or .rpm when the system uses apt or dnf/zypper (asks for your password
# once), otherwise the AppImage in ~/Applications with a menu entry. Pass --appimage to
# skip the system package: `curl … | sh -s -- --appimage`.
# macOS: Mimi.app into /Applications (or ~/Applications).
#
# MIMI_VERSION=v0.2.0 picks a release (default: the latest). Nothing is sent anywhere
# but GitHub, and Mimi itself never checks for updates: run this again to update.
set -eu

REPO=${MIMI_REPO:-TisaneFruitRouge/mimi}
VERSION=${MIMI_VERSION:-latest}
MODE=auto
for arg in "$@"; do
  case "$arg" in
    --appimage) MODE=appimage ;;
    -h | --help) sed -n '2,13p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "Unknown option: $arg" >&2; exit 1 ;;
  esac
done

say() { printf '%s\n' "$*"; }
fail() { printf 'Mimi install: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "this needs '$1', which isn't installed."; }
need curl
need uname

OS=$(uname -s)
ARCH=$(uname -m)
if [ "$(id -u)" = 0 ]; then SUDO=""; else SUDO=sudo; fi

# Pick the file from the release that fits this computer.
case "$OS" in
  Linux)
    [ "$ARCH" = x86_64 ] || fail "there's no Linux build for $ARCH yet, only x86_64."
    if [ "$MODE" = auto ] && command -v apt-get >/dev/null 2>&1; then KIND=deb; SUFFIX=_amd64.deb
    elif [ "$MODE" = auto ] && { command -v dnf >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1; }; then
      KIND=rpm; SUFFIX=.x86_64.rpm
    else KIND=appimage; SUFFIX=_amd64.AppImage; fi ;;
  Darwin)
    KIND=dmg
    case "$ARCH" in
      arm64) SUFFIX=_aarch64.dmg ;;
      x86_64) SUFFIX=_x64.dmg ;;
      *) fail "there's no macOS build for $ARCH." ;;
    esac ;;
  *) fail "Mimi runs on Linux and macOS only." ;;
esac

if [ "$VERSION" = latest ]; then API="https://api.github.com/repos/$REPO/releases/latest"
else API="https://api.github.com/repos/$REPO/releases/tags/$VERSION"; fi
RELEASE=$(curl -fsSL -H "Accept: application/vnd.github+json" "$API") ||
  fail "couldn't reach GitHub to find the release ($API)."

# Each asset lists its sha256 digest before its download address.
PICK=$(printf '%s' "$RELEASE" | tr ',' '\n' | awk -v suffix="$SUFFIX" '
  /"digest":/ { d = $0; sub(/.*"sha256:/, "", d); sub(/".*/, "", d) }
  /"browser_download_url":/ {
    u = $0; sub(/.*"browser_download_url": *"/, "", u); sub(/".*/, "", u)
    if (substr(u, length(u) - length(suffix) + 1) == suffix) { print u " " d; exit }
    d = ""
  }')
URL=${PICK%% *}
SHA=${PICK#* }
[ -n "$URL" ] || fail "the release has no $SUFFIX file."
[ "$SHA" != "$URL" ] || SHA=""

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FILE="$TMP/$(basename "$URL")"
say "Downloading $(basename "$URL")…"
curl -fL --progress-bar -o "$FILE" "$URL" || fail "the download failed."
if [ -n "$SHA" ]; then
  if command -v sha256sum >/dev/null 2>&1; then GOT=$(sha256sum "$FILE" | cut -d' ' -f1)
  else GOT=$(shasum -a 256 "$FILE" | cut -d' ' -f1); fi
  [ "$GOT" = "$SHA" ] || fail "the download is damaged (checksum mismatch). Please try again."
fi

case "$KIND" in
  deb)
    say "Installing the package (your password may be needed)…"
    $SUDO apt-get install -y "$FILE" ;;
  rpm)
    say "Installing the package (your password may be needed)…"
    if command -v dnf >/dev/null 2>&1; then $SUDO dnf install -y "$FILE"
    else $SUDO zypper --non-interactive install --allow-unsigned-rpm "$FILE"; fi ;;
  appimage)
    DEST="$HOME/Applications"
    mkdir -p "$DEST" "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/128x128/apps"
    install -m 755 "$FILE" "$DEST/Mimi.AppImage"
    ( cd "$TMP" && "$DEST/Mimi.AppImage" --appimage-extract '*.png' >/dev/null 2>&1 ) || true
    ICON=$(find "$TMP/squashfs-root" -path '*128x128/apps/*.png' 2>/dev/null | head -n 1)
    [ -n "$ICON" ] && cp "$ICON" "$HOME/.local/share/icons/hicolor/128x128/apps/mimi.png"
    cat > "$HOME/.local/share/applications/mimi.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Mimi
Comment=A private personal AI assistant
Exec="$DEST/Mimi.AppImage"
Icon=mimi
Categories=Office;
Terminal=false
EOF
    command -v fusermount >/dev/null 2>&1 || command -v fusermount3 >/dev/null 2>&1 ||
      say "Note: AppImages need FUSE. If Mimi doesn't open, install your distribution's 'fuse' or 'libfuse2' package." ;;
  dmg)
    MOUNT="$TMP/mount"
    mkdir -p "$MOUNT"
    hdiutil attach -nobrowse -quiet -mountpoint "$MOUNT" "$FILE"
    if [ -w /Applications ]; then DEST=/Applications; else DEST="$HOME/Applications"; mkdir -p "$DEST"; fi
    rm -rf "$DEST/Mimi.app"
    cp -R "$MOUNT/Mimi.app" "$DEST/"
    hdiutil detach -quiet "$MOUNT"
    # Builds aren't notarized yet; without this macOS refuses to open a downloaded app.
    xattr -dr com.apple.quarantine "$DEST/Mimi.app" 2>/dev/null || true ;;
esac

say ""
case "$KIND" in
  dmg) say "Mimi is installed in $DEST. Open it from Launchpad or Spotlight." ;;
  *) say "Mimi is installed. Open it from your applications menu." ;;
esac
