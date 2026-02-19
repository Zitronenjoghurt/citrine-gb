#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum PpuMode {
    OamScan = 2,
    Drawing = 3,
    HBlank = 0,
    VBlank = 1,
}

impl From<u8> for PpuMode {
    fn from(value: u8) -> Self {
        match value & 0b11 {
            0 => Self::HBlank,
            1 => Self::VBlank,
            2 => Self::OamScan,
            3 => Self::Drawing,
            _ => unreachable!(),
        }
    }
}

impl From<PpuMode> for u8 {
    fn from(value: PpuMode) -> Self {
        value as u8
    }
}
