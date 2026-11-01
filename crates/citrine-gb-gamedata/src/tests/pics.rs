use crate::SpriteError;
use crate::pic::{decode_gen1, decode_gen2, decompress_lz};

pub(crate) struct Planes {
    pub width: u8,
    pub height: u8,
    pub low: Vec<u8>,
    pub high: Vec<u8>,
}

impl Planes {
    pub fn random(width: u8, height: u8, seed: u32, density: u32) -> Self {
        let len = usize::from(width) * usize::from(height) * 8;
        let mut state = seed.max(1);
        let mut next = move || {
            let mut byte = 0u8;
            for bit in 0..8 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                if state % 100 < density {
                    byte |= 1 << bit;
                }
            }
            byte
        };
        let low = (0..len).map(|_| next()).collect();
        let high = (0..len).map(|_| next()).collect();
        Self {
            width,
            height,
            low,
            high,
        }
    }

    pub fn shade(&self, x: usize, y: usize) -> u8 {
        let rows = usize::from(self.height) * 8;
        let at = (x / 8) * rows + y;
        let bit = 7 - x % 8;
        ((self.low[at] >> bit) & 1) | (((self.high[at] >> bit) & 1) << 1)
    }
}

struct BitWriter {
    bytes: Vec<u8>,
    used: u8,
}

impl BitWriter {
    fn bit(&mut self, bit: u8) {
        if self.used == 0 {
            self.bytes.push(0);
        }
        let last = self.bytes.last_mut().unwrap();
        *last |= (bit & 1) << (7 - self.used);
        self.used = (self.used + 1) % 8;
    }

    fn bits(&mut self, value: usize, count: u32) {
        for i in (0..count).rev() {
            self.bit(((value >> i) & 1) as u8);
        }
    }
}

fn delta_encode(plane: &[u8], rows: usize) -> Vec<u8> {
    let mut out = plane.to_vec();
    for y in 0..rows {
        let mut previous = 0u8;
        for column in 0..plane.len() / rows {
            let byte = plane[column * rows + y];
            let mut encoded = 0u8;
            for bit in (0..8).rev() {
                let value = (byte >> bit) & 1;
                encoded |= (value ^ previous) << bit;
                previous = value;
            }
            out[column * rows + y] = encoded;
        }
    }
    out
}

fn pack(writer: &mut BitWriter, plane: &[u8], rows: usize) {
    let mut pairs = Vec::new();
    for column in 0..plane.len() / rows {
        for pair in 0..4 {
            for y in 0..rows {
                pairs.push((plane[column * rows + y] >> (6 - 2 * pair)) & 3);
            }
        }
    }
    writer.bit(u8::from(pairs[0] != 0));
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i] == 0 {
            let run = pairs[i..].iter().take_while(|&&p| p == 0).count();
            let mut ones = 0;
            while run >= (4 << ones) - 1 {
                ones += 1;
            }
            writer.bits((1 << ones) - 1, ones as u32);
            writer.bit(0);
            writer.bits(run - ((2 << ones) - 1), ones as u32 + 1);
            i += run;
        } else {
            while i < pairs.len() && pairs[i] != 0 {
                writer.bits(usize::from(pairs[i]), 2);
                i += 1;
            }
            if i < pairs.len() {
                writer.bits(0, 2);
            }
        }
    }
}

pub(crate) fn encode_gen1(planes: &Planes, mode: u8, first: usize) -> Vec<u8> {
    let rows = usize::from(planes.height) * 8;
    let both = [&planes.low, &planes.high];
    let (a, b) = (both[first], both[first ^ 1]);
    let a_xor_b: Vec<u8> = a.iter().zip(b.iter()).map(|(x, y)| x ^ y).collect();
    let encoded_a = delta_encode(a, rows);
    let encoded_b = match mode {
        0 => delta_encode(b, rows),
        1 => a_xor_b,
        _ => delta_encode(&a_xor_b, rows),
    };
    let mut writer = BitWriter {
        bytes: vec![(planes.width << 4) | planes.height],
        used: 0,
    };
    writer.bit(first as u8);
    pack(&mut writer, &encoded_a, rows);
    match mode {
        0 => writer.bit(0),
        1 => writer.bits(0b10, 2),
        _ => writer.bits(0b11, 2),
    }
    pack(&mut writer, &encoded_b, rows);
    writer.bytes
}

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
        0x02, 0x10, 0x20, 0x30, 0x22, 0x44, 0x43, 0x01, 0x02, 0x61, 0x81, 0x83, 0xA0, 0x00, 0x00,
        0xC1, 0x80, 0xED, 0x2B, 0xF0, 0x01, 0x00, 0x01, 0xFF, 0x99,
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
