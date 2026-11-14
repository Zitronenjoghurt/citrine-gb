const CONTROL_MASK: u16 = 0x3FFF;
const ALPHA_MASK: u16 = 0x1F1F;
const BRIGHTNESS_MASK: u16 = 0x1F;
const MAX_WEIGHT: u16 = 16;
const MAX_CHANNEL: u16 = 31;
const COLOR_MASK: u16 = 0x7FFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Alpha,
    Brighten,
    Darken,
}

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Blend {
    control: u16,
    alpha: u16,
    brightness: u16,
}

impl Blend {
    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        let (register, writable) = match offset {
            0 => (&mut self.control, CONTROL_MASK),
            2 => (&mut self.alpha, ALPHA_MASK),
            _ => (&mut self.brightness, BRIGHTNESS_MASK),
        };
        *register = ((*register & !mask) | (value & mask)) & writable;
    }

    pub fn read(&self, offset: usize) -> u16 {
        match offset {
            0 => self.control,
            2 => self.alpha,
            _ => 0,
        }
    }

    pub fn effect(&self) -> Effect {
        match self.control >> 6 & 0b11 {
            0 => Effect::None,
            1 => Effect::Alpha,
            2 => Effect::Brighten,
            _ => Effect::Darken,
        }
    }

    pub fn first(&self, layer: u8) -> bool {
        self.control >> layer & 1 != 0
    }

    pub fn second(&self, layer: u8) -> bool {
        self.control >> (8 + layer) & 1 != 0
    }

    pub fn alpha(&self, top: u16, below: u16) -> u16 {
        let (eva, evb) = self.weights();
        map_channels(top, below, |a, b| {
            ((a * eva + b * evb) >> 4).min(MAX_CHANNEL)
        })
    }

    pub fn brighten(&self, color: u16) -> u16 {
        let evy = self.evy();
        map_channels(color, 0, |a, _| a + (((MAX_CHANNEL - a) * evy) >> 4))
    }

    pub fn darken(&self, color: u16) -> u16 {
        let evy = self.evy();
        map_channels(color, 0, |a, _| a - ((a * evy) >> 4))
    }

    pub fn weights(&self) -> (u16, u16) {
        (
            (self.alpha & 0x1F).min(MAX_WEIGHT),
            (self.alpha >> 8 & 0x1F).min(MAX_WEIGHT),
        )
    }

    pub fn evy(&self) -> u16 {
        self.brightness.min(MAX_WEIGHT)
    }
}

pub(crate) fn map_channels(a: u16, b: u16, f: impl Fn(u16, u16) -> u16) -> u16 {
    let a = a & COLOR_MASK;
    let b = b & COLOR_MASK;
    [0, 5, 10].into_iter().fold(0, |out, shift| {
        out | f(a >> shift & 0x1F, b >> shift & 0x1F) << shift
    })
}

#[cfg(test)]
mod tests {
    use super::{Blend, Effect};

    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;

    fn blend(control: u16, alpha: u16, brightness: u16) -> Blend {
        let mut blend = Blend::default();
        blend.write(0, control, 0xFFFF);
        blend.write(2, alpha, 0xFFFF);
        blend.write(4, brightness, 0xFFFF);
        blend
    }

    #[test]
    fn alpha_weighs_each_channel_in_sixteenths_and_saturates() {
        let half = blend(0, 8 | 8 << 8, 0);
        assert_eq!(half.alpha(RED, GREEN), 15 | 15 << 5);
        let full = blend(0, 16 | 16 << 8, 0);
        assert_eq!(full.alpha(0x7FFF, 0x7FFF), 0x7FFF);
        let over = blend(0, 31 | 31 << 8, 0);
        assert_eq!(over.alpha(10, 10), 20);
        let top_only = blend(0, 16, 0);
        assert_eq!(top_only.alpha(RED | 0x8000, BLUE), RED);
    }

    #[test]
    fn brighten_and_darken_move_each_channel_by_evy_sixteenths() {
        let quarter = blend(0, 0, 4);
        assert_eq!(quarter.brighten(15), 19 | 7 << 5 | 7 << 10);
        assert_eq!(quarter.darken(15), 12);
        let over = blend(0, 0, 31);
        assert_eq!(over.brighten(RED), 0x7FFF);
        assert_eq!(over.darken(0x7FFF), 0);
    }

    #[test]
    fn bldcnt_picks_the_effect_and_targets() {
        let blend = blend(0b10 << 6 | 1 << 2 | 1 << 13, 0, 0);
        assert_eq!(blend.effect(), Effect::Brighten);
        assert!(blend.first(2) && !blend.first(0));
        assert!(blend.second(5) && !blend.second(2));
    }
}
