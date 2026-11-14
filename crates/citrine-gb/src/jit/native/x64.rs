mod asm;
mod runtime;

use runtime::{
    CYCLES_MASK, IMAGE_MASK, IMAGE_TO_NZCV, NZCV_MASK, PC_SLOT, SPSR_READ, SPSR_WRITE, runtime,
};

use super::super::lookup::LINKS;
use super::super::scan::{Block, Jump, Multiply, Offset, Plan, Target, Transfer, Width};
use super::{
    ALWAYS, Alu, Backend, CODE_CLASS, FLAG_C, FLAG_N, FLAG_V, FLAG_Z, Fetch, Info, Item, Kind,
    Last, Linked, Lowered, MARK, MAX_COST, Offsets, Operand, PENDING_LIMIT, Runtime, Setup, Stub,
    block_word, entry_word, link_owners, memory_word, step_word,
};
use crate::gba::cpu::psr::MODE_MASK;
use asm::{
    Alu as Op2, Asm, Label, Mem, R, R8, R9, R10, R11, R12, R13, R14, R15, RAX, RBP, RBX, RCX, RDI,
    RDX, RSI, RSP, Rot, at, cc, indexed,
};

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
const KEEP: u32 = 0xFF;
const PC: u32 = 15;
const NONSEQUENTIAL: u32 = 0;
const SEQUENTIAL: u32 = 1;
const INVERTED_ENTRY: u32 = 1 << 15;
const CODE_LIMIT: usize = 0x7FFF;
const AT_SHIFT: u32 = 26;
const AT_SHIFT_V4: u32 = 28;
const _: () = assert!(AT_SHIFT_V4 >= CODE_CLASS + 2);
const IMAGE_CARRY: u32 = 8;
const FIQ_MODE: u32 = 0x11;
const BANKED_MODES: u32 = 1 << 0x12 | 1 << 0x13 | 1 << 0x17 | 1 << 0x1B;
const BANK_SLOTS: u32 = 2 << 4 | 1 << 14 | 3 << 22;

const fn stub_slot(stub: Stub) -> i32 {
    8 * stub as i32
}

const FRAME_STUBS: i32 = Stub::Psr as i32 + 1;
const FRAME_CELL: i32 = 8 * FRAME_STUBS;
const FRAME_CORE: i32 = 16 * ((FRAME_STUBS + 2) / 2);
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
    pending: u32,
    flags: Flags,
    nf: bool,
}

struct Site {
    label: Label,
    word: u32,
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
    shared: Option<Label>,
    owners: [Option<usize>; LINKS],
    cpsr: i32,
    user_high: i32,
    banked: i32,
    v5: bool,
    code: u32,
}

pub(super) fn at_shift(v5: bool) -> u32 {
    if v5 { AT_SHIFT } else { AT_SHIFT_V4 }
}

pub struct X64;

impl Backend for X64 {
    const TIMED: bool = false;

    fn block(lowered: &Lowered, plan: &Plan, offsets: &Offsets) -> Option<Linked> {
        block(lowered, plan, offsets).map(Linked::plain)
    }

    fn runtime(setup: &Setup) -> Option<Runtime> {
        runtime(setup)
    }

    fn compiles(kind: Kind) -> bool {
        !matches!(
            kind,
            Kind::Swap(_) | Kind::Cp15 { load: Some(_) } | Kind::Restore { .. } | Kind::LoadPc(_)
        )
    }

    fn hop(table: usize, entry: u32) -> u64 {
        let target = (table + (entry as usize & CODE_LIMIT)) as u64;
        let inverted = u64::from(entry & INVERTED_ENTRY != 0) << HOP_INVERTED;
        target | inverted | u64::from((entry >> 16) & 0x7FF) << 48
    }
}

pub fn block(lowered: &Lowered, plan: &Plan, offsets: &Offsets) -> Option<Vec<u8>> {
    if lowered.items.iter().any(|item| item.condition > ALWAYS) {
        return None;
    }
    let first = pass(lowered, plan, offsets, Vec::new())?;
    let shorts = first.a.shorts()?;
    let second = pass(lowered, plan, offsets, shorts)?;
    let count = lowered.items.len();
    let Lower {
        a,
        heads,
        entries,
        items,
        ..
    } = second;
    let offsets: Vec<usize> = heads
        .iter()
        .map(|&head| a.bound(head))
        .collect::<Option<_>>()?;
    let mut bytes = a.finish()?;
    if bytes.len() > CODE_LIMIT {
        return None;
    }
    for (index, offset) in offsets.into_iter().enumerate() {
        let (pending, inverted) = entries[index];
        let offset = (offset - 4) as u32 | if inverted { INVERTED_ENTRY } else { 0 };
        let word = entry_word(offset, pending, items[index].entry)?;
        let at = 4 * (index + 1);
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    debug_assert_eq!(heads.len(), count);
    Some(bytes)
}

fn pass<'a>(
    lowered: &'a Lowered,
    plan: &'a Plan<'a>,
    offsets: &Offsets,
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
        shared: None,
        owners: link_owners(&lowered.items, plan),
        cpsr: from_regs(offsets.cpsr),
        user_high: from_regs(offsets.user_high),
        banked: from_regs(offsets.banked),
        v5: lowered.v5,
        code: if lowered.v5 { 0 } else { lowered.code },
    };
    lower.a.word(MARK);
    for _ in 0..count {
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
    lower.cold()?;
    Some(lower)
}

fn stepped(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Step | Kind::Exit | Kind::Msr { .. } | Kind::UserBlock(_) | Kind::Cp15 { load: None }
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
            self.a.store8_imm(at(RSP, FRAME_CELL), 1 << cell);
        }
        self.a.mov_imm(INFO, info.word()?);
        self.a.jmp_mem(at(RSP, stub_slot(stub)));
        Some(())
    }

    fn call(&mut self, stub: Stub, packed: u32) {
        self.a.mov_imm(WORD, packed);
        self.a.call_mem(at(RSP, stub_slot(stub)));
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
        if stepped(next.kind) {
            let value = match fetch {
                Fetch::Known(access) => access,
                Fetch::Core => KEEP,
            };
            self.a.mov_imm(FETCH, value);
        }
    }

    fn op(&mut self, index: usize) -> Option<()> {
        let head = self.heads[index];
        self.a.bind(head);
        self.entries
            .push((self.pending, self.flags.host == Some(true)));
        let item = self.items[index];
        match item.kind {
            Kind::Swap(_) | Kind::LoadPc(_) | Kind::Restore { .. } => None,
            Kind::Alu(alu) => {
                self.conditional(index, item.condition, |s| s.alu(index, alu));
                let last = self.spend(item.spend?, item.check, item.live);
                let fetch = if alu.shifted() {
                    NONSEQUENTIAL
                } else {
                    SEQUENTIAL
                };
                self.after_known(index, last, fetch)
            }
            Kind::Constant { rd, value } => {
                match self.home(rd) {
                    Some(reg) => self.a.mov_imm(reg, value),
                    None => self.a.store_imm(at(REGS, 4 * rd as i32), value),
                }
                let last = self.spend(item.cost, item.check, item.live);
                self.after_known(index, last, SEQUENTIAL)
            }
            Kind::Multiply(multiply) => {
                let nz = multiply.set_flags && item.live & (FLAG_N | FLAG_Z) != 0;
                let merge = nz && item.live & (FLAG_C | FLAG_V) != 0;
                if !nz || merge {
                    self.keep(item.live);
                }
                if item.spend.is_none() {
                    self.multiply_cycles(index, multiply)?;
                }
                let result = self.multiply(multiply);
                if nz {
                    self.multiply_flags(result, merge, item.live & FLAG_C != 0);
                } else {
                    self.clobber();
                }
                let last = match item.spend {
                    Some(spend) => self.spend(spend, item.check, item.live),
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
            Kind::Cp15 { .. } => self.cp15(index),
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
        let fixed = i32::try_from(1 + extra + self.plan.fetch[index][0]).ok()?;
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

    fn transfer(&mut self, index: usize, transfer: Transfer) -> Option<()> {
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
        let packed = memory_word(
            transfer.kind(),
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
        self.call(stub, packed);
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
        if block.loads_pc() && !self.v5 {
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
        if !block.load {
            for &guest in &registers {
                if let (Some(reg), true) = (self.home(guest), kept & 1 << guest == 0) {
                    self.a.store(false, at(REGS, 4 * guest as i32), reg);
                }
            }
        }
        let returns = block.loads_pc();
        let (stub, lists) = match (block.load, returns) {
            (true, true) => (Stub::Collect, list | targets << 16),
            (true, false) => (Stub::Gather, list | targets << 16),
            (false, _) => (Stub::Scatter, list),
        };
        self.a.mov_imm(VALUE, lists);
        let count = registers.len() as u32;
        let packed = if returns {
            block_word(count, 0, 0, self.pending)?
        } else {
            block_word(count, item.ahead, self.charged(index), self.pending)? | self.code
        };
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
        self.a.mov(false, VALUE, RCX);
        self.a.alu_imm(Op2::And, false, VALUE, CYCLES_MASK);
        self.a.load(false, TARGET, at(REGS, PC_SLOT));
        self.leave(
            Stub::Return,
            Info {
                at: index as u32 + 1,
                fetch: Fetch::Core,
                force: false,
                branch: true,
                last: Last::Ctx,
                pending: 0,
            },
        )?;
        let Some(skip) = skip else {
            return Some(());
        };
        self.a.bind(skip);
        self.flags = before;
        self.pending = pending;
        let last = self.spend(self.items[index].cost, true, 0xF);
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
            .is_some_and(|next| stepped(next.kind));
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
            if self.items[index].check {
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
        if self.last(index) {
            return self.leave(Stub::Exit, info);
        }
        let site = self.site(info)?;
        self.a.jcc(cc::NE, site);
        if item.check {
            self.check(info.at, 0, info)?;
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
        if !self.v5 || cost > MAX_COST {
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
        self.a.jmp_mem(at(RSP, stub_slot(Stub::Psr)));
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
            self.a.store8_imm(at(RSP, FRAME_CELL), 1 << cell);
        }
        self.a.mov_imm(INFO, info);
        self.a.jmp_mem(at(RSP, stub_slot(Stub::Step)));
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
        let Some(skip) = skip else {
            return Some(());
        };
        self.a.bind(skip);
        self.flags = before;
        let last = self.spend(item.cost, item.check, 0xF);
        self.after_known(index, last, SEQUENTIAL)
    }

    fn cold(&mut self) -> Option<()> {
        for skip in std::mem::take(&mut self.skips) {
            let item = self.items[skip.index];
            self.a.bind(skip.label);
            self.pending = skip.pending;
            self.flags = skip.flags;
            let last = self.spend(item.cost, true, 0xF);
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
            if item.check {
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
            self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
        }
        if let Some(shared) = self.shared {
            self.a.bind(shared);
            self.a.rot(Rot::Shr, false, INFO, at_shift(self.v5) as u8);
            self.a.alu_imm(Op2::Or, false, INFO, left(0).word()? as i32);
            self.a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
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
    use super::super::super::scan::{Op, classify, classify_thumb};
    use super::super::{Offsets, lower, lower_for};
    use super::runtime::OLD_LAST;
    use super::*;

    fn build(words: &[u32], thumb: bool) -> Vec<u8> {
        build_on::<true>(words, thumb).expect("emits")
    }

    fn build_on<const V5: bool>(words: &[u32], thumb: bool) -> Option<Vec<u8>> {
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
        };
        let lowered = lower_for(&plan, V5, V5, false, X64::compiles).expect("lowers");
        block(&lowered, &plan, &Offsets::of::<V5>())
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
        };
        let lowered = lower_for(&plan, true, true, false, X64::compiles).expect("lowers");
        let offsets = Offsets::of::<true>();
        let long = pass(&lowered, &plan, &offsets, Vec::new()).expect("first pass");
        let size = long.a.here();
        let shorts = long.a.shorts().expect("bound");
        assert!(shorts.iter().any(|&short| short));
        let bytes = block(&lowered, &plan, &offsets).expect("emits");
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
            let call = [0xFF, 0x54, 0x24, stub_slot(stub) as u8];
            bytes.windows(4).filter(|window| *window == call).count()
        };
        assert_eq!(calls(&bytes, Stub::Gather), 3);
        assert_eq!(calls(&bytes, Stub::Scatter), 2);
        assert_eq!(calls(&bytes, Stub::Step), 0);
        let pop = build(&[0xE8BD_8010], false);
        assert_eq!(calls(&pop, Stub::Collect), 1);
        assert_eq!(calls(&pop, Stub::Gather) + calls(&pop, Stub::Step), 0);
        let jump = [0xFF, 0x64, 0x24, stub_slot(Stub::Return) as u8];
        assert!(pop.windows(4).any(|window| window == jump));
        let maybe = build(&[0x18BD_8010], false);
        assert_eq!(calls(&maybe, Stub::Collect), 1);
        assert!(maybe.windows(4).any(|window| window == jump));
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
        let step = [0xFF, 0x54, 0x24, stub_slot(Stub::Step) as u8];
        let bytes = build(&words, false);
        assert_eq!(word(&bytes, 0), MARK);
        assert!(!bytes.windows(4).any(|window| window == step));
        let bytes = build(&[0x4088, 0x40C8, 0x4108, 0x41C8], true);
        assert_eq!(word(&bytes, 0), MARK);
        assert!(!bytes.windows(4).any(|window| window == step));
    }

    #[test]
    fn clz_runs_in_the_block() {
        let bytes = build(&[0xE16F_0F11, 0xE16F_6F17, 0xE16F_2F16], false);
        let step = [0xFF, 0x54, 0x24, stub_slot(Stub::Step) as u8];
        assert!(!bytes.windows(4).any(|window| window == step));
        let scans = bytes
            .windows(2)
            .filter(|window| *window == [0x0F, 0xBD])
            .count();
        assert_eq!(scans, 3);
    }

    #[test]
    fn flag_setting_multiplies_run_in_the_block() {
        let step = [0xFF, 0x54, 0x24, stub_slot(Stub::Step) as u8];
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
            let call = [0xFF, 0x54, 0x24, stub_slot(stub) as u8];
            bytes.windows(4).filter(|window| *window == call).count()
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
            (count(&bytes, 1), count(&bytes, 2))
        };
        assert_eq!(cells(&[0xE280_0001, 0xEAFF_FFFD]), (1, 0));
        assert_eq!(cells(&[0xE250_0001, 0x1AFF_FFFD]), (1, 1));
        assert_eq!(cells(&[0xE280_0001, 0xE12F_FF1E]), (0, 0));
        assert_eq!(cells(&[0x0A00_0002, 0xE280_0001, 0xEAFF_FFFD]), (1, 1));
        assert_eq!(cells(&[0x0A00_0002, 0x0A00_0001, 0xE12F_FF1E]), (1, 1));
    }

    #[test]
    fn psr_transfers_go_through_the_psr_stub() {
        let step = [0xFF, 0x54, 0x24, stub_slot(Stub::Step) as u8];
        let call = [0xFF, 0x54, 0x24, stub_slot(Stub::Psr) as u8];
        let jump = [0xFF, 0x64, 0x24, stub_slot(Stub::Psr) as u8];
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
            let at = bytes.windows(4).position(|window| window == jump).unwrap();
            assert_eq!(bytes[at - 5], 0xBE);
            let info = u32::from_le_bytes(bytes[at - 4..at].try_into().unwrap());
            assert_eq!(info & (SPSR_WRITE | SPSR_READ), kind);
            assert_eq!((info >> 8) & 0xF, fields);
            assert_eq!(info & 0xFF, words.len() as u32);
        }
    }

    #[test]
    fn cp15_cache_ops_jump_to_the_step_stub_and_the_dtcm_read_steps_out() {
        let step = [0xFF, 0x54, 0x24, stub_slot(Stub::Step) as u8];
        let jump = [0xFF, 0x64, 0x24, stub_slot(Stub::Step) as u8];
        let flag = [0xC6, 0x02, 0x01];
        for (op, steps, jumps) in [
            (0xEE07_CF9A, 0, 1),
            (0xEE07_0F3E, 0, 1),
            (0xEE19_0F11, 1, 0),
            (0xEE19_6F11, 1, 0),
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
            if let Some(at) = bytes.windows(4).position(|window| window == jump) {
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
        let [load, store] = [Stub::Load, Stub::Store].map(|s| stub_slot(s) as u8);
        let calls: Vec<(usize, u32, u8)> = bytes
            .windows(9)
            .enumerate()
            .filter(|(_, w)| w[0] == 0xBE && w[5..8] == [0xFF, 0x54, 0x24])
            .map(|(at, w)| {
                let word = u32::from_le_bytes(w[1..5].try_into().unwrap());
                (at + 9, word >> AT_SHIFT, w[8])
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
        a.jmp_mem(at(RSP, stub_slot(Stub::Exit)));
        let direct = a.finish().expect("assembles");
        assert!(runtime.bytes.windows(direct.len()).any(|w| w == direct));
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
        assert_eq!(stores(&[SPEC]), 6);
    }

    #[test]
    fn armv4_blocks_emit() {
        let calls = |bytes: &[u8], stub: Stub| {
            let call = [0xFF, 0x54, 0x24, stub_slot(stub) as u8];
            bytes.windows(4).filter(|window| *window == call).count()
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
            let call = [0xFF, 0x54, 0x24, stub_slot(Stub::Scatter) as u8];
            let scatter = bytes.windows(4).position(|window| window == call);
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
        let plain = X64::hop(0x7000_0000, entry_word(0x40, 0, 0).expect("fits"));
        assert_eq!(plain, 0x7000_0040);
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
}
