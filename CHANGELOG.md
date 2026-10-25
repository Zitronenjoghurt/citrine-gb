# Unreleased

## Added

- Touch layout for phones and narrow windows, with a menu for ROMs, homebrew, saves and settings
- On-screen buttons below the screen in portrait and beside or over it in landscape
- Size, opacity, left-handed mirroring, vibration and hide options for the on-screen buttons
- Diagonal input on the touch d-pad
- Fullscreen button in the web touch layout
- Standalone mode when the web app is opened from a phone's home screen
- Touch UI size slider in Settings → Controls
- Layout setting in Settings → General to force the touch or desktop UI
- Remappable keyboard and controller bindings, with a warning for unbound buttons
- Option to stop the left analog stick from steering the d-pad
- Remappable shortcuts for quick save (F8), save to a new slot (Shift+F8), quick load (hold F9),
  pause (F5), mute (F10), focus mode (F11), reset (Ctrl+R), screenshot (F12) and on-screen buttons
- Pause, reset, screenshot and mute buttons in the toolbar
- PNG screenshots
- Custom screen palettes with share codes such as `F2CE44-B89D32-72611C-292406`
- Twelve more built-in palettes: Amber, Aqua, Gold, Gooseberry, Hollow, Metallic, Mist, Mokky,
  Papercut, Slimy, Snowflake and Velvet
- Per-game palettes
- Snapshot names
- Sound settings: mute, output latency, per-channel toggles and an output device picker on native
- Screen scaling modes: integer, fit and stretch, set separately for desktop and touch
- Callback size, queue fill, resample rate, underrun and overrun rates, and a stats reset in the
  Audio Debug tab
- Shortcut, touch layout and palette sections on the welcome page

## Changed

- About 1.8x faster emulation, with the APU mix only recomputed when a channel changes
- Per-channel scope samples are only collected while the APU Waves tab is open
- Audio resampling holds the output buffer at the target latency
- Audio buffers are sized per backend, defaulting to 90 ms latency on the web and 40 ms on native
- The ring buffer holds at least three device callbacks
- Underruns fade out instead of cutting to silence
- Settings live in their own JSON file next to the saves
- Volume follows a squared curve
- The web app no longer draws under a notch, and touch gestures no longer scroll, zoom or select
  text on the controls
- Sound also starts on the first tap or key press
- Sound comes back on the next tap or key press after the browser suspends a hidden page
- Settings categories move to a row across the top on narrow panels

## Fixed

- Web audio no longer forces a 44.1 kHz audio context
- Left and right audio channels could swap when the ring buffer was nearly full
- Sound never started after a failed attempt to open the audio device
- The APU's sample buffer grew without limit while no audio stream was attached
- Picking "Custom" in the palette dropdown created an unusable transparent palette
- Settings from an older build no longer reset when a new field is added
- The upscaled frame buffer is no longer reallocated every frame
- Holding a gamepad axis no longer records duplicate input events
- Typo in the legacy save import message

---

# 0.6.0 - 2026-08-21

## Added

- Disassembly tab with static ROM analysis, instruction breakpoints and per-instruction reference info
- Audio Debug and APU Waves tabs for inspecting the sound hardware while a game runs
- Performance tab showing emulation and frame timings
- Input recording (Debug Actions tab): captures button presses with cycle timestamps for replay
- Snapshots: unlimited save-state slots per game with thumbnails
- A Saves tab with `.sav` import and export
- Recently played ROMs in the file menu (native only)
- Optional randomized power-on RAM, matching how real hardware boots (Settings → Developer)
- Full emulator state dump as JSON
- MBC5 cartridge support
- Frame-by-frame accuracy comparison against the SameBoy reference emulator (`lab/`), with committed
  result snapshots under `experiments/`
- Per-ROM mooneye test suite runner (`make test-mooneye`)

## Changed

- Saves are now stored per game (identified by ROM contents) in Citrine's application data folder
  instead of next to the ROM file. Existing `.sav` files are imported automatically the first time
  you load their game, and the originals are left in place

## Fixed

- Saves are now written when you close the emulator, so the last few seconds of play are no longer lost
- Bundled homebrew games can now save
- A `.gb` and a `.gbc` of the same game no longer share (and overwrite) one save file
- Saving in the web version now works outside Chromium-based browsers
- Inverted DMG post-boot `F` flags (half-carry/carry are set when the header checksum is non-zero)
- Interrupt dispatch now reads the target vector from `IE` after pushing the return address, so a
  stack push onto `$FFFF` can redirect or cancel the interrupt
- `RETI` now enables `IME` immediately instead of after the following instruction
- Breakpoints are now checked every instruction instead of once per frame, so they actually trigger
- Unused I/O register bits now read as 1 (serial `SC`, `TAC`), unmapped I/O registers read `0xFF`,
  and `IE` is a full 8-bit read/write register
- The HALT bug is now emulated
- Building the library standalone with only the `debug` feature failed to compile

---

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

- Matrix now uses the current theme's brightest color as its grid base color

---

# 0.3.0 - 2026-03-03

## Added

- MBC3 (without RTC) support, enabling games like Pokémon Red/Blue
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
- Save functionality for cartridges with a battery (native only, planned for web)
- Darker, original Game Boy theme

## Fixed

- Window scrolling glitches (most visible in Super Mario Land 2)
- Sprite overlap glitches
- Messy frame pacing

---

# 0.1.0 - 2026-02-28

## Features

- MacOS (arm), Windows and Web support
- Plays most ROM-only and MBC1 Game Boy games
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