use super::{Cpu, Mode};
use crate::debug::code::{Address, InstrClass};
use crate::debug::step::{Step, Stop};

pub trait Stepper {
    fn cpu(&self) -> &Cpu;
    fn step_instruction(&mut self) -> u32;
    fn breakpoint(&self) -> Option<Address>;
    fn line(&self) -> u16;
    fn frame(&self) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Goal {
    Instruction,
    Return {
        at: u32,
        sp: u32,
        mode: Option<Mode>,
    },
    Out {
        mode: Option<Mode>,
        depth: u32,
    },
    Line {
        line: u16,
    },
    Frame {
        frame: u64,
    },
    Cycles {
        left: u32,
    },
}

struct Ran {
    pc: u32,
    len: u32,
    class: InstrClass,
    mode: Option<Mode>,
}

impl Ran {
    fn of(cpu: &Cpu) -> Self {
        let decoded = cpu.instruction(false);
        Self {
            pc: cpu.pc(),
            len: u32::from(decoded.len),
            class: decoded.class,
            mode: cpu.mode(),
        }
    }

    fn branched(&self, cpu: &Cpu) -> bool {
        cpu.pc() != self.pc.wrapping_add(self.len)
    }
}

impl Goal {
    fn reached(&mut self, target: &impl Stepper, ran: &Ran, used: u32) -> bool {
        let cpu = target.cpu();
        match self {
            Self::Instruction => true,
            Self::Return { at, sp, mode } => {
                cpu.pc() == *at && cpu.reg(13) == *sp && cpu.mode() == *mode
            }
            Self::Out { mode, depth } => {
                if ran.mode != *mode || !ran.branched(cpu) {
                    return false;
                }
                match ran.class {
                    InstrClass::Call if cpu.mode() == *mode => {
                        *depth += 1;
                        false
                    }
                    InstrClass::Return if *depth == 0 => true,
                    InstrClass::Return => {
                        *depth -= 1;
                        false
                    }
                    _ => false,
                }
            }
            Self::Line { line } => target.line() != *line,
            Self::Frame { frame } => target.frame() != *frame,
            Self::Cycles { left } => {
                *left = left.saturating_sub(used);
                *left == 0
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stepping {
    goal: Option<Goal>,
}

impl Stepping {
    pub fn start(&mut self, target: &mut impl Stepper, step: Step, budget: u32) -> Stop {
        let cpu = target.cpu();
        let goal = match step {
            Step::Instruction => Goal::Instruction,
            Step::Over => {
                let current = cpu.instruction(true);
                if current.class == InstrClass::Call {
                    Goal::Return {
                        at: cpu.pc().wrapping_add(u32::from(current.len)),
                        sp: cpu.reg(13),
                        mode: cpu.mode(),
                    }
                } else {
                    Goal::Instruction
                }
            }
            Step::Out => Goal::Out {
                mode: cpu.mode(),
                depth: 0,
            },
            Step::Line => Goal::Line {
                line: target.line(),
            },
            Step::Frame => Goal::Frame {
                frame: target.frame(),
            },
            Step::Cycles(cycles) => Goal::Cycles { left: cycles },
        };
        self.goal = Some(goal);
        self.resume(target, budget)
    }

    pub fn resume(&mut self, target: &mut impl Stepper, budget: u32) -> Stop {
        let Some(mut goal) = self.goal.take() else {
            return Stop::Finished;
        };
        let mut spent = 0u32;
        loop {
            let ran = Ran::of(target.cpu());
            let used = target.step_instruction();
            if used == 0 {
                return Stop::Finished;
            }
            spent = spent.saturating_add(used);
            if goal.reached(target, &ran, used) {
                return Stop::Finished;
            }
            if let Some(at) = target.breakpoint() {
                return Stop::Breakpoint(at);
            }
            if spent >= budget {
                self.goal = Some(goal);
                return Stop::Unfinished;
            }
        }
    }

    pub fn pending(&self) -> bool {
        self.goal.is_some()
    }

    pub fn cancel(&mut self) {
        self.goal = None;
    }
}

#[cfg(test)]
mod tests;
