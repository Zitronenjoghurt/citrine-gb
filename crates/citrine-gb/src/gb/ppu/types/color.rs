#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RGBA([u8; 4]);

impl RGBA {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 0xFF)
    }

    pub const fn hex(hex: u32) -> Self {
        Self::new(
            ((hex >> 24) & 0xFF) as u8,
            ((hex >> 16) & 0xFF) as u8,
            ((hex >> 8) & 0xFF) as u8,
            (hex & 0xFF) as u8,
        )
    }

    pub fn rgb555(color: u16) -> Self {
        let scale = |c: u16| ((c << 3) | (c >> 2)) as u8;
        Self::rgb(
            scale(color & 0x1F),
            scale((color >> 5) & 0x1F),
            scale((color >> 10) & 0x1F),
        )
    }

    pub fn rgb555_screen(color: u16) -> Self {
        let r = (color & 0x1F) as u32;
        let g = ((color >> 5) & 0x1F) as u32;
        let b = ((color >> 10) & 0x1F) as u32;
        let mix = |value: u32| (value.min(960) >> 2) as u8;
        Self::rgb(
            mix(r * 26 + g * 4 + b * 2),
            mix(g * 24 + b * 8),
            mix(r * 6 + g * 4 + b * 22),
        )
    }

    pub const fn grey(value: u8) -> Self {
        Self::new(value, value, value, 0xFF)
    }

    pub fn r(&self) -> u8 {
        self.0[0]
    }

    pub fn g(&self) -> u8 {
        self.0[1]
    }

    pub fn b(&self) -> u8 {
        self.0[2]
    }

    pub fn a(&self) -> u8 {
        self.0[3]
    }
}

impl From<[u8; 4]> for RGBA {
    fn from(arr: [u8; 4]) -> Self {
        Self(arr)
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DmgColors {
    #[default]
    Theme,
    PaletteRam,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "strum", derive(strum_macros::EnumIter))]
pub enum ColorCorrection {
    #[default]
    Raw,
    Screen,
}

impl ColorCorrection {
    pub fn apply(&self, color: u16) -> RGBA {
        match self {
            Self::Raw => RGBA::rgb555(color),
            Self::Screen => RGBA::rgb555_screen(color),
        }
    }
}

impl std::fmt::Display for ColorCorrection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Raw => write!(f, "Raw"),
            Self::Screen => write!(f, "Like a GBC"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_colours_use_the_full_range() {
        assert_eq!(
            ColorCorrection::Raw.apply(0x7FFF),
            RGBA::rgb(0xFF, 0xFF, 0xFF)
        );
        assert_eq!(ColorCorrection::Raw.apply(0x001F), RGBA::rgb(0xFF, 0, 0));
    }

    #[test]
    fn screen_colours_are_softer() {
        assert_eq!(
            ColorCorrection::Screen.apply(0x7FFF),
            RGBA::rgb(240, 240, 240)
        );
        assert_eq!(ColorCorrection::Screen.apply(0x001F), RGBA::rgb(201, 0, 46));
        assert_eq!(ColorCorrection::Screen.apply(0x0000), RGBA::rgb(0, 0, 0));
    }
}
