use citrine_gb::gb::GameBoy;

pub struct Emulator {
    gb: GameBoy,
    texture: Option<egui::TextureHandle>,
    running: bool,
    last_frame: Option<web_time::Instant>,
    pub last_frame_secs: f64,
}

impl Default for Emulator {
    fn default() -> Self {
        Self {
            gb: GameBoy::new_dmg(0),
            texture: None,
            running: false,
            last_frame: None,
            last_frame_secs: 0.0,
        }
    }
}

impl Emulator {
    pub fn update(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            self.update_texture(ctx);
        }

        if !self.running {
            return;
        }

        let now = web_time::Instant::now();
        let should_run = match self.last_frame {
            Some(last) => now.duration_since(last).as_secs_f64() >= 1.0 / 59.7275,
            None => true,
        };

        if should_run {
            let start = web_time::Instant::now();
            self.gb.run_frame();
            self.update_texture(ctx);
            self.last_frame = Some(now);
            self.last_frame_secs = start.elapsed().as_secs_f64();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(tex) = &self.texture {
            let aspect = 160.0 / 144.0;
            let available = ui.available_size();

            let size = if available.x / available.y > aspect {
                egui::vec2(available.y * aspect, available.y)
            } else {
                egui::vec2(available.x, available.x / aspect)
            };

            ui.image(egui::load::SizedTexture::new(tex.id(), size));
            ui.ctx().request_repaint();
        }
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let image =
            egui::ColorImage::from_rgba_unmultiplied([160, 144], self.gb.frame().as_slice());

        match &mut self.texture {
            Some(tex) => tex.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.texture =
                    Some(ctx.load_texture("gb_screen", image, egui::TextureOptions::NEAREST))
            }
        }
    }
}
