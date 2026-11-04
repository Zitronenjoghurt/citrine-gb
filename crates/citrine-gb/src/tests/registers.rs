use crate::ReadMemory;
use crate::debug::registers::{Register, RegisterGroup};
use crate::gb::{GameBoy, GbModel};
use crate::rom::Rom;

const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");
const PETRIS: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/petris.gbc");

fn after_600_frames(data: &[u8], model: GbModel) -> GameBoy {
    let mut gb = GameBoy::new_empty(model);
    gb.load_rom_with_model(&Rom::new(data), model)
        .expect("load");
    for _ in 0..600 {
        gb.run_frame();
    }
    gb
}

fn groups(gb: &GameBoy) -> Vec<RegisterGroup> {
    let mut out = Vec::new();
    gb.registers(&mut out);
    out
}

fn row<'a>(groups: &'a [RegisterGroup], group: &str, name: &str) -> &'a Register {
    groups
        .iter()
        .find(|g| g.name == group)
        .and_then(|g| g.rows.iter().find(|r| r.name == name))
        .unwrap_or_else(|| panic!("no row {group} {name}"))
}

fn names(groups: &[RegisterGroup]) -> Vec<&str> {
    groups.iter().map(|g| g.name).collect()
}

fn pair(high: u8, low: u8) -> u32 {
    u32::from(u16::from_be_bytes([high, low]))
}

#[test]
fn a_dmg_shows_cpu_interrupts_ppu_and_timer() {
    let gb = after_600_frames(TOBU, GbModel::Dmg);

    assert_eq!(names(&groups(&gb)), ["CPU", "Interrupts", "PPU", "Timer"]);
}

#[test]
fn a_gbc_adds_its_banks_and_speed() {
    let gb = after_600_frames(PETRIS, GbModel::Cgb);
    let groups = groups(&gb);

    assert_eq!(names(&groups), ["CPU", "Interrupts", "PPU", "Timer", "GBC"]);
    let gbc: Vec<_> = groups[4].rows.iter().map(|r| r.name).collect();
    assert_eq!(gbc, ["KEY1", "VBK", "SVBK"]);
}

#[test]
fn every_row_shows_its_field_after_600_frames() {
    for (data, model) in [(TOBU, GbModel::Dmg), (PETRIS, GbModel::Cgb)] {
        let gb = after_600_frames(data, model);
        let groups = groups(&gb);
        let cpu = &gb.cpu;
        let ppu = &gb.ppu;
        let mut expected = vec![
            ("CPU", "AF", pair(cpu.a, u8::from(cpu.f)), 16),
            ("CPU", "BC", pair(cpu.b, cpu.c), 16),
            ("CPU", "DE", pair(cpu.d, cpu.e), 16),
            ("CPU", "HL", pair(cpu.h, cpu.l), 16),
            ("CPU", "SP", u32::from(cpu.sp), 16),
            ("CPU", "PC", u32::from(cpu.pc), 16),
            ("CPU", "IR", u32::from(cpu.ir), 8),
            ("CPU", "IME", u32::from(cpu.ime), 1),
            ("CPU", "Halted", u32::from(cpu.halted), 1),
            ("Interrupts", "IE", u32::from(gb.ic.enable), 8),
            ("Interrupts", "IF", u32::from(u8::from(gb.ic.flag)), 8),
            ("PPU", "LCDC", u32::from(u8::from(ppu.lcdc)), 8),
            ("PPU", "STAT", u32::from(u8::from(ppu.stat)), 8),
            ("PPU", "SCY", u32::from(ppu.scy), 8),
            ("PPU", "SCX", u32::from(ppu.scx), 8),
            ("PPU", "LY", u32::from(ppu.ly), 8),
            ("PPU", "LYC", u32::from(ppu.lyc), 8),
            ("PPU", "WY", u32::from(ppu.wy), 8),
            ("PPU", "WX", u32::from(ppu.wx), 8),
            ("PPU", "Dot", u32::from(ppu.line_dot_counter), 9),
            ("PPU", "Window line", u32::from(ppu.fetcher.wl), 8),
            ("Timer", "DIV", u32::from(gb.timer.div), 16),
            ("Timer", "TIMA", u32::from(gb.timer.tima), 8),
            ("Timer", "TMA", u32::from(gb.timer.tma), 8),
            ("Timer", "TAC", u32::from(gb.timer.tac), 8),
        ];
        if model.is_cgb() {
            expected.extend([
                ("GBC", "KEY1", u32::from(gb.cgb.read(0xFF4D)), 8),
                ("GBC", "VBK", u32::from(ppu.read_naive(0xFF4F)), 8),
                ("GBC", "SVBK", u32::from(gb.memory.svbk()), 8),
            ]);
        }

        for (group, name, value, bits) in &expected {
            let row = row(&groups, group, name);
            assert_eq!(
                (row.value, row.bits),
                (*value, *bits),
                "{model:?} {group} {name}"
            );
        }
        let rows: usize = groups.iter().map(|g| g.rows.len()).sum();
        assert_eq!(rows, expected.len(), "{model:?}");
    }
}

#[test]
fn bit_fields_read_their_bits() {
    let gb = after_600_frames(TOBU, GbModel::Dmg);
    let groups = groups(&gb);

    let af = row(&groups, "CPU", "AF");
    assert_eq!(af.field("Z"), Some(u32::from(gb.cpu.f.zero)));
    assert_eq!(af.field("C"), Some(u32::from(gb.cpu.f.carry)));
    let lcdc = row(&groups, "PPU", "LCDC");
    assert_eq!(
        lcdc.field("LCD on"),
        Some(u32::from(gb.ppu.lcdc.lcd_enabled))
    );
    assert_eq!(
        lcdc.field("OBJ size"),
        Some(u32::from(gb.ppu.lcdc.obj_size))
    );
    let stat = row(&groups, "PPU", "STAT");
    assert_eq!(
        stat.field("Mode"),
        Some(u32::from(u8::from(gb.ppu.stat.ppu_mode)))
    );
    let flags = row(&groups, "Interrupts", "IF");
    assert_eq!(flags.field("VBlank"), Some(u32::from(gb.ic.flag.vblank)));
    assert_eq!(flags.field("Missing"), None);
}

#[test]
fn a_register_shows_hex_at_its_width() {
    let gb = after_600_frames(TOBU, GbModel::Dmg);
    let groups = groups(&gb);

    assert_eq!(
        row(&groups, "CPU", "PC").hex(),
        format!("{:04X}", gb.cpu.pc)
    );
    assert_eq!(
        row(&groups, "PPU", "LY").hex(),
        format!("{:02X}", gb.ppu.ly)
    );
    assert_eq!(
        row(&groups, "PPU", "Dot").hex(),
        format!("{:03X}", gb.ppu.line_dot_counter)
    );
    assert_eq!(
        row(&groups, "CPU", "IME").hex(),
        format!("{:X}", u8::from(gb.cpu.ime))
    );
}
