use crate::console::Keys;

const KEY_MASK: u16 = 0x03FF;
const IRQ_ENABLE: u16 = 1 << 14;
const IRQ_ALL_KEYS: u16 = 1 << 15;
const KEYCNT_MASK: u16 = KEY_MASK | IRQ_ENABLE | IRQ_ALL_KEYS;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Keypad {
    held: Keys,
    control: u16,
}

impl Keypad {
    pub fn keyinput(self) -> u16 {
        !self.held.bits() & KEY_MASK
    }

    pub fn keycnt(self) -> u16 {
        self.control
    }

    pub fn set_keys(&mut self, keys: Keys) -> bool {
        self.held = keys;
        self.irq_condition()
    }

    pub fn write_keycnt(&mut self, value: u16, mask: u16) -> bool {
        let mask = mask & KEYCNT_MASK;
        self.control = (self.control & !mask) | (value & mask);
        self.irq_condition()
    }

    pub fn irq_condition(self) -> bool {
        if self.control & IRQ_ENABLE == 0 {
            return false;
        }
        let selected = self.control & KEY_MASK;
        let pressed = self.held.bits() & selected;
        if self.control & IRQ_ALL_KEYS != 0 {
            pressed == selected
        } else {
            pressed != 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANY_OF: u16 = IRQ_ENABLE;
    const ALL_OF: u16 = IRQ_ENABLE | IRQ_ALL_KEYS;

    fn with_control(control: u16) -> Keypad {
        let mut keypad = Keypad::default();
        keypad.write_keycnt(control, 0xFFFF);
        keypad
    }

    #[test]
    fn keyinput_reads_all_released_as_ones() {
        assert_eq!(Keypad::default().keyinput(), 0x03FF);
    }

    #[test]
    fn each_held_key_clears_its_keyinput_bit() {
        let order = [
            Keys::A,
            Keys::B,
            Keys::SELECT,
            Keys::START,
            Keys::RIGHT,
            Keys::LEFT,
            Keys::UP,
            Keys::DOWN,
            Keys::R,
            Keys::L,
        ];
        let mut keypad = Keypad::default();
        for (bit, key) in order.into_iter().enumerate() {
            keypad.set_keys(key);
            assert_eq!(keypad.keyinput(), 0x03FF & !(1 << bit), "{key:?}");
        }
    }

    #[test]
    fn keyinput_upper_bits_read_zero() {
        let mut keypad = Keypad::default();
        keypad.set_keys(Keys::from_bits_retain(0xFC00));
        assert_eq!(keypad.keyinput(), 0x03FF);
        keypad.set_keys(Keys::all());
        assert_eq!(keypad.keyinput(), 0);
    }

    #[test]
    fn keycnt_reads_back_keys_and_both_control_bits() {
        assert_eq!(with_control(0xFFFF).keycnt(), 0xC3FF);
        let mut keypad = with_control(0x4001);
        keypad.write_keycnt(0x8000, 0xFF00);
        assert_eq!(keypad.keycnt(), 0x8001);
    }

    #[test]
    fn no_irq_while_disabled() {
        let mut keypad = with_control(Keys::A.bits());
        assert!(!keypad.set_keys(Keys::A));
        let mut keypad = with_control(IRQ_ALL_KEYS | Keys::A.bits());
        assert!(!keypad.set_keys(Keys::A));
    }

    #[test]
    fn any_of_mode_fires_on_one_selected_key() {
        let mut keypad = with_control(ANY_OF | (Keys::A | Keys::START).bits());
        assert!(!keypad.set_keys(Keys::empty()));
        assert!(!keypad.set_keys(Keys::B | Keys::L));
        assert!(keypad.set_keys(Keys::START));
        assert!(keypad.set_keys(Keys::A | Keys::B));
    }

    #[test]
    fn all_of_mode_needs_every_selected_key() {
        let soft_reset = Keys::A | Keys::B | Keys::SELECT | Keys::START;
        let mut keypad = with_control(ALL_OF | soft_reset.bits());
        assert!(!keypad.set_keys(Keys::A | Keys::B | Keys::START));
        assert!(keypad.set_keys(soft_reset));
        assert!(keypad.set_keys(soft_reset | Keys::L));
        assert!(!keypad.set_keys(Keys::A));
    }

    #[test]
    fn writing_keycnt_checks_keys_already_held() {
        let mut keypad = Keypad::default();
        keypad.set_keys(Keys::R | Keys::L);
        assert!(!keypad.write_keycnt(Keys::L.bits(), 0xFFFF));
        assert!(keypad.write_keycnt(ANY_OF | Keys::L.bits(), 0xFFFF));
        assert!(keypad.write_keycnt(ALL_OF | (Keys::R | Keys::L).bits(), 0xFFFF));
        assert!(!keypad.write_keycnt(ALL_OF | (Keys::R | Keys::A).bits(), 0xFFFF));
    }
}
