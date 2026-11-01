/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff11--nr11-channel-1-length-timer--duty-cycle
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Channel12Timer {
    /// Write-only
    pub initial_length_timer: u8,
    /// Read/Write
    /// Controls the output waveform
    pub wave_duty: u8,
}

impl From<u8> for Channel12Timer {
    fn from(value: u8) -> Self {
        Self {
            initial_length_timer: value & 0b0011_1111,
            wave_duty: (value >> 6) & 0b11,
        }
    }
}

impl From<Channel12Timer> for u8 {
    fn from(value: Channel12Timer) -> Self {
        0x3F | ((value.wave_duty & 0b11) << 6)
    }
}
