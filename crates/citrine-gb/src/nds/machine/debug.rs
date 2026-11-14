use super::{Nds, scaled, side};
use crate::debug::breakpoints::Breakpoints;
use crate::debug::code::{Address, Line};
use crate::debug::memory::MemoryBlock;
use crate::debug::print::PrintLog;
use crate::debug::registers::RegisterGroup;
use crate::debug::step::{Step, Stop};
use crate::debug::trace::TraceLog;
use crate::debug::video::{Palette, SheetColors, Sprite, TileMap, TileSheet};
use crate::debug::{Debug, Features};
use crate::gba::cpu::step::{Stepper, Stepping};
use crate::gba::cpu::{Arm9, Cpu as Arm7, FEATURES};
use crate::gba::disasm::{CodeMap, CodeSource, Pc, lines};
use crate::gba::wait::Width;
use crate::nds::Cpu;
use crate::nds::bus::Buses;
use crate::nds::lcd::FRAME_CYCLES;
use std::collections::VecDeque;
use std::ops::Range;

mod io;
mod memory;

const CPUS: [Cpu; 2] = [Cpu::Arm9, Cpu::Arm7];
const STEP_BUDGET: u32 = FRAME_CYCLES * 2;

const ARM9_FEATURES: Features = Features {
    cpus: &["ARM9", "ARM7"],
    vectors: &[
        ("Reset", 0xFFFF_0000),
        ("Undefined", 0xFFFF_0004),
        ("SWI", 0xFFFF_0008),
        ("Prefetch abort", 0xFFFF_000C),
        ("Data abort", 0xFFFF_0010),
        ("IRQ", 0xFFFF_0018),
        ("FIQ", 0xFFFF_001C),
    ],
    ..FEATURES
};

const ARM7_FEATURES: Features = Features {
    cpus: ARM9_FEATURES.cpus,
    ..FEATURES
};

#[derive(Default)]
pub(super) struct NdsDebug {
    selected: usize,
    breakpoints: [Breakpoints; 2],
    code: [CodeMap; 2],
    stepping: Stepping,
}

impl NdsDebug {
    #[cfg(jit)]
    pub(super) fn no_breakpoints(&self, cpu: Cpu) -> bool {
        self.breakpoints[side(cpu)].is_empty()
    }

    pub(super) fn after_reset(self) -> Self {
        Self {
            selected: self.selected,
            breakpoints: self.breakpoints,
            ..Self::default()
        }
    }
}

struct Code<'a> {
    buses: &'a Buses,
    cpu: Cpu,
}

impl CodeSource for Code<'_> {
    fn read_code(&self, addr: u32, width: Width) -> u32 {
        memory::read(self.buses, self.cpu, addr, width)
    }

    fn rom_len(&self) -> usize {
        0
    }

    fn locate(&self, addr: u32) -> Option<Address> {
        memory::locate(self.buses, self.cpu, addr)
    }

    fn v5(&self) -> bool {
        self.cpu == Cpu::Arm9
    }
}

impl Nds {
    fn selected(&self) -> Cpu {
        CPUS[self.debug.selected]
    }

    fn code_source(&self, cpu: Cpu) -> Code<'_> {
        Code {
            buses: &self.buses,
            cpu,
        }
    }

    fn fetch_pc(&self, cpu: Cpu) -> Pc {
        let (addr, thumb) = match cpu {
            Cpu::Arm9 => (self.arm9.pc(), self.arm9.thumb()),
            Cpu::Arm7 => (self.arm7.pc(), self.arm7.thumb()),
        };
        Pc { addr, thumb }
    }

    fn analyze(&mut self, cpu: Cpu) {
        if !self.debugger.static_analysis_enabled {
            return;
        }
        let pc = self.fetch_pc(cpu);
        let source = Code {
            buses: &self.buses,
            cpu,
        };
        self.debug.code[side(cpu)].on_fetch(&source, pc);
    }

    fn breakpoint_on(&self, cpu: Cpu) -> Option<Address> {
        let breakpoints = &self.debug.breakpoints[side(cpu)];
        if breakpoints.is_empty() {
            return None;
        }
        memory::locate(&self.buses, cpu, self.pc_of(cpu)).filter(|at| breakpoints.contains(at))
    }

    pub(super) fn watches_instructions(&self, cpu: Cpu) -> bool {
        self.debugger.static_analysis_enabled || !self.debug.breakpoints[side(cpu)].is_empty()
    }

    pub(super) fn after_instruction(&mut self, cpu: Cpu, before: u32) -> bool {
        self.analyze(cpu);
        let hit = self.pc_of(cpu) != before && self.breakpoint_on(cpu).is_some();
        self.debugger.hit_breakpoint |= hit;
        hit
    }

    fn step_cpu(&mut self, cpu: Cpu) -> u32 {
        if self.faults[side(cpu)].is_some() {
            return 0;
        }
        if !self.awake(cpu) {
            let before = self.now;
            if self.run_slice() {
                self.frames += 1;
                self.publish();
            }
            return (self.now - before) as u32;
        }
        let used = self.execute(cpu);
        if used == 0 {
            return 0;
        }
        self.analyze(cpu);
        let time = match cpu {
            Cpu::Arm9 => self.arm9_cycles / 2,
            Cpu::Arm7 => self.arm7_cycles,
        };
        self.catch_up(time, cpu);
        used
    }

    fn catch_up(&mut self, time: u64, stepped: Cpu) {
        let other = match stepped {
            Cpu::Arm9 => Cpu::Arm7,
            Cpu::Arm7 => Cpu::Arm9,
        };
        while self.now < time {
            let until = self.buses.until_event().min((time - self.now) as u32);
            let target = self.now + u64::from(until);
            if self.awake(other) {
                self.run_cpu(other, scaled(other, target));
            } else {
                let clock = self.clock_mut(other);
                *clock = (*clock).max(scaled(other, target));
            }
            if self.buses.elapse(until) {
                self.frames += 1;
                self.publish();
            }
            self.charge_stalls();
            self.now = target;
        }
    }
}

struct Steps<'a, const V5: bool>(&'a mut Nds);

impl<const V5: bool> Steps<'_, V5> {
    const CPU: Cpu = if V5 { Cpu::Arm9 } else { Cpu::Arm7 };
}

impl Stepper<true> for Steps<'_, true> {
    fn cpu(&self) -> &Arm9 {
        &self.0.arm9
    }

    fn step_instruction(&mut self) -> u32 {
        self.0.step_cpu(Self::CPU)
    }

    fn breakpoint(&self) -> Option<Address> {
        self.0.breakpoint_on(Self::CPU)
    }

    fn line(&self) -> u16 {
        self.0.buses.lcd().line()
    }

    fn frame(&self) -> u64 {
        self.0.frames
    }
}

impl Stepper for Steps<'_, false> {
    fn cpu(&self) -> &Arm7 {
        &self.0.arm7
    }

    fn step_instruction(&mut self) -> u32 {
        self.0.step_cpu(Self::CPU)
    }

    fn breakpoint(&self) -> Option<Address> {
        self.0.breakpoint_on(Self::CPU)
    }

    fn line(&self) -> u16 {
        self.0.buses.lcd().line()
    }

    fn frame(&self) -> u64 {
        self.0.frames
    }
}

impl Debug for Nds {
    fn features(&self) -> Features {
        match self.selected() {
            Cpu::Arm9 => ARM9_FEATURES,
            Cpu::Arm7 => ARM7_FEATURES,
        }
    }

    fn registers(&self, out: &mut Vec<RegisterGroup>) {
        let cpu = self.selected();
        match cpu {
            Cpu::Arm9 => self.arm9.registers(out),
            Cpu::Arm7 => self.arm7.registers(out),
        }
        io::registers(&self.buses, cpu, out);
    }

    fn cpu(&self) -> usize {
        self.debug.selected
    }

    fn select_cpu(&mut self, cpu: usize) {
        if cpu < CPUS.len() && cpu != self.debug.selected {
            self.debug.stepping.cancel();
            self.debug.selected = cpu;
        }
    }

    fn memory_blocks(&self, out: &mut Vec<MemoryBlock>) {
        memory::blocks(&self.buses, self.selected(), out);
    }

    fn locate(&self, addr: u32) -> Option<Address> {
        memory::locate(&self.buses, self.selected(), addr)
    }

    fn peek(&self, at: Address, out: &mut [u8]) {
        memory::peek(&self.buses, self.selected(), at, out);
    }

    fn poke(&mut self, at: Address, bytes: &[u8]) {
        let cpu = self.selected();
        memory::poke(&mut self.buses, cpu, at, bytes);
    }

    fn pc(&self) -> Option<Address> {
        let cpu = self.selected();
        memory::locate(&self.buses, cpu, self.pc_of(cpu))
    }

    fn code_analysis(&self) -> bool {
        self.debugger.static_analysis_enabled
    }

    fn set_code_analysis(&mut self, on: bool) {
        self.debugger.static_analysis_enabled = on;
        if on {
            for cpu in CPUS {
                self.analyze(cpu);
            }
        }
    }

    fn code_rows(&self) -> usize {
        self.debug.code[self.debug.selected].rows()
    }

    fn code_row_of(&self, at: Address) -> Option<usize> {
        self.debug.code[self.debug.selected].row_of(at)
    }

    fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>) {
        let cpu = self.selected();
        lines(
            &self.debug.code[side(cpu)],
            &self.code_source(cpu),
            rows,
            out,
        );
    }

    fn breakpoints(&mut self) -> &mut Breakpoints {
        &mut self.debug.breakpoints[self.debug.selected]
    }

    fn cycles(&self) -> u128 {
        u128::from(self.now)
    }

    fn record_waves(&mut self, _on: bool) {}

    fn waves(&self) -> &[VecDeque<f32>] {
        self.debugger.waves()
    }

    fn step(&mut self, step: Step) -> Stop {
        let mut stepping = std::mem::take(&mut self.debug.stepping);
        let stop = match self.selected() {
            Cpu::Arm9 => stepping.start(&mut Steps::<true>(self), step, STEP_BUDGET),
            Cpu::Arm7 => stepping.start(&mut Steps::<false>(self), step, STEP_BUDGET),
        };
        self.debug.stepping = stepping;
        stop
    }

    fn step_pending(&self) -> bool {
        self.debug.stepping.pending()
    }

    fn resume_step(&mut self) -> Stop {
        let mut stepping = std::mem::take(&mut self.debug.stepping);
        let stop = match self.selected() {
            Cpu::Arm9 => stepping.resume(&mut Steps::<true>(self), STEP_BUDGET),
            Cpu::Arm7 => stepping.resume(&mut Steps::<false>(self), STEP_BUDGET),
        };
        self.debug.stepping = stepping;
        stop
    }

    fn cancel_step(&mut self) {
        self.debug.stepping.cancel();
    }

    fn take_breakpoint_hit(&mut self) -> bool {
        std::mem::take(&mut self.debugger.hit_breakpoint)
    }

    fn frame_cycles(&self) -> u32 {
        FRAME_CYCLES
    }

    fn scanline(&self) -> u16 {
        self.buses.lcd().line()
    }

    fn trace(&mut self) -> &mut TraceLog {
        &mut self.debugger.trace
    }

    fn tile_sheets(&self, _colors: SheetColors, _out: &mut Vec<TileSheet>) {}

    fn tile_maps(&self, _out: &mut Vec<TileMap>) {}

    fn sprites(&self, _out: &mut Vec<Sprite>) {}

    fn palettes(&self, _out: &mut Vec<Palette>) {}

    fn print_log(&mut self) -> Option<&mut PrintLog> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::disasm::decode_at;
    use crate::nds::machine::tests::nds;

    const ARM9_RAM: u32 = 0x0200_0000;
    const ARM7_RAM: u32 = 0x0380_0000;
    const BRANCH_TO_SELF: u32 = 0xEAFF_FFFE;
    const CLZ_R0_R1: u32 = 0xE16F_0F11;

    fn mov(register: u32, value: u32) -> u32 {
        0xE3A0_0000 | register << 12 | value
    }

    fn register(nds: &Nds, name: &str) -> u32 {
        let mut groups = Vec::new();
        nds.registers(&mut groups);
        groups[0]
            .rows
            .iter()
            .find(|row| row.name == name)
            .unwrap()
            .value
    }

    fn pc(nds: &Nds) -> Option<u32> {
        nds.pc().and_then(|at| at.nds_address())
    }

    #[test]
    fn the_cpu_switch_shows_each_cpus_own_registers_and_pc() {
        let mut nds = nds(
            &[mov(0, 0x11), BRANCH_TO_SELF],
            &[mov(0, 0x77), BRANCH_TO_SELF],
        );
        nds.run_frame();
        assert_eq!(nds.features().cpus, ["ARM9", "ARM7"]);
        assert_eq!(nds.cpu(), 0);
        assert_eq!(register(&nds, "R0"), 0x11);
        assert_eq!(pc(&nds), Some(ARM9_RAM + 4));
        nds.select_cpu(1);
        assert_eq!(nds.cpu(), 1);
        assert_eq!(register(&nds, "R0"), 0x77);
        assert_eq!(pc(&nds), Some(ARM7_RAM + 4));
        nds.select_cpu(2);
        assert_eq!(nds.cpu(), 1);
    }

    #[test]
    fn the_cpu_switch_picks_each_cpus_vectors_and_psr_flags() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        assert_eq!(nds.features().vectors[0], ("Reset", 0xFFFF_0000));
        let mut groups = Vec::new();
        nds.registers(&mut groups);
        let cpsr = groups[0]
            .rows
            .iter()
            .find(|row| row.name == "CPSR")
            .unwrap();
        assert!(cpsr.field("Q").is_some());
        nds.select_cpu(1);
        assert_eq!(nds.features().vectors[0], ("Reset", 0));
        nds.registers(&mut groups);
        let cpsr = groups[0]
            .rows
            .iter()
            .find(|row| row.name == "CPSR")
            .unwrap();
        assert!(cpsr.field("Q").is_none());
    }

    #[test]
    fn the_arm9_disassembles_armv5te_and_the_arm7_armv4t() {
        let nds = nds(&[BRANCH_TO_SELF, CLZ_R0_R1], &[BRANCH_TO_SELF]);
        let at = Pc {
            addr: ARM9_RAM + 4,
            thumb: false,
        };
        assert_eq!(decode_at(&nds.code_source(Cpu::Arm9), at).mnemonic, "CLZ");
        assert_ne!(decode_at(&nds.code_source(Cpu::Arm7), at).mnemonic, "CLZ");
    }

    #[test]
    fn static_analysis_lists_the_code_around_each_cpus_pc() {
        let mut nds = nds(&[CLZ_R0_R1, BRANCH_TO_SELF], &[mov(1, 1), BRANCH_TO_SELF]);
        nds.set_code_analysis(true);
        let mut lines = Vec::new();
        nds.disassemble(0..nds.code_rows(), &mut lines);
        assert_eq!(lines[0].text(), "CLZ R0, R1");
        assert_eq!(nds.code_row_of(nds.pc().unwrap()), Some(0));
        nds.select_cpu(1);
        nds.disassemble(0..nds.code_rows(), &mut lines);
        assert_eq!(lines[0].text(), "MOV R1, #0x1");
        assert_eq!(nds.code_row_of(nds.pc().unwrap()), Some(0));
    }

    #[test]
    fn a_step_runs_one_instruction_of_the_selected_cpu() {
        let code = |register| {
            (1..=64)
                .map(|value| mov(register, value))
                .chain([BRANCH_TO_SELF])
                .collect::<Vec<u32>>()
        };
        let mut nds = nds(&code(0), &code(1));
        nds.select_cpu(1);
        let start = pc(&nds).unwrap();
        assert_eq!(nds.step(Step::Instruction), Stop::Finished);
        assert_eq!(pc(&nds), Some(start + 4));
        assert_eq!(register(&nds, "R1"), 1);
        nds.select_cpu(0);
        let start = pc(&nds).unwrap();
        assert_eq!(nds.step(Step::Instruction), Stop::Finished);
        assert_eq!(pc(&nds), Some(start + 4));
        assert!(nds.arm7_cycles >= nds.arm9_cycles / 2);
        assert!(nds.arm7_cycles <= nds.arm9_cycles / 2 + 16);
    }

    #[test]
    fn breakpoints_belong_to_one_cpu_and_stop_the_frame() {
        let code = |register| {
            [
                mov(register, 1),
                mov(register, 2),
                mov(register, 3),
                BRANCH_TO_SELF,
            ]
        };
        let mut nds = nds(&code(0), &code(1));
        nds.select_cpu(1);
        let at = nds.locate(ARM7_RAM + 8).unwrap();
        nds.breakpoints().toggle(at);
        nds.select_cpu(0);
        assert!(nds.breakpoints().is_empty());
        nds.run_frame();
        assert!(nds.take_breakpoint_hit());
        assert_eq!(nds.frames(), 0);
        nds.select_cpu(1);
        assert_eq!(nds.pc(), Some(at));
        assert_eq!(register(&nds, "R1"), 2);
    }

    #[test]
    fn a_step_stops_at_the_selected_cpus_breakpoint() {
        let code = |register| {
            [
                mov(register, 1),
                mov(register, 2),
                mov(register, 3),
                BRANCH_TO_SELF,
            ]
        };
        let mut nds = nds(&code(0), &code(1));
        let at = nds.locate(ARM9_RAM + 12).unwrap();
        nds.breakpoints().toggle(at);
        assert_eq!(nds.step(Step::Frame), Stop::Breakpoint(at));
        assert_eq!(register(&nds, "R0"), 3);
    }

    #[test]
    fn a_frame_step_ends_on_the_next_frame() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        let mut stop = nds.step(Step::Frame);
        while stop == Stop::Unfinished {
            stop = nds.resume_step();
        }
        assert_eq!(stop, Stop::Finished);
        assert_eq!(nds.frames(), 1);
    }

    #[test]
    fn the_io_pane_reads_ipcfiforecv_without_taking_the_word() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        let buses = nds.buses_mut();
        buses.arm9().write(0x0400_0184, Width::Half, 1 << 15);
        buses.arm7().write(0x0400_0184, Width::Half, 1 << 15);
        buses.arm9().write(0x0400_0188, Width::Word, 0xABCD_0123);
        nds.select_cpu(1);
        let mut groups = Vec::new();
        for _ in 0..2 {
            nds.registers(&mut groups);
            let ipc = groups.iter().find(|group| group.name == "IPC").unwrap();
            let recv = ipc
                .rows
                .iter()
                .find(|row| row.name == "IPCFIFORECV")
                .unwrap();
            assert_eq!(recv.value, 0xABCD_0123);
        }
        let read = nds.buses_mut().arm7().read(0x0410_0000, Width::Word);
        assert_eq!(read, 0xABCD_0123);
    }

    #[test]
    fn a_soft_reset_keeps_the_selected_cpu_and_its_breakpoints() {
        let mut nds = nds(&[BRANCH_TO_SELF], &[BRANCH_TO_SELF]);
        nds.select_cpu(1);
        let at = nds.locate(ARM7_RAM).unwrap();
        nds.breakpoints().toggle(at);
        nds.soft_reset();
        assert_eq!(nds.cpu(), 1);
        assert!(nds.breakpoints().contains(&at));
    }
}
