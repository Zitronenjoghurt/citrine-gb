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
