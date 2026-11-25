mod asm;
mod runtime;

use runtime::{
    CODE_OVERLAP, CYCLES_MASK, IMAGE_MASK, IMAGE_TO_NZCV, NZCV_MASK, OVERLAP, PC_SLOT, SPSR_READ,
    SPSR_WRITE, runtime,
};

use super::super::QuickFetch;
use super::super::fast::{
    CODE_CYCLES, Fast, HALF_CYCLES, LONG_HALF, LONG_WORD, MAIN, PAGE_SHIFT, PAGE_SIZE, Page,
    SEQUENTIAL_CYCLES, WORD_CYCLES,
};
use super::super::helpers::{
    IDLES_SHIFT, RUN_COUNT_MASK, RUN_CYCLES_SHIFT, RUN_FIRST_SHIFT, THUMB,
};
use super::super::lookup::LINKS;
use super::super::marks::PAGE_BITS;
use super::super::scan::{Block, Jump, Multiply, Offset, Plan, Target, Transfer, Width};
use super::{
    ALL_FLAGS, ALWAYS, Alu, BIOS_FETCH, Backend, CODE_CLASS, FLAG_C, FLAG_N, FLAG_V, FLAG_Z, Fetch,
    IWRAM_FETCH, IWRAM_IDLES, IWRAM_RUN, Info, Item, Kind, Last, Linked, Lowered, MARK, MAX_COST,
    Offsets, Operand, PENDING_LIMIT, ROM_LITERAL, Runtime, SHIFT_IDLES, STUBS, Setup, Stub, Swap,
    block_word, entry_word, link_owners, load_pc_word, memory_word, restore_word, step_word,
};
use crate::gba::cpu::psr::MODE_MASK;
use crate::jit_cache::{LINK, Reloc};
use asm::{
    Alu as Op2, Asm, Label, Mem, R, R8, R9, R10, R11, R12, R13, R14, R15, RAX, RBP, RBX, RCX, RDI,
    RDX, RSI, RSP, Rot, at, cc, indexed,
};
use std::mem::{offset_of, size_of};

const PINNED: [Option<R>; 15] = [
    Some(RBX),
    Some(RBP),
    Some(RDI),
    Some(R8),
    Some(R12),
    Some(R13),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some(R14),
    Some(R15),
];
const _: () = assert!(PINNED[13].is_some() && PINNED[14].is_some());
const S: R = R9;
const DEBT: R = R10;
const REGS: R = R11;
const OLD: R = RDX;
const INFO: R = RSI;
const WORD: R = RSI;
const ADDRESS: R = RCX;
const VALUE: R = RDX;
const TARGET: R = RCX;
const FETCH: R = RCX;
const COUNTED: R = RSI;
const FETCH_PC: R = RDX;
const LEAD_SCALE: i32 = (1u32 << RUN_CYCLES_SHIFT | 1 << RUN_FIRST_SHIFT) as i32;
const KEEP: u32 = 0xFF;
pub(super) const SWAP_ENTRY: usize = 5;
const PC: u32 = 15;
const LR: u32 = 14;
const NONSEQUENTIAL: u32 = 0;
const SEQUENTIAL: u32 = 1;
const INVERTED_ENTRY: u32 = 1 << 15;
const CODE_LIMIT: usize = 0x7FFF;
const AT_SHIFT: u32 = 26;
const AT_SHIFT_V4: u32 = 28;
const _: () = assert!(AT_SHIFT_V4 >= CODE_CLASS + 2);
const _: () = assert!(size_of::<Page>().is_power_of_two());
const IMAGE_CARRY: u32 = 8;
const FIQ_MODE: u32 = 0x11;
const BANKED_MODES: u32 = 1 << 0x12 | 1 << 0x13 | 1 << 0x17 | 1 << 0x1B;
const BANK_SLOTS: u32 = 2 << 4 | 1 << 14 | 3 << 22;

const FRAME_CELL: i32 = 8;
const FRAME_KEY: i32 = FRAME_CELL + 4;
const FRAME_CORE: i32 = 16;
const FRAME_BUS: i32 = FRAME_CORE + 8;
const FRAME_WORDS: i32 = FRAME_CORE + 16;
const FRAME_DEBT: i32 = FRAME_CORE + 24;
const FRAME_BUDGET: i32 = FRAME_CORE + 32;
const FRAME_START: i32 = FRAME_CORE + 36;
const FRAME_BASE: i32 = FRAME_CORE + 40;
const FRAME_SHIFT: i32 = FRAME_CORE + 44;
const FRAME_LAST: i32 = FRAME_CORE + 48;
const FRAME_FORCE: i32 = FRAME_CORE + 52;
const FRAME_CTX: i32 = FRAME_CORE + 56;
const _: () = assert!(FRAME_KEY + 4 <= FRAME_CORE);
const FRAME_FAST: i32 = FRAME_CORE + 64;
const FRAME: i32 = (FRAME_FAST + 8 + 8 + 15) / 16 * 16 - 8;
const HOP_INVERTED: u32 = 59;
const HOP_TARGET: u64 = (1 << 48) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flags {
    host: Option<bool>,
    image: bool,
}

impl Flags {
    const ENTRY: Self = Self {
        host: None,
        image: true,
    };
}

#[derive(Clone, Copy)]
enum Val {
    Reg(R),
    Mem(Mem),
    Imm(u32),
}

struct Skip {
    label: Label,
    join: Label,
    index: usize,
    item: Item,
    pending: u32,
    flags: Flags,
    nf: bool,
}

struct Site {
    label: Label,
    word: u32,
}

struct ColdFetch {
    label: Label,
    back: Label,
    marker: u32,
    args: u64,
}

struct Slow {
    label: Label,
    back: Label,
    stub: Stub,
    word: u32,
    rd: Option<u32>,
    exit: Option<Label>,
    block: Option<SlowBlock>,
}

#[derive(Clone, Copy)]
struct SlowBlock {
    lists: u32,
    spill: u32,
    reload: u32,
    base: Val,
    moved: u32,
    replaced: bool,
}

#[derive(Clone, Copy)]
enum Out {
    Site,
    Shared,
    Stub,
}

struct Lower<'a> {
    a: Asm,
    plan: &'a Plan<'a>,
    items: &'a [Item],
    pending: u32,
    flags: Flags,
    heads: Vec<Label>,
    entries: Vec<(u32, bool)>,
    sites: Vec<Site>,
    skips: Vec<Skip>,
    slows: Vec<Slow>,
    fetches: Vec<ColdFetch>,
    shared: Option<Label>,
    owners: [Option<usize>; LINKS],
    regs: i32,
    cpsr: i32,
    user_high: i32,
    banked: i32,
    dtcm: i32,
    v5: bool,
    pages: bool,
    code: u32,
    timed: bool,
    quick: Option<QuickFetch>,
    runs: Vec<Option<usize>>,
    copied: Vec<Option<(Label, (u32, bool))>>,
    links: Vec<Reloc>,
}

pub(super) fn at_shift(v5: bool) -> u32 {
    if v5 { AT_SHIFT } else { AT_SHIFT_V4 }
}

pub struct X64;

impl Backend for X64 {
    const TIMED: bool = true;
    const PSR: bool = true;

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
        let delta = i32::try_from(to as i64 - (from as i64 + 4)).ok()?;
        word.copy_from_slice(&delta.to_le_bytes());
        Some(())
    }

    fn hop(table: usize, entry: u32) -> u64 {
        let target = (table + (entry as usize & CODE_LIMIT)) as u64;
        let inverted = u64::from(entry & INVERTED_ENTRY != 0) << HOP_INVERTED;
        target | inverted | u64::from((entry >> 16) & 0x7FF) << 48
    }

    fn inverted(entry: u32) -> u32 {
        u32::from(entry & INVERTED_ENTRY != 0) << 8
    }
}

pub fn block(lowered: &Lowered, plan: &Plan, offsets: &Offsets) -> Option<Linked> {
    if lowered.items.iter().any(|item| item.condition > ALWAYS) {
        return None;
    }
    let copy = lowered.items.iter().any(|item| item.entry > 0);
    copy.then(|| assemble(lowered, plan, offsets, true))
        .flatten()
        .or_else(|| assemble(lowered, plan, offsets, false))
}

fn assemble(lowered: &Lowered, plan: &Plan, offsets: &Offsets, copy: bool) -> Option<Linked> {
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
    let first = pass(lowered, plan, offsets, &tail, Vec::new())?;
    let shorts = first.a.shorts()?;
    let second = pass(lowered, plan, offsets, &tail, shorts)?;
    let count = lowered.items.len();
    let Lower {
        a,
        heads,
        entries,
        copied,
        links,
        ..
    } = second;
    let entry = |head: Label, (pending, inverted): (u32, bool), ahead: u32| {
        let offset = a.bound(head)?;
        let offset = (offset - 4) as u32 | if inverted { INVERTED_ENTRY } else { 0 };
        entry_word(offset, pending, ahead)
    };
    let mut words = Vec::with_capacity(count + tail.len());
    for (index, &head) in heads.iter().enumerate() {
        words.push(entry(head, entries[index], lowered.items[index].entry)?);
    }
    for (index, copy) in copied.into_iter().enumerate() {
        let word = match copy {
            Some((head, at)) => entry(head, at, 0)?,
            None => words[index],
        };
        words.push(word);
    }
    debug_assert_eq!(heads.len(), count);
    let mut bytes = a.finish()?;
    if bytes.len() > CODE_LIMIT {
        return None;
    }
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

fn run_fixed(multiply: Multiply) -> u32 {
    1 + u32::from(multiply.accumulate.is_some())
}

fn run_writes(item: Item) -> u32 {
    match item.kind {
        Kind::Alu(alu) if alu.writes_rd() => 1 << alu.rd,
        Kind::Constant { rd, .. } => 1 << rd,
        Kind::Multiply(multiply) => 1 << multiply.rd,
        _ => 0,
    }
}

fn owners(items: &[Item], plan: &Plan) -> [Option<usize>; LINKS] {
    let mut owners = link_owners(items, plan);
    let Some(last) = items.len().checked_sub(1) else {
        return owners;
    };
    if paired(items, last).is_none() && !checked(items[last]) {
        return owners;
    }
    let cell = owners.iter().position(Option::is_none).or_else(|| {
        owners
            .iter()
            .rposition(|owner| owner.is_some_and(|at| at < last))
    });
    if let Some(cell) = cell {
        owners[cell] = Some(last);
    }
    owners
}

fn checked(item: Item) -> bool {
    match item.kind {
        Kind::Jump(Jump {
            exchange: true,
            target: Target::Register { .. },
            ..
        }) => true,
        Kind::Block(block) => block.loads_pc(),
        _ => false,
    }
}

fn paired(items: &[Item], index: usize) -> Option<u32> {
    let item = items[index];
    let Kind::Jump(Jump {
        target:
            Target::Register {
                rm: LR,
                offset,
                mask,
            },
        ..
    }) = item.kind
    else {
        return None;
    };
    let before = items[index.checked_sub(1)?];
    let Kind::Constant { rd: LR, value } = before.kind else {
        return None;
    };
    (item.condition == ALWAYS && before.condition == ALWAYS)
        .then_some(value.wrapping_add(offset) & mask)
}

fn pass<'a>(
    lowered: &'a Lowered,
    plan: &'a Plan<'a>,
    offsets: &Offsets,
    tail: &'a [Item],
    shorts: Vec<bool>,
) -> Option<Lower<'a>> {
    let count = lowered.items.len();
    let from_regs = |offset: u32| offset as i32 - offsets.regs as i32;
    let mut lower = Lower {
        a: Asm::new(shorts),
        plan,
        items: &lowered.items,
        pending: 0,
        flags: Flags::ENTRY,
        heads: Vec::with_capacity(count),
        entries: Vec::with_capacity(count),
        sites: Vec::new(),
        skips: Vec::new(),
        slows: Vec::new(),
        fetches: Vec::new(),
        shared: None,
        owners: owners(&lowered.items, plan),
        regs: offsets.regs as i32,
        cpsr: from_regs(offsets.cpsr),
        user_high: from_regs(offsets.user_high),
        banked: from_regs(offsets.banked),
        dtcm: offsets.dtcm as i32,
        v5: lowered.v5,
        pages: lowered.pages,
        code: if lowered.v5 { 0 } else { lowered.code },
        timed: lowered.timed,
        quick: offsets.quick,
        runs: if lowered.timed && lowered.flat {
            flat_runs(&lowered.items, plan)
        } else {
            Vec::new()
        },
        copied: Vec::new(),
        links: Vec::new(),
    };
    lower.a.word(MARK | (tail.len() as u32) << 16);
    for _ in 0..count + tail.len() {
        lower.a.word(0);
    }
    for _ in 0..count {
        let head = lower.a.label();
        lower.heads.push(head);
    }
    if (0..count).filter(|&index| lower.shares(index)).count() > 1 {
        lower.shared = Some(lower.a.label());
    }
    for index in 0..count {
        lower.op(index)?;
    }
    if !tail.is_empty() {
        lower.copies(tail, &lowered.items)?;
    }
    lower.cold()?;
    Some(lower)
}

fn stepped(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Step
            | Kind::Exit
            | Kind::Msr { .. }
            | Kind::UserBlock(_)
            | Kind::Cp15 { load: None }
            | Kind::Restore { .. }
    )
}

fn left(at: u32) -> Info {
    Info {
        at,
        fetch: Fetch::Known(NONSEQUENTIAL),
        force: true,
        branch: false,
        last: Last::Ctx,
        pending: 0,
    }
}

fn psr_word(at: u32, fields: u32, kind: u32, cost: u32, pending: u32) -> Option<u32> {
    (at <= 0xFF && cost <= MAX_COST && pending <= 0x3FF)
        .then_some(at | fields << 8 | kind | cost << 14 | pending << 22)
}

fn condition_code(condition: u32, inverted: bool) -> u8 {
    match (condition, inverted) {
        (0x0, _) => cc::E,
        (0x1, _) => cc::NE,
        (0x2, false) | (0x3, true) => cc::B,
        (0x2, true) | (0x3, false) => cc::AE,
        (0x4, _) => cc::S,
        (0x5, _) => cc::NS,
        (0x6, _) => cc::O,
        (0x7, _) => cc::NO,
        (0x8, _) => cc::A,
        (0x9, _) => cc::BE,
        (0xA, _) => cc::GE,
        (0xB, _) => cc::L,
        (0xC, _) => cc::G,
        _ => cc::LE,
    }
}

impl<'a> Lower<'a> {
    fn copies(&mut self, items: &'a [Item], runs: &[Item]) -> Option<()> {
        if !self.timed {
            return None;
        }
        let heads = std::mem::take(&mut self.heads);
        let entries = std::mem::take(&mut self.entries);
        self.items = items;
        self.runs.clear();
        self.heads = (0..items.len()).map(|_| self.a.label()).collect();
        let mut copied = vec![None; items.len()];
        let mut index = 0;
        while index < items.len() {
            if runs[index].entry == 0 {
                index += 1;
                continue;
            }
            let from = index;
            self.pending = 0;
            self.flags = Flags::ENTRY;
            self.entries.clear();
            while runs.get(index)?.entry > 0 {
                self.op(index)?;
                index += 1;
            }
            let (pending, _) = *entries.get(index)?;
            if self.pending != pending {
                let moved = self.pending as i32 - pending as i32;
                self.a.lea(false, S, at(S, -moved));
            }
            self.pending = 0;
            self.head_flags()?;
            self.a.jmp(*heads.get(index)?);
            for (at, entry) in (from..index).zip(self.entries.drain(..)) {
                copied[at] = Some((self.heads[at], entry));
            }
        }
        self.copied = copied;
        self.heads = heads;
        self.entries = entries;
        Some(())
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

    fn tag(&self, index: usize) -> u32 {
        let at = index as u32 + 1;
        let shift = at_shift(self.v5);
        if at <= u32::MAX >> shift {
            at << shift
        } else {
            0
        }
    }

    fn shares(&self, index: usize) -> bool {
        matches!(self.items[index].kind, Kind::Transfer(transfer) if transfer.load())
            && self.tag(index) != 0
            && !self.last(index)
    }

    fn live(&self, index: usize) -> u8 {
        self.items[index].live
    }

    fn val(&self, guest: u32, pc: u32) -> Val {
        if guest == PC {
            return Val::Imm(pc);
        }
        match PINNED[guest as usize] {
            Some(reg) => Val::Reg(reg),
            None => Val::Mem(at(REGS, 4 * guest as i32)),
        }
    }

    fn home(&self, guest: u32) -> Option<R> {
        PINNED.get(guest as usize).copied().flatten()
    }

    fn put(&mut self, dest: R, value: Val) {
        match value {
            Val::Reg(reg) if reg == dest => {}
            Val::Reg(reg) => self.a.mov(false, dest, reg),
            Val::Mem(mem) => self.a.load(false, dest, mem),
            Val::Imm(value) => self.a.mov_imm(dest, value),
        }
    }

    fn write(&mut self, guest: u32, source: R) {
        match PINNED[guest as usize] {
            Some(reg) if reg == source => {}
            Some(reg) => self.a.mov(false, reg, source),
            None => self.a.store(false, at(REGS, 4 * guest as i32), source),
        }
    }

    fn apply(&mut self, op: Op2, dest: R, value: Val) {
        match value {
            Val::Reg(reg) => self.a.alu(op, false, dest, reg),
            Val::Mem(mem) => self.a.alu_load(op, dest, mem),
            Val::Imm(value) => self.a.alu_imm(op, false, dest, value as i32),
        }
    }

    fn save(&mut self) {
        if self.flags.image {
            return;
        }
        if let Some(inverted) = self.flags.host {
            if inverted {
                self.a.cmc();
                self.flags.host = Some(false);
            }
            self.a.lahf();
            self.a.setcc(cc::O, RAX);
            self.flags.image = true;
        }
    }

    fn restore(&mut self) {
        if self.flags.host.is_none() && self.flags.image {
            self.a.al_imm(Op2::Cmp, 0x81);
            self.a.sahf();
            self.flags.host = Some(false);
        }
    }

    fn keep(&mut self, live: u8) {
        if live != 0 {
            self.save();
        }
    }

    fn clobber(&mut self) {
        self.flags.host = None;
    }

    fn set(&mut self, inverted: bool) {
        self.flags = Flags {
            host: Some(inverted),
            image: false,
        };
    }

    fn canonical(&mut self) {
        self.save();
    }

    fn skip_unless(&mut self, condition: u32, skip: Label) {
        self.restore();
        let mut inverted = self.flags.host.unwrap_or(false);
        if matches!(condition, 0x8 | 0x9) && !inverted {
            self.a.cmc();
            inverted = true;
            self.flags.host = Some(true);
        }
        self.a.jcc(condition_code(condition, inverted) ^ 1, skip);
    }

    fn join(&mut self, before: Flags, live: u8) {
        if live == 0 {
            self.flags = Flags {
                host: None,
                image: false,
            };
            return;
        }
        let Some(wanted) = before.host else {
            self.flags.image &= before.image;
            return;
        };
        match self.flags.host {
            Some(now) if now == wanted => {}
            Some(_) => self.a.cmc(),
            None => {
                self.restore();
                if wanted {
                    self.a.cmc();
                }
            }
        }
        self.flags = Flags {
            host: Some(wanted),
            image: before.image && self.flags.image,
        };
    }

    fn site(&mut self, info: Info) -> Option<Label> {
        let label = self.a.label();
        self.sites.push(Site {
            label,
            word: info.word()?,
        });
        Some(label)
    }

    fn leave(&mut self, stub: Stub, info: Info) -> Option<()> {
        let through = stub == Stub::Exit && !info.branch && info.at as usize == self.items.len();
        let cell = self.cell(self.items.len()).filter(|_| through);
        self.leave_cell(stub, info, cell)
    }

    fn leave_cell(&mut self, stub: Stub, info: Info, cell: Option<usize>) -> Option<()> {
        self.canonical();
        if let Some(cell) = cell {
            self.a.store8_imm(at(RSP, FRAME_CELL), cell as u8 + 1);
        }
        self.a.mov_imm(INFO, info.word()?);
        self.reach(stub as u32, false);
        Some(())
    }

    fn reach(&mut self, target: u32, call: bool) {
        self.a.byte(if call { 0xE8 } else { 0xE9 });
        self.links.push(Reloc {
            offset: self.a.here() as u32,
            helper: LINK | target,
        });
        self.a.word(LINK | target);
    }

    fn call(&mut self, stub: Stub, packed: u32) {
        self.a.mov_imm(WORD, packed);
        self.reach(stub as u32, true);
    }

    fn flush(&mut self) {
        if self.pending > 0 {
            self.a.lea(false, S, at(S, -(self.pending as i32)));
            self.pending = 0;
        }
    }

    fn spend(&mut self, cost: u32, keep: bool, live: u8) -> Last {
        if !self.v5 {
            return self.spend_v4(cost, keep);
        }
        let last = match cost {
            0 => {
                self.a.lea(false, DEBT, at(DEBT, 1));
                Last::One
            }
            1 => Last::One,
            _ => {
                self.keep(live);
                if keep {
                    self.a.mov(false, OLD, DEBT);
                }
                self.a.alu(Op2::Xor, false, RCX, RCX);
                self.a.alu_imm(Op2::Sub, false, DEBT, (cost - 1) as i32);
                self.a.cmov(cc::S, DEBT, RCX);
                self.clobber();
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
                    self.a.mov_imm(OLD, 0);
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

    fn spend_counted(&mut self, cycles: R) -> Last {
        self.a.alu(Op2::Sub, false, S, cycles);
        self.a.store(false, at(RSP, FRAME_LAST), cycles);
        Last::Ctx
    }

    fn charged(&self, index: usize) -> u32 {
        if self.v5 {
            self.items[index].cost
        } else {
            index as u32
        }
    }

    fn check(&mut self, at_index: u32, ahead: u32, info: Info) -> Option<()> {
        self.save();
        self.a
            .lea(false, RCX, at(S, -((self.pending + ahead + 1) as i32)));
        self.a.alu(Op2::Cmp, false, RCX, DEBT);
        self.clobber();
        let site = self.site(Info {
            at: at_index,
            ..info
        })?;
        self.a.jcc(cc::L, site);
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
        if self.timed || stepped(next.kind) {
            let value = match fetch {
                Fetch::Known(access) => access,
                Fetch::Core => KEEP,
            };
            self.a.mov_imm(FETCH, value);
        }
    }

    fn head_flags(&mut self) -> Option<()> {
        self.save();
        if !self.flags.image {
            return None;
        }
        self.clobber();
        Some(())
    }

    fn call_fetch(&mut self, stub: Stub) {
        self.save();
        self.reach(stub as u32, true);
        self.clobber();
    }

    fn fetch(&mut self, index: usize) {
        if let Some((marker, latch)) = self.quick_word(index) {
            self.fetch_word(index, marker, latch);
            return;
        }
        let stub = if self.plan.thumb {
            Stub::FetchThumb
        } else {
            Stub::FetchArm
        };
        let latch = self.quick_args(index);
        self.fetch_pc(index, latch);
        self.call_fetch(stub);
    }

    fn fetch_pc(&mut self, index: usize, latch: u32) {
        let pc = self.pc_read(index);
        self.a
            .mov_imm64(FETCH_PC, u64::from(pc) | u64::from(latch) << 32);
    }

    fn quick_args(&mut self, index: usize) -> u32 {
        let Some(quick) = self.quick else {
            return 0;
        };
        let addr = self.pc_read(index);
        let word = self.plan.words.get(index + 2).copied();
        let (marker, latch) = match (quick.rom_offset(addr), word) {
            (Some(from), Some(word)) => {
                let last = if self.plan.thumb {
                    (word & 0xFFFF) * 0x0001_0001
                } else {
                    word
                };
                (from as u32, last)
            }
            (None, Some(word)) if !self.plan.thumb && addr >> 24 == quick.iwram_page => {
                (IWRAM_FETCH, word)
            }
            (None, Some(word)) if !self.plan.thumb && addr < quick.bios_end => (BIOS_FETCH, word),
            _ => (u32::MAX, 0),
        };
        self.a.mov_imm(WORD, marker);
        latch
    }

    fn quick_word(&self, index: usize) -> Option<(u32, u32)> {
        let quick = self.quick?;
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
        let Some(q) = self.quick else {
            return;
        };
        let (label, back) = (self.a.label(), self.a.label());
        let args = u64::from(self.pc_read(index)) | u64::from(latch) << 32;
        let open = if marker == IWRAM_FETCH {
            q.open_iwram
        } else {
            q.open_bios
        };
        let bit = q.prefetch_bit.trailing_zeros();
        let field = |offset: i32| at(RDX, offset);
        self.save();
        self.a.load(true, RDX, at(RSP, FRAME_BUS));
        self.a.load(false, RSI, field(q.now));
        self.a.alu_load(Op2::Cmp, RSI, field(q.dma_check));
        self.a.jcc(cc::AE, label);
        self.a
            .test_mem8(field(q.wait_bits + (bit / 8) as i32), 1 << (bit % 8));
        self.a.jcc(cc::E, label);
        self.a.alu_mem_imm_wide(Op2::Add, field(q.clock), 1);
        self.a.store8_imm(field(q.branched), 0);
        self.a.inc(RSI);
        self.a.store(false, field(q.sample), RSI);
        self.a.store(false, field(q.now), RSI);
        self.a.mov_imm64(RSI, args);
        self.a.store(true, field(q.open_addr), RSI);
        self.a.store_imm(field(open), latch);
        self.a.alu_imm(Op2::Sub, false, S, 1);
        self.a.store_imm(at(RSP, FRAME_LAST), 1);
        self.a.bind(back);
        self.clobber();
        self.fetches.push(ColdFetch {
            label,
            back,
            marker,
            args,
        });
    }

    fn iwram_word(&self, index: usize) -> Option<u32> {
        let quick = self.quick?;
        if self.plan.thumb || self.pc_read(index) >> 24 != quick.iwram_page {
            return None;
        }
        self.plan.words.get(index + 2).copied()
    }

    fn fetch_idles(&mut self, index: usize, idles: u32) {
        let thumb = if self.plan.thumb { THUMB } else { 0 };
        let packed = thumb | idles << IDLES_SHIFT;
        let latch = match self.iwram_word(index) {
            Some(word) => {
                self.a.mov_imm(WORD, packed | IWRAM_IDLES);
                word
            }
            None => {
                self.a.mov_imm(WORD, packed);
                0
            }
        };
        self.fetch_pc(index, latch);
        self.call_fetch(Stub::Fetch);
    }

    fn fetch_counted(&mut self, index: usize) {
        self.a.rot(Rot::Shl, false, COUNTED, IDLES_SHIFT as u8);
        if self.plan.thumb {
            self.a.alu_imm(Op2::Or, false, COUNTED, THUMB as i32);
        }
        let latch = match self.iwram_word(index) {
            Some(word) => {
                self.a.alu_imm(Op2::Or, false, COUNTED, IWRAM_IDLES as i32);
                word
            }
            None => 0,
        };
        self.fetch_pc(index, latch);
        self.call_fetch(Stub::Fetch);
    }

    fn flat_run(&mut self, first: usize, end: usize) -> Option<()> {
        let slow = self.a.label();
        let thumb = if self.plan.thumb { THUMB } else { 0 };
        let count = (end - first + 1) as u32;
        let latch = self.flat_latch(first, end);
        let flag = if latch.is_some() { IWRAM_RUN } else { 0 };
        let reserve = self.pending + self.items[end].ahead;
        let lead = match self.items[first].kind {
            Kind::Multiply(multiply) => Some(multiply),
            _ => None,
        };
        let rest: Vec<Multiply> = (first + 1..=end)
            .filter_map(|index| match self.items[index].kind {
                Kind::Multiply(multiply) => Some(multiply),
                _ => None,
            })
            .collect();
        let fixed = lead.iter().chain(&rest).map(|&m| run_fixed(m)).sum::<u32>();
        let packed = thumb
            | flag
            | lead.map_or(0, run_fixed) << RUN_FIRST_SHIFT
            | (count - 1) << IDLES_SHIFT
            | (count + fixed) << RUN_CYCLES_SHIFT;
        self.save();
        if lead.is_none() && rest.is_empty() {
            self.a.mov_imm(WORD, packed);
        } else {
            for (nth, &multiply) in rest.iter().enumerate() {
                let dest = if nth == 0 { WORD } else { RDX };
                self.run_term(dest, multiply);
                if nth > 0 {
                    self.a.alu(Op2::Add, false, WORD, RDX);
                }
            }
            if !rest.is_empty() {
                self.a.rot(Rot::Shl, false, WORD, RUN_CYCLES_SHIFT as u8);
            }
            if let Some(multiply) = lead {
                let dest = if rest.is_empty() { WORD } else { RDX };
                self.run_term(dest, multiply);
                self.a.imul_imm(dest, dest, LEAD_SCALE);
                if dest != WORD {
                    self.a.alu(Op2::Add, false, WORD, RDX);
                }
            }
            self.a.alu_imm(Op2::Add, false, WORD, packed as i32);
        }
        match latch {
            Some(latch) => self.fetch_pc(end, latch),
            None => self.a.mov_imm(FETCH_PC, self.pc_read(first)),
        }
        if reserve > 0 {
            self.a.alu_imm(Op2::Sub, false, S, reserve as i32);
        }
        self.clobber();
        self.call_fetch(Stub::FetchFlat);
        if reserve > 0 {
            self.a.lea(false, S, at(S, reserve as i32));
        }
        let entry = self.flags;
        self.a.jcc(cc::E, slow);
        for index in first..=end {
            match self.items[index].kind {
                Kind::Alu(alu) => self.alu(index, alu),
                Kind::Constant { rd, value } => self.set_guest(rd, value),
                Kind::Multiply(multiply) => {
                    self.save();
                    self.multiply(multiply);
                    self.clobber();
                }
                _ => return None,
            }
        }
        self.hand_fetch(end, Fetch::Known(SEQUENTIAL));
        self.head_flags()?;
        self.a.jmp(*self.heads.get(end + 1)?);
        self.a.bind(slow);
        self.flags = entry;
        Some(())
    }

    fn run_term(&mut self, dest: R, multiply: Multiply) {
        let rs = self.val(multiply.rs, 0);
        self.put(dest, rs);
        self.a.rot(Rot::Sar, false, dest, 31);
        self.apply(Op2::Xor, dest, rs);
        self.a.alu_imm(Op2::Or, false, dest, 1);
        self.a.bsr(dest, dest);
        self.a.rot(Rot::Shr, false, dest, 3);
    }

    fn flat_latch(&self, first: usize, end: usize) -> Option<u32> {
        let words = [
            *self.plan.words.get(end + 1)?,
            *self.plan.words.get(end + 2)?,
        ];
        self.quick?.iwram_latch(
            self.pc_read(first),
            self.pc_read(end),
            self.plan.thumb,
            words,
        )
    }

    fn spend_fetched(&mut self, cost: u32, keep: bool, live: u8) -> Last {
        if self.timed {
            Last::Ctx
        } else {
            self.spend(cost, keep, live)
        }
    }

    fn op(&mut self, index: usize) -> Option<()> {
        if self.timed {
            self.head_flags()?;
        }
        let head = self.heads[index];
        self.a.bind(head);
        self.entries
            .push((self.pending, self.flags.host == Some(true)));
        let run = self.runs.get(index).copied().flatten();
        if let Some(end) = run {
            self.flat_run(index, end)?;
        }
        let item = self.items[index];
        match item.kind {
            _ if !self.timed || run.is_some() => {}
            Kind::Step | Kind::Exit | Kind::Multiply(_) => {}
            Kind::Alu(alu) if alu.shifted() => self.fetch_idles(index, SHIFT_IDLES),
            _ => self.fetch(index),
        }
        match item.kind {
            Kind::Restore { offset } => self.exception_return(index, offset),
            Kind::Swap(swap) => self.swap(index, swap),
            Kind::LoadPc(transfer) => self.load_pc(index, transfer),
            Kind::Alu(alu) => {
                self.conditional(index, item.condition, |s| s.alu(index, alu));
                let last = if self.timed {
                    Last::Ctx
                } else {
                    self.spend(item.spend?, item.check, item.live)
                };
                let fetch = if alu.shifted() {
                    NONSEQUENTIAL
                } else {
                    SEQUENTIAL
                };
                self.after_known(index, last, fetch)
            }
            Kind::Constant { rd, value } => {
                self.set_guest(rd, value);
                let last = self.spend_fetched(item.cost, item.check, item.live);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Multiply(multiply) => {
                let nz = multiply.set_flags && item.live & (FLAG_N | FLAG_Z) != 0;
                let merge = nz && item.live & (FLAG_C | FLAG_V) != 0;
                if self.timed && item.spend.is_none() {
                    self.save();
                    if run.is_none() {
                        self.multiply_cycles(index, multiply)?;
                        self.clobber();
                        self.fetch_counted(index);
                    }
                } else {
                    if !nz || merge {
                        self.keep(item.live);
                    }
                    if item.spend.is_none() {
                        self.multiply_cycles(index, multiply)?;
                    }
                }
                let result = self.multiply(multiply);
                if nz {
                    self.multiply_flags(result, merge, item.live & FLAG_C != 0);
                } else {
                    self.clobber();
                }
                let last = match item.spend {
                    Some(spend) => self.spend(spend, item.check, item.live),
                    None if self.timed => Last::Ctx,
                    None => self.spend_counted(COUNTED),
                };
                self.after_known(index, last, NONSEQUENTIAL)
            }
            Kind::Mrs { rd, spsr } => {
                if spsr {
                    self.mrs_spsr(rd);
                } else {
                    self.mrs(rd);
                }
                let last = self.spend(item.cost, item.check, item.live);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Msr {
                operand,
                mask,
                spsr,
            } => self.msr(index, operand, mask, spsr),
            Kind::Clz { rd, rm } => {
                self.keep(item.live);
                self.clz(rd, rm);
                self.clobber();
                let last = self.spend(item.spend?, item.check, item.live);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Cp15 { load: Some(rd) } => {
                self.a.load(true, RCX, at(RSP, FRAME_CTX));
                self.a.load(true, RCX, at(RCX, self.dtcm));
                self.a.load(false, RCX, at(RCX, 0));
                self.write(rd, RCX);
                let last = self.spend(item.spend?, item.check, item.live);
                self.after_known(index, last, NONSEQUENTIAL)
            }
            Kind::Cp15 { load: None } => self.cp15(index),
            Kind::Transfer(transfer) => self.transfer(index, transfer),
            Kind::Block(block) => self.block(index, block, false),
            Kind::UserBlock(block) => self.user_block(index, block),
            Kind::Step => self.step(index),
            Kind::Exit => self.step_out(index),
            Kind::Branch(word) => self.branch(index, word),
            Kind::Jump(jump) => self.jump(index, jump),
        }
    }

    fn conditional(&mut self, index: usize, condition: u32, body: impl FnOnce(&mut Self)) {
        if condition == ALWAYS {
            body(self);
            return;
        }
        let skip = self.a.label();
        self.skip_unless(condition, skip);
        let before = self.flags;
        body(self);
        self.join(before, self.live(index));
        self.a.bind(skip);
    }

    fn shifted(&mut self, dest: R, rm: Val, shift: u32, amount: u32, live: u8) {
        self.put(dest, rm);
        match (shift, amount) {
            (0, 0) => {}
            (0, n) => {
                self.keep(live);
                self.a.rot(Rot::Shl, false, dest, n as u8);
                self.clobber();
            }
            (1, 0) => self.a.mov_imm(dest, 0),
            (1, n) => {
                self.keep(live);
                self.a.rot(Rot::Shr, false, dest, n as u8);
                self.clobber();
            }
            (2, 0) => {
                self.keep(live);
                self.a.rot(Rot::Sar, false, dest, 31);
                self.clobber();
            }
            (2, n) => {
                self.keep(live);
                self.a.rot(Rot::Sar, false, dest, n as u8);
                self.clobber();
            }
            (_, 0) => {
                self.carry_in(false);
                self.a.rot(Rot::Rcr, false, dest, 1);
                self.clobber();
            }
            (_, n) => {
                self.keep(live);
                self.a.rot(Rot::Ror, false, dest, n as u8);
                self.clobber();
            }
        }
    }

    fn carry_in(&mut self, inverted: bool) {
        self.save();
        self.restore();
        if self.flags.host != Some(inverted) {
            self.a.cmc();
            self.flags.host = Some(inverted);
        }
    }

    fn second(&mut self, operand: Operand, pc: u32, scratch: R, live: u8) -> Val {
        match operand {
            Operand::Immediate { value, .. } => Val::Imm(value),
            Operand::Register {
                rm,
                shift: 0,
                amount: 0,
            } => self.val(rm, pc),
            Operand::Register {
                shift: 1,
                amount: 0,
                ..
            } => Val::Imm(0),
            Operand::Register { rm, shift, amount } => {
                let value = self.val(rm, pc);
                self.shifted(scratch, value, shift, amount, live);
                Val::Reg(scratch)
            }
            Operand::Shifted { rm, shift, rs } => {
                let (rm, rs) = (self.val(rm, pc), self.val(rs, pc));
                self.shifted_by(shift, rm, rs, scratch, live, false)
            }
        }
    }

    fn shifted_by(
        &mut self,
        shift: u32,
        rm: Val,
        rs: Val,
        scratch: R,
        live: u8,
        carry: bool,
    ) -> Val {
        if carry {
            self.save();
        } else {
            self.keep(live);
        }
        match rs {
            Val::Reg(reg) => self.a.extend8(false, RCX, reg),
            Val::Mem(mem) => self.a.load8(false, RCX, mem),
            Val::Imm(value) => self.a.mov_imm(RCX, value & 0xFF),
        }
        if shift != 3 {
            self.a.mov_imm(RDX, 33);
            self.a.alu(Op2::Cmp, false, RCX, RDX);
            self.a.cmov(cc::AE, RCX, RDX);
        }
        self.extend(shift == 2, RDX, rm);
        match shift {
            0 => self.a.rot_cl(Rot::Shl, true, RDX),
            1 | 2 => {
                self.a.rot(Rot::Shl, true, RDX, 1);
                let kind = if shift == 1 { Rot::Shr } else { Rot::Sar };
                self.a.rot_cl(kind, true, RDX);
            }
            _ => self.a.rot_cl(Rot::Ror, false, RDX),
        }
        if carry {
            self.a.mov(true, RSI, RDX);
            match shift {
                0 => self.a.rot(Rot::Shr, true, RSI, (32 - IMAGE_CARRY) as u8),
                1 | 2 => self.a.rot(Rot::Shl, false, RSI, IMAGE_CARRY as u8),
                _ => self.a.rot(Rot::Shr, false, RSI, (31 - IMAGE_CARRY) as u8),
            }
            self.a.test(RCX, RCX);
            self.a.cmov(cc::E, RSI, RAX);
        }
        if matches!(shift, 1 | 2) {
            self.a.rot(Rot::Shr, true, RDX, 1);
        }
        self.clobber();
        self.put(scratch, Val::Reg(RDX));
        Val::Reg(scratch)
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
        let live = self.live(index);
        let flags = alu.set_flags;
        let keep_live = if flags { 0 } else { live };
        let carry = matches!(alu.opcode, 0x5..=0x7);
        let second = self.second(alu.operand, pc, RDX, if carry { 0xF } else { keep_live });
        let first = self.val(alu.rn, pc);
        if !flags && !carry && matches!(alu.opcode, 0x2 | 0x4) && self.lea_arith(alu, first, second)
        {
            return;
        }
        let (op, swap) = match alu.opcode {
            0x2 | 0xA => (Op2::Sub, false),
            0x3 => (Op2::Sub, true),
            0x4 | 0xB => (Op2::Add, false),
            0x5 => (Op2::Adc, false),
            0x6 => (Op2::Sbb, false),
            _ => (Op2::Sbb, true),
        };
        if carry {
            self.carry_in(op == Op2::Sbb);
        } else {
            self.keep(keep_live);
        }
        if alu.opcode == 0xA {
            let first = match first {
                Val::Reg(reg) => reg,
                other => {
                    self.put(RCX, other);
                    RCX
                }
            };
            self.apply(Op2::Cmp, first, second);
            self.set(true);
            return;
        }
        let (a, b) = if swap {
            (second, first)
        } else {
            (first, second)
        };
        let home = self.home(alu.rd).filter(|_| alu.writes_rd());
        let (a, b) = match home {
            Some(reg) if matches!(op, Op2::Add | Op2::Adc) && !self.reg_free(reg, b) => (b, a),
            _ => (a, b),
        };
        let in_place = home.is_none()
            && alu.writes_rd()
            && !swap
            && alu.rn == alu.rd
            && self.in_place(op, a, b);
        if !in_place {
            let dest = match home {
                Some(reg) if self.reg_free(reg, b) || !self.reg_free(reg, a) => reg,
                _ => RCX,
            };
            self.put(dest, a);
            self.apply(op, dest, b);
            if alu.writes_rd() {
                self.write(alu.rd, dest);
            }
        }
        if flags {
            self.set(matches!(op, Op2::Sub | Op2::Sbb));
        } else {
            self.clobber();
        }
    }

    fn reg_free(&self, reg: R, other: Val) -> bool {
        !matches!(other, Val::Reg(used) if used == reg)
    }

    fn in_place(&mut self, op: Op2, dest: Val, value: Val) -> bool {
        match (dest, value) {
            (Val::Mem(mem), Val::Reg(reg)) => self.a.alu_store(op, mem, reg),
            (Val::Mem(mem), Val::Imm(value)) => self.a.alu_mem_imm(op, mem, value as i32),
            _ => return false,
        }
        true
    }

    fn lea_arith(&mut self, alu: Alu, first: Val, second: Val) -> bool {
        let Some(dest) = self.home(alu.rd) else {
            return false;
        };
        let sub = alu.opcode == 0x2;
        let base = match first {
            Val::Reg(reg) => reg,
            _ => return false,
        };
        match second {
            Val::Imm(value) => {
                let disp = if sub {
                    (value as i32).wrapping_neg()
                } else {
                    value as i32
                };
                if disp == 0 {
                    self.put(dest, Val::Reg(base));
                } else {
                    self.a.lea(false, dest, at(base, disp));
                }
                true
            }
            Val::Reg(reg) if !sub => {
                self.a.lea(false, dest, indexed(base, reg, 1, 0));
                true
            }
            _ => false,
        }
    }

    fn logical(&mut self, index: usize, alu: Alu) {
        let pc = self.pc_read(index);
        let live = self.live(index);
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
        let flags = alu.set_flags && live != 0;
        if !alu.writes_rd() && !flags {
            return;
        }
        let needs_c = flags && live & FLAG_C != 0;
        let needs_v = flags && live & FLAG_V != 0;
        let old_c = needs_c && carry.is_none();
        let merge = needs_v || old_c;
        if merge {
            self.save();
            if old_c {
                self.a.mov(false, RSI, RAX);
            }
        }
        if let (true, Some(Carry::Bit(rm, _))) = (needs_c, carry) {
            let value = self.val(rm, pc);
            self.put(RSI, value);
        }
        if !flags {
            self.keep(live);
        }
        let second = match alu.operand {
            Operand::Shifted { rm, shift, rs } if needs_c => {
                let (rm, rs) = (self.val(rm, pc), self.val(rs, pc));
                self.shifted_by(shift, rm, rs, RCX, 0, true)
            }
            operand => self.second(operand, pc, RCX, if flags { 0 } else { live }),
        };
        let slot = self.home(alu.rd).is_none();
        let dest = match alu.opcode {
            0xD if slot => {
                let mem = at(REGS, 4 * alu.rd as i32);
                match second {
                    Val::Imm(value) if !flags => self.a.store_imm(mem, value),
                    Val::Reg(reg) => {
                        self.a.store(false, mem, reg);
                        if flags {
                            self.a.test(reg, reg);
                        }
                    }
                    other => {
                        self.put(RCX, other);
                        if flags {
                            self.a.test(RCX, RCX);
                        }
                        self.a.store(false, mem, RCX);
                    }
                }
                None
            }
            0xD | 0xF => {
                let dest = self.home(alu.rd).unwrap_or(RCX);
                self.put(dest, second);
                if alu.opcode == 0xF {
                    self.a.not(dest);
                }
                if flags {
                    self.a.test(dest, dest);
                }
                Some(dest)
            }
            0x8 => {
                let first = match self.val(alu.rn, pc) {
                    Val::Reg(reg) => reg,
                    other => {
                        self.put(RDX, other);
                        RDX
                    }
                };
                match second {
                    Val::Imm(value) => self.a.test_imm(first, value),
                    Val::Reg(reg) => self.a.test(first, reg),
                    Val::Mem(mem) => {
                        self.a.load(false, RCX, mem);
                        self.a.test(first, RCX);
                    }
                }
                None
            }
            _ => {
                let (op, invert) = match alu.opcode {
                    0x0 => (Op2::And, false),
                    0x1 | 0x9 => (Op2::Xor, false),
                    0xC => (Op2::Or, false),
                    _ => (Op2::And, true),
                };
                let second = match (invert, second) {
                    (false, value) => value,
                    (true, Val::Imm(value)) => Val::Imm(!value),
                    (true, other) => {
                        self.put(RCX, other);
                        self.a.not(RCX);
                        Val::Reg(RCX)
                    }
                };
                let first = self.val(alu.rn, pc);
                if slot && alu.writes_rd() && alu.rn == alu.rd && self.in_place(op, first, second) {
                    None
                } else {
                    let dest = match (alu.writes_rd(), self.home(alu.rd)) {
                        (true, Some(reg))
                            if self.reg_free(reg, second)
                                || matches!(first, Val::Reg(used) if used == reg) =>
                        {
                            reg
                        }
                        _ => RDX,
                    };
                    match (first, second) {
                        (Val::Reg(reg), _) if reg == dest => self.apply(op, dest, second),
                        (first, Val::Reg(reg)) if reg == dest => self.apply(op, dest, first),
                        (first, second) => {
                            self.put(dest, first);
                            self.apply(op, dest, second);
                        }
                    }
                    alu.writes_rd().then_some(dest)
                }
            }
        };
        if let Some(dest) = dest {
            self.write(alu.rd, dest);
        }
        if !flags {
            self.clobber();
            return;
        }
        if needs_c {
            match carry {
                Some(Carry::Fixed(true)) => self.a.stc(),
                Some(Carry::Bit(_, 0)) => self.a.rot(Rot::Ror, false, RSI, 1),
                Some(Carry::Bit(_, bit)) => self.a.rot(Rot::Rol, false, RSI, (32 - bit) as u8),
                Some(Carry::Host) => self.a.rot(Rot::Rol, false, RSI, (32 - IMAGE_CARRY) as u8),
                _ => {}
            }
        }
        if !merge {
            self.set(false);
            return;
        }
        self.a.lahf();
        if old_c {
            self.a.ah_imm(Op2::And, 0xFE);
            self.a.alu_imm(Op2::And, false, RSI, 0x100);
            self.a.alu(Op2::Or, false, RAX, RSI);
        }
        self.flags = Flags {
            host: None,
            image: true,
        };
    }

    fn multiply(&mut self, multiply: Multiply) -> R {
        let rm = self.val(multiply.rm, 0);
        let rs = self.val(multiply.rs, 0);
        match multiply.long {
            None => {
                let dest = match self.home(multiply.rd) {
                    Some(home) if multiply.accumulate != Some(multiply.rd) => home,
                    _ => RCX,
                };
                let (first, second) = if dest != RCX && multiply.rs == multiply.rd {
                    (rs, rm)
                } else {
                    (rm, rs)
                };
                self.put(dest, first);
                self.apply_imul(dest, second);
                if let Some(rn) = multiply.accumulate {
                    let rn = self.val(rn, 0);
                    self.apply(Op2::Add, dest, rn);
                }
                self.write(multiply.rd, dest);
                dest
            }
            Some(long) => {
                self.extend(long.signed, RCX, rm);
                self.extend(long.signed, RDX, rs);
                self.a.imul(true, RCX, RDX);
                if long.accumulate {
                    let low = self.val(long.low, 0);
                    let high = self.val(multiply.rd, 0);
                    self.put(RDX, high);
                    self.a.rot(Rot::Shl, true, RDX, 32);
                    self.a.alu(Op2::Add, true, RCX, RDX);
                    self.put(RDX, low);
                    self.a.alu(Op2::Add, true, RCX, RDX);
                }
                self.write(long.low, RCX);
                self.a.rot(Rot::Shr, true, RCX, 32);
                self.write(multiply.rd, RCX);
                RCX
            }
        }
    }

    fn multiply_flags(&mut self, result: R, merge: bool, old_c: bool) {
        if merge && old_c {
            self.a.sahf();
            self.a.dec(result);
            self.a.inc(result);
        } else {
            self.a.test(result, result);
        }
        if !merge {
            self.set(false);
            return;
        }
        self.a.lahf();
        self.flags = Flags {
            host: None,
            image: true,
        };
    }

    fn multiply_cycles(&mut self, index: usize, multiply: Multiply) -> Option<()> {
        let extra = match multiply.long {
            Some(long) => 1 + u32::from(long.accumulate),
            None => u32::from(multiply.accumulate.is_some()),
        };
        let fetch = if self.timed {
            0
        } else {
            self.plan.fetch[index][0]
        };
        let fixed = i32::try_from(1 + extra + fetch).ok()?;
        let rs = self.val(multiply.rs, 0);
        self.put(COUNTED, rs);
        if multiply.long.is_none_or(|long| long.signed) {
            self.a.mov(false, RDX, COUNTED);
            self.a.rot(Rot::Sar, false, RDX, 31);
            self.a.alu(Op2::Xor, false, COUNTED, RDX);
        }
        self.a.alu_imm(Op2::Or, false, COUNTED, 1);
        self.a.bsr(COUNTED, COUNTED);
        self.a.rot(Rot::Shr, false, COUNTED, 3);
        self.a.alu_imm(Op2::Add, false, COUNTED, fixed);
        Some(())
    }

    fn apply_imul(&mut self, dest: R, value: Val) {
        match value {
            Val::Reg(reg) => self.a.imul(false, dest, reg),
            Val::Mem(mem) => self.a.imul_load(dest, mem),
            Val::Imm(value) => {
                self.a.mov_imm(RDX, value);
                self.a.imul(false, dest, RDX);
            }
        }
    }

    fn extend(&mut self, signed: bool, dest: R, value: Val) {
        match (signed, value) {
            (false, value) => self.put(dest, value),
            (true, Val::Reg(reg)) => self.a.movsxd(dest, reg),
            (true, Val::Mem(mem)) => self.a.movsxd_load(dest, mem),
            (true, Val::Imm(value)) => self.a.mov_imm64(dest, value as i32 as i64 as u64),
        }
    }

    fn offset(&mut self, offset: Offset, pc: u32, scratch: R) -> Option<Val> {
        let value = match offset {
            Offset::Immediate(0) => return None,
            Offset::Immediate(value) => Val::Imm(value),
            Offset::Register(rm) => self.val(rm, pc),
            Offset::Shifted { rm, shift, amount } => {
                match self.second(Operand::Register { rm, shift, amount }, pc, scratch, 0) {
                    Val::Imm(0) => return None,
                    other => other,
                }
            }
        };
        Some(match value {
            Val::Mem(mem) => {
                self.a.load(false, scratch, mem);
                Val::Reg(scratch)
            }
            other => other,
        })
    }

    fn moved(&mut self, dest: R, base: Val, offset: Option<Val>, up: bool) {
        match (base, offset) {
            (Val::Reg(reg), Some(Val::Imm(value))) => {
                let disp = if up {
                    value as i32
                } else {
                    (value as i32).wrapping_neg()
                };
                self.a.lea(false, dest, at(reg, disp));
            }
            (Val::Reg(reg), Some(Val::Reg(index))) if up => {
                self.a.lea(false, dest, indexed(reg, index, 1, 0));
            }
            (Val::Imm(base), Some(Val::Imm(value))) => {
                let address = if up {
                    base.wrapping_add(value)
                } else {
                    base.wrapping_sub(value)
                };
                self.a.mov_imm(dest, address);
            }
            (base, offset) => {
                self.put(dest, base);
                if let Some(offset) = offset {
                    let op = if up { Op2::Add } else { Op2::Sub };
                    self.apply(op, dest, offset);
                }
            }
        }
    }

    fn literal(&self, index: usize, transfer: Transfer) -> Option<u32> {
        if self.quick.is_none() || !self.timed || self.v5 || self.pages || !self.plan.thumb {
            return None;
        }
        let addr = transfer.pc_literal(self.pc_read(index))?;
        let literal = self.plan.literal(index)?;
        (literal.addr == addr).then_some(literal.word)
    }

    fn transfer(&mut self, index: usize, transfer: Transfer) -> Option<()> {
        debug_assert!(transfer.width != Width::Word || !transfer.signed);
        let item = self.items[index];
        let conditional = item.condition != ALWAYS;
        let skip = conditional.then(|| self.a.label());
        if let Some(skip) = skip {
            self.skip_unless(item.condition, skip);
        }
        let before = self.flags;
        let pending = self.pending;
        self.save();
        let pc = self.pc_read(index);
        if !transfer.load() {
            if transfer.rd == PC {
                let value = pc.wrapping_add(4);
                let value = match transfer.width {
                    Width::Byte => value & 0xFF,
                    Width::Half => value & 0xFFFF,
                    Width::Word => value,
                };
                self.a.mov_imm(VALUE, value);
            } else {
                let rd = self.val(transfer.rd, pc);
                match (transfer.width, rd) {
                    (Width::Byte, Val::Reg(reg)) => self.a.extend8(false, VALUE, reg),
                    (Width::Byte, Val::Mem(mem)) => self.a.load8(false, VALUE, mem),
                    (Width::Half, Val::Reg(reg)) => self.a.extend16(false, VALUE, reg),
                    (Width::Half, Val::Mem(mem)) => self.a.load16(false, VALUE, mem),
                    (_, other) => self.put(VALUE, other),
                }
            }
        }
        let base = self.val(transfer.rn, pc);
        let scratch = if transfer.load() { RDX } else { RSI };
        let offset = self.offset(transfer.offset, pc, scratch);
        match (transfer.writeback(), transfer.pre()) {
            (true, true) => {
                self.moved(ADDRESS, base, offset, transfer.up());
                self.write(transfer.rn, ADDRESS);
            }
            (true, false) => {
                self.put(ADDRESS, base);
                if let Some(offset) = offset {
                    let op = if transfer.up() { Op2::Add } else { Op2::Sub };
                    match (base, offset) {
                        (Val::Reg(reg), offset) => self.apply(op, reg, offset),
                        (Val::Mem(mem), Val::Reg(reg)) => self.a.alu_store(op, mem, reg),
                        (Val::Mem(mem), Val::Imm(value)) => {
                            self.a.alu_mem_imm(op, mem, value as i32)
                        }
                        _ => return None,
                    }
                }
            }
            (false, true) => self.moved(ADDRESS, base, offset, transfer.up()),
            (false, false) => self.put(ADDRESS, base),
        }
        let tag = self.tag(index);
        let paged = self.pages && !self.timed && (self.v5 || self.code != 0);
        if paged {
            self.flush();
        }
        let literal = self.literal(index, transfer);
        debug_assert!(!paged || literal.is_none());
        let packed = memory_word(
            transfer.kind() | literal.map_or(0, |_| ROM_LITERAL),
            item.ahead,
            self.charged(index),
            self.pending,
        )? | tag
            | self.code << CODE_CLASS;
        let (stub, out) = match (transfer.load(), self.shared) {
            (false, _) if tag != 0 => (Stub::Store, Out::Stub),
            (false, _) => (Stub::Store, Out::Site),
            (true, Some(_)) if self.shares(index) => (Stub::Load, Out::Shared),
            (true, _) => (Stub::Load, Out::Site),
        };
        let out = if paged {
            let exit = match (out, self.shared) {
                _ if self.last(index) => None,
                (Out::Stub, _) => None,
                (Out::Shared, Some(shared)) => Some(shared),
                _ => Some(self.site(left(index as u32 + 1))?),
            };
            if transfer.load() {
                self.paged_load(transfer, self.charged(index), stub, packed, exit);
            } else {
                self.paged_store(transfer, self.charged(index), stub, packed, exit);
            }
            Out::Stub
        } else {
            match literal {
                Some(word) => {
                    self.a
                        .mov_imm64(WORD, u64::from(packed) | u64::from(word) << 32);
                    self.reach(stub as u32, true);
                }
                None => self.call(stub, packed),
            }
            out
        };
        self.pending = 0;
        self.flags = Flags {
            host: None,
            image: true,
        };
        if transfer.load() {
            self.write(transfer.rd, RCX);
        }
        self.after_memory(index, skip, before, pending, out)
    }

    fn paged_load(
        &mut self,
        transfer: Transfer,
        cost: u32,
        stub: Stub,
        word: u32,
        exit: Option<Label>,
    ) {
        let (slow, back) = (self.a.label(), self.a.label());
        self.a.load(true, RDX, at(RSP, FRAME_FAST));
        self.a
            .load(true, RDX, at(RDX, offset_of!(Fast, pages) as i32));
        self.a.mov(false, RSI, ADDRESS);
        self.a.rot(Rot::Shr, false, RSI, PAGE_SHIFT as u8);
        self.a.rot(
            Rot::Shl,
            false,
            RSI,
            size_of::<Page>().trailing_zeros() as u8,
        );
        self.a.alu(Op2::Add, true, RDX, RSI);
        self.a
            .load(!self.v5, RSI, at(RDX, offset_of!(Page, meta) as i32));
        self.a.test(RSI, RSI);
        self.a.jcc(cc::E, slow);
        match transfer.width {
            Width::Word => self.a.test_imm(ADDRESS, 3),
            Width::Half => self.a.test_imm(ADDRESS, 1),
            Width::Byte => {}
        }
        if transfer.width != Width::Byte {
            self.a.jcc(cc::NE, slow);
        }
        self.a
            .load(true, RDX, at(RDX, offset_of!(Page, addend) as i32));
        self.a.mov(false, ADDRESS, ADDRESS);
        let data = indexed(RDX, ADDRESS, 1, 0);
        match transfer.width {
            Width::Word => self.a.load(false, ADDRESS, data),
            Width::Half => self.a.load16(transfer.signed, ADDRESS, data),
            Width::Byte => self.a.load8(transfer.signed, ADDRESS, data),
        }
        if self.v5 {
            self.paged_cycles(transfer.width, cost);
        } else {
            self.paged_v4(true, transfer.width);
        }
        self.a.bind(back);
        self.slows.push(Slow {
            label: slow,
            back,
            stub,
            word,
            rd: Some(transfer.rd),
            exit,
            block: None,
        });
    }

    fn paged_store(
        &mut self,
        transfer: Transfer,
        cost: u32,
        stub: Stub,
        word: u32,
        exit: Option<Label>,
    ) {
        let (slow, back) = (self.a.label(), self.a.label());
        self.a.load(true, REGS, at(RSP, FRAME_FAST));
        self.a
            .load(true, REGS, at(REGS, offset_of!(Fast, pages) as i32));
        self.a.mov(false, RSI, ADDRESS);
        self.a.rot(Rot::Shr, false, RSI, PAGE_SHIFT as u8);
        self.a.rot(
            Rot::Shl,
            false,
            RSI,
            size_of::<Page>().trailing_zeros() as u8,
        );
        self.a.alu(Op2::Add, true, REGS, RSI);
        self.a
            .load(!self.v5, RSI, at(REGS, offset_of!(Page, meta) as i32));
        self.a.test(RSI, RSI);
        self.a.jcc(cc::E, slow);
        self.a.mov(false, RSI, ADDRESS);
        self.a.rot(Rot::Shr, false, RSI, PAGE_BITS as u8);
        self.a.alu_imm(Op2::And, false, RSI, 3);
        self.a
            .alu_load_wide(Op2::Add, RSI, at(REGS, offset_of!(Page, code) as i32));
        self.a.load8(false, RSI, at(RSI, 0));
        self.a.test(RSI, RSI);
        self.a.jcc(cc::NE, slow);
        self.a
            .load(true, RSI, at(REGS, offset_of!(Page, dirty) as i32));
        self.a.store8_imm(at(RSI, 0), 1);
        self.a
            .load(true, RSI, at(REGS, offset_of!(Page, addend) as i32));
        match transfer.width {
            Width::Word => self.a.alu_imm(Op2::And, false, ADDRESS, !3),
            Width::Half => self.a.alu_imm(Op2::And, false, ADDRESS, !1),
            Width::Byte => self.a.mov(false, ADDRESS, ADDRESS),
        }
        let data = indexed(RSI, ADDRESS, 1, 0);
        match transfer.width {
            Width::Word => self.a.store(false, data, VALUE),
            Width::Half => self.a.store16(data, VALUE),
            Width::Byte => self.a.store8(data, VALUE),
        }
        self.a
            .load(!self.v5, RSI, at(REGS, offset_of!(Page, meta) as i32));
        self.restore_regs();
        if self.v5 {
            self.paged_cycles(transfer.width, cost);
        } else {
            self.paged_v4(false, transfer.width);
        }
        self.a.bind(back);
        self.slows.push(Slow {
            label: slow,
            back,
            stub,
            word,
            rd: None,
            exit,
            block: None,
        });
    }

    fn paged_cycles(&mut self, width: Width, cost: u32) {
        let (long, timed) = (self.a.label(), self.a.label());
        let (cycles, flag) = match width {
            Width::Word => (WORD_CYCLES, LONG_WORD),
            Width::Byte | Width::Half => (HALF_CYCLES, LONG_HALF),
        };
        self.a.mov(false, RDX, RSI);
        if cycles > 0 {
            self.a.rot(Rot::Shr, false, RDX, cycles as u8);
        }
        self.a.extend8(false, RDX, RDX);
        self.a.test_imm(RSI, 1 << flag);
        self.a.jcc(cc::NE, long);
        self.a.alu_imm(Op2::Cmp, false, RDX, cost as i32);
        self.a.jcc(cc::AE, timed);
        self.a.mov_imm(RDX, cost);
        self.a.jmp(timed);
        self.a.bind(long);
        self.overlap(cost);
        self.a.bind(timed);
        self.paged_charge();
    }

    fn restore_regs(&mut self) {
        self.a.load(true, REGS, at(RSP, FRAME_CORE));
        self.a.lea(true, REGS, at(REGS, self.regs));
    }

    fn paged_v4(&mut self, load: bool, width: Width) {
        let cycles = match width {
            Width::Word => WORD_CYCLES,
            Width::Byte | Width::Half => HALF_CYCLES,
        };
        self.a.mov(false, RDX, RSI);
        if cycles > 0 {
            self.a.rot(Rot::Shr, false, RDX, cycles as u8);
        }
        self.a.extend8(false, RDX, RDX);
        self.v4_charge(load, if load { REGS } else { ADDRESS });
    }

    fn v4_charge(&mut self, load: bool, temp: R) {
        let (overlapped, done) = (self.a.label(), self.a.label());
        let (shift, load) = (CODE_CYCLES + 8 * self.code, i32::from(load));
        let near = self.code == 1;
        self.a.test_imm(RSI, 1 << MAIN);
        self.a.jcc(if near { cc::NE } else { cc::E }, overlapped);
        self.a.rot(Rot::Shr, true, RSI, shift as u8);
        self.a.extend8(false, RSI, RSI);
        if near {
            self.a.lea(false, RDX, indexed(RSI, RDX, 1, load));
        } else {
            self.a.alu(Op2::Add, false, RDX, RSI);
        }
        self.a.jmp(done);
        self.a.bind(overlapped);
        self.a.rot(Rot::Shr, true, RSI, shift as u8);
        self.a.extend8(false, RSI, RSI);
        if load != 0 {
            let grown = if near { RSI } else { RDX };
            self.a.lea(false, grown, at(grown, load));
        }
        self.a
            .lea(false, temp, indexed(RSI, RDX, 1, -(CODE_OVERLAP as i32)));
        self.a.alu(Op2::Cmp, false, RSI, RDX);
        self.a.cmov(cc::A, RDX, RSI);
        self.a.alu(Op2::Cmp, false, temp, RDX);
        self.a.cmov(cc::G, RDX, temp);
        if temp == REGS {
            self.restore_regs();
        }
        self.a.bind(done);
        self.a.alu(Op2::Sub, false, S, RDX);
        self.a.store(false, at(RSP, FRAME_LAST), RDX);
        self.a.store_imm(at(RSP, FRAME_FORCE), 0);
    }

    fn overlap(&mut self, cost: u32) {
        let kept = self.a.label();
        self.a
            .lea(false, RSI, at(RDX, cost as i32 - OVERLAP as i32));
        self.a.alu_imm(Op2::Cmp, false, RDX, cost as i32);
        self.a.jcc(cc::AE, kept);
        self.a.mov_imm(RDX, cost);
        self.a.bind(kept);
        self.a.alu(Op2::Cmp, false, RSI, RDX);
        self.a.cmov(cc::G, RDX, RSI);
    }

    fn paged_charge(&mut self) {
        self.a.alu(Op2::Sub, false, S, RDX);
        self.a.lea(false, RSI, at(RDX, -1));
        self.a.alu(Op2::Cmp, false, DEBT, RSI);
        self.a.cmov(cc::B, RSI, DEBT);
        self.a.alu(Op2::Sub, false, DEBT, RSI);
        self.a.alu(Op2::Sub, false, RDX, RSI);
        self.a.store(false, at(RSP, FRAME_LAST), RDX);
        self.a.store_imm(at(RSP, FRAME_FORCE), 0);
    }

    fn paged_block(&mut self, store: bool, registers: &[u32], targets: u32, cost: u32, slow: Slow) {
        let span = 4 * (registers.len() as i32 - 1);
        if span > 0 {
            self.a.mov(false, RSI, ADDRESS);
            self.a
                .alu_imm(Op2::And, false, RSI, ((PAGE_SIZE - 1) & !3) as i32);
            self.a.alu_imm(Op2::Add, false, RSI, span);
            self.a.test_imm(RSI, PAGE_SIZE);
            self.a.jcc(cc::NE, slow.label);
        }
        self.a.load(true, RDX, at(RSP, FRAME_FAST));
        self.a
            .load(true, RDX, at(RDX, offset_of!(Fast, pages) as i32));
        self.a.mov(false, RSI, ADDRESS);
        self.a.rot(Rot::Shr, false, RSI, PAGE_SHIFT as u8);
        self.a.rot(
            Rot::Shl,
            false,
            RSI,
            size_of::<Page>().trailing_zeros() as u8,
        );
        self.a.alu(Op2::Add, true, RDX, RSI);
        self.a
            .load(!self.v5, RSI, at(RDX, offset_of!(Page, meta) as i32));
        self.a.test(RSI, RSI);
        self.a.jcc(cc::E, slow.label);
        if store {
            let ends: &[i32] = if span > 0 { &[0, span] } else { &[0] };
            for &end in ends {
                self.a.lea(false, RSI, at(ADDRESS, end));
                self.a.rot(Rot::Shr, false, RSI, PAGE_BITS as u8);
                self.a.alu_imm(Op2::And, false, RSI, 3);
                self.a
                    .alu_load_wide(Op2::Add, RSI, at(RDX, offset_of!(Page, code) as i32));
                self.a.load8(false, RSI, at(RSI, 0));
                self.a.test(RSI, RSI);
                self.a.jcc(cc::NE, slow.label);
            }
            self.a
                .load(true, RSI, at(RDX, offset_of!(Page, dirty) as i32));
            self.a.store8_imm(at(RSI, 0), 1);
            self.a
                .load(!self.v5, RSI, at(RDX, offset_of!(Page, meta) as i32));
        }
        self.a.alu_imm(Op2::And, false, ADDRESS, !3);
        self.a
            .alu_load_wide(Op2::Add, ADDRESS, at(RDX, offset_of!(Page, addend) as i32));
        for (word, &guest) in registers.iter().enumerate() {
            let data = at(ADDRESS, 4 * word as i32);
            let slot = at(REGS, 4 * guest as i32);
            match (store, self.home(guest)) {
                (true, Some(reg)) => self.a.store(false, data, reg),
                (true, None) => {
                    self.a.load(false, VALUE, slot);
                    self.a.store(false, data, VALUE);
                }
                (false, _) if targets & 1 << guest == 0 => {}
                (false, Some(reg)) => self.a.load(false, reg, data),
                (false, None) => {
                    self.a.load(false, VALUE, data);
                    self.a.store(false, slot, VALUE);
                }
            }
        }
        self.a.mov(false, RDX, RSI);
        self.a.rot(Rot::Shr, false, RDX, WORD_CYCLES as u8);
        self.a.extend8(false, RDX, RDX);
        if span > 0 {
            self.a.mov(false, ADDRESS, RSI);
            self.a
                .rot(Rot::Shr, false, ADDRESS, SEQUENTIAL_CYCLES as u8);
            self.a.extend8(false, ADDRESS, ADDRESS);
            self.a.imul_imm(ADDRESS, ADDRESS, span / 4);
            self.a.alu(Op2::Add, false, RDX, ADDRESS);
        }
        if self.v5 {
            self.overlap(cost);
            self.paged_charge();
        } else {
            self.v4_charge(!store, ADDRESS);
        }
        self.slows.push(slow);
    }

    fn exception_return(&mut self, index: usize, offset: u32) -> Option<()> {
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
        self.canonical();
        self.a.mov_imm(INFO, word);
        self.reach(Stub::Restore as u32, true);
        self.flags = Flags {
            host: None,
            image: true,
        };
        self.step_out(index)
    }

    fn swap(&mut self, index: usize, swap: Swap) -> Option<()> {
        let item = self.items[index];
        let before = self.flags;
        let pending = self.pending;
        self.save();
        let pc = self.pc_read(index);
        let value = self.val(swap.rm, pc);
        match (swap.byte, value) {
            (true, Val::Reg(reg)) => self.a.extend8(false, VALUE, reg),
            (true, Val::Mem(mem)) => self.a.load8(false, VALUE, mem),
            (_, other) => self.put(VALUE, other),
        }
        let base = self.val(swap.rn, pc);
        self.put(ADDRESS, base);
        let kind = if swap.byte { 0 } else { 2 };
        let packed = memory_word(kind, item.ahead, self.charged(index), self.pending)?
            | self.code << CODE_CLASS;
        self.a.mov_imm(WORD, packed);
        self.reach(STUBS as u32, true);
        self.pending = 0;
        self.flags = Flags {
            host: None,
            image: true,
        };
        self.write(swap.rd, RCX);
        self.after_memory(index, None, before, pending, Out::Site)
    }

    fn load_pc(&mut self, index: usize, transfer: Transfer) -> Option<()> {
        let item = self.items[index];
        let skip = (item.condition != ALWAYS).then(|| self.a.label());
        if let Some(skip) = skip {
            self.skip_unless(item.condition, skip);
        }
        let before = self.flags;
        let pending = self.pending;
        self.save();
        let pc = self.pc_read(index);
        let base = self.val(transfer.rn, pc);
        let offset = self.offset(transfer.offset, pc, RDX);
        match (transfer.writeback(), transfer.pre()) {
            (true, true) => {
                self.moved(ADDRESS, base, offset, transfer.up());
                self.write(transfer.rn, ADDRESS);
            }
            (true, false) => {
                self.put(ADDRESS, base);
                if let Some(offset) = offset {
                    let op = if transfer.up() { Op2::Add } else { Op2::Sub };
                    match (base, offset) {
                        (Val::Reg(reg), offset) => self.apply(op, reg, offset),
                        (Val::Mem(mem), Val::Reg(reg)) => self.a.alu_store(op, mem, reg),
                        (Val::Mem(mem), Val::Imm(value)) => {
                            self.a.alu_mem_imm(op, mem, value as i32)
                        }
                        _ => return None,
                    }
                }
            }
            (false, true) => self.moved(ADDRESS, base, offset, transfer.up()),
            (false, false) => self.put(ADDRESS, base),
        }
        let word = load_pc_word(
            transfer.kind(),
            self.charged(index),
            index as u32 + 1,
            self.pending,
        )?;
        self.pending = 0;
        self.canonical();
        self.a.mov_imm(INFO, word);
        self.reach(Stub::LoadPc as u32, false);
        let Some(skip) = skip else {
            return Some(());
        };
        self.a.bind(skip);
        self.flags = before;
        self.pending = pending;
        let last = self.spend_fetched(item.cost, item.check, 0xF);
        self.after_known(index, last, SEQUENTIAL)
    }

    fn user_block(&mut self, index: usize, block: Block) -> Option<()> {
        if !self.last(index) {
            return None;
        }
        let pending = self.pending;
        let (banked, fiq) = (self.a.label(), self.a.label());
        self.save();
        self.a.load(false, RDX, at(REGS, self.cpsr));
        self.a.alu_imm(Op2::And, false, RDX, MODE_MASK as i32);
        self.a.alu_imm(Op2::Cmp, false, RDX, FIQ_MODE as i32);
        self.a.jcc(cc::E, fiq);
        self.a.mov_imm(RSI, BANKED_MODES);
        self.a.bt(RSI, RDX);
        self.a.jcc(cc::B, banked);
        self.clobber();
        let flags = self.flags;
        self.block(index, block, false)?;
        self.a.bind(banked);
        (self.pending, self.flags) = (pending, flags);
        self.block(index, block, true)?;
        self.a.bind(fiq);
        (self.pending, self.flags) = (pending, flags);
        self.step_out(index)
    }

    fn user_bank(&mut self, registers: &[u32], load: bool) {
        for &guest in registers.iter().filter(|&&guest| guest >= 13) {
            let slot = at(REGS, 4 * guest as i32);
            let bank = at(REGS, self.user_high + 4 * (guest as i32 - 8));
            let (from, to) = if load { (slot, bank) } else { (bank, slot) };
            self.a.load(false, RDX, from);
            self.a.store(false, to, RDX);
        }
        for guest in 8..13 {
            let bank = at(REGS, self.user_high + 4 * (guest as i32 - 8));
            self.stash(guest, bank, RDX);
        }
        self.a.load(false, RCX, at(REGS, self.cpsr));
        self.a.alu_imm(Op2::And, false, RCX, 0xF);
        self.a.alu(Op2::Add, false, RCX, RCX);
        self.a.mov_imm(RDX, BANK_SLOTS);
        self.a.rot_cl(Rot::Shr, false, RDX);
        self.a.alu_imm(Op2::And, false, RDX, 3);
        for guest in [13, 14] {
            let bank = indexed(REGS, RDX, 8, self.banked + 4 * (guest as i32 - 13));
            self.stash(guest, bank, RCX);
        }
    }

    fn stash(&mut self, guest: u32, mem: Mem, scratch: R) {
        let source = match self.val(guest, 0) {
            Val::Reg(reg) => reg,
            other => {
                self.put(scratch, other);
                scratch
            }
        };
        self.a.store(false, mem, source);
    }

    fn block(&mut self, index: usize, block: Block, user: bool) -> Option<()> {
        let item = self.items[index];
        if block.loads_pc() && !self.v5 && !self.timed {
            return None;
        }
        let skip = (item.condition != ALWAYS).then(|| self.a.label());
        if let Some(skip) = skip {
            self.skip_unless(item.condition, skip);
        }
        let before = self.flags;
        let pending = self.pending;
        self.save();
        let pc = self.pc_read(index);
        let registers: Vec<u32> = block.registers().collect();
        let kept = if user { 1 << 13 | 1 << 14 } else { 0 };
        if user && !block.load {
            self.user_bank(&registers, false);
        }
        let bytes = 4 * registers.len() as u32;
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
        let base = self.val(block.rn, pc);
        self.moved(ADDRESS, base, (start != 0).then_some(Val::Imm(start)), true);
        let list = u32::from(block.list);
        let keeps = if self.v5 {
            block.keeps_loaded_base()
        } else {
            list & 1 << block.rn != 0
        };
        let replaced = block.writeback && !(block.load && keeps);
        let targets = if replaced {
            list & !(1 << block.rn)
        } else {
            list
        };
        let early = !self.v5 && !block.load && replaced && registers.first() != Some(&block.rn);
        if early {
            self.write_back(base, moved)?;
        }
        let paged = self.pages
            && !self.timed
            && (self.v5 || self.code != 0)
            && !user
            && !registers.is_empty()
            && list & 1 << PC == 0;
        if paged {
            self.flush();
        }
        if !block.load && !paged {
            for &guest in &registers {
                if let (Some(reg), true) = (self.home(guest), kept & 1 << guest == 0) {
                    self.a.store(false, at(REGS, 4 * guest as i32), reg);
                }
            }
        }
        let returns = block.loads_pc();
        let collects = returns && self.v5;
        let (stub, lists) = match (block.load, collects) {
            (true, true) => (Stub::Collect, list | targets << 16),
            (true, false) => (Stub::Gather, list | targets << 16),
            (false, _) => (Stub::Scatter, list),
        };
        let count = registers.len() as u32;
        let packed = if collects {
            block_word(count, 0, 0, self.pending)?
        } else {
            block_word(count, item.ahead, self.charged(index), self.pending)? | self.code
        };
        if paged {
            let exit = if self.last(index) {
                None
            } else {
                Some(self.site(left(index as u32 + 1))?)
            };
            let (label, back) = (self.a.label(), self.a.label());
            let slow = Slow {
                label,
                back,
                stub,
                word: packed,
                rd: None,
                exit,
                block: Some(SlowBlock {
                    lists,
                    spill: if block.load { 0 } else { list },
                    reload: if block.load { targets } else { 0 },
                    base,
                    moved,
                    replaced: replaced && !early,
                }),
            };
            self.paged_block(!block.load, &registers, targets, self.charged(index), slow);
            if replaced && !early {
                self.write_back(base, moved)?;
            }
            self.a.bind(back);
            self.flags = Flags {
                host: None,
                image: true,
            };
            return self.after_memory(index, skip, before, pending, Out::Stub);
        }
        self.a.mov_imm(VALUE, lists);
        self.call(stub, packed);
        self.pending = 0;
        self.flags = Flags {
            host: None,
            image: true,
        };
        if block.load {
            for &guest in &registers {
                if let (Some(reg), true) = (self.home(guest), targets & !kept & 1 << guest != 0) {
                    self.a.load(false, reg, at(REGS, 4 * guest as i32));
                }
            }
            if user {
                self.user_bank(&registers, true);
            }
        }
        if replaced && !early {
            self.write_back(base, moved)?;
        }
        if returns {
            return self.returns(index, skip, before, pending);
        }
        self.after_memory(index, skip, before, pending, Out::Site)
    }

    fn spill(&mut self, guests: u32, load: bool) {
        for guest in (0..15).filter(|guest| guests & 1 << guest != 0) {
            if let Some(reg) = self.home(guest) {
                let slot = at(REGS, 4 * guest as i32);
                if load {
                    self.a.load(false, reg, slot);
                } else {
                    self.a.store(false, slot, reg);
                }
            }
        }
    }

    fn write_back(&mut self, base: Val, moved: u32) -> Option<()> {
        match base {
            Val::Reg(reg) => self.a.lea(false, reg, at(reg, moved as i32)),
            Val::Mem(mem) => {
                self.a.load(false, VALUE, mem);
                self.a.lea(false, VALUE, at(VALUE, moved as i32));
                self.a.store(false, mem, VALUE);
            }
            Val::Imm(_) => return None,
        }
        Some(())
    }

    fn returns(
        &mut self,
        index: usize,
        skip: Option<Label>,
        before: Flags,
        pending: u32,
    ) -> Option<()> {
        let (stub, cell) = if self.v5 {
            self.a.mov(false, VALUE, RCX);
            self.a.alu_imm(Op2::And, false, VALUE, CYCLES_MASK);
            (Stub::Return, self.cell(index))
        } else {
            (Stub::Branch, None)
        };
        self.a.load(false, TARGET, at(REGS, PC_SLOT));
        self.leave_cell(
            stub,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Core,
                force: false,
                branch: true,
                last: Last::Ctx,
                pending: 0,
            },
            cell,
        )?;
        let Some(skip) = skip else {
            return Some(());
        };
        self.a.bind(skip);
        self.flags = before;
        self.pending = pending;
        let last = self.spend_fetched(self.items[index].cost, true, 0xF);
        self.leave(
            Stub::Exit,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Known(SEQUENTIAL),
                force: false,
                branch: false,
                last,
                pending: self.pending,
            },
        )
    }

    fn after_memory(
        &mut self,
        index: usize,
        skip: Option<Label>,
        before: Flags,
        pending: u32,
        out: Out,
    ) -> Option<()> {
        let info = left(index as u32 + 1);
        let next_steps = self
            .items
            .get(index + 1)
            .is_some_and(|next| self.timed || stepped(next.kind));
        if self.last(index) {
            self.leave(Stub::Exit, info)?;
        } else {
            match (out, self.shared) {
                (Out::Stub, _) => {}
                (Out::Shared, Some(shared)) => self.a.jcc(cc::LE, shared),
                _ => {
                    let site = self.site(info)?;
                    self.a.jcc(cc::LE, site);
                }
            }
            if !self.timed && self.items[index].check {
                self.check(info.at, 0, info)?;
            }
            if next_steps {
                self.a.mov_imm(FETCH, NONSEQUENTIAL);
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
                flags: before,
                nf: next_steps,
            });
        }
        Some(())
    }

    fn step(&mut self, index: usize) -> Option<()> {
        let item = self.items[index];
        self.save();
        self.call(
            Stub::Step,
            step_word(index as u32, item.ahead, self.pending)?,
        );
        self.pending = 0;
        self.flags = Flags {
            host: None,
            image: true,
        };
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
            self.a.jcc(cc::NE, forced);
        }
        if self.last(index) {
            return self.leave(Stub::Exit, info);
        }
        if self.timed {
            self.check(info.at, item.ahead, info)?;
        } else {
            let site = self.site(info)?;
            self.a.jcc(cc::NE, site);
            if item.check {
                self.check(info.at, 0, info)?;
            }
        }
        self.hand_fetch(index, Fetch::Core);
        Some(())
    }

    fn mrs(&mut self, rd: u32) {
        self.save();
        self.a.mov(false, VALUE, RAX);
        self.a.alu_imm(Op2::And, false, VALUE, IMAGE_MASK);
        self.a.imul_imm(VALUE, VALUE, IMAGE_TO_NZCV);
        self.a.alu_imm(Op2::And, false, VALUE, NZCV_MASK as i32);
        self.a.load(false, RCX, at(REGS, self.cpsr));
        self.a.alu_imm(Op2::And, false, RCX, !NZCV_MASK as i32);
        self.a.alu(Op2::Or, false, RCX, VALUE);
        self.clobber();
        self.write(rd, RCX);
    }

    fn mrs_spsr(&mut self, rd: u32) {
        self.save();
        self.call(Stub::Psr, SPSR_READ);
        self.clobber();
        self.write(rd, VALUE);
    }

    fn msr(&mut self, index: usize, operand: Operand, mask: u32, spsr: bool) -> Option<()> {
        let cost = self.items[index].cost;
        let value = match operand {
            Operand::Immediate { value, .. } => Val::Imm(value),
            Operand::Register { rm, .. } => self.val(rm, self.pc_read(index)),
            Operand::Shifted { .. } => return self.step_out(index),
        };
        if (!self.v5 && !self.pages) || cost > MAX_COST {
            return self.step_out(index);
        }
        self.save();
        if self.pending + cost > PENDING_LIMIT {
            self.flush();
        }
        let fields = (0..4)
            .filter(|byte| mask & 0xFF << (8 * byte) != 0)
            .fold(0, |fields, byte| fields | 1 << byte);
        let kind = if spsr { SPSR_WRITE } else { 0 };
        let word = psr_word(index as u32 + 1, fields, kind, cost, self.pending + cost)?;
        self.put(VALUE, value);
        self.a.mov_imm(INFO, word);
        self.reach(Stub::Psr as u32, false);
        self.pending = 0;
        self.flags = Flags {
            host: None,
            image: true,
        };
        Some(())
    }

    fn clz(&mut self, rd: u32, rm: u32) {
        let dest = self.home(rd).unwrap_or(RDX);
        let source = match self.val(rm, 0) {
            Val::Reg(reg) => reg,
            other => {
                self.put(dest, other);
                dest
            }
        };
        self.a.mov_imm(RCX, 63);
        self.a.bsr(dest, source);
        self.a.cmov(cc::E, dest, RCX);
        self.a.alu_imm(Op2::Xor, false, dest, 31);
        self.write(rd, dest);
    }

    fn cp15(&mut self, index: usize) -> Option<()> {
        if !self.last(index) {
            return None;
        }
        let cost = self.items[index].spend?;
        if !self.v5 || !(2..=MAX_COST).contains(&cost) {
            return self.step_out(index);
        }
        self.save();
        if self.pending + cost > PENDING_LIMIT {
            self.flush();
        }
        let info = Info {
            at: index as u32 + 1,
            fetch: Fetch::Known(NONSEQUENTIAL),
            force: false,
            branch: false,
            last: Last::Old(cost),
            pending: self.pending + cost,
        }
        .word()?;
        if let Some(cell) = self.cell(self.items.len()) {
            self.a.store8_imm(at(RSP, FRAME_CELL), cell as u8 + 1);
        }
        self.a.mov_imm(INFO, info);
        self.reach(Stub::Step as u32, false);
        Some(())
    }

    fn step_out(&mut self, index: usize) -> Option<()> {
        self.save();
        self.call(Stub::Step, step_word(index as u32, 0, self.pending)?);
        self.pending = 0;
        self.flags = Flags {
            host: None,
            image: true,
        };
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
                s.set_guest(14, link);
            }
            s.a.mov_imm(TARGET, target);
            Stub::Branch
        })
    }

    fn set_guest(&mut self, guest: u32, value: u32) {
        match self.home(guest) {
            Some(reg) => self.a.mov_imm(reg, value),
            None => self.a.store_imm(at(REGS, 4 * guest as i32), value),
        }
    }

    fn jump(&mut self, index: usize, jump: Jump) -> Option<()> {
        let pc = self.pc_read(index);
        let link = jump.link.then(|| self.link(index));
        self.leave_to(index, |s| {
            match jump.target {
                Target::Fixed(target) => s.a.mov_imm(TARGET, target),
                Target::Register { rm, offset, mask } => {
                    let value = s.val(rm, pc);
                    s.put(TARGET, value);
                    if offset != 0 {
                        s.a.lea(false, TARGET, at(TARGET, offset as i32));
                    }
                    if mask != u32::MAX {
                        s.save();
                        s.a.alu_imm(Op2::And, false, TARGET, mask as i32);
                        s.clobber();
                    }
                    if let (Some(fixed), Some(cell)) = (paired(s.items, index), s.cell(index)) {
                        let other = s.a.label();
                        s.save();
                        s.a.alu_imm(Op2::Cmp, false, TARGET, fixed as i32);
                        s.a.jcc(cc::NE, other);
                        s.a.store8_imm(at(RSP, FRAME_CELL), cell as u8 + 1);
                        s.a.bind(other);
                        s.clobber();
                    }
                }
            }
            if let Some(link) = link {
                s.set_guest(14, link);
            }
            if jump.exchange {
                Stub::Exchange
            } else {
                Stub::Branch
            }
        })
    }

    fn leave_to(&mut self, index: usize, body: impl FnOnce(&mut Self) -> Stub) -> Option<()> {
        let item = self.items[index];
        let skip = (item.condition != ALWAYS).then(|| self.a.label());
        if let Some(skip) = skip {
            self.skip_unless(item.condition, skip);
        }
        let before = self.flags;
        let pending = self.pending;
        let stub = body(self);
        let cell = self
            .cell(index)
            .filter(|_| paired(self.items, index).is_none());
        if pending > 0 {
            self.a.lea(false, S, at(S, -(pending as i32)));
        }
        self.leave_cell(
            stub,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Core,
                force: false,
                branch: true,
                last: Last::Ctx,
                pending: 0,
            },
            cell,
        )?;
        let Some(skip) = skip else {
            return Some(());
        };
        self.a.bind(skip);
        self.flags = before;
        let last = self.spend_fetched(item.cost, item.check, 0xF);
        self.after_known(index, last, SEQUENTIAL)
    }

    fn cold(&mut self) -> Option<()> {
        for slow in std::mem::take(&mut self.slows) {
            self.a.bind(slow.label);
            if let Some(block) = slow.block {
                self.spill(block.spill, false);
                self.a.mov_imm(VALUE, block.lists);
            }
            self.call(slow.stub, slow.word);
            if let Some(rd) = slow.rd {
                self.write(rd, RCX);
            }
            if let Some(block) = slow.block {
                self.spill(block.reload, true);
                if block.replaced {
                    self.write_back(block.base, block.moved)?;
                }
            }
            if let Some(exit) = slow.exit {
                self.a.jcc(cc::LE, exit);
            }
            self.a.jmp(slow.back);
        }
        for skip in std::mem::take(&mut self.skips) {
            let item = skip.item;
            self.a.bind(skip.label);
            self.pending = skip.pending;
            self.flags = skip.flags;
            let last = self.spend_fetched(item.cost, true, 0xF);
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
            } else {
                self.save();
            }
            if skip.nf {
                self.a.mov_imm(FETCH, SEQUENTIAL);
            }
            self.a.jmp(skip.join);
        }
        for site in std::mem::take(&mut self.sites) {
            self.a.bind(site.label);
            self.a.mov_imm(INFO, site.word);
            self.reach(Stub::Exit as u32, false);
        }
        if let Some(shared) = self.shared {
            self.a.bind(shared);
            self.a.rot(Rot::Shr, false, INFO, at_shift(self.v5) as u8);
            self.a.alu_imm(Op2::Or, false, INFO, left(0).word()? as i32);
            self.reach(Stub::Exit as u32, false);
        }
        for fetch in std::mem::take(&mut self.fetches) {
            self.a.bind(fetch.label);
            self.a.mov_imm(WORD, fetch.marker);
            self.a.mov_imm64(FETCH_PC, fetch.args);
            self.reach(Stub::FetchArm as u32, true);
            self.a.jmp(fetch.back);
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
    use super::super::super::fast::{Cache, Spec};
    use super::super::super::helpers::Helpers;
    use super::super::super::lookup::NO_KEY;
    use super::super::super::scan::{Literal, Op, classify, classify_thumb};
    use super::super::{Offsets, lower, lower_for};
    use super::runtime::OLD_LAST;
    use super::*;

    fn build(words: &[u32], thumb: bool) -> Vec<u8> {
        build_on::<true>(words, thumb).expect("emits")
    }

    fn build_on<const V5: bool>(words: &[u32], thumb: bool) -> Option<Vec<u8>> {
        build_with::<V5>(words, thumb, false)
    }

    fn build_with<const V5: bool>(words: &[u32], thumb: bool, pages: bool) -> Option<Vec<u8>> {
        build_full::<V5>(words, thumb, pages, false)
    }

    fn build_timed<const V5: bool>(words: &[u32], thumb: bool, timed: bool) -> Option<Vec<u8>> {
        build_full::<V5>(words, thumb, false, timed)
    }

    fn build_full<const V5: bool>(
        words: &[u32],
        thumb: bool,
        pages: bool,
        timed: bool,
    ) -> Option<Vec<u8>> {
        linked_full::<V5>(words, thumb, pages, timed).map(|linked| linked.bytes)
    }

    fn linked_full<const V5: bool>(
        words: &[u32],
        thumb: bool,
        pages: bool,
        timed: bool,
    ) -> Option<Linked> {
        linked_class::<V5>(words, thumb, pages, timed, 0)
    }

    fn linked_class<const V5: bool>(
        words: &[u32],
        thumb: bool,
        pages: bool,
        timed: bool,
        code: u32,
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
        let mut lowered = lower_for(&plan, V5, V5, timed, pages, X64::compiles).expect("lowers");
        lowered.pages = pages;
        lowered.code = code;
        block(&lowered, &plan, &Offsets::of::<V5>())
    }

    fn site(op: u8, target: u32) -> [u8; 5] {
        let [a, b, c, d] = (LINK | target).to_le_bytes();
        [op, a, b, c, d]
    }

    fn call_to(stub: Stub) -> [u8; 5] {
        site(0xE8, stub as u32)
    }

    fn jump_to(stub: Stub) -> [u8; 5] {
        site(0xE9, stub as u32)
    }

    #[test]
    fn a_thumb_bl_pair_owns_the_taken_cell() {
        let words = [0xF000, 0xF808];
        let ops: Vec<Op> = words
            .iter()
            .enumerate()
            .map(|(at, &word)| classify_thumb::<true>(word, 0x0200_0000 + 2 * at as u32))
            .collect();
        let fetch = vec![[2, 1]; ops.len()];
        let all = [0xF000, 0xF808, 0, 0];
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: true,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let lowered = lower_for(&plan, true, true, false, true, X64::compiles).expect("lowers");
        let Kind::Constant { value, .. } = lowered.items[0].kind else {
            panic!("the prefix lowers to a constant");
        };
        assert_eq!(paired(&lowered.items, 1), Some(value.wrapping_add(16)));
        assert_eq!(owners(&lowered.items, &plan)[0], Some(1));
        assert_eq!(paired(&lowered.items[1..], 0), None);
        assert!(block(&lowered, &plan, &Offsets::of::<true>()).is_some());
    }

    #[test]
    fn register_leaves_take_a_free_cell() {
        for (words, thumb, owned) in [
            (
                vec![0xE1A0_0000, 0xE12F_FF1E],
                false,
                [Some(1), None, None, None],
            ),
            (vec![0x4600, 0xBD00], true, [Some(1), Some(2), None, None]),
            (
                vec![0x0A00_0002, 0xE12F_FF1E],
                false,
                [Some(0), Some(1), None, None],
            ),
        ] {
            let size = if thumb { 2 } else { 4 };
            let ops: Vec<Op> = words
                .iter()
                .enumerate()
                .map(|(at, &word)| {
                    if thumb {
                        classify_thumb::<true>(word, 0x0200_0000 + at as u32 * size)
                    } else {
                        classify::<true>(word)
                    }
                })
                .collect();
            let fetch = vec![[2, 1]; ops.len()];
            let mut all = words.clone();
            all.extend([0, 0]);
            let plan = Plan {
                pc: 0x0200_0000,
                thumb,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower_for(&plan, true, true, false, true, X64::compiles).expect("lowers");
            assert_eq!(lowered.items.len(), 2, "{words:x?}");
            assert_eq!(owners(&lowered.items, &plan), owned, "{words:x?}");
            assert!(block(&lowered, &plan, &Offsets::of::<true>()).is_some());
        }
    }

    #[test]
    fn links_become_rel32_from_the_end_of_the_field() {
        let mut word = [0; 4];
        X64::link(&mut word, 0x1000, 0x1010).expect("in range");
        assert_eq!(i32::from_le_bytes(word), 0xC);
        X64::link(&mut word, 0x2000, 0x1000).expect("in range");
        assert_eq!(i32::from_le_bytes(word), -0x1004);
        assert_eq!(X64::link(&mut word, 0, 1 << 33), None);
    }

    #[test]
    fn every_stub_site_is_a_link_to_a_runtime_target() {
        let words = [0xE591_0000, 0xE581_0004, 0xE103_9099, 0xE1A0_F00E];
        let psr = [0xE10F_1000, 0xE14F_2000, 0xE381_1080, 0xE121_F001];
        let thumb = [0x2001, 0x1840, 0x6808, 0xB510, 0x4770];
        for code in [
            linked_full::<true>(&words, false, false, false),
            linked_full::<false>(&psr, false, true, false),
            linked_full::<false>(&thumb, true, false, true),
        ] {
            let code = code.expect("emits");
            assert!(!code.links.is_empty());
            for link in &code.links {
                let at = link.offset as usize;
                assert!(matches!(code.bytes[at - 1], 0xE8 | 0xE9));
                assert_eq!(code.bytes[at..at + 4], link.helper.to_le_bytes());
                assert!((link.helper & !LINK) as usize <= STUBS);
            }
        }
    }

    #[test]
    fn the_swap_target_is_the_jump_in_front_of_the_store_stub() {
        let runtime = runtime(&Setup {
            offsets: &Offsets::of::<true>(),
            helpers: &Helpers::of::<true, crate::gba::bus::Bus>(),
            chain: None,
            psr: 0,
            specs: &[],
            pages: false,
            v5: true,
            timed: false,
        })
        .expect("assembles");
        let targets = X64::targets(&runtime);
        assert_eq!(targets.len(), STUBS + 1);
        assert_eq!(targets[..STUBS], runtime.stubs);
        assert_eq!(runtime.bytes[targets[STUBS]], 0xE9);
    }

    fn fetch_calls(bytes: &[u8], stub: Stub) -> usize {
        let call = call_to(stub);
        bytes.windows(call.len()).filter(|w| *w == call).count()
    }

    fn timed_plan_of(words: &[u32], pc: u32, thumb: bool) -> (Vec<Op>, Vec<[u32; 2]>, Vec<u32>) {
        let ops: Vec<Op> = words
            .iter()
            .enumerate()
            .map(|(at, &word)| {
                if thumb {
                    classify_thumb::<false>(word, pc + 2 * at as u32)
                } else {
                    classify::<false>(word)
                }
            })
            .collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        (ops, fetch, all)
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
        let (ops, fetch, all) = timed_plan_of(&words, 0x0300_0000, false);
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
        let flat = block(&lowered, &plan, &Offsets::of::<false>())
            .expect("emits")
            .bytes;
        assert_eq!(fetch_calls(&flat, Stub::FetchFlat), 2);
        lowered.flat = false;
        let plain = block(&lowered, &plan, &Offsets::of::<false>())
            .expect("emits")
            .bytes;
        assert_eq!(fetch_calls(&plain, Stub::FetchFlat), 0);
        let untimed = lower(&plan, false, false, false).expect("lowers");
        assert!(
            flat.len()
                > block(&untimed, &plan, &Offsets::of::<false>())
                    .expect("emits")
                    .bytes
                    .len()
        );
    }

    #[test]
    fn a_run_ends_on_the_first_op_that_checks_ahead() {
        let runs = |words: &[u32]| {
            let (ops, fetch, all) = timed_plan_of(words, 0x0300_0000, false);
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
        let (ops, fetch, all) = timed_plan_of(&words, 0x0300_0000, false);
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
        let flat = block(&lowered, &plan, &offsets).expect("emits").bytes;
        assert_eq!(fetch_calls(&flat, Stub::FetchFlat), 2);
        assert_eq!(fetch_calls(&flat, Stub::Fetch), 2);
        lowered.flat = false;
        let plain = block(&lowered, &plan, &offsets).expect("emits").bytes;
        assert_eq!(fetch_calls(&plain, Stub::FetchFlat), 0);
        assert_eq!(fetch_calls(&plain, Stub::Fetch), 3);
    }

    #[test]
    fn timed_branch_runs_get_short_budget_copies() {
        let words = [0xE3A0_0001, 0x1A00_0004, 0xE280_0001, 0xE280_0002];
        let bytes = build_timed::<false>(&words, false, true).expect("emits");
        let tails = (word(&bytes, 0) >> 16) as usize;
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
        assert_eq!(tails, words.len());
        assert!(word(&bytes, 2) >> 27 > 0);
        assert_ne!(word(&bytes, tails + 2), word(&bytes, 2));
        for at in 0..words.len() {
            assert_eq!(word(&bytes, tails + at + 1) >> 27, 0);
            assert!(4 + (word(&bytes, tails + at + 1) & 0x7FFF) as usize <= bytes.len());
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
    fn timed_fetches_call_the_stub_of_the_block_mode() {
        for (words, thumb, stub) in [
            (
                vec![0xE3A0_0001, 0xE280_0001, 0xE12F_FF1E],
                false,
                Stub::FetchArm,
            ),
            (vec![0x2001, 0x3001, 0x4770], true, Stub::FetchThumb),
        ] {
            let (ops, fetch, all) = timed_plan_of(&words, 0x0800_0000, thumb);
            let plan = Plan {
                pc: 0x0800_0000,
                thumb,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let bytes = block(&lowered, &plan, &Offsets::of::<false>())
                .expect("emits")
                .bytes;
            assert_eq!(fetch_calls(&bytes, stub), words.len());
            assert_eq!(fetch_calls(&bytes, Stub::Fetch), 0);
        }
    }

    fn marked_fetches(
        marker: u32,
        pc: u32,
        thumb: bool,
        words: &[u32],
    ) -> (Vec<u32>, usize, Vec<u32>) {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let (ops, fetch, mut all) = timed_plan_of(words, pc, thumb);
        let pad = all.len() - 2;
        all[pad..].fill(0xE1A0_0000);
        let plan = Plan {
            pc,
            thumb,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let lowered = lower(&plan, false, false, true).expect("lowers");
        let bytes = block(&lowered, &plan, &offsets).expect("emits").bytes;
        let latches: Vec<u32> = fetch_args(&bytes)
            .into_iter()
            .filter(|&(word, _)| word == marker)
            .map(|(_, rdx)| (rdx >> 32) as u32)
            .collect();
        (latches, fetch_calls(&bytes, Stub::FetchArm), all)
    }

    fn fetch_args(bytes: &[u8]) -> Vec<(u32, u64)> {
        bytes
            .windows(15)
            .filter(|w| w[0] == 0xBE && w[5..7] == [0x48, 0xBA])
            .map(|w| {
                (
                    u32::from_le_bytes(w[1..5].try_into().unwrap()),
                    u64::from_le_bytes(w[7..].try_into().unwrap()),
                )
            })
            .collect()
    }

    #[test]
    fn fetches_hand_the_latch_in_the_pc_register() {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let quick = offsets.quick.expect("gba has quick offsets");
        let arm = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let thumb = [0x2001, 0x3001, 0x3002, 0x4770];
        for (pc, is_thumb, words, pad, stub) in [
            (0x0300_0000, false, &arm, 0xE1A0_0000, Stub::FetchArm),
            (0x0000_0100, false, &arm, 0xE1A0_0000, Stub::FetchArm),
            (0x0800_1000, false, &arm, 0xE1A0_0000, Stub::FetchArm),
            (0x0800_1000, true, &thumb, 0x46C0, Stub::FetchThumb),
        ] {
            let (ops, fetch, mut all) = timed_plan_of(words, pc, is_thumb);
            let at = all.len() - 2;
            all[at..].fill(pad);
            let plan = Plan {
                pc,
                thumb: is_thumb,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let bytes = block(&lowered, &plan, &offsets).expect("emits").bytes;
            let size = if is_thumb { 2 } else { 4 };
            let args = fetch_args(&bytes);
            assert!(!args.is_empty());
            assert_eq!(args.len(), fetch_calls(&bytes, stub));
            for (word, rdx) in args {
                let read = rdx as u32;
                let fetched = all[((read - pc) / size) as usize];
                let marker = match quick.rom_offset(read) {
                    Some(from) => from as u32,
                    None if read >> 24 == quick.iwram_page => IWRAM_FETCH,
                    None => BIOS_FETCH,
                };
                let latch = if is_thumb {
                    (fetched & 0xFFFF) * 0x0001_0001
                } else {
                    fetched
                };
                assert_eq!(word, marker);
                assert_eq!((rdx >> 32) as u32, latch);
            }
        }
    }

    #[test]
    fn arm_fetches_from_iwram_hand_the_stub_their_word() {
        let marked =
            |pc: u32, thumb: bool, words: &[u32]| marked_fetches(IWRAM_FETCH, pc, thumb, words);
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let (latches, calls, all) = marked(0x0300_0000, false, &words);
        assert!(calls > 0);
        assert_eq!(latches.len(), calls);
        assert!(latches.iter().all(|latch| all.contains(latch)));
        assert!(marked(0x0800_0000, false, &words).0.is_empty());
        assert!(marked(0x0000_0100, false, &words).0.is_empty());
        let thumb = [0x2001, 0x3001, 0x3002, 0x4770];
        assert!(marked(0x0300_0000, true, &thumb).0.is_empty());
    }

    #[test]
    fn arm_fetches_from_bios_hand_the_stub_their_word() {
        let marked =
            |pc: u32, thumb: bool, words: &[u32]| marked_fetches(BIOS_FETCH, pc, thumb, words);
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let (latches, calls, all) = marked(0x0000_0100, false, &words);
        assert!(calls > 0);
        assert_eq!(latches.len(), calls);
        assert!(latches.iter().all(|latch| all.contains(latch)));
        for pc in [0x0300_0000, 0x0000_4000, 0x0800_0000] {
            assert!(marked(pc, false, &words).0.is_empty());
        }
        let thumb = [0x2001, 0x3001, 0x3002, 0x4770];
        assert!(marked(0x0000_0100, true, &thumb).0.is_empty());
    }

    #[test]
    fn iwram_and_bios_arm_fetches_hit_inline_and_call_the_stub_from_cold_code() {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let words = [0xE3A0_0001, 0xE280_0001, 0xE280_0002, 0xE12F_FF1E];
        let bus = [0x48, 0x8B, 0x54, 0x24, FRAME_BUS as u8];
        let call = call_to(Stub::FetchArm);
        for (pc, inline) in [
            (0x0300_0000, true),
            (0x0000_0100, true),
            (0x0800_0000, false),
        ] {
            let (ops, fetch, mut all) = timed_plan_of(&words, pc, false);
            let pad = all.len() - 2;
            all[pad..].fill(0xE1A0_0000);
            let plan = Plan {
                pc,
                thumb: false,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let bytes = block(&lowered, &plan, &offsets).expect("emits").bytes;
            let calls: Vec<usize> = bytes
                .windows(call.len())
                .enumerate()
                .filter(|(_, w)| *w == call)
                .map(|(at, _)| at + call.len())
                .collect();
            assert_eq!(calls.len(), words.len());
            let loads = bytes.windows(bus.len()).filter(|w| *w == bus).count();
            assert_eq!(loads, if inline { words.len() } else { 0 });
            let back = |at: usize| match bytes.get(at) {
                Some(0xEB) => bytes.get(at + 1).is_some_and(|&rel| rel >= 0x80),
                Some(0xE9) => bytes
                    .get(at + 1..at + 5)
                    .is_some_and(|rel| i32::from_le_bytes(rel.try_into().unwrap()) < 0),
                _ => false,
            };
            assert_eq!(calls.iter().all(|&at| back(at)), inline);
        }
    }

    #[test]
    fn iwram_fetches_with_idles_hand_the_stub_their_word() {
        let offsets = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let marked = |pc: u32, thumb: bool, words: &[u32]| {
            let (ops, fetch, mut all) = timed_plan_of(words, pc, thumb);
            let pad = all.len() - 2;
            all[pad..].fill(0xE1A0_0000);
            let plan = Plan {
                pc,
                thumb,
                words: &all,
                ops: &ops,
                fetch: &fetch,
                literals: &[],
            };
            let lowered = lower(&plan, false, false, true).expect("lowers");
            let bytes = block(&lowered, &plan, &offsets).expect("emits").bytes;
            let shifted = IWRAM_IDLES | SHIFT_IDLES << IDLES_SHIFT;
            let mut counted = vec![0x81, 0xCE];
            counted.extend_from_slice(&IWRAM_IDLES.to_le_bytes());
            counted.extend_from_slice(&[0x48, 0xBA]);
            let latches: Vec<u32> = fetch_args(&bytes)
                .into_iter()
                .filter(|&(word, _)| word == shifted)
                .map(|(_, rdx)| rdx)
                .chain(
                    bytes
                        .windows(16)
                        .filter(|w| w[..8] == counted[..])
                        .map(|w| u64::from_le_bytes(w[8..].try_into().unwrap())),
                )
                .map(|rdx| (rdx >> 32) as u32)
                .collect();
            (latches, fetch_calls(&bytes, Stub::Fetch), all)
        };
        let words = [
            0xE3A0_0001,
            0xE080_0110,
            0xE000_0291,
            0xE3A0_1002,
            0xE12F_FF1E,
        ];
        let (latches, calls, all) = marked(0x0300_0000, false, &words);
        assert!(calls > 0);
        assert_eq!(latches.len(), calls);
        assert!(latches.iter().all(|latch| all.contains(latch)));
        let (latches, calls, _) = marked(0x0800_0000, false, &words);
        assert!(calls > 0);
        assert!(latches.is_empty());
        let thumb = [0x2001, 0x4088, 0x4348, 0x4770];
        assert!(marked(0x0300_0000, true, &thumb).0.is_empty());
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
        let wide = |pc: u32| {
            let (ops, fetch, mut all) = timed_plan_of(&words, pc, false);
            let pad = all.len() - 2;
            all[pad..].fill(0xE1A0_0000);
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
            let bytes = block(&lowered, &plan, &offsets).expect("emits").bytes;
            assert_eq!(fetch_calls(&bytes, Stub::FetchFlat), 2);
            fetch_args(&bytes)
                .into_iter()
                .filter(|&(word, _)| word & IWRAM_RUN != 0 && word >> RUN_CYCLES_SHIFT <= 3)
                .collect::<Vec<(u32, u64)>>()
        };
        let packed = |count: u32, last: u32, latch: u64| {
            (
                count << RUN_CYCLES_SHIFT | (count - 1) << IDLES_SHIFT | IWRAM_RUN,
                u64::from(last) | latch << 32,
            )
        };
        assert_eq!(
            wide(0x0300_0000),
            [
                packed(3, 0x0300_0010, 0xE3A0_3002),
                packed(2, 0x0300_001C, 0xE1A0_0000)
            ]
        );
        assert!(wide(0x0800_0000).is_empty());
        let quick = offsets.quick.expect("gba has quick offsets");
        assert_eq!(
            quick.iwram_latch(0x0300_0004, 0x0300_0008, true, [0x1111_2222, 0x3333_4444]),
            Some(0x2222_4444)
        );
        assert_eq!(
            quick.iwram_latch(0x0300_0004, 0x0300_0006, true, [0x1111_2222, 0x3333_4444]),
            Some(0x4444_2222)
        );
        assert_eq!(
            quick.iwram_latch(0x0300_7FFC, 0x0400_0000, false, [1, 2]),
            None
        );
    }

    #[test]
    fn timed_shifts_and_multiplies_fetch_with_idles() {
        let bytes = build_timed::<false>(&[0xE081_0332, 0xE000_0291, 0xE280_0001], false, true)
            .expect("emits");
        assert_eq!(fetch_calls(&bytes, Stub::Fetch), 2);
        assert_eq!(fetch_calls(&bytes, Stub::FetchArm), 1);
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
            assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
            assert_ne!(bytes, build_on::<false>(words, false).expect("emits"));
        }
        let bytes = build_timed::<false>(&[0x2001, 0x1840, 0x6808, 0xB510, 0x4770], true, true)
            .expect("emits");
        assert_eq!(word(&bytes, 0) & 0xFFFF, MARK);
    }

    #[test]
    fn the_timed_runtime_has_every_fetch_stub() {
        let quick = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert!(quick.quick.is_some());
        let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
        let fetches = [
            Stub::Fetch,
            Stub::FetchArm,
            Stub::FetchThumb,
            Stub::FetchFlat,
        ];
        let mut sizes = Vec::new();
        for offsets in [Offsets::of::<false>(), quick] {
            for chain in [None, Some(0x1234_5678)] {
                let runtime = runtime(&Setup {
                    offsets: &offsets,
                    helpers: &helpers,
                    chain,
                    psr: 0,
                    specs: &[],
                    pages: false,
                    v5: false,
                    timed: true,
                })
                .expect("assembles");
                for stub in fetches {
                    let at = runtime.stubs[stub as usize];
                    assert!(at > 0 && at < runtime.bytes.len());
                }
                sizes.push(runtime.bytes.len());
            }
        }
        assert!(sizes[2] > sizes[0] && sizes[3] > sizes[1]);
        let untimed = runtime(&Setup {
            offsets: &Offsets::of::<false>(),
            helpers: &helpers,
            chain: None,
            psr: 0,
            specs: &[],
            pages: false,
            v5: false,
            timed: false,
        })
        .expect("assembles");
        assert!(
            fetches
                .iter()
                .all(|&stub| untimed.stubs[stub as usize] == 0)
        );
        assert!(untimed.bytes.len() < sizes[0]);
    }

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[4 * at..4 * at + 4].try_into().unwrap())
    }

    fn heads(bytes: &[u8], count: usize) -> Vec<usize> {
        (0..count)
            .map(|at| 4 + (word(bytes, at + 1) & 0x7FFF) as usize)
            .collect()
    }

    #[test]
    fn a_block_starts_with_the_mark_and_one_entry_per_op() {
        let words = [
            0xE3A0_0001,
            0xE090_1002,
            0x1591_0004,
            0xE1B0_2183,
            0xE000_0291,
            0xE92D_4010,
            0x0A00_0010,
        ];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0), MARK);
        let heads = heads(&bytes, words.len());
        assert_eq!(heads[0], 4 * (words.len() + 1));
        assert!(heads.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(heads.iter().all(|&head| head < bytes.len()));
    }

    #[test]
    fn a_flag_setting_subtract_marks_the_next_head_inverted() {
        let words = [0xE250_0001, 0xE280_1001, 0x0A00_0010];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 1) & INVERTED_ENTRY, 0);
        assert_ne!(word(&bytes, 2) & INVERTED_ENTRY, 0);
    }

    #[test]
    fn short_jumps_make_the_block_smaller_than_the_first_pass() {
        let words = [
            0xE350_0000,
            0x1280_0001,
            0x0281_1001,
            0xE282_2001,
            0xEAFF_FFFA,
        ];
        let ops: Vec<Op> = words.iter().map(|&word| classify::<true>(word)).collect();
        let fetch = vec![[2, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
            literals: &[],
        };
        let lowered = lower_for(&plan, true, true, false, false, X64::compiles).expect("lowers");
        let offsets = Offsets::of::<true>();
        let long = pass(&lowered, &plan, &offsets, &[], Vec::new()).expect("first pass");
        let size = long.a.here();
        let shorts = long.a.shorts().expect("bound");
        assert!(shorts.iter().any(|&short| short));
        let bytes = block(&lowered, &plan, &offsets).expect("emits").bytes;
        assert!(bytes.len() < size);
    }

    #[test]
    fn block_transfers_call_gather_and_scatter_instead_of_step() {
        let words = [
            0xE8B0_0003,
            0xE8A1_0006,
            0x0891_0FFF,
            0xE92D_4010,
            0xE8B7_00C0,
            0xEAFF_FFF9,
        ];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0), MARK);
        let calls = |bytes: &[u8], stub: Stub| {
            let call = call_to(stub);
            bytes.windows(5).filter(|window| *window == call).count()
        };
        assert_eq!(calls(&bytes, Stub::Gather), 3);
        assert_eq!(calls(&bytes, Stub::Scatter), 2);
        assert_eq!(calls(&bytes, Stub::Step), 0);
        let pop = build(&[0xE8BD_8010], false);
        assert_eq!(calls(&pop, Stub::Collect), 1);
        assert_eq!(calls(&pop, Stub::Gather) + calls(&pop, Stub::Step), 0);
        let jump = jump_to(Stub::Return);
        assert!(pop.windows(5).any(|window| window == jump));
        let maybe = build(&[0x18BD_8010], false);
        assert_eq!(calls(&maybe, Stub::Collect), 1);
        assert!(maybe.windows(5).any(|window| window == jump));
    }

    #[test]
    fn timed_armv4_returns_gather_and_leave_through_the_branch_stub() {
        let stubs = |words: &[u32], thumb: bool, timed: bool| {
            let linked = linked_full::<false>(words, thumb, false, timed).expect("emits");
            let found: Vec<u32> = linked
                .links
                .iter()
                .map(|link| link.helper & !LINK)
                .collect();
            (found, linked.bytes)
        };
        let has = |found: &[u32], stub: Stub| found.contains(&(stub as u32));
        let stores_cell = |bytes: &[u8]| {
            bytes
                .windows(4)
                .any(|window| window == [0xC6, 0x44, 0x24, FRAME_CELL as u8])
        };
        let (pop, bytes) = stubs(&[0xE8BD_8070], false, true);
        assert!(has(&pop, Stub::Gather) && has(&pop, Stub::Branch));
        for stub in [Stub::Step, Stub::Collect, Stub::Return] {
            assert!(!has(&pop, stub));
        }
        assert!(!stores_cell(&bytes));
        let (maybe, _) = stubs(&[0x18BD_8070], false, true);
        for stub in [Stub::Gather, Stub::Branch, Stub::Exit] {
            assert!(has(&maybe, stub));
        }
        assert!(!has(&maybe, Stub::Collect));
        let (thumb, bytes) = stubs(&[0xB510, 0xBD10], true, true);
        for stub in [Stub::Scatter, Stub::Gather, Stub::Branch] {
            assert!(has(&thumb, stub));
        }
        for stub in [Stub::Step, Stub::Collect, Stub::Return] {
            assert!(!has(&thumb, stub));
        }
        assert!(!stores_cell(&bytes));
        let (user, _) = stubs(&[0xE8FD_8010], false, true);
        assert!(has(&user, Stub::Step) && has(&user, Stub::Exit));
        assert!(!has(&user, Stub::Gather));
        let (untimed, _) = stubs(&[0xE8BD_8070], false, false);
        assert!(has(&untimed, Stub::Step));
        assert!(!has(&untimed, Stub::Gather));
    }

    #[test]
    fn word_loads_into_pc_jump_to_the_load_pc_stub_instead_of_stepping() {
        let step = call_to(Stub::Step);
        let jump = jump_to(Stub::LoadPc);
        for word in [
            0xE49D_F004,
            0xE593_F000,
            0xE693_F002,
            0xE5B3_F008,
            0x049D_F004,
        ] {
            let bytes = build(&[0xE280_0001, word], false);
            assert!(!bytes.windows(5).any(|window| window == step));
            assert!(bytes.windows(jump.len()).any(|window| window == jump));
        }
    }

    #[test]
    fn exception_returns_call_the_restore_stub_and_step_only_when_it_refuses() {
        let step = call_to(Stub::Step);
        let call = call_to(Stub::Restore);
        for word in [0xE25E_F004, 0xE1B0_F00E] {
            let bytes = build(&[0xE280_0001, word], false);
            let at = bytes
                .windows(call.len())
                .position(|window| window == call)
                .expect("calls the restore stub");
            assert!(bytes[at..].windows(5).any(|window| window == step));
        }
    }

    #[test]
    fn swaps_call_the_entry_in_front_of_the_store_stub_instead_of_stepping() {
        let step = call_to(Stub::Step);
        let call = site(0xE8, STUBS as u32);
        for word in [0xE143_A09A, 0xE103_9099, 0xE101_0092] {
            let bytes = build(&[0xE280_0001, word], false);
            let has = |pattern: &[u8]| bytes.windows(pattern.len()).any(|w| w == pattern);
            assert!(!has(&step), "{word:08X}");
            assert!(has(&call), "{word:08X}");
        }
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
        let step = call_to(Stub::Step);
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0), MARK);
        assert!(!bytes.windows(5).any(|window| window == step));
        let bytes = build(&[0x4088, 0x40C8, 0x4108, 0x41C8], true);
        assert_eq!(word(&bytes, 0), MARK);
        assert!(!bytes.windows(5).any(|window| window == step));
    }

    #[test]
    fn clz_runs_in_the_block() {
        let bytes = build(&[0xE16F_0F11, 0xE16F_6F17, 0xE16F_2F16], false);
        let step = call_to(Stub::Step);
        assert!(!bytes.windows(5).any(|window| window == step));
        let scans = bytes
            .windows(2)
            .filter(|window| *window == [0x0F, 0xBD])
            .count();
        assert_eq!(scans, 3);
    }

    #[test]
    fn flag_setting_multiplies_run_in_the_block() {
        let step = call_to(Stub::Step);
        let count = |bytes: &[u8], pattern: &[u8]| {
            bytes
                .windows(pattern.len())
                .filter(|window| *window == pattern)
                .count()
        };
        let into_r0 = [0x89, 0xEB, 0x0F, 0xAF, 0xDF];
        let merge = [0x9E, 0xFF, 0xCB, 0xFF, 0xC3, 0x9F];
        for (words, thumb, multiply, merges) in [
            (&[0x4348, 0x4770][..], true, &[0x0F, 0xAF, 0xDD][..], 1),
            (&[0xE010_0291, 0xE12F_FF1E][..], false, &into_r0[..], 1),
            (
                &[0xE010_0291, 0x03A0_3001, 0xE354_0000, 0xE12F_FF1E][..],
                false,
                &into_r0[..],
                0,
            ),
        ] {
            let bytes = build(words, thumb);
            assert_eq!(count(&bytes, &step), 0);
            assert_eq!(count(&bytes, multiply), 1);
            assert_eq!(count(&bytes, &merge), merges);
            assert_eq!(count(&bytes, &[0x85, 0xDB]), 1 - merges);
        }
    }

    #[test]
    fn user_bank_blocks_emit_only_last() {
        let calls = |bytes: &[u8], stub: Stub| {
            let call = call_to(stub);
            bytes.windows(5).filter(|window| *window == call).count()
        };
        for (words, stub) in [
            ([0xE3A0_0001, 0xE8D0_7FFF], Stub::Gather),
            ([0xE3A0_0001, 0xE9E0_7FFC], Stub::Scatter),
        ] {
            let bytes = build(&words, false);
            assert_eq!(word(&bytes, 0), MARK);
            assert_eq!(calls(&bytes, stub), 2);
            assert_eq!(calls(&bytes, Stub::Step), 1);
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
    fn known_targets_and_the_fall_through_mark_their_link_cell() {
        let cell = |value: u8| [0xC6, 0x44, 0x24, FRAME_CELL as u8, value];
        let count = |bytes: &[u8], value: u8| {
            bytes
                .windows(5)
                .filter(|window| *window == cell(value))
                .count()
        };
        let cells = |words: &[u32]| {
            let bytes = build(words, false);
            (count(&bytes, 1), count(&bytes, 2), count(&bytes, 3))
        };
        assert_eq!(cells(&[0xE280_0001, 0xEAFF_FFFD]), (1, 0, 0));
        assert_eq!(cells(&[0xE250_0001, 0x1AFF_FFFD]), (1, 1, 0));
        assert_eq!(cells(&[0xE280_0001, 0xE12F_FF1E]), (1, 0, 0));
        assert_eq!(cells(&[0x0A00_0002, 0xE280_0001, 0xEAFF_FFFD]), (1, 1, 0));
        assert_eq!(cells(&[0x0A00_0002, 0x0A00_0001, 0xE12F_FF1E]), (1, 1, 1));
        assert_eq!(cells(&[0x0A00_0002, 0xE250_0001, 0x1AFF_FFFD]), (1, 1, 1));
    }

    #[test]
    fn armv4_paged_psr_transfers_go_through_the_psr_stub() {
        let step = call_to(Stub::Step);
        let call = call_to(Stub::Psr);
        let jump = jump_to(Stub::Psr);
        let words = [0xE10F_1000, 0xE14F_2000, 0xE381_1080, 0xE121_F001];
        let counts = |pages: bool| {
            let bytes = build_with::<false>(&words, false, pages).expect("emits");
            [&step[..], &call, &jump].map(|pattern| {
                bytes
                    .windows(pattern.len())
                    .filter(|window| *window == pattern)
                    .count()
            })
        };
        assert_eq!(counts(true), [0, 1, 1]);
        assert_eq!(counts(false), [3, 0, 0]);
    }

    #[test]
    fn psr_transfers_go_through_the_psr_stub() {
        let step = call_to(Stub::Step);
        let call = call_to(Stub::Psr);
        let jump = jump_to(Stub::Psr);
        for (words, kind, fields, reads) in [
            (
                &[0xE257_0032, 0xE10F_1000, 0xE10F_6000, 0xE128_F000][..],
                0,
                0b1000,
                0,
            ),
            (&[0xE14F_2000, 0xE169_F002][..], SPSR_WRITE, 0b1001, 1),
            (&[0xE321_F01F][..], 0, 0b0001, 0),
        ] {
            let bytes = build(words, false);
            let count = |pattern: &[u8]| {
                bytes
                    .windows(pattern.len())
                    .filter(|window| *window == pattern)
                    .count()
            };
            assert_eq!(count(&step), 0);
            assert_eq!(count(&call), reads);
            assert_eq!(count(&jump), 1);
            let at = bytes.windows(5).position(|window| window == jump).unwrap();
            assert_eq!(bytes[at - 5], 0xBE);
            let info = u32::from_le_bytes(bytes[at - 4..at].try_into().unwrap());
            assert_eq!(info & (SPSR_WRITE | SPSR_READ), kind);
            assert_eq!((info >> 8) & 0xF, fields);
            assert_eq!(info & 0xFF, words.len() as u32);
        }
    }

    #[test]
    fn cp15_cache_ops_jump_to_the_step_stub_and_dtcm_reads_lower_natively() {
        let step = call_to(Stub::Step);
        let jump = jump_to(Stub::Step);
        let flag = [0xC6, 0x02, 0x01];
        for (op, steps, jumps) in [
            (0xEE07_CF9A, 0, 1),
            (0xEE07_0F3E, 0, 1),
            (0xEE19_0F11, 0, 0),
            (0xEE19_6F11, 0, 0),
        ] {
            let bytes = build(&[0xE3A0_C000, 0xE281_1001, op], false);
            let count = |pattern: &[u8]| {
                bytes
                    .windows(pattern.len())
                    .filter(|window| *window == pattern)
                    .count()
            };
            assert_eq!(word(&bytes, 0), MARK);
            assert_eq!(count(&step), steps, "{op:08X}");
            assert_eq!(count(&jump), jumps, "{op:08X}");
            assert_eq!(count(&flag), 0, "{op:08X}");
            if let Some(at) = bytes.windows(5).position(|window| window == jump) {
                assert_eq!(bytes[at - 5], 0xBE, "{op:08X}");
                let info = u32::from_le_bytes(bytes[at - 4..at].try_into().unwrap());
                assert_ne!(info & OLD_LAST, 0, "{op:08X}");
            }
        }
        assert_eq!(step_word(0xFF, 0x1F, 0x3FF).unwrap() & OLD_LAST, 0);
    }

    #[test]
    fn registers_in_memory_update_their_slot_in_place() {
        let bytes = build(&[0xE286_6001, 0xE187_7001, 0xE1A0_8001, 0xEAFF_FFFE], false);
        let has = |pattern: &[u8]| bytes.windows(pattern.len()).any(|w| w == pattern);
        assert!(has(&[0x41, 0x83, 0x43, 0x18, 0x01]));
        assert!(has(&[0x41, 0x09, 0x6B, 0x1C]));
        assert!(has(&[0x41, 0x89, 0x6B, 0x20]));
        assert!(!has(&[0x41, 0x8B, 0x4B, 0x18]));
        let load = build(&[0xE591_0004, 0xEAFF_FFFE], false);
        assert!(load.windows(3).any(|w| w == [0x8D, 0x4D, 0x04]));
    }

    #[test]
    fn stores_leave_from_their_stub_and_loads_share_one_exit() {
        let words = [
            0xE591_0000,
            0xE591_2004,
            0xE583_0000,
            0xE583_2004,
            0xEAFF_FFFE,
        ];
        let bytes = build(&words, false);
        let [load, store] = [Stub::Load, Stub::Store].map(|s| s as u8);
        let calls: Vec<(usize, u32, u8)> = bytes
            .windows(10)
            .enumerate()
            .filter(|(_, w)| w[0] == 0xBE && w[5] == 0xE8 && w[7..10] == [0, 0, 0x80])
            .map(|(at, w)| {
                let word = u32::from_le_bytes(w[1..5].try_into().unwrap());
                (at + 10, word >> AT_SHIFT, w[6])
            })
            .collect();
        let tags: Vec<(u32, u8)> = calls.iter().map(|&(_, tag, slot)| (tag, slot)).collect();
        assert_eq!(tags, [(1, load), (2, load), (3, store), (4, store)]);
        for &(after, _, slot) in &calls {
            let next = if slot == load { after + 2 } else { after };
            let jumps = bytes[next] == 0x7E || bytes[next..next + 2] == [0x0F, 0x8E];
            assert_eq!(jumps, slot == load);
        }
        let shared = [0xC1, 0xEE, AT_SHIFT as u8];
        assert_eq!(bytes.windows(3).filter(|w| *w == shared).count(), 1);
        let runtime = runtime(&Setup {
            offsets: &Offsets::of::<true>(),
            helpers: &Helpers::of::<true, crate::gba::bus::Bus>(),
            chain: None,
            psr: 0,
            specs: &[],
            pages: false,
            v5: true,
            timed: false,
        })
        .expect("assembles");
        let mut a = Asm::new(Vec::new());
        a.alu_imm(Op2::Add, true, RSP, 8);
        a.byte(0xE9);
        let pop = a.finish().expect("assembles");
        let exit = runtime.stubs[Stub::Exit as usize] as i64;
        let direct = runtime
            .bytes
            .windows(pop.len() + 4)
            .enumerate()
            .any(|(at, w)| {
                let delta = i32::from_le_bytes(w[pop.len()..].try_into().unwrap());
                w[..pop.len()] == pop[..] && (at + w.len()) as i64 + delta as i64 == exit
            });
        assert!(direct);
    }

    #[test]
    fn paged_transfers_read_the_page_table_and_call_the_stub_only_from_cold_code() {
        let words = [
            0xE591_0000,
            0xE1D1_20B4,
            0xE5D1_3006,
            0xE583_0000,
            0xEAFF_FFFE,
        ];
        let bytes = build_with::<true>(&words, false, true).expect("emits");
        let classed = [1, 2, 3].map(|code| {
            linked_class::<false>(&words, false, true, false, code)
                .expect("emits")
                .bytes
        });
        let [load, store] = [Stub::Load, Stub::Store].map(|s| s as u8);
        for bytes in std::iter::once(&bytes).chain(&classed) {
            let last = heads(bytes, words.len())[words.len() - 1];
            let calls: Vec<(usize, u8)> = bytes
                .windows(10)
                .enumerate()
                .filter(|(_, w)| w[0] == 0xBE && w[5] == 0xE8 && w[7..10] == [0, 0, 0x80])
                .map(|(at, w)| (at, w[6]))
                .collect();
            let slots: Vec<u8> = calls.iter().map(|&(_, slot)| slot).collect();
            assert_eq!(slots, [load, load, load, store]);
            assert!(calls.iter().all(|&(at, _)| at > last));
        }
        let plain = build_on::<true>(&words, false).expect("emits");
        let v4 = build_with::<false>(&words, false, true).expect("emits");
        for (dest, paged) in [(RDX, 3), (REGS, 1)] {
            let mut a = Asm::new(Vec::new());
            a.load(true, dest, at(RSP, FRAME_FAST));
            let page = a.finish().expect("assembles");
            let reads = |bytes: &[u8]| bytes.windows(page.len()).filter(|w| *w == page).count();
            assert_eq!(reads(&bytes), paged);
            assert!(classed.iter().all(|bytes| reads(bytes) == paged));
            assert_eq!(reads(&plain), 0);
            assert_eq!(reads(&v4), 0);
        }
    }

    #[test]
    fn paged_blocks_read_the_page_table_and_call_the_stub_only_from_cold_code() {
        let words = [
            0xE891_0045,
            0xE92D_4070,
            0xE8B0_0006,
            0xE883_0080,
            0xEAFF_FFFE,
        ];
        let bytes = build_with::<true>(&words, false, true).expect("emits");
        let classed = [1, 2, 3].map(|code| {
            linked_class::<false>(&words, false, true, false, code)
                .expect("emits")
                .bytes
        });
        let [gather, scatter] = [Stub::Gather, Stub::Scatter].map(|s| s as u8);
        for bytes in std::iter::once(&bytes).chain(&classed) {
            let last = heads(bytes, words.len())[words.len() - 1];
            let calls: Vec<(usize, u8)> = bytes
                .windows(10)
                .enumerate()
                .filter(|(_, w)| w[0] == 0xBE && w[5] == 0xE8 && w[7..10] == [0, 0, 0x80])
                .map(|(at, w)| (at, w[6]))
                .collect();
            let slots: Vec<u8> = calls.iter().map(|&(_, slot)| slot).collect();
            assert_eq!(slots, [gather, scatter, gather, scatter]);
            assert!(calls.iter().all(|&(at, _)| at > last));
        }
        let plain = build_on::<true>(&words, false).expect("emits");
        let v4 = build_with::<false>(&words, false, true).expect("emits");
        let mut a = Asm::new(Vec::new());
        a.load(true, RDX, at(RSP, FRAME_FAST));
        let page = a.finish().expect("assembles");
        let reads = |bytes: &[u8]| bytes.windows(page.len()).filter(|w| *w == page).count();
        assert_eq!(reads(&bytes), 4);
        assert!(classed.iter().all(|bytes| reads(bytes) == 4));
        assert_eq!(reads(&plain), 0);
        assert_eq!(reads(&v4), 0);
    }

    #[test]
    fn thumb_blocks_emit() {
        let words = [0x2001, 0x1840, 0x6808, 0x4770];
        let bytes = build(&words, true);
        assert_eq!(word(&bytes, 0), MARK);
        assert!(
            heads(&bytes, words.len())
                .iter()
                .all(|&head| head < bytes.len())
        );
    }

    #[test]
    fn the_runtime_assembles_with_and_without_chaining() {
        for chain in [None, Some(0x1234_5678)] {
            let runtime = runtime(&Setup {
                offsets: &Offsets::of::<true>(),
                helpers: &Helpers::of::<true, crate::gba::bus::Bus>(),
                chain,
                psr: 0,
                specs: &[],
                pages: false,
                v5: true,
                timed: false,
            })
            .expect("assembles");
            assert!(runtime.stubs[Stub::LoadPc as usize] > 0);
            let store = runtime.stubs[Stub::Store as usize];
            assert_eq!(runtime.bytes[store - SWAP_ENTRY], 0xE9);
            let restore = runtime.stubs[Stub::Restore as usize];
            assert!(restore > 0 && restore < runtime.bytes.len());
            assert!(
                runtime
                    .stubs
                    .iter()
                    .enumerate()
                    .all(|(stub, &at)| stub >= Stub::Psr as usize
                        || (at > 0 && at < runtime.bytes.len()))
            );
        }
    }

    #[test]
    fn pages_add_inline_paths_to_the_memory_stubs() {
        const SPECS: [Spec; 2] = [
            Spec {
                len: 0x8000,
                code: true,
                cache: None,
                half: [1; 2],
                word: [1; 2],
            },
            Spec {
                len: 0x40_0000,
                code: true,
                cache: Some(Cache {
                    shift: 12,
                    bit: 1,
                    code: 2,
                    line: 31,
                    cycles: [1, 2],
                }),
                half: [8, 2],
                word: [9, 2],
            },
        ];
        let (offsets, helpers) = (
            Offsets::of::<true>(),
            Helpers::of::<true, crate::gba::bus::Bus>(),
        );
        let [plain, fast] = [false, true].map(|pages| {
            runtime(&Setup {
                offsets: &offsets,
                helpers: &helpers,
                chain: None,
                psr: 0,
                specs: &SPECS,
                pages,
                v5: true,
                timed: false,
            })
            .expect("assembles")
        });
        assert!(fast.bytes.len() > plain.bytes.len() + 300);
        assert!(
            fast.stubs
                .iter()
                .enumerate()
                .all(|(stub, &at)| stub >= Stub::Psr as usize || (at > 0 && at < fast.bytes.len()))
        );
    }

    #[test]
    fn only_items_before_a_line_start_check_the_budget() {
        let mut a = Asm::new(Vec::new());
        a.lea(false, RCX, at(S, -1));
        a.alu(Op2::Cmp, false, RCX, DEBT);
        let check = a.finish().expect("assembles");
        let checks = |count: usize| {
            let bytes = build(&vec![0xE591_0000; count], false);
            bytes
                .windows(check.len())
                .filter(|window| *window == check)
                .count()
        };
        assert_eq!(checks(32), 0);
        assert_eq!(checks(33), 1);
    }

    #[test]
    fn a_skipped_load_keeps_the_flags_image_for_the_join() {
        let mut a = Asm::new(Vec::new());
        a.lahf();
        a.setcc(cc::O, RAX);
        let save = a.finish().expect("assembles");
        let saves = |words: &[u32]| {
            let bytes = build_on::<false>(words, false).expect("emits");
            bytes
                .windows(save.len())
                .filter(|window| *window == save)
                .count()
        };
        let always = saves(&[0xE350_0001, 0xE592_1000, 0x0281_1001, 0xE281_1001]);
        let skipped = saves(&[0xE350_0001, 0x1592_1000, 0x0281_1001, 0xE281_1001]);
        assert_eq!(skipped, always + 1);
    }

    #[test]
    fn branch_exchange_and_return_stubs_hop_inline_only_with_a_chain_and_code_windows() {
        const SPEC: Spec = Spec {
            len: 0x8000,
            code: true,
            cache: None,
            half: [1; 2],
            word: [1; 2],
        };
        let (offsets, helpers) = (
            Offsets::of::<true>(),
            Helpers::of::<true, crate::gba::bus::Bus>(),
        );
        let mut a = Asm::new(Vec::new());
        a.store(false, at(REGS, 8), RDI);
        let fast = a.finish().expect("assembles");
        let starts = |specs: &[Spec], chain: Option<usize>, v5: bool, pages: bool| {
            let runtime = runtime(&Setup {
                offsets: &offsets,
                helpers: &helpers,
                chain,
                psr: 0,
                specs,
                pages,
                v5,
                timed: false,
            })
            .expect("assembles");
            [Stub::Branch, Stub::Exchange, Stub::Return]
                .map(|stub| runtime.bytes[runtime.stubs[stub as usize]..].starts_with(&fast))
        };
        assert_eq!(starts(&[SPEC], Some(1), true, false), [true; 3]);
        assert_eq!(starts(&[SPEC], None, true, false), [false; 3]);
        assert_eq!(starts(&[], Some(1), true, false), [false; 3]);
        assert_eq!(starts(&[], Some(1), false, false), [false; 3]);
        assert_eq!(starts(&[], Some(1), false, true), [true, true, false]);
        assert_eq!(starts(&[], None, false, true), [false; 3]);
    }

    #[test]
    fn fast_refills_store_where_they_landed() {
        const SPEC: Spec = Spec {
            len: 0x8000,
            code: true,
            cache: None,
            half: [1; 2],
            word: [1; 2],
        };
        let (offsets, helpers) = (
            Offsets::of::<true>(),
            Helpers::of::<true, crate::gba::bus::Bus>(),
        );
        let mut a = Asm::new(Vec::new());
        a.store(false, at(RDI, offsets.landed as i32), RDX);
        let landed = a.finish().expect("assembles");
        let stores = |specs: &[Spec]| {
            runtime(&Setup {
                offsets: &offsets,
                helpers: &helpers,
                chain: None,
                psr: 0,
                specs,
                pages: false,
                v5: true,
                timed: false,
            })
            .expect("assembles")
            .bytes
            .windows(landed.len())
            .filter(|window| *window == landed)
            .count()
        };
        assert_eq!(stores(&[]), 0);
        assert_eq!(stores(&[SPEC]), 8);
    }

    #[test]
    fn armv4_paged_branch_and_exchange_leaves_refill_from_the_page_table() {
        let (offsets, helpers) = (
            Offsets::of::<false>(),
            Helpers::of::<false, crate::gba::bus::Bus>(),
        );
        let store = |base: R, offset: i32| {
            let mut a = Asm::new(Vec::new());
            a.store(false, at(base, offset), RDX);
            a.finish().expect("assembles")
        };
        let landed = store(RDI, offsets.landed as i32);
        let hopped = store(REGS, offsets.landed as i32 - offsets.regs as i32);
        let stores = |pages: bool, timed: bool| {
            let bytes = runtime(&Setup {
                offsets: &offsets,
                helpers: &helpers,
                chain: Some(0x1234_5678),
                psr: 0,
                specs: &[],
                pages,
                v5: false,
                timed,
            })
            .expect("assembles")
            .bytes;
            [&landed, &hopped].map(|store| {
                bytes
                    .windows(store.len())
                    .filter(|window| window == store)
                    .count()
            })
        };
        assert_eq!(stores(false, false), [0, 0]);
        assert_eq!(stores(true, false), [4, 4]);
        assert_eq!(stores(true, true), [0, 0]);
    }

    #[test]
    fn only_timed_gba_branch_and_exchange_leaves_land_inline() {
        let clears = |v5: bool, offsets: &Offsets, timed: bool, pages: bool| {
            let mut a = Asm::new(Vec::new());
            a.store_imm(at(RDI, offsets.refetched as i32), 0);
            let cleared = a.finish().expect("assembles");
            let helpers = if v5 {
                Helpers::of::<true, crate::gba::bus::Bus>()
            } else {
                Helpers::of::<false, crate::gba::bus::Bus>()
            };
            runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: Some(0x1234_5678),
                psr: 0,
                specs: &[],
                pages,
                v5,
                timed,
            })
            .expect("assembles")
            .bytes
            .windows(cleared.len())
            .filter(|window| *window == cleared)
            .count()
        };
        let quick = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(clears(false, &quick, true, false), 4);
        assert_eq!(clears(false, &quick, false, false), 0);
        assert_eq!(clears(false, &quick, true, true), 0);
        assert_eq!(clears(false, &Offsets::of::<false>(), true, false), 0);
        let ds = Offsets::of_bus::<true, crate::gba::bus::Bus>();
        assert_eq!(clears(true, &ds, true, false), 0);
    }

    #[test]
    fn only_timed_gba_leaves_read_bios_targets_inline() {
        let mut a = Asm::new(Vec::new());
        let bios = std::mem::offset_of!(crate::jit::Fast, bios) as i32;
        a.load(true, RSI, at(RSI, bios));
        let read = a.finish().expect("assembles");
        let reads = |v5: bool, offsets: &Offsets, timed: bool, pages: bool| {
            let helpers = if v5 {
                Helpers::of::<true, crate::gba::bus::Bus>()
            } else {
                Helpers::of::<false, crate::gba::bus::Bus>()
            };
            runtime(&Setup {
                offsets,
                helpers: &helpers,
                chain: Some(0x1234_5678),
                psr: 0,
                specs: &[],
                pages,
                v5,
                timed,
            })
            .expect("assembles")
            .bytes
            .windows(read.len())
            .filter(|window| *window == read)
            .count()
        };
        let quick = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(reads(false, &quick, true, false), 2);
        assert_eq!(reads(false, &quick, false, false), 0);
        assert_eq!(reads(false, &quick, true, true), 0);
        assert_eq!(reads(false, &Offsets::of::<false>(), true, false), 0);
        let ds = Offsets::of_bus::<true, crate::gba::bus::Bus>();
        assert_eq!(reads(true, &ds, true, false), 0);
    }

    #[test]
    fn only_timed_gba_thumb_fetches_settle_inline() {
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let q = gba.quick.expect("the GBA bus has quick fetches");
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
            [q.paused, q.active, q.force_n].map(|field| {
                let mut a = Asm::new(Vec::new());
                if field == q.active {
                    a.store_imm_wide(at(R11, field), 0x0101);
                } else {
                    a.store8_imm(at(R11, field), 1);
                }
                let set = a.finish().expect("assembles");
                bytes.windows(set.len()).filter(|w| *w == set).count()
            })
        };
        assert_eq!(sets(&gba, true), [1, 1, 1]);
        assert_eq!(sets(&gba, false), [0, 0, 0]);
        assert_eq!(sets(&Offsets::of::<false>(), true), [0, 0, 0]);
    }

    #[test]
    fn only_timed_gba_loads_read_rom_inline() {
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        let q = gba.quick.expect("the GBA bus has quick fetches");
        let mut a = Asm::new(Vec::new());
        a.test_imm(RCX, q.rom_start);
        let tested = a.finish().expect("assembles");
        let mut a = Asm::new(Vec::new());
        a.store_imm(at(RDI, q.active), 0);
        let stopped = a.finish().expect("assembles");
        let found = |offsets: &Offsets, timed: bool| {
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
            [&tested, &stopped].map(|code| {
                bytes
                    .windows(code.len())
                    .filter(|window| window == code)
                    .count()
            })
        };
        assert_eq!(found(&gba, true), [1, 1]);
        assert_eq!(found(&gba, false), [0, 0]);
        assert_eq!(found(&Offsets::of::<false>(), true), [0, 0]);
    }

    fn gba_runtime() -> Vec<u8> {
        let helpers = Helpers::of::<false, crate::gba::bus::Bus>();
        runtime(&Setup {
            offsets: &Offsets::of_bus::<false, crate::gba::bus::Bus>(),
            helpers: &helpers,
            chain: None,
            psr: 0,
            specs: &[],
            pages: false,
            v5: false,
            timed: true,
        })
        .expect("assembles")
        .bytes
    }

    fn found(bytes: &[u8], build: impl FnOnce(&mut Asm)) -> usize {
        let mut a = Asm::new(Vec::new());
        build(&mut a);
        let code = a.finish().expect("assembles");
        bytes
            .windows(code.len())
            .filter(|window| *window == code.as_slice())
            .count()
    }

    #[test]
    fn gba_ram_loads_read_the_wait_from_memory_and_add_the_clock_in_place() {
        let q = Offsets::of_bus::<false, crate::gba::bus::Bus>()
            .quick
            .expect("the GBA bus has quick fetches");
        let meta = offset_of!(Page, meta) as i32;
        let bytes = gba_runtime();
        let tested = found(&bytes, |a| a.alu_mem_imm_wide(Op2::Cmp, at(RAX, meta), 0));
        assert_eq!(tested, 2);
        assert_eq!(
            found(&bytes, |a| a.load8(false, R8, at(RAX, meta + 1))),
            tested
        );
        assert_eq!(
            found(&bytes, |a| a.load8(false, R8, at(RAX, meta))),
            2 * tested
        );
        assert_eq!(
            found(&bytes, |a| a.alu_store_wide(Op2::Add, at(RDI, q.clock), R8)),
            3 * tested
        );
        let old_wait = found(&bytes, |a| {
            a.mov(true, R8, R11);
            a.rot(Rot::Shr, true, R8, 8);
            a.extend8(false, R8, R8);
        });
        assert_eq!(old_wait, 0);
        let reloaded = found(&bytes, |a| {
            a.store(false, at(RDI, q.open_iwram), RDX);
            a.load(false, R11, at(RDI, q.open_iwram));
            a.store(false, at(RDI, q.open_last), R11);
        });
        assert_eq!(reloaded, 0);
        let latched = found(&bytes, |a| {
            a.store(false, at(RDI, q.open_iwram), RDX);
            a.store(false, at(RDI, q.open_last), RDX);
        });
        assert_eq!(latched, tested);
    }

    #[test]
    fn gba_ram_loads_decide_the_leave_with_one_setcc() {
        let bytes = gba_runtime();
        let room = |a: &mut Asm| {
            a.mov(false, R11, S);
            a.alu(Op2::Sub, false, R11, DEBT);
            a.extend8(false, RAX, RSI);
            a.rot(Rot::Shr, false, RAX, 3);
        };
        let quick = found(&bytes, |a| {
            room(a);
            a.alu(Op2::Xor, false, RDX, RDX);
            a.alu(Op2::Cmp, false, R11, RAX);
            a.setcc(cc::G, RDX);
            a.test(RDX, RDX);
        });
        assert!(quick > 0);
        let slow = found(&bytes, |a| {
            room(a);
            a.alu(Op2::Cmp, false, R11, RAX);
            a.mov_imm(RAX, 1);
            a.cmov(cc::LE, RDX, RAX);
            a.alu_imm(Op2::Xor, false, RDX, 1);
        });
        assert!(slow > 0);
        let decoded = found(&bytes, |a| {
            a.mov(false, RAX, RSI);
            a.rot(Rot::Shr, false, RAX, 3);
            a.alu_imm(Op2::And, false, RAX, 0x1F);
        });
        assert_eq!(decoded, 0);
    }

    #[test]
    fn gba_quick_branches_fold_the_clock_and_wait_and_restart_with_wide_stores() {
        let q = Offsets::of_bus::<false, crate::gba::bus::Bus>()
            .quick
            .expect("the GBA bus has quick fetches");
        assert!(q.packed());
        let bit = q.prefetch_bit.trailing_zeros();
        let wait = q.wait_bits + (bit / 8) as i32;
        let bytes = gba_runtime();
        assert_eq!(
            found(&bytes, |a| a.alu_mem_imm_wide(
                Op2::Add,
                at(RSI, q.clock),
                2
            )),
            4
        );
        assert_eq!(
            found(&bytes, |a| a.test_mem8(at(RSI, wait), 1 << (bit % 8))),
            6
        );
        assert_eq!(found(&bytes, |a| a.load8(false, R9, at(RSI, wait))), 0);
        for field in [q.stuck, q.flowing] {
            assert_eq!(found(&bytes, |a| a.test_mem8(at(RSI, field), 0xFF)), 2);
        }
        let restart = move |base: R| {
            move |a: &mut Asm| {
                a.store_imm_wide(at(base, q.count), 0);
                a.store_imm(at(base, q.parked), 0);
                a.store_imm_wide(at(base, q.active), 0x0101);
            }
        };
        assert_eq!(found(&bytes, restart(RSI)), 2);
        assert_eq!(found(&bytes, restart(R11)), 1);
        let stopped = move |base: R| {
            move |a: &mut Asm| {
                for zero in [q.head, q.count, q.parked] {
                    a.store_imm_wide(at(base, zero), 0);
                }
                a.store_imm(at(base, q.active), 0);
                a.store8_imm(at(base, q.stuck), 0);
                a.store8_imm(at(base, q.following), 0);
            }
        };
        assert_eq!(found(&bytes, stopped(RDI)), 1);
        assert_eq!(found(&bytes, stopped(R11)), 1);
        for field in [q.paused, q.following, q.force_n] {
            assert_eq!(found(&bytes, |a| a.store8_imm(at(RSI, field), 0)), 0);
        }
    }

    #[test]
    fn only_timed_gba_loads_read_rom_literals_inline() {
        let mut a = Asm::new(Vec::new());
        a.mov(true, RCX, RSI);
        a.rot(Rot::Shr, true, RCX, 32);
        let unpacked = a.finish().expect("assembles");
        let found = |offsets: &Offsets, timed: bool| {
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
            bytes
                .windows(unpacked.len())
                .filter(|window| *window == unpacked.as_slice())
                .count()
        };
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(found(&gba, true), 1);
        assert_eq!(found(&gba, false), 0);
        assert_eq!(found(&Offsets::of::<false>(), true), 0);
    }

    fn literal_loads(
        pc: u32,
        thumb: bool,
        words: &[u32],
        literals: &[Literal],
        offsets: &Offsets,
        timed: bool,
    ) -> usize {
        let (ops, fetch, mut all) = timed_plan_of(words, pc, thumb);
        let pad = all.len() - 2;
        all[pad..].fill(0xE1A0_0000);
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
        bytes
            .windows(10)
            .filter(|w| w[..2] == [0x48, 0xBE])
            .filter(|w| {
                let low = u32::from_le_bytes(w[2..6].try_into().unwrap());
                let high = u32::from_le_bytes(w[6..].try_into().unwrap());
                low & 7 == (ROM_LITERAL | 2) && high == 0xDEAD_BEEF
            })
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
        let mut a = Asm::new(Vec::new());
        a.bt(RDI, RCX);
        let tested = a.finish().expect("assembles");
        let mut a = Asm::new(Vec::new());
        a.bsr(RAX, RDX);
        let highest = a.finish().expect("assembles");
        let found = |offsets: &Offsets, timed: bool| {
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
            [&tested, &highest].map(|code| {
                bytes
                    .windows(code.len())
                    .filter(|window| window == code)
                    .count()
            })
        };
        let gba = Offsets::of_bus::<false, crate::gba::bus::Bus>();
        assert_eq!(found(&gba, true), [1, 1]);
        assert_eq!(found(&gba, false), [0, 0]);
        assert_eq!(found(&Offsets::of::<false>(), true), [0, 0]);
    }

    #[test]
    fn armv4_blocks_emit() {
        let calls = |bytes: &[u8], stub: Stub| {
            let call = call_to(stub);
            bytes.windows(5).filter(|window| *window == call).count()
        };
        let blocks: [&[u32]; 4] = [
            &[0xE3A0_0001, 0xE090_1002, 0x1591_0004, 0xE1B0_2183],
            &[0xE000_0291, 0xE020_3291, 0xE081_0392, 0xE0E1_0392],
            &[0xE8A1_0006, 0xE8A2_0006, 0xE8B1_0006, 0xE92D_4010],
            &[0xE3A0_0001, 0x0A00_0010],
        ];
        for words in blocks {
            let bytes = build_on::<false>(words, false).expect("emits");
            assert_eq!(word(&bytes, 0), MARK);
            assert_eq!(calls(&bytes, Stub::Step), 0);
        }
        let counted = |bytes: &[u8]| {
            bytes
                .windows(3)
                .filter(|window| *window == [0x0F, 0xBD, 0xF6])
                .count()
        };
        let multiplies = build_on::<false>(blocks[1], false).expect("emits");
        assert_eq!(counted(&multiplies), 4);
        assert_eq!(counted(&build(blocks[1], false)), 0);
        let transfers = build_on::<false>(blocks[2], false).expect("emits");
        assert_eq!(calls(&transfers, Stub::Scatter), 3);
        assert_eq!(calls(&transfers, Stub::Gather), 1);
        let bumped = |bytes: &[u8]| {
            let bump = bytes
                .windows(3)
                .position(|window| window == [0x8D, 0x7F, 0x08]);
            let call = call_to(Stub::Scatter);
            let scatter = bytes.windows(5).position(|window| window == call);
            bump.zip(scatter).map(|(bump, scatter)| bump < scatter)
        };
        let words = [0xE8A2_0006];
        assert_eq!(
            bumped(&build_on::<false>(&words, false).expect("emits")),
            Some(true)
        );
        assert_eq!(bumped(&build(&words, false)), Some(false));
        let pop = [0xE8BD_8010, 0, 0];
        let ops = [classify::<true>(pop[0])];
        let plan = Plan {
            pc: 0x0380_0000,
            thumb: false,
            words: &pop,
            ops: &ops,
            fetch: &[[2, 1]],
            literals: &[],
        };
        let lowered = lower(&plan, false, false, false).expect("lowers");
        assert!(block(&lowered, &plan, &Offsets::of::<false>()).is_none());
        let thumb =
            build_on::<false>(&[0x2001, 0x1840, 0x6808, 0xB510, 0x4770], true).expect("emits");
        assert_eq!(word(&thumb, 0), MARK);
        for (chain, pages) in [(None, false), (Some(0x1234_5678), false), (None, true)] {
            let runtime = runtime(&Setup {
                offsets: &Offsets::of::<false>(),
                helpers: &Helpers::of::<false, crate::gba::bus::Bus>(),
                chain,
                psr: 0,
                specs: &[],
                pages,
                v5: false,
                timed: false,
            })
            .expect("assembles");
            assert!(
                runtime
                    .stubs
                    .iter()
                    .enumerate()
                    .all(|(stub, &at)| stub >= Stub::Psr as usize
                        || (at > 0 && at < runtime.bytes.len()))
            );
        }
    }

    #[test]
    fn a_hop_carries_target_inversion_and_pending() {
        let entry = entry_word(0x123 | INVERTED_ENTRY, 5, 3).expect("fits");
        let hop = X64::hop(0x7000_0000, entry);
        assert_eq!(hop & HOP_TARGET, 0x7000_0123);
        assert_eq!(hop >> HOP_INVERTED & 1, 1);
        assert_eq!(hop >> 48 & 0x7FF, 5);
        assert_eq!(X64::inverted(entry), 0x100);
        let plain = X64::hop(0x7000_0000, entry_word(0x40, 0, 0).expect("fits"));
        assert_eq!(plain, 0x7000_0040);
        assert_eq!(X64::inverted(entry_word(0x40, 0, 0).expect("fits")), 0);
    }

    #[test]
    fn only_arm7_hops_check_the_ldm_glitch() {
        let checks = |v5: bool, offsets: &Offsets, helpers: &Helpers| {
            let runtime = runtime(&Setup {
                offsets,
                helpers,
                chain: Some(0x1234_5678),
                psr: 0,
                specs: &[],
                pages: false,
                v5,
                timed: false,
            })
            .expect("assembles");
            let mut a = Asm::new(Vec::new());
            a.load8(false, R11, at(RDX, offsets.ldm_glitch as i32));
            let check = a.finish().expect("assembles");
            runtime
                .bytes
                .windows(check.len())
                .filter(|window| *window == check)
                .count()
        };
        let arm7 = checks(
            false,
            &Offsets::of::<false>(),
            &Helpers::of::<false, crate::gba::bus::Bus>(),
        );
        let arm9 = checks(
            true,
            &Offsets::of::<true>(),
            &Helpers::of::<true, crate::gba::bus::Bus>(),
        );
        assert!(arm7 > 0);
        assert_eq!(arm9, 0);
    }

    #[test]
    fn untimed_branch_and_exchange_hops_keep_their_fetch_cost_and_exchange_checks_the_key() {
        const SPEC: Spec = Spec {
            len: 0x8000,
            code: true,
            cache: None,
            half: [1; 2],
            word: [1; 2],
        };
        let counts =
            |v5: bool, timed: bool, specs: &[Spec], offsets: &Offsets, helpers: &Helpers| {
                let runtime = runtime(&Setup {
                    offsets,
                    helpers,
                    chain: Some(0x1234_5678),
                    psr: 0,
                    specs,
                    pages: !v5,
                    v5,
                    timed,
                })
                .expect("assembles");
                let mut keyed = Asm::new(Vec::new());
                keyed.mov(false, RDX, RCX);
                keyed.alu_imm(Op2::Xor, false, RDX, NO_KEY as i32);
                let mut kept = Asm::new(Vec::new());
                kept.store(false, at(RDI, offsets.hop as i32 + 4), R14);
                [keyed, kept].map(|a| {
                    let check = a.finish().expect("assembles");
                    runtime
                        .bytes
                        .windows(check.len())
                        .filter(|window| *window == check)
                        .count()
                })
            };
        let arm9 = counts(
            true,
            false,
            &[SPEC],
            &Offsets::of::<true>(),
            &Helpers::of::<true, crate::gba::bus::Bus>(),
        );
        let arm7 = counts(
            false,
            false,
            &[],
            &Offsets::of::<false>(),
            &Helpers::of::<false, crate::gba::bus::Bus>(),
        );
        let gba = counts(
            false,
            true,
            &[],
            &Offsets::of::<false>(),
            &Helpers::of::<false, crate::gba::bus::Bus>(),
        );
        assert_eq!(arm9, [1, 2]);
        assert_eq!(arm7, [1, 2]);
        assert_eq!(gba, [0, 0]);
    }
}
