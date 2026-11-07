#!/bin/bash
# Renders the app icon (assets/icons/efa-e.svg) into what packaging needs: PNGs for Linux, the
# .ico for Windows (installer and exe resource) and the .icns for macOS. The output is committed,
# so this only needs to run after the SVG changes. Needs rsvg-convert, ImageMagick and either
# png2icns or Python with Pillow.
set -euo pipefail

ICONS=$(cd "$(dirname "$0")/../assets/icons" && pwd)
SVG="$ICONS/efa-e.svg"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

for size in 16 24 32 48 64 128 256 512 1024; do
    rsvg-convert -w "$size" -h "$size" "$SVG" -o "$tmp/$size.png"
done

cp "$tmp/32.png" "$ICONS/32x32.png"
cp "$tmp/128.png" "$ICONS/128x128.png"
cp "$tmp/256.png" "$ICONS/128x128@2x.png"
cp "$tmp/512.png" "$ICONS/512x512.png"

magick "$tmp"/{16,24,32,48,64,128,256}.png "$ICONS/efa.ico"

if command -v png2icns >/dev/null; then
    png2icns "$ICONS/efa.icns" "$tmp"/{16,32,128,256,512,1024}.png
else
    python3 - "$tmp/1024.png" "$ICONS/efa.icns" <<'PY'
import sys
from PIL import Image
Image.open(sys.argv[1]).save(sys.argv[2], format="ICNS")
PY
fi
