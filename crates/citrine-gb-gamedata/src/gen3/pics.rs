const LZ77: u8 = 0x10;
const TILE: usize = 8;
const TILE_BYTES: usize = 32;
pub const SIDE: usize = 64;
const COLORS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub pixels: Vec<u8>,
    pub palette: [[u8; 3]; COLORS],
}

impl Picture {
    pub fn rgba(&self) -> Vec<u8> {
        self.pixels
            .iter()
            .flat_map(|&index| {
                let [r, g, b] = self.palette[usize::from(index)];
                [r, g, b, if index == 0 { 0 } else { 0xFF }]
            })
            .collect()
    }
}

pub(crate) fn lz77(data: &[u8]) -> Option<Vec<u8>> {
    let (&kind, rest) = data.split_first()?;
    if kind != LZ77 || rest.len() < 3 {
        return None;
    }
    let size = usize::from(rest[0]) | usize::from(rest[1]) << 8 | usize::from(rest[2]) << 16;
    let mut out = Vec::with_capacity(size);
    let mut at = 4;
    while out.len() < size {
        let flags = *data.get(at)?;
        at += 1;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if flags >> bit & 1 == 0 {
                out.push(*data.get(at)?);
                at += 1;
                continue;
            }
            let (high, low) = (*data.get(at)?, *data.get(at + 1)?);
            at += 2;
            let length = usize::from(high >> 4) + 3;
            let back = (usize::from(high & 0xF) << 8 | usize::from(low)) + 1;
            let start = out.len().checked_sub(back)?;
            for i in 0..length {
                out.push(out[start + i]);
            }
        }
    }
    out.truncate(size);
    Some(out)
}

pub(crate) fn tiles(data: &[u8]) -> Option<Vec<u8>> {
    let tiles_per_row = SIDE / TILE;
    let data = data.get(..SIDE * SIDE / 2)?;
    let mut pixels = vec![0; SIDE * SIDE];
    for (tile, bytes) in data.as_chunks::<TILE_BYTES>().0.iter().enumerate() {
        let (tile_x, tile_y) = (tile % tiles_per_row * TILE, tile / tiles_per_row * TILE);
        for (i, &byte) in bytes.iter().enumerate() {
            let (x, y) = (tile_x + i % 4 * 2, tile_y + i / 4);
            pixels[y * SIDE + x] = byte & 0xF;
            pixels[y * SIDE + x + 1] = byte >> 4;
        }
    }
    Some(pixels)
}

pub(crate) fn palette(data: &[u8]) -> Option<[[u8; 3]; COLORS]> {
    let colors = data.get(..COLORS * 2)?;
    let mut palette = [[0; 3]; COLORS];
    for (color, pair) in palette.iter_mut().zip(colors.as_chunks::<2>().0) {
        let value = u16::from_le_bytes(*pair);
        let channel = |shift: u16| {
            let five = (value >> shift & 0x1F) as u8;
            five << 3 | five >> 2
        };
        *color = [channel(0), channel(5), channel(10)];
    }
    Some(palette)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz77_copies_literals_and_back_references() {
        let data = [0x10, 7, 0, 0, 0b0001_0000, b'a', b'b', b'c', 0x10, 0x02];
        assert_eq!(lz77(&data).as_deref(), Some(&b"abcabca"[..]));
    }

    #[test]
    fn lz77_refuses_other_kinds_and_short_data() {
        assert_eq!(lz77(&[0x11, 1, 0, 0, 0, 0]), None);
        assert_eq!(lz77(&[0x10, 9, 0, 0, 0, b'a']), None);
        assert_eq!(lz77(&[0x10, 4, 0, 0, 0x80, 0x00, 0x05]), None);
    }

    #[test]
    fn tiles_fill_rows_of_eight_low_nibble_first() {
        let mut data = vec![0u8; SIDE * SIDE / 2];
        data[0] = 0x21;
        data[TILE_BYTES + 4] = 0x0F;
        let pixels = tiles(&data).expect("tiles");
        assert_eq!(&pixels[..2], &[1, 2]);
        assert_eq!(pixels[SIDE + 8], 0xF);
        assert_eq!(tiles(&data[..100]), None);
    }

    #[test]
    fn palettes_widen_five_bit_channels() {
        let mut data = vec![0u8; 32];
        data[2..4].copy_from_slice(&0x7FFFu16.to_le_bytes());
        data[4..6].copy_from_slice(&0x001Fu16.to_le_bytes());
        let palette = palette(&data).expect("palette");
        assert_eq!(palette[1], [0xFF, 0xFF, 0xFF]);
        assert_eq!(palette[2], [0xFF, 0, 0]);
        let picture = Picture {
            pixels: vec![0, 2],
            palette,
        };
        assert_eq!(picture.rgba(), vec![0, 0, 0, 0, 0xFF, 0, 0, 0xFF]);
    }
}
