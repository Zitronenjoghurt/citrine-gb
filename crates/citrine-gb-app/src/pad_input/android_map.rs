use super::{PadAxis, PadEvent, PadInput, PadKey};
use crate::app::ui_state::keybinds::PadButton;
use crate::controller_debug::{AxisState, ButtonState, PadInfo, usb_id, yes_no};
use std::collections::{BTreeMap, VecDeque};

const TRIGGER_PRESS: f32 = 0.75;
const TRIGGER_RELEASE: f32 = 0.65;
const HAT_PRESS: f32 = 0.5;
const MOVED: f32 = 0.002;
const QUEUE_LIMIT: usize = 1024;

pub const AXIS_COUNT: usize = 8;
const AXIS_NAMES: [&str; AXIS_COUNT] = [
    "X", "Y", "Z", "RZ", "LTRIGGER", "RTRIGGER", "HAT_X", "HAT_Y",
];
const LEFT_TRIGGER: usize = 4;
const RIGHT_TRIGGER: usize = 5;
const HAT_X: usize = 6;
const HAT_Y: usize = 7;

const KEYCODE_DPAD_UP: i32 = 19;
const KEYCODE_DPAD_DOWN: i32 = 20;
const KEYCODE_DPAD_LEFT: i32 = 21;
const KEYCODE_DPAD_RIGHT: i32 = 22;
const KEYCODE_BUTTON_L2: i32 = 104;
const KEYCODE_BUTTON_R2: i32 = 105;

#[derive(Debug, Clone, PartialEq)]
pub enum AndroidInput {
    Added(Device),
    Removed,
    KeyDown(i32, bool),
    KeyUp(i32),
    Axes([f32; AXIS_COUNT]),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub name: String,
    pub vendor: i32,
    pub product: i32,
    pub descriptor: String,
}

pub fn key(code: i32) -> PadKey {
    PadKey::Known(match code {
        96 => PadButton::South,
        97 => PadButton::East,
        99 => PadButton::West,
        100 => PadButton::North,
        102 => PadButton::LeftTrigger,
        103 => PadButton::RightTrigger,
        KEYCODE_BUTTON_L2 => PadButton::LeftTrigger2,
        KEYCODE_BUTTON_R2 => PadButton::RightTrigger2,
        106 => PadButton::LeftThumb,
        107 => PadButton::RightThumb,
        108 => PadButton::Start,
        109 => PadButton::Select,
        110 => PadButton::Mode,
        KEYCODE_DPAD_UP => PadButton::DPadUp,
        KEYCODE_DPAD_DOWN => PadButton::DPadDown,
        KEYCODE_DPAD_LEFT => PadButton::DPadLeft,
        KEYCODE_DPAD_RIGHT => PadButton::DPadRight,
        98 => return PadKey::Other("C"),
        101 => return PadKey::Other("Z"),
        23 => return PadKey::Other("DPadCenter"),
        188..=203 => return PadKey::Other("Numbered button"),
        _ => return PadKey::Other("Unknown"),
    })
}

pub fn axis(index: usize) -> PadAxis {
    match index {
        0 => PadAxis::LeftStickX,
        1 => PadAxis::LeftStickY,
        2 => PadAxis::RightStickX,
        3 => PadAxis::RightStickY,
        LEFT_TRIGGER => PadAxis::LeftZ,
        RIGHT_TRIGGER => PadAxis::RightZ,
        HAT_X => PadAxis::DPadX,
        HAT_Y => PadAxis::DPadY,
        _ => PadAxis::Other,
    }
}

fn axis_value(index: usize, raw: f32) -> f32 {
    match index {
        1 | 3 | HAT_Y => -raw,
        _ => raw,
    }
}

fn key_code(code: i32) -> String {
    format!("keycode {code}")
}

fn axis_code(index: usize) -> String {
    format!("AXIS_{}", AXIS_NAMES[index])
}

fn trigger_button(index: usize) -> Option<PadButton> {
    match index {
        LEFT_TRIGGER => Some(PadButton::LeftTrigger2),
        RIGHT_TRIGGER => Some(PadButton::RightTrigger2),
        _ => None,
    }
}

fn hat_buttons(index: usize) -> (PadButton, PadButton) {
    match index {
        HAT_X => (PadButton::DPadLeft, PadButton::DPadRight),
        _ => (PadButton::DPadUp, PadButton::DPadDown),
    }
}

#[derive(Default)]
struct Pad {
    device: Device,
    connected: bool,
    axes: [f32; AXIS_COUNT],
    held: Vec<PadKey>,
    trigger_keys: bool,
}

impl Pad {
    fn hold(&mut self, key: PadKey, down: bool) {
        self.held.retain(|held| *held != key);
        if down {
            self.held.push(key);
        }
    }

    fn holds(&self, button: PadButton) -> bool {
        self.held.contains(&PadKey::Known(button))
    }
}

#[derive(Default)]
pub struct Translator {
    pads: BTreeMap<i32, Pad>,
    queue: VecDeque<PadEvent>,
}

impl Translator {
    pub fn push(&mut self, id: i32, input: AndroidInput) {
        for event in self.translate(id, input) {
            if self.queue.len() >= QUEUE_LIMIT {
                self.queue.pop_front();
            }
            self.queue.push_back(event);
        }
    }

    pub fn next_event(&mut self) -> Option<PadEvent> {
        self.queue.pop_front()
    }

    pub fn translate(&mut self, id: i32, input: AndroidInput) -> Vec<PadEvent> {
        let event = |input, code| PadEvent {
            pad: id as u32 as usize,
            input,
            code,
        };
        if let AndroidInput::Removed = input {
            let Some(pad) = self.pads.get_mut(&id) else {
                return Vec::new();
            };
            *pad = Pad {
                device: std::mem::take(&mut pad.device),
                ..Pad::default()
            };
            return vec![event(PadInput::Disconnected, None)];
        }
        let pad = self.pads.entry(id).or_default();
        let joined = !pad.connected;
        pad.connected = true;
        let mut events = Vec::new();
        if joined {
            events.push(event(PadInput::Connected, None));
        }
        match input {
            AndroidInput::Added(device) => pad.device = device,
            AndroidInput::Removed => {}
            AndroidInput::KeyDown(code, repeat) => {
                let key = key(code);
                pad.trigger_keys |= matches!(code, KEYCODE_BUTTON_L2 | KEYCODE_BUTTON_R2);
                pad.hold(key, true);
                let input = if repeat {
                    PadInput::Repeated(key)
                } else {
                    PadInput::Pressed(key)
                };
                events.push(event(input, Some(key_code(code))));
            }
            AndroidInput::KeyUp(code) => {
                let key = key(code);
                pad.hold(key, false);
                events.push(event(PadInput::Released(key), Some(key_code(code))));
            }
            AndroidInput::Axes(raw) => {
                for (index, &value) in raw.iter().enumerate() {
                    let before = pad.axes[index];
                    if (value - before).abs() < MOVED {
                        continue;
                    }
                    pad.axes[index] = value;
                    let code = axis_code(index);
                    if matches!(index, HAT_X | HAT_Y) {
                        let (negative, positive) = hat_buttons(index);
                        for (button, on) in [
                            (negative, value <= -HAT_PRESS),
                            (positive, value >= HAT_PRESS),
                        ] {
                            if on != pad.holds(button) {
                                let key = PadKey::Known(button);
                                pad.hold(key, on);
                                let input = if on {
                                    PadInput::Pressed(key)
                                } else {
                                    PadInput::Released(key)
                                };
                                events.push(event(input, Some(code.clone())));
                            }
                        }
                        continue;
                    }
                    events.push(event(
                        PadInput::Moved(axis(index), axis_value(index, value)),
                        Some(code.clone()),
                    ));
                    if let Some(button) = trigger_button(index)
                        && !pad.trigger_keys
                    {
                        let down = pad.holds(button);
                        let key = PadKey::Known(button);
                        if !down && value >= TRIGGER_PRESS {
                            pad.hold(key, true);
                            events.push(event(PadInput::Pressed(key), Some(code)));
                        } else if down && value <= TRIGGER_RELEASE {
                            pad.hold(key, false);
                            events.push(event(PadInput::Released(key), Some(code)));
                        }
                    }
                }
            }
        }
        events
    }

    pub fn pads(&self, seen: &[usize]) -> Vec<PadInfo> {
        self.pads
            .iter()
            .filter(|(id, pad)| pad.connected || seen.contains(&(**id as u32 as usize)))
            .map(|(id, pad)| info(*id, pad))
            .collect()
    }
}

fn info(id: i32, pad: &Pad) -> PadInfo {
    let buttons = pad
        .held
        .iter()
        .map(|key| ButtonState {
            name: key.to_string(),
            code: String::new(),
            pressed: true,
            value: 1.0,
        })
        .collect();
    let axes = pad
        .axes
        .iter()
        .enumerate()
        .map(|(index, &value)| AxisState {
            axis: axis(index),
            name: AXIS_NAMES[index].to_string(),
            code: axis_code(index),
            value: axis_value(index, value),
        })
        .collect();
    let device = &pad.device;
    let name = if device.name.is_empty() {
        format!("Controller {id}")
    } else {
        device.name.clone()
    };
    let known = |value: i32| u16::try_from(value).ok().filter(|v| *v != 0);
    PadInfo {
        id: id as u32 as usize,
        name,
        details: vec![
            ("Android device", id.to_string()),
            (
                "USB ID",
                usb_id(known(device.vendor), known(device.product)),
            ),
            ("Descriptor", device.descriptor.clone()),
            ("Connected", yes_no(pad.connected).to_string()),
        ],
        buttons,
        axes,
        raw: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::{BindingCapture, GbButton, KeyBindings};
    use crate::pad_input::{PadAction, route};

    fn inputs(events: Vec<PadEvent>) -> Vec<PadInput> {
        events.into_iter().map(|e| e.input).collect()
    }

    fn axes(set: &[(usize, f32)]) -> AndroidInput {
        let mut raw = [0.0; AXIS_COUNT];
        for &(index, value) in set {
            raw[index] = value;
        }
        AndroidInput::Axes(raw)
    }

    #[test]
    fn android_keycodes_take_the_binding_names() {
        assert_eq!(key(96), PadKey::Known(PadButton::South));
        assert_eq!(key(97), PadKey::Known(PadButton::East));
        assert_eq!(key(109), PadKey::Known(PadButton::Select));
        assert_eq!(key(KEYCODE_DPAD_LEFT), PadKey::Known(PadButton::DPadLeft));
        assert_eq!(key(98), PadKey::Other("C"));
        assert_eq!(key(4), PadKey::Other("Unknown"));
    }

    #[test]
    fn the_first_event_of_a_pad_connects_it() {
        let mut translator = Translator::default();
        let first = inputs(translator.translate(7, AndroidInput::KeyDown(96, false)));
        assert_eq!(
            first,
            [
                PadInput::Connected,
                PadInput::Pressed(PadKey::Known(PadButton::South))
            ]
        );
        let second = inputs(translator.translate(7, AndroidInput::KeyUp(96)));
        assert_eq!(
            second,
            [PadInput::Released(PadKey::Known(PadButton::South))]
        );
    }

    #[test]
    fn sticks_point_up_like_gilrs() {
        let mut translator = Translator::default();
        translator.translate(1, AndroidInput::Added(Device::default()));
        let up = inputs(translator.translate(1, axes(&[(1, -1.0)])));
        assert_eq!(up, [PadInput::Moved(PadAxis::LeftStickY, 1.0)]);
        let still = translator.translate(1, axes(&[(1, -1.0)]));
        assert!(still.is_empty(), "an unchanged axis sends nothing");
    }

    #[test]
    fn a_hat_presses_and_releases_the_dpad() {
        let mut translator = Translator::default();
        translator.translate(2, AndroidInput::Added(Device::default()));
        let up = PadKey::Known(PadButton::DPadUp);
        let right = PadKey::Known(PadButton::DPadRight);
        assert_eq!(
            inputs(translator.translate(2, axes(&[(HAT_Y, -1.0)]))),
            [PadInput::Pressed(up)]
        );
        assert_eq!(
            inputs(translator.translate(2, axes(&[(HAT_Y, -1.0), (HAT_X, 1.0)]))),
            [PadInput::Pressed(right)]
        );
        assert_eq!(
            inputs(translator.translate(2, axes(&[]))),
            [PadInput::Released(right), PadInput::Released(up)]
        );
    }

    #[test]
    fn a_trigger_axis_is_a_button_unless_the_pad_sends_trigger_keys() {
        let mut translator = Translator::default();
        translator.translate(3, AndroidInput::Added(Device::default()));
        let r2 = PadKey::Known(PadButton::RightTrigger2);
        let pulled = inputs(translator.translate(3, axes(&[(RIGHT_TRIGGER, 1.0)])));
        assert_eq!(pulled[1], PadInput::Pressed(r2));
        let let_go = inputs(translator.translate(3, axes(&[])));
        assert_eq!(let_go[1], PadInput::Released(r2));

        translator.translate(3, AndroidInput::KeyDown(KEYCODE_BUTTON_R2, false));
        translator.translate(3, AndroidInput::KeyUp(KEYCODE_BUTTON_R2));
        let pulled = inputs(translator.translate(3, axes(&[(RIGHT_TRIGGER, 1.0)])));
        assert_eq!(pulled, [PadInput::Moved(PadAxis::RightZ, 1.0)]);
    }

    #[test]
    fn unplugging_forgets_what_was_held() {
        let mut translator = Translator::default();
        translator.translate(
            4,
            AndroidInput::Added(Device {
                name: "Pad".into(),
                ..Device::default()
            }),
        );
        translator.translate(4, AndroidInput::KeyDown(96, false));
        let gone = inputs(translator.translate(4, AndroidInput::Removed));
        assert_eq!(gone, [PadInput::Disconnected]);
        let pads = translator.pads(&[4]);
        assert_eq!(pads[0].name, "Pad");
        assert!(pads[0].pressed().is_empty());
        assert!(translator.pads(&[]).is_empty());
        assert!(translator.translate(9, AndroidInput::Removed).is_empty());
    }

    #[test]
    fn the_queue_keeps_order_and_drops_the_oldest() {
        let mut translator = Translator::default();
        for _ in 0..QUEUE_LIMIT {
            translator.push(6, AndroidInput::KeyDown(96, true));
        }
        translator.push(6, AndroidInput::KeyUp(96));
        let mut last = None;
        let mut count = 0;
        while let Some(event) = translator.next_event() {
            last = Some(event.input);
            count += 1;
        }
        assert_eq!(count, QUEUE_LIMIT);
        assert_eq!(
            last,
            Some(PadInput::Released(PadKey::Known(PadButton::South)))
        );
    }

    #[test]
    fn android_input_drives_the_same_bindings() {
        let mut translator = Translator::default();
        let bindings = KeyBindings::default();
        let capture = BindingCapture::default();
        let events = translator.translate(5, AndroidInput::KeyDown(97, false));
        let action = route(events[1].input, &bindings, &capture, None);
        assert_eq!(action, PadAction::Press(GbButton::A));
    }
}
