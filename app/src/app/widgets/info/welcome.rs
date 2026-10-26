use super::Controls;
use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{GbButton, KeyBindings};
use strum::IntoEnumIterator;

pub fn build(controls: &Controls) -> String {
    format!(
        r#"
# Welcome to Citrine v{}
Thanks for using my emulator! If you find any issues, please report them on [GitHub](https://github.com/Zitronenjoghurt/citrine-gb/issues).

Native version: [GitHub](https://github.com/Zitronenjoghurt/citrine-gb/releases/latest)\
Web version: [Citrine Web](https://gb.lemon.industries)

---

# Stability
This emulator is still in **active** development. Things might **change or break** at any time.\
While there is a certain level of stability, it is not guaranteed.

---

{}
---

# Saving
Games that support battery saves on real hardware save automatically, and again when you close the emulator.

- Saves are stored per game, identified by the ROM's contents, so renaming or moving a ROM keeps its save.
- Where the data lives depends on your platform
    - Native: in Citrine's application data folder
    - Web: in the browser's local storage

# Saves & snapshots
The **Saves** tab stores as many complete save states per game as you like, each with a thumbnail, so
you can pick up exactly where you left off. It also shows where your in-game save is kept, and can
import or export a `.sav` file to move saves between emulators.

To name a snapshot, click or tap its name. Snapshots you don't rename keep a name like "Slot 1".

{}
---

# Palettes
**Settings → GB style** sets the four shades the screen uses. To make your own palette, press **New**,
pick four colours from lightest to darkest, and give it a name. Each palette has a share code such as
`F2CE44-B89D32-72611C-292406`. Copy yours to share it, or paste someone else's to use theirs. The
loaded game can also get a palette of its own.

{}"#,
        env!("CARGO_PKG_VERSION"),
        controls_section(controls),
        quick_slot_note(controls),
        layout_section(controls),
    )
}

const TOUCH: &str =
    "On phones and narrow windows, Citrine switches to a touch layout with an on-screen
d-pad, A and B, and Start and Select. The d-pad reads diagonals. In Settings → Controls
you can resize, fade or mirror the buttons for left-handed play, or hide them to play
with a controller. A paired controller works in both cases.";

const NOTHING_BOUND: &str = "Nothing bound";

fn controls_section(controls: &Controls) -> String {
    match controls {
        Controls::Touch => "# Controls
The on-screen d-pad reads diagonals. In **Settings → Controls** you can resize, fade or mirror
the buttons for left-handed play, or hide them to play with a paired controller.
"
        .to_owned(),
        Controls::Desktop { bindings, hotkeys } => format!(
            "# Controls\nYou can remap every binding below in **Settings → Controls**.\n\n\
             {}\n{}\n**Touch**\n\n{TOUCH}\n\n**Shortcuts**\n\n{}",
            keyboard_table(bindings),
            controller_table(bindings),
            shortcuts_table(hotkeys),
        ),
    }
}

fn table(head: (&str, &str), rows: impl IntoIterator<Item = (String, String)>) -> String {
    let mut table = format!("| {} | {} |\n|---|---|\n", head.0, head.1);
    for (label, value) in rows {
        table.push_str(&format!("| {label} | {value} |\n"));
    }
    table
}

fn joined(names: Vec<String>) -> String {
    if names.is_empty() {
        NOTHING_BOUND.to_owned()
    } else {
        names.join(", ")
    }
}

fn keyboard_table(bindings: &KeyBindings) -> String {
    table(
        ("Game Boy", "Keyboard"),
        GbButton::iter().map(|button| {
            let keys = bindings
                .keys_for(button)
                .into_iter()
                .map(|key| key.name().to_owned())
                .collect();
            (button.to_string(), joined(keys))
        }),
    )
}

fn controller_table(bindings: &KeyBindings) -> String {
    table(
        ("Game Boy", "Controller"),
        GbButton::iter().map(|button| {
            let mut pads: Vec<String> = bindings
                .pads_for(button)
                .into_iter()
                .map(|pad| pad.short_name().to_owned())
                .collect();
            if bindings.stick_as_dpad && GbButton::DPAD.contains(&button) {
                pads.push("Left stick".to_owned());
            }
            (button.to_string(), joined(pads))
        }),
    )
}

fn shortcuts_table(hotkeys: &Hotkeys) -> String {
    table(
        ("Action", "Keys"),
        HotkeyAction::iter().filter_map(|action| {
            let keys: Vec<String> = hotkeys
                .keys_for(action)
                .iter()
                .map(|hotkey| {
                    if action.is_hold() {
                        format!("Hold {hotkey}")
                    } else {
                        hotkey.to_string()
                    }
                })
                .collect();
            (!keys.is_empty()).then(|| (action.label().to_owned(), keys.join(", ")))
        }),
    )
}

fn quick_slot_note(controls: &Controls) -> &'static str {
    match controls {
        Controls::Touch => "",
        Controls::Desktop { .. } => {
            "Quick save and quick load use the slot marked with a star. You have to hold quick load
for a moment, so a stray press cannot throw away your progress.
"
        }
    }
}

fn layout_section(controls: &Controls) -> &'static str {
    match controls {
        Controls::Touch => "",
        Controls::Desktop { .. } => {
            "---

# Tabs, panels and windows
You can open new tabs from the menu bar, drag them into different arrangements, or drag one anywhere
to pop it out as a window. Focus mode, also in the menu bar, hides everything but the Game Boy screen.
"
        }
    }
}
