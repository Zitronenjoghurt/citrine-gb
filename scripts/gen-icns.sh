#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSETS="$ROOT/app/assets"
SRC="$ASSETS/icon.png"
SET="$ASSETS/icon.iconset"

command -v magick   >/dev/null || { echo "need ImageMagick (brew install imagemagick)" >&2; exit 1; }
command -v iconutil >/dev/null || { echo "need iconutil (Xcode command line tools)" >&2; exit 1; }

rm -rf "$SET"
mkdir -p "$SET"

emit_16()    { magick "$SRC" -filter Box     -resize "${1}x${1}" "$SET/$2"; }
emit_32()    { magick "$SRC" -filter Lanczos -resize "${1}x${1}" "$SET/$2"; }
emit_crisp() { magick "$SRC" -filter point   -resize "${1}x${1}" "$SET/$2"; }

emit_16     16   icon_16x16.png
emit_32     32   icon_16x16@2x.png
emit_32     32   icon_32x32.png
emit_crisp  64   icon_32x32@2x.png
emit_crisp  128  icon_128x128.png
emit_crisp  256  icon_128x128@2x.png
emit_crisp  256  icon_256x256.png
emit_crisp  512  icon_256x256@2x.png
emit_crisp  512  icon_512x512.png
emit_crisp  1024 icon_512x512@2x.png

iconutil -c icns "$SET" -o "$ASSETS/icon.icns"
rm -rf "$SET"

echo "wrote $ASSETS/icon.icns"
