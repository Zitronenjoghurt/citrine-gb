use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Hotkey {
    pub key: egui::Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Default for Hotkey {
    fn default() -> Self {
        Self {
            key: egui::Key::F8,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }
}

impl Hotkey {
    pub fn plain(key: egui::Key) -> Self {
        Self {
            key,
            ..Default::default()
        }
    }

    pub fn shift(key: egui::Key) -> Self {
        Self {
            key,
            shift: true,
            ..Default::default()
        }
    }

    pub fn ctrl(key: egui::Key) -> Self {
        Self {
            key,
            ctrl: true,
            ..Default::default()
        }
    }

    pub fn from_event(key: egui::Key, modifiers: egui::Modifiers) -> Self {
        Self {
            key,
            ctrl: modifiers.command || modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }

    pub fn matches(&self, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        self.key == key && *self == Self::from_event(key, modifiers)
    }
}

impl Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl {
            write!(f, "Ctrl+")?;
        }
        if self.shift {
            write!(f, "Shift+")?;
        }
        if self.alt {
            write!(f, "Alt+")?;
        }
        f.write_str(self.key.name())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, serde::Serialize, serde::Deserialize,
)]
pub enum HotkeyAction {
    QuickSave,
    QuickSaveNewSlot,
    QuickLoad,
    Screenshot,
    ToggleRecording,
    TogglePause,
    SoftReset,
    ToggleMute,
    ToggleFocusMode,
    ToggleTouchControls,
}

impl HotkeyAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::QuickSave => "Quick save",
            Self::QuickSaveNewSlot => "Save to a new slot",
            Self::QuickLoad => "Quick load",
            Self::Screenshot => "Screenshot",
            Self::ToggleRecording => "Record video",
            Self::TogglePause => "Pause / resume",
            Self::SoftReset => "Reset the game",
            Self::ToggleMute => "Mute",
            Self::ToggleFocusMode => "Focus mode",
            Self::ToggleTouchControls => "On-screen buttons",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::QuickSave => "Overwrite the starred snapshot slot",
            Self::QuickSaveNewSlot => "Take a snapshot in the next free slot",
            Self::QuickLoad => "Hold to restore the starred slot",
            Self::Screenshot => "Save the screen to Media",
            Self::ToggleRecording => "Start or stop recording the screen and sound",
            Self::TogglePause => "Stop and start emulation",
            Self::SoftReset => "Reset as if the console was power cycled",
            Self::ToggleMute => "Silence the sound without changing the volume",
            Self::ToggleFocusMode => "Hide everything but the screen",
            Self::ToggleTouchControls => "Show or hide the on-screen buttons",
        }
    }

    pub fn is_hold(self) -> bool {
        matches!(self, Self::QuickLoad)
    }

    fn default_binding(self) -> Option<Hotkey> {
        use egui::Key;
        Some(match self {
            Self::QuickSave => Hotkey::plain(Key::F8),
            Self::QuickSaveNewSlot => Hotkey::shift(Key::F8),
            Self::QuickLoad => Hotkey::plain(Key::F9),
            Self::Screenshot => Hotkey::plain(Key::F12),
            Self::ToggleRecording => Hotkey::shift(Key::F12),
            Self::TogglePause => Hotkey::plain(Key::F5),
            Self::SoftReset => Hotkey::ctrl(Key::R),
            Self::ToggleMute => Hotkey::plain(Key::F10),
            Self::ToggleFocusMode => Hotkey::plain(Key::F11),
            Self::ToggleTouchControls => return None,
        })
    }
}

impl Display for HotkeyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    bindings: Vec<(Hotkey, HotkeyAction)>,
    #[serde(default = "Vec::new")]
    offered: Vec<HotkeyAction>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            bindings: HotkeyAction::iter()
                .filter_map(|action| action.default_binding().map(|key| (key, action)))
                .collect(),
            offered: HotkeyAction::iter().collect(),
        }
    }
}

impl Hotkeys {
    pub fn action_for(&self, key: egui::Key, modifiers: egui::Modifiers) -> Option<HotkeyAction> {
        self.bindings
            .iter()
            .find(|(hotkey, _)| hotkey.matches(key, modifiers))
            .map(|(_, action)| *action)
    }

    pub fn keys_for(&self, action: HotkeyAction) -> Vec<Hotkey> {
        self.bindings
            .iter()
            .filter(|(_, bound)| *bound == action)
            .map(|(hotkey, _)| *hotkey)
            .collect()
    }

    pub fn key_name(&self, action: HotkeyAction) -> Option<String> {
        self.keys_for(action).first().map(Hotkey::to_string)
    }

    pub fn quick_keys(&self) -> Option<String> {
        let keys: Vec<String> = [HotkeyAction::QuickSave, HotkeyAction::QuickLoad]
            .into_iter()
            .filter_map(|action| self.key_name(action))
            .collect();
        (!keys.is_empty()).then(|| keys.join(" / "))
    }

    pub fn bind(&mut self, hotkey: Hotkey, action: HotkeyAction) {
        self.bindings.retain(|(bound, _)| *bound != hotkey);
        self.bindings.push((hotkey, action));
    }

    pub fn unbind(&mut self, hotkey: Hotkey) {
        self.bindings.retain(|(bound, _)| *bound != hotkey);
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn bind_new_defaults(&mut self) {
        if self.offered.is_empty() {
            self.offered = HotkeyAction::iter()
                .filter(|action| *action != HotkeyAction::ToggleRecording)
                .collect();
        }
        for action in HotkeyAction::iter() {
            if self.offered.contains(&action) {
                continue;
            }
            self.offered.push(action);
            if let Some(key) = action.default_binding()
                && !self.bindings.iter().any(|(bound, _)| *bound == key)
            {
                self.bindings.push((key, action));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    #[test]
    fn settings_from_before_recording_gain_its_hotkey_and_keep_the_rest() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("offered");
        old["bindings"]
            .as_array_mut()
            .unwrap()
            .retain(|b| b[1] != "ToggleRecording" && b[1] != "QuickSave");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        hotkeys.bind_new_defaults();
        assert_eq!(
            hotkeys.action_for(Key::F12, Modifiers::SHIFT),
            Some(HotkeyAction::ToggleRecording)
        );
        assert!(hotkeys.keys_for(HotkeyAction::QuickSave).is_empty());
        hotkeys.unbind(Hotkey::shift(Key::F12));
        hotkeys.bind_new_defaults();
        assert!(hotkeys.keys_for(HotkeyAction::ToggleRecording).is_empty());
    }

    #[test]
    fn the_documented_defaults_are_bound() {
        let hotkeys = Hotkeys::default();
        assert_eq!(
            hotkeys.action_for(Key::F8, Modifiers::NONE),
            Some(HotkeyAction::QuickSave)
        );
        assert_eq!(
            hotkeys.action_for(Key::F12, Modifiers::NONE),
            Some(HotkeyAction::Screenshot)
        );
        assert_eq!(
            hotkeys.action_for(Key::F5, Modifiers::NONE),
            Some(HotkeyAction::TogglePause)
        );
    }

    #[test]
    fn modifiers_have_to_match_exactly() {
        let hotkeys = Hotkeys::default();

        assert_eq!(
            hotkeys.action_for(Key::F8, Modifiers::SHIFT),
            Some(HotkeyAction::QuickSaveNewSlot),
            "shift picks the other action"
        );
        assert_eq!(
            hotkeys.action_for(Key::F8, Modifiers::NONE),
            Some(HotkeyAction::QuickSave),
            "and plain F8 still picks the first"
        );
        assert_eq!(
            hotkeys.action_for(Key::F8, Modifiers::ALT),
            None,
            "an unbound modifier combination does nothing"
        );
    }

    #[test]
    fn one_shortcut_only_ever_drives_one_action() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind(Hotkey::plain(Key::F8), HotkeyAction::Screenshot);

        assert_eq!(
            hotkeys.action_for(Key::F8, Modifiers::NONE),
            Some(HotkeyAction::Screenshot)
        );
        assert!(
            hotkeys.keys_for(HotkeyAction::QuickSave).is_empty(),
            "the old owner lost it"
        );
        assert_eq!(
            hotkeys.keys_for(HotkeyAction::Screenshot).len(),
            2,
            "and it joins the shortcuts that action already had"
        );
    }

    #[test]
    fn the_command_key_counts_as_ctrl() {
        let hotkeys = Hotkeys::default();
        let command = Modifiers {
            command: true,
            ..Modifiers::NONE
        };
        assert_eq!(
            hotkeys.action_for(Key::R, command),
            Some(HotkeyAction::SoftReset),
            "Cmd on macOS reaches the same shortcut as Ctrl elsewhere"
        );
    }

    #[test]
    fn shortcuts_read_the_way_they_are_written() {
        assert_eq!(Hotkey::plain(Key::F8).to_string(), "F8");
        assert_eq!(Hotkey::shift(Key::F8).to_string(), "Shift+F8");
        assert_eq!(Hotkey::ctrl(Key::R).to_string(), "Ctrl+R");
    }

    #[test]
    fn unbinding_leaves_the_action_with_nothing() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.unbind(Hotkey::plain(Key::F12));

        assert!(hotkeys.keys_for(HotkeyAction::Screenshot).is_empty());
        assert_eq!(hotkeys.action_for(Key::F12, Modifiers::NONE), None);
        assert!(!hotkeys.is_default());
    }

    #[test]
    fn every_action_is_reachable_from_the_settings_list() {
        for action in HotkeyAction::iter() {
            let bound = !Hotkeys::default().keys_for(action).is_empty();
            assert_eq!(
                bound,
                action != HotkeyAction::ToggleTouchControls,
                "{action:?}"
            );
            assert!(!action.label().is_empty());
            assert!(!action.description().is_empty());
        }
    }
}
