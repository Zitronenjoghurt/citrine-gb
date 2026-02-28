use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use gilrs::EventType::{ButtonPressed, ButtonReleased};

pub struct Emulator {
    pub gb: GameBoy,
    texture: Option<egui::TextureHandle>,
    pub running: bool,
    last_frame: Option<web_time::Instant>,
    pub last_frame_secs: f64,
}

impl Default for Emulator {
    fn default() -> Self {
        Self {
            gb: GameBoy::new_empty(GbModel::Dmg),
            texture: None,
            running: true,
            last_frame: None,
            last_frame_secs: 0.0,
        }
    }
}

impl Emulator {
    pub fn update(&mut self, ctx: &egui::Context, gil: &mut gilrs::Gilrs) {
        if self.texture.is_none() {
            self.update_texture(ctx);
        }

        if !self.running {
            return;
        }

        self.handle_input(ctx, gil);

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

    pub fn handle_input(&mut self, ctx: &egui::Context, gil: &mut gilrs::Gilrs) {
        while let Some(gilrs::Event { event, id, .. }) = gil.next_event() {
            println!("Gamepad {:?}: {:?}", id, event);
            match event {
                ButtonPressed(btn, ..) => {
                    if let Some(button) = gamepad_map(btn) {
                        self.gb.press_button(button);
                    }
                }
                ButtonReleased(btn, ..) => {
                    if let Some(button) = gamepad_map(btn) {
                        self.gb.release_button(button);
                    }
                }
                _ => {}
            }
        }

        ctx.input(|i| {
            for event in &i.events {
                let egui::Event::Key { key, pressed, .. } = event else {
                    continue;
                };

                let button = match key {
                    egui::Key::W | egui::Key::ArrowUp => Some(JoypadState::UP),
                    egui::Key::S | egui::Key::ArrowDown => Some(JoypadState::DOWN),
                    egui::Key::A | egui::Key::ArrowLeft => Some(JoypadState::LEFT),
                    egui::Key::D | egui::Key::ArrowRight => Some(JoypadState::RIGHT),
                    egui::Key::Q | egui::Key::Z | egui::Key::Y => Some(JoypadState::A),
                    egui::Key::E | egui::Key::X => Some(JoypadState::B),
                    egui::Key::Enter | egui::Key::Space => Some(JoypadState::START),
                    egui::Key::Backspace => Some(JoypadState::SELECT),
                    _ => None,
                };

                if let Some(button) = button {
                    if *pressed {
                        self.gb.press_button(button);
                    } else {
                        self.gb.release_button(button);
                    }
                }
            }
        });
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

    pub fn force_step(&mut self, ctx: &egui::Context, cycle_count: u32) {
        self.gb.run_cycles(cycle_count);
        self.update_texture(ctx);
    }

    pub fn soft_reset(&mut self, ctx: &egui::Context) {
        self.gb.soft_reset();
        self.update_texture(ctx);
    }

    pub fn reset_to(&mut self, ctx: &egui::Context, start_cycles: u32) {
        self.gb.soft_reset();
        self.gb.run_cycles(start_cycles);
        self.update_texture(ctx);
    }

    pub fn clear_frame(&mut self, ctx: &egui::Context) {
        self.gb.ppu.clear_frame();
        self.update_texture(ctx);
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

fn gamepad_map(btn: gilrs::Button) -> Option<JoypadState> {
    match btn {
        gilrs::Button::DPadUp => Some(JoypadState::UP),
        gilrs::Button::DPadDown => Some(JoypadState::DOWN),
        gilrs::Button::DPadLeft => Some(JoypadState::LEFT),
        gilrs::Button::DPadRight => Some(JoypadState::RIGHT),
        gilrs::Button::East => Some(JoypadState::A),
        gilrs::Button::South => Some(JoypadState::B),
        gilrs::Button::Start => Some(JoypadState::START),
        gilrs::Button::Select => Some(JoypadState::SELECT),
        _ => None,
    }
}
