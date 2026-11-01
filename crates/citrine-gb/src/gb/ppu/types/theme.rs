use crate::gb::ppu::types::color::RGBA;
use std::fmt::Display;

const fn shades(lightest: u32, light: u32, dark: u32, darkest: u32) -> [RGBA; 4] {
    [
        RGBA::hex(lightest << 8 | 0xFF),
        RGBA::hex(light << 8 | 0xFF),
        RGBA::hex(dark << 8 | 0xFF),
        RGBA::hex(darkest << 8 | 0xFF),
    ]
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "strum", derive(strum_macros::EnumIter))]
pub enum DmgTheme {
    #[default]
    Citrine,
    Original,
    Pocket,
    GreyScale,
    Amber,
    Aqua,
    Gold,
    Gooseberry,
    Hollow,
    Metallic,
    Mist,
    Mokky,
    Papercut,
    Slimy,
    Snowflake,
    Velvet,
    Custom([RGBA; 4]),
}

impl DmgTheme {
    pub const SELECTABLE: &'static [Self] = &[
        Self::Citrine,
        Self::Original,
        Self::Pocket,
        Self::GreyScale,
        Self::Amber,
        Self::Aqua,
        Self::Gold,
        Self::Gooseberry,
        Self::Hollow,
        Self::Metallic,
        Self::Mist,
        Self::Mokky,
        Self::Papercut,
        Self::Slimy,
        Self::Snowflake,
        Self::Velvet,
    ];

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
            Self::Amber => const { &shades(0xFED018, 0xD35600, 0x5E1210, 0x0D0405) },
            Self::Aqua => const { &shades(0x9FF4E5, 0x00B9BE, 0x005F8C, 0x002B59) },
            Self::Gold => const { &shades(0xCFAB51, 0x9D654C, 0x4D222C, 0x210B1B) },
            Self::Gooseberry => const { &shades(0xBFDCE1, 0xB883B9, 0x6C56AB, 0x48376C) },
            Self::Hollow => const { &shades(0xFAFBF6, 0xC6B7BE, 0x565A75, 0x0F0F1B) },
            Self::Metallic => const { &shades(0xC5DBD4, 0x778E98, 0x41485D, 0x221E31) },
            Self::Mist => const { &shades(0xC4F0C2, 0x5AB9A8, 0x1E606E, 0x2D1B00) },
            Self::Mokky => const { &shades(0xCCA66E, 0x99683D, 0x664930, 0x332920) },
            Self::Papercut => const { &shades(0xCDB27B, 0x8B7D73, 0x3F3D47, 0x0C0C0D) },
            Self::Slimy => const { &shades(0xD1CB95, 0x40985E, 0x1A644E, 0x04373B) },
            Self::Snowflake => const { &shades(0xE7EDEB, 0x8ECECE, 0x62A1C7, 0x3F6ECC) },
            Self::Velvet => const { &shades(0x9775A6, 0x683A68, 0x412752, 0x2D162C) },
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
            Self::Amber => write!(f, "Amber"),
            Self::Aqua => write!(f, "Aqua"),
            Self::Gold => write!(f, "Gold"),
            Self::Gooseberry => write!(f, "Gooseberry"),
            Self::Hollow => write!(f, "Hollow"),
            Self::Metallic => write!(f, "Metallic"),
            Self::Mist => write!(f, "Mist"),
            Self::Mokky => write!(f, "Mokky"),
            Self::Papercut => write!(f, "Papercut"),
            Self::Slimy => write!(f, "Slimy"),
            Self::Snowflake => write!(f, "Snowflake"),
            Self::Velvet => write!(f, "Velvet"),
            Self::Custom(_) => write!(f, "Custom"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(theme: &DmgTheme) -> String {
        theme
            .palette()
            .iter()
            .map(|c| format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b()))
            .collect::<Vec<_>>()
            .join("-")
    }

    #[test]
    fn the_named_palettes_match_the_codes_they_came_from() {
        let expected = [
            (DmgTheme::Amber, "FED018-D35600-5E1210-0D0405"),
            (DmgTheme::Aqua, "9FF4E5-00B9BE-005F8C-002B59"),
            (DmgTheme::Gold, "CFAB51-9D654C-4D222C-210B1B"),
            (DmgTheme::Gooseberry, "BFDCE1-B883B9-6C56AB-48376C"),
            (DmgTheme::Hollow, "FAFBF6-C6B7BE-565A75-0F0F1B"),
            (DmgTheme::Metallic, "C5DBD4-778E98-41485D-221E31"),
            (DmgTheme::Mist, "C4F0C2-5AB9A8-1E606E-2D1B00"),
            (DmgTheme::Mokky, "CCA66E-99683D-664930-332920"),
            (DmgTheme::Papercut, "CDB27B-8B7D73-3F3D47-0C0C0D"),
            (DmgTheme::Slimy, "D1CB95-40985E-1A644E-04373B"),
            (DmgTheme::Snowflake, "E7EDEB-8ECECE-62A1C7-3F6ECC"),
            (DmgTheme::Velvet, "9775A6-683A68-412752-2D162C"),
        ];

        for (theme, want) in expected {
            assert_eq!(code(&theme), want, "{theme}");
        }
    }

    #[test]
    fn every_selectable_palette_is_opaque_and_distinct() {
        let mut seen: Vec<String> = Vec::new();
        for theme in DmgTheme::SELECTABLE {
            assert!(
                theme.palette().iter().all(|c| c.a() == 0xFF),
                "{theme} has a transparent shade"
            );
            let code = code(theme);
            assert!(!seen.contains(&code), "{theme} repeats an earlier palette");
            seen.push(code);
        }
    }
}
