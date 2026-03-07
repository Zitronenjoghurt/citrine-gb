use crate::gb::ppu::types::color::RGBA;
use std::fmt::Display;

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "strum", derive(strum_macros::EnumIter))]
pub enum DmgTheme {
    #[default]
    Citrine,
    Original,
    Pocket,
    GreyScale,
    Custom([RGBA; 4]),
}

impl DmgTheme {
    pub const SELECTABLE: &'static [Self] =
        &[Self::Citrine, Self::Original, Self::Pocket, Self::GreyScale];

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
            Self::Original => {
                const {
                    // Might be too dark?
                    &[
                        RGBA::hex(0x8B956DFF),
                        RGBA::hex(0x4B5A3CFF),
                        RGBA::hex(0x253421FF),
                        RGBA::hex(0x0F140AFF),
                    ]
                    //&[
                    //    RGBA::hex(0xE0F8D0FF),
                    //    RGBA::hex(0x88C070FF),
                    //    RGBA::hex(0x346856FF),
                    //    RGBA::hex(0x081820FF),
                    //]
                    //&[
                    //    RGBA::rgb(0x9B, 0xBC, 0x0F),
                    //    RGBA::rgb(0x8B, 0xAC, 0x0F),
                    //    RGBA::rgb(0x30, 0x62, 0x30),
                    //    RGBA::rgb(0x0F, 0x38, 0x0F),
                    //]
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
            Self::Pocket => {
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
            Self::Original => write!(f, "Original"),
            Self::Pocket => write!(f, "Game Boy Pocket"),
            Self::GreyScale => write!(f, "Grey Scale"),
            Self::Custom(_) => write!(f, "Custom"),
        }
    }
}
