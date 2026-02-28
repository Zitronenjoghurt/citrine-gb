use crate::gb::ic::{ICInterface, Interrupt};
use crate::{ReadMemory, WriteMemory};
use bitflags::bitflags;

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

    pub fn cycle(&mut self, ic: &mut impl ICInterface) {
        if self.new_input {
            ic.request_interrupt(Interrupt::Joypad);
            self.new_input = false;
        }
    }

    pub fn press(&mut self, button: JoypadState) {
        if !self.held.contains(button) {
            self.held.insert(button);
            self.new_input = true;
        }
    }

    pub fn release(&mut self, button: JoypadState) {
        self.held.remove(button);
    }
}

impl WriteMemory for Joypad {
    fn write_naive(&mut self, addr: u16, value: u8) {
        if addr == 0xFF00 {
            self.register = value & 0x30;
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
    #[derive(Default, Debug, Clone, Copy)]
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
