use super::Emulator;
use crate::app::ui_state::settings::ScalingMode;
use citrine_gb::console::{Console, VideoSpec};

impl Emulator {
    pub(super) fn screen_layout(&self, available: egui::Vec2) -> (usize, egui::Vec2) {
        let mode = if self.enable_matrix {
            ScalingMode::Integer
        } else {
            self.scaling
        };
        let video = self.video();
        let layout = fit_screen(video, available, mode);
        if mode != ScalingMode::Integer || self.reserve <= 0.0 {
            return layout;
        }
        let full = fit_screen(video, available + egui::vec2(0.0, self.reserve), mode);
        if full.1.y > layout.1.y {
            (full.0, fit_screen(video, available, ScalingMode::Fit).1)
        } else {
            layout
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<egui::Rect> {
        let available = ui.available_size();
        let (ideal_render_scale, display_size) = self.screen_layout(available);

        if self.render_scale != ideal_render_scale {
            self.render_scale = ideal_render_scale;
            self.update_texture(ui.ctx());
        }

        let tex = self.texture.as_ref()?;
        let leftover_x = (available.x - display_size.x).max(0.0);
        let leftover_y = (available.y - display_size.y).max(0.0);

        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

        ui.add_space((leftover_y / 2.0).floor());
        let shown = ui.horizontal(|ui| {
            ui.add_space((leftover_x / 2.0).floor());
            ui.image(egui::load::SizedTexture::new(tex.id(), display_size))
                .rect
        });
        Some(shown.inner)
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
        let (ideal_render_scale, display_size) = self.screen_layout(area.size());

        if self.render_scale != ideal_render_scale {
            self.render_scale = ideal_render_scale;
            self.update_texture(ui.ctx());
        }

        let target = place(display_size);
        if let Some(tex) = &self.texture {
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), display_size))
                .paint_at(ui, target);
        }
        target
    }

    pub fn paint_fitted(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> egui::Rect {
        let (_, size) = fit_screen(self.video(), rect.size(), ScalingMode::Fit);
        let target = egui::Rect::from_center_size(rect.center(), size);
        if self.texture.is_none() {
            self.update_texture(ui.ctx());
        }
        if let Some(tex) = &self.texture {
            egui::Image::new(egui::load::SizedTexture::new(tex.id(), target.size()))
                .paint_at(ui, target);
        }
        target
    }

    pub fn screenshot(&self, scale: usize) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let source = if self.upscale_buffer.is_empty() {
            self.machine.frame()
        } else {
            &self.last_frame
        };
        self.screenshot_of(source, scale)
    }

    pub fn screenshot_of(&self, source: &[u8], scale: usize) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let mut rgba = Vec::new();
        self.paint(source, scale, &mut rgba);
        let video = self.video();
        (video.width * scale, video.height * scale, rgba)
    }

    pub fn clear_frame(&mut self, ctx: &egui::Context) {
        if let Some(gb) = self.machine.as_gb_mut() {
            gb.ppu.clear_frame();
        }
        self.update_texture(ctx);
    }

    pub(super) fn blend_frame(&mut self) {
        let current_raw = match &self.linked_frame {
            Some(frame) => frame.as_slice(),
            None => self.machine.frame(),
        };
        self.last_frame.resize(current_raw.len(), 0);
        let blend = if self.enable_ghosting {
            self.ghosting_blend
        } else {
            1.0
        };
        for (last, &current) in self.last_frame.iter_mut().zip(current_raw) {
            *last = (current as f32 * blend + *last as f32 * (1.0 - blend)) as u8;
        }
        for alpha in self.last_frame.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }

    pub(super) fn paint(&self, source: &[u8], scale: usize, out: &mut Vec<u8>) {
        let VideoSpec { width, height } = self.video();
        let upscaled_width = width * scale;
        out.clear();
        out.resize(upscaled_width * height * scale * 4, 0);
        let c0 = self.backdrop();
        let toward_backdrop = |pixel: [u8; 3], t: f32| {
            [
                (pixel[0] as f32 * t + c0[0] as f32 * (1.0 - t)) as u8,
                (pixel[1] as f32 * t + c0[1] as f32 * (1.0 - t)) as u8,
                (pixel[2] as f32 * t + c0[2] as f32 * (1.0 - t)) as u8,
            ]
        };

        for y in 0..height {
            for x in 0..width {
                let orig_idx = (y * width + x) * 4;
                let pixel = [source[orig_idx], source[orig_idx + 1], source[orig_idx + 2]];
                let edge = toward_backdrop(pixel, self.matrix_edge_brightness);
                let corner = toward_backdrop(pixel, self.matrix_corner_brightness);

                for dy in 0..scale {
                    for dx in 0..scale {
                        let up_idx = ((y * scale + dy) * upscaled_width + x * scale + dx) * 4;
                        let right = dx == scale - 1;
                        let bottom = dy == scale - 1;
                        let [r, g, b] = match (self.enable_matrix, right, bottom) {
                            (true, true, true) => corner,
                            (true, true, false) | (true, false, true) => edge,
                            _ => pixel,
                        };
                        out[up_idx..up_idx + 4].copy_from_slice(&[r, g, b, 255]);
                    }
                }
            }
        }
    }

    fn backdrop(&self) -> [u8; 3] {
        self.machine.as_gb().map_or([0; 3], |gb| {
            let color = gb.ppu.backdrop_color();
            [color.r(), color.g(), color.b()]
        })
    }

    pub(super) fn update_texture(&mut self, ctx: &egui::Context) {
        let scale = self.render_scale;
        let video = self.video();
        self.blend_frame();
        let mut upscaled_data = std::mem::take(&mut self.upscale_buffer);
        self.paint(&self.last_frame, scale, &mut upscaled_data);

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [video.width * scale, video.height * scale],
            &upscaled_data,
        );
        self.upscale_buffer = upscaled_data;

        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.texture =
                    Some(ctx.load_texture("gb_screen", image, egui::TextureOptions::NEAREST))
            }
        }
    }
}

fn fit_screen(video: VideoSpec, available: egui::Vec2, mode: ScalingMode) -> (usize, egui::Vec2) {
    let width = video.width as f32;
    let height = video.height as f32;
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
    use citrine_gb::console::ConsoleKind;

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
}
