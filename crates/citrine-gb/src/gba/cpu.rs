mod arith;
mod arm;
#[cfg(feature = "debug")]
pub mod debug;
mod exception;
pub(crate) mod memory;
pub(crate) mod psr;
pub(crate) mod registers;
pub(crate) mod shifter;
#[cfg(feature = "debug")]
pub mod step;
mod thumb;

#[cfg(feature = "debug")]
pub use debug::FEATURES;
pub use exception::Exception;
pub use psr::Mode;
pub use registers::CpuState;

use super::bus::Bus;
use super::wait::{Access, Width};
use psr::{FLAG_I, FLAG_T, Psr};
use registers::Registers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoprocessorRegister {
    pub coprocessor: u32,
    pub opcode1: u32,
    pub crn: u32,
    pub crm: u32,
    pub opcode2: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    Branch,
    BlockLoad,
}

pub trait CpuBus {
    fn fetch(&mut self, addr: u32, width: Width, access: Access) -> u32;
    fn read(&mut self, addr: u32, width: Width, access: Access) -> u32;
    fn write(&mut self, addr: u32, width: Width, value: u32, access: Access);
    fn idle(&mut self);
    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32;

    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.cycles(addr, width, access)
    }

    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        self.cycles(addr, width, access)
    }

    #[inline(always)]
    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let cycles = self.code_cycles(addr, width, access);
        (
            cycles,
            self.fetch(addr & !(width.bytes() as u32 - 1), width, access),
        )
    }

    #[inline(always)]
    fn quick_fetch(&mut self, addr: u32, width: Width) -> Option<(u32, u32)> {
        Some(self.timed_fetch(addr, width, Access::Sequential))
    }

    #[inline(always)]
    fn thumb_words(&self) -> bool {
        false
    }

    #[inline(always)]
    fn near_jumps(&self) -> bool {
        false
    }

    #[inline(always)]
    fn near_jump(
        &mut self,
        _pc: u32,
        _pc_width: Width,
        _access: Access,
        _target: u32,
        _width: Width,
    ) -> Option<(u32, [u32; 2])> {
        None
    }

    #[inline(always)]
    fn near_blocks(&self) -> bool {
        false
    }

    #[inline(always)]
    fn near_block_words(
        &mut self,
        _pc: u32,
        _width: Width,
        _access: Access,
        _addr: u32,
        _count: usize,
    ) -> Option<(u32, u32, &[[u8; 4]])> {
        None
    }

    #[inline(always)]
    fn near_block_words_mut(
        &mut self,
        _pc: u32,
        _width: Width,
        _access: Access,
        _addr: u32,
        _count: usize,
    ) -> Option<(u32, u32, &mut [[u8; 4]])> {
        None
    }

    #[inline(always)]
    fn dropped_fetch(&mut self, addr: u32, width: Width) {
        self.timed_fetch(addr, width, Access::Sequential);
    }

    #[inline(always)]
    fn timed_refill(&mut self, addr: u32, width: Width) -> [(u32, u32); 2] {
        let next = addr.wrapping_add(width.bytes() as u32);
        [
            self.timed_fetch(addr, width, Access::NonSequential),
            self.timed_fetch(next, width, Access::Sequential),
        ]
    }

    #[inline(always)]
    fn quick_write(&mut self, _addr: u32, _width: Width, _value: u32) -> Option<u32> {
        None
    }

    #[inline(always)]
    fn near_fetch(&self, _addr: u32, _width: Width) -> Option<(u32, u32)> {
        None
    }

    #[inline(always)]
    fn near_read(&self, _addr: u32, _width: Width) -> Option<(u32, u32)> {
        None
    }

    #[inline(always)]
    fn near_words(&self, _addr: u32, _count: usize) -> Option<(u32, &[[u8; 4]])> {
        None
    }

    #[inline(always)]
    fn near_words_mut(&mut self, _addr: u32, _count: usize) -> Option<(u32, &mut [[u8; 4]])> {
        None
    }

    #[inline(always)]
    fn near_drop(&self, _addr: u32) -> bool {
        false
    }

    #[inline(always)]
    fn near_refill(&self, _addr: u32, _width: Width) -> Option<[(u32, u32); 2]> {
        None
    }

    #[inline(always)]
    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        let cycles = self.data_cycles(addr, width, access);
        (cycles, self.read(addr, width, access))
    }

    #[inline(always)]
    fn timed_write(&mut self, addr: u32, width: Width, value: u32, access: Access) -> u32 {
        let cycles = self.data_cycles(addr, width, access);
        self.write(addr, width, value, access);
        cycles
    }

    #[inline(always)]
    fn timed_read_block(&mut self, addr: u32, values: &mut [u32]) -> u32 {
        read_words(self, addr, values)
    }

    #[inline(always)]
    fn timed_write_block(&mut self, addr: u32, values: &[u32]) -> u32 {
        write_words(self, addr, values)
    }

    fn branch(&mut self) {}

    fn jump(&mut self, _jump: Jump) {}

    fn settle(&mut self, cycles: u32) -> u32 {
        cycles
    }

    fn bus_value(&self) -> u32;

    fn drive(&mut self, value: u32);

    fn coprocessor_read(&mut self, _register: CoprocessorRegister) -> Option<u32> {
        None
    }

    fn coprocessor_write(&mut self, _register: CoprocessorRegister, _value: u32) -> bool {
        false
    }

    fn vector_base(&self) -> u32 {
        0
    }
}

#[inline(always)]
pub fn read_words(bus: &mut (impl CpuBus + ?Sized), addr: u32, values: &mut [u32]) -> u32 {
    let mut cycles = 0;
    let mut access = Access::NonSequential;
    let mut addr = addr;
    for value in values {
        let (spent, read) = bus.timed_read(addr, Width::Word, access);
        cycles += spent;
        *value = read;
        access = Access::Sequential;
        addr = addr.wrapping_add(4);
    }
    cycles
}

#[inline(always)]
pub fn write_words(bus: &mut (impl CpuBus + ?Sized), addr: u32, values: &[u32]) -> u32 {
    let mut cycles = 0;
    let mut access = Access::NonSequential;
    let mut addr = addr;
    for &value in values {
        cycles += bus.timed_write(addr, Width::Word, value, access);
        access = Access::Sequential;
        addr = addr.wrapping_add(4);
    }
    cycles
}

impl CpuBus for Bus {
    fn bus_value(&self) -> u32 {
        self.open_bus_value()
    }

    fn drive(&mut self, value: u32) {
        self.drive_open_bus(value);
    }

    fn fetch(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.fetch_code(addr, width)
    }

    fn read(&mut self, addr: u32, width: Width, _access: Access) -> u32 {
        self.cpu_read(addr, width)
    }

    fn write(&mut self, addr: u32, width: Width, value: u32, _access: Access) {
        self.cpu_write(addr, width, value);
    }

    #[inline(always)]
    fn idle(&mut self) {
        self.cpu_idle();
    }

    fn cycles(&self, addr: u32, width: Width, access: Access) -> u32 {
        Bus::cycles(self, addr, width, access)
    }

    #[inline(always)]
    fn code_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        Bus::code_cycles(self, addr, width, access)
    }

    #[inline(always)]
    fn timed_fetch(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        self.timed_code(addr, width, access)
    }

    #[inline(always)]
    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        Bus::data_cycles(self, addr, width, access)
    }

    #[inline(always)]
    fn timed_read(&mut self, addr: u32, width: Width, access: Access) -> (u32, u32) {
        self.timed_data(addr, width, access)
    }

    fn branch(&mut self) {
        Bus::branch(self);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Core<const V5: bool> {
    pub(crate) regs: Registers,
    pub(crate) pipeline: [u32; 2],
    pub(crate) next_fetch: Access,
    pub(crate) cycles: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) irq_masked: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) irq_held: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub(crate) ldm_glitch: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    glitched_reads: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) fetched: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) data: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) refetched: Refetch,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) word: CachedWord,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) landed: Landed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Landed(u32);

impl Default for Landed {
    fn default() -> Self {
        Self(u32::MAX)
    }
}

impl PartialEq for Landed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Landed {}

pub const END_LINE: u32 = 0x80;

pub struct Overshoot {
    left: u32,
    mark: Landed,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CachedWord {
    at: u32,
    value: u32,
}

impl CachedWord {
    pub(crate) const NONE: Self = Self {
        at: u32::MAX,
        value: 0,
    };
}

impl Default for CachedWord {
    fn default() -> Self {
        Self::NONE
    }
}

impl PartialEq for CachedWord {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for CachedWord {}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Refetch(pub(crate) u32);

impl PartialEq for Refetch {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Refetch {}

const DATA_OVERLAP: u32 = 6;

pub type Cpu = Core<false>;

pub type Arm9 = Core<true>;

impl<const V5: bool> Default for Core<V5> {
    fn default() -> Self {
        Self::from_state(&CpuState::default())
    }
}

impl<const V5: bool> Core<V5> {
    pub fn from_state(state: &CpuState) -> Self {
        Self {
            regs: Registers::from_state(state),
            pipeline: state.pipeline,
            next_fetch: state.next_fetch,
            cycles: 0,
            irq_masked: state.cpsr & FLAG_I != 0,
            irq_held: false,
            ldm_glitch: false,
            glitched_reads: false,
            fetched: 0,
            data: 0,
            refetched: Refetch(0),
            word: CachedWord::NONE,
            landed: Landed::default(),
        }
    }

    pub fn state(&self) -> CpuState {
        let mut state = self.regs.state();
        state.pipeline = self.pipeline;
        state.next_fetch = self.next_fetch;
        state
    }

    pub fn reg(&self, index: usize) -> u32 {
        self.regs.r[index]
    }

    pub fn cpsr(&self) -> u32 {
        self.regs.cpsr()
    }

    pub fn mode(&self) -> Option<Mode> {
        Mode::from_bits(self.regs.cpsr())
    }

    pub fn thumb(&self) -> bool {
        self.regs.cpsr() & FLAG_T != 0
    }

    pub fn pc(&self) -> u32 {
        let size = if self.thumb() { 2 } else { 4 };
        self.regs.r[15].wrapping_sub(size * 2)
    }

    pub fn pipeline(&self) -> [u32; 2] {
        self.pipeline
    }

    pub fn landed_here(mut self) -> Self {
        self.landed = Landed(self.pc());
        self
    }

    pub fn ends(&self) -> bool {
        let pc = self.pc();
        self.landed.0 == pc || pc.is_multiple_of(END_LINE)
    }

    #[inline(always)]
    pub fn overshoot(&mut self) -> Overshoot {
        let size = if self.thumb() { 2 } else { 4 };
        let pc = self.pc();
        let line = (END_LINE - pc % END_LINE) / size;
        let ahead = self.landed.0.wrapping_sub(pc);
        let left = if ahead.is_multiple_of(size) {
            line.min(ahead / size)
        } else {
            line
        };
        let mark = std::mem::take(&mut self.landed);
        Overshoot { left, mark }
    }

    #[inline(always)]
    pub fn overshot(&self, over: &mut Overshoot) -> bool {
        over.left -= 1;
        over.left == 0 || self.landed.0 != u32::MAX
    }

    #[inline(always)]
    pub fn end_overshoot(&mut self, over: Overshoot) {
        if self.landed.0 == u32::MAX {
            self.landed = over.mark;
        }
    }

    pub fn reset(&mut self, bus: &mut impl CpuBus) -> u32 {
        *self = Self::default();
        self.enter(bus, Exception::Reset, 0)
    }

    pub fn hold_irq(&mut self) {
        debug_assert!(!V5);
        self.irq_held = true;
    }

    pub fn runs_like(&self, other: &Self) -> bool {
        self.regs == other.regs
            && self.pipeline == other.pipeline
            && self.next_fetch == other.next_fetch
            && self.ldm_glitch == other.ldm_glitch
    }

    pub fn step(&mut self, bus: &mut impl CpuBus) -> u32 {
        self.step_inlined(bus)
    }

    #[inline(always)]
    pub fn step_inlined(&mut self, bus: &mut impl CpuBus) -> u32 {
        self.execute(bus)
    }

    #[cold]
    #[inline(never)]
    fn execute_arm_after_ldm_glitch(&mut self, bus: &mut impl CpuBus, op: u32) {
        self.ldm_glitch = false;
        self.glitched_reads = true;
        if op >> 28 == 0xE || Psr(self.regs.cpsr()).passes(op >> 28) {
            self.execute_arm(bus, op);
        } else {
            self.skip_arm(bus);
        }
        self.glitched_reads = false;
    }

    #[inline(always)]
    fn operand(&self, index: usize) -> u32 {
        let value = self.regs.r[index];
        if !V5 && self.glitched_reads && (8..15).contains(&index) {
            value | self.regs.user_reg(index)
        } else {
            value
        }
    }

    #[inline(always)]
    pub(crate) fn execute(&mut self, bus: &mut impl CpuBus) -> u32 {
        self.cycles = 0;
        self.irq_masked = self.regs.cpsr() & FLAG_I != 0;
        if !V5 {
            self.irq_held = false;
        }
        let op = self.pipeline[0];
        if self.thumb() {
            self.execute_thumb(bus, op & 0xFFFF);
        } else if !V5 && self.ldm_glitch {
            self.execute_arm_after_ldm_glitch(bus, op);
        } else if op >> 28 == 0xE || Psr(self.regs.cpsr()).passes(op >> 28) {
            self.execute_arm(bus, op);
        } else if V5 && op >> 28 == 0xF {
            self.execute_arm_unconditional(bus, op);
        } else {
            self.skip_arm(bus);
        }
        let cycles = if V5 { self.overlap() } else { self.cycles };
        self.cycles = bus.settle(cycles);
        self.cycles
    }

    fn overlap(&mut self) -> u32 {
        let (code, data) = (self.fetched, self.data);
        self.fetched = 0;
        self.cycles
            + if data == 0 {
                code
            } else {
                self.data = 0;
                (code + data)
                    .saturating_sub(DATA_OVERLAP)
                    .max(code)
                    .max(data)
            }
    }

    #[inline(never)]
    fn skip_arm(&mut self, bus: &mut impl CpuBus) {
        self.prefetch_arm(bus);
    }

    #[inline(always)]
    fn prefetch_arm(&mut self, bus: &mut impl CpuBus) {
        self.advance(bus, Width::Word);
    }

    #[inline(always)]
    fn prefetch_thumb(&mut self, bus: &mut impl CpuBus) {
        self.advance(bus, Width::Half);
    }

    #[inline(always)]
    fn advance(&mut self, bus: &mut impl CpuBus, width: Width) {
        if !V5 {
            return self.advance_v4(bus, width);
        }
        let pc = self.regs.r[15];
        match self.quick_next(bus, pc, width) {
            Some((cycles, next)) => self.advanced(pc, width, cycles, next),
            None => self.advance_elsewhere(bus, width),
        }
    }

    #[inline(always)]
    fn quick_next(&mut self, bus: &mut impl CpuBus, pc: u32, width: Width) -> Option<(u32, u32)> {
        if width == Width::Half && bus.thumb_words() {
            if pc & 2 == 0 {
                let (cycles, word) = bus.quick_fetch(pc, Width::Word)?;
                self.word = CachedWord {
                    at: pc,
                    value: word,
                };
                return Some((cycles, word & 0xFFFF));
            }
            if self.word.at == pc & !3 {
                return Some((0, self.word.value >> 16));
            }
        }
        bus.quick_fetch(pc, width)
    }

    #[inline(always)]
    fn near_thumb_next(
        &self,
        bus: &mut impl CpuBus,
        pc: u32,
    ) -> Option<(u32, u32, Option<CachedWord>)> {
        if bus.thumb_words() {
            if pc & 2 == 0 {
                let (cycles, word) = bus.near_fetch(pc, Width::Word)?;
                let cached = CachedWord {
                    at: pc,
                    value: word,
                };
                return Some((cycles, word & 0xFFFF, Some(cached)));
            }
            if self.word.at == pc & !3 {
                return Some((0, self.word.value >> 16, None));
            }
        }
        let (cycles, next) = bus.near_fetch(pc, Width::Half)?;
        Some((cycles, next, None))
    }

    #[inline(never)]
    fn advance_elsewhere(&mut self, bus: &mut impl CpuBus, width: Width) {
        let pc = self.regs.r[15];
        let (cycles, next) = bus.timed_fetch(pc, width, Access::Sequential);
        self.advanced(pc, width, cycles, next);
    }

    #[inline(always)]
    fn advance_quickly(&mut self, bus: &mut impl CpuBus, width: Width) -> bool {
        let pc = self.regs.r[15];
        match self.near_next(bus, pc, width) {
            Some(near) => {
                self.take_next(pc, width, near);
                true
            }
            None => false,
        }
    }

    #[inline(always)]
    fn near_next(
        &self,
        bus: &mut impl CpuBus,
        pc: u32,
        width: Width,
    ) -> Option<(u32, u32, Option<CachedWord>)> {
        if width == Width::Half {
            self.near_thumb_next(bus, pc)
        } else {
            bus.near_fetch(pc, width)
                .map(|(cycles, next)| (cycles, next, None))
        }
    }

    #[inline(always)]
    fn take_next(
        &mut self,
        pc: u32,
        width: Width,
        (cycles, next, word): (u32, u32, Option<CachedWord>),
    ) {
        if let Some(word) = word {
            self.word = word;
        }
        self.advanced(pc, width, cycles, next);
    }

    #[inline(always)]
    fn advanced(&mut self, pc: u32, width: Width, cycles: u32, next: u32) {
        self.fetched = cycles;
        self.pipeline = [self.pipeline[1], next];
        self.regs.r[15] = pc.wrapping_add(width.bytes() as u32);
        self.next_fetch = Access::Sequential;
    }

    #[inline(always)]
    fn prefetch_arm_before_jump(&mut self, bus: &mut impl CpuBus) {
        self.prefetch_before_jump(bus, Width::Word);
    }

    #[inline(always)]
    fn prefetch_thumb_before_jump(&mut self, bus: &mut impl CpuBus) {
        self.prefetch_before_jump(bus, Width::Half);
    }

    #[inline(always)]
    fn prefetch_before_jump(&mut self, bus: &mut impl CpuBus, width: Width) {
        if V5 {
            bus.dropped_fetch(self.regs.r[15], width);
        } else {
            self.advance_v4(bus, width);
        }
    }

    #[inline(always)]
    fn advance_v4(&mut self, bus: &mut impl CpuBus, width: Width) {
        let pc = self.regs.r[15];
        let next = self.code(bus, pc, width, self.next_fetch);
        self.pipeline = [self.pipeline[1], next];
        self.regs.r[15] = pc.wrapping_add(width.bytes() as u32);
        self.next_fetch = Access::Sequential;
    }

    pub(crate) fn reload(&mut self, bus: &mut impl CpuBus) {
        if self.thumb() {
            self.reload_thumb(bus);
        } else {
            self.reload_arm(bus);
        }
    }

    #[inline(never)]
    fn reload_arm(&mut self, bus: &mut impl CpuBus) {
        self.refill(bus, Width::Word);
    }

    #[inline(never)]
    fn reload_thumb(&mut self, bus: &mut impl CpuBus) {
        self.regs.r[15] &= !1;
        self.refill(bus, Width::Half);
    }

    #[inline(always)]
    fn refill(&mut self, bus: &mut impl CpuBus, width: Width) {
        bus.branch();
        if V5 && width == Width::Half {
            self.word = CachedWord::NONE;
        }
        let size = width.bytes() as u32;
        let pc = self.regs.r[15];
        let (first, second) = if V5 {
            let [(first_cycles, first), (second_cycles, second)] = bus.timed_refill(pc, width);
            self.cycles += first_cycles + second_cycles;
            self.refetched = Refetch(second_cycles);
            (first, second)
        } else {
            let first = self.code(bus, pc, width, Access::NonSequential);
            let second = self.code(bus, pc.wrapping_add(size), width, Access::Sequential);
            (first, second)
        };
        self.pipeline = [first, second];
        self.landed = Landed(pc);
        self.regs.r[15] = pc.wrapping_add(size * 2);
        self.next_fetch = Access::Sequential;
        self.irq_masked = self.regs.cpsr() & FLAG_I != 0;
    }

    #[inline(always)]
    fn jump_arm_nearby(&mut self, bus: &mut impl CpuBus, pc: u32, target: u32) -> bool {
        self.jump_nearby(bus, pc, target, Width::Word)
    }

    #[inline(always)]
    fn jump_thumb_nearby(&mut self, bus: &mut impl CpuBus, pc: u32, target: u32) -> bool {
        self.jump_nearby(bus, pc, target, Width::Half)
    }

    #[inline(always)]
    fn jump_nearby(&mut self, bus: &mut impl CpuBus, pc: u32, target: u32, width: Width) -> bool {
        if !bus.near_drop(pc) {
            return false;
        }
        let Some(fetches) = bus.near_refill(target, width) else {
            return false;
        };
        self.land_nearby(bus, target, width, fetches);
        true
    }

    #[inline(always)]
    fn exchange_nearby(&mut self, bus: &mut impl CpuBus, pc: u32, target: u32) -> bool {
        bus.near_drop(pc) && self.exchange_landed(bus, target)
    }

    #[inline(always)]
    fn exchange_landed(&mut self, bus: &mut impl CpuBus, target: u32) -> bool {
        let (width, target) = if target & 1 != 0 {
            (Width::Half, target & !1)
        } else {
            (Width::Word, target)
        };
        let Some(fetches) = bus.near_refill(target, width) else {
            return false;
        };
        let cpsr = self.regs.cpsr();
        self.regs.set_flags(if width == Width::Half {
            cpsr | FLAG_T
        } else {
            cpsr & !FLAG_T
        });
        self.land_nearby(bus, target, width, fetches);
        true
    }

    #[inline(always)]
    fn jump_v4_nearby(
        &mut self,
        bus: &mut impl CpuBus,
        pc: u32,
        pc_width: Width,
        target: u32,
        width: Width,
    ) -> bool {
        let Some((cycles, [first, second])) =
            bus.near_jump(pc, pc_width, self.next_fetch, target, width)
        else {
            return false;
        };
        self.cycles += cycles;
        self.pipeline = [first, second];
        self.landed = Landed(target);
        self.regs.r[15] = target.wrapping_add(width.bytes() as u32 * 2);
        self.next_fetch = Access::Sequential;
        self.irq_masked = self.regs.cpsr() & FLAG_I != 0;
        true
    }

    #[inline(always)]
    fn exchange_v4_nearby(&mut self, bus: &mut impl CpuBus, pc: u32, target: u32) -> bool {
        let (width, target) = if target & 1 != 0 {
            (Width::Half, target & !1)
        } else {
            (Width::Word, target)
        };
        if !self.jump_v4_nearby(bus, pc, Width::Word, target, width) {
            return false;
        }
        let cpsr = self.regs.cpsr();
        self.regs.set_flags(if width == Width::Half {
            cpsr | FLAG_T
        } else {
            cpsr & !FLAG_T
        });
        true
    }

    #[inline(always)]
    fn land_nearby(
        &mut self,
        bus: &mut impl CpuBus,
        target: u32,
        width: Width,
        [(first_cycles, first), (second_cycles, second)]: [(u32, u32); 2],
    ) {
        bus.branch();
        self.fetched = 0;
        if width == Width::Half {
            self.word = CachedWord::NONE;
        }
        self.cycles += first_cycles + second_cycles;
        self.refetched = Refetch(second_cycles);
        self.pipeline = [first, second];
        self.landed = Landed(target);
        self.regs.r[15] = target.wrapping_add(width.bytes() as u32 * 2);
        self.next_fetch = Access::Sequential;
        self.irq_masked = self.regs.cpsr() & FLAG_I != 0;
    }

    #[inline(always)]
    fn code(&mut self, bus: &mut impl CpuBus, addr: u32, width: Width, access: Access) -> u32 {
        let (cycles, value) = bus.timed_fetch(addr, width, access);
        self.cycles += cycles;
        value
    }

    #[cfg(jit)]
    pub(crate) fn jump(&mut self, bus: &mut impl CpuBus) {
        self.flush(bus);
        self.reload(bus);
    }

    fn jump_arm(&mut self, bus: &mut impl CpuBus) {
        self.flush(bus);
        self.reload_arm(bus);
    }

    #[inline(always)]
    fn jump_arm_inlined(&mut self, bus: &mut impl CpuBus) {
        self.flush(bus);
        self.refill(bus, Width::Word);
    }

    fn jump_thumb(&mut self, bus: &mut impl CpuBus) {
        self.flush(bus);
        self.reload_thumb(bus);
    }

    #[inline(always)]
    fn jump_thumb_inlined(&mut self, bus: &mut impl CpuBus) {
        self.flush(bus);
        self.regs.r[15] &= !1;
        self.refill(bus, Width::Half);
    }

    #[inline(always)]
    fn jump_thumb_aligned(&mut self, bus: &mut impl CpuBus) {
        self.flush(bus);
        self.refill(bus, Width::Half);
    }

    #[inline(always)]
    fn flush(&mut self, bus: &mut impl CpuBus) {
        if V5 {
            self.fetched = 0;
        } else {
            bus.jump(Jump::Branch);
        }
    }

    fn take_refetch(&mut self) {
        if V5 {
            self.fetched = self.refetched.0;
        }
    }

    pub(crate) fn load(
        &mut self,
        bus: &mut impl CpuBus,
        addr: u32,
        width: Width,
        access: Access,
    ) -> u32 {
        let (cycles, value) = bus.timed_read(addr, width, access);
        if V5 {
            self.data += cycles;
        } else {
            self.cycles += cycles;
        }
        self.next_fetch = Access::NonSequential;
        value
    }

    pub(crate) fn store(
        &mut self,
        bus: &mut impl CpuBus,
        addr: u32,
        width: Width,
        value: u32,
        access: Access,
    ) {
        let cycles = bus.timed_write(addr, width, value, access);
        if V5 {
            self.data += cycles;
        } else {
            self.cycles += cycles;
        }
        self.next_fetch = Access::NonSequential;
    }

    #[inline(always)]
    fn store_quickly(
        &mut self,
        bus: &mut impl CpuBus,
        addr: u32,
        width: Width,
        value: u32,
    ) -> bool {
        match bus.quick_write(addr, width, value) {
            Some(cycles) => {
                self.data += cycles;
                self.next_fetch = Access::NonSequential;
                true
            }
            None => false,
        }
    }

    #[inline(never)]
    fn store_elsewhere(&mut self, bus: &mut impl CpuBus, addr: u32, width: Width, value: u32) {
        self.store(bus, addr, width, value, Access::NonSequential);
    }

    fn idle(&mut self, bus: &mut impl CpuBus) {
        self.cycles += 1;
        self.next_fetch = Access::NonSequential;
        bus.idle();
    }

    fn idle_for(&mut self, bus: &mut impl CpuBus, cycles: u32) {
        for _ in 0..cycles {
            self.idle(bus);
        }
    }

    pub(crate) fn load_idle(&mut self, bus: &mut impl CpuBus) {
        if V5 {
            self.next_fetch = Access::NonSequential;
        } else {
            self.idle(bus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn boot(words: &[u32]) -> (Cpu, Bus) {
        let mut bus = Bus::new(&rom(words));
        let mut cpu = Cpu::default();
        cpu.reset(&mut bus);
        let mut state = cpu.state();
        state.r[15] = 0x0800_0000;
        let mut cpu = Cpu::from_state(&state);
        cpu.reload(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn reset_enters_supervisor_mode_at_the_reset_vector() {
        let mut bus = Bus::new(&[]);
        let mut cpu = Cpu::default();
        cpu.reset(&mut bus);
        assert_eq!(cpu.mode(), Some(Mode::Supervisor));
        assert_eq!(cpu.cpsr() & 0xFF, 0xD3);
        assert_eq!(cpu.reg(15), 8);
    }

    #[test]
    fn runs_a_counting_loop_from_rom() {
        let (mut cpu, mut bus) = boot(&[
            0xE3A0_0000,
            0xE280_0001,
            0xE350_0005,
            0x1AFF_FFFC,
            0xEAFF_FFFE,
        ]);
        for _ in 0..20 {
            cpu.step(&mut bus);
        }
        assert_eq!(cpu.reg(0), 5);
        assert_eq!(cpu.reg(15), 0x0800_0010 + 8);
    }

    #[test]
    fn store_and_load_go_through_the_bus() {
        let (mut cpu, mut bus) = boot(&[0xE3A0_0403, 0xE3A0_102A, 0xE580_1004, 0xE590_2004]);
        for _ in 0..4 {
            cpu.step(&mut bus);
        }
        assert_eq!(bus.read32(0x0300_0004), 42);
        assert_eq!(cpu.reg(2), 42);
    }

    #[test]
    fn bx_to_an_odd_address_runs_a_thumb_counting_loop() {
        let (mut cpu, mut bus) = boot(&[
            0xE28F_0005,
            0xE12F_FF10,
            0x0000_0000,
            0x3001_2000,
            0xD1FC_2805,
            0x0000_E7FE,
        ]);
        for _ in 0..30 {
            cpu.step(&mut bus);
        }
        assert!(cpu.thumb());
        assert_eq!(cpu.reg(0), 5);
        assert_eq!(cpu.reg(15), 0x0800_0014 + 4);
    }

    #[test]
    fn banked_registers_survive_a_round_trip_through_irq_mode() {
        let (mut cpu, mut bus) = boot(&[0xE3A0_D00A, 0xE3A0_E00B]);
        cpu.step(&mut bus);
        cpu.step(&mut bus);
        let mut state = cpu.state();
        state.cpsr &= !0x80;
        let mut cpu = Cpu::from_state(&state);
        cpu.interrupt(&mut bus, Exception::Irq);
        assert_eq!(cpu.mode(), Some(Mode::Irq));
        let state = cpu.state();
        assert_eq!(state.svc, [10, 11]);
        assert_eq!(state.irq[1], 0x0800_0008 + 4);
    }

    #[test]
    fn thumb_swi_and_undefined_enter_their_modes_from_the_table() {
        for (op, mode, vector) in [
            (0xDF00, Mode::Supervisor, 0x08),
            (0xDE00, Mode::Undefined, 0x04),
            (0xBE00, Mode::Undefined, 0x04),
        ] {
            let (mut cpu, mut bus) = boot(&[0xE28F_0001, 0xE12F_FF10, op]);
            for _ in 0..3 {
                cpu.step(&mut bus);
            }
            assert_eq!(cpu.mode(), Some(mode), "{op:04X}");
            assert_eq!(cpu.reg(14), 0x0800_000A, "{op:04X}");
            assert_eq!(cpu.reg(15), vector + 8, "{op:04X}");
        }
    }
}
