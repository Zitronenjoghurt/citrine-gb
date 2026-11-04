use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use egui::{Button, Response, Ui};

fn shortcut_hint(hotkeys: &Hotkeys, action: HotkeyAction, on: bool) -> String {
    let hint = action.hint(on);
    match hotkeys.key_name(action) {
        Some(key) => format!("{hint} ({key})"),
        None => hint.to_owned(),
    }
}

pub fn shortcut_button(
    ui: &mut Ui,
    hotkeys: &Hotkeys,
    action: HotkeyAction,
    on: bool,
    enabled: bool,
) -> Response {
    ui.add_enabled(enabled, Button::new(action.icon(on)))
        .on_hover_text(shortcut_hint(hotkeys, action, on))
}
