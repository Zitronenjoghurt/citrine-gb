use super::{Stepper, Stepping};
use crate::debug::code::Address;
use crate::debug::step::{Step, Stop};
use crate::gba::bus::Bus;
use crate::gba::cpu::psr::FLAG_T;
use crate::gba::cpu::{Cpu, Mode};
use crate::gba::disasm::code_address;
use std::collections::HashSet;

const ROM: u32 = 0x0800_0000;
const STACK: u32 = 0x0300_7F00;
const BUDGET: u32 = 10_000;

struct Rig {
    cpu: Cpu,
    bus: Bus,
    breakpoints: HashSet<u32>,
    cycles: u64,
}

impl Stepper for Rig {
    fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    fn step_instruction(&mut self) -> u32 {
        let used = self.cpu.step(&mut self.bus);
        self.cycles += u64::from(used);
        used
    }

    fn breakpoint(&self) -> Option<Address> {
        let pc = self.cpu.pc();
        self.breakpoints
            .contains(&pc)
            .then(|| code_address(pc, 0x100).expect("rom"))
    }

    fn line(&self) -> u16 {
        ((self.cycles / 1232) % 228) as u16
    }

    fn frame(&self) -> u64 {
        self.cycles / 280_896
    }
}

fn boot(rom: Vec<u8>, thumb: bool) -> Rig {
    let mut bus = Bus::new(&rom);
    let mut cpu = Cpu::default();
    cpu.reset(&mut bus);
    let mut state = cpu.state();
    state.r[15] = ROM;
    state.r[13] = STACK;
    state.cpsr = Mode::System.bits();
    if thumb {
        state.cpsr |= FLAG_T;
    }
    let mut cpu = Cpu::from_state(&state);
    cpu.reload(&mut bus);
    Rig {
        cpu,
        bus,
        breakpoints: HashSet::new(),
        cycles: 0,
    }
}

fn arm_program() -> Rig {
    let words: [u32; 10] = [
        0xEB00_0002,
        0xE3A0_1002,
        0xEAFF_FFFE,
        0xE1A0_0000,
        0xE92D_4000,
        0xE3A0_0001,
        0xEB00_0000,
        0xE8BD_8000,
        0xE280_0001,
        0xE12F_FF1E,
    ];
    boot(words.iter().flat_map(|w| w.to_le_bytes()).collect(), false)
}

fn thumb_program() -> Rig {
    let halves: [u16; 8] = [
        0xF000, 0xF804, 0x2102, 0xE7FE, 0x46C0, 0x46C0, 0xB500, 0x2001,
    ];
    let mut rom: Vec<u8> = halves.iter().flat_map(|h| h.to_le_bytes()).collect();
    rom.extend(0xBD00u16.to_le_bytes());
    boot(rom, true)
}

fn step(rig: &mut Rig, stepping: &mut Stepping, step: Step) -> Stop {
    stepping.start(rig, step, BUDGET)
}

#[test]
fn step_over_runs_a_whole_call() {
    let mut rig = arm_program();
    let mut stepping = Stepping::default();
    assert_eq!(step(&mut rig, &mut stepping, Step::Over), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x04);
    assert_eq!(rig.cpu.reg(0), 2);
    assert_eq!(rig.cpu.reg(13), STACK);
    assert_eq!(step(&mut rig, &mut stepping, Step::Over), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x08);
}

#[test]
fn step_over_a_nested_call_stops_on_the_next_line() {
    let mut rig = arm_program();
    let mut stepping = Stepping::default();
    for _ in 0..3 {
        step(&mut rig, &mut stepping, Step::Instruction);
    }
    assert_eq!(rig.cpu.pc(), ROM + 0x18);
    assert_eq!(step(&mut rig, &mut stepping, Step::Over), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x1C);
    assert_eq!(rig.cpu.reg(0), 2);
}

#[test]
fn step_out_skips_nested_returns() {
    let mut rig = arm_program();
    let mut stepping = Stepping::default();
    for _ in 0..2 {
        step(&mut rig, &mut stepping, Step::Instruction);
    }
    assert_eq!(rig.cpu.pc(), ROM + 0x14);
    assert_eq!(step(&mut rig, &mut stepping, Step::Out), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x04);
    assert_eq!(rig.cpu.reg(13), STACK);
}

#[test]
fn step_out_of_a_leaf_returns_to_its_caller() {
    let mut rig = arm_program();
    let mut stepping = Stepping::default();
    for _ in 0..4 {
        step(&mut rig, &mut stepping, Step::Instruction);
    }
    assert_eq!(rig.cpu.pc(), ROM + 0x20);
    assert_eq!(step(&mut rig, &mut stepping, Step::Out), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x1C);
}

#[test]
fn a_breakpoint_inside_the_call_stops_step_over() {
    let mut rig = arm_program();
    rig.breakpoints.insert(ROM + 0x20);
    let mut stepping = Stepping::default();
    let at = code_address(ROM + 0x20, 0x100).expect("rom");
    assert_eq!(
        step(&mut rig, &mut stepping, Step::Over),
        Stop::Breakpoint(at)
    );
    assert_eq!(rig.cpu.pc(), ROM + 0x20);
    assert!(!stepping.pending());
}

#[test]
fn a_step_that_runs_out_of_budget_can_resume_or_cancel() {
    let mut rig = arm_program();
    let mut stepping = Stepping::default();
    step(&mut rig, &mut stepping, Step::Over);
    step(&mut rig, &mut stepping, Step::Over);
    assert_eq!(rig.cpu.pc(), ROM + 0x08);
    assert_eq!(stepping.start(&mut rig, Step::Out, 100), Stop::Unfinished);
    assert!(stepping.pending());
    assert_eq!(stepping.resume(&mut rig, 100), Stop::Unfinished);
    stepping.cancel();
    assert!(!stepping.pending());
    assert_eq!(stepping.resume(&mut rig, 100), Stop::Finished);
}

#[test]
fn thumb_step_over_takes_both_halves_of_bl() {
    let mut rig = thumb_program();
    let mut stepping = Stepping::default();
    assert_eq!(step(&mut rig, &mut stepping, Step::Over), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x04);
    assert!(rig.cpu.thumb());
    assert_eq!(rig.cpu.reg(0), 1);
    assert_eq!(rig.cpu.reg(13), STACK);
}

#[test]
fn thumb_step_over_from_the_second_half_of_bl() {
    let mut rig = thumb_program();
    let mut stepping = Stepping::default();
    step(&mut rig, &mut stepping, Step::Instruction);
    assert_eq!(rig.cpu.pc(), ROM + 0x02);
    assert_eq!(step(&mut rig, &mut stepping, Step::Over), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x04);
}

#[test]
fn thumb_step_out_returns_through_pop_pc() {
    let mut rig = thumb_program();
    let mut stepping = Stepping::default();
    for _ in 0..3 {
        step(&mut rig, &mut stepping, Step::Instruction);
    }
    assert_eq!(rig.cpu.pc(), ROM + 0x0E);
    assert_eq!(step(&mut rig, &mut stepping, Step::Out), Stop::Finished);
    assert_eq!(rig.cpu.pc(), ROM + 0x04);
}

#[test]
fn cycles_and_frames_run_until_their_count() {
    let mut rig = arm_program();
    let mut stepping = Stepping::default();
    step(&mut rig, &mut stepping, Step::Over);
    step(&mut rig, &mut stepping, Step::Over);
    let before = rig.cycles;
    assert_eq!(
        step(&mut rig, &mut stepping, Step::Cycles(500)),
        Stop::Finished
    );
    assert!(rig.cycles - before >= 500);
    assert_eq!(
        stepping.start(&mut rig, Step::Frame, 600_000),
        Stop::Finished
    );
    assert_eq!(rig.frame(), 1);
}

struct Stuck(Cpu);

impl Stepper for Stuck {
    fn cpu(&self) -> &Cpu {
        &self.0
    }

    fn step_instruction(&mut self) -> u32 {
        0
    }

    fn breakpoint(&self) -> Option<Address> {
        None
    }

    fn line(&self) -> u16 {
        0
    }

    fn frame(&self) -> u64 {
        0
    }
}

#[test]
fn a_step_ends_when_the_target_cannot_run() {
    let mut stuck = Stuck(arm_program().cpu);
    let mut stepping = Stepping::default();
    assert_eq!(
        stepping.start(&mut stuck, Step::Out, BUDGET),
        Stop::Finished
    );
    assert!(!stepping.pending());
}
