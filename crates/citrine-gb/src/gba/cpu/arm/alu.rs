use super::super::arith::{Flags, add, sub};
use super::super::psr::Psr;
use super::super::shifter::{Shift, rotated_immediate};
use super::super::{Cpu, CpuBus};

impl Cpu {
    pub(in super::super) fn arm_data_processing(&mut self, bus: &mut impl CpuBus, op: u32) {
        let opcode = (op >> 21) & 0xF;
        let set_flags = op & (1 << 20) != 0;
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let cpsr = Psr(self.regs.cpsr());
        let carry = cpsr.c();

        let (a, b, shifter_carry) = if op & (1 << 25) != 0 {
            let (b, c) = rotated_immediate(op, carry);
            let a = self.operand(rn);
            self.prefetch(bus);
            (a, b, c)
        } else {
            let shift = Shift::from_bits(op >> 5);
            let rm = (op & 0xF) as usize;
            if op & 0x10 != 0 {
                let amount = self.operand(((op >> 8) & 0xF) as usize) & 0xFF;
                self.prefetch(bus);
                self.idle(bus);
                let (b, c) = shift.by_register(self.regs.r[rm], amount, carry);
                (self.regs.r[rn], b, c)
            } else {
                let amount = (op >> 7) & 0x1F;
                let (b, c) = shift.by_immediate(self.operand(rm), amount, carry);
                let a = self.operand(rn);
                self.prefetch(bus);
                (a, b, c)
            }
        };

        let v = cpsr.v();
        let (result, c, v) = match opcode {
            0x0 | 0x8 => (a & b, shifter_carry, v),
            0x1 | 0x9 => (a ^ b, shifter_carry, v),
            0x2 | 0xA => sub(a, b, true),
            0x3 => sub(b, a, true),
            0x4 | 0xB => add(a, b, false),
            0x5 => add(a, b, carry),
            0x6 => sub(a, b, carry),
            0x7 => sub(b, a, carry),
            0xC => (a | b, shifter_carry, v),
            0xD => (b, shifter_carry, v),
            0xE => (a & !b, shifter_carry, v),
            _ => (!b, shifter_carry, v),
        };
        let writes = !(0x8..=0xB).contains(&opcode);

        if set_flags {
            if rd == 15 && self.regs.spsr().is_some() {
                self.restore_cpsr();
            } else {
                let cpsr = Flags::of(result, c, v).apply(self.regs.cpsr());
                self.regs.set_cpsr(cpsr);
            }
        }
        if writes {
            self.write_pc_or_reg(bus, rd, result);
        }
    }
}
