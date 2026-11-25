#![no_main]

use citrine_gb_surface::codec::Decoder;
use citrine_gb_surface::mode::{Mode, ScreenShape, View};
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::proto::{Hello, HostMsg, SurfaceMsg, Wire};
use citrine_gb_surface::relay::{Control, DoorKey};
use citrine_gb_surface::touch::clamped;
use libfuzzer_sys::fuzz_target;

const SCREENS: [ScreenShape; 2] = [
    ScreenShape {
        width: 256,
        height: 192,
        touch: false,
    },
    ScreenShape {
        width: 256,
        height: 192,
        touch: true,
    },
];

fn mode(mode: &Mode) {
    let _ = mode.views(&SCREENS);
    let _ = mode.shows_screen(&SCREENS, 1);
    let _ = mode.toggled(&SCREENS, View::Controller);
}

fuzz_target!(|data: &[u8]| {
    match HostMsg::decode(data) {
        Some(HostMsg::Frame(frame)) => {
            let mut decoder = Decoder::default();
            let _ = decoder.decode(&frame);
        }
        Some(HostMsg::Mode(host_mode)) => mode(&host_mode),
        _ => {}
    }
    match SurfaceMsg::decode(data) {
        Some(SurfaceMsg::Hello(hello)) => {
            let _ = hello.prologue();
        }
        Some(SurfaceMsg::SetMode(surface_mode)) => mode(&surface_mode),
        Some(SurfaceMsg::Touch(Some(touch))) => {
            let _ = clamped(touch, &SCREENS);
        }
        _ => {}
    }
    let _ = Hello::front(data);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = Control::parse(text);
        let _ = DoorKey::parse(text);
        let _ = Code::parse(text);
    }
});
