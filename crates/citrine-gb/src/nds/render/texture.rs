pub const TEXTURE_SIZE: u32 = 0x8_0000;
pub const PALETTE_SIZE: u32 = 0x1_8000;

const OFFSET_MASK: u32 = 0xFFFF;
const OFFSET_SHIFT: u32 = 3;
const REPEAT_S: u32 = 1 << 16;
const REPEAT_T: u32 = 1 << 17;
const FLIP_S: u32 = 1 << 18;
const FLIP_T: u32 = 1 << 19;
const SIZE_S_SHIFT: u32 = 20;
const SIZE_T_SHIFT: u32 = 23;
const SIZE_MASK: u32 = 7;
pub const FORMAT_SHIFT: u32 = 26;
const FORMAT_MASK: u32 = 7;
const COLOR0_TRANSPARENT: u32 = 1 << 29;

const A3I5: u32 = 1;
const FOUR_COLORS: u32 = 2;
const SIXTEEN_COLORS: u32 = 3;
const PALETTE_256: u32 = 4;
pub const COMPRESSED: u32 = 5;
const A5I3: u32 = 6;
const DIRECT: u32 = 7;
const OPAQUE_BIT: u16 = 0x8000;
const DECODED_KEY: u32 = OFFSET_MASK | SIZE_MASK << SIZE_S_SHIFT | SIZE_MASK << SIZE_T_SHIFT;
const DECODED_CAP: usize = 1 << 21;

const SLOT: u32 = 0x2_0000;
const COMPRESSED_INDEX_HIGH: u32 = 0x1_0000;
const OPAQUE: u8 = 31;
const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;

pub trait TextureMemory {
    const DECODED: bool = false;

    fn texture(&self, address: u32) -> u8;
    fn texture_palette(&self, address: u32) -> u8;

    fn texture_bytes(&self) -> Option<(&[u8], &[u8])> {
        None
    }

    fn decoded(&self, _param: u32, _palette: u16) -> Option<&[u16]> {
        None
    }

    fn sampler(&self, param: u32, palette: u16) -> Sampler {
        Sampler::new(param, palette)
    }

    fn texture_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([self.texture(address), self.texture(address.wrapping_add(1))])
    }

    fn texture_palette_u16(&self, address: u32) -> u16 {
        u16::from_le_bytes([
            self.texture_palette(address),
            self.texture_palette(address.wrapping_add(1)),
        ])
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Texel {
    pub color: u16,
    pub alpha: u8,
}

pub fn format(param: u32) -> u32 {
    (param >> FORMAT_SHIFT) & FORMAT_MASK
}

pub fn sample<M: TextureMemory>(memory: &M, param: u32, palette: u16, s: i32, t: i32) -> Texel {
    Sampler::new(param, palette).sample(memory, s, t)
}

#[derive(Debug, Clone, Copy)]
pub struct Sampler {
    format: u32,
    width: i32,
    height: i32,
    repeat: [bool; 2],
    flip: [bool; 2],
    base: u32,
    palette: u32,
    color0: u8,
}

impl Sampler {
    pub fn new(param: u32, palette: u16) -> Self {
        Self {
            format: format(param),
            width: 8 << ((param >> SIZE_S_SHIFT) & SIZE_MASK),
            height: 8 << ((param >> SIZE_T_SHIFT) & SIZE_MASK),
            repeat: [param & REPEAT_S != 0, param & REPEAT_T != 0],
            flip: [param & FLIP_S != 0, param & FLIP_T != 0],
            base: (param & OFFSET_MASK) << OFFSET_SHIFT,
            palette: u32::from(palette),
            color0: if param & COLOR0_TRANSPARENT != 0 {
                0
            } else {
                OPAQUE
            },
        }
    }

    #[inline(always)]
    pub fn sample<M: TextureMemory>(&self, memory: &M, s: i32, t: i32) -> Texel {
        let s = wrap(s >> 4, self.width, self.repeat[0], self.flip[0]) as u32;
        let t = wrap(t >> 4, self.height, self.repeat[1], self.flip[1]) as u32;
        let width = self.width as u32;
        let base = self.base;
        let texel = t * width + s;
        let palette = self.palette;
        let indexed = |index: u32, base: u32| Texel {
            color: memory.texture_palette_u16(base + index * 2),
            alpha: if index == 0 { self.color0 } else { OPAQUE },
        };
        let format = if M::DECODED { DIRECT } else { self.format };
        match format {
            A3I5 => {
                let value = u32::from(memory.texture(base + texel));
                let alpha = (value >> 5) as u8;
                Texel {
                    color: memory.texture_palette_u16((palette << 4) + (value & 0x1F) * 2),
                    alpha: alpha * 4 + alpha / 2,
                }
            }
            FOUR_COLORS => {
                let value = u32::from(memory.texture(base + texel / 4));
                indexed((value >> ((s & 3) * 2)) & 3, palette << 3)
            }
            SIXTEEN_COLORS => {
                let value = u32::from(memory.texture(base + texel / 2));
                indexed((value >> ((s & 1) * 4)) & 0xF, palette << 4)
            }
            PALETTE_256 => indexed(u32::from(memory.texture(base + texel)), palette << 4),
            COMPRESSED => compressed(memory, base, width, s, t, palette << 4),
            A5I3 => {
                let value = u32::from(memory.texture(base + texel));
                Texel {
                    color: memory.texture_palette_u16((palette << 4) + (value & 7) * 2),
                    alpha: (value >> 3) as u8,
                }
            }
            DIRECT => {
                let color = memory.texture_u16(base + texel * 2);
                Texel {
                    color,
                    alpha: if color & 0x8000 != 0 { OPAQUE } else { 0 },
                }
            }
            _ => Texel {
                color: 0,
                alpha: OPAQUE,
            },
        }
    }
}

#[inline(always)]
fn wrap(coordinate: i32, size: i32, repeat: bool, flip: bool) -> i32 {
    if !repeat {
        coordinate.clamp(0, size - 1)
    } else if flip && coordinate & size != 0 {
        (size - 1) - (coordinate & (size - 1))
    } else {
        coordinate & (size - 1)
    }
}

fn compressed<M: TextureMemory>(
    memory: &M,
    base: u32,
    width: u32,
    s: u32,
    t: u32,
    palette: u32,
) -> Texel {
    let address = (base + (t & !3) * (width / 4) + (s & !3) + (t & 3)) & (TEXTURE_SIZE - 1);
    let mut index_address = SLOT + ((address & (SLOT - 1) & !3) >> 1);
    if address >= 2 * SLOT {
        index_address += COMPRESSED_INDEX_HIGH;
    }
    let code = if (SLOT..2 * SLOT).contains(&address) {
        0
    } else {
        (memory.texture(address) >> ((s & 3) * 2)) & 3
    };
    let info = memory.texture_u16(index_address);
    let base = palette + u32::from(info & 0x3FFF) * 4;
    let color = |index: u32| memory.texture_palette_u16(base + index * 2);
    let mode = info >> 14;
    let solid = |color: u16| Texel {
        color,
        alpha: OPAQUE,
    };
    match (code, mode) {
        (0, _) => solid(color(0)),
        (1, _) => solid(color(1)),
        (2, 1) => solid(mix(color(0), color(1), 1, 1)),
        (2, 3) => solid(mix(color(0), color(1), 5, 3)),
        (2, _) => solid(color(2)),
        (3, 2) => solid(color(3)),
        (3, 3) => solid(mix(color(0), color(1), 3, 5)),
        _ => Texel::default(),
    }
}

pub struct DecodedTexture<'a> {
    texels: &'a [u16],
}

impl<'a> DecodedTexture<'a> {
    pub fn new(texels: &'a [u16]) -> Self {
        Self { texels }
    }
}

impl TextureMemory for DecodedTexture<'_> {
    const DECODED: bool = true;

    fn texture(&self, _address: u32) -> u8 {
        0
    }

    fn texture_palette(&self, _address: u32) -> u8 {
        0
    }

    fn sampler(&self, param: u32, palette: u16) -> Sampler {
        let direct = param & !(FORMAT_MASK << FORMAT_SHIFT | OFFSET_MASK) | DIRECT << FORMAT_SHIFT;
        Sampler::new(direct, palette)
    }

    #[inline(always)]
    fn texture_u16(&self, address: u32) -> u16 {
        self.texels
            .get((address >> 1) as usize)
            .copied()
            .unwrap_or(0)
    }
}

#[derive(Debug, Default)]
pub struct Decoded {
    entries: Vec<(u64, usize)>,
    texels: Vec<u16>,
}

impl Clone for Decoded {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl Decoded {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.texels.clear();
    }

    pub fn prepare<M: TextureMemory>(&mut self, memory: &M, param: u32, palette: u16) {
        if format(param) != COMPRESSED {
            return;
        }
        let key = decoded_key(param, palette);
        let Err(at) = self.entries.binary_search_by_key(&key, |&(key, _)| key) else {
            return;
        };
        let start = self.texels.len();
        let size = (8usize << ((param >> SIZE_S_SHIFT) & SIZE_MASK))
            * (8usize << ((param >> SIZE_T_SHIFT) & SIZE_MASK));
        if start + size > DECODED_CAP {
            return;
        }
        self.texels.resize(start + size, 0);
        decode_compressed(memory, param, palette, &mut self.texels[start..]);
        self.entries.insert(at, (key, start));
    }

    pub fn get(&self, param: u32, palette: u16) -> Option<&[u16]> {
        let key = decoded_key(param, palette);
        let at = self
            .entries
            .binary_search_by_key(&key, |&(key, _)| key)
            .ok()?;
        let start = self.entries[at].1;
        let size = (8usize << ((param >> SIZE_S_SHIFT) & SIZE_MASK))
            * (8usize << ((param >> SIZE_T_SHIFT) & SIZE_MASK));
        self.texels.get(start..start + size)
    }
}

fn decoded_key(param: u32, palette: u16) -> u64 {
    u64::from(param & DECODED_KEY) | u64::from(palette) << 32
}

fn decode_compressed<M: TextureMemory>(memory: &M, param: u32, palette: u16, out: &mut [u16]) {
    let width = 8u32 << ((param >> SIZE_S_SHIFT) & SIZE_MASK);
    let height = 8u32 << ((param >> SIZE_T_SHIFT) & SIZE_MASK);
    let base = (param & OFFSET_MASK) << OFFSET_SHIFT;
    let palette = u32::from(palette) << 4;
    let opaque = |color: u16| color & !OPAQUE_BIT | OPAQUE_BIT;
    for top in (0..height).step_by(4) {
        for left in (0..width).step_by(4) {
            let address = (base + top * (width / 4) + left) & (TEXTURE_SIZE - 1);
            let mut index_address = SLOT + ((address & (SLOT - 1) & !3) >> 1);
            if address >= 2 * SLOT {
                index_address += COMPRESSED_INDEX_HIGH;
            }
            let info = memory.texture_u16(index_address);
            let colors = palette + u32::from(info & 0x3FFF) * 4;
            let color = |index: u32| memory.texture_palette_u16(colors + index * 2);
            let (first, second) = (color(0), color(1));
            let table = [
                opaque(first),
                opaque(second),
                match info >> 14 {
                    1 => opaque(mix(first, second, 1, 1)),
                    3 => opaque(mix(first, second, 5, 3)),
                    _ => opaque(color(2)),
                },
                match info >> 14 {
                    2 => opaque(color(3)),
                    3 => opaque(mix(first, second, 3, 5)),
                    _ => 0,
                },
            ];
            let skipped = (SLOT..2 * SLOT).contains(&address);
            for row in 0..4 {
                let codes = if skipped {
                    0
                } else {
                    memory.texture(address + row)
                };
                let at = ((top + row) * width + left) as usize;
                for (column, texel) in out[at..at + 4].iter_mut().enumerate() {
                    *texel = table[usize::from((codes >> (column * 2)) & 3)];
                }
            }
        }
    }
}

#[inline]
fn mix(a: u16, b: u16, weight_a: u32, weight_b: u32) -> u16 {
    let total = weight_a + weight_b;
    [RED, GREEN, BLUE].into_iter().fold(0, |out, mask| {
        let channel = (u32::from(a & mask) * weight_a + u32::from(b & mask) * weight_b) / total;
        out | (channel as u16 & mask)
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub struct Memory {
        pub texture: Vec<u8>,
        pub palette: Vec<u8>,
    }

    impl Default for Memory {
        fn default() -> Self {
            Self {
                texture: vec![0; TEXTURE_SIZE as usize],
                palette: vec![0; PALETTE_SIZE as usize],
            }
        }
    }

    impl Memory {
        pub fn set_color(&mut self, address: u32, color: u16) {
            let address = address as usize;
            self.palette[address..address + 2].copy_from_slice(&color.to_le_bytes());
        }

        pub fn set_texture_u16(&mut self, address: u32, value: u16) {
            let address = address as usize;
            self.texture[address..address + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    impl TextureMemory for Memory {
        fn texture(&self, address: u32) -> u8 {
            self.texture[(address % TEXTURE_SIZE) as usize]
        }

        fn texture_palette(&self, address: u32) -> u8 {
            self.palette[(address % PALETTE_SIZE) as usize]
        }
    }

    pub fn param(format: u32, offset: u32, size: u32) -> u32 {
        format << FORMAT_SHIFT | size << SIZE_S_SHIFT | size << SIZE_T_SHIFT | offset >> 3
    }

    fn at(memory: &Memory, param: u32, palette: u16, s: i32, t: i32) -> Texel {
        sample(memory, param, palette, s << 4, t << 4)
    }

    #[test]
    fn a3i5_expands_three_alpha_bits_to_five_per_gbatek() {
        let mut memory = Memory::default();
        memory.texture[0x100] = 0b101_00011;
        memory.set_color(0x20 + 3 * 2, 0x1234);
        let texel = at(&memory, param(A3I5, 0x100, 0), 2, 0, 0);
        assert_eq!(
            texel,
            Texel {
                color: 0x1234,
                alpha: 22
            }
        );
        memory.texture[0x100] = 0b111_00000;
        assert_eq!(at(&memory, param(A3I5, 0x100, 0), 2, 0, 0).alpha, 31);
    }

    #[test]
    fn four_colour_palettes_step_by_eight_bytes_per_gbatek() {
        let mut memory = Memory::default();
        memory.texture[0] = 0b10_01_00_11;
        memory.set_color(8 + 2 * 2, 0x7FFF);
        memory.set_color(8 + 3 * 2, 0x001F);
        let texture = param(FOUR_COLORS, 0, 0);
        assert_eq!(at(&memory, texture, 1, 0, 0).color, 0x001F);
        assert_eq!(at(&memory, texture, 1, 3, 0).color, 0x7FFF);
        assert_eq!(at(&memory, texture, 1, 1, 0).alpha, 31);
    }

    #[test]
    fn colour_zero_is_transparent_only_when_bit_29_says_so_per_gbatek() {
        let memory = Memory::default();
        for format in [FOUR_COLORS, SIXTEEN_COLORS, PALETTE_256] {
            let texture = param(format, 0, 0);
            assert_eq!(at(&memory, texture, 0, 0, 0).alpha, 31);
            assert_eq!(at(&memory, texture | COLOR0_TRANSPARENT, 0, 0, 0).alpha, 0);
        }
    }

    #[test]
    fn sixteen_colour_texels_take_the_low_nibble_first_per_gbatek() {
        let mut memory = Memory::default();
        memory.texture[0] = 0x4A;
        memory.set_color(0x10 * 3 + 0xA * 2, 0x0AAA);
        memory.set_color(0x10 * 3 + 0x4 * 2, 0x0444);
        let texture = param(SIXTEEN_COLORS, 0, 0);
        assert_eq!(at(&memory, texture, 3, 0, 0).color, 0x0AAA);
        assert_eq!(at(&memory, texture, 3, 1, 0).color, 0x0444);
    }

    #[test]
    fn palette_256_rows_are_the_texture_width_apart() {
        let mut memory = Memory::default();
        memory.texture[16 * 2 + 5] = 9;
        memory.set_color(9 * 2, 0x0123);
        assert_eq!(at(&memory, param(PALETTE_256, 0, 1), 0, 5, 2).color, 0x0123);
    }

    #[test]
    fn a5i3_keeps_five_alpha_bits_per_gbatek() {
        let mut memory = Memory::default();
        memory.texture[0] = 17 << 3 | 5;
        memory.set_color(5 * 2, 0x3333);
        let texel = at(&memory, param(A5I3, 0, 0), 0, 0, 0);
        assert_eq!(
            texel,
            Texel {
                color: 0x3333,
                alpha: 17
            }
        );
    }

    #[test]
    fn direct_colour_uses_bit_15_as_alpha_per_gbatek() {
        let mut memory = Memory::default();
        memory.set_texture_u16(0x40, 0x8000 | 0x1F);
        let texture = param(DIRECT, 0, 0);
        assert_eq!(
            at(&memory, texture, 0, 0, 4),
            Texel {
                color: 0x801F,
                alpha: 31
            }
        );
        assert_eq!(at(&memory, texture, 0, 1, 4).alpha, 0);
    }

    #[test]
    fn compressed_blocks_read_their_palette_from_slot_1_per_gbatek() {
        let mut memory = Memory::default();
        let width = 16;
        let block = 4 * (width / 4 + 2);
        memory.texture[block as usize + 1] = 0b11_10_01_00;
        let index = SLOT + block / 2;
        memory.set_texture_u16(index, 0x0002);
        let base = 0x10 + 2 * 4;
        for (color, value) in [0x001F, 0x03E0, 0x7C00, 0x7FFF].into_iter().enumerate() {
            memory.set_color(base + color as u32 * 2, value);
        }
        let texture = param(COMPRESSED, 0, 1);
        let texel = |s, mode: u16| {
            let mut memory_mode = Memory {
                texture: memory.texture.clone(),
                palette: memory.palette.clone(),
            };
            memory_mode.set_texture_u16(index, 0x0002 | mode << 14);
            at(&memory_mode, texture, 1, 8 + s, 4 + 1)
        };
        assert_eq!(texel(0, 0).color, 0x001F);
        assert_eq!(texel(1, 0).color, 0x03E0);
        assert_eq!(texel(2, 0).color, 0x7C00);
        assert_eq!(texel(3, 0).alpha, 0);
        assert_eq!(
            texel(3, 2),
            Texel {
                color: 0x7FFF,
                alpha: 31
            }
        );
    }

    #[test]
    fn compressed_modes_1_and_3_mix_the_first_two_colours_per_gbatek() {
        assert_eq!(mix(0x001F, 0x0000, 1, 1), 0x000F);
        assert_eq!(mix(31 << 5, 0, 5, 3), 19 << 5);
        assert_eq!(mix(0, 31 << 10, 5, 3), 11 << 10);
        assert_eq!(mix(31, 0, 3, 5), 11);
    }

    #[test]
    fn compressed_texels_in_slot_2_use_the_upper_half_of_slot_1_per_gbatek() {
        let mut memory = Memory::default();
        let block = 2 * SLOT + 0x40;
        memory.texture[block as usize] = 0b01;
        memory.set_texture_u16(SLOT + COMPRESSED_INDEX_HIGH + 0x20, 0x0001);
        memory.set_color(4 + 2, 0x4321);
        let texel = at(&memory, param(COMPRESSED, block, 0), 0, 0, 0);
        assert_eq!(texel.color, 0x4321);
    }

    #[test]
    fn compressed_texels_in_slot_1_read_as_code_0_like_melonds() {
        let mut memory = Memory::default();
        memory.texture[SLOT as usize + 2] = 0xFF;
        memory.set_color(0, 0x1111);
        memory.set_color(6, 0x2222);
        let texel = at(&memory, param(COMPRESSED, SLOT, 0), 0, 0, 2);
        assert_eq!(texel.color, 0x1111);
    }

    #[test]
    fn decoded_compressed_textures_sample_like_texel_by_texel_reads() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut memory = Memory::default();
        for byte in memory.texture.iter_mut().chain(memory.palette.iter_mut()) {
            *byte = next() as u8;
        }
        for round in 0..48 {
            let offset = match round % 4 {
                0 => SLOT,
                1 => 2 * SLOT + 0x40,
                2 => TEXTURE_SIZE - 0x40,
                _ => next() as u32 & (TEXTURE_SIZE - 1) & !7,
            };
            let sizes = next() as u32;
            let flags =
                next() as u32 & (REPEAT_S | REPEAT_T | FLIP_S | FLIP_T | COLOR0_TRANSPARENT);
            let texture = COMPRESSED << FORMAT_SHIFT
                | (sizes & 3) << SIZE_S_SHIFT
                | (sizes >> 2 & 3) << SIZE_T_SHIFT
                | flags
                | offset >> 3;
            let palette = next() as u16 & 0x1FFF;
            let mut decoded = Decoded::default();
            decoded.prepare(&memory, texture, palette);
            let texels = decoded.get(texture, palette).expect("a decoded texture");
            let width = 8i32 << (sizes & 3);
            let height = 8i32 << (sizes >> 2 & 3);
            assert_eq!(texels.len(), (width * height) as usize);
            let source = DecodedTexture::new(texels);
            let cached = source.sampler(texture, palette);
            let direct = Sampler::new(texture, palette);
            for t in -height..2 * height {
                for s in -width..2 * width {
                    let want = direct.sample(&memory, s << 4, t << 4);
                    let got = cached.sample(&source, s << 4, t << 4);
                    assert_eq!(
                        Texel {
                            color: got.color & 0x7FFF,
                            ..got
                        },
                        Texel {
                            color: want.color & 0x7FFF,
                            ..want
                        },
                        "texture {texture:08x} palette {palette:04x} at {s},{t}"
                    );
                }
            }
        }
    }

    #[test]
    fn clamping_holds_the_edge_texel_per_gbatek() {
        assert_eq!(wrap(-3, 8, false, false), 0);
        assert_eq!(wrap(11, 8, false, false), 7);
        assert_eq!(wrap(11, 8, false, true), 7);
    }

    #[test]
    fn repeat_wraps_and_flip_mirrors_every_other_copy_per_gbatek() {
        assert_eq!(wrap(11, 8, true, false), 3);
        assert_eq!(wrap(-1, 8, true, false), 7);
        assert_eq!(wrap(11, 8, true, true), 4);
        assert_eq!(wrap(-1, 8, true, true), 0);
        assert_eq!(wrap(17, 8, true, true), 1);
    }

    #[test]
    fn texture_coordinates_drop_their_four_fraction_bits_per_gbatek() {
        let mut memory = Memory::default();
        memory.texture[1] = 1;
        memory.set_color(2, 0x5555);
        let texture = param(PALETTE_256, 0, 0);
        assert_eq!(sample(&memory, texture, 0, 0x1F, 0).color, 0x5555);
        assert_eq!(sample(&memory, texture, 0, 0x0F, 0).color, 0);
    }
}
