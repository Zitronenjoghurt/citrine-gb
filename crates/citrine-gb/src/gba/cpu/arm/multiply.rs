mod booth;

use super::super::psr::{FLAG_C, FLAG_N, FLAG_Z};
use super::super::{Cpu, CpuBus};
use booth::Flavor;

fn signed_steps(value: u32) -> u32 {
    let mut steps = 1;
    let mut mask = 0xFFFF_FF00u32;
    while value & mask != 0 && value & mask != mask && steps < 4 {
        mask <<= 8;
        steps += 1;
    }
    steps
}

fn unsigned_steps(value: u32) -> u32 {
    let mut steps = 1;
    let mut mask = 0xFFFF_FF00u32;
    while value & mask != 0 && steps < 4 {
        mask <<= 8;
        steps += 1;
    }
    steps
}

pub fn multiply_carry(rm: u32, rs: u32, acc: u32) -> bool {
    booth::carry(Flavor::Short, rm, rs, u64::from(acc))
}

impl Cpu {
    pub(in super::super) fn arm_multiply(&mut self, bus: &mut impl CpuBus, op: u32) {
        let accumulate = op & (1 << 21) != 0;
        let set_flags = op & (1 << 20) != 0;
        let rd = ((op >> 16) & 0xF) as usize;
        let rn = ((op >> 12) & 0xF) as usize;
        self.prefetch(bus);
        let rs = self.factor(((op >> 8) & 0xF) as usize, accumulate);
        let rm = self.factor((op & 0xF) as usize, accumulate);
        let acc = if accumulate { self.operand(rn) } else { 0 };
        let result = self.multiply(bus, rm, rs, acc, accumulate);
        if set_flags {
            let carry = multiply_carry(rm, rs, acc);
            self.set_nzc(result >> 31 != 0, result == 0, carry);
        }
        self.write_pc_or_reg(bus, rd, result);
    }

    pub(in super::super) fn multiply(
        &mut self,
        bus: &mut impl CpuBus,
        rm: u32,
        rs: u32,
        acc: u32,
        accumulate: bool,
    ) -> u32 {
        for _ in 0..signed_steps(rs) + u32::from(accumulate) {
            self.idle(bus);
        }
        rm.wrapping_mul(rs).wrapping_add(acc)
    }

    pub(in super::super) fn arm_multiply_long(&mut self, bus: &mut impl CpuBus, op: u32) {
        let signed = op & (1 << 22) != 0;
        let accumulate = op & (1 << 21) != 0;
        let set_flags = op & (1 << 20) != 0;
        let rd_hi = ((op >> 16) & 0xF) as usize;
        let rd_lo = ((op >> 12) & 0xF) as usize;
        self.prefetch(bus);
        let rs = self.factor(((op >> 8) & 0xF) as usize, accumulate);
        let rm = self.factor((op & 0xF) as usize, accumulate);
        let product = if signed {
            (i64::from(rm as i32) * i64::from(rs as i32)) as u64
        } else {
            u64::from(rm) * u64::from(rs)
        };
        let acc = if accumulate {
            (u64::from(self.operand(rd_hi)) << 32) | u64::from(self.operand(rd_lo))
        } else {
            0
        };
        let result = product.wrapping_add(acc);
        let steps = if signed {
            signed_steps(rs)
        } else {
            unsigned_steps(rs)
        };
        for _ in 0..steps + 1 + u32::from(accumulate) {
            self.idle(bus);
        }
        if set_flags {
            let flavor = if signed {
                Flavor::LongSigned
            } else {
                Flavor::LongUnsigned
            };
            let carry = booth::carry(flavor, rm, rs, acc);
            self.set_nzc(result >> 63 != 0, result == 0, carry);
        }
        self.regs.r[rd_lo] = result as u32;
        self.regs.r[rd_hi] = (result >> 32) as u32;
        if rd_lo == 15 || rd_hi == 15 {
            self.reload(bus);
        }
    }

    fn factor(&self, index: usize, accumulate: bool) -> u32 {
        if accumulate {
            self.regs.r[index]
        } else {
            self.operand(index)
        }
    }

    fn set_nzc(&mut self, n: bool, z: bool, c: bool) {
        let mut cpsr = self.regs.cpsr() & !(FLAG_N | FLAG_Z | FLAG_C);
        for (set, flag) in [(n, FLAG_N), (z, FLAG_Z), (c, FLAG_C)] {
            if set {
                cpsr |= flag;
            }
        }
        self.regs.set_cpsr(cpsr);
    }
}
