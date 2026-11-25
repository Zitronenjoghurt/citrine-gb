use crate::app::ui_state::hotkeys::{HotkeyAction, Hotkeys};
use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, DsButton, Fire, GameButton, GbButton, KeyBindings, PadButton,
    Shoulder,
};
use crate::combo::{Bound, Chords, Edge, Mods};
use crate::controller_debug::PadInfo;
use std::fmt::{Display, Formatter};

#[cfg(any(target_os = "android", test))]
pub mod android_map;
#[cfg(not(any(windows, target_os = "android")))]
mod gilrs_source;
#[cfg(any(windows, test))]
pub(crate) mod sdl_map;
#[cfg(windows)]
mod sdl_source;

pub const STICK_HYSTERESIS: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadKey {
    Known(PadButton),
    Other(&'static str),
}

impl Display for PadKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Known(button) => write!(f, "{button:?}"),
            Self::Other(name) => f.write_str(name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadAxis {
    LeftStickX,
    LeftStickY,
    LeftZ,
    RightStickX,
    RightStickY,
    RightZ,
    DPadX,
    DPadY,
    Other,
}

impl PadAxis {
    pub fn is_left_stick(self) -> bool {
        matches!(self, Self::LeftStickX | Self::LeftStickY)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadInput {
    Pressed(PadKey),
    Repeated(PadKey),
    Released(PadKey),
    Changed(PadKey, f32),
    Moved(PadAxis, f32),
    Connected,
    Disconnected,
    Dropped,
    EffectDone,
    Other,
}

impl Display for PadInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pressed(key) => write!(f, "{key} pressed"),
            Self::Repeated(key) => write!(f, "{key} repeated"),
            Self::Released(key) => write!(f, "{key} released"),
            Self::Changed(key, value) => write!(f, "{key} at {value:.2}"),
            Self::Moved(axis, value) => write!(f, "{axis:?} at {value:+.2}"),
            Self::Connected => f.write_str("connected"),
            Self::Disconnected => f.write_str("disconnected"),
            Self::Dropped => f.write_str("dropped"),
            Self::EffectDone => f.write_str("rumble finished"),
            Self::Other => f.write_str("other event"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PadEvent {
    pub pad: usize,
    pub input: PadInput,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PadMotion {
    pub accel: [f32; 3],
    pub gyro: [f32; 3],
}

pub trait PadSource {
    fn next_event(&mut self) -> Option<PadEvent>;
    fn pads(&self, seen: &[usize]) -> Vec<PadInfo>;
    fn api(&self) -> String;
    fn rumble(&mut self, _strength: f32) {}
    fn motion(&mut self) -> Option<PadMotion> {
        None
    }
}

#[cfg(not(any(windows, target_os = "android")))]
pub fn open_source() -> Box<dyn PadSource> {
    Box::new(gilrs_source::GilrsPads::new())
}

#[cfg(target_os = "android")]
pub fn open_source() -> Box<dyn PadSource> {
    Box::new(crate::platform::android::pads::AndroidPads)
}

#[cfg(windows)]
pub fn open_source() -> Box<dyn PadSource> {
    Box::new(sdl_source::SdlPads::new())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    UnknownButton,
    Unbound,
    StickOff,
    UnusedAxis,
    StateOnly,
    NotInput,
    InCombo,
}

impl Display for Ignored {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownButton => f.write_str("a button Citrine can't bind"),
            Self::Unbound => f.write_str("not bound"),
            Self::StickOff => f.write_str("Left stick steers is off"),
            Self::UnusedAxis => f.write_str("Citrine doesn't use this axis"),
            Self::StateOnly => f.write_str("Citrine reads presses and releases only"),
            Self::NotInput => f.write_str("not a button or axis"),
            Self::InCombo => f.write_str("part of a combination"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadAction {
    Press(GbButton),
    Release(GbButton),
    Shoulder(Shoulder, bool),
    Ds(DsButton, bool),
    Steer {
        on: Option<GbButton>,
        off: &'static [GbButton],
    },
    Button(PadButton, bool),
    Shortcut(HotkeyAction, bool),
    Capture(PadButton, bool),
    Unplugged,
    Ignore(Ignored),
}

impl PadAction {
    pub fn describe(&self, changed: bool) -> String {
        let text = match self {
            Self::Press(button) => format!("Game Boy {button} down"),
            Self::Release(button) => format!("Game Boy {button} up"),
            Self::Shoulder(shoulder, true) => format!("{shoulder} down"),
            Self::Shoulder(shoulder, false) => format!("{shoulder} up"),
            Self::Ds(button, true) => format!("{button} down"),
            Self::Ds(button, false) => format!("{button} up"),
            Self::Steer { on: Some(on), off } => {
                let off: Vec<String> = off.iter().map(|b| b.to_string()).collect();
                format!("Game Boy {on} down, {} up", off.join(" and "))
            }
            Self::Steer { on: None, off } => {
                let off: Vec<String> = off.iter().map(|b| b.to_string()).collect();
                format!("Game Boy {} up", off.join(" and "))
            }
            Self::Button(pad, true) => format!("{} down", pad.short_name()),
            Self::Button(pad, false) => format!("{} up", pad.short_name()),
            Self::Shortcut(action, true) => format!("{action} down"),
            Self::Shortcut(action, false) => format!("{action} up"),
            Self::Capture(pad, _) => {
                return format!("taken by the binding being set ({})", pad.short_name());
            }
            Self::Unplugged => "Game Boy buttons it held up".to_string(),
            Self::Ignore(reason) => return format!("ignored: {reason}"),
        };
        if changed {
            text
        } else {
            format!("{text} (no change)")
        }
    }
}

pub fn route(
    input: PadInput,
    bindings: &KeyBindings,
    capture: &BindingCapture,
    steering: Option<GbButton>,
) -> PadAction {
    match input {
        PadInput::Pressed(PadKey::Known(pad)) if capture.wants(BindingKind::Gamepad) => {
            PadAction::Capture(pad, true)
        }
        PadInput::Released(PadKey::Known(pad)) if capture.holds_pad(pad) => {
            PadAction::Capture(pad, false)
        }
        PadInput::Pressed(PadKey::Known(pad)) => PadAction::Button(pad, true),
        PadInput::Released(PadKey::Known(pad)) => PadAction::Button(pad, false),
        PadInput::Pressed(_) | PadInput::Released(_) => PadAction::Ignore(Ignored::UnknownButton),
        PadInput::Moved(axis, value) => steer(axis, value, bindings, steering),
        PadInput::Repeated(_) | PadInput::Changed(..) => PadAction::Ignore(Ignored::StateOnly),
        PadInput::Disconnected => PadAction::Unplugged,
        _ => PadAction::Ignore(Ignored::NotInput),
    }
}

pub type PadTable = Vec<Bound<PadButton, Fire<GameButton>>>;

pub fn pad_table(bindings: &KeyBindings, hotkeys: &Hotkeys, ds: bool) -> PadTable {
    let game: PadTable = bindings
        .pad_table(ds)
        .into_iter()
        .map(|bound| bound.map(Fire::Button))
        .collect();
    let shortcuts: PadTable = hotkeys
        .pad_table()
        .into_iter()
        .filter(|shortcut| game.iter().all(|bound| bound.combo != shortcut.combo))
        .map(|bound| bound.map(Fire::Shortcut))
        .collect();
    game.into_iter().chain(shortcuts).collect()
}

#[derive(Debug, Default)]
pub struct PadChords(Vec<(usize, Chords<PadButton, Fire<GameButton>>)>);

impl PadChords {
    pub fn change(
        &mut self,
        pad: usize,
        button: PadButton,
        down: bool,
        table: &[Bound<PadButton, Fire<GameButton>>],
    ) -> Vec<Edge<Fire<GameButton>>> {
        let chords = match self.0.iter().position(|(index, _)| *index == pad) {
            Some(at) => &mut self.0[at].1,
            None => {
                self.0.push((pad, Chords::default()));
                &mut self.0.last_mut().expect("just pushed").1
            }
        };
        if down {
            chords.press(button, Mods::NONE, table)
        } else {
            chords.release(button)
        }
    }

    pub fn forget(&mut self, pad: usize) {
        self.0.retain(|(index, _)| *index != pad);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn holds(&self, target: Fire<GameButton>) -> bool {
        self.0.iter().any(|(_, chords)| chords.holds(target))
    }
}

pub fn game_action(button: GameButton, down: bool) -> PadAction {
    match (button, down) {
        (GameButton::Button(button), true) => PadAction::Press(button),
        (GameButton::Button(button), false) => PadAction::Release(button),
        (GameButton::Shoulder(shoulder), down) => PadAction::Shoulder(shoulder, down),
        (GameButton::Ds(button), down) => PadAction::Ds(button, down),
    }
}

pub fn resolved(
    button: PadButton,
    down: bool,
    edges: &[Edge<Fire<GameButton>>],
    table: &[Bound<PadButton, Fire<GameButton>>],
) -> PadAction {
    let fired = |fire: Fire<GameButton>, down: bool| match fire {
        Fire::Button(game) => game_action(game, down),
        Fire::Shortcut(action) => PadAction::Shortcut(action, down),
    };
    let first_down = edges.iter().find_map(|edge| match edge {
        Edge::Down(fire) => Some(*fire),
        Edge::Up(_) => None,
    });
    if let Some(fire) = first_down {
        return fired(fire, true);
    }
    if let Some(Edge::Up(fire)) = edges.first() {
        return fired(*fire, false);
    }
    let single = table
        .iter()
        .find(|bound| bound.combo.only() == Some(button));
    match single {
        Some(bound) => fired(bound.target, down),
        _ if table.iter().any(|bound| bound.combo.contains(&button)) => {
            PadAction::Ignore(Ignored::InCombo)
        }
        _ => PadAction::Ignore(Ignored::Unbound),
    }
}

#[cfg(test)]
pub(crate) fn route_alone(
    input: PadInput,
    bindings: &KeyBindings,
    capture: &BindingCapture,
    ds: bool,
) -> PadAction {
    match route(input, bindings, capture, None) {
        PadAction::Button(button, down) => {
            let table = pad_table(bindings, &Hotkeys::default(), ds);
            let edges = PadChords::default().change(0, button, down, &table);
            resolved(button, down, &edges, &table)
        }
        action => action,
    }
}

fn steer(
    axis: PadAxis,
    value: f32,
    bindings: &KeyBindings,
    steering: Option<GbButton>,
) -> PadAction {
    let (negative, positive) = match axis {
        PadAxis::DPadX | PadAxis::LeftStickX => (GbButton::Left, GbButton::Right),
        PadAxis::DPadY | PadAxis::LeftStickY => (GbButton::Down, GbButton::Up),
        _ => return PadAction::Ignore(Ignored::UnusedAxis),
    };
    if !bindings.stick_as_dpad {
        return PadAction::Ignore(Ignored::StickOff);
    }
    let press = bindings.deadzone();
    let past = |button: GbButton, reach: f32| {
        let limit = if steering == Some(button) {
            press - STICK_HYSTERESIS
        } else {
            press
        };
        reach > limit
    };
    if past(negative, -value) {
        PadAction::Steer {
            on: Some(negative),
            off: opposite(negative),
        }
    } else if past(positive, value) {
        PadAction::Steer {
            on: Some(positive),
            off: opposite(positive),
        }
    } else {
        PadAction::Steer {
            on: None,
            off: centre(negative),
        }
    }
}

fn opposite(button: GbButton) -> &'static [GbButton] {
    match button {
        GbButton::Left => &[GbButton::Right],
        GbButton::Right => &[GbButton::Left],
        GbButton::Up => &[GbButton::Down],
        _ => &[GbButton::Up],
    }
}

fn centre(button: GbButton) -> &'static [GbButton] {
    match button {
        GbButton::Left | GbButton::Right => &[GbButton::Left, GbButton::Right],
        _ => &[GbButton::Up, GbButton::Down],
    }
}

#[cfg(test)]
pub(crate) struct FakePads(pub(crate) std::collections::VecDeque<PadEvent>);

#[cfg(test)]
impl PadSource for FakePads {
    fn next_event(&mut self) -> Option<PadEvent> {
        self.0.pop_front()
    }

    fn pads(&self, _seen: &[usize]) -> Vec<PadInfo> {
        Vec::new()
    }

    fn api(&self) -> String {
        "fake".to_string()
    }
}

#[cfg(test)]
pub(crate) fn fake(events: &[(usize, PadInput)]) -> FakePads {
    FakePads(
        events
            .iter()
            .map(|&(pad, input)| PadEvent {
                pad,
                input,
                code: None,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::BindingTarget;

    fn route_default(input: PadInput) -> PadAction {
        route_alone(
            input,
            &KeyBindings::default(),
            &BindingCapture::default(),
            false,
        )
    }

    #[test]
    fn the_shoulder_buttons_press_l_and_r() {
        assert_eq!(
            route_default(PadInput::Pressed(PadKey::Known(PadButton::LeftTrigger))),
            PadAction::Shoulder(Shoulder::L, true)
        );
        assert_eq!(
            route_default(PadInput::Released(PadKey::Known(PadButton::RightTrigger))),
            PadAction::Shoulder(Shoulder::R, false)
        );
        assert_eq!(
            PadAction::Shoulder(Shoulder::L, true).describe(true),
            "L down"
        );
    }

    #[test]
    fn the_top_and_left_face_buttons_press_x_and_y_on_the_ds() {
        let route_on = |input: PadInput, ds: bool| {
            route_alone(
                input,
                &KeyBindings::default(),
                &BindingCapture::default(),
                ds,
            )
        };
        let north = PadInput::Pressed(PadKey::Known(PadButton::North));
        let west = PadInput::Released(PadKey::Known(PadButton::West));
        assert_eq!(route_on(north, true), PadAction::Ds(DsButton::X, true));
        assert_eq!(route_on(west, true), PadAction::Ds(DsButton::Y, false));
        assert_eq!(route_on(north, false), PadAction::Press(GbButton::B));
        assert_eq!(route_on(west, false), PadAction::Release(GbButton::A));
        let east = PadInput::Pressed(PadKey::Known(PadButton::East));
        assert_eq!(route_on(east, true), PadAction::Press(GbButton::A));
        assert_eq!(PadAction::Ds(DsButton::X, true).describe(true), "DS X down");
    }

    #[test]
    fn a_bound_button_presses_its_game_boy_button() {
        assert_eq!(
            route_default(PadInput::Pressed(PadKey::Known(PadButton::East))),
            PadAction::Press(GbButton::A)
        );
        assert_eq!(
            route_default(PadInput::Released(PadKey::Known(PadButton::DPadUp))),
            PadAction::Release(GbButton::Up)
        );
    }

    #[test]
    fn an_unbound_button_says_so() {
        assert_eq!(
            route_default(PadInput::Pressed(PadKey::Known(PadButton::LeftThumb))),
            PadAction::Ignore(Ignored::Unbound)
        );
        assert_eq!(
            route_default(PadInput::Pressed(PadKey::Other("Unknown"))),
            PadAction::Ignore(Ignored::UnknownButton)
        );
    }

    #[test]
    fn a_waiting_binding_takes_the_buttons_it_saw_go_down() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::A), BindingKind::Any);
        let bindings = KeyBindings::default();
        let south = PadKey::Known(PadButton::South);
        assert_eq!(
            route(PadInput::Pressed(south), &bindings, &capture, None),
            PadAction::Capture(PadButton::South, true)
        );
        capture.press_pad(PadButton::South);
        assert_eq!(
            route(PadInput::Released(south), &bindings, &capture, None),
            PadAction::Capture(PadButton::South, false)
        );
        assert_eq!(
            route_alone(
                PadInput::Released(PadKey::Known(PadButton::North)),
                &bindings,
                &capture,
                false
            ),
            PadAction::Release(GbButton::B),
            "a button held from before the capture lets go in the game"
        );
    }

    #[test]
    fn a_combo_waits_and_then_takes_its_buttons_from_the_game() {
        let mut bindings = KeyBindings::default();
        let menu = crate::combo::Combo::new([
            PadButton::LeftTrigger,
            PadButton::RightTrigger,
            PadButton::Select,
        ])
        .expect("a combo");
        bindings.bind_pads(menu, GameButton::Button(GbButton::Start));
        let table = pad_table(&bindings, &Hotkeys::default(), false);
        let mut chords = PadChords::default();
        let mut press = |button: PadButton| {
            let edges = chords.change(1, button, true, &table);
            resolved(button, true, &edges, &table)
        };
        assert_eq!(
            press(PadButton::LeftTrigger),
            PadAction::Shoulder(Shoulder::L, true)
        );
        assert_eq!(
            press(PadButton::RightTrigger),
            PadAction::Shoulder(Shoulder::R, true)
        );
        assert_eq!(press(PadButton::Select), PadAction::Press(GbButton::Start));
        assert!(chords.holds(Fire::Button(GameButton::Button(GbButton::Start))));
        assert!(!chords.holds(Fire::Button(GameButton::Shoulder(Shoulder::L))));
        chords.forget(1);
        assert!(!chords.holds(Fire::Button(GameButton::Button(GbButton::Start))));
    }

    #[test]
    fn a_pad_shortcut_fires_only_on_a_button_the_game_does_not_use() {
        let mut hotkeys = Hotkeys::default();
        hotkeys.bind_pad(PadButton::Mode, HotkeyAction::ToggleMute);
        hotkeys.bind_pad(PadButton::East, HotkeyAction::TogglePause);
        let table = pad_table(&KeyBindings::default(), &hotkeys, false);
        let mut chords = PadChords::default();
        let edges = chords.change(0, PadButton::Mode, true, &table);
        assert_eq!(
            edges,
            [Edge::Down(Fire::Shortcut(HotkeyAction::ToggleMute))]
        );
        assert_eq!(
            resolved(PadButton::Mode, true, &edges, &table),
            PadAction::Shortcut(HotkeyAction::ToggleMute, true)
        );
        let edges = chords.change(0, PadButton::East, true, &table);
        assert_eq!(
            edges,
            [Edge::Down(Fire::Button(GameButton::Button(GbButton::A)))]
        );
    }

    #[test]
    fn the_stick_steers_only_when_switched_on() {
        let mut bindings = KeyBindings::default();
        let capture = BindingCapture::default();
        assert_eq!(
            route(
                PadInput::Moved(PadAxis::LeftStickX, -0.9),
                &bindings,
                &capture,
                None,
            ),
            PadAction::Steer {
                on: Some(GbButton::Left),
                off: &[GbButton::Right]
            }
        );
        assert_eq!(
            route(
                PadInput::Moved(PadAxis::LeftStickY, 0.1),
                &bindings,
                &capture,
                None,
            ),
            PadAction::Steer {
                on: None,
                off: &[GbButton::Up, GbButton::Down]
            }
        );
        assert_eq!(
            route(
                PadInput::Moved(PadAxis::RightStickX, 1.0),
                &bindings,
                &capture,
                None,
            ),
            PadAction::Ignore(Ignored::UnusedAxis)
        );

        bindings.stick_as_dpad = false;
        assert_eq!(
            route(
                PadInput::Moved(PadAxis::LeftStickX, -0.9),
                &bindings,
                &capture,
                None,
            ),
            PadAction::Ignore(Ignored::StickOff)
        );
    }

    #[test]
    fn the_stick_lets_go_only_well_inside_the_dead_zone() {
        let bindings = KeyBindings::default();
        let capture = BindingCapture::default();
        let at = |value: f32, steering: Option<GbButton>| {
            route(
                PadInput::Moved(PadAxis::LeftStickX, value),
                &bindings,
                &capture,
                steering,
            )
        };
        let centred = PadAction::Steer {
            on: None,
            off: &[GbButton::Left, GbButton::Right],
        };
        let right = PadAction::Steer {
            on: Some(GbButton::Right),
            off: &[GbButton::Left],
        };
        assert_eq!(at(0.45, None), centred);
        assert_eq!(at(0.55, None), right);
        assert_eq!(at(0.45, Some(GbButton::Right)), right);
        assert_eq!(at(0.38, Some(GbButton::Right)), centred);
        assert_eq!(at(-0.45, Some(GbButton::Right)), centred);
    }

    #[test]
    fn a_pad_going_away_lets_go_of_its_buttons() {
        assert_eq!(route_default(PadInput::Disconnected), PadAction::Unplugged);
    }

    #[test]
    fn events_that_are_not_input_are_named() {
        assert_eq!(
            route_default(PadInput::Connected),
            PadAction::Ignore(Ignored::NotInput)
        );
        assert_eq!(
            route_default(PadInput::Changed(
                PadKey::Known(PadButton::RightTrigger2),
                0.4
            )),
            PadAction::Ignore(Ignored::StateOnly)
        );
    }

    #[test]
    fn a_press_that_changes_nothing_is_marked() {
        let action = PadAction::Press(GbButton::A);
        assert_eq!(action.describe(true), "Game Boy A down");
        assert_eq!(action.describe(false), "Game Boy A down (no change)");
    }
}
