use super::{Emulator, GB_HEIGHT, GB_WIDTH};
use crate::app::ui_state::settings::ScalingMode;

impl Emulator {
    pub(super) fn screen_layout(&self, available: egui::Vec2) -> (usize, egui::Vec2) {
        let width = GB_WIDTH as f32;
        let height = GB_HEIGHT as f32;
        let fit = (available.x / width).min(available.y / height).max(1.0);

        let mode = if self.enable_matrix {
            ScalingMode::Integer
        } else {
            self.scaling
        };

        match mode {
            ScalingMode::Integer => {
                let scale = fit.floor().max(1.0);
                (scale as usize, egui::vec2(width * scale, height * scale))
            }
            ScalingMode::Fit => (1, egui::vec2(width * fit, height * fit)),
            ScalingMode::Stretch => (
                1,
                egui::vec2(available.x.max(width), available.y.max(height)),
            ),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_size();
        let (ideal_render_scale, display_size) = self.screen_layout(available);

        if self.render_scale != ideal_render_scale {
            self.render_scale = ideal_render_scale;
            self.update_texture(ui.ctx());
        }

        if let Some(tex) = &self.texture {
            let leftover_x = (available.x - display_size.x).max(0.0);
            let leftover_y = (available.y - display_size.y).max(0.0);

            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

            ui.add_space((leftover_y / 2.0).floor());
            ui.horizontal(|ui| {
                ui.add_space((leftover_x / 2.0).floor());
                ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
            });
        }
    }

    pub fn ui_at(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        self.ui_placed(ui, rect, |size| {
            let centre = rect.center();
            let top_left = egui::pos2(
                (centre.x - size.x / 2.0).round(),
                (centre.y - size.y / 2.0).round(),
            );
            egui::Rect::from_min_size(top_left, size)
        });
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
        let target = crate::app::tabs::linked::fit(rect, f32::MAX);
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
            self.gb.frame().as_slice()
        } else {
            &self.last_frame
        };
        self.screenshot_of(source, scale)
    }

    pub fn screenshot_of(&self, source: &[u8], scale: usize) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let mut rgba = Vec::new();
        self.paint(source, scale, &mut rgba);
        (GB_WIDTH * scale, GB_HEIGHT * scale, rgba)
    }

    pub fn clear_frame(&mut self, ctx: &egui::Context) {
        self.gb.ppu.clear_frame();
        self.update_texture(ctx);
    }

    pub(super) fn blend_frame(&mut self) {
        let current_raw = match &self.linked_frame {
            Some(frame) => frame.as_slice(),
            None => self.gb.frame().as_slice(),
        };
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
        let upscaled_width = GB_WIDTH * scale;
        out.clear();
        out.resize(upscaled_width * GB_HEIGHT * scale * 4, 0);
        let c0 = self.gb.ppu.backdrop_color();
        let toward_backdrop = |pixel: [u8; 3], t: f32| {
            [
                (pixel[0] as f32 * t + c0.r() as f32 * (1.0 - t)) as u8,
                (pixel[1] as f32 * t + c0.g() as f32 * (1.0 - t)) as u8,
                (pixel[2] as f32 * t + c0.b() as f32 * (1.0 - t)) as u8,
            ]
        };

        for y in 0..GB_HEIGHT {
            for x in 0..GB_WIDTH {
                let orig_idx = (y * GB_WIDTH + x) * 4;
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

    pub(super) fn update_texture(&mut self, ctx: &egui::Context) {
        let scale = self.render_scale;
        self.blend_frame();
        let mut upscaled_data = std::mem::take(&mut self.upscale_buffer);
        self.paint(&self.last_frame, scale, &mut upscaled_data);

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [GB_WIDTH * scale, GB_HEIGHT * scale],
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
