use crate::gb::ppu::Ppu;
use crate::gb::ppu::types::color::{ColorCorrection, DmgColors, RGBA};
use crate::gb::ppu::types::theme::DmgTheme;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct ColorKey {
    theme: DmgTheme,
    colors: DmgColors,
    correction: ColorCorrection,
    cgb_mode: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ColorCache {
    key: Option<ColorKey>,
    bg: [RGBA; 32],
    obj: [RGBA; 32],
    bg_pairs: [u64; 128],
}

impl Default for ColorCache {
    fn default() -> Self {
        Self {
            key: None,
            bg: [RGBA::default(); 32],
            obj: [RGBA::default(); 32],
            bg_pairs: [0; 128],
        }
    }
}

impl ColorCache {
    #[inline(always)]
    pub(super) fn bg(&self, palette: u8, color_index: u8) -> RGBA {
        self.bg[((palette & 7) * 4 + (color_index & 3)) as usize]
    }

    #[inline(always)]
    pub(super) fn bg_pair(&self, palette: u8, pair: u8) -> u64 {
        self.bg_pairs[((palette & 7) * 16 + (pair & 15)) as usize]
    }

    #[inline(always)]
    pub(super) fn bg_span(&self, palette: u8, indices: u64) -> [u64; 4] {
        let pairs = indices | (indices >> 6);
        std::array::from_fn(|i| self.bg_pair(palette, (pairs >> (i * 16)) as u8))
    }

    fn refresh_bg_pairs(&mut self, palette: usize) {
        for pair in 0..16 {
            self.bg_pairs[palette * 16 + pair] =
                self.bg[palette * 4 + (pair & 3)].pair(self.bg[palette * 4 + (pair >> 2)]);
        }
    }

    #[inline(always)]
    pub(super) fn obj(&self, palette: u8, color_index: u8) -> RGBA {
        self.obj[((palette & 7) * 4 + (color_index & 3)) as usize]
    }

    pub(super) fn invalidate(&mut self) {
        self.key = None;
    }
}

impl Ppu {
    fn color_key(&self) -> ColorKey {
        ColorKey {
            theme: self.dmg_theme,
            colors: self.dmg_colors,
            correction: self.color_correction,
            cgb_mode: self.cgb_mode,
        }
    }

    #[inline(always)]
    pub(super) fn check_colors(&mut self) {
        let key = self.color_key();
        if self.colors.key != Some(key) {
            self.rebuild_colors(key);
        }
    }

    #[inline(never)]
    fn rebuild_colors(&mut self, key: ColorKey) {
        self.refresh_bg_colors();
        self.refresh_obj_colors();
        self.colors.key = Some(key);
    }

    fn refresh_bg_colors(&mut self) {
        self.colors.bg =
            std::array::from_fn(|i| self.apply_bg_palette((i / 4) as u8, (i % 4) as u8));
        for palette in 0..8 {
            self.colors.refresh_bg_pairs(palette);
        }
    }

    fn refresh_obj_colors(&mut self) {
        self.colors.obj =
            std::array::from_fn(|i| self.apply_sprite_palette((i / 4) as u8, (i % 4) as u8));
    }

    pub(super) fn dmg_palette_written(&mut self, obj: bool) {
        if self.cgb_mode {
            return;
        }
        if obj {
            self.refresh_obj_colors();
        } else {
            self.refresh_bg_colors();
        }
    }

    pub(super) fn palette_ram_written(&mut self, obj: bool, index: usize) {
        if !self.cgb_mode {
            if obj {
                self.refresh_obj_colors();
            } else {
                self.refresh_bg_colors();
            }
            return;
        }
        let entry = index / 2;
        let (palette, color_index) = ((entry / 4) as u8, (entry % 4) as u8);
        if obj {
            self.colors.obj[entry] = self.apply_sprite_palette(palette, color_index);
        } else {
            self.colors.bg[entry] = self.apply_bg_palette(palette, color_index);
            self.colors.refresh_bg_pairs(palette as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WriteMemory;
    use crate::gb::GbModel;

    fn assert_cache_matches(ppu: &Ppu) {
        for palette in 0..8 {
            for color_index in 0..4 {
                assert_eq!(
                    ppu.colors.bg(palette, color_index),
                    ppu.apply_bg_palette(palette, color_index)
                );
                assert_eq!(
                    ppu.colors.obj(palette, color_index),
                    ppu.apply_sprite_palette(palette, color_index)
                );
            }
            for pair in 0..16 {
                assert_eq!(
                    ppu.colors.bg_pair(palette, pair),
                    ppu.apply_bg_palette(palette, pair & 3)
                        .pair(ppu.apply_bg_palette(palette, pair >> 2))
                );
            }
        }
    }

    fn scribble(ppu: &mut Ppu, seed: u32) {
        let mut x = seed;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        };
        for _ in 0..200 {
            let addr =
                [0xFF47, 0xFF48, 0xFF49, 0xFF68, 0xFF69, 0xFF6A, 0xFF6B][next() as usize % 7];
            let value = next();
            ppu.write_naive(addr, value);
            assert_cache_matches(ppu);
        }
    }

    #[test]
    fn colour_cache_follows_palette_writes() {
        for model in [GbModel::Dmg, GbModel::Cgb] {
            let mut ppu = Ppu::new(model);
            ppu.check_colors();
            assert_cache_matches(&ppu);
            scribble(&mut ppu, 0x2545_F491);
            ppu.set_cgb_mode(false);
            ppu.check_colors();
            assert_cache_matches(&ppu);
            scribble(&mut ppu, 0x9E37_79B9);
        }
    }

    #[test]
    fn colour_cache_follows_settings() {
        let mut ppu = Ppu::new(GbModel::Cgb);
        ppu.check_colors();
        scribble(&mut ppu, 0x1234_5678);
        ppu.color_correction = ColorCorrection::Screen;
        ppu.check_colors();
        assert_cache_matches(&ppu);
        ppu.set_cgb_mode(false);
        ppu.dmg_colors = DmgColors::Theme;
        ppu.dmg_theme = DmgTheme::Pocket;
        ppu.check_colors();
        assert_cache_matches(&ppu);
        ppu.colors.invalidate();
        ppu.write_naive(0xFF47, 0x1B);
        ppu.check_colors();
        assert_cache_matches(&ppu);
    }
}
