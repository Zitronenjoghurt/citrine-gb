pub const DATA: usize = 0xC4;
pub const DIRECTION: usize = 0xC6;
pub const CONTROL: usize = 0xC8;
pub const END: usize = 0xCA;
const PINS: u8 = 0x0F;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Gpio {
    data: u8,
    direction: u8,
    readable: bool,
}

impl Gpio {
    pub fn readable(&self) -> bool {
        self.readable
    }

    pub fn pins(&self) -> u8 {
        self.data & self.direction
    }

    pub fn read16(&self, offset: usize, inputs: u8) -> u16 {
        match offset {
            DATA => u16::from(self.pins() | inputs & !self.direction & PINS),
            DIRECTION => u16::from(self.direction),
            CONTROL => u16::from(self.readable),
            _ => 0,
        }
    }

    pub fn write16(&mut self, offset: usize, value: u16) {
        let value = value as u8 & PINS;
        match offset {
            DATA => self.data = value,
            DIRECTION => self.direction = value,
            CONTROL => self.readable = value & 1 != 0,
            _ => {}
        }
    }
}
