use super::super::fast::{Fast, Spec, WINDOWS, Window};
use super::memory::guest;
use super::{Builder, SEQUENTIAL};
use crate::gba::cpu::psr::{FLAG_I, FLAG_T};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, MemFlagsData, Value, types};
use std::mem::offset_of;

#[derive(Clone, Copy)]
struct Exits {
    fill: Block,
    slow: Block,
    done: Block,
}

impl Builder<'_, '_> {
    pub(super) fn reload(&mut self, target: Value) -> Value {
        let Some(exits) = self.refetch_exits() else {
            return self.branch_helper(target);
        };
        let thumb = self.plan.thumb;
        let target = if thumb {
            self.b.ins().band_imm_u(target, i64::from(!1u32))
        } else {
            target
        };
        self.refetch(target, thumb, exits);
        self.b.switch_to_block(exits.slow);
        let cycles = self.branch_helper(target);
        self.b.ins().jump(exits.done, &[cycles.into()]);
        self.fill(exits, None)
    }

    pub(super) fn reload_exchange(&mut self, target: Value) -> Value {
        let Some(exits) = self.refetch_exits() else {
            return self.exchange_helper(target);
        };
        let odd = self.b.ins().band_imm_u(target, 1);
        let cpsr = self.b.use_var(self.vars.cpsr);
        let cleared = self.b.ins().band_imm_u(cpsr, i64::from(!FLAG_T));
        let bit = self
            .b
            .ins()
            .ishl_imm_u(odd, i64::from(FLAG_T.trailing_zeros()));
        let switched = self.b.ins().bor(cleared, bit);
        let (thumb, arm) = (self.b.create_block(), self.b.create_block());
        self.b.ins().brif(odd, thumb, &[], arm, &[]);
        self.b.switch_to_block(thumb);
        let half = self.b.ins().band_imm_u(target, i64::from(!1u32));
        self.refetch(half, true, exits);
        self.b.switch_to_block(arm);
        self.refetch(target, false, exits);
        self.b.switch_to_block(exits.slow);
        let cycles = self.exchange_helper(target);
        self.b.ins().jump(exits.done, &[cycles.into()]);
        self.fill(exits, Some(switched))
    }

    fn branch_helper(&mut self, target: Value) -> Value {
        let (core, bus) = (self.core, self.bus);
        self.call(self.sigs.branch, self.helpers.branch, &[core, bus, target])
    }

    fn exchange_helper(&mut self, target: Value) -> Value {
        let (core, bus) = (self.core, self.bus);
        let cpsr = self.b.use_var(self.vars.cpsr);
        self.store32(cpsr, self.layout.cpsr);
        let cycles = self.call(
            self.sigs.branch,
            self.helpers.exchange,
            &[core, bus, target],
        );
        let cpsr = self.load32(self.layout.cpsr);
        self.b.def_var(self.vars.cpsr, cpsr);
        cycles
    }

    fn code_windows(&self) -> Vec<usize> {
        if !self.v5 {
            return Vec::new();
        }
        (0..WINDOWS).filter(|&i| self.specs[i].code).collect()
    }

    fn refetch_exits(&mut self) -> Option<Exits> {
        if self.code_windows().is_empty() {
            return None;
        }
        let done = self.b.create_block();
        self.b.append_block_param(done, types::I32);
        let fill = self.b.create_block();
        for _ in 0..4 {
            self.b.append_block_param(fill, types::I32);
        }
        let slow = self.b.create_block();
        self.b.set_cold_block(slow);
        Some(Exits { fill, slow, done })
    }

    fn refetch(&mut self, target: Value, thumb: bool, exits: Exits) {
        let step: u32 = if thumb { 2 } else { 4 };
        if !thumb {
            let odd = self.b.ins().band_imm_u(target, 3);
            let aligned = self.b.create_block();
            self.b.ins().brif(odd, exits.slow, &[], aligned, &[]);
            self.b.switch_to_block(aligned);
        }
        let code = self.code_windows();
        for (n, &index) in code.iter().enumerate() {
            let last = n + 1 == code.len();
            let next = if last {
                exits.slow
            } else {
                self.b.create_block()
            };
            let hit = self.b.create_block();
            let base = self.window_field(index, offset_of!(Window, base), types::I32);
            let size = self.window_field(index, offset_of!(Window, size), types::I64);
            let offset = self.b.ins().isub(target, base);
            let wide = self.b.ins().uextend(types::I64, offset);
            let end = self.b.ins().iadd_imm_u(wide, i64::from(step));
            let inside = self.b.ins().icmp(IntCC::UnsignedLessThan, end, size);
            self.b.ins().brif(inside, hit, &[], next, &[]);
            self.b.switch_to_block(hit);
            let spec = self.specs[index];
            let after = self.b.ins().iadd_imm_u(offset, i64::from(step));
            let first = self.fetch_word(index, spec, offset, step);
            let second = self.fetch_word(index, spec, after, step);
            let cycles = self.refetch_cycles(spec, target, step);
            let r15 = self.b.ins().iadd_imm_u(target, i64::from(step * 2));
            if !self.timed {
                self.store32(target, self.layout.landed);
            }
            let args = [cycles, r15, first, second].map(Into::into);
            self.b.ins().jump(exits.fill, &args);
            if !last {
                self.b.switch_to_block(next);
            }
        }
    }

    fn fill(&mut self, exits: Exits, cpsr: Option<Value>) -> Value {
        self.b.switch_to_block(exits.fill);
        let [cycles, r15, first, second] = [0, 1, 2, 3].map(|i| self.b.block_params(exits.fill)[i]);
        self.store32(r15, self.layout.reg(15));
        self.store32(first, self.layout.pipeline);
        self.store32(second, self.layout.pipeline + 4);
        let sequential = self.b.ins().iconst(types::I8, SEQUENTIAL);
        self.store8(sequential, self.layout.next_fetch);
        if let Some(cpsr) = cpsr {
            self.b.def_var(self.vars.cpsr, cpsr);
        }
        let cpsr = self.b.use_var(self.vars.cpsr);
        let masked = self.b.ins().band_imm_u(cpsr, i64::from(FLAG_I));
        let masked = self.b.ins().icmp_imm_u(IntCC::NotEqual, masked, 0);
        self.store8(masked, self.layout.irq_masked);
        self.b.ins().jump(exits.done, &[cycles.into()]);
        self.b.switch_to_block(exits.done);
        self.b.block_params(exits.done)[0]
    }

    fn fetch_word(&mut self, index: usize, spec: Spec, offset: Value, step: u32) -> Value {
        let mask = (spec.len - 1) & !(step - 1);
        let offset = self.b.ins().band_imm_u(offset, i64::from(mask));
        let bytes = self.window_field(index, offset_of!(Window, bytes), self.pointer);
        let wide = self.b.ins().uextend(self.pointer, offset);
        let host = self.b.ins().iadd(bytes, wide);
        if step == 2 {
            self.b.ins().uload16(types::I32, guest(), host, 0)
        } else {
            self.b.ins().load(types::I32, guest(), host, 0)
        }
    }

    fn refetch_cycles(&mut self, spec: Spec, target: Value, step: u32) -> Value {
        let after = self.b.ins().iadd_imm_u(target, i64::from(step));
        let word = spec.word[0];
        let (first, second) = match spec.cache {
            None => (self.i32(word), self.i32(word)),
            Some(cache) => {
                let uncached = self.i32(word);
                let lookup = self.i32(cache.cycles[0]);
                let inside = self.i32(cache.cycles[1]);
                let pages = self.b.ins().load(
                    self.pointer,
                    MemFlagsData::trusted(),
                    self.fast,
                    offset_of!(Fast, cache) as i32,
                );
                let [hit, again] = [target, after].map(|addr| {
                    let page = self.b.ins().ushr_imm_u(addr, i64::from(cache.shift));
                    let page = self.b.ins().uextend(self.pointer, page);
                    let flags = self.b.ins().iadd(pages, page);
                    let flags = self.b.ins().uload8(types::I32, guest(), flags, 0);
                    self.b.ins().band_imm_u(flags, i64::from(cache.code))
                });
                let first = self.b.ins().select(hit, lookup, uncached);
                let line = self.b.ins().band_imm_u(after, i64::from(cache.line));
                let start = self.b.ins().icmp_imm_u(IntCC::Equal, line, 0);
                let cached = self.b.ins().select(start, lookup, inside);
                let second = self.b.ins().select(again, cached, uncached);
                (first, second)
            }
        };
        let second = if step == 2 {
            let half = self.b.ins().band_imm_u(after, 2);
            let zero = self.i32(0);
            self.b.ins().select(half, zero, second)
        } else {
            second
        };
        self.b.ins().iadd(first, second)
    }
}
