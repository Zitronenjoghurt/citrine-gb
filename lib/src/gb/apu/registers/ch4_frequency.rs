#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Channel4Frequency {
    /// Influences the frequency formula, (value of 0 = 0.5)
    pub clock_divider: u8,
    /// False = 15 bit
    /// True = 7 bit
    pub lfsr_width: bool,
    /// Influences the frequency formula
    pub clock_shift: u8,
}

impl From<u8> for Channel4Frequency {
    fn from(value: u8) -> Self {
        Self {
            clock_divider: value & 0b111,
            lfsr_width: (value & 0b1000) != 0,
            clock_shift: (value & 0b1111_0000) >> 4,
        }
    }
}

impl From<Channel4Frequency> for u8 {
    fn from(value: Channel4Frequency) -> Self {
        (value.clock_divider & 0b111)
            | ((value.lfsr_width as u8) << 3)
            | ((value.clock_shift & 0b1111) << 4)
    }
}
