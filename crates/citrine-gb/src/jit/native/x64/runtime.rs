use super::super::super::QuickFetch;
use super::super::super::fast::{
    CODE_CYCLES, Fast, HALF_CYCLES, HALF_SEQUENTIAL, LONG_AFTER, LONG_HALF, LONG_WORD, MAIN,
    PAGE_SHIFT, PAGE_SIZE, Page, SEQUENTIAL_CYCLES, Spec, WORD_CYCLES, Window,
};
use super::super::super::helpers::{
    EXIT, IDLES_SHIFT, LAST, RUN_COUNT_MASK, RUN_CYCLES_SHIFT, SEQUENTIAL, THUMB,
};
use super::super::super::lookup::{
    CELL_BASE, HASH, HASH_SHIFT, LINK_SHIFT, Link, NO_KEY, SLOT_CODE, SLOT_GENERATION,
    SLOT_PIPELINE, SLOT_SIZE, SLOT_START, SLOT_TAG, SLOT_WORDS,
};
use super::super::super::marks::PAGE_BITS;
use super::super::super::scan::Width;
use super::super::{
    BIOS_FETCH, CODE_CLASS, Fetch, IWRAM_FETCH, IWRAM_IDLES, IWRAM_RUN, Info, Last, Offsets,
    PSR_CPSR, PSR_RESTORE, RESTORE_FIELDS, ROM_LITERAL, Runtime, STUBS, Setup, Stub,
};
use super::asm::{
    Alu, Asm, Label, Mem, R, R8, R9, R10, R11, R12, R13, R14, R15, RAX, RBP, RBX, RCX, RDI, RDX,
    RSI, RSP, Rot, at, cc, indexed,
};
use super::{
    CODE_LIMIT, DEBT, FRAME, FRAME_BASE, FRAME_BUDGET, FRAME_BUS, FRAME_CELL, FRAME_CORE,
    FRAME_CTX, FRAME_DEBT, FRAME_FAST, FRAME_FORCE, FRAME_KEY, FRAME_LAST, FRAME_SHIFT,
    FRAME_START, FRAME_WORDS, HOP_INVERTED, HOP_TARGET, KEEP, PINNED, REGS, S, at_shift, left,
};
use crate::gba::cpu::psr::{Bank, FLAG_I, FLAG_T, MODE_MASK, Mode};
use std::mem::{offset_of, size_of};

const CALLEE: [R; 6] = [RBX, RBP, R12, R13, R14, R15];
const FAST_FREE: [R; 3] = [RDI, R8, R14];
const FIND_FREE: [R; 3] = [RBX, R12, R13];
const RETURN_INFO: u32 = 0x1A00;
const CHARGED: u32 = 1 << 31;
pub(super) const CYCLES_MASK: i32 = (EXIT - 1) as i32;
pub(super) const OLD_LAST: u32 = 2 << 12;
pub(super) const SPSR_WRITE: u32 = 1 << 12;
pub(super) const SPSR_READ: u32 = 1 << 13;
const FLAGS_FIELD: u32 = 1 << 11;
const CONTROL_FIELDS: u32 = 0x00FF_FFFF;
const MODE_BIT_4: u32 = 0x10;
pub(super) const IMAGE_MASK: i32 = 0xC101;
pub(super) const IMAGE_TO_NZCV: i32 = 0x1021_0000;
const NZCV_TO_IMAGE: i32 = 0x1081;
pub(super) const NZCV_MASK: u32 = 0xF000_0000;
pub(super) const PC_SLOT: i32 = 60;
const FAST_SAVED: [R; 3] = [RAX, RDI, R8];
const RESTARTED: i32 = 0x0101;
const _: () = assert!(WORD_CYCLES.is_multiple_of(8) && HALF_CYCLES.is_multiple_of(8));
const FAST_LOAD_SAVED: [R; 1] = [RAX];
const BLOCK_SAVED: [R; 4] = [RAX, RDI, R8, RBX];
const FETCH_SAVED: [R; 6] = [RAX, RDI, R8, S, DEBT, REGS];
const QUICK_SAVED: [R; 6] = [RAX, RCX, RDI, R8, R10, R11];
const LEAN_SAVED: [R; 3] = [RCX, R11, RDI];
const THUMB_SAVED: [R; 2] = [RCX, R11];
const SETTLE_SAVED: [R; 3] = [RAX, RDI, R8];
const ELSEWHERE_SAVED: [R; 2] = [RDI, R8];
pub const OVERLAP: u32 = 6;
pub const CODE_OVERLAP: u32 = 3;
const _: () = assert!(OVERLAP == LONG_AFTER);
const _: () = assert!(size_of::<Page>() == 32);

struct Tails {
    any: Label,
    short: Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leave {
    Branch,
    Exchange,
    Return,
    Restore,
}

struct Build<'a> {
    a: Asm,
    exit: Label,
    o: &'a Offsets,
    v5: bool,
    pages: bool,
    timed: bool,
}

pub fn runtime(setup: &Setup) -> Option<Runtime> {
    if !setup.v5 && !setup.specs.is_empty() {
        return None;
    }
    let (o, helpers, chain) = (setup.offsets, setup.helpers, setup.chain);
    let mut a = Asm::new(Vec::new());
    let exit = a.label();
    let mut build = Build {
        a,
        exit,
        o,
        v5: setup.v5,
        pages: setup.pages,
        timed: setup.timed,
    };
    let finish = build.a.label();
    let next = match chain {
        Some(_) => build.a.label(),
        None => finish,
    };
    let chained = build.a.label();
    let hops = chain.map(|_| (chained, finish));
    build.trampoline(chain.is_some());
    let exit = build.a.here();
    build.a.bind(build.exit);
    build.exit(chain, finish);
    let branch = build.a.here();
    build.hop_leave(Leave::Branch, setup.specs, hops)?;
    build.leave(helpers.branch, Leave::Branch, setup.specs, next, None, true)?;
    let exchange = build.a.here();
    build.hop_leave(Leave::Exchange, setup.specs, hops)?;
    build.leave(
        helpers.exchange,
        Leave::Exchange,
        setup.specs,
        next,
        None,
        true,
    )?;
    let returns = build.a.here();
    build.hop_leave(Leave::Return, setup.specs, hops)?;
    let loaded = build.a.label();
    build.leave(
        helpers.load_pc,
        Leave::Return,
        setup.specs,
        next,
        Some(loaded),
        true,
    )?;
    let restore = if setup.v5 {
        let at = build.a.here();
        build.restore(setup.psr);
        build.leave(
            helpers.branch,
            Leave::Restore,
            setup.specs,
            next,
            None,
            false,
        )?;
        at
    } else {
        0
    };
    let load_pc = build.a.here();
    build.load_pc(helpers.load, loaded);
    let swap = build.a.label();
    build.a.bind(swap);
    build.memory(helpers.swap, true, false, false)?;
    let load = build.a.here();
    let pages = setup.pages;
    let tail = build.memory(helpers.load, false, pages, true)?;
    build.a.jmp(swap);
    let store = build.a.here();
    build.memory(helpers.store, true, pages && !setup.v5, true)?;
    let gather = build.a.here();
    build.block(helpers.load_word, false, pages, Some(tail), true)?;
    let collect = build.a.here();
    build.block(helpers.load_word, false, pages && setup.v5, None, false)?;
    let scatter = build.a.here();
    build.block(helpers.store_word, true, pages, Some(tail), true)?;
    let step = build.a.here();
    let stepped = build.step(helpers.step)?;
    let psr = build.a.here();
    if setup.v5 || setup.pages {
        build.psr(stepped, setup.psr)?;
    }
    let mut fetches = [0; 4];
    if setup.timed {
        for (slot, mode) in fetches.iter_mut().zip([None, Some(0), Some(THUMB)]) {
            *slot = build.a.here();
            build.fetch(helpers.fetch, mode);
        }
        fetches[3] = build.a.here();
        build.fetch_flat(helpers.fetch_flat);
    }
    if let Some(chain) = chain {
        build.a.bind(next);
        build.chain(chain, finish, chained);
    }
    build.a.bind(finish);
    build.finish(chain.is_some());
    let mut stubs = [0; STUBS];
    for (stub, at) in [
        (Stub::Exit, exit),
        (Stub::Load, load),
        (Stub::Store, store),
        (Stub::Branch, branch),
        (Stub::Exchange, exchange),
        (Stub::Step, step),
        (Stub::Psr, psr),
        (Stub::Gather, gather),
        (Stub::Scatter, scatter),
        (Stub::Collect, collect),
        (Stub::Return, returns),
        (Stub::LoadPc, load_pc),
        (Stub::Restore, restore),
        (Stub::Fetch, fetches[0]),
        (Stub::FetchArm, fetches[1]),
        (Stub::FetchThumb, fetches[2]),
        (Stub::FetchFlat, fetches[3]),
    ] {
        stubs[stub as usize] = at;
    }
    Some(Runtime {
        bytes: build.a.finish()?,
        stubs,
    })
}

impl Build<'_> {
    fn guests(&mut self, load: bool) {
        self.some_guests(load, |_| true);
    }

    fn some_guests(&mut self, load: bool, which: impl Fn(R) -> bool) {
        for (guest, home) in PINNED.iter().enumerate() {
            let Some(reg) = *home else {
                continue;
            };
            if !which(reg) {
                continue;
            }
            let slot = at(REGS, 4 * guest as i32);
            if load {
                self.a.load(false, reg, slot);
            } else {
                self.a.store(false, slot, reg);
            }
        }
    }

    fn store_cpsr(&mut self, core: R, scratch: R, cpsr: R) {
        let o = self.o;
        self.a.mov(false, scratch, RAX);
        self.a.alu_imm(Alu::And, false, scratch, IMAGE_MASK);
        self.a.imul_imm(scratch, scratch, IMAGE_TO_NZCV);
        self.a.alu_imm(Alu::And, false, scratch, NZCV_MASK as i32);
        self.a.load(false, cpsr, at(core, o.cpsr as i32));
        self.a.alu_imm(Alu::And, false, cpsr, !NZCV_MASK as i32);
        self.a.alu(Alu::Or, false, cpsr, scratch);
        self.a.store(false, at(core, o.cpsr as i32), cpsr);
    }

    fn image(&mut self, dest: R, cpsr: R) {
        self.a.mov(false, dest, cpsr);
        self.a.rot(Rot::Shr, false, dest, 28);
        self.a.imul_imm(dest, dest, NZCV_TO_IMAGE);
        self.a.alu_imm(Alu::And, false, dest, IMAGE_MASK);
    }

    fn call(&mut self, helper: usize) {
        self.a.mov_imm64(RAX, helper as u64);
        self.a.call_reg(RAX);
    }

    fn store_pc(&mut self, frame: i32) {
        self.a.extend16(false, RAX, RSI);
        self.a.rot(Rot::Shr, false, RAX, 8);
        self.a.load(false, RCX, at(RSP, frame + FRAME_SHIFT));
        self.a.rot_cl(Rot::Shl, false, RAX);
        self.a.alu_load(Alu::Add, RAX, at(RSP, frame + FRAME_BASE));
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a
            .store(false, at(RDI, self.o.regs as i32 + PC_SLOT), RAX);
    }

    fn trampoline(&mut self, chained: bool) {
        let o = self.o;
        let (near, run, step) = (self.a.label(), self.a.label(), self.a.label());
        self.a.load(true, R10, at(RSP, 8));
        self.a.mov(false, RCX, RCX);
        self.a.load(false, RAX, indexed(R10, RCX, 4, 0));
        if self.timed {
            self.a.mov(false, R11, RAX);
            self.a.rot(Rot::Shr, false, R11, 27);
            self.a.alu(Alu::Cmp, false, RDX, R11);
            self.a.jcc(cc::BE, near);
        }
        self.a.bind(run);
        for reg in CALLEE {
            self.a.push(reg);
        }
        self.a.alu_imm(Alu::Sub, true, RSP, FRAME);
        self.a
            .load(true, R11, at(RSP, FRAME + 8 * CALLEE.len() as i32 + 16));
        self.a.store(true, at(RSP, FRAME_CTX), R11);
        if chained {
            self.a.store(true, at(R11, o.core as i32), RDI);
            self.a.store(true, at(R11, o.bus as i32), RSI);
            self.a.store(false, at(R11, o.budget as i32 + 4), RCX);
            self.a.store_imm(at(R11, o.acc as i32), 0);
            self.a.store8_imm(at(RSP, FRAME_CELL), 0);
        }
        self.a.store(true, at(RSP, FRAME_CORE), RDI);
        self.a.store(true, at(RSP, FRAME_BUS), RSI);
        self.a.store(true, at(RSP, FRAME_WORDS), R9);
        self.a.store(false, at(RSP, FRAME_BUDGET), RDX);
        self.a.store(false, at(RSP, FRAME_START), RCX);
        self.a.store(true, at(RSP, FRAME_FAST), R8);
        if !self.timed {
            self.a.load(true, R8, at(R8, o.fast_debt as i32));
            self.a.store(true, at(RSP, FRAME_DEBT), R8);
        }
        self.a.mov(false, S, RAX);
        self.a.rot(Rot::Shr, false, S, 16);
        self.a.alu_imm(Alu::And, false, S, 0x7FF);
        self.a.alu(Alu::Add, false, S, RDX);
        self.a.mov(false, RDX, RAX);
        self.a.alu_imm(Alu::And, false, RDX, 0x7FFF);
        self.a.alu(Alu::Add, true, RDX, R10);
        if self.timed {
            self.a.mov_imm(DEBT, 0);
        } else {
            self.a.load(false, DEBT, at(R8, 0));
        }
        self.a.alu(Alu::Add, false, S, DEBT);
        self.a.load(false, RSI, at(RDI, o.cpsr as i32));
        self.a.mov(false, R8, RSI);
        self.a.rot(Rot::Shr, false, R8, 5);
        self.a.alu_imm(Alu::And, false, R8, 1);
        self.a.neg(R8);
        self.a.alu_imm(Alu::Add, false, R8, 2);
        self.a.store(false, at(RSP, FRAME_SHIFT), R8);
        self.a.mov(false, R11, RCX);
        self.a.mov(false, RCX, R8);
        self.a.rot_cl(Rot::Shl, false, R11);
        self.a.load(false, R8, at(RDI, o.regs as i32 + PC_SLOT));
        self.a.alu(Alu::Sub, false, R8, R11);
        self.a.store(false, at(RSP, FRAME_BASE), R8);
        self.image(R8, RSI);
        self.a.mov(false, RCX, RAX);
        self.a.rot(Rot::Shr, false, RCX, 7);
        self.a.alu_imm(Alu::And, false, RCX, 0x100);
        self.enter(R8);
        self.a.lea(true, REGS, at(RDI, o.regs as i32));
        self.guests(true);
        self.a.mov_imm(RCX, KEEP);
        self.a.jmp_reg(RDX);
        if !self.timed {
            return;
        }
        self.a.bind(near);
        self.a.load(false, R11, at(R10, -4));
        self.a.rot(Rot::Shr, false, R11, 16);
        self.a.test(R11, R11);
        self.a.jcc(cc::E, step);
        self.a.alu(Alu::Add, false, R11, RCX);
        self.a.load(false, RAX, indexed(R10, R11, 4, 0));
        self.a.jmp(run);
        self.a.bind(step);
        self.a.mov_imm(RAX, EXIT as u32);
        self.a.ret();
    }

    fn store_debt(&mut self, scratch: R, frame: i32, debt: R) {
        if !self.timed {
            self.a.load(true, scratch, at(RSP, frame + FRAME_DEBT));
            self.a.store(false, at(scratch, 0), debt);
        }
    }

    fn exit(&mut self, chain: Option<usize>, finish: Label) {
        let o = self.o;
        let (kept, full, old, counted, forced) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.some_guests(false, |reg| !CALLEE.contains(&reg));
        self.a.rot(Rot::Shl, true, RDX, 32);
        self.a.alu(Alu::Or, true, RSI, RDX);
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, 22);
        self.a.alu(Alu::Sub, false, S, RCX);
        self.store_debt(RCX, 0, DEBT);
        self.a.test_imm(RSI, 1 << 11);
        self.a.jcc(cc::NE, full);
        self.a.mov(false, R8, RSI);
        self.a.alu_imm(Alu::And, false, R8, 0xFF);
        self.a.mov(false, RDX, R8);
        self.a.load(false, RCX, at(RSP, FRAME_SHIFT));
        self.a.rot_cl(Rot::Shl, false, RDX);
        self.a.alu_load(Alu::Add, RDX, at(RSP, FRAME_BASE));
        self.a.store(false, at(REGS, PC_SLOT), RDX);
        self.a.load(true, RCX, at(RSP, FRAME_WORDS));
        self.a.load(true, RDX, indexed(RCX, R8, 4, 0));
        self.a.store(true, at(RDI, o.pipeline as i32), RDX);
        self.a.test_imm(RSI, 1 << 9);
        self.a.jcc(cc::NE, kept);
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, 8);
        self.a.alu_imm(Alu::And, false, RCX, 1);
        self.a.store8(at(RDI, o.next_fetch as i32), RCX);
        self.a.bind(kept);
        self.a.load8(false, RCX, at(RDI, o.cpsr as i32));
        self.a.rot(Rot::Shr, false, RCX, 7);
        self.a.store8(at(RDI, o.irq_masked as i32), RCX);
        if let Some(chain) = chain {
            let (go, refused, called, missed, linked) = (
                self.a.label(),
                self.a.label(),
                self.a.label(),
                self.a.label(),
                self.a.label(),
            );
            self.a.test_imm(RSI, 1 << 10);
            self.a.jcc(cc::E, go);
            self.a.alu_mem_imm(Alu::Cmp, at(RSP, FRAME_FORCE), 0);
            self.a.jcc(cc::NE, full);
            self.a.bind(go);
            self.hop(called, missed, full, false);
            self.a.mov(false, R8, RAX);
            self.a.jmp(linked);
            self.a.bind(missed);
            self.a.load(true, R11, at(RSP, FRAME_CORE));
            self.a.lea(true, REGS, at(R11, o.regs as i32));
            self.a.jmp(full);
            self.a.bind(called);
            for reg in [RSI, S, DEBT, RAX] {
                self.a.push(reg);
            }
            self.a.load(true, RDI, at(RSP, 32 + FRAME_CTX));
            self.a.mov(false, RDX, RSI);
            self.a.alu_imm(Alu::And, false, RDX, 0xFF);
            self.a.mov(false, RSI, S);
            self.a.alu(Alu::Sub, false, RSI, DEBT);
            self.call(chain);
            for reg in [R8, DEBT, S, RSI] {
                self.a.pop(reg);
            }
            self.a.alu(Alu::Or, true, RAX, RAX);
            self.a.jcc(cc::E, refused);
            self.adopt(RDI);
            self.a.bind(linked);
            self.a.alu_imm(Alu::And, false, R8, IMAGE_MASK);
            self.enter(R8);
            self.some_guests(true, |reg| !CALLEE.contains(&reg));
            self.a.mov_imm(RCX, KEEP);
            self.a.jmp_reg(RDX);
            self.a.bind(refused);
            self.a.mov(false, RAX, R8);
            self.a.load(true, R11, at(RSP, FRAME_CORE));
            self.a.lea(true, REGS, at(R11, o.regs as i32));
        }
        self.a.bind(full);
        self.a.mov(true, RDX, RSI);
        self.a.rot(Rot::Shr, true, RDX, 32);
        self.a.mov(false, RSI, RSI);
        self.some_guests(false, |reg| CALLEE.contains(&reg));
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.store_cpsr(RDI, RCX, R8);
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, 12);
        self.a.alu_imm(Alu::And, false, RCX, 3);
        self.a.mov_imm(RBX, 1);
        self.a.alu_imm(Alu::Cmp, false, RCX, 1);
        self.a.jcc(cc::B, counted);
        self.a.jcc(cc::NE, old);
        self.a.load(false, RBX, at(RSP, FRAME_LAST));
        self.a.jmp(counted);
        self.a.bind(old);
        self.a.mov(false, RBX, RSI);
        self.a.rot(Rot::Shr, false, RBX, 14);
        self.a.alu_imm(Alu::And, false, RBX, 0xFF);
        self.a.alu(Alu::Sub, false, RBX, RDX);
        self.a.mov_imm(RCX, 1);
        self.a.alu_imm(Alu::Cmp, false, RBX, 1);
        self.a.cmov(cc::L, RBX, RCX);
        self.a.bind(counted);
        self.a.store(false, at(RDI, o.cycles as i32), RBX);
        self.a.store8_imm(at(RDI, o.irq_held as i32), 0);
        self.a.mov_imm(RBP, 0);
        self.a.test_imm(RSI, 1 << 10);
        self.a.jcc(cc::E, forced);
        self.a.load(false, RBP, at(RSP, FRAME_FORCE));
        self.a.bind(forced);
        self.a.jmp(finish);
    }

    fn settle(&mut self, cycles: R, debt: R, repaid: R) {
        self.a.lea(false, repaid, at(cycles, -1));
        self.a.alu(Alu::Cmp, false, debt, repaid);
        self.a.cmov(cc::B, repaid, debt);
        self.a.alu(Alu::Sub, false, debt, repaid);
        self.a.alu(Alu::Sub, false, cycles, repaid);
    }

    fn leave(
        &mut self,
        helper: usize,
        leave: Leave,
        specs: &[Spec],
        finish: Label,
        loaded: Option<Label>,
        charged: bool,
    ) -> Option<()> {
        let o = self.o;
        let returns = leave == Leave::Return;
        if returns || leave == Leave::Exchange {
            self.a.store8_imm(at(RSP, FRAME_CELL), 0);
        }
        self.guests(false);
        if returns {
            self.a.mov(false, R15, RDX);
        }
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.store_cpsr(RDI, RDX, R8);
        if !charged {
            self.a.mov(false, RDX, RSI);
            self.a.rot(Rot::Shr, false, RDX, 22);
            self.a.alu(Alu::Sub, false, S, RDX);
        }
        self.a.mov(false, RBX, RSI);
        self.a.mov(false, R12, S);
        self.a.mov(false, R13, DEBT);
        if let Some(loaded) = loaded {
            self.a.bind(loaded);
        }
        let (slow, called) = (self.a.label(), self.a.label());
        if self.refills(leave, specs) {
            self.refetch(leave, specs, slow, called)?;
        } else if let Some(q) = self.quick_leave(leave) {
            self.quick_branch(&q, leave, slow, called);
        }
        self.a.bind(slow);
        self.a.mov(false, RDX, RCX);
        self.a.load(true, RSI, at(RSP, FRAME_BUS));
        self.call(helper);
        self.a.bind(called);
        if leave == Leave::Restore {
            self.a.mov(false, RCX, RBX);
            self.a.rot(Rot::Shr, false, RCX, 14);
            self.a.alu_imm(Alu::And, false, RCX, 0xFF);
            self.a.alu(Alu::Add, false, RAX, RCX);
            self.a.alu_imm(Alu::And, false, RBX, !RESTORE_FIELDS as i32);
        }
        if returns {
            let refetched = self.a.label();
            self.a.mov(true, RDX, RAX);
            self.a.rot(Rot::Shr, true, RDX, 32);
            self.a.test_imm(R15, CHARGED);
            self.a.jcc(cc::E, refetched);
            self.a.mov(false, RDX, R15);
            self.a.rot(Rot::Shr, false, RDX, 16);
            self.a.alu_imm(Alu::And, false, RDX, 0x7FFF);
            self.a.alu_imm(Alu::And, false, R15, 0xFFFF);
            self.a.bind(refetched);
            self.overlap(RDX, R15, R11);
            self.a.alu(Alu::Add, false, RAX, RDX);
        }
        self.a.alu(Alu::Sub, false, R12, RAX);
        if self.timed {
            self.a.alu_load(Alu::Add, RAX, at(RSP, FRAME_LAST));
        }
        self.settle(RAX, R13, RCX);
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.a.store(false, at(RDI, o.cycles as i32), RAX);
        self.a.store8_imm(at(RDI, o.irq_held as i32), 0);
        self.store_debt(RCX, 0, R13);
        self.a.mov(false, RSI, RBX);
        self.a.mov(false, S, R12);
        self.a.mov(false, DEBT, R13);
        self.a.mov_imm(RBP, 0);
        self.a.jmp(finish);
        Some(())
    }

    fn overlap(&mut self, fetch: R, load: R, scratch: R) {
        self.a.lea(false, scratch, indexed(fetch, load, 1, -6));
        self.a.alu(Alu::Cmp, false, fetch, load);
        self.a.cmov(cc::B, fetch, load);
        self.a.alu(Alu::Cmp, false, scratch, fetch);
        self.a.cmov(cc::G, fetch, scratch);
    }

    fn restore(&mut self, helper: usize) {
        let restored = self.a.label();
        let lr = PINNED[14].expect("lr is pinned");
        self.a.push(RCX);
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, 13);
        self.a.alu_imm(Alu::And, false, RCX, 1);
        self.a.rot(Rot::Shl, false, RCX, 2);
        self.a.mov(false, RDX, lr);
        self.a.alu(Alu::Sub, false, RDX, RCX);
        self.guests(false);
        let saved = [RAX, RSI, RDX, S, DEBT, REGS];
        for reg in saved {
            self.a.push(reg);
        }
        let frame = 16 + 8 * saved.len() as i32;
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a.load(true, R8, at(RSP, frame + FRAME_BUS));
        self.a.mov_imm(RCX, PSR_RESTORE);
        self.call(helper);
        self.a.mov(false, RCX, RAX);
        for reg in saved.iter().rev() {
            self.a.pop(*reg);
        }
        self.guests(true);
        self.a.alu_imm(Alu::Cmp, false, RCX, 0);
        self.a.jcc(cc::NE, restored);
        self.a.pop(RCX);
        self.a.ret();
        self.a.bind(restored);
        self.image(RAX, RCX);
        self.a.mov(false, RCX, RDX);
        self.a.alu_imm(Alu::Add, true, RSP, 16);
    }

    fn load_pc(&mut self, helper: usize, loaded: Label) {
        let o = self.o;
        self.guests(false);
        self.a.mov(false, R15, RSI);
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.store_cpsr(RDI, RDX, R8);
        self.a.mov(false, RDX, RSI);
        self.a.rot(Rot::Shr, false, RDX, 19);
        self.a.alu_imm(Alu::And, false, RDX, 0x3FF);
        self.a.alu(Alu::Sub, false, S, RDX);
        self.a.mov(false, RBX, RSI);
        self.a.rot(Rot::Shr, false, RBX, 11);
        self.a.alu_imm(Alu::And, false, RBX, 0xFF);
        self.a.alu_imm(Alu::Or, false, RBX, RETURN_INFO as i32);
        self.a.mov(false, R12, S);
        self.a.mov(false, R13, DEBT);
        self.a.mov(false, RDX, RCX);
        self.a.mov(false, RCX, RSI);
        self.a.alu_imm(Alu::And, false, RCX, 7);
        self.a.load(true, RSI, at(RSP, FRAME_BUS));
        self.call(helper);
        self.a.rot(Rot::Shr, false, R15, 3);
        self.a.alu_imm(Alu::And, false, R15, 0xFF);
        self.a.rot(Rot::Shl, false, R15, 16);
        self.a.alu_imm(Alu::Or, false, R15, CHARGED as i32);
        self.a.mov(false, RCX, RAX);
        self.a.alu_imm(Alu::And, false, RCX, 0xFFFF);
        self.a.alu(Alu::Or, false, R15, RCX);
        self.a.mov(true, RCX, RAX);
        self.a.rot(Rot::Shr, true, RCX, 32);
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.a.load(false, R8, at(RDI, o.cpsr as i32));
        self.a.jmp(loaded);
    }

    fn refetch(&mut self, leave: Leave, specs: &[Spec], slow: Label, called: Label) -> Option<()> {
        let exchange = !matches!(leave, Leave::Branch | Leave::Restore);
        let thumb = self.a.label();
        if exchange {
            self.a.test_imm(RCX, 1);
        } else {
            self.a.test_imm(R8, FLAG_T);
        }
        self.a.jcc(cc::NE, thumb);
        self.a.test_imm(RCX, 3);
        self.a.jcc(cc::NE, slow);
        self.a.mov(false, RDX, RCX);
        if exchange {
            self.a.alu_imm(Alu::And, false, R8, !FLAG_T as i32);
        }
        self.fill(leave, specs, 4, slow, called)?;
        self.a.bind(thumb);
        self.a.mov(false, RDX, RCX);
        self.a.alu_imm(Alu::And, false, RDX, !1);
        if exchange {
            self.a.alu_imm(Alu::Or, false, R8, FLAG_T as i32);
        }
        self.fill(leave, specs, 2, slow, called)
    }

    fn quick_leave(&self, leave: Leave) -> Option<QuickFetch> {
        let branches = matches!(leave, Leave::Branch | Leave::Exchange);
        self.o
            .quick
            .filter(|_| branches && self.timed && !self.v5 && !self.pages)
    }

    fn quick_branch(&mut self, q: &QuickFetch, leave: Leave, slow: Label, called: Label) {
        let exchange = leave == Leave::Exchange;
        let thumb = self.a.label();
        if exchange {
            self.a.test_imm(RCX, 1);
        } else {
            self.a.test_imm(R8, FLAG_T);
        }
        self.a.jcc(cc::NE, thumb);
        self.a.test_imm(RCX, 3);
        self.a.jcc(cc::NE, slow);
        self.a.mov(false, RDX, RCX);
        if exchange {
            self.a.alu_imm(Alu::And, false, R8, !FLAG_T as i32);
        }
        let (bios, landed) = (self.a.label(), self.a.label());
        self.quick_iwram_arm(q, bios, slow);
        self.a.bind(landed);
        self.quick_landed(leave, 4, called);
        self.a.bind(bios);
        self.quick_bios_arm(q, slow);
        self.a.jmp(landed);
        self.a.bind(thumb);
        self.a.mov(false, RDX, RCX);
        self.a.alu_imm(Alu::And, false, RDX, !1);
        if exchange {
            self.a.alu_imm(Alu::Or, false, R8, FLAG_T as i32);
        }
        self.quick_rom_thumb(q, slow);
        self.quick_landed(leave, 2, called);
    }

    fn quick_iwram_arm(&mut self, q: &QuickFetch, other: Label, slow: Label) {
        self.a.mov(false, RAX, RDX);
        self.a.rot(Rot::Shr, false, RAX, 24);
        self.a.alu_imm(Alu::Cmp, false, RAX, q.iwram_page as i32);
        self.a.jcc(cc::NE, other);
        self.a.lea(false, RAX, at(RDX, 4));
        self.a.test_imm(RAX, PAGE_SIZE - 1);
        self.a.jcc(cc::E, slow);
        self.a.load(true, RSI, at(RSP, FRAME_FAST));
        self.a
            .load(true, RSI, at(RSI, offset_of!(Fast, pages) as i32));
        self.a.alu_imm(Alu::Cmp, true, RSI, 0);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, RAX, RDX);
        self.a.rot(Rot::Shr, false, RAX, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, RAX, 5);
        self.a.alu(Alu::Add, true, RSI, RAX);
        self.a
            .load(true, R9, at(RSI, offset_of!(Page, meta) as i32));
        self.a.test(R9, R9);
        self.a.jcc(cc::E, slow);
        self.a
            .load(true, RSI, at(RSI, offset_of!(Page, addend) as i32));
        self.quick_arm_words(q, q.open_iwram, slow);
    }

    fn quick_bios_arm(&mut self, q: &QuickFetch, slow: Label) {
        self.a
            .alu_imm(Alu::Cmp, false, RDX, (q.bios_end - 8) as i32);
        self.a.jcc(cc::A, slow);
        self.a.load(true, RSI, at(RSP, FRAME_FAST));
        self.a
            .load(true, RSI, at(RSI, offset_of!(Fast, bios) as i32));
        self.a.alu_imm(Alu::Cmp, true, RSI, 0);
        self.a.jcc(cc::E, slow);
        self.quick_arm_words(q, q.open_bios, slow);
    }

    fn quick_arm_words(&mut self, q: &QuickFetch, latch: i32, slow: Label) {
        let field = |offset: i32| at(RSI, offset);
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.load(false, R10, indexed(RSI, RDX, 1, 0));
        self.a.load(false, R14, indexed(RSI, RDX, 1, 4));
        self.a.load(true, RSI, at(RSP, FRAME_BUS));
        self.a.load(false, RAX, field(q.now));
        self.a.lea(true, R11, at(RAX, 1));
        self.a.load(false, R9, field(q.dma_check));
        self.a.alu(Alu::Cmp, true, R11, R9);
        self.a.jcc(cc::AE, slow);
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, slow);
        self.a.lea(false, RAX, at(RAX, 2));
        self.a.store(false, field(q.now), RAX);
        self.a.store(false, field(q.sample), RAX);
        self.a.alu_mem_imm_wide(Alu::Add, field(q.clock), 2);
        self.a.store8_imm(field(q.branched), 0);
        self.a.lea(false, RAX, at(RDX, 4));
        self.a.store(false, field(q.open_addr), RAX);
        self.a.store(false, field(latch), R14);
        self.a.store(false, field(q.open_last), R14);
        self.a.mov_imm(RAX, 2);
    }

    fn quick_rom_thumb(&mut self, q: &QuickFetch, slow: Label) {
        let field = |offset: i32| at(RSI, offset);
        let bit = q.prefetch_bit.trailing_zeros();
        let page = q.rom_page - 1;
        let (top, done, none, glitched) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a
            .lea(false, RAX, at(RDX, q.rom_start.wrapping_neg() as i32));
        self.a.alu_imm(Alu::Cmp, false, RAX, q.rom_span as i32);
        self.a.jcc(cc::AE, slow);
        self.a.lea(false, RAX, at(RDX, 4));
        self.a.test_imm(RAX, page & !3);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, RBP, RDX);
        self.a.alu_imm(Alu::And, false, RBP, q.rom_mirror as i32);
        self.a.mov(false, RAX, RBP);
        self.a.alu_imm(Alu::Sub, false, RAX, q.rom_skip as i32);
        self.a.alu_imm(Alu::Add, true, RAX, 2);
        self.a.load(true, RSI, at(RSP, FRAME_BUS));
        self.a.alu_load_wide(Alu::Cmp, RAX, field(q.plain_rom_end));
        self.a.jcc(cc::AE, slow);
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, slow);
        self.a.alu_load(Alu::Cmp, RDX, field(q.head));
        self.a.jcc(cc::E, slow);
        self.a.test_mem8(field(q.stuck), 0xFF);
        self.a.jcc(cc::NE, slow);
        self.a.test_mem8(field(q.flowing), 0xFF);
        self.a.jcc(cc::E, none);
        self.a.load(true, R9, field(q.clock));
        self.a.load(false, R10, field(q.cost));
        self.a.alu_load(Alu::Sub, R10, field(q.progress));
        self.a.alu_load_wide(Alu::Add, R10, field(q.since));
        self.a.load(false, R11, field(q.count));
        self.a.load(false, R14, field(q.head));
        self.a.bind(top);
        self.a.alu(Alu::Cmp, true, R9, R10);
        self.a.jcc(cc::B, done);
        self.a.inc(R11);
        self.a.alu_imm(Alu::Cmp, false, R11, q.capacity as i32);
        self.a.jcc(cc::E, none);
        self.a.jcc(cc::A, slow);
        self.a.lea(false, RAX, indexed(R14, R11, 2, 0));
        self.a.test_imm(RAX, page);
        self.a.jcc(cc::E, slow);
        self.a.load(false, RAX, field(q.cost));
        self.a.alu(Alu::Add, true, R10, RAX);
        self.a.jmp(top);
        self.a.bind(done);
        self.a.alu(Alu::Sub, true, R10, R9);
        self.a.alu_imm(Alu::Cmp, true, R10, 1);
        self.a.mov_imm(R15, 0);
        self.a.setcc(cc::E, R15);
        self.a.jmp(glitched);
        self.a.bind(none);
        self.a.mov_imm(R15, 0);
        self.a.bind(glitched);
        self.a.mov(false, RAX, RDX);
        self.a.rot(Rot::Shr, false, RAX, 25);
        self.a.alu_imm(Alu::And, false, RAX, 3);
        self.a
            .load8(false, R9, indexed(RSI, RAX, 1, q.first_halves));
        self.a.alu(Alu::Add, false, R15, R9);
        self.a.load(false, R9, field(q.now));
        self.a.lea(true, R11, indexed(R9, R15, 1, 0));
        self.a.load(false, R9, field(q.dma_check));
        self.a.alu(Alu::Cmp, true, R11, R9);
        self.a.jcc(cc::AE, slow);
        self.a
            .load8(false, R9, indexed(RSI, RAX, 1, q.sequential_halves));
        self.a.load(true, RAX, at(RSP, FRAME_FAST));
        self.a
            .load(true, RAX, at(RAX, offset_of!(Fast, rom) as i32));
        self.a.alu_imm(Alu::Cmp, true, RAX, 0);
        self.a.jcc(cc::E, slow);
        self.a.load16(false, R10, indexed(RAX, RBP, 1, 0));
        self.a.load16(false, R14, indexed(RAX, RBP, 1, 2));
        self.a.lea(false, RAX, at(R11, 1));
        self.a.store(false, field(q.sample), RAX);
        self.a.lea(false, RAX, indexed(R11, R9, 1, 0));
        self.a.store(false, field(q.now), RAX);
        self.a.lea(false, RAX, at(RDX, 2));
        self.a.store(false, field(q.open_addr), RAX);
        self.a.imul_imm(RAX, R14, 0x0001_0001);
        self.a.store(false, field(q.open_last), RAX);
        self.a.load(true, RAX, field(q.clock));
        self.a.store(true, field(q.since), RAX);
        self.a.alu(Alu::Add, true, RAX, R9);
        self.a.store(true, field(q.due), RAX);
        self.a.store(false, field(q.cost), R9);
        self.a.mov(false, RAX, RDX);
        self.a.alu_imm(Alu::Or, false, RAX, page as i32);
        self.a.inc(RAX);
        self.a.rot(Rot::Shr, false, RAX, 25);
        self.a.alu_imm(Alu::And, false, RAX, 3);
        self.a
            .load8(false, RAX, indexed(RSI, RAX, 1, q.sequential_halves));
        self.a.store(false, field(q.page_cost), RAX);
        self.a.lea(false, RAX, at(RDX, 4));
        self.a.store(false, field(q.head), RAX);
        self.restart(q, RSI);
        self.a.lea(false, RAX, indexed(R15, R9, 1, 0));
    }

    fn quick_landed(&mut self, leave: Leave, step: u32, called: Label) {
        let o = self.o;
        self.a.alu_load(Alu::Add, RAX, at(RDI, o.data as i32));
        for field in [o.fetched, o.data, o.refetched] {
            self.a.store_imm(at(RDI, field as i32), 0);
        }
        self.refilled(leave, step, called);
    }

    fn refills(&self, leave: Leave, specs: &[Spec]) -> bool {
        if self.v5 || !self.pages {
            specs.iter().any(|spec| spec.code)
        } else {
            leave != Leave::Return
        }
    }

    fn fill(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        step: u32,
        slow: Label,
        called: Label,
    ) -> Option<()> {
        if self.v5 || !self.pages {
            return self.refill(leave, specs, step, slow, called);
        }
        self.refill_page(step, slow);
        self.refilled(leave, step, called);
        Some(())
    }

    fn refill_page(&mut self, step: u32, slow: Label) {
        let (entry, later) = if step == 2 {
            (HALF_CYCLES, HALF_SEQUENTIAL)
        } else {
            (WORD_CYCLES, SEQUENTIAL_CYCLES)
        };
        self.a.lea(false, R11, at(RDX, step as i32));
        self.a.test_imm(R11, PAGE_SIZE - 1);
        self.a.jcc(cc::E, slow);
        self.a.load(true, RSI, at(RSP, FRAME_FAST));
        self.a
            .load(true, RSI, at(RSI, offset_of!(Fast, pages) as i32));
        self.a.mov(false, RAX, RDX);
        self.a.rot(Rot::Shr, false, RAX, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, RAX, 5);
        self.a.alu(Alu::Add, true, RSI, RAX);
        self.a
            .load(true, R9, at(RSI, offset_of!(Page, meta) as i32));
        self.a.test(R9, R9);
        self.a.jcc(cc::E, slow);
        self.a
            .load(true, RSI, at(RSI, offset_of!(Page, addend) as i32));
        for (fetch, value) in [(RDX, R10), (R11, R14)] {
            let code = indexed(RSI, fetch, 1, 0);
            if step == 2 {
                self.a.load16(false, value, code);
            } else {
                self.a.load(false, value, code);
            }
        }
        self.a.mov(true, RAX, R9);
        if entry > 0 {
            self.a.rot(Rot::Shr, true, RAX, entry as u8);
        }
        self.a.alu_imm(Alu::And, false, RAX, 0xFF);
        self.a.rot(Rot::Shr, true, R9, later as u8);
        self.a.alu_imm(Alu::And, false, R9, 0xFF);
        self.a.alu(Alu::Add, false, RAX, R9);
    }

    fn refill(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        step: u32,
        slow: Label,
        called: Label,
    ) -> Option<()> {
        let returns = leave == Leave::Return;
        self.a.load(true, RSI, at(RSP, FRAME_FAST));
        for (index, spec) in specs.iter().enumerate().rev() {
            if !spec.code {
                continue;
            }
            let next = self.a.label();
            let window = offset_of!(Fast, windows) + index * size_of::<Window>();
            let field = |offset: usize| at(RSI, (window + offset) as i32);
            self.a.mov(false, RAX, RDX);
            self.a
                .alu_load(Alu::Sub, RAX, field(offset_of!(Window, base)));
            self.a.lea(true, R11, at(RAX, step as i32));
            self.a
                .alu_load_wide(Alu::Cmp, R11, field(offset_of!(Window, size)));
            self.a.jcc(cc::AE, next);
            let mask = ((spec.len - 1) & !(step - 1)) as i32;
            self.a.load(true, R9, field(offset_of!(Window, bytes)));
            for (fetch, value) in [(0, R10), (step as i32, R14)] {
                self.a.lea(false, R11, at(RAX, fetch));
                self.a.alu_imm(Alu::And, false, R11, mask);
                let code = indexed(R9, R11, 1, 0);
                if step == 2 {
                    self.a.load16(false, value, code);
                } else {
                    self.a.load(false, value, code);
                }
            }
            self.refill_cycles(spec, step, returns)?;
            self.refilled(leave, step, called);
            self.a.bind(next);
        }
        self.a.jmp(slow);
        Some(())
    }

    fn refilled(&mut self, leave: Leave, step: u32, called: Label) {
        let o = self.o;
        let exchange = !matches!(leave, Leave::Branch | Leave::Restore);
        self.a.lea(false, R11, at(RDX, 2 * step as i32));
        self.a.store(false, at(RDI, o.regs as i32 + PC_SLOT), R11);
        if self.v5 || !self.timed {
            self.a.store(false, at(RDI, o.landed as i32), RDX);
        }
        self.a.store(false, at(RDI, o.pipeline as i32), R10);
        self.a.store(false, at(RDI, o.pipeline as i32 + 4), R14);
        self.a
            .store8_imm(at(RDI, o.next_fetch as i32), SEQUENTIAL as u8);
        if exchange {
            self.a.store(false, at(RDI, o.cpsr as i32), R8);
        }
        self.a.mov(false, R11, R8);
        self.a
            .rot(Rot::Shr, false, R11, FLAG_I.trailing_zeros() as u8);
        self.a.alu_imm(Alu::And, false, R11, 1);
        self.a.store8(at(RDI, o.irq_masked as i32), R11);
        if leave == Leave::Return {
            self.a.rot(Rot::Shl, true, RBP, 32);
            self.a.alu(Alu::Or, true, RAX, RBP);
        }
        self.a.jmp(called);
    }

    fn refill_cycles(&mut self, spec: &Spec, step: u32, returns: bool) -> Option<()> {
        let word = spec.word[0];
        let done = self.a.label();
        let Some(cache) = spec.cache else {
            self.a.mov_imm(RAX, word);
            if returns {
                self.a.mov_imm(RBP, 0);
            }
            if step == 2 {
                self.a.lea(false, R11, at(RDX, 2));
                self.a.test_imm(R11, 2);
                self.a.jcc(cc::NE, done);
            }
            self.a.mov_imm(RAX, 2 * word);
            if returns {
                self.a.mov_imm(RBP, word);
            }
            self.a.bind(done);
            return Some(());
        };
        if !cache.code.is_power_of_two() {
            return None;
        }
        let (first, second) = (self.a.label(), self.a.label());
        self.a
            .load(true, R9, at(RSI, offset_of!(Fast, cache) as i32));
        self.a.mov(false, R11, RDX);
        self.a.rot(Rot::Shr, false, R11, cache.shift as u8);
        self.a.load8(false, R11, indexed(R9, R11, 1, 0));
        self.a.mov_imm(RAX, word);
        self.a.test_imm(R11, u32::from(cache.code));
        self.a.jcc(cc::E, first);
        self.a.mov_imm(RAX, cache.cycles[0]);
        self.a.bind(first);
        if returns {
            self.a.mov_imm(RBP, 0);
        }
        self.a.lea(false, RCX, at(RDX, step as i32));
        if step == 2 {
            self.a.test_imm(RCX, 2);
            self.a.jcc(cc::NE, done);
        }
        self.a.mov(false, R11, RCX);
        self.a.rot(Rot::Shr, false, R11, cache.shift as u8);
        self.a.load8(false, R11, indexed(R9, R11, 1, 0));
        self.a.mov_imm(RBP, word);
        self.a.test_imm(R11, u32::from(cache.code));
        self.a.jcc(cc::E, second);
        self.a.mov(false, R11, RCX);
        self.a.alu_imm(Alu::And, false, R11, cache.line as i32);
        self.a.mov_imm(RBP, cache.cycles[1]);
        self.a.jcc(cc::NE, second);
        self.a.mov_imm(RBP, cache.cycles[0]);
        self.a.bind(second);
        self.a.alu(Alu::Add, false, RAX, RBP);
        self.a.bind(done);
        Some(())
    }

    fn finish(&mut self, chained: bool) {
        self.a.mov(false, RCX, RSI);
        self.a.alu_imm(Alu::And, false, RCX, 0xFF);
        self.a.alu_load(Alu::Sub, RCX, at(RSP, FRAME_START));
        if chained {
            self.a.load(true, R11, at(RSP, FRAME_CTX));
            self.a.alu_load(Alu::Add, RCX, at(R11, self.o.acc as i32));
        }
        self.a.load(false, RAX, at(RSP, FRAME_BUDGET));
        self.a.alu(Alu::Sub, false, RAX, S);
        self.a.alu(Alu::Add, false, RAX, DEBT);
        self.a.rot(Rot::Shl, false, RBP, 31);
        self.a.alu(Alu::Or, false, RAX, RBP);
        self.a.rot(Rot::Shl, true, RCX, 32);
        self.a.alu(Alu::Or, true, RAX, RCX);
        self.a.alu_imm(Alu::Add, true, RSP, FRAME);
        for reg in CALLEE.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.ret();
    }

    fn hop_leave(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        hops: Option<(Label, Label)>,
    ) -> Option<()> {
        let Some((chained, finish)) = hops.filter(|_| self.refills(leave, specs)) else {
            return Some(());
        };
        let exchange = leave != Leave::Branch;
        let returns = leave == Leave::Return;
        let o = self.o;
        let (full, slow, thumb, hit, empty) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        let (called, refused, found_called, found_refused) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        let fast = |reg: R| FAST_FREE.contains(&reg) || (returns && reg == R13);
        let found = |reg: R| fast(reg) || FIND_FREE.contains(&reg);
        self.some_guests(false, fast);
        let linked = matches!(leave, Leave::Branch | Leave::Exchange)
            && (self.v5 || self.pages && !self.timed);
        if linked {
            self.hop_linked(fast, exchange, refused);
        }
        if exchange {
            self.a.store(false, at(RSP, FRAME_KEY), RCX);
        }
        if returns {
            self.a.mov(false, R13, RDX);
        }
        if exchange {
            self.a.test_imm(RCX, 1);
        } else {
            self.a.load(false, RDX, self.core(o.cpsr));
            self.a.test_imm(RDX, FLAG_T);
        }
        self.a.jcc(cc::NE, thumb);
        if exchange {
            self.a.test_imm(RCX, 3);
            self.a.jcc(cc::NE, slow);
        }
        self.a.mov(false, RDX, RCX);
        self.hop_fill(specs, 4, slow, hit, exchange, returns)?;
        self.a.bind(thumb);
        self.a.mov(false, RDX, RCX);
        self.a.alu_imm(Alu::And, false, RDX, !1);
        self.hop_fill(specs, 2, slow, hit, exchange, returns)?;
        self.a.bind(slow);
        if returns {
            self.a.mov(false, RDX, R13);
        }
        self.some_guests(true, fast);
        self.a.jmp(full);
        self.a.bind(hit);
        if returns {
            self.overlap(RDI, R13, RCX);
            self.a.alu(Alu::Add, false, R14, RDI);
        }
        if linked {
            self.a.load(true, RDI, at(RSP, FRAME_CTX));
            self.a.store(false, at(RDI, o.hop as i32 + 4), R14);
            self.a.load(false, RCX, self.core(o.cpsr));
            self.a
                .rot(Rot::Shr, false, RCX, FLAG_T.trailing_zeros() as u8);
            self.a.alu_imm(Alu::And, false, RCX, 1);
            self.a.alu(Alu::Or, false, RCX, RDX);
            self.a.store(false, at(RDI, o.hop as i32), RCX);
        }
        self.a.load(false, RCX, self.core(o.cpsr));
        self.a
            .rot(Rot::Shr, false, RCX, FLAG_I.trailing_zeros() as u8);
        self.a.alu_imm(Alu::And, false, RCX, 1);
        self.a.store8(self.core(o.irq_masked), RCX);
        self.a.alu(Alu::Sub, false, S, R14);
        self.settle(R14, DEBT, RCX);
        self.a.store(false, self.core(o.cycles), R14);
        self.a.store8_imm(self.core(o.irq_held), 0);
        self.store_debt(RCX, 0, DEBT);
        self.hop(called, refused, empty, exchange);
        self.a.mov(false, R8, RAX);
        self.a.alu_imm(Alu::And, false, R8, IMAGE_MASK);
        self.enter(R8);
        self.some_guests(true, fast);
        self.a.mov_imm(RCX, KEEP);
        self.a.jmp_reg(RDX);
        self.a.bind(empty);
        self.some_guests(false, |reg| FIND_FREE.contains(&reg) && !fast(reg));
        self.a.mov(false, R14, RAX);
        self.find(found_called, found_refused);
        self.a.mov(false, R8, R14);
        self.a.alu_imm(Alu::And, false, R8, IMAGE_MASK);
        self.enter(R8);
        self.some_guests(true, found);
        self.a.mov_imm(RCX, KEEP);
        self.a.jmp_reg(RDX);
        for (from, to, spilled) in [
            (called, chained, false),
            (refused, finish, false),
            (found_called, chained, true),
            (found_refused, finish, true),
        ] {
            self.a.bind(from);
            self.a.load(true, RDI, at(RSP, FRAME_CORE));
            self.a.lea(true, REGS, at(RDI, o.regs as i32));
            if spilled {
                self.a.mov(false, RAX, R14);
                self.some_guests(false, |reg| !found(reg));
            } else {
                self.some_guests(false, |reg| !fast(reg));
            }
            self.store_cpsr(RDI, RDX, R8);
            self.a.mov_imm(RBP, 0);
            self.a.jmp(to);
        }
        self.a.bind(full);
        Some(())
    }

    fn core(&self, offset: u32) -> Mem {
        at(REGS, offset as i32 - self.o.regs as i32)
    }

    fn hop_fill(
        &mut self,
        specs: &[Spec],
        step: u32,
        slow: Label,
        hit: Label,
        exchange: bool,
        returns: bool,
    ) -> Option<()> {
        if self.v5 || !self.pages {
            return self.hop_refill(specs, step, slow, hit, exchange, returns);
        }
        self.hop_page(step, slow, hit, exchange);
        Some(())
    }

    fn hop_page(&mut self, step: u32, slow: Label, hit: Label, exchange: bool) {
        let o = self.o;
        let (entry, later) = if step == 2 {
            (HALF_CYCLES, HALF_SEQUENTIAL)
        } else {
            (WORD_CYCLES, SEQUENTIAL_CYCLES)
        };
        self.a.lea(false, R14, at(RDX, step as i32));
        self.a.test_imm(R14, PAGE_SIZE - 1);
        self.a.jcc(cc::E, slow);
        self.a.load(true, RDI, at(RSP, FRAME_FAST));
        self.a
            .load(true, RDI, at(RDI, offset_of!(Fast, pages) as i32));
        self.a.mov(false, R8, RDX);
        self.a.rot(Rot::Shr, false, R8, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, R8, 5);
        self.a.alu(Alu::Add, true, RDI, R8);
        self.a
            .load(true, R8, at(RDI, offset_of!(Page, meta) as i32));
        self.a.test(R8, R8);
        self.a.jcc(cc::E, slow);
        self.a
            .load(true, RDI, at(RDI, offset_of!(Page, addend) as i32));
        for (fetch, slot) in [(RDX, 0), (R14, 4)] {
            let code = indexed(RDI, fetch, 1, 0);
            if step == 2 {
                self.a.load16(false, RCX, code);
            } else {
                self.a.load(false, RCX, code);
            }
            self.a.store(false, self.core(o.pipeline + slot), RCX);
        }
        self.a.mov(true, R14, R8);
        if entry > 0 {
            self.a.rot(Rot::Shr, true, R14, entry as u8);
        }
        self.a.alu_imm(Alu::And, false, R14, 0xFF);
        self.a.rot(Rot::Shr, true, R8, later as u8);
        self.a.alu_imm(Alu::And, false, R8, 0xFF);
        self.a.alu(Alu::Add, false, R14, R8);
        self.hop_landed(step, hit, exchange);
    }

    fn hop_landed(&mut self, step: u32, hit: Label, exchange: bool) {
        let o = self.o;
        self.a.lea(false, RCX, at(RDX, 2 * step as i32));
        self.a.store(false, at(REGS, PC_SLOT), RCX);
        if self.v5 || !self.timed {
            self.a.store(false, self.core(o.landed), RDX);
        }
        self.a.store8_imm(self.core(o.next_fetch), SEQUENTIAL as u8);
        if exchange {
            self.a.load(false, RCX, self.core(o.cpsr));
            if step == 2 {
                self.a.alu_imm(Alu::Or, false, RCX, FLAG_T as i32);
            } else {
                self.a.alu_imm(Alu::And, false, RCX, !FLAG_T as i32);
            }
            self.a.store(false, self.core(o.cpsr), RCX);
        }
        self.a.jmp(hit);
    }

    fn hop_refill(
        &mut self,
        specs: &[Spec],
        step: u32,
        slow: Label,
        hit: Label,
        exchange: bool,
        returns: bool,
    ) -> Option<()> {
        let o = self.o;
        self.a.load(true, RDI, at(RSP, FRAME_FAST));
        for (index, spec) in specs.iter().enumerate().rev() {
            if !spec.code {
                continue;
            }
            let next = self.a.label();
            let window = offset_of!(Fast, windows) + index * size_of::<Window>();
            let field = |offset: usize| at(RDI, (window + offset) as i32);
            self.a.mov(false, R8, RDX);
            self.a
                .alu_load(Alu::Sub, R8, field(offset_of!(Window, base)));
            self.a.lea(true, R14, at(R8, step as i32));
            self.a
                .alu_load_wide(Alu::Cmp, R14, field(offset_of!(Window, size)));
            self.a.jcc(cc::AE, next);
            let mask = ((spec.len - 1) & !(step - 1)) as i32;
            self.a.load(true, RCX, field(offset_of!(Window, bytes)));
            for (fetch, value, slot) in [(0, R14, 0), (step as i32, R8, 4)] {
                self.a.lea(false, value, at(R8, fetch));
                self.a.alu_imm(Alu::And, false, value, mask);
                let code = indexed(RCX, value, 1, 0);
                if step == 2 {
                    self.a.load16(false, value, code);
                } else {
                    self.a.load(false, value, code);
                }
                self.a.store(false, self.core(o.pipeline + slot), value);
            }
            self.hop_cycles(spec, step, returns)?;
            self.hop_landed(step, hit, exchange);
            self.a.bind(next);
        }
        self.a.jmp(slow);
        Some(())
    }

    fn hop_cycles(&mut self, spec: &Spec, step: u32, returns: bool) -> Option<()> {
        let word = spec.word[0];
        let done = self.a.label();
        let Some(cache) = spec.cache else {
            self.a.mov_imm(R14, word);
            if returns {
                self.a.mov_imm(RDI, 0);
            }
            if step == 2 {
                self.a.lea(false, RCX, at(RDX, 2));
                self.a.test_imm(RCX, 2);
                self.a.jcc(cc::NE, done);
            }
            self.a.mov_imm(R14, 2 * word);
            if returns {
                self.a.mov_imm(RDI, word);
            }
            self.a.bind(done);
            return Some(());
        };
        if !cache.code.is_power_of_two() {
            return None;
        }
        let (first, second, none) = (self.a.label(), self.a.label(), self.a.label());
        self.a
            .load(true, RDI, at(RDI, offset_of!(Fast, cache) as i32));
        self.a.mov(false, RCX, RDX);
        self.a.rot(Rot::Shr, false, RCX, cache.shift as u8);
        self.a.load8(false, RCX, indexed(RDI, RCX, 1, 0));
        self.a.mov_imm(R14, word);
        self.a.test_imm(RCX, u32::from(cache.code));
        self.a.jcc(cc::E, first);
        self.a.mov_imm(R14, cache.cycles[0]);
        self.a.bind(first);
        self.a.lea(false, R8, at(RDX, step as i32));
        if step == 2 {
            self.a.test_imm(R8, 2);
            self.a.jcc(cc::NE, if returns { none } else { done });
        }
        self.a.mov(false, RCX, R8);
        self.a.rot(Rot::Shr, false, RCX, cache.shift as u8);
        self.a.load8(false, RCX, indexed(RDI, RCX, 1, 0));
        self.a.mov_imm(RDI, word);
        self.a.test_imm(RCX, u32::from(cache.code));
        self.a.jcc(cc::E, second);
        self.a.mov(false, RCX, R8);
        self.a.alu_imm(Alu::And, false, RCX, cache.line as i32);
        self.a.mov_imm(RDI, cache.cycles[1]);
        self.a.jcc(cc::NE, second);
        self.a.mov_imm(RDI, cache.cycles[0]);
        self.a.bind(second);
        self.a.alu(Alu::Add, false, R14, RDI);
        if returns && step == 2 {
            self.a.jmp(done);
            self.a.bind(none);
            self.a.mov_imm(RDI, 0);
        }
        self.a.bind(done);
        Some(())
    }

    fn chain(&mut self, chain: usize, finish: Label, called: Label) {
        let o = self.o;
        let (refused, linked, empty) = (self.a.label(), self.a.label(), self.a.label());
        self.a.test(RBP, RBP);
        self.a.jcc(cc::NE, finish);
        self.hop(called, finish, empty, false);
        self.a.jmp(linked);
        self.a.bind(empty);
        self.find(called, finish);
        self.a.jmp(linked);
        self.a.bind(called);
        for reg in [RSI, S, DEBT] {
            self.a.push(reg);
        }
        self.a.alu_imm(Alu::Sub, true, RSP, 8);
        self.a.load(true, RDI, at(RSP, 32 + FRAME_CTX));
        self.a.mov(false, RDX, RSI);
        self.a.alu_imm(Alu::And, false, RDX, 0xFF);
        self.a.mov(false, RSI, S);
        self.a.alu(Alu::Sub, false, RSI, DEBT);
        self.call(chain);
        self.a.alu_imm(Alu::Add, true, RSP, 8);
        for reg in [DEBT, S, RSI] {
            self.a.pop(reg);
        }
        self.a.alu(Alu::Or, true, RAX, RAX);
        self.a.jcc(cc::E, refused);
        self.adopt(R8);
        self.a.bind(linked);
        self.a.load(false, RSI, at(RDI, o.cpsr as i32));
        self.image(R8, RSI);
        self.enter(R8);
        self.guests(true);
        self.a.mov_imm(RCX, KEEP);
        self.a.jmp_reg(RDX);
        self.a.bind(refused);
        self.a.jmp(finish);
    }

    fn find(&mut self, called: Label, refused: Label) {
        let o = self.o;
        self.a.load(true, R11, at(RSP, FRAME_CTX));
        self.a.load(true, RDX, at(R11, o.stall_flag as i32));
        self.a.test(RDX, RDX);
        self.a.jcc(cc::E, called);
        self.a.alu_mem_imm(Alu::Cmp, at(RDX, 0), 0);
        self.a.jcc(cc::NE, called);
        self.a.load(true, RAX, at(R11, o.slots as i32));
        self.a.test(RAX, RAX);
        self.a.jcc(cc::E, called);
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        if !self.v5 {
            self.a.load8(false, RCX, at(RDI, o.ldm_glitch as i32));
            self.a.test(RCX, RCX);
            self.a.jcc(cc::NE, refused);
        }
        self.a.load(false, RCX, at(RDI, o.cpsr as i32));
        self.a
            .rot(Rot::Shr, false, RCX, FLAG_T.trailing_zeros() as u8);
        self.a.alu_imm(Alu::And, false, RCX, 1);
        self.a.load(false, RBX, at(RDI, o.regs as i32 + PC_SLOT));
        self.a.mov_imm(RDX, 8);
        self.a.rot_cl(Rot::Shr, false, RDX);
        self.a.mov(false, R8, RBX);
        self.a.alu(Alu::Sub, false, R8, RDX);
        self.a.mov_imm(RDX, 3);
        self.a.rot_cl(Rot::Shr, false, RDX);
        self.a.test(R8, RDX);
        self.a.jcc(cc::NE, refused);
        self.a.alu(Alu::Or, false, R8, RCX);
        self.a.imul_imm(RDX, R8, HASH as i32);
        self.a.rot(Rot::Shr, false, RDX, HASH_SHIFT as u8);
        self.a.imul_imm(RDX, RDX, SLOT_SIZE as i32);
        self.a.alu(Alu::Add, true, RDX, RAX);
        self.a.alu_imm(Alu::Xor, false, R8, NO_KEY as i32);
        self.a.alu_load(Alu::Cmp, R8, at(RDX, SLOT_TAG as i32));
        self.a.jcc(cc::NE, refused);
        self.a.load(true, RAX, at(RDI, o.pipeline as i32));
        self.a
            .alu_load_wide(Alu::Cmp, RAX, at(RDX, SLOT_PIPELINE as i32));
        self.a.jcc(cc::NE, refused);
        self.a.load(true, RAX, at(RDX, SLOT_CODE as i32));
        self.a.test_imm(RAX, 1);
        self.a.jcc(cc::E, refused);
        self.a.alu_imm(Alu::And, true, RAX, -2);
        self.a.load(false, R8, at(RDX, SLOT_START as i32));
        self.a.load(false, R12, indexed(RAX, R8, 4, 0));
        self.a.mov(false, R13, R12);
        self.a.rot(Rot::Shr, false, R13, 27);
        self.a.mov(false, R11, S);
        self.a.alu(Alu::Sub, false, R11, DEBT);
        self.a.alu(Alu::Cmp, false, R11, R13);
        self.a.jcc(cc::LE, refused);
        self.a.load(true, R11, at(RSP, FRAME_CTX));
        self.a.store(true, at(R11, o.slot as i32), RDX);
        self.a.mov(false, R13, RSI);
        self.a.alu_imm(Alu::And, false, R13, 0xFF);
        self.a.alu_load(Alu::Sub, R13, at(R11, o.budget as i32 + 4));
        self.a.alu_store(Alu::Add, at(R11, o.acc as i32), R13);
        self.a.store(false, at(R11, o.budget as i32 + 4), R8);
        self.a.store(false, at(RSP, FRAME_START), R8);
        self.a.load(true, R13, at(RDX, SLOT_WORDS as i32));
        self.a.store(true, at(RSP, FRAME_WORDS), R13);
        self.a.neg(RCX);
        self.a.alu_imm(Alu::Add, false, RCX, 2);
        self.a.rot_cl(Rot::Shl, false, R8);
        self.a.alu(Alu::Sub, false, RBX, R8);
        self.a.store(false, at(RSP, FRAME_BASE), RBX);
        self.a.store(false, at(RSP, FRAME_SHIFT), RCX);
        self.a.alu_mem_imm(Alu::Add, at(R11, o.chains as i32), 1);
        self.a.mov(false, RDX, R12);
        self.a.rot(Rot::Shr, false, RDX, 16);
        self.a.alu_imm(Alu::And, false, RDX, 0x7FF);
        self.a.alu(Alu::Add, false, S, RDX);
        self.a.mov(false, RCX, R12);
        self.a.rot(Rot::Shr, false, RCX, 7);
        self.a.alu_imm(Alu::And, false, RCX, 0x100);
        self.a.alu_imm(Alu::And, false, R12, CODE_LIMIT as i32);
        self.a.lea(true, RDX, indexed(RAX, R12, 1, 0));
        self.a.lea(true, REGS, at(RDI, o.regs as i32));
    }

    fn hop_linked(&mut self, fast: impl Fn(R) -> bool, exchange: bool, refused: Label) {
        let o = self.o;
        let link = |field: usize| field as i32;
        let normal = self.a.label();
        self.a.load8(false, RDX, at(RSP, FRAME_CELL));
        self.a.test(RDX, RDX);
        self.a.jcc(cc::E, normal);
        self.a.load(true, RDI, at(RSP, FRAME_CTX));
        self.a.load(true, R8, at(RDI, o.slot as i32));
        self.a.rot(Rot::Shl, false, RDX, LINK_SHIFT as u8);
        self.a.lea(true, R8, indexed(R8, RDX, 1, CELL_BASE));
        self.a.load(true, R14, at(R8, link(offset_of!(Link, slot))));
        self.a.alu(Alu::Or, true, R14, R14);
        self.a.jcc(cc::E, normal);
        self.a
            .load(false, RDX, at(R8, link(offset_of!(Link, generation))));
        self.a
            .alu_load(Alu::Cmp, RDX, at(R14, SLOT_GENERATION as i32));
        self.a.jcc(cc::NE, normal);
        if exchange {
            self.a.mov(false, RDX, RCX);
            self.a.alu_imm(Alu::Xor, false, RDX, NO_KEY as i32);
            self.a.alu_load(Alu::Cmp, RDX, at(R14, SLOT_TAG as i32));
            self.a.jcc(cc::NE, normal);
        }
        self.a.load(true, RDX, at(RDI, o.stall_flag as i32));
        self.a.alu_mem_imm(Alu::Cmp, at(RDX, 0), 0);
        self.a.jcc(cc::NE, normal);
        if !self.v5 {
            self.a.load8(false, RDX, self.core(o.ldm_glitch));
            self.a.test(RDX, RDX);
            self.a.jcc(cc::NE, normal);
        }
        self.a
            .load(false, RDX, at(R8, link(offset_of!(Link, cost))));
        let (committed, over) = (self.a.label(), self.a.label());
        self.a.alu(Alu::Sub, false, S, RDX);
        self.settle(RDX, DEBT, RDI);
        self.a
            .load(false, RDI, at(R8, link(offset_of!(Link, reach))));
        self.a.alu(Alu::Add, false, RDI, DEBT);
        self.a.alu(Alu::Cmp, false, S, RDI);
        self.a.jcc(cc::G, committed);
        self.a
            .alu_mem_imm(Alu::Cmp, at(R8, link(offset_of!(Link, cost))), -1);
        self.a.jcc(cc::NE, over);
        self.a
            .load(false, RDI, at(R8, link(offset_of!(Link, cost))));
        self.a.alu(Alu::Sub, false, RDI, RDX);
        self.a.alu(Alu::Add, false, DEBT, RDI);
        self.a
            .alu_load(Alu::Add, S, at(R8, link(offset_of!(Link, cost))));
        self.a.jmp(normal);
        self.a.bind(committed);
        self.land_linked(exchange);
        self.a.load(true, RDI, at(RSP, FRAME_CTX));
        self.a.mov(false, RDX, RSI);
        self.a.alu_imm(Alu::And, false, RDX, 0xFF);
        self.a.alu_load(Alu::Sub, RDX, at(RDI, o.budget as i32 + 4));
        self.a.alu_store(Alu::Add, at(RDI, o.acc as i32), RDX);
        self.a
            .load(false, RDX, at(R8, link(offset_of!(Link, start))));
        self.a.store(false, at(RDI, o.budget as i32 + 4), RDX);
        self.a.store(false, at(RSP, FRAME_START), RDX);
        self.a
            .load(true, RDX, at(R8, link(offset_of!(Link, words))));
        self.a.store(true, at(RSP, FRAME_WORDS), RDX);
        self.a.load(true, RDX, at(R8, link(offset_of!(Link, pc))));
        self.a.store(true, at(RSP, FRAME_BASE), RDX);
        self.a.alu_mem_imm(Alu::Add, at(RDI, o.chains as i32), 1);
        self.a.store(true, at(RDI, o.slot as i32), R14);
        self.a
            .alu_load(Alu::Add, S, at(R8, link(offset_of!(Link, pending))));
        self.a
            .load(false, RCX, at(R8, link(offset_of!(Link, inverted))));
        self.a.load(true, RDX, at(R8, link(offset_of!(Link, jump))));
        self.a.mov(false, R8, RAX);
        self.a.alu_imm(Alu::And, false, R8, IMAGE_MASK);
        self.enter(R8);
        self.some_guests(true, fast);
        self.a.mov_imm(RCX, KEEP);
        self.a.jmp_reg(RDX);
        self.a.bind(over);
        self.land_linked(exchange);
        self.a.jmp(refused);
        self.a.bind(normal);
    }

    fn land_linked(&mut self, exchange: bool) {
        let o = self.o;
        let link = |field: usize| field as i32;
        self.a.store(false, self.core(o.cycles), RDX);
        self.a.store8_imm(self.core(o.irq_held), 0);
        self.store_debt(RDX, 0, DEBT);
        self.a.store8_imm(at(RSP, FRAME_CELL), 0);
        if exchange {
            self.a.mov(false, RDX, RCX);
            self.a.alu_imm(Alu::And, false, RDX, 1);
            self.a
                .rot(Rot::Shl, false, RDX, FLAG_T.trailing_zeros() as u8);
            self.a
                .alu_mem_imm(Alu::And, self.core(o.cpsr), !FLAG_T as i32);
            self.a.alu_store(Alu::Or, self.core(o.cpsr), RDX);
            self.a.alu_imm(Alu::And, false, RCX, !1);
        }
        self.a.load(true, RDX, at(R14, SLOT_PIPELINE as i32));
        self.a.store(true, self.core(o.pipeline), RDX);
        self.a
            .load(false, RDX, at(R8, link(offset_of!(Link, pc) + 4)));
        self.a.lea(false, RDX, indexed(RCX, RDX, 4, 0));
        self.a.store(false, at(REGS, PC_SLOT), RDX);
        self.a.store(false, self.core(o.landed), RCX);
        self.a.store8_imm(self.core(o.next_fetch), SEQUENTIAL as u8);
        self.a.load(false, RCX, self.core(o.cpsr));
        self.a
            .rot(Rot::Shr, false, RCX, FLAG_I.trailing_zeros() as u8);
        self.a.alu_imm(Alu::And, false, RCX, 1);
        self.a.store8(self.core(o.irq_masked), RCX);
    }

    fn hop(&mut self, called: Label, refused: Label, empty: Label, checked: bool) {
        let o = self.o;
        let link = |field: usize| field as i32;
        self.a.load8(false, RCX, at(RSP, FRAME_CELL));
        self.a.store8_imm(at(RSP, FRAME_CELL), 0);
        self.a.test(RCX, RCX);
        self.a.jcc(cc::E, empty);
        self.a.load(true, R11, at(RSP, FRAME_CTX));
        self.a.load(true, RDI, at(R11, o.slot as i32));
        self.a.mov(false, R8, RCX);
        self.a.rot(Rot::Shl, false, R8, LINK_SHIFT as u8);
        self.a.lea(true, RDI, indexed(RDI, R8, 1, CELL_BASE));
        self.a.load(true, R8, at(RDI, link(offset_of!(Link, slot))));
        self.a.alu(Alu::Or, true, R8, R8);
        self.a.jcc(cc::E, called);
        self.a
            .load(false, RDX, at(RDI, link(offset_of!(Link, generation))));
        self.a
            .alu_load(Alu::Cmp, RDX, at(R8, SLOT_GENERATION as i32));
        self.a.jcc(cc::NE, called);
        if checked {
            self.a.load(false, RDX, at(RSP, FRAME_KEY));
            self.a.alu_imm(Alu::Xor, false, RDX, NO_KEY as i32);
            self.a.alu_load(Alu::Cmp, RDX, at(R8, SLOT_TAG as i32));
            let keyed = self.a.label();
            self.a.jcc(cc::E, keyed);
            self.a.load(true, RDI, at(RSP, FRAME_CORE));
            self.a.lea(true, REGS, at(RDI, o.regs as i32));
            self.a.jmp(empty);
            self.a.bind(keyed);
        }
        self.a.load(true, RDX, at(R11, o.stall_flag as i32));
        self.a.alu_mem_imm(Alu::Cmp, at(RDX, 0), 0);
        self.a.jcc(cc::NE, called);
        self.a.load(true, RDX, at(RSP, FRAME_CORE));
        if !self.v5 {
            self.a.load8(false, R11, at(RDX, o.ldm_glitch as i32));
            self.a.test(R11, R11);
            self.a.jcc(cc::NE, called);
        }
        self.a.load(true, RDX, at(RDX, o.pipeline as i32));
        self.a
            .alu_load_wide(Alu::Cmp, RDX, at(R8, SLOT_PIPELINE as i32));
        self.a.jcc(cc::NE, called);
        self.a
            .load(false, RDX, at(RDI, link(offset_of!(Link, ahead))));
        self.a.mov(false, R11, S);
        self.a.alu(Alu::Sub, false, R11, DEBT);
        self.a.alu(Alu::Cmp, false, R11, RDX);
        self.a.jcc(cc::LE, refused);
        self.a.load(true, R11, at(RSP, FRAME_CTX));
        self.a.mov(false, RDX, RSI);
        self.a.alu_imm(Alu::And, false, RDX, 0xFF);
        self.a.alu_load(Alu::Sub, RDX, at(R11, o.budget as i32 + 4));
        self.a.alu_store(Alu::Add, at(R11, o.acc as i32), RDX);
        self.a
            .load(false, RDX, at(RDI, link(offset_of!(Link, start))));
        self.a.store(false, at(R11, o.budget as i32 + 4), RDX);
        self.a.store(false, at(RSP, FRAME_START), RDX);
        self.a
            .load(true, RDX, at(RDI, link(offset_of!(Link, words))));
        self.a.store(true, at(RSP, FRAME_WORDS), RDX);
        self.a.load(true, RDX, at(RDI, link(offset_of!(Link, pc))));
        self.a.store(true, at(RSP, FRAME_BASE), RDX);
        self.a.alu_mem_imm(Alu::Add, at(R11, o.chains as i32), 1);
        self.a.store(true, at(R11, o.slot as i32), R8);
        self.a
            .alu_load(Alu::Add, S, at(RDI, link(offset_of!(Link, pending))));
        self.a
            .load(false, RCX, at(RDI, link(offset_of!(Link, inverted))));
        self.a
            .load(true, RDX, at(RDI, link(offset_of!(Link, jump))));
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.a.lea(true, REGS, at(RDI, o.regs as i32));
    }

    fn adopt(&mut self, scratch: R) {
        let o = self.o;
        self.a.mov(true, RDX, RAX);
        self.a.rot(Rot::Shr, true, RDX, 48);
        self.a.alu_imm(Alu::And, false, RDX, 0x7FF);
        self.a.alu(Alu::Add, false, S, RDX);
        self.a.mov(true, RCX, RAX);
        self.a.rot(Rot::Shr, true, RCX, HOP_INVERTED as u8 - 8);
        self.a.alu_imm(Alu::And, false, RCX, 0x100);
        self.a.mov_imm64(RDX, HOP_TARGET);
        self.a.alu(Alu::And, true, RDX, RAX);
        self.a.load(true, R11, at(RSP, FRAME_CTX));
        for (from, to, wide) in [
            (o.budget as i32 + 4, FRAME_START, false),
            (o.words as i32, FRAME_WORDS, true),
            (o.base as i32, FRAME_BASE, false),
            (o.base as i32 + 4, FRAME_SHIFT, false),
        ] {
            self.a.load(wide, scratch, at(R11, from));
            self.a.store(wide, at(RSP, to), scratch);
        }
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.a.lea(true, REGS, at(RDI, o.regs as i32));
    }

    fn enter(&mut self, image: R) {
        self.a.alu(Alu::Xor, false, RCX, image);
        self.a.mov(false, RAX, RCX);
        self.a.al_imm(Alu::Cmp, 0x81);
        self.a.sahf();
        self.a.mov(false, RAX, image);
    }

    fn memory(&mut self, helper: usize, store: bool, pages: bool, ram: bool) -> Option<Label> {
        let tail = self.a.label();
        let tails = Tails {
            any: self.a.label(),
            short: self.a.label(),
        };
        let saved = [RAX, RDI, R8, S, DEBT, RSI];
        let frame = 8 + 8 * saved.len() as i32 + 8;
        self.a.mov(false, R11, RSI);
        self.a.rot(Rot::Shr, false, R11, 16);
        self.a.alu_imm(Alu::And, false, R11, 0x3FF);
        self.a.alu(Alu::Sub, false, S, R11);
        let quick_ram = match self.o.quick {
            Some(q) if ram && !self.v5 && !pages => Some(q),
            _ => None,
        };
        let ram_tail = quick_ram.map(|q| self.ram(&q, store));
        if pages {
            let (slow, half, word) = (self.a.label(), self.a.label(), self.a.label());
            let bypass = self.a.label();
            if !self.v5 {
                self.a.test_imm(RSI, 3 << CODE_CLASS);
                self.a.jcc(cc::E, bypass);
            }
            let fast_saved = if store {
                &FAST_SAVED[..]
            } else {
                &FAST_LOAD_SAVED[..]
            };
            for &reg in fast_saved {
                self.a.push(reg);
            }
            let fast = 8 + 8 * fast_saved.len() as i32;
            self.page(store, fast, slow);
            self.a.test_imm(RSI, 2);
            self.a.jcc(cc::NE, word);
            self.a.test_imm(RSI, 1);
            self.a.jcc(cc::NE, half);
            self.paged(store, Width::Byte, &tails, slow);
            self.a.bind(half);
            self.paged(store, Width::Half, &tails, slow);
            self.a.bind(word);
            self.paged(store, Width::Word, &tails, slow);
            self.a.bind(slow);
            for reg in fast_saved.iter().rev() {
                self.a.pop(*reg);
            }
            self.a.bind(bypass);
        }
        for reg in saved {
            self.a.push(reg);
        }
        self.a.alu_imm(Alu::Sub, true, RSP, 8);
        if !self.v5 {
            self.a.mov(false, R11, RCX);
            self.store_pc(frame);
            self.a.mov(false, RCX, R11);
        }
        if store {
            self.a.xchg(false, RCX, RDX);
            self.a.mov(false, R8, RSI);
            self.a.alu_imm(Alu::And, false, R8, 7);
        } else {
            self.a.mov(false, RDX, RCX);
            self.a.mov(false, RCX, RSI);
            self.a.alu_imm(Alu::And, false, RCX, 7);
        }
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a.load(true, RSI, at(RSP, frame + FRAME_BUS));
        self.call(helper);
        self.a.alu_imm(Alu::Add, true, RSP, 8);
        for reg in saved[1..].iter().rev() {
            self.a.pop(*reg);
        }
        self.a.bind(tail);
        let frame = 16;
        self.a.mov(false, RCX, RAX);
        self.a.alu_imm(Alu::And, false, RCX, CYCLES_MASK);
        if self.v5 {
            self.a.extend16(false, RDX, RSI);
            self.a.rot(Rot::Shr, false, RDX, 8);
            self.a
                .lea(false, R11, indexed(RDX, RCX, 1, -(OVERLAP as i32)));
            self.a.alu(Alu::Cmp, false, RDX, RCX);
            self.a.cmov(cc::B, RDX, RCX);
            self.a.alu(Alu::Cmp, false, R11, RDX);
            self.a.cmov(cc::G, RDX, R11);
            self.a.alu(Alu::Sub, false, S, RDX);
            self.settle(RDX, DEBT, RCX);
            self.a.store(false, at(RSP, frame + FRAME_LAST), RDX);
        } else {
            self.a.alu(Alu::Sub, false, S, RCX);
            if self.timed {
                self.a.alu_load(Alu::Add, RCX, at(RSP, frame + FRAME_LAST));
            }
            self.a.store(false, at(RSP, frame + FRAME_LAST), RCX);
        }
        self.a.mov(false, RDX, RAX);
        self.a.rot(Rot::Shr, false, RDX, 31);
        self.a.store(false, at(RSP, frame + FRAME_FORCE), RDX);
        self.a.rot(Rot::Shr, true, RAX, 32);
        self.a.mov(false, RCX, RAX);
        self.spent();
        self.resume(store)?;
        if pages && self.v5 {
            self.a.bind(tails.any);
            self.quick(store, false)?;
            self.a.bind(tails.short);
            self.quick(store, true)?;
        } else if pages {
            self.a.bind(tails.short);
            self.quick_v4(store)?;
        }
        if let Some(ram_tail) = ram_tail {
            self.a.bind(ram_tail);
            self.ram_spent(store)?;
        }
        Some(tail)
    }

    fn ram(&mut self, q: &QuickFetch, store: bool) -> Label {
        let (slow, tail) = (self.a.label(), self.a.label());
        let (half, word) = (self.a.label(), self.a.label());
        let rom = (!store && self.timed).then(|| self.a.label());
        for reg in FAST_SAVED {
            self.a.push(reg);
        }
        let frame = 8 + 8 * FAST_SAVED.len() as i32;
        if let Some(rom) = rom {
            debug_assert!(q.rom_start.is_power_of_two());
            self.a.test_imm(RCX, q.rom_start);
            self.a.jcc(cc::NE, rom);
        }
        self.a.load(true, RAX, at(RSP, frame + FRAME_FAST));
        self.a
            .load(true, RAX, at(RAX, offset_of!(Fast, pages) as i32));
        self.a.alu_imm(Alu::Cmp, true, RAX, 0);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, R11, RCX);
        self.a.rot(Rot::Shr, false, R11, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, R11, 5);
        self.a.alu(Alu::Add, true, RAX, R11);
        self.a
            .alu_mem_imm_wide(Alu::Cmp, at(RAX, offset_of!(Page, meta) as i32), 0);
        self.a.jcc(cc::E, slow);
        if store {
            self.a
                .load(true, R8, at(RAX, offset_of!(Page, code) as i32));
            self.a.mov(false, RDI, RCX);
            self.a.rot(Rot::Shr, false, RDI, PAGE_BITS as u8);
            self.a.alu_imm(Alu::And, false, RDI, 3);
            self.a.load8(false, R8, indexed(R8, RDI, 1, 0));
            self.a.test(R8, R8);
            self.a.jcc(cc::NE, slow);
        }
        self.a.load(true, RDI, at(RSP, frame + FRAME_BUS));
        if store {
            for hit in q.code_hits {
                self.a.load8(false, R8, at(RDI, hit));
                self.a.test(R8, R8);
                self.a.jcc(cc::NE, slow);
            }
            self.a.load(false, R8, at(RDI, q.now));
            self.a.alu_load(Alu::Cmp, R8, at(RDI, q.dma_check));
            self.a.jcc(cc::AE, slow);
        }
        self.a.test_imm(RSI, 2);
        self.a.jcc(cc::NE, word);
        self.a.test_imm(RSI, 1);
        self.a.jcc(cc::NE, half);
        self.ram_access(q, store, Width::Byte, frame, slow, tail);
        self.a.bind(half);
        self.ram_access(q, store, Width::Half, frame, slow, tail);
        self.a.bind(word);
        self.ram_access(q, store, Width::Word, frame, slow, tail);
        if let Some(rom) = rom {
            self.a.bind(rom);
            self.rom(q, frame, slow, tail);
        }
        self.a.bind(slow);
        for reg in FAST_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        tail
    }

    fn ram_access(
        &mut self,
        q: &QuickFetch,
        store: bool,
        width: Width,
        frame: i32,
        slow: Label,
        tail: Label,
    ) {
        let shift = match width {
            Width::Word => WORD_CYCLES,
            Width::Byte | Width::Half => HALF_CYCLES,
        };
        let (lane, align) = match width {
            Width::Byte => (3, !0),
            Width::Half => (2, !1),
            Width::Word => (0, !3),
        };
        if !store && width == Width::Half {
            self.a.test_imm(RCX, 1);
            self.a.jcc(cc::NE, slow);
        }
        let wait = offset_of!(Page, meta) as i32 + (shift / 8) as i32;
        self.a.load8(false, R8, at(RAX, wait));
        if store {
            self.a
                .load(true, R11, at(RAX, offset_of!(Page, dirty) as i32));
            self.a.store8_imm(at(R11, 0), 1);
        } else {
            self.a.load(false, RDX, at(RDI, q.now));
            self.a.alu(Alu::Add, true, RDX, R8);
            self.a.load(false, R11, at(RDI, q.dma_check));
            self.a.alu(Alu::Cmp, true, RDX, R11);
            self.a.jcc(cc::AE, slow);
            self.a.inc(R8);
            self.a.lea(false, RDX, at(RDX, 1));
            self.a.store(false, at(RDI, q.now), RDX);
            self.a.store(false, at(RDI, q.sample), RDX);
        }
        self.a.load(true, RAX, at(RAX, 0));
        self.a.mov(false, R11, RCX);
        self.a.alu_imm(Alu::And, false, R11, align);
        let data = indexed(RAX, R11, 1, 0);
        match (store, width) {
            (true, Width::Byte) => self.a.store8(data, RDX),
            (true, Width::Half) => self.a.store16(data, RDX),
            (true, Width::Word) => self.a.store(false, data, RDX),
            (false, Width::Byte) => self.a.load8(false, RDX, data),
            (false, Width::Half) => self.a.load16(false, RDX, data),
            (false, Width::Word) => self.a.load(false, RDX, data),
        }
        let (ewram, merged) = (self.a.label(), self.a.label());
        if width != Width::Word {
            self.a.mov(false, R11, RCX);
            self.a.alu_imm(Alu::And, false, R11, lane);
        }
        self.a.test_imm(RCX, 1 << 24);
        self.a.jcc(cc::E, ewram);
        self.open_lane(width, q.open_iwram, RDX);
        if !store {
            if width == Width::Word {
                self.a.store(false, at(RDI, q.open_last), RDX);
            } else {
                self.a.load(false, R11, at(RDI, q.open_iwram));
                self.a.store(false, at(RDI, q.open_last), R11);
            }
            self.a.jmp(merged);
        }
        self.a.bind(ewram);
        if store {
            self.open_lane(width, q.open_last, RDX);
        } else {
            let latch = match width {
                Width::Byte => {
                    self.a.imul_imm(R11, RDX, 0x0101_0101);
                    R11
                }
                Width::Half => {
                    self.a.imul_imm(R11, RDX, 0x0001_0001);
                    R11
                }
                Width::Word => RDX,
            };
            self.a.store(false, at(RDI, q.open_last), latch);
        }
        self.a.bind(merged);
        if store {
            self.a.load(false, R11, at(RDI, q.now));
            self.a.lea(false, RAX, at(R11, 1));
            self.a.store(false, at(RDI, q.sample), RAX);
            self.a.alu(Alu::Add, false, R11, R8);
            self.a.store(false, at(RDI, q.now), R11);
        } else {
            match width {
                Width::Word => {
                    self.a.rot(Rot::Shl, false, RCX, 3);
                    self.a.rot_cl(Rot::Ror, false, RDX);
                    self.a.mov(false, RCX, RDX);
                }
                Width::Byte | Width::Half => {
                    let (unsigned, loaded) = (self.a.label(), self.a.label());
                    self.a.test_imm(RSI, 4);
                    self.a.jcc(cc::E, unsigned);
                    match width {
                        Width::Byte => self.a.extend8(true, RCX, RDX),
                        _ => self.a.extend16(true, RCX, RDX),
                    }
                    self.a.jmp(loaded);
                    self.a.bind(unsigned);
                    self.a.mov(false, RCX, RDX);
                    self.a.bind(loaded);
                }
            }
        }
        self.a.alu_store_wide(Alu::Add, at(RDI, q.clock), R8);
        self.a.load(true, R11, at(RSP, frame + FRAME_CORE));
        self.a.store8_imm(at(R11, self.o.next_fetch as i32), 0);
        self.a.mov(false, RDX, R8);
        self.a.pop(R8);
        self.a.pop(RDI);
        self.a.jmp(tail);
    }

    fn rom(&mut self, q: &QuickFetch, frame: i32, slow: Label, tail: Label) {
        let field = |offset: i32| at(RDI, offset);
        let page = q.rom_page - 1;
        let (flowing, top, done, none, glitched) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        let (half, word, commit) = (self.a.label(), self.a.label(), self.a.label());
        let literal = self.a.label();
        self.a.load(true, RDI, at(RSP, frame + FRAME_BUS));
        self.a.mov_imm(RDX, 0);
        self.a.load8(false, RAX, field(q.flowing));
        self.a.test(RAX, RAX);
        self.a.jcc(cc::NE, flowing);
        self.a.load8(false, RAX, field(q.active));
        self.a.test(RAX, RAX);
        self.a.jcc(cc::E, glitched);
        self.a.load8(false, RAX, field(q.paused));
        self.a.test(RAX, RAX);
        self.a.jcc(cc::E, slow);
        self.a.jmp(glitched);
        self.a.bind(flowing);
        self.a.load8(false, RAX, field(q.stuck));
        self.a.test(RAX, RAX);
        self.a.jcc(cc::NE, slow);
        self.a.load(false, RAX, field(q.cost));
        self.a.test(RAX, RAX);
        self.a.jcc(cc::E, slow);
        self.a.alu_load(Alu::Sub, RAX, field(q.progress));
        self.a.alu_load_wide(Alu::Add, RAX, field(q.since));
        self.a.load(true, R8, field(q.clock));
        self.a.load(false, R11, field(q.count));
        self.a.bind(top);
        self.a.alu(Alu::Cmp, true, R8, RAX);
        self.a.jcc(cc::B, done);
        self.a.inc(R11);
        self.a.alu_imm(Alu::Cmp, false, R11, q.capacity as i32);
        self.a.jcc(cc::E, none);
        self.a.jcc(cc::A, slow);
        self.a.load(false, RDX, field(q.head));
        self.a.lea(false, RDX, indexed(RDX, R11, 2, 0));
        self.a.test_imm(RDX, page);
        self.a.jcc(cc::E, slow);
        self.a.load(false, RDX, field(q.cost));
        self.a.alu(Alu::Add, true, RAX, RDX);
        self.a.jmp(top);
        self.a.bind(done);
        self.a.alu(Alu::Sub, true, RAX, R8);
        self.a.alu_imm(Alu::Cmp, true, RAX, 1);
        self.a.mov_imm(RDX, 0);
        self.a.setcc(cc::E, RDX);
        self.a.jmp(glitched);
        self.a.bind(none);
        self.a.mov_imm(RDX, 0);
        self.a.bind(glitched);
        self.a.test_imm(RSI, 2);
        self.a.jcc(cc::NE, word);
        self.a.test_imm(RSI, 1);
        self.a.jcc(cc::NE, half);
        self.rom_bounds(q, slow);
        self.rom_access(q, Width::Byte, frame, slow);
        self.a.jmp(commit);
        self.a.bind(half);
        self.rom_bounds(q, slow);
        self.rom_access(q, Width::Half, frame, slow);
        self.a.jmp(commit);
        self.a.bind(word);
        self.a.test_imm(RSI, ROM_LITERAL);
        self.a.jcc(cc::NE, literal);
        self.rom_bounds(q, slow);
        self.rom_access(q, Width::Word, frame, slow);
        self.a.jmp(commit);
        self.a.bind(literal);
        self.rom_literal(q, slow);
        self.a.bind(commit);
        self.a.load(true, RDX, field(q.clock));
        self.stopped(q, RDI, RDX, R11);
        self.a.load(true, R11, at(RSP, frame + FRAME_CORE));
        self.a.store8_imm(at(R11, self.o.next_fetch as i32), 0);
        self.a.lea(false, RDX, at(R8, 1));
        self.a.pop(R8);
        self.a.pop(RDI);
        self.a.jmp(tail);
    }

    fn rom_access(&mut self, q: &QuickFetch, width: Width, frame: i32, slow: Label) {
        let field = |offset: i32| at(RDI, offset);
        let align: u32 = match width {
            Width::Byte => !0,
            Width::Half => !1,
            Width::Word => !3,
        };
        if width == Width::Half {
            self.a.test_imm(RCX, 1);
            self.a.jcc(cc::NE, slow);
        }
        self.a.mov(false, R11, RCX);
        self.a
            .alu_imm(Alu::And, false, R11, (q.rom_mirror & align) as i32);
        self.a.mov(false, RAX, R11);
        self.a.alu_imm(Alu::Sub, false, RAX, q.rom_skip as i32);
        self.a.alu_load_wide(Alu::Cmp, RAX, field(q.plain_rom_end));
        self.a.jcc(cc::AE, slow);
        self.rom_wait(q, width, slow);
        self.a.load(true, RAX, at(RSP, frame + FRAME_FAST));
        self.a
            .load(true, RAX, at(RAX, offset_of!(Fast, rom) as i32));
        self.a.alu_imm(Alu::Cmp, true, RAX, 0);
        self.a.jcc(cc::E, slow);
        self.rom_now(q);
        let data = indexed(RAX, R11, 1, 0);
        match width {
            Width::Byte => {
                self.a.load8(false, RAX, data);
                self.a.imul_imm(R11, RAX, 0x0101_0101);
            }
            Width::Half => {
                self.a.load16(false, RAX, data);
                self.a.imul_imm(R11, RAX, 0x0001_0001);
            }
            Width::Word => {
                self.a.load(false, RAX, data);
                self.a.mov(false, R11, RAX);
            }
        }
        self.a.store(false, field(q.open_last), R11);
        match width {
            Width::Word => {
                self.a.rot(Rot::Shl, false, RCX, 3);
                self.a.rot_cl(Rot::Ror, false, RAX);
                self.a.mov(false, RCX, RAX);
            }
            Width::Byte | Width::Half => {
                let (unsigned, loaded) = (self.a.label(), self.a.label());
                self.a.test_imm(RSI, 4);
                self.a.jcc(cc::E, unsigned);
                match width {
                    Width::Byte => self.a.extend8(true, RCX, RAX),
                    _ => self.a.extend16(true, RCX, RAX),
                }
                self.a.jmp(loaded);
                self.a.bind(unsigned);
                self.a.mov(false, RCX, RAX);
                self.a.bind(loaded);
            }
        }
    }

    fn rom_bounds(&mut self, q: &QuickFetch, slow: Label) {
        self.a
            .lea(false, RAX, at(RCX, q.rom_start.wrapping_neg() as i32));
        self.a.alu_imm(Alu::Cmp, false, RAX, q.rom_span as i32);
        self.a.jcc(cc::AE, slow);
    }

    fn rom_wait(&mut self, q: &QuickFetch, width: Width, slow: Label) {
        let field = |offset: i32| at(RDI, offset);
        self.a.mov(false, RAX, RCX);
        self.a.rot(Rot::Shr, false, RAX, 25);
        self.a.alu_imm(Alu::And, false, RAX, 3);
        self.a
            .load8(false, R8, indexed(RDI, RAX, 1, q.first_halves));
        if width == Width::Word {
            self.a
                .load8(false, RAX, indexed(RDI, RAX, 1, q.sequential_halves));
            self.a.alu(Alu::Add, false, R8, RAX);
        }
        self.a.alu(Alu::Add, false, R8, RDX);
        self.a.load(false, RDX, field(q.now));
        self.a.alu(Alu::Add, true, RDX, R8);
        self.a.load(false, RAX, field(q.dma_check));
        self.a.alu(Alu::Cmp, true, RDX, RAX);
        self.a.jcc(cc::AE, slow);
    }

    fn rom_now(&mut self, q: &QuickFetch) {
        let field = |offset: i32| at(RDI, offset);
        self.a.lea(false, RDX, at(RDX, 1));
        self.a.store(false, field(q.now), RDX);
        self.a.store(false, field(q.sample), RDX);
    }

    fn rom_literal(&mut self, q: &QuickFetch, slow: Label) {
        self.rom_wait(q, Width::Word, slow);
        self.rom_now(q);
        self.a.mov(true, RCX, RSI);
        self.a.rot(Rot::Shr, true, RCX, 32);
        self.a.store(false, at(RDI, q.open_last), RCX);
    }

    fn open_lane(&mut self, width: Width, field: i32, value: R) {
        let lane = indexed(RDI, R11, 1, field);
        match width {
            Width::Byte => self.a.store8(lane, value),
            Width::Half => self.a.store16(lane, value),
            Width::Word => self.a.store(false, at(RDI, field), value),
        }
    }

    fn ram_spent(&mut self, store: bool) -> Option<()> {
        let frame = 16;
        self.a.alu(Alu::Sub, false, S, RDX);
        self.a.alu_load(Alu::Add, RDX, at(RSP, frame + FRAME_LAST));
        self.a.store(false, at(RSP, frame + FRAME_LAST), RDX);
        self.a.store_imm(at(RSP, frame + FRAME_FORCE), 0);
        if self.timed {
            self.room();
            self.a.alu(Alu::Xor, false, RDX, RDX);
            self.a.alu(Alu::Cmp, false, R11, RAX);
            self.a.setcc(cc::G, RDX);
            self.a.test(RDX, RDX);
        } else {
            self.a.mov_imm(RDX, 0);
            self.spent();
        }
        self.resume(store)
    }

    fn quick_v4(&mut self, store: bool) -> Option<()> {
        let frame = 16;
        self.v4_cycles(!store, CODE_CLASS, [R11, RDX, RAX, R11]);
        self.a.alu(Alu::Sub, false, S, RDX);
        self.a.store(false, at(RSP, frame + FRAME_LAST), RDX);
        self.a.store_imm(at(RSP, frame + FRAME_FORCE), 0);
        self.a.mov_imm(RDX, 0);
        self.spent();
        self.resume(store)
    }

    fn v4_cycles(&mut self, load: bool, class: u32, [meta, data, code, temp]: [R; 4]) {
        let (code_main, other, third, data_fast, overlapped, done) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        let load = u32::from(load);
        self.a.mov(true, code, meta);
        self.a.rot(Rot::Shr, true, code, (CODE_CYCLES + 8) as u8);
        self.a.test_imm(RSI, 2 << class);
        self.a.jcc(cc::NE, code_main);
        self.a.extend8(false, code, code);
        self.a.test_imm(meta, 1 << MAIN);
        self.a.jcc(cc::NE, other);
        self.a.lea(false, data, indexed(code, data, 1, load as i32));
        self.a.jmp(done);
        self.a.bind(other);
        self.a.lea(false, code, at(code, load as i32));
        self.a.jmp(overlapped);
        self.a.bind(code_main);
        self.a.rot(Rot::Shr, false, code, 8);
        self.a.test_imm(RSI, 1 << class);
        self.a.jcc(cc::E, third);
        self.a.rot(Rot::Shr, false, code, 8);
        self.a.bind(third);
        self.a.extend8(false, code, code);
        self.a.test_imm(meta, 1 << MAIN);
        self.a.jcc(cc::E, data_fast);
        self.a.alu(Alu::Add, false, data, code);
        self.a.jmp(done);
        self.a.bind(data_fast);
        self.a.lea(false, data, at(data, load as i32));
        self.a.bind(overlapped);
        self.a
            .lea(false, temp, indexed(code, data, 1, -(CODE_OVERLAP as i32)));
        self.a.alu(Alu::Cmp, false, code, data);
        self.a.cmov(cc::A, data, code);
        self.a.alu(Alu::Cmp, false, temp, data);
        self.a.cmov(cc::G, data, temp);
        self.a.bind(done);
    }

    fn quick(&mut self, store: bool, short: bool) -> Option<()> {
        let frame = 16;
        self.a.extend16(false, RAX, RSI);
        self.a.rot(Rot::Shr, false, RAX, 8);
        if !short {
            self.a
                .lea(false, R11, indexed(RAX, RDX, 1, -(OVERLAP as i32)));
        }
        self.a.alu(Alu::Cmp, false, RAX, RDX);
        self.a.cmov(cc::B, RAX, RDX);
        if !short {
            self.a.alu(Alu::Cmp, false, R11, RAX);
            self.a.cmov(cc::G, RAX, R11);
        }
        self.a.alu(Alu::Sub, false, S, RAX);
        self.settle(RAX, DEBT, RDX);
        self.a.store(false, at(RSP, frame + FRAME_LAST), RAX);
        self.a.store_imm(at(RSP, frame + FRAME_FORCE), 0);
        self.a.mov_imm(RDX, 0);
        self.spent();
        self.resume(store)
    }

    fn room(&mut self) {
        self.a.mov(false, R11, S);
        self.a.alu(Alu::Sub, false, R11, DEBT);
        self.a.extend8(false, RAX, RSI);
        self.a.rot(Rot::Shr, false, RAX, 3);
    }

    fn spent(&mut self) {
        if self.timed {
            self.room();
            self.a.alu(Alu::Cmp, false, R11, RAX);
            self.a.mov_imm(RAX, 1);
            self.a.cmov(cc::LE, RDX, RAX);
        }
        self.a.alu_imm(Alu::Xor, false, RDX, 1);
    }

    fn resume(&mut self, store: bool) -> Option<()> {
        let frame = 16;
        self.a.load(true, R11, at(RSP, frame + FRAME_CORE));
        self.a.lea(true, REGS, at(R11, self.o.regs as i32));
        self.a.pop(RAX);
        if store {
            let (leave, back) = (self.a.label(), self.a.label());
            self.a.jcc(cc::LE, leave);
            self.a.ret();
            self.a.bind(leave);
            self.a.rot(Rot::Shr, false, RSI, at_shift(self.v5) as u8);
            self.a.jcc(cc::E, back);
            self.a.alu_imm(Alu::Or, false, RSI, left(0).word()? as i32);
            self.a.alu_imm(Alu::Add, true, RSP, 8);
            self.a.jmp(self.exit);
            self.a.bind(back);
        }
        self.a.ret();
        Some(())
    }

    fn page(&mut self, store: bool, fast: i32, slow: Label) {
        self.a.load(true, RAX, at(RSP, fast + FRAME_FAST));
        self.a
            .load(true, RAX, at(RAX, offset_of!(Fast, pages) as i32));
        self.a.mov(false, R11, RCX);
        self.a.rot(Rot::Shr, false, R11, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, R11, 5);
        self.a.alu(Alu::Add, true, RAX, R11);
        self.a
            .load(true, R11, at(RAX, offset_of!(Page, meta) as i32));
        self.a.test(R11, R11);
        self.a.jcc(cc::E, slow);
        if store {
            self.a
                .load(true, RDI, at(RAX, offset_of!(Page, code) as i32));
            self.a.mov(false, R8, RCX);
            self.a.rot(Rot::Shr, false, R8, PAGE_BITS as u8);
            self.a.alu_imm(Alu::And, false, R8, 3);
            self.a.load8(false, RDI, indexed(RDI, R8, 1, 0));
            self.a.test(RDI, RDI);
            self.a.jcc(cc::NE, slow);
            self.a
                .load(true, RDI, at(RAX, offset_of!(Page, dirty) as i32));
            self.a.store8(at(RDI, 0), R11);
        }
    }

    fn paged(&mut self, store: bool, width: Width, tails: &Tails, slow: Label) {
        if !self.v5 && !store && width == Width::Half {
            self.a.test_imm(RCX, 1);
            self.a.jcc(cc::NE, slow);
        }
        let mask = match width {
            Width::Byte => None,
            Width::Half => Some(!1),
            Width::Word => Some(!3),
        };
        let (cycles, flag) = match width {
            Width::Word => (WORD_CYCLES, LONG_WORD),
            Width::Byte | Width::Half => (HALF_CYCLES, LONG_HALF),
        };
        let at_addr = if store { R8 } else { RDX };
        self.a.load(true, if store { RDI } else { RAX }, at(RAX, 0));
        self.a.mov(false, at_addr, RCX);
        if let Some(mask) = mask {
            self.a.alu_imm(Alu::And, false, at_addr, mask);
        }
        if store {
            let data = indexed(RDI, R8, 1, 0);
            match width {
                Width::Byte => self.a.store8(data, RDX),
                Width::Half => self.a.store16(data, RDX),
                Width::Word => self.a.store(false, data, RDX),
            }
            self.a.pop(R8);
            self.a.pop(RDI);
        } else {
            let data = indexed(RAX, RDX, 1, 0);
            match width {
                Width::Word => {
                    self.a.load(false, RDX, data);
                    self.a.rot(Rot::Shl, false, RCX, 3);
                    self.a.rot_cl(Rot::Ror, false, RDX);
                    self.a.mov(false, RCX, RDX);
                }
                Width::Byte | Width::Half => {
                    let (unsigned, loaded) = (self.a.label(), self.a.label());
                    self.a.test_imm(RSI, 4);
                    self.a.jcc(cc::E, unsigned);
                    self.narrow(width, true, RCX, data);
                    self.a.jmp(loaded);
                    self.a.bind(unsigned);
                    self.narrow(width, false, RCX, data);
                    self.a.bind(loaded);
                }
            }
        }
        self.a.mov(false, RDX, R11);
        if cycles > 0 {
            self.a.rot(Rot::Shr, false, RDX, cycles as u8);
        }
        self.a.extend8(false, RDX, RDX);
        if self.v5 {
            self.a.test_imm(R11, 1 << flag);
            self.a.jcc(cc::NE, tails.any);
        }
        self.a.jmp(tails.short);
    }

    fn narrow(&mut self, width: Width, signed: bool, dest: R, data: Mem) {
        if width == Width::Byte {
            self.a.load8(signed, dest, data);
        } else {
            self.a.load16(signed, dest, data);
        }
    }

    fn block(
        &mut self,
        helper: usize,
        store: bool,
        pages: bool,
        tail: Option<Label>,
        ram: bool,
    ) -> Option<()> {
        let regs = |o: &Offsets| indexed(RDI, RCX, 4, o.regs as i32);
        let saved = [RAX, RDI, R8, S, DEBT, RSI];
        let kept = [RBX, RBP, R12, R13, R14];
        let frame = 8 + 8 * (saved.len() + kept.len()) as i32;
        let word = self.a.label();
        self.a.mov(false, R11, RSI);
        self.a.rot(Rot::Shr, false, R11, 16);
        self.a.alu_imm(Alu::And, false, R11, 0x3FF);
        self.a.alu(Alu::Sub, false, S, R11);
        let quick_ram = match self.o.quick {
            Some(q) if ram && self.timed && !self.v5 && !pages => Some(q),
            _ => None,
        };
        if let Some(q) = quick_ram {
            self.ram_block(&q, store)?;
        }
        for reg in saved.into_iter().chain(kept) {
            self.a.push(reg);
        }
        self.a.mov(false, RBX, RCX);
        self.a.mov(false, RBP, RDX);
        self.a.alu_imm(Alu::And, false, RBP, 0xFFFF);
        if !store {
            self.a.mov(false, R12, RDX);
            self.a.rot(Rot::Shr, false, R12, 16);
        }
        self.a.mov_imm(R13, 0);
        self.a.mov_imm(R14, 0);
        let done = self.a.label();
        if pages {
            let slow = self.a.label();
            if !self.v5 {
                self.a.test_imm(RSI, 3);
                self.a.jcc(cc::E, slow);
            }
            self.a.load(true, RDI, at(RSP, frame + FRAME_FAST));
            self.a.mov(false, R8, RSI);
            self.a.rot(Rot::Shr, false, R8, 26);
            self.a.alu_imm(Alu::And, false, R8, 0x1F);
            self.a.rot(Rot::Shl, false, R8, 2);
            self.a.alu_imm(Alu::Sub, false, R8, 4);
            self.span(store, frame, slow);
            if !self.v5 {
                self.v4_cycles(!store, 0, [R11, R13, RAX, RDX]);
            }
            self.a.jmp(done);
            self.a.bind(slow);
        }
        if !self.v5 {
            self.store_pc(frame);
        }
        self.a.bind(word);
        self.a.lea(false, RDX, at(RBP, -1));
        self.a.alu(Alu::And, false, RDX, RBP);
        self.a.mov_imm(RAX, 0);
        self.a.setcc(cc::E, RAX);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        if store {
            self.a.lea(false, R8, indexed(R14, RAX, LAST as u8, 0));
            self.a.bsf(RCX, RBP);
            self.a.load(false, RCX, regs(self.o));
        } else {
            self.a.lea(false, RCX, indexed(R14, RAX, LAST as u8, 0));
        }
        self.a.mov(false, RDX, RBX);
        self.a.load(true, RSI, at(RSP, frame + FRAME_BUS));
        self.call(helper);
        if !store {
            let skip = self.a.label();
            self.a.bsf(RCX, RBP);
            self.a.mov(false, RDX, R12);
            self.a.rot_cl(Rot::Shr, false, RDX);
            self.a.test_imm(RDX, 1);
            self.a.jcc(cc::E, skip);
            self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
            self.a.mov(true, R11, RAX);
            self.a.rot(Rot::Shr, true, R11, 32);
            self.a.store(false, regs(self.o), R11);
            self.a.bind(skip);
        }
        self.a.mov(false, RCX, RAX);
        self.a.alu_imm(Alu::And, false, RCX, CYCLES_MASK);
        self.a.alu(Alu::Add, false, R13, RCX);
        self.a.alu_imm(Alu::And, false, RAX, EXIT as u32 as i32);
        self.a.alu(Alu::Or, false, R13, RAX);
        self.a.alu_imm(Alu::Add, false, RBX, 4);
        self.a.mov_imm(R14, SEQUENTIAL);
        self.a.lea(false, RAX, at(RBP, -1));
        self.a.alu(Alu::And, false, RBP, RAX);
        self.a.jcc(cc::NE, word);
        self.a.bind(done);
        let Some(tail) = tail else {
            self.a.mov(false, RCX, R13);
            for reg in saved.iter().chain(&kept).rev() {
                self.a.pop(*reg);
            }
            self.a.load(true, R11, at(RSP, 8 + FRAME_CORE));
            self.a.lea(true, REGS, at(R11, self.o.regs as i32));
            self.a.ret();
            return Some(());
        };
        self.a.mov(false, RAX, R13);
        for reg in saved[1..].iter().chain(&kept).rev() {
            self.a.pop(*reg);
        }
        self.a.jmp(tail);
        Some(())
    }

    fn ram_block(&mut self, q: &QuickFetch, store: bool) -> Option<()> {
        let slot = self.o.regs as i32;
        let regs = move |index: R| indexed(R11, index, 4, slot);
        let (slow, ewram, copy) = (self.a.label(), self.a.label(), self.a.label());
        for reg in BLOCK_SAVED {
            self.a.push(reg);
        }
        let frame = 8 + 8 * BLOCK_SAVED.len() as i32;
        self.a.load(true, RAX, at(RSP, frame + FRAME_FAST));
        self.a
            .load(true, RAX, at(RAX, offset_of!(Fast, pages) as i32));
        self.a.alu_imm(Alu::Cmp, true, RAX, 0);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, R11, RCX);
        self.a.rot(Rot::Shr, false, R11, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, R11, 5);
        self.a.alu(Alu::Add, true, RAX, R11);
        self.a
            .load(true, R11, at(RAX, offset_of!(Page, meta) as i32));
        self.a.test(R11, R11);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, R8, RSI);
        self.a.rot(Rot::Shr, false, R8, 26);
        self.a.alu_imm(Alu::And, false, R8, 0x1F);
        self.a.mov(false, RBX, RCX);
        self.a
            .alu_imm(Alu::And, false, RBX, ((PAGE_SIZE - 1) & !3) as i32);
        self.a.lea(false, RBX, indexed(RBX, R8, 4, -4));
        self.a.test_imm(RBX, PAGE_SIZE);
        self.a.jcc(cc::NE, slow);
        if store {
            self.a
                .load(true, RDI, at(RAX, offset_of!(Page, code) as i32));
            for first in [true, false] {
                if first {
                    self.a.mov(false, RBX, RCX);
                } else {
                    self.a.lea(false, RBX, indexed(RCX, R8, 4, -4));
                }
                self.a.rot(Rot::Shr, false, RBX, PAGE_BITS as u8);
                self.a.alu_imm(Alu::And, false, RBX, 3);
                self.a.load8(false, RBX, indexed(RDI, RBX, 1, 0));
                self.a.test(RBX, RBX);
                self.a.jcc(cc::NE, slow);
            }
        }
        self.a.load(true, RDI, at(RSP, frame + FRAME_BUS));
        if store {
            for hit in q.code_hits {
                self.a.load8(false, RBX, at(RDI, hit));
                self.a.test(RBX, RBX);
                self.a.jcc(cc::NE, slow);
            }
        }
        self.a.rot(Rot::Shr, true, R11, WORD_CYCLES as u8);
        self.a.extend8(false, R11, R11);
        self.a.imul(false, R8, R11);
        self.a.load(false, RBX, at(RDI, q.now));
        self.a.alu(Alu::Add, true, RBX, R8);
        if store {
            self.a.alu(Alu::Sub, true, RBX, R11);
        }
        self.a.load(false, R11, at(RDI, q.dma_check));
        self.a.alu(Alu::Cmp, true, RBX, R11);
        self.a.jcc(cc::AE, slow);
        if store {
            self.a.lea(false, R11, at(RBX, 1));
            self.a.store(false, at(RDI, q.sample), R11);
            self.a
                .load(true, R11, at(RAX, offset_of!(Page, meta) as i32));
            self.a.rot(Rot::Shr, true, R11, WORD_CYCLES as u8);
            self.a.extend8(false, R11, R11);
            self.a.alu(Alu::Add, false, RBX, R11);
            self.a.store(false, at(RDI, q.now), RBX);
            self.a
                .load(true, R11, at(RAX, offset_of!(Page, dirty) as i32));
            self.a.store8_imm(at(R11, 0), 1);
        } else {
            self.a.lea(false, RBX, at(RBX, 1));
            self.a.store(false, at(RDI, q.now), RBX);
            self.a.store(false, at(RDI, q.sample), RBX);
            self.a.inc(R8);
        }
        self.a.load(true, R11, at(RDI, q.clock));
        self.a.alu(Alu::Add, true, R11, R8);
        self.a.store(true, at(RDI, q.clock), R11);
        self.a
            .load(true, RBX, at(RAX, offset_of!(Page, addend) as i32));
        self.a.mov(false, RAX, RCX);
        self.a.alu_imm(Alu::And, false, RAX, !3);
        self.a.alu(Alu::Add, true, RBX, RAX);
        self.a.load(true, R11, at(RSP, frame + FRAME_CORE));
        self.a.store8_imm(at(R11, self.o.next_fetch as i32), 0);
        if store {
            self.a.bsr(RAX, RDX);
            self.a.load(false, RAX, regs(RAX));
        } else {
            self.a.mov(false, RAX, RSI);
            self.a.rot(Rot::Shr, false, RAX, 26);
            self.a.alu_imm(Alu::And, false, RAX, 0x1F);
            self.a.load(false, RAX, indexed(RBX, RAX, 4, -4));
        }
        self.a.store(false, at(RDI, q.open_last), RAX);
        self.a.test_imm(RCX, 1 << 24);
        self.a.jcc(cc::E, ewram);
        self.a.store(false, at(RDI, q.open_iwram), RAX);
        self.a.bind(ewram);
        if !store {
            self.a.mov(false, RDI, RDX);
            self.a.rot(Rot::Shr, false, RDI, 16);
        }
        self.a.extend16(false, RDX, RDX);
        self.a.bind(copy);
        self.a.bsf(RCX, RDX);
        if store {
            self.a.load(false, RAX, regs(RCX));
            self.a.store(false, at(RBX, 0), RAX);
        } else {
            let skip = self.a.label();
            self.a.bt(RDI, RCX);
            self.a.jcc(cc::AE, skip);
            self.a.load(false, RAX, at(RBX, 0));
            self.a.store(false, regs(RCX), RAX);
            self.a.bind(skip);
        }
        self.a.alu_imm(Alu::Add, true, RBX, 4);
        self.a.lea(false, RAX, at(RDX, -1));
        self.a.alu(Alu::And, false, RDX, RAX);
        self.a.jcc(cc::NE, copy);
        self.a.mov(false, RDX, R8);
        for reg in BLOCK_SAVED[1..].iter().rev() {
            self.a.pop(*reg);
        }
        self.ram_spent(false)?;
        self.a.bind(slow);
        for reg in BLOCK_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        Some(())
    }

    fn span(&mut self, store: bool, frame: i32, slow: Label) {
        let last = indexed(RBX, R8, 1, 0);
        self.a
            .load(true, RDI, at(RDI, offset_of!(Fast, pages) as i32));
        self.a.mov(false, RAX, RBX);
        self.a.rot(Rot::Shr, false, RAX, PAGE_SHIFT as u8);
        self.a.rot(Rot::Shl, true, RAX, 5);
        self.a.alu(Alu::Add, true, RDI, RAX);
        self.a
            .load(true, R11, at(RDI, offset_of!(Page, meta) as i32));
        self.a.test(R11, R11);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, RAX, RBX);
        self.a
            .alu_imm(Alu::And, false, RAX, ((PAGE_SIZE - 1) & !3) as i32);
        self.a.alu(Alu::Add, false, RAX, R8);
        self.a.test_imm(RAX, PAGE_SIZE);
        self.a.jcc(cc::NE, slow);
        if store {
            self.a
                .load(true, RCX, at(RDI, offset_of!(Page, code) as i32));
            for page in [at(RBX, 0), last] {
                self.a.lea(false, RDX, page);
                self.a.rot(Rot::Shr, false, RDX, PAGE_BITS as u8);
                self.a.alu_imm(Alu::And, false, RDX, 3);
                self.a.load8(false, RDX, indexed(RCX, RDX, 1, 0));
                self.a.test(RDX, RDX);
                self.a.jcc(cc::NE, slow);
            }
            self.a
                .load(true, RCX, at(RDI, offset_of!(Page, dirty) as i32));
            self.a.store8(at(RCX, 0), R11);
        }
        self.a.load(true, R14, at(RDI, 0));
        self.a.mov(false, RAX, RBX);
        self.a.alu_imm(Alu::And, false, RAX, !3);
        self.a.alu(Alu::Add, true, R14, RAX);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        let copy = self.a.label();
        self.a.bind(copy);
        self.a.bsf(RCX, RBP);
        let slot = indexed(RDI, RCX, 4, self.o.regs as i32);
        if store {
            self.a.load(false, RAX, slot);
            self.a.store(false, at(R14, 0), RAX);
        } else {
            let skip = self.a.label();
            self.a.mov(false, RDX, R12);
            self.a.rot_cl(Rot::Shr, false, RDX);
            self.a.test_imm(RDX, 1);
            self.a.jcc(cc::E, skip);
            self.a.load(false, RAX, at(R14, 0));
            self.a.store(false, slot, RAX);
            self.a.bind(skip);
        }
        self.a.alu_imm(Alu::Add, true, R14, 4);
        self.a.lea(false, RAX, at(RBP, -1));
        self.a.alu(Alu::And, false, RBP, RAX);
        self.a.jcc(cc::NE, copy);
        self.a.mov(false, R13, R8);
        self.a.rot(Rot::Shr, false, R13, 2);
        self.a.mov(false, RAX, R11);
        self.a.rot(Rot::Shr, false, RAX, SEQUENTIAL_CYCLES as u8);
        self.a.extend8(false, RAX, RAX);
        self.a.imul(false, R13, RAX);
        self.a.mov(false, RAX, R11);
        self.a.rot(Rot::Shr, false, RAX, WORD_CYCLES as u8);
        self.a.extend8(false, RAX, RAX);
        self.a.alu(Alu::Add, false, R13, RAX);
    }

    fn run_step(&mut self, helper: usize) {
        let o = self.o;
        let frame = 8;
        let kept = self.a.label();
        self.a.mov(false, RDX, RSI);
        self.a.rot(Rot::Shr, false, RDX, 16);
        self.a.alu_imm(Alu::And, false, RDX, 0x3FF);
        self.a.alu(Alu::Sub, false, S, RDX);
        self.guests(false);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.store_cpsr(RDI, RDX, R8);
        self.store_debt(RDX, frame, DEBT);
        self.a.mov(false, RBX, S);
        self.a.alu(Alu::Sub, false, RBX, DEBT);
        self.a.mov(false, RDX, RSI);
        self.a.alu_imm(Alu::And, false, RDX, 0xFF);
        self.a.mov(false, R12, RDX);
        self.a.mov(false, R13, RCX);
        self.a.load(false, RCX, at(RSP, frame + FRAME_SHIFT));
        self.a.rot_cl(Rot::Shl, false, RDX);
        self.a.alu_load(Alu::Add, RDX, at(RSP, frame + FRAME_BASE));
        self.a.store(false, at(REGS, PC_SLOT), RDX);
        self.a.load(true, RDX, at(RSP, frame + FRAME_WORDS));
        self.a.load(true, RDX, indexed(RDX, R12, 4, 0));
        self.a.store(true, at(RDI, o.pipeline as i32), RDX);
        self.a.alu_imm(Alu::Cmp, false, R13, KEEP as i32);
        self.a.jcc(cc::E, kept);
        self.a.store8(at(RDI, o.next_fetch as i32), R13);
        self.a.bind(kept);
        self.a.load(true, RSI, at(RSP, frame + FRAME_BUS));
        self.a.alu_imm(Alu::Sub, true, RSP, 8);
        self.call(helper);
        self.a.alu_imm(Alu::Add, true, RSP, 8);
        self.a.mov(true, R12, RAX);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a.lea(true, REGS, at(RDI, o.regs as i32));
        self.a.load(false, RCX, at(RDI, o.cpsr as i32));
        self.image(RAX, RCX);
        if self.timed {
            self.a.mov_imm(DEBT, 0);
        } else {
            self.a.load(true, RDX, at(RSP, frame + FRAME_DEBT));
            self.a.load(false, DEBT, at(RDX, 0));
        }
        self.a.mov(false, RCX, R12);
        self.a.alu_imm(Alu::And, false, RCX, CYCLES_MASK);
        self.a.alu(Alu::Sub, false, RBX, RCX);
        self.a.lea(false, S, indexed(RBX, DEBT, 1, 0));
        self.a.store(false, at(RSP, frame + FRAME_LAST), RCX);
        self.a.mov(false, RDX, R12);
        self.a.rot(Rot::Shr, false, RDX, 31);
        self.a.store(false, at(RSP, frame + FRAME_FORCE), RDX);
        self.guests(true);
    }

    fn step(&mut self, helper: usize) -> Option<Label> {
        let (cp15, stepped) = (self.a.label(), self.a.label());
        if self.v5 {
            self.a.test_imm(RSI, OLD_LAST);
            self.a.jcc(cc::NE, cp15);
        }
        self.run_step(helper);
        self.a.test_imm(RDX, 1);
        self.a.ret();
        if self.v5 {
            self.a.bind(cp15);
            self.cp15(stepped);
        }
        if self.v5 || self.pages {
            self.a.bind(stepped);
            self.step_and_leave(helper)?;
        }
        Some(stepped)
    }

    fn cp15(&mut self, slow: Label) {
        let o = self.o;
        let settled = self.a.label();
        self.a.load(true, RDX, at(RSP, FRAME_CTX));
        self.a.load(true, RDX, at(RDX, o.attention as i32));
        self.a.alu(Alu::Or, true, RDX, RDX);
        self.a.jcc(cc::E, slow);
        self.a.store8_imm(at(RDX, 0), 1);
        self.a.load(true, RDX, at(RSP, FRAME_CTX));
        self.a.load(true, RDX, at(RDX, o.pending as i32));
        self.a.store8_imm(at(RDX, 0), 1);
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, 14);
        self.a.alu_imm(Alu::And, false, RCX, 0xFF);
        self.a.alu_imm(Alu::Sub, false, RCX, 1);
        self.a.mov(false, RDX, DEBT);
        self.a.alu(Alu::Sub, false, DEBT, RCX);
        self.a.jcc(cc::NS, settled);
        self.a.alu(Alu::Xor, false, DEBT, DEBT);
        self.a.bind(settled);
        self.a.jmp(self.exit);
    }

    fn step_and_leave(&mut self, helper: usize) -> Option<()> {
        let stepped = Info {
            at: 0,
            fetch: Fetch::Core,
            force: true,
            branch: true,
            last: Last::Ctx,
            pending: 0,
        }
        .word()?;
        self.a.push(RSI);
        self.a.mov(false, RDX, RSI);
        self.a.rot(Rot::Shr, false, RDX, 14);
        self.a.alu_imm(Alu::And, false, RDX, 0xFF);
        self.a.rot(Rot::Shl, false, RDX, 22);
        self.a.alu(Alu::Sub, false, RSI, RDX);
        self.a.mov(false, RDX, RSI);
        self.a.rot(Rot::Shr, false, RDX, 22);
        self.a.rot(Rot::Shl, false, RDX, 16);
        self.a.alu_imm(Alu::And, false, RSI, 0xFF);
        self.a.alu_imm(Alu::Sub, false, RSI, 1);
        self.a.alu(Alu::Or, false, RSI, RDX);
        self.run_step(helper);
        self.a.pop(RSI);
        self.a.alu_imm(Alu::And, false, RSI, 0xFF);
        self.a.alu_imm(Alu::Or, false, RSI, stepped as i32);
        self.a.jmp(self.exit);
        Some(())
    }

    fn fetch(&mut self, helper: usize, mode: Option<u32>) {
        let o = self.o;
        if let Some(quick) = o.quick {
            let slow = self.a.label();
            match mode {
                Some(mode) => self.quick_fetch(&quick, mode == THUMB, slow),
                None => self.idles_iwram(&quick, slow),
            }
            self.a.bind(slow);
        }
        let frame = self.fetch_frame();
        match mode {
            Some(mode) => self.a.lea(false, RSI, at(RCX, mode as i32)),
            None => self.a.alu(Alu::Or, false, RSI, RCX),
        }
        self.a.xchg(false, RSI, RDX);
        self.a.load(true, RDI, at(RSP, frame + FRAME_BUS));
        self.call(helper);
        self.fetched();
        self.a.mov(false, RSI, RAX);
        self.a.pop(RAX);
        self.a.alu(Alu::Sub, false, S, RSI);
        self.a.store(false, at(RSP, 8 + FRAME_LAST), RSI);
        self.a.ret();
    }

    fn fetch_frame(&mut self) -> i32 {
        let known = self.a.label();
        for reg in FETCH_SAVED {
            self.a.push(reg);
        }
        self.a.alu_imm(Alu::Sub, true, RSP, 8);
        let frame = 8 + 8 * FETCH_SAVED.len() as i32 + 8;
        self.a.alu_imm(Alu::Cmp, false, RCX, KEEP as i32);
        self.a.jcc(cc::NE, known);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a.load8(false, RCX, at(RDI, self.o.next_fetch as i32));
        self.a.bind(known);
        frame
    }

    fn fetched(&mut self) {
        self.a.alu_imm(Alu::Add, true, RSP, 8);
        for reg in FETCH_SAVED[1..].iter().rev() {
            self.a.pop(*reg);
        }
    }

    fn quick_fetch(&mut self, q: &QuickFetch, thumb: bool, slow: Label) {
        if thumb {
            self.quick_thumb(q, slow);
            return;
        }
        let (miss, iwram) = (self.a.label(), self.a.label());
        let field = |offset: i32| at(R11, offset);
        self.a.test_imm(RSI, 1 << 31);
        self.a.jcc(cc::NE, iwram);
        for reg in QUICK_SAVED {
            self.a.push(reg);
        }
        let frame = 8 + 8 * QUICK_SAVED.len() as i32;
        self.a.load(true, R11, at(RSP, frame + FRAME_BUS));
        self.a.load(false, RAX, field(q.now));
        self.a.alu_load(Alu::Cmp, RAX, field(q.dma_check));
        self.a.jcc(cc::AE, miss);
        self.a.mov(false, R10, RSI);
        self.a.alu_load_wide(Alu::Cmp, R10, field(q.plain_rom_end));
        self.a.jcc(cc::NS, miss);
        self.a.alu_load(Alu::Cmp, RDX, field(q.head));
        self.a.jcc(cc::NE, miss);
        self.a.load(true, RDI, field(q.clock));
        self.a.load(true, R8, field(q.due));
        self.a.mov(true, RCX, RDI);
        self.a.alu(Alu::Sub, true, RCX, R8);
        self.a.alu(Alu::Or, true, RCX, R8);
        self.a.jcc(cc::NS, miss);
        self.a.load(false, RCX, field(q.count));
        self.a.test(RCX, RCX);
        self.a.jcc(cc::E, miss);
        self.quick_hit(q, 2, miss);
        self.a.lea(false, R10, at(RAX, 1));
        self.a.store(false, field(q.sample), R10);
        self.a.alu(Alu::Add, false, RAX, RCX);
        self.a.store(false, field(q.now), RAX);
        self.a.store(true, field(q.open_addr), RDX);
        self.a.mov(false, RSI, RCX);
        for reg in QUICK_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.alu(Alu::Sub, false, S, RSI);
        self.a.store(false, at(RSP, 8 + FRAME_LAST), RSI);
        self.a.ret();
        let (bios, missed) = (self.a.label(), self.a.label());
        self.a.bind(iwram);
        self.quick_word(q, IWRAM_FETCH, q.open_iwram, bios, missed);
        self.a.bind(bios);
        self.quick_word(q, BIOS_FETCH, q.open_bios, slow, missed);
        self.a.bind(missed);
        self.a.pop(R11);
        self.a.jmp(slow);
        self.a.bind(miss);
        for reg in QUICK_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
    }

    fn quick_thumb(&mut self, q: &QuickFetch, slow: Label) {
        let (miss, once, tail) = (self.a.label(), self.a.label(), self.a.label());
        let (due, elsewhere, drained, flight) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        let field = |offset: i32| at(R11, offset);
        self.a.test_imm(RSI, 1 << 31);
        self.a.jcc(cc::NE, slow);
        for reg in THUMB_SAVED {
            self.a.push(reg);
        }
        let frame = 8 + 8 * THUMB_SAVED.len() as i32;
        self.a.load(true, R11, at(RSP, frame + FRAME_BUS));
        self.a.alu_load_wide(Alu::Cmp, RSI, field(q.plain_rom_end));
        self.a.jcc(cc::NS, miss);
        self.a.load(false, RSI, field(q.now));
        self.a.alu_load(Alu::Cmp, RSI, field(q.dma_check));
        self.a.jcc(cc::AE, miss);
        self.a.alu_load(Alu::Cmp, RDX, field(q.head));
        self.a.jcc(cc::NE, elsewhere);
        self.a.load(true, RCX, field(q.clock));
        self.a.alu_load_wide(Alu::Sub, RCX, field(q.due));
        self.a.alu_load_wide(Alu::Or, RCX, field(q.due));
        self.a.jcc(cc::NS, due);
        self.a.load(false, RCX, field(q.count));
        self.a.test(RCX, RCX);
        self.a.jcc(cc::E, flight);
        self.thumb_hit(q, drained);
        self.a.bind(once);
        self.a.load(false, RSI, field(q.now));
        self.a.inc(RSI);
        self.a.store(false, field(q.sample), RSI);
        self.a.store(false, field(q.now), RSI);
        self.a.store(true, field(q.open_addr), RDX);
        for reg in THUMB_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.alu_imm(Alu::Sub, false, S, 1);
        self.a.store_imm(at(RSP, 8 + FRAME_LAST), 1);
        self.a.ret();
        self.a.bind(flight);
        self.thumb_flight(q, miss);
        self.a.bind(tail);
        self.a.store(true, field(q.open_addr), RDX);
        self.a.load(false, RDX, field(q.now));
        self.a.lea(false, RCX, at(RDX, 1));
        self.a.store(false, field(q.sample), RCX);
        self.a.alu(Alu::Add, false, RDX, RSI);
        self.a.store(false, field(q.now), RDX);
        for reg in THUMB_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.alu(Alu::Sub, false, S, RSI);
        self.a.store(false, at(RSP, 8 + FRAME_LAST), RSI);
        self.a.ret();
        self.a.bind(due);
        self.thumb_settled(q, miss, once);
        self.a.bind(elsewhere);
        self.thumb_elsewhere(q, frame, miss, tail);
        self.a.bind(drained);
        self.a.load(true, RSI, field(q.clock));
        self.stopped(q, R11, RSI, RCX);
        self.a.store8_imm(field(q.force_n), 1);
        self.a.jmp(once);
        self.a.bind(miss);
        for reg in THUMB_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
    }

    fn quick_word(&mut self, q: &QuickFetch, marker: u32, latch: i32, next: Label, miss: Label) {
        let field = |offset: i32| at(R11, offset);
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.alu_imm(Alu::Cmp, false, RSI, marker as i32);
        self.a.jcc(cc::NE, next);
        self.a.push(R11);
        let frame = 16;
        self.a.load(true, R11, at(RSP, frame + FRAME_BUS));
        self.a.load(false, RSI, field(q.now));
        self.a.alu_load(Alu::Cmp, RSI, field(q.dma_check));
        self.a.jcc(cc::AE, miss);
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, miss);
        self.a.alu_mem_imm_wide(Alu::Add, field(q.clock), 1);
        self.a.store8_imm(field(q.branched), 0);
        self.a.inc(RSI);
        self.a.store(false, field(q.sample), RSI);
        self.a.store(false, field(q.now), RSI);
        self.a.store(true, field(q.open_addr), RDX);
        self.a.rot(Rot::Shr, true, RDX, 32);
        self.a.store(false, field(latch), RDX);
        self.a.pop(R11);
        self.a.alu_imm(Alu::Sub, false, S, 1);
        self.a.store_imm(at(RSP, 8 + FRAME_LAST), 1);
        self.a.ret();
    }

    fn idles_iwram(&mut self, q: &QuickFetch, slow: Label) {
        let miss = self.a.label();
        let field = |offset: i32| at(R11, offset);
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.test_imm(RSI, IWRAM_IDLES);
        self.a.jcc(cc::E, slow);
        for reg in LEAN_SAVED {
            self.a.push(reg);
        }
        let frame = 8 + 8 * LEAN_SAVED.len() as i32;
        self.a.load(true, R11, at(RSP, frame + FRAME_BUS));
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, IDLES_SHIFT as u8);
        self.a.load(false, RDI, field(q.now));
        self.a.alu(Alu::Add, false, RDI, RCX);
        self.a.jcc(cc::B, miss);
        self.a.alu_load(Alu::Cmp, RDI, field(q.dma_check));
        self.a.jcc(cc::AE, miss);
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, miss);
        self.a.inc(RCX);
        self.a.alu_store_wide(Alu::Add, field(q.clock), RCX);
        self.a.store8_imm(field(q.branched), 0);
        self.a.inc(RDI);
        self.a.store(false, field(q.sample), RDI);
        self.a.store(false, field(q.now), RDI);
        self.a.store(true, field(q.open_addr), RDX);
        self.a.rot(Rot::Shr, true, RDX, 32);
        self.a.store(false, field(q.open_iwram), RDX);
        self.a.alu(Alu::Sub, false, S, RCX);
        self.a.store(false, at(RSP, frame + FRAME_LAST), RCX);
        for reg in LEAN_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.ret();
        self.a.bind(miss);
        for reg in LEAN_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
    }

    fn quick_hit(&mut self, q: &QuickFetch, halves: u32, miss: Label) {
        let field = |offset: i32| at(R11, offset);
        self.a.load8(false, R10, field(q.paused));
        self.a.lea(true, R8, at(RCX, -(halves as i32)));
        self.a.alu(Alu::Sub, true, R8, R10);
        self.a.jcc(cc::S, miss);
        self.a.store8_imm(field(q.branched), 0);
        self.a.alu_imm(Alu::Sub, false, RCX, halves as i32);
        self.a.store(false, field(q.count), RCX);
        self.a.lea(false, RCX, at(RDX, 2 * halves as i32));
        self.a.store(false, field(q.head), RCX);
        self.a.lea(true, RDI, at(RDI, 1));
        self.a.store(true, field(q.clock), RDI);
        self.a.mov_imm(RCX, 1);
    }

    fn thumb_hit(&mut self, q: &QuickFetch, drained: Label) {
        let field = |offset: i32| at(R11, offset);
        self.a.load8(false, RSI, field(q.paused));
        self.a.alu(Alu::Cmp, false, RCX, RSI);
        self.a.jcc(cc::BE, drained);
        self.a.store8_imm(field(q.branched), 0);
        self.a.dec(RCX);
        self.a.store(false, field(q.count), RCX);
        self.a.lea(false, RCX, at(RDX, 2));
        self.a.store(false, field(q.head), RCX);
        self.a.alu_mem_imm_wide(Alu::Add, field(q.clock), 1);
    }

    fn thumb_flight(&mut self, q: &QuickFetch, miss: Label) {
        let field = |offset: i32| at(R11, offset);
        let page = 1u32
            .checked_shl(q.rom_page.trailing_zeros())
            .map_or(u32::MAX, |bit| bit - 1);
        self.a.lea(false, RCX, at(RDX, 2));
        self.a.test_imm(RCX, page);
        self.a.jcc(cc::E, miss);
        self.a.load8(false, RCX, field(q.branched));
        self.a.alu_load8(Alu::Or, RCX, field(q.stuck));
        self.a.jcc(cc::NE, miss);
        self.a.load(true, RSI, field(q.clock));
        self.a.load(true, RCX, field(q.since));
        self.a.alu(Alu::Sub, true, RCX, RSI);
        self.a.rot(Rot::Shr, true, RCX, 63);
        self.a.alu_load(Alu::Or, RCX, field(q.progress));
        self.a.setcc(cc::NE, RCX);
        self.a.alu_store8(Alu::Or, field(q.following), RCX);
        self.a.store_imm(field(q.progress), 0);
        self.a.store(true, field(q.since), RSI);
        self.a.lea(false, RCX, at(RDX, 2));
        self.a.store(false, field(q.head), RCX);
        self.a.load(false, RCX, field(q.cost));
        self.a.alu(Alu::Add, true, RCX, RSI);
        self.a.alu_load_wide(Alu::Sub, RSI, field(q.due));
        self.a.store(true, field(q.due), RCX);
        self.a.neg(RSI);
    }

    fn thumb_settled(&mut self, q: &QuickFetch, miss: Label, once: Label) {
        let field = |offset: i32| at(R11, offset);
        let page = q.rom_page - 1;
        let (top, settled, paused, hit, missed) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        for reg in SETTLE_SAVED {
            self.a.push(reg);
        }
        self.a.load(true, RDI, field(q.clock));
        self.a.mov(true, RCX, RDI);
        self.a.alu_load_wide(Alu::Sub, RCX, field(q.due));
        self.a.test_mem8(field(q.flowing), 0xFF);
        self.a.jcc(cc::E, missed);
        self.a.test_mem8(field(q.stuck), 0xFF);
        self.a.jcc(cc::NE, missed);
        self.a.load(false, RSI, field(q.cost));
        self.a.test(RSI, RSI);
        self.a.jcc(cc::E, missed);
        self.a.load(false, RAX, field(q.count));
        self.a.bind(top);
        self.a.inc(RAX);
        self.a.alu_imm(Alu::Cmp, false, RAX, q.capacity as i32);
        self.a.jcc(cc::E, paused);
        self.a.jcc(cc::A, missed);
        self.a.lea(false, R8, indexed(RDX, RAX, 2, 0));
        self.a.test_imm(R8, page);
        self.a.jcc(cc::E, missed);
        self.a.alu(Alu::Cmp, true, RCX, RSI);
        self.a.jcc(cc::B, settled);
        self.a.alu(Alu::Sub, true, RCX, RSI);
        self.a.jmp(top);
        self.a.bind(settled);
        self.a.dec(RAX);
        self.a.store(false, field(q.count), RAX);
        self.a.store(false, field(q.progress), RCX);
        self.a.lea(true, R8, indexed(RDI, RSI, 1, 0));
        self.a.alu(Alu::Sub, true, R8, RCX);
        self.a.store(true, field(q.due), R8);
        self.a.jmp(hit);
        self.a.bind(paused);
        self.a.store_imm(field(q.count), q.capacity - 1);
        self.a.store_imm(field(q.progress), 0);
        self.a.store8_imm(field(q.paused), 1);
        self.a.store8_imm(field(q.flowing), 0);
        self.a.alu_imm(Alu::Or, true, R8, -1);
        self.a.store(true, field(q.due), R8);
        self.a.bind(hit);
        self.a.store(true, field(q.since), RDI);
        self.a.store8_imm(field(q.following), 1);
        self.a.store8_imm(field(q.branched), 0);
        self.a.lea(false, R8, at(RDX, 2));
        self.a.store(false, field(q.head), R8);
        self.a.lea(true, RDI, at(RDI, 1));
        self.a.store(true, field(q.clock), RDI);
        for reg in SETTLE_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.jmp(once);
        self.a.bind(missed);
        for reg in SETTLE_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.jmp(miss);
    }

    fn thumb_elsewhere(&mut self, q: &QuickFetch, frame: i32, miss: Label, tail: Label) {
        let field = |offset: i32| at(R11, offset);
        let bit = q.prefetch_bit.trailing_zeros();
        let page = q.rom_page - 1;
        let (known, first, priced, missed) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        for reg in ELSEWHERE_SAVED {
            self.a.push(reg);
        }
        let frame = frame + 8 * ELSEWHERE_SAVED.len() as i32;
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, missed);
        self.a.test_mem8(field(q.flowing), 0xFF);
        self.a.jcc(cc::NE, missed);
        self.a.lea(false, RSI, at(RDX, 2));
        self.a.test_imm(RSI, page);
        self.a.jcc(cc::E, missed);
        self.a.alu_imm(Alu::Cmp, false, RCX, KEEP as i32);
        self.a.jcc(cc::NE, known);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a.load8(false, RCX, at(RDI, self.o.next_fetch as i32));
        self.a.bind(known);
        self.a.mov(false, R8, RDX);
        self.a.rot(Rot::Shr, false, R8, 25);
        self.a.alu_imm(Alu::And, false, R8, 3);
        self.a.test_imm(RCX, SEQUENTIAL);
        self.a.jcc(cc::E, first);
        self.a.test_mem8(field(q.force_n), 0xFF);
        self.a.jcc(cc::NE, first);
        self.a.test_imm(RDX, page);
        self.a.jcc(cc::E, first);
        self.a
            .load8(false, RSI, indexed(R11, R8, 1, q.sequential_halves));
        self.a.jmp(priced);
        self.a.bind(first);
        self.a
            .load8(false, RSI, indexed(R11, R8, 1, q.first_halves));
        self.a.bind(priced);
        self.a
            .load8(false, RDI, indexed(R11, R8, 1, q.sequential_halves));
        self.a.store(false, field(q.cost), RDI);
        self.a.load(true, R8, field(q.clock));
        self.a.store(true, field(q.since), R8);
        self.a.alu(Alu::Add, true, R8, RDI);
        self.a.store(true, field(q.due), R8);
        self.a.mov(false, R8, RDX);
        self.a.alu_imm(Alu::Or, false, R8, page as i32);
        self.a.inc(R8);
        self.a.rot(Rot::Shr, false, R8, 25);
        self.a.alu_imm(Alu::And, false, R8, 3);
        self.a
            .load8(false, R8, indexed(R11, R8, 1, q.sequential_halves));
        self.a.store(false, field(q.page_cost), R8);
        self.a.lea(false, R8, at(RDX, 2));
        self.a.store(false, field(q.head), R8);
        self.restart(q, R11);
        for reg in ELSEWHERE_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.jmp(tail);
        self.a.bind(missed);
        for reg in ELSEWHERE_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.jmp(miss);
    }

    fn stopped(&mut self, q: &QuickFetch, base: R, clock: R, scratch: R) {
        let field = |offset: i32| at(base, offset);
        self.a.store(true, field(q.since), clock);
        self.a.lea(true, clock, at(clock, 1));
        self.a.store(true, field(q.clock), clock);
        self.a.alu_imm(Alu::Or, true, scratch, -1);
        self.a.store(true, field(q.due), scratch);
        if q.packed() {
            for zero in [q.head, q.count, q.parked] {
                self.a.store_imm_wide(field(zero), 0);
            }
            self.a.store_imm(field(q.active), 0);
            for off in [q.stuck, q.following] {
                self.a.store8_imm(field(off), 0);
            }
            return;
        }
        for zero in [q.head, q.count, q.progress, q.cost, q.page_cost, q.parked] {
            self.a.store_imm(field(zero), 0);
        }
        for off in [
            q.paused,
            q.branched,
            q.active,
            q.flowing,
            q.stuck,
            q.following,
        ] {
            self.a.store8_imm(field(off), 0);
        }
    }

    fn restart(&mut self, q: &QuickFetch, base: R) {
        let field = |offset: i32| at(base, offset);
        if q.packed() {
            self.a.store_imm_wide(field(q.count), 0);
            self.a.store_imm(field(q.parked), 0);
            self.a.store_imm_wide(field(q.active), RESTARTED);
            return;
        }
        for zero in [q.count, q.progress, q.parked] {
            self.a.store_imm(field(zero), 0);
        }
        for off in [q.paused, q.branched, q.stuck, q.following, q.force_n] {
            self.a.store8_imm(field(off), 0);
        }
        for on in [q.active, q.flowing] {
            self.a.store8_imm(field(on), 1);
        }
    }

    fn fetch_flat(&mut self, helper: usize) {
        if let Some(quick) = self.o.quick {
            let slow = self.a.label();
            self.flat_iwram(&quick, slow);
            self.a.bind(slow);
        }
        let frame = self.fetch_frame();
        self.a.alu(Alu::Or, false, RSI, RCX);
        self.a.xchg(false, RSI, RDX);
        self.a.mov(false, RCX, S);
        self.a.load(true, RDI, at(RSP, frame + FRAME_BUS));
        self.call(helper);
        self.fetched();
        self.a.mov(true, RDX, RAX);
        self.a.rot(Rot::Shr, true, RDX, 32);
        self.a.store(false, at(RSP, 16 + FRAME_LAST), RDX);
        self.a.alu(Alu::Sub, false, S, RAX);
        self.a.alu(Alu::Sub, false, RAX, RDX);
        self.a.pop(RAX);
        self.a.ret();
    }

    fn flat_iwram(&mut self, q: &QuickFetch, slow: Label) {
        let miss = self.a.label();
        let field = |offset: i32| at(R11, offset);
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.test_imm(RSI, IWRAM_RUN);
        self.a.jcc(cc::E, slow);
        for reg in LEAN_SAVED {
            self.a.push(reg);
        }
        let frame = 8 + 8 * LEAN_SAVED.len() as i32;
        self.a.load(true, R11, at(RSP, frame + FRAME_BUS));
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, RUN_CYCLES_SHIFT as u8);
        self.a.alu(Alu::Cmp, false, RCX, S);
        self.a.jcc(cc::GE, miss);
        self.a.load(false, RDI, field(q.now));
        self.a.alu(Alu::Add, false, RDI, RCX);
        self.a.jcc(cc::B, miss);
        self.a.alu_load(Alu::Cmp, RDI, field(q.dma_check));
        self.a.jcc(cc::A, miss);
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, miss);
        self.a.alu_store_wide(Alu::Add, field(q.clock), RCX);
        self.a.store8_imm(field(q.branched), 0);
        self.a.store(false, field(q.sample), RDI);
        self.a.store(false, field(q.now), RDI);
        self.a.store(true, field(q.open_addr), RDX);
        self.a.rot(Rot::Shr, true, RDX, 32);
        self.a.store(false, field(q.open_iwram), RDX);
        self.a.store_imm(at(RSP, frame + FRAME_LAST), 1);
        self.a.alu(Alu::Sub, false, S, RCX);
        for reg in LEAN_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
        self.a.ret();
        self.a.bind(miss);
        self.a.mov(false, RCX, RSI);
        self.a.alu_imm(Alu::And, false, RCX, THUMB as i32);
        self.a.mov_imm(RDI, 4);
        self.a.alu(Alu::Sub, false, RDI, RCX);
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, IDLES_SHIFT as u8);
        self.a.alu_imm(Alu::And, false, RCX, RUN_COUNT_MASK as i32);
        self.a.imul(false, RCX, RDI);
        self.a.alu(Alu::Sub, false, RDX, RCX);
        for reg in LEAN_SAVED.iter().rev() {
            self.a.pop(*reg);
        }
    }

    fn fields(&mut self, word: R) {
        self.a.rot(Rot::Shr, false, word, 8);
        self.a.alu_imm(Alu::And, false, word, 0xF);
        self.a.imul_imm(word, word, 0x4001);
        self.a.alu_imm(Alu::And, false, word, 0x3_0003);
        self.a.imul_imm(word, word, 0x81);
        self.a.alu_imm(Alu::And, false, word, 0x0101_0101);
        self.a.imul_imm(word, word, 0xFF);
    }

    fn spsr_slot(&mut self, slot: R, none: Label) {
        let slots = (0..16).fold(0u64, |slots, mode| {
            let index = Bank::of(MODE_BIT_4 | mode).spsr_index();
            slots | index.map_or(0xF, |index| index as u64) << (4 * mode)
        });
        self.a.test_imm(RCX, MODE_BIT_4);
        self.a.jcc(cc::E, none);
        self.a.alu_imm(Alu::And, false, RCX, 0xF);
        self.a.rot(Rot::Shl, false, RCX, 2);
        self.a.mov_imm64(slot, slots);
        self.a.rot_cl(Rot::Shr, true, slot);
        self.a.alu_imm(Alu::And, false, slot, 0xF);
        self.a.alu_imm(Alu::Cmp, false, slot, 0xF);
        self.a.jcc(cc::E, none);
    }

    fn psr(&mut self, stepped: Label, helper: usize) -> Option<()> {
        let o = self.o;
        let regs = |offset: u32| at(REGS, offset as i32 - o.regs as i32);
        let spsr = |slot: R| indexed(REGS, slot, 4, o.spsr as i32 - o.regs as i32);
        let info = |last: Last| {
            Info {
                at: 0,
                fetch: Fetch::Known(SEQUENTIAL),
                force: true,
                branch: true,
                last,
                pending: 0,
            }
            .word()
        };
        let (old, one) = (info(Last::Old(0))?, info(Last::One)?);
        let (read, current, write, slow, mode, flags, kept, tail, low, settled) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a.test_imm(RSI, SPSR_READ);
        self.a.jcc(cc::NE, read);
        self.a.push(RCX);
        self.a.push(RSI);
        self.a.test_imm(RSI, SPSR_WRITE);
        self.a.jcc(cc::NE, write);
        self.fields(RSI);
        self.a.load(false, RCX, regs(o.cpsr));
        self.a.alu_imm(Alu::Or, false, RDX, MODE_BIT_4 as i32);
        self.a.alu(Alu::Xor, false, RDX, RCX);
        self.a.alu(Alu::And, false, RDX, RSI);
        self.a.test_imm(RSI, CONTROL_FIELDS);
        self.a.jcc(cc::E, flags);
        self.a.mov(false, RSI, RCX);
        self.a.alu_imm(Alu::And, false, RSI, MODE_MASK as i32);
        self.a
            .alu_imm(Alu::Cmp, false, RSI, Mode::User.bits() as i32);
        self.a.jcc(cc::E, slow);
        self.a.test_imm(RDX, FLAG_T);
        self.a.jcc(cc::NE, slow);
        self.a.test_imm(RDX, MODE_MASK);
        self.a.jcc(cc::NE, mode);
        self.a.bind(flags);
        self.a.alu(Alu::Xor, false, RCX, RDX);
        self.a.store(false, regs(o.cpsr), RCX);
        self.a.mov(false, RSI, RCX);
        self.a.alu(Alu::Xor, false, RSI, RDX);
        self.a
            .rot(Rot::Shr, false, RSI, FLAG_I.trailing_zeros() as u8);
        self.a.alu_imm(Alu::And, false, RSI, 1);
        self.a.store8(regs(o.irq_masked), RSI);
        self.a.test_imm(RDX, !NZCV_MASK);
        self.a.setcc(cc::NE, RDX);
        self.a.extend8(false, RDX, RDX);
        self.a.store(false, at(RSP, 16 + FRAME_FORCE), RDX);
        self.a.load(false, RSI, at(RSP, 0));
        self.a.test_imm(RSI, FLAGS_FIELD);
        self.a.jcc(cc::E, tail);
        self.image(RAX, RCX);
        self.a.jmp(tail);
        self.a.bind(slow);
        self.a.pop(RSI);
        self.a.pop(RCX);
        self.a.jmp(stepped);
        self.a.bind(mode);
        self.guests(false);
        let saved = [RAX, RCX, RDX, S, DEBT, REGS];
        for reg in saved {
            self.a.push(reg);
        }
        let frame = 8 * (2 + saved.len() as i32);
        self.a.load(true, RDI, at(RSP, frame + FRAME_CORE));
        self.a.mov(false, RSI, RCX);
        self.a.alu(Alu::Xor, false, RSI, RDX);
        self.a.mov_imm(RDX, u32::MAX);
        self.a.mov_imm(RCX, PSR_CPSR);
        self.call(helper);
        for reg in saved.iter().rev() {
            self.a.pop(*reg);
        }
        self.guests(true);
        self.a.jmp(flags);
        self.a.bind(write);
        self.a.load(false, RCX, regs(o.cpsr));
        self.spsr_slot(RSI, kept);
        self.a.mov(false, RCX, RSI);
        self.a.load(false, RSI, at(RSP, 0));
        self.fields(RSI);
        self.a.alu(Alu::And, false, RDX, RSI);
        self.a.not(RSI);
        self.a.alu_load(Alu::And, RSI, spsr(RCX));
        self.a.alu(Alu::Or, false, RDX, RSI);
        self.a.store(false, spsr(RCX), RDX);
        self.a.bind(kept);
        self.a.store_imm(at(RSP, 16 + FRAME_FORCE), 0);
        self.a.load8(false, RCX, regs(o.cpsr));
        self.a
            .rot(Rot::Shr, false, RCX, FLAG_I.trailing_zeros() as u8);
        self.a.store8(regs(o.irq_masked), RCX);
        self.a.bind(tail);
        self.a.load(false, RSI, at(RSP, 0));
        self.a.alu_imm(Alu::And, false, RSI, 0xFF);
        self.a.load(false, RCX, at(RSP, 16 + FRAME_SHIFT));
        self.a.mov(false, RDX, RSI);
        self.a.rot_cl(Rot::Shl, false, RDX);
        self.a.alu_load(Alu::Add, RDX, at(RSP, 16 + FRAME_BASE));
        self.a.store(false, at(REGS, PC_SLOT), RDX);
        self.a.load(true, RCX, at(RSP, 16 + FRAME_WORDS));
        self.a.load(true, RDX, indexed(RCX, RSI, 4, 0));
        self.a.store(true, regs(o.pipeline), RDX);
        self.a.store8_imm(regs(o.next_fetch), SEQUENTIAL as u8);
        self.a.pop(RSI);
        self.a.pop(RCX);
        self.a.mov(false, RCX, RSI);
        self.a.rot(Rot::Shr, false, RCX, 14);
        self.a.alu_imm(Alu::And, false, RCX, 0xFF);
        self.a.alu_imm(Alu::Cmp, false, RCX, 2);
        self.a.jcc(cc::B, low);
        self.a.alu_imm(Alu::And, false, RSI, !0x3F00);
        self.a.alu_imm(Alu::Or, false, RSI, old as i32);
        if self.v5 {
            self.a.mov(false, RDX, DEBT);
            self.a.alu_imm(Alu::Sub, false, RCX, 1);
            self.a.alu(Alu::Sub, false, DEBT, RCX);
            self.a.jcc(cc::NS, settled);
            self.a.alu(Alu::Xor, false, DEBT, DEBT);
            self.a.bind(settled);
        } else {
            self.a.mov_imm(RDX, 0);
        }
        self.a.jmp(self.exit);
        self.a.bind(low);
        self.a.alu_imm(Alu::And, false, RSI, !0x3F_FF00);
        self.a.alu_imm(Alu::Or, false, RSI, one as i32);
        if self.v5 {
            self.a.alu_imm(Alu::Cmp, false, RCX, 1);
            self.a.alu_imm(Alu::Adc, false, DEBT, 0);
        }
        self.a.jmp(self.exit);
        self.a.bind(read);
        self.a.load(false, RCX, regs(o.cpsr));
        self.spsr_slot(RSI, current);
        self.a.load(false, RDX, spsr(RSI));
        self.a.ret();
        self.a.bind(current);
        self.a.mov(false, RDX, RAX);
        self.a.alu_imm(Alu::And, false, RDX, IMAGE_MASK);
        self.a.imul_imm(RDX, RDX, IMAGE_TO_NZCV);
        self.a.alu_imm(Alu::And, false, RDX, NZCV_MASK as i32);
        self.a.load(false, RCX, regs(o.cpsr));
        self.a.alu_imm(Alu::And, false, RCX, !NZCV_MASK as i32);
        self.a.alu(Alu::Or, false, RDX, RCX);
        self.a.ret();
        Some(())
    }
}
