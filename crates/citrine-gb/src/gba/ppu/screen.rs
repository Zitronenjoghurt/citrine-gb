use std::sync::OnceLock;

const LCD_GAMMA: f64 = 4.0;
const OUT_GAMMA: f64 = 2.2;
const COLOURS: usize = 1 << 15;

pub(crate) fn rgba(color: u16) -> [u8; 4] {
    static TABLE: OnceLock<Box<[[u8; 4]]>> = OnceLock::new();
    TABLE.get_or_init(|| (0..COLOURS as u16).map(mix).collect())[usize::from(color & 0x7FFF)]
}

fn mix(color: u16) -> [u8; 4] {
    let linear = |shift: u16| (f64::from((color >> shift) & 0x1F) / 31.0).powf(LCD_GAMMA);
    let (r, g, b) = (linear(0), linear(5), linear(10));
    let out = |value: f64| {
        let level = (value / 255.0).powf(1.0 / OUT_GAMMA) * 255.0 * 255.0 / 280.0;
        level.round().clamp(0.0, 255.0) as u8
    };
    [
        out(255.0 * r + 50.0 * g),
        out(10.0 * r + 230.0 * g + 30.0 * b),
        out(50.0 * r + 10.0 * g + 220.0 * b),
        0xFF,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_stays_black_and_white_turns_the_screen_s_warm_white() {
        assert_eq!(rgba(0), [0, 0, 0, 0xFF]);
        let [r, g, b, _] = rgba(0x7FFF);
        assert!(r > 240 && g > 225 && b > 230, "{r} {g} {b}");
        assert!(r > g && b > g);
    }

    #[test]
    fn mid_tones_come_out_darker_than_the_raw_colours() {
        let mid = 16 | 16 << 5 | 16 << 10;
        let raw = super::super::rgba(mid);
        let screen = rgba(mid);
        for channel in 0..3 {
            assert!(screen[channel] < raw[channel], "{screen:?} vs {raw:?}");
        }
    }

    #[test]
    fn pure_red_bleeds_into_the_other_channels() {
        let [r, g, b, _] = rgba(0x1F);
        assert!(r > 200);
        assert!(g > 0 && b > 0 && g < r / 2 && b < r / 2, "{r} {g} {b}");
    }

    #[test]
    fn the_unused_top_bit_is_ignored() {
        assert_eq!(rgba(0x8000 | 0x1234), rgba(0x1234));
    }
}
