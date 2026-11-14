use super::super::super::fast::{
    CODE_CYCLES, Fast, HALF_CYCLES, LONG_AFTER, LONG_HALF, LONG_WORD, MAIN, PAGE_SHIFT, PAGE_SIZE,
    Page, SEQUENTIAL_CYCLES, Spec, WORD_CYCLES, Window,
};
use super::super::super::helpers::{EXIT, LAST, SEQUENTIAL};
use super::super::super::lookup::{Link, SLOT_GENERATION, SLOT_LINKS, SLOT_PIPELINE};
use super::super::super::marks::PAGE_BITS;
use super::super::super::scan::Width;
use super::super::{CODE_CLASS, Fetch, Info, Last, Offsets, PSR_CPSR, Runtime, STUBS, Setup, Stub};
use super::asm::{
    Alu, Asm, Label, Mem, R, R8, R9, R10, R11, R12, R13, R14, R15, RAX, RBP, RBX, RCX, RDI, RDX,
    RSI, RSP, Rot, at, cc, indexed,
};
use super::{
    DEBT, FRAME, FRAME_BASE, FRAME_BUDGET, FRAME_BUS, FRAME_CELL, FRAME_CORE, FRAME_CTX,
    FRAME_DEBT, FRAME_FAST, FRAME_FORCE, FRAME_LAST, FRAME_SHIFT, FRAME_START, FRAME_STUBS,
    FRAME_WORDS, HOP_INVERTED, HOP_TARGET, KEEP, PINNED, REGS, S, at_shift, left, stub_slot,
};
use crate::gba::cpu::psr::{Bank, FLAG_I, FLAG_T, MODE_MASK, Mode};
use std::mem::{offset_of, size_of};

const CALLEE: [R; 6] = [RBX, RBP, R12, R13, R14, R15];
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
const FAST_LOAD_SAVED: [R; 1] = [RAX];
const OVERLAP: u32 = 6;
const CODE_OVERLAP: u32 = 3;
const _: () = assert!(OVERLAP == LONG_AFTER);

struct Tails {
    any: Label,
    short: Label,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leave {
    Branch,
    Exchange,
    Return,
}

struct Build<'a> {
    a: Asm,
    o: &'a Offsets,
    v5: bool,
}

pub fn runtime(setup: &Setup) -> Option<Runtime> {
    if !setup.v5 && !setup.specs.is_empty() {
        return None;
    }
    let (o, helpers, chain) = (setup.offsets, setup.helpers, setup.chain);
    let mut build = Build {
        a: Asm::new(Vec::new()),
        o,
        v5: setup.v5,
    };
    let finish = build.a.label();
    let next = match chain {
        Some(_) => build.a.label(),
        None => finish,
    };
    build.trampoline(chain.is_some());
    let exit = build.a.here();
    build.exit(chain, finish);
    let branch = build.a.here();
    build.leave(helpers.branch, Leave::Branch, setup.specs, next)?;
    let exchange = build.a.here();
    build.leave(helpers.exchange, Leave::Exchange, setup.specs, next)?;
    let returns = build.a.here();
    build.leave(helpers.load_pc, Leave::Return, setup.specs, next)?;
    let load = build.a.here();
    let pages = setup.pages;
    let tail = build.memory(helpers.load, false, pages)?;
    let store = build.a.here();
    build.memory(helpers.store, true, pages)?;
    let gather = build.a.here();
    build.block(helpers.load_word, false, pages, Some(tail))?;
    let collect = build.a.here();
    build.block(helpers.load_word, false, pages && setup.v5, None)?;
    let scatter = build.a.here();
    build.block(helpers.store_word, true, pages, Some(tail))?;
    let step = build.a.here();
    let stepped = build.step(helpers.step)?;
    let psr = build.a.here();
    if setup.v5 {
        build.psr(stepped, setup.psr)?;
    }
    if let Some(chain) = chain {
        build.a.bind(next);
        build.chain(chain, finish);
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
        self.a.load(true, R10, at(RSP, 8));
        self.a.mov(false, RCX, RCX);
        self.a.load(false, RAX, indexed(R10, RCX, 4, 0));
        for reg in CALLEE {
            self.a.push(reg);
        }
        self.a.alu_imm(Alu::Sub, true, RSP, FRAME);
        self.a
            .load(true, R11, at(RSP, FRAME + 8 * CALLEE.len() as i32 + 16));
        for chunk in 0..(FRAME_STUBS + 1) / 2 {
            let start = (16 * chunk).min(8 * FRAME_STUBS - 16);
            self.a.copy16(true, 0, at(R11, o.stubs as i32 + start));
            self.a.copy16(false, 0, at(RSP, start));
        }
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
        self.a.load(true, R8, at(R8, o.fast_debt as i32));
        self.a.store(true, at(RSP, FRAME_DEBT), R8);
        self.a.mov(false, S, RAX);
        self.a.rot(Rot::Shr, false, S, 16);
        self.a.alu_imm(Alu::And, false, S, 0x7FF);
        self.a.alu(Alu::Add, false, S, RDX);
        self.a.mov(false, RDX, RAX);
        self.a.alu_imm(Alu::And, false, RDX, 0x7FFF);
        self.a.alu(Alu::Add, true, RDX, R10);
        self.a.load(false, DEBT, at(R8, 0));
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
        self.a.load(true, RCX, at(RSP, FRAME_DEBT));
        self.a.store(false, at(RCX, 0), DEBT);
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
            self.hop(called, missed, full);
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

    fn leave(&mut self, helper: usize, leave: Leave, specs: &[Spec], finish: Label) -> Option<()> {
        let o = self.o;
        let returns = leave == Leave::Return;
        self.guests(false);
        if returns {
            self.a.mov(false, R15, RDX);
        }
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.store_cpsr(RDI, RDX, R8);
        self.a.mov(false, RDX, RSI);
        self.a.rot(Rot::Shr, false, RDX, 22);
        self.a.alu(Alu::Sub, false, S, RDX);
        self.a.mov(false, RBX, RSI);
        self.a.mov(false, R12, S);
        self.a.mov(false, R13, DEBT);
        let (slow, called) = (self.a.label(), self.a.label());
        if specs.iter().any(|spec| spec.code) {
            self.refetch(leave, specs, slow, called)?;
        }
        self.a.bind(slow);
        self.a.mov(false, RDX, RCX);
        self.a.load(true, RSI, at(RSP, FRAME_BUS));
        self.call(helper);
        self.a.bind(called);
        if returns {
            self.a.mov(true, RDX, RAX);
            self.a.rot(Rot::Shr, true, RDX, 32);
            self.a.lea(false, R11, indexed(RDX, R15, 1, -6));
            self.a.alu(Alu::Cmp, false, RDX, R15);
            self.a.cmov(cc::B, RDX, R15);
            self.a.alu(Alu::Cmp, false, R11, RDX);
            self.a.cmov(cc::G, RDX, R11);
            self.a.alu(Alu::Add, false, RAX, RDX);
        }
        self.a.alu(Alu::Sub, false, R12, RAX);
        self.settle(RAX, R13, RCX);
        self.a.load(true, RDI, at(RSP, FRAME_CORE));
        self.a.store(false, at(RDI, o.cycles as i32), RAX);
        self.a.store8_imm(at(RDI, o.irq_held as i32), 0);
        self.a.load(true, RCX, at(RSP, FRAME_DEBT));
        self.a.store(false, at(RCX, 0), R13);
        self.a.mov(false, RSI, RBX);
        self.a.mov(false, S, R12);
        self.a.mov(false, DEBT, R13);
        self.a.mov_imm(RBP, 0);
        self.a.jmp(finish);
        Some(())
    }

    fn refetch(&mut self, leave: Leave, specs: &[Spec], slow: Label, called: Label) -> Option<()> {
        let exchange = leave != Leave::Branch;
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
        self.refill(leave, specs, 4, slow, called)?;
        self.a.bind(thumb);
        self.a.mov(false, RDX, RCX);
        self.a.alu_imm(Alu::And, false, RDX, !1);
        if exchange {
            self.a.alu_imm(Alu::Or, false, R8, FLAG_T as i32);
        }
        self.refill(leave, specs, 2, slow, called)
    }

    fn refill(
        &mut self,
        leave: Leave,
        specs: &[Spec],
        step: u32,
        slow: Label,
        called: Label,
    ) -> Option<()> {
        let o = self.o;
        let (exchange, returns) = (leave != Leave::Branch, leave == Leave::Return);
        self.a.load(true, RSI, at(RSP, FRAME_FAST));
        for (index, spec) in specs.iter().enumerate() {
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
            self.a.lea(false, R11, at(RDX, 2 * step as i32));
            self.a.store(false, at(RDI, o.regs as i32 + PC_SLOT), R11);
            self.a.store(false, at(RDI, o.landed as i32), RDX);
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
            if returns {
                self.a.rot(Rot::Shl, true, RBP, 32);
                self.a.alu(Alu::Or, true, RAX, RBP);
            }
            self.a.jmp(called);
            self.a.bind(next);
        }
        self.a.jmp(slow);
        Some(())
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

    fn chain(&mut self, chain: usize, finish: Label) {
        let o = self.o;
        let (refused, called, linked) = (self.a.label(), self.a.label(), self.a.label());
        self.a.test(RBP, RBP);
        self.a.jcc(cc::NE, finish);
        self.hop(called, finish, called);
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

    fn hop(&mut self, called: Label, refused: Label, empty: Label) {
        let o = self.o;
        let link = |field: usize| field as i32;
        let first = self.a.label();
        self.a.load8(false, RCX, at(RSP, FRAME_CELL));
        self.a.store8_imm(at(RSP, FRAME_CELL), 0);
        self.a.test(RCX, RCX);
        self.a.jcc(cc::E, empty);
        self.a.load(true, R11, at(RSP, FRAME_CTX));
        self.a.load(true, RDI, at(R11, o.slot as i32));
        self.a.alu(Alu::Or, true, RDI, RDI);
        self.a.jcc(cc::E, called);
        self.a.lea(true, RDI, at(RDI, SLOT_LINKS as i32));
        self.a.test_imm(RCX, 2);
        self.a.jcc(cc::E, first);
        self.a.lea(true, RDI, at(RDI, size_of::<Link>() as i32));
        self.a.bind(first);
        self.a.load(true, R8, at(RDI, link(offset_of!(Link, slot))));
        self.a.alu(Alu::Or, true, R8, R8);
        self.a.jcc(cc::E, called);
        self.a
            .load(false, RDX, at(RDI, link(offset_of!(Link, generation))));
        self.a
            .alu_load(Alu::Cmp, RDX, at(R8, SLOT_GENERATION as i32));
        self.a.jcc(cc::NE, called);
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
            .load(false, RCX, at(RDI, link(offset_of!(Link, entry))));
        self.a.mov(false, RDX, RCX);
        self.a.rot(Rot::Shr, false, RDX, 27);
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
        self.a.store(true, at(R11, o.words as i32), RDX);
        self.a.store(true, at(RSP, FRAME_WORDS), RDX);
        self.a.load(true, RDX, at(RDI, link(offset_of!(Link, pc))));
        self.a.store(true, at(R11, o.base as i32), RDX);
        self.a.store(true, at(RSP, FRAME_BASE), RDX);
        self.a.alu_mem_imm(Alu::Add, at(R11, o.chains as i32), 1);
        self.a.store(true, at(R11, o.slot as i32), R8);
        self.a.mov(false, RDX, RCX);
        self.a.rot(Rot::Shr, false, RDX, 16);
        self.a.alu_imm(Alu::And, false, RDX, 0x7FF);
        self.a.alu(Alu::Add, false, S, RDX);
        self.a.rot(Rot::Shr, false, RCX, 7);
        self.a.alu_imm(Alu::And, false, RCX, 0x100);
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

    fn memory(&mut self, helper: usize, store: bool, pages: bool) -> Option<Label> {
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
        Some(tail)
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

    fn spent(&mut self) {
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
            self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
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
        self.a.load(true, RDX, at(RSP, frame + FRAME_DEBT));
        self.a.store(false, at(RDX, 0), DEBT);
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
        self.a.load(true, RDX, at(RSP, frame + FRAME_DEBT));
        self.a.load(false, DEBT, at(RDX, 0));
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
        self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
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
        self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
        Some(())
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
        self.a.mov(false, RDX, DEBT);
        self.a.alu_imm(Alu::Sub, false, RCX, 1);
        self.a.alu(Alu::Sub, false, DEBT, RCX);
        self.a.jcc(cc::NS, settled);
        self.a.alu(Alu::Xor, false, DEBT, DEBT);
        self.a.bind(settled);
        self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
        self.a.bind(low);
        self.a.alu_imm(Alu::And, false, RSI, !0x3F_FF00);
        self.a.alu_imm(Alu::Or, false, RSI, one as i32);
        self.a.alu_imm(Alu::Cmp, false, RCX, 1);
        self.a.alu_imm(Alu::Adc, false, DEBT, 0);
        self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
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
