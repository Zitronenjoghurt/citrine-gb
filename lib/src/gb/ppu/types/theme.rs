use crate::gb::ppu::types::color::RGBA;
use std::fmt::Display;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DmgTheme {
    #[default]
    Citrine,
    GreyScale,
    GbPocket,
    Custom([RGBA; 4]),
}

impl DmgTheme {
    pub const SELECTABLE: &'static [Self] = &[Self::Citrine, Self::GreyScale, Self::GbPocket];

    pub fn palette(&self) -> &[RGBA; 4] {
        match self {
            Self::Citrine => {
                const {
                    &[
                        RGBA::rgb(0xF2, 0xCE, 0x44),
                        RGBA::rgb(0xB8, 0x9D, 0x32),
                        RGBA::rgb(0x72, 0x61, 0x1C),
                        RGBA::rgb(0x29, 0x24, 0x06),
                    ]
                }
            }
            Self::GreyScale => {
                const {
                    &[
                        RGBA::grey(0xFF),
                        RGBA::grey(0xAA),
                        RGBA::grey(0x55),
                        RGBA::grey(0x00),
                    ]
                }
            }
            Self::GbPocket => {
                const {
                    &[
                        RGBA::rgb(0xC5, 0xCA, 0xA4),
                        RGBA::rgb(0x8C, 0x92, 0x6B),
                        RGBA::rgb(0x4A, 0x51, 0x38),
                        RGBA::rgb(0x18, 0x18, 0x18),
                    ]
                }
            }
            Self::Custom(palette) => palette,
        }
    }

    pub fn color_from_shade(&self, shade: u8) -> RGBA {
        self.palette()[(shade & 0b11) as usize]
    }
}

impl Display for DmgTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Citrine => write!(f, "Citrine"),
            Self::GreyScale => write!(f, "Grey Scale"),
            Self::GbPocket => write!(f, "Game Boy Pocket"),
            Self::Custom(_) => write!(f, "Custom"),
        }
    }
}
