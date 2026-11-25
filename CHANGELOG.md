# 0.13.0 - 2026-11-25

## Added

- Replays: Record keeps every input instead of a video, plays back in the game, renders the video afterwards and can keep each frame's timings
- Save a clip of the last 30 seconds of play, with a trim panel that also crops replays
- Layout menu for every console with built-in and saved screen layouts, per game and per mode
- Edit layout for screens, buttons and HUDs on phone, desktop and big screen
- Direct connection in Play together, with a switch to the relay and back mid-game
- GBA JIT recompiler on macOS, Linux and Android, for a quarter to a third less CPU time a frame
- RetroAchievements for GBA and DS games, and leaderboards with live attempts in the HUD
- DS microphone on desktop, Android and the web, with Push to talk
- Settings search, and settings in three sections that are the same on desktop, phone and big screen
- Settings for one game, for every row that changes how a game looks, sounds or runs
- This game panel for the running game's console, clock, camera, printer and cheats
- Saves on this network between your devices on the same Wi-Fi, and a Sync page with the saves other devices keep
- Storage page with the space your data and the cache take, Clear for each cache, and Export, Import and Delete everything
- Key combinations of up to 4 keys or controller buttons for game buttons and shortcuts
- DS texture filters Bilinear, Bicubic and xBR, and anisotropic filtering
- Pokéwalker across your devices: Nearby on the same network, Move here, borrowing for a transfer, and steps on Android while the app is closed
- Chat replies, your own typing text, and a Newest messages bar on phones
- Online values in the game HUD: ping, input delay, connection and waits
- Open menu entries for every boot ROM, BIOS and firmware file, and the GBC boot ROM before Game Boy Color games
- Theme that follows the system, fast forward by Hold or Toggle, Quick load on a hold, and play, mute or pause in the background
- Real fullscreen on Shift+F11, a hidden pointer over games, library subfolders, a capture folder, tilt keys and GBA screen colours of their own
- Device names you choose, shown on your other devices
- Discord screenshot of the top or bottom DS screen
- Server: STUN on UDP port 3478 for direct connections, set with CITRINE_STUN_PORT
- `make publish` for the citrine-gb crate and the two crates it needs

## Changed

- Playing together needs 0.13.0 on both sides
- DS on a phone opens Big top in landscape and Stacked at full width in portrait, with the frame hidden
- Console icons in place of the GB, GBC, GBA and DS badges, and new icons for consoles, games, saves and the link cable
- Citrine is now licensed under GPL-3.0-or-later
- Licences page in About with every library the app ships and the projects its code comes from, on the web too
- Faster DS JIT on x86-64 and Apple Silicon
- Faster DS 3D on the GPU at every resolution, with half the CPU time to prepare a frame
- DS games on desktop draw on 2 helper threads instead of up to 7
- Smooth 3D edges is now Anti-aliasing, with 4x and 8x MSAA
- End-to-end encrypted second screens, with a 5-digit security code on both devices
- Lower and steadier input delay in Play together on DS
- Cloud saves download before a library game first opens on a device
- Smaller apps: the Android APK by 11.9 MB and the desktop app by 1.4 MiB
- RetroAchievements sign-in token in the system keyring on Windows and Linux, and out of Android backups
- Big screen with tabs across the top, a Home menu with Friends and Volume, and 4 games a row
- Big screen game menu with every entry of the phone's More menu, a Volume slider, the DS lid and Swap screens
- Desktop settings with checkboxes before their labels, controls in one column on every page and values in percent
- Pokéwalker moves only on Move here, which asks before it replaces a walker with steps
- Chat on a phone with the emoji panel in the keyboard's place and messages that move with the keyboard
- Saves show the date and time they were saved
- Time played card in Stats for the period or day you pick
- Questions that delete, reset or quit start on Cancel with a keyboard or pad
- Keyboard shortcuts sync to your other devices, and controller shortcuts stay on each device

## Fixed

- Android push notifications reach the phone
- Less judder on 120 Hz and faster screens, fast forward at full speed there, and no stall every 30 seconds on desktop
- The app no longer draws on every screen refresh with no game running, or slows down after you add a big game folder
- DS 3D on the GPU no longer crashes on GPUs without storage textures, such as OpenGL-only Linux, or when a phone's driver can't build a shader
- A DS game no longer freezes when it fills the 3D command queue right after a buffer swap
- DS online on the relay no longer stops with error 60000 or 85030, and a save from another device no longer warns that it belongs to another DS
- Achievements no longer stay on "Loading achievements"
- Discord shows GBA and DS screens, 3D included
- Games in zips show their names and covers and import their old .sav, and DS games over 64 MB show theirs
- A settings file with a value the app can't read keeps every other value, the sign-in included
- The web app keeps stats, the Pokéwalker, chats and the RetroAchievements sign-in after a reload
- In Play together the HUD, the Controller pane and second screens follow the session's console
- Big screen: the pad reaches page numbers, sliders, info marks and sheets, and Back returns to the row you left
- Security: damaged or crafted snapshots, saves, zips, chat pictures, cheats, DS files and relay messages no longer crash, hang or fill the app's memory
- Security: the app remembers each friend's chat key, so a server can't swap in its own
- Security: a friend's game state in Play together saves apart from your own saves, and the JIT cache on disk is signed per install
- Server: limits on connections, uploads, GTS tokens and login attempts, and sign-ups that no longer fail during the hourly backup

# 0.12.0 - 2026-11-14

## Added

- Nintendo DS games on desktop, Android and the web, with the built-in FreeBIOS or your own BIOS and firmware
- DS saves, snapshots, autosaves and cloud saves, with DeSmuME .dsv import
- DS screen layouts with touch, X and Y buttons and a lid you can close
- DS 3D at up to 8x resolution with Smooth 3D edges, and a JIT recompiler, on desktop and Android
- DS settings for nickname, birthday, language, favourite colour, date and time
- Second screens: a paired phone or tablet, another window or a second display shows the DS bottom screen, a controller or the game info
- DS online through Kaeru WFC or another server, over your relay or a direct connection
- DS local wireless and experimental Download Play in Play together
- Game info for Pokémon Diamond to Black 2 and White 2, with wild lists and legality marks in Gen 4
- Pokéwalker for HeartGold and SoulSilver, with steps from the game or an Android phone's step counter
- GTS on your relay for the Gen 4 Pokémon games and Black and White, with trades in the app and optional simulated trainers
- Game library with folders, cover tiles, search, filters, sorting, your own tags and a Game info panel
- Game covers for Game Boy, Game Boy Color, Game Boy Advance and DS games
- Games pane on desktop with search, Open and recent games
- Games open from .zip files
- Big screen layout for a controller or TV remote, with a games shelf, an in-game menu and an on-screen keyboard, also on Android TV
- Rewind of up to 2 minutes on every system
- Fast forward on desktop at 2x to 10x or Max, from Space, a controller button or the toolbar
- Cheats: Game Genie, GameShark, Action Replay and CodeBreaker codes, saved per game and synced with your account
- RetroAchievements for Game Boy and Game Boy Color, with an Achievements pane, unlock notices and progress in the library
- Match refresh rate, Same-size pixels and sound Slowdown settings
- Android push notifications for friend requests, play invites and messages, and play invites that ring a closed app
- Focus mode shows new messages, play invites and friend requests beside the game
- Game HUD with FPS, a frame graph, frame time and other live values over the game
- Performance recordings and Game report: a zip of frame timings or of the last seconds of play to send with a bug report
- Threads pane, a DS debugger for both CPUs and a DS files tool

## Changed

- Playing together needs 0.12.0 on both sides, and online play needs an updated server
- Game Boy and Game Boy Color games need about a tenth of the CPU time, Game Boy Advance games under a fifth
- The console runs on its own thread on desktop and Android, and in a Web Worker in browsers that allow threads
- Three or four linked Game Boy Advances run in parallel
- Game Boy Advance games open faster
- Every game frame shows for the same number of screen refreshes on desktop
- Saves, save states and autosaves no longer stall the game
- Enter presses Start, and Space fast-forwards
- Online play shares games up to 32 MB, enough for every Game Boy Advance game
- Faster answers from the relay in online play
- Popups and invite banners float beside the game
- Notices wrap, and one you have to act on stays until you do
- Error messages for failed loads, saves, deletes and renames name the game or save
- Settings pages share one layout, with the same names and places on desktop and phone
- Matrix is now LCD grid, and the sign-in key is called Relay ID everywhere
- Focus mode starts off on every launch
- Performance tab with frames a second, a 10-second frame graph and a bar for each part of a frame
- ROM info with one wording for every console
- Journey names only the badges and places you have reached
- Phone game strip without the game's name
- Desktop look for check boxes, menu ticks and progress bars
- Server: rate limits for shared games, CITRINE_ISOLATION for browser threads, and connection logs with the app version and close reason

## Fixed

- Game Boy and Game Boy Color timing matches SameBoy for DIV after power-on, GDMA length, the CGB halt wake-up, the joypad interrupt and frames with the LCD off
- A Game Boy Advance game no longer freezes when it shortens the cartridge wait states during a prefetch
- Game Boy Advance sound plays at the speaker's rate when sound was on before the game opened
- Sound plays without gaps when a game runs below full speed
- Ruby, Sapphire and Emerald export their clock inside the .sav for mGBA
- Recording no longer stops with "couldn't keep up"
- LCD grid no longer tints the whole picture at 1x
- A friend whose browser tab or phone app sat in the background no longer drops off the relay
- The app no longer freezes for up to 3 s when it connects to a relay or syncs cloud saves
- Link groups no longer split when a member who stopped answering is unlinked
- A Play together session that can't start shows the reason
- Notices no longer cover the game, another pane or the phone's touch buttons
- Desktop toolbar fits narrow windows, with a More menu for the least used tools
- Debugger code is readable in light and dark themes
- Online settings on a phone no longer shows an empty Account card while signed in
- Server startup waits for the database upgrade on a slow disk

# 0.11.0 - 2026-11-04

## Added

- Game Boy Advance games on desktop, web and Android
- Game Boy Advance battery saves, save states, autosaves and cloud saves, with mGBA .sav import and export
- Game Boy Advance online play and link cable, online or between two games side by side
- Rooms of up to 16 people with room chat
- Link groups of three or four on Game Boy Advance
- Game Boy and Game Boy Advance players link by one playing the other's game
- Six Game Boy Advance homebrew games: CASCADE7, GBArcade, Hyperspace Roll, Nonogram Advance, Pentris and The Hat Chooses the Wizard
- Built-in open source GBA BIOS, or a BIOS file of your own
- Pokémon game info for Ruby, Sapphire, Emerald, FireRed and LeafGreen
- Pokémon Map tab with the region map and the wild Pokémon where you stand
- Pokémon Battle tab in Red to Crystal, and a Journey tab with gym leaders and the Elite Four
- MBC6, MBC7, MMM01, TAMA5, HuC1, HuC3, Pocket Camera and Wisdom Tree cartridges, including Kirby Tilt 'n' Tumble and Robopon Sun
- Cartridge settings for Clock, Rumble, Tilt, Sunlight, Camera and Printer
- Rumble on controllers and Android phones
- Tilt with keys, a controller's right stick or a phone's motion sensor
- Sunlight from a phone's light sensor or a slider
- Pocket Camera through a webcam, a phone's front camera or a picture you choose, with a live preview to pick the view
- Game Boy Printer, with prints in Media
- Infrared between two games on one screen
- Online input delay setting
- Debugger trace of the last 30,000 instructions, with text export
- Memory tool with hex view, edits and watchpoints
- Video tool with tiles, background maps, sprites and palettes

## Changed

- Every game with friends is a room, and a friend who drops has a minute to come back
- Playing together needs 0.11.0 on both sides
- Brand yellow on desktop toggles, sliders, focus rings and main buttons
- Darker red on Delete, Reset and other destructive buttons
- Higher contrast for faint text in every theme
- Toasts and download bars stay off the game picture on desktop and the web
- Progress bars for game transfers, chat files, data export and import, recordings and cloud saves
- Data export and import run in the background
- Web saves in the browser's database, with room for many more save states
- One Debugger tab for Disassembly and Time control, with step over, step out, line and frame steps
- One Dev tools menu in the toolbar
- Disassembly marks RAM code and lines the CPU ran
- Registers tab as one table with named bits
- Phone sheets open above the bottom bar

## Fixed

- New devices get chats from before the relay kept history
- Retrying an unsent chat message no longer sends it twice
- Chat edits and deletes made after a dropped send reach your friend
- Game Boy Color passes the last two Mealybug Tearoom tile select tests
- Muted sound channels stay muted after loading a snapshot or autosave
- Time control's reset stops at breakpoints and keeps frame and sound in step
- Hiding the web page and then closing the game no longer saves the autosave twice
- Autosaves made in the same minute show their seconds
- Saves page no longer cuts off the top autosave
- Phone bar no longer shows tooltips

# 0.10.0 - 2026-11-03

## Added

- Android app
- In-app updates on Windows, macOS, Linux and Android
- New phone layout with a bar for Games, Friends, Media, Settings and You
- Phone game strip with 3x fast forward, pause and save states
- Hide the phone UI for a full-screen game, with touch buttons that hide while you use a controller
- Switch between several accounts on one device
- Typing indicators in chat
- Recent ROMs on Android and the web, and a ROM folder on Android
- Tab jumps between the game and the open chat
- Download links in About

## Changed

- Full chat history on every device, with Show older messages
- Bigger touch d-pad with a spot that presses A and B
- Games pause while you're on another phone screen

## Fixed

- Touch d-pad slips into diagonals far less
- Discord status no longer shows the start-up screen or your friend's game
- A lost connection pauses your game
- Settings side bar and page scroll separately
- Reading a chat on one device clears its unread count on the others
- Signing out keeps the server address and password
- Touch controls stay out of the phone's back swipe
- Vibration on press lasts long enough to feel

# 0.9.0 - 2026-11-02

## Added

- One account on several devices being online at once
- Edit and delete sent chat messages
- Two-factor sign-in for relay accounts
- Share custom palettes in chat
- Link cable stats per game and per friend
- Copy or share screenshots and recordings from Media
- Encrypted relay connections, with a warning when a relay's key changes
- Device list on the account page, with sign-out for other devices
- Delete your relay account
- Stricter relay limits on connections, sign-ups, storage and messages
- Admin panel: total storage cap, address blocks, bulk bans, account deletion and limit stats

## Changed

- Relays need Citrine 0.9.0 or newer
- Leaving a relay keeps your relay ID and signs out only this device
- Cloud sync retries after 30 seconds and holds off when storage is full
- Chat bursts go out at the relay's pace instead of failing
- Clear message when a friend's inbox is full
- Game sharing only on invite-only relays
- Play screen says when watching needs game sharing
- Banned accounts' files stop downloading

## Fixed

- Phone sound starts on the first tap and comes back after switching apps
- Back button from an event partner to the event list

# 0.8.0 - 2026-11-01

## Added

- Online play with rollback: invite a friend, join them or just watch them play
- Local link cable: play two games side by side
- Cloud saves with a conflict picker
- Friends tab with requests, blocking and an online status
- End-to-end encrypted chat with read receipts and emojis by RhosGFX
- Discord Rich Presence on desktop, with screenshots and Pokémon progress
- Game info tab for Pokémon Red to Crystal: party, boxes, bag and Pokédex
- Event partner that trades official event Pokémon to your game
- Stats tab: play time charts, per-game stats and Pokémon progress
- Send screenshots, recordings and input replays in chat
- Settings and stats sync between devices
- Real game names from MAME's game list
- Full-size viewer for screenshots and recordings
- Focus mode: drag the screen around, Esc to leave
- Settings → Your data: export, import or delete everything
- Controller buttons as shortcuts
- Games that run on both consoles remember which one you picked

## Changed

- Improved emulation performance
- Web version follows browser zoom and HiDPI screens
- Setting hints open from an info button
- Focus mode hint fades after 3 seconds
- Version number at the bottom of Settings
- No more "Audio started" message

## Fixed

- Windows build no longer opens a console window
- Oversized buttons and a blurry picture in Chrome's phone emulation
- Time control stepping stops at breakpoints
- APU tab mutes survive setting changes
- Media tab buttons fit the panel
- Settings no longer show through the page picker
- Disassembly shows every byte of an instruction
- Style page palette colours line up

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
- Audio no longer swaps left and right, stays silent after a failed device open, or overflows
  during long debugger steps
- Web audio no longer forces a 44.1 kHz audio context
- MBC1 multicart ROMs now bank correctly
- MBC3 games with a clock and MBC5 rumble games with a battery now keep their saves
- Importing a wrongly sized save for an MBC2 game no longer crashes
- Battery saves no longer lose data on reset, game switch, snapshot or export
- Typo in the legacy save import message
- Settings from an older build no longer reset when a new field is added
- Picking "Custom" in the palette dropdown created an unusable transparent palette
- Game titles no longer end in a box on colour games
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
- Full emulator state dump as JSON
- Optional randomized power-on RAM, matching how real hardware boots (Settings → Developer)
- Snapshots: unlimited save-state slots per game with thumbnails
- A Saves tab with `.sav` import and export
- Recently played ROMs in the file menu (native only)
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
- Unused I/O register bits now read as 1 (serial `SC`, `TAC`), unmapped I/O registers read `0xFF`,
  and `IE` is a full 8-bit read/write register
- The HALT bug is now emulated
- Breakpoints are now checked every instruction instead of once per frame, so they actually trigger
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
