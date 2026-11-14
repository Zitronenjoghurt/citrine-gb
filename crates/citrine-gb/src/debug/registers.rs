use crate::ReadMemory;
use crate::gb::GameBoy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterGroup {
    pub name: &'static str,
    pub rows: Vec<Register>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register {
    pub name: &'static str,
    pub value: u32,
    pub bits: u8,
    pub fields: &'static [BitField],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitField {
    pub name: &'static str,
    pub shift: u8,
    pub width: u8,
    pub values: &'static [(u32, &'static str)],
}

impl BitField {
    pub const fn bit(name: &'static str, shift: u8) -> Self {
        Self {
            name,
            shift,
            width: 1,
            values: &[],
        }
    }

    pub const fn wide(name: &'static str, shift: u8, width: u8) -> Self {
        Self {
            name,
            shift,
            width,
            values: &[],
        }
    }

    pub const fn named(mut self, values: &'static [(u32, &'static str)]) -> Self {
        self.values = values;
        self
    }

    pub fn value_name(&self, value: u32) -> Option<&'static str> {
        self.values
            .iter()
            .find(|(known, _)| *known == value)
            .map(|(_, name)| *name)
    }

    pub fn read(&self, value: u32) -> u32 {
        (value >> self.shift) & ((1 << self.width) - 1)
    }
}

impl Register {
    pub fn new(name: &'static str, value: impl Into<u32>, bits: u8) -> Self {
        Self {
            name,
            value: value.into(),
            bits,
            fields: &[],
        }
    }

    pub fn with_fields(mut self, fields: &'static [BitField]) -> Self {
        self.fields = fields;
        self
    }

    pub fn field(&self, name: &str) -> Option<u32> {
        self.fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.read(self.value))
    }

    pub fn hex(&self) -> String {
        let digits = usize::from(self.bits.div_ceil(4));
        format!("{:0digits$X}", self.value)
    }

    pub fn binary(&self) -> String {
        let digits = usize::from(self.bits);
        format!("{:0digits$b}", self.value)
    }
}

const FLAGS: &[BitField] = &[
    BitField::bit("Z", 7),
    BitField::bit("N", 6),
    BitField::bit("H", 5),
    BitField::bit("C", 4),
];

const INTERRUPTS: &[BitField] = &[
    BitField::bit("VBlank", 0),
    BitField::bit("LCD", 1),
    BitField::bit("Timer", 2),
    BitField::bit("Serial", 3),
    BitField::bit("Joypad", 4),
];

const LCDC: &[BitField] = &[
    BitField::bit("LCD on", 7),
    BitField::bit("Window map", 6),
    BitField::bit("Window on", 5),
    BitField::bit("Tile data", 4),
    BitField::bit("BG map", 3),
    BitField::bit("OBJ size", 2),
    BitField::bit("OBJ on", 1),
    BitField::bit("BG on", 0),
];

const STAT: &[BitField] = &[
    BitField::bit("LYC int", 6),
    BitField::bit("Mode 2 int", 5),
    BitField::bit("Mode 1 int", 4),
    BitField::bit("Mode 0 int", 3),
    BitField::bit("LY=LYC", 2),
    BitField::wide("Mode", 0, 2),
];

const TAC: &[BitField] = &[BitField::bit("On", 2), BitField::wide("Clock", 0, 2)];

const KEY1: &[BitField] = &[BitField::bit("Double speed", 7), BitField::bit("Armed", 0)];

const VBK: &[BitField] = &[BitField::bit("Bank", 0)];

const SVBK: &[BitField] = &[BitField::wide("Bank", 0, 3)];

fn pair(high: u8, low: u8) -> u16 {
    u16::from_be_bytes([high, low])
}

impl GameBoy {
    pub fn registers(&self, out: &mut Vec<RegisterGroup>) {
        out.clear();
        let cpu = &self.cpu;
        out.push(RegisterGroup {
            name: "CPU",
            rows: vec![
                Register::new("AF", pair(cpu.a, cpu.f.into()), 16).with_fields(FLAGS),
                Register::new("BC", pair(cpu.b, cpu.c), 16),
                Register::new("DE", pair(cpu.d, cpu.e), 16),
                Register::new("HL", pair(cpu.h, cpu.l), 16),
                Register::new("SP", cpu.sp, 16),
                Register::new("PC", cpu.pc, 16),
                Register::new("IR", cpu.ir, 8),
                Register::new("IME", cpu.ime, 1),
                Register::new("Halted", cpu.halted, 1),
            ],
        });
        out.push(RegisterGroup {
            name: "Interrupts",
            rows: vec![
                Register::new("IE", self.ic.enable, 8).with_fields(INTERRUPTS),
                Register::new("IF", u8::from(self.ic.flag), 8).with_fields(INTERRUPTS),
            ],
        });
        let ppu = &self.ppu;
        out.push(RegisterGroup {
            name: "PPU",
            rows: vec![
                Register::new("LCDC", u8::from(ppu.lcdc), 8).with_fields(LCDC),
                Register::new("STAT", u8::from(ppu.stat), 8).with_fields(STAT),
                Register::new("SCY", ppu.scy, 8),
                Register::new("SCX", ppu.scx, 8),
                Register::new("LY", ppu.ly, 8),
                Register::new("LYC", ppu.lyc, 8),
                Register::new("WY", ppu.wy, 8),
                Register::new("WX", ppu.wx, 8),
                Register::new("Dot", ppu.line_dot_counter, 9),
                Register::new("Window line", ppu.fetcher.wl, 8),
            ],
        });
        let timer = &self.timer;
        out.push(RegisterGroup {
            name: "Timer",
            rows: vec![
                Register::new("DIV", timer.div, 16),
                Register::new("TIMA", timer.tima, 8),
                Register::new("TMA", timer.tma, 8),
                Register::new("TAC", timer.tac, 8).with_fields(TAC),
            ],
        });
        if self.model.is_cgb() {
            out.push(RegisterGroup {
                name: "GBC",
                rows: vec![
                    Register::new("KEY1", self.cgb.read(0xFF4D), 8).with_fields(KEY1),
                    Register::new("VBK", ppu.read_naive(0xFF4F), 8).with_fields(VBK),
                    Register::new("SVBK", self.memory.svbk(), 8).with_fields(SVBK),
                ],
            });
        }
    }
}

#[cfg(all(test, feature = "debug"))]
mod tests {
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
        assert_eq!(
            flags.field("VBlank"),
            Some(u32::from(
                gb.ic.flag.is_enabled(crate::gb::ic::Interrupt::VBlank)
            ))
        );
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
}
