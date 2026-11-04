use super::super::arith::{Flags, add, sub};
use super::super::arm::multiply::multiply_carry;
use super::super::psr::Psr;
use super::super::shifter::Shift;
use super::super::{Cpu, CpuBus};
use super::low;

impl Cpu {
    fn set_flags(&mut self, result: u32, c: bool, v: bool) {
        let cpsr = Flags::of(result, c, v).apply(self.regs.cpsr());
        self.regs.set_cpsr(cpsr);
    }

    pub(in super::super) fn thumb_shift(&mut self, bus: &mut impl CpuBus, op: u32) {
        let shift = Shift::from_bits(op >> 11);
        let amount = (op >> 6) & 0x1F;
        let cpsr = Psr(self.regs.cpsr());
        let (result, c) = shift.by_immediate(self.regs.r[low(op, 3)], amount, cpsr.c());
        self.prefetch(bus);
        self.regs.r[low(op, 0)] = result;
        self.set_flags(result, c, cpsr.v());
    }

    pub(in super::super) fn thumb_add_sub(&mut self, bus: &mut impl CpuBus, op: u32) {
        let a = self.regs.r[low(op, 3)];
        let b = if op & (1 << 10) != 0 {
            (op >> 6) & 7
        } else {
            self.regs.r[low(op, 6)]
        };
        let (result, c, v) = if op & (1 << 9) != 0 {
            sub(a, b, true)
        } else {
            add(a, b, false)
        };
        self.prefetch(bus);
        self.regs.r[low(op, 0)] = result;
        self.set_flags(result, c, v);
    }

    pub(in super::super) fn thumb_immediate(&mut self, bus: &mut impl CpuBus, op: u32) {
        let rd = low(op, 8);
        let a = self.regs.r[rd];
        let b = op & 0xFF;
        let cpsr = Psr(self.regs.cpsr());
        self.prefetch(bus);
        let (result, c, v) = match (op >> 11) & 3 {
            0 => (b, cpsr.c(), cpsr.v()),
            1 | 3 => sub(a, b, true),
            _ => add(a, b, false),
        };
        if (op >> 11) & 3 != 1 {
            self.regs.r[rd] = result;
        }
        self.set_flags(result, c, v);
    }

    pub(in super::super) fn thumb_alu(&mut self, bus: &mut impl CpuBus, op: u32) {
        let opcode = (op >> 6) & 0xF;
        let rd = low(op, 0);
        let a = self.regs.r[rd];
        let b = self.regs.r[low(op, 3)];
        let cpsr = Psr(self.regs.cpsr());
        let (c, v) = (cpsr.c(), cpsr.v());
        self.prefetch(bus);
        let (result, c, v) = match opcode {
            0x0 | 0x8 => (a & b, c, v),
            0x1 => (a ^ b, c, v),
            0x2 | 0x3 | 0x4 | 0x7 => {
                let shift = match opcode {
                    0x2 => Shift::Lsl,
                    0x3 => Shift::Lsr,
                    0x4 => Shift::Asr,
                    _ => Shift::Ror,
                };
                self.idle(bus);
                let (result, c) = shift.by_register(a, b & 0xFF, c);
                (result, c, v)
            }
            0x5 => add(a, b, c),
            0x6 => sub(a, b, c),
            0x9 => sub(0, b, true),
            0xA => sub(a, b, true),
            0xB => add(a, b, false),
            0xC => (a | b, c, v),
            0xD => {
                let result = self.multiply(bus, b, a, 0, false);
                (result, multiply_carry(b, a, 0), v)
            }
            0xE => (a & !b, c, v),
            _ => (!b, c, v),
        };
        if !matches!(opcode, 0x8 | 0xA | 0xB) {
            self.regs.r[rd] = result;
        }
        self.set_flags(result, c, v);
    }

    pub(in super::super) fn thumb_hi_register(&mut self, bus: &mut impl CpuBus, op: u32) {
        let rd = (op & 7) as usize | ((op >> 4) & 8) as usize;
        let value = self.regs.r[((op >> 3) & 0xF) as usize];
        match (op >> 8) & 3 {
            0 => {
                let result = self.regs.r[rd].wrapping_add(value);
                self.prefetch(bus);
                self.write_pc_or_reg(bus, rd, result);
            }
            1 => {
                let (result, c, v) = sub(self.regs.r[rd], value, true);
                self.prefetch(bus);
                self.set_flags(result, c, v);
            }
            2 => {
                self.prefetch(bus);
                self.write_pc_or_reg(bus, rd, value);
            }
            _ => self.branch_exchange(bus, value),
        }
    }

    pub(in super::super) fn thumb_load_address(&mut self, bus: &mut impl CpuBus, op: u32) {
        let base = if op & (1 << 11) != 0 {
            self.regs.r[13]
        } else {
            self.regs.r[15] & !2
        };
        self.prefetch(bus);
        self.regs.r[low(op, 8)] = base.wrapping_add((op & 0xFF) << 2);
    }

    pub(in super::super) fn thumb_adjust_sp(&mut self, bus: &mut impl CpuBus, op: u32) {
        let offset = (op & 0x7F) << 2;
        let sp = self.regs.r[13];
        self.prefetch(bus);
        self.regs.r[13] = if op & (1 << 7) != 0 {
            sp.wrapping_sub(offset)
        } else {
            sp.wrapping_add(offset)
        };
    }
}
