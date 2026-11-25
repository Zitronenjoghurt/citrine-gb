use crate::app::ui_state::keybinds::PadButton;
use crate::combo::{Bound, Combo, Mods};
use crate::icons;
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "StoredKey", into = "StoredKey")]
pub struct Hotkey {
    keys: Combo<egui::Key>,
    mods: Mods,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct StoredKey {
    key: egui::Key,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    with: Vec<egui::Key>,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl Default for StoredKey {
    fn default() -> Self {
        Self {
            key: egui::Key::F8,
            with: Vec::new(),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }
}

impl From<StoredKey> for Hotkey {
    fn from(stored: StoredKey) -> Self {
        let mods = Mods {
            ctrl: stored.ctrl,
            shift: stored.shift,
            alt: stored.alt,
        };
        let keys = Combo::new(stored.with.iter().copied().chain([stored.key]))
            .unwrap_or_else(|| Combo::single(stored.key));
        Self { keys, mods }
    }
}

impl From<Hotkey> for StoredKey {
    fn from(hotkey: Hotkey) -> Self {
        let (key, with) = match hotkey.keys.inputs().split_last() {
            Some((key, with)) => (*key, with.to_vec()),
            None => (egui::Key::F8, Vec::new()),
        };
        Self {
            key,
            with,
            ctrl: hotkey.mods.ctrl,
            shift: hotkey.mods.shift,
            alt: hotkey.mods.alt,
        }
    }
}

impl Default for Hotkey {
    fn default() -> Self {
        Self::plain(egui::Key::F8)
    }
}

impl Hotkey {
    pub fn new(keys: Combo<egui::Key>, mods: Mods) -> Self {
        Self { keys, mods }
    }

    pub fn plain(key: egui::Key) -> Self {
        Self::new(Combo::single(key), Mods::NONE)
    }

    pub fn shift(key: egui::Key) -> Self {
        Self::new(
            Combo::single(key),
            Mods {
                shift: true,
                ..Mods::NONE
            },
        )
    }

    pub fn ctrl(key: egui::Key) -> Self {
        Self::new(
            Combo::single(key),
            Mods {
                ctrl: true,
                ..Mods::NONE
            },
        )
    }

    pub fn from_event(key: egui::Key, modifiers: egui::Modifiers) -> Self {
        Self::new(Combo::single(key), Mods::of(modifiers))
    }

    pub fn keys(&self) -> &Combo<egui::Key> {
        &self.keys
    }

    pub fn mods(&self) -> Mods {
        self.mods
    }

    pub fn single_key(&self) -> Option<egui::Key> {
        self.keys.only()
    }

    pub fn is_combo(&self) -> bool {
        !self.keys.is_single()
    }

    pub fn types_text(&self) -> bool {
        !self.mods.ctrl
            && !self.mods.alt
            && self
                .single_key()
                .is_some_and(|key| key.name().chars().count() == 1)
    }

    pub fn matches(&self, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        self.single_key() == Some(key) && self.mods == Mods::of(modifiers)
    }
}

impl Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mods = self.mods.names().map(str::to_owned);
        let keys = self.keys.inputs().iter().map(|key| match key {
            egui::Key::Escape => "Esc".to_owned(),
            key => key.name().to_owned(),
        });
        let parts: Vec<String> = mods.chain(keys).collect();
        f.write_str(&parts.join("+"))
    }
}

pub const FAST_FORWARD_KEY: egui::Key = egui::Key::Space;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum QuickLoadKey {
    Press,
    #[default]
    Hold,
}

impl Display for QuickLoadKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Press => "Press",
            Self::Hold => "Hold 2 s",
        })
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
    Fullscreen,
    OpenSettings,
    PushToTalk,
    SaveClip,
}

impl HotkeyAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::QuickSave => "Quick save",
            Self::QuickSaveNewSlot => "Save to a new slot",
            Self::QuickLoad => "Quick load",
            Self::Screenshot => "Screenshot",
            Self::ToggleRecording => "Record",
            Self::SaveClip => "Save a clip",
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
            Self::Fullscreen => "Fullscreen",
            Self::OpenSettings => "Open Settings",
            Self::PushToTalk => "Push to talk",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::QuickSave => "Overwrite the starred snapshot slot",
            Self::QuickSaveNewSlot => "Take a snapshot in the next free slot",
            Self::QuickLoad => "Hold to restore the starred slot",
            Self::Screenshot => "Save the screen to Media",
            Self::ToggleRecording => "Start or stop recording the screen and sound",
            Self::SaveClip => "Save the last seconds you played, cut to the part you want",
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
            Self::Fullscreen => "Fill the screen with the app, or go back to a window",
            Self::OpenSettings => "Open Settings, ready to search",
            Self::PushToTalk => "Hold so the DS game hears the microphone, with Push to talk on",
        }
    }

    pub fn icon(self, on: bool) -> &'static str {
        match self {
            Self::TogglePause if on => icons::PAUSE,
            Self::TogglePause => icons::PLAY,
            Self::Rewind => icons::REWIND,
            Self::FastForward => icons::FAST_FORWARD,
            Self::SoftReset => icons::POWER,
            Self::Screenshot => icons::CAMERA,
            Self::SaveClip => icons::SCISSORS,
            Self::ToggleMute if on => icons::SPEAKER_SLASH,
            Self::ToggleMute => icons::SPEAKER_HIGH,
            Self::ToggleFocusMode | Self::LeaveFocusMode => icons::FRAME_CORNERS,
            Self::SwapScreens => icons::ARROWS_DOWN_UP,
            Self::OneScreen => icons::DS_TOP_ONLY,
            Self::NextScreenLayout => icons::SQUARE_SPLIT_VERTICAL,
            Self::ToggleLid if on => icons::SUN,
            Self::ToggleLid => icons::MOON,
            Self::StepInstruction => icons::ARROW_DOWN,
            Self::StepOver => icons::ARROW_BEND_DOWN_RIGHT,
            Self::StepOut => icons::ARROW_BEND_LEFT_UP,
            Self::StepLine => icons::ROWS,
            Self::StepFrame => icons::FILM_STRIP,
            Self::ToggleHuds => icons::GAUGE,
            Self::PushToTalk => icons::MICROPHONE,
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

    pub fn is_window(self) -> bool {
        matches!(self, Self::Fullscreen | Self::OpenSettings)
    }

    pub fn is_talk(self) -> bool {
        self == Self::PushToTalk
    }

    pub fn is_clip(self) -> bool {
        self == Self::SaveClip
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
            Self::SaveClip => Hotkey::ctrl(Key::F12),
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
            Self::Fullscreen => Hotkey::shift(Key::F11),
            Self::OpenSettings => Hotkey::ctrl(Key::Comma),
            Self::PushToTalk => Hotkey::plain(Key::T),
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
    pads: Vec<(Combo<PadButton>, HotkeyAction)>,
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
    key_combos: Vec<(Hotkey, HotkeyAction)>,
    #[serde(default)]
    pad_combos: Vec<(Combo<PadButton>, HotkeyAction)>,
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
    #[serde(default)]
    window: LaterHotkeys,
    #[serde(default)]
    talk: LaterHotkeys,
    #[serde(default)]
    clip: LaterHotkeys,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct LaterHotkeys {
    bindings: Vec<(Hotkey, HotkeyAction)>,
    pads: Vec<(PadButton, HotkeyAction)>,
    offered: Vec<HotkeyAction>,
    key_combos: Vec<(Hotkey, HotkeyAction)>,
    pad_combos: Vec<(Combo<PadButton>, HotkeyAction)>,
}

impl LaterHotkeys {
    fn take(hotkeys: &mut Hotkeys, later: fn(HotkeyAction) -> bool) -> Self {
        let (key_combos, bindings) = hotkeys
            .bindings
            .extract_if(.., |(_, action)| later(*action))
            .partition(|(hotkey, _)| hotkey.is_combo());
        let (singles, pad_combos): (Vec<_>, Vec<_>) = hotkeys
            .pads
            .extract_if(.., |(_, action)| later(*action))
            .partition(|(combo, _)| combo.is_single());
        Self {
            bindings,
            pads: singles
                .into_iter()
                .filter_map(|(combo, action)| combo.only().map(|pad| (pad, action)))
                .collect(),
            offered: hotkeys
                .offered
                .extract_if(.., |action| later(*action))
                .collect(),
            key_combos,
            pad_combos,
        }
    }

    fn put(self, hotkeys: &mut Hotkeys) {
        hotkeys.bindings.extend(self.bindings);
        hotkeys.bindings.extend(self.key_combos);
        hotkeys.pads.extend(
            self.pads
                .into_iter()
                .map(|(pad, action)| (Combo::single(pad), action)),
        );
        hotkeys.pads.extend(self.pad_combos);
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
        stored.window = LaterHotkeys::default();
        stored.talk = LaterHotkeys::default();
        stored.clip = LaterHotkeys::default();
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
        let window = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_window);
        let talk = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_talk);
        let clip = LaterHotkeys::take(&mut hotkeys, HotkeyAction::is_clip);
        let offered = std::mem::take(&mut hotkeys.offered);
        let base = LaterHotkeys::take(&mut hotkeys, |_| true);
        Self {
            bindings: base.bindings,
            pads: base.pads,
            offered,
            key_combos: base.key_combos,
            pad_combos: base.pad_combos,
            screens,
            debugger,
            rewind,
            fast_forward,
            hud,
            window,
            talk,
            clip,
        }
    }
}

impl From<StoredHotkeys> for Hotkeys {
    fn from(stored: StoredHotkeys) -> Self {
        let mut hotkeys = Self {
            bindings: Vec::new(),
            pads: Vec::new(),
            offered: stored.offered,
        };
        let base = LaterHotkeys {
            bindings: stored.bindings,
            pads: stored.pads,
            offered: Vec::new(),
            key_combos: stored.key_combos,
            pad_combos: stored.pad_combos,
        };
        base.put(&mut hotkeys);
        stored.screens.put(&mut hotkeys);
        stored.rewind.put(&mut hotkeys);
        stored.fast_forward.put(&mut hotkeys);
        stored.debugger.put(&mut hotkeys);
        stored.hud.put(&mut hotkeys);
        stored.window.put(&mut hotkeys);
        stored.talk.put(&mut hotkeys);
        stored.clip.put(&mut hotkeys);
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
    pub fn key_table(&self) -> Vec<Bound<egui::Key, HotkeyAction>> {
        self.bindings
            .iter()
            .map(|(hotkey, action)| Bound::exact(hotkey.keys.clone(), hotkey.mods, *action))
            .collect()
    }

    pub fn pad_table(&self) -> Vec<Bound<PadButton, HotkeyAction>> {
        self.pads
            .iter()
            .map(|(combo, action)| Bound::any_mods(combo.clone(), *action))
            .collect()
    }

    pub fn key_in_combo(&self, key: egui::Key) -> bool {
        self.bindings
            .iter()
            .any(|(hotkey, _)| hotkey.is_combo() && hotkey.keys.contains(&key))
    }

    pub fn owner_of(&self, hotkey: &Hotkey) -> Option<HotkeyAction> {
        self.bindings
            .iter()
            .find(|(bound, _)| bound == hotkey)
            .map(|(_, action)| *action)
    }

    pub fn owner_of_pads(&self, combo: &Combo<PadButton>) -> Option<HotkeyAction> {
        self.pads
            .iter()
            .find(|(bound, _)| bound == combo)
            .map(|(_, action)| *action)
    }

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
            .map(|(hotkey, _)| hotkey.clone())
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

    pub fn unbind(&mut self, hotkey: &Hotkey) {
        self.bindings.retain(|(bound, _)| bound != hotkey);
    }

    pub fn unbind_keys(&mut self, keys: &Combo<egui::Key>) {
        self.bindings
            .retain(|(bound, _)| bound.keys != *keys || bound.mods != Mods::NONE);
    }

    pub fn action_for_pad(&self, pad: PadButton) -> Option<HotkeyAction> {
        self.owner_of_pads(&Combo::single(pad))
    }

    pub fn pads_for(&self, action: HotkeyAction) -> Vec<Combo<PadButton>> {
        self.pads
            .iter()
            .filter(|(_, bound)| *bound == action)
            .map(|(combo, _)| combo.clone())
            .collect()
    }

    pub fn bind_pad(&mut self, pad: PadButton, action: HotkeyAction) {
        self.bind_pads(Combo::single(pad), action);
    }

    pub fn bind_pads(&mut self, combo: Combo<PadButton>, action: HotkeyAction) {
        self.pads.retain(|(bound, _)| *bound != combo);
        self.pads.push((combo, action));
    }

    pub fn unbind_pad(&mut self, pad: PadButton) {
        self.unbind_pads(&Combo::single(pad));
    }

    pub fn unbind_pads(&mut self, combo: &Combo<PadButton>) {
        self.pads.retain(|(bound, _)| bound != combo);
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
        hotkeys.unbind(&Hotkey::shift(Key::F12));
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
        hotkeys.unbind(&Hotkey::plain(Key::F12));

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
        old.as_object_mut().unwrap().remove("window");
        old.as_object_mut().unwrap().remove("talk");
        old.as_object_mut().unwrap().remove("clip");
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
    fn shift_f11_goes_fullscreen_and_ctrl_comma_opens_settings() {
        let hotkeys = Hotkeys::default();
        assert_eq!(
            hotkeys.action_for(Key::F11, Modifiers::SHIFT),
            Some(HotkeyAction::Fullscreen)
        );
        assert_eq!(
            hotkeys.action_for(Key::F11, Modifiers::NONE),
            Some(HotkeyAction::ToggleFocusMode)
        );
        assert_eq!(
            hotkeys.action_for(Key::Comma, Modifiers::CTRL),
            Some(HotkeyAction::OpenSettings)
        );
        assert_eq!(
            hotkeys.action_for(Key::Comma, Modifiers::COMMAND),
            Some(HotkeyAction::OpenSettings),
            "Cmd+Comma on macOS"
        );
        assert_eq!(Hotkey::ctrl(Key::Comma).to_string(), "Ctrl+Comma");
    }

    #[test]
    fn a_build_from_before_the_window_shortcuts_still_reads_the_hotkeys() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(
            PadButton::iter().next().unwrap(),
            HotkeyAction::OpenSettings,
        );
        let stored = serde_json::to_string(&hotkeys).unwrap();

        let older: Result<OlderHotkeys, _> = serde_json::from_str(&stored);

        assert!(older.is_ok(), "{stored}");
        let back: Hotkeys = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, hotkeys);
    }

    #[test]
    fn hotkeys_from_before_the_window_shortcuts_gain_their_keys() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("window");
        old.as_object_mut().unwrap().remove("talk");
        old.as_object_mut().unwrap().remove("clip");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::Fullscreen).is_empty());
        assert!(!hotkeys.offers(HotkeyAction::OpenSettings));

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::F11, Modifiers::SHIFT),
            Some(HotkeyAction::Fullscreen)
        );
        assert_eq!(
            hotkeys.action_for(Key::Comma, Modifiers::CTRL),
            Some(HotkeyAction::OpenSettings)
        );
        assert_eq!(hotkeys, Hotkeys::default());
    }

    #[test]
    fn hotkeys_from_before_push_to_talk_gain_its_key() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("talk");
        old.as_object_mut().unwrap().remove("clip");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::PushToTalk).is_empty());
        assert!(!hotkeys.offers(HotkeyAction::PushToTalk));

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::T, Modifiers::NONE),
            Some(HotkeyAction::PushToTalk)
        );
        assert_eq!(hotkeys, Hotkeys::default());
    }

    #[test]
    fn a_build_from_before_save_a_clip_still_reads_the_hotkeys() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(PadButton::iter().next().unwrap(), HotkeyAction::SaveClip);
        let stored = serde_json::to_string(&hotkeys).unwrap();

        let older: Result<OlderHotkeys, _> = serde_json::from_str(&stored);

        assert!(older.is_ok(), "{stored}");
        let back: Hotkeys = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, hotkeys);
    }

    #[test]
    fn hotkeys_from_before_save_a_clip_gain_its_key() {
        let mut old = serde_json::to_value(Hotkeys::default()).unwrap();
        old.as_object_mut().unwrap().remove("clip");
        let mut hotkeys: Hotkeys = serde_json::from_value(old).unwrap();
        assert!(hotkeys.keys_for(HotkeyAction::SaveClip).is_empty());
        assert!(!hotkeys.offers(HotkeyAction::SaveClip));

        hotkeys.bind_new_defaults();

        assert_eq!(
            hotkeys.action_for(Key::F12, Modifiers::CTRL),
            Some(HotkeyAction::SaveClip)
        );
        assert_eq!(hotkeys, Hotkeys::default());
    }

    #[test]
    fn escape_reads_as_esc_like_its_key_cap() {
        assert_eq!(Hotkey::plain(Key::Escape).to_string(), "Esc");
    }

    fn keys(list: &[Key]) -> Combo<Key> {
        Combo::new(list.iter().copied()).expect("a combo")
    }

    fn ctrl_s_d() -> Hotkey {
        Hotkey::new(
            keys(&[Key::S, Key::D]),
            Mods {
                ctrl: true,
                ..Mods::NONE
            },
        )
    }

    #[test]
    fn a_key_combo_reads_with_its_modifiers_first() {
        assert_eq!(ctrl_s_d().to_string(), "Ctrl+D+S");
        assert!(ctrl_s_d().is_combo());
        assert_eq!(ctrl_s_d().single_key(), None);
        assert!(!ctrl_s_d().matches(Key::S, Modifiers::CTRL));
    }

    #[test]
    fn a_single_key_stores_exactly_as_before() {
        let stored = serde_json::to_value(Hotkey::shift(Key::F8)).unwrap();
        assert_eq!(
            stored,
            serde_json::json!({"key": "F8", "ctrl": false, "shift": true, "alt": false})
        );
    }

    #[test]
    fn combos_store_apart_so_older_builds_never_read_their_last_key() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind(ctrl_s_d(), HotkeyAction::ToggleMute);
        hotkeys.bind(
            Hotkey::new(keys(&[Key::Q, Key::W]), Mods::NONE),
            HotkeyAction::Rewind,
        );
        let menu = Combo::new([
            PadButton::LeftTrigger,
            PadButton::RightTrigger,
            PadButton::Select,
        ])
        .unwrap();
        hotkeys.bind_pads(menu.clone(), HotkeyAction::ToggleFocusMode);
        hotkeys.bind_pad(PadButton::Mode, HotkeyAction::FastForward);
        let stored = serde_json::to_value(&hotkeys).unwrap();
        let text = stored.to_string();

        assert_eq!(stored["key_combos"].as_array().unwrap().len(), 1, "{text}");
        assert_eq!(stored["rewind"]["key_combos"].as_array().unwrap().len(), 1);
        assert_eq!(stored["pad_combos"].as_array().unwrap().len(), 1);
        assert_eq!(stored["fast_forward"]["pads"].as_array().unwrap().len(), 1);
        let older: OlderHotkeys = serde_json::from_value(stored.clone()).unwrap();
        assert!(older.bindings.iter().all(|(hotkey, _)| !hotkey.is_combo()));

        let back: Hotkeys = serde_json::from_value(stored).unwrap();
        assert_eq!(back.owner_of(&ctrl_s_d()), Some(HotkeyAction::ToggleMute));
        assert_eq!(
            back.owner_of_pads(&menu),
            Some(HotkeyAction::ToggleFocusMode)
        );
        assert_eq!(
            back.action_for_pad(PadButton::Mode),
            Some(HotkeyAction::FastForward)
        );
        assert_eq!(back.keys_for(HotkeyAction::Rewind).len(), 2);
        assert!(back.key_in_combo(Key::Q));
        assert!(!back.key_in_combo(Key::F8));
    }

    #[test]
    fn a_game_binding_takes_only_the_shortcut_with_no_modifiers() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind(ctrl_s_d(), HotkeyAction::ToggleMute);
        hotkeys.bind(
            Hotkey::new(keys(&[Key::S, Key::D]), Mods::NONE),
            HotkeyAction::Screenshot,
        );
        hotkeys.unbind_keys(&keys(&[Key::S, Key::D]));
        assert_eq!(
            hotkeys.owner_of(&ctrl_s_d()),
            Some(HotkeyAction::ToggleMute)
        );
        assert_eq!(
            hotkeys.keys_for(HotkeyAction::Screenshot),
            [Hotkey::plain(Key::F12)]
        );
    }
}
