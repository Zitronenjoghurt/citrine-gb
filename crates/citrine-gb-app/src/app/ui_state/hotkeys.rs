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

pub const FAST_FORWARD_KEY: egui::Key = egui::Key::Space;

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
    SwapScreens,
    OneScreen,
    NextScreenLayout,
    ToggleLid,
    Rewind,
    FastForward,
    StepInstruction,
    StepOver,
    StepOut,
    StepLine,
    StepFrame,
    ToggleHuds,
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
            Self::Rewind => "Rewind",
            Self::FastForward => "Fast forward",
            Self::SoftReset => "Reset the game",
            Self::ToggleMute => "Mute",
            Self::ToggleFocusMode => "Focus mode",
            Self::LeaveFocusMode => "Leave focus mode",
            Self::ToggleTouchControls => "On-screen buttons",
            Self::SwapScreens => "Swap screens",
            Self::OneScreen => "Only one screen",
            Self::NextScreenLayout => "Next screen layout",
            Self::ToggleLid => "Close the lid",
            Self::SwitchGame => "Switch game",
            Self::FocusChat => "Type in chat",
            Self::StepInstruction => "Step one instruction",
            Self::StepOver => "Step over",
            Self::StepOut => "Step out",
            Self::StepLine => "Step one line",
            Self::StepFrame => "Step one frame",
            Self::ToggleHuds => "Show HUDs",
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
            Self::Rewind => "Hold to play the game backwards",
            Self::FastForward => "Hold to run the game at the fast forward speed",
            Self::SoftReset => "Reset as if the console was power cycled",
            Self::ToggleMute => "Silence the sound without changing the volume",
            Self::ToggleFocusMode => "Hide everything but the screen, or show it all again",
            Self::LeaveFocusMode => "Show everything again when only the screen shows",
            Self::ToggleTouchControls => "Show or hide the on-screen buttons",
            Self::SwapScreens => "Also picks the big screen, or the one shown alone",
            Self::OneScreen => "Show only one DS screen, or both again",
            Self::NextScreenLayout => "Switch between Stacked, Side by side and Big and small",
            Self::ToggleLid => "Close or open the DS lid. Most games sleep while it is closed",
            Self::SwitchGame => "Play the other linked game with the same controls",
            Self::FocusChat => "Move into the open chat and back to the game",
            Self::StepInstruction => "Run the next instruction, into a call too",
            Self::StepOver => "Run the next instruction, a call to its return",
            Self::StepOut => "Run until the current call returns",
            Self::StepLine => "Run until the screen moves to its next line",
            Self::StepFrame => "Run until the next frame is drawn",
            Self::ToggleHuds => "Show or hide the HUDs over the game",
        }
    }

    pub fn icon(self, on: bool) -> &'static str {
        match self {
            Self::TogglePause if on => icons::PAUSE,
            Self::TogglePause => icons::PLAY,
            Self::Rewind => icons::REWIND,
            Self::FastForward => icons::FAST_FORWARD,
            Self::SoftReset => icons::ARROW_COUNTER_CLOCKWISE,
            Self::Screenshot => icons::CAMERA,
            Self::ToggleMute if on => icons::SPEAKER_SLASH,
            Self::ToggleMute => icons::SPEAKER_HIGH,
            Self::ToggleFocusMode | Self::LeaveFocusMode => icons::FRAME_CORNERS,
            Self::SwapScreens => icons::ARROWS_DOWN_UP,
            Self::OneScreen => icons::MONITOR,
            Self::NextScreenLayout => icons::SQUARE_SPLIT_VERTICAL,
            Self::ToggleLid if on => icons::SUN,
            Self::ToggleLid => icons::MOON,
            Self::StepInstruction => icons::ARROW_DOWN,
            Self::StepOver => icons::ARROW_BEND_DOWN_RIGHT,
            Self::StepOut => icons::ARROW_BEND_LEFT_UP,
            Self::StepLine => icons::ROWS,
            Self::StepFrame => icons::FILM_STRIP,
            Self::ToggleHuds => icons::GAUGE,
            _ => "",
        }
    }

    pub fn hint(self, on: bool) -> &'static str {
        match self {
            Self::TogglePause if on => "Pause",
            Self::TogglePause => "Resume",
            Self::ToggleMute if on => "Unmute",
            Self::ToggleLid if on => "Open the lid",
            Self::Rewind => "Hold to rewind",
            other => other.label(),
        }
    }

    pub fn is_ds(self) -> bool {
        matches!(
            self,
            Self::SwapScreens | Self::OneScreen | Self::NextScreenLayout | Self::ToggleLid
        )
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

    pub fn is_rewind(self) -> bool {
        self == Self::Rewind
    }

    pub fn is_fast_forward(self) -> bool {
        self == Self::FastForward
    }

    pub fn is_hud(self) -> bool {
        self == Self::ToggleHuds
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
            Self::Rewind => Hotkey::plain(Key::R),
            Self::FastForward => Hotkey::plain(FAST_FORWARD_KEY),
            Self::SoftReset => Hotkey::ctrl(Key::R),
            Self::ToggleMute => Hotkey::plain(Key::F10),
            Self::ToggleFocusMode => Hotkey::plain(Key::F11),
            Self::LeaveFocusMode => Hotkey::plain(Key::Escape),
            Self::ToggleTouchControls => return None,
            Self::SwapScreens => Hotkey::plain(Key::F2),
            Self::OneScreen => Hotkey::shift(Key::F2),
            Self::NextScreenLayout => Hotkey::ctrl(Key::F2),
            Self::SwitchGame => Hotkey::plain(Key::F6),
            Self::FocusChat => Hotkey::plain(Key::Tab),
            Self::StepInstruction => Hotkey::plain(Key::F7),
            Self::StepOver => Hotkey::plain(Key::F3),
            Self::StepOut => Hotkey::plain(Key::F4),
            Self::ToggleHuds => Hotkey::shift(Key::F3),
            Self::ToggleLid | Self::StepLine | Self::StepFrame => return None,
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
    screens: LaterHotkeys,
    #[serde(default)]
    debugger: LaterHotkeys,
    #[serde(default)]
    rewind: LaterHotkeys,
    #[serde(default)]
    fast_forward: LaterHotkeys,
    #[serde(default)]
    hud: LaterHotkeys,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct LaterHotkeys {
    bindings: Vec<(Hotkey, HotkeyAction)>,
    pads: Vec<(PadButton, HotkeyAction)>,
    offered: Vec<HotkeyAction>,
}

impl LaterHotkeys {
    fn take(hotkeys: &mut Hotkeys, later: fn(HotkeyAction) -> bool) -> Self {
        Self {
            bindings: hotkeys
                .bindings
                .extract_if(.., |(_, action)| later(*action))
                .collect(),
            pads: hotkeys
                .pads
                .extract_if(.., |(_, action)| later(*action))
                .collect(),
            offered: hotkeys
                .offered
                .extract_if(.., |action| later(*action))
                .collect(),
        }
    }

    fn put(self, hotkeys: &mut Hotkeys) {
        hotkeys.bindings.extend(self.bindings);
        hotkeys.pads.extend(self.pads);
        if !hotkeys.offered.is_empty() {
            hotkeys.offered.extend(self.offered);
        }
    }
}

impl Default for StoredHotkeys {
    fn default() -> Self {
        let mut stored = Self::from(Hotkeys::default());
        stored.screens = LaterHotkeys::default();
        stored.debugger = LaterHotkeys::default();
        stored.rewind = LaterHotkeys::default();
        stored.fast_forward = LaterHotkeys::default();
        stored.hud = LaterHotkeys::default();
        stored
    }
}

impl From<Hotkeys> for StoredHotkeys {
    fn from(mut hotkeys: Hotkeys) -> Self {
        let debugger = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_debugger);
        let screens = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_ds);
        let rewind = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_rewind);
        let fast_forward = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_fast_forward);
        let hud = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_hud);
        Self {
            bindings: hotkeys.bindings,
            pads: hotkeys.pads,
            offered: hotkeys.offered,
            screens,
            debugger,
            rewind,
            fast_forward,
            hud,
        }
    }
}

impl From<StoredHotkeys> for Hotkeys {
    fn from(stored: StoredHotkeys) -> Self {
        let mut hotkeys = Self {
            bindings: stored.bindings,
            pads: stored.pads,
            offered: stored.offered,
        };
        stored.screens.put(&mut hotkeys);
        stored.rewind.put(&mut hotkeys);
        stored.fast_forward.put(&mut hotkeys);
        stored.debugger.put(&mut hotkeys);
        stored.hud.put(&mut hotkeys);
        hotkeys
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

    pub fn offers(&self, action: HotkeyAction) -> bool {
        self.offered.contains(&action)
    }

    pub fn offer_unbound(&mut self, action: HotkeyAction) {
        if !self.offered.is_empty() && !self.offers(action) {
            self.offered.push(action);
        }
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
        assert_eq!(
            hotkeys.action_for(Key::R, Modifiers::NONE),
            Some(HotkeyAction::Rewind)
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
                        | HotkeyAction::ToggleLid
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
        old.as_object_mut().unwrap().remove("hud");
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
    fn a_build_from_before_rewind_still_reads_the_hotkeys() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(PadButton::iter().next().unwrap(), HotkeyAction::Rewind);
        let stored = serde_json::to_string(&hotkeys).unwrap();

        let older: Result<OlderHotkeys, _> = serde_json::from_str(&stored);

        assert!(older.is_ok(), "{stored}");
        let back: Hotkeys = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, hotkeys);
    }

    #[test]
    fn hotkeys_from_before_rewind_gain_its_key() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("rewind");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::Rewind).is_empty());

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::R, Modifiers::NONE),
            Some(HotkeyAction::Rewind)
        );
    }

    #[test]
    fn hotkeys_from_before_the_ds_screens_gain_their_keys() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("screens");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::SwapScreens).is_empty());

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::F2, Modifiers::NONE),
            Some(HotkeyAction::SwapScreens)
        );
        assert_eq!(
            hotkeys.action_for(Key::F2, Modifiers::CTRL),
            Some(HotkeyAction::NextScreenLayout)
        );
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
    fn holding_space_fast_forwards_by_default() {
        let hotkeys = Hotkeys::default();
        assert_eq!(
            hotkeys.action_for(Key::Space, Modifiers::NONE),
            Some(HotkeyAction::FastForward)
        );
        assert_eq!(
            hotkeys.action_for(Key::Tab, Modifiers::NONE),
            Some(HotkeyAction::FocusChat)
        );
    }

    #[test]
    fn a_build_from_before_fast_forward_still_reads_the_hotkeys() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(PadButton::iter().next().unwrap(), HotkeyAction::FastForward);
        let stored = serde_json::to_string(&hotkeys).unwrap();

        let older: Result<OlderHotkeys, _> = serde_json::from_str(&stored);

        assert!(older.is_ok(), "{stored}");
        let back: Hotkeys = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, hotkeys);
    }

    #[test]
    fn hotkeys_from_before_fast_forward_gain_its_key() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("fast_forward");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::FastForward).is_empty());
        assert!(!hotkeys.offers(HotkeyAction::FastForward));

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::Space, Modifiers::NONE),
            Some(HotkeyAction::FastForward)
        );
        assert!(hotkeys.offers(HotkeyAction::FastForward));
    }

    #[test]
    fn a_build_from_before_the_hud_still_reads_the_hotkeys() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(PadButton::iter().next().unwrap(), HotkeyAction::ToggleHuds);
        let stored = serde_json::to_string(&hotkeys).unwrap();

        let older: Result<OlderHotkeys, _> = serde_json::from_str(&stored);

        assert!(older.is_ok(), "{stored}");
        let back: Hotkeys = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, hotkeys);
    }

    #[test]
    fn hotkeys_from_before_the_hud_gain_its_key() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("hud");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::ToggleHuds).is_empty());
        assert!(!hotkeys.offers(HotkeyAction::ToggleHuds));

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::F3, Modifiers::SHIFT),
            Some(HotkeyAction::ToggleHuds)
        );
        assert!(hotkeys.offers(HotkeyAction::ToggleHuds));
    }

    #[test]
    fn escape_reads_as_esc_like_its_key_cap() {
        assert_eq!(Hotkey::plain(Key::Escape).to_string(), "Esc");
    }
}
