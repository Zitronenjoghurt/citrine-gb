use crate::gb::bus::CpuBusInterface;
use crate::gb::ic::ICInterface;
use crate::gb::GbModel;
use crate::instructions::{Cond, Instruction, R16Mem, R16Stk, R16, R8};
use crate::utils::*;

#[cfg(not(feature = "debug"))]
pub trait Bus: CpuBusInterface + ICInterface {}
#[cfg(not(feature = "debug"))]
impl<T: CpuBusInterface + ICInterface> Bus for T {}
#[cfg(feature = "debug")]
pub trait Bus: CpuBusInterface + ICInterface + crate::debug::DebuggerInterface {}
#[cfg(feature = "debug")]
impl<T: CpuBusInterface + ICInterface + crate::debug::DebuggerInterface> Bus for T {}

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
    pub ime_next: bool,
    pub halted: bool,
    pub model: GbModel,
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
            ime_next: false,
            halted: false,
            model: GbModel::Dmg,
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
            ime_next: false,
            halted: false,
            model: GbModel::Cgb,
        }
    }

    pub fn step(&mut self, bus: &mut impl Bus) {
        #[cfg(feature = "debug")]
        {
            if bus.break_at(self.pc) {
                return;
            }
        }

        if self.halted {
            bus.cycle();
            if bus.has_pending_interrupt() {
                // ToDo: Halt bug => https://gbdev.io/pandocs/halt.html#halt-bug
                self.halted = false;
            }
            return;
        }

        self.interrupt_handler(bus);

        match self.decode(bus) {
            Instruction::NOP => {}
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
            Instruction::RLCA => self.rlca(),
            Instruction::RRCA => self.rrca(),
            Instruction::RLA => self.rla(),
            Instruction::RRA => self.rra(),
            Instruction::DAA => self.daa(),
            Instruction::CPL => self.cpl(),
            Instruction::SCF => self.scf(),
            Instruction::CCF => self.ccf(),
            Instruction::JR_n => self.jr_n(bus),
            Instruction::JR_c_n(cond) => self.jr_c_n(bus, cond),
            Instruction::STOP => {}
            Instruction::HALT | Instruction::LD_r_r(R8::HL, R8::HL) => self.halted = true,
            Instruction::LD_r_r(dest, src) => self.ld_r_r(bus, dest, src),
            Instruction::ADD_r(r8) => self.add_r(bus, r8),
            Instruction::ADC_r(r8) => self.adc_r(bus, r8),
            Instruction::SUB_r(r8) => self.sub_r(bus, r8),
            Instruction::SBC_r(r8) => self.sbc_r(bus, r8),
            Instruction::AND_r(r8) => self.and_r(bus, r8),
            Instruction::XOR_r(r8) => self.xor_r(bus, r8),
            Instruction::OR_r(r8) => self.or_r(bus, r8),
            Instruction::CP_r(r8) => self.cp_r(bus, r8),
            Instruction::ADD_n => self.add_n(bus),
            Instruction::ADC_n => self.adc_n(bus),
            Instruction::SUB_n => self.sub_n(bus),
            Instruction::SBC_n => self.sbc_n(bus),
            Instruction::AND_n => self.and_n(bus),
            Instruction::XOR_n => self.xor_n(bus),
            Instruction::OR_n => self.or_n(bus),
            Instruction::CP_n => self.cp_n(bus),
            Instruction::POP(stk) => self.pop_rr(bus, stk),
            Instruction::PUSH(stk) => self.push_rr(bus, stk),
            Instruction::RET => self.ret(bus),
            Instruction::RETI => self.reti(bus),
            Instruction::RET_c(cond) => self.ret_c(bus, cond),
            Instruction::JP_nn => self.jump_nn(bus),
            Instruction::JP_c_nn(cond) => self.jump_c_nn(bus, cond),
            Instruction::JP_HL => self.jump_hl(),
            Instruction::CALL_nn => self.call_nn(bus),
            Instruction::CALL_c_nn(cond) => self.call_c_nn(bus, cond),
            Instruction::RST_n(tgt) => self.rst(bus, tgt),
            Instruction::LDH_C_A => self.ldh_c_a(bus),
            Instruction::LDH_A_C => self.ldh_a_c(bus),
            Instruction::LDH_n_A => self.ldh_n_a(bus),
            Instruction::LDH_A_n => self.ldh_a_n(bus),
            Instruction::LD_nn_A => self.ld_nn_a(bus),
            Instruction::LD_A_nn => self.ld_a_nn(bus),
            Instruction::ADD_SP_n => self.add_sp_n(bus),
            Instruction::LD_HL_SP_n => self.ld_hl_sp_n(bus),
            Instruction::LD_SP_HL => self.ld_sp_hl(bus),
            Instruction::DI => {
                self.ime_next = false;
                self.ime = false;
            }
            Instruction::EI => self.ime_next = true,
            Instruction::RLC_r(r8) => self.rlc_r(bus, r8),
            Instruction::RRC_r(r8) => self.rrc_r(bus, r8),
            Instruction::RL_r(r8) => self.rl_r(bus, r8),
            Instruction::RR_r(r8) => self.rr_r(bus, r8),
            Instruction::SLA_r(r8) => self.sla_r(bus, r8),
            Instruction::SRA_r(r8) => self.sra_r(bus, r8),
            Instruction::SWAP_r(r8) => self.swap_r(bus, r8),
            Instruction::SRL_r(r8) => self.srl_r(bus, r8),
            Instruction::BIT_r(index, r8) => self.bit_r(bus, r8, index),
            Instruction::RES_r(index, r8) => self.res_r(bus, r8, index),
            Instruction::SET_r(index, r8) => self.set_r(bus, r8, index),
        }

        self.fetch(bus);
    }

    pub fn fetch(&mut self, bus: &mut impl Bus) {
        self.ir = self.read_program(bus);
    }

    pub fn decode(&mut self, bus: &mut impl Bus) -> Instruction {
        if self.ir != 0xCB {
            Instruction::decode(self.ir)
        } else {
            self.fetch(bus);
            Instruction::decode_prefixed(self.ir)
        }
    }

    fn interrupt_handler(&mut self, bus: &mut impl Bus) {
        if self.ime
            && let Some(interrupt) = bus.take_interrupt()
        {
            self.ime = false;
            self.ime_next = false;

            bus.cycle();
            bus.cycle();
            self.push_word(bus, self.pc.wrapping_sub(1)); // ToDo: Potential pain point, check if this is correct
            self.pc = interrupt.vector();

            self.fetch(bus);
        }

        // EI is delayed by one instruction
        self.ime = self.ime_next;
    }

    pub fn soft_reset(&mut self, header_checksum: u8) {
        match self.model {
            GbModel::Dmg => *self = Self::new_dmg(header_checksum),
            GbModel::Cgb => *self = Self::new_cgb(),
        }
    }
}

// Program helpers
impl Cpu {
    pub fn read_program(&mut self, bus: &mut impl Bus) -> u8 {
        let byte = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        byte
    }

    pub fn read_program_lh(&mut self, bus: &mut impl Bus) -> (u8, u8) {
        (self.read_program(bus), self.read_program(bus))
    }

    pub fn read_program_nn(&mut self, bus: &mut impl Bus) -> u16 {
        word(self.read_program(bus), self.read_program(bus))
    }
}

// Stack helpers
impl Cpu {
    pub fn pop(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        value
    }

    pub fn pop_word(&mut self, bus: &mut impl Bus) -> u16 {
        word(self.pop(bus), self.pop(bus))
    }

    pub fn push(&mut self, bus: &mut impl Bus, value: u8) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, value);
    }

    pub fn push_word(&mut self, bus: &mut impl Bus, value: u16) {
        self.push(bus, hi(value));
        self.push(bus, lo(value));
    }
}

// Instruction execution
impl Cpu {
    pub fn ld_rr_nn(&mut self, bus: &mut impl Bus, r16: R16) {
        let (low, high) = self.read_program_lh(bus);
        self.set_r16_lh(r16, low, high);
    }

    pub fn ld_rr_a(&mut self, bus: &mut impl Bus, r16_mem: R16Mem) {
        let address = self.get_r16mem(r16_mem);
        bus.write(address, self.a);
    }

    pub fn ld_a_rr(&mut self, bus: &mut impl Bus, r16_mem: R16Mem) {
        let address = self.get_r16mem(r16_mem);
        self.a = bus.read(address);
    }

    pub fn ld_nn_sp(&mut self, bus: &mut impl Bus) {
        let address = self.read_program_nn(bus);
        bus.write_word(address, self.sp);
    }

    pub fn inc_rr(&mut self, bus: &mut impl Bus, r16: R16) {
        bus.cycle();

        self.set_r16_nn(r16, self.get_r16_nn(r16).wrapping_add(1));
    }

    pub fn dec_rr(&mut self, bus: &mut impl Bus, r16: R16) {
        bus.cycle();

        self.set_r16_nn(r16, self.get_r16_nn(r16).wrapping_sub(1));
    }

    pub fn add_hl_rr(&mut self, bus: &mut impl Bus, r16: R16) {
        bus.cycle();

        let (result, hc, c) = add_words(self.get_r16_nn(R16::HL), self.get_r16_nn(r16));
        self.set_r16_nn(R16::HL, result);
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn inc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, hc, _) = add_bytes(self.get_r8(bus, r8), 1);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
    }

    pub fn dec_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, hc, _) = sub_bytes(self.get_r8(bus, r8), 1);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
    }

    pub fn ld_r_n(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.read_program(bus);
        self.set_r8(bus, r8, value);
    }

    pub fn rlca(&mut self) {
        let (result, c) = rotate_left_get_carry(self.a);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn rrca(&mut self) {
        let (result, c) = rotate_right_get_carry(self.a);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn rla(&mut self) {
        let (result, c) = rotate_left_through_carry(self.a, self.f.carry);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn rra(&mut self) {
        let (result, c) = rotate_right_through_carry(self.a, self.f.carry);
        self.a = result;
        self.f.carry = c;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn daa(&mut self) {
        let mut carry = self.f.carry;
        let mut adjust: u8 = 0;

        let result = if self.f.subtract {
            if self.f.half_carry {
                adjust += 0x06;
            }
            if self.f.carry {
                adjust += 0x60;
            }
            self.a.wrapping_sub(adjust)
        } else {
            if self.f.half_carry || self.a & 0x0F > 0x09 {
                adjust += 0x06;
            }
            if self.f.carry || self.a > 0x99 {
                adjust += 0x60;
                carry = true;
            }
            self.a.wrapping_add(adjust)
        };

        self.a = result;
        self.f.carry = carry;
        self.f.zero = result == 0;
        self.f.half_carry = false;
    }

    pub fn cpl(&mut self) {
        self.a = !self.a;
        self.f.subtract = true;
        self.f.half_carry = true;
    }

    pub fn scf(&mut self) {
        self.f.carry = true;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn ccf(&mut self) {
        self.f.carry = !self.f.carry;
        self.f.subtract = false;
        self.f.half_carry = false;
    }

    pub fn jr_n(&mut self, bus: &mut impl Bus) {
        let offset = self.read_program(bus) as i8;

        bus.cycle();
        let (new_pc, _, _) = add_word_signed_byte(self.pc, offset);

        self.pc = new_pc;
    }

    pub fn jr_c_n(&mut self, bus: &mut impl Bus, cond: Cond) {
        let offset = self.read_program(bus) as i8;

        if self.f.cond_true(cond) {
            bus.cycle();
            let (new_pc, _, _) = add_word_signed_byte(self.pc, offset);
            self.pc = new_pc;
        }
    }

    pub fn ld_r_r(&mut self, bus: &mut impl Bus, dest: R8, src: R8) {
        let value = self.get_r8(bus, src);
        self.set_r8(bus, dest, value);
    }

    pub fn add_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.add_a(value);
    }

    pub fn adc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.adc_a(value);
    }

    pub fn sub_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.sub_a(value);
    }

    pub fn sbc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.sbc_a(value);
    }

    pub fn and_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.and_a(value);
    }

    pub fn xor_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.xor_a(value);
    }

    pub fn or_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.or_a(value);
    }

    pub fn cp_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        self.cp_a(value);
    }

    pub fn add_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.add_a(value);
    }

    pub fn adc_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.adc_a(value);
    }

    pub fn sub_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.sub_a(value);
    }

    pub fn sbc_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.sbc_a(value);
    }

    pub fn and_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.and_a(value);
    }

    pub fn xor_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.xor_a(value);
    }

    pub fn or_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.or_a(value);
    }

    pub fn cp_n(&mut self, bus: &mut impl Bus) {
        let value = self.read_program(bus);
        self.cp_a(value);
    }

    pub fn pop_rr(&mut self, bus: &mut impl Bus, r16stk: R16Stk) {
        let value = self.pop_word(bus);
        self.set_r16stk(r16stk, value);
    }

    pub fn push_rr(&mut self, bus: &mut impl Bus, r16stk: R16Stk) {
        // Internal cycle: CPU computes decremented SP before first write
        bus.cycle();

        let value = self.get_r16stk(r16stk);
        self.push_word(bus, value);
    }

    pub fn ret(&mut self, bus: &mut impl Bus) {
        let address = self.pop_word(bus);

        bus.cycle();
        self.pc = address;
    }

    pub fn reti(&mut self, bus: &mut impl Bus) {
        self.ret(bus);
        self.ime = true;
    }

    pub fn ret_c(&mut self, bus: &mut impl Bus, cond: Cond) {
        bus.cycle();

        if self.f.cond_true(cond) {
            self.ret(bus);
        }
    }

    pub fn jump_nn(&mut self, bus: &mut impl Bus) {
        let address = self.read_program_nn(bus);

        bus.cycle();
        self.pc = address;
    }

    pub fn jump_c_nn(&mut self, bus: &mut impl Bus, cond: Cond) {
        let address = self.read_program_nn(bus);

        if self.f.cond_true(cond) {
            bus.cycle();
            self.pc = address;
        }
    }

    pub fn jump_hl(&mut self) {
        self.pc = self.get_r16_nn(R16::HL);
    }

    pub fn call_nn(&mut self, bus: &mut impl Bus) {
        let address = self.read_program_nn(bus);

        bus.cycle();
        self.push_word(bus, self.pc);

        self.pc = address;
    }

    pub fn call_c_nn(&mut self, bus: &mut impl Bus, cond: Cond) {
        let address = self.read_program_nn(bus);

        if self.f.cond_true(cond) {
            bus.cycle();
            self.push_word(bus, self.pc);
            self.pc = address;
        }
    }

    pub fn rst(&mut self, bus: &mut impl Bus, address_lsb: u8) {
        bus.cycle();
        self.push_word(bus, self.pc);
        self.pc = word(address_lsb, 0x00);
    }

    pub fn ldh_c_a(&mut self, bus: &mut impl Bus) {
        bus.write(word(self.c, 0xFF), self.a);
    }

    pub fn ldh_a_c(&mut self, bus: &mut impl Bus) {
        self.a = bus.read(word(self.c, 0xFF));
    }

    pub fn ldh_n_a(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        bus.write(word(address_low, 0xFF), self.a);
    }

    pub fn ldh_a_n(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        self.a = bus.read(word(address_low, 0xFF));
    }

    pub fn ld_nn_a(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        let address_high = self.read_program(bus);
        bus.write(word(address_low, address_high), self.a);
    }

    pub fn ld_a_nn(&mut self, bus: &mut impl Bus) {
        let address_low = self.read_program(bus);
        let address_high = self.read_program(bus);
        self.a = bus.read(word(address_low, address_high));
    }

    pub fn add_sp_n(&mut self, bus: &mut impl Bus) {
        let offset = self.read_program(bus) as i8;

        bus.cycle();
        let (result, hc, c) = add_word_signed_byte(self.sp, offset);

        bus.cycle();
        self.sp = result;
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn ld_hl_sp_n(&mut self, bus: &mut impl Bus) {
        let offset = self.read_program(bus) as i8;

        bus.cycle();
        let (result, hc, c) = add_word_signed_byte(self.sp, offset);

        self.set_r16_nn(R16::HL, result);
        self.f.zero = false;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn ld_sp_hl(&mut self, bus: &mut impl Bus) {
        bus.cycle();
        self.sp = self.get_r16_nn(R16::HL);
    }

    pub fn rlc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_left_get_carry(self.get_r8(bus, r8));
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn rrc_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_right_get_carry(self.get_r8(bus, r8));
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn rl_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_left_through_carry(self.get_r8(bus, r8), self.f.carry);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn rr_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let (result, c) = rotate_right_through_carry(self.get_r8(bus, r8), self.f.carry);
        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = c;
    }

    pub fn sla_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let carry = get_bit(value, 7);
        let result = value << 1;

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = carry;
    }

    pub fn sra_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let carry = get_bit(value, 0);
        // Shift right while persisting the sign bit
        let result = set_bit(value >> 1, 7, get_bit(value, 7));

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = carry;
    }

    pub fn swap_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let result = value.rotate_right(4);

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = false;
    }

    pub fn srl_r(&mut self, bus: &mut impl Bus, r8: R8) {
        let value = self.get_r8(bus, r8);
        let carry = get_bit(value, 0);
        let result = value >> 1;

        self.set_r8(bus, r8, result);
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = carry;
    }

    pub fn bit_r(&mut self, bus: &mut impl Bus, r8: R8, index: u8) {
        let value = self.get_r8(bus, r8);
        self.f.zero = !get_bit(value, (index & 0b111) as usize);
        self.f.subtract = false;
        self.f.half_carry = true;
    }

    pub fn set_r(&mut self, bus: &mut impl Bus, r8: R8, index: u8) {
        let value = self.get_r8(bus, r8);
        self.set_r8(bus, r8, set_bit(value, (index & 0b111) as usize, true));
    }

    pub fn res_r(&mut self, bus: &mut impl Bus, r8: R8, index: u8) {
        let value = self.get_r8(bus, r8);
        self.set_r8(bus, r8, set_bit(value, (index & 0b111) as usize, false));
    }
}

// 8-Bit Arithmetics
impl Cpu {
    pub fn add_a(&mut self, value: u8) {
        let (result, hc, c) = add_bytes(self.a, value);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn adc_a(&mut self, value: u8) {
        let (result, hc, c) = add_bytes_carry(self.a, value, self.f.carry);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = false;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn sub_a(&mut self, value: u8) {
        let (result, hc, c) = sub_bytes(self.a, value);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn sbc_a(&mut self, value: u8) {
        let (result, hc, c) = sub_bytes_carry(self.a, value, self.f.carry);
        self.a = result;
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
        self.f.carry = c;
    }

    pub fn and_a(&mut self, value: u8) {
        self.a &= value;
        self.f.zero = self.a == 0;
        self.f.subtract = false;
        self.f.half_carry = true;
        self.f.carry = false;
    }

    pub fn xor_a(&mut self, value: u8) {
        self.a ^= value;
        self.f.zero = self.a == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = false;
    }

    pub fn or_a(&mut self, value: u8) {
        self.a |= value;
        self.f.zero = self.a == 0;
        self.f.subtract = false;
        self.f.half_carry = false;
        self.f.carry = false;
    }

    pub fn cp_a(&mut self, value: u8) {
        let (result, hc, c) = sub_bytes(self.a, value);
        self.f.zero = result == 0;
        self.f.subtract = true;
        self.f.half_carry = hc;
        self.f.carry = c;
    }
}

// Register helpers
impl Cpu {
    pub fn get_r8(&self, bus: &mut impl Bus, r8: R8) -> u8 {
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

    pub fn set_r8(&mut self, bus: &mut impl Bus, r8: R8, value: u8) {
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

    pub fn get_r16stk(&mut self, r16stk: R16Stk) -> u16 {
        match r16stk {
            R16Stk::BC => word(self.c, self.b),
            R16Stk::DE => word(self.e, self.d),
            R16Stk::HL => word(self.l, self.h),
            R16Stk::AF => word(self.f.into(), self.a),
        }
    }

    pub fn set_r16stk(&mut self, r16stk: R16Stk, value: u16) {
        match r16stk {
            R16Stk::BC => self.set_r16_nn(R16::BC, value),
            R16Stk::DE => self.set_r16_nn(R16::DE, value),
            R16Stk::HL => self.set_r16_nn(R16::HL, value),
            R16Stk::AF => {
                self.f = Flags::from(lo(value));
                self.a = hi(value);
            }
        }
    }
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
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

impl Flags {
    pub fn cond_true(&self, cond: Cond) -> bool {
        match cond {
            Cond::NZ => !self.zero,
            Cond::Z => self.zero,
            Cond::NC => !self.carry,
            Cond::C => self.carry,
        }
    }
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
