mod asm;
mod runtime;

use std::cmp::Ordering;

use runtime::{DATA_OVERLAP, runtime};

use super::super::fast::{HALF_CYCLES, LONG_HALF, LONG_WORD, PAGE_SHIFT, WORD_CYCLES};
use super::super::helpers::{
    EXIT, IDLES_SHIFT, RUN_COUNT_MASK, RUN_CYCLES_SHIFT, RUN_FIRST_SHIFT, THUMB,
};
use super::super::lookup::LINKS;
use super::super::scan::{Block, Jump, Multiply, Offset, Plan, Target, Transfer, Width};
use super::{
    ALL_FLAGS, ALWAYS, Alu, BIOS_FETCH, BLOCK_WORDS, Backend, CODE_CLASS, FLAG_C, FLAG_N, FLAG_V,
    FLAG_Z, Fetch, IWRAM_FETCH, IWRAM_IDLES, IWRAM_RUN, Info, Item, Kind, Last, Linked, Lowered,
    MARK, Offsets, Operand, PENDING_LIMIT, PSR_CPSR, PSR_READ_SPSR, PSR_SPSR, ROM_LITERAL, Runtime,
    SHIFT_IDLES, STUBS, Setup, Stub, Swap, block_word, entry_word, link_owners, load_pc_word,
    memory_word, restore_word, step_word,
};
use crate::jit_cache::{LINK, Reloc};
use asm::{Asm, Indexed, Label, Logic, R, Shift, ZR, invert, logical_immediate};

const GUEST: [R; 15] = [19, 20, 21, 22, 23, 24, 25, 26, 9, 10, 11, 12, 13, 27, 14];
const S: R = 15;
const DEBT: R = 8;
const CTX: R = 28;
const OLD: R = 7;
const CHECK: R = 16;
const INFO: R = 16;
const PSR_RESULT: R = 17;
const KEEP: u32 = 0xFF;
const CARRY: u32 = 1 << 29;
const PC: u32 = 15;
const FIQ_MODE: u32 = 0x11;
const BANKED_MODES: u32 = 1 << 0x12 | 1 << 0x13 | 1 << 0x17 | 1 << 0x1B;
const BANK_SLOTS: u32 = 2 << 4 | 1 << 14 | 3 << 22;
const SWAP_ENTRY: usize = 4;
const SWAP: u32 = STUBS as u32;
const LEAVE_PENDING: u32 = 6;
const LEAVE_RD: u32 = 22;
const AT_SHIFT: u32 = 26;
const AT_SHIFT_V4: u32 = 28;
const _: () = assert!(LEAVE_RD + 4 <= AT_SHIFT && AT_SHIFT_V4 >= CODE_CLASS + 2);
const NONSEQUENTIAL: u32 = 0;
const SEQUENTIAL: u32 = 1;
const PSR_FLAGS: u32 = 0xFF00_0000;
const NZCV: u32 = 0xF000_0000;
const MODE_BIT_4: u32 = 0x10;
const USER_MODE: u32 = 0x10;
const MODE_MASK: u32 = 0x1F;
const CONTROL_BITS: u32 = 0x0FFF_FFFF;

#[derive(Clone, Copy)]
enum Second {
    Immediate(u32),
    Register(R, Shift, u32),
    Zero,
}

struct Skip {
    label: Label,
    join: Label,
    index: usize,
    item: Item,
    pending: u32,
    nf: bool,
}

struct Slow {
    label: Label,
    back: Label,
    stub: Stub,
    word: u32,
}

struct ColdFetch {
    label: Label,
    back: Label,
    marker: u32,
}

struct Lower<'a> {
    a: Asm,
    plan: &'a Plan<'a>,
    items: &'a [Item],
    o: &'a Offsets,
    pending: u32,
    heads: Vec<Label>,
    entries: Vec<u32>,
    sites: Vec<(Label, u32)>,
    skips: Vec<Skip>,
    slows: Vec<Slow>,
    fetches: Vec<ColdFetch>,
    links: Vec<Reloc>,
    owners: [Option<usize>; LINKS],
    v5: bool,
    pages: bool,
    code: u32,
    timed: bool,
    runs: Vec<Option<usize>>,
}

pub struct A64;

impl Backend for A64 {
    fn block(lowered: &Lowered, plan: &Plan, offsets: &Offsets) -> Option<Linked> {
        block(lowered, plan, offsets)
    }

    fn runtime(setup: &Setup) -> Option<Runtime> {
        runtime(setup)
    }

    fn targets(runtime: &Runtime) -> Vec<usize> {
        let mut targets = runtime.stubs.to_vec();
        targets.push(runtime.stubs[Stub::Store as usize] - SWAP_ENTRY);
        targets
    }

    fn link(word: &mut [u8], from: usize, to: usize) -> Option<()> {
        let delta = (to as i64 - from as i64) / 4;
        if !(-(1 << 25)..1 << 25).contains(&delta) {
            return None;
        }
        let old = u32::from_le_bytes(word.try_into().ok()?);
        let new = old & 0xFC00_0000 | delta as u32 & 0x03FF_FFFF;
        word.copy_from_slice(&new.to_le_bytes());
        Some(())
    }
}

fn at_shift(v5: bool) -> u32 {
    if v5 { AT_SHIFT } else { AT_SHIFT_V4 }
}

fn left_word() -> Option<u32> {
    Info {
        at: 0,
        fetch: Fetch::Known(NONSEQUENTIAL),
        force: true,
        branch: false,
        last: Last::Ctx,
        pending: 0,
    }
    .word()
}

fn leave_word(kind: u32, ahead: u32, cost: u32, pending: u32, rd: u32) -> Option<u32> {
    (pending < 1 << LEAVE_PENDING && rd < PC)
        .then(|| memory_word(kind, ahead, cost, 0))
        .flatten()
        .map(|word| word | pending << 16 | rd << LEAVE_RD)
}

pub fn block(lowered: &Lowered, plan: &Plan, o: &Offsets) -> Option<Linked> {
    let copy = lowered.items.iter().any(|item| item.entry > 0);
    copy.then(|| assemble(lowered, plan, o, true))
        .flatten()
        .or_else(|| assemble(lowered, plan, o, false))
}

fn assemble(lowered: &Lowered, plan: &Plan, o: &Offsets, copy: bool) -> Option<Linked> {
    let count = lowered.items.len();
    let tail: Vec<Item> = if copy {
        lowered
            .items
            .iter()
            .map(|&item| Item {
                check: true,
                ahead: 0,
                entry: 0,
                live: ALL_FLAGS,
                ..item
            })
            .collect()
    } else {
        Vec::new()
    };
    let tails = if copy { count } else { 0 };
    let mut lower = Lower {
        a: Asm::new(),
        plan,
        items: &lowered.items,
        o,
        pending: 0,
        heads: Vec::new(),
        entries: Vec::new(),
        sites: Vec::new(),
        skips: Vec::new(),
        slows: Vec::new(),
        fetches: Vec::new(),
        links: Vec::new(),
        owners: link_owners(&lowered.items, plan),
        v5: lowered.v5,
        pages: lowered.pages,
        code: if lowered.v5 { 0 } else { lowered.code },
        timed: lowered.timed,
        runs: if lowered.timed && lowered.flat {
            flat_runs(&lowered.items, plan)
        } else {
            Vec::new()
        },
    };
    lower.a.emit(MARK | (tails as u32) << 16);
    for _ in 0..count + tails {
        lower.a.emit(0);
    }
    let normal = lower.pass(&lowered.items, 0)?;
    let short = if copy {
        lower.copies(&tail, &lowered.items, &normal)?
    } else {
        Vec::new()
    };
    lower.cold()?;
    let a = lower.a;
    let links = lower.links;
    let mut words = Vec::with_capacity(count + tails);
    for (index, &(head, pending)) in normal.iter().enumerate() {
        words.push(entry_word(
            a.bound(head)? as u32 - 1,
            pending,
            lowered.items[index].entry,
        )?);
    }
    for (index, short) in short.into_iter().enumerate() {
        let word = match short {
            Some((head, pending)) => entry_word(a.bound(head)? as u32 - 1, pending, 0)?,
            None => words[index],
        };
        words.push(word);
    }
    let mut bytes = a.finish()?;
    for (index, word) in words.into_iter().enumerate() {
        let at = 4 * (index + 1);
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    Some(Linked { bytes, links })
}

fn flat_runs(items: &[Item], plan: &Plan) -> Vec<Option<usize>> {
    let straight = |index: usize| plan.ops.get(index).is_some_and(|op| op.straight());
    let fits = |index: usize, written: u32| {
        let item = items[index];
        item.condition == ALWAYS
            && match item.kind {
                Kind::Alu(alu) => !alu.shifted() && straight(index),
                Kind::Constant { .. } => straight(index),
                Kind::Multiply(multiply) => {
                    run_multiply(multiply) && written & 1 << multiply.rs == 0
                }
                _ => false,
            }
    };
    let usable = items.len().saturating_sub(1);
    let mut runs = vec![None; items.len()];
    let mut first = 0;
    while first < usable {
        let mut end = first;
        let mut written = 0;
        while end < usable && end - first <= RUN_COUNT_MASK as usize && fits(end, written) {
            written |= run_writes(items[end]);
            end += 1;
            if items[end - 1].ahead > 0 {
                break;
            }
        }
        let mut last = end;
        while last > first && matches!(items[last - 1].kind, Kind::Multiply(_)) {
            last -= 1;
        }
        if last - first >= 2 {
            runs[first] = Some(last - 1);
        }
        first = end.max(first + 1);
    }
    runs
}

fn run_multiply(multiply: Multiply) -> bool {
    multiply.condition == ALWAYS
        && multiply.long.is_none()
        && !multiply.set_flags
        && [multiply.rd, multiply.rm, multiply.rs]
            .iter()
            .all(|&reg| reg != PC)
        && multiply.accumulate.is_none_or(|rn| rn != PC)
}

fn run_writes(item: Item) -> u32 {
    match item.kind {
        Kind::Alu(alu) if alu.writes_rd() => 1 << alu.rd,
        Kind::Constant { rd, .. } => 1 << rd,
        Kind::Multiply(multiply) => 1 << multiply.rd,
        _ => 0,
    }
}

impl<'a> Lower<'a> {
    fn pass(&mut self, items: &'a [Item], from: usize) -> Option<Vec<(Label, u32)>> {
        self.items = items;
        self.pending = 0;
        self.heads = (0..items.len()).map(|_| self.a.label()).collect();
        self.entries.clear();
        for index in from..items.len() {
            self.op(index)?;
        }
        Some(
            self.heads[from..]
                .iter()
                .copied()
                .zip(self.entries.drain(..))
                .collect(),
        )
    }

    fn copies(
        &mut self,
        items: &'a [Item],
        runs: &[Item],
        normal: &[(Label, u32)],
    ) -> Option<Vec<Option<(Label, u32)>>> {
        self.items = items;
        self.runs.clear();
        self.heads = (0..items.len()).map(|_| self.a.label()).collect();
        let mut words = vec![None; items.len()];
        let mut index = 0;
        while index < items.len() {
            if runs[index].entry == 0 {
                index += 1;
                continue;
            }
            let from = index;
            self.pending = 0;
            self.entries.clear();
            while runs.get(index)?.entry > 0 {
                self.op(index)?;
                index += 1;
            }
            let (head, pending) = *normal.get(index)?;
            match self.pending.cmp(&pending) {
                Ordering::Greater => self.a.sub_imm(false, false, S, S, self.pending - pending),
                Ordering::Less => self.a.add_imm(false, false, S, S, pending - self.pending),
                Ordering::Equal => {}
            }
            self.pending = 0;
            self.a.b(head);
            for (at, entry) in (from..index).zip(self.entries.drain(..)) {
                words[at] = Some((self.heads[at], entry));
            }
        }
        Some(words)
    }
}

impl Lower<'_> {
    fn address(&self, index: usize) -> u32 {
        let size = if self.plan.thumb { 2 } else { 4 };
        self.plan.pc.wrapping_add(index as u32 * size)
    }

    fn pc_read(&self, index: usize) -> u32 {
        self.address(index + 2)
    }

    fn link(&self, index: usize) -> u32 {
        if self.plan.thumb {
            self.address(index + 1) | 1
        } else {
            self.address(index + 1)
        }
    }

    fn last(&self, index: usize) -> bool {
        index + 1 == self.items.len()
    }

    fn cell(&self, at: usize) -> Option<usize> {
        self.owners.iter().position(|&owner| owner == Some(at))
    }

    fn reg(&mut self, index: u32, pc: u32, scratch: R) -> R {
        if index == PC {
            self.a.mov_imm(scratch, pc);
            scratch
        } else {
            GUEST[index as usize]
        }
    }

    fn reach(&mut self, target: u32, call: bool) {
        self.links.push(Reloc {
            offset: 4 * self.a.here() as u32,
            helper: LINK | target,
        });
        self.a.emit(if call { 0x9400_0000 } else { 0x1400_0000 });
    }

    fn call(&mut self, stub: Stub, packed: u32) {
        self.a.mov_imm(2, packed);
        self.reach(stub as u32, true);
    }

    fn leave(&mut self, stub: Stub, info: Info) -> Option<()> {
        let through = stub == Stub::Exit && !info.branch && info.at as usize == self.items.len();
        let cell = self.cell(self.items.len()).filter(|_| through);
        self.leave_cell(stub, info, cell)
    }

    fn leave_cell(&mut self, stub: Stub, info: Info, cell: Option<usize>) -> Option<()> {
        self.a.mov_imm(INFO, info.word()?);
        if let Some(cell) = cell {
            self.a.movk(true, INFO, cell as u32 + 1, 2);
        }
        self.reach(stub as u32, false);
        Some(())
    }

    fn site(&mut self, info: Info) -> Option<Label> {
        let label = self.a.label();
        self.sites.push((label, info.word()?));
        Some(label)
    }

    fn flush(&mut self) {
        if self.pending > 0 {
            self.a.sub_imm(false, false, S, S, self.pending);
            self.pending = 0;
        }
    }

    fn spend(&mut self, cost: u32, keep: bool) -> Last {
        if !self.v5 {
            return self.spend_v4(cost, keep);
        }
        let last = match cost {
            0 => {
                self.a.add_imm(false, false, DEBT, DEBT, 1);
                Last::One
            }
            1 => Last::One,
            _ => {
                if keep {
                    self.a.mov(OLD, DEBT);
                }
                self.a.sub_imm(false, false, DEBT, DEBT, cost - 1);
                self.a
                    .logical(Logic::Bic, false, DEBT, DEBT, DEBT, Shift::Asr, 31);
                Last::Old(cost)
            }
        };
        self.pending += cost;
        if self.pending > PENDING_LIMIT {
            self.flush();
        }
        last
    }

    fn spend_v4(&mut self, cost: u32, keep: bool) -> Last {
        let last = match cost {
            1 => Last::One,
            _ => {
                if keep {
                    self.a.mov(OLD, ZR);
                }
                Last::Old(cost)
            }
        };
        self.pending += cost;
        if self.pending > PENDING_LIMIT {
            self.flush();
        }
        last
    }

    fn fetch(&mut self, index: usize, idles: Option<R>) {
        if let (None, Some((marker, latch))) = (idles, self.quick_word(index)) {
            self.fetch_word(index, marker, latch);
            return;
        }
        let thumb = if self.plan.thumb { THUMB } else { 0 };
        if let Some(idles) = idles {
            let word = self.iwram_word(index);
            let flag = if word.is_some() { IWRAM_IDLES } else { 0 };
            self.a.mov_imm(2, thumb | flag);
            self.a
                .logical(Logic::Orr, false, 2, 2, idles, Shift::Lsl, IDLES_SHIFT);
            if let Some(word) = word {
                self.a.mov_imm(3, word);
            }
        }
        self.a.mov_imm(1, self.pc_read(index));
        if idles.is_none() {
            self.quick_args(index);
        }
        let stub = match idles {
            Some(_) => Stub::Fetch,
            None if self.plan.thumb => Stub::FetchThumb,
            None => Stub::FetchArm,
        };
        self.reach(stub as u32, true);
    }

    fn quick_args(&mut self, index: usize) {
        let Some(quick) = self.o.quick else {
            return;
        };
        let addr = self.pc_read(index);
        let word = self.plan.words.get(index + 2).copied();
        match (quick.rom_offset(addr), word) {
            (Some(from), Some(word)) => {
                let last = if self.plan.thumb {
                    (word & 0xFFFF) * 0x0001_0001
                } else {
                    word
                };
                self.a.mov_imm(2, from as u32);
                self.a.mov_imm(3, last);
            }
            (None, Some(word)) if !self.plan.thumb && addr >> 24 == quick.iwram_page => {
                self.a.mov_imm(2, IWRAM_FETCH);
                self.a.mov_imm(3, word);
            }
            (None, Some(word)) if !self.plan.thumb && addr < quick.bios_end => {
                self.a.mov_imm(2, BIOS_FETCH);
                self.a.mov_imm(3, word);
            }
            _ => self.a.mov_imm(2, u32::MAX),
        }
    }

    fn quick_word(&self, index: usize) -> Option<(u32, u32)> {
        let quick = self.o.quick?;
        let addr = self.pc_read(index);
        let word = *self.plan.words.get(index + 2)?;
        if self.plan.thumb || quick.rom_offset(addr).is_some() {
            None
        } else if addr >> 24 == quick.iwram_page {
            Some((IWRAM_FETCH, word))
        } else if addr < quick.bios_end {
            Some((BIOS_FETCH, word))
        } else {
            None
        }
    }

    fn fetch_word(&mut self, index: usize, marker: u32, latch: u32) {
        let Some(q) = self.o.quick else {
            return;
        };
        let (label, back) = (self.a.label(), self.a.label());
        let open = if marker == IWRAM_FETCH {
            q.open_iwram
        } else {
            q.open_bios
        };
        let bit = q.prefetch_bit.trailing_zeros();
        self.a.mov_imm(1, self.pc_read(index));
        self.a.mov_imm(3, latch);
        self.a.ldr(true, 4, CTX, self.o.bus);
        self.bus_field(4, false, 5, q.now);
        self.bus_field(4, false, 6, q.dma_check);
        self.a.arith(true, true, false, 6, 5, 6, Shift::Lsl, 0);
        self.a.tbz(false, 6, 63, label);
        self.bus_field(1, false, 6, q.wait_bits + (bit / 8) as i32);
        self.a.tbz(false, 6, bit % 8, label);
        self.bus_field(8, false, 6, q.clock);
        self.a.add_imm(true, false, 6, 6, 1);
        self.bus_field(8, true, 6, q.clock);
        self.bus_field(1, true, ZR, q.branched);
        self.bus_field(4, true, 3, open);
        self.a.add_imm(false, false, 5, 5, 1);
        self.bus_field(4, true, 5, q.sample);
        self.bus_field(4, true, 5, q.now);
        self.bus_field(4, true, 1, q.open_addr);
        self.bus_field(4, true, 3, q.open_last);
        self.a.sub_imm(false, false, S, S, 1);
        self.a.mov_imm(2, 1);
        self.a.str(false, 2, CTX, self.o.last);
        self.a.bind(back);
        self.fetches.push(ColdFetch {
            label,
            back,
            marker,
        });
    }

    fn bus_field(&mut self, scale: u32, store: bool, rt: R, offset: i32) {
        let offset = offset as u32;
        let (base, offset) = if offset.is_multiple_of(scale) && offset / scale < 0x1000 {
            (4, offset)
        } else {
            self.a.mov_imm(17, offset);
            self.a.arith(true, false, false, 17, 4, 17, Shift::Lsl, 0);
            (17, 0)
        };
        match (scale, store) {
            (1, false) => self.a.ldrb(rt, base, offset),
            (1, true) => self.a.strb(rt, base, offset),
            (8, false) => self.a.ldr(true, rt, base, offset),
            (8, true) => self.a.str(true, rt, base, offset),
            (_, false) => self.a.ldr(false, rt, base, offset),
            (_, true) => self.a.str(false, rt, base, offset),
        }
    }

    fn iwram_word(&self, index: usize) -> Option<u32> {
        let quick = self.o.quick?;
        if self.plan.thumb || self.pc_read(index) >> 24 != quick.iwram_page {
            return None;
        }
        self.plan.words.get(index + 2).copied()
    }

    fn fetch_idles(&mut self, index: usize, idles: u32) {
        let thumb = if self.plan.thumb { THUMB } else { 0 };
        let packed = thumb | idles << IDLES_SHIFT;
        match self.iwram_word(index) {
            Some(word) => {
                self.a.mov_imm(2, packed | IWRAM_IDLES);
                self.a.mov_imm(3, word);
            }
            None => self.a.mov_imm(2, packed),
        }
        self.a.mov_imm(1, self.pc_read(index));
        self.reach(Stub::Fetch as u32, true);
    }

    fn flat_run(&mut self, first: usize, end: usize) -> Option<()> {
        let slow = self.a.label();
        let thumb = if self.plan.thumb { THUMB } else { 0 };
        let count = (end - first + 1) as u32;
        let latch = self.flat_latch(first, end);
        let flag = if latch.is_some() { IWRAM_RUN } else { 0 };
        let reserve = self.pending + self.items[end].ahead;
        if !Asm::fits_arith(reserve) {
            return None;
        }
        let packed = thumb | flag | (count - 1) << IDLES_SHIFT | count << RUN_CYCLES_SHIFT;
        let multiplies: Vec<(usize, Multiply)> = (first..=end)
            .filter_map(|index| match self.items[index].kind {
                Kind::Multiply(multiply) => Some((index, multiply)),
                _ => None,
            })
            .collect();
        if multiplies.is_empty() {
            self.a.mov_imm(1, self.pc_read(first));
            self.a.mov_imm(2, packed);
        } else {
            self.a.mov_imm(2, packed);
            for (index, multiply) in multiplies {
                self.multiply_cycles(index, multiply)?;
                self.a
                    .arith(false, false, false, 2, 2, 3, Shift::Lsl, RUN_CYCLES_SHIFT);
                if index == first {
                    self.a
                        .arith(false, false, false, 2, 2, 3, Shift::Lsl, RUN_FIRST_SHIFT);
                }
            }
            self.a.mov_imm(1, self.pc_read(first));
        }
        if let Some(latch) = latch {
            self.a.mov_imm(3, latch);
        }
        if reserve > 0 {
            self.a.sub_imm(false, false, S, S, reserve);
        }
        self.reach(Stub::FetchFlat as u32, true);
        if reserve > 0 {
            self.a.add_imm(false, false, S, S, reserve);
        }
        self.a.cbz(false, 1, slow);
        for index in first..=end {
            match self.items[index].kind {
                Kind::Alu(alu) => self.alu(index, alu),
                Kind::Constant { rd, value } => self.a.mov_imm(GUEST[rd as usize], value),
                Kind::Multiply(multiply) => self.multiply(multiply),
                _ => return None,
            }
        }
        self.hand_fetch(end, Fetch::Known(SEQUENTIAL));
        self.a.b(*self.heads.get(end + 1)?);
        self.a.bind(slow);
        Some(())
    }

    fn flat_latch(&self, first: usize, end: usize) -> Option<u32> {
        let words = [
            *self.plan.words.get(end + 1)?,
            *self.plan.words.get(end + 2)?,
        ];
        self.o.quick?.iwram_latch(
            self.pc_read(first),
            self.pc_read(end),
            self.plan.thumb,
            words,
        )
    }

    fn spend_counted(&mut self, cycles: R) -> Last {
        self.a
            .arith(false, true, false, S, S, cycles, Shift::Lsl, 0);
        self.a.str(false, cycles, CTX, self.o.last);
        Last::Ctx
    }

    fn charged(&self, index: usize) -> u32 {
        if self.v5 {
            self.items[index].cost
        } else {
            index as u32
        }
    }

    fn check(&mut self, at: u32, ahead: u32, info: Info) -> Option<()> {
        self.a
            .arith(false, true, false, CHECK, S, DEBT, Shift::Lsl, 0);
        self.a
            .sub_imm(false, false, CHECK, CHECK, self.pending + ahead + 1);
        let site = self.site(Info { at, ..info })?;
        self.a.tbz(true, CHECK, 31, site);
        Some(())
    }

    fn after_known(&mut self, index: usize, last: Last, fetch: u32) -> Option<()> {
        let item = self.items[index];
        let info = Info {
            at: index as u32 + 1,
            fetch: Fetch::Known(fetch),
            force: false,
            branch: false,
            last,
            pending: self.pending,
        };
        if self.last(index) {
            return self.leave(Stub::Exit, info);
        }
        if item.check {
            self.check(index as u32 + 1, item.ahead, info)?;
        }
        self.hand_fetch(index, Fetch::Known(fetch));
        Some(())
    }

    fn hand_fetch(&mut self, index: usize, fetch: Fetch) {
        let Some(next) = self.items.get(index + 1) else {
            return;
        };
        if self.timed
            || matches!(
                next.kind,
                Kind::Step
                    | Kind::Exit
                    | Kind::Msr { spsr: false, .. }
                    | Kind::Restore { .. }
                    | Kind::UserBlock(_)
                    | Kind::Cp15 { load: None }
            )
        {
            let value = match fetch {
                Fetch::Known(access) => access,
                Fetch::Core => KEEP,
            };
            self.a.mov_imm(0, value);
        }
    }

    fn op(&mut self, index: usize) -> Option<()> {
        let head = self.heads[index];
        self.a.bind(head);
        self.entries.push(self.pending);
        let run = self.runs.get(index).copied().flatten();
        if let Some(end) = run {
            self.flat_run(index, end)?;
        }
        let item = self.items[index];
        match item.kind {
            _ if !self.timed || run.is_some() => {}
            Kind::Step | Kind::Exit | Kind::Multiply(_) => {}
            Kind::Alu(alu) if alu.shifted() => {
                self.fetch_idles(index, SHIFT_IDLES);
            }
            _ => self.fetch(index, None),
        }
        match item.kind {
            Kind::Alu(alu) => {
                self.conditional(item.condition, |s| s.alu(index, alu));
                let last = if self.timed {
                    Last::Ctx
                } else {
                    self.spend(item.spend?, item.check)
                };
                let fetch = if alu.shifted() {
                    NONSEQUENTIAL
                } else {
                    SEQUENTIAL
                };
                self.after_known(index, last, fetch)
            }
            Kind::Constant { rd, value } => {
                self.a.mov_imm(GUEST[rd as usize], value);
                let last = self.spend_fetched(item.cost, item.check);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Multiply(multiply) => {
                let last = match item.spend {
                    Some(spend) => {
                        self.multiply(multiply);
                        self.multiply_flags(index, multiply);
                        self.spend(spend, item.check)
                    }
                    None if self.timed => {
                        if run.is_none() {
                            self.multiply_cycles(index, multiply)?;
                            self.fetch(index, Some(3));
                        }
                        self.multiply(multiply);
                        Last::Ctx
                    }
                    None => {
                        self.multiply_cycles(index, multiply)?;
                        self.multiply(multiply);
                        self.spend_counted(3)
                    }
                };
                self.after_known(index, last, NONSEQUENTIAL)
            }
            Kind::Mrs { rd, spsr } => {
                if spsr {
                    self.mrs_spsr(rd);
                } else {
                    self.mrs(rd);
                }
                let last = self.spend(item.cost, item.check);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Msr {
                operand,
                mask,
                spsr: false,
            } => self.msr(index, operand, mask),
            Kind::Msr {
                operand,
                mask,
                spsr: true,
            } => self.spsr(index, operand, mask),
            Kind::Clz { rd, rm } => {
                self.a.clz(GUEST[rd as usize], GUEST[rm as usize]);
                let last = self.spend(item.spend?, item.check);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Cp15 { load } => self.cp15(index, load),
            Kind::Restore { offset } => self.restore(index, offset),
            Kind::Transfer(transfer) => self.transfer(index, transfer),
            Kind::Block(block) => self.block(index, block, false),
            Kind::UserBlock(block) => self.user_block(index, block),
            Kind::Swap(swap) => self.swap(index, swap),
            Kind::Step => self.step(index),
            Kind::Exit => self.step_out(index),
            Kind::Branch(word) => self.branch(index, word),
            Kind::Jump(jump) => self.jump(index, jump),
            Kind::LoadPc(transfer) => self.load_pc(index, transfer),
        }
    }

    fn spend_fetched(&mut self, cost: u32, keep: bool) -> Last {
        if self.timed {
            Last::Ctx
        } else {
            self.spend(cost, keep)
        }
    }

    fn conditional(&mut self, condition: u32, body: impl FnOnce(&mut Self)) {
        if condition == ALWAYS {
            body(self);
            return;
        }
        let skip = self.a.label();
        self.a.b_cond(invert(condition), skip);
        body(self);
        self.a.bind(skip);
    }

    fn second(&mut self, operand: Operand, pc: u32, value: R, carry: R) -> Second {
        match operand {
            Operand::Immediate { value, .. } => Second::Immediate(value),
            Operand::Shifted { rm, shift, rs } => self.shifted(rm, shift, rs, value, None),
            Operand::Register { rm, shift, amount } => {
                let rm = self.reg(rm, pc, value);
                match (shift, amount) {
                    (0, n) => Second::Register(rm, Shift::Lsl, n),
                    (1, 0) => Second::Zero,
                    (1, n) => Second::Register(rm, Shift::Lsr, n),
                    (2, 0) => Second::Register(rm, Shift::Asr, 31),
                    (2, n) => Second::Register(rm, Shift::Asr, n),
                    (_, 0) => {
                        self.a.cset(carry, 2);
                        self.a.lsr(value, rm, 1);
                        self.a
                            .logical(Logic::Orr, false, value, value, carry, Shift::Lsl, 31);
                        Second::Register(value, Shift::Lsl, 0)
                    }
                    (_, n) => Second::Register(rm, Shift::Ror, n),
                }
            }
        }
    }

    fn shifted(&mut self, rm: u32, shift: u32, rs: u32, value: R, carry: Option<R>) -> Second {
        const AMOUNT: R = 0;
        const FRESH: R = 1;
        const SAVED: R = 4;
        const WORK: R = 6;
        const EQ: u32 = 0;
        const LO: u32 = 3;
        let (rm, rs) = (GUEST[rm as usize], GUEST[rs as usize]);
        let clamp = shift != 3;
        let touches = clamp || carry.is_some();
        let (immr, imms) = logical_immediate(0xFF).expect("0xFF is a logical immediate");
        self.a.logical_imm(Logic::And, AMOUNT, rs, immr, imms);
        if touches {
            self.a.mrs_nzcv(SAVED);
        }
        if clamp {
            self.a.mov_imm(WORK, 33);
            self.a.cmp_imm(AMOUNT, 33);
            self.a.csel(AMOUNT, AMOUNT, WORK, LO);
        }
        match shift {
            0 => {
                self.a.mov(WORK, rm);
                self.a.lslv64(WORK, WORK, AMOUNT);
                if carry.is_some() {
                    self.a.ubfm(true, FRESH, WORK, 32, 32);
                }
                self.a.mov(value, WORK);
            }
            1 | 2 => {
                if shift == 1 {
                    self.a.mov(WORK, rm);
                } else {
                    self.a.sxtw(WORK, rm);
                }
                self.a.ubfm(true, WORK, WORK, 63, 62);
                if shift == 1 {
                    self.a.lsrv64(WORK, WORK, AMOUNT);
                } else {
                    self.a.asrv64(WORK, WORK, AMOUNT);
                }
                if carry.is_some() {
                    self.a.ubfx(FRESH, WORK, 0, 1);
                }
                self.a.lsr64(value, WORK, 1);
            }
            _ => {
                self.a.rorv(value, rm, AMOUNT);
                if carry.is_some() {
                    self.a.lsr(FRESH, value, 31);
                }
            }
        }
        if let Some(carry) = carry {
            self.a.ubfx(carry, SAVED, 29, 1);
            self.a.cmp_imm(AMOUNT, 0);
            self.a.csel(carry, carry, FRESH, EQ);
        }
        if touches {
            self.a.msr_nzcv(SAVED);
        }
        Second::Register(value, Shift::Lsl, 0)
    }

    fn plain(&mut self, second: Second, scratch: R) -> R {
        match second {
            Second::Immediate(value) => {
                self.a.mov_imm(scratch, value);
                scratch
            }
            Second::Zero => ZR,
            Second::Register(rm, Shift::Lsl, 0) => rm,
            Second::Register(rm, shift, amount) => {
                match shift {
                    Shift::Lsl => self.a.lsl(scratch, rm, amount),
                    Shift::Lsr => self.a.lsr(scratch, rm, amount),
                    Shift::Asr => self.a.asr(scratch, rm, amount),
                    Shift::Ror => self.a.ror(scratch, rm, amount),
                }
                scratch
            }
        }
    }

    fn alu(&mut self, index: usize, alu: Alu) {
        if alu.logical() {
            self.logical(index, alu);
        } else {
            self.arithmetic(index, alu);
        }
    }

    fn arithmetic(&mut self, index: usize, alu: Alu) {
        let pc = self.pc_read(index);
        let dest = if alu.writes_rd() {
            GUEST[alu.rd as usize]
        } else {
            ZR
        };
        let flags = alu.set_flags;
        let second = self.second(alu.operand, pc, 2, 3);
        let first = self.reg(alu.rn, pc, 1);
        let (sub, swap, carry) = match alu.opcode {
            0x2 | 0xA => (true, false, false),
            0x3 => (true, true, false),
            0x4 | 0xB => (false, false, false),
            0x5 => (false, false, true),
            0x6 => (true, false, true),
            _ => (true, true, true),
        };
        if carry || swap {
            let second = self.plain(second, 2);
            let (a, b) = if swap {
                (second, first)
            } else {
                (first, second)
            };
            if carry {
                self.a.carry(sub, flags, dest, a, b);
            } else {
                self.a.arith(false, true, flags, dest, a, b, Shift::Lsl, 0);
            }
            return;
        }
        match second {
            Second::Immediate(value) if Asm::fits_arith(value) => {
                self.a.arith_imm(false, sub, flags, dest, first, value);
            }
            Second::Immediate(value) if !flags && Asm::fits_arith(value.wrapping_neg()) => {
                self.a
                    .arith_imm(false, !sub, false, dest, first, value.wrapping_neg());
            }
            Second::Register(rm, shift, amount) if shift != Shift::Ror => {
                self.a
                    .arith(false, sub, flags, dest, first, rm, shift, amount);
            }
            other => {
                let rm = self.plain(other, 2);
                self.a
                    .arith(false, sub, flags, dest, first, rm, Shift::Lsl, 0);
            }
        }
    }

    fn logical(&mut self, index: usize, alu: Alu) {
        let pc = self.pc_read(index);
        let live = self.items[index].live;
        let carry = match alu.operand {
            Operand::Immediate { carry, .. } => carry.map(Carry::Fixed),
            Operand::Register { rm, shift, amount } => match (shift, amount) {
                (0, 0) => None,
                (0, n) => Some(Carry::Bit(rm, 32 - n)),
                (1 | 2, 0) => Some(Carry::Bit(rm, 31)),
                (_, 0) => Some(Carry::Bit(rm, 0)),
                (_, n) => Some(Carry::Bit(rm, n - 1)),
            },
            Operand::Shifted { .. } => Some(Carry::Host),
        };
        let needs_nz = alu.set_flags && live & (FLAG_N | FLAG_Z) != 0;
        let needs_c = alu.set_flags && live & FLAG_C != 0;
        let needs_v = alu.set_flags && live & FLAG_V != 0;
        let cheap_c = !needs_c || carry == Some(Carry::Fixed(false));
        let merge = alu.set_flags && !(cheap_c && !needs_v);
        let flags = alu.set_flags && (needs_nz || needs_c || merge);
        let dest = match (alu.writes_rd(), flags) {
            (true, _) => GUEST[alu.rd as usize],
            (false, true) => 0,
            (false, false) => return,
        };
        let shifted = match alu.operand {
            Operand::Shifted { rm, shift, rs } => {
                Some(self.shifted(rm, shift, rs, 2, merge.then_some(3)))
            }
            _ => None,
        };
        if merge {
            if let Some(Carry::Bit(rm, bit)) = carry {
                let rm = self.reg(rm, pc, 3);
                self.a.ubfx(3, rm, bit, 1);
            }
            self.a.mrs_nzcv(4);
        }
        let second = match shifted {
            Some(second) => second,
            None => self.second(alu.operand, pc, 2, 5),
        };
        let first = match alu.opcode {
            0xD | 0xF => ZR,
            _ => self.reg(alu.rn, pc, 1),
        };
        let direct = flags
            && !merge
            && matches!(alu.opcode, 0x0 | 0x8 | 0xE)
            && self.logic_into(Logic::Ands, alu.opcode == 0xE, dest, first, second);
        if direct {
            return;
        }
        let (logic, invert) = match alu.opcode {
            0x0 | 0x8 => (Logic::And, false),
            0x1 | 0x9 => (Logic::Eor, false),
            0xC | 0xD => (Logic::Orr, false),
            0xE => (Logic::And, true),
            _ => (Logic::Orr, true),
        };
        match (alu.opcode, second) {
            (0xD, Second::Immediate(value)) => self.a.mov_imm(dest, value),
            (0xF, Second::Immediate(value)) => self.a.mov_imm(dest, !value),
            _ => {
                let done = self.logic_into(logic, invert, dest, first, second);
                debug_assert!(done);
            }
        }
        if !flags {
            return;
        }
        let result = if dest == ZR { 0 } else { dest };
        self.a.tst(result, result);
        if !merge {
            return;
        }
        self.a.mrs_nzcv(5);
        self.a.bfm(5, 4, 0, 29);
        match carry {
            Some(Carry::Fixed(set)) => {
                let (logic, mask) = if set {
                    (Logic::Orr, CARRY)
                } else {
                    (Logic::And, !CARRY)
                };
                if let Some((immr, imms)) = logical_immediate(mask) {
                    self.a.logical_imm(logic, 5, 5, immr, imms);
                }
            }
            Some(Carry::Bit(..) | Carry::Host) => self.a.bfi(5, 3, 29, 1),
            None => {}
        }
        self.a.msr_nzcv(5);
    }

    fn logic_into(
        &mut self,
        logic: Logic,
        invert: bool,
        dest: R,
        first: R,
        second: Second,
    ) -> bool {
        match second {
            Second::Immediate(value) => {
                let value = if invert { !value } else { value };
                match logical_immediate(value) {
                    Some((immr, imms)) if dest != ZR || logic == Logic::Ands => {
                        self.a.logical_imm(logic, dest, first, immr, imms);
                    }
                    _ => {
                        self.a.mov_imm(6, value);
                        self.a.logical(logic, false, dest, first, 6, Shift::Lsl, 0);
                    }
                }
            }
            Second::Zero => {
                let logic = if invert { logic.inverted() } else { logic };
                self.a.logical(logic, false, dest, first, ZR, Shift::Lsl, 0);
            }
            Second::Register(rm, shift, amount) => {
                let logic = if invert { logic.inverted() } else { logic };
                self.a.logical(logic, false, dest, first, rm, shift, amount);
            }
        }
        true
    }

    fn multiply(&mut self, multiply: Multiply) {
        let rm = GUEST[multiply.rm as usize];
        let rs = GUEST[multiply.rs as usize];
        let rd = GUEST[multiply.rd as usize];
        match multiply.long {
            None => {
                let acc = multiply.accumulate.map_or(ZR, |rn| GUEST[rn as usize]);
                self.a.mul(rd, rm, rs, acc);
            }
            Some(long) => {
                let low = GUEST[long.low as usize];
                let acc = if long.accumulate {
                    self.a.mov(0, low);
                    self.a.logical(Logic::Orr, true, 0, 0, rd, Shift::Lsl, 32);
                    0
                } else {
                    ZR
                };
                self.a.mul_long(long.signed, 0, rm, rs, acc);
                self.a.lsr64(1, 0, 32);
                self.a.mov(low, 0);
                self.a.mov(rd, 1);
            }
        }
    }

    fn multiply_flags(&mut self, index: usize, multiply: Multiply) {
        let live = self.items[index].live;
        if !multiply.set_flags || live & (FLAG_N | FLAG_Z) == 0 {
            return;
        }
        let rd = GUEST[multiply.rd as usize];
        let merge = live & (FLAG_C | FLAG_V) != 0;
        if merge {
            self.a.mrs_nzcv(4);
        }
        self.a.tst(rd, rd);
        if merge {
            self.a.mrs_nzcv(5);
            self.a.bfm(5, 4, 0, 29);
            self.a.msr_nzcv(5);
        }
    }

    fn multiply_cycles(&mut self, index: usize, multiply: Multiply) -> Option<()> {
        let rs = GUEST[multiply.rs as usize];
        let extra = match multiply.long {
            Some(long) => 1 + u32::from(long.accumulate),
            None => u32::from(multiply.accumulate.is_some()),
        };
        let fetch = if self.timed {
            0
        } else {
            self.plan.fetch[index][0]
        };
        let fixed = 39 + 8 * (extra + fetch);
        if !Asm::fits_arith(fixed) {
            return None;
        }
        let (immr, imms) = logical_immediate(1)?;
        if multiply.long.is_some_and(|long| !long.signed) {
            self.a.logical_imm(Logic::Orr, 3, rs, immr, imms);
        } else {
            self.a.logical(Logic::Eor, false, 3, rs, rs, Shift::Asr, 31);
            self.a.logical_imm(Logic::Orr, 3, 3, immr, imms);
        }
        self.a.clz(3, 3);
        self.a.mov_imm(4, fixed);
        self.a.arith(false, true, false, 3, 4, 3, Shift::Lsl, 0);
        self.a.lsr(3, 3, 3);
        Some(())
    }

    fn offset(&mut self, offset: Offset, pc: u32) -> Option<Second> {
        match offset {
            Offset::Immediate(0) => None,
            Offset::Immediate(value) => Some(Second::Immediate(value)),
            Offset::Register(rm) => Some(Second::Register(self.reg(rm, pc, 4), Shift::Lsl, 0)),
            Offset::Shifted { rm, shift, amount } => {
                let second = self.second(Operand::Register { rm, shift, amount }, pc, 4, 5);
                match second {
                    Second::Zero => None,
                    Second::Register(rm, Shift::Ror, amount) => {
                        self.a.ror(4, rm, amount);
                        Some(Second::Register(4, Shift::Lsl, 0))
                    }
                    other => Some(other),
                }
            }
        }
    }

    fn moved(&mut self, dest: R, base: R, offset: Option<Second>, up: bool) {
        match offset {
            None | Some(Second::Zero) => {
                if dest != base {
                    self.a.mov(dest, base);
                }
            }
            Some(Second::Immediate(value)) => {
                self.a.arith_imm(false, !up, false, dest, base, value)
            }
            Some(Second::Register(rm, shift, amount)) => {
                self.a
                    .arith(false, !up, false, dest, base, rm, shift, amount);
            }
        }
    }

    fn literal(&self, index: usize, transfer: Transfer) -> Option<u32> {
        if self.o.quick.is_none() || !self.timed || self.v5 || self.pages || !self.plan.thumb {
            return None;
        }
        let addr = transfer.pc_literal(self.pc_read(index))?;
        let literal = self.plan.literal(index)?;
        (literal.addr == addr).then_some(literal.word)
    }

    fn transfer(&mut self, index: usize, transfer: Transfer) -> Option<()> {
        debug_assert!(transfer.width != Width::Word || !transfer.signed);
        let item = self.items[index];
        let pending = self.pending;
        let conditional = item.condition != ALWAYS;
        let skip = conditional.then(|| self.a.label());
        if let Some(skip) = skip {
            self.a.b_cond(invert(item.condition), skip);
        }
        let pc = self.pc_read(index);
        if !transfer.load() {
            let value = pc.wrapping_add(4);
            if transfer.rd == PC {
                let value = match transfer.width {
                    Width::Byte => value & 0xFF,
                    Width::Half => value & 0xFFFF,
                    Width::Word => value,
                };
                self.a.mov_imm(1, value);
            } else {
                let rd = GUEST[transfer.rd as usize];
                match transfer.width {
                    Width::Byte => self.a.logical_imm(Logic::And, 1, rd, 0, 7),
                    Width::Half => self.a.logical_imm(Logic::And, 1, rd, 0, 15),
                    Width::Word => self.a.mov(1, rd),
                }
            }
        }
        let base = self.reg(transfer.rn, pc, 3);
        let offset = self.offset(transfer.offset, pc);
        match (transfer.writeback(), transfer.pre()) {
            (true, true) => {
                self.moved(base, base, offset, transfer.up());
                self.a.mov(0, base);
            }
            (true, false) => {
                self.a.mov(0, base);
                self.moved(base, base, offset, transfer.up());
            }
            (false, true) => self.moved(0, base, offset, transfer.up()),
            (false, false) => self.a.mov(0, base),
        }
        let paged = transfer.load()
            && self.v5
            && self.pages
            && !self.timed
            && (self.tag(index).is_some() || self.last(index));
        if paged {
            self.flush();
        }
        let literal = self.literal(index, transfer);
        debug_assert!(!paged || literal.is_none());
        let kind = transfer.kind() | literal.map_or(0, |_| ROM_LITERAL);
        let (ahead, cost) = (item.ahead, self.charged(index));
        let leave = self.tag(index).and_then(|tag| match transfer.load() {
            true => leave_word(kind, ahead, cost, self.pending, transfer.rd).map(|word| word | tag),
            false => memory_word(kind, ahead, cost, self.pending).map(|word| word | tag),
        });
        let (stub, packed) = match (transfer.load(), leave) {
            (true, Some(word)) => (Stub::LoadLeave, word),
            (false, Some(word)) => (Stub::StoreLeave, word),
            (true, None) => (Stub::Load, memory_word(kind, ahead, cost, self.pending)?),
            (false, None) => (Stub::Store, memory_word(kind, ahead, cost, self.pending)?),
        };
        let packed = packed | self.code << CODE_CLASS;
        if paged {
            self.paged_load(transfer, cost, stub, packed)?;
        } else if let Some(word) = literal {
            self.a
                .mov_imm64(2, u64::from(packed) | u64::from(word) << 32);
            self.reach(stub as u32, true);
        } else {
            self.call(stub, packed);
        }
        self.pending = 0;
        if transfer.load() {
            self.a.mov(GUEST[transfer.rd as usize], 0);
        }
        self.after_memory(index, skip, pending, leave.is_none())
    }

    fn paged_load(&mut self, transfer: Transfer, cost: u32, stub: Stub, word: u32) -> Option<()> {
        let (slow, back, long, timed) = (
            self.a.label(),
            self.a.label(),
            self.a.label(),
            self.a.label(),
        );
        self.a.ldr(true, 4, CTX, self.o.pages);
        self.a.lsr(5, 0, PAGE_SHIFT);
        self.a.arith(true, false, false, 5, 4, 5, Shift::Lsl, 5);
        self.a.pair(true, true, 6, 7, 5, 0);
        self.a.cbz(false, 7, slow);
        match transfer.width {
            Width::Word => {
                let (immr, imms) = logical_immediate(!3)?;
                self.a.logical_imm(Logic::And, 5, 0, immr, imms);
                self.a.indexed(Indexed::LoadWord, 4, 6, 5);
                self.a.lsl(5, 0, 3);
                self.a.rorv(0, 4, 5);
            }
            Width::Half => {
                let (immr, imms) = logical_immediate(!1)?;
                self.a.logical_imm(Logic::And, 5, 0, immr, imms);
                let op = if transfer.signed {
                    Indexed::LoadSignedHalf
                } else {
                    Indexed::LoadHalf
                };
                self.a.indexed(op, 0, 6, 5);
            }
            Width::Byte => {
                let op = if transfer.signed {
                    Indexed::LoadSignedByte
                } else {
                    Indexed::LoadByte
                };
                self.a.indexed(op, 0, 6, 0);
            }
        }
        let (cycles, flag) = match transfer.width {
            Width::Word => (WORD_CYCLES, LONG_WORD),
            Width::Byte | Width::Half => (HALF_CYCLES, LONG_HALF),
        };
        self.a.mrs_nzcv(3);
        self.a.tbz(true, 7, flag, long);
        self.a.ubfx(7, 7, cycles, 8);
        self.a.mov_imm(4, cost);
        self.a.cmp(4, 7);
        self.a.csel(6, 4, 7, 0x8);
        self.a.b(timed);
        self.a.bind(long);
        self.a.ubfx(7, 7, cycles, 8);
        self.a
            .add_imm(false, false, 6, 7, cost.saturating_sub(DATA_OVERLAP));
        self.a.bind(timed);
        self.a.arith(false, true, false, S, S, 6, Shift::Lsl, 0);
        self.a.sub_imm(false, false, 7, 6, 1);
        self.a.cmp(DEBT, 7);
        self.a.csel(7, DEBT, 7, 0x3);
        self.a
            .arith(false, true, false, DEBT, DEBT, 7, Shift::Lsl, 0);
        self.a.arith(false, true, false, 7, 6, 7, Shift::Lsl, 0);
        self.a.str(false, 7, CTX, self.o.last);
        self.a.str(false, ZR, CTX, self.o.force);
        self.a.msr_nzcv(3);
        self.a.bind(back);
        self.slows.push(Slow {
            label: slow,
            back,
            stub,
            word,
        });
        Some(())
    }

    fn tag(&self, index: usize) -> Option<u32> {
        let at = index as u32 + 1;
        let shift = at_shift(self.v5);
        (!self.last(index) && at <= u32::MAX >> shift).then_some(at << shift)
    }

    fn swap(&mut self, index: usize, swap: Swap) -> Option<()> {
        let item = self.items[index];
        let pending = self.pending;
        let rm = GUEST[swap.rm as usize];
        let kind = if swap.byte {
            self.a.logical_imm(Logic::And, 1, rm, 0, 7);
            0
        } else {
            self.a.mov(1, rm);
            2
        };
        self.a.mov(0, GUEST[swap.rn as usize]);
        let packed = memory_word(kind, item.ahead, item.cost, self.pending)?;
        self.a.mov_imm(2, packed);
        self.reach(SWAP, true);
        self.pending = 0;
        self.a.mov(GUEST[swap.rd as usize], 0);
        self.after_memory(index, None, pending, true)
    }

    fn load_pc(&mut self, index: usize, transfer: Transfer) -> Option<()> {
        let condition = self.items[index].condition;
        let skip = (condition != ALWAYS).then(|| self.a.label());
        if let Some(skip) = skip {
            self.a.b_cond(invert(condition), skip);
        }
        let pending = self.pending;
        let pc = self.pc_read(index);
        let base = self.reg(transfer.rn, pc, 3);
        let offset = self.offset(transfer.offset, pc);
        match (transfer.writeback(), transfer.pre()) {
            (true, true) => {
                self.moved(base, base, offset, transfer.up());
                self.a.mov(0, base);
            }
            (true, false) => {
                self.a.mov(0, base);
                self.moved(base, base, offset, transfer.up());
            }
            (false, true) => self.moved(0, base, offset, transfer.up()),
            (false, false) => self.a.mov(0, base),
        }
        let word = load_pc_word(
            transfer.kind(),
            self.charged(index),
            index as u32 + 1,
            self.pending,
        )?;
        self.pending = 0;
        self.a.mov_imm(2, word);
        self.reach(Stub::LoadPc as u32, false);
        self.skipped(index, skip, pending)
    }

    fn user_block(&mut self, index: usize, block: Block) -> Option<()> {
        if !self.last(index) {
            return None;
        }
        let pending = self.pending;
        let (banked, fiq) = (self.a.label(), self.a.label());
        self.a.ldr(true, 4, CTX, self.o.core);
        self.a.ldr(false, 5, 4, self.o.cpsr);
        self.a.ubfm(false, 5, 5, 0, 4);
        self.a.sub_imm(false, false, 6, 5, FIQ_MODE);
        self.a.cbz(false, 6, fiq);
        self.a.mov_imm(6, BANKED_MODES);
        self.a.lsrv64(6, 6, 5);
        self.a.tbz(true, 6, 0, banked);
        self.block(index, block, false)?;
        self.a.bind(banked);
        self.pending = pending;
        self.block(index, block, true)?;
        self.a.bind(fiq);
        self.pending = pending;
        self.step_out(index)
    }

    fn user_bank(&mut self, registers: &[u32], load: bool) {
        self.a.ldr(true, 4, CTX, self.o.core);
        for (at, &reg) in registers.iter().enumerate() {
            if reg < 13 {
                continue;
            }
            let slot = self.o.block + 4 * at as u32;
            let bank = self.o.user_high + 4 * (reg - 8);
            if load {
                self.a.ldr(false, 3, CTX, slot);
                self.a.str(false, 3, 4, bank);
            } else {
                self.a.ldr(false, 3, 4, bank);
                self.a.str(false, 3, CTX, slot);
            }
        }
        for (at, &host) in GUEST[8..13].iter().enumerate() {
            self.a.str(false, host, 4, self.o.user_high + 4 * at as u32);
        }
        self.a.ldr(false, 5, 4, self.o.cpsr);
        self.a.ubfm(false, 5, 5, 31, 3);
        self.a.mov_imm(6, BANK_SLOTS);
        self.a.lsrv64(6, 6, 5);
        self.a.ubfm(false, 6, 6, 0, 1);
        self.a.arith(true, false, false, 6, 4, 6, Shift::Lsl, 3);
        self.a.str(false, GUEST[13], 6, self.o.banked);
        self.a.str(false, GUEST[14], 6, self.o.banked + 4);
    }

    fn block(&mut self, index: usize, block: Block, user: bool) -> Option<()> {
        let item = self.items[index];
        let pending = self.pending;
        if self.o.block + 4 * BLOCK_WORDS as u32 > 256 {
            return None;
        }
        let skip = (item.condition != ALWAYS).then(|| self.a.label());
        if let Some(skip) = skip {
            self.a.b_cond(invert(item.condition), skip);
        }
        let registers: Vec<u32> = block.registers().collect();
        let count = registers.len() as u32;
        let bytes = 4 * count;
        let base = GUEST[block.rn as usize];
        let start = match (block.up, block.pre) {
            (true, true) => 4,
            (true, false) => 0,
            (false, true) => bytes.wrapping_neg(),
            (false, false) => 4u32.wrapping_sub(bytes),
        };
        let moved = if block.up {
            bytes
        } else {
            bytes.wrapping_neg()
        };
        self.add_value(0, base, start);
        let keeps = if self.v5 {
            block.keeps_loaded_base()
        } else {
            block.list & 1 << block.rn != 0
        };
        let replaced = block.writeback && !(block.load && keeps);
        let returns = block.loads_pc();
        if returns && !self.v5 && !self.timed {
            return None;
        }
        let collects = returns && self.v5;
        let stub = if block.load {
            if replaced {
                self.add_value(base, base, moved);
            }
            if collects {
                Stub::Collect
            } else {
                Stub::Gather
            }
        } else {
            let early = !self.v5 && replaced && registers.first() != Some(&block.rn);
            if early {
                self.add_value(base, base, moved);
            }
            let values: Vec<Option<R>> = registers
                .iter()
                .map(|&reg| Some(GUEST[reg as usize]))
                .collect();
            self.buffer(false, &values);
            if user {
                self.user_bank(&registers, false);
            }
            if replaced && !early {
                self.add_value(base, base, moved);
            }
            Stub::Scatter
        };
        let packed = if collects {
            block_word(count, 0, 0, self.pending)?
        } else {
            block_word(count, item.ahead, self.charged(index), self.pending)? | self.code
        };
        self.call(stub, packed);
        self.pending = 0;
        if block.load {
            if collects {
                let (immr, imms) = logical_immediate(EXIT as u32 - 1)?;
                self.a.logical_imm(Logic::And, 1, 0, immr, imms);
            }
            let targets: Vec<Option<R>> = registers
                .iter()
                .map(|&reg| {
                    (reg != PC && !(replaced && reg == block.rn) && !(user && reg >= 13))
                        .then(|| GUEST[reg as usize])
                })
                .collect();
            self.buffer(true, &targets);
            if user {
                self.user_bank(&registers, true);
            }
        }
        if returns {
            self.a.ldr(false, 0, CTX, self.o.block + 4 * (count - 1));
            self.leave(
                if self.v5 { Stub::Return } else { Stub::Branch },
                Info {
                    at: index as u32 + 1,
                    fetch: Fetch::Core,
                    force: false,
                    branch: true,
                    last: Last::Ctx,
                    pending: 0,
                },
            )?;
            return self.skipped(index, skip, pending);
        }
        self.after_memory(index, skip, pending, true)
    }

    fn buffer(&mut self, load: bool, regs: &[Option<R>]) {
        let mut at = 0;
        while at < regs.len() {
            let offset = self.o.block + 4 * at as u32;
            match (regs[at], regs.get(at + 1).copied().flatten()) {
                (Some(first), Some(second)) => {
                    self.a.pair(load, false, first, second, CTX, offset as i32);
                    at += 2;
                }
                (Some(first), None) => {
                    if load {
                        self.a.ldr(false, first, CTX, offset);
                    } else {
                        self.a.str(false, first, CTX, offset);
                    }
                    at += 1;
                }
                (None, _) => at += 1,
            }
        }
    }

    fn after_memory(
        &mut self,
        index: usize,
        skip: Option<Label>,
        pending: u32,
        checked: bool,
    ) -> Option<()> {
        let info = Info {
            at: index as u32 + 1,
            fetch: Fetch::Known(NONSEQUENTIAL),
            force: true,
            branch: false,
            last: Last::Ctx,
            pending: 0,
        };
        let next_steps = self.items.get(index + 1).is_some_and(|next| {
            self.timed
                || matches!(
                    next.kind,
                    Kind::Step
                        | Kind::Exit
                        | Kind::Msr { spsr: false, .. }
                        | Kind::Restore { .. }
                        | Kind::UserBlock(_)
                        | Kind::Cp15 { load: None }
                )
        });
        if self.last(index) {
            self.leave(Stub::Exit, info)?;
        } else {
            if checked {
                let site = self.site(info)?;
                self.a.cbz(true, 1, site);
            }
            if !self.timed && self.items[index].check {
                self.check(info.at, 0, info)?;
            }
            if next_steps {
                self.a.mov_imm(0, NONSEQUENTIAL);
            }
        }
        if let Some(label) = skip {
            let join = self.a.label();
            self.a.bind(join);
            self.skips.push(Skip {
                label,
                join,
                index,
                item: self.items[index],
                pending,
                nf: next_steps,
            });
        }
        Some(())
    }

    fn mrs(&mut self, rd: u32) {
        let rd = GUEST[rd as usize];
        self.a.ldr(true, 1, CTX, self.o.core);
        self.a.ldr(false, rd, 1, self.o.cpsr);
        self.a.mrs_nzcv(1);
        self.a.lsr(1, 1, 28);
        self.a.bfi(rd, 1, 28, 4);
    }

    fn mrs_spsr(&mut self, rd: u32) {
        self.a.mov_imm(5, PSR_READ_SPSR);
        self.reach(Stub::Psr as u32, true);
        self.a.mov(GUEST[rd as usize], PSR_RESULT);
    }

    fn msr(&mut self, index: usize, operand: Operand, mask: u32) -> Option<()> {
        let (core, old, new, scratch, changed) = (1, 2, 3, 4, 5);
        let control = mask & !PSR_FLAGS != 0;
        let forced = mask & MODE_BIT_4;
        let slow = self.a.label();
        let pending = self.pending;
        self.a.ldr(true, core, CTX, self.o.core);
        self.a.ldr(false, old, core, self.o.cpsr);
        if control {
            self.a.ubfx(scratch, old, 0, 5);
            self.a.sub_imm(false, false, scratch, scratch, USER_MODE);
            self.a.cbz(false, scratch, slow);
        }
        self.a.mov_imm(scratch, mask);
        match operand {
            Operand::Immediate { value, .. } => self.a.mov_imm(new, (value | forced) & mask),
            Operand::Register { rm, .. } => {
                self.a.logical(
                    Logic::And,
                    false,
                    new,
                    GUEST[rm as usize],
                    scratch,
                    Shift::Lsl,
                    0,
                );
                if forced != 0 {
                    let (immr, imms) = logical_immediate(forced)?;
                    self.a.logical_imm(Logic::Orr, new, new, immr, imms);
                }
            }
            Operand::Shifted { .. } => return None,
        }
        self.a
            .logical(Logic::Bic, false, scratch, old, scratch, Shift::Lsl, 0);
        self.a
            .logical(Logic::Orr, false, new, new, scratch, Shift::Lsl, 0);
        if mask != 0 {
            self.a
                .logical(Logic::Eor, false, changed, new, old, Shift::Lsl, 0);
            if control {
                self.a.tbz(true, changed, 5, slow);
            }
            let (immr, imms) = logical_immediate(CONTROL_BITS)?;
            self.a.logical_imm(Logic::And, changed, changed, immr, imms);
            self.a
                .arith(false, true, false, scratch, ZR, changed, Shift::Lsl, 0);
            self.a
                .logical(Logic::Orr, false, scratch, scratch, changed, Shift::Lsl, 0);
            self.a.lsr(scratch, scratch, 31);
            self.a.str(false, scratch, CTX, self.o.force);
            if control {
                let same = self.a.label();
                let (immr, imms) = logical_immediate(MODE_MASK)?;
                self.a.logical_imm(Logic::And, scratch, changed, immr, imms);
                self.a.cbz(false, scratch, same);
                self.a.mov_imm(scratch, mask);
                self.a.mov_imm(changed, PSR_CPSR);
                self.reach(Stub::Psr as u32, true);
                self.a.bind(same);
            }
        }
        self.a.str(false, new, core, self.o.cpsr);
        if mask & NZCV != 0 {
            self.a.msr_nzcv(new);
        }
        self.prefetched(index, core, old);
        let last = self.spend(self.items[index].cost, true);
        self.leave(
            Stub::Exit,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Known(SEQUENTIAL),
                force: mask != 0,
                branch: true,
                last,
                pending: self.pending,
            },
        )?;
        if control {
            self.a.bind(slow);
            self.pending = pending;
            self.step_out(index)?;
        }
        Some(())
    }

    fn spsr(&mut self, index: usize, operand: Operand, mask: u32) -> Option<()> {
        let (core, old, value) = (1, 2, 3);
        self.a.ldr(true, core, CTX, self.o.core);
        self.a.ldr(false, old, core, self.o.cpsr);
        match operand {
            Operand::Immediate {
                value: constant, ..
            } => self.a.mov_imm(value, constant),
            Operand::Register { rm, .. } => self.a.mov(value, GUEST[rm as usize]),
            Operand::Shifted { .. } => return None,
        }
        self.a.mov_imm(4, mask);
        self.a.mov_imm(5, PSR_SPSR);
        self.reach(Stub::Psr as u32, true);
        self.prefetched(index, core, old);
        let last = self.spend(self.items[index].cost, true);
        self.leave(
            Stub::Exit,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Known(SEQUENTIAL),
                force: false,
                branch: true,
                last,
                pending: self.pending,
            },
        )
    }

    fn restore(&mut self, index: usize, offset: u32) -> Option<()> {
        let info = Info {
            at: index as u32 + 1,
            fetch: Fetch::Core,
            force: false,
            branch: true,
            last: Last::Ctx,
            pending: self.pending,
        };
        let Some(word) = restore_word(info.word()?, offset, self.items[index].cost) else {
            return self.step_out(index);
        };
        self.a.mov_imm(INFO, word);
        self.reach(Stub::Restore as u32, false);
        Some(())
    }

    fn cp15(&mut self, index: usize, load: Option<u32>) -> Option<()> {
        let item = self.items[index];
        let Some(rd) = load else {
            if !self.last(index) {
                return None;
            }
            let slow = self.a.label();
            let pending = self.pending;
            self.a.ldr(true, 1, CTX, self.o.attention);
            self.a.cbz_wide(false, 1, slow);
            self.a.mov_imm(2, 1);
            self.a.strb(2, 1, 0);
            self.a.ldr(true, 1, CTX, self.o.pending);
            self.a.strb(2, 1, 0);
            let last = self.spend(item.spend?, item.check);
            self.after_known(index, last, NONSEQUENTIAL)?;
            self.a.bind(slow);
            self.pending = pending;
            return self.step_out(index);
        };
        self.a.ldr(true, 1, CTX, self.o.dtcm);
        self.a.ldr(false, GUEST[rd as usize], 1, 0);
        let last = self.spend(item.spend?, item.check);
        self.after_known(index, last, NONSEQUENTIAL)
    }

    fn prefetched(&mut self, index: usize, core: R, old: R) {
        let o = self.o;
        let (first, second, at) = (4, 5, 6);
        self.a.mov_imm(first, self.pc_read(index + 1));
        self.a.str(false, first, core, o.regs + 4 * PC);
        self.a.ldr(true, at, CTX, o.words);
        self.a.add_imm(true, false, at, at, 4 * (index as u32 + 1));
        self.a.pair(true, false, first, second, at, 0);
        self.a.add_imm(true, false, at, core, o.pipeline);
        self.a.pair(false, false, first, second, at, 0);
        self.a.mov_imm(first, SEQUENTIAL);
        self.a.strb(first, core, o.next_fetch);
        self.a.ubfx(first, old, 7, 1);
        self.a.strb(first, core, o.irq_masked);
    }

    fn step(&mut self, index: usize) -> Option<()> {
        let item = self.items[index];
        self.call(
            Stub::Step,
            step_word(index as u32, item.ahead, self.pending)?,
        );
        self.pending = 0;
        let info = Info {
            at: index as u32 + 1,
            fetch: Fetch::Core,
            force: true,
            branch: false,
            last: Last::Ctx,
            pending: 0,
        };
        if self.timed {
            let forced = self.site(Info {
                branch: true,
                ..info
            })?;
            self.a.cbz(true, 6, forced);
        }
        if self.last(index) {
            return self.leave(Stub::Exit, info);
        }
        let site = self.site(info)?;
        self.a.cbz(true, 1, site);
        if !self.timed && item.check {
            self.check(info.at, 0, info)?;
        }
        self.hand_fetch(index, Fetch::Core);
        Some(())
    }

    fn step_out(&mut self, index: usize) -> Option<()> {
        self.call(Stub::Step, step_word(index as u32, 0, self.pending)?);
        self.pending = 0;
        self.leave(
            Stub::Exit,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Core,
                force: true,
                branch: true,
                last: Last::Ctx,
                pending: 0,
            },
        )
    }

    fn branch(&mut self, index: usize, word: u32) -> Option<()> {
        let offset = (((word << 8) as i32) >> 6) as u32;
        let target = self.pc_read(index).wrapping_add(offset);
        let link = (word & (1 << 24) != 0).then(|| self.link(index));
        self.leave_to(index, |s| {
            if let Some(link) = link {
                s.a.mov_imm(GUEST[14], link);
            }
            s.a.mov_imm(0, target);
            Stub::Branch
        })
    }

    fn jump(&mut self, index: usize, jump: Jump) -> Option<()> {
        let pc = self.pc_read(index);
        let link = jump.link.then(|| self.link(index));
        self.leave_to(index, |s| {
            match jump.target {
                Target::Fixed(target) => s.a.mov_imm(0, target),
                Target::Register { rm, offset, mask } => {
                    let value = s.reg(rm, pc, 0);
                    s.add_value(0, value, offset);
                    if mask != u32::MAX {
                        match logical_immediate(mask) {
                            Some((immr, imms)) => s.a.logical_imm(Logic::And, 0, 0, immr, imms),
                            None => {
                                s.a.mov_imm(1, mask);
                                s.a.logical(Logic::And, false, 0, 0, 1, Shift::Lsl, 0);
                            }
                        }
                    }
                }
            }
            if let Some(link) = link {
                s.a.mov_imm(GUEST[14], link);
            }
            if jump.exchange {
                Stub::Exchange
            } else {
                Stub::Branch
            }
        })
    }

    fn add_value(&mut self, dest: R, source: R, value: u32) {
        if value == 0 {
            if dest != source {
                self.a.mov(dest, source);
            }
        } else if Asm::fits_arith(value) {
            self.a.add_imm(false, false, dest, source, value);
        } else if Asm::fits_arith(value.wrapping_neg()) {
            self.a
                .sub_imm(false, false, dest, source, value.wrapping_neg());
        } else {
            self.a.mov_imm(1, value);
            self.a
                .arith(false, false, false, dest, source, 1, Shift::Lsl, 0);
        }
    }

    fn leave_to(&mut self, index: usize, body: impl FnOnce(&mut Self) -> Stub) -> Option<()> {
        let item = self.items[index];
        let skip = (item.condition != ALWAYS).then(|| self.a.label());
        if let Some(skip) = skip {
            self.a.b_cond(invert(item.condition), skip);
        }
        let pending = self.pending;
        let stub = body(self);
        let cell = self.cell(index);
        self.leave_cell(
            stub,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Core,
                force: false,
                branch: true,
                last: Last::Ctx,
                pending,
            },
            cell,
        )?;
        self.skipped(index, skip, pending)
    }

    fn skipped(&mut self, index: usize, skip: Option<Label>, pending: u32) -> Option<()> {
        let Some(skip) = skip else {
            return Some(());
        };
        self.a.bind(skip);
        self.pending = pending;
        let item = self.items[index];
        let last = self.spend_fetched(item.cost, item.check);
        self.after_known(index, last, SEQUENTIAL)
    }

    fn cold(&mut self) -> Option<()> {
        for slow in std::mem::take(&mut self.slows) {
            self.a.bind(slow.label);
            self.call(slow.stub, slow.word);
            self.a.b(slow.back);
        }
        for skip in std::mem::take(&mut self.skips) {
            let item = skip.item;
            self.a.bind(skip.label);
            self.pending = skip.pending;
            let last = self.spend_fetched(item.cost, true);
            let info = Info {
                at: skip.index as u32 + 1,
                fetch: Fetch::Known(SEQUENTIAL),
                force: false,
                branch: false,
                last,
                pending: 0,
            };
            if self.last(skip.index) {
                self.leave(
                    Stub::Exit,
                    Info {
                        pending: self.pending,
                        ..info
                    },
                )?;
                continue;
            }
            self.flush();
            if self.timed || item.check {
                self.check(info.at, item.ahead, info)?;
            }
            if skip.nf {
                self.a.mov_imm(0, SEQUENTIAL);
            }
            self.a.b(skip.join);
        }
        for (label, word) in std::mem::take(&mut self.sites) {
            self.a.bind(label);
            self.a.mov_imm(INFO, word);
            self.reach(Stub::Exit as u32, false);
        }
        for fetch in std::mem::take(&mut self.fetches) {
            self.a.bind(fetch.label);
            self.call(Stub::FetchArm, fetch.marker);
            self.a.b(fetch.back);
        }
        Some(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carry {
    Fixed(bool),
    Bit(u32, u32),
    Host,
}

#[cfg(test)]
mod tests {
    use super::super::super::helpers::Helpers;
    use super::super::super::scan::{Literal, Op, classify, classify_thumb};
    use super::super::{Offsets, lower};
    use super::*;

    fn build(words: &[u32], thumb: bool) -> Vec<u8> {
        build_on::<true>(words, thumb).expect("emits")
    }

    fn build_on<const V5: bool>(words: &[u32], thumb: bool) -> Option<Vec<u8>> {
        build_timed::<V5>(words, thumb, false)
    }

    fn build_timed<const V5: bool>(words: &[u32], thumb: bool, timed: bool) -> Option<Vec<u8>> {
        linked_timed::<V5>(words, thumb, timed).map(|code| code.bytes)
    }

    fn linked_on<const V5: bool>(words: &[u32], thumb: bool) -> Option<Linked> {
        linked_timed::<V5>(words, thumb, false)
    }

    fn linked_timed<const V5: bool>(words: &[u32], thumb: bool, timed: bool) -> Option<Linked> {
        linked_with::<V5>(words, thumb, timed, false)
    }

    fn linked_with<const V5: bool>(
        words: &[u32],
        thumb: bool,
        timed: bool,
        pages: bool,
    ) -> Option<Linked> {
        let size = if thumb { 2 } else { 4 };
        let ops: Vec<Op> = words
            .iter()
            .enumerate()
            .map(|(at, &word)| {
                if thumb {
                    classify_thumb::<V5>(word, 0x0200_0000 + at as u32 * size)
                } else {
                    classify::<V5>(word)
                }
            })
            .collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let mut lowered = lower(&plan, V5, V5, timed).expect("lowers");
        lowered.pages = pages;
        block(&lowered, &plan, &Offsets::of::<V5>())
    }

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[4 * at..4 * at + 4].try_into().unwrap())
    }

    #[test]
    fn timed_straight_runs_fetch_once_up_to_the_last_op() {
        let words = [
            0xE3A0_0001,
            0xE280_1001,
            0xE081_2000,
            0xE591_0004,
            0xE3A0_3002,
            0xE283_3001,
            0xE12F_FF1E,
        ];
        let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc: 0x0300_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let mut lowered = lower(&plan, false, false, true).expect("lowers");
        lowered.flat = true;
        assert_eq!(
            flat_runs(&lowered.items, &plan),
            [Some(2), None, None, None, Some(5), None, None]
        );
        let untimed = lower(&plan, false, false, false).expect("lowers");
        assert!(
            block(&lowered, &plan, &Offsets::of::<false>())
                .expect("emits")
                .bytes
                .len()
                > block(&untimed, &plan, &Offsets::of::<false>())
                    .expect("emits")
                    .bytes
                    .len()
        );
    }

    #[test]
    fn a_run_ends_on_the_first_op_that_checks_ahead() {
        let runs = |words: &[u32]| {
            let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
            let fetch = vec![[2, 1]; ops.len()];
            let mut all = words.to_vec();
            all.extend([0, 0]);
            let plan = Plan {
                pc: 0x0300_0000,
                thumb: false,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            assert!(lowered.items[1].ahead > 0);
            flat_runs(&lowered.items, &plan)
        };
        let compare = [
            0xE3A0_0001,
            0xE350_0000,
            0x0A00_0000,
            0xE3A0_3002,
            0xE12F_FF1E,
        ];
        assert_eq!(runs(&compare), [Some(1), None, None, None, None]);
        let multiply = [
            0xE3A0_0001,
            0xE003_0291,
            0x0A00_0000,
            0xE3A0_3002,
            0xE12F_FF1E,
        ];
        assert_eq!(runs(&multiply), [None; 5]);
    }

    #[test]
    fn runs_take_multiplies_whose_rs_no_earlier_op_writes() {
        let words = [
            0xE3A0_0001,
            0xE003_0291,
            0xE080_2003,
            0xE004_0291,
            0xE284_5001,
            0xE006_0195,
            0xE12F_FF1E,
        ];
        let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0xE1A0_0000, 0xE1A0_0000]);
        let plan = Plan {
            pc: 0x0300_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let mut lowered = lower(&plan, false, false, true).expect("lowers");
        assert!(matches!(lowered.items[1].kind, Kind::Multiply(_)));
        lowered.flat = true;
        assert_eq!(
            flat_runs(&lowered.items, &plan),
            [Some(2), None, None, Some(4), None, None, None]
        );
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let calls = |code: &Linked, stub: Stub| {
            code.links
                .iter()
                .filter(|link| link.helper & !LINK == stub as u32)
                .count()
        };
        let flat = block(&lowered, &plan, &offsets).expect("emits");
        assert_eq!(calls(&flat, Stub::FetchFlat), 2);
        assert_eq!(calls(&flat, Stub::Fetch), 2);
        lowered.flat = false;
        let plain = block(&lowered, &plan, &offsets).expect("emits");
        assert_eq!(calls(&plain, Stub::FetchFlat), 0);
        assert_eq!(calls(&plain, Stub::Fetch), 3);
    }

    #[test]
    fn a_block_starts_with_the_mark_and_one_entry_per_op() {
        let words = [
            0xE3A0_0001,
            0xE090_1002,
            0x1591_0004,
            0xE1B0_2183,
            0xE0000291,
            0xE92D_4010,
            0x0A00_0010,
        ];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0), MARK);
        let first = word(&bytes, 1) & 0xFFFF;
        assert_eq!(first, words.len() as u32);
        let mut previous = 0;
        for at in 0..words.len() {
            let offset = word(&bytes, at + 1) & 0xFFFF;
            assert!(offset >= first && (offset as usize) < bytes.len() / 4);
            assert!(offset >= previous);
            assert_eq!(word(&bytes, at + 1) >> 27, 0);
            previous = offset;
        }
    }

    #[test]
    fn timed_branch_runs_get_short_budget_copies() {
        let words = [0xE3A0_0001, 0x1A00_0004, 0xE280_0001, 0xE280_0002];
        let bytes = build_timed::<false>(&words, false, true).expect("emits");
        let tails = (word(&bytes, 0) >> 16) as usize;
        assert_eq!(tails, words.len());
        assert!(word(&bytes, 2) >> 27 > 0);
        for at in 0..words.len() {
            assert_eq!(word(&bytes, tails + at + 1) >> 27, 0);
        }
    }

    #[test]
    fn ds_blocks_have_no_short_budget_copy() {
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE591_0004];
        assert_eq!(word(&build(&words, false), 0), MARK);
        assert_eq!(
            word(&build_on::<false>(&words, false).expect("emits"), 0),
            MARK
        );
    }

    #[test]
    fn each_known_exit_of_a_block_marks_its_own_link_cell() {
        let mark = |cell: u32| 0xF280_0000 | 2 << 21 | (cell + 1) << 5 | INFO;
        let cells = |words: &[u32]| {
            let bytes = build(words, false);
            let tails = (word(&bytes, 0) >> 16) as usize;
            let end = (0..tails)
                .filter(|&at| word(&bytes, tails + at + 1) != word(&bytes, at + 1))
                .map(|at| (word(&bytes, tails + at + 1) & 0xFFFF) as usize)
                .min()
                .unwrap_or(bytes.len() / 4);
            let count = |cell: u32| {
                (0..end)
                    .filter(|&at| word(&bytes, at) == mark(cell))
                    .count()
            };
            (count(0), count(1), count(2))
        };
        assert_eq!(cells(&[0xE280_0001, 0xEAFF_FFFD]), (1, 0, 0));
        assert_eq!(cells(&[0x0A00_0002, 0xE280_0001, 0xEAFF_FFFD]), (1, 1, 0));
        assert_eq!(cells(&[0x0A00_0002, 0xE250_0001, 0x1AFF_FFFD]), (1, 1, 1));
        assert_eq!(cells(&[0x0A00_0002, 0x0A00_0001, 0xE12F_FF1E]), (1, 1, 0));
    }

    #[test]
    fn links_become_direct_branches_in_both_directions() {
        let mut word = 0x9400_0000u32.to_le_bytes();
        A64::link(&mut word, 0x1000, 0x1008).expect("in range");
        assert_eq!(u32::from_le_bytes(word), 0x9400_0002);
        let mut word = 0x1400_0000u32.to_le_bytes();
        A64::link(&mut word, 0x1008, 0x1000).expect("in range");
        assert_eq!(u32::from_le_bytes(word), 0x17FF_FFFE);
        let mut word = 0x9400_0000u32.to_le_bytes();
        assert_eq!(A64::link(&mut word, 0, 1 << 27), None);
    }

    #[test]
    fn timed_fetches_call_the_stub_of_the_block_mode() {
        for (words, thumb, stub) in [
            (
                vec![0xE3A0_0001, 0xE280_0001, 0xE12F_FF1E],
                false,
                Stub::FetchArm,
            ),
            (vec![0x2001, 0x3001, 0x4770], true, Stub::FetchThumb),
        ] {
            let ops: Vec<Op> = words
                .iter()
                .enumerate()
                .map(|(at, &word)| {
                    if thumb {
                        classify_thumb::<false>(word, 0x0800_0000 + 2 * at as u32)
                    } else {
                        classify::<false>(word)
                    }
                })
                .collect();
            let fetch = vec![[2, 1]; ops.len()];
            let mut all = words.clone();
            all.extend([0, 0]);
            let plan = Plan {
                pc: 0x0800_0000,
                thumb,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let code = block(&lowered, &plan, &Offsets::of::<false>()).expect("emits");
            let calls: Vec<u32> = code.links.iter().map(|link| link.helper & !LINK).collect();
            assert!(calls.contains(&(stub as u32)));
            assert!(!calls.contains(&(Stub::Fetch as u32)));
        }
    }

    fn marked_fetches(marker: u32, pc: u32, thumb: bool, words: &[u32]) -> (usize, usize) {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let mut mark = Asm::new();
        mark.mov_imm(2, marker);
        let mark = mark.finish().expect("assembles");
        let size = if thumb { 2 } else { 4 };
        let ops: Vec<Op> = words
            .iter()
            .enumerate()
            .map(|(at, &word)| {
                if thumb {
                    classify_thumb::<false>(word, pc + size * at as u32)
                } else {
                    classify::<false>(word)
                }
            })
            .collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc,
            thumb,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let lowered = lower(&plan, false, false, true).expect("lowers");
        let code = block(&lowered, &plan, &offsets).expect("emits");
        let calls = code
            .links
            .iter()
            .filter(|link| link.helper & !LINK == Stub::FetchArm as u32)
            .count();
        let marks = code
            .bytes
            .chunks(4)
            .filter(|w| *w == mark.as_slice())
            .count();
        (marks, calls)
    }

    #[test]
    fn arm_fetches_from_iwram_hand_the_stub_their_word() {
        let marked =
            |pc: u32, thumb: bool, words: &[u32]| marked_fetches(IWRAM_FETCH, pc, thumb, words);
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let (marks, calls) = marked(0x0300_0000, false, &words);
        assert!(calls > 0);
        assert_eq!(marks, calls);
        assert_eq!(marked(0x0800_0000, false, &words).0, 0);
        assert_eq!(marked(0x0000_0100, false, &words).0, 0);
        let thumb = [0x2001, 0x3001, 0x3002, 0x4770];
        assert_eq!(marked(0x0300_0000, true, &thumb).0, 0);
    }

    #[test]
    fn arm_fetches_from_bios_hand_the_stub_their_word() {
        let marked =
            |pc: u32, thumb: bool, words: &[u32]| marked_fetches(BIOS_FETCH, pc, thumb, words);
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let (marks, calls) = marked(0x0000_0100, false, &words);
        assert!(calls > 0);
        assert_eq!(marks, calls);
        for pc in [0x0300_0000, 0x0000_4000, 0x0800_0000] {
            assert_eq!(marked(pc, false, &words).0, 0);
        }
        let thumb = [0x2001, 0x3001, 0x3002, 0x4770];
        assert_eq!(marked(0x0000_0100, true, &thumb).0, 0);
    }

    #[test]
    fn iwram_and_bios_arm_fetches_hit_inline_and_call_the_stub_from_cold_code() {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let mut bus = Asm::new();
        bus.ldr(true, 4, CTX, offsets.bus);
        let bus = bus.finish().expect("assembles");
        for (pc, inline) in [
            (0x0300_0000, true),
            (0x0000_0100, true),
            (0x0800_0000, false),
        ] {
            let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
            let fetch = vec![[2, 1]; ops.len()];
            let mut all = words.to_vec();
            all.extend([0xE1A0_0000, 0xE1A0_0000]);
            let plan = Plan {
                pc,
                thumb: false,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let code = block(&lowered, &plan, &offsets).expect("emits");
            let calls: Vec<usize> = code
                .links
                .iter()
                .filter(|link| link.helper & !LINK == Stub::FetchArm as u32)
                .map(|link| link.offset as usize)
                .collect();
            assert_eq!(calls.len(), words.len());
            let loads = code
                .bytes
                .chunks(4)
                .filter(|w| *w == bus.as_slice())
                .count();
            assert_eq!(loads, if inline { words.len() } else { 0 });
            let back = |at: usize| {
                code.bytes.get(at + 4..at + 8).is_some_and(|w| {
                    let w = u32::from_le_bytes(w.try_into().unwrap());
                    w & 0xFC00_0000 == 0x1400_0000 && w & 1 << 25 != 0
                })
            };
            assert_eq!(calls.iter().all(|&at| back(at)), inline);
        }
    }

    #[test]
    fn iwram_fetches_with_idles_hand_the_stub_their_word() {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let markers: Vec<Vec<u8>> = [IWRAM_IDLES, IWRAM_IDLES | SHIFT_IDLES << IDLES_SHIFT]
            .into_iter()
            .map(|packed| {
                let mut marker = Asm::new();
                marker.mov_imm(2, packed);
                marker.finish().expect("assembles")
            })
            .collect();
        let marked = |pc: u32, thumb: bool, words: &[u32]| {
            let size = if thumb { 2 } else { 4 };
            let ops: Vec<Op> = words
                .iter()
                .enumerate()
                .map(|(at, &word)| {
                    if thumb {
                        classify_thumb::<false>(word, pc + size * at as u32)
                    } else {
                        classify::<false>(word)
                    }
                })
                .collect();
            let fetch = vec![[2, 1]; ops.len()];
            let mut all = words.to_vec();
            all.extend([0, 0]);
            let plan = Plan {
                pc,
                thumb,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let code = block(&lowered, &plan, &offsets).expect("emits");
            let calls = code
                .links
                .iter()
                .filter(|link| link.helper & !LINK == Stub::Fetch as u32)
                .count();
            let marks = code
                .bytes
                .chunks(4)
                .filter(|w| markers.iter().any(|marker| *w == marker.as_slice()))
                .count();
            (marks, calls)
        };
        let words = [
            0xE3A0_0001,
            0xE080_0110,
            0xE000_0291,
            0xE3A0_1002,
            0xE12F_FF1E,
        ];
        let (marks, calls) = marked(0x0300_0000, false, &words);
        assert!(calls > 0);
        assert_eq!(marks, calls);
        let (marks, calls) = marked(0x0800_0000, false, &words);
        assert!(calls > 0);
        assert_eq!(marks, 0);
        let thumb = [0x2001, 0x4088, 0x4348, 0x4770];
        assert_eq!(marked(0x0300_0000, true, &thumb).0, 0);
    }

    #[test]
    fn iwram_flat_runs_hand_the_stub_their_latch() {
        let words = [
            0xE3A0_0001,
            0xE280_1001,
            0xE081_2000,
            0xE591_0004,
            0xE3A0_3002,
            0xE283_3001,
            0xE12F_FF1E,
        ];
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let args = |count: u32, latch: u32| {
            let mut asm = Asm::new();
            asm.mov_imm(
                2,
                count << RUN_CYCLES_SHIFT | (count - 1) << IDLES_SHIFT | IWRAM_RUN,
            );
            asm.mov_imm(3, latch);
            asm.finish().expect("assembles")
        };
        let has = |bytes: &[u8], wanted: &[u8]| {
            bytes
                .chunks(4)
                .collect::<Vec<_>>()
                .windows(wanted.len() / 4)
                .any(|w| w.concat() == wanted)
        };
        let code = |pc: u32| {
            let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
            let fetch = vec![[2, 1]; ops.len()];
            let mut all = words.to_vec();
            all.extend([0xE1A0_0000, 0xE1A0_0000]);
            let plan = Plan {
                pc,
                thumb: false,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let mut lowered = lower(&plan, false, false, true).expect("lowers");
            lowered.flat = true;
            block(&lowered, &plan, &offsets).expect("emits").bytes
        };
        let iwram = code(0x0300_0000);
        assert!(has(&iwram, &args(3, 0xE3A0_3002)));
        assert!(has(&iwram, &args(2, 0xE1A0_0000)));
        let rom = code(0x0800_0000);
        assert!(!has(&rom, &args(3, 0xE3A0_3002)[..4]));
        assert!(!has(&rom, &args(2, 0xE1A0_0000)[..4]));
    }

    #[test]
    fn every_stub_call_is_a_link_to_a_runtime_target() {
        let code =
            linked_on::<true>(&[0xE591_0000, 0xE581_0004, 0xE1A0_F00E], false).expect("emits");
        assert!(!code.links.is_empty());
        for link in &code.links {
            let at = link.offset as usize;
            let word = u32::from_le_bytes(code.bytes[at..at + 4].try_into().unwrap());
            assert!(word == 0x9400_0000 || word == 0x1400_0000);
            assert!((link.helper & !LINK) as usize <= STUBS);
        }
    }

    #[test]
    fn paged_loads_read_the_page_table_and_call_the_stub_only_from_cold_code() {
        let words = [
            0xE591_0000,
            0xE1D1_20B4,
            0xE5D1_3006,
            0xE583_0000,
            0xEAFF_FFFE,
        ];
        let calls = |code: &Linked, stub: Stub| -> Vec<u32> {
            code.links
                .iter()
                .filter(|link| link.helper & !LINK == stub as u32)
                .map(|link| link.offset)
                .collect()
        };
        let mut a = Asm::new();
        a.ldr(true, 4, CTX, Offsets::of::<true>().pages);
        let page = a.finish().expect("assembles");
        let reads = |code: &Linked| code.bytes.windows(4).filter(|w| *w == page).count();
        let code = linked_with::<true>(&words, false, false, true).expect("emits");
        let (loads, stores) = (
            calls(&code, Stub::LoadLeave),
            calls(&code, Stub::StoreLeave),
        );
        assert_eq!((loads.len(), stores.len(), reads(&code)), (3, 1, 3));
        assert!(loads.iter().all(|&at| at > stores[0]));
        let plain = linked_with::<true>(&words, false, false, false).expect("emits");
        let loads = calls(&plain, Stub::LoadLeave);
        assert_eq!(loads.len(), 3);
        assert!(
            loads
                .iter()
                .all(|&at| at < calls(&plain, Stub::StoreLeave)[0])
        );
        assert_eq!(reads(&plain), 0);
        let v4 = linked_with::<false>(&words, false, false, true).expect("emits");
        assert_eq!(reads(&v4), 0);
    }

    #[test]
    fn only_timed_gba_branch_and_exchange_leaves_land_inline() {
        let clears =
            |offsets: &Offsets, chain: Option<usize>, timed: bool, v5: bool, pages: bool| {
                let mut a = Asm::new();
                a.str(false, ZR, 1, offsets.refetched);
                let cleared = a.finish().expect("assembles");
                let helpers = if v5 {
                    Helpers::of::<true, crate::gba::bus::Bus>()
                } else {
                    Helpers::of::<false, crate::gba::bus::Bus>()
                };
                runtime(&Setup {
                    offsets,
                    helpers: &helpers,
                    chain,
                    psr: 0,
                    specs: &[],
                    pages,
                    v5,
                    timed,
                })
                .expect("assembles")
                .bytes
                .chunks(4)
                .filter(|word| *word == cleared)
                .count()
            };
        let quick = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        for chain in [None, Some(0x1234_5678)] {
            assert_eq!(clears(&quick, chain, true, false, false), 4);
            assert_eq!(clears(&quick, chain, false, false, false), 0);
            assert_eq!(clears(&quick, chain, true, false, true), 0);
            assert_eq!(
                clears(&Offsets::of::<false>(), chain, true, false, false),
                0
            );
            let ds = Offsets::of_bus::<true, crate::gba::bus::Bus>();
            assert_eq!(clears(&ds, chain, true, true, false), 0);
        }
    }

    #[test]
    fn only_timed_gba_leaves_read_bios_targets_inline() {
        let mut a = Asm::new();
        a.ldr(
            true,
            7,
            7,
            std::mem::offset_of!(crate::jit::Fast, bios) as u32,
        );
        let read = a.finish().expect("assembles");
        let reads =
            |offsets: &Offsets, chain: Option<usize>, timed: bool, v5: bool, pages: bool| {
                let helpers = if v5 {
                    Helpers::of::<true, crate::gba::bus::Bus>()
                } else {
                    Helpers::of::<false, crate::gba::bus::Bus>()
                };
                runtime(&Setup {
                    offsets,
                    helpers: &helpers,
                    chain,
                    psr: 0,
                    specs: &[],
                    pages,
                    v5,
                    timed,
                })
                .expect("assembles")
                .bytes
                .chunks(4)
                .filter(|word| *word == read)
                .count()
            };
        let quick = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        for chain in [None, Some(0x1234_5678)] {
            assert_eq!(reads(&quick, chain, true, false, false), 2);
            assert_eq!(reads(&quick, chain, false, false, false), 0);
            assert_eq!(reads(&quick, chain, true, false, true), 0);
            assert_eq!(reads(&Offsets::of::<false>(), chain, true, false, false), 0);
            let ds = Offsets::of_bus::<true, crate::gba::bus::Bus>();
            assert_eq!(reads(&ds, chain, true, true, false), 0);
        }
    }

    #[test]
    fn untimed_branch_and_exchange_hops_keep_their_fetch_cost_and_exchange_checks_the_key() {
        use super::super::super::fast::Spec;
        const SPEC: Spec = Spec {
            len: 0x8000,
            code: true,
            cache: None,
            half: [1; 2],
            word: [1; 2],
        };
        let counts = |v5: bool, timed: bool, specs: &[Spec], offsets: &Offsets| {
            let helpers = if v5 {
                Helpers::of::<true, crate::gba::bus::Bus>()
            } else {
                Helpers::of::<false, crate::gba::bus::Bus>()
            };
            let bytes = runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: Some(0x1234_5678),
                psr: 0,
                specs,
                pages: !v5,
                v5,
                timed,
            })
            .expect("assembles")
            .bytes;
            let mut keyed = Asm::new();
            keyed.logical(Logic::Eor, false, 2, 2, 0, Shift::Lsl, 0);
            keyed.cmp_imm(2, super::super::super::lookup::NO_KEY);
            let mut kept = Asm::new();
            kept.str(false, 0, CTX, offsets.hop + 4);
            [keyed, kept].map(|a| {
                let check = a.finish().expect("assembles");
                bytes
                    .windows(check.len())
                    .step_by(4)
                    .filter(|window| *window == check)
                    .count()
            })
        };
        assert_eq!(counts(true, false, &[SPEC], &Offsets::of::<true>()), [1, 2]);
        assert_eq!(counts(false, false, &[], &Offsets::of::<false>()), [1, 2]);
        assert_eq!(counts(false, true, &[], &Offsets::of::<false>()), [0, 0]);
    }

    #[test]
    fn only_timed_gba_thumb_fetches_settle_inline() {
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let q = gba.quick.expect("the GBA bus has quick fetches");
        let set = |field: i32| {
            let mut a = Asm::new();
            if field == q.active {
                a.mov_imm(7, 0x0101);
                bus_store(&mut a, 8, 7, field);
            } else {
                bus_store(&mut a, 1, 7, field);
            }
            a.finish().expect("assembles")
        };
        let sets = |offsets: &Offsets, timed: bool| {
            let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
            let bytes = runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: None,
                psr: 0,
                specs: &[],
                pages: false,
                v5: false,
                timed,
            })
            .expect("assembles")
            .bytes;
            let words: Vec<&[u8]> = bytes.chunks(4).collect();
            [q.paused, q.active, q.force_n].map(|field| {
                let store = set(field);
                let store: Vec<&[u8]> = store.chunks(4).collect();
                words
                    .windows(store.len())
                    .filter(|w| *w == store.as_slice())
                    .count()
            })
        };
        assert_eq!(sets(&gba, true), [1, 1, 1]);
        assert_eq!(sets(&gba, false), [0, 0, 0]);
        assert_eq!(sets(&Offsets::of::<false>(), true), [0, 0, 0]);
    }

    fn bus_store(a: &mut Asm, scale: u32, rt: R, field: i32) {
        let offset = field as u32;
        let (base, offset) = if offset.is_multiple_of(scale) && offset / scale < 0x1000 {
            (4, offset)
        } else {
            a.mov_imm(17, offset);
            a.arith(true, false, false, 17, 4, 17, Shift::Lsl, 0);
            (17, 0)
        };
        match scale {
            1 => a.strb(rt, base, offset),
            4 => a.str(false, rt, base, offset),
            _ => a.str(true, rt, base, offset),
        }
    }

    fn bus_load(a: &mut Asm, rt: R, field: i32) {
        let offset = field as u32;
        if offset.is_multiple_of(4) && offset / 4 < 0x1000 {
            a.ldr(false, rt, 4, offset);
        } else {
            a.mov_imm(17, offset);
            a.arith(true, false, false, 17, 4, 17, Shift::Lsl, 0);
            a.ldr(false, rt, 17, 0);
        }
    }

    fn found_words(bytes: &[u8], build: impl FnOnce(&mut Asm)) -> usize {
        let mut a = Asm::new();
        build(&mut a);
        let code = a.finish().expect("assembles");
        let code: Vec<&[u8]> = code.chunks(4).collect();
        let words: Vec<&[u8]> = bytes.chunks(4).collect();
        words
            .windows(code.len())
            .filter(|window| *window == code.as_slice())
            .count()
    }

    #[test]
    fn gba_prefetch_restarts_and_stops_with_wide_stores_and_word_loads_skip_the_reload() {
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let q = gba.quick.expect("the GBA bus has quick fetches");
        assert!(q.packed());
        let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
        let bytes = runtime(&Setup {
            offsets: &gba,
            helpers: &helpers,
            chain: None,
            psr: 0,
            specs: &[],
            pages: false,
            v5: false,
            timed: true,
        })
        .expect("assembles")
        .bytes;
        let restart = move |one: R| {
            move |a: &mut Asm| {
                bus_store(a, 8, ZR, q.count);
                bus_store(a, 4, ZR, q.parked);
                a.mov_imm(one, 0x0101);
                bus_store(a, 8, one, q.active);
            }
        };
        assert!(found_words(&bytes, restart(5)) > 0);
        assert_eq!(found_words(&bytes, restart(7)), 1);
        let stopped = |a: &mut Asm| {
            for zero in [q.head, q.count, q.parked] {
                bus_store(a, 8, ZR, zero);
            }
            bus_store(a, 4, ZR, q.active);
            bus_store(a, 1, ZR, q.stuck);
            bus_store(a, 1, ZR, q.following);
        };
        assert!(found_words(&bytes, stopped) > 0);
        for field in [q.paused, q.force_n] {
            assert_eq!(found_words(&bytes, |a| bus_store(a, 1, ZR, field)), 0);
        }
        let reloaded = found_words(&bytes, |a| {
            bus_store(a, 4, 3, q.open_iwram);
            bus_load(a, 16, q.open_iwram);
            bus_store(a, 4, 16, q.open_last);
        });
        assert_eq!(reloaded, 0);
        let copied = found_words(&bytes, |a| {
            a.mov(16, 3);
            bus_store(a, 4, 16, q.open_last);
        });
        assert_eq!(copied, 0);
    }

    #[test]
    fn only_timed_gba_loads_read_rom_inline() {
        let found = |offsets: &Offsets, timed: bool, pages: bool| {
            let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
            let bytes = runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: None,
                psr: 0,
                specs: &[],
                pages,
                v5: false,
                timed,
            })
            .expect("assembles")
            .bytes;
            (0..bytes.len() / 4)
                .filter(|&at| word(&bytes, at) & 0xFFF8_001F == 0x37D8_0000)
                .count()
        };
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(found(&gba, true, false), 2);
        assert_eq!(found(&gba, false, false), 0);
        assert_eq!(found(&gba, true, true), 0);
        assert_eq!(found(&Offsets::of::<false>(), true, false), 0);
    }

    #[test]
    fn only_timed_gba_loads_read_rom_literals_inline() {
        let mut a = Asm::new();
        a.lsr64(0, 2, 32);
        let unpacked = word(&a.finish().expect("assembles"), 0);
        assert_eq!(unpacked, 0xD360_FC40);
        let found = |offsets: &Offsets, timed: bool, pages: bool| {
            let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
            let bytes = runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: None,
                psr: 0,
                specs: &[],
                pages,
                v5: false,
                timed,
            })
            .expect("assembles")
            .bytes;
            (0..bytes.len() / 4)
                .filter(|&at| word(&bytes, at) == unpacked)
                .count()
        };
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(found(&gba, true, false), 2);
        assert_eq!(found(&gba, false, false), 0);
        assert_eq!(found(&gba, true, true), 0);
        assert_eq!(found(&Offsets::of::<false>(), true, false), 0);
    }

    fn literal_loads(
        pc: u32,
        thumb: bool,
        words: &[u32],
        literals: &[Literal],
        offsets: &Offsets,
        timed: bool,
    ) -> usize {
        let size = if thumb { 2 } else { 4 };
        let ops: Vec<Op> = words
            .iter()
            .enumerate()
            .map(|(at, &word)| {
                if thumb {
                    classify_thumb::<false>(word, pc + size * at as u32)
                } else {
                    classify::<false>(word)
                }
            })
            .collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc,
            thumb,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals,
        };
        let lowered = lower(&plan, false, false, timed).expect("lowers");
        let bytes = block(&lowered, &plan, offsets).expect("emits").bytes;
        let mut marks = Asm::new();
        marks.movk(true, 2, 0xBEEF, 2);
        marks.movk(true, 2, 0xDEAD, 3);
        let marks = marks.finish().expect("assembles");
        let (beef, dead) = (word(&marks, 0), word(&marks, 1));
        let movz = |at: usize| {
            let w = word(&bytes, at);
            w & 0xFFE0_001F == 0xD280_0002 && (w >> 5) & 7 == (ROM_LITERAL | 2)
        };
        (0..(bytes.len() / 4).saturating_sub(1))
            .filter(|&at| word(&bytes, at) == beef && word(&bytes, at + 1) == dead)
            .filter(|&at| (1..=2).any(|back| at >= back && movz(at - back)))
            .count()
    }

    #[test]
    fn thumb_rom_literals_hand_the_load_stub_their_word() {
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let words = [0x4A01, 0x1880, 0x4A01, 0x4770];
        let baked = [Literal {
            index: 0,
            addr: 0x0800_0108,
            word: 0xDEAD_BEEF,
        }];
        let moved = [Literal {
            addr: 0x0800_010C,
            ..baked[0]
        }];
        assert_eq!(
            literal_loads(0x0800_0100, true, &words, &baked, &gba, true),
            1
        );
        assert_eq!(literal_loads(0x0800_0100, true, &words, &[], &gba, true), 0);
        assert_eq!(
            literal_loads(0x0800_0100, true, &words, &moved, &gba, true),
            0
        );
        assert_eq!(
            literal_loads(
                0x0800_0100,
                true,
                &words,
                &baked,
                &Offsets::of::<false>(),
                true
            ),
            0
        );
        assert_eq!(
            literal_loads(0x0800_0100, true, &words, &baked, &gba, false),
            0
        );
        let arm = [0xE59F_0004, 0xE12F_FF1E];
        let arm_literal = [Literal {
            addr: 0x0800_010C,
            ..baked[0]
        }];
        assert_eq!(
            literal_loads(0x0800_0100, false, &arm, &arm_literal, &gba, true),
            0
        );
    }

    #[test]
    fn only_timed_gba_block_transfers_copy_ram_inline() {
        let mut a = Asm::new();
        a.mul(1, 1, 7, ZR);
        let multiplied = a.finish().expect("assembles");
        assert_eq!(multiplied, 0x1B07_7C21u32.to_le_bytes());
        let found = |offsets: &Offsets, timed: bool, pages: bool| {
            let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
            let bytes = runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: None,
                psr: 0,
                specs: &[],
                pages,
                v5: false,
                timed,
            })
            .expect("assembles")
            .bytes;
            (0..bytes.len() / 4)
                .filter(|&at| word(&bytes, at) == 0x1B07_7C21)
                .count()
        };
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(found(&gba, true, false), 2);
        assert_eq!(found(&gba, false, false), 0);
        assert_eq!(found(&gba, true, true), 0);
        assert_eq!(found(&Offsets::of::<false>(), true, false), 0);
    }

    #[test]
    fn block_transfers_with_the_base_in_the_list_emit() {
        let words = [0xE8B0_0003, 0xE8A1_0006, 0x0891_0FFF, 0xE8BD_8010];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
    }

    #[test]
    fn user_bank_blocks_emit_only_last() {
        for words in [[0xE3A0_0001, 0xE8D0_7FFF], [0xE3A0_0001, 0xE9E0_7FFC]] {
            let bytes = build(&words, false);
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        }
        let words = [0xE8D0_7FFF, 0xE3A0_0001, 0, 0];
        let ops = [classify::<true>(words[0]), classify::<true>(words[1])];
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &words,
            ops: &ops,
            fetch: &[[2, 1]; 2],
            literals: &[],
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert!(block(&lowered, &plan, &Offsets::of::<true>()).is_none());
    }

    #[test]
    fn pc_loading_blocks_emit() {
        for words in [[0xE3A0_0001, 0x18BD_8010], [0xE3A0_0001, 0xE8B0_8002]] {
            let bytes = build(&words, false);
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        }
        let bytes = build(&[0x2001, 0xBD10], true);
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
    }

    #[test]
    fn timed_armv4_returns_gather_and_leave_through_the_branch_stub() {
        let stubs = |words: &[u32], thumb: bool, timed: bool| -> Vec<u32> {
            linked_timed::<false>(words, thumb, timed)
                .expect("emits")
                .links
                .iter()
                .map(|link| link.helper & !LINK)
                .collect()
        };
        let has = |found: &[u32], stub: Stub| found.contains(&(stub as u32));
        let pop = stubs(&[0xE8BD_8070], false, true);
        assert!(has(&pop, Stub::Gather) && has(&pop, Stub::Branch));
        for stub in [Stub::Step, Stub::Collect, Stub::Return] {
            assert!(!has(&pop, stub));
        }
        let maybe = stubs(&[0x18BD_8070], false, true);
        for stub in [Stub::Gather, Stub::Branch, Stub::Exit] {
            assert!(has(&maybe, stub));
        }
        assert!(!has(&maybe, Stub::Collect));
        let thumb = stubs(&[0xB510, 0xBD10], true, true);
        for stub in [Stub::Scatter, Stub::Gather, Stub::Branch] {
            assert!(has(&thumb, stub));
        }
        for stub in [Stub::Step, Stub::Collect, Stub::Return] {
            assert!(!has(&thumb, stub));
        }
        let user = stubs(&[0xE8FD_8010], false, true);
        assert!(has(&user, Stub::Step) && has(&user, Stub::Exit));
        assert!(!has(&user, Stub::Gather));
        let untimed = stubs(&[0xE8BD_8070], false, false);
        assert!(has(&untimed, Stub::Step));
        assert!(!has(&untimed, Stub::Gather));
    }

    #[test]
    fn cpsr_transfer_blocks_emit() {
        for words in [
            [0xE10F_0000, 0xE380_1080, 0xE121_F001],
            [0xE10F_0000, 0xE3A0_1000, 0xE328_F20F],
            [0xE10F_0000, 0xE3A0_1000, 0xE329_F01F],
            [0xE3A0_1000, 0xE49D_0004, 0xE121_F000],
        ] {
            let bytes = build(&words, false);
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        }
    }

    #[test]
    fn swaps_emit() {
        let bytes = build(&[0xE100_2091, 0xE143_A09A, 0xE100_4094], false);
        assert_eq!(word(&bytes, 0), MARK);
    }

    #[test]
    fn shifts_by_register_emit() {
        let words = [
            0xE1B0_2011,
            0xE1B0_3031,
            0xE0A6_6052,
            0xE1F0_E071,
            0xE111_0011,
            0xE051_C051,
        ];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        let bytes = build(&[0x4088, 0x40C8, 0x4108, 0x41C8], true);
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
    }

    #[test]
    fn cp15_blocks_emit() {
        for words in [
            [0xE3A0_C000, 0xE281_1001, 0xEE07_CF9A],
            [0xE3A0_0000, 0xE281_1001, 0xEE07_0F3E],
            [0xE3A0_0000, 0xE281_1001, 0xEE19_0F11],
        ] {
            let bytes = build(&words, false);
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        }
    }

    #[test]
    fn psr_writes_emit() {
        for words in [
            [0xE3A0_0000, 0xE281_1001, 0xE121_F003],
            [0xE3A0_0000, 0xE281_1001, 0xE16F_F001],
            [0xE3A0_0000, 0xE281_1001, 0xE369_F01F],
            [0xE14F_2000, 0xE281_1001, 0xE121_F002],
        ] {
            let bytes = build(&words, false);
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        }
    }

    #[test]
    fn thumb_blocks_emit() {
        let words = [0x2001, 0x1840, 0x6808, 0x4770];
        let bytes = build(&words, true);
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
    }

    #[test]
    fn armv4_blocks_emit() {
        let blocks: [&[u32]; 4] = [
            &[0xE3A0_0001, 0xE090_1002, 0x1591_0004, 0xE1B0_2183],
            &[0xE000_0291, 0xE020_3291, 0xE081_0392, 0xE0E1_0392],
            &[0xE8A1_0006, 0xE8A2_0006, 0xE8B1_0006, 0xE92D_4010],
            &[0xE3A0_0001, 0x0A00_0010],
        ];
        for words in blocks {
            let bytes = build_on::<false>(words, false).expect("emits");
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        }
        let bytes =
            build_on::<false>(&[0x2001, 0x1840, 0x6808, 0xB510, 0x4770], true).expect("emits");
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
    }

    #[test]
    fn timed_blocks_emit() {
        let blocks: [&[u32]; 4] = [
            &[0xE3A0_0001, 0xE090_1002, 0x1591_0004, 0xE1B0_2183],
            &[0xE000_0291, 0xE020_3291, 0xE081_0392, 0xE0E1_0392],
            &[0xE8A1_0006, 0xE8A2_0006, 0xE8B1_0006, 0xE92D_4010],
            &[0xE3A0_0001, 0x0A00_0010],
        ];
        for words in blocks {
            let bytes = build_timed::<false>(words, false, true).expect("emits");
            assert_eq!(word(&bytes, 0), MARK);
            assert_ne!(bytes, build_on::<false>(words, false).expect("emits"));
        }
        let bytes = build_timed::<false>(&[0x2001, 0x1840, 0x6808, 0xB510, 0x4770], true, true)
            .expect("emits");
        assert_eq!(word(&bytes, 0), MARK);
    }
}
