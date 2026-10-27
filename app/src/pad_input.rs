use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, GbButton, KeyBindings, PadButton,
};
use gilrs::{Axis, Button, EventType};
use std::fmt::{Display, Formatter};

const STICK_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadInput {
    Pressed(Button),
    Repeated(Button),
    Released(Button),
    Changed(Button, f32),
    Moved(Axis, f32),
    Connected,
    Disconnected,
    Dropped,
    EffectDone,
    Other,
}

impl PadInput {
    pub fn from_gilrs(event: &EventType) -> Self {
        match *event {
            EventType::ButtonPressed(button, _) => Self::Pressed(button),
            EventType::ButtonRepeated(button, _) => Self::Repeated(button),
            EventType::ButtonReleased(button, _) => Self::Released(button),
            EventType::ButtonChanged(button, value, _) => Self::Changed(button, value),
            EventType::AxisChanged(axis, value, _) => Self::Moved(axis, value),
            EventType::Connected => Self::Connected,
            EventType::Disconnected => Self::Disconnected,
            EventType::Dropped => Self::Dropped,
            EventType::ForceFeedbackEffectCompleted => Self::EffectDone,
            _ => Self::Other,
        }
    }

    pub fn code(event: &EventType) -> Option<String> {
        match event {
            EventType::ButtonPressed(_, code)
            | EventType::ButtonRepeated(_, code)
            | EventType::ButtonReleased(_, code)
            | EventType::ButtonChanged(_, _, code)
            | EventType::AxisChanged(_, _, code) => Some(code.to_string()),
            _ => None,
        }
    }
}

impl Display for PadInput {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pressed(button) => write!(f, "{button:?} pressed"),
            Self::Repeated(button) => write!(f, "{button:?} repeated"),
            Self::Released(button) => write!(f, "{button:?} released"),
            Self::Changed(button, value) => write!(f, "{button:?} at {value:.2}"),
            Self::Moved(axis, value) => write!(f, "{axis:?} at {value:+.2}"),
            Self::Connected => f.write_str("connected"),
            Self::Disconnected => f.write_str("disconnected"),
            Self::Dropped => f.write_str("dropped"),
            Self::EffectDone => f.write_str("rumble finished"),
            Self::Other => f.write_str("other event"),
        }
    }
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
            Self::Ignore(reason) => return format!("ignored: {reason}"),
        };
        if changed {
            text
        } else {
            format!("{text} (no change)")
        }
    }
}

pub fn route(input: PadInput, bindings: &KeyBindings, capture: &BindingCapture) -> PadAction {
    match input {
        PadInput::Pressed(button) => {
            let Some(pad) = PadButton::from_gilrs(button) else {
                return PadAction::Ignore(Ignored::UnknownButton);
            };
            if capture.wants(BindingKind::Gamepad) {
                return PadAction::Capture(pad);
            }
            bound(pad, bindings).map_or_else(PadAction::Ignore, PadAction::Press)
        }
        PadInput::Released(button) => {
            let Some(pad) = PadButton::from_gilrs(button) else {
                return PadAction::Ignore(Ignored::UnknownButton);
            };
            bound(pad, bindings).map_or_else(PadAction::Ignore, PadAction::Release)
        }
        PadInput::Moved(axis, value) => steer(axis, value, bindings.stick_as_dpad),
        PadInput::Repeated(_) | PadInput::Changed(..) => PadAction::Ignore(Ignored::StateOnly),
        _ => PadAction::Ignore(Ignored::NotInput),
    }
}

fn bound(pad: PadButton, bindings: &KeyBindings) -> Result<GbButton, Ignored> {
    bindings.button_for_pad(pad).ok_or(Ignored::Unbound)
}

fn steer(axis: Axis, value: f32, stick_on: bool) -> PadAction {
    let (negative, positive) = match axis {
        Axis::DPadX | Axis::LeftStickX => (GbButton::Left, GbButton::Right),
        Axis::DPadY | Axis::LeftStickY => (GbButton::Down, GbButton::Up),
        _ => return PadAction::Ignore(Ignored::UnusedAxis),
    };
    if !stick_on {
        return PadAction::Ignore(Ignored::StickOff);
    }
    if value < -STICK_THRESHOLD {
        PadAction::Steer {
            on: Some(negative),
            off: opposite(negative),
        }
    } else if value > STICK_THRESHOLD {
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
        route(input, &KeyBindings::default(), &BindingCapture::default())
    }

    #[test]
    fn a_bound_button_presses_its_game_boy_button() {
        assert_eq!(
            route_default(PadInput::Pressed(Button::East)),
            PadAction::Press(GbButton::A)
        );
        assert_eq!(
            route_default(PadInput::Released(Button::DPadUp)),
            PadAction::Release(GbButton::Up)
        );
    }

    #[test]
    fn an_unbound_button_says_so() {
        assert_eq!(
            route_default(PadInput::Pressed(Button::LeftTrigger)),
            PadAction::Ignore(Ignored::Unbound)
        );
        assert_eq!(
            route_default(PadInput::Pressed(Button::Unknown)),
            PadAction::Ignore(Ignored::UnknownButton)
        );
    }

    #[test]
    fn a_waiting_binding_takes_presses_but_not_releases() {
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::A), BindingKind::Any);
        let bindings = KeyBindings::default();
        assert_eq!(
            route(PadInput::Pressed(Button::South), &bindings, &capture),
            PadAction::Capture(PadButton::South)
        );
        assert_eq!(
            route(PadInput::Released(Button::South), &bindings, &capture),
            PadAction::Release(GbButton::B)
        );
    }

    #[test]
    fn the_stick_steers_only_when_switched_on() {
        let mut bindings = KeyBindings::default();
        let capture = BindingCapture::default();
        assert_eq!(
            route(PadInput::Moved(Axis::LeftStickX, -0.9), &bindings, &capture),
            PadAction::Steer {
                on: Some(GbButton::Left),
                off: &[GbButton::Right]
            }
        );
        assert_eq!(
            route(PadInput::Moved(Axis::LeftStickY, 0.1), &bindings, &capture),
            PadAction::Steer {
                on: None,
                off: &[GbButton::Up, GbButton::Down]
            }
        );
        assert_eq!(
            route(PadInput::Moved(Axis::RightStickX, 1.0), &bindings, &capture),
            PadAction::Ignore(Ignored::UnusedAxis)
        );

        bindings.stick_as_dpad = false;
        assert_eq!(
            route(PadInput::Moved(Axis::LeftStickX, -0.9), &bindings, &capture),
            PadAction::Ignore(Ignored::StickOff)
        );
    }

    #[test]
    fn events_that_are_not_input_are_named() {
        assert_eq!(
            route_default(PadInput::Connected),
            PadAction::Ignore(Ignored::NotInput)
        );
        assert_eq!(
            route_default(PadInput::Changed(Button::RightTrigger2, 0.4)),
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
