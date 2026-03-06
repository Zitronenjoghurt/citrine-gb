[![Crates.io](https://img.shields.io/crates/v/citrine-gb)](https://crates.io/crates/citrine-gb)

# Citrine

**WORK IN PROGRESS**\
A soon-to-be cycle-accurate Game Boy and Game Boy Color emulator.

The core functionality is available as a [standalone crate](lib/README.md).

# Features

- Supports Windows, MacOS and Web
- Plays Game Boy games with MBC1, MBC2 and MBC3 cartridges (no RTC support yet)
- Controller support for native (not web)
- Complete Game Boy audio
- (M-)Cycle-accurate instruction and memory timing
- Save states for games that included a battery
- Included open source homebrew games
- Basic debugging tools

# Planned

- UI overhaul
- Improved controller support
- Game Boy Color support
- Support for more cartridge types
- Improved UX
- Accuracy improvements (more E2E tests)
- Better save states (for all games)
- RTC support
- Core library (documentation) improvements (DX)