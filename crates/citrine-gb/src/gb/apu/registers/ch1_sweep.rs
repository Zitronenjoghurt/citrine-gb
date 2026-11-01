/// Source: https://gbdev.io/pandocs/Audio_Registers.html#ff10--nr10-channel-1-sweep
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Channel1Sweep {
    pub individual_step: u8,
    /// False = Addition / Increase
    /// True = Subtraction / Decrease
    pub direction: bool,
    /// Dictates how often sweep operations happen (in units of 128 Hz)
    /// => Is ignored till the current sweep operation is done, except:
    /// If 0 is written => iterations instantly disabled
    pub pace: u8,
}

impl From<u8> for Channel1Sweep {
    fn from(value: u8) -> Self {
        Self {
            individual_step: value & 0b111,
            direction: (value & 0b1000) != 0,
            pace: (value >> 4) & 0b111,
        }
    }
}

impl From<Channel1Sweep> for u8 {
    fn from(value: Channel1Sweep) -> Self {
        (value.individual_step & 0b111)
            | ((value.direction as u8) << 3)
            | ((value.pace & 0b111) << 4)
            | 0x80
    }
}
