#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InterruptController {
    pub enable: u8,
    pub flag: InterruptFlags,
}

impl Default for InterruptController {
    fn default() -> Self {
        Self {
            enable: 0x00,
            flag: 0xE1.into(),
        }
    }
}

impl InterruptController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn soft_reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_enabled(&self, interrupt: Interrupt) -> bool {
        self.enable & interrupt.mask() != 0
    }
}

pub trait ICInterface {
    fn request_interrupt(&mut self, interrupt: Interrupt);
    fn enabled_interrupts(&self) -> u8 {
        0
    }
    fn next_interrupt(&self, _enabled: u8) -> Option<Interrupt> {
        None
    }
    fn acknowledge_interrupt(&mut self, _interrupt: Interrupt) {}
    fn has_pending_interrupt(&self) -> bool {
        false
    }
}

impl ICInterface for InterruptController {
    fn request_interrupt(&mut self, interrupt: Interrupt) {
        self.flag.set(interrupt, true);
    }

    fn enabled_interrupts(&self) -> u8 {
        self.enable
    }

    fn next_interrupt(&self, enabled: u8) -> Option<Interrupt> {
        let pending = enabled & self.flag.bits();
        Interrupt::PRIORITY
            .iter()
            .find(|interrupt| pending & interrupt.mask() != 0)
            .copied()
    }

    fn acknowledge_interrupt(&mut self, interrupt: Interrupt) {
        self.flag.set(interrupt, false);
    }

    fn has_pending_interrupt(&self) -> bool {
        self.enable & self.flag.bits() != 0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Interrupt {
    Joypad,
    Timer,
    Serial,
    Lcd,
    VBlank,
}

impl Interrupt {
    pub const PRIORITY: &'static [Self; 5] = &[
        Self::VBlank,
        Self::Lcd,
        Self::Timer,
        Self::Serial,
        Self::Joypad,
    ];

    pub fn mask(&self) -> u8 {
        match self {
            Self::VBlank => 0x01,
            Self::Lcd => 0x02,
            Self::Timer => 0x04,
            Self::Serial => 0x08,
            Self::Joypad => 0x10,
        }
    }

    pub fn vector(&self) -> u16 {
        match self {
            Self::VBlank => 0x40,
            Self::Lcd => 0x48,
            Self::Timer => 0x50,
            Self::Serial => 0x58,
            Self::Joypad => 0x60,
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "FlagBits", into = "FlagBits"))]
pub struct InterruptFlags(u8);

impl InterruptFlags {
    pub fn set(&mut self, interrupt: Interrupt, enabled: bool) {
        if enabled {
            self.0 |= interrupt.mask();
        } else {
            self.0 &= !interrupt.mask();
        }
    }

    pub fn is_enabled(&self, interrupt: Interrupt) -> bool {
        self.0 & interrupt.mask() != 0
    }

    pub fn bits(&self) -> u8 {
        self.0
    }
}

impl From<u8> for InterruptFlags {
    fn from(value: u8) -> Self {
        Self(value & 0x1F)
    }
}

impl From<InterruptFlags> for u8 {
    fn from(value: InterruptFlags) -> Self {
        0xE0 | value.0
    }
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "InterruptFlags")]
struct FlagBits {
    joypad: bool,
    serial: bool,
    timer: bool,
    lcd: bool,
    vblank: bool,
}

#[cfg(feature = "serde")]
impl From<FlagBits> for InterruptFlags {
    fn from(bits: FlagBits) -> Self {
        let mut flags = Self::default();
        flags.set(Interrupt::Joypad, bits.joypad);
        flags.set(Interrupt::Serial, bits.serial);
        flags.set(Interrupt::Timer, bits.timer);
        flags.set(Interrupt::Lcd, bits.lcd);
        flags.set(Interrupt::VBlank, bits.vblank);
        flags
    }
}

#[cfg(feature = "serde")]
impl From<InterruptFlags> for FlagBits {
    fn from(flags: InterruptFlags) -> Self {
        Self {
            joypad: flags.is_enabled(Interrupt::Joypad),
            serial: flags.is_enabled(Interrupt::Serial),
            timer: flags.is_enabled(Interrupt::Timer),
            lcd: flags.is_enabled(Interrupt::Lcd),
            vblank: flags.is_enabled(Interrupt::VBlank),
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn flags_keep_their_five_named_bools_in_states() {
        let mut flags = InterruptFlags::default();
        flags.set(Interrupt::Timer, true);
        flags.set(Interrupt::VBlank, true);
        let value = serde_json::to_value(flags).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "joypad": false,
                "serial": false,
                "timer": true,
                "lcd": false,
                "vblank": true,
            })
        );
        let back: InterruptFlags = serde_json::from_value(value).unwrap();
        assert_eq!(back, flags);
        assert_eq!(u8::from(back), 0xE5);
    }
}
