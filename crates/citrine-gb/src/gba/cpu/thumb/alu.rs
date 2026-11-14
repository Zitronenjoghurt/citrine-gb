use super::super::arith::{Flags, add, sub};
use super::super::arm::multiply::multiply_carry;
use super::super::psr::Psr;
use super::super::shifter::Shift;
use super::super::{Core, CpuBus};
use super::low;

impl<const V5: bool> Core<V5> {
    fn set_flags(&mut self, result: u32, c: bool, v: bool) {
        let cpsr = Flags::of(result, c, v).apply(self.regs.cpsr());
        self.regs.set_flags(cpsr);
    }

    pub(in super::super) fn thumb_shift<B: CpuBus, const SHIFT: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let shift = Shift::from_bits(SHIFT);
        let amount = (op >> 6) & 0x1F;
        let cpsr = Psr(self.regs.cpsr());
        let (result, c) = shift.by_immediate(self.regs.r[low(op, 3)], amount, cpsr.c());
        self.regs.r[low(op, 0)] = result;
        self.set_flags(result, c, cpsr.v());
        self.prefetch_thumb(bus);
    }

    pub(in super::super) fn thumb_add_sub<B: CpuBus, const FORM: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let a = self.regs.r[low(op, 3)];
        let b = if FORM & 2 != 0 {
            (op >> 6) & 7
        } else {
            self.regs.r[low(op, 6)]
        };
        let (result, c, v) = if FORM & 1 != 0 {
            sub(a, b, true)
        } else {
            add(a, b, false)
        };
        self.regs.r[low(op, 0)] = result;
        self.set_flags(result, c, v);
        self.prefetch_thumb(bus);
    }

    pub(in super::super) fn thumb_immediate<B: CpuBus, const OPCODE: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let rd = low(op, 8);
        let a = self.regs.r[rd];
        let b = op & 0xFF;
        let cpsr = Psr(self.regs.cpsr());
        let (result, c, v) = match OPCODE {
            0 => (b, cpsr.c(), cpsr.v()),
            1 | 3 => sub(a, b, true),
            _ => add(a, b, false),
        };
        if OPCODE != 1 {
            self.regs.r[rd] = result;
        }
        self.set_flags(result, c, v);
        self.prefetch_thumb(bus);
    }

    pub(in super::super) fn thumb_alu<B: CpuBus, const HIGH: u32>(&mut self, bus: &mut B, op: u32) {
        let opcode = (HIGH << 2) | ((op >> 6) & 3);
        let rd = low(op, 0);
        let a = self.regs.r[rd];
        let b = self.regs.r[low(op, 3)];
        let cpsr = Psr(self.regs.cpsr());
        let (c, v) = (cpsr.c(), cpsr.v());
        let busy = matches!(opcode, 0x2 | 0x3 | 0x4 | 0x7 | 0xD);
        if busy {
            self.prefetch_thumb(bus);
        }
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
                let result = self.multiply(bus, b, a, 0, false, true);
                let carry = if V5 { c } else { multiply_carry(b, a, 0) };
                (result, carry, v)
            }
            0xE => (a & !b, c, v),
            _ => (!b, c, v),
        };
        if !matches!(opcode, 0x8 | 0xA | 0xB) {
            self.regs.r[rd] = result;
        }
        self.set_flags(result, c, v);
        if !busy {
            self.prefetch_thumb(bus);
        }
    }

    pub(in super::super) fn thumb_hi_register<B: CpuBus, const OPCODE: u32>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        let rd = (op & 7) as usize | ((op >> 4) & 8) as usize;
        let value = self.regs.r[((op >> 3) & 0xF) as usize];
        match OPCODE {
            0 | 2 => {
                let result = if OPCODE == 0 {
                    self.regs.r[rd].wrapping_add(value)
                } else {
                    value
                };
                if rd == 15 {
                    self.prefetch_thumb(bus);
                    self.write_pc_or_reg(bus, rd, result);
                } else {
                    self.regs.r[rd] = result;
                    self.prefetch_thumb(bus);
                }
            }
            1 => {
                let (result, c, v) = sub(self.regs.r[rd], value, true);
                self.set_flags(result, c, v);
                self.prefetch_thumb(bus);
            }
            _ if V5 => {
                let pc = self.regs.r[15];
                if !self.exchange_nearby(bus, pc, value) {
                    return self.thumb_bx_elsewhere(bus, op);
                }
                if op & (1 << 7) != 0 {
                    self.regs.r[14] = pc.wrapping_sub(2) | 1;
                }
            }
            _ => {
                self.prefetch_thumb_before_jump(bus);
                self.branch_exchange(bus, value);
            }
        }
    }

    #[inline(never)]
    fn thumb_bx_elsewhere(&mut self, bus: &mut impl CpuBus, op: u32) {
        let value = self.regs.r[((op >> 3) & 0xF) as usize];
        if op & (1 << 7) != 0 {
            self.regs.r[14] = self.regs.r[15].wrapping_sub(2) | 1;
        }
        self.prefetch_thumb_before_jump(bus);
        self.branch_exchange(bus, value);
    }

    pub(in super::super) fn thumb_load_address(&mut self, bus: &mut impl CpuBus, op: u32) {
        let base = if op & (1 << 11) != 0 {
            self.regs.r[13]
        } else {
            self.regs.r[15] & !2
        };
        self.regs.r[low(op, 8)] = base.wrapping_add((op & 0xFF) << 2);
        self.prefetch_thumb(bus);
    }

    pub(in super::super) fn thumb_adjust_sp(&mut self, bus: &mut impl CpuBus, op: u32) {
        let offset = (op & 0x7F) << 2;
        let sp = self.regs.r[13];
        self.regs.r[13] = if op & (1 << 7) != 0 {
            sp.wrapping_sub(offset)
        } else {
            sp.wrapping_add(offset)
        };
        self.prefetch_thumb(bus);
    }
}
