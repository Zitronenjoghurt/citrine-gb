use super::{CYCLES_PER_FRAME, Gba};
use crate::debug::breakpoints::Breakpoints;
use crate::debug::code::{Address, Line};
use crate::debug::memory::MemoryBlock;
use crate::debug::print::PrintLog;
use crate::debug::registers::RegisterGroup;
use crate::debug::step::{Step, Stop};
use crate::debug::trace::TraceLog;
use crate::debug::video::{Palette, SheetColors, Sprite, TileMap, TileSheet};
use crate::debug::{Debug, Features};
use crate::gba::cpu::Cpu;
use crate::gba::cpu::debug::FEATURES;
use crate::gba::cpu::step::Stepper;
use crate::gba::disasm::{Pc, code_address, lines};
use std::collections::VecDeque;
use std::ops::Range;

const STEP_BUDGET: u32 = (CYCLES_PER_FRAME * 2) as u32;

impl Gba {
    pub fn take_breakpoint_hit(&mut self) -> bool {
        std::mem::take(&mut self.debugger.hit_breakpoint)
    }

    pub fn step_pending(&self) -> bool {
        self.stepping.pending()
    }

    pub fn resume_step(&mut self) -> Stop {
        let mut stepping = std::mem::take(&mut self.stepping);
        let stop = stepping.resume(self, STEP_BUDGET);
        self.stepping = stepping;
        stop
    }

    pub(super) fn drain_waves(&mut self) {
        for levels in self.bus.sound_mut().take_taps() {
            self.debugger.push_waves(&levels);
        }
    }

    pub(super) fn on_fetch(&mut self) {
        if self.debugger.static_analysis_enabled {
            let pc = Pc {
                addr: self.cpu.pc(),
                thumb: self.cpu.thumb(),
            };
            self.code.on_fetch(&self.bus, pc);
        }
    }

    pub(super) fn breakpoint_after(&mut self, before: u32) -> bool {
        let hit = self.cpu.pc() != before && self.breakpoint().is_some();
        self.debugger.hit_breakpoint |= hit;
        hit
    }

    fn code_address(&self, addr: u32) -> Option<Address> {
        code_address(addr, self.bus.rom_len())
    }
}

impl Stepper for Gba {
    fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    fn step_instruction(&mut self) -> u32 {
        let mut used = 0;
        loop {
            let dma = self.bus.dma_active() && !self.bus.power().running();
            let cycles = Gba::step(self);
            used += cycles;
            if !dma || cycles == 0 {
                return used;
            }
        }
    }

    fn breakpoint(&self) -> Option<Address> {
        if self.debugger.breakpoints.is_empty() {
            return None;
        }
        self.code_address(self.cpu.pc())
            .filter(|at| self.debugger.breakpoints.contains(at))
    }

    fn line(&self) -> u16 {
        self.bus.ppu().vcount()
    }

    fn frame(&self) -> u64 {
        self.cycles / CYCLES_PER_FRAME
    }
}

impl Debug for Gba {
    fn features(&self) -> Features {
        FEATURES
    }

    fn registers(&self, out: &mut Vec<RegisterGroup>) {
        self.cpu.registers(out);
        self.io_registers(out);
    }

    fn memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
        self.gba_memory_blocks(out);
    }

    fn locate(&self, addr: u32) -> Option<Address> {
        self.gba_locate(addr)
    }

    fn peek(&self, at: Address, out: &mut [u8]) {
        self.gba_peek(at, out);
    }

    fn poke(&mut self, at: Address, bytes: &[u8]) {
        self.gba_poke(at, bytes);
    }

    fn pc(&self) -> Option<Address> {
        self.code_address(self.cpu.pc())
    }

    fn code_analysis(&self) -> bool {
        self.debugger.static_analysis_enabled
    }

    fn set_code_analysis(&mut self, on: bool) {
        self.debugger.static_analysis_enabled = on;
    }

    fn code_rows(&self) -> usize {
        self.code.rows()
    }

    fn code_row_of(&self, at: Address) -> Option<usize> {
        self.code.row_of(at)
    }

    fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>) {
        lines(&self.code, &self.bus, rows, out);
    }

    fn breakpoints(&mut self) -> &mut Breakpoints {
        &mut self.debugger.breakpoints
    }

    fn cycles(&self) -> u128 {
        u128::from(self.cycles)
    }

    fn record_waves(&mut self, on: bool) {
        self.debugger.record_channels = on;
        self.bus.sound_mut().record_taps(on);
    }

    fn waves(&self) -> &[VecDeque<f32>] {
        self.debugger.waves()
    }

    fn step(&mut self, step: Step) -> Stop {
        let mut stepping = std::mem::take(&mut self.stepping);
        let stop = stepping.start(self, step, STEP_BUDGET);
        self.stepping = stepping;
        stop
    }

    fn step_pending(&self) -> bool {
        Gba::step_pending(self)
    }

    fn resume_step(&mut self) -> Stop {
        Gba::resume_step(self)
    }

    fn cancel_step(&mut self) {
        self.stepping.cancel();
    }

    fn take_breakpoint_hit(&mut self) -> bool {
        Gba::take_breakpoint_hit(self)
    }

    fn frame_cycles(&self) -> u32 {
        CYCLES_PER_FRAME as u32
    }

    fn scanline(&self) -> u16 {
        self.bus.ppu().vcount()
    }

    fn trace(&mut self) -> &mut TraceLog {
        &mut self.debugger.trace
    }

    fn tile_sheets(&self, colors: SheetColors, out: &mut Vec<TileSheet>) {
        Gba::tile_sheets(self, colors, out);
    }

    fn tile_maps(&self, out: &mut Vec<TileMap>) {
        Gba::tile_maps(self, out);
    }

    fn sprites(&self, out: &mut Vec<Sprite>) {
        Gba::sprites(self, out);
    }

    fn palettes(&self, out: &mut Vec<Palette>) {
        Gba::palettes(self, out);
    }

    fn print_log(&mut self) -> Option<&mut PrintLog> {
        Some(self.bus.debug_print_mut().log_mut())
    }
}

mod io;
mod memory;

#[cfg(test)]
mod tests;
