use crate::gb::bus::BusInterface;
use crate::instructions::{Instruction, R16};
use crate::utils::{hi, lo, word};

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Cpu {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub f: Flags,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    pub ime: bool,
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

    pub fn cycle(&mut self, bus: &mut impl BusInterface) {
        let opcode = self.read_program(bus);
        match Instruction::decode(opcode) {
            Instruction::Nop => bus.cycle(),
            Instruction::LdRrNn(r16) => self.ld_rr_nn(bus, r16),
        }
    }

    pub fn read_program(&mut self, bus: &mut impl BusInterface) -> u8 {
        let byte = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        byte
    }

    pub fn read_program_lh(&mut self, bus: &mut impl BusInterface) -> (u8, u8) {
        (self.read_program(bus), self.read_program(bus))
    }
}

// Instruction execution
impl Cpu {
    pub fn ld_rr_nn(&mut self, bus: &mut impl BusInterface, r16: R16) {
        let (low, high) = self.read_program_lh(bus);
        self.set_r16_lh(r16, low, high);
    }
}

// Register helpers
impl Cpu {
    pub fn set_r16_lh(&mut self, r16: R16, low: u8, high: u8) {
        match r16 {
            R16::BC => {
                self.c = low;
                self.b = high;
            }
            R16::DE => {
                self.e = low;
                self.d = high;
            }
            R16::HL => {
                self.l = low;
                self.h = high;
            }
            R16::SP => self.sp = word(low, high),
        }
    }

    pub fn set_r16_nn(&mut self, r16: R16, value: u16) {
        self.set_r16_lh(r16, lo(value), hi(value))
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
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
