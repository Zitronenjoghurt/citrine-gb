use super::{CodeMap, CodeSource, Pc, arm, code_address, lines, thumb};
use crate::debug::code::{Address, BlockId, InstrClass};
use crate::debug::disassembly::Confidence;
use crate::gba::wait::Width;
use crate::instructions::FlowControl;

const ROM: u32 = 0x0800_0000;

fn arm_at(addr: u32) -> Pc {
    Pc { addr, thumb: false }
}

fn thumb_at(addr: u32) -> Pc {
    Pc { addr, thumb: true }
}

#[test]
fn arm_text_follows_the_data_sheet() {
    let cases: &[(u32, &str)] = &[
        (0xE3A0_0001, "MOV R0, #0x1"),
        (0xE091_0002, "ADDS R0, R1, R2"),
        (0x1081_0182, "ADDNE R0, R1, R2, LSL #3"),
        (0xE1A0_0061, "MOV R0, R1, RRX"),
        (0xE1A0_0231, "MOV R0, R1, LSR R2"),
        (0xE1A0_0021, "MOV R0, R1, LSR #32"),
        (0xE351_0000, "CMP R1, #0x0"),
        (0xE591_0004, "LDR R0, [R1, #0x4]"),
        (0xE491_0004, "LDR R0, [R1], #0x4"),
        (0xE711_0102, "LDR R0, [R1, -R2, LSL #2]"),
        (0xE5B1_0004, "LDR R0, [R1, #0x4]!"),
        (0xE5D1_0000, "LDRB R0, [R1]"),
        (0xE4B1_0004, "LDRT R0, [R1], #0x4"),
        (0xE1D1_00B2, "LDRH R0, [R1, #0x2]"),
        (0xE191_00D2, "LDRSB R0, [R1, R2]"),
        (0xE151_00F2, "LDRSH R0, [R1, #-0x2]"),
        (0xE92D_4030, "STMDB SP!, {R4, R5, LR}"),
        (0xE8BD_800F, "LDMIA SP!, {R0-R3, PC}"),
        (0xE8D0_0003, "LDMIA R0, {R0, R1}^"),
        (0xE000_0291, "MUL R0, R1, R2"),
        (0xE021_0392, "MLA R1, R2, R3, R0"),
        (0xE081_0392, "UMULL R0, R1, R2, R3"),
        (0xE0F1_0392, "SMLALS R0, R1, R2, R3"),
        (0xE102_0091, "SWP R0, R1, [R2]"),
        (0xE142_0091, "SWPB R0, R1, [R2]"),
        (0xE10F_0000, "MRS R0, CPSR"),
        (0xE14F_0000, "MRS R0, SPSR"),
        (0xE129_F000, "MSR CPSR, R0"),
        (0xE328_F20F, "MSR CPSR_flg, #0xF0000000"),
        (0xE12F_F000, "MSR CPSR_fsxc, R0"),
        (0xEF05_0000, "SWI 0x050000"),
        (0xE12F_FF1E, "BX LR"),
        (0x012F_FF1E, "BXEQ LR"),
        (0xEE01_2310, "MCR p3, 0, R2, c1, c0"),
        (0xEE11_2370, "MRC p3, 0, R2, c1, c0, 3"),
        (0xEE12_3405, "CDP p4, 1, c3, c2, c5"),
        (0xED91_1402, "LDC p4, c1, [R1, #0x8]"),
        (0xE6000010, "UND"),
    ];
    for &(op, text) in cases {
        assert_eq!(arm(ROM, op).text(), text, "{op:08X}");
    }
}

#[test]
fn arm_branches_show_their_targets() {
    assert_eq!(arm(ROM, 0xEB00_0002).text(), "BL 08000010");
    assert_eq!(arm(0x0800_0100, 0x1AFF_FFFE).text(), "BNE 08000100");
    assert_eq!(arm(0x0300_0000, 0xEAFF_FFFE).text(), "B 03000000");
}

#[test]
fn arm_flow_classes() {
    let cases: &[(u32, InstrClass, FlowControl<Pc>)] = &[
        (
            0xEB00_0002,
            InstrClass::Call,
            FlowControl::Call(arm_at(0x0800_0010)),
        ),
        (
            0xEA00_0002,
            InstrClass::Jump,
            FlowControl::Jump(arm_at(0x0800_0010)),
        ),
        (
            0x1A00_0002,
            InstrClass::Jump,
            FlowControl::ConditionalJump(arm_at(0x0800_0010)),
        ),
        (0xE12F_FF1E, InstrClass::Return, FlowControl::Return),
        (
            0x012F_FF1E,
            InstrClass::Return,
            FlowControl::ConditionalReturn,
        ),
        (0xE12F_FF10, InstrClass::Jump, FlowControl::UnknownJump),
        (0xE1A0_F00E, InstrClass::Return, FlowControl::Return),
        (0xE1B0_F00E, InstrClass::Return, FlowControl::Return),
        (0xE25E_F004, InstrClass::Return, FlowControl::Return),
        (0xE8BD_8010, InstrClass::Return, FlowControl::Return),
        (0xE891_8000, InstrClass::Return, FlowControl::Return),
        (0xE49D_F004, InstrClass::Return, FlowControl::Return),
        (0xE59F_F000, InstrClass::Jump, FlowControl::UnknownJump),
        (0xE08F_F100, InstrClass::Jump, FlowControl::UnknownJump),
        (0xE92D_4030, InstrClass::Stack, FlowControl::Continue),
        (0xE891_0003, InstrClass::Load, FlowControl::Continue),
        (0xE1A0_0001, InstrClass::Load, FlowControl::Continue),
        (0xE081_0002, InstrClass::Arithmetic, FlowControl::Continue),
        (0xE001_0002, InstrClass::Bits, FlowControl::Continue),
        (0xE129_F000, InstrClass::Control, FlowControl::Continue),
        (0xEF00_0005, InstrClass::Call, FlowControl::Continue),
        (0xE600_0010, InstrClass::Invalid, FlowControl::Invalid),
        (0xFA00_0002, InstrClass::Jump, FlowControl::Continue),
    ];
    for &(op, class, flow) in cases {
        let decoded = arm(ROM, op);
        assert_eq!((decoded.class, decoded.flow), (class, flow), "{op:08X}");
        assert_eq!(decoded.len, 4);
    }
}

#[test]
fn thumb_text_follows_the_data_sheet() {
    let cases: &[(u16, &str)] = &[
        (0x0088, "LSL R0, R1, #2"),
        (0x0808, "LSR R0, R1, #32"),
        (0x1888, "ADD R0, R1, R2"),
        (0x1C48, "ADD R0, R1, #0x1"),
        (0x2001, "MOV R0, #0x1"),
        (0x2A10, "CMP R2, #0x10"),
        (0x4348, "MUL R0, R1"),
        (0x4248, "NEG R0, R1"),
        (0x4448, "ADD R0, R9"),
        (0x4770, "BX LR"),
        (0x46F7, "MOV PC, LR"),
        (0x4801, "LDR R0, [PC, #0x4]"),
        (0x5888, "LDR R0, [R1, R2]"),
        (0x5C88, "LDRB R0, [R1, R2]"),
        (0x5288, "STRH R0, [R1, R2]"),
        (0x5688, "LDSB R0, [R1, R2]"),
        (0x5A88, "LDRH R0, [R1, R2]"),
        (0x5E88, "LDSH R0, [R1, R2]"),
        (0x6848, "LDR R0, [R1, #0x4]"),
        (0x6808, "LDR R0, [R1]"),
        (0x7848, "LDRB R0, [R1, #0x1]"),
        (0x8848, "LDRH R0, [R1, #0x2]"),
        (0x9001, "STR R0, [SP, #0x4]"),
        (0xA001, "ADD R0, PC, #0x4"),
        (0xA801, "ADD R0, SP, #0x4"),
        (0xB004, "ADD SP, #0x10"),
        (0xB084, "ADD SP, #-0x10"),
        (0xB510, "PUSH {R4, LR}"),
        (0xB4F0, "PUSH {R4-R7}"),
        (0xBD10, "POP {R4, PC}"),
        (0xC106, "STMIA R1!, {R1, R2}"),
        (0xC9FF, "LDMIA R1!, {R0-R7}"),
        (0xDF05, "SWI 0x05"),
        (0xDE00, "UND"),
        (0xB100, "UND"),
    ];
    for &(op, text) in cases {
        assert_eq!(thumb(ROM, op, None).text(), text, "{op:04X}");
    }
}

#[test]
fn thumb_branches_show_their_targets() {
    assert_eq!(thumb(0x0800_0100, 0xD0FE, None).text(), "BEQ 08000100");
    assert_eq!(thumb(0x0800_0100, 0xE7FE, None).text(), "B 08000100");
    let pair = thumb(ROM, 0xF000, Some(0xF808));
    assert_eq!((pair.text(), pair.len), ("BL 08000014".to_string(), 4));
    let back = thumb(0x0800_1000, 0xF7FF, Some(0xFFFE));
    assert_eq!(back.text(), "BL 08001000");
    assert_eq!(thumb(ROM, 0xF7FF, None).text(), "BL PC-0x1000");
    assert_eq!(thumb(ROM, 0xF808, None).text(), "BL LR+0x10");
}

#[test]
fn thumb_flow_classes() {
    let cases: &[(u16, Option<u16>, InstrClass, FlowControl<Pc>)] = &[
        (
            0xF000,
            Some(0xF808),
            InstrClass::Call,
            FlowControl::Call(thumb_at(0x0800_0014)),
        ),
        (
            0xF000,
            Some(0x2001),
            InstrClass::Call,
            FlowControl::Continue,
        ),
        (0xF808, None, InstrClass::Call, FlowControl::Continue),
        (0x4770, None, InstrClass::Return, FlowControl::Return),
        (0x46F7, None, InstrClass::Return, FlowControl::Return),
        (0xBD10, None, InstrClass::Return, FlowControl::Return),
        (0x4700, None, InstrClass::Jump, FlowControl::UnknownJump),
        (0x4687, None, InstrClass::Jump, FlowControl::UnknownJump),
        (0x4487, None, InstrClass::Jump, FlowControl::UnknownJump),
        (
            0xE002,
            None,
            InstrClass::Jump,
            FlowControl::Jump(thumb_at(0x0800_0008)),
        ),
        (
            0xD002,
            None,
            InstrClass::Jump,
            FlowControl::ConditionalJump(thumb_at(0x0800_0008)),
        ),
        (0xB510, None, InstrClass::Stack, FlowControl::Continue),
        (0xBC10, None, InstrClass::Stack, FlowControl::Continue),
        (0x2001, None, InstrClass::Load, FlowControl::Continue),
        (0xDF05, None, InstrClass::Call, FlowControl::Continue),
        (0xDE00, None, InstrClass::Invalid, FlowControl::Invalid),
    ];
    for &(op, next, class, flow) in cases {
        let decoded = thumb(ROM, op, next);
        assert_eq!((decoded.class, decoded.flow), (class, flow), "{op:04X}");
    }
}

struct Memory {
    rom: Vec<u8>,
    iwram: Vec<u8>,
}

impl Memory {
    fn new() -> Self {
        Self {
            rom: vec![0; 0x100],
            iwram: vec![0; 0x8000],
        }
    }

    fn bytes(&mut self, addr: u32) -> &mut [u8] {
        match addr >> 24 {
            0x08 => &mut self.rom[(addr & 0xFF) as usize..],
            _ => &mut self.iwram[(addr & 0x7FFF) as usize..],
        }
    }

    fn arm(&mut self, addr: u32, ops: &[u32]) {
        for (i, op) in ops.iter().enumerate() {
            self.bytes(addr + 4 * i as u32)[..4].copy_from_slice(&op.to_le_bytes());
        }
    }

    fn thumb(&mut self, addr: u32, ops: &[u16]) {
        for (i, op) in ops.iter().enumerate() {
            self.bytes(addr + 2 * i as u32)[..2].copy_from_slice(&op.to_le_bytes());
        }
    }
}

impl CodeSource for Memory {
    fn read_code(&self, addr: u32, width: Width) -> u32 {
        let bytes = match addr >> 24 {
            0x08 => &self.rom[(addr & 0xFF) as usize..],
            _ => &self.iwram[(addr & 0x7FFF) as usize..],
        };
        (0..width.bytes()).fold(0, |value, i| {
            value | u32::from(bytes.get(i).copied().unwrap_or(0)) << (8 * i)
        })
    }

    fn rom_len(&self) -> usize {
        self.rom.len()
    }
}

fn texts(map: &CodeMap, src: &Memory) -> Vec<String> {
    let mut out = Vec::new();
    lines(map, src, 0..map.rows(), &mut out);
    out.iter()
        .map(|line| format!("{} {}", line.at, line.text()))
        .collect()
}

#[test]
fn a_mixed_program_reads_each_part_in_its_own_state() {
    let mut src = Memory::new();
    src.arm(ROM, &[0xE28F_0001, 0xE12F_FF10, 0xEAFF_FFFE]);
    src.thumb(ROM + 0x0C, &[0x2001, 0xF000, 0xF801, 0x4770, 0x4770]);
    let mut map = CodeMap::default();
    map.on_fetch(&src, arm_at(ROM));
    map.on_fetch(&src, thumb_at(ROM + 0x0C));
    assert_eq!(
        texts(&map, &src),
        [
            "08000000 ADD R0, PC, #0x1",
            "08000004 BX R0",
            "0800000C MOV R0, #0x1",
            "0800000E BL 08000014",
            "08000012 BX LR",
            "08000014 BX LR",
        ]
    );
    let mut out = Vec::new();
    lines(&map, &src, 3..4, &mut out);
    assert_eq!(out[0].bytes(), [0x00, 0xF0, 0x01, 0xF8]);
    assert_eq!(out[0].class, InstrClass::Call);
}

#[test]
fn code_in_iwram_is_kept_as_fetched_and_read_live() {
    let mut src = Memory::new();
    src.arm(0x0300_0000, &[0xE3A0_0001, 0xEAFF_FFFD]);
    let mut map = CodeMap::default();
    map.on_fetch(&src, arm_at(0x0300_0000));
    assert_eq!(texts(&map, &src), ["03000000 MOV R0, #0x1"]);
    src.arm(0x0300_0000, &[0xE3A0_0002]);
    assert_eq!(texts(&map, &src), ["03000000 MOV R0, #0x2"]);
    let at = code_address(0x0300_0000, 0).expect("iwram");
    assert!(at.is_ram());
    assert_eq!(map.row_of(at), Some(0));
}

#[test]
fn addresses_name_their_block() {
    let cases = [
        (0x0000_0100, BlockId::Bios, 0x100),
        (0x0204_0010, BlockId::Ewram, 0x10),
        (0x0300_8004, BlockId::Iwram, 0x4),
        (0x0A00_0020, BlockId::GamePak, 0x20),
    ];
    for (addr, block, offset) in cases {
        let at = code_address(addr, 0x100).expect("code");
        assert_eq!((at.block, at.offset), (block, offset), "{addr:08X}");
    }
    assert_eq!(code_address(0x0800_0100, 0x100), None);
    assert_eq!(code_address(0x0600_0000, 0x100), None);
    assert_eq!(
        Address::gba(BlockId::GamePak, 0x1234).to_string(),
        "08001234"
    );
}

#[test]
fn analysis_follows_branches_and_stops_at_returns() {
    let mut src = Memory::new();
    src.arm(
        ROM,
        &[
            0x0A00_0001,
            0xE3A0_0001,
            0xE12F_FF1E,
            0xE3A0_0002,
            0xE12F_FF1E,
            0xE3A0_0003,
        ],
    );
    let mut map = CodeMap::default();
    map.on_fetch(&src, arm_at(ROM));
    let confidences: Vec<(u32, Confidence)> = map
        .iter()
        .map(|entry| (entry.at.offset, entry.confidence))
        .collect();
    assert_eq!(
        confidences,
        [
            (0x00, Confidence::Fetched),
            (0x04, Confidence::Unconditional),
            (0x08, Confidence::Unconditional),
            (0x0C, Confidence::Conditional),
            (0x10, Confidence::Conditional),
        ]
    );
}
