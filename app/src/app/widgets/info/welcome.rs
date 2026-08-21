pub fn build() -> String {
    format!(
        r#"
# Welcome to Citrine v{}
Thanks for using my emulator! If you find any issues, please report them on [GitHub](https://github.com/Zitronenjoghurt/citrine-gb/issues).

Native version (Windows, MacOS): [GitHub](https://github.com/Zitronenjoghurt/citrine-gb/releases/latest)\
Web Version: [Citrine Web](https://gb.lemon.industries)

---

# Stability
This emulator is still in **active** development. Things might **change or break** at any time.\
While there is a certain level of stability, it is not guaranteed.

---

# Controls

**Keyboard**

| Action     | Keys             |
|------------|------------------|
| Start      | Enter, Space     |
| Select     | Backspace        |
| Directions | WASD, Arrow Keys |
| A          | Y, Z, Q, O       |
| B          | X, E, P          |

**Controller**

| Action     | Buttons                         |
|------------|---------------------------------|
| Start      | + / Start                       |
| Select     | - / Select                      |
| Directions | D-Pad, Left Stick               |
| A          | East / West (Nintendo: A / Y)   |
| B          | South / North (Nintendo: B / X) |

---

# Saving
Games that support battery saves on real hardware save automatically, and again when you close the emulator.

- Saves are stored per game, identified by the ROM's contents, so renaming or moving a ROM keeps its save.
- Where the data lives depends on your platform
    - Native (Windows, MacOS): in Citrine's application data folder
    - Web: in the browser's local storage

# Saves & Snapshots
The **Saves** tab stores as many complete save states per game as you like, each with a thumbnail, so
you can pick up exactly where you left off. It also shows where your in-game save is kept, and can
import or export a `.sav` file to move saves between emulators.

| Action | Keys |
|--------|------|
| Overwrite the quick slot | F8 |
| Save to a new slot | Shift + F8 |
| Load the quick slot | Hold F9 |

---

# Tabs, Panels and Windows
The UI is highly flexible. You can open new tabs via the menu bar, drag and drop them into different configurations, or drag them anywhere to pop them out as a window. Adjust it to your hearts content. Optionally, there is also a focus mode that disables all UI besides the Game Boy screen (you can find it in the menu bar too).

"#,
        env!("CARGO_PKG_VERSION")
    )
}
