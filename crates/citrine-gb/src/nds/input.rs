use super::Cpu;
use crate::console::Keys;
use crate::gba::keypad::{KEY_MASK, Keypad};

const EXT_X: u16 = 1 << 0;
const EXT_Y: u16 = 1 << 1;
const EXT_PEN_UP: u16 = 1 << 6;
const EXT_HINGE_CLOSED: u16 = 1 << 7;
const EXT_RELEASED: u16 = 0x007F;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Input {
    keypads: [Keypad; 2],
    keys: Keys,
    pen: bool,
}

fn side(cpu: Cpu) -> usize {
    match cpu {
        Cpu::Arm9 => 0,
        Cpu::Arm7 => 1,
    }
}

impl Input {
    pub fn keys(&self) -> Keys {
        self.keys
    }

    pub fn set_keys(&mut self, keys: Keys) -> [bool; 2] {
        self.keys = keys;
        let pad = Keys::from_bits_truncate(keys.bits() & KEY_MASK);
        self.keypads.each_mut().map(|keypad| keypad.set_keys(pad))
    }

    pub fn set_pen(&mut self, down: bool) {
        self.pen = down;
    }

    pub fn keyinput(&self) -> u16 {
        self.keypads[0].keyinput()
    }

    pub fn keycnt(&self, cpu: Cpu) -> u16 {
        self.keypads[side(cpu)].keycnt()
    }

    pub fn write_keycnt(&mut self, cpu: Cpu, value: u16, mask: u16) -> bool {
        self.keypads[side(cpu)].write_keycnt(value, mask)
    }

    pub fn extkeyin(&self) -> u16 {
        let mut value = EXT_RELEASED;
        if self.keys.contains(Keys::X) {
            value &= !EXT_X;
        }
        if self.keys.contains(Keys::Y) {
            value &= !EXT_Y;
        }
        if self.pen {
            value &= !EXT_PEN_UP;
        }
        if self.keys.contains(Keys::LID) {
            value |= EXT_HINGE_CLOSED;
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_held_reads_all_released() {
        let input = Input::default();
        assert_eq!(input.keyinput(), 0x03FF);
        assert_eq!(input.extkeyin(), 0x007F);
    }

    #[test]
    fn x_and_y_read_in_extkeyin_not_keyinput() {
        let mut input = Input::default();
        input.set_keys(Keys::X | Keys::A);
        assert_eq!(input.keyinput(), 0x03FE);
        assert_eq!(input.extkeyin(), 0x007E);
        input.set_keys(Keys::Y);
        assert_eq!(input.keyinput(), 0x03FF);
        assert_eq!(input.extkeyin(), 0x007D);
    }

    #[test]
    fn the_pen_clears_bit_6_and_a_closed_lid_sets_bit_7() {
        let mut input = Input::default();
        input.set_pen(true);
        input.set_keys(Keys::LID);
        assert_eq!(input.extkeyin(), 0x00BF);
    }

    #[test]
    fn each_cpu_has_its_own_keycnt() {
        let mut input = Input::default();
        input.write_keycnt(Cpu::Arm7, 0x4001, 0xFFFF);
        assert_eq!(input.keycnt(Cpu::Arm9), 0);
        assert_eq!(input.keycnt(Cpu::Arm7), 0x4001);
        assert_eq!(input.set_keys(Keys::A), [false, true]);
    }
}
