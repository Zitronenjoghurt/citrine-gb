# Citrine for Android

The Android app is the Rust app in [crates/citrine-gb-app](../crates/citrine-gb-app) built as a library, inside a small
Kotlin activity (GameActivity). The Rust side lives in `crates/citrine-gb-app/src/platform/android/`, and all the Kotlin
is in this folder.

# Building

You need:

- the Android SDK with platform 36 and NDK 29.0.14206865 (`ANDROID_HOME` set, or `sdk.dir` in `local.properties`)
- JDK 21
- `rustup target add aarch64-linux-android` and `cargo install cargo-ndk`

```sh
cd android
./gradlew assembleDebug                                    # app/build/outputs/apk/debug/app-debug.apk
./gradlew assembleDebug -Pcitrine.abis=arm64-v8a,x86_64    # also for the emulator
./gradlew assembleDebug -Pcitrine.rustProfile=dev          # Rust debug build, quicker to build, slower to play
```

Gradle runs `cargo ndk` for the Rust library on every build; Cargo only rebuilds what changed.

# Release builds

```sh
cd android
./gradlew assembleRelease    # app/build/outputs/apk/release/app-release.apk
./gradlew bundleRelease      # app/build/outputs/bundle/release/app-release.aab
```

A release build uses the Rust `dist-android` profile (the desktop `dist` profile with the symbol table kept), shrinks
the Kotlin side with R8 (`app/proguard-rules.pro` keeps every name Rust calls through JNI) and has the same crash report
screen as a debug build. Both build types keep the Rust library's symbol table in the APK, so a crash report names the
Rust functions. A release build also writes `app/build/outputs/mapping/release/mapping.txt`, R8's renames. Keep it
with each release to read the Kotlin part of a crash report.

The version name is the workspace version and the version code is `major * 10000 + minor * 100 + patch`, so 0.10.0
is 1000.

## The release key

The release key is yours and never goes in this repo. Make it once, outside the repo, and keep a backup: Android only
installs an update signed with the same key.

```sh
keytool -genkeypair -keystore citrine-release.jks -storetype PKCS12 -alias citrine -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=Zitronenjoghurt"
```

Gradle reads the key from these environment variables, or from `android/keystore.properties` (gitignored) when they
are not set:

| Variable | keystore.properties |
|---|---|
| `CITRINE_KEYSTORE` (path to the .jks) | `storeFile` |
| `CITRINE_KEYSTORE_PASSWORD` | `storePassword` |
| `CITRINE_KEY_ALIAS` | `keyAlias` |
| `CITRINE_KEY_PASSWORD` (the same as the keystore password for PKCS12) | `keyPassword` |

Without a key, the release build still runs and makes an unsigned APK (`app-release-unsigned.apk`) and AAB.

A release APK and a debug APK are signed with different keys, so one can't install over the other. Export your saves
first: uninstalling the app deletes its data folder.

## Release APK in CI

The `android` job in `.github/workflows/release.yml` builds the release APK for a tag next to the desktop apps. It
needs two repository secrets: `ANDROID_KEYSTORE_BASE64` (`base64 -w0 citrine-release.jks`) and
`ANDROID_KEYSTORE_PASSWORD`. The keystore is PKCS12, so the key has the same password, and the alias is always
`citrine`.

- It runs on `ubuntu-latest` with JDK 21 (temurin), the SDK packages `platforms;android-36`, `build-tools;36.1.0` and
  `ndk;29.0.14206865`, Rust stable with the `aarch64-linux-android` target and `cargo-ndk`.
- It writes the key to `$RUNNER_TEMP/release.jks` and runs `./gradlew --no-daemon assembleRelease` with
  `CITRINE_KEYSTORE_PASSWORD` and `CITRINE_KEY_PASSWORD` both set from the password and `CITRINE_KEY_ALIAS` set to
  `citrine`.
- The signed APK goes up as `citrine-<tag>-android.apk`, attested like the desktop archives. The `release` job
  attaches it and lists it in `SHA256SUMS.txt`. `mapping.txt` goes up as the `android-mapping` artifact, which stays
  off the release page.
- Without the two secrets (a fork, or before they are added) the job still builds, but the APK is unsigned and
  nothing is uploaded, since Android refuses to install an unsigned APK.

## Update signing

The app installs an update only when `SHA256SUMS.txt` carries a minisign signature from the key built into it
(`PUBLIC_KEY` in `crates/citrine-gb-app/src/update/worker.rs`). The `release` job signs the file into
`SHA256SUMS.txt.minisig` when the repository secret `UPDATE_SIGNING_KEY` holds the secret key. Without the secret, or in
a build without the public key, the app shows the new version with a link to the release page and installs nothing.

To make the key pair on a Mac:

```
brew install minisign
minisign -G -W -p citrine-update.pub -s citrine-update.key
gh secret set UPDATE_SIGNING_KEY -R Zitronenjoghurt/citrine-gb < citrine-update.key
```

`-W` leaves the secret key without a password, so the job can sign without a prompt. Keep `citrine-update.key` somewhere
safe and offline: with a new key, apps that have the old one refuse the update and point to the release page. The
second line of `citrine-update.pub` goes into `PUBLIC_KEY`.

# Signing

Debug builds are signed with `debug.keystore` in this folder (password `android`, alias `androiddebugkey`), so every
debug APK installs over the last one, whoever built it. Debug keys are not secret. A release key never goes in this
repo.

# Crash reports

The launcher opens `StartActivity`, which starts the game right away unless the last run crashed. Then it shows a
report to copy or share: the device, the Rust panic with its backtrace (`crash-rust.txt` in the data folder), a Kotlin
exception (`crash-java.txt`), Android's own exit record and the app's recent log.

# Files

Saves, settings, media and the window state live in the app's own data folder. Load ROM, save import and every export
use Android's own file picker, so no storage permission is needed. A save pair (.sav with its .rtc) asks for two
file names in a row. The E2E export asks for a folder once and copies the test into it.

# Phone glue

Kotlin talks to Rust through `MainActivity`'s `native*` functions; Rust calls back into public `MainActivity` methods
(`keyboard`, `copyText`, `shareFile`, `openDocuments`, `haptic` and the rest) through `platform::android::java`.

- Gamepads come from `dispatchKeyEvent` and `dispatchGenericMotionEvent` plus the input device listener, not from
  GameActivity's input buffer, whose glue drops gamepad motion events.
- The soft keyboard edits a one-space text that `platform::ime` turns into egui key and text events. A physical
  keyboard's keys go straight to GameActivity's key handler, and Ctrl+V pastes the Android clipboard.
- Back steps back through the app like Esc and leaves the app from its root screen.
