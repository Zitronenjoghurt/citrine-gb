use super::super::super::QuickFetch;
use super::super::super::fast::{
    CODE_CYCLES, Fast, HALF_CYCLES, HALF_SEQUENTIAL, LONG_AFTER, LONG_HALF, LONG_WORD, MAIN,
    PAGE_SHIFT, PAGE_SIZE, Page, SEQUENTIAL_CYCLES, Spec, WORD_CYCLES, Window,
};
use super::super::super::helpers::{
    EXIT, IDLES_SHIFT, LAST, RUN_COUNT_MASK, RUN_CYCLES_SHIFT, THUMB,
};
use super::super::super::lookup::{
    CELL_BASE, HASH, HASH_SHIFT, LINK_SHIFT, Link, NO_KEY, SLOT_CODE, SLOT_GENERATION,
    SLOT_PIPELINE, SLOT_SIZE, SLOT_START, SLOT_TAG, SLOT_WORDS,
};
use super::super::super::marks::PAGE_BITS;
use super::super::super::scan::Width;
use super::super::{
    BIOS_FETCH, CODE_CLASS, IWRAM_FETCH, IWRAM_IDLES, IWRAM_RUN, Offsets, PSR_RESTORE,
    RESTORE_FIELDS, ROM_LITERAL, Runtime, STUBS, Setup, Stub,
};
use super::asm::{Asm, Indexed, LR, Label, Logic, R, SP, Shift, ZR, logical_immediate};
use super::{
    CTX, DEBT, GUEST, KEEP, LEAVE_PENDING, LEAVE_RD, OLD, PSR_RESULT, S, SEQUENTIAL, at_shift,
    left_word,
};
use crate::gba::cpu::psr::{FLAG_I, FLAG_T};
use std::mem::{offset_of, size_of};

#[derive(Clone, Copy)]
enum Field {
    Byte,
    Word,
    Wide,
}

const FRAME: i32 = 112;
const FLAGS: u32 = 96;
const SAVED: i32 = 96;
const NZCV_MASK: u32 = 0xF000_0000;
const CYCLES_MASK: u32 = EXIT as u32 - 1;
pub(super) const DATA_OVERLAP: u32 = 6;
const CODE_OVERLAP: u32 = 3;
const RETURN_INFO: u32 = 0x1A00;
const STEPPED_INFO: u32 = 0x1E00;
const RESTARTED: u32 = 0x0101;
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

struct Lookup {
    pc: R,
    tmp: R,
    key: R,
    entry: R,
    start: R,
    pipe: [R; 2],
}

const FIND_STORED: Lookup = Lookup {
    pc: 9,
    tmp: 10,
    key: 11,
    entry: 12,
    start: 13,
    pipe: [13, 14],
};

const FIND_LIVE: Lookup = Lookup {
    pc: 0,
    tmp: 5,
    key: 17,
    entry: LR,
    start: 4,
    pipe: [17, 5],
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
    let tail = build.memory(helpers.load, false, setup.pages, windows, false, true)?;
    let swap = build.a.label();
    build.a.b(swap);
    let store = build.here();
    build.memory(helpers.store, true, setup.pages, windows, false, true)?;
    let load_leave = build.here();
    build.memory(helpers.load, false, setup.pages, windows, true, true)?;
    let store_leave = build.here();
    build.memory(helpers.store, true, setup.pages, windows, true, true)?;
    let gather = build.here();
    build.block(helpers.load_word, false, setup.pages, windows, tail, true)?;
    let scatter = build.here();
    build.block(helpers.store_word, true, setup.pages, windows, tail, true)?;
    let collect = build.here();
    let collected = build.a.label();
    build.block(
        helpers.load_word,
        false,
        setup.pages && setup.v5,
        windows,
        collected,
        false,
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
    build.memory(helpers.swap, true, false, &[], false, false)?;
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

    fn keep_flags(&mut self, core: R, cpsr: R, scratch: R) {
        self.a.ldr(false, cpsr, core, self.o.cpsr);
        self.a.mrs_nzcv(scratch);
        self.a.str(false, scratch, SP, FLAGS);
    }

    fn merge_flags(&mut self, core: R, cpsr: R, scratch: R) {
        self.a.ldr(false, cpsr, core, self.o.cpsr);
        self.a.ldr(false, scratch, SP, FLAGS);
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
        if live.is_some() {
            self.keep_flags(0, 2, 3);
        } else {
            self.store_cpsr(0, 2, 3);
        }
        let finish = live.unwrap_or(finish);
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
        self.keep_flags(1, 3, 4);
        self.a.ubfx(4, 16, 22, 10);
        self.a.arith(false, true, false, S, S, 4, Shift::Lsl, 0);
        let (slow, called) = (self.a.label(), self.a.label());
        let refills = self.refills(leave, specs);
        let linked = refills && self.linked(leave);
        if linked {
            let normal = self.a.label();
            self.hop_linked(leave == Leave::Exchange, normal);
            self.a.bind(normal);
        }
        if refills {
            self.refetch(leave, specs, slow, called, LIVE)?;
        } else if let Some(q) = self.quick_leave(leave) {
            self.quick_branch(&q, leave, slow, called)?;
        }
        self.a.bind(slow);
        self.merge_flags(1, 3, 4);
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
        if linked {
            self.a.str(false, 0, CTX, o.hop + 4);
            self.a.ubfx(2, 3, FLAG_T.trailing_zeros(), 1);
            self.a.logical(Logic::Orr, false, 2, 2, 6, Shift::Lsl, 0);
            self.a.str(false, 2, CTX, o.hop);
        }
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
        } else if let Some(q) = self.quick_leave(leave) {
            self.quick_branch(&q, leave, slow, called)?;
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

    fn quick_leave(&self, leave: Leave) -> Option<QuickFetch> {
        let branches = matches!(leave, Leave::Branch | Leave::Exchange);
        self.o
            .quick
            .filter(|_| branches && self.timed && !self.v5 && !self.pages)
    }

    fn quick_branch(
        &mut self,
        q: &QuickFetch,
        leave: Leave,
        slow: Label,
        called: Label,
    ) -> Option<()> {
        let thumb = self.a.label();
        if leave == Leave::Exchange {
            self.a.tbz(true, 0, 0, thumb);
        } else {
            self.a.tbz(true, 3, FLAG_T.trailing_zeros(), thumb);
        }
        self.mask(6, 0, 3);
        self.a.cbz(true, 6, slow);
        self.a.mov(6, 0);
        let (bios, landed) = (self.a.label(), self.a.label());
        self.quick_iwram_arm(q, bios, slow);
        self.a.bind(landed);
        self.quick_landed(leave, 4, called);
        self.a.bind(bios);
        self.quick_bios_arm(q, slow);
        self.a.b(landed);
        self.a.bind(thumb);
        self.mask(6, 0, !1);
        self.quick_rom_thumb(q, slow)?;
        self.quick_landed(leave, 2, called);
        Some(())
    }

    fn quick_iwram_arm(&mut self, q: &QuickFetch, other: Label, slow: Label) {
        let o = self.o;
        self.a.lsr(5, 6, 24);
        self.a.cmp_imm(5, q.iwram_page);
        self.a.b_cond(0x1, other);
        self.a.add_imm(false, false, 5, 6, 4);
        self.a.ubfx(5, 5, 0, PAGE_SHIFT);
        self.a.cbz(false, 5, slow);
        self.a.ldr(true, 5, CTX, o.fast);
        self.a.ldr(true, 5, 5, offset_of!(Fast, pages) as u32);
        self.a.cbz_wide(false, 5, slow);
        self.a.lsr(7, 6, PAGE_SHIFT);
        self.a.arith(true, false, false, 5, 5, 7, Shift::Lsl, 5);
        self.a.pair(true, true, 7, 2, 5, 0);
        self.a.cbz(false, 2, slow);
        self.quick_arm_words(q, q.open_iwram, slow);
    }

    fn quick_bios_arm(&mut self, q: &QuickFetch, slow: Label) {
        let o = self.o;
        self.a.mov_imm(5, q.bios_end - 8);
        self.a.cmp(6, 5);
        self.a.b_cond(0x8, slow);
        self.a.ldr(true, 7, CTX, o.fast);
        self.a.ldr(true, 7, 7, offset_of!(Fast, bios) as u32);
        self.a.cbz_wide(false, 7, slow);
        self.quick_arm_words(q, q.open_bios, slow);
    }

    fn quick_arm_words(&mut self, q: &QuickFetch, latch: i32, slow: Label) {
        let o = self.o;
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.indexed(Indexed::LoadWord, 2, 7, 6);
        self.a.add_imm(false, false, 5, 6, 4);
        self.a.indexed(Indexed::LoadWord, LR, 7, 5);
        self.a.ldr(true, 4, CTX, o.bus);
        self.bus_field(Field::Word, false, 5, q.now);
        self.bus_field(Field::Word, false, 7, q.dma_check);
        self.a.arith(true, true, false, 7, 7, 5, Shift::Lsl, 0);
        self.a.sub_imm(true, false, 7, 7, 2);
        self.a.tbz(true, 7, 63, slow);
        self.bus_field(Field::Byte, false, 7, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 7, bit % 8, slow);
        self.a.add_imm(false, false, 5, 5, 2);
        self.bus_field(Field::Word, true, 5, q.now);
        self.bus_field(Field::Word, true, 5, q.sample);
        self.bus_field(Field::Wide, false, 7, q.clock);
        self.a.add_imm(true, false, 7, 7, 2);
        self.bus_field(Field::Wide, true, 7, q.clock);
        self.bus_field(Field::Byte, true, ZR, q.branched);
        self.a.add_imm(false, false, 5, 6, 4);
        self.bus_field(Field::Word, true, 5, q.open_addr);
        self.bus_field(Field::Word, true, LR, latch);
        self.bus_field(Field::Word, true, LR, q.open_last);
        self.a.str(false, 2, 1, o.pipeline);
        self.a.str(false, LR, 1, o.pipeline + 4);
        self.a.mov_imm(2, 2);
    }

    fn quick_rom_thumb(&mut self, q: &QuickFetch, slow: Label) -> Option<()> {
        let o = self.o;
        let bit = q.prefetch_bit.trailing_zeros();
        let page = q.rom_page - 1;
        let (end_r, end_s) = logical_immediate(page & !3)?;
        let (mirror_r, mirror_s) = logical_immediate(q.rom_mirror)?;
        let (page_r, page_s) = logical_immediate(page)?;
        let (top, done, none, glitched) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a.mov_imm(5, q.rom_start);
        self.a.arith(false, true, false, 5, 6, 5, Shift::Lsl, 0);
        self.a.mov_imm(7, q.rom_span);
        self.a.cmp(5, 7);
        self.a.b_cond(0x2, slow);
        self.a.add_imm(false, false, 5, 6, 4);
        self.a.logical_imm(Logic::And, 5, 5, end_r, end_s);
        self.a.cbz(false, 5, slow);
        self.a.logical_imm(Logic::And, 5, 6, mirror_r, mirror_s);
        self.a.sub_imm(false, false, 7, 5, q.rom_skip);
        self.a.add_imm(true, false, 7, 7, 2);
        self.a.ldr(true, 4, CTX, o.bus);
        self.bus_field(Field::Wide, false, 2, q.plain_rom_end);
        self.a.arith(true, true, false, 7, 7, 2, Shift::Lsl, 0);
        self.a.tbz(false, 7, 63, slow);
        self.bus_field(Field::Byte, false, 7, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 7, bit % 8, slow);
        self.bus_field(Field::Word, false, 7, q.head);
        self.a.cmp(7, 6);
        self.a.b_cond(0x0, slow);
        self.bus_field(Field::Byte, false, 7, q.stuck);
        self.a.cbz(true, 7, slow);
        self.bus_field(Field::Byte, false, 7, q.flowing);
        self.a.cbz(false, 7, none);
        self.bus_field(Field::Wide, false, 5, q.clock);
        self.bus_field(Field::Word, false, 7, q.cost);
        self.bus_field(Field::Word, false, 3, q.progress);
        self.a.arith(false, true, false, 7, 7, 3, Shift::Lsl, 0);
        self.bus_field(Field::Wide, false, LR, q.since);
        self.a.arith(true, false, false, 7, LR, 7, Shift::Lsl, 0);
        self.bus_field(Field::Word, false, 3, q.count);
        self.bus_field(Field::Word, false, LR, q.head);
        self.a.bind(top);
        self.a.arith(true, true, true, ZR, 5, 7, Shift::Lsl, 0);
        self.a.b_cond(0x3, done);
        self.a.add_imm(false, false, 3, 3, 1);
        self.a.cmp_imm(3, q.capacity);
        self.a.b_cond(0x0, none);
        self.a.b_cond(0x8, slow);
        self.a.arith(false, false, false, 2, LR, 3, Shift::Lsl, 1);
        self.a.logical_imm(Logic::And, 2, 2, page_r, page_s);
        self.a.cbz(false, 2, slow);
        self.bus_field(Field::Word, false, 2, q.cost);
        self.a.arith(true, false, false, 7, 7, 2, Shift::Lsl, 0);
        self.a.b(top);
        self.a.bind(done);
        self.a.arith(true, true, false, 7, 7, 5, Shift::Lsl, 0);
        self.a.sub_imm(true, true, ZR, 7, 1);
        self.a.cset(2, 0x0);
        self.a.b(glitched);
        self.a.bind(none);
        self.a.mov_imm(2, 0);
        self.a.bind(glitched);
        self.a.ubfx(7, 6, 25, 2);
        self.bus_byte(5, 7, q.first_halves);
        self.a.arith(false, false, false, 2, 2, 5, Shift::Lsl, 0);
        self.bus_field(Field::Word, false, 5, q.now);
        self.a.arith(true, false, false, LR, 5, 2, Shift::Lsl, 0);
        self.bus_field(Field::Word, false, 5, q.dma_check);
        self.a.arith(true, true, false, 5, 5, LR, Shift::Lsl, 0);
        self.a.sub_imm(true, false, 5, 5, 1);
        self.a.tbz(true, 5, 63, slow);
        self.a.ldr(true, 7, CTX, o.fast);
        self.a.ldr(true, 7, 7, offset_of!(Fast, rom) as u32);
        self.a.cbz_wide(false, 7, slow);
        self.a.logical_imm(Logic::And, 5, 6, mirror_r, mirror_s);
        self.a.indexed(Indexed::LoadHalf, 3, 7, 5);
        self.a.add_imm(false, false, 5, 5, 2);
        self.a.indexed(Indexed::LoadHalf, 5, 7, 5);
        self.a.str(false, 3, 1, o.pipeline);
        self.a.str(false, 5, 1, o.pipeline + 4);
        self.a.logical(Logic::Orr, false, 5, 5, 5, Shift::Lsl, 16);
        self.bus_field(Field::Word, true, 5, q.open_last);
        self.a.add_imm(false, false, 5, 6, 2);
        self.bus_field(Field::Word, true, 5, q.open_addr);
        self.a.ubfx(7, 6, 25, 2);
        self.bus_byte(3, 7, q.sequential_halves);
        self.a.add_imm(false, false, 5, LR, 1);
        self.bus_field(Field::Word, true, 5, q.sample);
        self.a.arith(false, false, false, 5, LR, 3, Shift::Lsl, 0);
        self.bus_field(Field::Word, true, 5, q.now);
        self.bus_field(Field::Word, true, 3, q.cost);
        self.bus_field(Field::Wide, false, 5, q.clock);
        self.bus_field(Field::Wide, true, 5, q.since);
        self.a.arith(true, false, false, 5, 5, 3, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 5, q.due);
        self.a.logical_imm(Logic::Orr, 5, 6, page_r, page_s);
        self.a.add_imm(false, false, 5, 5, 1);
        self.a.ubfx(5, 5, 25, 2);
        self.bus_byte(5, 5, q.sequential_halves);
        self.bus_field(Field::Word, true, 5, q.page_cost);
        self.a.add_imm(false, false, 5, 6, 4);
        self.bus_field(Field::Word, true, 5, q.head);
        self.restart(q, 5);
        self.a.arith(false, false, false, 2, 2, 3, Shift::Lsl, 0);
        Some(())
    }

    fn quick_landed(&mut self, leave: Leave, step: u32, called: Label) {
        let o = self.o;
        self.a.ldr(false, 3, 1, o.cpsr);
        if leave == Leave::Exchange {
            if step == 2 {
                self.mask_or(3, FLAG_T);
            } else {
                self.mask(3, 3, !FLAG_T);
            }
        }
        self.a.ldr(false, 5, 1, o.data);
        self.a.arith(false, false, false, 2, 2, 5, Shift::Lsl, 0);
        for field in [o.fetched, o.data, o.refetched] {
            self.a.str(false, ZR, 1, field);
        }
        self.refilled(leave, step, called, LIVE);
    }

    fn bus_byte(&mut self, rt: R, index: R, offset: i32) {
        let offset = offset as u32;
        if offset < 0x1000 {
            self.a
                .arith(true, false, false, 17, 4, index, Shift::Lsl, 0);
            self.a.ldrb(rt, 17, offset);
        } else {
            self.a.mov_imm(17, offset);
            self.a.arith(true, false, false, 17, 4, 17, Shift::Lsl, 0);
            self.a.indexed(Indexed::LoadByte, rt, 17, index);
        }
    }

    fn refills(&self, leave: Leave, specs: &[Spec]) -> bool {
        if self.v5 || !self.pages {
            specs.iter().any(|spec| spec.code)
        } else {
            leave != Leave::Return
        }
    }

    fn linked(&self, leave: Leave) -> bool {
        matches!(leave, Leave::Branch | Leave::Exchange) && (self.v5 || self.pages && !self.timed)
    }

    fn hop_linked(&mut self, exchange: bool, normal: Label) {
        let o = self.o;
        let link = |field: usize| field as u32;
        self.a.lsr64(5, 16, 32);
        self.a.cbz(false, 5, normal);
        self.a.ldr(true, 6, CTX, o.slot);
        self.a
            .arith(true, false, false, 6, 6, 5, Shift::Lsl, LINK_SHIFT);
        self.a.sub_imm(true, false, 6, 6, -CELL_BASE as u32);
        self.a.ldr(true, 7, 6, link(offset_of!(Link, slot)));
        self.a.cbz_wide(false, 7, normal);
        self.a.ldr(false, 4, 6, link(offset_of!(Link, generation)));
        self.a.ldr(false, 2, 7, SLOT_GENERATION);
        self.a.cmp(4, 2);
        self.a.b_cond(0x1, normal);
        if exchange {
            self.a.ldr(false, 2, 7, SLOT_TAG);
            self.a.logical(Logic::Eor, false, 2, 2, 0, Shift::Lsl, 0);
            self.a.cmp_imm(2, NO_KEY);
            self.a.b_cond(0x1, normal);
        }
        self.a.ldr(true, 2, CTX, o.stall_flag);
        self.a.ldr(false, 2, 2, 0);
        self.a.cbz(true, 2, normal);
        if !self.v5 {
            self.a.ldrb(2, 1, o.ldm_glitch);
            self.a.cbz(true, 2, normal);
        }
        self.a.ldr(false, 2, 6, link(offset_of!(Link, cost)));
        self.a.add_imm(false, false, 4, 2, 1);
        self.a.cbz(false, 4, normal);
        self.a.arith(false, true, false, 17, S, 2, Shift::Lsl, 0);
        if self.timed {
            self.a.ldr(false, 5, CTX, o.last);
            self.a.arith(false, false, false, 2, 2, 5, Shift::Lsl, 0);
        }
        self.a.mov(4, DEBT);
        self.settle(2, 4, 5, 2);
        self.a.ldr(false, 5, 6, link(offset_of!(Link, ahead)));
        self.a.arith(false, false, false, 5, 5, 4, Shift::Lsl, 0);
        self.a.cmp(17, 5);
        self.a.b_cond(0xD, normal);
        self.a.mov(S, 17);
        self.a.mov(DEBT, 4);
        self.a.str(false, 2, 1, o.cycles);
        self.a.strb(ZR, 1, o.irq_held);
        self.store_debt(5);
        if exchange {
            self.mask(5, 0, 1);
            self.mask(3, 3, !FLAG_T);
            self.a.logical(
                Logic::Orr,
                false,
                3,
                3,
                5,
                Shift::Lsl,
                FLAG_T.trailing_zeros(),
            );
            self.a.str(false, 3, 1, o.cpsr);
            self.mask(0, 0, !1);
        }
        self.a.ldr(false, 5, 6, link(offset_of!(Link, pc) + 4));
        self.a.arith(false, false, false, 5, 0, 5, Shift::Lsl, 2);
        self.a.str(false, 5, 1, o.regs + 60);
        if !self.timed {
            self.a.str(false, 0, 1, o.landed);
        }
        self.a.mov_imm(5, SEQUENTIAL);
        self.a.strb(5, 1, o.next_fetch);
        self.a.ubfx(5, 3, FLAG_I.trailing_zeros(), 1);
        self.a.strb(5, 1, o.irq_masked);
        self.a.pair(true, false, 4, 5, 7, SLOT_PIPELINE as i32);
        self.a.add_imm(true, false, 17, 1, o.pipeline);
        self.a.pair(false, false, 4, 5, 17, 0);
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
        self.a.ldr(false, 4, 6, link(offset_of!(Link, pending)));
        self.a.arith(false, false, false, S, S, 4, Shift::Lsl, 0);
        self.a.ldr(true, 16, 6, link(offset_of!(Link, jump)));
        self.a.ldr(false, 17, SP, FLAGS);
        self.a.msr_nzcv(17);
        self.a.mov_imm(0, KEEP);
        self.a.br(16);
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
        let windows = specs.iter().enumerate();
        let order: Vec<_> = if self.v5 {
            windows.rev().collect()
        } else {
            windows.collect()
        };
        for (index, spec) in order {
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
        let (rust, to_rust) = (self.a.label(), self.a.label());
        self.a.bind(live);
        self.a.cbz(true, 5, to_finish);
        self.hop(to_call, to_refuse, jump);
        self.a.bind(to_call);
        self.find(&FIND_LIVE, to_rust, to_refuse, jump);
        for (from, to) in [(to_rust, rust), (to_refuse, refused), (to_finish, finish)] {
            self.a.bind(from);
            self.a.ldr(true, 4, CTX, o.core);
            self.merge_flags(4, 2, 3);
            self.a.add_imm(true, false, 4, 4, o.regs);
            self.guest_regs(false, 4);
            self.a.b(to);
        }
        self.a.bind(jump);
        self.a.ldr(false, 17, SP, FLAGS);
        self.a.msr_nzcv(17);
        self.a.mov_imm(0, KEEP);
        self.a.br(16);
        self.a.bind(stored);
        self.a.cbz(true, 5, finish);
        self.hop(called, refused, enter);
        self.a.bind(called);
        self.find(&FIND_STORED, rust, refused, enter);
        self.a.bind(rust);
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

    fn find(&mut self, r: &Lookup, called: Label, refused: Label, enter: Label) {
        let o = self.o;
        self.a.lsr64(3, 16, 32);
        self.a.cbz(true, 3, called);
        self.a.ldr(true, 2, CTX, o.stall_flag);
        self.a.cbz_wide(false, 2, called);
        self.a.ldr(false, 2, 2, 0);
        self.a.cbz(true, 2, called);
        self.a.ldr(true, 6, CTX, o.slots);
        self.a.cbz_wide(false, 6, called);
        self.a.ldr(true, 4, CTX, o.core);
        if !self.v5 {
            self.a.ldrb(2, 4, o.ldm_glitch);
            self.a.cbz(true, 2, refused);
        }
        self.a.ldr(false, 3, 4, o.cpsr);
        self.a.ubfx(3, 3, FLAG_T.trailing_zeros(), 1);
        self.a.ldr(false, r.pc, 4, o.regs + 60);
        self.a.mov_imm(r.tmp, 8);
        self.a.lsrv64(r.tmp, r.tmp, 3);
        self.a
            .arith(false, true, false, r.key, r.pc, r.tmp, Shift::Lsl, 0);
        self.a.mov_imm(r.tmp, 3);
        self.a.lsrv64(r.tmp, r.tmp, 3);
        self.a.tst(r.key, r.tmp);
        self.a.b_cond(0x1, refused);
        self.a
            .logical(Logic::Orr, false, r.key, r.key, 3, Shift::Lsl, 0);
        self.a.mov_imm(r.tmp, HASH);
        self.a.mul(r.entry, r.key, r.tmp, ZR);
        self.a.lsr(r.entry, r.entry, HASH_SHIFT);
        self.a.mov_imm(r.tmp, SLOT_SIZE);
        self.a.mul(r.entry, r.entry, r.tmp, ZR);
        self.a
            .arith(true, false, false, 6, 6, r.entry, Shift::Lsl, 0);
        self.a.ldr(false, r.tmp, 6, SLOT_TAG);
        self.a
            .logical(Logic::Eor, false, r.tmp, r.tmp, r.key, Shift::Lsl, 0);
        self.a.cmp_imm(r.tmp, NO_KEY);
        self.a.b_cond(0x1, refused);
        self.a.add_imm(true, false, 7, 4, o.pipeline);
        self.a.pair(true, false, 1, 2, 7, 0);
        self.a
            .pair(true, false, r.pipe[0], r.pipe[1], 6, SLOT_PIPELINE as i32);
        self.a
            .logical(Logic::Eor, false, 1, 1, r.pipe[0], Shift::Lsl, 0);
        self.a
            .logical(Logic::Eor, false, 2, 2, r.pipe[1], Shift::Lsl, 0);
        self.a.logical(Logic::Orr, false, 1, 1, 2, Shift::Lsl, 0);
        self.a.cbz(true, 1, refused);
        self.a.ldr(true, 7, 6, SLOT_CODE);
        self.a.tbz(false, 7, 0, refused);
        self.a.sub_imm(true, false, 7, 7, 1);
        self.a.ldr(false, r.start, 6, SLOT_START);
        self.a.add_ext(r.tmp, 7, r.start, 2);
        self.a.ldr(false, r.entry, r.tmp, 0);
        self.a.arith(false, true, false, 1, S, DEBT, Shift::Lsl, 0);
        self.a.lsr(2, r.entry, 27);
        self.a.cmp(1, 2);
        self.a.b_cond(0xD, refused);
        self.a.str(true, 6, CTX, o.slot);
        self.mask(2, 16, 0xFF);
        self.a.ldr(false, 5, CTX, o.budget + 4);
        self.a.arith(false, true, false, 2, 2, 5, Shift::Lsl, 0);
        self.a.ldr(false, 5, CTX, o.acc);
        self.a.arith(false, false, false, 5, 5, 2, Shift::Lsl, 0);
        self.a.str(false, 5, CTX, o.acc);
        self.a.str(false, r.start, CTX, o.budget + 4);
        self.a.ldr(true, 2, 6, SLOT_WORDS);
        self.a.str(true, 2, CTX, o.words);
        self.a.mov_imm(2, 2);
        self.a.arith(false, true, false, 2, 2, 3, Shift::Lsl, 0);
        self.a.lslv(5, r.start, 2);
        self.a
            .arith(false, true, false, r.pc, r.pc, 5, Shift::Lsl, 0);
        self.a.pair(false, false, r.pc, 2, CTX, o.base as i32);
        self.a.ldr(false, 2, CTX, o.chains);
        self.a.add_imm(false, false, 2, 2, 1);
        self.a.str(false, 2, CTX, o.chains);
        self.a.ubfx(2, r.entry, 16, 11);
        self.a.arith(false, false, false, S, S, 2, Shift::Lsl, 0);
        self.mask(2, r.entry, 0xFFFF);
        self.a.add_ext(16, 7, 2, 2);
        self.a.b(enter);
    }

    fn hop(&mut self, called: Label, refused: Label, enter: Label) {
        let o = self.o;
        let link = |field: usize| field as u32;
        self.a.lsr64(3, 16, 32);
        self.a.cbz(false, 3, called);
        self.a.ldr(true, 6, CTX, o.slot);
        self.a
            .arith(true, false, false, 6, 6, 3, Shift::Lsl, LINK_SHIFT);
        self.a.sub_imm(true, false, 6, 6, -CELL_BASE as u32);
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
        ram: bool,
    ) -> Option<Label> {
        let o = self.o;
        let (slow, tail) = (self.a.label(), self.a.label());
        let tails = (self.a.label(), self.a.label());
        let out = leave.then(|| self.a.label());
        let pending = if leave && !store { LEAVE_PENDING } else { 10 };
        self.a.ubfx(3, 2, 16, pending);
        self.a.arith(false, true, false, S, S, 3, Shift::Lsl, 0);
        let quick_ram = match o.quick {
            Some(q) if ram && !self.v5 && !pages && specs.is_empty() => Some(q),
            _ => None,
        };
        let ram_tail = match quick_ram {
            Some(q) => Some(self.ram(&q, store, slow)?),
            None => None,
        };
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
        if let Some(ram_tail) = ram_tail {
            self.a.bind(ram_tail);
            self.ram_spent(out);
        }
        if let Some(out) = out {
            self.a.bind(out);
            self.leave_memory(store)?;
        }
        Some(tail)
    }

    fn ram(&mut self, q: &QuickFetch, store: bool, slow: Label) -> Option<Label> {
        let tail = self.a.label();
        let (half, word) = (self.a.label(), self.a.label());
        let rom = (!store && self.timed).then(|| self.a.label());
        if let Some(rom) = rom {
            debug_assert!(q.rom_start.is_power_of_two());
            self.a.tbz(true, 0, q.rom_start.trailing_zeros(), rom);
        }
        self.a.ldr(true, 6, CTX, self.o.fast);
        self.a.ldr(true, 6, 6, offset_of!(Fast, pages) as u32);
        self.a.cbz_wide(false, 6, slow);
        self.a.lsr(5, 0, PAGE_SHIFT);
        self.a.arith(true, false, false, 5, 6, 5, Shift::Lsl, 5);
        self.a.pair(true, true, 6, 7, 5, 0);
        self.a.cbz(false, 7, slow);
        if store {
            self.a
                .pair(true, true, 16, 17, 5, offset_of!(Page, dirty) as i32);
            self.a
                .ubfx(3, 0, PAGE_BITS as u32, PAGE_SHIFT - PAGE_BITS as u32);
            self.a.indexed(Indexed::LoadByte, 3, 17, 3);
            self.a.cbz(true, 3, slow);
        }
        self.a.ldr(true, 4, CTX, self.o.bus);
        if store {
            for hit in q.code_hits {
                self.bus_field(Field::Byte, false, 3, hit);
                self.a.cbz(true, 3, slow);
            }
            self.bus_field(Field::Word, false, 3, q.now);
            self.bus_field(Field::Word, false, 5, q.dma_check);
            self.a.arith(true, true, false, 5, 5, 3, Shift::Lsl, 0);
            self.a.sub_imm(true, false, 5, 5, 1);
            self.a.tbz(true, 5, 63, slow);
        }
        self.a.tbz(true, 2, 1, word);
        self.a.tbz(true, 2, 0, half);
        self.ram_access(q, store, Width::Byte, slow, tail)?;
        self.a.bind(half);
        self.ram_access(q, store, Width::Half, slow, tail)?;
        self.a.bind(word);
        self.ram_access(q, store, Width::Word, slow, tail)?;
        if let Some(rom) = rom {
            self.a.bind(rom);
            self.rom(q, slow, tail)?;
        }
        Some(tail)
    }

    fn ram_access(
        &mut self,
        q: &QuickFetch,
        store: bool,
        width: Width,
        slow: Label,
        tail: Label,
    ) -> Option<()> {
        let shift = match width {
            Width::Word => WORD_CYCLES,
            Width::Byte | Width::Half => HALF_CYCLES,
        };
        if !store && width == Width::Half {
            self.a.tbz(true, 0, 0, slow);
        }
        self.a.ubfx(7, 7, shift, 8);
        if store {
            self.a.mov_imm(3, 1);
            self.a.strb(3, 16, 0);
        } else {
            self.bus_field(Field::Word, false, 3, q.now);
            self.bus_field(Field::Word, false, 16, q.dma_check);
            self.a.arith(true, true, false, 16, 16, 3, Shift::Lsl, 0);
            self.a.arith(true, true, false, 16, 16, 7, Shift::Lsl, 0);
            self.a.sub_imm(true, false, 16, 16, 1);
            self.a.tbz(true, 16, 63, slow);
            self.a.add_imm(false, false, 7, 7, 1);
            self.a.arith(false, false, false, 3, 3, 7, Shift::Lsl, 0);
            self.bus_field(Field::Word, true, 3, q.now);
            self.bus_field(Field::Word, true, 3, q.sample);
        }
        let at = self.aligned(width)?;
        let op = match (store, width) {
            (true, Width::Byte) => Indexed::StoreByte,
            (true, Width::Half) => Indexed::StoreHalf,
            (true, Width::Word) => Indexed::StoreWord,
            (false, Width::Byte) => Indexed::LoadByte,
            (false, Width::Half) => Indexed::LoadHalf,
            (false, Width::Word) => Indexed::LoadWord,
        };
        let value = if store { 1 } else { 3 };
        self.a.indexed(op, value, 6, at);
        let (ewram, merged) = (self.a.label(), self.a.label());
        let lane = match width {
            Width::Byte => Some(3),
            Width::Half => Some(2),
            Width::Word => None,
        };
        if let Some(lane) = lane {
            let (immr, imms) = logical_immediate(lane)?;
            self.a.logical_imm(Logic::And, 5, 0, immr, imms);
        }
        self.a.tbz(false, 0, 24, ewram);
        self.open_lane(width, q.open_iwram, value);
        if !store {
            if width == Width::Word {
                self.bus_field(Field::Word, true, value, q.open_last);
            } else {
                self.bus_field(Field::Word, false, 16, q.open_iwram);
                self.bus_field(Field::Word, true, 16, q.open_last);
            }
            self.a.b(merged);
        }
        self.a.bind(ewram);
        if store {
            self.open_lane(width, q.open_last, value);
        } else {
            let latch = match width {
                Width::Byte => {
                    self.a.logical(Logic::Orr, false, 16, 3, 3, Shift::Lsl, 8);
                    self.a
                        .logical(Logic::Orr, false, 16, 16, 16, Shift::Lsl, 16);
                    16
                }
                Width::Half => {
                    self.a.logical(Logic::Orr, false, 16, 3, 3, Shift::Lsl, 16);
                    16
                }
                Width::Word => value,
            };
            self.bus_field(Field::Word, true, latch, q.open_last);
        }
        self.a.bind(merged);
        if store {
            self.bus_field(Field::Word, false, 3, q.now);
            self.a.add_imm(false, false, 16, 3, 1);
            self.bus_field(Field::Word, true, 16, q.sample);
            self.a.arith(false, false, false, 3, 3, 7, Shift::Lsl, 0);
            self.bus_field(Field::Word, true, 3, q.now);
        } else {
            match width {
                Width::Word => {
                    self.a.lsl(5, 0, 3);
                    self.a.rorv(0, 3, 5);
                }
                Width::Byte | Width::Half => {
                    let (unsigned, loaded) = (self.a.label(), self.a.label());
                    let top = if width == Width::Byte { 7 } else { 15 };
                    self.a.tbz(false, 2, 2, unsigned);
                    self.a.sbfm(0, 3, 0, top);
                    self.a.b(loaded);
                    self.a.bind(unsigned);
                    self.a.mov(0, 3);
                    self.a.bind(loaded);
                }
            }
        }
        self.bus_field(Field::Wide, false, 16, q.clock);
        self.a.arith(true, false, false, 16, 16, 7, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 16, q.clock);
        self.a.ldr(true, 16, CTX, self.o.core);
        self.a.strb(ZR, 16, self.o.next_fetch);
        self.a.b(tail);
        Some(())
    }

    fn rom(&mut self, q: &QuickFetch, slow: Label, tail: Label) -> Option<()> {
        let shift = 32 - q.rom_page.trailing_zeros();
        let (flowing, top, done, glitched) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        let (half, word, commit) = (self.a.label(), self.a.label(), self.a.label());
        let literal = self.a.label();
        self.a.ldr(true, 4, CTX, self.o.bus);
        self.a.mov_imm(1, 0);
        self.bus_field(Field::Byte, false, 6, q.flowing);
        self.a.cbz(true, 6, flowing);
        self.bus_field(Field::Byte, false, 6, q.active);
        self.a.cbz(false, 6, glitched);
        self.bus_field(Field::Byte, false, 6, q.paused);
        self.a.cbz(false, 6, slow);
        self.a.b(glitched);
        self.a.bind(flowing);
        self.bus_field(Field::Byte, false, 6, q.stuck);
        self.a.cbz(true, 6, slow);
        self.bus_field(Field::Word, false, 7, q.cost);
        self.a.cbz(false, 7, slow);
        self.bus_field(Field::Word, false, 6, q.progress);
        self.a.arith(false, true, false, 6, 7, 6, Shift::Lsl, 0);
        self.bus_field(Field::Wide, false, 5, q.since);
        self.a.arith(true, false, false, 5, 5, 6, Shift::Lsl, 0);
        self.bus_field(Field::Wide, false, 3, q.clock);
        self.bus_field(Field::Word, false, 16, q.count);
        self.bus_field(Field::Word, false, 6, q.head);
        self.a.bind(top);
        self.a.arith(true, true, false, 17, 3, 5, Shift::Lsl, 0);
        self.a.tbz(true, 17, 63, done);
        self.a.add_imm(false, false, 16, 16, 1);
        self.a.sub_imm(false, false, 17, 16, q.capacity);
        self.a.cbz(false, 17, glitched);
        self.a.tbz(false, 17, 31, slow);
        self.a.arith(false, false, false, 17, 6, 16, Shift::Lsl, 1);
        self.a.lsl(17, 17, shift);
        self.a.cbz(false, 17, slow);
        self.a.arith(true, false, false, 5, 5, 7, Shift::Lsl, 0);
        self.a.b(top);
        self.a.bind(done);
        self.a.arith(true, true, false, 5, 5, 3, Shift::Lsl, 0);
        self.a.sub_imm(true, false, 5, 5, 1);
        self.a.cbz_wide(true, 5, glitched);
        self.a.mov_imm(1, 1);
        self.a.bind(glitched);
        self.a.tbz(true, 2, 1, word);
        self.a.tbz(true, 2, 0, half);
        self.rom_bounds(q, slow);
        self.rom_access(q, Width::Byte, slow)?;
        self.a.b(commit);
        self.a.bind(half);
        self.rom_bounds(q, slow);
        self.rom_access(q, Width::Half, slow)?;
        self.a.b(commit);
        self.a.bind(word);
        self.a.tbz(true, 2, ROM_LITERAL.trailing_zeros(), literal);
        self.rom_bounds(q, slow);
        self.rom_access(q, Width::Word, slow)?;
        self.a.b(commit);
        self.a.bind(literal);
        self.rom_literal(q, slow);
        self.a.bind(commit);
        self.bus_field(Field::Wide, false, 6, q.clock);
        self.stopped(q);
        self.a.ldr(true, 6, CTX, self.o.core);
        self.a.strb(ZR, 6, self.o.next_fetch);
        self.a.add_imm(false, false, 7, 7, 1);
        self.a.b(tail);
        Some(())
    }

    fn rom_access(&mut self, q: &QuickFetch, width: Width, slow: Label) -> Option<()> {
        let (align, op): (u32, Indexed) = match width {
            Width::Byte => (!0, Indexed::LoadByte),
            Width::Half => (!1, Indexed::LoadHalf),
            Width::Word => (!3, Indexed::LoadWord),
        };
        let (immr, imms) = logical_immediate(q.rom_mirror & align)?;
        if width == Width::Half {
            self.a.tbz(true, 0, 0, slow);
        }
        self.a.logical_imm(Logic::And, 5, 0, immr, imms);
        self.a.sub_imm(false, false, 6, 5, q.rom_skip);
        self.bus_field(Field::Wide, false, 7, q.plain_rom_end);
        self.a.arith(true, true, false, 6, 6, 7, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, slow);
        self.rom_wait(q, width, slow);
        self.a.ldr(true, 16, CTX, self.o.fast);
        self.a.ldr(true, 16, 16, offset_of!(Fast, rom) as u32);
        self.a.cbz_wide(false, 16, slow);
        self.rom_now(q);
        self.a.indexed(op, 3, 16, 5);
        match width {
            Width::Byte => {
                self.a.logical(Logic::Orr, false, 6, 3, 3, Shift::Lsl, 8);
                self.a.logical(Logic::Orr, false, 6, 6, 6, Shift::Lsl, 16);
            }
            Width::Half => self.a.logical(Logic::Orr, false, 6, 3, 3, Shift::Lsl, 16),
            Width::Word => self.a.mov(6, 3),
        }
        self.bus_field(Field::Word, true, 6, q.open_last);
        match width {
            Width::Word => {
                self.a.lsl(5, 0, 3);
                self.a.rorv(0, 3, 5);
            }
            Width::Byte | Width::Half => {
                let (unsigned, loaded) = (self.a.label(), self.a.label());
                let top = if width == Width::Byte { 7 } else { 15 };
                self.a.tbz(false, 2, 2, unsigned);
                self.a.sbfm(0, 3, 0, top);
                self.a.b(loaded);
                self.a.bind(unsigned);
                self.a.mov(0, 3);
                self.a.bind(loaded);
            }
        }
        Some(())
    }

    fn rom_bounds(&mut self, q: &QuickFetch, slow: Label) {
        self.a.mov_imm(6, q.rom_start);
        self.a.arith(false, true, false, 5, 0, 6, Shift::Lsl, 0);
        self.a.mov_imm(6, q.rom_span);
        self.a.arith(true, true, false, 6, 5, 6, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, slow);
    }

    fn rom_wait(&mut self, q: &QuickFetch, width: Width, slow: Label) {
        self.a.ubfx(6, 0, 25, 2);
        self.bus_byte(7, 6, q.first_halves);
        if width == Width::Word {
            self.bus_byte(3, 6, q.sequential_halves);
            self.a.arith(false, false, false, 7, 7, 3, Shift::Lsl, 0);
        }
        self.a.arith(false, false, false, 7, 7, 1, Shift::Lsl, 0);
        self.bus_field(Field::Word, false, 3, q.now);
        self.bus_field(Field::Word, false, 6, q.dma_check);
        self.a.arith(true, false, false, 3, 3, 7, Shift::Lsl, 0);
        self.a.arith(true, true, false, 6, 6, 3, Shift::Lsl, 0);
        self.a.sub_imm(true, false, 6, 6, 1);
        self.a.tbz(true, 6, 63, slow);
    }

    fn rom_now(&mut self, q: &QuickFetch) {
        self.a.add_imm(false, false, 3, 3, 1);
        self.bus_field(Field::Word, true, 3, q.now);
        self.bus_field(Field::Word, true, 3, q.sample);
    }

    fn rom_literal(&mut self, q: &QuickFetch, slow: Label) {
        self.rom_wait(q, Width::Word, slow);
        self.rom_now(q);
        self.a.lsr64(0, 2, 32);
        self.bus_field(Field::Word, true, 0, q.open_last);
    }

    fn open_lane(&mut self, width: Width, field: i32, value: R) {
        let op = match width {
            Width::Byte => Indexed::StoreByte,
            Width::Half => Indexed::StoreHalf,
            Width::Word => {
                self.bus_field(Field::Word, true, value, field);
                return;
            }
        };
        self.a.mov_imm(17, field as u32);
        self.a.arith(true, false, false, 17, 4, 17, Shift::Lsl, 0);
        self.a.indexed(op, value, 17, 5);
    }

    fn ram_spent(&mut self, out: Option<Label>) {
        let o = self.o;
        self.a.mrs_nzcv(3);
        self.a.arith(false, true, false, S, S, 7, Shift::Lsl, 0);
        self.a.ldr(false, 6, CTX, o.last);
        self.a.arith(false, false, false, 6, 6, 7, Shift::Lsl, 0);
        self.a.str(false, 6, CTX, o.last);
        self.a.str(false, ZR, CTX, o.force);
        self.spent(out);
        self.a.msr_nzcv(3);
        self.a.ret();
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
        ram: bool,
    ) -> Option<()> {
        let slow = self.a.label();
        let quick_ram = match self.o.quick {
            Some(q) if ram && self.timed && !self.v5 && !pages && specs.is_empty() => Some(q),
            _ => None,
        };
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
        } else if let Some(q) = quick_ram {
            self.ram_block(&q, store, slow, tail)?;
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

    fn ram_block(&mut self, q: &QuickFetch, store: bool, slow: Label, tail: Label) -> Option<()> {
        let (offset_r, offset_s) = logical_immediate((PAGE_SIZE - 1) & !3)?;
        let (word_r, word_s) = logical_immediate(!3)?;
        let pages = PAGE_SHIFT - PAGE_BITS as u32;
        let (copy, ewram) = (self.a.label(), self.a.label());
        self.a.ldr(true, 6, CTX, self.o.fast);
        self.a.ldr(true, 6, 6, offset_of!(Fast, pages) as u32);
        self.a.cbz_wide(false, 6, slow);
        self.a.lsr(5, 0, PAGE_SHIFT);
        self.a.arith(true, false, false, 5, 6, 5, Shift::Lsl, 5);
        self.a.pair(true, true, 6, 7, 5, 0);
        self.a.cbz(false, 7, slow);
        self.a.ubfx(1, 2, 26, 5);
        self.a.logical_imm(Logic::And, 16, 0, offset_r, offset_s);
        self.a.arith(false, false, false, 16, 16, 1, Shift::Lsl, 2);
        self.a.sub_imm(false, false, 16, 16, 4);
        self.a.tbz(true, 16, PAGE_SHIFT, slow);
        if store {
            self.a.ldr(true, 17, 5, offset_of!(Page, code) as u32);
            for first in [true, false] {
                if first {
                    self.a.ubfx(16, 0, PAGE_BITS as u32, pages);
                } else {
                    self.a.arith(false, false, false, 16, 0, 1, Shift::Lsl, 2);
                    self.a.sub_imm(false, false, 16, 16, 4);
                    self.a.ubfx(16, 16, PAGE_BITS as u32, pages);
                }
                self.a.indexed(Indexed::LoadByte, 16, 17, 16);
                self.a.cbz(true, 16, slow);
            }
        }
        self.a.ldr(true, 4, CTX, self.o.bus);
        if store {
            for hit in q.code_hits {
                self.bus_field(Field::Byte, false, 16, hit);
                self.a.cbz(true, 16, slow);
            }
        }
        self.a.logical_imm(Logic::And, 17, 0, word_r, word_s);
        self.a.arith(true, false, false, 6, 6, 17, Shift::Lsl, 0);
        self.a.ubfx(7, 7, WORD_CYCLES, 8);
        self.a.mul(1, 1, 7, ZR);
        self.bus_field(Field::Word, false, 16, q.now);
        self.a.arith(true, false, false, 16, 16, 1, Shift::Lsl, 0);
        if store {
            self.a.arith(true, true, false, 16, 16, 7, Shift::Lsl, 0);
        }
        self.bus_field(Field::Word, false, 17, q.dma_check);
        self.a.arith(true, true, false, 17, 16, 17, Shift::Lsl, 0);
        self.a.tbz(false, 17, 63, slow);
        if store {
            self.a.ldr(true, 5, 5, offset_of!(Page, dirty) as u32);
            self.a.mov_imm(17, 1);
            self.a.strb(17, 5, 0);
            self.a.add_imm(false, false, 5, 16, 1);
            self.bus_field(Field::Word, true, 5, q.sample);
            self.a.arith(false, false, false, 16, 16, 7, Shift::Lsl, 0);
            self.bus_field(Field::Word, true, 16, q.now);
        } else {
            self.a.add_imm(false, false, 16, 16, 1);
            self.bus_field(Field::Word, true, 16, q.now);
            self.bus_field(Field::Word, true, 16, q.sample);
            self.a.add_imm(false, false, 1, 1, 1);
        }
        self.bus_field(Field::Wide, false, 16, q.clock);
        self.a.arith(true, false, false, 16, 16, 1, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 16, q.clock);
        self.a.mov(7, 1);
        self.a.ldr(true, 16, CTX, self.o.core);
        self.a.strb(ZR, 16, self.o.next_fetch);
        self.a.add_imm(true, false, 5, CTX, self.o.block);
        self.a.ubfx(1, 2, 26, 5);
        self.a.lsl(1, 1, 2);
        self.a.bind(copy);
        self.a.sub_imm(false, false, 1, 1, 4);
        let (from, to) = if store { (5, 6) } else { (6, 5) };
        self.a.indexed(Indexed::LoadWord, 17, from, 1);
        self.a.indexed(Indexed::StoreWord, 17, to, 1);
        self.a.cbz(true, 1, copy);
        self.a.ubfx(1, 2, 26, 5);
        self.a.lsl(1, 1, 2);
        self.a.sub_imm(false, false, 1, 1, 4);
        self.a.indexed(Indexed::LoadWord, 16, 6, 1);
        self.bus_field(Field::Word, true, 16, q.open_last);
        self.a.tbz(false, 0, 24, ewram);
        self.bus_field(Field::Word, true, 16, q.open_iwram);
        self.a.bind(ewram);
        self.a.mov(0, 7);
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
        if let Some(quick) = o.quick {
            let slow = self.a.label();
            match mode {
                Some(mode) => self.quick_fetch(&quick, mode == THUMB, slow),
                None => self.idles_iwram(&quick, slow),
            }
            self.a.bind(slow);
        }
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

    fn quick_fetch(&mut self, q: &QuickFetch, thumb: bool, slow: Label) {
        let o = self.o;
        let halves = if thumb { 1 } else { 2 };
        let tail = self.a.label();
        let iwram = if thumb { slow } else { self.a.label() };
        let [due, elsewhere, drained] = if thumb {
            [self.a.label(), self.a.label(), self.a.label()]
        } else {
            [slow; 3]
        };
        self.a.tbz(true, 2, 31, iwram);
        self.a.ldr(true, 4, CTX, o.bus);
        self.bus_field(Field::Word, false, 5, q.now);
        self.bus_field(Field::Word, false, 6, q.dma_check);
        self.a.arith(true, true, false, 6, 5, 6, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, slow);
        self.bus_field(Field::Wide, false, 6, q.plain_rom_end);
        self.a.arith(true, true, false, 6, 2, 6, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, slow);
        self.bus_field(Field::Word, false, 6, q.head);
        self.a.arith(false, true, false, 6, 6, 1, Shift::Lsl, 0);
        self.a.cbz(true, 6, elsewhere);
        self.bus_field(Field::Wide, false, 6, q.clock);
        self.bus_field(Field::Wide, false, 16, q.due);
        self.a.arith(true, true, false, 2, 6, 16, Shift::Lsl, 0);
        self.a.logical(Logic::Orr, true, 2, 2, 16, Shift::Lsl, 0);
        self.a.tbz(false, 2, 63, due);
        self.bus_field(Field::Word, false, 2, q.count);
        if thumb {
            let flight = self.a.label();
            self.a.cbz(false, 2, flight);
            self.quick_hit(q, halves, drained);
            self.a.b(tail);
            self.a.bind(flight);
            self.quick_flight(q, slow);
        } else {
            self.a.cbz(false, 2, slow);
            self.quick_hit(q, halves, slow);
        }
        self.a.bind(tail);
        self.a.add_imm(false, false, 7, 5, 1);
        self.bus_field(Field::Word, true, 7, q.sample);
        self.a.arith(false, false, false, 5, 5, 2, Shift::Lsl, 0);
        self.bus_field(Field::Word, true, 5, q.now);
        self.bus_field(Field::Word, true, 1, q.open_addr);
        self.bus_field(Field::Word, true, 3, q.open_last);
        self.a.arith(false, true, false, S, S, 2, Shift::Lsl, 0);
        self.a.str(false, 2, CTX, o.last);
        self.a.ret();
        if thumb {
            self.a.bind(due);
            self.quick_settled(q, slow);
            self.a.b(tail);
            self.a.bind(elsewhere);
            self.quick_elsewhere(q, slow);
            self.a.b(tail);
            self.a.bind(drained);
            self.quick_drained(q);
            self.a.b(tail);
        } else {
            let bios = self.a.label();
            self.a.bind(iwram);
            self.quick_word(q, IWRAM_FETCH, q.open_iwram, bios, slow);
            self.a.bind(bios);
            self.quick_word(q, BIOS_FETCH, q.open_bios, slow, slow);
        }
    }

    fn quick_word(&mut self, q: &QuickFetch, marker: u32, latch: i32, next: Label, slow: Label) {
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.add_imm(false, false, 6, 2, marker.wrapping_neg());
        self.a.cbz(true, 6, next);
        self.a.ldr(true, 4, CTX, self.o.bus);
        self.bus_field(Field::Word, false, 5, q.now);
        self.bus_field(Field::Word, false, 6, q.dma_check);
        self.a.arith(true, true, false, 6, 5, 6, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, slow);
        self.bus_field(Field::Byte, false, 6, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 6, bit % 8, slow);
        self.bus_field(Field::Wide, false, 6, q.clock);
        self.a.add_imm(true, false, 6, 6, 1);
        self.bus_field(Field::Wide, true, 6, q.clock);
        self.bus_field(Field::Byte, true, ZR, q.branched);
        self.bus_field(Field::Word, true, 3, latch);
        self.a.add_imm(false, false, 5, 5, 1);
        self.bus_field(Field::Word, true, 5, q.sample);
        self.bus_field(Field::Word, true, 5, q.now);
        self.bus_field(Field::Word, true, 1, q.open_addr);
        self.bus_field(Field::Word, true, 3, q.open_last);
        self.a.sub_imm(false, false, S, S, 1);
        self.a.mov_imm(2, 1);
        self.a.str(false, 2, CTX, self.o.last);
        self.a.ret();
    }

    fn idles_iwram(&mut self, q: &QuickFetch, slow: Label) {
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.tbz(false, 2, IWRAM_IDLES.trailing_zeros(), slow);
        self.a.ldr(true, 4, CTX, self.o.bus);
        self.a.lsr(7, 2, IDLES_SHIFT);
        self.bus_field(Field::Word, false, 5, q.now);
        self.bus_field(Field::Word, false, 6, q.dma_check);
        self.a.arith(true, true, false, 6, 6, 5, Shift::Lsl, 0);
        self.a.arith(true, true, false, 6, 6, 7, Shift::Lsl, 0);
        self.a.sub_imm(true, false, 6, 6, 1);
        self.a.tbz(true, 6, 63, slow);
        self.bus_field(Field::Byte, false, 6, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 6, bit % 8, slow);
        self.a.add_imm(false, false, 7, 7, 1);
        self.bus_field(Field::Wide, false, 6, q.clock);
        self.a.arith(true, false, false, 6, 6, 7, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 6, q.clock);
        self.bus_field(Field::Byte, true, ZR, q.branched);
        self.a.arith(false, false, false, 5, 5, 7, Shift::Lsl, 0);
        self.bus_field(Field::Word, true, 5, q.sample);
        self.bus_field(Field::Word, true, 5, q.now);
        self.bus_field(Field::Word, true, 3, q.open_iwram);
        self.bus_field(Field::Word, true, 3, q.open_last);
        self.bus_field(Field::Word, true, 1, q.open_addr);
        self.a.str(false, 7, CTX, self.o.last);
        self.a.arith(false, true, false, S, S, 7, Shift::Lsl, 0);
        self.a.ret();
    }

    fn flat_iwram(&mut self, q: &QuickFetch, slow: Label) {
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.tbz(false, 2, IWRAM_RUN.trailing_zeros(), slow);
        self.a.ldr(true, 4, CTX, self.o.bus);
        self.a.lsr(7, 2, RUN_CYCLES_SHIFT);
        self.a.arith(false, true, false, 6, S, 7, Shift::Lsl, 0);
        self.a.tbz(true, 6, 31, slow);
        self.a.cbz(false, 6, slow);
        self.bus_field(Field::Word, false, 5, q.now);
        self.bus_field(Field::Word, false, 6, q.dma_check);
        self.a.arith(true, true, false, 6, 6, 5, Shift::Lsl, 0);
        self.a.arith(true, true, false, 6, 6, 7, Shift::Lsl, 0);
        self.a.tbz(true, 6, 63, slow);
        self.bus_field(Field::Byte, false, 6, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 6, bit % 8, slow);
        self.bus_field(Field::Wide, false, 6, q.clock);
        self.a.arith(true, false, false, 6, 6, 7, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 6, q.clock);
        self.bus_field(Field::Byte, true, ZR, q.branched);
        self.a.arith(false, false, false, 5, 5, 7, Shift::Lsl, 0);
        self.bus_field(Field::Word, true, 5, q.sample);
        self.bus_field(Field::Word, true, 5, q.now);
        self.bus_field(Field::Word, true, 3, q.open_iwram);
        self.bus_field(Field::Word, true, 3, q.open_last);
        self.a.ubfx(0, 2, IDLES_SHIFT, RUN_COUNT_MASK.count_ones());
        self.a.lsl(0, 0, 2);
        self.a.ubfx(6, 2, THUMB.trailing_zeros(), 1);
        self.a.lsrv64(0, 0, 6);
        self.a.arith(false, false, false, 0, 1, 0, Shift::Lsl, 0);
        self.bus_field(Field::Word, true, 0, q.open_addr);
        self.a.mov_imm(6, 1);
        self.a.str(false, 6, CTX, self.o.last);
        self.a.arith(false, true, false, S, S, 7, Shift::Lsl, 0);
        self.a.ret();
    }

    fn quick_hit(&mut self, q: &QuickFetch, halves: u32, slow: Label) {
        self.bus_field(Field::Byte, false, 7, q.paused);
        self.a.arith(true, true, false, 7, 2, 7, Shift::Lsl, 0);
        self.a.sub_imm(true, false, 7, 7, halves);
        self.a.tbz(true, 7, 63, slow);
        self.bus_field(Field::Byte, true, ZR, q.branched);
        self.a.sub_imm(false, false, 2, 2, halves);
        self.bus_field(Field::Word, true, 2, q.count);
        self.a.add_imm(false, false, 2, 1, 2 * halves);
        self.bus_field(Field::Word, true, 2, q.head);
        self.a.add_imm(true, false, 6, 6, 1);
        self.bus_field(Field::Wide, true, 6, q.clock);
        self.a.mov_imm(2, 1);
    }

    fn quick_flight(&mut self, q: &QuickFetch, slow: Label) {
        self.a.add_imm(false, false, 2, 1, 2);
        self.a.lsl(2, 2, 32 - q.rom_page.trailing_zeros());
        self.a.cbz(false, 2, slow);
        self.bus_field(Field::Byte, false, 2, q.branched);
        self.bus_field(Field::Byte, false, 7, q.stuck);
        self.a.logical(Logic::Orr, false, 2, 2, 7, Shift::Lsl, 0);
        self.a.cbz(true, 2, slow);
        self.a.arith(true, true, false, 2, 16, 6, Shift::Lsl, 0);
        self.bus_field(Field::Byte, false, 7, q.following);
        self.bus_field(Field::Wide, false, 16, q.since);
        self.a.arith(true, true, false, 16, 16, 6, Shift::Lsl, 0);
        self.a.lsr64(16, 16, 63);
        self.a.logical(Logic::Orr, false, 7, 7, 16, Shift::Lsl, 0);
        self.bus_field(Field::Word, false, 16, q.progress);
        self.a.arith(true, true, false, 16, ZR, 16, Shift::Lsl, 0);
        self.a.lsr64(16, 16, 63);
        self.a.logical(Logic::Orr, false, 7, 7, 16, Shift::Lsl, 0);
        self.bus_field(Field::Byte, true, 7, q.following);
        self.bus_field(Field::Word, true, ZR, q.progress);
        self.bus_field(Field::Wide, true, 6, q.since);
        self.a.add_imm(false, false, 7, 1, 2);
        self.bus_field(Field::Word, true, 7, q.head);
        self.bus_field(Field::Word, false, 7, q.cost);
        self.a.arith(true, false, false, 7, 6, 7, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 7, q.due);
    }

    fn quick_settled(&mut self, q: &QuickFetch, slow: Label) {
        let shift = 32 - q.rom_page.trailing_zeros();
        let (top, settled, paused, hit) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a.arith(true, true, false, 2, 6, 16, Shift::Lsl, 0);
        self.bus_field(Field::Byte, false, 7, q.flowing);
        self.a.cbz(false, 7, slow);
        self.bus_field(Field::Byte, false, 7, q.stuck);
        self.a.cbz(true, 7, slow);
        self.bus_field(Field::Word, false, 7, q.cost);
        self.a.cbz(false, 7, slow);
        self.bus_field(Field::Word, false, 16, q.count);
        self.a.bind(top);
        self.a.add_imm(false, false, 16, 16, 1);
        self.a.sub_imm(false, false, 17, 16, q.capacity);
        self.a.cbz(false, 17, paused);
        self.a.tbz(false, 17, 31, slow);
        self.a.arith(false, false, false, 17, 1, 16, Shift::Lsl, 1);
        self.a.lsl(17, 17, shift);
        self.a.cbz(false, 17, slow);
        self.a.arith(true, true, false, 17, 2, 7, Shift::Lsl, 0);
        self.a.tbz(true, 17, 63, settled);
        self.a.mov64(2, 17);
        self.a.b(top);
        self.a.bind(settled);
        self.a.sub_imm(false, false, 16, 16, 1);
        self.bus_field(Field::Word, true, 16, q.count);
        self.bus_field(Field::Word, true, 2, q.progress);
        self.a.arith(true, false, false, 16, 6, 7, Shift::Lsl, 0);
        self.a.arith(true, true, false, 16, 16, 2, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 16, q.due);
        self.a.b(hit);
        self.a.bind(paused);
        self.a.mov_imm(16, q.capacity - 1);
        self.bus_field(Field::Word, true, 16, q.count);
        self.bus_field(Field::Word, true, ZR, q.progress);
        self.a.mov_imm(7, 1);
        self.bus_field(Field::Byte, true, 7, q.paused);
        self.bus_field(Field::Byte, true, ZR, q.flowing);
        self.a.logical(Logic::Orn, true, 16, ZR, ZR, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 16, q.due);
        self.a.bind(hit);
        self.bus_field(Field::Wide, true, 6, q.since);
        self.a.mov_imm(7, 1);
        self.bus_field(Field::Byte, true, 7, q.following);
        self.bus_field(Field::Byte, true, ZR, q.branched);
        self.a.add_imm(false, false, 7, 1, 2);
        self.bus_field(Field::Word, true, 7, q.head);
        self.a.add_imm(true, false, 6, 6, 1);
        self.bus_field(Field::Wide, true, 6, q.clock);
        self.a.mov_imm(2, 1);
    }

    fn quick_elsewhere(&mut self, q: &QuickFetch, slow: Label) {
        let o = self.o;
        let bit = q.prefetch_bit.trailing_zeros();
        let page_bits = q.rom_page.trailing_zeros();
        let (known, first, priced) = (self.a.label(), self.a.label(), self.a.label());
        self.bus_field(Field::Byte, false, 6, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 6, bit % 8, slow);
        self.bus_field(Field::Byte, false, 6, q.flowing);
        self.a.cbz(true, 6, slow);
        self.a.add_imm(false, false, 6, 1, 2);
        self.a.lsl(6, 6, 32 - page_bits);
        self.a.cbz(false, 6, slow);
        self.a.sub_imm(false, false, 7, 0, KEEP);
        self.a.cbz(true, 7, known);
        self.a.ldr(true, 7, CTX, o.core);
        self.a.ldrb(0, 7, o.next_fetch);
        self.a.bind(known);
        self.a.ubfx(6, 1, 25, 2);
        self.a.tbz(false, 0, SEQUENTIAL.trailing_zeros(), first);
        self.bus_field(Field::Byte, false, 7, q.force_n);
        self.a.cbz(true, 7, first);
        self.a.lsl(7, 1, 32 - page_bits);
        self.a.cbz(false, 7, first);
        self.bus_byte(2, 6, q.sequential_halves);
        self.a.b(priced);
        self.a.bind(first);
        self.bus_byte(2, 6, q.first_halves);
        self.a.bind(priced);
        self.bus_byte(7, 6, q.sequential_halves);
        self.bus_field(Field::Word, true, 7, q.cost);
        self.bus_field(Field::Wide, false, 6, q.clock);
        self.bus_field(Field::Wide, true, 6, q.since);
        self.a.arith(true, false, false, 6, 6, 7, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 6, q.due);
        self.a.lsr(6, 1, page_bits);
        self.a.add_imm(false, false, 6, 6, 1);
        self.a.ubfx(6, 6, 25 - page_bits, 2);
        self.bus_byte(6, 6, q.sequential_halves);
        self.bus_field(Field::Word, true, 6, q.page_cost);
        self.a.add_imm(false, false, 6, 1, 2);
        self.bus_field(Field::Word, true, 6, q.head);
        self.restart(q, 7);
    }

    fn quick_drained(&mut self, q: &QuickFetch) {
        self.stopped(q);
        self.a.mov_imm(7, 1);
        self.bus_field(Field::Byte, true, 7, q.force_n);
        self.a.mov_imm(2, 1);
    }

    fn stopped(&mut self, q: &QuickFetch) {
        self.bus_field(Field::Wide, true, 6, q.since);
        self.a.add_imm(true, false, 6, 6, 1);
        self.bus_field(Field::Wide, true, 6, q.clock);
        self.a.logical(Logic::Orn, true, 16, ZR, ZR, Shift::Lsl, 0);
        self.bus_field(Field::Wide, true, 16, q.due);
        if q.packed() {
            for zero in [q.head, q.count, q.parked] {
                self.bus_field(Field::Wide, true, ZR, zero);
            }
            self.bus_field(Field::Word, true, ZR, q.active);
            for off in [q.stuck, q.following] {
                self.bus_field(Field::Byte, true, ZR, off);
            }
            return;
        }
        for zero in [q.head, q.count, q.progress, q.cost, q.page_cost, q.parked] {
            self.bus_field(Field::Word, true, ZR, zero);
        }
        for off in [
            q.paused,
            q.branched,
            q.active,
            q.flowing,
            q.stuck,
            q.following,
        ] {
            self.bus_field(Field::Byte, true, ZR, off);
        }
    }

    fn restart(&mut self, q: &QuickFetch, one: R) {
        if q.packed() {
            self.bus_field(Field::Wide, true, ZR, q.count);
            self.bus_field(Field::Word, true, ZR, q.parked);
            self.a.mov_imm(one, RESTARTED);
            self.bus_field(Field::Wide, true, one, q.active);
            return;
        }
        for zero in [q.count, q.progress, q.parked] {
            self.bus_field(Field::Word, true, ZR, zero);
        }
        for off in [q.paused, q.branched, q.stuck, q.following, q.force_n] {
            self.bus_field(Field::Byte, true, ZR, off);
        }
        self.a.mov_imm(one, 1);
        for on in [q.active, q.flowing] {
            self.bus_field(Field::Byte, true, one, on);
        }
    }

    fn bus_field(&mut self, field: Field, store: bool, rt: R, offset: i32) {
        let offset = offset as u32;
        let scale = match field {
            Field::Byte => 1,
            Field::Word => 4,
            Field::Wide => 8,
        };
        let (base, offset) = if offset.is_multiple_of(scale) && offset / scale < 0x1000 {
            (4, offset)
        } else {
            self.a.mov_imm(17, offset);
            self.a.arith(true, false, false, 17, 4, 17, Shift::Lsl, 0);
            (17, 0)
        };
        match (field, store) {
            (Field::Byte, false) => self.a.ldrb(rt, base, offset),
            (Field::Byte, true) => self.a.strb(rt, base, offset),
            (Field::Word, false) => self.a.ldr(false, rt, base, offset),
            (Field::Word, true) => self.a.str(false, rt, base, offset),
            (Field::Wide, false) => self.a.ldr(true, rt, base, offset),
            (Field::Wide, true) => self.a.str(true, rt, base, offset),
        }
    }

    fn fetch_flat(&mut self, helper: usize) {
        let o = self.o;
        if let Some(quick) = o.quick {
            let slow = self.a.label();
            self.flat_iwram(&quick, slow);
            self.a.bind(slow);
        }
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
