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
    NdsBios9,
    NdsBios7,
    NdsItcm,
    NdsMainRam,
    NdsDtcm,
    NdsSharedWram,
    NdsArm7Wram,
    NdsIo,
    NdsFifo,
    NdsPalette,
    NdsVram(u8),
    NdsOam,
}

pub const GBA_BANK: u32 = 0x1_0000;

pub const NDS_BANK_SHIFT: u32 = 14;

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
                | BlockId::NdsBios9
                | BlockId::NdsBios7
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

    pub fn nds_address(&self) -> Option<u32> {
        let base = match self.block {
            BlockId::NdsBios9 => 0xFFFF_0000,
            BlockId::NdsBios7 | BlockId::NdsItcm => 0x0000_0000,
            BlockId::NdsMainRam => 0x0200_0000,
            BlockId::NdsSharedWram => 0x0300_0000,
            BlockId::NdsArm7Wram => 0x0380_0000,
            BlockId::NdsIo => 0x0400_0000,
            BlockId::NdsFifo => 0x0410_0000,
            BlockId::NdsPalette => 0x0500_0000,
            BlockId::NdsOam => 0x0700_0000,
            BlockId::NdsDtcm | BlockId::NdsVram(_) => u32::from(self.bank) << NDS_BANK_SHIFT,
            _ => return None,
        };
        Some(base.wrapping_add(self.offset))
    }

    pub fn flat_address(&self) -> Option<u32> {
        self.gba_address().or_else(|| self.nds_address())
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(addr) = self.flat_address() {
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

#[cfg(all(test, feature = "debug"))]
mod tests {
    use crate::WriteMemory;
    use crate::debug::code::{Address, BlockId, InstrClass, Line, OperandKind};
    use crate::debug::disassembly::{Confidence, decode};
    use crate::debug::step::Stop;
    use crate::gb::{GameBoy, GbModel};
    use crate::rom::Rom;

    const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");

    const OAM_DMA: [u8; 10] = [0x3E, 0xC0, 0xE0, 0x46, 0x3E, 0x28, 0x3D, 0x20, 0xFD, 0xC9];

    const COPY_AND_CALL: [u8; 20] = [
        0x21, 0x80, 0xFF, 0x11, 0x00, 0x02, 0x06, 0x0A, 0x1A, 0x22, 0x13, 0x05, 0x20, 0xFA, 0xCD,
        0x80, 0xFF, 0x18, 0xFE, 0x00,
    ];

    fn dma_rom() -> GameBoy {
        let mut data = vec![0u8; 0x8000];
        data[0x0100..0x0104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
        data[0x0150..0x0150 + COPY_AND_CALL.len()].copy_from_slice(&COPY_AND_CALL);
        data[0x0200..0x0200 + OAM_DMA.len()].copy_from_slice(&OAM_DMA);
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(&data)).expect("load");
        gb
    }

    fn all_lines(gb: &GameBoy) -> Vec<Line> {
        let mut lines = Vec::new();
        gb.disassemble(0..gb.code_rows(), &mut lines);
        lines
    }

    #[test]
    fn every_line_reads_like_the_old_disassembly_after_600_frames() {
        let mut gb = GameBoy::new_empty(GbModel::Dmg);
        gb.load_rom(&Rom::new(TOBU)).expect("load");
        gb.debugger.static_analysis_enabled = true;
        for _ in 0..600 {
            gb.run_frame();
        }

        let lines = all_lines(&gb);

        assert!(lines.len() > 500, "{} lines", lines.len());
        for line in &lines {
            let mut ctx = [0; 3];
            ctx[..line.bytes().len()].copy_from_slice(line.bytes());
            let expected = decode(&ctx).string_context(&ctx);
            assert_eq!(line.text(), expected, "at {}", line.at);
        }
        assert!(lines.windows(2).all(|pair| pair[0].at < pair[1].at));
    }

    #[test]
    fn a_breakpoint_on_the_oam_dma_routine_in_hram_stops_there() {
        let mut gb = dma_rom();
        let routine = Address::hram(0);
        gb.debugger.breakpoints.insert(routine);

        let stop = gb.run_for(100_000);

        assert_eq!(stop, Stop::Breakpoint(routine));
        assert_eq!(gb.cpu.pc.wrapping_sub(1), 0xFF80);
    }

    #[test]
    fn code_fetched_from_hram_shows_as_ram() {
        let mut gb = dma_rom();
        gb.debugger.static_analysis_enabled = true;
        gb.run_frame();

        let lines = all_lines(&gb);
        let ram: Vec<_> = lines.iter().filter(|line| line.at.is_ram()).collect();

        assert_eq!(ram[0].at, Address::hram(0));
        assert_eq!(ram[0].text(), "LD A, C0");
        assert_eq!(ram[0].confidence, Confidence::Fetched);
        let texts: Vec<_> = ram.iter().map(|line| line.text()).collect();
        assert!(texts.contains(&"RET".to_string()), "{texts:?}");
        assert!(
            lines
                .iter()
                .take_while(|l| !l.at.is_ram())
                .all(|l| l.at.block == BlockId::Rom)
        );
    }

    #[test]
    fn ram_code_is_read_again_each_time() {
        let mut gb = dma_rom();
        gb.debugger.static_analysis_enabled = true;
        gb.run_frame();
        gb.memory.write_naive(0xFF80, 0x00);

        let lines = all_lines(&gb);
        let first = lines
            .iter()
            .find(|line| line.at == Address::hram(0))
            .expect("hram");

        assert_eq!(first.text(), "NOP");
    }

    #[test]
    fn follow_pc_finds_the_line_holding_the_pc() {
        let mut gb = dma_rom();
        gb.debugger.static_analysis_enabled = true;
        gb.run_frame();

        let pc = gb.pc_address().expect("pc");
        let row = gb.code_row_of(pc).expect("row");
        let mut lines = Vec::new();
        gb.disassemble(row..row + 1, &mut lines);

        assert!(
            lines[0].at <= pc && pc < lines[0].end(),
            "{} {}",
            lines[0].at,
            pc
        );
    }

    #[test]
    fn an_address_shows_its_bank_and_cpu_address() {
        assert_eq!(Address::rom(0, 0x0150).to_string(), "00:0150");
        assert_eq!(Address::rom(1, 0x0150).to_string(), "01:4150");
        assert_eq!(Address::rom(0x1A0, 0x0000).to_string(), "01A0:4000");
        assert_eq!(Address::wram(0, 0x10).to_string(), "00:C010");
        assert_eq!(Address::wram(3, 0x10).to_string(), "03:D010");
        assert_eq!(Address::hram(2).to_string(), "00:FF82");
    }

    #[test]
    fn echo_ram_and_banked_wram_name_the_bank() {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        gb.load_rom_with_model(&Rom::new(TOBU), GbModel::Cgb)
            .expect("load");
        gb.memory.set_svbk(3);

        assert_eq!(gb.code_address(0xD010), Some(Address::wram(3, 0x10)));
        assert_eq!(gb.code_address(0xF010), Some(Address::wram(3, 0x10)));
        assert_eq!(gb.code_address(0xC123), Some(Address::wram(0, 0x123)));
        assert_eq!(gb.code_address(0xFF85), Some(Address::hram(5)));
        assert_eq!(gb.code_address(0x9000), None);
    }

    #[test]
    fn lines_carry_a_class_and_operand_kinds() {
        let mut gb = dma_rom();
        gb.debugger.static_analysis_enabled = true;
        gb.run_frame();
        let lines = all_lines(&gb);
        let at = |cpu: u16| {
            lines
                .iter()
                .find(|line| line.at.cpu_address() == cpu && !line.at.is_ram())
                .unwrap_or_else(|| panic!("no line at {cpu:04X}"))
        };

        let call = at(0x015E);
        assert_eq!(call.class, InstrClass::Call);
        assert_eq!(call.bytes(), [0xCD, 0x80, 0xFF]);
        assert_eq!(call.operands[0].kind, OperandKind::Address);
        let load = at(0x0150);
        assert_eq!(load.class, InstrClass::Load);
        assert_eq!(
            load.operands.iter().map(|op| op.kind).collect::<Vec<_>>(),
            [OperandKind::Register, OperandKind::Immediate]
        );
        assert_eq!(at(0x015C).class, InstrClass::Jump);
        assert_eq!(at(0x015C).operands[0].kind, OperandKind::Condition);
    }
}
