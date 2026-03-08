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
The auto-save feature is a bit unstable at the moment. While it generally works quite well, there are some things to keep in mind:
- It only works for cartridges that had batteries included (you can see if the cartridge supports battery saves in the ROM info panel).
    - Some games constantly write to the cartridge's RAM, causing a save to happen every 5s (the defined save-cooldown).
    - Some games only write to the cartridge's RAM if you save in-game (wait for at least 5s after saving in-game before you close the emulator).
- Where the data is stored depends on your platform
    - Native (Windows, MacOS)
        - A save file with the same name as the ROM is created in the same folder as the ROM, it has the extension `.sav`.
        - Because of this system, bundled homebrew ROMs have no save-support on native yet.
    - Web
        - The save data is stored in the browser's local storage.

---

# Tabs, Panels and Windows
The UI is highly flexible. You can open new tabs via the menu bar, drag and drop them into different configurations, or drag them anywhere to pop them out as a window. Adjust it to your hearts content. Optionally, there is also a focus mode that disables all UI besides the Game Boy screen (you can find it in the menu bar too).

"#,
        env!("CARGO_PKG_VERSION")
    )
}
