/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff25--nr51-sound-panning
/// Controls in which output (left/right) a channel is played
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SoundPanning {
    pub channel_1_right: bool,
    pub channel_2_right: bool,
    pub channel_3_right: bool,
    pub channel_4_right: bool,
    pub channel_1_left: bool,
    pub channel_2_left: bool,
    pub channel_3_left: bool,
    pub channel_4_left: bool,
}

impl From<u8> for SoundPanning {
    fn from(value: u8) -> Self {
        Self {
            channel_1_right: (value & 0b0000_0001) != 0,
            channel_2_right: (value & 0b0000_0010) != 0,
            channel_3_right: (value & 0b0000_0100) != 0,
            channel_4_right: (value & 0b0000_1000) != 0,
            channel_1_left: (value & 0b0001_0000) != 0,
            channel_2_left: (value & 0b0010_0000) != 0,
            channel_3_left: (value & 0b0100_0000) != 0,
            channel_4_left: (value & 0b1000_0000) != 0,
        }
    }
}

impl From<SoundPanning> for u8 {
    fn from(value: SoundPanning) -> Self {
        (value.channel_1_right as u8)
            | ((value.channel_2_right as u8) << 1)
            | ((value.channel_3_right as u8) << 2)
            | ((value.channel_4_right as u8) << 3)
            | ((value.channel_1_left as u8) << 4)
            | ((value.channel_2_left as u8) << 5)
            | ((value.channel_3_left as u8) << 6)
            | ((value.channel_4_left as u8) << 7)
    }
}
