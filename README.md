[![Crates.io](https://img.shields.io/crates/v/citrine-gb)](https://crates.io/crates/citrine-gb)
![](http://tokei.lemon.industries/b1/github/Zitronenjoghurt/citrine-gb?category=code&type=Rust&logo=https://simpleicons.org/icons/rust.svg)

# Citrine

**WORK IN PROGRESS**\
A soon-to-be cycle-accurate Game Boy and Game Boy Color emulator.

The core functionality is available as a [standalone crate](crates/citrine-gb/README.md).

### AI Use
After I burned out on the project around v0.6.0 but still had friends enjoying the emulator, I set to improve the accuracy and implement certain features with Claude x-x Please bear with me! The core worked end-to-end with the Game Boy, it still had some minor inaccuracies tho. Now most inaccuracies have been ironed out, GBC support is in and even rollback netcode for online play with friends. I always stayed part of the creative and technical process behind the new features that I would have probably never had the motivation to tackle completely alone.

# Features

Read [FEATURES.md](FEATURES.md) for a list of current and planned features.

# Supported platforms

- Windows (x86_64)
- macOS (universal, Apple Silicon and Intel)
- Linux (x86_64 and aarch64)
- Android (8 or newer, 64-bit ARM)
- Web (via [Citrine Web](https://gb.lemon.industries))

Download the latest build from [Releases](https://github.com/Zitronenjoghurt/citrine-gb/releases).

# Accuracy

Citrine is compared frame-by-frame against the [SameBoy](https://github.com/LIJI32/SameBoy) reference
emulator by the harness in [crates/citrine-gb-lab/](crates/citrine-gb-lab/README.md). Result snapshots live in
[experiments/](experiments/README.md).

# Testing

```sh
cargo test --release -p citrine-gb                 # unit and e2e tests
make ledger-check                                  # fails if a ROM in passing.txt stopped passing
make ledger                                        # also adds new passes to passing.txt
cargo test --release --test ledger -- --failures   # also prints why each ROM failed
cargo test --release --test ledger -- --only timer # runs only the lines that contain timer
```

The e2e tests in [tests/e2e](tests/e2e) run a ROM and compare the frame with a saved PNG. They
take the ROM from `roms/test` if it's there, otherwise from the ledger's archives. The CPU tests
need the `tests/cpu-adtennant` submodule (2.2 GB). A test whose ROMs or data are missing prints
`skipped ...` with how to get them instead of failing.

The test ledger runs freely licensed test ROM suites (mooneye, blargg, SameSuite, AGE, dmg-acid2,
cgb-acid2, rtc3test and Mealybug Tearoom) on DMG and CGB, and the GBA test ROMs of jsmolka and
alyosha-tas on GBA.
[tests/ledger/manifest.toml](tests/ledger/manifest.toml) pins every archive and ROM by SHA-256
with the consoles it has to pass on, and [tests/ledger/passing.txt](tests/ledger/passing.txt) lists
every ROM and console that passes. The archives are downloaded on the first run into
`~/.cache/citrine-gb/ledger` (or `$LEDGER_CACHE`) and never committed.

Each ROM is judged the way its suite says: the Fibonacci registers at `LD B, B` for mooneye,
SameSuite and AGE, the serial text or the result code at `$A000` for blargg, the reference image
at `LD B, B` for acid2 and Mealybug Tearoom, and the reference image after pressing the subtest's
buttons and waiting a fixed time for rtc3test. A GBA test ends in a one-instruction loop with the
number of its first failed test in a register (`r12` for most, 0 when all passed). Images are
compared as DMG shades or 15-bit colours on CGB and GBA. The GBA lines run with the `gba` feature,
which `make ledger` turns on; without it they fail with "no GBA core to run it on". With no BIOS
yet, a GBA test that calls the BIOS stops at its first SWI.

`make ledger-bios GBA_BIOS=<file>` runs the GBA lines again on a BIOS dump of your own, booting
through its intro, and keeps the passes in
[tests/ledger/passing-bios.txt](tests/ledger/passing-bios.txt); `make ledger-bios-check` checks
them. Without the file both print a skip and pass.

The ledger only grows: `make ledger-check` fails as soon as a line in passing.txt stops passing.
To drop a line, delete it by hand and say why in the commit.

A ROM that can't pass on any hardware gets `unpassable = "<why>"` in the manifest. It still runs,
but it stays out of the score and passing.txt and prints on its own line with the reason. If it
ever passes, the ledger fails until the mark comes off.

# Server

[crates/citrine-gb-server/](crates/citrine-gb-server/README.md) hosts the web version of Citrine and the online relay for playing with friends, cloud saves, etc.
