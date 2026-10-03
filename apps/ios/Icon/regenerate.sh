#!/usr/bin/env bash
# Renders the iPhone app icon from icon.svg. Needs rsvg-convert (librsvg).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
rsvg-convert -w 1024 -h 1024 -b "#fffefa" "$here/icon.svg" -o "$here/../Mimi/Assets.xcassets/AppIcon.appiconset/icon-1024.png"
