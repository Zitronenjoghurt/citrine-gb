use crate::gb::ic::{ICInterface, Interrupt};
use crate::{ReadMemory, WriteMemory};
use bitflags::bitflags;

#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Joypad {
    register: u8,
    held: JoypadState,
    new_input: bool,
}

impl Default for Joypad {
    fn default() -> Self {
        Self {
            register: 0xCF,
            held: JoypadState::empty(),
            new_input: false,
        }
    }
}

impl Joypad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_new_input(&self) -> bool {
        self.new_input
    }

    pub fn cycle(&mut self, ic: &mut impl ICInterface) {
        if self.new_input {
            ic.request_interrupt(Interrupt::Joypad);
            self.new_input = false;
        }
    }

    pub fn press(&mut self, button: JoypadState) {
        self.set(self.held | button);
    }

    pub fn release(&mut self, button: JoypadState) {
        self.set(self.held - button);
    }

    pub fn set(&mut self, held: JoypadState) {
        let before = self.lines();
        self.held = held;
        self.note_falling_lines(before);
    }

    fn lines(&self) -> u8 {
        self.read_naive(0xFF00) & 0x0F
    }

    fn note_falling_lines(&mut self, before: u8) {
        if before & !self.lines() != 0 {
            self.new_input = true;
        }
    }

    pub fn held(&self) -> JoypadState {
        self.held
    }
}

impl WriteMemory for Joypad {
    fn write_naive(&mut self, addr: u16, value: u8) {
        if addr == 0xFF00 {
            let before = self.lines();
            self.register = value & 0x30;
            self.note_falling_lines(before);
        }
    }
}

impl ReadMemory for Joypad {
    fn read_naive(&self, addr: u16) -> u8 {
        if addr != 0xFF00 {
            return 0xFF;
        }

        let register = self.register;
        let mut lower = 0x0F;

        if register & 0x20 == 0 {
            if self.held.contains(JoypadState::A) {
                lower &= !0x01;
            }
            if self.held.contains(JoypadState::B) {
                lower &= !0x02;
            }
            if self.held.contains(JoypadState::SELECT) {
                lower &= !0x04;
            }
            if self.held.contains(JoypadState::START) {
                lower &= !0x08;
            }
        }

        if register & 0x10 == 0 {
            if self.held.contains(JoypadState::RIGHT) {
                lower &= !0x01;
            }
            if self.held.contains(JoypadState::LEFT) {
                lower &= !0x02;
            }
            if self.held.contains(JoypadState::UP) {
                lower &= !0x04;
            }
            if self.held.contains(JoypadState::DOWN) {
                lower &= !0x08;
            }
        }

        0xC0 | (register & 0x30) | lower
    }
}

bitflags! {
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct JoypadState: u8 {
        const A = 0b0000_0001;
        const B = 0b0000_0010;
        const SELECT = 0b0000_0100;
        const START = 0b0000_1000;
        const RIGHT = 0b0001_0000;
        const LEFT = 0b0010_0000;
        const UP = 0b0100_0000;
        const DOWN = 0b1000_0000;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReadMemory;
    use crate::WriteMemory;
    use crate::console::Keys;
    use crate::gb::ic::InterruptController;
    use crate::gb::ic::{ICInterface, Interrupt};
    use crate::gb::joypad::JoypadState;
    use crate::gb::{GameBoy, GbModel};

    fn raised(joypad: &mut Joypad) -> bool {
        let mut ic = InterruptController::new();
        joypad.cycle(&mut ic);
        ic.flag.is_enabled(crate::gb::ic::Interrupt::Joypad)
    }

    #[test]
    fn pressing_a_selected_key_raises_the_interrupt() {
        let mut joypad = Joypad::new();
        joypad.write_naive(0xFF00, 0x20);
        joypad.press(JoypadState::RIGHT);
        assert!(raised(&mut joypad));
        assert!(!raised(&mut joypad));
    }

    #[test]
    fn pressing_an_unselected_key_raises_nothing() {
        let mut joypad = Joypad::new();
        joypad.write_naive(0xFF00, 0x20);
        joypad.press(JoypadState::A);
        assert!(!raised(&mut joypad));
    }

    #[test]
    fn selecting_a_line_with_a_held_key_raises_the_interrupt() {
        let mut joypad = Joypad::new();
        joypad.write_naive(0xFF00, 0x30);
        joypad.press(JoypadState::START);
        assert!(!raised(&mut joypad));
        joypad.write_naive(0xFF00, 0x10);
        assert!(raised(&mut joypad));
    }

    #[test]
    fn releasing_a_key_raises_nothing() {
        let mut joypad = Joypad::new();
        joypad.write_naive(0xFF00, 0x10);
        joypad.press(JoypadState::B);
        raised(&mut joypad);
        joypad.release(JoypadState::B);
        assert!(!raised(&mut joypad));
    }

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
}
