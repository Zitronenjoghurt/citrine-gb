use super::super::scan::Multiply;
use super::{Builder, NONSEQUENTIAL};
use crate::gba::cpu::psr::{FLAG_N, FLAG_Z};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, Value, types};

const IDLES: u32 = 1;
const FLAG_IDLES: u32 = 3;

impl Builder<'_, '_> {
    pub(super) fn multiply(&mut self, index: usize, multiply: Multiply, cost: Value) {
        let pc = self.pc_read(index);
        let rm = self.reg(multiply.rm, pc);
        let rs = self.reg(multiply.rs, pc);
        match multiply.long {
            None => {
                let mut result = self.b.ins().imul(rm, rs);
                if let Some(rn) = multiply.accumulate {
                    let acc = self.reg(rn, pc);
                    result = self.b.ins().iadd(result, acc);
                }
                if multiply.set_flags {
                    let carry = self.carry_in();
                    self.set_flags(result, Some(carry), None);
                }
                self.set_reg(multiply.rd, result);
            }
            Some(long) => {
                let i64t = types::I64;
                let (rm, rs) = if long.signed {
                    (
                        self.b.ins().sextend(i64t, rm),
                        self.b.ins().sextend(i64t, rs),
                    )
                } else {
                    (
                        self.b.ins().uextend(i64t, rm),
                        self.b.ins().uextend(i64t, rs),
                    )
                };
                let mut result = self.b.ins().imul(rm, rs);
                if long.accumulate {
                    let low = self.reg(long.low, pc);
                    let high = self.reg(multiply.rd, pc);
                    let low = self.b.ins().uextend(i64t, low);
                    let high = self.b.ins().uextend(i64t, high);
                    let high = self.b.ins().ishl_imm_u(high, 32);
                    let acc = self.b.ins().bor(high, low);
                    result = self.b.ins().iadd(result, acc);
                }
                let low = self.b.ins().ireduce(types::I32, result);
                let high = self.b.ins().ushr_imm_u(result, 32);
                let high = self.b.ins().ireduce(types::I32, high);
                if multiply.set_flags {
                    self.set_long_flags(result, high);
                }
                self.set_reg(long.low, low);
                self.set_reg(multiply.rd, high);
            }
        }
        let idles = if self.v5 {
            self.i32(if multiply.set_flags {
                FLAG_IDLES
            } else {
                IDLES
            })
        } else {
            self.v4_idles(multiply, rs)
        };
        let cycles = self.idled(index, cost, idles);
        self.add_cycles(cycles);
        self.set_force(None);
        self.set_next_fetch(NONSEQUENTIAL);
    }

    fn v4_idles(&mut self, multiply: Multiply, rs: Value) -> Value {
        let unsigned = multiply.long.is_some_and(|long| !long.signed);
        let value = if unsigned {
            rs
        } else {
            let sign = self.b.ins().sshr_imm_u(rs, 31);
            self.b.ins().bxor(rs, sign)
        };
        let mut steps = self.i32(1);
        for limit in [0x100, 0x1_0000, 0x100_0000] {
            let above = self
                .b
                .ins()
                .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, value, limit);
            let above = self.b.ins().uextend(types::I32, above);
            steps = self.b.ins().iadd(steps, above);
        }
        let extra = match multiply.long {
            Some(long) => 1 + u32::from(long.accumulate),
            None => u32::from(multiply.accumulate.is_some()),
        };
        self.b.ins().iadd_imm_u(steps, i64::from(extra))
    }

    fn set_long_flags(&mut self, result: Value, high: Value) {
        let cpsr = self.b.use_var(self.vars.cpsr);
        let bits = self.b.ins().band_imm_u(cpsr, i64::from(!(FLAG_N | FLAG_Z)));
        let negative = self.b.ins().band_imm_u(high, i64::from(FLAG_N));
        let bits = self.b.ins().bor(bits, negative);
        let zero = self.b.ins().icmp_imm_u(IntCC::Equal, result, 0);
        let zero = self.b.ins().uextend(types::I32, zero);
        let zero = self.b.ins().ishl_imm_u(zero, 30);
        let bits = self.b.ins().bor(bits, zero);
        self.b.def_var(self.vars.cpsr, bits);
    }
}
