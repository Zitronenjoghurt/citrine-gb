use crate::gb::bus::BusInterface;
use crate::instructions::{Instruction, R16Mem, R16, R8};
use crate::utils::{add_bytes, add_words, hi, lo, sub_bytes, word};

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
    pub ir: u8,
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
            ir: 0x00,
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
            ir: 0x00,
            ime: false,
        }
    }

    pub fn step(&mut self, bus: &mut impl BusInterface) {
        match self.decode() {
            Instruction::Nop => {}
            Instruction::LD_rr_nn(r16) => self.ld_rr_nn(bus, r16),
            Instruction::LD_rr_A(r16mem) => self.ld_rr_a(bus, r16mem),
            Instruction::LD_A_rr(r16mem) => self.ld_a_rr(bus, r16mem),
            Instruction::LD_nn_SP => self.ld_nn_sp(bus),
            Instruction::INC_rr(r16) => self.inc_rr(bus, r16),
            Instruction::DEC_rr(r16) => self.dec_rr(bus, r16),
            Instruction::ADD_HL_rr(r16) => self.add_hl_rr(bus, r16),
            Instruction::INC_r(r8) => self.inc_r(bus, r8),
            Instruction::DEC_r(r8) => self.dec_r(bus, r8),
            Instruction::LD_r_n(r8) => self.ld_r_n(bus, r8),
        }

        self.fetch(bus);
    }

    pub fn fetch(&mut self, bus: &mut impl BusInterface) {
        self.ir = self.read_program(bus);
    }

    pub fn decode(&mut self) -> Instruction {
        Instruction::decode(self.ir)
    }

    pub fn read_program(&mut self, bus: &mut impl BusInterface) -> u8 {
        let byte = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        byte
    }

    pub fn read_program_lh(&mut self, bus: &mut impl BusInterface) -> (u8, u8) {
        (self.read_program(bus), self.read_program(bus))
    }

    pub fn read_program_nn(&mut self, bus: &mut impl BusInterface) -> u16 {
        word(self.read_program(bus), self.read_program(bus))
    }
}

// Instruction execution
impl Cpu {
    pub fn ld_rr_nn(&mut self, bus: &mut impl BusInterface, r16: R16) {
        let (low, high) = self.read_program_lh(bus);
        self.set_r16_lh(r16, low, high);
    }

    pub fn ld_rr_a(&mut self, bus: &mut impl BusInterface, r16_mem: R16Mem) {
        let address = self.get_r16mem(r16_mem);
        bus.write(address, self.a);
    }

    pub fn ld_a_rr(&mut self, bus: &mut impl BusInterface, r16_mem: R16Mem) {
        let address = self.get_r16mem(r16_mem);
        self.a = bus.read(address);
    }

    pub fn ld_nn_sp(&mut self, bus: &mut impl BusInterface) {
        let address = self.read_program_nn(bus);
        bus.write_word(address, self.sp);
    }

    pub fn inc_rr(&mut self, bus: &mut impl BusInterface, r16: R16) {
        bus.cycle();

        self.set_r16_nn(r16, self.get_r16_nn(r16).wrapping_add(1));
    }

    pub fn dec_rr(&mut self, bus: &mut impl BusInterface, r16: R16) {
        bus.cycle();

        self.set_r16_nn(r16, self.get_r16_nn(r16).wrapping_sub(1));
    }

    pub fn add_hl_rr(&mut self, bus: &mut impl BusInterface, r16: R16) {
        bus.cycle();

        let (result, hc, c) = add_words(self.get_r16_nn(R16::HL), self.get_r16_nn(r16));
        self.set_r16_nn(R16::HL, result);
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn inc_r(&mut self, bus: &mut impl BusInterface, r8: R8) {
        let (result, hc, _) = add_bytes(self.get_r8(bus, r8), 1);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
    }

    pub fn dec_r(&mut self, bus: &mut impl BusInterface, r8: R8) {
        let (result, hc, _) = sub_bytes(self.get_r8(bus, r8), 1);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
    }

    pub fn ld_r_n(&mut self, bus: &mut impl BusInterface, r8: R8) {
        let value = self.read_program(bus);
        self.set_r8(bus, r8, value);
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

    pub fn get_r16_nn(&self, r16: R16) -> u16 {
        match r16 {
            R16::BC => word(self.c, self.b),
            R16::DE => word(self.e, self.d),
            R16::HL => word(self.l, self.h),
            R16::SP => self.sp,
        }
    }

    pub fn get_r16mem(&mut self, r16mem: R16Mem) -> u16 {
        match r16mem {
            R16Mem::BC => word(self.c, self.b),
            R16Mem::DE => word(self.e, self.d),
            R16Mem::HLinc => {
                let value = word(self.l, self.h);
                self.set_r16_nn(R16::HL, value.wrapping_add(1));
                value
            }
            R16Mem::HLdec => {
                let value = word(self.l, self.h);
                self.set_r16_nn(R16::HL, value.wrapping_sub(1));
                value
            }
        }
    }

    pub fn get_r8(&self, bus: &mut impl BusInterface, r8: R8) -> u8 {
        match r8 {
            R8::B => self.b,
            R8::C => self.c,
            R8::D => self.d,
            R8::E => self.e,
            R8::H => self.h,
            R8::L => self.l,
            R8::HL => bus.read(self.get_r16_nn(R16::HL)),
            R8::A => self.a,
        }
    }

    pub fn set_r8(&mut self, bus: &mut impl BusInterface, r8: R8, value: u8) {
        match r8 {
            R8::B => self.b = value,
            R8::C => self.c = value,
            R8::D => self.d = value,
            R8::E => self.e = value,
            R8::H => self.h = value,
            R8::L => self.l = value,
            R8::HL => bus.write(self.get_r16_nn(R16::HL), value),
            R8::A => self.a = value,
        }
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
