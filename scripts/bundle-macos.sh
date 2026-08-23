#!/usr/bin/env bash
# Assembles Citrine.app from the two already-built Darwin binaries and wraps it
# in a drag-to-Applications disk image.
#
# Expects both slices to exist already:
#   target/aarch64-apple-darwin/dist/citrine-gb-app
#   target/x86_64-apple-darwin/dist/citrine-gb-app
#
set -euo pipefail

VERSION="${1:?usage: bundle-macos.sh <version>   (e.g. 0.6.0, no leading v)}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/target/macos"
APP="$OUT/Citrine.app"
STAGE="$OUT/stage"
DMG="$OUT/citrine-v${VERSION}-macos.dmg"

rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$STAGE"

lipo -create -output "$APP/Contents/MacOS/citrine" \
	"$ROOT/target/aarch64-apple-darwin/dist/citrine-gb-app" \
	"$ROOT/target/x86_64-apple-darwin/dist/citrine-gb-app"
chmod +x "$APP/Contents/MacOS/citrine"

cp "$ROOT/app/assets/icon.icns" "$APP/Contents/Resources/icon.icns"
cp "$ROOT/LICENSE"              "$APP/Contents/Resources/LICENSE"
sed "s/__VERSION__/${VERSION}/g" "$ROOT/app/macos/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

codesign --force --sign - --timestamp=none "$APP"
codesign --verify --deep --strict "$APP"

cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create \
	-volname "Citrine ${VERSION}" \
	-srcfolder "$STAGE" \
	-fs HFS+ \
	-format UDZO \
	-ov \
	-quiet \
	"$DMG"

rm -rf "$STAGE"

echo "$DMG"
