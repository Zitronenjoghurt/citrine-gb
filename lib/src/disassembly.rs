use crate::gb::cartridge::RomLocation;
use crate::instructions::Instruction;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Display;

pub trait DisassemblySource {
    fn read_rom_address(&self, addr: u16) -> u8;
    fn probe_rom_location(&self, addr: u16) -> RomLocation;
    fn read_rom_location(&self, loc: RomLocation) -> u8;
}

#[derive(Debug, Copy, Clone)]
pub struct DecodedInstruction {
    pub loc: RomLocation,
    pub instruction: Instruction,
    pub ctx: [u8; 3],
    pub confidence: Confidence,
}

impl Display for DecodedInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} = {}",
            self.loc,
            self.instruction.string_context(&self.ctx)
        )
    }
}

#[derive(Debug, Default)]
pub struct Disassembly {
    entries: BTreeMap<RomLocation, DecodedInstruction>,
}

impl Disassembly {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_fetch(&mut self, src: &impl DisassemblySource, start: u16) {
        self.analyze(src, start, Confidence::Fetched);
    }

    pub fn analyze(
        &mut self,
        src: &impl DisassemblySource,
        start: u16,
        start_confidence: Confidence,
    ) {
        let mut worklist = VecDeque::new();
        worklist.push_back((start, start_confidence));

        while let Some((addr, current_conf)) = worklist.pop_front() {
            if addr > 0x7FFF {
                continue;
            }

            let loc = src.probe_rom_location(addr);
            if let Some(existing) = self.entries.get(&loc)
                && existing.confidence >= current_conf
            {
                continue;
            }

            let decoded = self.decode(src, addr, current_conf);
            if matches!(decoded.instruction, Instruction::Invalid(_)) {
                continue;
            }

            let len = decoded.instruction.length() as u16;
            if addr < 0x4000 && addr + len > 0x4000 {
                continue;
            }

            if !self.resolve_overlaps(&decoded) {
                continue;
            }

            self.entries.insert(decoded.loc, decoded);

            let propagate_conf = if current_conf == Confidence::Fetched {
                Confidence::Unconditional
            } else {
                current_conf
            };

            let fallthrough = addr.wrapping_add(len);
            let flow = decoded.instruction.flow_control(addr, &decoded.ctx);

            match flow {
                FlowControl::Continue => {
                    worklist.push_back((fallthrough, propagate_conf));
                }
                FlowControl::Jump(target) => {
                    worklist.push_back((target, propagate_conf));
                }
                FlowControl::Call(target) => {
                    worklist.push_back((target, propagate_conf));
                    worklist.push_back((fallthrough, propagate_conf));
                }
                FlowControl::ConditionalJump(target) => {
                    worklist.push_back((target, Confidence::Conditional));
                    worklist.push_back((fallthrough, propagate_conf));
                }
                FlowControl::ConditionalReturn => {
                    worklist.push_back((fallthrough, propagate_conf));
                }
                FlowControl::Return | FlowControl::UnknownJump | FlowControl::Invalid => {}
            }
        }
    }

    fn decode(
        &mut self,
        src: &impl DisassemblySource,
        addr: u16,
        confidence: Confidence,
    ) -> DecodedInstruction {
        let mut ctx = [0u8; 3];
        ctx[0] = src.read_rom_address(addr);
        ctx[1] = src.read_rom_address(addr.wrapping_add(1));
        ctx[2] = src.read_rom_address(addr.wrapping_add(2));

        let instruction = if ctx[0] != 0xCB {
            Instruction::decode(ctx[0])
        } else {
            Instruction::decode_prefixed(ctx[1])
        };

        DecodedInstruction {
            loc: src.probe_rom_location(addr),
            instruction,
            ctx,
            confidence,
        }
    }

    fn resolve_overlaps(&mut self, decoded: &DecodedInstruction) -> bool {
        let mut overlapping_locs = Vec::new();
        let len = decoded.instruction.length() as i16;

        for lookback in 1..=3 {
            let prev_loc = decoded.loc.offset(-lookback);
            if let Some(existing) = self.entries.get(&prev_loc)
                && existing.instruction.length() as i16 > lookback
            {}
        }

        for lookforward in 0..len {
            let next_loc = decoded.loc.offset(lookforward);
            if self.entries.contains_key(&next_loc) {
                overlapping_locs.push(next_loc);
            }
        }

        overlapping_locs.sort();
        overlapping_locs.dedup();

        for loc in &overlapping_locs {
            let existing = self.entries.get(loc).unwrap();
            if existing.confidence >= decoded.confidence {
                return false;
            }
        }

        for loc in overlapping_locs {
            self.entries.remove(&loc);
        }

        true
    }

    pub fn iter(&self) -> impl Iterator<Item = &DecodedInstruction> {
        self.entries.values()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get_by_index(&self, index: usize) -> Option<&DecodedInstruction> {
        self.entries.values().nth(index)
    }
}

impl Display for Disassembly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for entry in self.entries.values() {
            writeln!(f, "{entry}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Copy, Clone)]
pub enum FlowControl {
    Continue,
    Jump(u16),
    ConditionalJump(u16),
    UnknownJump,
    Call(u16),
    Return,
    ConditionalReturn,
    Invalid,
}

impl FlowControl {
    pub fn successors(&self, address: u16, instr_length: u16) -> [Option<u16>; 2] {
        let fallthrough = address.wrapping_add(instr_length);
        match self {
            FlowControl::Continue => [Some(fallthrough), None],
            FlowControl::Jump(target) => [Some(*target), None],
            FlowControl::ConditionalJump(target) => [Some(*target), Some(fallthrough)],
            FlowControl::Call(target) => [Some(*target), Some(fallthrough)],
            FlowControl::ConditionalReturn => [Some(fallthrough), None],
            FlowControl::UnknownJump | FlowControl::Return | FlowControl::Invalid => [None, None],
        }
    }
}

/// The confidence of a disassembly entry.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// The entry is reachable through conditional branching of executed instructions.
    Conditional,
    /// The entry is reachable through unconditional branching of executed instructions.
    Unconditional,
    /// The CPU actually fetched the entry.
    Fetched,
}
