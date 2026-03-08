# 0.5.0 - 2026-03-08

## Added

- Tabs system with custom splitting and window-popout
- Focus mode for only viewing the Game Boy screen
- Info panel
- UI themes

## Changed

- Full UI redesign

---

# 0.4.0 - 2026-03-06

## Added

- Full Game Boy audio support
- Volume slider in settings window

## Fixed

- Theme resetting when loading a boot rom

## Changed

- Matrix now uses the current themes brightest color as its base color for the grid

---

# 0.3.0 - 2026-03-03

## Added

- MBC3 (without RTC) support, enabling playing games like Pokémon Red/Blue
- Saves in web
- Bundled homebrew demo games

## Changed

- Improved controller support for native

---

# 0.2.0 - 2026-03-01

## Added

- Support for MBC2 cartridges
- Configurable matrix/grid overlay
- Configurable ghosting
- Save functionality (native only, planned for web) for compatible cartridges (that have a battery and generally support
  saving on actual hardware)
- Darker, original Game Boy theme

## Fixed

- Window scrolling glitches (especially prominent in Super Mario Land 2)
- Sprite overlap glitches
- Messy frame pacing

---

# 0.1.0 - 2026-02-28

## Features

- MacOS (arm), Windows and Web support
- Plays most Rom only and MBC1 game boy games
- (M-)Cycle-accurate instruction and memory timing
- Debug tools like viewing registers in-ui, cycle stepping, end-to-end test exports

## Planned

- Sound
- Accuracy improvements
- Save states
- Full Game Boy Color support
- Support for most cartridge types
- Improved debugging tools
- Bugfixes and UI improvements
- and more...