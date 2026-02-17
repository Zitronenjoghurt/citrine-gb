use crate::gb::bus::Bus;

#[derive(Debug, Default)]
pub struct Cpu {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    f: Flags,
    h: u8,
    l: u8,
    sp: u16,
    pc: u16,
    ime: bool,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_dmg(header_checksum: u8) -> Self {
        let flags = if header_checksum == 0x00 {
            Flags {
                zero: true,
                subtract: false,
                half_carry: true,
                carry: true,
            }
        } else {
            Flags {
                zero: true,
                ..Default::default()
            }
        };

        Self {
            a: 0x01,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            f: flags,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
        }
    }

    pub fn new_cgb() -> Self {
        let flags = Flags {
            zero: true,
            ..Default::default()
        };

        Self {
            a: 0x11,
            b: 0x00,
            c: 0x00,
            d: 0xFF,
            e: 0x56,
            f: flags,
            h: 0x00,
            l: 0x0D,
            sp: 0xFFFE,
            pc: 0x100,
            ime: false,
        }
    }

    pub fn cycle(&mut self, bus: &mut Bus) {}
}

#[derive(Debug, Default)]
pub struct Flags {
    /// Z = Set to true if the result of the operation is equal to 0
    pub zero: bool,
    /// N = Set to true if the operation was a subtraction
    pub subtract: bool,
    /// H = Set to true if there was an overflow from the lower 4 bits to the upper 4 bits
    pub half_carry: bool,
    /// C = Set to true if the operation resulted in an overflow
    pub carry: bool,
}

impl From<Flags> for u8 {
    fn from(value: Flags) -> Self {
        (if value.zero { 0b1000_0000 } else { 0 })
            | (if value.subtract { 0b0100_0000 } else { 0 })
            | (if value.half_carry { 0b0010_0000 } else { 0 })
            | (if value.carry { 0b0001_0000 } else { 0 })
    }
}

impl From<u8> for Flags {
    fn from(value: u8) -> Self {
        Self {
            zero: (value & 0b1000_0000) != 0,
            subtract: (value & 0b0100_0000) != 0,
            half_carry: (value & 0b0010_0000) != 0,
            carry: (value & 0b0001_0000) != 0,
        }
    }
}
