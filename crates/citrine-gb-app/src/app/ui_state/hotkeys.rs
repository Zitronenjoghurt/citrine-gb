use crate::app::ui_state::keybinds::PadButton;
use crate::icons;
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

    pub fn types_text(&self) -> bool {
        !self.ctrl && !self.alt && self.key.name().chars().count() == 1
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
        match self.key {
            egui::Key::Escape => f.write_str("Esc"),
            key => f.write_str(key.name()),
        }
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
    LeaveFocusMode,
    ToggleTouchControls,
    SwitchGame,
    FocusChat,
    StepInstruction,
    StepOver,
    StepOut,
    StepLine,
    StepFrame,
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
            Self::LeaveFocusMode => "Leave focus mode",
            Self::ToggleTouchControls => "On-screen buttons",
            Self::SwitchGame => "Switch game",
            Self::FocusChat => "Type in chat",
            Self::StepInstruction => "Step one instruction",
            Self::StepOver => "Step over",
            Self::StepOut => "Step out",
            Self::StepLine => "Step one line",
            Self::StepFrame => "Step one frame",
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
            Self::ToggleFocusMode => "Hide everything but the screen, or show it all again",
            Self::LeaveFocusMode => "Show everything again when only the screen shows",
            Self::ToggleTouchControls => "Show or hide the on-screen buttons",
            Self::SwitchGame => "Play the other linked game with the same controls",
            Self::FocusChat => "Move into the open chat and back to the game",
            Self::StepInstruction => "Run the next instruction, into a call too",
            Self::StepOver => "Run the next instruction, a call to its return",
            Self::StepOut => "Run until the current call returns",
            Self::StepLine => "Run until the screen moves to its next line",
            Self::StepFrame => "Run until the next frame is drawn",
        }
    }

    pub fn icon(self, on: bool) -> &'static str {
        match self {
            Self::TogglePause if on => icons::PAUSE,
            Self::TogglePause => icons::PLAY,
            Self::SoftReset => icons::ARROW_COUNTER_CLOCKWISE,
            Self::Screenshot => icons::CAMERA,
            Self::ToggleMute if on => icons::SPEAKER_SLASH,
            Self::ToggleMute => icons::SPEAKER_HIGH,
            Self::ToggleFocusMode | Self::LeaveFocusMode => icons::FRAME_CORNERS,
            Self::StepInstruction => icons::ARROW_DOWN,
            Self::StepOver => icons::ARROW_BEND_DOWN_RIGHT,
            Self::StepOut => icons::ARROW_BEND_LEFT_UP,
            Self::StepLine => icons::ROWS,
            Self::StepFrame => icons::FILM_STRIP,
            _ => "",
        }
    }

    pub fn hint(self, on: bool) -> &'static str {
        match self {
            Self::TogglePause if on => "Pause",
            Self::TogglePause => "Resume",
            Self::ToggleMute if on => "Unmute",
            other => other.label(),
        }
    }

    pub fn is_debugger(self) -> bool {
        matches!(
            self,
            Self::StepInstruction
                | Self::StepOver
                | Self::StepOut
                | Self::StepLine
                | Self::StepFrame
        )
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
            Self::LeaveFocusMode => Hotkey::plain(Key::Escape),
            Self::ToggleTouchControls => return None,
            Self::SwitchGame => Hotkey::plain(Key::F6),
            Self::FocusChat => Hotkey::plain(Key::Tab),
            Self::StepInstruction => Hotkey::plain(Key::F7),
            Self::StepOver => Hotkey::plain(Key::F3),
            Self::StepOut => Hotkey::plain(Key::F4),
            Self::StepLine | Self::StepFrame => return None,
        })
    }
}

impl Display for HotkeyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "StoredHotkeys", into = "StoredHotkeys")]
pub struct Hotkeys {
    bindings: Vec<(Hotkey, HotkeyAction)>,
    pads: Vec<(PadButton, HotkeyAction)>,
    offered: Vec<HotkeyAction>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct StoredHotkeys {
    bindings: Vec<(Hotkey, HotkeyAction)>,
    #[serde(default = "Vec::new")]
    pads: Vec<(PadButton, HotkeyAction)>,
    #[serde(default = "Vec::new")]
    offered: Vec<HotkeyAction>,
    #[serde(default)]
    debugger: DebuggerHotkeys,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct DebuggerHotkeys {
    bindings: Vec<(Hotkey, HotkeyAction)>,
    pads: Vec<(PadButton, HotkeyAction)>,
    offered: Vec<HotkeyAction>,
}

impl Default for StoredHotkeys {
    fn default() -> Self {
        let mut stored = Self::from(Hotkeys::default());
        stored.debugger = DebuggerHotkeys::default();
        stored
    }
}

fn split<T>(list: Vec<T>, action: impl Fn(&T) -> HotkeyAction) -> (Vec<T>, Vec<T>) {
    list.into_iter()
        .partition(|item| !action(item).is_debugger())
}

impl From<Hotkeys> for StoredHotkeys {
    fn from(hotkeys: Hotkeys) -> Self {
        let (bindings, debugger_bindings) = split(hotkeys.bindings, |(_, action)| *action);
        let (pads, debugger_pads) = split(hotkeys.pads, |(_, action)| *action);
        let (offered, debugger_offered) = split(hotkeys.offered, |action| *action);
        Self {
            bindings,
            pads,
            offered,
            debugger: DebuggerHotkeys {
                bindings: debugger_bindings,
                pads: debugger_pads,
                offered: debugger_offered,
            },
        }
    }
}

impl From<StoredHotkeys> for Hotkeys {
    fn from(stored: StoredHotkeys) -> Self {
        let StoredHotkeys {
            mut bindings,
            mut pads,
            mut offered,
            debugger,
        } = stored;
        bindings.extend(debugger.bindings);
        pads.extend(debugger.pads);
        if !offered.is_empty() {
            offered.extend(debugger.offered);
        }
        Self {
            bindings,
            pads,
            offered,
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            bindings: HotkeyAction::iter()
                .filter_map(|action| action.default_binding().map(|key| (key, action)))
                .collect(),
            pads: Vec::new(),
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

    pub fn action_for_pad(&self, pad: PadButton) -> Option<HotkeyAction> {
        self.pads
            .iter()
            .find(|(bound, _)| *bound == pad)
            .map(|(_, action)| *action)
    }

    pub fn pads_for(&self, action: HotkeyAction) -> Vec<PadButton> {
        self.pads
            .iter()
            .filter(|(_, bound)| *bound == action)
            .map(|(pad, _)| *pad)
            .collect()
    }

    pub fn bind_pad(&mut self, pad: PadButton, action: HotkeyAction) {
        self.pads.retain(|(bound, _)| *bound != pad);
        self.pads.push((pad, action));
    }

    pub fn unbind_pad(&mut self, pad: PadButton) {
        self.pads.retain(|(bound, _)| *bound != pad);
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn bind_new_defaults(&mut self) {
        if self.offered.is_empty() {
            self.offered = HotkeyAction::iter()
                .filter(|action| {
                    !matches!(
                        action,
                        HotkeyAction::ToggleRecording | HotkeyAction::LeaveFocusMode
                    )
                })
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
                !matches!(
                    action,
                    HotkeyAction::ToggleTouchControls
                        | HotkeyAction::StepLine
                        | HotkeyAction::StepFrame
                ),
                "{action:?}"
            );
            assert!(!action.label().is_empty());
            assert!(!action.description().is_empty());
        }
    }

    #[derive(serde::Deserialize)]
    enum OlderAction {
        QuickSave,
        QuickSaveNewSlot,
        QuickLoad,
        Screenshot,
        ToggleRecording,
        TogglePause,
        SoftReset,
        ToggleMute,
        ToggleFocusMode,
        LeaveFocusMode,
        ToggleTouchControls,
        SwitchGame,
        FocusChat,
    }

    #[derive(serde::Deserialize)]
    #[expect(dead_code, reason = "only decoded, the way an older build reads it")]
    struct OlderHotkeys {
        bindings: Vec<(Hotkey, OlderAction)>,
        pads: Vec<(PadButton, OlderAction)>,
        offered: Vec<OlderAction>,
    }

    #[test]
    fn a_build_from_before_the_debugger_still_reads_the_hotkeys() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(PadButton::iter().next().unwrap(), HotkeyAction::StepOver);
        let stored = serde_json::to_string(&hotkeys).unwrap();

        let older: Result<OlderHotkeys, _> = serde_json::from_str(&stored);

        assert!(older.is_ok(), "{stored}");
        let back: Hotkeys = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, hotkeys);
    }

    #[test]
    fn hotkeys_from_before_the_debugger_gain_its_steps() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("debugger");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::StepInstruction).is_empty());

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::F7, Modifiers::NONE),
            Some(HotkeyAction::StepInstruction)
        );
        assert_eq!(hotkeys, Hotkeys::default());
    }

    #[test]
    fn the_debugger_steps_have_their_keys() {
        let hotkeys = Hotkeys::default();
        for (key, action) in [
            (Key::F7, HotkeyAction::StepInstruction),
            (Key::F3, HotkeyAction::StepOver),
            (Key::F4, HotkeyAction::StepOut),
        ] {
            assert_eq!(hotkeys.action_for(key, Modifiers::NONE), Some(action));
        }
    }

    #[test]
    fn escape_reads_as_esc_like_its_key_cap() {
        assert_eq!(Hotkey::plain(Key::Escape).to_string(), "Esc");
    }
}
