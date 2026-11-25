#!/usr/bin/env bash
# Rebuilds crates/citrine-gb-app/assets/licences.md, the library licences the
# app's Licences page shows. Run it from the repository root after a
# dependency change. Needs curl and python3.
set -euo pipefail

VERSION=0.8.4
ROOT=$(git rev-parse --show-toplevel)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ASSET=x86_64-unknown-linux-musl ;;
  Linux-aarch64) ASSET=aarch64-unknown-linux-musl ;;
  Darwin-arm64) ASSET=aarch64-apple-darwin ;;
  Darwin-x86_64) ASSET=x86_64-apple-darwin ;;
  *) echo "no cargo-about build for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

curl -fsSL "https://github.com/EmbarkStudios/cargo-about/releases/download/$VERSION/cargo-about-$VERSION-$ASSET.tar.gz" \
  | tar xz -C "$WORK"
ABOUT=$(find "$WORK" -name cargo-about -type f | head -1)

cd "$ROOT/crates/citrine-gb-app"
"$ABOUT" generate --frozen \
  -c "$ROOT/scripts/licences/about.toml" \
  "$ROOT/scripts/licences/about.hbs" \
  -o "$WORK/licences.md"
python3 -I "$ROOT/scripts/licences/group.py" "$WORK/licences.md" "$ROOT/crates/citrine-gb-app/assets/licences.md"

if [ -z "${ANDROID_HOME:-}" ] || [ -z "${JAVA_HOME:-}" ]; then
  echo "kept the Android list: set ANDROID_HOME and JAVA_HOME to rebuild crates/citrine-gb-app/assets/notices/android.md" >&2
  exit 0
fi
cd "$ROOT/android"
./gradlew --no-daemon --console=plain -q :app:dependencies --configuration releaseRuntimeClasspath > "$WORK/android-deps.txt"
python3 -I "$ROOT/scripts/licences/android.py" "$WORK/android-deps.txt" \
  "${GRADLE_USER_HOME:-$HOME/.gradle}/caches/modules-2/files-2.1" \
  "$ROOT/crates/citrine-gb-app/assets/notices/android.md"
