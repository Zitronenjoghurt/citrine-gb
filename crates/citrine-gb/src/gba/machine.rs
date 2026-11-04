mod cart;
mod console;
#[cfg(feature = "debug")]
mod debug;
#[cfg(feature = "persistence")]
mod link;
#[cfg(feature = "persistence")]
mod rollback;

use super::bios::{Bios, BiosKind, Boot};
use super::bus::Bus;
use super::cpu::{Cpu, Exception};
use super::power::Power;
use crate::console::Keys;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;

pub const CYCLES_PER_FRAME: u64 = 280_896;
const HALT_STEP: u32 = 1232;
const WAKE_CYCLES: u32 = 1;
const FRESH_HALT_CYCLES: u32 = WAKE_CYCLES + 1;
const HELD_CYCLES: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Fault {
    Unsupported(u32),
}

pub struct Gba {
    cpu: Cpu,
    bus: Bus,
    bios: Bios,
    boot: Boot,
    keys: Keys,
    cycles: u64,
    frame_end: u64,
    fault: Option<Fault>,
    rumble: bool,
    #[cfg(feature = "debug")]
    debugger: crate::debug::Debugger,
    #[cfg(feature = "debug")]
    code: super::disasm::CodeMap,
    #[cfg(feature = "debug")]
    stepping: super::cpu::step::Stepping,
    #[cfg(feature = "rom-id")]
    rom_id: RomId,
}

impl Gba {
    pub fn new(rom: &[u8]) -> Self {
        Self::with_bios(rom, Bios::built_in(), Boot::Skip)
    }

    pub fn with_bios(rom: &[u8], bios: Bios, boot: Boot) -> Self {
        let mut bus = Bus::new(rom);
        let cpu = bios.boot(&mut bus, boot);
        Self {
            cpu,
            bus,
            bios,
            boot,
            keys: Keys::empty(),
            cycles: boot.entry_cycles(),
            frame_end: 0,
            fault: None,
            rumble: false,
            #[cfg(feature = "debug")]
            debugger: crate::debug::Debugger::default(),
            #[cfg(feature = "debug")]
            code: super::disasm::CodeMap::default(),
            #[cfg(feature = "debug")]
            stepping: super::cpu::step::Stepping::default(),
            #[cfg(feature = "rom-id")]
            rom_id: RomId::new(rom),
        }
    }

    pub fn bios(&self) -> BiosKind {
        self.bios.kind()
    }

    pub fn boot(&self) -> Boot {
        self.boot
    }

    pub fn power(&self) -> Power {
        self.bus.power().settled()
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    pub fn frame(&self) -> &[u8] {
        self.bus.ppu().frame()
    }

    pub fn fault(&self) -> Option<Fault> {
        self.fault
    }

    pub fn run_frame(&mut self) {
        if self.fault.is_some() {
            return;
        }
        self.catch_up_frame_end();
        while self.cycles < self.frame_end {
            #[cfg(feature = "debug")]
            let before = self.cpu.pc();
            if self.step() == 0 {
                break;
            }
            #[cfg(feature = "debug")]
            if self.breakpoint_after(before) {
                return;
            }
        }
        #[cfg(feature = "debug")]
        self.drain_waves();
        self.rumble = self.bus.cart_mut().take_rumble();
    }

    fn catch_up_frame_end(&mut self) {
        while self.cycles >= self.frame_end {
            self.frame_end += CYCLES_PER_FRAME;
        }
    }

    pub fn step(&mut self) -> u32 {
        if self.fault.is_some() {
            return 0;
        }
        let stopped = self.bus.power() == Power::Stopped;
        let cycles = if self.bus.power().running() {
            self.run_cpu()
        } else if let Some(cycles) = self.enter_halt() {
            cycles
        } else if self.bus.dma_active() {
            self.bus.start_cpu_step();
            let cycles = self.bus.run_dma();
            self.resume_after_dma().unwrap_or(cycles)
        } else {
            self.sleep()
        };
        if stopped {
            self.bus.elapse_interrupts(cycles);
        } else {
            self.bus.step(cycles);
            self.bus.step_video(cycles);
        }
        self.cycles += u64::from(cycles);
        cycles
    }

    pub fn soft_reset(&mut self) {
        let bios = std::mem::take(&mut self.bios);
        let fresh = Self::with_bios(self.bus.rom(), bios, self.boot);
        let old = std::mem::replace(self, fresh);
        *self.bus.backup_mut() = old.bus.backup().clone();
        *self.bus.cart_mut() = old.bus.cart().clone();
        self.bus.sound_mut().keep_settings(old.bus.sound());
        self.set_keys(old.keys);
    }

    pub fn set_keys(&mut self, keys: Keys) {
        self.keys = keys;
        self.bus.set_keys(keys);
    }

    #[inline(always)]
    fn run_cpu(&mut self) -> u32 {
        self.bus.start_cpu_step();
        if self.bus.interrupts().pending()
            && let Some(cycles) = self.cpu.interrupt(&mut self.bus, Exception::Irq)
        {
            return cycles + self.bus.dma_cycles();
        }
        #[cfg(feature = "debug")]
        self.on_fetch();
        let cycles = self.cpu.step(&mut self.bus);
        if cycles == 0 {
            self.fault = Some(Fault::Unsupported(self.next_instruction()));
        }
        cycles + self.bus.dma_cycles()
    }

    fn enter_halt(&mut self) -> Option<u32> {
        if !self.bus.settle_halt() || self.bus.dma_active() || !self.bus.interrupts().wakes() {
            return None;
        }
        self.bus.start_cpu_step();
        let entry = if self.bus.interrupts().pending() {
            self.cpu.interrupt(&mut self.bus, Exception::Irq)
        } else {
            None
        };
        self.bus.wake();
        match entry {
            Some(cycles) => Some(cycles + self.bus.dma_cycles() + FRESH_HALT_CYCLES),
            None => {
                self.cpu.hold_irq();
                Some(FRESH_HALT_CYCLES)
            }
        }
    }

    fn resume_after_dma(&mut self) -> Option<u32> {
        if self.bus.power() != Power::Halted
            || self.bus.dma_active()
            || !self.bus.woke_before(HELD_CYCLES)
        {
            return None;
        }
        self.bus.wake();
        self.bus.resume_held(WAKE_CYCLES, HELD_CYCLES);
        #[cfg(feature = "debug")]
        self.on_fetch();
        let cycles = self.cpu.step(&mut self.bus);
        let unpaid = self.bus.end_held();
        if cycles == 0 && unpaid == HELD_CYCLES {
            self.fault = Some(Fault::Unsupported(self.next_instruction()));
        }
        Some(self.bus.step_cycles())
    }

    fn sleep(&mut self) -> u32 {
        if !self.bus.interrupts().wakes() {
            return self.bus.until_event().clamp(1, HALT_STEP);
        }
        self.bus.wake();
        self.cpu.hold_irq();
        WAKE_CYCLES
    }

    fn next_instruction(&self) -> u32 {
        let width = if self.cpu.thumb() { 2 } else { 4 };
        self.cpu.reg(15).wrapping_sub(2 * width)
    }
}

#[cfg(test)]
mod tests;
