use super::{PadAxis, PadEvent, PadInput, PadKey, PadSource};
use crate::app::ui_state::keybinds::PadButton;
use crate::controller_debug::{
    AxisState, ButtonState, PadInfo, RawState, hex_bytes, usb_id, yes_no,
};
use gilrs::{Axis, Button, EventType, GamepadId, Gilrs, MappingSource, PowerInfo};

const BUTTONS: [Button; 19] = [
    Button::South,
    Button::East,
    Button::North,
    Button::West,
    Button::C,
    Button::Z,
    Button::LeftTrigger,
    Button::LeftTrigger2,
    Button::RightTrigger,
    Button::RightTrigger2,
    Button::Select,
    Button::Start,
    Button::Mode,
    Button::LeftThumb,
    Button::RightThumb,
    Button::DPadUp,
    Button::DPadDown,
    Button::DPadLeft,
    Button::DPadRight,
];

const AXES: [Axis; 8] = [
    Axis::LeftStickX,
    Axis::LeftStickY,
    Axis::LeftZ,
    Axis::RightStickX,
    Axis::RightStickY,
    Axis::RightZ,
    Axis::DPadX,
    Axis::DPadY,
];

pub struct GilrsPads {
    gil: Gilrs,
    ids: Vec<GamepadId>,
}

impl GilrsPads {
    pub fn new() -> Self {
        Self {
            gil: Gilrs::new().unwrap(),
            ids: Vec::new(),
        }
    }
}

impl PadSource for GilrsPads {
    fn next_event(&mut self) -> Option<PadEvent> {
        let gilrs::Event { id, event, .. } = self.gil.next_event()?;
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
        Some(PadEvent {
            pad: usize::from(id),
            input: input(&event),
            code: code(&event),
        })
    }

    fn pads(&self, seen: &[usize]) -> Vec<PadInfo> {
        let mut pads: Vec<PadInfo> = self.gil.gamepads().map(|(_, pad)| info(&pad)).collect();
        for &id in &self.ids {
            if self.gil.connected_gamepad(id).is_none() && seen.contains(&usize::from(id)) {
                pads.push(info(&self.gil.gamepad(id)));
            }
        }
        pads.sort_by_key(|pad| pad.id);
        pads
    }

    fn api(&self) -> String {
        api_name().to_string()
    }
}

fn api_name() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "browser Gamepad API"
    } else if cfg!(target_os = "linux") {
        "evdev with udev"
    } else if cfg!(target_os = "macos") {
        "IOKit HID"
    } else {
        "unknown"
    }
}

pub fn key(button: Button) -> PadKey {
    PadKey::Known(match button {
        Button::South => PadButton::South,
        Button::East => PadButton::East,
        Button::North => PadButton::North,
        Button::West => PadButton::West,
        Button::LeftTrigger => PadButton::LeftTrigger,
        Button::LeftTrigger2 => PadButton::LeftTrigger2,
        Button::RightTrigger => PadButton::RightTrigger,
        Button::RightTrigger2 => PadButton::RightTrigger2,
        Button::Select => PadButton::Select,
        Button::Start => PadButton::Start,
        Button::Mode => PadButton::Mode,
        Button::LeftThumb => PadButton::LeftThumb,
        Button::RightThumb => PadButton::RightThumb,
        Button::DPadUp => PadButton::DPadUp,
        Button::DPadDown => PadButton::DPadDown,
        Button::DPadLeft => PadButton::DPadLeft,
        Button::DPadRight => PadButton::DPadRight,
        Button::C => return PadKey::Other("C"),
        Button::Z => return PadKey::Other("Z"),
        _ => return PadKey::Other("Unknown"),
    })
}

fn axis(axis: Axis) -> PadAxis {
    match axis {
        Axis::LeftStickX => PadAxis::LeftStickX,
        Axis::LeftStickY => PadAxis::LeftStickY,
        Axis::LeftZ => PadAxis::LeftZ,
        Axis::RightStickX => PadAxis::RightStickX,
        Axis::RightStickY => PadAxis::RightStickY,
        Axis::RightZ => PadAxis::RightZ,
        Axis::DPadX => PadAxis::DPadX,
        Axis::DPadY => PadAxis::DPadY,
        _ => PadAxis::Other,
    }
}

fn input(event: &EventType) -> PadInput {
    match *event {
        EventType::ButtonPressed(button, _) => PadInput::Pressed(key(button)),
        EventType::ButtonRepeated(button, _) => PadInput::Repeated(key(button)),
        EventType::ButtonReleased(button, _) => PadInput::Released(key(button)),
        EventType::ButtonChanged(button, value, _) => PadInput::Changed(key(button), value),
        EventType::AxisChanged(a, value, _) => PadInput::Moved(axis(a), value),
        EventType::Connected => PadInput::Connected,
        EventType::Disconnected => PadInput::Disconnected,
        EventType::Dropped => PadInput::Dropped,
        EventType::ForceFeedbackEffectCompleted => PadInput::EffectDone,
        _ => PadInput::Other,
    }
}

fn code(event: &EventType) -> Option<String> {
    match event {
        EventType::ButtonPressed(_, code)
        | EventType::ButtonRepeated(_, code)
        | EventType::ButtonReleased(_, code)
        | EventType::ButtonChanged(_, _, code)
        | EventType::AxisChanged(_, _, code) => Some(code.to_string()),
        _ => None,
    }
}

fn info(pad: &gilrs::Gamepad) -> PadInfo {
    let buttons = BUTTONS
        .iter()
        .filter_map(|&button| {
            let code = pad.button_code(button)?;
            let data = pad.button_data(button);
            Some(ButtonState {
                name: format!("{button:?}"),
                code: code.to_string(),
                pressed: data.is_some_and(|d| d.is_pressed()),
                value: data.map_or(0.0, |d| d.value()),
            })
        })
        .collect();
    let axes = AXES
        .iter()
        .filter_map(|&axis| {
            let code = pad.axis_code(axis)?;
            Some(AxisState {
                axis: self::axis(axis),
                name: format!("{axis:?}"),
                code: code.to_string(),
                value: pad.value(axis),
            })
        })
        .collect();
    let state = pad.state();
    let mut raw: Vec<RawState> = state
        .buttons()
        .map(|(code, data)| (code, data.value()))
        .chain(state.axes().map(|(code, data)| (code, data.value())))
        .filter(|(code, _)| pad.axis_or_btn_name(*code).is_none())
        .map(|(code, value)| RawState {
            code: code.to_string(),
            value,
        })
        .collect();
    raw.sort_by(|a, b| a.code.cmp(&b.code));

    let mut details = vec![("OS name", pad.os_name().to_string())];
    if let Some(map_name) = pad.map_name() {
        details.push(("Mapping name", map_name.to_string()));
    }
    details.extend([
        ("Mapping", mapping_name(pad.mapping_source()).to_string()),
        ("USB ID", usb_id(pad.vendor_id(), pad.product_id())),
        ("UUID", uuid_text(&pad.uuid())),
        ("Power", power_name(pad.power_info())),
        ("Connected", yes_no(pad.is_connected()).to_string()),
        ("Rumble", yes_no(pad.is_ff_supported()).to_string()),
    ]);

    PadInfo {
        id: usize::from(pad.id()),
        name: pad.name().to_string(),
        details,
        buttons,
        axes,
        raw,
    }
}

fn mapping_name(source: MappingSource) -> &'static str {
    match source {
        MappingSource::SdlMappings => "SDL mapping",
        MappingSource::Driver => "driver layout",
        MappingSource::None => "none",
    }
}

fn power_name(power: PowerInfo) -> String {
    match power {
        PowerInfo::Unknown => "unknown".to_string(),
        PowerInfo::Wired => "wired".to_string(),
        PowerInfo::Discharging(level) => format!("battery at {level}%"),
        PowerInfo::Charging(level) => format!("charging, {level}%"),
        PowerInfo::Charged => "charged".to_string(),
    }
}

fn uuid_text(uuid: &[u8; 16]) -> String {
    let hex = hex_bytes(uuid);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gilrs_buttons_keep_their_binding_names() {
        assert_eq!(key(Button::South), PadKey::Known(PadButton::South));
        assert_eq!(
            key(Button::RightTrigger2),
            PadKey::Known(PadButton::RightTrigger2)
        );
        assert_eq!(key(Button::C), PadKey::Other("C"));
        assert_eq!(key(Button::Unknown), PadKey::Other("Unknown"));
    }

    #[test]
    fn a_uuid_reads_in_groups() {
        assert_eq!(
            uuid_text(&[0x12; 16]),
            "12121212-1212-1212-1212-121212121212"
        );
    }

    #[test]
    fn gilrs_events_become_pad_input() {
        assert_eq!(input(&EventType::Connected), PadInput::Connected);
        assert_eq!(
            input(&EventType::ForceFeedbackEffectCompleted),
            PadInput::EffectDone
        );
    }
}
