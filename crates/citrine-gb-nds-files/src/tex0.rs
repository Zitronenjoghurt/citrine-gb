use super::{RomError, slice, u16_at, u32_at};
use crate::texture::{Texels, TextureFormat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureInfo {
    pub name: String,
    pub format: TextureFormat,
    pub width: usize,
    pub height: usize,
    pub color0_transparent: bool,
    offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteInfo {
    pub name: String,
    offset: usize,
}

const SLOT: usize = 0x2_0000;
const TEXTURE_MEMORY: usize = 0x8_0000;
const PALETTE_MEMORY: usize = 0x1_8000;

pub struct Vram {
    pub texture: Vec<u8>,
    pub palette: Vec<u8>,
}

#[derive(Debug)]
pub struct Tex0<'a> {
    textures: Vec<TextureInfo>,
    palettes: Vec<PaletteInfo>,
    texel_block: &'a [u8],
    compressed_block: &'a [u8],
    index_block: &'a [u8],
    palette_block: &'a [u8],
}

impl<'a> Tex0<'a> {
    pub fn find(file: &'a [u8]) -> Result<Self, RomError> {
        let sections = usize::from(u16_at(file, 0x0E)?);
        for i in 0..sections {
            let at = u32_at(file, 0x10 + i * 4)? as usize;
            if file.get(at..at + 4) == Some(b"TEX0") {
                return Self::parse(&file[at..]);
            }
        }
        Err(RomError::MissingSection("TEX0"))
    }

    pub fn parse(tex0: &'a [u8]) -> Result<Self, RomError> {
        if tex0.get(..4) != Some(b"TEX0") {
            return Err(RomError::MissingSection("TEX0"));
        }
        let texture_dict = usize::from(u16_at(tex0, 0x0E)?);
        let texel_start = u32_at(tex0, 0x14)? as usize;
        let compressed_start = u32_at(tex0, 0x24)? as usize;
        let index_start = u32_at(tex0, 0x28)? as usize;
        let palette_len = usize::from(u16_at(tex0, 0x30)?) << 3;
        let palette_dict = u32_at(tex0, 0x34)? as usize;
        let palette_start = u32_at(tex0, 0x38)? as usize;
        let textures = dict(tex0, texture_dict, 8)?
            .into_iter()
            .filter_map(|(name, entry)| {
                let params = u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
                Some(TextureInfo {
                    name,
                    format: TextureFormat::from_bits(params >> 26)?,
                    width: 8 << ((params >> 20) & 7),
                    height: 8 << ((params >> 23) & 7),
                    color0_transparent: params & (1 << 29) != 0,
                    offset: ((params & 0xFFFF) as usize) << 3,
                })
            })
            .collect();
        let palettes = if palette_dict == 0 {
            Vec::new()
        } else {
            dict(tex0, palette_dict, 4)?
                .into_iter()
                .map(|(name, entry)| PaletteInfo {
                    name,
                    offset: usize::from(u16::from_le_bytes([entry[0], entry[1]])) << 3,
                })
                .collect()
        };
        let tail = |start: usize| tex0.get(start..).unwrap_or(&[]);
        Ok(Self {
            textures,
            palettes,
            texel_block: tail(texel_start),
            compressed_block: tail(compressed_start),
            index_block: tail(index_start),
            palette_block: tex0
                .get(palette_start..palette_start + palette_len)
                .unwrap_or_else(|| tail(palette_start)),
        })
    }

    pub fn textures(&self) -> &[TextureInfo] {
        &self.textures
    }

    pub fn palettes(&self) -> &[PaletteInfo] {
        &self.palettes
    }

    pub fn texels(&self, texture: usize) -> Option<Texels<'a>> {
        let info = self.textures.get(texture)?;
        let mut texels = Texels {
            format: info.format,
            width: info.width,
            height: info.height,
            data: &[],
            indices: &[],
            color0_transparent: info.color0_transparent,
        };
        let len = texels.byte_len();
        let window = |block: &'a [u8], start: usize, len: usize| {
            block
                .get(start..(start + len).min(block.len()))
                .unwrap_or(&[])
        };
        if info.format == TextureFormat::Compressed {
            texels.data = window(self.compressed_block, info.offset, len);
            texels.indices = window(self.index_block, info.offset / 2, len / 2);
        } else {
            texels.data = window(self.texel_block, info.offset, len);
        }
        Some(texels)
    }

    pub fn palette(&self, palette: usize) -> Option<&'a [u8]> {
        let info = self.palettes.get(palette)?;
        self.palette_block.get(info.offset..)
    }

    pub fn palette_for(&self, texture: usize) -> Option<usize> {
        let info = self.textures.get(texture)?;
        if !info.format.uses_palette() || self.palettes.is_empty() {
            return None;
        }
        let paired = format!("{}_pl", info.name);
        self.palettes
            .iter()
            .position(|p| p.name == paired)
            .or_else(|| self.palettes.iter().position(|p| p.name == info.name))
            .or(Some(0))
    }

    fn extent(&self, compressed: bool) -> usize {
        self.textures
            .iter()
            .enumerate()
            .filter(|(_, info)| (info.format == TextureFormat::Compressed) == compressed)
            .filter_map(|(index, info)| Some(info.offset + self.texels(index)?.byte_len()))
            .max()
            .unwrap_or(0)
    }

    fn compressed_base(&self) -> usize {
        let plain = self.extent(false).next_multiple_of(8);
        if plain + self.extent(true) <= SLOT {
            plain
        } else {
            2 * SLOT
        }
    }

    pub fn vram(&self) -> Vram {
        let mut texture = vec![0u8; TEXTURE_MEMORY];
        let mut palette = vec![0u8; PALETTE_MEMORY];
        let copy = |into: &mut [u8], at: usize, from: &[u8], len: usize| {
            let len = len.min(from.len()).min(into.len().saturating_sub(at));
            into[at..at + len].copy_from_slice(&from[..len]);
        };
        copy(&mut texture, 0, self.texel_block, self.extent(false));
        let base = self.compressed_base();
        let compressed = self.extent(true);
        copy(&mut texture, base, self.compressed_block, compressed);
        copy(
            &mut texture,
            SLOT + base / 2,
            self.index_block,
            compressed / 2,
        );
        copy(
            &mut palette,
            0,
            self.palette_block,
            self.palette_block.len(),
        );
        Vram { texture, palette }
    }

    pub fn texture_param(&self, texture: usize) -> Option<u32> {
        let info = self.textures.get(texture)?;
        let offset = if info.format == TextureFormat::Compressed {
            self.compressed_base() + info.offset
        } else {
            info.offset
        };
        let size = |n: usize| (n / 8).trailing_zeros();
        Some(
            ((offset >> 3) as u32 & 0xFFFF)
                | size(info.width) << 20
                | size(info.height) << 23
                | info.format.bits() << 26
                | u32::from(info.color0_transparent) << 29,
        )
    }

    pub fn palette_base(&self, palette: usize, format: TextureFormat) -> Option<u32> {
        let offset = self.palettes.get(palette)?.offset as u32;
        Some(if format == TextureFormat::Colors4 {
            offset >> 3
        } else {
            offset >> 4
        })
    }

    pub fn texture_named(&self, name: &str) -> Option<usize> {
        self.textures.iter().position(|t| t.name == name)
    }

    pub fn palette_named(&self, name: &str) -> Option<usize> {
        self.palettes.iter().position(|p| p.name == name)
    }

    pub fn decode(&self, texture: usize, palette: Option<usize>) -> Option<Vec<u8>> {
        let texels = self.texels(texture)?;
        let colors = palette.and_then(|p| self.palette(p)).unwrap_or(&[]);
        Some(texels.decode(colors))
    }
}

fn dict(tex0: &[u8], at: usize, unit: usize) -> Result<Vec<(String, Vec<u8>)>, RomError> {
    let count = usize::from(*tex0.get(at + 1).ok_or(RomError::Truncated(at + 2))?);
    let data = at + 0x10 + 4 * count;
    let names = data + count * unit;
    (0..count)
        .map(|i| {
            let entry = slice(tex0, data + i * unit, data + (i + 1) * unit)?.to_vec();
            let raw = slice(tex0, names + i * 16, names + (i + 1) * 16)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            Ok((String::from_utf8_lossy(&raw[..end]).into_owned(), entry))
        })
        .collect()
}

#[cfg(any(test, feature = "fixtures"))]
pub struct FixtureTexture<'a> {
    pub name: &'a str,
    pub format: u32,
    pub width_shift: u32,
    pub height_shift: u32,
    pub data: &'a [u8],
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build(textures: &[FixtureTexture], palettes: &[(&str, &[u8])]) -> Vec<u8> {
    let names = |list: Vec<&str>| -> Vec<u8> {
        list.iter()
            .flat_map(|n| {
                let mut raw = n.as_bytes().to_vec();
                raw.resize(16, 0);
                raw
            })
            .collect()
    };
    let make_dict = |entries: Vec<Vec<u8>>, list: Vec<&str>, unit: u16| -> Vec<u8> {
        let count = entries.len();
        let mut out = vec![0, count as u8, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];
        out.extend(std::iter::repeat_n(0, 4 * count));
        out.extend_from_slice(&unit.to_le_bytes());
        out.extend_from_slice(&((4 + count * unit as usize) as u16).to_le_bytes());
        for entry in entries {
            out.extend(entry);
        }
        out.extend(names(list));
        let len = out.len() as u16;
        out[2..4].copy_from_slice(&len.to_le_bytes());
        out
    };
    let mut texel_data = Vec::new();
    let mut entries = Vec::new();
    for t in textures {
        let params = ((texel_data.len() as u32) >> 3)
            | (t.width_shift << 20)
            | (t.height_shift << 23)
            | (t.format << 26);
        entries.push([params.to_le_bytes(), [0; 4]].concat());
        texel_data.extend_from_slice(t.data);
        while texel_data.len() % 8 != 0 {
            texel_data.push(0);
        }
    }
    let texture_dict = make_dict(entries, textures.iter().map(|t| t.name).collect(), 8);
    let mut palette_data = Vec::new();
    let mut palette_entries = Vec::new();
    for (_, colors) in palettes {
        palette_entries.push([((palette_data.len() >> 3) as u16).to_le_bytes(), [0; 2]].concat());
        palette_data.extend_from_slice(colors);
        while palette_data.len() % 8 != 0 {
            palette_data.push(0);
        }
    }
    let palette_dict = make_dict(palette_entries, palettes.iter().map(|p| p.0).collect(), 4);
    let header = 0x3C;
    let texture_dict_at = header;
    let palette_dict_at = texture_dict_at + texture_dict.len();
    let texel_at = palette_dict_at + palette_dict.len();
    let palette_at = texel_at + texel_data.len();
    let mut tex0 = vec![0u8; header];
    tex0[..4].copy_from_slice(b"TEX0");
    tex0[0x0E..0x10].copy_from_slice(&(texture_dict_at as u16).to_le_bytes());
    tex0[0x14..0x18].copy_from_slice(&(texel_at as u32).to_le_bytes());
    tex0[0x24..0x28].copy_from_slice(&(texel_at as u32).to_le_bytes());
    tex0[0x28..0x2C].copy_from_slice(&(texel_at as u32).to_le_bytes());
    tex0[0x30..0x32].copy_from_slice(&((palette_data.len() >> 3) as u16).to_le_bytes());
    tex0[0x34..0x38].copy_from_slice(&(palette_dict_at as u32).to_le_bytes());
    tex0[0x38..0x3C].copy_from_slice(&(palette_at as u32).to_le_bytes());
    tex0.extend(texture_dict);
    tex0.extend(palette_dict);
    tex0.extend(texel_data);
    tex0.extend(palette_data);
    let len = tex0.len() as u32;
    tex0[4..8].copy_from_slice(&len.to_le_bytes());
    let mut file = b"BTX0\xFF\xFE\x01\x00".to_vec();
    file.extend_from_slice(&(len + 0x14).to_le_bytes());
    file.extend_from_slice(&[0x10, 0, 1, 0]);
    file.extend_from_slice(&0x14u32.to_le_bytes());
    file.extend(tex0);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 2] = [0x1F, 0x00];
    const GREEN: [u8; 2] = [0xE0, 0x03];

    fn two_textures() -> Vec<u8> {
        build(
            &[
                FixtureTexture {
                    name: "stone",
                    format: 3,
                    width_shift: 0,
                    height_shift: 0,
                    data: &[0x10; 32],
                },
                FixtureTexture {
                    name: "sky",
                    format: 7,
                    width_shift: 1,
                    height_shift: 0,
                    data: &[0x00, 0xFC].repeat(128),
                },
            ],
            &[
                ("other_pl", &[RED, GREEN].concat()),
                ("stone_pl", &[GREEN, RED].concat()),
            ],
        )
    }

    #[test]
    fn lists_textures_with_size_and_format() {
        let file = two_textures();
        let tex0 = Tex0::find(&file).unwrap();
        let summary: Vec<_> = tex0
            .textures()
            .iter()
            .map(|t| (t.name.as_str(), t.format, t.width, t.height))
            .collect();
        assert_eq!(
            summary,
            [
                ("stone", TextureFormat::Colors16, 8, 8),
                ("sky", TextureFormat::Direct, 16, 8),
            ]
        );
        let names: Vec<_> = tex0.palettes().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["other_pl", "stone_pl"]);
    }

    #[test]
    fn pairs_a_texture_with_its_named_palette() {
        let file = two_textures();
        let tex0 = Tex0::find(&file).unwrap();
        assert_eq!(tex0.palette_for(0), Some(1));
        assert_eq!(tex0.palette_for(1), None);
    }

    #[test]
    fn decodes_through_the_chosen_palette() {
        let file = two_textures();
        let tex0 = Tex0::find(&file).unwrap();
        let paired = tex0.decode(0, Some(1)).unwrap();
        assert_eq!(&paired[..4], &[0, 255, 0, 255]);
        assert_eq!(&paired[4..8], &[255, 0, 0, 255]);
        let other = tex0.decode(0, Some(0)).unwrap();
        assert_eq!(&other[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn decodes_direct_colour_without_a_palette() {
        let file = two_textures();
        let tex0 = Tex0::find(&file).unwrap();
        let rgba = tex0.decode(1, None).unwrap();
        assert_eq!(rgba.len(), 16 * 8 * 4);
        assert_eq!(&rgba[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn vram_places_texels_and_params_point_at_them() {
        let file = two_textures();
        let tex0 = Tex0::find(&file).unwrap();
        let vram = tex0.vram();
        assert_eq!(&vram.texture[..2], &[0x10, 0x10]);
        assert_eq!(&vram.texture[32..34], &[0x00, 0xFC]);
        assert_eq!(&vram.palette[..4], &[0x1F, 0x00, 0xE0, 0x03]);
        assert_eq!(tex0.texture_param(0), Some(3 << 26));
        assert_eq!(tex0.texture_param(1), Some(4 | 1 << 20 | 7 << 26));
        assert_eq!(tex0.palette_base(1, TextureFormat::Colors4), Some(1));
        assert_eq!(tex0.texture_named("sky"), Some(1));
        assert_eq!(tex0.palette_named("stone_pl"), Some(1));
    }

    #[test]
    fn a_file_without_tex0_says_so() {
        let file = b"BMD0\xFF\xFE\x01\x00\x14\x00\x00\x00\x10\x00\x00\x00".to_vec();
        assert_eq!(
            Tex0::find(&file).unwrap_err(),
            RomError::MissingSection("TEX0")
        );
    }

    #[test]
    fn a_truncated_dict_is_an_error_not_a_panic() {
        let mut file = two_textures();
        file.truncate(0x14 + 0x3C + 0x14);
        assert!(Tex0::find(&file).is_err());
    }
}
