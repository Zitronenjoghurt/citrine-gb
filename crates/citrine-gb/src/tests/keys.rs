use crate::console::Keys;
use crate::gb::ic::{ICInterface, Interrupt};
use crate::gb::joypad::JoypadState;
use crate::gb::{GameBoy, GbModel};
use crate::{ReadMemory, WriteMemory};

#[derive(Default)]
struct Requests(usize);

impl ICInterface for Requests {
    fn request_interrupt(&mut self, interrupt: Interrupt) {
        assert!(matches!(interrupt, Interrupt::Joypad));
        self.0 += 1;
    }
}

#[test]
fn low_byte_is_the_joypad_state() {
    let pairs = [
        (Keys::A, JoypadState::A),
        (Keys::B, JoypadState::B),
        (Keys::SELECT, JoypadState::SELECT),
        (Keys::START, JoypadState::START),
        (Keys::RIGHT, JoypadState::RIGHT),
        (Keys::LEFT, JoypadState::LEFT),
        (Keys::UP, JoypadState::UP),
        (Keys::DOWN, JoypadState::DOWN),
    ];
    for (keys, state) in pairs {
        assert_eq!(keys.bits(), u16::from(state.bits()));
        assert_eq!(Keys::from(state), keys);
        assert_eq!(JoypadState::from(keys), state);
    }
    assert_eq!(Keys::R.bits(), 0x100);
    assert_eq!(Keys::L.bits(), 0x200);
    assert_eq!(JoypadState::from(Keys::L | Keys::R), JoypadState::empty());
    assert_eq!(JoypadState::from(Keys::all()), JoypadState::all());
}

fn reads(gb: &mut GameBoy) -> [u8; 4] {
    [0x00, 0x10, 0x20, 0x30].map(|select| {
        gb.joypad.write_naive(0xFF00, select);
        gb.joypad.read_naive(0xFF00)
    })
}

#[test]
fn set_keys_acts_like_press_and_release() {
    let steps = [
        Keys::A,
        Keys::A | Keys::B,
        Keys::A | Keys::B,
        Keys::B,
        Keys::empty(),
        Keys::L,
        Keys::L | Keys::DOWN,
        Keys::R | Keys::UP | Keys::START,
        Keys::all(),
        Keys::SELECT,
        Keys::empty(),
    ];
    let mut by_button = GameBoy::new_empty(GbModel::Dmg);
    let mut by_keys = GameBoy::new_empty(GbModel::Dmg);
    let mut previous = Keys::empty();
    for keys in steps {
        let state = JoypadState::from(keys);
        by_button.release_button(JoypadState::from(previous) - state);
        by_button.press_button(state);
        by_keys.set_keys(keys);
        previous = keys;

        let mut button_requests = Requests::default();
        let mut keys_requests = Requests::default();
        by_button.joypad.cycle(&mut button_requests);
        by_keys.joypad.cycle(&mut keys_requests);
        assert_eq!(button_requests.0, keys_requests.0, "{keys:?}");
        assert_eq!(reads(&mut by_button), reads(&mut by_keys), "{keys:?}");
        assert_eq!(by_keys.keys(), Keys::from(state));
    }
}
