# 0.7.2 - 2026-10-30

## Added

- Stick dead zone setting in Settings → Controls

## Fixed

- PlayStation and Switch controllers now work on Windows through SDL
- D-pad taps and left and right register properly with the left stick steering on

# 0.7.1 - 2026-10-27

## Added

- Controller debug tab with live controller state, an input log and a report to copy or save

## Fixed

- A binding left waiting after leaving Settings → Controls no longer blocks input
- Buttons held through a snapshot or ROM load no longer stick
- Flicking the stick from one side to the other no longer leaves the first direction held

# 0.7.0 - 2026-10-26

## Added

- Game Boy Color support with colours, tile attributes, object priority, double speed, eight WRAM
  banks, HDMA and general-purpose DMA
- Console setting in Settings → General: Automatic, Game Boy or Game Boy Color
- GBC colours for original Game Boy games, either the set the GBC picks or one of the twelve its
  boot screen offers
- GBC Screen setting for softer colours like the GBC's screen or raw colours
- MBC3 real-time clock, kept in battery saves and snapshots
- Serial port with the internal clock
- Twelve more built-in palettes, per-game palettes and custom palettes with share codes such as
  `F2CE44-B89D32-72611C-292406`
- Screen scaling modes: integer, fit and stretch
- Touch layout for phones and narrow windows, with on-screen buttons and a menu for ROMs, homebrew,
  saves and settings
- Size, opacity, left-handed mirroring, vibration and hide options for the on-screen buttons
- Layout setting to force the touch or desktop UI, and a touch UI size slider
- Fullscreen button and home screen standalone mode on the web
- Remappable keyboard and controller bindings
- Remappable shortcuts for quick save (F8), save to a new slot (Shift+F8), quick load (hold F9),
  pause (F5), mute (F10), focus mode (F11), reset (Ctrl+R), screenshot (F12), recording
  (Shift+F12) and the on-screen buttons
- Option to stop the left analog stick from steering the d-pad
- Pause, reset, screenshot and mute buttons in the toolbar
- MP4 screen recording with sound, using Cisco's OpenH264 codec on desktop, downloaded after asking
- Video Codec setting in Settings → General and an OpenH264 licence page in About
- Screenshot and recording scale settings in Settings → General
- Media tab listing screenshots and recordings with thumbnails, filters, export and delete
- Autosaves every few minutes of play, on close, on game switch and after a crash, with the
  interval and count in Settings → General
- Names for snapshots and battery saves
- Battery save import and export with the clock data of SameBoy, BGB, VBA-M and mGBA, and
  RetroArch's .srm and .rtc files
- Dropping a ROM or save file on the window opens it
- Sound settings: mute, output latency, per-channel toggles and an output device picker on native
- Queue, resampling, underrun and overrun stats in the Audio Debug tab
- Six more homebrew games: Tobu Tobu Girl Deluxe, µCity, Libbet and the Magic Floor,
  Dawn Will Come, Petris and Postie
- Test ledger (`make ledger`) that tracks which mooneye, blargg, SameSuite, AGE, acid2, rtc3test
  and Mealybug Tearoom ROMs pass on DMG and CGB

## Changed

- About 1.8x faster emulation
- Emulation catches up after a slow frame instead of dropping the time
- Sound is resampled to hold the chosen latency, fades out when the queue runs dry and crossfades
  ahead when it overfills
- Default audio latency is 100 ms on the web and 80 ms on native
- Volume follows a squared curve
- Web sound plays on the browser's audio thread and starts or resumes on the next tap or key press
- Screenshots are PNGs and save straight into Citrine's data folder (browser storage on web)
  instead of asking where
- Importing a save restarts the game with it, asking first once the game has run a few seconds
- Reset and deleting a snapshot ask first
- Unnamed snapshots count from Slot 1
- Settings live in their own JSON file next to the saves
- The General Info tab is now called About and shows the keys and buttons you bound
- Settings hints sit under the row they explain, and categories move to a top row on narrow panels
- Side panel tabs that don't fit shorten their names and keep only the open tab's close button
- Notifications sit at the bottom of the game view and shorten long names
- Labels and hover texts use sentence case
- The e2e tests fall back to the ledger's ROM archives, and tests with missing ROMs skip with a
  reason

## Fixed

- HALT now checks for interrupts at the hardware's dot, and an interrupt pending at a HALT right
  after EI returns to the HALT
- 16-bit increments, pushes, pops and OAM accesses during mode 2 now corrupt OAM on DMG like the
  hardware
- OAM DMA starts a cycle after the write, keeps OAM blocked when restarted and no longer blocks
  the I/O registers on DMG
- Without a boot ROM, DIV, the sound, joypad and PPU registers and VRAM now start where the boot
  ROM leaves them
- With a boot ROM, the boot logo and game start now run on time
- LY, STAT modes, STAT interrupts and the LY=LYC flag now change at the hardware's dot in each line
- Mode 3 now lasts as long as on the hardware with scrolling, objects and the window
- The CPU can no longer access VRAM and OAM while the PPU is using them
- Writes to LCDC, the palettes, SCX, SCY and WX during mode 3 now land at the hardware's dot
- The window now matches the hardware at WX 0, with mid-line changes and when enabled or moved
  mid-frame
- The screen is blank while the LCD is off and for the first frame after it turns on
- Sound channels now match the hardware in NR52 status, length counters, triggers, the sweep, wave
  RAM, square duty, envelopes and noise timing
- Sound is 6 dB quieter, the same level as SameBoy, and loud games no longer clip
- MBC1 multicart ROMs now bank correctly
- MBC3 games with a clock and MBC5 rumble games with a battery now keep their saves
- Importing a wrongly sized save for an MBC2 game no longer crashes
- Battery saves no longer lose data on reset, game switch, snapshot or export
- Settings from an older build no longer reset when a new field is added
- Picking "Custom" in the palette dropdown created an unusable transparent palette
- Game titles no longer end in a box on colour games
- Typo in the legacy save import message
- Audio no longer swaps left and right, stays silent after a failed device open, or overflows
  during long debugger steps
- Web audio no longer forces a 44.1 kHz audio context
- Holding a gamepad axis no longer records duplicate input events
- Settings, Saves and General Info tabs fit narrow panels without scrolling sideways
- Tab bars that do not fit one row become a dropdown
- Snapshot rows keep their buttons on one line
- Info pages no longer show arrows as boxes or bullets apart from their text, and tables have a
  header row and stripes
- Audio Debug values no longer run past the panel edge at large UI scales
- The mooneye runner no longer counts DMG-0-only tests as DMG tests

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
