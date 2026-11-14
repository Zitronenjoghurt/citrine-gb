use super::compose::{BRIGHTEN_BIAS, DARKEN_BIAS};
use super::{HEIGHT, Line3d, MASTER_DARKEN_BIAS, WIDTH, color};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Mix {
    #[default]
    Native,
    Over,
    Blend,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Shade {
    #[default]
    None,
    Brighten(u8),
    Darken(u8),
}

impl Shade {
    pub fn code(self) -> u32 {
        match self {
            Shade::None => 0,
            Shade::Brighten(factor) => 1 | u32::from(factor) << 2,
            Shade::Darken(factor) => 2 | u32::from(factor) << 2,
        }
    }

    fn apply(self, pixel: u32, brighten_bias: u32, darken_bias: u32) -> u32 {
        match self {
            Shade::None => pixel,
            Shade::Brighten(factor) => color::brighten(pixel, factor.into(), brighten_bias),
            Shade::Darken(factor) => color::darken(pixel, factor.into(), darken_bias),
        }
    }

    pub fn fills(self) -> bool {
        match self {
            Shade::None => false,
            Shade::Brighten(factor) | Shade::Darken(factor) => {
                u32::from(factor) >= color::FULL_SHADE
            }
        }
    }

    pub fn master(self, pixel: u32) -> u32 {
        self.apply(pixel, 0, MASTER_DARKEN_BIAS)
    }

    pub fn master_line(self, line: &mut [u32; WIDTH]) {
        match self {
            Shade::None | Shade::Brighten(0) | Shade::Darken(0) => {}
            Shade::Brighten(factor) => color::brighten_line(line, factor.into()),
            Shade::Darken(factor) => color::darken_line(line, factor.into(), MASTER_DARKEN_BIAS),
        }
    }

    pub fn effect(self, pixel: u32) -> u32 {
        self.apply(pixel, BRIGHTEN_BIAS, DARKEN_BIAS)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Layered {
    pub native: u32,
    pub under: u32,
    pub below: u32,
    pub mix: Mix,
    pub shade: Shade,
}

impl Layered {
    pub fn finish(&self, three_d: Option<(u32, u8)>, master: Shade) -> u32 {
        let pixel = match (self.mix, three_d) {
            (Mix::Native, _) => return self.native,
            (_, None) => self.under,
            (Mix::Over, Some((pixel, _))) => self.shade.effect(pixel),
            (Mix::Blend, Some((pixel, alpha))) => {
                color::blend_3d(pixel, self.below, u32::from(alpha))
            }
        };
        master.master(pixel)
    }
}

const SCROLL_MASK: usize = 0x1FF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    Swap,
    Capture,
}

#[derive(Debug, Clone)]
pub struct Composite {
    pixels: Box<[Layered]>,
    scrolls: Box<[u16]>,
    masters: Box<[Shade]>,
    on_top: bool,
    fallback: Option<Fallback>,
    shows_3d: bool,
}

impl Default for Composite {
    fn default() -> Self {
        Self {
            pixels: vec![Layered::default(); WIDTH * HEIGHT].into_boxed_slice(),
            scrolls: vec![0; HEIGHT].into_boxed_slice(),
            masters: vec![Shade::None; HEIGHT].into_boxed_slice(),
            on_top: false,
            fallback: None,
            shows_3d: false,
        }
    }
}

impl Composite {
    pub fn pixels(&self) -> &[Layered] {
        &self.pixels
    }

    pub fn on_top(&self) -> bool {
        self.on_top
    }

    pub fn scroll(&self, y: usize) -> u16 {
        self.scrolls[y]
    }

    pub fn master(&self, y: usize) -> Shade {
        self.masters[y]
    }

    pub fn fallback(&self) -> Option<Fallback> {
        self.fallback
    }

    pub fn shows_3d(&self) -> bool {
        self.shows_3d
    }

    pub fn upscalable(&self) -> bool {
        self.fallback.is_none() && self.shows_3d
    }

    pub(crate) fn start_line(&mut self, y: usize, on_top: bool) -> &mut [Layered] {
        if y == 0 {
            self.on_top = on_top;
            self.fallback = None;
            self.shows_3d = false;
        } else if self.on_top != on_top {
            self.fallback = self.fallback.or(Some(Fallback::Swap));
        }
        self.line_mut(y)
    }

    pub(crate) fn line_mut(&mut self, y: usize) -> &mut [Layered] {
        &mut self.pixels[y * WIDTH..(y + 1) * WIDTH]
    }

    pub(crate) fn fill_3d(&mut self, y: usize, line: &Line3d, row: &mut [u32]) {
        let scroll = usize::from(self.scrolls[y]);
        let master = self.masters[y];
        for (x, (pixel, out)) in self.line_mut(y).iter_mut().zip(row).enumerate() {
            if pixel.mix == Mix::Native {
                continue;
            }
            let from = (x + scroll) & SCROLL_MASK;
            let three_d = line
                .alpha
                .get(from)
                .filter(|&&alpha| alpha != 0)
                .map(|&alpha| (line.color[from], alpha));
            pixel.native = pixel.finish(three_d, master);
            *out = pixel.native;
        }
    }

    pub(crate) fn needs_3d(&self, y: usize) -> bool {
        self.pixels[y * WIDTH..(y + 1) * WIDTH]
            .iter()
            .any(|pixel| pixel.mix != Mix::Native)
    }

    pub(crate) fn end_line(&mut self, y: usize, scroll: Result<u16, Fallback>, master: Shade) {
        self.masters[y] = master;
        match scroll {
            Ok(scroll) => self.scrolls[y] = scroll,
            Err(reason) => self.fallback = self.fallback.or(Some(reason)),
        }
        self.shows_3d |= self.pixels[y * WIDTH..(y + 1) * WIDTH]
            .iter()
            .any(|pixel| pixel.mix != Mix::Native);
    }
}
