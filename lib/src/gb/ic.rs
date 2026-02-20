#[derive(Debug)]
pub struct InterruptController {
    pub enable: InterruptFlags,
    pub flag: InterruptFlags,
}

impl Default for InterruptController {
    fn default() -> Self {
        Self {
            enable: 0x00.into(),
            flag: 0xE1.into(),
        }
    }
}

impl InterruptController {
    pub fn new() -> Self {
        Self::default()
    }
}

pub trait ICInterface {
    fn request_interrupt(&mut self, interrupt: Interrupt);
    fn take_interrupt(&mut self) -> Option<Interrupt> {
        None
    }
}

impl ICInterface for InterruptController {
    fn request_interrupt(&mut self, interrupt: Interrupt) {
        self.flag.set(interrupt, true);
    }

    fn take_interrupt(&mut self) -> Option<Interrupt> {
        for interrupt in Interrupt::PRIORITY {
            if self.enable.is_enabled(*interrupt) && self.flag.is_enabled(*interrupt) {
                self.flag.set(*interrupt, false);
                return Some(*interrupt);
            }
        }
        None
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Interrupt {
    Joypad,
    Timer,
    Serial,
    Lcd,
    Vblank,
}

impl Interrupt {
    pub const PRIORITY: &'static [Self; 5] = &[
        Self::Vblank,
        Self::Lcd,
        Self::Timer,
        Self::Serial,
        Self::Joypad,
    ];

    pub fn vector(&self) -> u16 {
        match self {
            Self::Vblank => 0x40,
            Self::Lcd => 0x48,
            Self::Timer => 0x50,
            Self::Serial => 0x58,
            Self::Joypad => 0x60,
        }
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub struct InterruptFlags {
    pub joypad: bool,
    pub serial: bool,
    pub timer: bool,
    pub lcd: bool,
    pub vblank: bool,
}

impl InterruptFlags {
    pub fn set(&mut self, interrupt: Interrupt, enabled: bool) {
        match interrupt {
            Interrupt::Joypad => self.joypad = enabled,
            Interrupt::Timer => self.timer = enabled,
            Interrupt::Serial => self.serial = enabled,
            Interrupt::Lcd => self.lcd = enabled,
            Interrupt::Vblank => self.vblank = enabled,
        }
    }

    pub fn is_enabled(&self, interrupt: Interrupt) -> bool {
        match interrupt {
            Interrupt::Joypad => self.joypad,
            Interrupt::Timer => self.timer,
            Interrupt::Serial => self.serial,
            Interrupt::Lcd => self.lcd,
            Interrupt::Vblank => self.vblank,
        }
    }
}

impl From<u8> for InterruptFlags {
    fn from(value: u8) -> Self {
        Self {
            joypad: (value & 0b0001_0000) != 0,
            serial: (value & 0b0000_1000) != 0,
            timer: (value & 0b0000_0100) != 0,
            lcd: (value & 0b0000_0010) != 0,
            vblank: (value & 0b0000_0001) != 0,
        }
    }
}

impl From<InterruptFlags> for u8 {
    fn from(value: InterruptFlags) -> Self {
        0xE0 | ((value.joypad as u8) << 4)
            | ((value.serial as u8) << 3)
            | ((value.timer as u8) << 2)
            | ((value.lcd as u8) << 1)
            | (value.vblank as u8)
    }
}
