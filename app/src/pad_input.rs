use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, GbButton, KeyBindings, PadButton,
};
use crate::controller_debug::PadInfo;
use std::fmt::{Display, Formatter};

#[cfg(not(windows))]
mod gilrs_source;
#[cfg(any(windows, test))]
mod sdl_map;
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

pub trait PadSource {
    fn next_event(&mut self) -> Option<PadEvent>;
    fn pads(&self, seen: &[usize]) -> Vec<PadInfo>;
    fn api(&self) -> String;
}

#[cfg(not(windows))]
pub fn open_source() -> Box<dyn PadSource> {
    Box::new(gilrs_source::GilrsPads::new())
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadAction {
    Press(GbButton),
    Release(GbButton),
    Steer {
        on: Option<GbButton>,
        off: &'static [GbButton],
    },
    Capture(PadButton),
    Unplugged,
    Ignore(Ignored),
}

impl PadAction {
    pub fn describe(&self, changed: bool) -> String {
        let text = match self {
            Self::Press(button) => format!("Game Boy {button} down"),
            Self::Release(button) => format!("Game Boy {button} up"),
            Self::Steer { on: Some(on), off } => {
                let off: Vec<String> = off.iter().map(|b| b.to_string()).collect();
                format!("Game Boy {on} down, {} up", off.join(" and "))
            }
            Self::Steer { on: None, off } => {
                let off: Vec<String> = off.iter().map(|b| b.to_string()).collect();
                format!("Game Boy {} up", off.join(" and "))
            }
            Self::Capture(pad) => {
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
        PadInput::Pressed(key) => {
            let PadKey::Known(pad) = key else {
                return PadAction::Ignore(Ignored::UnknownButton);
            };
            if capture.wants(BindingKind::Gamepad) {
                return PadAction::Capture(pad);
            }
            bound(pad, bindings).map_or_else(PadAction::Ignore, PadAction::Press)
        }
        PadInput::Released(key) => {
            let PadKey::Known(pad) = key else {
                return PadAction::Ignore(Ignored::UnknownButton);
            };
            bound(pad, bindings).map_or_else(PadAction::Ignore, PadAction::Release)
        }
        PadInput::Moved(axis, value) => steer(axis, value, bindings, steering),
        PadInput::Repeated(_) | PadInput::Changed(..) => PadAction::Ignore(Ignored::StateOnly),
        PadInput::Disconnected => PadAction::Unplugged,
        _ => PadAction::Ignore(Ignored::NotInput),
    }
}

fn bound(pad: PadButton, bindings: &KeyBindings) -> Result<GbButton, Ignored> {
    bindings.button_for_pad(pad).ok_or(Ignored::Unbound)
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
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::BindingTarget;

    fn route_default(input: PadInput) -> PadAction {
        route(
            input,
            &KeyBindings::default(),
            &BindingCapture::default(),
            None,
        )
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
            route_default(PadInput::Pressed(PadKey::Known(PadButton::LeftTrigger))),
            PadAction::Ignore(Ignored::Unbound)
        );
        assert_eq!(
            route_default(PadInput::Pressed(PadKey::Other("Unknown"))),
            PadAction::Ignore(Ignored::UnknownButton)
        );
    }

    #[test]
    fn a_waiting_binding_takes_presses_but_not_releases() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::A), BindingKind::Any);
        let bindings = KeyBindings::default();
        assert_eq!(
            route(
                PadInput::Pressed(PadKey::Known(PadButton::South)),
                &bindings,
                &capture,
                None
            ),
            PadAction::Capture(PadButton::South)
        );
        assert_eq!(
            route(
                PadInput::Released(PadKey::Known(PadButton::South)),
                &bindings,
                &capture,
                None
            ),
            PadAction::Release(GbButton::B)
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
                None
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
                None
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
                None
            ),
            PadAction::Ignore(Ignored::UnusedAxis)
        );

        bindings.stick_as_dpad = false;
        assert_eq!(
            route(
                PadInput::Moved(PadAxis::LeftStickX, -0.9),
                &bindings,
                &capture,
                None
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
