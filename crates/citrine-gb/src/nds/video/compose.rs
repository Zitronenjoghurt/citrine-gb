use super::color;
use super::composite::{Mix, Shade};
use super::sprite::Sprites;
use super::{OPAQUE, WIDTH};
use crate::ppu2d::blend::{Blend, Effect};
use crate::ppu2d::window::ALL_LAYERS;

const EFFECTS: u8 = 1 << 5;
const OBJ_LAYER: u8 = 4;
const BACKDROP_LAYER: u8 = 5;
const BACKDROP_PRIORITY: u8 = 4;
pub(super) const BRIGHTEN_BIAS: u32 = 8;
pub(super) const DARKEN_BIAS: u32 = 7;
const BITMAP_WEIGHTS: u32 = 16;
const OBJ_BIT: u8 = 1 << OBJ_LAYER;
const LAYER_SHIFT: u32 = 16;
const OPAQUE_3D: u32 = 31;

pub(super) struct ThreeD<'a> {
    pub alpha: &'a [u8; WIDTH],
    pub color: &'a [u32; WIDTH],
}

pub(super) struct Compose<'a> {
    pub order: &'a [usize],
    pub priorities: [u8; 4],
    pub layers: &'a [[u16; WIDTH]; 4],
    pub three_d: Option<ThreeD<'a>>,
    pub masks: &'a [u8; WIDTH],
    pub windowed: bool,
    pub sprites: &'a Sprites,
    pub blend: &'a Blend,
    pub backdrop: u16,
}

impl Compose<'_> {
    pub fn line(&self, effect: Effect, out: &mut [u32; WIDTH]) {
        let shown = self.shown_layers();
        let mut stacked = Stacked::new(self.backdrop);
        if self.shades_or_blends(effect) && self.mixes_any(effect, shown) {
            self.stack::<true>(shown, &mut stacked);
            self.mix_line(&stacked, effect, out);
            return;
        }
        self.stack::<false>(shown, &mut stacked);
        match &self.three_d {
            Some(three_d) => stacked.tops_over_3d(three_d, out),
            None => stacked.widen_tops(out),
        }
    }

    pub fn stacked(&self) -> Stacked {
        let mut stacked = Stacked::new(self.backdrop);
        self.stack::<true>(self.shown_layers(), &mut stacked);
        stacked
    }

    fn shown_layers(&self) -> u8 {
        let backgrounds = self
            .order
            .iter()
            .filter(|&&bg| any_opaque(&self.layers[bg]))
            .fold(0, |bits, &bg| bits | 1 << bg);
        let sprites = if self.sprites.levels != 0 { OBJ_BIT } else { 0 };
        backgrounds | sprites | 1 << BACKDROP_LAYER
    }

    fn mixes_any(&self, effect: Effect, shown: u8) -> bool {
        let semi = if self.sprites.any_semi { OBJ_BIT } else { 0 };
        if (self.special(effect) | semi) & shown == 0 {
            return false;
        }
        !self.windowed || self.masks.iter().fold(0, |any, &mask| any | mask) & EFFECTS != 0
    }

    fn special(&self, effect: Effect) -> u8 {
        let first = match effect {
            Effect::None => 0,
            _ => (0..=BACKDROP_LAYER)
                .filter(|&layer| self.blend.first(layer))
                .fold(0, |bits, layer| bits | 1 << layer),
        };
        if self.three_d.is_some() {
            first | 1
        } else {
            first
        }
    }

    fn stack<const BELOW: bool>(&self, shown: u8, stacked: &mut Stacked) {
        let masks = self.windowed.then_some(self.masks);
        for priority in (0..BACKDROP_PRIORITY).rev() {
            for &bg in self.order.iter().rev() {
                if self.priorities[bg] == priority && shown & 1 << bg != 0 {
                    stacked.push_background::<BELOW>(&self.layers[bg], masks, bg);
                }
            }
            if self.sprites.levels & 1 << priority != 0 {
                stacked.push_sprites::<BELOW>(self.sprites, masks, priority);
            }
        }
    }

    pub fn mix_line(&self, stacked: &Stacked, effect: Effect, out: &mut [u32; WIDTH]) {
        if !self.shades_or_blends(effect) {
            match &self.three_d {
                Some(three_d) => stacked.tops_over_3d(three_d, out),
                None => stacked.widen_tops(out),
            }
            return;
        }
        match (&self.three_d, self.windowed) {
            (Some(three_d), true) => self.mix_into::<true>(stacked, effect, Some(three_d), out),
            (Some(three_d), false) => self.mix_into::<false>(stacked, effect, Some(three_d), out),
            (None, true) => self.mix_into::<true>(stacked, effect, None, out),
            (None, false) => self.mix_into::<false>(stacked, effect, None, out),
        }
    }

    fn shades_or_blends(&self, effect: Effect) -> bool {
        let second = (0..=BACKDROP_LAYER).any(|layer| self.blend.second(layer));
        match effect {
            Effect::Brighten | Effect::Darken => true,
            Effect::Alpha => second,
            Effect::None => second && (self.three_d.is_some() || self.sprites.any_semi),
        }
    }

    #[inline(always)]
    fn mix_into<const WINDOWED: bool>(
        &self,
        stacked: &Stacked,
        effect: Effect,
        three_d: Option<&ThreeD>,
        out: &mut [u32; WIDTH],
    ) {
        match three_d {
            Some(three_d) => stacked.tops_over_3d(three_d, out),
            None => stacked.widen_tops(out),
        }
        let special = self.special(effect);
        for (x, pixel) in out.iter_mut().enumerate() {
            let mask = if WINDOWED { self.masks[x] } else { ALL_LAYERS };
            let layer = split(stacked.top[x]).1;
            let semi = layer == OBJ_LAYER && self.sprites.semi[x];
            if mask & EFFECTS == 0 || !(semi || special >> layer & 1 != 0) {
                continue;
            }
            *pixel = self.mix_with(x, stacked, effect, three_d, mask).0;
        }
    }

    pub fn shows_3d_on_top(&self, x: usize) -> bool {
        let mask = self.masks[x];
        let sprite = mask & (1 << OBJ_LAYER) != 0 && self.sprites.color[x] & OPAQUE != 0;
        for &bg in self.order {
            if mask & (1 << bg) == 0 || (bg != 0 && self.layers[bg][x] & OPAQUE == 0) {
                continue;
            }
            if sprite && self.sprites.priority[x] <= self.priorities[bg] {
                return false;
            }
            return bg == 0;
        }
        false
    }

    pub fn mixes_3d(&self, x: usize, below: u8, effect: Effect) -> (Mix, Shade) {
        if self.masks[x] & EFFECTS == 0 {
            return (Mix::Over, Shade::None);
        }
        if self.blend.second(below) {
            return (Mix::Blend, Shade::None);
        }
        if !self.blend.first(0) {
            return (Mix::Over, Shade::None);
        }
        let evy = self.blend.evy() as u8;
        let shade = match effect {
            Effect::Brighten => Shade::Brighten(evy),
            Effect::Darken => Shade::Darken(evy),
            _ => Shade::None,
        };
        (Mix::Over, shade)
    }

    pub fn mix(&self, x: usize, stacked: &Stacked, effect: Effect) -> (u32, u32, u8) {
        self.mix_with(x, stacked, effect, self.three_d.as_ref(), self.masks[x])
    }

    #[inline(always)]
    fn mix_with(
        &self,
        x: usize,
        stacked: &Stacked,
        effect: Effect,
        three_d: Option<&ThreeD>,
        mask: u8,
    ) -> (u32, u32, u8) {
        let wide = |layer: u8, value: u16| match three_d {
            Some(three_d) if layer == 0 => three_d.color[x],
            _ => color::widen(value),
        };
        let (top, top_layer) = split(stacked.top[x]);
        let semi = top_layer == OBJ_LAYER && self.sprites.semi[x];
        let alpha_3d = match three_d {
            Some(three_d) if top_layer == 0 => Some(u32::from(three_d.alpha[x])),
            _ => None,
        };
        let top = wide(top_layer, top);
        let shaded = effect != Effect::None && self.blend.first(top_layer);
        if mask & EFFECTS == 0 || !(semi || shaded || alpha_3d.is_some()) {
            return (top, top, top_layer);
        }
        let (below, below_layer) = split(stacked.below[x]);
        let second = self.blend.second(below_layer);
        if second && alpha_3d == Some(OPAQUE_3D) {
            return (top, top, top_layer);
        }
        let below = if second { wide(below_layer, below) } else { 0 };
        let (eva, evb) = self.blend.weights();
        let (eva, evb) = (u32::from(eva), u32::from(evb));
        let evy = u32::from(self.blend.evy());
        if let Some(alpha) = alpha_3d.filter(|_| second) {
            return (color::blend_3d(top, below, alpha), top, top_layer);
        }
        let mixed = if semi && second {
            match u32::from(self.sprites.alpha[x]) {
                0 => color::blend(top, below, eva, evb),
                alpha => color::blend(top, below, alpha + 1, BITMAP_WEIGHTS - (alpha + 1)),
            }
        } else if !(semi || self.blend.first(top_layer)) {
            top
        } else {
            match effect {
                Effect::Alpha if second => color::blend(top, below, eva, evb),
                Effect::Brighten => color::brighten(top, evy, BRIGHTEN_BIAS),
                Effect::Darken => color::darken(top, evy, DARKEN_BIAS),
                _ => top,
            }
        };
        (mixed, top, top_layer)
    }
}

pub(super) struct Stacked {
    top: [u32; WIDTH],
    below: [u32; WIDTH],
}

impl Stacked {
    fn new(backdrop: u16) -> Self {
        let backdrop = entry(backdrop, BACKDROP_LAYER);
        Self {
            top: [backdrop; WIDTH],
            below: [backdrop; WIDTH],
        }
    }

    fn widen_tops(&self, out: &mut [u32; WIDTH]) {
        for (pixel, &top) in out.iter_mut().zip(&self.top) {
            *pixel = color::widen(top as u16);
        }
    }

    fn tops_over_3d(&self, three_d: &ThreeD, out: &mut [u32; WIDTH]) {
        for ((pixel, &top), &color) in out.iter_mut().zip(&self.top).zip(three_d.color) {
            let (value, layer) = split(top);
            *pixel = if layer == 0 {
                color
            } else {
                color::widen(value)
            };
        }
    }

    fn push_background<const BELOW: bool>(
        &mut self,
        values: &[u16; WIDTH],
        masks: Option<&[u8; WIDTH]>,
        bg: usize,
    ) {
        let layer = entry(0, bg as u8);
        let slots = self.top.iter_mut().zip(&mut self.below);
        let Some(masks) = masks else {
            for ((top, below), &value) in slots.zip(values) {
                push::<BELOW>(top, below, value & OPAQUE != 0, u32::from(value) | layer);
            }
            return;
        };
        let bit = 1 << bg;
        for ((top, below), (&value, &mask)) in slots.zip(values.iter().zip(masks)) {
            let shown = (mask & bit != 0) & (value & OPAQUE != 0);
            push::<BELOW>(top, below, shown, u32::from(value) | layer);
        }
    }

    fn push_sprites<const BELOW: bool>(
        &mut self,
        sprites: &Sprites,
        masks: Option<&[u8; WIDTH]>,
        priority: u8,
    ) {
        let layer = entry(0, OBJ_LAYER);
        let slots = self.top.iter_mut().zip(&mut self.below);
        let pixels = sprites.color.iter().zip(&sprites.priority);
        let Some(masks) = masks else {
            for ((top, below), (&color, &level)) in slots.zip(pixels) {
                let shown = (color & OPAQUE != 0) & (level == priority);
                push::<BELOW>(top, below, shown, u32::from(color) | layer);
            }
            return;
        };
        for ((top, below), ((&color, &level), &mask)) in slots.zip(pixels.zip(masks)) {
            let shown = (mask & OBJ_BIT != 0) & (color & OPAQUE != 0) & (level == priority);
            push::<BELOW>(top, below, shown, u32::from(color) | layer);
        }
    }
}

#[inline(always)]
fn push<const BELOW: bool>(top: &mut u32, below: &mut u32, shown: bool, value: u32) {
    let old = *top;
    if BELOW {
        *below = if shown { old } else { *below };
    }
    *top = if shown { value } else { old };
}

fn any_opaque(values: &[u16; WIDTH]) -> bool {
    values.iter().fold(0, |any, &value| any | value) & OPAQUE != 0
}

fn entry(value: u16, layer: u8) -> u32 {
    u32::from(value) | u32::from(layer) << LAYER_SHIFT
}

fn split(entry: u32) -> (u16, u8) {
    (entry as u16, (entry >> LAYER_SHIFT) as u8)
}

#[cfg(test)]
mod tests {
    use super::super::tests::Scene;
    use super::super::{EngineKind, HEIGHT, Layered, Line3d, Mix, Shade, WIDTH, color};

    const DISPCNT: usize = 0x00;
    const BG0CNT: usize = 0x08;
    const BG0HOFS: usize = 0x10;
    const BG1CNT: usize = 0x0A;
    const WIN0H: usize = 0x40;
    const WIN0V: usize = 0x44;
    const WININ: usize = 0x48;
    const WINOUT: usize = 0x4A;
    const BLDCNT: usize = 0x50;
    const BLDALPHA: usize = 0x52;
    const BLDY: usize = 0x54;
    const MASTER_BRIGHT: usize = 0x6C;
    const GRAPHICS: u32 = 1 << 16;
    const THREE_D: u32 = 1 << 3;
    const BG0: u32 = 1 << 8;
    const BG1: u32 = 1 << 9;
    const OBJ: u32 = 1 << 12;
    const WIN0: u32 = 1 << 13;
    const OBJ_WINDOW: u32 = 1 << 15;
    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;
    const BLUE: u16 = 0x7C00;
    const PURPLE: u16 = 15 | 15 << 10;

    fn layers(dispcnt: u32) -> Scene {
        let mut scene = Scene::new(EngineKind::A);
        scene.write32(DISPCNT, GRAPHICS | dispcnt);
        scene.vram.bg[32..64].fill(0x11);
        scene.vram.bg[64..96].fill(0x22);
        for cell in 0..1024 {
            scene.vram.put_bg16(0x800 + cell * 2, 1);
            scene.vram.put_bg16(0x1000 + cell * 2, 2);
        }
        scene.write(BG0CNT, 1 << 8);
        scene.write(BG1CNT, 1 | 2 << 8);
        scene.color(0, GREEN);
        scene.color(1, RED);
        scene.color(2, BLUE);
        scene
    }

    fn sprite(scene: &mut Scene, mode: u16) {
        for index in 1..128 {
            scene.oam[index * 8..index * 8 + 2].copy_from_slice(&(1u16 << 9).to_le_bytes());
        }
        scene.oam[..6].copy_from_slice(&[0, (mode << 2) as u8, 8, 0, 1, 0]);
        scene.vram.obj[32..64].fill(0x11);
        scene.color(0x101, GREEN);
    }

    #[test]
    fn window_0_shows_its_own_layers_inside_and_winout_outside() {
        let mut scene = layers(BG0 | BG1 | WIN0);
        scene.write(WIN0H, 4 << 8 | 8);
        scene.write(WIN0V, 10);
        scene.write(WININ, 0b10);
        scene.write(WINOUT, 0b01);
        let out = scene.line(0);
        assert_eq!([out[3], out[4], out[7], out[8]], [RED, BLUE, BLUE, RED]);
        assert_eq!(scene.line(10)[4], RED);
        assert_eq!(scene.engine.read(WININ), 0b10);
        assert_eq!(scene.engine.read(WINOUT), 0b01);
    }

    #[test]
    fn x2_of_0_reaches_the_right_edge_and_x1_x2_of_0_hide_the_window() {
        let mut scene = layers(BG0 | BG1 | WIN0);
        scene.write(WIN0V, 192);
        scene.write(WININ, 0b10);
        scene.write(WINOUT, 0b01);
        scene.write(WIN0H, 200 << 8);
        let out = scene.line(0);
        assert_eq!([out[199], out[200], out[WIDTH - 1]], [RED, BLUE, BLUE]);
        scene.write(WIN0H, 0);
        assert_eq!(scene.line(0), [RED; WIDTH]);
    }

    #[test]
    fn the_vertical_compare_sees_the_low_8_bits_of_lines_past_255() {
        let mut scene = layers(BG0 | BG1 | WIN0);
        scene.write(WIN0H, 1 << 8);
        scene.write(WIN0V, 2 << 8 | 100);
        scene.write(WININ, 0b10);
        scene.write(WINOUT, 0b01);
        for line in HEIGHT as u16..263 {
            scene.engine.start_line(line);
        }
        scene.engine.start_line(0);
        assert_eq!(scene.render(0)[1], BLUE);
    }

    #[test]
    fn the_obj_window_uses_the_high_byte_of_winout() {
        let mut scene = layers(BG0 | BG1 | OBJ | OBJ_WINDOW);
        sprite(&mut scene, 2);
        scene.write(WINOUT, 0b01 | 0b10 << 8);
        let out = scene.line(0);
        assert_eq!([out[7], out[8], out[15], out[16]], [RED, BLUE, BLUE, RED]);
    }

    #[test]
    fn alpha_blends_a_first_target_over_a_second_target() {
        let mut scene = layers(BG0 | BG1);
        scene.write(BLDCNT, 1 | 1 << 6 | 0b10 << 8);
        scene.write(BLDALPHA, 8 | 8 << 8);
        assert_eq!(scene.line(0)[0], PURPLE);
        assert_eq!(scene.engine.read(BLDCNT), 1 | 1 << 6 | 0b10 << 8);
        assert_eq!(scene.engine.read(BLDALPHA), 8 | 8 << 8);
    }

    #[test]
    fn brightness_moves_first_targets_and_bldy_is_write_only() {
        let mut scene = layers(BG0);
        scene.write(BLDCNT, 1 | 2 << 6);
        scene.write(BLDY, 8);
        assert_eq!(scene.line(0)[0], 0x1F | 16 << 5 | 16 << 10);
        scene.write(BLDCNT, 1 | 3 << 6);
        assert_eq!(scene.line(0)[0], 15);
        assert_eq!(scene.engine.read(BLDY), 0);
    }

    #[test]
    fn the_window_effect_bit_turns_blending_off() {
        let mut scene = layers(BG0 | BG1 | WIN0);
        scene.write(BLDCNT, 1 | 1 << 6 | 0b10 << 8);
        scene.write(BLDALPHA, 8 | 8 << 8);
        scene.write(WIN0H, 4 << 8 | 8);
        scene.write(WIN0V, 192);
        scene.write(WININ, 0b11);
        scene.write(WINOUT, 0b10_0011);
        let out = scene.line(0);
        assert_eq!([out[3], out[4]], [PURPLE, RED]);
    }

    #[test]
    fn a_semi_transparent_sprite_blends_whatever_the_effect() {
        let mut scene = layers(BG0 | OBJ);
        sprite(&mut scene, 1);
        scene.color(0x101, BLUE);
        scene.write(BLDCNT, 1 << 8);
        scene.write(BLDALPHA, 8 | 8 << 8);
        let out = scene.line(0);
        assert_eq!([out[7], out[8]], [RED, PURPLE]);
    }

    #[test]
    fn the_3d_layer_blends_by_its_own_alpha_over_a_second_target() {
        let mut scene = layers(BG0 | BG1 | THREE_D);
        let mut line = Line3d::default();
        line.color.fill(color::widen(RED));
        line.alpha.fill(15);
        line.alpha[1] = 31;
        scene.three_d = Some(line);
        scene.write(BLDCNT, 0b10 << 8);
        let out = scene.line(0);
        assert_eq!([out[0], out[1]], [PURPLE, RED]);
        scene.write(BLDCNT, 1 | 2 << 6);
        scene.write(BLDY, 16);
        assert_eq!(scene.line(0)[0], 0x7FFF);
    }

    #[test]
    fn a_master_shade_on_a_whole_line_matches_shading_each_pixel() {
        let mut seed = 0x9E37_79B9u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let shades = (0..=16)
            .flat_map(|factor| [Shade::Brighten(factor), Shade::Darken(factor)])
            .chain([Shade::None]);
        for shade in shades {
            let line: [u32; WIDTH] = std::array::from_fn(|x| match x {
                0..64 => x as u32 * 0x1041,
                _ => next() & color::WHITE,
            });
            let mut shaded = line;
            shade.master_line(&mut shaded);
            for (x, (&pixel, &out)) in line.iter().zip(&shaded).enumerate() {
                assert_eq!(out, shade.master(pixel), "{shade:?} x {x}");
            }
        }
    }

    #[test]
    fn master_brightness_moves_the_finished_line_up_or_down() {
        let mut scene = Scene::new(EngineKind::A);
        let mut line = [0u32; WIDTH];
        line[1] = color::WHITE;
        scene.write(MASTER_BRIGHT, 1 << 14 | 8);
        let mut up = line;
        scene.engine.master_brightness(&mut up);
        assert_eq!(
            [up[0], up[1]],
            [color::from_channels(31, 31, 31), color::WHITE]
        );
        scene.write(MASTER_BRIGHT, 2 << 14 | 20);
        let mut down = line;
        scene.engine.master_brightness(&mut down);
        assert_eq!([down[0], down[1]], [0, 0]);
        scene.write(MASTER_BRIGHT, 2 << 14 | 8);
        let mut down = line;
        scene.engine.master_brightness(&mut down);
        assert_eq!(down[1], color::from_channels(31, 31, 31));
        scene.write(MASTER_BRIGHT, 3 << 14 | 8);
        let mut reserved = line;
        scene.engine.master_brightness(&mut reserved);
        assert_eq!(reserved, line);
        assert_eq!(scene.engine.read(MASTER_BRIGHT), 3 << 14 | 8);
    }

    fn layered(scene: &mut Scene) -> (u16, Vec<Layered>) {
        let mut out = scene.line_wide(0);
        scene.engine.master_brightness(&mut out);
        let mut into = vec![Layered::default(); WIDTH];
        let scroll = scene.engine.layered_line(&scene.palette, &out, &mut into);
        (scroll, into)
    }

    fn translucent_red() -> Scene {
        let mut scene = layers(BG0 | BG1 | THREE_D);
        let mut line = Line3d::default();
        line.color.fill(color::widen(RED));
        line.alpha[1] = 31;
        line.alpha[2] = 15;
        scene.three_d = Some(line);
        scene
    }

    #[test]
    fn the_3d_layer_records_what_shows_under_it() {
        let mut scene = translucent_red();
        let (scroll, pixels) = layered(&mut scene);
        assert_eq!(scroll, 0);
        let at = |x: usize| (pixels[x].native, pixels[x].under, pixels[x].mix);
        let [red, blue] = [RED, BLUE].map(color::widen);
        assert_eq!(at(0), (blue, blue, Mix::Over));
        assert_eq!(at(1), (red, blue, Mix::Over));
        assert_eq!(at(2), (red, blue, Mix::Over));
    }

    #[test]
    fn a_second_target_under_the_3d_layer_blends() {
        let mut scene = translucent_red();
        scene.write(BLDCNT, 0b10 << 8);
        let (scroll, pixels) = layered(&mut scene);
        assert_eq!(scroll, 0);
        assert_eq!(pixels[2].mix, Mix::Blend);
        assert_eq!(pixels[2].below, color::widen(BLUE));
        assert_eq!(pixels[2].native, scene.line_wide(0)[2]);
    }

    #[test]
    fn a_layer_above_the_3d_layer_keeps_its_native_pixels() {
        let mut scene = translucent_red();
        scene.write(BG0CNT, 1);
        scene.write(BG1CNT, 2 << 8);
        let (scroll, pixels) = layered(&mut scene);
        assert_eq!(scroll, 0);
        assert!(pixels.iter().all(|pixel| pixel.mix == Mix::Native));
    }

    fn shaded_3d() -> Scene {
        let mut scene = translucent_red();
        let mut line = Line3d::default();
        for x in 0..WIDTH {
            line.color[x] = color::from_channels(x as u8, (x * 3) as u8, (x * 7) as u8);
            line.alpha[x] = (x % 32) as u8;
        }
        scene.three_d = Some(line);
        scene
    }

    fn finishes_like_the_screen(scene: &mut Scene) {
        let mut out = scene.line_wide(0);
        scene.engine.master_brightness(&mut out);
        let (_, pixels) = layered(scene);
        let line = scene.three_d.clone().expect("a 3d line");
        let master = scene.engine.master_shade();
        assert!(pixels.iter().any(|pixel| pixel.mix != Mix::Native));
        for (x, pixel) in pixels.iter().enumerate() {
            let three_d = (line.alpha[x] != 0).then_some((line.color[x], line.alpha[x]));
            assert_eq!(
                pixel.finish(three_d, master),
                out[x],
                "pixel {x}, {pixel:?}"
            );
        }
    }

    #[test]
    fn brightness_on_3d_is_followed_per_pixel() {
        for (effect, evy) in [(2, 8), (3, 8), (2, 16), (3, 3)] {
            let mut scene = shaded_3d();
            scene.write(BLDCNT, 1 | effect << 6);
            scene.write(BLDY, evy);
            let (scroll, pixels) = layered(&mut scene);
            assert_eq!(scroll, 0);
            assert!(pixels.iter().any(|pixel| pixel.shade != Shade::None));
            finishes_like_the_screen(&mut scene);
        }
    }

    #[test]
    fn master_brightness_is_followed_per_line() {
        for master in [1 << 14 | 4, 1 << 14 | 16, 2 << 14 | 9, 2 << 14 | 31] {
            for bldcnt in [0, 0b10 << 8, 1 | 2 << 6, 1 | 3 << 6] {
                let mut scene = shaded_3d();
                scene.write(BLDCNT, bldcnt);
                scene.write(BLDY, 6);
                scene.write(MASTER_BRIGHT, master);
                assert_ne!(scene.engine.master_shade(), Shade::None);
                finishes_like_the_screen(&mut scene);
            }
        }
    }

    #[test]
    fn scrolled_3d_is_followed_with_its_scroll() {
        let mut scene = translucent_red();
        scene.write(BG0HOFS, 511);
        let (scroll, pixels) = layered(&mut scene);
        assert_eq!(scroll, 511);
        let [red, blue] = [RED, BLUE].map(color::widen);
        assert_eq!(pixels[0].native, blue);
        assert_eq!(pixels[2].native, red);
        assert_eq!(pixels[2].mix, Mix::Over);
    }

    #[test]
    fn the_3d_layer_scrolls_and_wraps_at_every_offset() {
        let mut scene = layers(BG0 | THREE_D);
        let mut line = Line3d::default();
        for x in 0..WIDTH {
            line.color[x] = color::widen(x as u16 & 0x1F);
            line.alpha[x] = if x % 3 == 0 { 0 } else { 31 };
        }
        scene.three_d = Some(line.clone());
        for scroll in [0, 1, 37, 255, 256, 257, 300, 511] {
            scene.write(BG0HOFS, scroll);
            let out = scene.line(0);
            for (x, &pixel) in out.iter().enumerate() {
                let source = (x + usize::from(scroll)) & 0x1FF;
                let expected = if source < WIDTH && line.alpha[source] != 0 {
                    source as u16 & 0x1F
                } else {
                    GREEN
                };
                assert_eq!(pixel, expected, "scroll {scroll} x {x}");
            }
        }
    }

    #[test]
    fn without_3d_every_pixel_is_native() {
        let mut scene = layers(BG0 | BG1);
        let (scroll, pixels) = layered(&mut scene);
        assert_eq!(scroll, 0);
        assert!(pixels.iter().all(|pixel| pixel.mix == Mix::Native));
    }

    fn walked(compose: &super::Compose, x: usize) -> (u32, u32) {
        use super::{BACKDROP_LAYER, OBJ_BIT, OBJ_LAYER, entry};
        let mask = compose.masks[x];
        let mut found = [entry(compose.backdrop, BACKDROP_LAYER); 2];
        let mut seen = 0;
        let color = compose.sprites.color[x];
        let mut sprite = mask & OBJ_BIT != 0 && color & super::OPAQUE != 0;
        for &bg in compose.order {
            let value = compose.layers[bg][x];
            if mask & (1 << bg) == 0 || value & super::OPAQUE == 0 {
                continue;
            }
            if sprite && compose.sprites.priority[x] <= compose.priorities[bg] {
                found[seen] = entry(color, OBJ_LAYER);
                seen += 1;
                sprite = false;
                if seen == 2 {
                    break;
                }
            }
            found[seen] = entry(value, bg as u8);
            seen += 1;
            if seen == 2 {
                break;
            }
        }
        if sprite && seen < 2 {
            found[seen] = entry(color, OBJ_LAYER);
        }
        (found[0], found[1])
    }

    #[test]
    fn stacking_a_line_matches_walking_down_the_layer_order_and_mixing_matches_per_pixel() {
        use super::super::sprite::Sprites;
        use super::{Compose, OPAQUE, ThreeD};
        use crate::ppu2d::blend::Blend;
        let mut seed = 0x2545_F491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..300 {
            let mut layers = [[0u16; WIDTH]; 4];
            layers
                .iter_mut()
                .flatten()
                .for_each(|value| *value = next() as u16);
            for layer in &mut layers {
                if next() % 4 == 0 {
                    layer.fill(0);
                }
            }
            let mut masks = [0u8; WIDTH];
            let open = next() % 2 == 0;
            let effects = if next() % 4 == 0 { 0x1F } else { 0x3F };
            masks
                .iter_mut()
                .for_each(|mask| *mask = if open { 0x3F } else { next() as u8 & effects });
            let mut sprites = Sprites::default();
            let sprite_colors = if next() % 4 == 0 { 0x7FFF } else { 0xFFFF };
            for (color, priority) in sprites.color.iter_mut().zip(&mut sprites.priority) {
                *color = next() as u16 & sprite_colors;
                *priority = next() as u8;
                if *color & OPAQUE != 0 {
                    *priority %= 4;
                    sprites.levels |= 1 << *priority;
                }
            }
            let priorities = [0; 4].map(|_: u8| (next() % 4) as u8);
            let mut order: Vec<usize> = (0..4).filter(|_| next() % 3 != 0).collect();
            order.sort_by_key(|&bg| (priorities[bg], bg));
            let mut blend = Blend::default();
            for offset in [0, 2, 4] {
                blend.write(offset, next() as u16, 0xFFFF);
            }
            if next() % 3 == 0 {
                blend.write(0, next() as u16 & 0xFF, 0xFFFF);
            }
            for (semi, alpha) in sprites.semi.iter_mut().zip(&mut sprites.alpha) {
                *semi = next() % 4 == 0;
                *alpha = next() as u8 & 0xF;
            }
            sprites.any_semi = next() % 2 == 0;
            if !sprites.any_semi {
                sprites.semi.fill(false);
            }
            let mut line = Line3d::default();
            for (color, alpha) in line.color.iter_mut().zip(&mut line.alpha) {
                *color = next() & 0x3_FFFF;
                *alpha = next() as u8 & 0x1F;
            }
            let compose = Compose {
                order: &order,
                priorities,
                layers: &layers,
                three_d: (next() % 2 == 0).then_some(ThreeD {
                    alpha: &line.alpha,
                    color: &line.color,
                }),
                masks: &masks,
                windowed: !open,
                sprites: &sprites,
                blend: &blend,
                backdrop: next() as u16 & 0x7FFF,
            };
            let stacked = compose.stacked();
            for (x, pair) in stacked.top.iter().zip(&stacked.below).enumerate() {
                assert_eq!(
                    (*pair.0, *pair.1),
                    walked(&compose, x),
                    "x {x} order {order:?}"
                );
            }
            let effect = blend.effect();
            let mut mixed = [0; WIDTH];
            compose.mix_line(&stacked, effect, &mut mixed);
            for (x, &pixel) in mixed.iter().enumerate() {
                assert_eq!(pixel, compose.mix(x, &stacked, effect).0, "x {x}");
            }
            let mut lined = [0; WIDTH];
            compose.line(effect, &mut lined);
            assert_eq!(lined, mixed);
        }
    }
}
