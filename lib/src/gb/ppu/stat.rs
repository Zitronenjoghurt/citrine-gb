use crate::gb::ppu::mode::PpuMode;

/// Source: https://gbdev.io/pandocs/STAT.html#ff41--stat-lcd-status
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct STAT {
    pub lyc_interrupt: bool,
    pub mode2_interrupt: bool, // OAMSearch interrupt
    pub mode1_interrupt: bool, // VBlank interrupt
    pub mode0_interrupt: bool, // HBlank interrupt
    pub lyc_equals_ly: bool,
    pub ppu_mode: PpuMode,
}

impl From<u8> for STAT {
    fn from(value: u8) -> Self {
        Self {
            lyc_interrupt: (value & 0b0100_0000) != 0,
            mode2_interrupt: (value & 0b0010_0000) != 0,
            mode1_interrupt: (value & 0b0001_0000) != 0,
            mode0_interrupt: (value & 0b0000_1000) != 0,
            lyc_equals_ly: (value & 0b0000_0100) != 0,
            ppu_mode: (value & 0b11).into(),
        }
    }
}

impl From<STAT> for u8 {
    fn from(value: STAT) -> Self {
        0x80 | ((value.lyc_interrupt as u8) << 6)
            | ((value.mode2_interrupt as u8) << 5)
            | ((value.mode1_interrupt as u8) << 4)
            | ((value.mode0_interrupt as u8) << 3)
            | ((value.lyc_equals_ly as u8) << 2)
            | ((value.ppu_mode as u8) & 0b11)
    }
}
