use super::{Builder, NONSEQUENTIAL, SEQUENTIAL};
use crate::gba::cpu::psr::{FLAG_C, FLAG_N, FLAG_V, FLAG_Z};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{InstBuilder, Value, types};

impl Builder<'_, '_> {
    pub(super) fn carry_in(&mut self) -> Value {
        let cpsr = self.b.use_var(self.vars.cpsr);
        let shifted = self.b.ins().ushr_imm_u(cpsr, 29);
        self.b.ins().band_imm_u(shifted, 1)
    }

    pub(super) fn bit(&mut self, value: Value, at: i64) -> Value {
        let shifted = self.b.ins().ushr_imm_u(value, at);
        self.b.ins().band_imm_u(shifted, 1)
    }

    pub(super) fn shift_immediate(
        &mut self,
        value: Value,
        shift: u32,
        amount: u32,
    ) -> (Value, Value) {
        let amount = i64::from(amount);
        match (shift, amount) {
            (0, 0) => (value, self.carry_in()),
            (0, n) => {
                let result = self.b.ins().ishl_imm_u(value, n);
                (result, self.bit(value, 32 - n))
            }
            (1, 0) => (self.i32(0), self.bit(value, 31)),
            (1, n) => {
                let result = self.b.ins().ushr_imm_u(value, n);
                (result, self.bit(value, n - 1))
            }
            (2, 0) => {
                let result = self.b.ins().sshr_imm_u(value, 31);
                (result, self.bit(value, 31))
            }
            (2, n) => {
                let result = self.b.ins().sshr_imm_u(value, n);
                (result, self.bit(value, n - 1))
            }
            (_, 0) => {
                let carry = self.carry_in();
                let top = self.b.ins().ishl_imm_u(carry, 31);
                let low = self.b.ins().ushr_imm_u(value, 1);
                let result = self.b.ins().bor(top, low);
                (result, self.bit(value, 0))
            }
            (_, n) => {
                let result = self.b.ins().rotr_imm_u(value, n);
                (result, self.bit(result, 31))
            }
        }
    }

    pub(super) fn alu(&mut self, index: usize, op: u32, cost: Value) {
        let pc = self.pc_read(index);
        let opcode = (op >> 21) & 0xF;
        let set_flags = op & (1 << 20) != 0;
        let rn = (op >> 16) & 0xF;
        let rd = (op >> 12) & 0xF;
        let mut cycles = None;
        let mut access = SEQUENTIAL;
        let (first, second, shifter_carry) = if op & (1 << 25) != 0 {
            let rotate = ((op >> 8) & 0xF) * 2;
            let value = (op & 0xFF).rotate_right(rotate);
            let carry = if rotate == 0 {
                self.carry_in()
            } else {
                self.i32(value >> 31)
            };
            let first = self.reg(rn, pc);
            (first, self.i32(value), carry)
        } else if op & 0x10 != 0 {
            let rs = self.reg((op >> 8) & 0xF, pc);
            let amount = self.b.ins().band_imm_u(rs, 0xFF);
            let idle = self.i32(1);
            cycles = Some(self.idled(index, cost, idle));
            access = NONSEQUENTIAL;
            let value = self.reg(op & 0xF, pc.wrapping_add(4));
            let first = self.reg(rn, pc.wrapping_add(4));
            let kind = self.i32((op >> 5) & 3);
            let carry = self.carry_in();
            let packed = self.call(
                self.sigs.shift,
                self.helpers.shift,
                &[kind, value, amount, carry],
            );
            let second = self.b.ins().ireduce(types::I32, packed);
            let high = self.b.ins().ushr_imm_u(packed, 32);
            let carry = self.b.ins().ireduce(types::I32, high);
            (first, second, carry)
        } else {
            let value = self.reg(op & 0xF, pc);
            let (second, carry) = self.shift_immediate(value, (op >> 5) & 3, (op >> 7) & 0x1F);
            let first = self.reg(rn, pc);
            (first, second, carry)
        };
        let logical = |_: &mut Self, result: Value| (result, Some(shifter_carry), None::<Value>);
        let (result, carry, overflow) = match opcode {
            0x0 | 0x8 => {
                let result = self.b.ins().band(first, second);
                logical(self, result)
            }
            0x1 | 0x9 => {
                let result = self.b.ins().bxor(first, second);
                logical(self, result)
            }
            0xC => {
                let result = self.b.ins().bor(first, second);
                logical(self, result)
            }
            0xD => logical(self, second),
            0xE => {
                let result = self.b.ins().band_not(first, second);
                logical(self, result)
            }
            0xF => {
                let result = self.b.ins().bnot(second);
                logical(self, result)
            }
            0x2 | 0xA => self.add(first, second, true, None),
            0x3 => self.add(second, first, true, None),
            0x4 | 0xB => self.add(first, second, false, None),
            0x5 => {
                let carry = self.carry_in();
                self.add(first, second, false, Some(carry))
            }
            0x6 => {
                let carry = self.carry_in();
                self.add(first, second, true, Some(carry))
            }
            _ => {
                let carry = self.carry_in();
                self.add(second, first, true, Some(carry))
            }
        };
        if set_flags {
            self.set_flags(result, carry, overflow);
        }
        if !(0x8..=0xB).contains(&opcode) {
            self.set_reg(rd, result);
        }
        let cycles = match cycles {
            Some(cycles) => cycles,
            None => self.fetched(index, cost),
        };
        self.add_cycles(cycles);
        self.set_force(None);
        self.set_next_fetch(access);
    }

    pub(super) fn add(
        &mut self,
        first: Value,
        second: Value,
        subtract: bool,
        carry: Option<Value>,
    ) -> (Value, Option<Value>, Option<Value>) {
        let i64t = types::I64;
        let second = if subtract {
            self.b.ins().bnot(second)
        } else {
            second
        };
        let carry = match carry {
            Some(carry) => carry,
            None => self.i32(u32::from(subtract)),
        };
        let wide_first = self.b.ins().uextend(i64t, first);
        let wide_second = self.b.ins().uextend(i64t, second);
        let wide_carry = self.b.ins().uextend(i64t, carry);
        let sum = self.b.ins().iadd(wide_first, wide_second);
        let sum = self.b.ins().iadd(sum, wide_carry);
        let result = self.b.ins().ireduce(types::I32, sum);
        let high = self.b.ins().ushr_imm_u(sum, 32);
        let carry_out = self.b.ins().ireduce(types::I32, high);
        let same = self.b.ins().bxor(first, second);
        let same = self.b.ins().bnot(same);
        let changed = self.b.ins().bxor(first, result);
        let overflow = self.b.ins().band(same, changed);
        let overflow = self.b.ins().ushr_imm_u(overflow, 31);
        (result, Some(carry_out), Some(overflow))
    }

    pub(super) fn set_flags(
        &mut self,
        result: Value,
        carry: Option<Value>,
        overflow: Option<Value>,
    ) {
        let cpsr = self.b.use_var(self.vars.cpsr);
        let keep = if overflow.is_some() {
            !(FLAG_N | FLAG_Z | FLAG_C | FLAG_V)
        } else {
            !(FLAG_N | FLAG_Z | FLAG_C)
        };
        let mut bits = self.b.ins().band_imm_u(cpsr, i64::from(keep));
        let negative = self.b.ins().band_imm_u(result, i64::from(FLAG_N));
        bits = self.b.ins().bor(bits, negative);
        let zero = self.b.ins().icmp_imm_u(IntCC::Equal, result, 0);
        let zero = self.b.ins().uextend(types::I32, zero);
        let zero = self.b.ins().ishl_imm_u(zero, 30);
        bits = self.b.ins().bor(bits, zero);
        if let Some(carry) = carry {
            let carry = self.b.ins().ishl_imm_u(carry, 29);
            bits = self.b.ins().bor(bits, carry);
        }
        if let Some(overflow) = overflow {
            let overflow = self.b.ins().ishl_imm_u(overflow, 28);
            bits = self.b.ins().bor(bits, overflow);
        }
        self.b.def_var(self.vars.cpsr, bits);
    }
}
