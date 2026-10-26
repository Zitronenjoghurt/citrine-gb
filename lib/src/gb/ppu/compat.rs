use crate::gb::ppu::Ppu;
use crate::gb::ppu::types::color::{DmgColors, RGBA};
use std::fmt::Display;

const TITLE: std::ops::Range<usize> = 0x134..0x144;
const OLD_LICENSEE: usize = 0x14B;
const NEW_LICENSEE: std::ops::Range<usize> = 0x144..0x146;

const TITLE_CHECKSUMS: [u8; 94] = [
    0x00, 0x88, 0x16, 0x36, 0xD1, 0xDB, 0xF2, 0x3C, 0x8C, 0x92, 0x3D, 0x5C, 0x58, 0xC9, 0x3E, 0x70,
    0x1D, 0x59, 0x69, 0x19, 0x35, 0xA8, 0x14, 0xAA, 0x75, 0x95, 0x99, 0x34, 0x6F, 0x15, 0xFF, 0x97,
    0x4B, 0x90, 0x17, 0x10, 0x39, 0xF7, 0xF6, 0xA2, 0x49, 0x4E, 0x43, 0x68, 0xE0, 0x8B, 0xF0, 0xCE,
    0x0C, 0x29, 0xE8, 0xB7, 0x86, 0x9A, 0x52, 0x01, 0x9D, 0x71, 0x9C, 0xBD, 0x5D, 0x6D, 0x67, 0x3F,
    0x6B, 0xB3, 0x46, 0x28, 0xA5, 0xC6, 0xD3, 0x27, 0x61, 0x18, 0x66, 0x6A, 0xBF, 0x0D, 0xF4, 0xB3,
    0x46, 0x28, 0xA5, 0xC6, 0xD3, 0x27, 0x61, 0x18, 0x66, 0x6A, 0xBF, 0x0D, 0xF4, 0xB3,
];

const FIRST_DUPLICATE: usize = 65;

const FOURTH_LETTERS: &[u8; 29] = b"BEFAARBEKEK R-URAR INAILICE R";

const COMBINATION_PER_CHECKSUM: [u8; 94] = [
    0, 4, 5, 35, 34, 3, 31, 15, 10, 5, 19, 36, 7, 37, 30, 44, 21, 32, 31, 20, 5, 33, 13, 14, 5, 29,
    5, 18, 9, 3, 2, 26, 25, 25, 41, 42, 26, 45, 42, 45, 36, 38, 26, 42, 30, 41, 34, 34, 5, 42, 6,
    5, 33, 25, 42, 42, 40, 2, 16, 25, 42, 42, 5, 0, 39, 36, 22, 25, 6, 32, 12, 36, 11, 39, 18, 39,
    24, 31, 50, 17, 46, 6, 27, 0, 47, 41, 41, 0, 0, 19, 34, 23, 18, 29,
];

const COMBINATIONS: [[u8; 3]; 51] = [
    [16, 16, 116],
    [72, 72, 72],
    [80, 80, 80],
    [96, 96, 96],
    [36, 36, 36],
    [0, 0, 0],
    [108, 108, 108],
    [20, 20, 20],
    [48, 48, 48],
    [104, 104, 104],
    [64, 32, 32],
    [16, 112, 112],
    [16, 8, 8],
    [12, 16, 16],
    [16, 116, 116],
    [112, 16, 112],
    [8, 68, 8],
    [64, 64, 32],
    [16, 16, 28],
    [16, 16, 72],
    [16, 16, 80],
    [76, 76, 36],
    [15, 15, 44],
    [68, 68, 8],
    [16, 16, 8],
    [16, 16, 12],
    [112, 112, 0],
    [12, 12, 0],
    [0, 0, 4],
    [72, 88, 72],
    [80, 88, 80],
    [96, 88, 96],
    [64, 88, 32],
    [68, 16, 52],
    [111, 0, 56],
    [111, 16, 60],
    [76, 91, 36],
    [64, 112, 40],
    [16, 92, 112],
    [68, 88, 8],
    [16, 0, 8],
    [16, 112, 12],
    [112, 12, 0],
    [12, 112, 16],
    [84, 112, 16],
    [12, 112, 0],
    [100, 12, 112],
    [0, 112, 32],
    [16, 12, 112],
    [112, 12, 24],
    [16, 112, 116],
];

const COLORS: [u16; 120] = [
    0x7FFF, 0x32BF, 0x00D0, 0x0000, 0x639F, 0x4279, 0x15B0, 0x04CB, 0x7FFF, 0x6E31, 0x454A, 0x0000,
    0x7FFF, 0x1BEF, 0x0200, 0x0000, 0x7FFF, 0x421F, 0x1CF2, 0x0000, 0x7FFF, 0x5294, 0x294A, 0x0000,
    0x7FFF, 0x03FF, 0x012F, 0x0000, 0x7FFF, 0x03EF, 0x01D6, 0x0000, 0x7FFF, 0x42B5, 0x3DC8, 0x0000,
    0x7E74, 0x03FF, 0x0180, 0x0000, 0x67FF, 0x77AC, 0x1A13, 0x2D6B, 0x7ED6, 0x4BFF, 0x2175, 0x0000,
    0x53FF, 0x4A5F, 0x7E52, 0x0000, 0x4FFF, 0x7ED2, 0x3A4C, 0x1CE0, 0x03ED, 0x7FFF, 0x255F, 0x0000,
    0x036A, 0x021F, 0x03FF, 0x7FFF, 0x7FFF, 0x01DF, 0x0112, 0x0000, 0x231F, 0x035F, 0x00F2, 0x0009,
    0x7FFF, 0x03EA, 0x011F, 0x0000, 0x299F, 0x001A, 0x000C, 0x0000, 0x7FFF, 0x027F, 0x001F, 0x0000,
    0x7FFF, 0x03E0, 0x0206, 0x0120, 0x7FFF, 0x7EEB, 0x001F, 0x7C00, 0x7FFF, 0x3FFF, 0x7E00, 0x001F,
    0x7FFF, 0x03FF, 0x001F, 0x0000, 0x03FF, 0x001F, 0x000C, 0x0000, 0x7FFF, 0x033F, 0x0193, 0x0000,
    0x0000, 0x4200, 0x037F, 0x7FFF, 0x7FFF, 0x7E8C, 0x7C00, 0x0000, 0x7FFF, 0x1BEF, 0x6180, 0x0000,
];

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "strum", derive(strum_macros::EnumIter))]
pub enum CompatPalette {
    #[default]
    Auto,
    Brown,
    Red,
    DarkBrown,
    PaleYellow,
    Orange,
    Yellow,
    Blue,
    DarkBlue,
    Grey,
    Green,
    DarkGreen,
    Inverted,
}

impl CompatPalette {
    pub const SELECTABLE: &'static [Self] = &[
        Self::Auto,
        Self::Brown,
        Self::Red,
        Self::DarkBrown,
        Self::PaleYellow,
        Self::Orange,
        Self::Yellow,
        Self::Blue,
        Self::DarkBlue,
        Self::Grey,
        Self::Green,
        Self::DarkGreen,
        Self::Inverted,
    ];

    fn combination(&self, rom: &[u8]) -> usize {
        match self {
            Self::Auto => combination_for_rom(rom),
            Self::Brown => 5,
            Self::Red => 43,
            Self::DarkBrown => 28,
            Self::PaleYellow => 8,
            Self::Orange => 3,
            Self::Yellow => 49,
            Self::Blue => 48,
            Self::DarkBlue => 40,
            Self::Grey => 7,
            Self::Green => 1,
            Self::DarkGreen => 0,
            Self::Inverted => 6,
        }
    }

    pub fn colors(&self, rom: &[u8]) -> [[RGBA; 4]; 3] {
        let [obj0, obj1, bg] = COMBINATIONS[self.combination(rom)];
        [bg, obj0, obj1]
            .map(|offset| std::array::from_fn(|i| RGBA::rgb555(COLORS[offset as usize + i])))
    }
}

impl Display for CompatPalette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "GBC Colours"),
            Self::Brown => write!(f, "GBC Brown"),
            Self::Red => write!(f, "GBC Red"),
            Self::DarkBrown => write!(f, "GBC Dark Brown"),
            Self::PaleYellow => write!(f, "GBC Pale Yellow"),
            Self::Orange => write!(f, "GBC Orange"),
            Self::Yellow => write!(f, "GBC Yellow"),
            Self::Blue => write!(f, "GBC Blue"),
            Self::DarkBlue => write!(f, "GBC Dark Blue"),
            Self::Grey => write!(f, "GBC Grey"),
            Self::Green => write!(f, "GBC Green"),
            Self::DarkGreen => write!(f, "GBC Dark Green"),
            Self::Inverted => write!(f, "GBC Inverted"),
        }
    }
}

fn is_nintendo(rom: &[u8]) -> bool {
    match rom.get(OLD_LICENSEE) {
        Some(0x01) => true,
        Some(0x33) => rom.get(NEW_LICENSEE) == Some(b"01".as_slice()),
        _ => false,
    }
}

fn combination_for_rom(rom: &[u8]) -> usize {
    if !is_nintendo(rom) {
        return 0;
    }
    let Some(title) = rom.get(TITLE) else {
        return 0;
    };
    let checksum = title.iter().fold(0u8, |sum, &b| sum.wrapping_add(b));
    TITLE_CHECKSUMS
        .iter()
        .enumerate()
        .find(|&(i, &c)| {
            c == checksum
                && (i < FIRST_DUPLICATE || FOURTH_LETTERS[i - FIRST_DUPLICATE] == title[3])
        })
        .map(|(i, _)| COMBINATION_PER_CHECKSUM[i] as usize)
        .unwrap_or(0)
}

fn palette_bytes(offset: u8) -> [u8; 8] {
    let mut bytes = [0; 8];
    for (i, color) in COLORS[offset as usize..offset as usize + 4]
        .iter()
        .enumerate()
    {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&color.to_le_bytes());
    }
    bytes
}

impl Ppu {
    pub fn load_compat_palettes(&mut self, rom: &[u8]) {
        self.write_compat_palette(CompatPalette::Auto, rom);
        self.opri = 1;
    }

    pub fn set_compat_palette(&mut self, palette: CompatPalette, rom: &[u8]) {
        if self.cgb_mode {
            return;
        }
        self.write_compat_palette(palette, rom);
        self.dmg_colors = DmgColors::PaletteRam;
    }

    fn write_compat_palette(&mut self, palette: CompatPalette, rom: &[u8]) {
        let [obj0, obj1, bg] = COMBINATIONS[palette.combination(rom)];
        self.obj_palette_ram[0..8].copy_from_slice(&palette_bytes(obj0));
        self.obj_palette_ram[8..16].copy_from_slice(&palette_bytes(obj1));
        self.bg_palette_ram[0..8].copy_from_slice(&palette_bytes(bg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gb::GbModel;

    fn header(old_licensee: u8, new_licensee: &[u8; 2], checksum: u8, fourth: u8) -> Vec<u8> {
        let mut rom = vec![0; 0x150];
        rom[OLD_LICENSEE] = old_licensee;
        rom[NEW_LICENSEE].copy_from_slice(new_licensee);
        rom[0x134] = checksum.wrapping_sub(fourth);
        rom[0x137] = fourth;
        rom
    }

    #[test]
    fn other_licensees_get_the_default() {
        assert_eq!(combination_for_rom(&header(0x00, b"00", 0xDB, b'R')), 0);
        assert_eq!(combination_for_rom(&header(0x33, b"08", 0xDB, b'R')), 0);
    }

    #[test]
    fn nintendo_titles_pick_by_checksum() {
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0xDB, b'R')), 3);
        assert_eq!(combination_for_rom(&header(0x33, b"01", 0xDB, b'R')), 3);
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0x14, b'K')), 13);
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0x42, b'K')), 0);
    }

    #[test]
    fn duplicate_checksums_pick_by_fourth_letter() {
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0x46, b'E')), 22);
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0x46, b'R')), 46);
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0x46, b'X')), 0);
        assert_eq!(combination_for_rom(&header(0x01, b"00", 0xB3, b'R')), 29);
    }

    #[test]
    fn default_palettes_land_in_palette_ram() {
        let mut ppu = Ppu::new(GbModel::Cgb);
        ppu.set_cgb_mode(false);
        ppu.load_compat_palettes(&header(0x00, b"00", 0, 0));

        let colors = |ram: &[u8]| -> Vec<u16> {
            ram.chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect()
        };
        assert_eq!(
            colors(&ppu.bg_palette_ram[0..8]),
            [0x7FFF, 0x1BEF, 0x6180, 0x0000]
        );
        assert_eq!(
            colors(&ppu.obj_palette_ram[0..8]),
            [0x7FFF, 0x421F, 0x1CF2, 0x0000]
        );
        assert_eq!(
            colors(&ppu.obj_palette_ram[8..16]),
            [0x7FFF, 0x421F, 0x1CF2, 0x0000]
        );
        assert_eq!(ppu.opri, 1);
        assert!(!ppu.cgb_mode());
    }

    #[test]
    fn manual_palettes_ignore_the_title() {
        let rom = header(0x01, b"00", 0xDB, b'R');
        let grey = CompatPalette::Grey.colors(&rom);
        for palette in grey {
            assert_eq!(palette.map(|c| c.r()), [0xFF, 0xA5, 0x52, 0x00]);
        }
        assert_eq!(
            CompatPalette::Auto.colors(&rom)[0],
            CompatPalette::Orange.colors(&rom)[0]
        );
    }

    #[test]
    fn a_chosen_palette_renders_on_dmg_too() {
        let mut ppu = Ppu::new(GbModel::Dmg);
        ppu.set_compat_palette(CompatPalette::Inverted, &header(0x00, b"00", 0, 0));
        assert_eq!(ppu.dmg_colors, DmgColors::PaletteRam);
        assert_eq!(ppu.backdrop_color(), RGBA::rgb(0, 0, 0));
        ppu.soft_reset();
        assert_eq!(ppu.backdrop_color(), RGBA::rgb(0, 0, 0));
    }
}
