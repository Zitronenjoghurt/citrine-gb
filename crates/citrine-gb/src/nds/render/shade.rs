use super::texture::Texel;
use super::{Pixel, expand};

pub const DECAL: u32 = 1;
pub const TOON: u32 = 2;
pub const SHADOW: u32 = 3;

const OPAQUE: u32 = 31;

#[inline]
pub fn shade(mode: u32, vertex: [u8; 3], alpha: u8, texel: Option<Texel>) -> Pixel {
    let Some(texel) = texel else {
        let [r, g, b] = vertex;
        return Pixel { r, g, b, alpha };
    };
    let alpha = u32::from(alpha);
    let texture = |shift: u16| u32::from(expand(texel.color >> shift));
    let texture = [texture(0), texture(5), texture(10)];
    let texture_alpha = u32::from(texel.alpha);
    match mode {
        DECAL | SHADOW => {
            let mixed = |channel: usize| {
                let (t, v) = (texture[channel], u32::from(vertex[channel]));
                match texture_alpha {
                    0 => v,
                    OPAQUE => t,
                    a => (t * a + v * (OPAQUE - a)) >> 5,
                }
            };
            pixel([mixed(0), mixed(1), mixed(2)], alpha)
        }
        _ => {
            let modulated = |channel: usize| {
                ((texture[channel] + 1) * (u32::from(vertex[channel]) + 1) - 1) >> 6
            };
            pixel(
                [modulated(0), modulated(1), modulated(2)],
                ((texture_alpha + 1) * (alpha + 1) - 1) >> 5,
            )
        }
    }
}

pub fn shade_toon(
    vertex: [u8; 3],
    alpha: u8,
    texel: Option<Texel>,
    table: &[u16; 32],
    highlight: bool,
) -> Pixel {
    let tone = table[usize::from(vertex[0] >> 1) & 31];
    let tone = [0, 5, 10].map(|shift| expand(tone >> shift));
    if !highlight {
        return shade(TOON, tone, alpha, texel);
    }
    let mut out = shade(TOON, [vertex[0]; 3], alpha, texel);
    out.r = (out.r + tone[0]).min(63);
    out.g = (out.g + tone[1]).min(63);
    out.b = (out.b + tone[2]).min(63);
    out
}

fn pixel([r, g, b]: [u32; 3], alpha: u32) -> Pixel {
    Pixel {
        r: r as u8,
        g: g as u8,
        b: b as u8,
        alpha: alpha as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULATE: u32 = 0;

    fn texel(color: u16, alpha: u8) -> Option<Texel> {
        Some(Texel { color, alpha })
    }

    fn rgb(pixel: Pixel) -> [u8; 3] {
        [pixel.r, pixel.g, pixel.b]
    }

    #[test]
    fn untextured_pixels_take_the_vertex_colour_and_polygon_alpha() {
        let out = shade(MODULATE, [10, 20, 30], 17, None);
        assert_eq!(
            out,
            Pixel {
                r: 10,
                g: 20,
                b: 30,
                alpha: 17
            }
        );
    }

    #[test]
    fn modulation_multiplies_texture_and_vertex_per_gbatek() {
        let out = shade(MODULATE, [63, 31, 0], 31, texel(0x7FFF, 31));
        assert_eq!(rgb(out), [63, 31, 0]);
        let half = shade(
            MODULATE,
            [63, 63, 63],
            31,
            texel(15 | 15 << 5 | 15 << 10, 31),
        );
        assert_eq!(rgb(half), [31, 31, 31]);
    }

    #[test]
    fn modulated_alpha_multiplies_five_bit_alphas_like_melonds() {
        assert_eq!(shade(MODULATE, [0; 3], 31, texel(0, 31)).alpha, 31);
        assert_eq!(shade(MODULATE, [0; 3], 15, texel(0, 31)).alpha, 15);
        assert_eq!(shade(MODULATE, [0; 3], 31, texel(0, 0)).alpha, 0);
    }

    #[test]
    fn decal_shows_the_vertex_colour_through_transparent_texels_per_gbatek() {
        assert_eq!(
            rgb(shade(DECAL, [5, 6, 7], 31, texel(0x7FFF, 0))),
            [5, 6, 7]
        );
        assert_eq!(
            rgb(shade(DECAL, [5, 6, 7], 31, texel(0x7FFF, 31))),
            [63, 63, 63]
        );
        assert_eq!(shade(DECAL, [5, 6, 7], 12, texel(0x7FFF, 31)).alpha, 12);
    }

    #[test]
    fn decal_mixes_by_the_texel_alpha_in_five_bits_like_melonds() {
        let out = shade(DECAL, [0, 0, 0], 31, texel(0x001F, 16));
        assert_eq!(out.r, 31);
    }

    #[test]
    fn toon_mode_shades_like_modulation_until_the_toon_table_exists() {
        let texture = texel(0x1234, 20);
        assert_eq!(
            shade(TOON, [9, 8, 7], 20, texture),
            shade(MODULATE, [9, 8, 7], 20, texture)
        );
    }

    #[test]
    fn shadow_polygons_blend_their_texture_like_decal_like_melonds() {
        let texture = texel(0x1234, 20);
        assert_eq!(
            shade(SHADOW, [9, 8, 7], 20, texture),
            shade(DECAL, [9, 8, 7], 20, texture)
        );
    }

    fn table() -> [u16; 32] {
        let mut table = [0; 32];
        table[4] = 31;
        table[5] = 10 << 5;
        table
    }

    #[test]
    fn toon_mode_takes_the_vertex_colour_from_the_toon_table_like_melonds() {
        let out = shade_toon([9, 40, 50], 31, None, &table(), false);
        assert_eq!(rgb(out), [63, 0, 0]);
        let out = shade_toon([10, 40, 50], 31, None, &table(), false);
        assert_eq!(rgb(out), [0, 21, 0]);
    }

    #[test]
    fn highlight_mode_greys_the_vertex_and_adds_the_toon_colour_like_melonds() {
        let out = shade_toon([11, 40, 50], 31, None, &table(), true);
        assert_eq!(rgb(out), [11, 32, 11]);
        let out = shade_toon([9, 0, 0], 31, None, &table(), true);
        assert_eq!(rgb(out), [63, 9, 9]);
    }
}
