use crate::debug::disassembly::{Confidence, DisassemblySource, Row, decode};
use crate::gb::GameBoy;
use crate::gb::memory::Memory;
use crate::instructions::{Instruction, Operand as Sm83Operand};
use std::fmt::{Display, Formatter};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BlockId {
    BootRom,
    Rom,
    Vram,
    CartRam,
    Wram,
    Oam,
    Io,
    Hram,
    Bios,
    Ewram,
    Iwram,
    GamePak,
    GbaIo,
    Palette,
    GbaVram,
    GbaOam,
    GamePak1,
    GamePak2,
    GbaSave,
}

pub const GBA_BANK: u32 = 0x1_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Address {
    pub block: BlockId,
    pub bank: u16,
    pub offset: u32,
}

impl Address {
    pub const fn rom(bank: u16, offset: u32) -> Self {
        Self {
            block: BlockId::Rom,
            bank,
            offset,
        }
    }

    pub const fn wram(bank: u16, offset: u32) -> Self {
        Self {
            block: BlockId::Wram,
            bank,
            offset,
        }
    }

    pub const fn hram(offset: u32) -> Self {
        Self {
            block: BlockId::Hram,
            bank: 0,
            offset,
        }
    }

    pub fn offset_by(self, delta: i64) -> Self {
        Self {
            offset: (i64::from(self.offset) + delta).max(0) as u32,
            ..self
        }
    }

    pub const fn gba(block: BlockId, offset: u32) -> Self {
        Self {
            block,
            bank: 0,
            offset,
        }
    }

    pub const fn at(block: BlockId, bank: u16, offset: u32) -> Self {
        Self {
            block,
            bank,
            offset,
        }
    }

    pub fn is_ram(&self) -> bool {
        !matches!(
            self.block,
            BlockId::Rom
                | BlockId::BootRom
                | BlockId::Bios
                | BlockId::GamePak
                | BlockId::GamePak1
                | BlockId::GamePak2
        )
    }

    pub fn cpu_address(&self) -> u16 {
        let base: u16 = match (self.block, self.bank) {
            (BlockId::BootRom, _) => 0x0000,
            (BlockId::Rom, 0) => 0x0000,
            (BlockId::Rom, _) => 0x4000,
            (BlockId::Vram, _) => 0x8000,
            (BlockId::CartRam, _) => 0xA000,
            (BlockId::Wram, 0) => 0xC000,
            (BlockId::Wram, _) => 0xD000,
            (BlockId::Oam, _) => 0xFE00,
            (BlockId::Io, _) => 0xFF00,
            (BlockId::Hram, _) => 0xFF80,
            _ => return self.gba_address().unwrap_or(self.offset) as u16,
        };
        base.wrapping_add(self.offset as u16)
    }

    pub fn gba_address(&self) -> Option<u32> {
        let base = match self.block {
            BlockId::Bios => 0x0000_0000,
            BlockId::Ewram => 0x0200_0000,
            BlockId::Iwram => 0x0300_0000,
            BlockId::GbaIo => 0x0400_0000,
            BlockId::Palette => 0x0500_0000,
            BlockId::GbaVram => 0x0600_0000,
            BlockId::GbaOam => 0x0700_0000,
            BlockId::GamePak => 0x0800_0000,
            BlockId::GamePak1 => 0x0A00_0000,
            BlockId::GamePak2 => 0x0C00_0000,
            BlockId::GbaSave => 0x0E00_0000,
            _ => return None,
        };
        Some(base + u32::from(self.bank) * GBA_BANK + self.offset)
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(addr) = self.gba_address() {
            write!(f, "{addr:08X}")
        } else if self.bank > 0xFF {
            write!(f, "{:04X}:{:04X}", self.bank, self.cpu_address())
        } else {
            write!(f, "{:02X}:{:04X}", self.bank, self.cpu_address())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrClass {
    Jump,
    Call,
    Return,
    Stack,
    Load,
    Arithmetic,
    Bits,
    Control,
    Invalid,
}

impl InstrClass {
    pub const ALL: [Self; 9] = [
        Self::Jump,
        Self::Call,
        Self::Return,
        Self::Stack,
        Self::Load,
        Self::Arithmetic,
        Self::Bits,
        Self::Control,
        Self::Invalid,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandKind {
    Register,
    Condition,
    Memory,
    Immediate,
    Address,
    Offset,
}

impl OperandKind {
    pub const ALL: [Self; 6] = [
        Self::Register,
        Self::Condition,
        Self::Memory,
        Self::Immediate,
        Self::Address,
        Self::Offset,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operand {
    pub text: String,
    pub kind: OperandKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub at: Address,
    bytes: [u8; 4],
    len: u8,
    pub mnemonic: &'static str,
    pub operands: Vec<Operand>,
    pub class: InstrClass,
    pub confidence: Confidence,
    pub cycles: u8,
    pub note: Option<String>,
}

impl Line {
    pub(crate) fn sm83(at: Address, ctx: [u8; 3], confidence: Confidence) -> Self {
        let instruction = decode(&ctx);
        let operands = match instruction {
            Instruction::Invalid(op) => vec![Operand {
                text: format!("OP ({op:02X})"),
                kind: OperandKind::Immediate,
            }],
            _ => instruction
                .operands(&ctx)
                .into_iter()
                .flatten()
                .map(|operand| Operand {
                    text: operand.to_string(),
                    kind: operand_kind(&operand),
                })
                .collect(),
        };
        let len = instruction.length().min(3);
        let mut bytes = [0; 4];
        bytes[..len].copy_from_slice(&ctx[..len]);
        Self {
            at,
            bytes,
            len: len as u8,
            mnemonic: instruction.mnemonic(),
            operands,
            class: class(&instruction),
            confidence,
            cycles: instruction.machine_cycles(),
            note: Some(instruction.educational_text(&ctx)),
        }
    }

    #[cfg(feature = "gba")]
    pub(crate) fn code(
        at: Address,
        code: &[u8],
        mnemonic: &'static str,
        operands: Vec<Operand>,
        class: InstrClass,
        confidence: Confidence,
    ) -> Self {
        let len = code.len().min(4);
        let mut bytes = [0; 4];
        bytes[..len].copy_from_slice(&code[..len]);
        Self {
            at,
            bytes,
            len: len as u8,
            mnemonic,
            operands,
            class,
            confidence,
            cycles: 0,
            note: None,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    pub fn end(&self) -> Address {
        self.at.offset_by(i64::from(self.len.max(1)))
    }

    pub fn text(&self) -> String {
        text(self.mnemonic, &self.operands)
    }
}

pub(crate) fn text(mnemonic: &str, operands: &[Operand]) -> String {
    let operands: Vec<&str> = operands.iter().map(|op| op.text.as_str()).collect();
    if operands.is_empty() {
        mnemonic.to_string()
    } else {
        format!("{mnemonic} {}", operands.join(", "))
    }
}

fn operand_kind(operand: &Sm83Operand) -> OperandKind {
    match operand {
        Sm83Operand::Reg(_) => OperandKind::Register,
        Sm83Operand::Cond(_) => OperandKind::Condition,
        Sm83Operand::MemReg(_) => OperandKind::Memory,
        Sm83Operand::Imm8(_) | Sm83Operand::Imm16(_) => OperandKind::Immediate,
        Sm83Operand::Address(_) => OperandKind::Address,
        Sm83Operand::Offset(_) | Sm83Operand::SpOffset(_) => OperandKind::Offset,
    }
}

pub(crate) fn class(instruction: &Instruction) -> InstrClass {
    use Instruction::*;
    match instruction {
        JP_nn | JP_c_nn(_) | JR_n | JR_c_n(_) | JP_HL => InstrClass::Jump,
        CALL_nn | CALL_c_nn(_) | RST_n(_) => InstrClass::Call,
        RET | RETI | RET_c(_) => InstrClass::Return,
        PUSH(_) | POP(_) => InstrClass::Stack,
        LD_rr_nn(_)
        | LD_rr_A(_)
        | LD_A_rr(_)
        | LD_nn_SP
        | LD_r_n(_)
        | LD_r_r(_, _)
        | LDH_C_A
        | LDH_A_C
        | LDH_n_A
        | LDH_A_n
        | LD_nn_A
        | LD_A_nn
        | LD_HL_SP_n
        | LD_SP_HL => InstrClass::Load,
        INC_rr(_) | DEC_rr(_) | ADD_HL_rr(_) | INC_r(_) | DEC_r(_) | ADD_r(_) | ADC_r(_)
        | SUB_r(_) | SBC_r(_) | AND_r(_) | XOR_r(_) | OR_r(_) | CP_r(_) | ADD_n | ADC_n | SUB_n
        | SBC_n | AND_n | XOR_n | OR_n | CP_n | ADD_SP_n | DAA => InstrClass::Arithmetic,
        RLCA
        | RRCA
        | RLA
        | RRA
        | RLC_r(_)
        | RRC_r(_)
        | RL_r(_)
        | RR_r(_)
        | SLA_r(_)
        | SRA_r(_)
        | SWAP_r(_)
        | SRL_r(_)
        | BIT_r(_, _)
        | RES_r(_, _)
        | SET_r(_, _)
        | CPL
        | SCF
        | CCF => InstrClass::Bits,
        NOP | STOP | HALT | DI | EI => InstrClass::Control,
        Invalid(_) => InstrClass::Invalid,
    }
}

impl Memory {
    pub(crate) fn code_address(&self, addr: u16) -> Option<Address> {
        match addr {
            0xC000..=0xCFFF => Some(Address::wram(0, u32::from(addr - 0xC000))),
            0xD000..=0xDFFF => Some(Address::wram(
                self.switchable_bank() as u16,
                u32::from(addr - 0xD000),
            )),
            0xE000..=0xFDFF => self.code_address(addr - 0x2000),
            0xFF80..=0xFFFE => Some(Address::hram(u32::from(addr - 0xFF80))),
            _ => None,
        }
    }

    fn read_code(&self, at: Address) -> u8 {
        let offset = at.offset as usize;
        let byte = match at.block {
            BlockId::Wram => self
                .wram_bank(usize::from(at.bank))
                .and_then(|bank| bank.get(offset)),
            BlockId::Hram => self.hram().get(offset),
            _ => None,
        };
        byte.copied().unwrap_or(0xFF)
    }
}

impl GameBoy {
    pub fn code_address(&self, addr: u16) -> Option<Address> {
        match addr {
            0x0000..=0x7FFF => Some(self.cartridge.rom_address(addr)),
            _ => self.memory.code_address(addr),
        }
    }

    pub fn pc_address(&self) -> Option<Address> {
        self.code_address(self.cpu.pc.wrapping_sub(1))
    }

    pub fn code_rows(&self) -> usize {
        self.debugger.disassembly.rows()
    }

    pub fn code_row_of(&self, at: Address) -> Option<usize> {
        self.debugger.disassembly.row_of(at)
    }

    pub fn disassemble(&self, rows: Range<usize>, out: &mut Vec<Line>) {
        out.clear();
        for index in rows {
            let line = match self.debugger.disassembly.row(index) {
                Some(Row::Rom(entry)) => Line::sm83(entry.at, entry.ctx, entry.confidence),
                Some(Row::Ram(at)) => {
                    let ctx = [0, 1, 2].map(|i| self.memory.read_code(at.offset_by(i)));
                    Line::sm83(at, ctx, Confidence::Fetched)
                }
                None => break,
            };
            out.push(line);
        }
    }
}
