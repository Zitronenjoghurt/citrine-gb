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

    pub fn analyze(&mut self, src: &impl DisassemblySource, start: u16) {
        let mut worklist = VecDeque::new();
        worklist.push_back(start);

        while let Some(addr) = worklist.pop_front() {
            if addr > 0x7FFF {
                continue;
            }

            let decoded = self.decode(src, addr);
            if self.has_collision(decoded.loc)
                || addr < 0x4000 && addr + decoded.instruction.length() as u16 > 0x4000
            {
                // Collided with existing instruction (falsely analyzing data) or crosses bank boundary (non-deterministic)
                continue;
            }

            self.entries.insert(decoded.loc, decoded);

            let flow = decoded.instruction.flow_control(addr, &decoded.ctx);
            for succ in flow
                .successors(addr, decoded.instruction.length() as u16)
                .into_iter()
                .flatten()
            {
                worklist.push_back(succ);
            }
        }
    }

    fn decode(&mut self, src: &impl DisassemblySource, addr: u16) -> DecodedInstruction {
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
        }
    }

    fn has_collision(&self, loc: RomLocation) -> bool {
        self.entries.contains_key(&loc) || self.location_overlaps_existing(loc)
    }

    fn location_overlaps_existing(&self, loc: RomLocation) -> bool {
        for lookback in 1..=2 {
            if let Some(existing) = self.entries.get(&loc.offset(-(lookback as i16)))
                && existing.instruction.length() > lookback
            {
                return true;
            }
        }
        false
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
