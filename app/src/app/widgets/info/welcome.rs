pub fn build() -> String {
    format!(
        r#"
# Welcome to Citrine v{}
Thanks for using my emulator! If you find any issues, please report them on [GitHub](https://github.com/Zitronenjoghurt/citrine-gb/issues).

Native version: [GitHub](https://github.com/Zitronenjoghurt/citrine-gb/releases/latest)\
Web Version: [Citrine Web](https://gb.lemon.industries)

---

# Stability
This emulator is still in **active** development. Things might **change or break** at any time.\
While there is a certain level of stability, it is not guaranteed.

---

# Controls
You can remap every binding below in **Settings → Controls**.

**Keyboard** (default)

| Action     | Keys             |
|------------|------------------|
| Start      | Enter, Space     |
| Select     | Backspace        |
| Directions | WASD, Arrow Keys |
| A          | Y, Z, Q, O       |
| B          | X, E, P          |

**Controller** (default)

| Action     | Buttons                         |
|------------|---------------------------------|
| Start      | + / Start                       |
| Select     | - / Select                      |
| Directions | D-Pad, Left Stick               |
| A          | East / West (Nintendo: A / Y)   |
| B          | South / North (Nintendo: B / X) |

**Touch**

On phones and narrow windows, Citrine switches to a touch layout with an on-screen
d-pad, A and B, and Start and Select. The d-pad reads diagonals. In Settings → Controls
you can resize, fade or mirror the buttons for left-handed play, or hide them to play
with a controller. A paired controller works in both cases.

**Shortcuts** (default)

| Action | Keys |
|--------|------|
| Quick save | F8 |
| Save to a new slot | Shift + F8 |
| Quick load | Hold F9 |
| Pause / resume | F5 |
| Screenshot | F12 |
| Mute | F10 |
| Focus mode | F11 |
| Reset the game | Ctrl + R |

---

# Saving
Games that support battery saves on real hardware save automatically, and again when you close the emulator.

- Saves are stored per game, identified by the ROM's contents, so renaming or moving a ROM keeps its save.
- Where the data lives depends on your platform
    - Native: in Citrine's application data folder
    - Web: in the browser's local storage

# Saves & Snapshots
The **Saves** tab stores as many complete save states per game as you like, each with a thumbnail, so
you can pick up exactly where you left off. To name a snapshot, press the pencil next to it. Snapshots
you don't rename stay "Slot {{index}}". It also shows where your in-game save is kept, and can import
or export a `.sav` file to move saves between emulators.

Quick save and quick load use the slot marked with a star. You have to hold quick load
for a moment, so a stray press cannot throw away your progress.

---

# Palettes
**Settings → GB Style** sets the four shades the screen uses. To make your own palette, press **New**,
pick four colours from lightest to darkest, and give it a name. Each palette has a share code such as
`F2CE44-B89D32-72611C-292406`. Copy yours to share it, or paste someone else's to use theirs. The
loaded game can also get a palette of its own.

---

# Tabs, Panels and Windows
The UI is highly flexible. You can open new tabs via the menu bar, drag and drop them into different configurations, or drag them anywhere to pop them out as a window. Adjust it to your hearts content. Optionally, there is also a focus mode that disables all UI besides the Game Boy screen (you can find it in the menu bar too).

"#,
        env!("CARGO_PKG_VERSION")
    )
}
