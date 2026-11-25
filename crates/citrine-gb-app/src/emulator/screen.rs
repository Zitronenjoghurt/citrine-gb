use super::layout::{ScreenLayout, ScreenSpots};
use super::{Emulator, runner};
use crate::app::ui_state::settings::ScalingMode;
use citrine_gb::console::{Console, Touch};
use citrine_gb_replay::Input;
use egui::{Color32, ColorImage};
use std::sync::Arc;

const POINTER_REST_SECS: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Shown {
    pub(super) scale: usize,
    pub(super) size: egui::Vec2,
    pub(super) linear: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Output {
    pub(super) scaling: ScalingMode,
    pub(super) matrix: bool,
    pub(super) same_size_pixels: bool,
}

impl Output {
    fn same_size(self) -> bool {
        self.same_size_pixels && self.scaling != ScalingMode::Integer
    }

    pub(super) fn mode(self) -> ScalingMode {
        if self.matrix && !self.same_size() {
            ScalingMode::Integer
        } else {
            self.scaling
        }
    }

    pub(super) fn show(self, screens: ScreenLayout, available: egui::Vec2) -> Shown {
        let mode = self.mode();
        let (scale, size) = fit_screen(screens, available, mode);
        if self.same_size() {
            return Shown {
                scale: prescale(screens, size),
                size,
                linear: true,
            };
        }
        Shown {
            scale,
            size,
            linear: false,
        }
    }
}

pub(super) fn draws_grid(matrix: bool, scale: usize) -> bool {
    matrix && scale >= 2
}

fn prescale(screens: ScreenLayout, shown: egui::Vec2) -> usize {
    let frame = screens.size();
    let whole = (shown.x / frame.x).min(shown.y / frame.y) + 0.01;
    whole.floor().max(1.0) as usize
}

impl Emulator {
    fn output(&self) -> Output {
        Output {
            scaling: self.scaling,
            matrix: self.enable_matrix,
            same_size_pixels: self.same_size_pixels,
        }
    }

    fn fit(
        &mut self,
        ctx: &egui::Context,
        available: egui::Vec2,
        only: Option<usize>,
    ) -> (ScreenLayout, egui::Vec2) {
        let output = self.output();
        let layout = arranged(self.layout_of(only), available, output.mode());
        if only.is_none() {
            self.shown_layout = Some(layout);
        }
        let shown = output.show(layout, available);
        let second = only.is_some_and(|index| index > 0);
        if !second && (self.render_scale != shown.scale || self.linear != shown.linear) {
            self.render_scale = shown.scale;
            self.linear = shown.linear;
            self.update_texture(ctx);
        }
        (layout, shown.size)
    }

    fn sampled(&self, layout: ScreenLayout) -> ScreenLayout {
        if self.linear {
            layout.sampled_at(self.render_scale)
        } else {
            layout
        }
    }

    pub fn two_screens(&self) -> bool {
        self.video().screens.len() > 1
    }

    pub fn screen_tabs(&self) -> bool {
        self.two_screens() && self.ds_screens.tabs_shown(self.screen_context)
    }

    pub fn lent_screen(&self) -> Option<usize> {
        self.screen_away.filter(|_| self.two_screens())
    }

    pub fn layout(&self) -> ScreenLayout {
        self.layout_of(None)
    }

    pub fn shown_layout(&self) -> ScreenLayout {
        self.shown_layout
            .filter(|layout| layout.video() == self.video())
            .unwrap_or_else(|| self.layout())
    }

    fn layout_of(&self, only: Option<usize>) -> ScreenLayout {
        let layout = ScreenLayout::new(self.video(), &self.ds_screens, self.screen_context);
        let kept = self.lent_screen().map(|away| usize::from(away == 0));
        match only.or(kept) {
            Some(index) => layout.only(index),
            None => layout,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<egui::Rect> {
        self.ui_screen(ui, None)
    }

    pub fn ui_screen(&mut self, ui: &mut egui::Ui, only: Option<usize>) -> Option<egui::Rect> {
        let available = ui.available_size();
        let (layout, display_size) = self.fit(ui.ctx(), available, only);

        let spots = self.take_spots().filter(|_| only.is_none());
        let tex = self.texture.as_ref()?;
        let leftover_x = (available.x - display_size.x).max(0.0);
        let leftover_y = (available.y - display_size.y).max(0.0);

        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

        ui.add_space((leftover_y / 2.0).floor());
        let layout = self.sampled(layout);
        let shown = ui.horizontal(|ui| {
            ui.add_space((leftover_x / 2.0).floor());
            let (rect, _) = ui.allocate_exact_size(display_size, egui::Sense::hover());
            let layout = match &spots {
                Some(spots) => layout.spotted(rect, spots),
                None => layout,
            };
            layout.paint(ui, tex.id(), self.upscaled_screen(ui.ctx()), rect);
            (rect, layout)
        });
        let (rect, layout) = shown.inner;
        if only.is_none_or(|index| index == 0) {
            self.place_screens(ui.ctx(), layout, rect);
        }
        self.see_touch(layout.point(ui, rect));
        Some(rect)
    }

    fn place_screens(&mut self, ctx: &egui::Context, layout: ScreenLayout, picture: egui::Rect) {
        self.placed_pass = Some(ctx.cumulative_pass_nr());
        self.placed_screens.clear();
        self.placed_screens
            .extend(layout.place(picture).map(|placed| placed.rect));
        self.placed_touch = layout
            .place(picture)
            .find(|placed| placed.screen.touch)
            .map(|placed| placed.rect);
    }

    pub fn placed_screens(&self) -> &[egui::Rect] {
        &self.placed_screens
    }

    pub fn rest_pointer(&self, ctx: &egui::Context) {
        let drawn = self.placed_pass == Some(ctx.cumulative_pass_nr());
        let over = drawn
            && ctx.pointer_hover_pos().is_some_and(|at| {
                ctx.layer_id_at(at)
                    .is_some_and(|layer| layer.order == egui::Order::Background)
                    && self.placed_screens.iter().any(|rect| rect.contains(at))
            });
        let still = ctx.input(|i| i.pointer.time_since_last_movement());
        if self.hide_pointer && self.is_running() && over && still >= POINTER_REST_SECS {
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
    }

    fn take_spots(&mut self) -> Option<ScreenSpots> {
        let spots = self.screen_spots.take();
        #[cfg(all(test, feature = "nds"))]
        {
            self.spotted_room = spots.map(|spots| spots.room);
        }
        spots
    }

    #[cfg(all(test, feature = "nds"))]
    pub fn spotted_room(&self) -> Option<egui::Rect> {
        self.spotted_room
    }

    pub fn placed_touch(&self) -> Option<egui::Rect> {
        self.placed_touch
    }

    fn see_touch(&mut self, seen: Option<Option<Touch>>) {
        if let Some(touch) = seen {
            self.touch_seen = Some(self.touch_seen.flatten().or(touch));
        }
    }

    pub fn take_touch(&mut self) -> Option<Option<Touch>> {
        self.touch_seen.take()
    }

    pub fn set_touch(&mut self, touch: Option<Touch>) {
        self.touch = touch;
        self.touch_owed = true;
        if self.seat.is_home() {
            self.give_touch();
        }
    }

    pub(super) fn give_touch(&mut self) {
        if !std::mem::take(&mut self.touch_owed) {
            return;
        }
        let core = self.seat.core_mut();
        core.feed(Input::Touch(self.touch.map(|touch| (touch.x, touch.y))));
        core.flight.set_touch(self.touch);
    }

    pub fn ui_at(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> egui::Rect {
        self.ui_placed(ui, rect, |size| {
            let centre = rect.center();
            let top_left = egui::pos2(
                (centre.x - size.x / 2.0).round(),
                (centre.y - size.y / 2.0).round(),
            );
            egui::Rect::from_min_size(top_left, size)
        })
    }

    pub fn ui_placed(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        place: impl FnOnce(egui::Vec2) -> egui::Rect,
    ) -> egui::Rect {
        let (layout, display_size) = self.fit(ui.ctx(), area.size(), None);
        let target = place(display_size);
        let layout = self.sampled(layout);
        let layout = match self.take_spots() {
            Some(spots) => layout.spotted(target, &spots),
            None => layout,
        };
        self.place_screens(ui.ctx(), layout, target);
        if let Some(tex) = &self.texture {
            layout.paint(ui, tex.id(), self.upscaled_screen(ui.ctx()), target);
        }
        self.see_touch(layout.point(ui, target));
        target
    }

    pub fn paint_fitted(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> egui::Rect {
        let layout = self.layout().for_area(rect.size());
        self.shown_layout = Some(layout);
        let (_, size) = fit_screen(layout, rect.size(), ScalingMode::Fit);
        let target = egui::Rect::from_center_size(rect.center(), size);
        self.place_screens(ui.ctx(), layout, target);
        if self.texture.is_none() {
            self.update_texture(ui.ctx());
        }
        if let Some(tex) = &self.texture {
            let upscaled = self.upscaled_screen(ui.ctx());
            self.sampled(layout).paint(ui, tex.id(), upscaled, target);
        }
        target
    }

    pub fn screenshot(&mut self, scale: usize) -> (usize, usize, Vec<u8>) {
        self.finish_picture();
        let scale = scale.max(1);
        let source = self.shown_frame();
        self.screenshot_of(source, scale)
    }

    pub fn screens_picture(&mut self) -> Vec<u8> {
        self.finish_picture();
        self.paint_frame(self.shown_frame(), 1)
    }

    fn shown_frame(&self) -> &[u8] {
        let core = self.seat.core();
        if core.screen.painted.is_empty() {
            core.machine.frame()
        } else {
            &core.screen.last_frame
        }
    }

    fn paint_frame(&self, source: &[u8], scale: usize) -> Vec<u8> {
        let mut rgba = Vec::new();
        let backdrop = runner::backdrop(&self.seat.core().machine);
        let effects = runner::Effects {
            matrix: self.enable_matrix,
            scale,
            ..self.effects()
        };
        runner::paint(source, self.video(), backdrop, effects, scale, &mut rgba);
        rgba
    }

    pub fn screenshot_of(&self, source: &[u8], scale: usize) -> (usize, usize, Vec<u8>) {
        self.screenshot_of_screen(source, scale, None)
    }

    pub fn screenshot_of_screen(
        &self,
        source: &[u8],
        scale: usize,
        only: Option<usize>,
    ) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let video = self.video();
        let rgba = self.paint_frame(source, scale);
        if !self.two_screens() {
            return (video.width * scale, video.height * scale, rgba);
        }
        let layout = match only {
            Some(index) => self.shown_layout().only(index),
            None => self.shown_layout(),
        };
        let mut arranged = Vec::new();
        layout.compose(&rgba, scale, &mut arranged);
        let (width, height) = layout.frame_size(scale);
        (width, height, arranged)
    }

    pub fn clear_frame(&mut self, ctx: &egui::Context) {
        if let Some(gb) = self.seat.core_mut().machine.as_gb_mut() {
            gb.ppu.clear_frame();
        }
        self.update_texture(ctx);
    }

    pub(super) fn update_texture(&mut self, ctx: &egui::Context) {
        if self.defers_3d() {
            self.painted_early = true;
        } else {
            self.painted_early = false;
            self.seat.core_mut().machine.finish_picture();
        }
        self.repaint();
        self.upload(ctx);
    }

    fn repaint(&mut self) {
        let effects = self.effects();
        let linked = self
            .linked_frame
            .as_deref()
            .map(|frame| (frame, self.linked_video));
        self.seat.core_mut().paint(effects, linked);
    }

    pub(crate) fn finish_picture(&mut self) {
        let filled = self.seat.core_mut().machine.finish_picture();
        if std::mem::take(&mut self.painted_early) || filled {
            self.repaint();
        }
    }

    pub(super) fn upload(&mut self, ctx: &egui::Context) {
        let screen = &self.seat.core().screen;
        let image = refill(&mut self.screen_image, screen.size, &screen.painted);
        let options = if self.linear {
            egui::TextureOptions::LINEAR
        } else {
            egui::TextureOptions::NEAREST
        };
        match &mut self.texture {
            Some(tex) => tex.set(image, options),
            None => self.texture = Some(ctx.load_texture("gb_screen", image, options)),
        }
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        if self.linked_frame.is_none() {
            self.draw_3d(None);
        }
        #[cfg(feature = "nds")]
        self.sample_load();
    }

    #[cfg(not(all(feature = "nds", not(target_arch = "wasm32"))))]
    fn upscaled_screen(&self, _ctx: &egui::Context) -> Option<(egui::TextureId, usize)> {
        None
    }

    #[cfg(not(all(feature = "nds", not(target_arch = "wasm32"))))]
    pub(super) fn defers_3d(&self) -> bool {
        false
    }
}

fn refill(kept: &mut Option<Arc<ColorImage>>, size: [usize; 2], rgba: &[u8]) -> Arc<ColorImage> {
    let reused = kept.as_mut().and_then(Arc::get_mut);
    match reused.filter(|image| image.size == size) {
        Some(image) => {
            for (pixel, &[r, g, b, a]) in image.pixels.iter_mut().zip(rgba.as_chunks().0) {
                *pixel = Color32::from_rgba_premultiplied(r, g, b, a);
            }
        }
        None => *kept = Some(Arc::new(ColorImage::from_rgba_premultiplied(size, rgba))),
    }
    Arc::clone(kept.as_ref().expect("the image was just filled"))
}

fn arranged(layout: ScreenLayout, available: egui::Vec2, mode: ScalingMode) -> ScreenLayout {
    let layout = layout.for_area(available);
    if mode != ScalingMode::Integer || !layout.big_and_small() {
        return layout;
    }
    let size = layout.size();
    let fit = (available.x / size.x).min(available.y / size.y);
    if fit < 1.0 {
        return layout;
    }
    let scale = fit.floor();
    let small = (scale * layout.share()).floor();
    if small >= 1.0 {
        layout.with_share(small / scale)
    } else {
        layout
    }
}

fn fit_screen(
    layout: impl Into<ScreenLayout>,
    available: egui::Vec2,
    mode: ScalingMode,
) -> (usize, egui::Vec2) {
    let egui::Vec2 {
        x: width,
        y: height,
    } = layout.into().size();
    let fit = (available.x / width).min(available.y / height).max(0.1);

    match mode {
        ScalingMode::Integer if fit < 1.0 => (1, egui::vec2(width * fit, height * fit)),
        ScalingMode::Integer => {
            let scale = fit.floor();
            (scale as usize, egui::vec2(width * scale, height * scale))
        }
        ScalingMode::Fit => (1, egui::vec2(width * fit, height * fit)),
        ScalingMode::Stretch => (1, available),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::console::{ConsoleKind, Screen, VideoSpec};

    #[test]
    fn integer_scaling_picks_the_largest_whole_multiple_for_each_screen() {
        let desktop = egui::vec2(1000.0, 640.0);
        let gb = fit_screen(ConsoleKind::Gb.video(), desktop, ScalingMode::Integer);
        let gba = fit_screen(ConsoleKind::Gba.video(), desktop, ScalingMode::Integer);
        assert_eq!(gb, (4, egui::vec2(640.0, 576.0)));
        assert_eq!(gba, (4, egui::vec2(960.0, 640.0)));

        let phone = egui::vec2(412.0, 400.0);
        let gb = fit_screen(ConsoleKind::Gb.video(), phone, ScalingMode::Integer);
        let gba = fit_screen(ConsoleKind::Gba.video(), phone, ScalingMode::Integer);
        assert_eq!(gb, (2, egui::vec2(320.0, 288.0)));
        assert_eq!(gba, (1, egui::vec2(240.0, 160.0)));
    }

    #[test]
    fn a_gba_screen_fits_wider_than_a_gb_screen() {
        let area = egui::vec2(900.0, 500.0);
        let (_, gb) = fit_screen(ConsoleKind::Gb.video(), area, ScalingMode::Fit);
        let (_, gba) = fit_screen(ConsoleKind::Gba.video(), area, ScalingMode::Fit);
        assert!((gb.y - 500.0).abs() < 0.01 && (gba.y - 500.0).abs() < 0.01);
        assert!((gba.x / gba.y - 1.5).abs() < 0.001);
        assert!((gb.x / gb.y - 160.0 / 144.0).abs() < 0.001);
    }

    #[test]
    fn a_screen_smaller_than_one_pixel_per_dot_shrinks_to_fit() {
        let (scale, size) = fit_screen(
            ConsoleKind::Gba.video(),
            egui::vec2(200.0, 400.0),
            ScalingMode::Integer,
        );
        assert_eq!(scale, 1);
        assert!((size.x - 200.0).abs() < 0.01);
        assert!((size.y - 200.0 / 1.5).abs() < 0.01);
    }

    #[test]
    fn every_scaling_mode_keeps_the_screen_inside_its_pane() {
        let panes = [
            egui::vec2(1000.0, 640.0),
            egui::vec2(853.3, 533.7),
            egui::vec2(200.0, 120.0),
            egui::vec2(150.0, 400.0),
        ];
        for console in [ConsoleKind::Gb, ConsoleKind::Gba] {
            for mode in [ScalingMode::Integer, ScalingMode::Fit, ScalingMode::Stretch] {
                for pane in panes {
                    let (_, size) = fit_screen(console.video(), pane, mode);
                    assert!(
                        size.x <= pane.x && size.y <= pane.y,
                        "{console:?} {mode:?} {size:?} in {pane:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn two_screens_scale_together_in_each_arrangement() {
        use crate::app::ui_state::ds_screens::{Arrangement, DsScreens, ScreenContext};
        const TWO: [Screen; 2] = [
            Screen::whole(256, 192),
            Screen {
                x: 0,
                y: 192,
                width: 256,
                height: 192,
                touch: true,
            },
        ];
        let desktop = egui::vec2(1280.0, 800.0);
        let fit = |arrangement| {
            let mut screens = DsScreens::default();
            screens.desktop.arrangement = arrangement;
            let layout = ScreenLayout::new(VideoSpec::of(&TWO), &screens, ScreenContext::Desktop);
            fit_screen(layout, desktop, ScalingMode::Integer)
        };
        assert_eq!(fit(Arrangement::Stacked), (2, egui::vec2(512.0, 768.0)));
        assert_eq!(fit(Arrangement::SideBySide), (2, egui::vec2(1024.0, 384.0)));
    }

    #[test]
    fn integer_scaling_gives_the_small_screen_its_own_whole_scale() {
        use crate::app::ui_state::ds_screens::{Arrangement, DsScreens, ScreenContext, SmallSize};
        const TWO: [Screen; 2] = [
            Screen::whole(256, 192),
            Screen {
                x: 0,
                y: 192,
                width: 256,
                height: 192,
                touch: true,
            },
        ];
        let area = egui::vec2(1200.0, 600.0);
        let layout = |small| {
            let mut screens = DsScreens::default();
            screens.desktop.arrangement = Arrangement::BigAndSmall;
            screens.small = small;
            ScreenLayout::new(VideoSpec::of(&TWO), &screens, ScreenContext::Desktop)
        };
        let half = arranged(layout(SmallSize::Half), area, ScalingMode::Integer);
        let (scale, size) = fit_screen(half, area, ScalingMode::Integer);
        assert_eq!(scale, 3);
        assert!((size - egui::vec2(1024.0, 576.0)).length() < 0.01);
        let rects: Vec<_> = half
            .place(egui::Rect::from_min_size(egui::Pos2::ZERO, size))
            .map(|p| p.rect.size())
            .collect();
        assert!((rects[0] - egui::vec2(768.0, 576.0)).length() < 0.01);
        assert!((rects[1] - egui::vec2(256.0, 192.0)).length() < 0.01);
        let fit = arranged(layout(SmallSize::Half), area, ScalingMode::Fit);
        assert_eq!(fit.share(), 0.5);
    }

    const GB_DESKTOP: egui::Vec2 = egui::vec2(1000.0, 640.0);

    fn output(scaling: ScalingMode, matrix: bool, same_size_pixels: bool) -> Output {
        Output {
            scaling,
            matrix,
            same_size_pixels,
        }
    }

    fn gb() -> ScreenLayout {
        ConsoleKind::Gb.video().into()
    }

    #[test]
    fn with_same_size_pixels_off_fit_and_stretch_sample_one_texel_per_dot_as_before() {
        let fit = output(ScalingMode::Fit, false, false).show(gb(), GB_DESKTOP);
        assert_eq!((fit.scale, fit.linear), (1, false));
        assert!(
            (fit.size.y - 640.0).abs() < 0.01 && (fit.size.x - 640.0 * 160.0 / 144.0).abs() < 0.01
        );
        let stretch = output(ScalingMode::Stretch, false, false).show(gb(), GB_DESKTOP);
        assert_eq!(
            stretch,
            Shown {
                scale: 1,
                size: GB_DESKTOP,
                linear: false
            }
        );
        let matrix = output(ScalingMode::Fit, true, false).show(gb(), GB_DESKTOP);
        assert_eq!(
            matrix,
            Shown {
                scale: 4,
                size: egui::vec2(640.0, 576.0),
                linear: false
            }
        );
    }

    #[test]
    fn at_scale_one_without_effects_the_texture_is_the_frame_byte_for_byte() {
        const ROW: [Screen; 1] = [Screen::whole(3, 2)];
        let frame: Vec<u8> = (0..24).map(|i| (i * 10) as u8 | 3).collect();
        let mut expected = frame.clone();
        for alpha in expected.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        let mut out = Vec::new();
        runner::paint(
            &frame,
            VideoSpec::of(&ROW),
            [0; 3],
            effects(false),
            1,
            &mut out,
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn same_size_pixels_prescale_by_the_largest_whole_multiple_and_sample_linearly() {
        let fit = output(ScalingMode::Fit, false, true).show(gb(), GB_DESKTOP);
        assert_eq!((fit.scale, fit.linear), (4, true));
        assert!((fit.size.y - 640.0).abs() < 0.01);
        let stretch = output(ScalingMode::Stretch, false, true).show(gb(), GB_DESKTOP);
        assert_eq!(
            stretch,
            Shown {
                scale: 4,
                size: GB_DESKTOP,
                linear: true
            }
        );
        let small = output(ScalingMode::Fit, false, true).show(gb(), egui::vec2(200.0, 150.0));
        assert_eq!((small.scale, small.linear), (1, true));
        let exact = output(ScalingMode::Fit, false, true).show(gb(), egui::vec2(480.0, 431.99));
        assert_eq!(exact.scale, 3);
    }

    #[test]
    fn same_size_pixels_change_nothing_with_integer_scaling() {
        for matrix in [false, true] {
            for pane in [
                GB_DESKTOP,
                egui::vec2(412.0, 400.0),
                egui::vec2(100.0, 90.0),
            ] {
                assert_eq!(
                    output(ScalingMode::Integer, matrix, true).show(gb(), pane),
                    output(ScalingMode::Integer, matrix, false).show(gb(), pane),
                );
            }
        }
    }

    #[test]
    fn the_matrix_keeps_fit_scaling_with_same_size_pixels() {
        let pane = egui::vec2(240.0, 216.0);
        let shown = output(ScalingMode::Fit, true, true).show(gb(), pane);
        assert_eq!(
            shown,
            Shown {
                scale: 1,
                size: pane,
                linear: true
            }
        );
        assert!(!draws_grid(true, shown.scale));
        let large = output(ScalingMode::Fit, true, true).show(gb(), GB_DESKTOP);
        assert_eq!((large.scale, large.linear), (4, true));
        assert!((large.size.y - 640.0).abs() < 0.01);
        assert!(draws_grid(true, large.scale));
        assert!(!draws_grid(true, 1));
        assert!(!draws_grid(false, 4));
    }

    fn effects(matrix: bool) -> runner::Effects {
        runner::Effects {
            matrix,
            edge: 0.5,
            corner: 0.25,
            ghosting: false,
            blend: 1.0,
            scale: 1,
        }
    }

    fn sample(row: &[f32], at: f32) -> f32 {
        let t = at * row.len() as f32 - 0.5;
        let left = t.floor();
        let pick = |i: f32| row[(i.max(0.0) as usize).min(row.len() - 1)];
        let w = t - left;
        pick(left) * (1.0 - w) + pick(left + 1.0) * w
    }

    fn sharp_bilinear_simple(source: &[f32], wide: usize, at: f32) -> f32 {
        let scale = ((wide / source.len()) as f32).max(1.0);
        let texel = at * source.len() as f32;
        let floored = texel.floor();
        let centre = texel - floored - 0.5;
        let range = 0.5 - 0.5 / scale;
        let f = (centre - centre.clamp(-range, range)) * scale + 0.5;
        sample(source, (floored + f) / source.len() as f32)
    }

    fn same_size_row(source: &[u8], wide: usize) -> Vec<f32> {
        let width = source.len();
        let screens: &'static [Screen] = Box::leak(Box::new([Screen::whole(width, 1)]));
        let video = VideoSpec::of(screens);
        let frame: Vec<u8> = source.iter().flat_map(|&v| [v, v, v, 255]).collect();
        let pane = egui::vec2(wide as f32, wide as f32 / width as f32);
        let shown = output(ScalingMode::Stretch, false, true).show(video.into(), pane);
        let mut texture = Vec::new();
        runner::paint(
            &frame,
            video,
            [0; 3],
            effects(false),
            shown.scale,
            &mut texture,
        );
        let row: Vec<f32> = texture
            .chunks(4)
            .take(width * shown.scale)
            .map(|p| p[0] as f32)
            .collect();
        (0..wide)
            .map(|x| sample(&row, (x as f32 + 0.5) / wide as f32))
            .collect()
    }

    #[test]
    fn same_size_pixels_match_retroarch_sharp_bilinear_on_a_small_frame() {
        assert_eq!(same_size_row(&[0, 255], 5), [0.0, 0.0, 127.5, 255.0, 255.0]);
        let source = [0u8, 255, 60, 200, 10, 120];
        let levels: Vec<f32> = source.iter().map(|&v| v as f32).collect();
        for wide in [7, 13, 15, 22, 40] {
            let ours = same_size_row(&source, wide);
            for (x, value) in ours.iter().enumerate() {
                let at = (x as f32 + 0.5) / wide as f32;
                let theirs = sharp_bilinear_simple(&levels, wide, at);
                assert!(
                    (value - theirs).abs() < 0.01,
                    "{wide} {x}: {value} {theirs}"
                );
            }
        }
    }

    #[test]
    fn the_matrix_grid_darkens_the_last_texel_row_and_column_of_each_dot() {
        const DOTS: [Screen; 1] = [Screen::whole(2, 1)];
        let frame = [200, 200, 200, 255, 40, 80, 120, 255];
        let mut out = Vec::new();
        runner::paint(
            &frame,
            VideoSpec::of(&DOTS),
            [0; 3],
            effects(true),
            2,
            &mut out,
        );
        let reds: Vec<u8> = out.chunks(4).map(|p| p[0]).collect();
        assert_eq!(reds, [200, 100, 40, 20, 100, 50, 20, 10]);
    }

    #[test]
    fn the_matrix_leaves_a_one_times_picture_untinted() {
        const DOTS: [Screen; 1] = [Screen::whole(2, 1)];
        let frame = [200, 200, 200, 255, 40, 80, 120, 255];
        let mut out = Vec::new();
        runner::paint(
            &frame,
            VideoSpec::of(&DOTS),
            [0; 3],
            effects(true),
            1,
            &mut out,
        );
        assert_eq!(out, frame);
    }

    #[test]
    fn without_the_grid_each_dot_fills_a_whole_block_at_any_scale() {
        const DOTS: [Screen; 1] = [Screen::whole(3, 2)];
        let frame: Vec<u8> = (0..24).map(|i| (i * 11) as u8).collect();
        for scale in 1..=4 {
            let wide = 3 * scale;
            let mut expected = vec![0; wide * 2 * scale * 4];
            for (at, dot) in expected.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = (at % wide / scale, at / wide / scale);
                let pixel = &frame[(y * 3 + x) * 4..][..3];
                *dot = [pixel[0], pixel[1], pixel[2], 255];
            }
            for matrix in [false, true] {
                let paint = |out: &mut Vec<u8>| {
                    let video = VideoSpec::of(&DOTS);
                    runner::paint(&frame, video, [0; 3], effects(matrix), scale, out);
                };
                let mut out = vec![9; 500];
                paint(&mut out);
                if !draws_grid(matrix, scale) {
                    assert_eq!(out, expected, "scale {scale}");
                }
                let mut fresh = Vec::new();
                paint(&mut fresh);
                assert_eq!(out, fresh, "scale {scale} matrix {matrix}");
            }
        }
    }

    #[test]
    fn a_screen_image_nobody_else_holds_is_refilled_in_place() {
        let mut kept = None;
        let first = refill(&mut kept, [1, 1], &[1, 2, 3, 4]);
        let at = first.pixels.as_ptr();
        drop(first);
        let second = refill(&mut kept, [1, 1], &[5, 6, 7, 8]);
        assert_eq!(second.pixels.as_ptr(), at);
        let held = refill(&mut kept, [1, 1], &[9, 9, 9, 9]);
        let pixel = Color32::from_rgba_premultiplied;
        assert_eq!(second.pixels, [pixel(5, 6, 7, 8)]);
        assert_eq!(held.pixels, [pixel(9, 9, 9, 9)]);
        let wider = refill(&mut kept, [2, 1], &[1; 8]);
        assert_eq!(wider.size, [2, 1]);
    }
}
