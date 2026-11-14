pub const WHITE: u32 = 0x3_FFFF;
pub(super) const CHANNEL: u32 = 0x3F;
const SHIFTS: [u32; 3] = [0, 6, 12];
const LOW_TWO: u32 = 0x03_0303;
const OPAQUE: u32 = 0xFF00_0000;
const RED_BLUE: u32 = CHANNEL | CHANNEL << 12;
const GREEN: u32 = CHANNEL << 6;
pub(super) const FULL_SHADE: u32 = 16;

pub fn widen(color: u16) -> u32 {
    let color = u32::from(color);
    (color & 0x1F) << 1 | (color >> 5 & 0x1F) << 7 | (color >> 10 & 0x1F) << 13
}

pub fn narrow(color: u32) -> u16 {
    SHIFTS.iter().zip([0, 5, 10]).fold(0, |out, (shift, to)| {
        out | ((color >> shift & CHANNEL) >> 1) << to
    }) as u16
}

pub fn from_channels(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) & CHANNEL | (u32::from(g) & CHANNEL) << 6 | (u32::from(b) & CHANNEL) << 12
}

pub fn map(a: u32, b: u32, f: impl Fn(u32, u32) -> u32) -> u32 {
    SHIFTS.iter().fold(0, |out, &shift| {
        out | f(a >> shift & CHANNEL, b >> shift & CHANNEL).min(CHANNEL) << shift
    })
}

pub fn blend(top: u32, below: u32, eva: u32, evb: u32) -> u32 {
    map(top, below, |a, b| (a * eva + b * evb + 8) >> 4)
}

pub fn blend_3d(top: u32, below: u32, alpha: u32) -> u32 {
    let eva = alpha + 1;
    if eva == 32 {
        return top;
    }
    map(top, below, |a, b| (a * eva + b * (32 - eva) + 16) >> 5)
}

pub fn brighten(color: u32, factor: u32, bias: u32) -> u32 {
    map(color, 0, |a, _| a + (((CHANNEL - a) * factor + bias) >> 4))
}

pub fn darken(color: u32, factor: u32, bias: u32) -> u32 {
    map(color, 0, |a, _| a - ((a * factor + bias) >> 4))
}

pub fn brighten_line(line: &mut [u32], factor: u32) {
    if factor >= FULL_SHADE {
        line.fill(WHITE);
        return;
    }
    let factor = factor & 0xF;
    for pixel in line {
        let color = *pixel & WHITE;
        let room = color ^ WHITE;
        let red_blue = (((room & RED_BLUE) * factor) >> 4) & RED_BLUE;
        let green = (((room & GREEN) * factor) >> 4) & GREEN;
        *pixel = color + red_blue + green;
    }
}

pub fn darken_line(line: &mut [u32], factor: u32, bias: u32) {
    debug_assert!(bias < FULL_SHADE);
    if factor >= FULL_SHADE {
        line.fill(0);
        return;
    }
    let (factor, bias) = (factor & 0xF, bias & 0xF);
    let red_blue_bias = bias | bias << 12;
    let green_bias = bias << 6;
    for pixel in line {
        let color = *pixel & WHITE;
        let red_blue = (((color & RED_BLUE) * factor + red_blue_bias) >> 4) & RED_BLUE;
        let green = (((color & GREEN) * factor + green_bias) >> 4) & GREEN;
        *pixel = color - red_blue - green;
    }
}

pub fn rgba(color: u32) -> [u8; 4] {
    let spread = color & CHANNEL | color << 2 & CHANNEL << 8 | color << 4 & CHANNEL << 16;
    (spread << 2 | spread >> 4 & LOW_TWO | OPAQUE).to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_15_bit_color_widens_by_one_bit_per_channel_like_melonds() {
        assert_eq!(widen(0x7FFF), from_channels(62, 62, 62));
        assert_eq!(widen(1 | 2 << 5 | 3 << 10), from_channels(2, 4, 6));
    }

    #[test]
    fn narrowing_drops_the_low_bit_and_undoes_widening() {
        assert_eq!(narrow(from_channels(63, 1, 32)), 31 | 16 << 10);
        assert_eq!(narrow(widen(0x1234)), 0x1234);
    }

    #[test]
    fn the_screen_shows_all_64_levels_of_a_channel() {
        assert_eq!(rgba(from_channels(63, 1, 0)), [255, 4, 0, 255]);
        assert_eq!(rgba(widen(0x7FFF)), [251, 251, 251, 255]);
    }

    #[test]
    fn every_color_widens_each_channel_by_its_top_bits() {
        for color in 0..=WHITE {
            let [r, g, b, a] = rgba(color | 0xFFFC_0000);
            let expected = SHIFTS.map(|shift| {
                let level = (color >> shift & CHANNEL) as u8;
                level << 2 | level >> 4
            });
            assert_eq!([r, g, b], expected, "{color:05x}");
            assert_eq!(a, 0xFF);
        }
    }

    #[test]
    fn blends_round_to_nearest_like_melonds() {
        assert_eq!(
            blend(from_channels(1, 0, 0), 0, 8, 8),
            from_channels(1, 0, 0)
        );
        assert_eq!(blend(WHITE, WHITE, 16, 16), WHITE);
        assert_eq!(
            blend_3d(from_channels(63, 0, 0), from_channels(0, 63, 0), 15),
            from_channels(32, 32, 0)
        );
        assert_eq!(
            blend_3d(from_channels(7, 0, 0), 0, 31),
            from_channels(7, 0, 0)
        );
    }

    #[test]
    fn brightness_steps_carry_their_bias() {
        assert_eq!(brighten(0, 16, 8), WHITE);
        assert_eq!(
            brighten(from_channels(62, 0, 0), 1, 8),
            from_channels(62, 4, 4)
        );
        assert_eq!(darken(WHITE, 16, 7), 0);
        assert_eq!(
            darken(from_channels(1, 0, 0), 1, 0xF),
            from_channels(0, 0, 0)
        );
    }

    #[test]
    fn whole_line_shades_match_each_pixel_shaded_alone_at_every_factor() {
        let colors: Vec<u32> = (0..=WHITE).step_by(7).collect();
        for factor in 0..=16 {
            let mut bright: Vec<u32> = colors.iter().map(|&color| color | OPAQUE).collect();
            let mut dark = bright.clone();
            brighten_line(&mut bright, factor);
            darken_line(&mut dark, factor, 0xF);
            for ((&color, &bright), &dark) in colors.iter().zip(&bright).zip(&dark) {
                assert_eq!(bright, brighten(color, factor, 0), "factor {factor}");
                assert_eq!(dark, darken(color, factor, 0xF), "factor {factor}");
            }
        }
    }
}
