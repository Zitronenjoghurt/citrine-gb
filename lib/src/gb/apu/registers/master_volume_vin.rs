/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff24--nr50-master-volume--vin-panning
/// - VIN controls if left/right output is enabled
/// - Left/Right volume 0 = 1, volume 7 = 8 (1-indexed)
///
/// The amplifier never mutes a non-silent input
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MasterVolumeVin {
    pub volume_right: u8,
    pub vin_right: bool,
    pub volume_left: u8,
    pub vin_left: bool,
}

impl From<u8> for MasterVolumeVin {
    fn from(value: u8) -> Self {
        Self {
            volume_right: value & 0b111,
            vin_right: (value & 0b1000) != 0,
            volume_left: (value >> 4) & 0b111,
            vin_left: (value >> 7) != 0,
        }
    }
}

impl From<MasterVolumeVin> for u8 {
    fn from(value: MasterVolumeVin) -> Self {
        (value.volume_right & 0b111)
            | ((value.vin_right as u8) << 3)
            | ((value.volume_left & 0b111) << 4)
            | ((value.vin_left as u8) << 7)
    }
}
