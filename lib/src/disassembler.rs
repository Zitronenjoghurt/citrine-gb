use crate::instructions::Instruction;
use crate::ReadMemory;
use std::collections::BTreeMap;
use std::fmt::Display;

#[derive(Debug, Copy, Clone)]
pub struct DecodedInstruction {
    pub addr: u16,
    pub instruction: Instruction,
    pub ctx: [u8; 3],
}

impl Display for DecodedInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04X}: {}",
            self.addr,
            self.instruction.string_context(&self.ctx)
        )
    }
}

#[derive(Debug, Default)]
pub struct Disassembly {
    entries: BTreeMap<u16, DecodedInstruction>,
}

// ToDo: Recursive traversal => trace jumps, etc.
impl Disassembly {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode_at(&mut self, mem: &impl ReadMemory, addr: u16) -> DecodedInstruction {
        let mut ctx = [0u8; 3];
        ctx[0] = mem.read_naive(addr);
        ctx[1] = mem.read_naive(addr + 1);
        ctx[2] = mem.read_naive(addr + 2);

        let instruction = if ctx[0] != 0xCB {
            Instruction::decode(ctx[0])
        } else {
            Instruction::decode_prefixed(ctx[1])
        };

        let entry = DecodedInstruction {
            addr,
            instruction,
            ctx,
        };

        self.entries.insert(addr, entry);
        entry
    }

    pub fn decode_range(&mut self, mem: &impl ReadMemory, start: u16, end: u16) {
        let mut addr = start;
        while addr >= start && addr <= end {
            let entry = self.decode_at(mem, addr);
            if let Some(jump_addr) = entry
                .instruction
                .unconditional_jump_target(addr, &entry.ctx)
            {
                addr = jump_addr;
            } else {
                addr += entry.instruction.length() as u16;
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &DecodedInstruction> {
        self.entries.values()
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
