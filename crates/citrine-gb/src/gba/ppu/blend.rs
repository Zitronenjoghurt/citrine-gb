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
        let eva = (self.alpha & 0x1F).min(MAX_WEIGHT);
        let evb = (self.alpha >> 8 & 0x1F).min(MAX_WEIGHT);
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

    fn evy(&self) -> u16 {
        self.brightness.min(MAX_WEIGHT)
    }
}

fn map_channels(a: u16, b: u16, f: impl Fn(u16, u16) -> u16) -> u16 {
    let a = a & COLOR_MASK;
    let b = b & COLOR_MASK;
    [0, 5, 10].into_iter().fold(0, |out, shift| {
        out | f(a >> shift & 0x1F, b >> shift & 0x1F) << shift
    })
}

#[cfg(test)]
mod tests;
