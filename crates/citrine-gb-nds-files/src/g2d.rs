use super::{RomError, slice, u16_at, u32_at};
use crate::texture::rgb555;

const TILE: usize = 8;
const MAX_SIDE: usize = 4096;

fn chunk<'a>(file: &'a [u8], tag: &'static str) -> Result<&'a [u8], RomError> {
    let mut at = usize::from(u16_at(file, 0x0C)?);
    let count = u16_at(file, 0x0E)?;
    for _ in 0..count {
        let name = slice(file, at, at + 4)?;
        let len = (u32_at(file, at + 4)? as usize).max(8);
        if name == tag.as_bytes() {
            return Ok(file.get(at..at + len).unwrap_or(&file[at..]));
        }
        at += len;
    }
    Err(RomError::MissingSection(tag))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Bpp4,
    Bpp8,
}

impl Depth {
    fn from_bits(bits: u32) -> Self {
        if bits == 3 { Self::Bpp4 } else { Self::Bpp8 }
    }

    pub fn colors(self) -> usize {
        match self {
            Self::Bpp4 => 16,
            Self::Bpp8 => 256,
        }
    }

    fn tile_bytes(self) -> usize {
        match self {
            Self::Bpp4 => 32,
            Self::Bpp8 => 64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Nclr<'a> {
    pub depth: Depth,
    pub colors: &'a [u8],
}

impl<'a> Nclr<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self, RomError> {
        let pltt = chunk(file, "TTLP")?;
        let depth = Depth::from_bits(u32_at(pltt, 0x08)?);
        let len = u32_at(pltt, 0x10)? as usize;
        let start = 8 + u32_at(pltt, 0x14)? as usize;
        let colors = pltt
            .get(start..(start + len).min(pltt.len()))
            .ok_or(RomError::Truncated(start))?;
        Ok(Self { depth, colors })
    }

    pub fn color_count(&self) -> usize {
        self.colors.len() / 2
    }

    pub fn rows(&self) -> usize {
        self.color_count().div_ceil(16).max(1)
    }

    pub fn rgba(&self, index: usize) -> [u8; 4] {
        let at = index * 2;
        match self.colors.get(at..at + 2) {
            Some(raw) => {
                let [r, g, b] = rgb555(u16::from_le_bytes([raw[0], raw[1]]));
                [r, g, b, 255]
            }
            None => [0, 0, 0, 255],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ncgr<'a> {
    pub depth: Depth,
    pub width_tiles: Option<usize>,
    pub tiled: bool,
    pub data: &'a [u8],
}

pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl<'a> Ncgr<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self, RomError> {
        let rahc = chunk(file, "RAHC")?;
        let width = u16_at(rahc, 0x0A)?;
        let depth = Depth::from_bits(u32_at(rahc, 0x0C)?);
        let tiled = u32_at(rahc, 0x14)? & 0xFF == 0;
        let len = u32_at(rahc, 0x18)? as usize;
        let start = 8 + u32_at(rahc, 0x1C)? as usize;
        let data = rahc
            .get(start..(start + len).min(rahc.len()))
            .ok_or(RomError::Truncated(start))?;
        Ok(Self {
            depth,
            width_tiles: (width != 0xFFFF && width != 0).then_some(usize::from(width)),
            tiled,
            data,
        })
    }

    pub fn tile_count(&self) -> usize {
        self.data.len() / self.depth.tile_bytes()
    }

    pub fn default_width(&self) -> usize {
        self.width_tiles
            .unwrap_or_else(|| self.tile_count().clamp(1, 32))
            .max(1)
    }

    fn index(&self, tile: usize, x: usize, y: usize) -> Option<usize> {
        let base = tile * self.depth.tile_bytes();
        match self.depth {
            Depth::Bpp4 => {
                let byte = *self.data.get(base + (y * TILE + x) / 2)?;
                Some(usize::from(if x.is_multiple_of(2) {
                    byte & 15
                } else {
                    byte >> 4
                }))
            }
            Depth::Bpp8 => self.data.get(base + y * TILE + x).map(|&b| usize::from(b)),
        }
    }

    fn linear_index(&self, width: usize, x: usize, y: usize) -> Option<usize> {
        let at = y * width + x;
        match self.depth {
            Depth::Bpp4 => {
                let byte = *self.data.get(at / 2)?;
                Some(usize::from(if at.is_multiple_of(2) {
                    byte & 15
                } else {
                    byte >> 4
                }))
            }
            Depth::Bpp8 => self.data.get(at).map(|&b| usize::from(b)),
        }
    }

    pub fn decode(&self, palette: &Nclr, row: usize, width_tiles: usize) -> Picture {
        let width_tiles = width_tiles.max(1);
        let height_tiles = self.tile_count().div_ceil(width_tiles).max(1);
        let (width, height) = (width_tiles * TILE, height_tiles * TILE);
        let mut rgba = vec![0u8; width * height * 4];
        let base = match self.depth {
            Depth::Bpp4 => row * 16,
            Depth::Bpp8 => 0,
        };
        for y in 0..height {
            for x in 0..width {
                let index = if self.tiled {
                    let tile = (y / TILE) * width_tiles + x / TILE;
                    self.index(tile, x % TILE, y % TILE)
                } else {
                    self.linear_index(width, x, y)
                };
                if let Some(index @ 1..) = index {
                    let at = (y * width + x) * 4;
                    rgba[at..at + 4].copy_from_slice(&palette.rgba(base + index));
                }
            }
        }
        Picture {
            width,
            height,
            rgba,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Nscr<'a> {
    pub width: usize,
    pub height: usize,
    entries: &'a [u8],
}

impl<'a> Nscr<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self, RomError> {
        let scrn = chunk(file, "NRCS")?;
        let width = usize::from(u16_at(scrn, 0x08)?);
        let height = usize::from(u16_at(scrn, 0x0A)?);
        let len = u32_at(scrn, 0x10)? as usize;
        let entries = scrn
            .get(0x14..(0x14 + len).min(scrn.len()))
            .ok_or(RomError::Truncated(0x14))?;
        if width > MAX_SIDE || height > MAX_SIDE {
            return Err(RomError::TooLarge(width, height));
        }
        let needed = (width / TILE) * (height / TILE) * 2;
        if entries.len() < needed {
            return Err(RomError::Truncated(0x14 + needed));
        }
        Ok(Self {
            width,
            height,
            entries,
        })
    }

    pub fn decode(&self, tiles: &Ncgr, palette: &Nclr) -> Picture {
        let columns = self.width / TILE;
        let mut rgba = vec![0u8; self.width * self.height * 4];
        for y in 0..self.height {
            for x in 0..self.width {
                let at = ((y / TILE) * columns + x / TILE) * 2;
                let Some(raw) = self.entries.get(at..at + 2) else {
                    continue;
                };
                let entry = u16::from_le_bytes([raw[0], raw[1]]);
                let tile = usize::from(entry & 0x3FF);
                let mut tx = x % TILE;
                let mut ty = y % TILE;
                if entry & 0x400 != 0 {
                    tx = TILE - 1 - tx;
                }
                if entry & 0x800 != 0 {
                    ty = TILE - 1 - ty;
                }
                let bank = match tiles.depth {
                    Depth::Bpp4 => usize::from(entry >> 12) * 16,
                    Depth::Bpp8 => 0,
                };
                if let Some(index @ 1..) = tiles.index(tile, tx, ty) {
                    let out = (y * self.width + x) * 4;
                    rgba[out..out + 4].copy_from_slice(&palette.rgba(bank + index));
                }
            }
        }
        Picture {
            width: self.width,
            height: self.height,
            rgba,
        }
    }
}

#[cfg(any(test, feature = "fixtures"))]
fn wrap(magic: &[u8; 4], tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let chunk_len = (body.len() + 8) as u32;
    let mut file = magic.to_vec();
    file.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]);
    file.extend_from_slice(&(chunk_len + 0x10).to_le_bytes());
    file.extend_from_slice(&[0x10, 0x00, 0x01, 0x00]);
    file.extend_from_slice(tag);
    file.extend_from_slice(&chunk_len.to_le_bytes());
    file.extend_from_slice(body);
    file
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_nclr(bpp4: bool, colors: &[u16]) -> Vec<u8> {
    let raw: Vec<u8> = colors.iter().flat_map(|c| c.to_le_bytes()).collect();
    let mut body = Vec::new();
    body.extend_from_slice(&(if bpp4 { 3u32 } else { 4 }).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    body.extend_from_slice(&0x10u32.to_le_bytes());
    body.extend(raw);
    wrap(b"RLCN", b"TTLP", &body)
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_ncgr(bpp4: bool, width_tiles: u16, data: &[u8]) -> Vec<u8> {
    let tile = if bpp4 { 32 } else { 64 };
    let height = (data.len() / tile).div_ceil(usize::from(width_tiles.max(1))) as u16;
    let mut body = Vec::new();
    body.extend_from_slice(&height.to_le_bytes());
    body.extend_from_slice(&width_tiles.to_le_bytes());
    body.extend_from_slice(&(if bpp4 { 3u32 } else { 4 }).to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(&0x18u32.to_le_bytes());
    body.extend_from_slice(data);
    wrap(b"RGCN", b"RAHC", &body)
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_nscr(width: u16, height: u16, entries: &[u16]) -> Vec<u8> {
    let raw: Vec<u8> = entries.iter().flat_map(|e| e.to_le_bytes()).collect();
    let mut body = Vec::new();
    body.extend_from_slice(&width.to_le_bytes());
    body.extend_from_slice(&height.to_le_bytes());
    body.extend_from_slice(&[0, 0, 0, 0]);
    body.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    body.extend(raw);
    wrap(b"RCSN", b"NRCS", &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;

    fn two_rows() -> Vec<u8> {
        let mut colors = vec![0u16; 32];
        colors[1] = RED;
        colors[17] = GREEN;
        build_nclr(true, &colors)
    }

    fn tiles() -> Vec<u8> {
        let mut data = vec![0x11u8; 32];
        data[0] = 0x10;
        data.extend([0u8; 32]);
        build_ncgr(true, 2, &data)
    }

    #[test]
    fn reads_palette_rows_and_colours() {
        let file = two_rows();
        let nclr = Nclr::parse(&file).unwrap();
        assert_eq!(nclr.depth, Depth::Bpp4);
        assert_eq!(nclr.rows(), 2);
        assert_eq!(nclr.rgba(1), [255, 0, 0, 255]);
        assert_eq!(nclr.rgba(17), [0, 255, 0, 255]);
    }

    #[test]
    fn decodes_tiles_through_the_chosen_palette_row() {
        let palette_file = two_rows();
        let palette = Nclr::parse(&palette_file).unwrap();
        let tile_file = tiles();
        let ncgr = Ncgr::parse(&tile_file).unwrap();
        assert_eq!(ncgr.width_tiles, Some(2));
        assert_eq!(ncgr.tile_count(), 2);
        let first = ncgr.decode(&palette, 0, 2);
        assert_eq!((first.width, first.height), (16, 8));
        assert_eq!(&first.rgba[..4], &[0, 0, 0, 0]);
        assert_eq!(&first.rgba[4..8], &[255, 0, 0, 255]);
        let second = ncgr.decode(&palette, 1, 2);
        assert_eq!(&second.rgba[4..8], &[0, 255, 0, 255]);
    }

    #[test]
    fn a_narrower_width_stacks_the_tiles() {
        let palette_file = two_rows();
        let palette = Nclr::parse(&palette_file).unwrap();
        let tile_file = tiles();
        let picture = Ncgr::parse(&tile_file).unwrap().decode(&palette, 0, 1);
        assert_eq!((picture.width, picture.height), (8, 16));
    }

    #[test]
    fn screens_flip_tiles_and_pick_palette_banks() {
        let mut colors = vec![0u16; 32];
        colors[1] = RED;
        colors[17] = BLUE;
        let palette_file = build_nclr(true, &colors);
        let palette = Nclr::parse(&palette_file).unwrap();
        let mut data = vec![0u8; 32];
        data[0] = 0x01;
        let tile_file = build_ncgr(true, 1, &data);
        let ncgr = Ncgr::parse(&tile_file).unwrap();
        let screen_file = build_nscr(16, 8, &[0x0000, 0x1400]);
        let screen = Nscr::parse(&screen_file).unwrap();
        let picture = screen.decode(&ncgr, &palette);
        assert_eq!((picture.width, picture.height), (16, 8));
        assert_eq!(&picture.rgba[..4], &[255, 0, 0, 255]);
        let flipped = 15 * 4;
        assert_eq!(&picture.rgba[flipped..flipped + 4], &[0, 0, 255, 255]);
        assert_eq!(&picture.rgba[8 * 4..8 * 4 + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn a_huge_screen_is_an_error() {
        let file = build_nscr(0xFFFF, 0xFFFF, &[0; 4]);
        assert_eq!(
            Nscr::parse(&file).unwrap_err(),
            RomError::TooLarge(0xFFFF, 0xFFFF)
        );
    }

    #[test]
    fn a_screen_bigger_than_its_entries_is_an_error() {
        let file = build_nscr(256, 192, &[0; 4]);
        assert_eq!(
            Nscr::parse(&file).unwrap_err(),
            RomError::Truncated(0x14 + 32 * 24 * 2)
        );
        let full = build_nscr(256, 192, &[0; 32 * 24]);
        assert_eq!(Nscr::parse(&full).unwrap().width, 256);
    }

    #[test]
    fn a_wrong_file_is_an_error() {
        assert!(Nclr::parse(&tiles()).is_err());
        assert!(Ncgr::parse(b"RGCN").is_err());
    }
}
