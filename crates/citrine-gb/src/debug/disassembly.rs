use crate::debug::code::Address;
use crate::instructions::{FlowControl, Instruction};
use std::collections::VecDeque;

pub trait CodeEntry: Copy {
    type Pc: Copy;
    const MAX_LEN: u32;
    fn at(&self) -> Address;
    fn size(&self) -> u32;
    fn confidence(&self) -> Confidence;

    fn end(&self) -> Address {
        self.at().offset_by(i64::from(self.size().max(1)))
    }

    fn contains(&self, at: Address) -> bool {
        self.at() <= at && at < self.end()
    }
}

pub trait Decode<S: ?Sized>: CodeEntry {
    fn locate(src: &S, pc: Self::Pc) -> Option<Address>;
    fn decode(
        src: &S,
        pc: Self::Pc,
        confidence: Confidence,
    ) -> Option<(Self, FlowControl<Self::Pc>, Self::Pc)>;
}

pub trait DisassemblySource {
    fn read_rom_address(&self, addr: u16) -> u8;
    fn rom_address(&self, addr: u16) -> Address;
}

#[derive(Debug, Copy, Clone)]
pub struct DecodedInstruction {
    pub at: Address,
    pub instruction: Instruction,
    pub ctx: [u8; 3],
    pub confidence: Confidence,
}

impl DecodedInstruction {
    pub fn instruction_bytes(&self) -> &[u8] {
        &self.ctx[0..self.instruction.length()]
    }
}

impl CodeEntry for DecodedInstruction {
    type Pc = u16;
    const MAX_LEN: u32 = 3;

    fn at(&self) -> Address {
        self.at
    }

    fn size(&self) -> u32 {
        self.instruction.length() as u32
    }

    fn confidence(&self) -> Confidence {
        self.confidence
    }
}

impl<S: DisassemblySource + ?Sized> Decode<S> for DecodedInstruction {
    fn locate(src: &S, pc: u16) -> Option<Address> {
        (pc <= 0x7FFF).then(|| src.rom_address(pc))
    }

    fn decode(src: &S, addr: u16, confidence: Confidence) -> Option<(Self, FlowControl, u16)> {
        let ctx = [0, 1, 2].map(|i| src.read_rom_address(addr.wrapping_add(i)));
        let decoded = DecodedInstruction {
            at: src.rom_address(addr),
            instruction: decode(&ctx),
            ctx,
            confidence,
        };
        if matches!(decoded.instruction, Instruction::Invalid(_)) {
            return None;
        }
        let len = decoded.instruction.length() as u16;
        if addr < 0x4000 && addr + len > 0x4000 {
            return None;
        }
        let flow = decoded.instruction.flow_control(addr, &decoded.ctx);
        Some((decoded, flow, addr.wrapping_add(len)))
    }
}

pub(crate) enum Row<'a, E = DecodedInstruction> {
    Rom(&'a E),
    Ram(Address),
}

pub(crate) fn decode(ctx: &[u8; 3]) -> Instruction {
    if ctx[0] != 0xCB {
        Instruction::decode(ctx[0])
    } else {
        Instruction::decode_prefixed(ctx[1])
    }
}

#[derive(Debug)]
pub struct Disassembly<E = DecodedInstruction> {
    rom: Vec<E>,
    ram: Vec<Address>,
}

impl<E> Default for Disassembly<E> {
    fn default() -> Self {
        Self {
            rom: Vec::new(),
            ram: Vec::new(),
        }
    }
}

impl Disassembly {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<E: CodeEntry> Disassembly<E> {
    pub fn on_fetch<S: ?Sized>(&mut self, src: &S, start: E::Pc)
    where
        E: Decode<S>,
    {
        self.analyze(src, start, Confidence::Fetched);
    }

    pub fn on_ram_fetch(&mut self, at: Address) {
        if let Err(index) = self.ram.binary_search(&at) {
            self.ram.insert(index, at);
        }
    }

    pub fn analyze<S: ?Sized>(&mut self, src: &S, start: E::Pc, start_confidence: Confidence)
    where
        E: Decode<S>,
    {
        let mut worklist = VecDeque::new();
        worklist.push_back((start, start_confidence));

        while let Some((pc, current_conf)) = worklist.pop_front() {
            let Some(at) = E::locate(src, pc) else {
                continue;
            };
            if let Ok(index) = self.find(at)
                && self.rom[index].confidence() >= current_conf
            {
                continue;
            }

            let Some((decoded, flow, fallthrough)) = E::decode(src, pc, current_conf) else {
                continue;
            };

            if !self.resolve_overlaps(&decoded) {
                continue;
            }

            match self.find(decoded.at()) {
                Ok(index) => self.rom[index] = decoded,
                Err(index) => self.rom.insert(index, decoded),
            }

            if decoded.at().is_ram() {
                continue;
            }

            let propagate_conf = if current_conf == Confidence::Fetched {
                Confidence::Unconditional
            } else {
                current_conf
            };

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

    fn find(&self, at: Address) -> Result<usize, usize> {
        self.rom.binary_search_by(|entry| entry.at().cmp(&at))
    }

    fn resolve_overlaps(&mut self, decoded: &E) -> bool {
        let at = decoded.at();
        let first = self
            .rom
            .partition_point(|entry| entry.at() < at.offset_by(1 - i64::from(E::MAX_LEN)));
        let end = decoded.end();
        let overlapping: Vec<usize> = (first..self.rom.len())
            .take_while(|&index| self.rom[index].at() < end)
            .filter(|&index| {
                let entry = &self.rom[index];
                entry.at().block == at.block && entry.at().bank == at.bank && entry.end() > at
            })
            .collect();

        if overlapping
            .iter()
            .any(|&index| self.rom[index].confidence() >= decoded.confidence())
        {
            return false;
        }

        for index in overlapping.into_iter().rev() {
            self.rom.remove(index);
        }
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.rom.iter()
    }

    pub fn ram(&self) -> &[Address] {
        &self.ram
    }

    pub fn rows(&self) -> usize {
        self.rom.len() + self.ram.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows() == 0
    }

    pub(crate) fn row(&self, index: usize) -> Option<Row<'_, E>> {
        match self.rom.get(index) {
            Some(entry) => Some(Row::Rom(entry)),
            None => self.ram.get(index - self.rom.len()).copied().map(Row::Ram),
        }
    }

    pub fn row_of(&self, at: Address) -> Option<usize> {
        let index = self.rom.partition_point(|entry| entry.at() <= at);
        if let Some(index) = index.checked_sub(1)
            && self.rom[index].contains(at)
        {
            return Some(index);
        }
        let index = self
            .ram
            .partition_point(|&start| start <= at)
            .checked_sub(1)?;
        let start = self.ram[index];
        let near = start.block == at.block && start.bank == at.bank && at.offset - start.offset < 3;
        near.then_some(self.rom.len() + index)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    Conditional,
    Unconditional,
    Fetched,
}
