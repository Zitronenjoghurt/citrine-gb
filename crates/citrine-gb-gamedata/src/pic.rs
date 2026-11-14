#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub width: u8,
    pub height: u8,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SpriteError {
    #[error("the ROM's picture tables were not found")]
    NoTable,
    #[error("the game has no picture for this species")]
    NoSpecies,
    #[error("the picture reads past the end of the ROM")]
    OutOfRange,
    #[error("the picture data is malformed")]
    BadData,
}

impl Sprite {
    pub fn width_px(&self) -> usize {
        usize::from(self.width) * 8
    }

    pub fn height_px(&self) -> usize {
        usize::from(self.height) * 8
    }

    pub fn pixel(&self, x: usize, y: usize) -> u8 {
        self.pixels[y * self.width_px() + x]
    }

    pub(crate) fn from_tiles(width: u8, height: u8, tiles: &[u8]) -> Self {
        let w = usize::from(width) * 8;
        let h = usize::from(height) * 8;
        let mut pixels = vec![0; w * h];
        for (index, tile) in tiles.as_chunks::<16>().0.iter().enumerate() {
            let column = index / usize::from(height);
            let row = index % usize::from(height);
            for y in 0..8 {
                let (low, high) = (tile[2 * y], tile[2 * y + 1]);
                for x in 0..8 {
                    let bit = 7 - x;
                    let shade = ((low >> bit) & 1) | (((high >> bit) & 1) << 1);
                    pixels[(row * 8 + y) * w + column * 8 + x] = shade;
                }
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }
}

struct Bits<'a> {
    rom: &'a [u8],
    at: usize,
    current: u8,
    left: u8,
}

impl Bits<'_> {
    fn byte(&mut self) -> Result<u8, SpriteError> {
        let byte = *self.rom.get(self.at).ok_or(SpriteError::OutOfRange)?;
        self.at += 1;
        Ok(byte)
    }

    fn bit(&mut self) -> Result<u8, SpriteError> {
        if self.left == 0 {
            self.current = self.byte()?;
            self.left = 8;
        }
        self.left -= 1;
        Ok((self.current >> self.left) & 1)
    }
}

pub(crate) fn decode_gen1(rom: &[u8], at: usize) -> Result<Sprite, SpriteError> {
    let mut bits = Bits {
        rom,
        at,
        current: 0,
        left: 0,
    };
    let size = bits.byte()?;
    let (width, height) = (size >> 4, size & 0x0F);
    if !(1..=7).contains(&width) || !(1..=7).contains(&height) {
        return Err(SpriteError::BadData);
    }
    let rows = usize::from(height) * 8;
    let columns = usize::from(width);
    let mut planes = [vec![0u8; rows * columns], vec![0u8; rows * columns]];
    let first = usize::from(bits.bit()?);
    fill_plane(&mut bits, &mut planes[first], rows)?;
    let mode = match bits.bit()? {
        0 => 0,
        _ => 1 + bits.bit()?,
    };
    fill_plane(&mut bits, &mut planes[first ^ 1], rows)?;
    let [a, b] = if first == 0 {
        let [low, high] = &mut planes;
        [low, high]
    } else {
        let [low, high] = &mut planes;
        [high, low]
    };
    match mode {
        0 => {
            delta_decode(a, rows);
            delta_decode(b, rows);
        }
        1 => {
            delta_decode(a, rows);
            xor_into(b, a);
        }
        _ => {
            delta_decode(b, rows);
            delta_decode(a, rows);
            xor_into(b, a);
        }
    }
    let mut tiles = Vec::with_capacity(rows * columns * 2);
    for column in 0..columns {
        for y in 0..rows {
            tiles.push(planes[0][column * rows + y]);
            tiles.push(planes[1][column * rows + y]);
        }
    }
    Ok(Sprite::from_tiles(width, height, &tiles))
}

fn fill_plane(bits: &mut Bits, plane: &mut [u8], rows: usize) -> Result<(), SpriteError> {
    let total = plane.len() * 4;
    let mut pos = 0;
    let mut zeros = bits.bit()? == 0;
    while pos < total {
        if zeros {
            let mut ones = 0;
            while bits.bit()? == 1 {
                ones += 1;
                if ones >= 15 {
                    return Err(SpriteError::BadData);
                }
            }
            let mut value = 0usize;
            for _ in 0..=ones {
                value = (value << 1) | usize::from(bits.bit()?);
            }
            let count = value + (2 << ones) - 1;
            pos = (pos + count).min(total);
            zeros = false;
        } else {
            let pair = (bits.bit()? << 1) | bits.bit()?;
            if pair == 0 {
                zeros = true;
                continue;
            }
            let column = pos / (4 * rows);
            let rest = pos % (4 * rows);
            let shift = 6 - 2 * (rest / rows);
            plane[column * rows + rest % rows] |= pair << shift;
            pos += 1;
        }
    }
    Ok(())
}

fn delta_decode(plane: &mut [u8], rows: usize) {
    for y in 0..rows {
        let mut state = 0u8;
        for column in 0..plane.len() / rows {
            let byte = &mut plane[column * rows + y];
            let mut out = 0u8;
            for bit in (0..8).rev() {
                state ^= (*byte >> bit) & 1;
                out |= state << bit;
            }
            *byte = out;
        }
    }
}

fn xor_into(target: &mut [u8], source: &[u8]) {
    target
        .iter_mut()
        .zip(source)
        .for_each(|(target, source)| *target ^= source);
}

pub(crate) fn decode_gen2(
    rom: &[u8],
    at: usize,
    width: u8,
    height: u8,
) -> Result<Sprite, SpriteError> {
    let need = usize::from(width) * usize::from(height) * 16;
    let data = decompress_lz(rom, at, need)?;
    if data.len() < need {
        return Err(SpriteError::BadData);
    }
    Ok(Sprite::from_tiles(width, height, &data[..need]))
}

pub(crate) fn decompress_lz(
    rom: &[u8],
    mut at: usize,
    limit: usize,
) -> Result<Vec<u8>, SpriteError> {
    let read = |at: usize| rom.get(at).copied().ok_or(SpriteError::OutOfRange);
    let mut out: Vec<u8> = Vec::with_capacity(limit.min(0x4000));
    while out.len() < limit {
        let head = read(at)?;
        if head == 0xFF {
            break;
        }
        at += 1;
        let (command, count) = if head >> 5 == 7 {
            let low = read(at)?;
            at += 1;
            (
                (head >> 2) & 7,
                ((usize::from(head & 3) << 8) | usize::from(low)) + 1,
            )
        } else {
            (head >> 5, usize::from(head & 0x1F) + 1)
        };
        match command {
            0 => {
                let bytes = rom.get(at..at + count).ok_or(SpriteError::OutOfRange)?;
                out.extend_from_slice(bytes);
                at += count;
            }
            1 => {
                let byte = read(at)?;
                out.extend(std::iter::repeat_n(byte, count));
                at += 1;
            }
            2 => {
                let pair = [read(at)?, read(at + 1)?];
                out.extend((0..count).map(|i| pair[i & 1]));
                at += 2;
            }
            3 => out.extend(std::iter::repeat_n(0, count)),
            _ => {
                let offset = read(at)?;
                let start = if offset & 0x80 != 0 {
                    at += 1;
                    out.len()
                        .checked_sub(usize::from(offset & 0x7F) + 1)
                        .ok_or(SpriteError::BadData)?
                } else {
                    let low = read(at + 1)?;
                    at += 2;
                    (usize::from(offset) << 8) | usize::from(low)
                };
                for i in 0..count {
                    let source = match command {
                        6 => start.checked_sub(i),
                        _ => Some(start + i),
                    };
                    let byte = *source
                        .and_then(|source| out.get(source))
                        .ok_or(SpriteError::BadData)?;
                    out.push(if command == 5 {
                        byte.reverse_bits()
                    } else {
                        byte
                    });
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::SpriteError;
    use crate::pic::{decode_gen1, decode_gen2, decompress_lz};
    use crate::test_support::{Planes, encode_gen1};

    fn assert_decodes(planes: &Planes, mode: u8, first: usize) {
        let data = encode_gen1(planes, mode, first);
        let mut rom = vec![0xAA; 3];
        rom.extend(&data);
        let sprite = decode_gen1(&rom, 3).expect("decodes");
        assert_eq!((sprite.width, sprite.height), (planes.width, planes.height));
        for y in 0..sprite.height_px() {
            for x in 0..sprite.width_px() {
                assert_eq!(
                    sprite.pixel(x, y),
                    planes.shade(x, y),
                    "mode {mode} first {first} at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn gen1_pics_decode_in_every_mode_and_size() {
        let mut seed = 7;
        for (width, height) in [(1, 1), (4, 4), (5, 5), (6, 6), (7, 7), (3, 6), (7, 2)] {
            for density in [0, 3, 30, 50, 97, 100] {
                for mode in 0..3 {
                    for first in 0..2 {
                        seed += 1;
                        let planes = Planes::random(width, height, seed, density);
                        assert_decodes(&planes, mode, first);
                    }
                }
            }
        }
    }

    #[test]
    fn gen1_pic_shades_come_from_both_planes() {
        let mut planes = Planes::random(1, 1, 1, 0);
        planes.low[0] = 0b1010_0000;
        planes.high[0] = 0b1100_0000;
        planes.high[7] = 0b0000_0001;
        let rom = encode_gen1(&planes, 0, 0);
        let sprite = decode_gen1(&rom, 0).expect("decodes");
        assert_eq!(&sprite.pixels[..4], &[3, 2, 1, 0]);
        assert_eq!(sprite.pixel(7, 7), 2);
        assert_eq!(sprite.pixels.iter().filter(|&&p| p != 0).count(), 4);
    }

    #[test]
    fn gen1_pics_refuse_bad_sizes_and_short_data() {
        assert_eq!(decode_gen1(&[0x08, 0xFF], 0), Err(SpriteError::BadData));
        assert_eq!(decode_gen1(&[0x80, 0xFF], 0), Err(SpriteError::BadData));
        assert_eq!(decode_gen1(&[0x00], 0), Err(SpriteError::BadData));
        let planes = Planes::random(5, 5, 99, 50);
        let data = encode_gen1(&planes, 2, 1);
        assert_eq!(
            decode_gen1(&data[..data.len() / 2], 0),
            Err(SpriteError::OutOfRange)
        );
        assert_eq!(decode_gen1(&data, data.len()), Err(SpriteError::OutOfRange));
        assert_eq!(
            decode_gen1(&[0x11, 0x3F, 0xFF, 0xFF], 0),
            Err(SpriteError::BadData)
        );
    }

    #[test]
    fn lz_runs_every_command() {
        let stream = [
            0x02, 0x10, 0x20, 0x30, 0x22, 0x44, 0x43, 0x01, 0x02, 0x61, 0x81, 0x83, 0xA0, 0x00,
            0x00, 0xC1, 0x80, 0xED, 0x2B, 0xF0, 0x01, 0x00, 0x01, 0xFF, 0x99,
        ];
        let out = decompress_lz(&stream, 0, usize::MAX).expect("decompresses");
        let mut want = vec![
            0x10, 0x20, 0x30, 0x44, 0x44, 0x44, 0x01, 0x02, 0x01, 0x02, 0x00, 0x00,
        ];
        want.extend([0x01, 0x02]);
        want.push(0x08);
        want.extend([0x08, 0x02]);
        want.extend(vec![0; 300]);
        want.extend([0x20, 0x30]);
        assert_eq!(out.len(), want.len());
        assert_eq!(out, want);
    }

    #[test]
    fn lz_stops_at_the_limit_and_checks_its_reads() {
        let stream = [0xE4, 0xFF, 0x11, 0xFF];
        assert_eq!(decompress_lz(&stream, 0, 16).expect("ok").len(), 256);
        assert_eq!(
            decompress_lz(&[0x05, 0x01], 0, 64),
            Err(SpriteError::OutOfRange)
        );
        assert_eq!(decompress_lz(&[0x00], 0, 64), Err(SpriteError::OutOfRange));
        assert_eq!(
            decompress_lz(&[0x81, 0x80, 0xFF], 0, 64),
            Err(SpriteError::BadData)
        );
        assert_eq!(
            decompress_lz(&[0x00, 0x01, 0x81, 0x00, 0x05, 0xFF], 0, 64),
            Err(SpriteError::BadData)
        );
        assert_eq!(
            decompress_lz(&[0x00, 0x01, 0xC1, 0x80, 0xFF], 0, 64),
            Err(SpriteError::BadData)
        );
    }

    #[test]
    fn gen2_pics_lay_tiles_out_by_column() {
        let mut tiles = Vec::new();
        for tile in 0..4u8 {
            for row in 0..8u8 {
                tiles.push(if row == tile { 0xFF } else { 0 });
                tiles.push(if tile % 2 == 1 { 0x80 } else { 0 });
            }
        }
        let mut stream = vec![0xE0, 0x3F];
        stream.extend(&tiles[..64]);
        stream.push(0xFF);
        let sprite = decode_gen2(&stream, 0, 2, 2).expect("decodes");
        assert_eq!((sprite.width_px(), sprite.height_px()), (16, 16));
        assert_eq!(sprite.pixel(3, 0), 1);
        assert_eq!(sprite.pixel(0, 9), 3);
        assert_eq!(sprite.pixel(1, 9), 1);
        assert_eq!(sprite.pixel(0, 10), 2);
        assert_eq!(sprite.pixel(12, 2), 1);
        assert_eq!(sprite.pixel(8, 3), 0);
        assert_eq!(sprite.pixel(8, 11), 3);
        assert_eq!(sprite.pixel(9, 11), 1);
        assert_eq!(sprite.pixel(8, 12), 2);
        assert_eq!(decode_gen2(&stream, 0, 3, 3), Err(SpriteError::BadData));
    }
}
