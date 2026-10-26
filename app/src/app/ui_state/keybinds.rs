use citrine_gb::gb::joypad::JoypadState;
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
    pub fn joypad(self) -> JoypadState {
        match self {
            Self::Up => JoypadState::UP,
            Self::Down => JoypadState::DOWN,
            Self::Left => JoypadState::LEFT,
            Self::Right => JoypadState::RIGHT,
            Self::A => JoypadState::A,
            Self::B => JoypadState::B,
            Self::Start => JoypadState::START,
            Self::Select => JoypadState::SELECT,
        }
    }

    pub const DPAD: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];
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
    pub fn from_gilrs(button: gilrs::Button) -> Option<Self> {
        Some(match button {
            gilrs::Button::South => Self::South,
            gilrs::Button::East => Self::East,
            gilrs::Button::North => Self::North,
            gilrs::Button::West => Self::West,
            gilrs::Button::LeftTrigger => Self::LeftTrigger,
            gilrs::Button::LeftTrigger2 => Self::LeftTrigger2,
            gilrs::Button::RightTrigger => Self::RightTrigger,
            gilrs::Button::RightTrigger2 => Self::RightTrigger2,
            gilrs::Button::Select => Self::Select,
            gilrs::Button::Start => Self::Start,
            gilrs::Button::Mode => Self::Mode,
            gilrs::Button::LeftThumb => Self::LeftThumb,
            gilrs::Button::RightThumb => Self::RightThumb,
            gilrs::Button::DPadUp => Self::DPadUp,
            gilrs::Button::DPadDown => Self::DPadDown,
            gilrs::Button::DPadLeft => Self::DPadLeft,
            gilrs::Button::DPadRight => Self::DPadRight,
            _ => return None,
        })
    }
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
    Shortcut(crate::app::ui_state::hotkeys::HotkeyAction),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    keyboard: Vec<(egui::Key, GbButton)>,
    gamepad: Vec<(PadButton, GbButton)>,
    pub stick_as_dpad: bool,
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
            stick_as_dpad: true,
        }
    }
}

impl KeyBindings {
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

    pub fn bind_key(&mut self, key: egui::Key, button: GbButton) {
        self.keyboard.retain(|(bound, _)| *bound != key);
        self.keyboard.push((key, button));
    }

    pub fn bind_pad(&mut self, pad: PadButton, button: GbButton) {
        self.gamepad.retain(|(bound, _)| *bound != pad);
        self.gamepad.push((pad, button));
    }

    pub fn unbind_key(&mut self, key: egui::Key) {
        self.keyboard.retain(|(bound, _)| *bound != key);
    }

    pub fn unbind_pad(&mut self, pad: PadButton) {
        self.gamepad.retain(|(bound, _)| *bound != pad);
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

    pub fn take(&mut self) -> Option<(BindingTarget, Captured)> {
        let captured = self.captured.take()?;
        let pending = self.pending.take()?;
        Some((pending.target, captured))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key;

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
    fn gilrs_buttons_we_do_not_bind_are_ignored() {
        assert_eq!(
            PadButton::from_gilrs(gilrs::Button::South),
            Some(PadButton::South)
        );
        assert_eq!(PadButton::from_gilrs(gilrs::Button::Unknown), None);
    }
}
