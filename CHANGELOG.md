# 0.13.0

## Added

- Layout: the Screens button and menu are now Layout, for every console. It shows picture cards of the built-in layouts that fit this mode (DS: Stacked, Side by side, Big top, Big bottom, Top only, Bottom only; Game Boy and GBA: Standard), the layouts saved in this mode, and New. Two bars pick where a card applies: All games or this game, and Every mode or this one (portrait, landscape, window, full screen, big screen); the sheet opens on the level the current layout comes from. Rows show the other modes' layouts and open them. Edit layout saves the layout in use in place with Save, without asking a name, and a built-in keeps its name with the changes. An edited built-in has Reset in its card menu, which asks first and brings the built-in back. New asks a name (My layout, My layout 2) and Create makes it; Cancel, Back and Esc keep nothing. A long press or right-click on a saved card renames or deletes it, and Delete asks first. On a phone it is a sheet from the bar above the game (a side sheet in landscape), on desktop a menu from the layout chip with F2 and Shift+F2, and on the big screen a panel from Layout in the game menu. Screen layout, Swap screens and Screens shown left the DS settings page
- Recordings: Record saves a replay, the game's start state and every input, instead of a video file. A minute of inputs takes under 2 KB on every console; the start state is 41 KB for a Game Boy game, 92 KB for Crystal, 646 KB for Emerald and 6.9 MB for a DS game. While recording, Start over goes back to where the recording began. The video renders from the replay afterwards, Right away by default or When I ask (Settings, Capture, Render the video), with a Render video button and a progress bar in Media
- Recordings: Play on a replay in Media plays it in the game, with a bar above the screen: play and pause, drag to any frame, and Close, which brings back the running game as it was. While a replay plays, the toolbar leaves out Pause, Rewind and Saves. During Play together or with a link cable, Play opens the replay's video instead, and the live game keeps running
- Recordings: with "Performance data in recordings" on in the Performance pane (or Performance data on the record bar), a replay keeps, for every frame, how long the game, the display, the app, the 3D GPU, the JIT and the sound took and which frames were dropped or shown twice (115 KB a minute at 60 Hz). Export performance data in Media saves it as a zip, and the frame times show as a graph above the seek bar. A click on the graph seeks there, and hovering shows that frame's times against the time a frame may take, such as "Frame at 0:08 took 3.0 ms of 16.7 ms"
- Recordings: Save a clip (beside Record in the toolbar and in More on a phone, Ctrl+F12) keeps the last 30 seconds without recording first. The clip opens paused under the game with a strip of pictures: drag its start and end, play the part, then Save clip puts it in Media as a replay, and Cancel brings back the running game. Cutting a 30 s clip took 2.1 ms on Crystal, 4.8 ms on Emerald and 49 ms on Platinum, and the files were 90 KB, 644 KB and 6.8 MB. The same panel crops a replay from Media into a new one, from the scissors button on its bar, and the original stays. Settings, Capture, Clip length sets how far back it reaches: Off, 10 s, 30 s (the default), 1 min or 2 min. The app keeps the longer of Rewind and Clip length, so a clip reaches back its full length with Rewind off or shorter, and Rewind still stops at its own length. With Clip length Off, Save a clip is greyed out and says where to turn it on. Settings from before take the Rewind length as the Clip length, so no clip gets shorter
- Controls: a game button or a shortcut can be bound to a combination of up to 4 keys or up to 4 controller buttons, such as Q + W or L1 + R1 + Select. Hold them in Settings, Controls and let go to save. The game sees the combination's first keys until its last one goes down, then they let go. Settings from older versions load unchanged, and an older version reading new settings keeps its single-key bindings
- Pokéwalker: the walk card ends with where steps come from: "This phone" when the phone counts them, else a "Count steps walked in HeartGold" switch
- Pokéwalker: a device that doesn't hold the walker shows the holder's last save (Pokémon, route, walk steps, the week's steps, caught Pokémon and items) under "On <device>" with Move here, without downloading the walker: 0 fetches in the tests, where 0.12.0 fetched every newer copy
- Devices: rename this device from the devices list; the server shows the new name after the next sign-in
- Android: steps count while the app is closed, as long as a walker is out on the phone. A foreground service with a "Pokéwalker" notification records steps per minute (24 bytes a minute, 8 days at most, under 280 KB) and starts again after a reboot or an app update; the walker credits them when the app opens
- Pokéwalker: Connect on a device that doesn't hold the walker borrows it for the game and sends it home after the transfer, with 1 click on that device and none on the phone in the tests and the picture runs. The phone shows "<device>, Using it for HeartGold" meanwhile
- Pokéwalker: devices on the same network list each other under "Nearby", with Move here on the device next to the one holding the walker; it moves with 1 tap there and 1 tap on Allow on the holder the first time (0.63 to 0.69 s after the first tap over three picture runs), and without asking after that. Works with cloud saves off; a device without a used walker offers nothing, and when the holder leaves the network its card says "Not nearby" with no buttons
- Pokéwalker: a copy the app keeps aside shows as a card at the top of the Walker page with its steps and why it was kept: Replace on Move here, an import, a cloud fetch over changes made here, or a change made while another device held the walker. Use it swaps the copy in and the replaced walker becomes the card's copy, so a second Use it swaps back; close hides the card and the copy stays in Your data. A copy with no steps, Pokémon or items shows no card. The copy kept on Replace now holds the steps walked since the last save, up to 60 s of them before
- Play together: Settings, Online, Play together has a "Direct connection" switch. When both players turn it on, the game runs straight between them, and it moves to the relay and back mid-game when the direct path drops or returns, with no restart or resync. The ping in the bar and in the room rows has a mark for the path: a two-headed arrow when direct, a globe through the relay. Hovering it, or tapping it on a phone, says "Direct connection" or "Through the relay"
- Game HUD: an Online group of values. Ping is the round trip to the other player, to each player in a group, or to the server when signed in without a game together. Input delay shows frames and ms, as in "4 frames, 67 ms". Connection says Direct or Relay, and Waits counts the times the game paused for the other player in the last minute. They refresh twice a second and show a dash without a session
- Server: the relay answers STUN on UDP port CITRINE_STUN_PORT (3478 by default, 0 turns it off), so two players can learn their internet addresses and play straight to each other. The Docker compose file publishes the port; a server behind a firewall needs UDP 3478 open
- DS: games hear the microphone on desktop, Android and the web. Settings, DS, Microphone, Input is Off by default; picking Same as system or a device asks the system for the microphone at that moment, and a refused or missing microphone gives the game silence and a notice with a Settings button. With Push to talk on, the game hears you only while the Talk control is held: a key in Controls (T by default, also on pads) or a Talk button on the touch screen. A small microphone mark beside the game screen shows whenever the game can hear you. Play together and game reports hear silence
- Discord: with a DS game, Settings, General, Discord has a Screens choice for the screenshot: Both (as before), Top screen or Bottom screen. The Discord settings now also show in the touch layout on desktop, where they were missing
- Chat: reply to a message with Reply in its menu, the reply button beside it on desktop, or a swipe right on a phone. The reply shows the original's author and first line above it, and a click on that line scrolls to the original. A friend on 0.12.0 gets the reply as a plain message
- Chat: Settings, Online has "Typing text", the words friends see while you type, such as "Clara is meowing" (32 characters at most; empty keeps "is typing"). It syncs to your other devices; friends on 0.12.0 still see "is typing"
- Chat: on a phone, a bar above the message field shows once you scroll up a few messages from the newest. It says "Newest messages" or how many of the friend's messages came in since, sits below the list so it covers no message, and a tap scrolls to the newest message
- Achievements for GBA and DS games: the Achievements tab lists the set, unlocks show a notice, and the library shows each game's progress, the same as for Game Boy games
- Edit layout: on a phone, Edit layout in More (and first in the DS screens sheet) pauses the game and outlines each screen and button group. Drag one to move it, drag a corner of the selected one to resize it, and set the buttons' opacity in the bar at the top. Reset puts the default back and turns into Undo until the next change, and Done or Back ends editing. Each console keeps its own layout for portrait and landscape. HUDs move and resize in Edit layout too. On desktop, Edit layout in the Screens menu moves and sizes the DS screens and HUDs inside the game pane. On the big screen, Edit layout in the game menu takes the place of Move screen: the d-pad moves the picked item, the shoulder buttons pick the next one, the triggers resize it, North resets or undoes and Back ends editing
- Settings: pages in three sections, the same on desktop, phone and big screen: All consoles (Screen, Sound, Controls, Playing, Capture), Consoles (Game Boy, GBA, DS) and Citrine (App, Online, Sync, Storage, About). Every page shows the same rows whatever game runs, or with none
- Settings: a search field above the pages finds a setting by its name or a related word, such as "cursor" or "box art". Picking a hit opens its page at that row, which lights up for a moment. Ctrl+Comma (Cmd+Comma on a Mac) opens Settings with the search field ready, and on the big screen the top face button does
- This game: the settings that belong to the running game (its console, game clock, recenter tilt, camera, Game Boy Printer, cheats and Save game report) moved out of Settings into This game, opened from the toolbar or the This game tile in More on a phone. In a game together it waits until you play on your own
- Settings for one game: while a game runs, a Settings page with rows that can differ for that game starts with "All games" and the game's name. With the game picked, a change applies only to that game; its rows show "This game" with an x that goes back to the value for all games, and the other rows show "All games". With All games picked, a row the game has its own value for shows a pin, and hovering it shows that value. At 1280 wide, the boxes beside the names get narrower so each name fits on one line, and hovering a cut value shows it in full. Esc or Back steps back to All games, and on the big screen L2 and R2 switch and West removes the game's value. In a game together, a change applies to all games. Every row that changes how one game looks, sounds or runs works this way: scaling, same size pixels, LCD grid, ghosting, volume, the sound while slow or fast forwarding, fast forward speed, rewind, the palette, screen colours, Run games on, each console's sound channels, sunlight, the DS screen, 3D and JIT rows, and the DS language, which applies at the next start. Rows about the app, the device, accounts, sync, files and Controls stay the same for all games, and a test fails when a new game row is neither. This game lists the game's own values under "Changed from all games", and "All settings for this game" opens Settings for it. Palettes and consoles set per game in 0.12.0 carry over, and the values sync to other devices except those of rows that stay on the device, such as volume and the DS 3D rows
- Settings, Game Boy: Palette has a reset arrow back to Citrine, like the rows around it, and its box lines up with theirs
- Settings, App, Look: Theme has "Same as system", which follows the system's light or dark mode
- Settings, Sound, Timing: "While fast forwarding" plays the sound Silent or Sped up
- Settings, Playing, Speed: the fast forward key works by Hold or Toggle
- Settings, Playing, Saves: Quick load loads on a 2 s hold, so a stray press no longer loads over the game; Press brings back the old way
- Settings, Playing, Pause: "In the background" picks Play on, Mute or Pause when the app loses focus (Play on by default)
- Settings, App, Look: "Hide the pointer over games" (off by default), and "Fullscreen" for the real fullscreen, which Shift+F11 also turns on and off. F11 stays focus mode
- Settings, App, Library: "Look in subfolders": Off, 1 level, 2 levels or All levels
- Settings, Capture: a Folder for screenshots and recordings on desktop
- Settings on desktop at the narrowest dock (217 pt): the DS birthday month reads Jan to Dec where its whole name would push the day past the row, as "September" ran 31 pt short; wherever the chosen month's whole name fits, as at 1280, it reads January to December, as on the phone, and the Discord preview wraps "Choosing a game" instead of cutting it
- Copy buttons show the copy icon alone, with "Copy" on hover or focus and a check for 2 s after a copy, so the Online ID fits beside its label at the 217 pt dock, where it ran 5 pt short. On a phone the Online ID row keeps the ID and shows the check after a tap. The check is drawn in the accent colour. On desktop the tip opens left of the button where that covers no other control, else on the side that covers the least, so it no longer covers the Display name below, and it goes once the check shows and stays away until the pointer leaves the button, so it no longer covers the Relay ID's copy button
- Settings on desktop: checkboxes sit at the left edge of the labels, as on a Mac, with the label after the box, and a row that only works while the checkbox above it is on starts where that checkbox's label starts: the LCD grid, Ghosting and HUD rows on Screen, the on-screen button and big screen rows on Controls, the Discord rows on App, and Typing text on Online. A heading's text starts at the same edge, with its icon to the left of it, on every page with headings, Games and Achievements included
- Settings on desktop at the narrowest dock (217 pt): the rows of a page start their controls at one x, set by the narrowest value that still shows whole, and a section moves off it only where one of its labels' longest word needs the room, so no label breaks inside a word. The rows of a section share one x wherever every value in it still shows whole; a section where one would cut, as in Sound, keeps each row at its own x. Where that word still leaves a value short, the gap before the section's values narrows by up to 4 pt, as for "Screens shown" on DS, by 2.4 pt. On the DS page Nickname and the colours sat 17.5 pt right of the selects and 3D resolution 56.5 pt; now Nickname, Birthday, Language, Colour and Clock share one x and "3D resolution" keeps its label on one line, its select 44.4 pt right of the page's column. On App, "Shot every" and "DS screens" put their selects at one x, each label on one line. In Controls, "Scaling with buttons" has no info mark, and the notes for "With menus hidden" and "Confirm button" moved to the info marks of their headings, On-screen buttons and Big screen, so both rows keep their values whole at that width
- Settings, DS: the microphone level is a meter of 12 green segments, none lit while silent, in place of a bar that looked like a slider. Unlit segments have the progress bars' track colour, at 1.5:1 or more against the page in light and dark
- Settings, DS: the first favourite colour is "Grey", spelt like "Colour" on the same page
- Settings, Controls: Reset to defaults is as wide as the selects above it, so it ends at their right edge
- Settings, Online: the "Leave or delete" heading has a warning icon, no longer the same icon as the Leave this relay button
- Open menu: lists every file the app can open by name: GB boot ROM, GBC boot ROM, GBA BIOS, DS ARM9 BIOS, DS ARM7 BIOS and DS firmware. Each pick is checked by its size: a file picked for the wrong slot goes to the one its size fits, and a size no slot takes gets a notice with a Pick another button. A kept file runs with the next game and shows in its console's settings; the Game Boy page has a row for each boot ROM. A Game Boy boot ROM no longer needs a running game, and a Game Boy Color boot ROM now runs before Game Boy Color games. On a phone, Games, More games has "BIOS and boot ROMs" with the same files. The menu's game entry is now "ROM", and opening a game reads "Open a ROM" on desktop and phone. Storage counts the kept files under "BIOS and boot ROMs"
- Settings, Controls: the four tilt directions are keys of their own under Tilt keys, set like any other key
- Settings, GBA: Screen colours has its own setting; before, it changed with the Game Boy Color screen. Saved settings keep their colours
- Achievements: a game with RetroAchievements leaderboards gets a Leaderboards tab beside Achievements, with each leaderboard's title and your best score and rank. A leaderboard opens to its top 10 and the 5 places around you; Esc or Back returns to the list. While an attempt runs, its title and score show in a HUD block of their own in a free corner of the game screen, several attempts in one block, and a notice says when an attempt starts, fails or is submitted with its rank. The game card shows the line your RetroAchievements profile shows while you play
- Crate: `make publish` puts citrine-gb on crates.io together with the citrine-gb-blip and citrine-gb-threads crates it needs. `make publish-check`, part of `make check`, packages the three without uploading and builds the packaged citrine-gb with no features, `gba` and `nds`. The crate README shows how to load and run a game and what each feature adds
- Settings has a Storage page: the space your data and the cache take, with Clear for each kind of cache and Clear all. Every Clear asks first and lists each kind with its size and what clearing costs, such as covers downloading again. The page's Your data section has Export, Import and Delete everything under a row per kind with its count and size, such as "3 saves · 96 KB" (an empty row shows "0 B" alone, and a row too narrow for both drops the count and keeps the size), and all three act on the kinds it lists; there is no separate Your data page, and settings saved on it open Storage. Searching "your data", "export", "import", "delete" or "backup" lands there
- Settings, DS, 3D: Texture filter (Nearest, Bilinear, Bicubic, xBR) and Anisotropic filtering (Off, Trilinear, 2x, 4x, 16x), shown above Native; searching "pixelated" or "shimmer" finds them. New players start on Bicubic with 4x, which smooths the blocky near floor of a Mario Kart DS race and stops far floors and signs from shimmering. At 4x on an Apple Silicon Mac it takes 2.8 ms of GPU time a frame in a Mario Kart DS race, up from 0.5 ms, and 2.3 ms in HeartGold, up from 0.8 ms. Bilinear takes about half the GPU time of Bicubic, and xBR about 1.5 times
- Settings, DS, 3D: settings from 0.12 keep Nearest with Anisotropic filtering Off, the 0.12 look, and new players still start on Bicubic with 4x. In a Mario Kart DS race at 3x with smooth edges on an Apple Silicon Mac, Nearest with Off takes 3.6 ms of GPU time a frame, against 4.6 ms in 0.12 and 6.0 ms with Bicubic and 4x
- Settings, Citrine: a Sync page after Online. It holds the Cloud switches that were under Online, "Download saves for" (Your library or All games), and "Saves elsewhere": the games other devices keep saves for that this device doesn't download, each with where they are, how many and their size, and a Get button with a progress bar by bytes
- Settings, Sync, in the desktop and Android apps: "Saves on this network" copies battery saves, state slots and autosaves straight between your devices on the same Wi-Fi, without the cloud or a server. Devices signed in to your account sync by themselves; a paired device syncs once both sides turn on "Sync saves" for it under "Devices on this network". Each device downloads the games "Download saves for" picks, the same as from the cloud, and two different saves open the save dialog with the other device's name
- Settings, Sync, in the desktop and Android apps: "Sync settings" works with "Saves on this network" too, so your theme, palettes, key bindings and screen effects copy between your devices on the same Wi-Fi with Cloud saves off. Volume, controllers, touch controls and the UI size stay on each device, as with the cloud
- DS: under Settings, DS, Microphone, Input a bar shows how loud the picked microphone is while the page is open, or says why it hears nothing (no permission, no microphone found). The microphone closes when the page does

## Changed

- DS on a phone in landscape opens Big top: the top screen is 524x393 and the touch screen 296x222 at 915x412. The frame starts hidden, and an eye button that covers no game pixels and no button brings it back. Buttons over the game start at 50% opacity (was 75%), buttons beside it at 75%
- DS on a phone in portrait opens Stacked with both screens the full width: 400x300 each at 412x915, where they were 282x212. The frame starts hidden, as in landscape, and the eye button between L and R brings it back. Below the screens sit L and R, then the D-pad and face buttons, then Select and Start
- Builds: a clean release build runs the app's build script (ROM and emoji packing) in 5 to 8 s instead of 38 to 64 s on a Mac, with byte-identical output
- Icons: the Game Boy, Game Boy Color, Game Boy Advance and DS, a game, This game, the Games tab, Change game, Homebrew, the link cable plugged in and pulled out, Top only and Bottom only, Replay, save states, the Pokéwalker and the itch.io link have their own icons, drawn to match the rest, where a phone, a gamepad, a SIM card, a plug or a floppy disk stood in. Play together, Recent, Online, Undo, Friends and boot ROMs each use one icon everywhere, Reset shows a power button, and the Play on Game Boy Color button shows the console it switches to, and a kept Pokéwalker copy shows an archive box instead of the Recent clock
- Consoles show as their icons instead of GB, GBC, GBA and DS badges: in the library and its filters, the recent games menu and list, saves, the link panel, Homebrew and Settings. A game without a cover shows its console's icon alone on its tile, on the phone and the big screen shelf too. The saves badge draws its console at 16 pt on whole pixels with square or one-pixel corners, sharp at 1x and 2x, centres it on its date, and names it on hover to its right, on two lines, or over its card's name when the window has no room, never over a button, its date or another card. The phone link strip shows the short name beside the icon (GBC) and keeps 10 pt from the screen edges
- The Open menu's boot ROM, BIOS and firmware rows show their console with a mark: a power button on the boot ROMs, a chip on the GBA BIOS, 9 and 7 on the DS BIOS files and a gear on the DS firmware
- Recent menu: a Game Boy or Game Boy Color game's submenu lists both consoles by icon and name, the one it plays on first, and a game without that submenu names its console on hover. In the console filter, "All consoles" is the first tick row, ticked when no console is picked, and starts where the console names start
- Settings, Screen: "Every pixel the same size" is now "Every pixel is the same size", on one line at 1280
- Settings on desktop: every label and heading starts at one text edge, and each checkbox hangs left of it with a 4 pt gap, so long names stay on one line; clicking the label ticks the box too. Settings that depend on another (Grid lines, Grid corners, Ghosting strength, HUD opacity) sit one step in. Empty boxes have an edge at 3:1 contrast in light and dark. Sliders and menus keep their column
- Settings, Screen: with the LCD grid on at Fit or Stretch, the line under the LCD grid row is gone on desktop and phone; its info mark says the game shows at "Integer (sharpest)" and how "Every pixel is the same size" keeps your scaling
- Settings on desktop: Grid lines, Grid corners, Ghosting strength, Volume and the touch controls' Size and Opacity read as percent, like 15%, as on a phone, and Sunlight reads 50% instead of 50 %
- Citrine is now licensed under GPL-3.0-or-later
- Settings, Controls on a phone: the key sheets have the phone look, with each key as a big chip with an X to remove it and a round + button to add one
- Settings, DS, 3D: "Smooth 3D edges" is now "Anti-aliasing", and its choices Off, On and More read Off, 4x MSAA and 8x MSAA. A saved choice stays as it was
- Achievements: the game card shows the line your RetroAchievements profile shows while you play
- About: Licences lists every library the app ships, with its licence text
- About: Licences also names the projects that parts of Citrine's code and data come from, such as melonDS, SameBoy and mGBA, and the fonts, icons and Android libraries the app ships. The web app has the Licences page too
- Android: the release APK is 11.9 MB smaller (70.1 to 58.2 MB) because it no longer ships the app library's symbol tables. Crash reports list frame addresses, and scripts/android-symbolize.sh turns them back into names
- RetroAchievements: on Windows and Linux the sign-in token is kept in the system keyring (Credential Manager, or the Secret Service on Linux) instead of the settings file, and a token already in the file moves there at the next start. Where no keyring answers, it stays in the file as before. On Android the token moves to a file of its own that Android's backups and device transfers leave out
- Android: the secret key for nearby play over the local network and its list of known devices stay out of Android's backups and device transfers, so a phone restored from a backup makes a key of its own instead of copying the old phone's
- Cloud saves: a game in your library gets its saves from the cloud before it is opened on this device, so the first launch starts from the newest save. Before, nothing came down until the game was opened there
- GBA games run on the JIT recompiler, on by default where the DS JIT is (desktop and Android, not Windows), with their own switch: Settings, Game Boy Advance, Performance has "JIT recompiler" and "Clear JIT cache", as the DS page does. Against the interpreter, a frame of an Emerald boot, walk and battle, a FireRed walk and a Ruby walk takes 26 to 31% less time on an Intel i5-3470 with 22 to 27% fewer x86-64 instructions, and 25 to 34% less time on an Apple M6 Mac with 29 to 34% fewer instructions. On both the JIT writes the game's ARM and Thumb code as machine code itself, without Cranelift
- The GBA JIT bakes Thumb ROM literals into its loads, ends a block on ldm and pop with pc in its own code, skips the code-write check on entries that wrote nothing, and fetches code in shorter stubs: an Emerald battle and a Ruby walk run 5.3 to 6.2% fewer x86-64 instructions a frame, and those plus an Emerald boot run 0.8 to 1.0% fewer on an Apple M6 Mac
- On x86-64 the GBA JIT fetches Thumb code from ROM in a shorter stub: an Emerald battle and a Ruby walk run 2.9 to 3.4% fewer instructions a frame
- The GBA JIT runs straight IWRAM ARM code with multiplies in it as one timed fetch: an Emerald battle and a Ruby walk run 2.4 to 2.9% fewer x86-64 instructions a frame, and those plus an Emerald boot run 1.4 to 2.8% fewer on an Apple M6 Mac
- The GBA JIT fetches ARM code from IWRAM and the BIOS inside its blocks and calls its fetch stub only on a miss: an Emerald battle and a Ruby walk run 1.5 to 1.7% fewer x86-64 instructions a frame, and those plus an Emerald boot run 0.6 to 1.2% fewer on an Apple M6 Mac
- The GBA JIT's load, store and branch stubs add the clock and test the wait state in fewer instructions: an Emerald battle and a Ruby walk run 0.8 to 1.1% fewer x86-64 instructions a frame in the shipped build, and those plus an Emerald boot run 0.2 to 0.6% fewer on an Apple M6 Mac
- Desktop and Android apps: Direct connection runs on the pure-Rust crypto and no longer builds aws-lc, so the desktop app is 1.45 MiB smaller (78.1 down to 76.7 MiB on macOS) and the Android build compiles no aws-lc C or assembly
- Play together: a session message the relay drops, over its rate limit or while a player reconnects, goes again and arrives in order. A 7 MB resync past a 1 MB relay limit reaches the friend in the tests, where before its dropped pieces lost the whole resync. Play together needs 0.13.0 on both sides
- DS 3D on the GPU with anti-aliasing takes 6 to 11% less GPU time a frame on an Apple M6 Mac at 4x: a Mario Kart DS race 6.15 down to 5.45 ms, HeartGold 6.3 down to 5.9 ms
- DS 3D on the GPU with anti-aliasing takes 6 to 9% less GPU time a frame at 8x on an Apple M6 Mac, with the same pixels: HeartGold 8.88 down to 8.05 ms, a Mario Kart DS race 8.14 down to 7.61 ms. At 2x and 4x it takes up to 3% less
- DS games on a desktop with 5 or more cores draw each frame on 2 helper threads instead of up to 7: on an Apple M6 Mac (12 cores) a Mario Kart DS race takes 6% less CPU a frame (3.84 down to 3.62 ms) and Explorers of Sky 13% less (1.60 down to 1.39 ms), with the median frame time 2.47 down to 2.33 ms and 1.07 down to 1.01 ms. A 4-core desktop keeps its 3 threads, and phones are unchanged
- DS 3D on the GPU without anti-aliasing draws in a compute pass where the GPU has one, with the same pixels, and takes 16 to 23% less GPU time a frame at 4x on an Apple M6 Mac: HeartGold 1.44 down to 1.21 ms, a Mario Kart DS race 1.62 down to 1.25 ms. At 2x it takes 27 to 33% less, at 8x 12 to 16% less
- DS 3D on the GPU with anti-aliasing fills cracks between polygons in a separate small pass, so it no longer waits for the 1x pass: a Mario Kart DS race at 3x takes 3.37 ms of GPU time a frame on an Apple M6 Mac, down from 3.57 ms
- DS 3D on the GPU above 1x draws opaque polygons with the GPU's own rasteriser and takes 15 to 34% less GPU time a frame on an Apple M6 Mac: at 4x a Mario Kart DS race 1.31 down to 1.12 ms, its menu 1.55 down to 1.06 ms, HeartGold 1.26 down to 1.04 ms. With anti-aliasing it takes 7 to 15% less at 4x and 11 to 21% less at 8x: HeartGold at 4x 2.49 down to 2.30 ms, a Mario Kart DS cup scene at 8x 8.64 down to 6.99 ms. 1x is unchanged and stays pixel-exact
- DS 3D on the GPU above 1x sends each polygon to the GPU as one piece per band between its corners and takes a further 8 to 15% less GPU time a frame at 4x and 13 to 19% less at 8x on an Apple M6 Mac: a Mario Kart DS race at 4x 1.77 down to 1.51 ms and at 8x 5.61 down to 4.68 ms, HeartGold at 4x 1.73 down to 1.55 ms. On a Mac up to 14 of a 4x frame's 786432 pixels differ from before by rounding; 1x is unchanged
- DS 3D on the GPU prepares each frame's polygons for the GPU in about half the CPU time: Mario Kart DS 0.7 to 1.5 ms down to 0.5 to 0.85 ms a frame, HeartGold 1.0 down to 0.55 ms on an Apple M6 Mac
- DS 3D on the GPU with a texture filter draws polygons with transparent texels in the hardware pass and reads Bicubic and xBR taps from the decoded texture atlas, with the same pixels. In a Mario Kart DS race at 3x with smooth edges on an Apple M6 Mac, Bicubic with 4x takes 5.9 ms of GPU time a frame, down from 6.6 ms, and Bilinear with 2x 5.1 ms, down from 5.9 ms. At one GPU clock that is 36% and 20% less
- Pokéwalker: opening the Walker page no longer moves the walker to this device; Move here does
- Pokéwalker: the "Going back to <device>" card no longer shows Connect or Move here while the walker returns
- Pokéwalker: a device that turns Cloud saves off while another device holds the walker shows "On <device>" with "Cloud saves are off on this device" and no buttons, until the holder moves the walker here over the network or Cloud saves comes back on. Before, it started an empty walker of its own with 0 steps, which games could use
- Pokéwalker: Move here over a walker that has steps asks first, "Replace the walker on this device?", and names its steps and Pokémon; Replace keeps a copy in Your data. The move takes 1 tap more on that device, and none when the walker there has no steps
- Pokéwalker: a move over the network whose last message is lost now ends with the taking device holding the walker; before, a reply lost after the holder stored the move, or a taker that restarted before saving it, left neither device holding it. Tested by cutting the move at 3 points
- Pokéwalker: Connect looks for the game for 30 s, so the game finds the walker after 1 press, where the picture runs needed 3 presses before
- Cloud saves and the Pokéwalker show other devices the name chosen for this device instead of its system name
- Cloud saves: a save downloaded from the cloud is written to storage off the game's frame. In the 3 MB download test the frame that applies it took a median 0.15 ms and at most 3.95 ms, down from 3.70 and 23.0 ms
- The DS JIT looks up the next block of a jump inside its own code instead of calling back into the emulator: HeartGold runs 1.7 to 1.8% fewer x86-64 instructions a frame and Mario Kart DS 0.9% fewer, and 1.35% and 0.8% fewer on an Apple Silicon Mac
- The DS JIT runs ldr pc, DTCM reads, swaps and exception returns in its own code on x86-64, and conditional ldr pc on both x86-64 and Apple Silicon: HeartGold runs 1.5 to 1.6% fewer x86-64 instructions a frame and Mario Kart DS 0.2% fewer, and 0.17% and 0.33% fewer on an Apple Silicon Mac
- The DS JIT reads the ARM9 page table for loads in its own code on x86-64 and Apple Silicon: HeartGold runs 1.3% fewer x86-64 instructions a frame and Mario Kart DS 1.1% fewer, and 0.55% and 0.49% fewer on an Apple Silicon Mac
- The DS JIT jumps from an ARM9 block to the next linked block without saving and reloading every register on x86-64: HeartGold runs 1.2 to 1.3% fewer x86-64 instructions a frame, and Mario Kart DS 0.4% fewer in the dist build and the same in release
- The DS JIT also takes that path for ARM9 bx and blx jumps and for jumps to a block that isn't linked yet, on x86-64: HeartGold runs 0.75% fewer x86-64 instructions a frame and Mario Kart DS 0.34% fewer
- The DS JIT looks up the next block of a jump to a block that isn't linked yet without saving and reloading guest registers on Apple Silicon: HeartGold runs 0.59% fewer instructions a frame and Mario Kart DS 0.32% fewer
- The DS JIT keeps guest flags out of memory between linked blocks and checks main RAM before ITCM when it refetches code on Apple Silicon: HeartGold runs 0.88% fewer instructions a frame and Mario Kart DS 0.28% fewer
- The DS JIT checks main RAM before ITCM when it refetches code, and returns from ARM9 functions look up the next block without saving every register, on x86-64: HeartGold runs 0.45% fewer x86-64 instructions a frame and Mario Kart DS 0.05% fewer
- The DS JIT looks up the next ARM7 block of a jump or bx in the page table in its own code on x86-64: HeartGold runs 1.3% fewer x86-64 instructions a frame and Mario Kart DS 1.1% fewer
- The DS JIT runs ARM7 status register reads and writes in its own code, and finds the next block of an ARM7 jump or bx without leaving its code, on x86-64: HeartGold runs 0.58% fewer x86-64 instructions a frame and Mario Kart DS 0.5% fewer
- The DS JIT reads the ARM9 page table for stores in its own code on x86-64, and ARM9 stores to IO registers skip a call on x86-64 and Apple Silicon: HeartGold runs 0.9 to 1.0% fewer x86-64 instructions a frame and Mario Kart DS 0.58% fewer in the dist build, 0.16% more in release where the unchanged 3D rasterizer compiles differently, and 0.6% and 0.22% fewer on an Apple Silicon Mac
- The DS JIT on x86-64 jumps to the next block through a checked link cell for bx, blx, Thumb bl pairs and returns, skips the stub's refill on linked jumps and calls its runtime stubs directly, and ARM9 stores take one inlined path per width on x86-64 and Apple Silicon: HeartGold runs 2.3 to 3.0% fewer x86-64 instructions a frame and Mario Kart DS 0.63 to 1.6% fewer, and 0.38% and 0.15% fewer on an Apple Silicon Mac
- The DS JIT skips its scan for written code when it enters a block and no code page was written, and reuses the block lookup it made before a resume; on x86-64, ARM9 bx and blx jumps also take a linked cell when the target matches: HeartGold runs 0.54 to 1.4% fewer x86-64 instructions a frame and Mario Kart DS 0.66% fewer, and 1.5% and 1.1% fewer on an Apple Silicon Mac
- The DS JIT gives each block four link cells instead of two, so early ARM9 branches jump straight to their linked block; on x86-64, ARM7 jumps also take the linked path and linked jumps check the slice budget with fewer instructions: HeartGold runs 1.8% fewer x86-64 instructions a frame and Mario Kart DS 0.75% fewer, and on an Apple Silicon Mac Mario Kart DS runs 0.24% fewer and HeartGold about the same
- The DS JIT runs ARM9 ldm and stm through the page table in its own code on x86-64, and calls the emulator only for memory the table doesn't cover: HeartGold runs 1.9 to 2.4% fewer instructions a frame and Mario Kart DS 0.8 to 1.4% fewer
- The DS JIT also runs ARM7 loads, stores, ldm and stm through the page table in its own code on x86-64: HeartGold runs 0.8 to 0.9% fewer instructions a frame and Mario Kart DS 0.5% fewer
- On arm64 (Apple Silicon Macs and Android phones), DS JIT branch and exchange jumps take the linked path to the next block before looking it up again: HeartGold runs 1.9% fewer arm64 instructions a frame and Mario Kart DS 0.7% fewer
- On x86-64, a DS JIT jump to a linked block checks fewer things on the way, and one that runs out of cycles leaves at once instead of going back to look up the block: HeartGold runs 1.2% fewer x86-64 instructions a frame and Mario Kart DS 0.7% fewer
- DMA into the DS 3D command queue moves words in batches: Mario Kart DS runs 2.0 to 2.2% fewer x86-64 instructions a frame and HeartGold 0.55% fewer
- Second screens: the link between the two devices is encrypted end to end, on the same network and through the relay, and both sides prove they hold the account key, the code or the saved pairing before anything else is sent. A device in the middle that only passes messages on gets no session, and a phone with a wrong key gets no Allow prompt. A 0.12 device is told to update Citrine on both devices
- Second screens: each try of a pairing code checks one code with the device that shows it, so a device on the network can no longer work the code out from one try, and the search for that device no longer sends the code. After 5 wrong codes the code stops working and the devices list says "The code stopped after 5 wrong tries"; Add a device makes a new code. A phone that lost the connection keeps its pairing when a message that didn't come through the encrypted link says it was removed
- Second screens: a phone that lost the connection comes back with a key both devices worked out when it joined, which never crosses the network, so someone who copied what the host sent it can no longer take its place. Coming back after a drop takes the same steps as before and asks nothing. A device on the previous test version is told to update Citrine on both devices
- Second screens: a device that asks to join shows a 5-digit security code with "Check that <host> shows the same digits", and the host shows the same code above Allow with "Allow if <device> shows the same digits". While it waits, its Join button becomes Cancel. A device listening in between gets other digits on each screen and can't choose them, because the phone fixes its part of the code before it connects. A host takes at most 10 asks in 10 minutes from one device and 30 from all devices together, then says "Too many tries. Wait 10 minutes"
- Storage, Your data: with nothing stored, Delete everything and Export first are off
- Big screen: a confirm sheet, such as Delete everything or a second screen asking to join, starts the controller on Cancel or Not now, with the action one press up. The grab handle is no longer a stop, and moving through a sheet no longer scrolls the page behind it
- Confirms: the question is a bold heading and the sentences after it sit under it in normal text, on desktop and phone, so a long ask such as Forget a server reads as a question with a note instead of a block of text. Notes under a confirm are no longer grey, a confirm's wrapped lines are balanced so no word sits alone on the last line, and Delete everything says "Your online account and cloud copies stay." On the phone the heading is 20 pt over 14 pt notes, sits 4 pt over its note and 20 pt over the buttons, Delete everything lists Export first, Delete everything, Cancel in the same order as on desktop, and the first Tab in a confirm lands on Cancel. On desktop a focused confirm button keeps its own colour with a 2 pt ring outside it (it turned the grey of the dimmed screen before), every button has 12 pt on each side of its label, "Don't ask again" sits 20 pt under the note and 12 pt over the buttons, and the card has 10 pt of padding on every side, so the ring clears its edge by 6 pt. On the phone a focused button keeps its size, and stacked buttons sit 12 pt apart (8 pt before), so the pad ring sits 2 pt from its own button and 7 pt from the next.
- Confirm buttons: on a phone Cancel is a full-width button like the confirm's other buttons, so Delete everything no longer mixes a grey bar (Export first) with a grey pill (Cancel). On desktop the focus ring around a confirm's checkbox row is the height of a button ring and leaves 12 pt beside the box and the text, like a button's label
- Phone switches: an off switch in light mode has a 1 pt edge in the theme's box edge colour, at least 3:1 against its sheet or card, so it no longer looks disabled. In dark mode an off switch's knob is grey and turns white as it switches on. Settings pages, This game, You, Cheats, the Reset confirm and the switch rows in phone menus all get it
- Dark theme: grey text on phone cards and sheets, such as the "Security code" label, has at least 4.5:1 contrast
- Desktop: the top bar is three groups with labels. Games and Open sit at the left. With a game open, pause, rewind, fast forward, a Saves menu (quick save F8, save to a new slot Shift+F8, quick load F9, all saves), screenshot, record, mute and a This game menu (game info, cheats, the DS lid, link, reset, ROM info, Save a bug report) follow. Friends, Media, Settings, Focus and More sit at the right. Games, Friends, Media and Settings light up while their pane is open; a click brings a covered pane forward and closes an open one. With no game the play buttons are gone instead of greyed out, and while playing together rewind and fast forward are hidden. A narrow window drops labels first, then moves the least used buttons into More. "Save game report" is now "Save a bug report"
- Play together: a DS game eases its input delay down to the current ping after it moves to a direct connection, and counts waits toward a step up only when the ping asks for more delay. In the tests a direct path of 15 ms each way ends at 4 frames (67 ms) instead of 8 (133 ms), 45 ms each way at 6 instead of 8, and the relay at 45 ms with 45 ms of jitter at 8 instead of 10, all with no waits
- Play together: when the ping to a DS friend jumps, the input delay jumps with it in one step instead of one frame at a time, holds through the bad spell, and goes back to where it was once the ping settles. In the tests a 10 s spell of 135 ms more ping each way pauses the game 39 times a minute through the relay and 17 on a direct connection, down from 287 and 227, and the delay is back 1.7 to 2.0 s after the spell instead of 7 to 14 s
- Play together: after a bad ping spell with a DS friend, the input delay comes back down once and stays there, and a single late frame no longer raises it unless the ping really jumped. In the tests the delay is back to normal 1.5 s after a 135 ms spell ends through the relay and 2.0 to 2.8 s after on a direct connection, with no second rise.
- Shortcuts: keyboard shortcuts sync to your other devices and controller shortcuts stay on each device. Shortcuts stop syncing between 0.12 and 0.13 devices until both run 0.13
- DS Wi-Fi: the switch that skips the relay is now "Connect without the relay", so it no longer shares its name with Play together's Direct connection
- Desktop library: the game tiles sit 12 pt under the filter row instead of 3 pt. Tiles grow from 100 to 124 pt wide, then the gaps between them widen up to 20 pt before the tiles grow again, so a new column changes the tile size less when the pane is resized: 30 pt instead of 38 from 3 to 4 columns, 24 instead of 27 from 4 to 5
- Library and Recent games: a long game name takes two lines on its tile, and line one ends in "…". When names on the page begin alike, line two shows the part that differs, as "The Legend of…" over "…Oracle of Ages" beside "…Oracle of Seasons". Before, each of them read "The Legend o…". A row where every name fits keeps one line, and hovering a cut name shows it whole
- Stats: the Time played card under the chart shows the period picked above it (this week, month or year, or the weeks in the calendar) or a day picked in the calendar: its play time, the games played most, sessions, snapshots, link cable and time online. The x on the card or Esc clears a picked day. Playing together and Snapshots have their own all-time sections
- Desktop stats: Week, Month, Year and Calendar sit in a segmented control, and the page keeps one width whether or not it scrolls, so the scroll bar no longer covers the numbers on the right
- Play together, DS: when the round trip to the friend is 120 ms or less, the input delay is one frame lower, with no more waits: 4 down to 3 frames over the relay at a 75 ms round trip, 3 down to 2 on a direct link at 22 ms. Slower links keep the delay they had
- File sizes show one decimal in MB and GB everywhere ("2.0 MB"), and the Storage page's heading totals equal the sum of the sizes shown under them
- Storage, Your data: saves and save states, screenshots and recordings, game names, recent games and game copies have rows of their own, and Other data files shows when it holds anything. Export, Import and Delete everything act on every row; game copies are deleted but stay out of the export, and Delete everything's ask says so
- Web: Storage counts game copies. Copies from before this version are measured once in the background, and their row reads Counting… until then
- Big screen: the hint bar shows Change only on a row that left and right change, and names what the confirm button does on Storage rows, such as Clear
- Desktop: the window layout file moves from eframe's folder into Citrine's data folder, so Storage counts it under Settings
- Asks that delete, forget, reset, clear, replace or quit open with Cancel focused when the focus was already on a control (keyboard or pad), so a second press of the same key never confirms them. Asks that lose unsaved progress say "Unsaved progress is lost." and open on Cancel too
- Chat: on a phone, the emoji panel opens in the keyboard's place and at its height, as in WhatsApp, and the keyboard button beside the field brings the keyboard back. The message field stays where it is while they switch
- Chat: on a phone, the messages move with the keyboard as it opens and closes, so the newest message stays in view. The place bar steps aside while the keyboard or the emoji panel is up. Turned sideways, the side rail stays with the emoji panel and with the keyboard; above the keyboard it shows its 5 entries as icons without labels so all of them fit, and the message field keeps its place when the two switch. Turned sideways with the keyboard up the header shrinks to one 28 pt line with the back arrow, the friend's avatar and name. A date in the chat sits 12 pt below the message before it and close to its own message
- Chat: the emoji panel's rows fill its whole width with more columns, instead of 8 columns and an empty strip on the right, and the panel ends on a whole row instead of half a row. Its card runs down to the screen's edge, under the gesture bar, as the keyboard does, instead of ending 34 pt above it, and its rows sit with the same space above the first, between each row and below the last, down to the top of the gesture handle
- Play together, DS: each side sets its input delay from how long its own machine takes to replay a frame, so two Macs over the relay at 45 ms each way with 15 ms jitter start at 4 frames of delay instead of 6, and your own presses show on screen after 67 ms instead of 100 ms (Mario Kart DS race, measured in a test). Other computers, the web app, phones, and a Mac playing one of those keep the delay they had. During play each side times its own frames and rolls back fewer frames when they run slower than that
- Settings on desktop: where the window leaves room, the controls on every page start at one edge, the same on each page, so switching pages no longer moves them. Every slider's value box has one width, so sliders end at one edge too, and no select or level bar runs under the reset buttons. UI scale reads as percent, like 150%. Long labels are shorter, with the detail in their info mark where it needs one: "Count in-game steps", "Friends see what I play", "Popups in focus mode", "If the game runs slow", "Use light sensor", "Use firmware user", "Shot every" and "DS screens" under the Discord screenshot, "Both", "Top" and "Bottom" as the choices of "DS screens" and of "Screens shown" on the DS page, "GBC boot ROM", and "Screen colours" on the Game Boy page, the name the same setting has on the GBA page
- Phone: a page cut off above a notice fades out at its end
- Saves: Other battery saves is a heading like Snapshots and Autosaves
- Saves: snapshots, autosaves and other battery saves show the date and time they were saved, where snapshots said "just now" or "5m ago", and Battery save says when it was saved. Every time on the page shows seconds when two of them share a minute, and Delete on an autosave asks "Delete the autosave from <time>?" or "Delete <friend>'s autosave from <time>?", with the time as its row shows it
- Phone: a delete question on Saves moves the row it asks about above the question, where the question could cover it
- Saves: notices name a snapshot or autosave without quotes, such as "Deleted Mika's autosave from Nov 16, 16:23:53", where the quotes ran into the apostrophe in a friend's name
- Saves: while a delete question is open, the row it asks about stays bright and only the rest of the screen dims, on phone and desktop, so two rows with the same name can no longer be mixed up
- Desktop: a popup with buttons takes one line when the game pane is wide enough, where its text wrapped or its buttons sat on a second row

## Fixed

- DS 3D: a phone whose GPU driver can't build one of the 3D shaders keeps the game running. The 3D moves to the GPU path without compute shaders, or to the CPU when that fails too, and the log names the failed path. Before, the app closed when a DS game first drew 3D, as on a Galaxy S25 Ultra
- Recordings: a replay recorded on a screen faster than 60 Hz plays back the same game to its last frame. Before, the clock catch-up of the update before Record went into the replay a second time and the one after the last frame was missing, so a Mario Kart DS replay recorded at 180 Hz went another way at every check from frame 14 to 1874 and at its end
- Recordings: a DS game keeps running on its own thread and drawing 3D on the other cores while Record runs, as it does without a recording. Before, Record moved it onto the window's thread
- Recordings: a DS replay with the 3D on the GPU plays at 1.5 to 1.6 ms a frame on an Apple M6 Mac for Mario Kart DS, down from 2.1 to 2.4 ms. Before, playback drew every 3D frame in software as well as on the GPU
- Settings: a settings file with a value this build can't read keeps every other value, the online sign-in included, and only that value goes back to its default. A file cut off part way keeps the values before the cut. The unreadable file is kept once, next to settings.json, as settings-0.13.0.unreadable.json. Your data imports settings the same way. Before, one unreadable value reset all settings and signed you out
- Link cable: Change keys opens Settings, Controls scrolled to "Two players, one keyboard", on desktop and phone. Before, it opened Controls at the top
- Recent games: a game in a folder whose name ends in .zip shows that folder as its folder. Before, it showed the folder above it
- Library and Recent games: a zip replaced by another of the same size within 10 s lists the new zip's games. Before, it could keep the old zip's list
- Edit layout: DS screens moved on desktop keep their arrangement when the game pane changes size and in focus mode (F11). Before, the moved screen and the other one scaled apart, so they overlapped at 1920x1080 and 1280x520 and drifted at 900 and 700 wide, and focus mode dropped the moved layout. On-screen buttons and HUDs keep their distance to the nearest edges when the room changes size
- Saves: a game inside a zip imports its old .sav from the zip's folder, once, as a loose game does: the .sav named like the game (game.sav for pack.zip/game.gba), or the one named like the zip (pack.sav) when the zip holds only that game. Before, its .sav was never imported
- Recent games: a game inside a zip shows the folder the zip sits in, in the Link cable and Play together game lists. Before, it showed the folder inside the zip or the zip's own name
- Library and Recent games: a game taken out of its zip counts as missing, while the zip's other games stay. Before, it counted as present as long as the zip existed
- Library: a game inside a zip shows its name and cover, as the same file outside a zip does, up to the same sizes (64 MB, 512 MB for DS). Before, it showed its file name and no cover. Its saves and cover are the same as the unzipped game's. Recent games get their names from games in zips too, Show in Finder (Files, Explorer) shows the zip, and Play together finds a partner's game inside a zip
- Library: DS games over 64 MB, such as the 128 MB Pokémon games, show their name and cover. Before, the library stopped reading files at 64 MB, so they showed their file name and no cover. DS files up to 512 MB are read now, one big file at a time; Game Boy and GBA files keep the 64 MB limit
- Library: a genre shows one name whichever source it came from. Before, a game whose genre came from Wikipedia showed it in Wikidata's words, such as "Role-playing video game" for Pokémon Platinum, beside "Role-playing (RPG)" on other games
- On Apple Silicon the GBA and DS JIT sets the carry flag after a flag-setting logical instruction with a rotated immediate whose top bit is clear, such as `teq r0, #0x10000000`, when the next instruction sets the other flags and reads the carry (adcs, sbcs, rscs). It kept the old carry, so the result came out one too high in 16 of 360000 random blocks of the JIT check
- HUDs: "More HUD settings" opens Settings, Screen at its HUDs section; it opened the DS settings
- Settings on desktop: on the Game Boy page the palette picker, its colours and the New palette and Duplicate buttons start at the dropdowns' left edge. Before, they did not line up with the dropdowns
- Big screen: a Settings page fades out above the button hints at the foot of the screen and, once scrolled, under the title at its top, and the focused row scrolls clear of both. Before, the page was cut off hard at both edges
- Security: when a friend's game state replaces yours during Play together, everything the game saves (battery save, autosaves, snapshots) goes to a separate file and your own saves stay as they were. A notice asks whether to use the friend's saves (yours stays in Saves under Other battery saves, with the name you gave it or as "Your save before <friend>'s", with Undo) or delete them, and it comes back after a restart until you answer. Closing the notice keeps your save. Either way the friend's saves show in Saves under their name ("<friend>'s save", "<friend>'s Gym", "<friend>'s autosave"), and their autosaves list with yours under Autosaves, newest first, on every device the saves sync to; your own autosaves never push them out or get pushed out by them. A DS game synced from a friend no longer hands over its Wi-Fi connections
- Security: a GBA or DS snapshot whose memory sizes differ from the console's, from a file, rewind or a player in Play together, is refused as damaged. Before, the JIT took the snapshot's sizes, so a crafted one let game code read and write the app's memory outside the console
- Fast forward reaches its speed on a fast screen. It ran each refresh's share of frames only on the refreshes where a normal-speed frame was due, so a 180 Hz screen crammed three frames into a third of the refreshes and 3x ran at about 2x. On an Apple Mac at 180 Hz, Emerald at 3x now runs 2.97x (1.55x before) and at 5x 4.76x (1.50x); at 120 Hz 3x runs 2.99x (1.94x). The HUD's speed shows the speed reached
- The sound counter no longer reports underruns after fast forward or a pause. Sound stops during both, and the queue's refill to its latency afterwards (80 ms by default) was counted as the sound running dry, so a recording showed one underrun burst for each fast forward
- Games judder less on screens whose refresh rate macOS or Windows reports. The app worked the refresh rate out from its own update times, and late updates could pull that estimate 2-3% off, which dropped the screen match (a 59.73 fps game shown at an even 60 fps) and held a frame for one refresh too long one to six times a second. It now uses the reported rate unless updates come more than 10% faster than that rate. Play together and linked games keep the old timing
- macOS: on a screen that refreshes faster than the game runs, the app skips the window updates that bring no new frame. A Game Boy game on a 180 Hz screen updates the window on 2 of every 3 refreshes instead of every refresh
- Game Boy and Game Boy Color games judder less on screens of 120 Hz and faster. The window showed each frame in the same update that started it, so it sometimes waited for the console, and after such a wait the frame often stayed on screen one refresh too long: in a Pokémon Blue recording on an Apple Mac at 180 Hz, the window waited on 6.7% of frames, and 70% of those waits came before a frame held too long. A frame now shows one refresh after it starts, as GBA and DS frames do when they need it, so the window never waits for it. The picture comes one refresh later, 5.6 ms at 180 Hz. At 60 Hz, with a link cable, in Play together and in fast forward nothing changes
- macOS, DS: games on screens of 120 Hz and faster drop and repeat fewer frames when the 3D scene is heavy. While the core was still out on a frame, the window slept until the next frame was due and sometimes picked the frame up a refresh late; it now checks on every refresh again, as in 0.12.0. In a 381 s Mario Kart DS run at 180 Hz on 0.13.0, 102 frames dropped and 2096 repeated, against 2 and 497 in 658 s on the build before
- HUD: FPS counts the frames that reached the screen. Before, it counted emulated frames, so an update that ran 3 frames counted 3 and the HUD read 60 through a stutter. In that Mario Kart DS run the worst 2 s windows now read 55 to 57
- Games stay matched to the screen in heavy DS scenes and after a slow stretch. A run of slow frames turned the match off, often for the rest of the session, and the app then held frames for an extra refresh far more often. In a Mario Kart DS race recorded on a 180 Hz Mac, the match now holds the whole race, where before it held 42% and switched 68 times. Frames shown one refresh too long drop from 586 to 354 a minute. In Pokémon Emerald the match holds 99.6% of the time instead of 6.5%, and those frames drop from 67 to 13 a minute. A single slow frame also no longer delays the picture by one refresh for the next second
- Desktop: a game no longer stalls for a frame every 30 seconds. The app kept its window, layout and panel state every 30 seconds in the middle of play, which took 22-32 ms on an Apple Mac at 180 Hz. It now keeps them while no game runs and on close; game saves, settings and stats still save on their own schedule
- Security: the JIT cache on disk is signed with a key only this install knows, and a file that fails the check or is over 64 MiB is dropped. Before, anything that could write to the cache folder could hand the JIT host code to run
- Snapshots: a snapshot, rewind or rollback state saved by another game is refused with "It belongs to another game." on Game Boy, GBA and DS. Before, it loaded into the running game. Snapshots from older versions load as before
- Security: a damaged GBA snapshot, rewind or rollback state, from a file or a player in Play together, no longer crashes or hangs the app. Out-of-range interrupt queues, screen and sound counters, prefetch counts and clock values are cut back into range as the state loads. Before, some of them crashed the app and a huge clock value froze it.
- Security: a damaged Game Boy snapshot or rollback state, from a file or a player in Play together, no longer crashes the app. A state that claims another cartridge's ROM or RAM size keeps the loaded game's sizes, and an out-of-range master volume or screen counter is cut back into range as the state loads.
- Security: a snapshot or save file that unpacks past 64 MiB is refused as damaged, and a list inside a snapshot that claims billions of entries reserves at most 1 MiB until its entries actually arrive. Before, a small crafted file could make the app take all of the computer's memory and close. A snapshot picture that unpacks past its size is dropped and shows blank.
- Achievements: for a game file RetroAchievements doesn't recognise, such as a region with no set of its own, the Achievements tab says "This version of the game has no achievements" instead of "This game has no achievements yet"
- Stats: a game that was opened but never played says "Not played yet" in the games list instead of a last played date in 1970, and its page leaves out the first and last played dates
- Desktop: the Games button, a menu entry or a notice's button brings its tab to the front with the focus, also when the tab is open behind another one or the Games pane was closed. A tab opened while the tools sit in a floating window opens in that window instead of over the game
- Settings, DS: the ARM9 BIOS, ARM7 BIOS and firmware rows keep showing their file names after a game closes
- Notices: with no game on screen, a notice no longer covers the page; the page makes room for it at its foot. On a phone, a notice from a sheet shows above the sheet instead of behind it. On desktop, a notice on a page widens to keep its text on one line
- Settings: the dropdowns of a group share one width and one left edge; a file name too long for its dropdown shortens in the middle, keeping its extension and the letter before it, so its label keeps one line, and shows whole on hover
- Your data: every count says what it counts ("0 saves", "0 files"), and on desktop the delete button reads "Delete everything" in the phone's red, a darker red in light mode that reads on the button. Saves and the records the app keeps sit under "Saves and app data", above "BIOS and boot ROMs". "DS wifi connection" is now "DS wifi". On a phone every row is one line: a count row shows "0 saves, 0 B" beside its name, a BIOS or boot ROM row its size or "None"
- Big screen: walking down a settings page past an info mark goes on to the next row, where the mark's hint took the focus and the page jumped back to its first row
- Big screen: the scroll bar is no longer a stop, so Right from a row's last control stays on the row instead of lighting a 2 pt sliver at the screen edge
- Big screen: the highlight ring follows its element: a game tile rings its cover without the title, a page dot gets a round ring, and an info mark's ring sits centred on the mark without touching its label
- Big screen: Down from a full-width row, such as the Pokéwalker, goes to the first game in the library's left column, where it jumped to the Filter button at the right end and then the right column
- Big screen: Down from a game on the Recent shelf goes to what lies below it, where the first game went to the library's second column
- Recent games: a game names the console the library identified, as its library tile does, where a Game Boy Color game with a .gb file said Game Boy when its file was missing
- Volume and the other sliders have a knob ringed in the app's colour and a darker empty track, where a plain white knob and the track's far end were lost on a light panel; in the game menu the Volume track ends where the values above it end
- Big screen: choosing a page in the games list keeps the focus on the page numbers, where it jumped back to the filters at the top
- Big screen: Down from a game in the left or right column reaches the page numbers under the games, where it skipped them and landed on the row below
- Big screen: the pad skips a page arrow that does nothing, such as Back on the first page, so Down from the games lands on a page number
- Big screen: the button hint says Choose on every page, where the pages said Open and the menus Choose for the same button
- Game menu: Resume and Save state have no arrow, since neither opens a page
- The dim behind a sheet or menu is lighter, so yellow under it stays yellow, where it turned olive in light mode and brown in dark mode
- Big screen: Back from a settings page lands on the row that opened it, and L1 and R1 come back to the row you left, where both went to the first row
- Big screen: L1 and R1 in the game menu move the focus into the new section, where it stayed on Games in the left bar
- Big screen: an empty Media page says to press Home, where it named the phone's More button
- Big screen: a sheet opens with the focus on its first control, where the grab handle at its top took it
- Big screen: Left and Right move a sheet's slider (Volume, Latency, Clock, UI size and the rest), where only a finger or mouse could
- Game menu: a Volume slider takes the place of the Sound row, so Left and Right or a drag set the volume and 0 turns the sound off, where the row could only turn it on and off
- Big screen: an open sheet's button hints say Choose and Close, where they kept the page's hints, such as Search on the Games page
- Big screen: for its first 6 seconds a game shows the pad's Select and Start buttons with "Hold for menu". The buttons carry the names that pad prints on them. The hint stops once the game menu has been opened. Before, a player whose Home button Steam or macOS takes had no sign of the menu
- Big screen: the pad keyboard's Done hint calls Start by the pad's own name (Menu on Xbox, Options on PlayStation, + on Switch), where it always said Start
- Big screen: the left face button opens the focused game's sheet (tags, details, Remove), which only a long press could open
- Big screen: the left face button shows the hint behind a settings row's info mark, which the pad could not reach; Back or moving on hides it
- Big screen: the shoulder buttons go to the previous and next picture or recording in the viewer, where they switched the page behind it; Left and Right move a recording's seek bar by 5 seconds, and the button hints stay in view under the viewer
- Big screen: the game menu has every entry the phone's More menu has (Record video, HUD, Link a game, Achievements, Cheats, Stats, Game report, Play on another console); Settings is its own tab
- Big screen: the game menu turns the sound off and on, and for a DS game closes the lid and swaps the screens; with a linked game it switches between the two
- Big screen: a settings hint with no room under its info mark opens above the mark, where it covered the next row and ran past the foot of its card
- Big screen: the places are tabs across the top (Games, Media, Settings, You) with L1 and R1 shown at the ends of the bar, where a bar down the left side took the page's width. In a game the game is the first tab, left of the places, which stay where they are, and picking it returns to the game with its menu open
- Big screen: Home outside a game opens the Home menu: Friends, with a dot when a friend waits, Volume, and Quit Citrine except on the web. Start opens it too, for pads whose Home button Steam or macOS takes
- Big screen: the library shows 4 games a row, where it showed the phone's 3. Its tiles have the Recent shelf's white card and round corners, where they were grey with an outline
- Phone and big screen: Game covers, Progress in the library and Check at start explain themselves behind an info mark, where each had a second line under its name
- Big screen: Record video sits right after Screenshot in the game menu
- Phone and big screen: ROM info, the cartridge header, is in the More menu and the game menu, where only the desktop app had it
- Big screen: the focused row or menu entry has the ring the tabs and buttons have, drawn inside its edge, where a pale fill was hard to find and a yellow one looked like the current tab
- Big screen: Search, Open a game and the media filter sit at the right end of the tab bar
- Big screen: a game with no cover names its console in 14 pt text in the library and on the Recent shelf, where small grey text could not be read from the couch
- In Play together, the HUD, the Controller pane and a phone used as a second screen follow the console of the game being played, so a DS session after a GBA game shows the DS values and buttons, and the second-screen card shows the session's game, also to a player who joined without a game of their own
- Desktop: the HUD button beside the game's tabs no longer runs under the next pane in a narrow window. The tab titles shorten to make room, and in a very narrow pane the button drops its word
- Desktop: a narrow playing-together row shows the ping in ms instead of only the word "Good"; the word is in its tooltip
- A DS game no longer freezes when it fills the 3D command queue right after a buffer swap; the write now waits until vblank
- Server: a sign-up or other write that finds the database busy with the hourly tidy or backup now waits up to 15 s, where it waited 5 s and then failed with a server error. Under heavy disk load the tidy at server start took up to 6 s and failed sign-ups in the tests
- Server: stopping the server now waits until the relay connections and their DS online streams, the hourly tidy and backup, push notifications and second-screen links have ended and the database is closed, where it returned while they still wrote to it. Connections that haven't ended after 10 s are cut
- Android: push notifications reach the phone. Firebase refused every one with 400 Bad Request, because the server named the sender under "from", a key Firebase keeps for itself; it is "sender" now. Needs the 0.13.0 server and the 0.13.0 app together
- Server: push notifications log why each one went or didn't: "went to 1 of 1 phones", no phone registered, the kind switched off on every phone, or Firebase's own reason when it refuses, such as SENDER_ID_MISMATCH for an app from another Firebase project than the key. Each phone that registers logs which kinds it takes. Before, a push that didn't arrive left no line, or only "403 Forbidden"
- Big screen: scrolling a long page with the right stick keeps the highlight on a card in the same column, where it could jump to the side rail or the scroll bar and the triggers then stopped paging
- Big screen: a sheet dims the whole screen above the button hints, as the Home menu does, and leaves only the current place's tab lit. L1 and R1 hide and do nothing until it closes, where the top bar stayed bright under a sheet and L1 and R1 changed place under the Home menu
- Big screen: when the game menu has more rows below, the top of the next row shows at its foot and fades out, where the list stopped on a whole row and looked complete
- Big screen: library game names are 16 pt, the size of the Recent shelf's, and sit 5 pt under their tile, clear of its highlight ring, with 24 pt between rows, where the ring touched them and a name sat as close to the next row as to its own
- Big screen: the Filter button's ring stays inside the games column, with even room on both sides of its label
- Discord shows the screen of GBA and DS games, 3D included. The app shrinks a picture over the relay's 64 KB instead of dropping it and logs when even the smallest one is too big; the relay takes screens of any shape up to 1024 px a side, where it took only the Game Boy shape
- Desktop app on Windows and macOS: Settings and recordings name the refresh rate of the monitor the window is on, read from the system, and follow the window to another monitor. A window on a 60 Hz monitor next to a 240 Hz one draws about 60 times a second, where Windows vsync paced it at the 240 Hz monitor's rate (214 draws a second in a recording) and Settings named that monitor's rate
- Games tab: the app no longer slows down while no game runs. With 5000 games an idle library frame takes about 1 ms, where it took 7 to 17 ms, and while 200 covers download a frame takes 1 to 6 ms, where it took 85 to 145 ms with single frames up to 390 ms. The library read, decoded and uploaded every visible cover again on each frame, and sorted the whole list again on each frame
- With no game loaded, the app no longer draws on every screen refresh: an idle home screen, Friends page or open chat now draws about 20 times a second, as a paused game does, where it drew 180 times a second on a 180 Hz screen. An open chat draws only the messages in view, so a frame with 200 messages takes about 3.4 ms in the test build, where it took 20 ms, and all 2000 messages of a long chat take 3.7 ms
- DS online through the Citrine relay: a game save that first went online through Kaeru WFC or another server no longer stops with error 60000; the relay keeps the profile the save holds, and only the first account that brings a profile can use it
- DS online: a game save moved from another device or emulator no longer warns that it was registered on another DS; Citrine answers the Wi-Fi user ID the save holds and, the first time, restarts the game once during its boot screens so the game reads it
- Achievements: the tab no longer stays on "Loading achievements". A server that doesn't answer says "Can't reach RetroAchievements" and a server error says "Achievements didn't load", both with Try again
- A DS game with 3D on the GPU no longer crashes the app on a GPU without storage textures, such as an OpenGL-only Linux desktop
- On a phone in landscape, the DS touch screen takes taps again, where the on-screen buttons took every tap between their two columns, the screens included
- Games tab: the app no longer stays slow for a minute or more after you add a big game folder. 5000 new games are identified in 1.3 to 1.8 seconds, where it took 83 to 85 seconds, and while that runs a frame takes 1.8 to 4.8 ms on average, where it took 3.4 to 7 ms. The library now reads several game files at once away from the screen thread and adds them to the list in batches, and it loads game info sheets away from the screen thread too
- Settings: a DS game no longer shows the Game Boy sound channel switches; each console's page lists its own channels
- Settings: Save game report shows for any loaded game; on desktop it was missing for DS games and for GBA games without cartridge extras
- Settings, Sound: the output device that follows the system reads "Same as system", the name the microphone and the theme use, instead of "System default"
- Settings on a phone, App: Game covers, Progress in the library and Check at start are one line each, with what they do behind their info mark
- Settings on desktop, Online: "Leave this relay" and "Delete account" sit under their own heading, "Leave or delete", instead of under Cloud
- Progress bars (the microphone level, storage, achievements) draw their empty track at 1.5:1 contrast or more, against the page on desktop and against the card on a phone, in light and dark. On a phone the track was 1.13:1 in light and 1.24:1 in dark
- The boot ROMs are "GB boot ROM" and "GBC boot ROM" on the Game Boy settings page and in Your data, the Open menu, the phone's BIOS and boot ROMs sheet and the notices about it
- Settings, Screen: desktop Scaling sets the desktop layout's scaling, and the touch layout's has its own row, "Scaling with buttons" in Controls, On-screen buttons; before, one label changed two fields
- Settings: the desktop UI size row the touch layout never reached is gone
- Chat: a picture whose PNG header claims a huge size no longer crashes the app each time the chat opens. The app refuses a picture over 8192 by 8192 pixels, or over 4096 by 4096 in a chat, before it takes any memory for it
- Chat: a download stops with "The server couldn't send the file" when the server announces another size than the shared file's, or sends more bytes than that size
- Chat: the history keeps the newest 128 MB of messages. Past that, the oldest messages of the largest chat go first, so a friend who floods a chat can't stop every chat from loading
- Chat: read ticks under a message of only emoji showed in dark brown, which vanished on the dark page, on desktop too. The time and ticks of such a message now sit on a small chip in the bubble's colour, as in WhatsApp, so read ticks have one colour everywhere and stand apart from delivered ticks in every theme. A deleted message shows its time without ticks, after "Deleted" as on every other bubble
- Phone, dark mode: the border of a text field being typed in, the chat's message field among them, was dark brown on the dark page, and selected text in the phone's settings fields was amber on amber. Every phone field now has the amber border and keeps dark text on the amber selection
- Chat: on a phone, messages no longer draw over the emoji panel or the message field while the keyboard is open, and the emoji panel no longer stacks on top of the keyboard
- Online: the app drops the connection to a relay that sends 256 MB more than the app has read, and on desktop and Android it refuses a relay message over 1 MB, where before it took up to 64 MB
- Play together: the app ignores a link group or a watch start that names more than 4 players, a session ends when a friend's messages waiting for a game to load pass 64 MB or 16,384 messages, and messages a friend sends out of order hold at most 64 MB
- Second screens: a phone refuses a host's picture over 1024 by 1024 pixels and a game icon whose pixels don't match its size. A host keeps at most 16 connections that haven't said hello, 4 from one address, and answers at most 32 nearby searches a second
- Server: one account keeps at most 64 MB of sent chat messages on the relay (CITRINE_CHAT_HISTORY_MB). Past that the relay drops the account's oldest messages that every device has received, and refuses new ones only while 64 MB of them still wait for delivery. Before, one account could fill the server's disk
- Server: wrong admin passwords, admin two-step codes and Second Screen codes count per IPv6 /64 network, so a caller can no longer get new tries by changing addresses inside its network
- Server: a connection that never signs in, a Second Screen link that stops reading, and a client that only sends pings are closed after their timeouts, so they can't hold the relay's 500 connection slots or fill its memory
- Server: oversized Playing and invite messages are refused instead of cutting the friend's connection; GTS tokens are capped at 16 per account so one account can't block the GTS for everyone; upload quota counts all of an account's devices together; an account keeps at most its 64 most recently used devices
- Friends: the app now remembers each friend's chat key the first time it sees it, so a server can no longer swap in its own key to read your chats. A friend's card shows a security code to compare in person or on a call; if the key changes, the friend shows "Security code changed" and chats and Play with them stay paused until you compare the new code and tap Digits match. Friends you already had are remembered as they are, with no warning
- Achievements: a damaged or crafted .nds file no longer crashes the app when the library scans it
- DS files tab: a crafted DS game no longer hangs the app or fills its memory with file names, nested archives, sound names or a huge picture size
- Your data: importing a crafted bundle no longer fills memory or crashes the app on its next start. An import now keeps this device's DS online server, Discord and update check settings, and imported cheats arrive turned off unless the same code is already on here. Recent games on network paths are skipped
- Battery saves: a save file over 32 MB, or one that isn't a regular file, is no longer read, so a crafted folder can't stall or fill the app's memory
- Games in a .zip: an entry name can no longer point outside the zip, such as to a network share
- Play together: a Game Boy link partner or a relay can no longer grow the app's link queue or room member list without limit
- Cheats: a Game Boy or Game Boy Advance code with a huge repeat count no longer freezes the game
- Second Screen: a crafted picture from the host can no longer make the screen device run out of memory and close
- About: Licences has a "Nintendo WFC server list" section with the GameSpy SDK licence (BSD 3-clause, IGN 2011), whose server list cipher the DS online stand-ins follow
- DS online: Mario Kart DS no longer stops with error 85030 when it looks for opponents through the Citrine relay. Two players on the relay now find each other, pick karts and a course and race together
- Web: stats, the Pokéwalker, chats and the RetroAchievements sign-in are kept after a reload. Each start used to move them where the app no longer read them; what 0.12.0 moved comes back once on the first start
- Delete everything also removes loose files in the cache folder, not only its folders
- Second Screen: when a phone stops reading and the server lets it go, the host keeps its door for the next phone. Before, the server could also close the host as refused for the frames that queued up meanwhile (2 of 30 test runs under load on Linux). A host or phone that sends more than 2 MB a second is now slowed down instead of cut off
- DS online: a Mario Kart DS match on the relay no longer fails when one match-up packet is lost on the way; the stand-in sends it again until the DS answers, and a DS now hears the other DS from its first packet
- Relay server: a DS can send to another player's DS only after the two met through the match list, a match message or a match-up, no longer to any player's address
- Recordings: a clip cropped from a replay keeps the frame times of its range, counted from the clip's start, so the graph above the seek bar shows them. Before, the crop dropped them, and a crop of the whole replay did too
- Recordings: a clip or recording with a button tapped on back-to-back frames, such as a touch button mashed on a phone, plays back the same as it was played. Before, the replay missed the let-go between two taps, and a Game Boy game went another way from the second tap on

# 0.12.0

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
