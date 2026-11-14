use super::super::fast::{Fast, Spec, Window};
use super::super::marks::PAGE_BITS;
use super::super::scan::Width;
use super::Builder;
use crate::gba::wait::Access;
use crate::paged::PAGE_SHIFT;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, MemFlagsData, Type, Value, types};
use cranelift_frontend::Variable;
use std::mem::{offset_of, size_of};

fn bytes(width: Width) -> u32 {
    match width {
        Width::Byte => 1,
        Width::Half => 2,
        Width::Word => 4,
    }
}

fn slot(access: Access) -> usize {
    match access {
        Access::NonSequential => 0,
        Access::Sequential => 1,
    }
}

const SPAN_BYTES: usize = 16 * 4;
const _: () = assert!(SPAN_BYTES <= 1 << PAGE_BITS && SPAN_BYTES <= 1 << PAGE_SHIFT);

fn span_access(at: usize) -> Access {
    if at == 0 {
        Access::NonSequential
    } else {
        Access::Sequential
    }
}

pub(super) fn guest() -> MemFlagsData {
    MemFlagsData::new().with_notrap()
}

impl Builder<'_, '_> {
    pub(super) fn load_memory(
        &mut self,
        addr: Value,
        width: Width,
        signed: bool,
        rotate: bool,
        access: Access,
        slow: impl FnOnce(&mut Self) -> Value,
    ) -> (Value, Value, Value) {
        let value = self.b.declare_var(types::I32);
        let cycles = self.b.declare_var(types::I32);
        let force = self.b.declare_var(types::I8);
        let done = self.b.create_block();
        let fallback = self.b.create_block();
        self.cold(fallback);
        let shared = self.b.create_block();
        self.b.append_block_param(shared, self.pointer);
        self.b.append_block_param(shared, types::I32);
        self.open_windows(fallback);
        for index in 0..self.windows() {
            let spec = self.specs[index];
            let hit = self.b.create_block();
            let next = self.next_window(index, fallback);
            let offset = self.enter_window(index, addr, hit, next);
            let offset = self.align(offset, spec, width);
            let host = self.host(index, offset);
            let cost = self.window_cycles(spec, addr, width, access);
            self.b.ins().jump(shared, &[host.into(), cost.into()]);
            self.b.switch_to_block(next);
        }
        let packed = slow(self);
        let high = self.b.ins().ushr_imm_u(packed, 32);
        let loaded = self.b.ins().ireduce(types::I32, high);
        let (cost, exit) = self.split(packed);
        self.b.def_var(value, loaded);
        self.b.def_var(cycles, cost);
        self.b.def_var(force, exit);
        self.b.ins().jump(done, &[]);
        if self.windows() > 0 {
            self.b.switch_to_block(shared);
            let host = self.b.block_params(shared)[0];
            let cost = self.b.block_params(shared)[1];
            self.load_hit(
                value, cycles, force, done, host, cost, addr, width, signed, rotate,
            );
        }
        self.b.switch_to_block(done);
        (
            self.b.use_var(value),
            self.b.use_var(cycles),
            self.b.use_var(force),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn load_hit(
        &mut self,
        value: Variable,
        cycles: Variable,
        force: Variable,
        done: Block,
        host: Value,
        cost: Value,
        addr: Value,
        width: Width,
        signed: bool,
        rotate: bool,
    ) {
        let loaded = match (width, signed) {
            (Width::Word, _) if !rotate => self.b.ins().load(types::I32, guest(), host, 0),
            (Width::Word, _) => {
                let word = self.b.ins().load(types::I32, guest(), host, 0);
                let low = self.b.ins().band_imm_u(addr, 3);
                let amount = self.b.ins().ishl_imm_u(low, 3);
                self.b.ins().rotr(word, amount)
            }
            (Width::Half, false) => self.b.ins().uload16(types::I32, guest(), host, 0),
            (Width::Half, true) => self.b.ins().sload16(types::I32, guest(), host, 0),
            (Width::Byte, false) => self.b.ins().uload8(types::I32, guest(), host, 0),
            (Width::Byte, true) => self.b.ins().sload8(types::I32, guest(), host, 0),
        };
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.def_var(value, loaded);
        self.b.def_var(cycles, cost);
        self.b.def_var(force, zero);
        self.b.ins().jump(done, &[]);
    }

    pub(super) fn store_memory(
        &mut self,
        addr: Value,
        value: Value,
        width: Width,
        access: Access,
        slow: impl FnOnce(&mut Self) -> Value,
    ) -> (Value, Value) {
        let cycles = self.b.declare_var(types::I32);
        let force = self.b.declare_var(types::I8);
        let done = self.b.create_block();
        let fallback = self.b.create_block();
        self.cold(fallback);
        let shared = self.b.create_block();
        self.b.append_block_param(shared, self.pointer);
        self.b.append_block_param(shared, types::I32);
        self.open_windows(fallback);
        for index in 0..self.windows() {
            let spec = self.specs[index];
            let hit = self.b.create_block();
            let next = self.next_window(index, fallback);
            let offset = self.enter_window(index, addr, hit, next);
            let offset = self.align(offset, spec, width);
            if spec.code {
                self.leave_code_pages(index, offset, fallback);
            }
            self.mark_dirty(index, offset);
            let host = self.host(index, offset);
            let cost = self.window_cycles(spec, addr, width, access);
            self.b.ins().jump(shared, &[host.into(), cost.into()]);
            self.b.switch_to_block(next);
        }
        let packed = slow(self);
        let (cost, exit) = self.split(packed);
        self.b.def_var(cycles, cost);
        self.b.def_var(force, exit);
        self.b.ins().jump(done, &[]);
        if self.windows() > 0 {
            self.b.switch_to_block(shared);
            let host = self.b.block_params(shared)[0];
            let cost = self.b.block_params(shared)[1];
            self.store_hit(cycles, force, done, host, cost, value, width);
        }
        self.b.switch_to_block(done);
        (self.b.use_var(cycles), self.b.use_var(force))
    }

    #[allow(clippy::too_many_arguments)]
    fn store_hit(
        &mut self,
        cycles: Variable,
        force: Variable,
        done: Block,
        host: Value,
        cost: Value,
        value: Value,
        width: Width,
    ) {
        match width {
            Width::Word => self.b.ins().store(guest(), value, host, 0),
            Width::Half => self.b.ins().istore16(guest(), value, host, 0),
            Width::Byte => self.b.ins().istore8(guest(), value, host, 0),
        };
        let zero = self.b.ins().iconst(types::I8, 0);
        self.b.def_var(cycles, cost);
        self.b.def_var(force, zero);
        self.b.ins().jump(done, &[]);
    }

    pub(super) fn block_memory(
        &mut self,
        start: Value,
        count: usize,
        stores: Option<&[Value]>,
        slow: impl Fn(&mut Self, usize, Value) -> Value,
    ) -> (Vec<Value>, Value, Value) {
        let loads: Vec<Variable> = match stores {
            Some(_) => Vec::new(),
            None => (0..count).map(|_| self.b.declare_var(types::I32)).collect(),
        };
        let cycles = self.b.declare_var(types::I32);
        let force = self.b.declare_var(types::I8);
        let done = self.b.create_block();
        let fallback = self.b.create_block();
        self.cold(fallback);
        let span = 4 * (count as i64 - 1);
        let last = self.b.ins().iadd_imm_u(start, span);
        self.open_windows(fallback);
        for index in 0..self.windows() {
            let spec = self.specs[index];
            let hit = self.b.create_block();
            let whole = self.b.create_block();
            let next = self.next_window(index, fallback);
            let offset = self.enter_window(index, start, hit, next);
            let mut inside = self.holds(index, last, IntCC::UnsignedLessThan);
            for earlier in 0..index {
                let outside = self.holds(earlier, last, IntCC::UnsignedGreaterThanOrEqual);
                inside = self.b.ins().band(inside, outside);
            }
            self.b.ins().brif(inside, whole, &[], fallback, &[]);
            self.b.switch_to_block(whole);
            let first = self.align(offset, spec, Width::Word);
            let end = self.b.ins().iadd_imm_u(offset, span);
            let end = self.align(end, spec, Width::Word);
            let ends: &[Value] = if count > 1 { &[first, end] } else { &[first] };
            if stores.is_some() {
                for &at in ends {
                    if spec.code {
                        self.leave_code_pages(index, at, fallback);
                    }
                }
                for &at in ends {
                    self.mark_dirty(index, at);
                }
            }
            let mut total = self.i32(0);
            for at in 0..count {
                let word = match at {
                    0 => first,
                    _ if at + 1 == count => end,
                    _ => {
                        let word = self.b.ins().iadd_imm_u(offset, 4 * at as i64);
                        self.align(word, spec, Width::Word)
                    }
                };
                let host = self.host(index, word);
                match stores {
                    Some(values) => {
                        self.b.ins().store(guest(), values[at], host, 0);
                    }
                    None => {
                        let loaded = self.b.ins().load(types::I32, guest(), host, 0);
                        self.b.def_var(loads[at], loaded);
                    }
                }
                let addr = self.b.ins().iadd_imm_u(start, 4 * at as i64);
                let cost = self.window_cycles(spec, addr, Width::Word, span_access(at));
                total = self.b.ins().iadd(total, cost);
            }
            let zero = self.b.ins().iconst(types::I8, 0);
            self.b.def_var(cycles, total);
            self.b.def_var(force, zero);
            self.b.ins().jump(done, &[]);
            self.b.switch_to_block(next);
        }
        let mut total = self.i32(0);
        let mut exits = self.b.ins().iconst(types::I8, 0);
        for at in 0..count {
            let addr = self.b.ins().iadd_imm_u(start, 4 * at as i64);
            let packed = slow(self, at, addr);
            if let Some(&var) = loads.get(at) {
                let high = self.b.ins().ushr_imm_u(packed, 32);
                let loaded = self.b.ins().ireduce(types::I32, high);
                self.b.def_var(var, loaded);
            }
            let (cost, exit) = self.split(packed);
            total = self.b.ins().iadd(total, cost);
            exits = self.b.ins().bor(exits, exit);
        }
        self.b.def_var(cycles, total);
        self.b.def_var(force, exits);
        self.b.ins().jump(done, &[]);
        self.b.switch_to_block(done);
        let loaded = loads.iter().map(|&var| self.b.use_var(var)).collect();
        (loaded, self.b.use_var(cycles), self.b.use_var(force))
    }

    fn holds(&mut self, index: usize, addr: Value, cc: IntCC) -> Value {
        let base = self.window_field(index, offset_of!(Window, base), types::I32);
        let size = self.window_field(index, offset_of!(Window, size), types::I64);
        let offset = self.b.ins().isub(addr, base);
        let wide = self.b.ins().uextend(types::I64, offset);
        self.b.ins().icmp(cc, wide, size)
    }

    fn cold(&mut self, fallback: Block) {
        if self.windows() > 0 {
            self.b.set_cold_block(fallback);
        }
    }

    fn windows(&self) -> usize {
        if self.v5 { self.specs.len() } else { 0 }
    }

    fn open_windows(&mut self, fallback: Block) {
        if self.windows() == 0 {
            self.b.ins().jump(fallback, &[]);
            self.b.switch_to_block(fallback);
        }
    }

    fn next_window(&mut self, index: usize, fallback: Block) -> Block {
        if index + 1 == self.windows() {
            fallback
        } else {
            self.b.create_block()
        }
    }

    pub(super) fn window_field(&mut self, index: usize, field: usize, ty: Type) -> Value {
        let at = offset_of!(Fast, windows) + index * size_of::<Window>() + field;
        self.b
            .ins()
            .load(ty, MemFlagsData::trusted(), self.fast, at as i32)
    }

    fn enter_window(&mut self, index: usize, addr: Value, hit: Block, next: Block) -> Value {
        let base = self.window_field(index, offset_of!(Window, base), types::I32);
        let size = self.window_field(index, offset_of!(Window, size), types::I64);
        let offset = self.b.ins().isub(addr, base);
        let wide = self.b.ins().uextend(types::I64, offset);
        let inside = self.b.ins().icmp(IntCC::UnsignedLessThan, wide, size);
        self.b.ins().brif(inside, hit, &[], next, &[]);
        self.b.switch_to_block(hit);
        offset
    }

    fn align(&mut self, offset: Value, spec: Spec, width: Width) -> Value {
        let mask = (spec.len - 1) & !(bytes(width) - 1);
        self.b.ins().band_imm_u(offset, i64::from(mask))
    }

    fn host(&mut self, index: usize, offset: Value) -> Value {
        let base = self.window_field(index, offset_of!(Window, bytes), self.pointer);
        let wide = self.b.ins().uextend(self.pointer, offset);
        self.b.ins().iadd(base, wide)
    }

    fn leave_code_pages(&mut self, index: usize, offset: Value, fallback: Block) {
        let write = self.b.create_block();
        let page = self.b.ins().ushr_imm_u(offset, PAGE_BITS as i64);
        let page = self.b.ins().uextend(self.pointer, page);
        let flags = self.window_field(index, offset_of!(Window, code), self.pointer);
        let flag = self.b.ins().iadd(flags, page);
        let flag = self.b.ins().uload8(types::I32, guest(), flag, 0);
        self.b.ins().brif(flag, fallback, &[], write, &[]);
        self.b.switch_to_block(write);
    }

    fn mark_dirty(&mut self, index: usize, offset: Value) {
        let dirty = self.window_field(index, offset_of!(Window, dirty), self.pointer);
        let page = self.b.ins().ushr_imm_u(offset, PAGE_SHIFT as i64);
        let page = self.b.ins().uextend(self.pointer, page);
        let dirty = self.b.ins().iadd(dirty, page);
        let one = self.b.ins().iconst(types::I8, 1);
        self.b.ins().store(guest(), one, dirty, 0);
    }

    fn window_cycles(&mut self, spec: Spec, addr: Value, width: Width, access: Access) -> Value {
        let slot = slot(access);
        let uncached = match width {
            Width::Word => spec.word[slot],
            Width::Byte | Width::Half => spec.half[slot],
        };
        let uncached = self.i32(uncached);
        let Some(cache) = spec.cache else {
            return uncached;
        };
        let pages = self.b.ins().load(
            self.pointer,
            MemFlagsData::trusted(),
            self.fast,
            offset_of!(Fast, cache) as i32,
        );
        let page = self.b.ins().ushr_imm_u(addr, i64::from(cache.shift));
        let page = self.b.ins().uextend(self.pointer, page);
        let flags = self.b.ins().iadd(pages, page);
        let flags = self.b.ins().uload8(types::I32, guest(), flags, 0);
        let hit = self.b.ins().band_imm_u(flags, i64::from(cache.bit));
        let cached = self.i32(cache.cycles[slot]);
        self.b.ins().select(hit, cached, uncached)
    }
}
