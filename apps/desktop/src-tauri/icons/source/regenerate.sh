#!/usr/bin/env bash
# Rebuilds every app icon in src-tauri/icons/ from the master drawings in this folder:
# icon.svg for most sizes, icon-small.svg (fewer details, larger character) for 16-32 px.
# Needs rsvg-convert (librsvg), ImageMagick 7 (`magick`), node, and the app's
# node_modules (for `tauri icon`). Run it from anywhere.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
icons="$(dirname "$here")"
desktop="$(cd "$icons/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

render() { rsvg-convert -w "$2" -h "$2" "$here/$1" -o "$3"; }

# Everything Tauri makes, from the full drawing. The Android and iOS sets aren't used.
render icon.svg 1024 "$tmp/icon.png"
(cd "$desktop" && pnpm -s tauri icon "$tmp/icon.png" -o "$tmp/out" > /dev/null 2>&1)
cp "$tmp"/out/*.png "$tmp/out/icon.icns" "$icons/"

# The smallest sizes from the simplified drawing.
for size in 16 24 30 32; do render icon-small.svg "$size" "$tmp/small-$size.png"; done
for size in 48 64 256; do render icon.svg "$size" "$tmp/full-$size.png"; done
cp "$tmp/small-32.png" "$icons/32x32.png"
cp "$tmp/small-30.png" "$icons/Square30x30Logo.png"

# icon.ico (PNG entries), and icon.icns with its 16 and 32 px entries (is32/s8mk,
# il32/l8mk, ic11) swapped for the small drawing.
magick "$tmp/small-16.png" -depth 8 "rgba:$tmp/small-16.rgba"
magick "$tmp/small-32.png" -depth 8 "rgba:$tmp/small-32.rgba"
node - "$icons" "$tmp" <<'EOF'
const fs = require("node:fs");
const [icons, tmp] = process.argv.slice(2);
const file = `${icons}/icon.icns`;

const ico = [
  [16, "small-16"], [24, "small-24"], [32, "small-32"],
  [48, "full-48"], [64, "full-64"], [256, "full-256"],
].map(([size, name]) => [size, fs.readFileSync(`${tmp}/${name}.png`)]);
const header = Buffer.alloc(6 + 16 * ico.length);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(ico.length, 4);
let offset = header.length;
ico.forEach(([size, png], i) => {
  const at = 6 + 16 * i;
  header.writeUInt8(size % 256, at);
  header.writeUInt8(size % 256, at + 1);
  header.writeUInt16LE(1, at + 4);
  header.writeUInt16LE(32, at + 6);
  header.writeUInt32LE(png.length, at + 8);
  header.writeUInt32LE(offset, at + 12);
  offset += png.length;
});
fs.writeFileSync(`${icons}/icon.ico`, Buffer.concat([header, ...ico.map(([, png]) => png)]));

const rgba = (size) => fs.readFileSync(`${tmp}/small-${size}.rgba`);
const entry = (type, data) => {
  const head = Buffer.alloc(8);
  head.write(type, 0, "ascii");
  head.writeUInt32BE(data.length + 8, 4);
  return Buffer.concat([head, data]);
};
// Legacy RGB entries: each channel on its own, run-length packed (literal runs only).
const rgb = (pixels) => {
  const out = [];
  for (let channel = 0; channel < 3; channel++) {
    const bytes = [];
    for (let i = channel; i < pixels.length; i += 4) bytes.push(pixels[i]);
    for (let i = 0; i < bytes.length; i += 128) {
      const run = bytes.slice(i, i + 128);
      out.push(run.length - 1, ...run);
    }
  }
  return Buffer.from(out);
};
const alpha = (pixels) => Buffer.from(pixels.filter((_, i) => i % 4 === 3));
const replaced = {
  is32: rgb(rgba(16)),
  s8mk: alpha(rgba(16)),
  il32: rgb(rgba(32)),
  l8mk: alpha(rgba(32)),
  ic11: fs.readFileSync(`${tmp}/small-32.png`),
};
const icns = fs.readFileSync(file);
const entries = [];
for (let at = 8; at < icns.length; ) {
  const type = icns.toString("ascii", at, at + 4);
  const length = icns.readUInt32BE(at + 4);
  entries.push(entry(type, replaced[type] ?? icns.subarray(at + 8, at + length)));
  at += length;
}
const body = Buffer.concat(entries);
fs.writeFileSync(file, Buffer.concat([entry("icns", body).subarray(0, 8), body]));
EOF

echo "Icons written to $icons"
