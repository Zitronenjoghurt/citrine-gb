mod alu;
mod transfer;

use super::exception::Exception;
use super::psr::Psr;
use super::{Cpu, CpuBus};

fn low(op: u32, at: u32) -> usize {
    ((op >> at) & 7) as usize
}

fn sign_extend(value: u32, bits: u32) -> u32 {
    let shift = 32 - bits;
    (((value << shift) as i32) >> shift) as u32
}

impl Cpu {
    pub(super) fn execute_thumb(&mut self, bus: &mut impl CpuBus, op: u32) {
        match op >> 11 {
            0b00000..=0b00010 => self.thumb_shift(bus, op),
            0b00011 => self.thumb_add_sub(bus, op),
            0b00100..=0b00111 => self.thumb_immediate(bus, op),
            0b01000 if op & (1 << 10) == 0 => self.thumb_alu(bus, op),
            0b01000 => self.thumb_hi_register(bus, op),
            0b01001 => self.thumb_load_pc_relative(bus, op),
            0b01010 | 0b01011 if op & (1 << 9) == 0 => self.thumb_register_offset(bus, op),
            0b01010 | 0b01011 => self.thumb_halfword_register_offset(bus, op),
            0b01100..=0b01111 => self.thumb_immediate_offset(bus, op),
            0b10000 | 0b10001 => self.thumb_halfword_immediate_offset(bus, op),
            0b10010 | 0b10011 => self.thumb_sp_relative(bus, op),
            0b10100 | 0b10101 => self.thumb_load_address(bus, op),
            0b10110 | 0b10111 => match (op >> 8) & 0xF {
                0b0000 => self.thumb_adjust_sp(bus, op),
                0b0100 | 0b0101 | 0b1100 | 0b1101 => self.thumb_push_pop(bus, op),
                _ => self.thumb_undefined(bus),
            },
            0b11000 | 0b11001 => self.thumb_block_transfer(bus, op),
            0b11010 | 0b11011 => match (op >> 8) & 0xF {
                0xE => self.thumb_undefined(bus),
                0xF => self.thumb_software_interrupt(bus),
                condition => self.thumb_conditional_branch(bus, op, condition),
            },
            0b11100 => self.thumb_branch(bus, sign_extend(op & 0x7FF, 11) << 1),
            0b11110 => self.thumb_long_branch_prefix(bus, op),
            0b11111 => self.thumb_long_branch_suffix(bus, op),
            _ => self.thumb_undefined(bus),
        }
    }

    fn thumb_conditional_branch(&mut self, bus: &mut impl CpuBus, op: u32, condition: u32) {
        if Psr(self.regs.cpsr()).passes(condition) {
            self.thumb_branch(bus, sign_extend(op & 0xFF, 8) << 1);
        } else {
            self.prefetch(bus);
        }
    }

    fn thumb_branch(&mut self, bus: &mut impl CpuBus, offset: u32) {
        let target = self.regs.r[15].wrapping_add(offset);
        self.prefetch(bus);
        self.regs.r[15] = target;
        self.reload(bus);
    }

    fn thumb_long_branch_prefix(&mut self, bus: &mut impl CpuBus, op: u32) {
        let high = sign_extend(op & 0x7FF, 11) << 12;
        self.regs.r[14] = self.regs.r[15].wrapping_add(high);
        self.prefetch(bus);
    }

    fn thumb_long_branch_suffix(&mut self, bus: &mut impl CpuBus, op: u32) {
        let target = self.regs.r[14].wrapping_add((op & 0x7FF) << 1);
        let link = self.regs.r[15].wrapping_sub(2) | 1;
        self.prefetch(bus);
        self.regs.r[14] = link;
        self.regs.r[15] = target;
        self.reload(bus);
    }

    fn thumb_software_interrupt(&mut self, bus: &mut impl CpuBus) {
        let link = self.regs.r[15].wrapping_sub(2);
        self.prefetch(bus);
        self.enter(bus, Exception::SoftwareInterrupt, link);
    }

    fn thumb_undefined(&mut self, bus: &mut impl CpuBus) {
        let link = self.regs.r[15].wrapping_sub(2);
        self.prefetch(bus);
        self.enter(bus, Exception::Undefined, link);
    }
}
