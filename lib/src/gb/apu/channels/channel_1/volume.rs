/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff12--nr12-channel-1-volume--envelope
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Channel1Volume {
    /// Envelope ticks at 64 Hz and will be increased/decreased every x ticks (x = sweep pace)
    /// 0 = envelope disabled
    pub sweep_pace: u8,
    /// False = Decrease volume over time
    /// True = Increase volume over time
    pub envelope_direction: bool,
    /// How loud the channel is initially
    /// ! Bits are readable but not updated by the envelope
    pub initial_volume: u8,
}

impl From<u8> for Channel1Volume {
    fn from(value: u8) -> Self {
        Self {
            sweep_pace: value & 0b111,
            envelope_direction: (value & 0b1000) != 0,
            initial_volume: (value >> 4) & 0b1111,
        }
    }
}

impl From<Channel1Volume> for u8 {
    fn from(value: Channel1Volume) -> Self {
        (value.sweep_pace & 0b111)
            | ((value.envelope_direction as u8) << 3)
            | ((value.initial_volume & 0b1111) << 4)
    }
}
