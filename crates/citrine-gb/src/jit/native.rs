#[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
mod a64;
#[cfg_attr(not(all(target_arch = "x86_64", unix)), allow(dead_code))]
mod x64;

use super::JitBus;
use super::arena::{Arena, Code, Linked, Prelude};
use super::emit::{Emitted, Emitter};
use super::fast::{Fast, Spec};
use super::helpers::Helpers;
use super::lookup::{LINKS, Link, Lookup};
use super::scan::{Block, Jump, Multiply, Offset, Op, Plan, Target, Transfer, Width};
use crate::gba::cpu::psr::{FLAG_I, FLAG_T};
use crate::gba::cpu::registers::{
    BANKED_OFFSET, CPSR_OFFSET, Registers, SPSR_OFFSET, USER_HIGH_OFFSET,
};
use crate::gba::cpu::{Core, END_LINE};
use std::cmp::Reverse;
use std::mem::offset_of;
use std::sync::{Arc, OnceLock};

pub const MARK: u32 = 0xC17E;

pub trait Backend {
    const HOSTED: bool = true;
    const V4: bool = true;
    const TIMED: bool = true;
    fn block(lowered: &Lowered, plan: &Plan, offsets: &Offsets) -> Option<Linked>;
    fn runtime(setup: &Setup) -> Option<Runtime>;
    fn targets(_: &Runtime) -> Vec<usize> {
        Vec::new()
    }
    fn link(_: &mut [u8], _from: usize, _to: usize) -> Option<()> {
        None
    }
    fn compiles(_: Kind) -> bool {
        true
    }
    fn hop(table: usize, entry: u32) -> u64 {
        (table + 4 * (entry & 0xFFFF) as usize) as u64 | u64::from((entry >> 16) & 0x7FF) << 48
    }
}

pub struct Setup<'a> {
    pub offsets: &'a Offsets,
    pub helpers: &'a Helpers,
    pub chain: Option<usize>,
    pub psr: usize,
    pub specs: &'a [Spec],
    pub pages: bool,
    pub v5: bool,
    pub timed: bool,
}

pub struct Runtime {
    pub bytes: Vec<u8>,
    pub stubs: [usize; STUBS],
}

#[cfg(target_arch = "aarch64")]
type Host = a64::A64;
#[cfg(all(target_arch = "x86_64", unix))]
type Host = x64::X64;
#[cfg(not(any(target_arch = "aarch64", all(target_arch = "x86_64", unix))))]
type Host = Absent;

#[cfg(test)]
pub(super) const HOST_TIMED: bool = Host::TIMED;

#[cfg(not(any(target_arch = "aarch64", all(target_arch = "x86_64", unix))))]
pub struct Absent;

#[cfg(not(any(target_arch = "aarch64", all(target_arch = "x86_64", unix))))]
impl Backend for Absent {
    const HOSTED: bool = false;

    fn block(_: &Lowered, _: &Plan, _: &Offsets) -> Option<Linked> {
        None
    }

    fn runtime(_: &Setup) -> Option<Runtime> {
        None
    }
}
const RUN_LIMIT: u32 = 16;
const ALWAYS: u32 = 0xE;
const FLAG_N: u8 = 8;
const FLAG_Z: u8 = 4;
const FLAG_C: u8 = 2;
const FLAG_V: u8 = 1;
const ALL_FLAGS: u8 = 0xF;
const MULTIPLY_IDLES: u32 = 1;
const MULTIPLY_FLAG_IDLES: u32 = 3;
pub const SHIFT_IDLES: u32 = 1;
const PC: u32 = 15;
const COPROCESSOR_WRITE_IDLES: u32 = 2;
const COPROCESSOR_READ_IDLES: u32 = 3;
const MAX_COST: u32 = 0xFF;
const V4_BY_DEFAULT: bool = true;
const TAKEN: usize = 0;
const FALL_THROUGH: usize = 1;
const GBA_BY_DEFAULT: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stub {
    Exit,
    Load,
    Store,
    Branch,
    Exchange,
    Step,
    Gather,
    Scatter,
    Collect,
    Return,
    Psr,
    LoadPc,
    Fetch,
    Restore,
    FetchFlat,
    FetchArm,
    FetchThumb,
    LoadLeave,
    StoreLeave,
}

pub const STUBS: usize = 19;
pub const BLOCK_WORDS: usize = 16;

#[repr(C)]
pub struct Ctx {
    core: usize,
    bus: usize,
    words: usize,
    debt: usize,
    block: [u32; BLOCK_WORDS],
    window: [u32; 2],
    pc: [u32; 2],
    stubs: [usize; STUBS],
    last: u32,
    force: u32,
    acc: u32,
    stall: u32,
    chains: u32,
    lookup: usize,
    fast: usize,
    pages: usize,
    slot: usize,
    stall_flag: usize,
    attention: usize,
    pending: usize,
    dtcm: usize,
}

const _: () = assert!(offset_of!(Ctx, block) + 4 * BLOCK_WORDS <= 256);
const _: () = assert!(offset_of!(Ctx, pc) + 8 <= 256);
const _: () = assert!(offset_of!(Ctx, attention) < 0x1000);
const _: () = assert!(offset_of!(Ctx, pending) < 0x1000);
const _: () = assert!(offset_of!(Ctx, dtcm) < 0x1000);

pub struct Offsets {
    pub core: u32,
    pub bus: u32,
    pub words: u32,
    pub debt: u32,
    pub stubs: u32,
    pub budget: u32,
    pub base: u32,
    pub last: u32,
    pub force: u32,
    pub acc: u32,
    pub fast: u32,
    pub pages: u32,
    pub slot: u32,
    pub stall_flag: u32,
    pub attention: u32,
    pub pending: u32,
    pub dtcm: u32,
    pub chains: u32,
    pub block: u32,
    pub regs: u32,
    pub cpsr: u32,
    pub user_high: u32,
    pub banked: u32,
    pub spsr: u32,
    pub pipeline: u32,
    pub ldm_glitch: u32,
    pub next_fetch: u32,
    pub cycles: u32,
    pub irq_masked: u32,
    pub irq_held: u32,
    pub landed: u32,
    pub fast_debt: u32,
}

impl Offsets {
    pub fn of<const V5: bool>() -> Self {
        let regs = offset_of!(Core<V5>, regs);
        let at = |offset: usize| offset as u32;
        Self {
            core: at(offset_of!(Ctx, core)),
            bus: at(offset_of!(Ctx, bus)),
            words: at(offset_of!(Ctx, words)),
            debt: at(offset_of!(Ctx, debt)),
            stubs: at(offset_of!(Ctx, stubs)),
            budget: at(offset_of!(Ctx, window)),
            base: at(offset_of!(Ctx, pc)),
            last: at(offset_of!(Ctx, last)),
            force: at(offset_of!(Ctx, force)),
            acc: at(offset_of!(Ctx, acc)),
            fast: at(offset_of!(Ctx, fast)),
            pages: at(offset_of!(Ctx, pages)),
            slot: at(offset_of!(Ctx, slot)),
            stall_flag: at(offset_of!(Ctx, stall_flag)),
            attention: at(offset_of!(Ctx, attention)),
            pending: at(offset_of!(Ctx, pending)),
            dtcm: at(offset_of!(Ctx, dtcm)),
            chains: at(offset_of!(Ctx, chains)),
            block: at(offset_of!(Ctx, block)),
            regs: at(regs + offset_of!(Registers, r)),
            cpsr: at(regs + CPSR_OFFSET),
            user_high: at(regs + USER_HIGH_OFFSET),
            banked: at(regs + BANKED_OFFSET),
            spsr: at(regs + SPSR_OFFSET),
            pipeline: at(offset_of!(Core<V5>, pipeline)),
            ldm_glitch: at(offset_of!(Core<V5>, ldm_glitch)),
            next_fetch: at(offset_of!(Core<V5>, next_fetch)),
            cycles: at(offset_of!(Core<V5>, cycles)),
            irq_masked: at(offset_of!(Core<V5>, irq_masked)),
            irq_held: at(offset_of!(Core<V5>, irq_held)),
            landed: at(offset_of!(Core<V5>, landed)),
            fast_debt: at(offset_of!(Fast, debt)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Last {
    One,
    Ctx,
    Old(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    Known(u32),
    Core,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    pub at: u32,
    pub fetch: Fetch,
    pub force: bool,
    pub branch: bool,
    pub last: Last,
    pub pending: u32,
}

pub const PENDING_LIMIT: u32 = 1000;
pub const PSR_CPSR: u32 = 0;
pub const PSR_SPSR: u32 = 1;
pub const PSR_READ_SPSR: u32 = 2;
pub const PSR_RESTORE: u32 = 3;
pub const RESTORE_FIELDS: u32 = 0x003F_E000;

impl Info {
    pub fn word(self) -> Option<u32> {
        let (fetch, keep) = match self.fetch {
            Fetch::Known(access) => (access & 1, 0),
            Fetch::Core => (0, 1),
        };
        let (mode, cost) = match self.last {
            Last::One => (0, 0),
            Last::Ctx => (1, 0),
            Last::Old(cost) => (2, cost),
        };
        (self.at <= 0xFF && cost <= MAX_COST && self.pending <= 0x3FF).then_some(
            self.at
                | fetch << 8
                | keep << 9
                | u32::from(self.force) << 10
                | u32::from(self.branch) << 11
                | mode << 12
                | cost << 14
                | self.pending << 22,
        )
    }
}

pub const CODE_CLASS: u32 = 26;

pub fn memory_word(kind: u32, ahead: u32, cost: u32, pending: u32) -> Option<u32> {
    (ahead <= 0x1F && cost <= MAX_COST && pending <= 0x3FF)
        .then_some(kind | ahead << 3 | cost << 8 | pending << 16)
}

pub fn load_pc_word(kind: u32, charged: u32, at: u32, pending: u32) -> Option<u32> {
    (kind <= 7 && charged <= MAX_COST && at <= 0xFF && pending <= 0x3FF)
        .then_some(kind | charged << 3 | at << 11 | pending << 19)
}

pub fn restore_word(info: u32, offset: u32, cost: u32) -> Option<u32> {
    (info & RESTORE_FIELDS == 0 && matches!(offset, 0 | 4) && cost <= MAX_COST)
        .then_some(info | (offset >> 2) << 13 | cost << 14)
}

pub fn block_word(count: u32, ahead: u32, cost: u32, pending: u32) -> Option<u32> {
    if count as usize > BLOCK_WORDS {
        return None;
    }
    memory_word(0, ahead, cost, pending).map(|word| word | count << 26)
}

pub fn step_word(at: u32, ahead: u32, pending: u32) -> Option<u32> {
    (at <= 0xFF && ahead <= 0x1F && pending <= 0x3FF).then_some(at | ahead << 8 | pending << 16)
}

pub fn entry_word(offset: u32, pending: u32, ahead: u32) -> Option<u32> {
    (offset <= 0xFFFF && pending <= 0x7FF && ahead <= 0x1F)
        .then_some(offset | pending << 16 | ahead << 27)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    Immediate { value: u32, carry: Option<bool> },
    Register { rm: u32, shift: u32, amount: u32 },
    Shifted { rm: u32, shift: u32, rs: u32 },
}

impl Operand {
    pub fn changes_carry(self) -> bool {
        match self {
            Operand::Immediate { carry, .. } => carry.is_some(),
            Operand::Register { shift, amount, .. } => shift != 0 || amount != 0,
            Operand::Shifted { .. } => true,
        }
    }

    pub fn reads_carry(self) -> bool {
        matches!(
            self,
            Operand::Register {
                shift: 3,
                amount: 0,
                ..
            } | Operand::Shifted { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alu {
    pub opcode: u32,
    pub set_flags: bool,
    pub rn: u32,
    pub rd: u32,
    pub operand: Operand,
}

impl Alu {
    pub fn decode(op: u32) -> Self {
        let operand = if op & (1 << 25) != 0 {
            let rotate = ((op >> 8) & 0xF) * 2;
            let value = (op & 0xFF).rotate_right(rotate);
            Operand::Immediate {
                value,
                carry: (rotate != 0).then_some(value >> 31 != 0),
            }
        } else if op & 0x10 != 0 {
            Operand::Shifted {
                rm: op & 0xF,
                shift: (op >> 5) & 3,
                rs: (op >> 8) & 0xF,
            }
        } else {
            Operand::Register {
                rm: op & 0xF,
                shift: (op >> 5) & 3,
                amount: (op >> 7) & 0x1F,
            }
        };
        Self {
            opcode: (op >> 21) & 0xF,
            set_flags: op & (1 << 20) != 0,
            rn: (op >> 16) & 0xF,
            rd: (op >> 12) & 0xF,
            operand,
        }
    }

    pub fn logical(self) -> bool {
        matches!(self.opcode, 0x0 | 0x1 | 0x8 | 0x9 | 0xC..=0xF)
    }

    pub fn shifted(self) -> bool {
        matches!(self.operand, Operand::Shifted { .. })
    }

    fn reads_pc(self) -> bool {
        let rn = !matches!(self.opcode, 0xD | 0xF) && self.rn == PC;
        match self.operand {
            Operand::Shifted { rm, rs, .. } => rn || rm == PC || rs == PC,
            _ => false,
        }
    }

    pub fn writes_rd(self) -> bool {
        !(0x8..=0xB).contains(&self.opcode)
    }

    fn reads(self) -> u8 {
        let carry = matches!(self.opcode, 0x5..=0x7) || self.operand.reads_carry();
        if carry { FLAG_C } else { 0 }
    }

    fn writes(self) -> u8 {
        match (self.set_flags, self.logical()) {
            (false, _) => 0,
            (true, false) => ALL_FLAGS,
            (true, true) if self.operand.changes_carry() => FLAG_N | FLAG_Z | FLAG_C,
            (true, true) => FLAG_N | FLAG_Z,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swap {
    pub rd: u32,
    pub rm: u32,
    pub rn: u32,
    pub byte: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Alu(Alu),
    Constant {
        rd: u32,
        value: u32,
    },
    Multiply(Multiply),
    Transfer(Transfer),
    Block(Block),
    Mrs {
        rd: u32,
        spsr: bool,
    },
    Msr {
        operand: Operand,
        mask: u32,
        spsr: bool,
    },
    UserBlock(Block),
    Clz {
        rd: u32,
        rm: u32,
    },
    Cp15 {
        load: Option<u32>,
    },
    Swap(Swap),
    Restore {
        offset: u32,
    },
    Step,
    Exit,
    Branch(u32),
    Jump(Jump),
    LoadPc(Transfer),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub condition: u32,
    pub cost: u32,
    pub spend: Option<u32>,
    pub check: bool,
    pub ahead: u32,
    pub entry: u32,
    pub live: u8,
}

impl Item {
    pub fn helper(&self) -> bool {
        self.spend.is_none()
    }

    fn run(&self) -> Option<u32> {
        match self.kind {
            Kind::Branch(_) if self.condition != ALWAYS => Some(self.cost),
            _ => self.spend,
        }
    }
}

fn link_owners(items: &[Item], plan: &Plan) -> [Option<usize>; LINKS] {
    let size = if plan.thumb { 2 } else { 4 };
    let address = |index: usize| plan.pc.wrapping_add(index as u32 * size);
    let target = |index: usize| match items[index].kind {
        Kind::Branch(word) => {
            Some(address(index + 2).wrapping_add((((word << 8) as i32) >> 6) as u32))
        }
        Kind::Jump(Jump {
            target: Target::Fixed(target),
            ..
        }) => Some(target),
        _ => None,
    };
    let mut owners = [None; LINKS];
    let Some(last) = items.len().checked_sub(1) else {
        return owners;
    };
    if target(last).is_some() {
        owners[TAKEN] = Some(last);
    }
    let item = items[last];
    if !matches!(item.kind, Kind::Branch(_) | Kind::Jump(_)) || item.condition < ALWAYS {
        owners[FALL_THROUGH] = Some(items.len());
    }
    let mut early: Vec<_> = (0..last)
        .filter_map(|index| target(index).map(|to| (to > address(index), Reverse(index))))
        .collect();
    early.sort_unstable();
    let free = owners.iter_mut().filter(|owner| owner.is_none());
    for (owner, (_, Reverse(index))) in free.zip(early) {
        *owner = Some(index);
    }
    owners
}

pub struct Lowered {
    pub items: Vec<Item>,
    pub v5: bool,
    pub timed: bool,
    pub code: u32,
    pub flat: bool,
}

fn kind(op: Op, cp15: bool) -> Kind {
    match op {
        Op::Alu(word) => match Alu::decode(word) {
            alu if alu.shifted() && (word >> 28 != ALWAYS || alu.reads_pc()) => Kind::Step,
            alu => Kind::Alu(alu),
        },
        Op::Constant { rd, value } => Kind::Constant { rd, value },
        Op::Multiply(multiply)
            if multiply.condition == ALWAYS && !(multiply.set_flags && multiply.long.is_some()) =>
        {
            Kind::Multiply(multiply)
        }
        Op::Block(block) => Kind::Block(block),
        Op::Step(word)
            if word >> 28 == ALWAYS
                && word & 0x0FBF_0FFF == 0x010F_0000
                && (word >> 12) & 0xF != PC =>
        {
            Kind::Mrs {
                rd: (word >> 12) & 0xF,
                spsr: word & (1 << 22) != 0,
            }
        }
        Op::Step(word)
            if word & 0x0FFF_0FF0 == 0x016F_0F10 && word >> 28 == ALWAYS && word & 0xF != PC =>
        {
            Kind::Clz {
                rd: (word >> 12) & 0xF,
                rm: word & 0xF,
            }
        }
        Op::Step(word) => swap(word).map_or(Kind::Step, Kind::Swap),
        Op::Multiply(_) => Kind::Step,
        Op::Exit(word) => msr(word)
            .or_else(|| user_block(word).map(Kind::UserBlock))
            .or_else(|| coprocessor(word).filter(|_| cp15))
            .or_else(|| load_pc(word).map(Kind::LoadPc))
            .or_else(|| restore(word))
            .unwrap_or(Kind::Exit),
        Op::Transfer(transfer) => Kind::Transfer(transfer),
        Op::Branch(word) => Kind::Branch(word),
        Op::Jump(jump) => Kind::Jump(jump),
    }
}

fn coprocessor(word: u32) -> Option<Kind> {
    if word >> 28 != ALWAYS || word & 0x0FE0_0F10 != 0x0E00_0F10 {
        return None;
    }
    let (rd, crn, crm, opcode2) = (
        (word >> 12) & 0xF,
        (word >> 16) & 0xF,
        word & 0xF,
        (word >> 5) & 7,
    );
    match (word & (1 << 20) != 0, crn, crm, opcode2) {
        (false, 7, 10, 1 | 2 | 4) | (false, 7, 14, 1) | (false, 7, 6, 1) => {
            Some(Kind::Cp15 { load: None })
        }
        (true, 9, 1, 0) if rd != 15 => Some(Kind::Cp15 { load: Some(rd) }),
        _ => None,
    }
}

fn load_pc(word: u32) -> Option<Transfer> {
    let rn = (word >> 16) & 0xF;
    let register = word & (1 << 25) != 0;
    if word >> 28 != ALWAYS
        || word & 0x0C50_F000 != 0x0410_F000
        || rn == PC
        || register && word & 0x10 != 0
    {
        return None;
    }
    let offset = if register {
        Offset::Shifted {
            rm: word & 0xF,
            shift: (word >> 5) & 3,
            amount: (word >> 7) & 0x1F,
        }
    } else {
        Offset::Immediate(word & 0xFFF)
    };
    Some(Transfer {
        op: word,
        rn,
        rd: PC,
        width: Width::Word,
        signed: false,
        offset,
    })
}

fn restore(word: u32) -> Option<Kind> {
    let offset = match word {
        0xE1B0_F00E => 0,
        word if word & 0xFFFF_F000 == 0xE25E_F000 => {
            (word & 0xFF).rotate_right(2 * ((word >> 8) & 0xF))
        }
        _ => return None,
    };
    Some(Kind::Restore { offset })
}

fn msr(word: u32) -> Option<Kind> {
    let immediate = word & 0x0FB0_F000 == 0x0320_F000;
    let register = word & 0x0FB0_FFF0 == 0x0120_F000 && word & 0xF != 15;
    if word >> 28 != ALWAYS || !(immediate || register) {
        return None;
    }
    let operand = if immediate {
        Operand::Immediate {
            value: (word & 0xFF).rotate_right(((word >> 8) & 0xF) * 2),
            carry: None,
        }
    } else {
        Operand::Register {
            rm: word & 0xF,
            shift: 0,
            amount: 0,
        }
    };
    let mask = (0..4)
        .filter(|byte| word & (1 << (16 + byte)) != 0)
        .fold(0, |mask, byte| mask | 0xFF << (8 * byte));
    Some(Kind::Msr {
        operand,
        mask,
        spsr: word & (1 << 22) != 0,
    })
}

fn swap(word: u32) -> Option<Swap> {
    let (rd, rm, rn) = ((word >> 12) & 0xF, word & 0xF, (word >> 16) & 0xF);
    (word >> 28 == ALWAYS && word & 0x0FB0_0FF0 == 0x0100_0090 && rd != PC && rm != PC && rn != PC)
        .then_some(Swap {
            rd,
            rm,
            rn,
            byte: word & (1 << 22) != 0,
        })
}

fn user_block(word: u32) -> Option<Block> {
    let list = word & 0xFFFF;
    let rn = (word >> 16) & 0xF;
    (word & 0x0E40_0000 == 0x0840_0000
        && word >> 28 == ALWAYS
        && list != 0
        && list & (1 << PC) == 0
        && rn < 8)
        .then_some(Block {
            condition: ALWAYS,
            rn,
            list: list as u16,
            load: word & (1 << 20) != 0,
            up: word & (1 << 23) != 0,
            pre: word & (1 << 24) != 0,
            writeback: word & (1 << 21) != 0,
        })
}

fn condition_reads(condition: u32) -> u8 {
    match condition {
        0x0 | 0x1 => FLAG_Z,
        0x2 | 0x3 => FLAG_C,
        0x4 | 0x5 => FLAG_N,
        0x6 | 0x7 => FLAG_V,
        0x8 | 0x9 => FLAG_C | FLAG_Z,
        0xA | 0xB => FLAG_N | FLAG_V,
        0xC | 0xD => FLAG_N | FLAG_Z | FLAG_V,
        _ => 0,
    }
}

fn bound(spend: u32, v5: bool) -> u32 {
    if v5 { spend.max(1) } else { spend }
}

#[cfg(test)]
pub fn lower(plan: &Plan, v5: bool, cp15: bool, timed: bool) -> Option<Lowered> {
    lower_for(plan, v5, cp15, timed, |_| true)
}

fn lower_for(
    plan: &Plan,
    v5: bool,
    cp15: bool,
    timed: bool,
    compiles: impl Fn(Kind) -> bool,
) -> Option<Lowered> {
    let count = plan.ops.len();
    let mut items = Vec::with_capacity(count);
    for (index, &op) in plan.ops.iter().enumerate() {
        let kind = match kind(op, cp15) {
            Kind::Mrs { .. } | Kind::Clz { .. } if !v5 => Kind::Step,
            Kind::Multiply(multiply) if !v5 && multiply.set_flags => Kind::Step,
            Kind::Msr { .. } | Kind::LoadPc(_) | Kind::Restore { .. } if !v5 => Kind::Exit,
            kind if compiles(kind) => kind,
            Kind::UserBlock(_)
            | Kind::Cp15 { .. }
            | Kind::Msr { .. }
            | Kind::LoadPc(_)
            | Kind::Restore { .. } => Kind::Exit,
            _ => Kind::Step,
        };
        let cost = plan.fetch[index][1];
        if cost > MAX_COST || !v5 && !timed && cost == 0 {
            return None;
        }
        let condition = match kind {
            Kind::Step | Kind::Exit | Kind::UserBlock(_) => ALWAYS,
            _ => op.condition(),
        };
        let spend = match kind {
            _ if timed => None,
            Kind::Alu(alu) if alu.shifted() && v5 => Some(cost + SHIFT_IDLES),
            Kind::Alu(alu) if alu.shifted() => Some(plan.fetch[index][0] + SHIFT_IDLES),
            Kind::Alu(_)
            | Kind::Constant { .. }
            | Kind::Mrs { .. }
            | Kind::Msr { .. }
            | Kind::Clz { .. } => Some(cost),
            Kind::Multiply(multiply) if v5 && multiply.set_flags => {
                Some(cost + MULTIPLY_FLAG_IDLES)
            }
            Kind::Multiply(_) if v5 => Some(cost + MULTIPLY_IDLES),
            Kind::Cp15 { load: None } => Some(cost + COPROCESSOR_WRITE_IDLES),
            Kind::Cp15 { load: Some(_) } => Some(cost + COPROCESSOR_READ_IDLES),
            _ => None,
        };
        items.push(Item {
            kind,
            condition,
            cost,
            spend,
            check: true,
            ahead: 0,
            entry: 0,
            live: ALL_FLAGS,
        });
    }
    if !timed {
        let size = if plan.thumb { 2 } else { 4 };
        for (index, item) in items.iter_mut().enumerate() {
            let next = plan.pc.wrapping_add(size * (index as u32 + 1));
            item.check = index + 1 == count || next.is_multiple_of(END_LINE);
        }
        return Some(flags(items, v5, timed));
    }
    let mut run = 0;
    for (index, item) in items.iter_mut().enumerate() {
        let last = index + 1 == count;
        match item.run().map(|spend| bound(spend, v5)) {
            Some(bound) if !last && run + bound <= RUN_LIMIT => {
                run += bound;
                item.check = false;
            }
            _ => run = 0,
        }
    }
    let mut ahead = 0;
    for item in items.iter_mut().rev() {
        item.ahead = ahead;
        ahead = match (item.check, item.run()) {
            (false, Some(spend)) => bound(spend, v5) + ahead,
            _ => 0,
        };
        item.entry = ahead;
    }
    Some(flags(items, v5, timed))
}

fn flags(mut items: Vec<Item>, v5: bool, timed: bool) -> Lowered {
    let mut live = ALL_FLAGS;
    for item in items.iter_mut().rev() {
        if item.check || item.helper() {
            live = ALL_FLAGS;
        }
        item.live = live;
        let (reads, writes) = match item.kind {
            Kind::Alu(alu) => (alu.reads(), alu.writes()),
            Kind::Transfer(transfer) => (
                if matches!(
                    transfer.offset,
                    Offset::Shifted {
                        shift: 3,
                        amount: 0,
                        ..
                    }
                ) {
                    FLAG_C
                } else {
                    0
                },
                0,
            ),
            Kind::Multiply(multiply) if multiply.set_flags => (0, FLAG_N | FLAG_Z),
            Kind::Constant { .. }
            | Kind::Clz { .. }
            | Kind::Swap(_)
            | Kind::Multiply(_)
            | Kind::Block(_)
            | Kind::Cp15 { .. } => (0, 0),
            Kind::Mrs { .. }
            | Kind::Msr { .. }
            | Kind::Restore { .. }
            | Kind::Step
            | Kind::Exit
            | Kind::UserBlock(_)
            | Kind::Branch(_)
            | Kind::Jump(_)
            | Kind::LoadPc(_) => (ALL_FLAGS, 0),
        };
        let writes = if item.condition == ALWAYS { writes } else { 0 };
        live = reads | condition_reads(item.condition) | (live & !writes);
    }
    Lowered {
        items,
        v5,
        timed,
        code: 0,
        flat: false,
    }
}

pub fn emit<const V5: bool, B: JitBus>(emitter: &mut Emitter, plan: &Plan) -> Option<Emitted> {
    let native = lower_for(plan, V5, B::CP15, B::TIMED_FETCH, Host::compiles)
        .map(|mut lowered| {
            lowered.flat = B::flat_code(plan.pc);
            if !V5 {
                let size = if plan.thumb { 2 } else { 4 };
                let end = plan.pc.wrapping_add(size * (plan.words.len() as u32 + 1));
                lowered.code = B::code_class(plan.pc, end, plan.thumb);
                for item in &mut lowered.items {
                    match item.kind {
                        Kind::UserBlock(_) => item.kind = Kind::Exit,
                        Kind::Swap(_) => item.kind = Kind::Step,
                        _ => {}
                    }
                }
            }
            lowered
        })
        .and_then(|lowered| Host::block(&lowered, plan, &Offsets::of::<V5>()));
    match native {
        Some(code) => Some(Emitted {
            code: code.bytes,
            relocs: code.links,
            portable: true,
        }),
        None => emitter.emit::<V5, B>(plan),
    }
}

pub fn is_native(bytes: &[u8]) -> bool {
    Host::HOSTED && bytes.get(..2) == Some(&MARK.to_le_bytes()[..2])
}

type Trampoline =
    unsafe extern "C" fn(*mut u8, *mut u8, u32, u32, *mut Fast, *const u32, usize, *mut Ctx) -> u64;

struct Shared {
    _code: Code,
    trampoline: Trampoline,
    stubs: [usize; STUBS],
    prelude: Option<Prelude>,
}

pub fn link(word: &mut [u8], from: usize, to: usize) -> Option<()> {
    Host::link(word, from, to)
}

pub struct Native {
    shared: Arc<Shared>,
    ctx: Box<Ctx>,
}

fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| {
        Host::HOSTED
            && std::env::var("CITRINE_JIT_BACKEND").map_or(true, |backend| backend != "cranelift")
    })
}

fn v4_wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| {
        std::env::var("CITRINE_JIT_V4").map_or(V4_BY_DEFAULT, |backend| backend == "native")
    })
}

fn gba_wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| {
        std::env::var("CITRINE_JIT_GBA").map_or(GBA_BY_DEFAULT, |backend| backend == "native")
    })
}

unsafe impl Send for Native {}
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Native {
    pub fn new<const V5: bool, B: JitBus>() -> Option<Self> {
        let wanted_here = if B::TIMED_FETCH {
            Host::TIMED && gba_wanted()
        } else {
            V5 || Host::V4 && v4_wanted()
        };
        if !wanted_here || !wanted() {
            return None;
        }
        Self::start::<V5, B>()
    }

    pub(super) fn start<const V5: bool, B: JitBus>() -> Option<Self> {
        let offsets = Offsets::of::<V5>();
        let helpers = Helpers::of::<V5, B>();
        let chain = chains().then_some(chain::<V5, B> as *const () as usize);
        let specs: &[Spec] = if V5 { &B::SPECS } else { &[] };
        let runtime = Host::runtime(&Setup {
            offsets: &offsets,
            helpers: &helpers,
            chain,
            psr: psr::<V5, B> as *const () as usize,
            specs,
            pages: B::PAGES,
            v5: V5,
            timed: B::TIMED_FETCH,
        })?;
        let code = Arena::default().place(Linked::plain(runtime.bytes.clone()), None)?;
        let entry = code.entry();
        let mut stubs = [0; STUBS];
        for (slot, offset) in stubs.iter_mut().zip(runtime.stubs) {
            *slot = entry + offset;
        }
        let trampoline = unsafe { std::mem::transmute::<usize, Trampoline>(entry) };
        let targets = Host::targets(&runtime);
        let prelude = if targets.is_empty() {
            None
        } else {
            Some(Prelude::new(&runtime.bytes, targets)?)
        };
        Some(Self::over(Arc::new(Shared {
            _code: code,
            trampoline,
            stubs,
            prelude,
        })))
    }

    pub fn prelude(&self) -> Option<&Prelude> {
        self.shared.prelude.as_ref()
    }

    pub fn fork(&self) -> Self {
        Self::over(Arc::clone(&self.shared))
    }

    fn over(shared: Arc<Shared>) -> Self {
        let stubs = shared.stubs;
        Self {
            shared,
            ctx: Box::new(Ctx {
                core: 0,
                bus: 0,
                words: 0,
                debt: 0,
                stubs,
                window: [0; 2],
                pc: [0; 2],
                last: 0,
                force: 0,
                acc: 0,
                stall: 0,
                chains: 0,
                lookup: 0,
                fast: 0,
                pages: 0,
                slot: 0,
                stall_flag: 0,
                attention: 0,
                pending: 0,
                dtcm: 0,
                block: [0; BLOCK_WORDS],
            }),
        }
    }

    #[expect(clippy::too_many_arguments, reason = "one block run")]
    pub unsafe fn call<const V5: bool, B: JitBus>(
        &mut self,
        table: usize,
        core: &mut Core<V5>,
        bus: &mut B,
        budget: u32,
        start: u32,
        fast: *mut Fast,
        words: *const u32,
        lookup: &mut Lookup,
        slot: usize,
    ) -> u64 {
        self.ctx.stall_flag = bus.stall_flag() as usize;
        if B::CP15 {
            let cells = bus.cp15_cells();
            self.ctx.attention = cells.attention as usize;
            self.ctx.pending = cells.pending as usize;
            self.ctx.dtcm = cells.dtcm as usize;
        }
        self.ctx.slot = if self.ctx.stall_flag != 0 { slot } else { 0 };
        let core = (core as *mut Core<V5>).cast();
        let bus = (bus as *mut B).cast();
        self.ctx.lookup = lookup as *mut Lookup as usize;
        if B::PAGES {
            self.ctx.pages = unsafe { (*fast).pages }.addr();
        }
        let ctx: *mut Ctx = &mut *self.ctx;
        unsafe { (self.shared.trampoline)(core, bus, budget, start, fast, words, table & !1, ctx) }
    }

    pub fn settled(&mut self) -> (u32, u32) {
        (
            std::mem::take(&mut self.ctx.stall),
            std::mem::take(&mut self.ctx.chains),
        )
    }
}

fn chains() -> bool {
    static CHAINS: OnceLock<bool> = OnceLock::new();
    *CHAINS.get_or_init(|| std::env::var("CITRINE_JIT_CHAIN").map_or(true, |chain| chain != "off"))
}

unsafe extern "C" fn psr<const V5: bool, B: JitBus>(
    core: *mut Core<V5>,
    value: u32,
    mask: u32,
    request: u32,
    bus: *const B,
) -> u32 {
    let regs = unsafe { &mut (*core).regs };
    match request {
        PSR_RESTORE => {
            let Some(spsr) = regs.spsr() else {
                return 0;
            };
            if !unsafe { &*bus }.runs_on(spsr & FLAG_I != 0) {
                return 0;
            }
            regs.set_cpsr(spsr);
            return spsr;
        }
        PSR_SPSR => {
            if let Some(old) = regs.spsr() {
                regs.set_spsr((old & !mask) | (value & mask));
            }
        }
        PSR_READ_SPSR => return regs.spsr().unwrap_or(regs.cpsr()),
        _ => {
            let old = regs.cpsr();
            regs.set_cpsr((old & !mask) | (value & mask));
        }
    }
    0
}

unsafe extern "C" fn chain<const V5: bool, B: JitBus>(
    ctx: *mut Ctx,
    rem: u32,
    at: u32,
    cell: u32,
) -> u64 {
    let ctx = unsafe { &mut *ctx };
    debug_assert!(cell.count_ones() <= 1 && cell >> LINKS == 0);
    let core = unsafe { &mut *(ctx.core as *mut Core<V5>) };
    let bus = unsafe { &mut *(ctx.bus as *mut B) };
    let lookup = unsafe { &mut *(ctx.lookup as *mut Lookup) };
    let stall = bus.take_stall();
    if stall != 0 {
        ctx.stall = stall;
        return 0;
    }
    if core.ldm_glitch {
        return 0;
    }
    let thumb = (core.regs.cpsr() & FLAG_T) >> FLAG_T.trailing_zeros();
    let r15 = core.regs.r[15];
    let pc = r15.wrapping_sub(8 >> thumb);
    if pc & (3 >> thumb) != 0 {
        return 0;
    }
    let Some(((code, start, words), slot)) = lookup.find_slot(pc | thumb, core.pipeline) else {
        return 0;
    };
    if code & 1 == 0 {
        return 0;
    }
    let table = code & !1;
    let entry = unsafe { *(table as *const u32).add(start as usize) };
    if rem as i32 <= (entry >> 27) as i32 {
        return 0;
    }
    let shift = 2 - thumb;
    let base = [r15.wrapping_sub(start << shift), shift];
    let hop = Host::hop(table, entry);
    if ctx.slot != 0 && cell != 0 {
        let link = Link {
            slot,
            jump: (hop & ((1 << 48) - 1)) as usize,
            words: words as usize,
            pc: base,
            generation: 0,
            start,
            entry,
        };
        lookup.link(ctx.slot, cell.trailing_zeros() as usize, link);
    }
    if ctx.stall_flag != 0 {
        ctx.slot = slot;
    }
    ctx.acc = ctx.acc.wrapping_add(at.wrapping_sub(ctx.window[1]));
    ctx.window[1] = start;
    ctx.words = words as usize;
    ctx.pc = base;
    ctx.chains += 1;
    hop
}

#[cfg(test)]
mod tests {
    use super::super::scan::{classify, classify_thumb};
    use super::*;

    fn plan_of(words: &[u32], fetch: u32) -> (Vec<Op>, Vec<[u32; 2]>, Vec<u32>) {
        let ops: Vec<Op> = words.iter().map(|&word| classify::<true>(word)).collect();
        let fetch = vec![[fetch, fetch]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        (ops, fetch, all)
    }

    #[test]
    fn free_link_cells_go_to_early_branches_backward_first() {
        let owners = |words: &[u32]| {
            let (ops, fetch, all) = plan_of(words, 1);
            let plan = Plan {
                pc: 0x0200_0000,
                thumb: false,
                words: &all,
                ops: &ops,
                fetch: &fetch,
            };
            link_owners(
                &lower(&plan, true, true, false).expect("lowers").items,
                &plan,
            )
        };
        assert_eq!(owners(&[0xE280_0001, 0xEAFF_FFFD]), [Some(1), None]);
        assert_eq!(
            owners(&[0x0A00_0002, 0xE250_0001, 0x1AFF_FFFD]),
            [Some(2), Some(3)]
        );
        assert_eq!(
            owners(&[0x0A00_0002, 0x1AFF_FFFE, 0xE280_0001, 0xEAFF_FFFD]),
            [Some(3), Some(1)]
        );
        assert_eq!(
            owners(&[0x0A00_0002, 0x0A00_0001, 0xE280_0001, 0xE12F_FF1E]),
            [Some(1), Some(0)]
        );
    }

    #[test]
    fn ds_blocks_check_only_before_a_line_starts() {
        let words = [
            0xE3A0_0001,
            0xE280_0001,
            0xE591_0004,
            0xE280_0001,
            0xE280_0001,
            0xE280_0001,
        ];
        let (ops, fetch, all) = plan_of(&words, 6);
        let plan = Plan {
            pc: 0x0200_0070,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        let checks: Vec<bool> = lowered.items.iter().map(|item| item.check).collect();
        assert_eq!(checks, [false, false, false, true, false, true]);
        assert!(
            lowered
                .items
                .iter()
                .all(|item| item.entry == 0 && item.ahead == 0)
        );
        let plan = Plan {
            pc: 0x0200_0060,
            ..plan
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        let checks: Vec<bool> = lowered.items.iter().map(|item| item.check).collect();
        assert_eq!(checks, [false, false, false, false, false, true]);
    }

    #[test]
    fn a_timed_conditional_branch_inside_a_run_needs_no_check() {
        let words = [0xE3A0_0001, 0x1A00_0004, 0xE280_0001, 0xE280_0001];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, true).expect("lowers");
        assert!(matches!(lowered.items[1].kind, Kind::Branch(_)));
        assert!(lowered.items[1].helper());
        let checks: Vec<bool> = lowered.items.iter().map(|item| item.check).collect();
        assert_eq!(checks, [true, false, true, true]);
        let entries: Vec<u32> = lowered.items.iter().map(|item| item.entry).collect();
        assert_eq!(entries, [0, 1, 0, 0]);
        assert_eq!(lowered.items[0].ahead, 1);
    }

    #[test]
    fn armv4_ds_blocks_refuse_free_fetches() {
        let words = [0xE3A0_0001, 0xE000_0291, 0xE280_0001, 0xE280_0001];
        let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
        let fetch = vec![[3, 2]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc: 0x0380_0078,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, false, false, false).expect("lowers");
        assert!(!lowered.v5);
        assert!(matches!(lowered.items[1].kind, Kind::Multiply(_)));
        assert_eq!(lowered.items[1].spend, None);
        let checks: Vec<bool> = lowered.items.iter().map(|item| item.check).collect();
        assert_eq!(checks, [false, true, false, true]);
        let free = vec![[0, 0]; ops.len()];
        let plan = Plan {
            fetch: &free,
            ..plan
        };
        assert!(lower(&plan, false, false, false).is_none());
        assert!(lower(&plan, true, true, false).is_some());
    }

    #[test]
    fn flags_overwritten_before_use_are_dead() {
        let words = [0xE210_0003, 0xE290_0001, 0xE280_0001, 0xE280_0001];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert_eq!(lowered.items[0].live, 0);
        assert_eq!(lowered.items[1].live, ALL_FLAGS);
    }

    #[test]
    fn flag_setting_multiplies_lower_natively_on_the_arm9() {
        let words = [0xE011_0392, 0xE0D1_0392, 0x1000_0291, 0xE280_0001];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert!(matches!(lowered.items[0].kind, Kind::Multiply(m) if m.set_flags));
        assert_eq!(lowered.items[0].spend, Some(1 + MULTIPLY_FLAG_IDLES));
        assert_eq!(lowered.items[1].kind, Kind::Step);
        assert_eq!(lowered.items[2].kind, Kind::Step);
        let thumb = classify_thumb::<true>(0x434B, 0x0200_0000);
        assert!(matches!(kind(thumb, true), Kind::Multiply(m) if m.set_flags));
    }

    #[test]
    fn flag_setting_multiplies_step_on_armv4() {
        let words = [0xE011_0392, 0xE001_0392, 0xE280_0001];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        for timed in [false, true] {
            let lowered = lower(&plan, false, false, timed).expect("lowers");
            assert_eq!(lowered.items[0].kind, Kind::Step);
            assert!(matches!(lowered.items[1].kind, Kind::Multiply(m) if !m.set_flags));
        }
    }

    #[test]
    fn exception_returns_restore_natively_on_the_arm9() {
        assert_eq!(
            kind(classify::<true>(0xE25E_F004), true),
            Kind::Restore { offset: 4 }
        );
        assert_eq!(
            kind(classify::<true>(0xE1B0_F00E), true),
            Kind::Restore { offset: 0 }
        );
        assert_eq!(kind(classify::<true>(0x025E_F004), true), Kind::Exit);
        assert_eq!(kind(classify::<true>(0xE24E_F004), true), Kind::Exit);
        let words = [0xE280_0001, 0xE25E_F004];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert_eq!(lowered.items[1].kind, Kind::Restore { offset: 4 });
        assert_eq!(lowered.items[1].spend, None);
        let lowered = lower(&plan, false, false, false).expect("lowers");
        assert_eq!(lowered.items[1].kind, Kind::Exit);
    }

    #[test]
    fn block_transfers_lower_natively() {
        let words = [0xE92D_4010, 0xE8BD_0011, 0xE8BD_8010];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert!(matches!(lowered.items[0].kind, Kind::Block(_)));
        assert!(matches!(lowered.items[1].kind, Kind::Block(_)));
        assert!(matches!(lowered.items[2].kind, Kind::Block(block) if block.loads_pc()));
        assert_eq!(
            block_word(16, 2, 3, 4),
            Some(2 << 3 | 3 << 8 | 4 << 16 | 16 << 26)
        );
        assert_eq!(block_word(17, 0, 0, 0), None);
    }

    #[test]
    fn cpsr_transfers_lower_natively() {
        let words = [0xE10F_0000, 0xE380_1080, 0xE121_F001];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert_eq!(lowered.items[0].kind, Kind::Mrs { rd: 0, spsr: false });
        assert_eq!(lowered.items[0].spend, Some(1));
        assert_eq!(lowered.items[1].live, ALL_FLAGS);
        assert_eq!(
            kind(classify::<true>(0xE14F_2000), true),
            Kind::Mrs { rd: 2, spsr: true }
        );
        assert_eq!(kind(classify::<true>(0xE14F_F000), true), Kind::Exit);
        assert_eq!(
            lowered.items[2].kind,
            Kind::Msr {
                operand: Operand::Register {
                    rm: 1,
                    shift: 0,
                    amount: 0
                },
                mask: 0xFF,
                spsr: false
            }
        );
        assert_eq!(
            kind(classify::<true>(0xE328_F20F), true),
            Kind::Msr {
                operand: Operand::Immediate {
                    value: 0xF000_0000,
                    carry: None
                },
                mask: 0xFF00_0000,
                spsr: false
            }
        );
        assert_eq!(
            kind(classify::<true>(0xE169_F002), true),
            Kind::Msr {
                operand: Operand::Register {
                    rm: 2,
                    shift: 0,
                    amount: 0
                },
                mask: 0xFF00_00FF,
                spsr: true
            }
        );
        for (word, fallback) in [
            (0x014F_0000, Kind::Step),
            (0x010F_0000, Kind::Step),
            (0xE121_F00F, Kind::Exit),
            (0x0121_F000, Kind::Exit),
        ] {
            assert_eq!(kind(classify::<true>(word), true), fallback, "{word:08X}");
        }
    }

    #[test]
    fn user_bank_blocks_lower_natively_without_pc_or_a_banked_base() {
        let words = [
            0xE8D0_7FFF,
            0xE9E0_7FFC,
            0xE8FD_8000,
            0xE8DD_4000,
            0x18D0_7FFF,
            0xE8D0_0000,
        ];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert!(matches!(
            lowered.items[0].kind,
            Kind::UserBlock(block) if block.load && block.list == 0x7FFF && !block.writeback
        ));
        assert!(matches!(
            lowered.items[1].kind,
            Kind::UserBlock(block) if !block.load && block.pre && block.up && block.writeback
        ));
        for item in &lowered.items[2..] {
            assert_eq!(item.kind, Kind::Exit);
        }
    }

    #[test]
    fn shifts_by_register_lower_natively_unless_conditional_or_reading_pc() {
        let words = [0xE1B0_0211, 0xE081_0332, 0x11A0_0211, 0xE1A0_021F];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        for item in &lowered.items[..2] {
            assert!(matches!(item.kind, Kind::Alu(alu) if alu.shifted()));
            assert_eq!(item.spend, Some(1 + SHIFT_IDLES));
        }
        assert_eq!(lowered.items[2].kind, Kind::Step);
        assert_eq!(lowered.items[3].kind, Kind::Step);
    }

    #[test]
    fn armv4_shifts_by_register_pay_a_nonsequential_fetch_and_an_idle() {
        let words = [0xE1B0_0211, 0xE081_0332, 0x11A0_0211, 0xE1A0_021F];
        let ops: Vec<Op> = words.iter().map(|&word| classify::<false>(word)).collect();
        let fetch = vec![[4, 1]; ops.len()];
        let mut all = words.to_vec();
        all.extend([0, 0]);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        for timed in [false, true] {
            let lowered = lower(&plan, false, false, timed).expect("lowers");
            for item in &lowered.items[..2] {
                assert!(matches!(item.kind, Kind::Alu(alu) if alu.shifted()));
                let spend = (!timed).then_some(4 + SHIFT_IDLES);
                assert_eq!(item.spend, spend);
            }
            assert_eq!(lowered.items[2].kind, Kind::Step);
            assert_eq!(lowered.items[3].kind, Kind::Step);
        }
    }

    #[test]
    fn unconditional_clz_lowers_natively_unless_reading_pc() {
        let words = [0xE16F_2F11, 0x116F_2F11, 0xE16F_2F1F];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert_eq!(lowered.items[0].kind, Kind::Clz { rd: 2, rm: 1 });
        assert_eq!(lowered.items[0].spend, Some(1));
        assert_eq!(lowered.items[1].kind, Kind::Step);
        assert_eq!(lowered.items[2].kind, Kind::Step);
    }

    #[test]
    fn cp15_cache_ops_and_the_dtcm_read_lower_natively() {
        let write = Kind::Cp15 { load: None };
        for (word, native) in [
            (0xEE07_CF9A, write),
            (0xEE07_0F3E, write),
            (0xEE07_0F3A, write),
            (0xEE07_2F5A, write),
            (0xEE07_0F36, write),
            (0xEE19_0F11, Kind::Cp15 { load: Some(0) }),
            (0xEE19_FF11, Kind::Exit),
            (0xEE07_0F90, Kind::Exit),
            (0xEE07_0F15, Kind::Exit),
            (0xEE07_0F35, Kind::Exit),
            (0xEE01_0F10, Kind::Exit),
            (0xEE19_0F31, Kind::Exit),
            (0xEE27_0F3E, Kind::Exit),
            (0xEE07_0E3E, Kind::Exit),
            (0x0E07_0F3E, Kind::Exit),
        ] {
            assert_eq!(kind(classify::<true>(word), true), native, "{word:08X}");
            assert_eq!(
                kind(classify::<true>(word), false),
                Kind::Exit,
                "{word:08X}"
            );
        }
        let (ops, fetch, all) = plan_of(&[0xEE19_3F11], 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let item = lower(&plan, true, true, false).expect("lowers").items[0];
        assert_eq!(item.kind, Kind::Cp15 { load: Some(3) });
        assert_eq!(item.spend, Some(item.cost + COPROCESSOR_READ_IDLES));
    }

    #[test]
    fn unconditional_swaps_lower_natively_unless_naming_pc() {
        let words = [
            0xE100_2091,
            0xE143_A09A,
            0x1100_2091,
            0xE10F_2091,
            0xE100_209F,
        ];
        let (ops, fetch, all) = plan_of(&words, 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let lowered = lower(&plan, true, true, false).expect("lowers");
        assert_eq!(
            lowered.items[0].kind,
            Kind::Swap(Swap {
                rd: 2,
                rm: 1,
                rn: 0,
                byte: false
            })
        );
        assert_eq!(
            lowered.items[1].kind,
            Kind::Swap(Swap {
                rd: 10,
                rm: 10,
                rn: 3,
                byte: true
            })
        );
        assert!(lowered.items[1].helper());
        for item in &lowered.items[2..] {
            assert_eq!(item.kind, Kind::Step);
        }
    }

    #[test]
    fn word_loads_into_pc_lower_to_a_load_pc_exit() {
        let pop = Transfer {
            op: 0xE49D_F004,
            rn: 13,
            rd: PC,
            width: Width::Word,
            signed: false,
            offset: Offset::Immediate(4),
        };
        let table = Transfer {
            op: 0xE793_F102,
            rn: 3,
            rd: PC,
            width: Width::Word,
            signed: false,
            offset: Offset::Shifted {
                rm: 2,
                shift: 0,
                amount: 2,
            },
        };
        for (word, native) in [
            (0xE49D_F004, Kind::LoadPc(pop)),
            (0xE793_F102, Kind::LoadPc(table)),
            (0xE59F_F018, Kind::Exit),
            (0xE5D3_F000, Kind::Exit),
            (0x059D_F000, Kind::Exit),
            (0xE793_F112, Kind::Exit),
            (0xE1D3_F0B0, Kind::Exit),
        ] {
            assert_eq!(kind(classify::<true>(word), true), native, "{word:08X}");
        }
        let (ops, fetch, all) = plan_of(&[0xE49D_F004], 1);
        let plan = Plan {
            pc: 0x0200_0000,
            thumb: false,
            words: &all,
            ops: &ops,
            fetch: &fetch,
        };
        let item = lower(&plan, true, true, false).expect("lowers").items[0];
        assert_eq!(item.kind, Kind::LoadPc(pop));
        assert_eq!(item.spend, None);
        let item = lower(&plan, false, true, false).expect("lowers").items[0];
        assert_eq!(item.kind, Kind::Exit);
    }

    #[test]
    fn exit_words_pack_every_field() {
        let info = Info {
            at: 7,
            fetch: Fetch::Known(1),
            force: true,
            branch: false,
            last: Last::Old(3),
            pending: 12,
        };
        assert_eq!(
            info.word(),
            Some(7 | 1 << 8 | 1 << 10 | 2 << 12 | 3 << 14 | 12 << 22)
        );
        let long = Info {
            pending: 0x400,
            ..info
        };
        assert_eq!(long.word(), None);
    }
}
