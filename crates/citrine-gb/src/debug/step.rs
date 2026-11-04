use crate::debug::breakpoints::Access;
use crate::debug::code::{Address, InstrClass, class};
use crate::gb::GameBoy;
use crate::instructions::Instruction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Instruction,
    Over,
    Out,
    Line,
    Frame,
    Cycles(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Finished,
    Breakpoint(Address),
    Watchpoint {
        at: Address,
        access: Access,
        by: Option<Address>,
    },
    Unfinished,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Goal {
    Instruction,
    Return { line: u16, sp: u16 },
    Out { sp: u16 },
    Line { line: u8, left: u32 },
    Frame,
    Cycles { left: u32 },
}

impl Goal {
    fn reached(&mut self, gb: &GameBoy, ran: Instruction, used: u32) -> bool {
        match self {
            Self::Instruction => true,
            Self::Return { line, sp } => gb.line() == *line && gb.cpu.sp == *sp,
            Self::Out { sp } => class(&ran) == InstrClass::Return && gb.cpu.sp > *sp,
            Self::Line { line, left } => {
                *left = left.saturating_sub(used);
                if gb.ppu.lcdc.lcd_enabled {
                    gb.ppu.line() != *line
                } else {
                    *left == 0
                }
            }
            Self::Frame => {
                gb.ppu.frame_ready
                    || (!gb.ppu.lcdc.lcd_enabled && gb.cycle_counter >= gb.frame_cycles())
            }
            Self::Cycles { left } => {
                *left = left.saturating_sub(used);
                *left == 0
            }
        }
    }
}

impl GameBoy {
    pub fn run_step(&mut self, step: Step) -> Stop {
        let goal = match step {
            Step::Instruction => Goal::Instruction,
            Step::Over => {
                let instruction = Instruction::decode(self.cpu.ir);
                if class(&instruction) == InstrClass::Call {
                    Goal::Return {
                        line: self.line().wrapping_add(instruction.length() as u16),
                        sp: self.cpu.sp,
                    }
                } else {
                    Goal::Instruction
                }
            }
            Step::Out => Goal::Out { sp: self.cpu.sp },
            Step::Line => Goal::Line {
                line: self.ppu.line(),
                left: self.frame_cycles() / 154,
            },
            Step::Frame => {
                self.ppu.frame_ready = false;
                Goal::Frame
            }
            Step::Cycles(cycles) => Goal::Cycles { left: cycles },
        };
        self.debugger.step_goal = Some(goal);
        self.resume_step()
    }

    pub fn resume_step(&mut self) -> Stop {
        let Some(mut goal) = self.debugger.step_goal.take() else {
            return Stop::Finished;
        };
        let budget = self.frame_cycles() * 2;
        let mut spent = 0u32;
        let stop = loop {
            let before = self.cycle_counter;
            let ran = Instruction::decode(self.cpu.ir);
            self.step();
            let used = self.cycle_counter.wrapping_sub(before);
            spent = spent.saturating_add(used);
            if let Some(stop) = self.debugger.breakpoints.take_pending() {
                break stop;
            }
            if goal.reached(self, ran, used) {
                break Stop::Finished;
            }
            if let Some(hit) = self.breakpoint_hit() {
                break hit;
            }
            if spent >= budget {
                self.debugger.step_goal = Some(goal);
                break Stop::Unfinished;
            }
        };
        let frame_cycles = self.frame_cycles();
        while self.cycle_counter >= frame_cycles {
            self.cycle_counter -= frame_cycles;
        }
        self.apu.flush_audio();
        stop
    }

    pub fn step_pending(&self) -> bool {
        self.debugger.step_goal.is_some()
    }

    pub fn cancel_step(&mut self) {
        self.debugger.step_goal = None;
    }

    fn line(&self) -> u16 {
        self.cpu.pc.wrapping_sub(1)
    }
}
