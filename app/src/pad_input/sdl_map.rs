use super::{PadAxis, PadEvent, PadInput, PadKey};
use crate::app::ui_state::keybinds::PadButton;
use crate::controller_debug::hex_bytes;

const TRIGGER_PRESS: f32 = 0.75;
const TRIGGER_RELEASE: f32 = 0.65;

pub const BUTTON_COUNT: u8 = 26;
pub const AXIS_COUNT: u8 = 6;

const LEFT_TRIGGER: u8 = 4;
const RIGHT_TRIGGER: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdlInput {
    Added,
    Removed,
    Remapped,
    ButtonDown(u8),
    ButtonUp(u8),
    Axis(u8, i16),
}

pub fn key(button: u8) -> PadKey {
    PadKey::Known(match button {
        0 => PadButton::South,
        1 => PadButton::East,
        2 => PadButton::West,
        3 => PadButton::North,
        4 => PadButton::Select,
        5 => PadButton::Mode,
        6 => PadButton::Start,
        7 => PadButton::LeftThumb,
        8 => PadButton::RightThumb,
        9 => PadButton::LeftTrigger,
        10 => PadButton::RightTrigger,
        11 => PadButton::DPadUp,
        12 => PadButton::DPadDown,
        13 => PadButton::DPadLeft,
        14 => PadButton::DPadRight,
        15 => return PadKey::Other("Misc1"),
        16 => return PadKey::Other("RightPaddle1"),
        17 => return PadKey::Other("LeftPaddle1"),
        18 => return PadKey::Other("RightPaddle2"),
        19 => return PadKey::Other("LeftPaddle2"),
        20 => return PadKey::Other("Touchpad"),
        21 => return PadKey::Other("Misc2"),
        22 => return PadKey::Other("Misc3"),
        23 => return PadKey::Other("Misc4"),
        24 => return PadKey::Other("Misc5"),
        25 => return PadKey::Other("Misc6"),
        _ => return PadKey::Other("Unknown"),
    })
}

pub fn axis(axis: u8) -> PadAxis {
    match axis {
        0 => PadAxis::LeftStickX,
        1 => PadAxis::LeftStickY,
        2 => PadAxis::RightStickX,
        3 => PadAxis::RightStickY,
        LEFT_TRIGGER => PadAxis::LeftZ,
        RIGHT_TRIGGER => PadAxis::RightZ,
        _ => PadAxis::Other,
    }
}

pub fn axis_value(axis: u8, raw: i16) -> f32 {
    let value = if raw < 0 {
        f32::from(raw) / 32768.0
    } else {
        f32::from(raw) / 32767.0
    };
    match axis {
        1 | 3 => -value,
        _ => value,
    }
}

fn trigger_button(axis: u8) -> Option<PadButton> {
    match axis {
        LEFT_TRIGGER => Some(PadButton::LeftTrigger2),
        RIGHT_TRIGGER => Some(PadButton::RightTrigger2),
        _ => None,
    }
}

#[derive(Default)]
pub struct Translator {
    triggers_down: Vec<(u32, u8)>,
}

impl Translator {
    pub fn translate(&mut self, pad: u32, input: SdlInput) -> Vec<PadEvent> {
        let event = |input, code| PadEvent {
            pad: pad as usize,
            input,
            code,
        };
        match input {
            SdlInput::Added => vec![event(PadInput::Connected, None)],
            SdlInput::Removed => {
                self.triggers_down.retain(|&(id, _)| id != pad);
                vec![event(PadInput::Disconnected, None)]
            }
            SdlInput::Remapped => vec![event(PadInput::Other, None)],
            SdlInput::ButtonDown(button) => {
                vec![event(
                    PadInput::Pressed(key(button)),
                    Some(button_code(button)),
                )]
            }
            SdlInput::ButtonUp(button) => {
                vec![event(
                    PadInput::Released(key(button)),
                    Some(button_code(button)),
                )]
            }
            SdlInput::Axis(number, raw) => {
                let value = axis_value(number, raw);
                let code = axis_code(number);
                let mut events = vec![event(
                    PadInput::Moved(axis(number), value),
                    Some(code.clone()),
                )];
                if let Some(button) = trigger_button(number) {
                    let down = self.triggers_down.contains(&(pad, number));
                    if !down && value >= TRIGGER_PRESS {
                        self.triggers_down.push((pad, number));
                        events.push(event(PadInput::Pressed(PadKey::Known(button)), Some(code)));
                    } else if down && value <= TRIGGER_RELEASE {
                        self.triggers_down.retain(|&held| held != (pad, number));
                        events.push(event(PadInput::Released(PadKey::Known(button)), Some(code)));
                    }
                }
                events
            }
        }
    }
}

pub fn button_code(button: u8) -> String {
    format!("button {button}")
}

pub fn axis_code(axis: u8) -> String {
    format!("axis {axis}")
}

pub fn type_name(kind: i32) -> &'static str {
    match kind {
        1 => "standard",
        2 => "Xbox 360",
        3 => "Xbox One",
        4 => "PS3",
        5 => "PS4",
        6 => "PS5",
        7 => "Switch Pro",
        8 => "Joy-Con (L)",
        9 => "Joy-Con (R)",
        10 => "Joy-Con pair",
        11 => "GameCube",
        12 => "Steam",
        _ => "unknown",
    }
}

pub fn connection_name(state: i32) -> &'static str {
    match state {
        1 => "wired",
        2 => "wireless",
        _ => "unknown",
    }
}

pub fn battery_text(state: i32, percent: i32) -> String {
    let level = if percent >= 0 {
        format!(", {percent}%")
    } else {
        String::new()
    };
    match state {
        1 => format!("on battery{level}"),
        2 => "no battery".to_string(),
        3 => format!("charging{level}"),
        4 => "charged".to_string(),
        -1 => "error".to_string(),
        _ => "unknown".to_string(),
    }
}

pub fn driver_name(guid: &[u8; 16]) -> &'static str {
    match guid[14] {
        b'h' => "HIDAPI",
        b'x' => "XInput",
        b'w' => "Windows.Gaming.Input",
        b'g' => "GameInput",
        b'r' => "RawInput",
        b'v' => "virtual",
        _ => "DirectInput",
    }
}

pub fn guid_text(guid: &[u8; 16]) -> String {
    hex_bytes(guid)
}

pub fn version_text(version: i32) -> String {
    format!(
        "SDL {}.{}.{}",
        version / 1_000_000,
        version / 1000 % 1000,
        version % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::{BindingCapture, GbButton, KeyBindings};
    use crate::pad_input::{PadAction, route};

    #[test]
    fn sdl_buttons_take_the_binding_names() {
        let expected = [
            PadButton::South,
            PadButton::East,
            PadButton::West,
            PadButton::North,
            PadButton::Select,
            PadButton::Mode,
            PadButton::Start,
            PadButton::LeftThumb,
            PadButton::RightThumb,
            PadButton::LeftTrigger,
            PadButton::RightTrigger,
            PadButton::DPadUp,
            PadButton::DPadDown,
            PadButton::DPadLeft,
            PadButton::DPadRight,
        ];
        for (number, button) in expected.into_iter().enumerate() {
            assert_eq!(key(number as u8), PadKey::Known(button), "button {number}");
        }
        assert_eq!(key(20), PadKey::Other("Touchpad"));
        assert_eq!(key(BUTTON_COUNT), PadKey::Other("Unknown"));
    }

    #[test]
    fn every_pad_button_has_an_sdl_source() {
        use strum::IntoEnumIterator;
        let reachable: Vec<PadKey> = (0..BUTTON_COUNT)
            .map(key)
            .chain([
                PadKey::Known(PadButton::LeftTrigger2),
                PadKey::Known(PadButton::RightTrigger2),
            ])
            .collect();
        for button in PadButton::iter() {
            assert!(
                reachable.contains(&PadKey::Known(button)),
                "{button:?} can't be pressed through SDL"
            );
        }
    }

    #[test]
    fn sdl_sticks_point_up_like_gilrs() {
        assert_eq!(axis(1), PadAxis::LeftStickY);
        assert_eq!(axis_value(1, -32768), 1.0);
        assert_eq!(axis_value(1, 32767), -1.0);
        assert_eq!(axis_value(0, -32768), -1.0);
        assert_eq!(axis_value(0, 32767), 1.0);
        assert_eq!(axis_value(0, 0), 0.0);
        assert_eq!(axis(LEFT_TRIGGER), PadAxis::LeftZ);
        assert_eq!(axis_value(RIGHT_TRIGGER, 32767), 1.0);
        assert_eq!(axis(AXIS_COUNT), PadAxis::Other);
    }

    #[test]
    fn a_trigger_pressed_past_the_threshold_is_a_button() {
        let mut translator = Translator::default();
        let inputs = |events: Vec<PadEvent>| -> Vec<PadInput> {
            events.into_iter().map(|e| e.input).collect()
        };
        let r2 = PadKey::Known(PadButton::RightTrigger2);

        let half = inputs(translator.translate(1, SdlInput::Axis(RIGHT_TRIGGER, 16000)));
        assert_eq!(half.len(), 1, "half way is only a move");

        let full = inputs(translator.translate(1, SdlInput::Axis(RIGHT_TRIGGER, 32767)));
        assert_eq!(full[1], PadInput::Pressed(r2));
        let again = inputs(translator.translate(1, SdlInput::Axis(RIGHT_TRIGGER, 30000)));
        assert_eq!(again.len(), 1, "held, no second press");

        let other_pad = inputs(translator.translate(2, SdlInput::Axis(RIGHT_TRIGGER, 0)));
        assert_eq!(other_pad.len(), 1, "another pad's trigger is its own");

        let back = inputs(translator.translate(1, SdlInput::Axis(RIGHT_TRIGGER, 0)));
        assert_eq!(back[1], PadInput::Released(r2));
    }

    #[test]
    fn unplugging_forgets_a_held_trigger() {
        let mut translator = Translator::default();
        translator.translate(3, SdlInput::Axis(LEFT_TRIGGER, 32767));
        let gone = translator.translate(3, SdlInput::Removed);
        assert_eq!(gone[0].input, PadInput::Disconnected);
        let fresh = translator.translate(3, SdlInput::Axis(LEFT_TRIGGER, 32767));
        assert_eq!(
            fresh[1].input,
            PadInput::Pressed(PadKey::Known(PadButton::LeftTrigger2))
        );
    }

    #[test]
    fn sdl_input_drives_the_same_bindings() {
        let mut translator = Translator::default();
        let bindings = KeyBindings::default();
        let capture = BindingCapture::default();
        let actions = |events: Vec<PadEvent>| -> Vec<PadAction> {
            events
                .into_iter()
                .map(|e| route(e.input, &bindings, &capture, None))
                .collect()
        };
        assert_eq!(
            actions(translator.translate(1, SdlInput::ButtonDown(1))),
            vec![PadAction::Press(GbButton::A)]
        );
        assert_eq!(
            actions(translator.translate(1, SdlInput::ButtonUp(11))),
            vec![PadAction::Release(GbButton::Up)]
        );
        assert_eq!(
            actions(translator.translate(1, SdlInput::Axis(1, -32768))),
            vec![PadAction::Steer {
                on: Some(GbButton::Up),
                off: &[GbButton::Down]
            }]
        );
    }

    #[test]
    fn sdl_events_carry_their_codes() {
        let mut translator = Translator::default();
        let down = translator.translate(7, SdlInput::ButtonDown(0));
        assert_eq!(down[0].pad, 7);
        assert_eq!(down[0].code.as_deref(), Some("button 0"));
        assert_eq!(
            translator.translate(7, SdlInput::Added)[0].input,
            PadInput::Connected
        );
        assert_eq!(
            translator.translate(7, SdlInput::Remapped)[0].input,
            PadInput::Other
        );
    }

    #[test]
    fn pad_details_read_plainly() {
        assert_eq!(type_name(6), "PS5");
        assert_eq!(type_name(0), "unknown");
        assert_eq!(connection_name(2), "wireless");
        assert_eq!(connection_name(-1), "unknown");
        assert_eq!(battery_text(1, 80), "on battery, 80%");
        assert_eq!(battery_text(3, -1), "charging");
        assert_eq!(battery_text(2, -1), "no battery");
        assert_eq!(version_text(3_004_018), "SDL 3.4.18");

        let mut guid = [0u8; 16];
        guid[0] = 0x05;
        guid[14] = b'h';
        assert_eq!(driver_name(&guid), "HIDAPI");
        assert_eq!(guid_text(&guid), "05000000000000000000000000006800");
        guid[14] = 0;
        assert_eq!(driver_name(&guid), "DirectInput");
    }
}
