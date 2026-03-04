/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff26--nr52-audio-master-control
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AudioMasterControl {
    // Read-only, do not control whether the channels are active => allows to check if they are active
    pub channel_1_enabled: bool,
    pub channel_2_enabled: bool,
    pub channel_3_enabled: bool,
    pub channel_4_enabled: bool,
    /// Controls whether the APU is powered on or not
    /// When turned off, all APU registers are cleared and become read-only (except NR52 itself)
    /// Does not affect Wave RAM or the DIV-APU counter
    pub audio_enabled: bool,
}

impl From<u8> for AudioMasterControl {
    fn from(value: u8) -> Self {
        Self {
            channel_1_enabled: (value & 0b0000_0001) != 0,
            channel_2_enabled: (value & 0b0000_0010) != 0,
            channel_3_enabled: (value & 0b0000_0100) != 0,
            channel_4_enabled: (value & 0b0000_1000) != 0,
            audio_enabled: (value & 0b1000_0000) != 0,
        }
    }
}

impl From<AudioMasterControl> for u8 {
    fn from(value: AudioMasterControl) -> Self {
        0x70 | (value.channel_1_enabled as u8)
            | ((value.channel_2_enabled as u8) << 1)
            | ((value.channel_3_enabled as u8) << 2)
            | ((value.channel_4_enabled as u8) << 3)
            | ((value.audio_enabled as u8) << 7)
    }
}
