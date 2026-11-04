mod arith;
mod arm;
#[cfg(feature = "debug")]
pub mod debug;
mod exception;
mod memory;
mod psr;
mod registers;
mod shifter;
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

    fn branch(&mut self) {}

    fn bus_value(&self) -> u32;

    fn drive(&mut self, value: u32);
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
    fn data_cycles(&mut self, addr: u32, width: Width, access: Access) -> u32 {
        Bus::data_cycles(self, addr, width, access)
    }

    fn branch(&mut self) {
        Bus::branch(self);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cpu {
    regs: Registers,
    pipeline: [u32; 2],
    next_fetch: Access,
    cycles: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    irq_masked: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    irq_held: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    ldm_glitch: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    glitched_reads: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::from_state(&CpuState::default())
    }
}

impl Cpu {
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

    pub fn reset(&mut self, bus: &mut impl CpuBus) -> u32 {
        *self = Self::default();
        self.enter(bus, Exception::Reset, 0)
    }

    pub fn hold_irq(&mut self) {
        self.irq_held = true;
    }

    pub fn step(&mut self, bus: &mut impl CpuBus) -> u32 {
        if self.ldm_glitch {
            return self.step_after_ldm_glitch(bus);
        }
        self.execute(bus)
    }

    #[cold]
    fn step_after_ldm_glitch(&mut self, bus: &mut impl CpuBus) -> u32 {
        self.ldm_glitch = false;
        self.glitched_reads = true;
        let cycles = self.execute(bus);
        self.glitched_reads = false;
        cycles
    }

    #[inline(always)]
    fn operand(&self, index: usize) -> u32 {
        let value = self.regs.r[index];
        if self.glitched_reads && (8..15).contains(&index) {
            value | self.regs.user_reg(index)
        } else {
            value
        }
    }

    #[inline(always)]
    fn execute(&mut self, bus: &mut impl CpuBus) -> u32 {
        self.cycles = 0;
        self.irq_masked = self.regs.cpsr() & FLAG_I != 0;
        self.irq_held = false;
        let op = self.pipeline[0];
        if self.thumb() {
            self.execute_thumb(bus, op & 0xFFFF);
        } else if Psr(self.regs.cpsr()).passes(op >> 28) {
            self.execute_arm(bus, op);
        } else {
            self.prefetch(bus);
        }
        self.cycles
    }

    fn prefetch(&mut self, bus: &mut impl CpuBus) {
        let pc = self.regs.r[15];
        let access = self.next_fetch;
        let (width, size) = if self.thumb() {
            (Width::Half, 2)
        } else {
            (Width::Word, 4)
        };
        let next = self.code(bus, pc, width, access);
        self.pipeline = [self.pipeline[1], next];
        self.regs.r[15] = pc.wrapping_add(size);
        self.next_fetch = Access::Sequential;
    }

    fn reload(&mut self, bus: &mut impl CpuBus) {
        bus.branch();
        let (width, size) = if self.thumb() {
            self.regs.r[15] &= !1;
            (Width::Half, 2)
        } else {
            (Width::Word, 4)
        };
        let pc = self.regs.r[15];
        let first = self.code(bus, pc, width, Access::NonSequential);
        let second = self.code(bus, pc.wrapping_add(size), width, Access::Sequential);
        self.pipeline = [first, second];
        self.regs.r[15] = pc.wrapping_add(size * 2);
        self.next_fetch = Access::Sequential;
        self.irq_masked = self.regs.cpsr() & FLAG_I != 0;
    }

    #[inline(always)]
    fn code(&mut self, bus: &mut impl CpuBus, addr: u32, width: Width, access: Access) -> u32 {
        self.cycles += bus.code_cycles(addr, width, access);
        bus.fetch(addr & !(width.bytes() as u32 - 1), width, access)
    }

    fn load(&mut self, bus: &mut impl CpuBus, addr: u32, width: Width, access: Access) -> u32 {
        self.cycles += bus.data_cycles(addr, width, access);
        self.next_fetch = Access::NonSequential;
        bus.read(addr, width, access)
    }

    fn store(
        &mut self,
        bus: &mut impl CpuBus,
        addr: u32,
        width: Width,
        value: u32,
        access: Access,
    ) {
        self.cycles += bus.data_cycles(addr, width, access);
        self.next_fetch = Access::NonSequential;
        bus.write(addr, width, value, access);
    }

    fn idle(&mut self, bus: &mut impl CpuBus) {
        self.cycles += 1;
        self.next_fetch = Access::NonSequential;
        bus.idle();
    }
}

#[cfg(test)]
mod tests;
