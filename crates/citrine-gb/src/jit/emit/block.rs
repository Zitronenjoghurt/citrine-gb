use super::super::helpers::LAST;
use super::super::scan::Block;
use super::{Builder, NONSEQUENTIAL, PC, SEQUENTIAL};
use cranelift_codegen::ir::{InstBuilder, Value, types};

impl Builder<'_, '_> {
    pub(super) fn block(&mut self, index: usize, block: Block, cost: Value) {
        let pc = self.pc_read(index);
        let base = self.reg(block.rn, pc);
        let bytes = self.i32(u32::from(block.list).count_ones() * 4);
        let new_base = if block.up {
            self.b.ins().iadd(base, bytes)
        } else {
            self.b.ins().isub(base, bytes)
        };
        let start = if block.up { base } else { new_base };
        let start = if block.pre == block.up {
            self.b.ins().iadd_imm_u(start, 4)
        } else {
            start
        };
        let (core, bus) = (self.core, self.bus);
        let registers: Vec<u32> = block.registers().collect();
        let count = registers.len();
        let cost = self.store_fetch(index, cost);
        let sequential = move |at: usize| {
            let sequential = if at == 0 { NONSEQUENTIAL } else { SEQUENTIAL };
            if at + 1 == count {
                sequential | i64::from(LAST)
            } else {
                sequential
            }
        };
        let mut target = None;
        let (data, force) = if block.load {
            let (values, data, force) = self.block_memory(start, count, None, |s, at, addr| {
                let sequential = s.b.ins().iconst(types::I32, sequential(at));
                s.call(
                    s.sigs.load_word,
                    s.helpers.load_word,
                    &[core, bus, addr, sequential],
                )
            });
            for (&reg, value) in registers.iter().zip(values) {
                if reg == PC {
                    target = Some(value);
                } else {
                    self.set_reg(reg, value);
                }
            }
            (data, force)
        } else {
            let values: Vec<Value> = registers
                .iter()
                .enumerate()
                .map(|(at, &reg)| {
                    if !self.v5 && block.writeback && reg == block.rn && at > 0 {
                        new_base
                    } else {
                        self.reg(reg, pc)
                    }
                })
                .collect();
            let (_, data, force) = self.block_memory(start, count, Some(&values), |s, at, addr| {
                let sequential = s.b.ins().iconst(types::I32, sequential(at));
                s.call(
                    s.sigs.store_word,
                    s.helpers.store_word,
                    &[core, bus, addr, values[at], sequential],
                )
            });
            (data, force)
        };
        let keeps = if self.v5 {
            block.keeps_loaded_base()
        } else {
            block.load && block.list & (1 << block.rn) != 0
        };
        if block.writeback && !keeps {
            self.set_reg(block.rn, new_base);
        }
        if let Some(target) = target {
            self.load_pc(index, target, data);
            return;
        }
        let cycles = self.settled(cost, data);
        self.add_cycles(cycles);
        self.set_force(Some(force));
        self.set_next_fetch(NONSEQUENTIAL);
    }

    fn load_pc(&mut self, index: usize, target: Value, data: Value) {
        let cpsr = self.b.use_var(self.vars.cpsr);
        self.store32(cpsr, self.layout.cpsr);
        let (core, bus) = (self.core, self.bus);
        let packed = self.call(
            self.sigs.load_pc,
            self.helpers.load_pc,
            &[core, bus, target],
        );
        let cpsr = self.load32(self.layout.cpsr);
        self.b.def_var(self.vars.cpsr, cpsr);
        let reload = self.b.ins().ireduce(types::I32, packed);
        let high = self.b.ins().ushr_imm_u(packed, 32);
        let refetched = self.b.ins().ireduce(types::I32, high);
        let overlap = self.overlap(refetched, data);
        let cycles = self.b.ins().iadd(reload, overlap);
        self.leave(index, cycles);
    }
}
