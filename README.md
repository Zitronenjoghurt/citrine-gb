[![Crates.io](https://img.shields.io/crates/v/citrine-gb)](https://crates.io/crates/citrine-gb)
![](http://tokei.lemon.industries/b1/github/Zitronenjoghurt/citrine-gb?category=code&type=Rust&logo=https://simpleicons.org/icons/rust.svg)

# Citrine

**WORK IN PROGRESS**\
A soon-to-be cycle-accurate Game Boy and Game Boy Color emulator.

The core functionality is available as a [standalone crate](lib/README.md).

# Features

Read [FEATURES.md](FEATURES.md) for a list of current and planned features.

# Accuracy

Citrine is compared frame-by-frame against the [SameBoy](https://github.com/LIJI32/SameBoy) reference
emulator by the harness in [lab/](lab/README.md). Result snapshots live in
[experiments/](experiments/README.md).

# Testing

```sh
cargo test --release -p citrine-gb                 # unit and e2e tests
make ledger-check                                  # fails if a ROM in passing.txt stopped passing
make ledger                                        # also adds new passes to passing.txt
cargo test --release --test ledger -- --failures   # also prints why each ROM failed
```

The e2e tests in [tests/e2e](tests/e2e) run a ROM and compare the frame with a saved PNG. They
take the ROM from `roms/test` if it's there, otherwise from the ledger's archives. The CPU tests
need the `tests/cpu-adtennant` submodule (2.2 GB). A test whose ROMs or data are missing prints
`skipped ...` with how to get them instead of failing.

The test ledger runs freely licensed test ROM suites (mooneye, blargg, SameSuite, AGE, dmg-acid2,
cgb-acid2, rtc3test and Mealybug Tearoom) on DMG and CGB.
[tests/ledger/manifest.toml](tests/ledger/manifest.toml) pins every archive and ROM by SHA-256
with the models it has to pass on, and [tests/ledger/passing.txt](tests/ledger/passing.txt) lists
every ROM and model that passes. The archives are downloaded on the first run into
`~/.cache/citrine-gb/ledger` (or `$LEDGER_CACHE`) and never committed.

Each ROM is judged the way its suite says: the Fibonacci registers at `LD B, B` for mooneye,
SameSuite and AGE, the serial text or the result code at `$A000` for blargg, the reference image
at `LD B, B` for acid2 and Mealybug Tearoom, and the reference image after pressing the subtest's
buttons and waiting a fixed time for rtc3test. Images are compared as DMG shades or 15-bit CGB
colours.

The ledger only grows: `make ledger-check` fails as soon as a line in passing.txt stops passing.
To drop a line, delete it by hand and say why in the commit.

# Supported platforms

- Windows (x86_64)
- macOS (universal, Apple Silicon and Intel)
- Linux (x86_64 and aarch64)
- Web (via [Citrine Web](https://gb.lemon.industries))

Download the latest build from [Releases](https://github.com/Zitronenjoghurt/citrine-gb/releases).
