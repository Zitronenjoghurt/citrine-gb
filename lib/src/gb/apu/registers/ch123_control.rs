/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff14--nr14-channel-1-period-high--control
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Channel123Control {
    /// Write-only
    /// Upper 3 bits of the 11-bit period
    pub period_high: u8,
    /// Read/Write
    /// Controls whether the channel's length timer is enabled
    pub length_enable: bool,
    /// Write-only
    pub trigger: bool,
}

impl From<u8> for Channel123Control {
    fn from(value: u8) -> Self {
        Self {
            period_high: value & 0b111,
            length_enable: (value & 0b0100_0000) != 0,
            trigger: (value & 0b1000_0000) != 0,
        }
    }
}

impl From<Channel123Control> for u8 {
    fn from(value: Channel123Control) -> Self {
        0b1011_1111 | ((value.length_enable as u8) << 6)
    }
}
