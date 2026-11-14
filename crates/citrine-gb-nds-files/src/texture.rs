#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    A3I5,
    Colors4,
    Colors16,
    Colors256,
    Compressed,
    A5I3,
    Direct,
}

impl TextureFormat {
    pub fn from_bits(bits: u32) -> Option<Self> {
        match bits & 7 {
            1 => Some(Self::A3I5),
            2 => Some(Self::Colors4),
            3 => Some(Self::Colors16),
            4 => Some(Self::Colors256),
            5 => Some(Self::Compressed),
            6 => Some(Self::A5I3),
            7 => Some(Self::Direct),
            _ => None,
        }
    }

    pub fn bits(self) -> u32 {
        match self {
            Self::A3I5 => 1,
            Self::Colors4 => 2,
            Self::Colors16 => 3,
            Self::Colors256 => 4,
            Self::Compressed => 5,
            Self::A5I3 => 6,
            Self::Direct => 7,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A3I5 => "A3I5",
            Self::Colors4 => "4 colours",
            Self::Colors16 => "16 colours",
            Self::Colors256 => "256 colours",
            Self::Compressed => "4×4 compressed",
            Self::A5I3 => "A5I3",
            Self::Direct => "Direct colour",
        }
    }

    pub fn bits_per_texel(self) -> usize {
        match self {
            Self::Colors4 | Self::Compressed => 2,
            Self::Colors16 => 4,
            Self::A3I5 | Self::Colors256 | Self::A5I3 => 8,
            Self::Direct => 16,
        }
    }

    pub fn uses_palette(self) -> bool {
        self != Self::Direct
    }

    pub fn palette_colors(self) -> usize {
        match self {
            Self::Colors4 => 4,
            Self::Colors16 => 16,
            Self::A3I5 => 32,
            Self::A5I3 => 8,
            Self::Colors256 => 256,
            Self::Compressed => 4,
            Self::Direct => 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Texels<'a> {
    pub format: TextureFormat,
    pub width: usize,
    pub height: usize,
    pub data: &'a [u8],
    pub indices: &'a [u8],
    pub color0_transparent: bool,
}

impl Texels<'_> {
    pub fn byte_len(&self) -> usize {
        self.width * self.height * self.format.bits_per_texel() / 8
    }

    pub fn decode(&self, palette: &[u8]) -> Vec<u8> {
        let mut rgba = vec![0u8; self.width * self.height * 4];
        for y in 0..self.height {
            for x in 0..self.width {
                let at = (y * self.width + x) * 4;
                rgba[at..at + 4].copy_from_slice(&self.texel(palette, x, y));
            }
        }
        rgba
    }

    fn texel(&self, palette: &[u8], x: usize, y: usize) -> [u8; 4] {
        let index = y * self.width + x;
        let color = |i: usize| rgb555(read_u16(palette, i * 2));
        let byte = |at: usize| self.data.get(at).copied().unwrap_or(0);
        let paletted = |i: usize| {
            if i == 0 && self.color0_transparent {
                [0; 4]
            } else {
                with_alpha(color(i), 255)
            }
        };
        match self.format {
            TextureFormat::Colors4 => {
                paletted(usize::from(byte(index / 4) >> ((index % 4) * 2)) & 3)
            }
            TextureFormat::Colors16 => {
                paletted(usize::from(byte(index / 2) >> ((index % 2) * 4)) & 15)
            }
            TextureFormat::Colors256 => paletted(usize::from(byte(index))),
            TextureFormat::A3I5 => {
                let value = byte(index);
                let alpha = value >> 5;
                with_alpha(
                    color(usize::from(value & 31)),
                    scale_alpha(u32::from(alpha) * 4 + u32::from(alpha) / 2),
                )
            }
            TextureFormat::A5I3 => {
                let value = byte(index);
                with_alpha(
                    color(usize::from(value & 7)),
                    scale_alpha(u32::from(value >> 3)),
                )
            }
            TextureFormat::Direct => {
                let value = read_u16(self.data, index * 2);
                with_alpha(rgb555(value), if value & 0x8000 != 0 { 255 } else { 0 })
            }
            TextureFormat::Compressed => self.compressed(palette, x, y),
        }
    }

    fn compressed(&self, palette: &[u8], x: usize, y: usize) -> [u8; 4] {
        let block = (y / 4) * (self.width / 4) + x / 4;
        let word = read_u32(self.data, block * 4);
        let texel = (word >> ((y % 4) * 8 + (x % 4) * 2)) & 3;
        let slot = read_u16(self.indices, block * 2);
        let base = usize::from(slot & 0x3FFF) * 2;
        let mode = slot >> 14;
        let color = |i: usize| rgb555(read_u16(palette, (base + i) * 2));
        let mix = |a: [u8; 3], b: [u8; 3], wa: u16, wb: u16| {
            let channel =
                |i: usize| ((u16::from(a[i]) * wa + u16::from(b[i]) * wb) / (wa + wb)) as u8;
            [channel(0), channel(1), channel(2)]
        };
        let rgb = match (mode, texel) {
            (0 | 1, 3) => return [0; 4],
            (1, 2) => mix(color(0), color(1), 1, 1),
            (3, 2) => mix(color(0), color(1), 5, 3),
            (3, 3) => mix(color(0), color(1), 3, 5),
            (_, texel) => color(texel as usize),
        };
        with_alpha(rgb, 255)
    }
}

pub fn palette_rgba(palette: &[u8]) -> Vec<[u8; 4]> {
    (0..palette.len() / 2)
        .map(|i| with_alpha(rgb555(read_u16(palette, i * 2)), 255))
        .collect()
}

pub fn rgb555(color: u16) -> [u8; 3] {
    let channel = |shift: u16| {
        let value = ((color >> shift) & 31) as u8;
        (value << 3) | (value >> 2)
    };
    [channel(0), channel(5), channel(10)]
}

fn with_alpha(rgb: [u8; 3], alpha: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

fn scale_alpha(alpha: u32) -> u8 {
    ((alpha.min(31) * 255 + 15) / 31) as u8
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    bytes
        .get(at..at + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;
    const WHITE: u16 = 0x7FFF;

    fn palette(colors: &[u16]) -> Vec<u8> {
        colors.iter().flat_map(|c| c.to_le_bytes()).collect()
    }

    fn texels(format: TextureFormat, width: usize, height: usize, data: &[u8]) -> Texels<'_> {
        Texels {
            format,
            width,
            height,
            data,
            indices: &[],
            color0_transparent: false,
        }
    }

    #[test]
    fn five_bit_channels_fill_eight_bits() {
        assert_eq!(rgb555(WHITE), [255, 255, 255]);
        assert_eq!(rgb555(RED), [255, 0, 0]);
        assert_eq!(rgb555(BLUE), [0, 0, 255]);
        assert_eq!(rgb555(0x0010), [132, 0, 0]);
    }

    #[test]
    fn four_colours_pack_four_texels_a_byte_from_the_low_bits() {
        let pal = palette(&[RED, GREEN, BLUE, WHITE]);
        let rgba = texels(TextureFormat::Colors4, 4, 1, &[0b11_10_01_00]).decode(&pal);
        assert_eq!(
            rgba,
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255
            ]
        );
    }

    #[test]
    fn colour_zero_can_be_transparent() {
        let pal = palette(&[RED, GREEN]);
        let mut tex = texels(TextureFormat::Colors16, 2, 1, &[0x10]);
        tex.color0_transparent = true;
        assert_eq!(tex.decode(&pal), [0, 0, 0, 0, 0, 255, 0, 255]);
    }

    #[test]
    fn a3i5_and_a5i3_carry_their_alpha() {
        let pal = palette(&[RED, GREEN]);
        let a3i5 = texels(TextureFormat::A3I5, 2, 1, &[0b111_00001, 0b000_00000]).decode(&pal);
        assert_eq!(a3i5, [0, 255, 0, 255, 255, 0, 0, 0]);
        let a5i3 = texels(TextureFormat::A5I3, 1, 1, &[(16 << 3) | 1]).decode(&pal);
        assert_eq!(a5i3, [0, 255, 0, 132]);
    }

    #[test]
    fn direct_colour_uses_bit_15_as_alpha() {
        let data = [(BLUE | 0x8000).to_le_bytes(), RED.to_le_bytes()].concat();
        let rgba = texels(TextureFormat::Direct, 2, 1, &data).decode(&[]);
        assert_eq!(rgba, [0, 0, 255, 255, 255, 0, 0, 0]);
    }

    #[test]
    fn compressed_blocks_follow_their_mode() {
        let pal = palette(&[0, 0, 0x0000, 0x7FFF, GREEN, BLUE]);
        let word: u32 = 0b11_10_01_00;
        let data = [word.to_le_bytes(), word.to_le_bytes()].concat();
        let indices = [(1u16 | (3 << 14)).to_le_bytes(), 2u16.to_le_bytes()].concat();
        let tex = Texels {
            format: TextureFormat::Compressed,
            width: 8,
            height: 4,
            data: &data,
            indices: &indices,
            color0_transparent: false,
        };
        let rgba = tex.decode(&pal);
        let pixel = |x: usize| &rgba[x * 4..x * 4 + 4];
        assert_eq!(pixel(0), [0, 0, 0, 255]);
        assert_eq!(pixel(1), [255, 255, 255, 255]);
        assert_eq!(pixel(2), [95, 95, 95, 255]);
        assert_eq!(pixel(3), [159, 159, 159, 255]);
        assert_eq!(pixel(4), [0, 255, 0, 255]);
        assert_eq!(pixel(5), [0, 0, 255, 255]);
        assert_eq!(pixel(7), [0, 0, 0, 0]);
    }

    #[test]
    fn short_data_reads_as_zero_not_a_panic() {
        let tex = texels(TextureFormat::Colors256, 4, 4, &[1]);
        assert_eq!(tex.decode(&[]).len(), 64);
    }
}
