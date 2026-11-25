use crate::combo::{Bound, Combo, Mods};
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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, serde::Serialize, serde::Deserialize,
)]
pub enum DsButton {
    X,
    Y,
}

impl DsButton {
    pub fn key(self) -> Keys {
        match self {
            Self::X => Keys::X,
            Self::Y => Keys::Y,
        }
    }

    fn default_pad(self) -> (PadButton, GbButton) {
        match self {
            Self::X => (PadButton::North, GbButton::B),
            Self::Y => (PadButton::West, GbButton::A),
        }
    }
}

impl Display for DsButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::X => "DS X",
            Self::Y => "DS Y",
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DsButtons {
    keyboard: Vec<(egui::Key, DsButton)>,
    gamepad: Vec<(PadButton, DsButton)>,
    adopted: bool,
}

impl Default for DsButtons {
    fn default() -> Self {
        Self {
            keyboard: Self::KEYS.to_vec(),
            gamepad: DsButton::iter()
                .map(|button| (button.default_pad().0, button))
                .collect(),
            adopted: true,
        }
    }
}

impl DsButtons {
    const KEYS: [(egui::Key, DsButton); 2] =
        [(egui::Key::C, DsButton::X), (egui::Key::V, DsButton::Y)];

    fn missing() -> Self {
        Self {
            keyboard: Vec::new(),
            gamepad: Vec::new(),
            adopted: false,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, serde::Serialize, serde::Deserialize,
)]
pub enum Tilt {
    Up,
    Left,
    Down,
    Right,
}

impl Tilt {
    pub fn push(self) -> [f32; 2] {
        match self {
            Self::Up => [0.0, -1.0],
            Self::Left => [-1.0, 0.0],
            Self::Down => [0.0, 1.0],
            Self::Right => [1.0, 0.0],
        }
    }

    fn default_key(self) -> egui::Key {
        match self {
            Self::Up => egui::Key::I,
            Self::Left => egui::Key::J,
            Self::Down => egui::Key::K,
            Self::Right => egui::Key::L,
        }
    }
}

impl Display for Tilt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Up => "Tilt up",
            Self::Left => "Tilt left",
            Self::Down => "Tilt down",
            Self::Right => "Tilt right",
        })
    }
}

fn default_tilt_keys() -> [Option<egui::Key>; 4] {
    let mut keys = [None; 4];
    for tilt in Tilt::iter() {
        keys[tilt as usize] = Some(tilt.default_key());
    }
    keys
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
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    EnumIter,
    serde::Serialize,
    serde::Deserialize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GameButton {
    Button(GbButton),
    Shoulder(Shoulder),
    Ds(DsButton),
}

impl GameButton {
    pub fn key(self) -> Keys {
        match self {
            Self::Button(button) => button.key(),
            Self::Shoulder(shoulder) => shoulder.key(),
            Self::Ds(button) => button.key(),
        }
    }

    pub fn of(target: BindingTarget) -> Option<Self> {
        match target {
            BindingTarget::Button(button) => Some(Self::Button(button)),
            BindingTarget::Shoulder(shoulder) => Some(Self::Shoulder(shoulder)),
            BindingTarget::Ds(button) => Some(Self::Ds(button)),
            BindingTarget::Shortcut(_) | BindingTarget::LinkKey(..) | BindingTarget::Tilt(_) => {
                None
            }
        }
    }

    fn is_ds(self) -> bool {
        matches!(self, Self::Ds(_))
    }
}

impl Display for GameButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Button(button) => button.fmt(f),
            Self::Shoulder(shoulder) => shoulder.fmt(f),
            Self::Ds(button) => button.fmt(f),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fire<B> {
    Button(B),
    Shortcut(crate::app::ui_state::hotkeys::HotkeyAction),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GameCombos {
    keyboard: Vec<(Combo<egui::Key>, GameButton)>,
    gamepad: Vec<(Combo<PadButton>, GameButton)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTarget {
    Button(GbButton),
    Shoulder(Shoulder),
    Ds(DsButton),
    Shortcut(crate::app::ui_state::hotkeys::HotkeyAction),
    LinkKey(crate::local_link::Half, GbButton),
    Tilt(Tilt),
}

impl BindingTarget {
    pub fn buttons() -> impl Iterator<Item = Self> {
        let buttons = GbButton::iter().map(Self::Button);
        #[cfg(feature = "gba")]
        let buttons = buttons.chain(Shoulder::iter().map(Self::Shoulder));
        #[cfg(feature = "nds")]
        let buttons = buttons.chain(DsButton::iter().map(Self::Ds));
        buttons
    }

    pub fn button_name(self) -> Option<String> {
        match self {
            Self::Button(button) => Some(button.to_string()),
            Self::Shoulder(shoulder) => Some(shoulder.to_string()),
            Self::Ds(button) => Some(button.to_string()),
            Self::Shortcut(_) | Self::LinkKey(..) | Self::Tilt(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    keyboard: Vec<(egui::Key, GbButton)>,
    gamepad: Vec<(PadButton, GbButton)>,
    shoulders: Shoulders,
    #[serde(default = "DsButtons::missing")]
    ds: DsButtons,
    combos: GameCombos,
    #[serde(default = "default_tilt_keys")]
    tilt: [Option<egui::Key>; 4],
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
            ds: DsButtons::default(),
            combos: GameCombos::default(),
            tilt: default_tilt_keys(),
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

    pub fn input_for_key(&self, key: egui::Key, ds: bool) -> Option<Keys> {
        if ds && let Some(button) = self.ds_for_key(key) {
            return Some(button.key());
        }
        self.button_for_key(key)
            .map(GbButton::key)
            .or_else(|| self.shoulder_for_key(key).map(Shoulder::key))
    }

    pub fn ds_for_key(&self, key: egui::Key) -> Option<DsButton> {
        self.ds
            .keyboard
            .iter()
            .find(|(bound, _)| *bound == key)
            .map(|(_, button)| *button)
    }

    pub fn ds_for_pad(&self, pad: PadButton) -> Option<DsButton> {
        self.ds
            .gamepad
            .iter()
            .find(|(bound, _)| *bound == pad)
            .map(|(_, button)| *button)
    }

    pub fn ds_keys(&self, button: DsButton) -> Vec<egui::Key> {
        self.ds
            .keyboard
            .iter()
            .filter(|(_, bound)| *bound == button)
            .map(|(key, _)| *key)
            .collect()
    }

    pub fn ds_pads(&self, button: DsButton) -> Vec<PadButton> {
        self.ds
            .gamepad
            .iter()
            .filter(|(_, bound)| *bound == button)
            .map(|(pad, _)| *pad)
            .collect()
    }

    pub fn bind_ds_key(&mut self, key: egui::Key, button: DsButton) {
        self.ds.keyboard.retain(|(bound, _)| *bound != key);
        self.shoulders.keyboard.retain(|(bound, _)| *bound != key);
        self.ds.keyboard.push((key, button));
    }

    pub fn bind_ds_pad(&mut self, pad: PadButton, button: DsButton) {
        self.ds.gamepad.retain(|(bound, _)| *bound != pad);
        self.shoulders.gamepad.retain(|(bound, _)| *bound != pad);
        self.ds.gamepad.push((pad, button));
    }

    pub fn remove_key(&mut self, target: BindingTarget, key: egui::Key) {
        match target {
            BindingTarget::Ds(_) => self.ds.keyboard.retain(|(bound, _)| *bound != key),
            _ => {
                self.keyboard.retain(|(bound, _)| *bound != key);
                self.shoulders.keyboard.retain(|(bound, _)| *bound != key);
            }
        }
    }

    pub fn remove_pad(&mut self, target: BindingTarget, pad: PadButton) {
        match target {
            BindingTarget::Ds(_) => self.ds.gamepad.retain(|(bound, _)| *bound != pad),
            _ => {
                self.gamepad.retain(|(bound, _)| *bound != pad);
                self.shoulders.gamepad.retain(|(bound, _)| *bound != pad);
            }
        }
    }

    pub fn adopt_ds(&mut self) -> bool {
        if self.ds.adopted {
            return false;
        }
        self.ds.adopted = true;
        for (key, button) in DsButtons::KEYS {
            let taken = self.button_for_key(key).is_some()
                || self.shoulder_for_key(key).is_some()
                || self.ds_for_key(key).is_some();
            if !taken {
                self.ds.keyboard.push((key, button));
            }
        }
        for button in DsButton::iter() {
            let (pad, shared) = button.default_pad();
            let taken = self
                .button_for_pad(pad)
                .is_some_and(|bound| bound != shared)
                || self.shoulder_for_pad(pad).is_some()
                || self.ds_for_pad(pad).is_some();
            if !taken {
                self.ds.gamepad.push((pad, button));
            }
        }
        true
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
            BindingTarget::Ds(button) => (self.ds_keys(button), self.ds_pads(button)),
            BindingTarget::Tilt(tilt) => (self.tilt_key(tilt).into_iter().collect(), Vec::new()),
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
        self.ds.keyboard.retain(|(bound, _)| *bound != key);
        for slot in &mut self.tilt {
            if *slot == Some(key) {
                *slot = None;
            }
        }
    }

    pub fn tilt_key(&self, tilt: Tilt) -> Option<egui::Key> {
        self.tilt[tilt as usize]
    }

    pub fn tilt_for_key(&self, key: egui::Key) -> Option<Tilt> {
        Tilt::iter().find(|tilt| self.tilt_key(*tilt) == Some(key))
    }

    pub fn bind_tilt_key(&mut self, tilt: Tilt, key: egui::Key) {
        self.unbind_key(key);
        self.tilt[tilt as usize] = Some(key);
    }

    pub fn clear_tilt_key(&mut self, tilt: Tilt) {
        self.tilt[tilt as usize] = None;
    }

    pub fn buttons_are_default(&self) -> bool {
        *self
            == Self {
                tilt: self.tilt,
                ..Self::default()
            }
    }

    pub fn reset_buttons(&mut self) {
        *self = Self {
            tilt: self.tilt,
            ..Self::default()
        };
    }

    pub fn tilt_keys_are_default(&self) -> bool {
        self.tilt == default_tilt_keys()
    }

    pub fn reset_tilt_keys(&mut self) {
        self.tilt = default_tilt_keys();
    }

    pub fn unbind_pad(&mut self, pad: PadButton) {
        self.gamepad.retain(|(bound, _)| *bound != pad);
        self.shoulders.gamepad.retain(|(bound, _)| *bound != pad);
        self.ds.gamepad.retain(|(bound, _)| *bound != pad);
    }

    pub fn binds_key(&self, key: egui::Key) -> bool {
        self.input_for_key(key, true).is_some()
    }

    pub fn drop_old_default(&mut self, key: egui::Key, button: GbButton) {
        self.keyboard.retain(|bound| *bound != (key, button));
    }

    pub fn unbound(&self) -> Vec<GbButton> {
        GbButton::iter()
            .filter(|button| {
                let target = BindingTarget::Button(*button);
                self.key_combos(target).is_empty() && self.pad_combos(target).is_empty()
            })
            .collect()
    }

    fn single_key(&self, key: egui::Key, ds: bool) -> Option<GameButton> {
        if ds && let Some(button) = self.ds_for_key(key) {
            return Some(GameButton::Ds(button));
        }
        self.button_for_key(key)
            .map(GameButton::Button)
            .or_else(|| self.shoulder_for_key(key).map(GameButton::Shoulder))
    }

    fn single_pad(&self, pad: PadButton, ds: bool) -> Option<GameButton> {
        if ds && let Some(button) = self.ds_for_pad(pad) {
            return Some(GameButton::Ds(button));
        }
        self.button_for_pad(pad)
            .map(GameButton::Button)
            .or_else(|| self.shoulder_for_pad(pad).map(GameButton::Shoulder))
    }

    pub fn bound_keys(&self) -> Vec<egui::Key> {
        let mut keys: Vec<egui::Key> = Vec::new();
        let lists = self
            .keyboard
            .iter()
            .map(|(key, _)| *key)
            .chain(self.shoulders.keyboard.iter().map(|(key, _)| *key))
            .chain(self.ds.keyboard.iter().map(|(key, _)| *key));
        for key in lists {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    fn bound_pads(&self) -> Vec<PadButton> {
        let mut pads: Vec<PadButton> = Vec::new();
        let lists = self
            .gamepad
            .iter()
            .map(|(pad, _)| *pad)
            .chain(self.shoulders.gamepad.iter().map(|(pad, _)| *pad))
            .chain(self.ds.gamepad.iter().map(|(pad, _)| *pad));
        for pad in lists {
            if !pads.contains(&pad) {
                pads.push(pad);
            }
        }
        pads
    }

    pub fn key_table(&self, ds: bool) -> Vec<Bound<egui::Key, GameButton>> {
        let singles = self.bound_keys().into_iter().filter_map(|key| {
            self.single_key(key, ds)
                .map(|button| Bound::any_mods(Combo::single(key), button))
        });
        singles
            .chain(combo_table(&self.combos.keyboard, ds))
            .collect()
    }

    pub fn pad_table(&self, ds: bool) -> Vec<Bound<PadButton, GameButton>> {
        let singles = self.bound_pads().into_iter().filter_map(|pad| {
            self.single_pad(pad, ds)
                .map(|button| Bound::any_mods(Combo::single(pad), button))
        });
        singles
            .chain(combo_table(&self.combos.gamepad, ds))
            .collect()
    }

    pub fn key_combos(&self, target: BindingTarget) -> Vec<Combo<egui::Key>> {
        let (keys, _) = self.bound_inputs(target);
        let combos = combos_of(&self.combos.keyboard, target);
        keys.into_iter().map(Combo::single).chain(combos).collect()
    }

    pub fn pad_combos(&self, target: BindingTarget) -> Vec<Combo<PadButton>> {
        let (_, pads) = self.bound_inputs(target);
        let combos = combos_of(&self.combos.gamepad, target);
        pads.into_iter().map(Combo::single).chain(combos).collect()
    }

    pub fn bind_keys(&mut self, combo: Combo<egui::Key>, button: GameButton) {
        match (combo.only(), button) {
            (Some(key), GameButton::Button(button)) => self.bind_key(key, button),
            (Some(key), GameButton::Shoulder(shoulder)) => self.bind_shoulder_key(key, shoulder),
            (Some(key), GameButton::Ds(button)) => self.bind_ds_key(key, button),
            (None, _) => {
                let ds = button.is_ds();
                self.combos
                    .keyboard
                    .retain(|(bound, owner)| *bound != combo || owner.is_ds() != ds);
                self.combos.keyboard.push((combo, button));
            }
        }
    }

    pub fn bind_pads(&mut self, combo: Combo<PadButton>, button: GameButton) {
        match (combo.only(), button) {
            (Some(pad), GameButton::Button(button)) => self.bind_pad(pad, button),
            (Some(pad), GameButton::Shoulder(shoulder)) => self.bind_shoulder_pad(pad, shoulder),
            (Some(pad), GameButton::Ds(button)) => self.bind_ds_pad(pad, button),
            (None, _) => {
                let ds = button.is_ds();
                self.combos
                    .gamepad
                    .retain(|(bound, owner)| *bound != combo || owner.is_ds() != ds);
                self.combos.gamepad.push((combo, button));
            }
        }
    }

    pub fn unbind_keys(&mut self, combo: &Combo<egui::Key>) {
        match combo.only() {
            Some(key) => self.unbind_key(key),
            None => self.combos.keyboard.retain(|(bound, _)| bound != combo),
        }
    }

    pub fn unbind_pads(&mut self, combo: &Combo<PadButton>) {
        match combo.only() {
            Some(pad) => self.unbind_pad(pad),
            None => self.combos.gamepad.retain(|(bound, _)| bound != combo),
        }
    }

    pub fn remove_keys(&mut self, target: BindingTarget, combo: &Combo<egui::Key>) {
        match combo.only() {
            Some(key) => self.remove_key(target, key),
            None => self
                .combos
                .keyboard
                .retain(|(bound, owner)| bound != combo || Some(*owner) != GameButton::of(target)),
        }
    }

    pub fn remove_pads(&mut self, target: BindingTarget, combo: &Combo<PadButton>) {
        match combo.only() {
            Some(pad) => self.remove_pad(target, pad),
            None => self
                .combos
                .gamepad
                .retain(|(bound, owner)| bound != combo || Some(*owner) != GameButton::of(target)),
        }
    }

    pub fn owner_of_keys(&self, combo: &Combo<egui::Key>, ds: bool) -> Option<GameButton> {
        match combo.only() {
            Some(key) => self.single_key(key, ds),
            None => owner_in(&self.combos.keyboard, combo, ds),
        }
    }

    pub fn owner_of_pads(&self, combo: &Combo<PadButton>, ds: bool) -> Option<GameButton> {
        match combo.only() {
            Some(pad) => self.single_pad(pad, ds),
            None => owner_in(&self.combos.gamepad, combo, ds),
        }
    }

    pub fn moves_keys_from(
        &self,
        combo: &Combo<egui::Key>,
        button: GameButton,
    ) -> Option<GameButton> {
        let owner = match combo.only() {
            Some(key) => {
                let ds = self.ds_for_key(key).map(GameButton::Ds);
                let shoulder = self.shoulder_for_key(key).map(GameButton::Shoulder);
                let game = self.button_for_key(key).map(GameButton::Button);
                if button.is_ds() {
                    ds.or(shoulder)
                } else {
                    game.or(shoulder).or(ds)
                }
            }
            None => same_family(&self.combos.keyboard, combo, button),
        };
        owner.filter(|owner| *owner != button)
    }

    pub fn moves_pads_from(
        &self,
        combo: &Combo<PadButton>,
        button: GameButton,
    ) -> Option<GameButton> {
        let owner = match combo.only() {
            Some(pad) => {
                let ds = self.ds_for_pad(pad).map(GameButton::Ds);
                let shoulder = self.shoulder_for_pad(pad).map(GameButton::Shoulder);
                let game = self.button_for_pad(pad).map(GameButton::Button);
                if button.is_ds() {
                    ds.or(shoulder)
                } else {
                    game.or(shoulder).or(ds)
                }
            }
            None => same_family(&self.combos.gamepad, combo, button),
        };
        owner.filter(|owner| *owner != button)
    }

    pub fn key_in_combo(&self, key: egui::Key) -> bool {
        self.combos
            .keyboard
            .iter()
            .any(|(combo, _)| combo.contains(&key))
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn combo_table<I: Ord + Copy>(
    combos: &[(Combo<I>, GameButton)],
    ds: bool,
) -> impl Iterator<Item = Bound<I, GameButton>> + '_ {
    combos
        .iter()
        .filter(move |(combo, button)| {
            let shadowed = !button.is_ds()
                && combos
                    .iter()
                    .any(|(other, owner)| owner.is_ds() && other == combo);
            if button.is_ds() {
                ds
            } else {
                !(ds && shadowed)
            }
        })
        .map(|(combo, button)| Bound::any_mods(combo.clone(), *button))
}

fn combos_of<I: Ord + Copy>(
    combos: &[(Combo<I>, GameButton)],
    target: BindingTarget,
) -> impl Iterator<Item = Combo<I>> + '_ {
    combos
        .iter()
        .filter(move |(_, button)| Some(*button) == GameButton::of(target))
        .map(|(combo, _)| combo.clone())
}

fn same_family<I: Ord + Copy>(
    combos: &[(Combo<I>, GameButton)],
    combo: &Combo<I>,
    button: GameButton,
) -> Option<GameButton> {
    combos
        .iter()
        .find(|(bound, owner)| bound == combo && owner.is_ds() == button.is_ds())
        .map(|(_, owner)| *owner)
}

fn owner_in<I: Ord + Copy>(
    combos: &[(Combo<I>, GameButton)],
    combo: &Combo<I>,
    ds: bool,
) -> Option<GameButton> {
    let owners = || {
        combos
            .iter()
            .filter(|(bound, _)| bound == combo)
            .map(|(_, button)| *button)
    };
    owners()
        .find(|button| ds && button.is_ds())
        .or_else(|| owners().find(|button| !button.is_ds()))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingBinding {
    pub target: BindingTarget,
    pub kind: BindingKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Captured {
    Keys(Combo<egui::Key>, Mods),
    Pads(Combo<PadButton>),
}

pub const CAPTURE_GIVES_UP: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct BindingCapture {
    pub pending: Option<PendingBinding>,
    seen_keys: Vec<egui::Key>,
    seen_pads: Vec<PadButton>,
    held_keys: Vec<egui::Key>,
    held_pads: Vec<PadButton>,
    mods: Mods,
    captured: Option<Captured>,
    quiet_since: Option<web_time::Instant>,
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
        let chosen = match (self.seen_keys.is_empty(), self.seen_pads.is_empty()) {
            (false, _) => Some(BindingKind::Keyboard),
            (_, false) => Some(BindingKind::Gamepad),
            _ => None,
        };
        self.captured.is_none()
            && chosen.is_none_or(|chosen| chosen == kind)
            && self
                .pending
                .is_some_and(|p| p.kind == kind || p.kind == BindingKind::Any)
    }

    pub fn start(&mut self, target: BindingTarget, kind: BindingKind) {
        *self = Self {
            pending: Some(PendingBinding { target, kind }),
            shown: self.shown,
            ..Self::default()
        };
    }

    pub fn cancel(&mut self) {
        *self = Self {
            shown: self.shown,
            ..Self::default()
        };
    }

    pub fn press_key(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        if !self.wants(BindingKind::Keyboard) || self.held_keys.contains(&key) {
            return;
        }
        self.quiet_since = None;
        self.held_keys.push(key);
        self.mods = self.mods.union(Mods::of(modifiers));
        if !self.seen_keys.contains(&key) && self.seen_keys.len() < crate::combo::MOST_INPUTS {
            self.seen_keys.push(key);
        }
        let single = self.pending.is_some_and(|pending| {
            matches!(
                pending.target,
                BindingTarget::LinkKey(..) | BindingTarget::Tilt(_)
            )
        });
        if single {
            self.held_keys.clear();
            self.captured = Some(Captured::Keys(Combo::single(key), self.mods));
        }
    }

    pub fn release_key(&mut self, key: egui::Key) -> bool {
        if !self.held_keys.contains(&key) {
            return false;
        }
        self.held_keys.retain(|held| *held != key);
        if self.held_keys.is_empty() {
            self.captured = Combo::new(self.seen_keys.iter().copied())
                .map(|combo| Captured::Keys(combo, self.mods));
        }
        true
    }

    pub fn press_pad(&mut self, pad: PadButton) {
        if !self.wants(BindingKind::Gamepad) || self.held_pads.contains(&pad) {
            return;
        }
        self.quiet_since = None;
        self.held_pads.push(pad);
        if !self.seen_pads.contains(&pad) && self.seen_pads.len() < crate::combo::MOST_INPUTS {
            self.seen_pads.push(pad);
        }
    }

    pub fn release_pad(&mut self, pad: PadButton) -> bool {
        if !self.held_pads.contains(&pad) {
            return false;
        }
        self.held_pads.retain(|held| *held != pad);
        if self.held_pads.is_empty() {
            self.captured = Combo::new(self.seen_pads.iter().copied()).map(Captured::Pads);
        }
        true
    }

    pub fn holds_pad(&self, pad: PadButton) -> bool {
        self.held_pads.contains(&pad)
    }

    pub fn holding(&self) -> Option<Captured> {
        if let Some(combo) = Combo::new(self.seen_keys.iter().copied()) {
            return Some(Captured::Keys(combo, self.mods));
        }
        Combo::new(self.seen_pads.iter().copied()).map(Captured::Pads)
    }

    pub fn give_up_if_quiet(&mut self, now: web_time::Instant) {
        let busy = !self.held_keys.is_empty() || !self.held_pads.is_empty();
        if !self.is_waiting() || busy || self.captured.is_some() {
            self.quiet_since = None;
            return;
        }
        let since = *self.quiet_since.get_or_insert(now);
        if now.duration_since(since) >= CAPTURE_GIVES_UP {
            self.cancel();
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
        let target = self.pending?.target;
        self.cancel();
        Some((target, captured))
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
        assert_eq!(bindings.input_for_key(Key::Num1, false), Some(Keys::L));
        assert_eq!(bindings.input_for_key(Key::Num3, false), Some(Keys::R));
        assert_eq!(bindings.input_for_key(Key::Q, false), Some(Keys::A));
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
        assert_eq!(bindings.input_for_key(Key::Q, false), Some(Keys::L));

        bindings.bind_key(Key::Num1, GbButton::B);
        assert_eq!(bindings.shoulder_keys(Shoulder::L), [Key::Q]);
        assert_eq!(bindings.input_for_key(Key::Num1, false), Some(Keys::B));

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
        assert_eq!(bindings.input_for_key(Key::Num1, false), Some(Keys::SELECT));
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

    #[test]
    fn x_and_y_have_their_own_defaults_on_the_ds_only() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.ds_keys(DsButton::X), [Key::C]);
        assert_eq!(bindings.ds_keys(DsButton::Y), [Key::V]);
        assert_eq!(bindings.ds_pads(DsButton::X), [PadButton::North]);
        assert_eq!(bindings.ds_pads(DsButton::Y), [PadButton::West]);
        assert_eq!(bindings.input_for_key(Key::C, true), Some(Keys::X));
        assert_eq!(bindings.input_for_key(Key::V, true), Some(Keys::Y));
        assert_eq!(bindings.input_for_key(Key::C, false), None);
        assert_eq!(bindings.input_for_key(Key::Q, true), Some(Keys::A));
        assert_eq!(
            bindings.button_for_pad(PadButton::North),
            Some(GbButton::B),
            "north stays B off the ds"
        );
        assert_eq!(bindings.button_for_pad(PadButton::West), Some(GbButton::A));
        let hotkeys = crate::app::ui_state::hotkeys::Hotkeys::default();
        for key in [Key::C, Key::V] {
            for modifiers in [
                egui::Modifiers::NONE,
                egui::Modifiers::SHIFT,
                egui::Modifiers::CTRL,
            ] {
                assert_eq!(hotkeys.action_for(key, modifiers), None, "{key:?}");
            }
        }
    }

    #[test]
    fn a_ds_binding_keeps_the_game_boy_one_and_a_game_boy_binding_takes_it() {
        let mut bindings = KeyBindings::default();
        bindings.bind_ds_pad(PadButton::South, DsButton::Y);
        assert_eq!(bindings.ds_for_pad(PadButton::South), Some(DsButton::Y));
        assert_eq!(bindings.button_for_pad(PadButton::South), Some(GbButton::B));

        bindings.remove_pad(BindingTarget::Ds(DsButton::Y), PadButton::South);
        assert_eq!(bindings.ds_for_pad(PadButton::South), None);
        assert_eq!(bindings.button_for_pad(PadButton::South), Some(GbButton::B));

        bindings.remove_pad(BindingTarget::Button(GbButton::B), PadButton::North);
        assert_eq!(bindings.button_for_pad(PadButton::North), None);
        assert_eq!(bindings.ds_for_pad(PadButton::North), Some(DsButton::X));

        bindings.bind_key(Key::C, GbButton::Start);
        assert_eq!(bindings.input_for_key(Key::C, true), Some(Keys::START));
        assert!(bindings.ds_keys(DsButton::X).is_empty());

        bindings.bind_shoulder_key(Key::V, Shoulder::L);
        assert_eq!(bindings.input_for_key(Key::V, true), Some(Keys::L));
    }

    #[test]
    fn old_settings_keep_their_binds_and_get_x_and_y_where_free() {
        let saved = r#"{"keyboard":[["C","Start"],["W","Up"]],"gamepad":[["North","Select"],["West","A"]],"stick_as_dpad":true}"#;
        let mut bindings: KeyBindings = serde_json::from_str(saved).expect("loads");
        let before = bindings.clone();
        assert!(bindings.adopt_ds());
        assert_eq!(bindings.keyboard, before.keyboard);
        assert_eq!(bindings.gamepad, before.gamepad);
        assert_eq!(bindings.shoulders, before.shoulders);
        assert_eq!(bindings.input_for_key(Key::C, true), Some(Keys::START));
        assert!(bindings.ds_keys(DsButton::X).is_empty());
        assert_eq!(bindings.ds_keys(DsButton::Y), [Key::V]);
        assert!(bindings.ds_pads(DsButton::X).is_empty());
        assert_eq!(bindings.ds_pads(DsButton::Y), [PadButton::West]);
        assert_eq!(
            bindings.button_for_pad(PadButton::North),
            Some(GbButton::Select)
        );

        bindings.unbind_key(Key::V);
        assert!(!bindings.adopt_ds());
        assert!(bindings.ds_keys(DsButton::Y).is_empty());
        let mut fresh = KeyBindings::default();
        assert!(!fresh.adopt_ds());
        assert_eq!(fresh, KeyBindings::default());
    }

    #[test]
    fn tilt_keys_start_on_i_j_k_l_and_move_when_rebound() {
        let mut bindings = KeyBindings::default();
        assert_eq!(bindings.tilt_for_key(Key::J), Some(Tilt::Left));
        assert_eq!(bindings.tilt_key(Tilt::Up), Some(Key::I));

        bindings.bind_tilt_key(Tilt::Left, Key::G);
        assert_eq!(bindings.tilt_for_key(Key::G), Some(Tilt::Left));
        assert_eq!(bindings.tilt_for_key(Key::J), None);

        bindings.bind_tilt_key(Tilt::Right, Key::G);
        assert_eq!(bindings.tilt_key(Tilt::Left), None);
        assert_eq!(bindings.tilt_key(Tilt::Right), Some(Key::G));

        bindings.bind_key(Key::I, GbButton::A);
        assert_eq!(
            bindings.tilt_key(Tilt::Up),
            None,
            "a game button takes the key"
        );
        assert_eq!(bindings.tilt_for_key(Key::K), Some(Tilt::Down));
    }

    #[test]
    fn binds_saved_before_tilt_keys_tilt_with_i_j_k_l() {
        let saved = r#"{"keyboard":[["W","Up"]],"stick_as_dpad":true}"#;
        let bindings: KeyBindings = serde_json::from_str(saved).expect("loads");
        let keys: Vec<Option<Key>> = Tilt::iter().map(|tilt| bindings.tilt_key(tilt)).collect();
        assert_eq!(
            keys,
            [Some(Key::I), Some(Key::J), Some(Key::K), Some(Key::L)]
        );
    }

    #[test]
    fn a_build_without_ds_buttons_loads_binds_saved_with_x_and_y() {
        let mut bindings = KeyBindings::default();
        bindings.bind_ds_key(Key::K, DsButton::X);
        bindings.bind_key(Key::J, GbButton::A);
        let saved = serde_json::to_string(&bindings).expect("saves");
        assert!(saved.contains(r#""ds":"#));

        let old: BindingsBeforeShoulders = serde_json::from_str(&saved).expect("loads");
        assert!(old.keyboard.contains(&(Key::J, GbButton::A)));
        assert!(old.keyboard.iter().all(|(key, _)| *key != Key::K));
        assert_eq!(old.gamepad.len(), KeyBindings::default().gamepad.len());

        let loaded: KeyBindings = serde_json::from_str(&saved).expect("loads");
        assert_eq!(loaded, bindings);
    }

    fn pads(list: &[PadButton]) -> Combo<PadButton> {
        Combo::new(list.iter().copied()).expect("a combo")
    }

    fn keys(list: &[Key]) -> Combo<Key> {
        Combo::new(list.iter().copied()).expect("a combo")
    }

    const MENU_PADS: [PadButton; 3] = [
        PadButton::LeftTrigger,
        PadButton::RightTrigger,
        PadButton::Select,
    ];

    #[test]
    fn holding_buttons_and_letting_go_captures_them_all() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::A), BindingKind::Any);
        for pad in MENU_PADS {
            capture.press_pad(pad);
        }
        assert_eq!(capture.holding(), Some(Captured::Pads(pads(&MENU_PADS))));
        assert!(capture.release_pad(PadButton::Select));
        assert!(capture.take().is_none(), "two buttons are still down");
        capture.release_pad(PadButton::LeftTrigger);
        capture.release_pad(PadButton::RightTrigger);
        assert_eq!(
            capture.take(),
            Some((
                BindingTarget::Button(GbButton::A),
                Captured::Pads(pads(&MENU_PADS))
            ))
        );
        assert!(!capture.is_waiting());
    }

    #[test]
    fn keys_capture_with_their_modifiers() {
        let mut capture = BindingCapture::default();
        let target =
            BindingTarget::Shortcut(crate::app::ui_state::hotkeys::HotkeyAction::ToggleMute);
        capture.start(target, BindingKind::Any);
        let ctrl_shift = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        capture.press_key(Key::M, ctrl_shift);
        assert!(
            !capture.wants(BindingKind::Gamepad),
            "no keys and buttons mixed"
        );
        capture.press_pad(PadButton::South);
        capture.release_key(Key::M);
        let ctrl_shift = Mods {
            ctrl: true,
            shift: true,
            alt: false,
        };
        assert_eq!(
            capture.take(),
            Some((target, Captured::Keys(keys(&[Key::M]), ctrl_shift)))
        );
    }

    #[test]
    fn a_button_held_before_the_capture_is_not_part_of_it() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::B), BindingKind::Gamepad);
        assert!(
            !capture.release_pad(PadButton::South),
            "the press that opened the capture lets go"
        );
        assert!(capture.is_waiting());
        capture.press_pad(PadButton::North);
        capture.release_pad(PadButton::North);
        assert_eq!(
            capture.take().map(|(_, captured)| captured),
            Some(Captured::Pads(Combo::single(PadButton::North)))
        );
    }

    #[test]
    fn a_split_keyboard_key_saves_on_its_press() {
        let mut capture = BindingCapture::default();
        let target = BindingTarget::LinkKey(crate::local_link::Half::Left, GbButton::A);
        capture.start(target, BindingKind::Keyboard);
        capture.press_key(Key::Q, egui::Modifiers::NONE);
        assert!(
            !capture.release_key(Key::Q),
            "the release goes on to the game"
        );
        assert_eq!(
            capture.take(),
            Some((target, Captured::Keys(Combo::single(Key::Q), Mods::NONE)))
        );
    }

    #[test]
    fn a_capture_takes_at_most_four_inputs() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::B), BindingKind::Keyboard);
        for key in [Key::A, Key::B, Key::C, Key::D, Key::E] {
            capture.press_key(key, egui::Modifiers::NONE);
        }
        for key in [Key::A, Key::B, Key::C, Key::D, Key::E] {
            capture.release_key(key);
        }
        assert_eq!(
            capture.take().map(|(_, captured)| captured),
            Some(Captured::Keys(
                keys(&[Key::A, Key::B, Key::C, Key::D]),
                Mods::NONE
            ))
        );
    }

    #[test]
    fn a_capture_with_nothing_pressed_gives_up_after_five_seconds() {
        let start = web_time::Instant::now();
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::B), BindingKind::Gamepad);
        capture.give_up_if_quiet(start);
        capture.give_up_if_quiet(start + std::time::Duration::from_secs(4));
        assert!(capture.is_waiting());
        capture.press_pad(PadButton::North);
        capture.give_up_if_quiet(start + std::time::Duration::from_secs(20));
        assert!(capture.is_waiting(), "a held button keeps it open");
        capture.cancel();
        capture.start(BindingTarget::Button(GbButton::B), BindingKind::Gamepad);
        capture.give_up_if_quiet(start);
        capture.give_up_if_quiet(start + CAPTURE_GIVES_UP);
        assert!(!capture.is_waiting());
    }

    #[test]
    fn a_combo_binds_next_to_the_single_keys() {
        let mut bindings = KeyBindings::default();
        let menu = pads(&MENU_PADS);
        bindings.bind_pads(menu.clone(), GameButton::Button(GbButton::Start));
        bindings.bind_keys(
            keys(&[Key::S, Key::D]),
            GameButton::Button(GbButton::Select),
        );
        assert_eq!(
            bindings.pad_combos(BindingTarget::Button(GbButton::Start)),
            [Combo::single(PadButton::Start), menu.clone()]
        );
        assert_eq!(
            bindings.button_for_key(Key::S),
            Some(GbButton::Down),
            "a combo leaves its keys' own bindings alone"
        );
        bindings.bind_pads(menu.clone(), GameButton::Button(GbButton::A));
        assert_eq!(
            bindings.owner_of_pads(&menu, false),
            Some(GameButton::Button(GbButton::A)),
            "the same combo twice moves it"
        );
        assert_eq!(
            bindings.pad_combos(BindingTarget::Button(GbButton::Start)),
            [Combo::single(PadButton::Start)]
        );
        bindings.remove_pads(BindingTarget::Button(GbButton::A), &menu);
        assert_eq!(bindings.owner_of_pads(&menu, false), None);
    }

    #[test]
    fn the_tables_hold_singles_and_combos_and_ds_only_on_the_ds() {
        let mut bindings = KeyBindings::default();
        let menu = pads(&MENU_PADS);
        bindings.bind_pads(menu.clone(), GameButton::Ds(DsButton::X));
        let off = bindings.pad_table(false);
        assert!(off.iter().all(|bound| bound.combo != menu));
        assert!(
            off.iter()
                .any(|bound| bound.combo == Combo::single(PadButton::North)
                    && bound.target == GameButton::Button(GbButton::B))
        );
        let on = bindings.pad_table(true);
        assert!(on.iter().any(|bound| bound.combo == menu));
        assert!(
            on.iter()
                .any(|bound| bound.combo == Combo::single(PadButton::North)
                    && bound.target == GameButton::Ds(DsButton::X))
        );
        assert!(on.iter().all(|bound| bound.mods.is_none()));
    }

    #[test]
    fn combos_save_and_older_builds_read_the_single_binds_only() {
        let mut bindings = KeyBindings::default();
        bindings.bind_keys(
            keys(&[Key::S, Key::D]),
            GameButton::Button(GbButton::Select),
        );
        bindings.bind_pads(pads(&MENU_PADS), GameButton::Shoulder(Shoulder::L));
        let saved = serde_json::to_string(&bindings).expect("saves");
        let loaded: KeyBindings = serde_json::from_str(&saved).expect("loads");
        assert_eq!(loaded, bindings);
        let old: BindingsBeforeShoulders = serde_json::from_str(&saved).expect("loads");
        assert_eq!(old.keyboard, KeyBindings::default().keyboard);
        assert_eq!(old.gamepad, KeyBindings::default().gamepad);
    }

    #[test]
    fn settings_from_before_combos_load_unchanged() {
        let saved = r#"{"keyboard":[["W","Up"]],"gamepad":[["East","A"]],"stick_as_dpad":true}"#;
        let bindings: KeyBindings = serde_json::from_str(saved).expect("loads");
        assert_eq!(bindings.combos, GameCombos::default());
        assert_eq!(
            bindings.key_combos(BindingTarget::Button(GbButton::Up)),
            [Combo::single(Key::W)]
        );
        assert_eq!(
            bindings.key_table(false),
            [
                Bound::any_mods(Combo::single(Key::W), GameButton::Button(GbButton::Up)),
                Bound::any_mods(Combo::single(Key::Num1), GameButton::Shoulder(Shoulder::L)),
                Bound::any_mods(Combo::single(Key::Num3), GameButton::Shoulder(Shoulder::R)),
            ]
        );
    }

    #[test]
    fn a_button_bound_only_to_a_combo_counts_as_bound() {
        let mut bindings = KeyBindings::default();
        for key in bindings.keys_for(GbButton::Start) {
            bindings.unbind_key(key);
        }
        for pad in bindings.pads_for(GbButton::Start) {
            bindings.unbind_pad(pad);
        }
        bindings.bind_keys(keys(&[Key::S, Key::D]), GameButton::Button(GbButton::Start));
        assert!(bindings.unbound().is_empty());
    }
}
