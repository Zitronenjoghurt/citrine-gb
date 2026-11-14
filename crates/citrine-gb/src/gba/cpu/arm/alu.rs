use super::super::arith::{Flags, add, sub};
use super::super::psr::Psr;
use super::super::shifter::{Shift, rotated_immediate};
use super::super::{Core, CpuBus};

pub(super) const IMMEDIATE: u8 = 0;
pub(super) const SHIFT_BY_IMMEDIATE: u8 = 1;
pub(super) const SHIFT_BY_REGISTER: u8 = 2;

pub(super) const fn operand_form(op: u32) -> u8 {
    if op & (1 << 25) != 0 {
        IMMEDIATE
    } else if op & 0x10 != 0 {
        SHIFT_BY_REGISTER
    } else {
        SHIFT_BY_IMMEDIATE
    }
}

impl<const V5: bool> Core<V5> {
    pub(in super::super) fn arm_data_processing(&mut self, bus: &mut impl CpuBus, op: u32) {
        let opcode = (op >> 21) & 0xF;
        let set_flags = op & (1 << 20) != 0;
        let into_pc = op & 0xF000 == 0xF000;
        self.data_processing(bus, op, opcode, set_flags, operand_form(op), into_pc);
    }

    pub(super) fn arm_alu<B: CpuBus, const OPCODE: u32, const FLAGS: bool, const FORM: u8>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        if op & 0xF000 == 0xF000 {
            return self.arm_alu_into_pc::<B, OPCODE, FLAGS, FORM>(bus, op);
        }
        self.data_processing(bus, op, OPCODE, FLAGS, FORM, false);
    }

    #[inline(never)]
    fn arm_alu_into_pc<B: CpuBus, const OPCODE: u32, const FLAGS: bool, const FORM: u8>(
        &mut self,
        bus: &mut B,
        op: u32,
    ) {
        self.data_processing(bus, op, OPCODE, FLAGS, FORM, true);
    }

    #[inline(always)]
    fn data_processing(
        &mut self,
        bus: &mut impl CpuBus,
        op: u32,
        opcode: u32,
        set_flags: bool,
        form: u8,
        into_pc: bool,
    ) {
        let rn = ((op >> 16) & 0xF) as usize;
        let rd = ((op >> 12) & 0xF) as usize;
        let cpsr = Psr(self.regs.cpsr());
        let carry = cpsr.c();
        let late = form != SHIFT_BY_REGISTER && !into_pc;

        let (a, b, shifter_carry) = if form == IMMEDIATE {
            let (b, c) = rotated_immediate(op, carry);
            let a = self.operand(rn);
            if !late {
                self.prefetch_arm(bus);
            }
            (a, b, c)
        } else {
            let shift = Shift::from_bits(op >> 5);
            let rm = (op & 0xF) as usize;
            if form == SHIFT_BY_REGISTER {
                let amount = self.operand(((op >> 8) & 0xF) as usize) & 0xFF;
                self.prefetch_arm(bus);
                self.idle(bus);
                let (b, c) = shift.by_register(self.regs.r[rm], amount, carry);
                (self.regs.r[rn], b, c)
            } else {
                let amount = (op >> 7) & 0x1F;
                let (b, c) = shift.by_immediate(self.operand(rm), amount, carry);
                let a = self.operand(rn);
                if !late {
                    self.prefetch_arm(bus);
                }
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
            if into_pc && self.regs.spsr().is_some() {
                self.restore_cpsr();
            } else {
                let cpsr = Flags::of(result, c, v).apply(self.regs.cpsr());
                self.regs.set_flags(cpsr);
            }
        }
        if writes && into_pc {
            self.write_pc_or_reg(bus, rd, result);
        } else if writes {
            self.regs.r[rd] = result;
        }
        if late {
            self.prefetch_arm(bus);
        }
    }
}
