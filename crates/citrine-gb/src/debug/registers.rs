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
