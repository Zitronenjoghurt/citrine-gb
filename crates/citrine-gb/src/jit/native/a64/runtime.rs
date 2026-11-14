use super::super::super::fast::{
    CODE_CYCLES, Fast, HALF_CYCLES, HALF_SEQUENTIAL, LONG_AFTER, LONG_HALF, LONG_WORD, MAIN,
    PAGE_SHIFT, PAGE_SIZE, Page, SEQUENTIAL_CYCLES, Spec, WORD_CYCLES, Window,
};
use super::super::super::helpers::{EXIT, LAST, THUMB};
use super::super::super::lookup::{Link, SLOT_GENERATION, SLOT_LINKS, SLOT_PIPELINE};
use super::super::super::marks::PAGE_BITS;
use super::super::super::scan::Width;
use super::super::{CODE_CLASS, Offsets, PSR_RESTORE, RESTORE_FIELDS, Runtime, STUBS, Setup, Stub};
use super::asm::{Asm, Indexed, LR, Label, Logic, R, SP, Shift, ZR, logical_immediate};
use super::{
    CTX, DEBT, GUEST, KEEP, LEAVE_PENDING, LEAVE_RD, OLD, PSR_RESULT, S, SEQUENTIAL, at_shift,
    left_word,
};
use crate::gba::cpu::psr::{FLAG_I, FLAG_T};
use std::mem::{offset_of, size_of};

const FRAME: i32 = 96;
const SAVED: i32 = 96;
const NZCV_MASK: u32 = 0xF000_0000;
const CYCLES_MASK: u32 = EXIT as u32 - 1;
const DATA_OVERLAP: u32 = 6;
const CODE_OVERLAP: u32 = 3;
const RETURN_INFO: u32 = 0x1A00;
const STEPPED_INFO: u32 = 0x1E00;
const _: () = assert!(DATA_OVERLAP == LONG_AFTER);
const _: () = assert!(size_of::<Page>() == 32);
const _: () = assert!(offset_of!(Page, meta) == 8 && offset_of!(Page, code) == 24);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leave {
    Branch,
    Exchange,
    Return,
    Restore,
}

#[derive(Clone, Copy)]
struct Scratch {
    words: [R; 2],
    cycles: [R; 2],
    cache: R,
    line: R,
}

const STORED: Scratch = Scratch {
    words: [22, 23],
    cycles: [24, 27],
    cache: 25,
    line: 26,
};

const LIVE: Scratch = Scratch {
    words: [2, LR],
    cycles: [2, LR],
    cache: 5,
    line: 7,
};

struct Build<'a> {
    a: Asm,
    o: &'a Offsets,
    v5: bool,
    timed: bool,
    pages: bool,
    exit: Option<Label>,
}

pub fn runtime(setup: &Setup) -> Option<Runtime> {
    let (o, helpers, chain) = (setup.offsets, setup.helpers, setup.chain);
    let mut build = Build {
        a: Asm::new(),
        o,
        v5: setup.v5,
        timed: setup.timed,
        pages: setup.pages,
        exit: None,
    };
    let finish = build.a.label();
    let next = match chain {
        Some(_) => build.a.label(),
        None => finish,
    };
    let live = chain.map(|_| build.a.label());
    let (exited, stepped, psr_called, returned) = (
        build.a.label(),
        build.a.label(),
        build.a.label(),
        build.a.label(),
    );
    build.trampoline();
    let exit = build.here();
    build.a.bind(exited);
    let leaves = build.a.label();
    build.a.bind(leaves);
    build.exit = Some(leaves);
    build.exit(next, live);
    let branch = build.here();
    build.leave(helpers.branch, Leave::Branch, setup.specs, next, live)?;
    let exchange = build.here();
    build.leave(helpers.exchange, Leave::Exchange, setup.specs, next, live)?;
    let returns = build.here();
    build.a.bind(returned);
    build.leave(helpers.load_pc, Leave::Return, setup.specs, next, None)?;
    let restore = build.here();
    build.restore(psr_called, stepped, exited);
    build.leave(helpers.branch, Leave::Restore, setup.specs, next, live)?;
    let load = build.here();
    let windows = if setup.pages { &[][..] } else { setup.specs };
    let tail = build.memory(helpers.load, false, setup.pages, windows, false)?;
    let swap = build.a.label();
    build.a.b(swap);
    let store = build.here();
    build.memory(helpers.store, true, setup.pages, windows, false)?;
    let load_leave = build.here();
    build.memory(helpers.load, false, setup.pages, windows, true)?;
    let store_leave = build.here();
    build.memory(helpers.store, true, setup.pages, windows, true)?;
    let gather = build.here();
    build.block(helpers.load_word, false, setup.pages, windows, tail)?;
    let scatter = build.here();
    build.block(helpers.store_word, true, setup.pages, windows, tail)?;
    let collect = build.here();
    let collected = build.a.label();
    build.block(
        helpers.load_word,
        false,
        setup.pages && setup.v5,
        windows,
        collected,
    )?;
    build.a.bind(collected);
    build.a.msr_nzcv(3);
    build.a.ret();
    let step = build.here();
    build.a.bind(stepped);
    build.step(helpers.step);
    let psr = build.here();
    build.a.bind(psr_called);
    build.psr(setup.psr);
    let load_pc = build.here();
    build.load_pc(helpers.load, setup.pages && setup.v5, windows, returned)?;
    build.a.bind(swap);
    build.memory(helpers.swap, true, false, &[], false)?;
    let fetch = build.here();
    if setup.timed {
        build.fetch(helpers.fetch, None);
    }
    let fetch_arm = build.here();
    if setup.timed {
        build.fetch(helpers.fetch, Some(0));
    }
    let fetch_thumb = build.here();
    if setup.timed {
        build.fetch(helpers.fetch, Some(THUMB));
    }
    let fetch_flat = build.here();
    if setup.timed {
        build.fetch_flat(helpers.fetch_flat);
    }
    if let (Some(chain), Some(live)) = (chain, live) {
        build.chain(chain, next, finish, live);
    }
    build.a.bind(finish);
    build.finish();
    let mut stubs = [0; STUBS];
    for (stub, at) in [
        (Stub::Exit, exit),
        (Stub::Load, load),
        (Stub::Store, store),
        (Stub::Branch, branch),
        (Stub::Exchange, exchange),
        (Stub::Step, step),
        (Stub::Gather, gather),
        (Stub::Scatter, scatter),
        (Stub::Collect, collect),
        (Stub::Return, returns),
        (Stub::Psr, psr),
        (Stub::LoadPc, load_pc),
        (Stub::Fetch, fetch),
        (Stub::Restore, restore),
        (Stub::FetchFlat, fetch_flat),
        (Stub::FetchArm, fetch_arm),
        (Stub::FetchThumb, fetch_thumb),
        (Stub::LoadLeave, load_leave),
        (Stub::StoreLeave, store_leave),
    ] {
        stubs[stub as usize] = 4 * at;
    }
    Some(Runtime {
        bytes: build.a.finish()?,
        stubs,
    })
}

impl Build<'_> {
    fn here(&self) -> usize {
        self.a.here()
    }

    fn mask_or(&mut self, rd: R, bits: u32) {
        if let Some((immr, imms)) = logical_immediate(bits) {
            self.a.logical_imm(Logic::Orr, rd, rd, immr, imms);
        }
    }

    fn mask(&mut self, rd: R, rn: R, mask: u32) {
        if let Some((immr, imms)) = logical_immediate(mask) {
            self.a.logical_imm(Logic::And, rd, rn, immr, imms);
        }
    }

    fn guest_regs(&mut self, load: bool, base: R) {
        let pairs = [
            (GUEST[0], GUEST[1], 0),
            (GUEST[2], GUEST[3], 8),
            (GUEST[4], GUEST[5], 16),
            (GUEST[6], GUEST[7], 24),
            (GUEST[8], GUEST[9], 32),
            (GUEST[10], GUEST[11], 40),
            (GUEST[12], GUEST[13], 48),
        ];
        for (first, second, offset) in pairs {
            self.a.pair(load, false, first, second, base, offset);
        }
        if load {
            self.a.ldr(false, GUEST[14], base, 56);
        } else {
            self.a.str(false, GUEST[14], base, 56);
        }
    }

    fn store_cpsr(&mut self, core: R, cpsr: R, scratch: R) {
        self.a.ldr(false, cpsr, core, self.o.cpsr);
        self.a.mrs_nzcv(scratch);
        self.a.lsr(scratch, scratch, 28);
        self.a.bfi(cpsr, scratch, 28, 4);
        self.a.str(false, cpsr, core, self.o.cpsr);
    }

    fn call(&mut self, helper: usize) {
        self.a.mov_imm64(17, helper as u64);
        self.a.blr(17);
    }

    fn store_pc(&mut self, core: R, packed: R, scratch: [R; 2]) {
        let [at, shift] = scratch;
        self.a.ubfx(packed, packed, 8, 8);
        self.a.pair(true, false, at, shift, CTX, self.o.base as i32);
        self.a.lslv(packed, packed, shift);
        self.a
            .arith(false, false, false, packed, at, packed, Shift::Lsl, 0);
        self.a.str(false, packed, core, self.o.regs + 60);
    }

    fn trampoline(&mut self) {
        let o = self.o;
        let (near, run, step) = (self.a.label(), self.a.label(), self.a.label());
        self.a.ldr_index(16, 6, 3);
        if self.timed {
            self.a.lsr(17, 16, 27);
            self.a.cmp(2, 17);
            self.a.b_cond(0x9, near);
        }
        self.a.bind(run);
        self.a.push_pair(29, LR, FRAME);
        self.a.mov64(29, SP);
        for (at, first) in [19, 21, 23, 25, 27].into_iter().enumerate() {
            self.a
                .pair(false, true, first, first + 1, SP, 16 * (at as i32 + 1));
        }
        self.a.mov64(CTX, 7);
        self.a.str(false, ZR, CTX, o.acc);
        self.a.str(true, 4, CTX, o.fast);
        self.a.pair(false, true, 0, 1, CTX, o.core as i32);
        self.a.str(true, 5, CTX, o.words);
        self.a.pair(false, false, 2, 3, CTX, o.budget as i32);
        if self.timed {
            self.a.mov(DEBT, ZR);
        } else {
            self.a.ldr(true, 4, 4, o.fast_debt);
            self.a.str(true, 4, CTX, o.debt);
            self.a.ldr(false, DEBT, 4, 0);
        }
        self.a.ubfx(17, 16, 16, 11);
        self.a.arith(false, false, false, S, 2, DEBT, Shift::Lsl, 0);
        self.a.arith(false, false, false, S, S, 17, Shift::Lsl, 0);
        self.a.add_imm(true, false, 4, 0, o.regs);
        self.guest_regs(true, 4);
        self.a.ldr(false, 5, 4, 60);
        self.a.ldr(false, 17, 0, o.cpsr);
        self.a.ubfx(1, 17, 5, 1);
        self.a.mov_imm(4, 2);
        self.a.arith(false, true, false, 4, 4, 1, Shift::Lsl, 0);
        self.a.lslv(1, 3, 4);
        self.a.arith(false, true, false, 5, 5, 1, Shift::Lsl, 0);
        self.a.pair(false, false, 5, 4, CTX, o.base as i32);
        self.mask(17, 17, NZCV_MASK);
        self.a.msr_nzcv(17);
        self.mask(16, 16, 0xFFFF);
        self.a.add_ext(16, 6, 16, 2);
        self.a.mov_imm(0, KEEP);
        self.a.br(16);
        if !self.timed {
            return;
        }
        self.a.bind(near);
        self.a.sub_imm(true, false, 17, 6, 4);
        self.a.ldr(false, 17, 17, 0);
        self.a.lsr(17, 17, 16);
        self.a.cbz(false, 17, step);
        self.a.arith(false, false, false, 17, 17, 3, Shift::Lsl, 0);
        self.a.ldr_index(16, 6, 17);
        self.a.b(run);
        self.a.bind(step);
        self.a.movz(true, 0, (EXIT >> 16) as u32, 1);
        self.a.ret();
    }

    fn exit(&mut self, finish: Label, live: Option<Label>) {
        let o = self.o;
        let info = 16;
        self.a.ldr(true, 0, CTX, o.core);
        self.a.add_imm(true, false, 1, 0, o.regs);
        if live.is_none() {
            self.guest_regs(false, 1);
        }
        let finish = live.unwrap_or(finish);
        self.store_cpsr(0, 2, 3);
        self.a.ubfx(3, info, 22, 10);
        self.a.arith(false, true, false, S, S, 3, Shift::Lsl, 0);
        let (counted, old, done) = (self.a.label(), self.a.label(), self.a.label());
        self.a.ubfx(3, info, 12, 2);
        self.a.mov_imm(4, 1);
        self.a.cmp_imm(3, 1);
        self.a.b_cond(0x3, done);
        self.a.b_cond(0x1, old);
        self.a.ldr(false, 4, CTX, o.last);
        self.a.b(counted);
        self.a.bind(old);
        self.a.ubfx(4, info, 14, 8);
        self.a.arith(false, true, false, 4, 4, OLD, Shift::Lsl, 0);
        self.a.cmp_imm(4, 1);
        self.a.csinc(4, 4, ZR, 0xA);
        self.a.bind(done);
        self.a.bind(counted);
        self.a.str(false, 4, 0, o.cycles);
        self.a.strb(ZR, 0, o.irq_held);
        self.store_debt(3);
        let (forced, kept, normal) = (self.a.label(), self.a.label(), self.a.label());
        self.a.mov_imm(5, 0);
        self.a.tbz(false, info, 10, forced);
        self.a.ldr(false, 5, CTX, o.force);
        self.a.bind(forced);
        self.a.tbz(false, info, 11, normal);
        self.a.b(finish);
        self.a.bind(normal);
        self.mask(3, info, 0xFF);
        self.a.pair(true, false, 4, 6, CTX, o.base as i32);
        self.a.lslv(6, 3, 6);
        self.a.arith(false, false, false, 4, 4, 6, Shift::Lsl, 0);
        self.a.str(false, 4, 1, 60);
        self.a.ldr(true, 6, CTX, o.words);
        self.a.add_ext(6, 6, 3, 2);
        self.a.pair(true, false, 3, 4, 6, 0);
        self.a.add_imm(true, false, 6, 0, o.pipeline);
        self.a.pair(false, false, 3, 4, 6, 0);
        self.a.tbz(true, info, 9, kept);
        self.a.ubfx(3, info, 8, 1);
        self.a.strb(3, 0, o.next_fetch);
        self.a.bind(kept);
        self.a.ubfx(3, 2, 7, 1);
        self.a.strb(3, 0, o.irq_masked);
        self.a.b(finish);
    }

    fn leave(
        &mut self,
        helper: usize,
        leave: Leave,
        specs: &[Spec],
        finish: Label,
        live: Option<Label>,
    ) -> Option<()> {
        match live {
            Some(live) => self.leave_live(helper, leave, specs, finish, live),
            None => self.leave_stored(helper, leave, specs, finish),
        }
    }

    fn leave_live(
        &mut self,
        helper: usize,
        leave: Leave,
        specs: &[Spec],
        finish: Label,
        live: Label,
    ) -> Option<()> {
        let o = self.o;
        self.a.ldr(true, 1, CTX, o.core);
        self.store_cpsr(1, 3, 4);
        self.a.ubfx(4, 16, 22, 10);
        self.a.arith(false, true, false, S, S, 4, Shift::Lsl, 0);
        let (slow, called) = (self.a.label(), self.a.label());
        if self.refills(leave, specs) {
            self.refetch(leave, specs, slow, called, LIVE)?;
        }
        self.a.bind(slow);
        self.a.add_imm(true, false, 2, 1, o.regs);
        self.guest_regs(false, 2);
        self.a.mov64(19, 16);
        self.a.mov(20, S);
        self.a.mov(21, DEBT);
        self.a.mov(2, 0);
        self.a.mov64(0, 1);
        self.a.ldr(true, 1, CTX, o.bus);
        self.call(helper);
        self.own_cost(leave, 19);
        self.a.arith(false, true, false, 20, 20, 0, Shift::Lsl, 0);
        if self.timed {
            self.a.ldr(false, 5, CTX, o.last);
            self.a.arith(false, false, false, 0, 0, 5, Shift::Lsl, 0);
        }
        self.settle(0, 21, 3, 4);
        self.a.ldr(true, 0, CTX, o.core);
        self.a.str(false, 4, 0, o.cycles);
        self.a.strb(ZR, 0, o.irq_held);
        if !self.timed {
            self.a.ldr(true, 3, CTX, o.debt);
            self.a.str(false, 21, 3, 0);
        }
        self.a.mov64(16, 19);
        self.a.mov(S, 20);
        self.a.mov(DEBT, 21);
        self.a.mov_imm(5, 0);
        self.a.b(finish);
        self.a.bind(called);
        self.own_cost(leave, 16);
        self.a.arith(false, true, false, S, S, 0, Shift::Lsl, 0);
        if self.timed {
            self.a.ldr(false, 5, CTX, o.last);
            self.a.arith(false, false, false, 0, 0, 5, Shift::Lsl, 0);
        }
        self.settle(0, DEBT, 2, 4);
        self.a.str(false, 4, 1, o.cycles);
        self.a.strb(ZR, 1, o.irq_held);
        self.store_debt(3);
        self.a.mov_imm(5, 0);
        self.a.b(live);
        Some(())
    }

    fn leave_stored(
        &mut self,
        helper: usize,
        leave: Leave,
        specs: &[Spec],
        finish: Label,
    ) -> Option<()> {
        let o = self.o;
        let returns = leave == Leave::Return;
        if returns {
            self.a.mov(5, 1);
        }
        self.a.ldr(true, 1, CTX, o.core);
        self.a.add_imm(true, false, 2, 1, o.regs);
        self.guest_regs(false, 2);
        self.store_cpsr(1, 3, 4);
        if returns {
            self.a.mov(19, 16);
        } else {
            self.a.mov64(19, 16);
        }
        if returns {
            self.a.logical(Logic::Orr, true, 19, 19, 5, Shift::Lsl, 32);
        }
        self.a.ubfx(4, 16, 22, 10);
        self.a.arith(false, true, false, 20, S, 4, Shift::Lsl, 0);
        self.a.mov(21, DEBT);
        let (slow, called) = (self.a.label(), self.a.label());
        if self.refills(leave, specs) {
            self.refetch(leave, specs, slow, called, STORED)?;
        }
        self.a.bind(slow);
        self.a.mov(2, 0);
        self.a.mov64(0, 1);
        self.a.ldr(true, 1, CTX, o.bus);
        self.call(helper);
        self.a.bind(called);
        if returns {
            let refetched = self.a.label();
            self.a.lsr64(5, 0, 32);
            self.a.lsr64(6, 19, 32);
            self.a.tbz(false, 6, 31, refetched);
            self.a.ubfx(5, 6, 16, 15);
            self.a.bind(refetched);
            self.a.ubfx(6, 6, 0, 16);
            self.overlap(4, 5, 6);
            self.a.arith(false, false, false, 0, 0, 4, Shift::Lsl, 0);
        }
        self.own_cost(leave, 19);
        self.a.arith(false, true, false, 20, 20, 0, Shift::Lsl, 0);
        if self.timed {
            self.a.ldr(false, 5, CTX, o.last);
            self.a.arith(false, false, false, 0, 0, 5, Shift::Lsl, 0);
        }
        self.settle(0, 21, 3, 4);
        self.a.ldr(true, 0, CTX, o.core);
        self.a.str(false, 4, 0, o.cycles);
        self.a.strb(ZR, 0, o.irq_held);
        if !self.timed {
            self.a.ldr(true, 3, CTX, o.debt);
            self.a.str(false, 21, 3, 0);
        }
        if returns {
            self.a.mov(16, 19);
        } else {
            self.a.mov64(16, 19);
        }
        self.a.mov(S, 20);
        self.a.mov(DEBT, 21);
        self.a.mov_imm(5, 0);
        self.a.b(finish);
        Some(())
    }

    fn own_cost(&mut self, leave: Leave, info: R) {
        if leave == Leave::Restore {
            self.a.arith(true, false, false, 0, 0, info, Shift::Lsr, 32);
            self.a.mov(info, info);
        }
    }

    fn refills(&self, leave: Leave, specs: &[Spec]) -> bool {
        if self.v5 || !self.pages {
            specs.iter().any(|spec| spec.code)
        } else {
            leave != Leave::Return
        }
    }

    fn refetch(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        slow: Label,
        called: Label,
        scratch: Scratch,
    ) -> Option<()> {
        let exchange = !matches!(leave, Leave::Branch | Leave::Restore);
        let thumb = self.a.label();
        if exchange {
            self.a.tbz(true, 0, 0, thumb);
        } else {
            self.a.tbz(true, 3, FLAG_T.trailing_zeros(), thumb);
        }
        self.mask(6, 0, 3);
        self.a.cbz(true, 6, slow);
        self.a.mov(6, 0);
        if exchange {
            self.mask(3, 3, !FLAG_T);
        }
        self.fill(leave, specs, 4, slow, called, scratch)?;
        self.a.bind(thumb);
        self.mask(6, 0, !1);
        if exchange {
            let (immr, imms) = logical_immediate(FLAG_T)?;
            self.a.logical_imm(Logic::Orr, 3, 3, immr, imms);
        }
        self.fill(leave, specs, 2, slow, called, scratch)
    }

    fn fill(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        step: u32,
        slow: Label,
        called: Label,
        scratch: Scratch,
    ) -> Option<()> {
        if self.v5 || !self.pages {
            return self.refill(leave, specs, step, slow, called, scratch);
        }
        self.refill_page(step, slow, scratch);
        self.refilled(leave, step, called, scratch);
        Some(())
    }

    fn refill_page(&mut self, step: u32, slow: Label, scratch: Scratch) {
        let [first, second] = scratch.words;
        let [cycles, _] = scratch.cycles;
        let (load, entry, later) = if step == 2 {
            (Indexed::LoadHalf, HALF_CYCLES, HALF_SEQUENTIAL)
        } else {
            (Indexed::LoadWord, WORD_CYCLES, SEQUENTIAL_CYCLES)
        };
        self.a.add_imm(false, false, 5, 6, step);
        self.a.ubfx(4, 5, 0, PAGE_SHIFT);
        self.a.cbz(false, 4, slow);
        self.a.ldr(true, 4, CTX, self.o.pages);
        self.a.lsr(7, 6, PAGE_SHIFT);
        self.a.arith(true, false, false, 4, 4, 7, Shift::Lsl, 5);
        self.a.pair(true, true, 7, 17, 4, 0);
        self.a.cbz(false, 17, slow);
        self.a.indexed(load, first, 7, 6);
        self.a.indexed(load, second, 7, 5);
        self.a.add_imm(true, false, 4, 1, self.o.pipeline);
        self.a.pair(false, false, first, second, 4, 0);
        self.a.ubfx(cycles, 17, entry, 8);
        if later < 32 {
            self.a.ubfx(4, 17, later, 8);
        } else {
            self.a.lsr64(4, 17, later);
        }
        self.a
            .arith(false, false, false, cycles, cycles, 4, Shift::Lsl, 0);
    }

    fn refilled(&mut self, leave: Leave, step: u32, called: Label, scratch: Scratch) {
        let o = self.o;
        let exchange = !matches!(leave, Leave::Branch | Leave::Restore);
        let returns = leave == Leave::Return;
        let [cycles, refetched] = scratch.cycles;
        self.a.add_imm(false, false, 17, 6, 2 * step);
        self.a.str(false, 17, 1, o.regs + 60);
        if !self.timed {
            self.a.str(false, 6, 1, o.landed);
        }
        self.a.mov_imm(17, SEQUENTIAL);
        self.a.strb(17, 1, o.next_fetch);
        if exchange {
            self.a.str(false, 3, 1, o.cpsr);
        }
        self.a.ubfx(17, 3, FLAG_I.trailing_zeros(), 1);
        self.a.strb(17, 1, o.irq_masked);
        if returns {
            self.a
                .logical(Logic::Orr, true, 0, cycles, refetched, Shift::Lsl, 32);
        } else {
            self.a.mov(0, cycles);
        }
        self.a.b(called);
    }

    fn refill(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        step: u32,
        slow: Label,
        called: Label,
        scratch: Scratch,
    ) -> Option<()> {
        let o = self.o;
        let returns = leave == Leave::Return;
        let [first, second] = scratch.words;
        let load = if step == 2 {
            Indexed::LoadHalf
        } else {
            Indexed::LoadWord
        };
        self.a.ldr(true, 4, CTX, o.fast);
        for (index, spec) in specs.iter().enumerate() {
            if !spec.code {
                continue;
            }
            let next = self.a.label();
            let window = offset_of!(Fast, windows) + index * size_of::<Window>();
            let field = |at: usize| (window + at) as u32;
            self.a.ldr(false, 5, 4, field(offset_of!(Window, base)));
            self.a.ldr(true, 7, 4, field(offset_of!(Window, size)));
            self.a.arith(false, true, false, 5, 6, 5, Shift::Lsl, 0);
            self.a.add_imm(true, false, 17, 5, step);
            self.a.arith(true, true, false, 17, 17, 7, Shift::Lsl, 0);
            self.a.tbz(false, 17, 63, next);
            let (immr, imms) = logical_immediate((spec.len - 1) & !(step - 1))?;
            self.a.ldr(true, 7, 4, field(offset_of!(Window, bytes)));
            self.a.logical_imm(Logic::And, 17, 5, immr, imms);
            self.a.indexed(load, first, 7, 17);
            self.a.add_imm(false, false, 17, 5, step);
            self.a.logical_imm(Logic::And, 17, 17, immr, imms);
            self.a.indexed(load, second, 7, 17);
            self.a.add_imm(true, false, 17, 1, o.pipeline);
            self.a.pair(false, false, first, second, 17, 0);
            self.refill_cycles(spec, step, returns, scratch)?;
            self.refilled(leave, step, called, scratch);
            self.a.bind(next);
        }
        self.a.b(slow);
        Some(())
    }

    fn refill_cycles(
        &mut self,
        spec: &Spec,
        step: u32,
        refetched: bool,
        scratch: Scratch,
    ) -> Option<()> {
        let [sum, second] = scratch.cycles;
        let (table, line) = (scratch.cache, scratch.line);
        let word = spec.word[0];
        let done = self.a.label();
        let Some(cache) = spec.cache else {
            self.a.mov_imm(sum, word);
            if refetched {
                self.a.mov_imm(second, 0);
            }
            if step == 2 {
                self.a.add_imm(false, false, 17, 6, step);
                self.a.tbz(true, 17, 1, done);
            }
            self.a.mov_imm(sum, 2 * word);
            if refetched {
                self.a.mov_imm(second, word);
            }
            self.a.bind(done);
            return Some(());
        };
        if !cache.code.is_power_of_two() {
            return None;
        }
        let (line_r, line_s) = logical_immediate(cache.line)?;
        let code = cache.code.trailing_zeros();
        let (first, later) = (self.a.label(), self.a.label());
        self.a.ldr(true, table, 4, offset_of!(Fast, cache) as u32);
        self.a.lsr(line, 6, cache.shift);
        self.a.indexed(Indexed::LoadByte, line, table, line);
        self.a.mov_imm(sum, word);
        self.a.tbz(false, line, code, first);
        self.a.mov_imm(sum, cache.cycles[0]);
        self.a.bind(first);
        if refetched {
            self.a.mov_imm(second, 0);
        }
        self.a.add_imm(false, false, 17, 6, step);
        if step == 2 {
            self.a.tbz(true, 17, 1, done);
        }
        self.a.lsr(line, 17, cache.shift);
        self.a.indexed(Indexed::LoadByte, line, table, line);
        self.a.mov_imm(second, word);
        self.a.tbz(false, line, code, later);
        self.a.logical_imm(Logic::And, line, 17, line_r, line_s);
        self.a.mov_imm(second, cache.cycles[1]);
        self.a.cbz(true, line, later);
        self.a.mov_imm(second, cache.cycles[0]);
        self.a.bind(later);
        self.a
            .arith(false, false, false, sum, sum, second, Shift::Lsl, 0);
        self.a.bind(done);
        Some(())
    }

    fn overlap(&mut self, into: R, code: R, data: R) {
        self.a
            .arith(false, false, false, 7, code, data, Shift::Lsl, 0);
        self.a.sub_imm(false, true, 7, 7, DATA_OVERLAP);
        self.a.csel(7, 7, ZR, 0x8);
        self.a.cmp(code, data);
        self.a.csel(into, code, data, 0x8);
        self.a.cmp(7, into);
        self.a.csel(into, 7, into, 0x8);
    }

    fn settle(&mut self, cycles: R, debt: R, repaid: R, last: R) {
        self.a.sub_imm(false, false, repaid, cycles, 1);
        self.a.cmp(debt, repaid);
        self.a.csel(repaid, debt, repaid, 0x3);
        self.a
            .arith(false, true, false, debt, debt, repaid, Shift::Lsl, 0);
        self.a
            .arith(false, true, false, last, cycles, repaid, Shift::Lsl, 0);
    }

    fn finish(&mut self) {
        let o = self.o;
        self.mask(3, 16, 0xFF);
        self.a.pair(true, false, 4, 6, CTX, o.budget as i32);
        self.a.arith(false, true, false, 3, 3, 6, Shift::Lsl, 0);
        self.a.ldr(false, 6, CTX, o.acc);
        self.a.arith(false, false, false, 3, 3, 6, Shift::Lsl, 0);
        self.a.arith(false, true, false, 6, S, DEBT, Shift::Lsl, 0);
        self.a.arith(false, true, false, 4, 4, 6, Shift::Lsl, 0);
        self.a.logical(Logic::Orr, false, 4, 4, 5, Shift::Lsl, 31);
        self.a.logical(Logic::Orr, true, 0, 4, 3, Shift::Lsl, 32);
        for (at, first) in [19, 21, 23, 25, 27].into_iter().enumerate() {
            self.a
                .pair(true, true, first, first + 1, SP, 16 * (at as i32 + 1));
        }
        self.a.pop_pair(29, LR, FRAME);
        self.a.ret();
    }

    fn chain(&mut self, chain: usize, stored: Label, finish: Label, live: Label) {
        let o = self.o;
        let (refused, called, enter) = (self.a.label(), self.a.label(), self.a.label());
        let (to_call, to_refuse, to_finish, jump) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a.bind(live);
        self.a.cbz(true, 5, to_finish);
        self.hop(to_call, to_refuse, jump);
        for (from, to) in [(to_call, called), (to_refuse, refused), (to_finish, finish)] {
            self.a.bind(from);
            self.a.ldr(true, 4, CTX, o.core);
            self.a.add_imm(true, false, 4, 4, o.regs);
            self.guest_regs(false, 4);
            self.a.b(to);
        }
        self.a.bind(jump);
        self.a.ldr(true, 4, CTX, o.core);
        self.a.ldr(false, 17, 4, o.cpsr);
        self.mask(17, 17, NZCV_MASK);
        self.a.msr_nzcv(17);
        self.a.mov_imm(0, KEEP);
        self.a.br(16);
        self.a.bind(stored);
        self.a.cbz(true, 5, finish);
        self.hop(called, refused, enter);
        self.a.bind(called);
        self.a.push_pair(DEBT, S, 32);
        self.a.str(true, 16, SP, 16);
        self.a.arith(false, true, false, 1, S, DEBT, Shift::Lsl, 0);
        self.mask(2, 16, 0xFF);
        self.a.lsr64(3, 16, 32);
        self.a.mov64(0, CTX);
        self.call(chain);
        self.a.ldr(true, 16, SP, 16);
        self.a.pop_pair(DEBT, S, 32);
        self.a.cbz_wide(false, 0, refused);
        self.a.lsr64(6, 0, 48);
        self.a.arith(false, false, false, S, S, 6, Shift::Lsl, 0);
        self.a.ubfm(true, 16, 0, 0, 47);
        self.a.bind(enter);
        self.a.ldr(true, 4, CTX, o.core);
        self.a.ldr(false, 17, 4, o.cpsr);
        self.a.add_imm(true, false, 4, 4, o.regs);
        self.guest_regs(true, 4);
        self.mask(17, 17, NZCV_MASK);
        self.a.msr_nzcv(17);
        self.a.mov_imm(0, KEEP);
        self.a.br(16);
        self.a.bind(refused);
        self.a.mov_imm(5, 0);
        self.a.b(finish);
    }

    fn hop(&mut self, called: Label, refused: Label, enter: Label) {
        let o = self.o;
        let link = |field: usize| field as u32;
        let first = self.a.label();
        self.a.lsr64(3, 16, 32);
        self.a.cbz(false, 3, called);
        self.a.ldr(true, 6, CTX, o.slot);
        self.a.cbz_wide(false, 6, called);
        self.a.add_imm(true, false, 6, 6, SLOT_LINKS);
        self.a.tbz(false, 3, 1, first);
        self.a.add_imm(true, false, 6, 6, size_of::<Link>() as u32);
        self.a.bind(first);
        self.a.ldr(true, 7, 6, link(offset_of!(Link, slot)));
        self.a.cbz_wide(false, 7, called);
        self.a.ldr(false, 4, 6, link(offset_of!(Link, generation)));
        self.a.ldr(false, 2, 7, SLOT_GENERATION);
        self.a.cmp(4, 2);
        self.a.b_cond(0x1, called);
        self.a.ldr(true, 2, CTX, o.stall_flag);
        self.a.ldr(false, 2, 2, 0);
        self.a.cbz(true, 2, called);
        self.a.ldr(true, 4, CTX, o.core);
        if !self.v5 {
            self.a.ldrb(2, 4, o.ldm_glitch);
            self.a.cbz(true, 2, called);
        }
        self.a.add_imm(true, false, 4, 4, o.pipeline);
        self.a.pair(true, false, 1, 2, 4, 0);
        self.a.pair(true, false, 3, 4, 7, SLOT_PIPELINE as i32);
        self.a.logical(Logic::Eor, false, 1, 1, 3, Shift::Lsl, 0);
        self.a.logical(Logic::Eor, false, 2, 2, 4, Shift::Lsl, 0);
        self.a.logical(Logic::Orr, false, 1, 1, 2, Shift::Lsl, 0);
        self.a.cbz(true, 1, called);
        self.a.arith(false, true, false, 1, S, DEBT, Shift::Lsl, 0);
        self.a.ldr(false, 2, 6, link(offset_of!(Link, entry)));
        self.a.lsr(4, 2, 27);
        self.a.cmp(1, 4);
        self.a.b_cond(0xD, refused);
        self.mask(4, 16, 0xFF);
        self.a.pair(true, false, 3, 5, CTX, o.budget as i32);
        self.a.arith(false, true, false, 4, 4, 5, Shift::Lsl, 0);
        self.a.ldr(false, 5, CTX, o.acc);
        self.a.arith(false, false, false, 5, 5, 4, Shift::Lsl, 0);
        self.a.str(false, 5, CTX, o.acc);
        self.a.ldr(false, 4, 6, link(offset_of!(Link, start)));
        self.a.pair(false, false, 3, 4, CTX, o.budget as i32);
        self.a.ldr(true, 4, 6, link(offset_of!(Link, words)));
        self.a.str(true, 4, CTX, o.words);
        self.a.ldr(true, 4, 6, link(offset_of!(Link, pc)));
        self.a.str(true, 4, CTX, o.base);
        self.a.ldr(false, 4, CTX, o.chains);
        self.a.add_imm(false, false, 4, 4, 1);
        self.a.str(false, 4, CTX, o.chains);
        self.a.str(true, 7, CTX, o.slot);
        self.a.ubfx(4, 2, 16, 11);
        self.a.arith(false, false, false, S, S, 4, Shift::Lsl, 0);
        self.a.ldr(true, 16, 6, link(offset_of!(Link, jump)));
        self.a.b(enter);
    }

    fn memory(
        &mut self,
        helper: usize,
        store: bool,
        pages: bool,
        specs: &[Spec],
        leave: bool,
    ) -> Option<Label> {
        let o = self.o;
        let (slow, tail) = (self.a.label(), self.a.label());
        let tails = (self.a.label(), self.a.label());
        let out = leave.then(|| self.a.label());
        let pending = if leave && !store { LEAVE_PENDING } else { 10 };
        self.a.ubfx(3, 2, 16, pending);
        self.a.arith(false, true, false, S, S, 3, Shift::Lsl, 0);
        if pages {
            let (half, word) = (self.a.label(), self.a.label());
            if !self.v5 {
                self.a.ubfx(4, 2, CODE_CLASS, 2);
                self.a.cbz(false, 4, slow);
            }
            self.page(store, slow);
            self.a.tbz(true, 2, 1, word);
            self.a.tbz(true, 2, 0, half);
            self.paged(store, Width::Byte, tails, slow)?;
            self.a.bind(half);
            self.paged(store, Width::Half, tails, slow)?;
            self.a.bind(word);
            self.paged(store, Width::Word, tails, slow)?;
        } else if !specs.is_empty() {
            let (half, word) = (self.a.label(), self.a.label());
            self.a.tbz(true, 2, 1, word);
            self.a.tbz(true, 2, 0, half);
            self.windows(store, specs, Width::Byte, slow, tails)?;
            self.a.bind(half);
            self.windows(store, specs, Width::Half, slow, tails)?;
            self.a.bind(word);
            self.windows(store, specs, Width::Word, slow, tails)?;
        }
        self.a.bind(slow);
        self.slow_memory(helper, store);
        self.a.bind(tail);
        if self.v5 {
            self.a.ubfx(4, 2, 8, 8);
            self.mask(5, 0, CYCLES_MASK);
            self.overlap(6, 4, 5);
            self.a.arith(false, true, false, S, S, 6, Shift::Lsl, 0);
            self.settle(6, DEBT, 7, 7);
            self.a.str(false, 7, CTX, o.last);
        } else {
            self.mask(6, 0, CYCLES_MASK);
            self.a.arith(false, true, false, S, S, 6, Shift::Lsl, 0);
            if self.timed {
                self.a.ldr(false, 7, CTX, o.last);
                self.a.arith(false, false, false, 6, 6, 7, Shift::Lsl, 0);
            }
            self.a.str(false, 6, CTX, o.last);
        }
        self.a.lsr(6, 0, 31);
        self.a.str(false, 6, CTX, o.force);
        if let Some(out) = out {
            self.a.lsr64(0, 0, 32);
            self.a.cbz(true, 6, out);
            self.spent(Some(out));
        } else if self.timed {
            self.spent(None);
            self.a.logical(Logic::Orr, false, 1, 1, 6, Shift::Lsl, 0);
            self.a.lsr64(0, 0, 32);
        } else {
            self.a.mov(1, 6);
            self.a.lsr64(0, 0, 32);
        }
        self.a.msr_nzcv(3);
        self.a.ret();
        if pages && self.v5 {
            self.fast_tails(tails, out);
        } else if pages {
            self.a.bind(tails.0);
            self.a.mrs_nzcv(3);
            self.v4_cycles(!store, CODE_CLASS);
            self.v4_tail(out);
        } else if !specs.is_empty() {
            self.fast_tails(tails, out);
        }
        if let Some(out) = out {
            self.a.bind(out);
            self.leave_memory(store)?;
        }
        Some(tail)
    }

    fn leave_memory(&mut self, store: bool) -> Option<()> {
        let info = self.a.label();
        self.a.msr_nzcv(3);
        if !store {
            self.put_loaded(0, 4, info);
        }
        self.a.bind(info);
        self.a.mov_imm(16, left_word()?);
        self.a
            .logical(Logic::Orr, false, 16, 16, 2, Shift::Lsr, at_shift(self.v5));
        self.a.b(self.exit?);
        Some(())
    }

    fn put_loaded(&mut self, prefix: usize, bits: u32, done: Label) {
        if bits == 0 {
            if let Some(&rd) = GUEST.get(prefix) {
                self.a.mov(rd, 0);
            }
            self.a.b(done);
            return;
        }
        let one = self.a.label();
        self.a.tbz(true, 2, LEAVE_RD + bits - 1, one);
        self.put_loaded(2 * prefix, bits - 1, done);
        self.a.bind(one);
        self.put_loaded(2 * prefix + 1, bits - 1, done);
    }

    fn v4_cycles(&mut self, load: bool, class: u32) {
        let (code_main, other, data_fast, overlapped, done) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a.ubfx(4, 2, class, 2);
        self.a.lsl(5, 4, 3);
        self.a.add_imm(false, false, 5, 5, CODE_CYCLES);
        self.a.lsrv64(5, 7, 5);
        self.a.ubfx(5, 5, 0, 8);
        self.a.tbz(true, 4, 1, code_main);
        self.a.tbz(true, 7, MAIN, other);
        self.a.arith(false, false, false, 6, 5, 6, Shift::Lsl, 0);
        if load {
            self.a.add_imm(false, false, 6, 6, 1);
        }
        self.a.b(done);
        self.a.bind(other);
        if load {
            self.a.add_imm(false, false, 5, 5, 1);
        }
        self.a.b(overlapped);
        self.a.bind(code_main);
        self.a.tbz(false, 7, MAIN, data_fast);
        self.a.arith(false, false, false, 6, 5, 6, Shift::Lsl, 0);
        self.a.b(done);
        self.a.bind(data_fast);
        if load {
            self.a.add_imm(false, false, 6, 6, 1);
        }
        self.a.bind(overlapped);
        self.a.arith(false, false, false, 16, 5, 6, Shift::Lsl, 0);
        self.a.sub_imm(false, false, 16, 16, CODE_OVERLAP);
        self.a.cmp(5, 6);
        self.a.csel(6, 5, 6, 0x8);
        self.a.cmp(16, 6);
        self.a.csel(6, 16, 6, 0xC);
        self.a.bind(done);
    }

    fn v4_tail(&mut self, out: Option<Label>) {
        let o = self.o;
        self.a.arith(false, true, false, S, S, 6, Shift::Lsl, 0);
        self.a.str(false, 6, CTX, o.last);
        self.a.str(false, ZR, CTX, o.force);
        self.spent(out);
        self.a.msr_nzcv(3);
        self.a.ret();
    }

    fn spent(&mut self, out: Option<Label>) {
        if !self.timed {
            if out.is_none() {
                self.a.mov(1, ZR);
            }
            return;
        }
        self.a.ubfx(7, 2, 3, 5);
        self.a.arith(false, true, false, 4, S, DEBT, Shift::Lsl, 0);
        self.a.cmp(4, 7);
        match out {
            Some(out) => self.a.b_cond(0xD, out),
            None => self.a.cset(1, 0xD),
        }
    }

    fn slow_memory(&mut self, helper: usize, store: bool) {
        self.a.push_pair(8, 9, SAVED);
        self.a.pair(false, true, 10, 11, SP, 16);
        self.a.pair(false, true, 12, 13, SP, 32);
        self.a.pair(false, true, 14, 15, SP, 48);
        self.a.mrs_nzcv(3);
        self.a.pair(false, true, 3, LR, SP, 64);
        self.a.str(false, 2, SP, 80);
        if store {
            self.mask(4, 2, 7);
            self.a.mov(3, 1);
        } else {
            self.mask(3, 2, 7);
        }
        self.a.mov(2, 0);
        self.a.pair(true, true, 0, 1, CTX, self.o.core as i32);
        if !self.v5 {
            self.a.ldr(false, 5, SP, 80);
            self.store_pc(0, 5, [6, 7]);
        }
        self.call(helper);
        self.a.pair(true, true, 10, 11, SP, 16);
        self.a.pair(true, true, 12, 13, SP, 32);
        self.a.pair(true, true, 14, 15, SP, 48);
        self.a.pair(true, true, 3, LR, SP, 64);
        self.a.ldr(false, 2, SP, 80);
        self.a.pop_pair(8, 9, SAVED);
    }

    fn load_pc(
        &mut self,
        helper: usize,
        pages: bool,
        specs: &[Spec],
        returned: Label,
    ) -> Option<()> {
        let (slow, fast, flagged) = (self.a.label(), self.a.label(), self.a.label());
        self.a.ubfx(3, 2, 19, 10);
        self.a.arith(false, true, false, S, S, 3, Shift::Lsl, 0);
        if pages {
            self.page(false, slow);
            self.load(Width::Word)?;
            self.a.ubfx(1, 7, WORD_CYCLES, 8);
            self.a.b(flagged);
        } else if !specs.is_empty() {
            self.windows(false, specs, Width::Word, slow, (fast, fast))?;
        }
        self.a.bind(slow);
        self.slow_memory(helper, false);
        self.mask(1, 0, CYCLES_MASK);
        self.a.lsr64(0, 0, 32);
        self.a.msr_nzcv(3);
        if !pages && !specs.is_empty() {
            self.a.b(flagged);
            self.a.bind(fast);
            self.a.mov(1, 7);
        }
        self.a.bind(flagged);
        self.a.ubfx(3, 2, 3, 8);
        self.a.logical(Logic::Orr, false, 1, 1, 3, Shift::Lsl, 16);
        self.mask_or(1, 1 << 31);
        self.a.mov_imm(16, RETURN_INFO);
        self.a.bfm(16, 2, 11, 18);
        self.a.b(returned);
        Some(())
    }

    fn restore(&mut self, psr: Label, step: Label, exit: Label) {
        let restored = self.a.label();
        self.a.mov(7, 0);
        self.a.ubfx(5, 16, 13, 1);
        self.a
            .arith(false, true, false, 6, GUEST[14], 5, Shift::Lsl, 2);
        self.a.mov_imm(5, PSR_RESTORE);
        self.a.bl(psr);
        self.a.cbz(true, PSR_RESULT, restored);
        self.a.mov(0, 7);
        self.mask(2, 16, 0xFF);
        self.a.sub_imm(false, false, 2, 2, 1);
        self.a.lsr(3, 16, 22);
        self.a.logical(Logic::Orr, false, 2, 2, 3, Shift::Lsl, 16);
        self.a.bl(step);
        self.mask(16, 2, 0xFF);
        self.a.add_imm(false, false, 16, 16, 1);
        self.mask_or(16, STEPPED_INFO);
        self.a.b(exit);
        self.a.bind(restored);
        self.a.msr_nzcv(PSR_RESULT);
        self.a.mov(0, 6);
        self.a.ubfx(3, 16, 14, 8);
        self.mask(16, 16, !RESTORE_FIELDS);
        self.a.logical(Logic::Orr, true, 16, 16, 3, Shift::Lsl, 32);
    }

    fn fast_tails(&mut self, (short, long): (Label, Label), out: Option<Label>) {
        let o = self.o;
        let overlapped = self.a.label();
        self.a.bind(long);
        self.a.mrs_nzcv(3);
        self.a.ubfx(4, 2, 8, 8);
        self.a.sub_imm(false, true, 6, 4, DATA_OVERLAP);
        self.a.csel(6, 6, ZR, 0x8);
        self.a.arith(false, false, false, 6, 6, 7, Shift::Lsl, 0);
        self.a.b(overlapped);
        self.a.bind(short);
        self.a.mrs_nzcv(3);
        self.a.ubfx(4, 2, 8, 8);
        self.a.cmp(4, 7);
        self.a.csel(6, 4, 7, 0x8);
        self.a.bind(overlapped);
        self.a.arith(false, true, false, S, S, 6, Shift::Lsl, 0);
        self.settle(6, DEBT, 7, 7);
        self.a.str(false, 7, CTX, o.last);
        self.a.str(false, ZR, CTX, o.force);
        self.spent(out);
        self.a.msr_nzcv(3);
        self.a.ret();
    }

    fn block(
        &mut self,
        helper: usize,
        store: bool,
        pages: bool,
        specs: &[Spec],
        tail: Label,
    ) -> Option<()> {
        let slow = self.a.label();
        self.a.mrs_nzcv(3);
        self.a.ubfx(4, 2, 16, 10);
        self.a.arith(false, true, false, S, S, 4, Shift::Lsl, 0);
        if pages {
            self.span(store, slow, tail)?;
        } else if !specs.is_empty() {
            self.a.ubfx(1, 2, 26, 5);
            self.a.lsl(1, 1, 2);
            self.a.sub_imm(false, false, 1, 1, 4);
            self.a.ldr(true, 4, CTX, self.o.fast);
            for (index, spec) in specs.iter().enumerate() {
                self.window_span(index, spec, store, slow, tail)?;
            }
        }
        self.a.bind(slow);
        self.a.push_pair(8, 9, 128);
        self.a.pair(false, true, 10, 11, SP, 16);
        self.a.pair(false, true, 12, 13, SP, 32);
        self.a.pair(false, true, 14, 15, SP, 48);
        self.a.pair(false, true, 3, LR, SP, 64);
        self.a.pair(false, true, 2, 19, SP, 80);
        self.a.pair(false, true, 20, 21, SP, 96);
        self.a.str(true, 22, SP, 112);
        if !self.v5 {
            self.a.ldr(true, 5, CTX, self.o.core);
            self.a.mov(6, 2);
            self.store_pc(5, 6, [7, 16]);
        }
        self.a.mov(19, 0);
        self.a.mov_imm(20, 0);
        self.a.ubfx(21, 2, 26, 5);
        self.a.mov_imm(22, 0);
        let word = self.a.label();
        self.a.bind(word);
        self.a.cmp_imm(20, 0);
        self.a.cset(5, 0x1);
        self.a.add_imm(false, false, 16, 20, 1);
        self.a.cmp(16, 21);
        self.a.mov_imm(17, LAST);
        self.a.csel(16, 17, ZR, 0x0);
        self.a.logical(Logic::Orr, false, 5, 5, 16, Shift::Lsl, 0);
        self.a.add_imm(true, false, 17, CTX, self.o.block);
        self.a.lsl(16, 20, 2);
        if store {
            self.a.mov(4, 5);
            self.a.indexed(Indexed::LoadWord, 3, 17, 16);
        } else {
            self.a.mov(3, 5);
        }
        self.a.mov(2, 19);
        self.a.pair(true, true, 0, 1, CTX, self.o.core as i32);
        self.call(helper);
        if !store {
            self.a.lsr64(16, 0, 32);
            self.a.add_imm(true, false, 17, CTX, self.o.block);
            self.a.lsl(6, 20, 2);
            self.a.indexed(Indexed::StoreWord, 16, 17, 6);
        }
        self.mask(16, 0, EXIT as u32);
        self.a.logical(Logic::Orr, false, 22, 22, 16, Shift::Lsl, 0);
        self.mask(16, 0, CYCLES_MASK);
        self.a.arith(false, false, false, 22, 22, 16, Shift::Lsl, 0);
        self.a.add_imm(false, false, 19, 19, 4);
        self.a.add_imm(false, false, 20, 20, 1);
        self.a.cmp(20, 21);
        self.a.b_cond(0x1, word);
        self.a.mov(0, 22);
        self.a.pair(true, true, 10, 11, SP, 16);
        self.a.pair(true, true, 12, 13, SP, 32);
        self.a.pair(true, true, 14, 15, SP, 48);
        self.a.pair(true, true, 3, LR, SP, 64);
        self.a.pair(true, true, 2, 19, SP, 80);
        self.a.pair(true, true, 20, 21, SP, 96);
        self.a.ldr(true, 22, SP, 112);
        self.a.pop_pair(8, 9, 128);
        self.a.b(tail);
        Some(())
    }

    fn page(&mut self, store: bool, slow: Label) {
        self.a.ldr(true, 4, CTX, self.o.pages);
        self.a.lsr(5, 0, PAGE_SHIFT);
        self.a.arith(true, false, false, 5, 4, 5, Shift::Lsl, 5);
        self.a.pair(true, true, 6, 7, 5, 0);
        self.a.cbz(false, 7, slow);
        if store {
            self.a
                .pair(true, true, 16, 17, 5, offset_of!(Page, dirty) as i32);
            self.a
                .ubfx(4, 0, PAGE_BITS as u32, PAGE_SHIFT - PAGE_BITS as u32);
            self.a.indexed(Indexed::LoadByte, 4, 17, 4);
            self.a.cbz(true, 4, slow);
            self.a.strb(7, 16, 0);
        }
    }

    fn aligned(&mut self, width: Width) -> Option<R> {
        let bytes = match width {
            Width::Byte => return Some(0),
            Width::Half => 2,
            Width::Word => 4,
        };
        let (immr, imms) = logical_immediate(!(bytes - 1))?;
        self.a.logical_imm(Logic::And, 5, 0, immr, imms);
        Some(5)
    }

    fn load(&mut self, width: Width) -> Option<()> {
        let at = self.aligned(width)?;
        match width {
            Width::Word => {
                self.a.indexed(Indexed::LoadWord, 4, 6, at);
                self.a.lsl(5, 0, 3);
                self.a.rorv(0, 4, 5);
            }
            Width::Byte | Width::Half => {
                let (plain, signed) = match width {
                    Width::Byte => (Indexed::LoadByte, Indexed::LoadSignedByte),
                    _ => (Indexed::LoadHalf, Indexed::LoadSignedHalf),
                };
                let (unsigned, loaded) = (self.a.label(), self.a.label());
                self.a.tbz(false, 2, 2, unsigned);
                self.a.indexed(signed, 0, 6, at);
                self.a.b(loaded);
                self.a.bind(unsigned);
                self.a.indexed(plain, 0, 6, at);
                self.a.bind(loaded);
            }
        }
        Some(())
    }

    fn paged(
        &mut self,
        store: bool,
        width: Width,
        (short, long): (Label, Label),
        slow: Label,
    ) -> Option<()> {
        if !self.v5 && !store && width == Width::Half {
            self.a.tbz(true, 0, 0, slow);
        }
        if store {
            let at = self.aligned(width)?;
            let op = match width {
                Width::Byte => Indexed::StoreByte,
                Width::Half => Indexed::StoreHalf,
                Width::Word => Indexed::StoreWord,
            };
            self.a.indexed(op, 1, 6, at);
        } else {
            self.load(width)?;
        }
        let (cycles, flag) = match width {
            Width::Word => (WORD_CYCLES, LONG_WORD),
            Width::Byte | Width::Half => (HALF_CYCLES, LONG_HALF),
        };
        if !self.v5 {
            self.a.ubfx(6, 7, cycles, 8);
            self.a.b(short);
            return Some(());
        }
        let longer = self.a.label();
        self.a.tbz(true, 7, flag, longer);
        self.a.ubfx(7, 7, cycles, 8);
        self.a.b(short);
        self.a.bind(longer);
        self.a.ubfx(7, 7, cycles, 8);
        self.a.b(long);
        Some(())
    }

    fn span(&mut self, store: bool, slow: Label, tail: Label) -> Option<()> {
        let (offset_r, offset_s) = logical_immediate((PAGE_SIZE - 1) & !3)?;
        let (word_r, word_s) = logical_immediate(!3)?;
        if !self.v5 {
            self.a.ubfx(1, 2, 0, 2);
            self.a.cbz(false, 1, slow);
        }
        self.a.ldr(true, 4, CTX, self.o.pages);
        self.a.lsr(5, 0, PAGE_SHIFT);
        self.a.arith(true, false, false, 4, 4, 5, Shift::Lsl, 5);
        self.a.pair(true, true, 6, 5, 4, 0);
        self.a.cbz(false, 5, slow);
        self.a.ubfx(1, 2, 26, 5);
        self.a.lsl(1, 1, 2);
        self.a.sub_imm(false, false, 1, 1, 4);
        self.a.logical_imm(Logic::And, 16, 0, offset_r, offset_s);
        self.a.arith(false, false, false, 16, 16, 1, Shift::Lsl, 0);
        self.a.tbz(true, 16, PAGE_SHIFT, slow);
        self.a.logical_imm(Logic::And, 7, 0, word_r, word_s);
        if store {
            self.a
                .pair(true, true, 16, 17, 4, offset_of!(Page, dirty) as i32);
            let pages = PAGE_SHIFT - PAGE_BITS as u32;
            self.a.ubfx(4, 7, PAGE_BITS as u32, pages);
            self.a.indexed(Indexed::LoadByte, 4, 17, 4);
            self.a.cbz(true, 4, slow);
            self.a.arith(false, false, false, 4, 7, 1, Shift::Lsl, 0);
            self.a.ubfx(4, 4, PAGE_BITS as u32, pages);
            self.a.indexed(Indexed::LoadByte, 4, 17, 4);
            self.a.cbz(true, 4, slow);
            self.a.strb(5, 16, 0);
        }
        self.a.add_ext(6, 6, 7, 0);
        self.a.add_imm(true, false, 7, CTX, self.o.block);
        self.a.ubfx(1, 2, 26, 5);
        self.a.lsl(1, 1, 2);
        let copy = self.a.label();
        self.a.bind(copy);
        self.a.sub_imm(false, false, 1, 1, 4);
        let (from, to) = if store { (7, 6) } else { (6, 7) };
        self.a.indexed(Indexed::LoadWord, 17, from, 1);
        self.a.indexed(Indexed::StoreWord, 17, to, 1);
        self.a.cbz(true, 1, copy);
        self.a.ubfx(1, 2, 26, 5);
        self.a.sub_imm(false, false, 1, 1, 1);
        self.a.ubfx(16, 5, SEQUENTIAL_CYCLES, 8);
        self.a.ubfx(17, 5, WORD_CYCLES, 8);
        if self.v5 {
            self.a.mul(0, 1, 16, 17);
        } else {
            self.a.mul(6, 1, 16, 17);
            self.a.mov64(7, 5);
            self.v4_cycles(!store, 0);
            self.a.mov(0, 6);
        }
        self.a.b(tail);
        Some(())
    }

    fn window_span(
        &mut self,
        index: usize,
        spec: &Spec,
        store: bool,
        slow: Label,
        tail: Label,
    ) -> Option<()> {
        let next = self.a.label();
        let field = |window: usize, at: usize| {
            (offset_of!(Fast, windows) + window * size_of::<Window>() + at) as u32
        };
        let (base, size) = (offset_of!(Window, base), offset_of!(Window, size));
        self.a.ldr(false, 5, 4, field(index, base));
        self.a.ldr(true, 16, 4, field(index, size));
        self.a.arith(false, true, false, 5, 0, 5, Shift::Lsl, 0);
        self.a.arith(true, true, false, 6, 5, 16, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, next);
        self.a.arith(true, false, false, 7, 5, 1, Shift::Lsl, 0);
        self.a.arith(true, true, false, 6, 7, 16, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, slow);
        for earlier in 0..index {
            self.a.ldr(false, 16, 4, field(earlier, base));
            self.a.arith(false, false, false, 17, 0, 1, Shift::Lsl, 0);
            self.a.arith(false, true, false, 17, 17, 16, Shift::Lsl, 0);
            self.a.ldr(true, 16, 4, field(earlier, size));
            self.a.arith(true, true, false, 17, 17, 16, Shift::Lsl, 0);
            self.a.tbz(true, 17, 63, slow);
        }
        if !spec.len.is_power_of_two() {
            return None;
        }
        self.mask(5, 5, (spec.len - 1) & !3);
        self.a.arith(false, false, false, 7, 5, 1, Shift::Lsl, 0);
        self.a.lsr(16, 7, spec.len.trailing_zeros());
        self.a.cbz(true, 16, slow);
        if store {
            if spec.code {
                self.a
                    .ldr(true, 6, 4, field(index, offset_of!(Window, code)));
                for at in [5, 7] {
                    self.a.lsr(16, at, PAGE_BITS as u32);
                    self.a.indexed(Indexed::LoadByte, 16, 6, 16);
                    self.a.cbz(true, 16, slow);
                }
            }
            self.a
                .ldr(true, 6, 4, field(index, offset_of!(Window, dirty)));
            self.a.mov_imm(17, 1);
            for at in [5, 7] {
                self.a.lsr(16, at, PAGE_SHIFT);
                self.a.indexed(Indexed::StoreByte, 17, 6, 16);
            }
        }
        if let Some(cache) = spec.cache {
            if !cache.bit.is_power_of_two() {
                return None;
            }
            self.a.ldr(true, 16, 4, offset_of!(Fast, cache) as u32);
        }
        self.a
            .ldr(true, 6, 4, field(index, offset_of!(Window, bytes)));
        self.a.arith(true, false, false, 6, 6, 5, Shift::Lsl, 0);
        self.a.add_imm(true, false, 7, CTX, self.o.block);
        self.a.ubfx(1, 2, 26, 5);
        self.a.lsl(1, 1, 2);
        let copy = self.a.label();
        self.a.bind(copy);
        self.a.sub_imm(false, false, 1, 1, 4);
        let (from, to) = if store { (7, 6) } else { (6, 7) };
        self.a.indexed(Indexed::LoadWord, 17, from, 1);
        self.a.indexed(Indexed::StoreWord, 17, to, 1);
        self.a.cbz(true, 1, copy);
        self.a.ubfx(1, 2, 26, 5);
        self.a.sub_imm(false, false, 1, 1, 1);
        match spec.cache {
            None => {
                self.a.mov_imm(16, spec.word[1]);
                self.a.mov_imm(17, spec.word[0]);
                self.a.mul(5, 1, 16, 17);
            }
            Some(cache) => {
                let bit = cache.bit.trailing_zeros();
                self.a.mov(7, 0);
                self.a.mov_imm(5, 0);
                self.a.mov_imm(4, spec.word[0]);
                self.a.mov_imm(6, cache.cycles[0]);
                let (word, missed) = (self.a.label(), self.a.label());
                self.a.bind(word);
                self.a.lsr(17, 7, cache.shift);
                self.a.indexed(Indexed::LoadByte, 17, 16, 17);
                self.a.tbz(false, 17, bit, missed);
                self.a.mov(4, 6);
                self.a.bind(missed);
                self.a.arith(false, false, false, 5, 5, 4, Shift::Lsl, 0);
                self.a.mov_imm(4, spec.word[1]);
                self.a.mov_imm(6, cache.cycles[1]);
                self.a.add_imm(false, false, 7, 7, 4);
                self.a.sub_imm(false, false, 1, 1, 1);
                self.a.tbz(false, 1, 31, word);
            }
        }
        self.a.mov(0, 5);
        self.a.b(tail);
        self.a.bind(next);
        Some(())
    }

    fn windows(
        &mut self,
        store: bool,
        specs: &[Spec],
        width: Width,
        slow: Label,
        tails: (Label, Label),
    ) -> Option<()> {
        let bytes = match width {
            Width::Byte => 1,
            Width::Half => 2,
            Width::Word => 4,
        };
        self.a.ldr(true, 4, CTX, self.o.fast);
        for (index, spec) in specs.iter().enumerate() {
            let next = self.a.label();
            let window = offset_of!(Fast, windows) + index * size_of::<Window>();
            let field = |at: usize| (window + at) as u32;
            self.a
                .pair(true, true, 6, 5, 4, field(offset_of!(Window, size)) as i32);
            self.a.arith(false, true, false, 5, 0, 5, Shift::Lsl, 0);
            self.a.arith(true, true, false, 6, 5, 6, Shift::Lsl, 0);
            self.a.tbz(false, 6, 63, next);
            let (immr, imms) = logical_immediate((spec.len - 1) & !(bytes - 1))?;
            self.a.logical_imm(Logic::And, 5, 5, immr, imms);
            if store {
                if spec.code {
                    self.a.ldr(true, 6, 4, field(offset_of!(Window, code)));
                    self.a.lsr(7, 5, PAGE_BITS as u32);
                    self.a.indexed(Indexed::LoadByte, 7, 6, 7);
                    self.a.cbz(true, 7, slow);
                }
                self.a.ldr(true, 6, 4, field(offset_of!(Window, dirty)));
                self.a.lsr(7, 5, PAGE_SHIFT);
                self.a.mov_imm(16, 1);
                self.a.indexed(Indexed::StoreByte, 16, 6, 7);
                self.a.ldr(true, 6, 4, field(offset_of!(Window, bytes)));
                let op = match width {
                    Width::Byte => Indexed::StoreByte,
                    Width::Half => Indexed::StoreHalf,
                    Width::Word => Indexed::StoreWord,
                };
                self.a.indexed(op, 1, 6, 5);
            } else {
                self.a.ldr(true, 6, 4, field(offset_of!(Window, bytes)));
                match width {
                    Width::Word => {
                        self.a.indexed(Indexed::LoadWord, 6, 6, 5);
                        self.a.lsl(7, 0, 3);
                        self.a.rorv(6, 6, 7);
                    }
                    Width::Byte | Width::Half => {
                        let (plain, signed) = match width {
                            Width::Byte => (Indexed::LoadByte, Indexed::LoadSignedByte),
                            _ => (Indexed::LoadHalf, Indexed::LoadSignedHalf),
                        };
                        let (unsigned, loaded) = (self.a.label(), self.a.label());
                        self.a.tbz(false, 2, 2, unsigned);
                        self.a.indexed(signed, 6, 6, 5);
                        self.a.b(loaded);
                        self.a.bind(unsigned);
                        self.a.indexed(plain, 6, 6, 5);
                        self.a.bind(loaded);
                    }
                }
            }
            self.window_cycles(spec, width, store, tails)?;
            self.a.bind(next);
        }
        self.a.b(slow);
        Some(())
    }

    fn window_cycles(
        &mut self,
        spec: &Spec,
        width: Width,
        store: bool,
        (short, long): (Label, Label),
    ) -> Option<()> {
        let uncached = match width {
            Width::Word => spec.word[0],
            Width::Byte | Width::Half => spec.half[0],
        };
        let tail = |cycles: u32| if cycles > DATA_OVERLAP { long } else { short };
        if let Some(cache) = spec.cache {
            if !cache.bit.is_power_of_two() {
                return None;
            }
            self.a.ldr(true, 16, 4, offset_of!(Fast, cache) as u32);
            self.a.lsr(17, 0, cache.shift);
            self.a.indexed(Indexed::LoadByte, 17, 16, 17);
        }
        if store {
            self.a.mov(0, ZR);
        } else {
            self.a.mov(0, 6);
        }
        self.a.mov_imm(7, uncached);
        match spec.cache {
            Some(cache) => {
                self.a
                    .tbz(false, 17, cache.bit.trailing_zeros(), tail(uncached));
                self.a.mov_imm(7, cache.cycles[0]);
                self.a.b(tail(cache.cycles[0]));
            }
            None => self.a.b(tail(uncached)),
        }
        Some(())
    }

    fn fetch(&mut self, helper: usize, mode: Option<u32>) {
        let o = self.o;
        let known = self.a.label();
        self.a.push_pair(8, 9, SAVED);
        self.a.pair(false, true, 10, 11, SP, 16);
        self.a.pair(false, true, 12, 13, SP, 32);
        self.a.pair(false, true, 14, 15, SP, 48);
        self.a.mrs_nzcv(3);
        self.a.pair(false, true, 3, LR, SP, 64);
        self.a.cmp_imm(0, KEEP);
        self.a.b_cond(0x1, known);
        self.a.ldr(true, 3, CTX, o.core);
        self.a.ldrb(0, 3, o.next_fetch);
        self.a.bind(known);
        match mode {
            Some(mode) => self.a.add_imm(false, false, 2, 0, mode),
            None => self.a.logical(Logic::Orr, false, 2, 2, 0, Shift::Lsl, 0),
        }
        self.a.ldr(true, 0, CTX, o.bus);
        self.call(helper);
        self.a.pair(true, true, 10, 11, SP, 16);
        self.a.pair(true, true, 12, 13, SP, 32);
        self.a.pair(true, true, 14, 15, SP, 48);
        self.a.pair(true, true, 3, LR, SP, 64);
        self.a.pop_pair(8, 9, SAVED);
        self.a.arith(false, true, false, S, S, 0, Shift::Lsl, 0);
        self.a.str(false, 0, CTX, o.last);
        self.a.msr_nzcv(3);
        self.a.ret();
    }

    fn fetch_flat(&mut self, helper: usize) {
        let o = self.o;
        let known = self.a.label();
        self.a.push_pair(8, 9, SAVED);
        self.a.pair(false, true, 10, 11, SP, 16);
        self.a.pair(false, true, 12, 13, SP, 32);
        self.a.pair(false, true, 14, 15, SP, 48);
        self.a.mrs_nzcv(4);
        self.a.pair(false, true, 4, LR, SP, 64);
        self.a.cmp_imm(0, KEEP);
        self.a.b_cond(0x1, known);
        self.a.ldr(true, 3, CTX, o.core);
        self.a.ldrb(0, 3, o.next_fetch);
        self.a.bind(known);
        self.a.logical(Logic::Orr, false, 2, 2, 0, Shift::Lsl, 0);
        self.a.mov(3, S);
        self.a.ldr(true, 0, CTX, o.bus);
        self.call(helper);
        self.a.pair(true, true, 10, 11, SP, 16);
        self.a.pair(true, true, 12, 13, SP, 32);
        self.a.pair(true, true, 14, 15, SP, 48);
        self.a.pair(true, true, 4, LR, SP, 64);
        self.a.pop_pair(8, 9, SAVED);
        self.a.lsr64(3, 0, 32);
        self.a.str(false, 3, CTX, o.last);
        self.a.arith(false, true, false, S, S, 0, Shift::Lsl, 0);
        self.a.arith(false, true, false, 1, 0, 3, Shift::Lsl, 0);
        self.a.msr_nzcv(4);
        self.a.ret();
    }

    fn store_debt(&mut self, scratch: R) {
        if !self.timed {
            self.a.ldr(true, scratch, CTX, self.o.debt);
            self.a.str(false, DEBT, scratch, 0);
        }
    }

    fn step(&mut self, helper: usize) {
        let o = self.o;
        let kept = self.a.label();
        self.a.ubfx(3, 2, 16, 10);
        self.a.arith(false, true, false, S, S, 3, Shift::Lsl, 0);
        self.a.push_pair(LR, 2, 32);
        self.a.ldr(true, 1, CTX, o.core);
        self.a.add_imm(true, false, 3, 1, o.regs);
        self.guest_regs(false, 3);
        self.store_cpsr(1, 4, 5);
        self.store_debt(5);
        self.a.arith(false, true, false, 5, S, DEBT, Shift::Lsl, 0);
        self.a.str(false, 5, SP, 16);
        self.mask(4, 2, 0xFF);
        self.a.pair(true, false, 5, 6, CTX, o.base as i32);
        self.a.lslv(6, 4, 6);
        self.a.arith(false, false, false, 5, 5, 6, Shift::Lsl, 0);
        self.a.str(false, 5, 3, 60);
        self.a.ldr(true, 5, CTX, o.words);
        self.a.add_ext(5, 5, 4, 2);
        self.a.pair(true, false, 5, 6, 5, 0);
        self.a.add_imm(true, false, 4, 1, o.pipeline);
        self.a.pair(false, false, 5, 6, 4, 0);
        self.a.cmp_imm(0, KEEP);
        self.a.b_cond(0x0, kept);
        self.a.strb(0, 1, o.next_fetch);
        self.a.bind(kept);
        self.a.mov64(0, 1);
        self.a.ldr(true, 1, CTX, o.bus);
        self.call(helper);
        self.a.ldr(true, 1, CTX, o.core);
        self.a.add_imm(true, false, 3, 1, o.regs);
        self.guest_regs(true, 3);
        self.a.ldr(false, 4, 1, o.cpsr);
        self.mask(4, 4, NZCV_MASK);
        if self.timed {
            self.a.mov(DEBT, ZR);
        } else {
            self.a.ldr(true, 5, CTX, o.debt);
            self.a.ldr(false, DEBT, 5, 0);
        }
        self.a.ldr(false, 5, SP, 16);
        self.mask(6, 0, CYCLES_MASK);
        self.a.arith(false, true, false, 5, 5, 6, Shift::Lsl, 0);
        self.a.arith(false, false, false, S, 5, DEBT, Shift::Lsl, 0);
        self.a.str(false, 6, CTX, o.last);
        self.a.lsr(6, 0, 31);
        self.a.str(false, 6, CTX, o.force);
        self.a.pop_pair(LR, 2, 32);
        if self.timed {
            self.a.ubfx(7, 2, 8, 5);
            self.a.cmp(5, 7);
            self.a.cset(1, 0xD);
            self.a.logical(Logic::Orr, false, 1, 1, 6, Shift::Lsl, 0);
        } else {
            self.a.mov(1, 6);
        }
        self.a.msr_nzcv(4);
        self.a.ret();
    }

    fn psr(&mut self, helper: usize) {
        let o = self.o;
        self.a.push_pair(LR, 1, 96);
        self.a.pair(false, true, 2, 3, SP, 16);
        self.a.pair(false, true, 4, 5, SP, 32);
        self.a.pair(false, true, 6, 7, SP, 48);
        self.a.pair(false, true, 8, 15, SP, 64);
        self.a.mrs_nzcv(0);
        self.a.pair(false, true, 16, 0, SP, 80);
        self.a.ldr(true, 0, CTX, o.core);
        self.a.add_imm(true, false, 6, 0, o.regs);
        self.guest_regs(false, 6);
        self.store_cpsr(0, 6, 7);
        self.a.mov(1, 3);
        self.a.mov(2, 4);
        self.a.mov(3, 5);
        self.a.ldr(true, 4, CTX, o.bus);
        self.call(helper);
        self.a.mov(PSR_RESULT, 0);
        self.a.ldr(true, 1, CTX, o.core);
        self.a.add_imm(true, false, 2, 1, o.regs);
        self.guest_regs(true, 2);
        self.a.pair(true, true, 16, 0, SP, 80);
        self.a.msr_nzcv(0);
        self.a.pair(true, true, 8, 15, SP, 64);
        self.a.pair(true, true, 6, 7, SP, 48);
        self.a.pair(true, true, 4, 5, SP, 32);
        self.a.pair(true, true, 2, 3, SP, 16);
        self.a.pop_pair(LR, 1, 96);
        self.a.ret();
    }
}
