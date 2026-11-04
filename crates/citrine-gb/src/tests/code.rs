use crate::WriteMemory;
use crate::debug::code::{Address, BlockId, InstrClass, Line, OperandKind};
use crate::debug::disassembly::{Confidence, decode};
use crate::debug::step::Stop;
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");

const OAM_DMA: [u8; 10] = [0x3E, 0xC0, 0xE0, 0x46, 0x3E, 0x28, 0x3D, 0x20, 0xFD, 0xC9];

const COPY_AND_CALL: [u8; 20] = [
    0x21, 0x80, 0xFF, 0x11, 0x00, 0x02, 0x06, 0x0A, 0x1A, 0x22, 0x13, 0x05, 0x20, 0xFA, 0xCD, 0x80,
    0xFF, 0x18, 0xFE, 0x00,
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
