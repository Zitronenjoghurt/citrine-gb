use citrine_gb::console::Keys;
use std::fmt::Display;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, serde::Serialize, serde::Deserialize,
)]
pub enum GbButton {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Start,
    Select,
}

impl GbButton {
    pub fn key(self) -> Keys {
        match self {
            Self::Up => Keys::UP,
            Self::Down => Keys::DOWN,
            Self::Left => Keys::LEFT,
            Self::Right => Keys::RIGHT,
            Self::A => Keys::A,
            Self::B => Keys::B,
            Self::Start => Keys::START,
            Self::Select => Keys::SELECT,
        }
    }

    pub const DPAD: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, serde::Serialize, serde::Deserialize,
)]
pub enum Shoulder {
    L,
    R,
}

impl Shoulder {
    pub fn key(self) -> Keys {
        match self {
            Self::L => Keys::L,
            Self::R => Keys::R,
        }
    }
}

impl Display for Shoulder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::L => "L",
            Self::R => "R",
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Shoulders {
    keyboard: Vec<(egui::Key, Shoulder)>,
    gamepad: Vec<(PadButton, Shoulder)>,
    #[serde(default)]
    triggers: bool,
}

impl Default for Shoulders {
    fn default() -> Self {
        Self {
            keyboard: vec![
                (egui::Key::Num1, Shoulder::L),
                (egui::Key::Num3, Shoulder::R),
            ],
            gamepad: vec![
                (PadButton::LeftTrigger, Shoulder::L),
                (PadButton::LeftTrigger2, Shoulder::L),
                (PadButton::RightTrigger, Shoulder::R),
                (PadButton::RightTrigger2, Shoulder::R),
            ],
            triggers: true,
        }
    }
}

impl Display for GbButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::A => "A",
            Self::B => "B",
            Self::Start => "Start",
            Self::Select => "Select",
        };
        f.write_str(name)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, serde::Serialize, serde::Deserialize,
)]
pub enum PadButton {
    South,
    East,
    North,
    West,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    Select,
    Start,
    Mode,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl PadButton {
    pub fn short_name(self) -> &'static str {
        match self {
            Self::South => "South",
            Self::East => "East",
            Self::North => "North",
            Self::West => "West",
            Self::LeftTrigger => "L1",
            Self::LeftTrigger2 => "L2",
            Self::RightTrigger => "R1",
            Self::RightTrigger2 => "R2",
            Self::Select => "Select",
            Self::Start => "Start",
            Self::Mode => "Home",
            Self::LeftThumb => "L3",
            Self::RightThumb => "R3",
            Self::DPadUp => "Pad up",
            Self::DPadDown => "Pad down",
            Self::DPadLeft => "Pad left",
            Self::DPadRight => "Pad right",
        }
    }
}

impl Display for PadButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::South => "South (A / B)",
            Self::East => "East (B / A)",
            Self::North => "North (Y / X)",
            Self::West => "West (X / Y)",
            Self::LeftTrigger => "L1",
            Self::LeftTrigger2 => "L2",
            Self::RightTrigger => "R1",
            Self::RightTrigger2 => "R2",
            Self::Select => "Select",
            Self::Start => "Start",
            Self::Mode => "Mode / Home",
            Self::LeftThumb => "Left stick press",
            Self::RightThumb => "Right stick press",
            Self::DPadUp => "D-pad up",
            Self::DPadDown => "D-pad down",
            Self::DPadLeft => "D-pad left",
            Self::DPadRight => "D-pad right",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Keyboard,
    Gamepad,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BindingTarget {
    Button(GbButton),
    Shoulder(Shoulder),
    Shortcut(crate::app::ui_state::hotkeys::HotkeyAction),
    LinkKey(crate::local_link::Half, GbButton),
}

impl BindingTarget {
    pub fn buttons() -> impl Iterator<Item = Self> {
        let buttons = GbButton::iter().map(Self::Button);
        #[cfg(feature = "gba")]
        let buttons = buttons.chain(Shoulder::iter().map(Self::Shoulder));
        buttons
    }

    pub fn button_name(self) -> Option<String> {
        match self {
            Self::Button(button) => Some(button.to_string()),
            Self::Shoulder(shoulder) => Some(shoulder.to_string()),
            Self::Shortcut(_) | Self::LinkKey(..) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    keyboard: Vec<(egui::Key, GbButton)>,
    gamepad: Vec<(PadButton, GbButton)>,
    shoulders: Shoulders,
    pub stick_as_dpad: bool,
    pub stick_deadzone: u8,
}

impl Default for KeyBindings {
    fn default() -> Self {
        use egui::Key as K;
        Self {
            keyboard: vec![
                (K::W, GbButton::Up),
                (K::ArrowUp, GbButton::Up),
                (K::S, GbButton::Down),
                (K::ArrowDown, GbButton::Down),
                (K::A, GbButton::Left),
                (K::ArrowLeft, GbButton::Left),
                (K::D, GbButton::Right),
                (K::ArrowRight, GbButton::Right),
                (K::Q, GbButton::A),
                (K::Z, GbButton::A),
                (K::Y, GbButton::A),
                (K::O, GbButton::A),
                (K::E, GbButton::B),
                (K::X, GbButton::B),
                (K::P, GbButton::B),
                (K::Enter, GbButton::Start),
                (K::Space, GbButton::Start),
                (K::Backspace, GbButton::Select),
            ],
            gamepad: vec![
                (PadButton::DPadUp, GbButton::Up),
                (PadButton::DPadDown, GbButton::Down),
                (PadButton::DPadLeft, GbButton::Left),
                (PadButton::DPadRight, GbButton::Right),
                (PadButton::East, GbButton::A),
                (PadButton::West, GbButton::A),
                (PadButton::South, GbButton::B),
                (PadButton::North, GbButton::B),
                (PadButton::Start, GbButton::Start),
                (PadButton::Select, GbButton::Select),
            ],
            shoulders: Shoulders::default(),
            stick_as_dpad: true,
            stick_deadzone: Self::DEFAULT_DEADZONE,
        }
    }
}

impl KeyBindings {
    pub const DEFAULT_DEADZONE: u8 = 50;
    pub const DEADZONES: std::ops::RangeInclusive<u8> = 20..=90;

    pub fn deadzone_percent(&self) -> u8 {
        self.stick_deadzone
            .clamp(*Self::DEADZONES.start(), *Self::DEADZONES.end())
    }

    pub fn deadzone(&self) -> f32 {
        f32::from(self.deadzone_percent()) / 100.0
    }

    pub fn button_for_key(&self, key: egui::Key) -> Option<GbButton> {
        self.keyboard
            .iter()
            .find(|(bound, _)| *bound == key)
            .map(|(_, button)| *button)
    }

    pub fn button_for_pad(&self, pad: PadButton) -> Option<GbButton> {
        self.gamepad
            .iter()
            .find(|(bound, _)| *bound == pad)
            .map(|(_, button)| *button)
    }

    pub fn keys_for(&self, button: GbButton) -> Vec<egui::Key> {
        self.keyboard
            .iter()
            .filter(|(_, bound)| *bound == button)
            .map(|(key, _)| *key)
            .collect()
    }

    pub fn pads_for(&self, button: GbButton) -> Vec<PadButton> {
        self.gamepad
            .iter()
            .filter(|(_, bound)| *bound == button)
            .map(|(pad, _)| *pad)
            .collect()
    }

    pub fn input_for_key(&self, key: egui::Key) -> Option<Keys> {
        self.button_for_key(key)
            .map(GbButton::key)
            .or_else(|| self.shoulder_for_key(key).map(Shoulder::key))
    }

    pub fn shoulder_for_key(&self, key: egui::Key) -> Option<Shoulder> {
        if self.button_for_key(key).is_some() {
            return None;
        }
        self.shoulders
            .keyboard
            .iter()
            .find(|(bound, _)| *bound == key)
            .map(|(_, shoulder)| *shoulder)
    }

    pub fn shoulder_for_pad(&self, pad: PadButton) -> Option<Shoulder> {
        if self.button_for_pad(pad).is_some() {
            return None;
        }
        self.shoulders
            .gamepad
            .iter()
            .find(|(bound, _)| *bound == pad)
            .map(|(_, shoulder)| *shoulder)
    }

    pub fn adopt_triggers(&mut self) -> bool {
        if self.shoulders.triggers {
            return false;
        }
        self.shoulders.triggers = true;
        for (pad, shoulder) in [
            (PadButton::LeftTrigger2, Shoulder::L),
            (PadButton::RightTrigger2, Shoulder::R),
        ] {
            let taken = self.button_for_pad(pad).is_some()
                || self
                    .shoulders
                    .gamepad
                    .iter()
                    .any(|(bound, _)| *bound == pad);
            if !taken {
                self.shoulders.gamepad.push((pad, shoulder));
            }
        }
        true
    }

    pub fn bound_inputs(&self, target: BindingTarget) -> (Vec<egui::Key>, Vec<PadButton>) {
        match target {
            BindingTarget::Button(button) => (self.keys_for(button), self.pads_for(button)),
            BindingTarget::Shoulder(shoulder) => {
                (self.shoulder_keys(shoulder), self.shoulder_pads(shoulder))
            }
            BindingTarget::Shortcut(_) | BindingTarget::LinkKey(..) => (Vec::new(), Vec::new()),
        }
    }

    pub fn shoulder_keys(&self, shoulder: Shoulder) -> Vec<egui::Key> {
        self.shoulders
            .keyboard
            .iter()
            .filter(|(key, bound)| *bound == shoulder && self.button_for_key(*key).is_none())
            .map(|(key, _)| *key)
            .collect()
    }

    pub fn shoulder_pads(&self, shoulder: Shoulder) -> Vec<PadButton> {
        self.shoulders
            .gamepad
            .iter()
            .filter(|(pad, bound)| *bound == shoulder && self.button_for_pad(*pad).is_none())
            .map(|(pad, _)| *pad)
            .collect()
    }

    pub fn bind_key(&mut self, key: egui::Key, button: GbButton) {
        self.unbind_key(key);
        self.keyboard.push((key, button));
    }

    pub fn bind_pad(&mut self, pad: PadButton, button: GbButton) {
        self.unbind_pad(pad);
        self.gamepad.push((pad, button));
    }

    pub fn bind_shoulder_key(&mut self, key: egui::Key, shoulder: Shoulder) {
        self.unbind_key(key);
        self.shoulders.keyboard.push((key, shoulder));
    }

    pub fn bind_shoulder_pad(&mut self, pad: PadButton, shoulder: Shoulder) {
        self.unbind_pad(pad);
        self.shoulders.gamepad.push((pad, shoulder));
    }

    pub fn unbind_key(&mut self, key: egui::Key) {
        self.keyboard.retain(|(bound, _)| *bound != key);
        self.shoulders.keyboard.retain(|(bound, _)| *bound != key);
    }

    pub fn unbind_pad(&mut self, pad: PadButton) {
        self.gamepad.retain(|(bound, _)| *bound != pad);
        self.shoulders.gamepad.retain(|(bound, _)| *bound != pad);
    }

    pub fn unbound(&self) -> Vec<GbButton> {
        GbButton::iter()
            .filter(|button| self.keys_for(*button).is_empty() && self.pads_for(*button).is_empty())
            .collect()
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingBinding {
    pub target: BindingTarget,
    pub kind: BindingKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Captured {
    Key(egui::Key, egui::Modifiers),
    Pad(PadButton),
}

#[derive(Debug, Default)]
pub struct BindingCapture {
    pub pending: Option<PendingBinding>,
    pub captured: Option<Captured>,
    shown: bool,
}

impl BindingCapture {
    pub fn is_waiting(&self) -> bool {
        self.pending.is_some()
    }

    pub fn waiting_for(&self, target: BindingTarget) -> bool {
        self.pending.is_some_and(|p| p.target == target)
    }

    pub fn wants(&self, kind: BindingKind) -> bool {
        self.pending
            .is_some_and(|p| p.kind == kind || p.kind == BindingKind::Any)
    }

    pub fn start(&mut self, target: BindingTarget, kind: BindingKind) {
        self.pending = Some(PendingBinding { target, kind });
        self.captured = None;
    }

    pub fn cancel(&mut self) {
        self.pending = None;
        self.captured = None;
    }

    pub fn record(&mut self, captured: Captured) {
        if self.pending.is_some() {
            self.captured = Some(captured);
        }
    }

    pub fn mark_shown(&mut self) {
        self.shown = true;
    }

    pub fn end_frame(&mut self) {
        if !self.shown {
            self.cancel();
        }
        self.shown = false;
    }

    pub fn take(&mut self) -> Option<(BindingTarget, Captured)> {
        let captured = self.captured.take()?;
        let pending = self.pending.take()?;
        Some((pending.target, captured))
    }
}

pub fn key_label(key: egui::Key) -> String {
    match key {
        egui::Key::ArrowUp => crate::icons::ARROW_UP.into(),
        egui::Key::ArrowDown => crate::icons::ARROW_DOWN.into(),
        egui::Key::ArrowLeft => crate::icons::ARROW_LEFT.into(),
        egui::Key::ArrowRight => crate::icons::ARROW_RIGHT.into(),
        other => other.name().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::gb::joypad::JoypadState;
    use egui::Key;

    #[test]
    fn every_button_keeps_its_bit() {
        let bits: Vec<(GbButton, u16, JoypadState)> = GbButton::iter()
            .map(|button| (button, button.key().bits(), button.key().into()))
            .collect();
        assert_eq!(
            bits,
            [
                (GbButton::Up, 0x40, JoypadState::UP),
                (GbButton::Down, 0x80, JoypadState::DOWN),
                (GbButton::Left, 0x20, JoypadState::LEFT),
                (GbButton::Right, 0x10, JoypadState::RIGHT),
                (GbButton::A, 0x01, JoypadState::A),
                (GbButton::B, 0x02, JoypadState::B),
                (GbButton::Start, 0x08, JoypadState::START),
                (GbButton::Select, 0x04, JoypadState::SELECT),
            ]
        );
    }

    #[test]
    fn the_defaults_cover_every_button() {
        assert!(
            KeyBindings::default().unbound().is_empty(),
            "a fresh install must be playable"
        );
    }

    #[test]
    fn the_defaults_match_the_documented_controls() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.button_for_key(Key::W), Some(GbButton::Up));
        assert_eq!(bindings.button_for_key(Key::ArrowUp), Some(GbButton::Up));
        assert_eq!(
            bindings.button_for_key(Key::Backspace),
            Some(GbButton::Select)
        );
        assert_eq!(bindings.button_for_key(Key::F1), None);
        assert_eq!(
            bindings.button_for_pad(PadButton::East),
            Some(GbButton::A),
            "east is A, matching the Nintendo layout"
        );
    }

    #[test]
    fn one_key_only_ever_drives_one_button() {
        let mut bindings = KeyBindings::default();
        bindings.bind_key(Key::W, GbButton::B);

        assert_eq!(bindings.button_for_key(Key::W), Some(GbButton::B));
        assert!(
            !bindings.keys_for(GbButton::Up).contains(&Key::W),
            "the old binding is released"
        );
        assert!(
            bindings.keys_for(GbButton::Up).contains(&Key::ArrowUp),
            "the button's other keys are untouched"
        );
    }

    #[test]
    fn rebinding_a_key_to_the_button_it_already_has_is_not_a_duplicate() {
        let mut bindings = KeyBindings::default();
        bindings.bind_key(Key::W, GbButton::Up);
        assert_eq!(
            bindings
                .keys_for(GbButton::Up)
                .iter()
                .filter(|k| **k == Key::W)
                .count(),
            1
        );
    }

    #[test]
    fn unbinding_everything_is_reported() {
        let mut bindings = KeyBindings::default();
        for key in bindings.keys_for(GbButton::Start) {
            bindings.unbind_key(key);
        }
        for pad in bindings.pads_for(GbButton::Start) {
            bindings.unbind_pad(pad);
        }
        assert_eq!(bindings.unbound(), vec![GbButton::Start]);
        assert!(!bindings.is_default());
    }

    #[test]
    fn a_binding_nobody_can_see_stops_waiting() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::A), BindingKind::Any);
        capture.mark_shown();
        capture.end_frame();
        assert!(capture.is_waiting(), "the controls are on screen");

        capture.end_frame();
        assert!(
            !capture.is_waiting(),
            "the controls closed, so the controller and keyboard play again"
        );
        assert!(!capture.wants(BindingKind::Gamepad));
    }

    #[test]
    fn the_dead_zone_saves_and_old_files_get_the_default() {
        let old: KeyBindings = serde_json::from_str(r#"{"stick_as_dpad":true}"#).expect("loads");
        assert_eq!(old.stick_deadzone, KeyBindings::DEFAULT_DEADZONE);

        let mut bindings = KeyBindings {
            stick_deadzone: 30,
            ..KeyBindings::default()
        };
        let saved = serde_json::to_string(&bindings).expect("saves");
        let loaded: KeyBindings = serde_json::from_str(&saved).expect("loads");
        assert_eq!(loaded.stick_deadzone, 30);
        assert!(!loaded.is_default());

        bindings.stick_deadzone = 5;
        assert_eq!(bindings.deadzone_percent(), 20);
    }

    #[test]
    fn saved_controller_bindings_still_load() {
        let saved = r#"{"gamepad":[["LeftTrigger2","A"],["DPadUp","Up"]],"stick_as_dpad":false}"#;
        let bindings: KeyBindings = serde_json::from_str(saved).expect("loads");
        assert_eq!(
            bindings.button_for_pad(PadButton::LeftTrigger2),
            Some(GbButton::A)
        );
        assert_eq!(
            bindings.button_for_pad(PadButton::DPadUp),
            Some(GbButton::Up)
        );
        assert!(!bindings.stick_as_dpad);
    }

    #[test]
    fn l_and_r_have_their_own_defaults() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.shoulder_keys(Shoulder::L), [Key::Num1]);
        assert_eq!(bindings.shoulder_keys(Shoulder::R), [Key::Num3]);
        assert_eq!(
            bindings.shoulder_pads(Shoulder::L),
            [PadButton::LeftTrigger, PadButton::LeftTrigger2]
        );
        assert_eq!(
            bindings.shoulder_pads(Shoulder::R),
            [PadButton::RightTrigger, PadButton::RightTrigger2]
        );
        assert_eq!(bindings.input_for_key(Key::Num1), Some(Keys::L));
        assert_eq!(bindings.input_for_key(Key::Num3), Some(Keys::R));
        assert_eq!(bindings.input_for_key(Key::Q), Some(Keys::A));
    }

    #[test]
    fn saved_shoulders_take_the_triggers_once_unless_they_are_used() {
        let saved = r#"{"gamepad":[["RightTrigger2","A"]],"shoulders":{"keyboard":[],"gamepad":[["LeftTrigger","L"],["RightTrigger","R"]]}}"#;
        let mut bindings: KeyBindings = serde_json::from_str(saved).expect("loads");
        assert!(bindings.adopt_triggers());
        assert_eq!(
            bindings.shoulder_pads(Shoulder::L),
            [PadButton::LeftTrigger, PadButton::LeftTrigger2]
        );
        assert_eq!(
            bindings.shoulder_pads(Shoulder::R),
            [PadButton::RightTrigger]
        );
        assert_eq!(
            bindings.button_for_pad(PadButton::RightTrigger2),
            Some(GbButton::A)
        );

        bindings.unbind_pad(PadButton::LeftTrigger2);
        assert!(!bindings.adopt_triggers());
        assert_eq!(
            bindings.shoulder_pads(Shoulder::L),
            [PadButton::LeftTrigger]
        );
        assert!(!KeyBindings::default().adopt_triggers());
    }

    #[test]
    fn the_shoulder_keys_collide_with_no_shortcut() {
        let hotkeys = crate::app::ui_state::hotkeys::Hotkeys::default();
        for key in [Key::Num1, Key::Num3] {
            for modifiers in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
                assert_eq!(hotkeys.action_for(key, modifiers), None, "{key:?}");
            }
        }
    }

    #[test]
    fn a_key_moves_between_a_button_and_a_shoulder() {
        let mut bindings = KeyBindings::default();
        bindings.bind_shoulder_key(Key::Q, Shoulder::L);
        assert_eq!(bindings.button_for_key(Key::Q), None);
        assert_eq!(bindings.input_for_key(Key::Q), Some(Keys::L));

        bindings.bind_key(Key::Num1, GbButton::B);
        assert_eq!(bindings.shoulder_keys(Shoulder::L), [Key::Q]);
        assert_eq!(bindings.input_for_key(Key::Num1), Some(Keys::B));

        bindings.bind_shoulder_pad(PadButton::East, Shoulder::R);
        assert_eq!(bindings.button_for_pad(PadButton::East), None);
        assert_eq!(
            bindings.shoulder_for_pad(PadButton::East),
            Some(Shoulder::R)
        );

        bindings.unbind_pad(PadButton::East);
        assert_eq!(bindings.shoulder_for_pad(PadButton::East), None);
    }

    #[test]
    fn old_settings_keep_their_binds_and_get_l_and_r() {
        let saved = r#"{"keyboard":[["Num1","Select"],["W","Up"]],"gamepad":[["LeftTrigger","Start"]],"stick_as_dpad":true}"#;
        let bindings: KeyBindings = serde_json::from_str(saved).expect("loads");
        assert_eq!(bindings.input_for_key(Key::Num1), Some(Keys::SELECT));
        assert!(bindings.shoulder_keys(Shoulder::L).is_empty());
        assert_eq!(bindings.shoulder_keys(Shoulder::R), [Key::Num3]);
        assert_eq!(
            bindings.button_for_pad(PadButton::LeftTrigger),
            Some(GbButton::Start)
        );
        assert_eq!(bindings.shoulder_for_pad(PadButton::LeftTrigger), None);
        assert_eq!(
            bindings.shoulder_for_pad(PadButton::RightTrigger),
            Some(Shoulder::R)
        );
    }

    #[derive(serde::Deserialize)]
    #[serde(default)]
    struct BindingsBeforeShoulders {
        keyboard: Vec<(egui::Key, GbButton)>,
        gamepad: Vec<(PadButton, GbButton)>,
        stick_as_dpad: bool,
    }

    impl Default for BindingsBeforeShoulders {
        fn default() -> Self {
            Self {
                keyboard: Vec::new(),
                gamepad: Vec::new(),
                stick_as_dpad: true,
            }
        }
    }

    #[test]
    fn a_build_without_shoulders_loads_binds_saved_with_l_and_r() {
        let mut bindings = KeyBindings::default();
        bindings.bind_shoulder_key(Key::K, Shoulder::L);
        bindings.bind_shoulder_pad(PadButton::LeftTrigger2, Shoulder::R);
        bindings.bind_key(Key::J, GbButton::A);
        let saved = serde_json::to_string(&bindings).expect("saves");
        assert!(saved.contains("shoulders"));

        let old: BindingsBeforeShoulders = serde_json::from_str(&saved).expect("loads");
        assert!(old.keyboard.contains(&(Key::J, GbButton::A)));
        assert!(old.keyboard.iter().all(|(key, _)| *key != Key::K));
        assert!(
            old.gamepad
                .iter()
                .all(|(pad, _)| *pad != PadButton::LeftTrigger2)
        );
        assert_eq!(old.gamepad.len(), KeyBindings::default().gamepad.len());
        assert!(old.stick_as_dpad);
    }
}
