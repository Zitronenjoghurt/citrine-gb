use super::super::exception::Exception;
use super::super::psr::{FLAG_Q, FLAG_T};
use super::super::{CoprocessorRegister, Core, CpuBus};

const PSR_FLAGS_NZCV: u32 = 0xF000_0000;
const COPROCESSOR_WRITE_CYCLES: u32 = 2;
const COPROCESSOR_READ_CYCLES: u32 = 3;

fn saturate(value: i64) -> (u32, bool) {
    if value > i64::from(i32::MAX) {
        (i32::MAX as u32, true)
    } else if value < i64::from(i32::MIN) {
        (i32::MIN as u32, true)
    } else {
        (value as u32, false)
    }
}

fn half(value: u32, top: bool) -> i32 {
    if top {
        i32::from((value >> 16) as i16)
    } else {
        i32::from(value as i16)
    }
}

fn reg(op: u32, at: u32) -> usize {
    ((op >> at) & 0xF) as usize
}

impl<const V5: bool> Core<V5> {
    pub(in super::super) fn execute_arm_unconditional(&mut self, bus: &mut impl CpuBus, op: u32) {
        if op & 0x0E00_0000 == 0x0A00_0000 {
            self.arm_branch_exchange_immediate(bus, op);
        } else if op & 0x0D70_F000 == 0x0550_F000 {
            self.prefetch_arm(bus);
        } else {
            self.arm_undefined(bus);
        }
    }

    pub(super) fn arm_v5_misc(&mut self, bus: &mut impl CpuBus, op: u32) {
        match ((op >> 4) & 0xF, (op >> 21) & 3) {
            (0b0001, 0b11) => self.arm_count_leading_zeros(bus, op),
            (0b0011, 0b01) => self.arm_branch_link_exchange(bus, op),
            (0b0101, kind) => self.arm_saturating(bus, op, kind),
            (0b0111, 0b01) => self.arm_breakpoint(bus),
            (bits, kind) if bits & 0b1001 == 0b1000 => self.arm_halfword_multiply(bus, op, kind),
            _ => self.arm_undefined(bus),
        }
    }

    fn arm_branch_exchange_immediate(&mut self, bus: &mut impl CpuBus, op: u32) {
        let offset = ((((op << 8) as i32) >> 6) as u32) | ((op >> 23) & 2);
        let pc = self.regs.r[15];
        self.prefetch_arm_before_jump(bus);
        self.regs.r[14] = pc.wrapping_sub(4);
        let cpsr = self.regs.cpsr();
        self.regs.set_cpsr(cpsr | FLAG_T);
        self.regs.r[15] = pc.wrapping_add(offset);
        self.jump_thumb(bus);
    }

    fn arm_branch_link_exchange(&mut self, bus: &mut impl CpuBus, op: u32) {
        let target = self.operand(reg(op, 0));
        self.regs.r[14] = self.regs.r[15].wrapping_sub(4);
        self.prefetch_arm_before_jump(bus);
        self.branch_exchange(bus, target);
    }

    fn arm_count_leading_zeros(&mut self, bus: &mut impl CpuBus, op: u32) {
        let value = self.operand(reg(op, 0)).leading_zeros();
        self.prefetch_arm(bus);
        self.write_pc_or_reg(bus, reg(op, 12), value);
    }

    fn arm_saturating(&mut self, bus: &mut impl CpuBus, op: u32, kind: u32) {
        let m = i64::from(self.operand(reg(op, 0)) as i32);
        let mut n = i64::from(self.operand(reg(op, 16)) as i32);
        let mut saturated = false;
        if kind & 2 != 0 {
            let (doubled, clipped) = saturate(n * 2);
            n = i64::from(doubled as i32);
            saturated = clipped;
        }
        let (result, clipped) = saturate(if kind & 1 == 0 { m + n } else { m - n });
        self.prefetch_arm(bus);
        if saturated || clipped {
            self.set_sticky_overflow();
        }
        self.write_pc_or_reg(bus, reg(op, 12), result);
    }

    fn arm_halfword_multiply(&mut self, bus: &mut impl CpuBus, op: u32, kind: u32) {
        let rd = reg(op, 16);
        let rn = reg(op, 12);
        let m = self.operand(reg(op, 0));
        let s = self.operand(reg(op, 8));
        let x = op & (1 << 5) != 0;
        let y = op & (1 << 6) != 0;
        self.prefetch_arm(bus);
        match kind {
            0 => {
                let product = half(m, x) * half(s, y);
                let result = self.accumulate_sticky(product, self.regs.r[rn]);
                self.write_pc_or_reg(bus, rd, result);
            }
            1 => {
                let product = ((i64::from(m as i32) * i64::from(half(s, y))) >> 16) as i32;
                let result = if x {
                    product as u32
                } else {
                    self.accumulate_sticky(product, self.regs.r[rn])
                };
                self.write_pc_or_reg(bus, rd, result);
            }
            2 => {
                let product = i64::from(half(m, x) * half(s, y));
                let acc = ((u64::from(self.regs.r[rd]) << 32) | u64::from(self.regs.r[rn])) as i64;
                let result = acc.wrapping_add(product) as u64;
                self.idle(bus);
                self.regs.r[rn] = result as u32;
                self.regs.r[rd] = (result >> 32) as u32;
                if rd == 15 || rn == 15 {
                    self.reload(bus);
                }
            }
            _ => {
                let product = half(m, x) * half(s, y);
                self.write_pc_or_reg(bus, rd, product as u32);
            }
        }
    }

    fn accumulate_sticky(&mut self, product: i32, acc: u32) -> u32 {
        let (result, overflow) = product.overflowing_add(acc as i32);
        if overflow {
            self.set_sticky_overflow();
        }
        result as u32
    }

    fn set_sticky_overflow(&mut self) {
        let cpsr = self.regs.cpsr();
        self.regs.set_flags(cpsr | FLAG_Q);
    }

    fn arm_breakpoint(&mut self, bus: &mut impl CpuBus) {
        let link = self.regs.r[15].wrapping_sub(4);
        self.prefetch_arm(bus);
        self.enter(bus, Exception::PrefetchAbort, link);
    }

    pub(super) fn arm_coprocessor_transfer(&mut self, bus: &mut impl CpuBus, op: u32) {
        let register = CoprocessorRegister {
            coprocessor: (op >> 8) & 0xF,
            opcode1: (op >> 21) & 7,
            crn: (op >> 16) & 0xF,
            crm: op & 0xF,
            opcode2: (op >> 5) & 7,
        };
        let rd = reg(op, 12);
        let link = self.regs.r[15].wrapping_sub(4);
        let value = self.regs.r[rd];
        self.prefetch_arm(bus);
        if op & (1 << 20) == 0 {
            if bus.coprocessor_write(register, value) {
                self.idle_for(bus, COPROCESSOR_WRITE_CYCLES);
            } else {
                self.enter(bus, Exception::Undefined, link);
            }
            return;
        }
        let read = bus.coprocessor_read(register);
        if read.is_some() {
            self.idle_for(bus, COPROCESSOR_READ_CYCLES);
        }
        match read {
            None => {
                self.enter(bus, Exception::Undefined, link);
            }
            Some(value) if rd == 15 => {
                let cpsr = self.regs.cpsr();
                self.regs
                    .set_cpsr((cpsr & !PSR_FLAGS_NZCV) | (value & PSR_FLAGS_NZCV));
            }
            Some(value) => self.regs.r[rd] = value,
        }
    }

    pub(crate) fn load_pc_exchange(&mut self, bus: &mut impl CpuBus, value: u32) {
        let cpsr = self.regs.cpsr();
        if value & 1 != 0 {
            self.regs.set_cpsr(cpsr | FLAG_T);
        } else {
            self.regs.set_cpsr(cpsr & !FLAG_T);
        }
        self.regs.r[15] = value;
        self.reload(bus);
    }
}
