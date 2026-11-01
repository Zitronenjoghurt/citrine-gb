#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BgAttributes {
    pub bg_priority: bool,
    pub y_flip: bool,
    pub x_flip: bool,
    pub bank: bool,
    pub palette: u8,
}

impl From<u8> for BgAttributes {
    fn from(value: u8) -> Self {
        Self {
            bg_priority: (value & 0b1000_0000) != 0,
            y_flip: (value & 0b0100_0000) != 0,
            x_flip: (value & 0b0010_0000) != 0,
            bank: (value & 0b0000_1000) != 0,
            palette: value & 0b0000_0111,
        }
    }
}
