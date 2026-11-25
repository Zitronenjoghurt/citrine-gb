use super::super::QuickFetch;
use super::super::helpers::{IDLES_SHIFT, RUN_DONE_SHIFT, RUN_LAST_SHIFT, THUMB};
use super::super::scan::Op;
use super::{Builder, DATA_OVERLAP, Fetch, Merge};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, InstructionData, Opcode, Value, types};

struct FlatRun {
    quick: QuickFetch,
    count: u32,
    last: u32,
    latch: u32,
}

impl Builder<'_, '_> {
    pub(super) fn load_next_fetch(&mut self) {
        let value =
            self.b
                .ins()
                .uload8(types::I32, Self::flags(), self.core, self.layout.next_fetch);
        self.b.def_var(self.vars.next_fetch, value);
        self.fetch = Fetch::Runtime;
    }

    pub(super) fn set_next_fetch(&mut self, access: i64) {
        let value = self.b.ins().iconst(types::I32, access);
        self.b.def_var(self.vars.next_fetch, value);
        self.fetch = Fetch::Known(access);
    }

    pub(super) fn fetch_cost(&mut self, index: usize) -> Value {
        let [_, sequential] = self.plan.fetch[index];
        self.i32(sequential)
    }

    pub(super) fn fetched(&mut self, index: usize, cost: Value) -> Value {
        if !self.timed {
            return cost;
        }
        match self.merge.get(index).copied().unwrap_or(Merge::Alone) {
            Merge::Alone => self.timed_fetch(index, None),
            Merge::First { end } => {
                let access = self.fetch_access();
                self.fetch_run_inline(index, end, access);
                self.merged = true;
                self.i32(0)
            }
            Merge::Member { .. } => {
                self.merged = true;
                self.i32(0)
            }
        }
    }

    pub(super) fn fetch_access(&mut self) -> Value {
        let thumb = if self.plan.thumb { THUMB } else { 0 };
        match self.fetch {
            Fetch::Known(access) => self.i32(access as u32 | thumb),
            Fetch::Runtime => {
                let next_fetch = self.b.use_var(self.vars.next_fetch);
                self.b.ins().bor_imm_u(next_fetch, i64::from(thumb))
            }
        }
    }

    fn fetch_run_inline(&mut self, index: usize, end: usize, access: Value) {
        let Some(flat) = self.iwram_run(index, end) else {
            self.fetch_run(index, end, access);
            return;
        };
        let slow = self.b.create_block();
        let done = self.b.create_block();
        self.b.set_cold_block(slow);
        let total = self.b.use_var(self.vars.total);
        self.flat_run(&flat, total, slow);
        let total_after = self.b.ins().iadd_imm_u(total, i64::from(flat.count));
        self.b.def_var(self.vars.total, total_after);
        let last = self.i32(1);
        self.b.def_var(self.vars.last, last);
        let stop = self.i32((end + 2) as u32);
        self.b.def_var(self.vars.stop, stop);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(slow);
        let ran = self.call_fetch_run(index, end, access, total);
        self.ran(index, total, ran);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(done);
    }

    pub(super) fn fetch_run(&mut self, index: usize, end: usize, access: Value) {
        let total = self.b.use_var(self.vars.total);
        let ran = self.call_fetch_run(index, end, access, total);
        self.ran(index, total, ran);
    }

    fn call_fetch_run(&mut self, index: usize, end: usize, access: Value, total: Value) -> Value {
        let addr = self.i32(self.pc_read(index));
        let count = self.i32((end - index + 1) as u32);
        self.call(
            self.sigs.fetch_run,
            self.helpers.fetch_run,
            &[self.bus, addr, access, count, total, self.budget],
        )
    }

    fn iwram_run(&self, index: usize, end: usize) -> Option<FlatRun> {
        let quick = self.quick?;
        let (first, last) = (self.pc_read(index), self.pc_read(end));
        let words = [
            *self.plan.words.get(end + 1)?,
            *self.plan.words.get(end + 2)?,
        ];
        let latch = quick.iwram_latch(first, last, self.plan.thumb, words)?;
        Some(FlatRun {
            quick,
            count: (end - index + 1) as u32,
            last,
            latch,
        })
    }

    fn flat_run(&mut self, flat: &FlatRun, total: Value, slow: Block) {
        let quick = flat.quick;
        let (i32t, i64t) = (types::I32, types::I64);
        let flags = Self::flags();
        let bus = self.bus;
        let fast = self.b.create_block();
        let count = i64::from(flat.count);
        let now = self.b.ins().load(i32t, flags, bus, quick.now);
        let dma = self.b.ins().load(i32t, flags, bus, quick.dma_check);
        let wide_now = self.b.ins().uextend(i64t, now);
        let wide_dma = self.b.ins().uextend(i64t, dma);
        let reach = self.b.ins().iadd_imm_u(wide_now, count);
        let mut ok = self
            .b
            .ins()
            .icmp(IntCC::UnsignedLessThanOrEqual, reach, wide_dma);
        let wide_total = self.b.ins().uextend(i64t, total);
        let wide_budget = self.b.ins().uextend(i64t, self.budget);
        let spent = self.b.ins().iadd_imm_u(wide_total, count);
        let room = self
            .b
            .ins()
            .icmp(IntCC::UnsignedLessThan, spent, wide_budget);
        ok = self.b.ins().band(ok, room);
        let enabled = self.prefetch_enabled(&quick);
        ok = self.b.ins().band(ok, enabled);
        self.b.ins().brif(ok, fast, &[], slow, &[]);
        self.b.switch_to_block(fast);
        let clock = self.b.ins().load(i64t, flags, bus, quick.clock);
        let clock = self.b.ins().iadd_imm_u(clock, count);
        self.b.ins().store(flags, clock, bus, quick.clock);
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.ins().store(flags, zero, bus, quick.branched);
        let now = self.b.ins().iadd_imm_u(now, count);
        self.b.ins().store(flags, now, bus, quick.sample);
        self.b.ins().store(flags, now, bus, quick.now);
        let latch = self.i32(flat.latch);
        self.b.ins().store(flags, latch, bus, quick.open_iwram);
        self.b.ins().store(flags, latch, bus, quick.open_last);
        let at = self.i32(flat.last);
        self.b.ins().store(flags, at, bus, quick.open_addr);
    }

    fn prefetch_enabled(&mut self, quick: &QuickFetch) -> Value {
        let bits = self
            .b
            .ins()
            .uload16(types::I32, Self::flags(), self.bus, quick.wait_bits);
        let bit = self.b.ins().band_imm_u(bits, i64::from(quick.prefetch_bit));
        self.b.ins().icmp_imm_u(IntCC::NotEqual, bit, 0)
    }

    fn ran(&mut self, index: usize, total: Value, ran: Value) {
        let sum = self.b.ins().ireduce(types::I32, ran);
        let done = self.b.ins().ushr_imm_u(ran, i64::from(RUN_DONE_SHIFT));
        let done = self.b.ins().ireduce(types::I32, done);
        let done = self.b.ins().band_imm_u(done, 0xFFFF);
        let last = self.b.ins().ushr_imm_u(ran, i64::from(RUN_LAST_SHIFT));
        let last = self.b.ins().ireduce(types::I32, last);
        let total = self.b.ins().iadd(total, sum);
        self.b.def_var(self.vars.total, total);
        self.b.def_var(self.vars.last, last);
        let stop = self.b.ins().iadd_imm_u(done, index as i64);
        self.b.def_var(self.vars.stop, stop);
    }

    fn timed_fetch(&mut self, index: usize, idles: Option<Value>) -> Value {
        if idles.is_none()
            && let Some(cycles) = self.iwram_fetch(index)
        {
            return cycles;
        }
        self.helper_fetch(index, idles)
    }

    fn iwram_fetch(&mut self, index: usize) -> Option<Value> {
        let quick = self.quick?;
        let addr = self.pc_read(index);
        if addr >> 24 != quick.iwram_page {
            return None;
        }
        let word = *self.plan.words.get(index + 2)?;
        let (i32t, i64t) = (types::I32, types::I64);
        let flags = Self::flags();
        let bus = self.bus;
        let fast = self.b.create_block();
        let slow = self.b.create_block();
        let done = self.b.create_block();
        self.b.append_block_param(done, i32t);
        self.b.set_cold_block(slow);

        let now = self.b.ins().load(i32t, flags, bus, quick.now);
        let dma = self.b.ins().load(i32t, flags, bus, quick.dma_check);
        let early = self.b.ins().icmp(IntCC::UnsignedLessThan, now, dma);
        let enabled = self.prefetch_enabled(&quick);
        let ok = self.b.ins().band(early, enabled);
        self.b.ins().brif(ok, fast, &[], slow, &[]);

        self.b.switch_to_block(fast);
        let sample = self.b.ins().iadd_imm_u(now, 1);
        self.b.ins().store(flags, sample, bus, quick.sample);
        self.b.ins().store(flags, sample, bus, quick.now);
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.ins().store(flags, zero, bus, quick.branched);
        let clock = self.b.ins().load(i64t, flags, bus, quick.clock);
        let clock = self.b.ins().iadd_imm_u(clock, 1);
        self.b.ins().store(flags, clock, bus, quick.clock);
        let at = self.i32(addr);
        self.b.ins().store(flags, at, bus, quick.open_addr);
        let latch = if self.plan.thumb {
            let shift = 8 * (addr & 2);
            let latch = self.b.ins().load(i32t, flags, bus, quick.open_iwram);
            let kept = self
                .b
                .ins()
                .band_imm_u(latch, i64::from(!(0xFFFF_u32 << shift)));
            self.b
                .ins()
                .bor_imm_u(kept, i64::from((word & 0xFFFF) << shift))
        } else {
            self.i32(word)
        };
        self.b.ins().store(flags, latch, bus, quick.open_iwram);
        self.b.ins().store(flags, latch, bus, quick.open_last);
        let one = self.i32(1);
        self.b.ins().jump(done, &[one.into()]);

        self.b.switch_to_block(slow);
        let called = self.helper_fetch(index, None);
        self.b.ins().jump(done, &[called.into()]);

        self.b.switch_to_block(done);
        Some(self.b.block_params(done)[0])
    }

    fn helper_fetch(&mut self, index: usize, idles: Option<Value>) -> Value {
        let addr = self.i32(self.pc_read(index));
        let access = self.fetch_access();
        let packed = match idles {
            Some(idles) => {
                let idles = self.b.ins().ishl_imm_u(idles, i64::from(IDLES_SHIFT));
                self.b.ins().bor(idles, access)
            }
            None => access,
        };
        self.call(
            self.sigs.fetch,
            self.helpers.fetch,
            &[self.bus, addr, packed],
        )
    }

    pub(super) fn idled(&mut self, index: usize, cost: Value, idles: Value) -> Value {
        if self.v5 {
            return self.sum(cost, idles);
        }
        if self.timed {
            return self.timed_fetch(index, Some(idles));
        }
        let [nonsequential, _] = self.plan.fetch[index];
        let nonsequential = self.i32(nonsequential);
        self.sum(idles, nonsequential)
    }

    fn sum(&mut self, a: Value, b: Value) -> Value {
        match (self.known(a), self.known(b)) {
            (Some(a), Some(b)) => self.b.ins().iconst(types::I32, a + b),
            _ => self.b.ins().iadd(a, b),
        }
    }

    pub(super) fn settled(&mut self, cost: Value, helper: Value) -> Value {
        if self.v5 {
            self.overlap(cost, helper)
        } else if self.timed {
            self.b.ins().iadd(cost, helper)
        } else {
            helper
        }
    }

    pub(super) fn store_fetch(&mut self, index: usize, cost: Value) -> Value {
        if self.timed {
            return self.timed_fetch(index, None);
        }
        if !self.v5 {
            let r15 = self.i32(self.pc_read(index));
            self.store32(r15, self.layout.reg(15));
        }
        cost
    }

    pub(super) fn branch_fetch(&mut self, index: usize) -> Option<Value> {
        self.timed.then(|| self.timed_fetch(index, None))
    }

    pub(super) fn load_debt(&mut self) {
        if self.v5 {
            let debt = self.b.ins().load(types::I32, Self::flags(), self.debt, 0);
            self.b.def_var(self.vars.debt, debt);
        }
    }

    pub(super) fn store_debt(&mut self) {
        if self.v5 {
            let debt = self.b.use_var(self.vars.debt);
            self.b.ins().store(Self::flags(), debt, self.debt, 0);
        }
    }

    pub(super) fn overlap(&mut self, code: Value, data: Value) -> Value {
        let sum = self.b.ins().iadd(code, data);
        let over =
            self.b
                .ins()
                .icmp_imm_u(IntCC::UnsignedGreaterThan, sum, i64::from(DATA_OVERLAP));
        let overlap = self.i32(DATA_OVERLAP);
        let less = self.b.ins().isub(sum, overlap);
        let zero = self.i32(0);
        let less = self.b.ins().select(over, less, zero);
        let longer = self.b.ins().umax(code, data);
        self.b.ins().umax(less, longer)
    }

    pub(super) fn add_cycles(&mut self, cycles: Value) {
        if std::mem::take(&mut self.merged) {
            self.note(None);
            return;
        }
        let known = self.known(cycles);
        let bound = if self.v5 {
            known.map(|cycles| cycles.max(1))
        } else {
            known
        };
        self.note(bound);
        if !self.v5 {
            self.settle(cycles);
            return;
        }
        let debt = self.b.use_var(self.vars.debt);
        if known == Some(0) {
            let debt = self.b.ins().iadd_imm_u(debt, 1);
            self.b.def_var(self.vars.debt, debt);
            let one = self.i32(1);
            self.settle(one);
            return;
        }
        let one = self.i32(1);
        let spare = self.b.ins().isub(cycles, one);
        let repaid = self.b.ins().umin(debt, spare);
        let debt = self.b.ins().isub(debt, repaid);
        self.b.def_var(self.vars.debt, debt);
        let cycles = self.b.ins().isub(cycles, repaid);
        self.settle(cycles);
    }

    fn note(&mut self, bound: Option<i64>) {
        let bound = bound.and_then(|bound| u32::try_from(bound).ok());
        self.cost = self.cost.zip(bound).map(|(cost, bound)| cost.max(bound));
    }

    fn known(&self, value: Value) -> Option<i64> {
        let inst = self.b.func.dfg.value_def(value).inst()?;
        match &self.b.func.dfg.insts[inst] {
            InstructionData::UnaryImm {
                opcode: Opcode::Iconst,
                imm,
            } => Some(imm.bits()),
            _ => None,
        }
    }

    pub(super) fn add_settled(&mut self, cycles: Value) {
        self.note(None);
        self.settle(cycles);
    }

    fn settle(&mut self, cycles: Value) {
        let total = self.b.use_var(self.vars.total);
        let total = self.b.ins().iadd(total, cycles);
        self.b.def_var(self.vars.total, total);
        self.b.def_var(self.vars.last, cycles);
    }

    pub(super) fn set_force(&mut self, force: Option<Value>) {
        let force = match force {
            Some(value) => {
                self.note(None);
                value
            }
            None => self.b.ins().iconst(types::I8, 0),
        };
        self.b.def_var(self.vars.force, force);
    }
}

pub(super) fn merges(ops: &[Op]) -> Vec<Merge> {
    let mut merge = vec![Merge::Alone; ops.len()];
    let mut first = 0;
    while first < ops.len() {
        let mut end = first;
        while end < ops.len() && ops[end].straight() {
            end += 1;
        }
        if end - first >= 2 {
            merge[first] = Merge::First { end: end - 1 };
            for member in &mut merge[first + 1..end] {
                *member = Merge::Member { end: end - 1 };
            }
        }
        first = end.max(first + 1);
    }
    merge
}
