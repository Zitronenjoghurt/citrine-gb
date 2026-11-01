use super::sdl_map::{self, AXIS_COUNT, BUTTON_COUNT, SdlInput, Translator};
use super::{PadEvent, PadSource};
use crate::controller_debug::{AxisState, ButtonState, PadInfo, usb_id, yes_no};
use sdl3_sys::error::SDL_GetError;
use sdl3_sys::events::{
    SDL_EVENT_GAMEPAD_ADDED, SDL_EVENT_GAMEPAD_AXIS_MOTION, SDL_EVENT_GAMEPAD_BUTTON_DOWN,
    SDL_EVENT_GAMEPAD_BUTTON_UP, SDL_EVENT_GAMEPAD_REMAPPED, SDL_EVENT_GAMEPAD_REMOVED, SDL_Event,
    SDL_EventType, SDL_PollEvent,
};
use sdl3_sys::gamepad::{
    SDL_CloseGamepad, SDL_Gamepad, SDL_GamepadAxis, SDL_GamepadButton, SDL_GamepadConnected,
    SDL_GamepadHasAxis, SDL_GamepadHasButton, SDL_GetGamepadAxis, SDL_GetGamepadButton,
    SDL_GetGamepadConnectionState, SDL_GetGamepadGUIDForID, SDL_GetGamepadMapping,
    SDL_GetGamepadName, SDL_GetGamepadPowerInfo, SDL_GetGamepadProduct, SDL_GetGamepadProperties,
    SDL_GetGamepadSerial, SDL_GetGamepadType, SDL_GetGamepadVendor, SDL_OpenGamepad,
    SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN,
};
use sdl3_sys::hints::{
    SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, SDL_HINT_JOYSTICK_ENHANCED_REPORTS,
    SDL_HINT_JOYSTICK_HIDAPI, SDL_HINT_JOYSTICK_HIDAPI_PS4, SDL_HINT_JOYSTICK_HIDAPI_PS5,
    SDL_HINT_JOYSTICK_HIDAPI_SWITCH, SDL_HINT_JOYSTICK_THREAD, SDL_SetHint,
};
use sdl3_sys::init::{SDL_INIT_GAMEPAD, SDL_Init, SDL_QuitSubSystem};
use sdl3_sys::joystick::SDL_JoystickID;
use sdl3_sys::properties::SDL_GetBooleanProperty;
use sdl3_sys::stdinc::SDL_free;
use sdl3_sys::version::SDL_GetVersion;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, c_char};

const HINTS: [(*const c_char, &CStr); 7] = [
    (SDL_HINT_JOYSTICK_HIDAPI, c"1"),
    (SDL_HINT_JOYSTICK_HIDAPI_PS4, c"1"),
    (SDL_HINT_JOYSTICK_HIDAPI_PS5, c"1"),
    (SDL_HINT_JOYSTICK_HIDAPI_SWITCH, c"1"),
    (SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS, c"1"),
    (SDL_HINT_JOYSTICK_THREAD, c"1"),
    (SDL_HINT_JOYSTICK_ENHANCED_REPORTS, c"auto"),
];

pub struct SdlPads {
    started: bool,
    api: String,
    open: BTreeMap<u32, *mut SDL_Gamepad>,
    gone: BTreeMap<u32, PadInfo>,
    translator: Translator,
    queue: VecDeque<PadEvent>,
}

impl SdlPads {
    pub fn new() -> Self {
        let version = sdl_map::version_text(SDL_GetVersion());
        let started = unsafe {
            for (name, value) in HINTS {
                SDL_SetHint(name, value.as_ptr());
            }
            SDL_Init(SDL_INIT_GAMEPAD)
        };
        let api = if started {
            version
        } else {
            format!("{version}, failed to start: {}", last_error())
        };
        Self {
            started,
            api,
            open: BTreeMap::new(),
            gone: BTreeMap::new(),
            translator: Translator::default(),
            queue: VecDeque::new(),
        }
    }

    fn open_pad(&mut self, id: u32) {
        if self.open.contains_key(&id) {
            return;
        }
        let pad = unsafe { SDL_OpenGamepad(SDL_JoystickID(id)) };
        if !pad.is_null() {
            self.gone.remove(&id);
            self.open.insert(id, pad);
        }
    }

    fn close_pad(&mut self, id: u32) {
        let Some(pad) = self.open.remove(&id) else {
            return;
        };
        let mut info = unsafe { info(id, pad) };
        for (label, value) in &mut info.details {
            if *label == "Connected" {
                *value = yes_no(false).to_string();
            }
        }
        self.gone.insert(id, info);
        unsafe { SDL_CloseGamepad(pad) };
    }
}

impl Drop for SdlPads {
    fn drop(&mut self) {
        if !self.started {
            return;
        }
        unsafe {
            for (_, pad) in std::mem::take(&mut self.open) {
                SDL_CloseGamepad(pad);
            }
            SDL_QuitSubSystem(SDL_INIT_GAMEPAD);
        }
    }
}

impl PadSource for SdlPads {
    fn next_event(&mut self) -> Option<PadEvent> {
        while self.queue.is_empty() {
            if !self.started {
                return None;
            }
            let mut event = SDL_Event::default();
            if !unsafe { SDL_PollEvent(&mut event) } {
                return None;
            }
            let Some((id, input)) = (unsafe { read(&event) }) else {
                continue;
            };
            match input {
                SdlInput::Added => self.open_pad(id),
                SdlInput::Removed => self.close_pad(id),
                _ => {}
            }
            self.queue.extend(self.translator.translate(id, input));
        }
        self.queue.pop_front()
    }

    fn pads(&self, seen: &[usize]) -> Vec<PadInfo> {
        let mut pads: Vec<PadInfo> = self
            .open
            .iter()
            .map(|(&id, &pad)| unsafe { info(id, pad) })
            .collect();
        pads.extend(
            self.gone
                .iter()
                .filter(|(id, _)| seen.contains(&(**id as usize)))
                .map(|(_, info)| info.clone()),
        );
        pads.sort_by_key(|pad| pad.id);
        pads
    }

    fn api(&self) -> String {
        self.api.clone()
    }
}

unsafe fn read(event: &SDL_Event) -> Option<(u32, SdlInput)> {
    unsafe {
        let kind = SDL_EventType(event.r#type);
        Some(match kind {
            SDL_EVENT_GAMEPAD_ADDED => (event.gdevice.which.0, SdlInput::Added),
            SDL_EVENT_GAMEPAD_REMOVED => (event.gdevice.which.0, SdlInput::Removed),
            SDL_EVENT_GAMEPAD_REMAPPED => (event.gdevice.which.0, SdlInput::Remapped),
            SDL_EVENT_GAMEPAD_BUTTON_DOWN => (
                event.gbutton.which.0,
                SdlInput::ButtonDown(event.gbutton.button),
            ),
            SDL_EVENT_GAMEPAD_BUTTON_UP => (
                event.gbutton.which.0,
                SdlInput::ButtonUp(event.gbutton.button),
            ),
            SDL_EVENT_GAMEPAD_AXIS_MOTION => (
                event.gaxis.which.0,
                SdlInput::Axis(event.gaxis.axis, event.gaxis.value),
            ),
            _ => return None,
        })
    }
}

unsafe fn info(id: u32, pad: *mut SDL_Gamepad) -> PadInfo {
    unsafe {
        let guid = SDL_GetGamepadGUIDForID(SDL_JoystickID(id)).data;
        let known = |id: u16| (id != 0).then_some(id);
        let mut percent = -1;
        let power = SDL_GetGamepadPowerInfo(pad, &mut percent).0;
        let rumble = SDL_GetBooleanProperty(
            SDL_GetGamepadProperties(pad),
            SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN,
            false,
        );
        let mapping = SDL_GetGamepadMapping(pad);
        let mapping_text = text(mapping);
        SDL_free(mapping.cast());

        let mut details = vec![
            (
                "Type",
                sdl_map::type_name(SDL_GetGamepadType(pad).0).to_string(),
            ),
            ("Driver", sdl_map::driver_name(&guid).to_string()),
            (
                "USB ID",
                usb_id(
                    known(SDL_GetGamepadVendor(pad)),
                    known(SDL_GetGamepadProduct(pad)),
                ),
            ),
            ("GUID", sdl_map::guid_text(&guid)),
            (
                "Connection",
                sdl_map::connection_name(SDL_GetGamepadConnectionState(pad).0).to_string(),
            ),
            ("Battery", sdl_map::battery_text(power, percent)),
        ];
        if let Some(serial) = text(SDL_GetGamepadSerial(pad)) {
            details.push(("Serial", serial));
        }
        if let Some(mapping) = mapping_text {
            details.push(("Mapping", mapping));
        }
        details.extend([
            ("Connected", yes_no(SDL_GamepadConnected(pad)).to_string()),
            ("Rumble", yes_no(rumble).to_string()),
        ]);

        let buttons = (0..BUTTON_COUNT)
            .filter(|&b| SDL_GamepadHasButton(pad, SDL_GamepadButton(b.into())))
            .map(|b| {
                let pressed = SDL_GetGamepadButton(pad, SDL_GamepadButton(b.into()));
                ButtonState {
                    name: sdl_map::key(b).to_string(),
                    code: sdl_map::button_code(b),
                    pressed,
                    value: if pressed { 1.0 } else { 0.0 },
                }
            })
            .collect();
        let axes = (0..AXIS_COUNT)
            .filter(|&a| SDL_GamepadHasAxis(pad, SDL_GamepadAxis(a.into())))
            .map(|a| AxisState {
                axis: sdl_map::axis(a),
                name: format!("{:?}", sdl_map::axis(a)),
                code: sdl_map::axis_code(a),
                value: sdl_map::axis_value(a, SDL_GetGamepadAxis(pad, SDL_GamepadAxis(a.into()))),
            })
            .collect();

        PadInfo {
            id: id as usize,
            name: text(SDL_GetGamepadName(pad)).unwrap_or_else(|| "unknown".to_string()),
            details,
            buttons,
            axes,
            raw: Vec::new(),
        }
    }
}

unsafe fn text(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    (!text.is_empty()).then_some(text)
}

fn last_error() -> String {
    unsafe { text(SDL_GetError()) }.unwrap_or_else(|| "unknown error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::KeyBindings;
    use crate::controller_debug::{Report, SystemInfo};

    #[test]
    fn sdl_starts_without_a_window() {
        let mut pads = SdlPads::new();
        let api = pads.api();
        assert!(api.starts_with("SDL 3."), "{api}");
        assert!(!api.contains("failed"), "{api}");
        while pads.next_event().is_some() {}
        let found = pads.pads(&[]);
        let system = SystemInfo::current(api.clone());
        let report = Report {
            system: &system,
            focused: None,
            pads: &found,
            bindings: &KeyBindings::default(),
            capture: None,
            held: &[],
            log: None,
        }
        .to_string();
        println!("{report}");
        assert!(report.contains(&format!("Controller API: {api}")));
    }
}
