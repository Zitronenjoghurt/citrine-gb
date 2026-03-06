use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::sdump::SDump;
use citrine_gb::rom::Rom;
use gilrs::Axis;
use gilrs::EventType::{AxisChanged, ButtonPressed, ButtonReleased};
use ringbuf::producer::Producer;
use ringbuf::HeapProd;

const FRAME_TIME: f64 = 1.0 / 59.7275;
const GB_WIDTH: usize = 160;
const GB_HEIGHT: usize = 144;
const FRAME_SCALE: usize = 3;

pub struct Emulator {
    pub gb: GameBoy,
    pub running: bool,
    pub audio_producer: Option<HeapProd<f32>>,
    pub enable_matrix: bool,
    /// 0.0 to 1.0 (e.g., 0.85 = 15% darker)
    pub matrix_edge_brightness: f32,
    /// 0.0 to 1.0 (e.g., 0.75 = 25% darker)
    pub matrix_corner_brightness: f32,
    pub enable_ghosting: bool,
    /// 0.0 (pure smear) to 1.0 (instant, no ghosting)
    pub ghosting_blend: f32,
    texture: Option<egui::TextureHandle>,
    last_update: Option<web_time::Instant>,
    time_accumulator: f64,
    pub last_frame_secs: f64,
    last_frame: Vec<u8>,
    #[cfg(not(target_arch = "wasm32"))]
    rom_path: Option<std::path::PathBuf>,
    pub last_save: Option<web_time::Instant>,
    pub save_loaded: bool,
}

impl Default for Emulator {
    fn default() -> Self {
        Self {
            gb: GameBoy::new_empty(GbModel::Dmg),
            running: true,
            audio_producer: None,
            enable_matrix: true,
            matrix_edge_brightness: 0.85,
            matrix_corner_brightness: 0.75,
            enable_ghosting: true,
            ghosting_blend: 0.7,
            texture: None,
            last_update: None,
            time_accumulator: 0.0,
            last_frame_secs: 0.0,
            last_frame: vec![0; GB_WIDTH * GB_HEIGHT * 4],
            #[cfg(not(target_arch = "wasm32"))]
            rom_path: None,
            last_save: None,
            save_loaded: false,
        }
    }
}

impl Emulator {
    pub fn update(&mut self, ctx: &egui::Context, gil: &mut gilrs::Gilrs) -> GbResult<()> {
        if self.texture.is_none() {
            self.update_texture(ctx);
        }

        if !self.running {
            self.last_update = Some(web_time::Instant::now());
            return Ok(());
        }

        self.handle_input(ctx, gil);

        let now = web_time::Instant::now();
        let dt = match self.last_update {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };

        self.last_update = Some(now);
        self.time_accumulator += dt;

        if self.time_accumulator > FRAME_TIME * 5.0 {
            self.time_accumulator = FRAME_TIME * 5.0;
        }

        let mut ran_frame = false;
        while self.time_accumulator >= FRAME_TIME {
            let start = web_time::Instant::now();

            self.gb.run_frame();

            self.last_frame_secs = start.elapsed().as_secs_f64();
            self.time_accumulator -= FRAME_TIME;
            ran_frame = true;
        }

        if ran_frame {
            self.update_texture(ctx);

            if let Some(producer) = &mut self.audio_producer {
                let samples = &self.gb.apu.audio_buffer;
                let _ = producer.push_slice(samples);
                self.gb.apu.audio_buffer.clear();
            }
        }

        self.handle_save()?;

        Ok(())
    }

    pub fn handle_input(&mut self, ctx: &egui::Context, gil: &mut gilrs::Gilrs) {
        while let Some(gilrs::Event { event, .. }) = gil.next_event() {
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
                AxisChanged(axis, value, ..) => match axis {
                    Axis::DPadX => self.handle_x_axis(value),
                    Axis::DPadY => self.handle_y_axis(value),
                    Axis::LeftStickX => self.handle_x_axis(value),
                    Axis::LeftStickY => self.handle_y_axis(value),
                    _ => {}
                },
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

    pub fn handle_x_axis(&mut self, value: f32) {
        if value < -0.5 {
            self.gb.press_button(JoypadState::LEFT);
        } else if value > 0.5 {
            self.gb.press_button(JoypadState::RIGHT);
        } else {
            self.gb.release_button(JoypadState::LEFT);
            self.gb.release_button(JoypadState::RIGHT);
        }
    }

    pub fn handle_y_axis(&mut self, value: f32) {
        if value < -0.5 {
            self.gb.press_button(JoypadState::DOWN);
        } else if value > 0.5 {
            self.gb.press_button(JoypadState::UP);
        } else {
            self.gb.release_button(JoypadState::UP);
            self.gb.release_button(JoypadState::DOWN);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(tex) = &self.texture {
            let available = ui.available_size();

            let tex_width = (GB_WIDTH * FRAME_SCALE) as f32;
            let tex_height = (GB_HEIGHT * FRAME_SCALE) as f32;

            let scale_x = available.x / tex_width;
            let scale_y = available.y / tex_height;

            let mut scale_factor = scale_x.min(scale_y);

            if self.enable_matrix {
                scale_factor = scale_factor.floor().max(1.0);
            }

            let final_size = egui::vec2(tex_width * scale_factor, tex_height * scale_factor);

            let leftover_x = (available.x - final_size.x).max(0.0);
            let leftover_y = (available.y - final_size.y).max(0.0);

            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

            ui.add_space((leftover_y / 2.0).floor());
            ui.horizontal(|ui| {
                ui.add_space((leftover_x / 2.0).floor());
                ui.image(egui::load::SizedTexture::new(tex.id(), final_size));
            });

            ui.ctx().request_repaint();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_rom(&mut self, rom: &Rom, path: Option<&std::path::Path>) -> GbResult<()> {
        self.rom_path = path.map(|p| p.to_owned());
        let sdump = if let Some(path) = path
            && let sav_path = path.with_extension("sav")
            && sav_path.exists()
        {
            Some(SDump::load(&sav_path)?)
        } else {
            None
        };

        self.running = false;

        self.gb.load_rom(rom)?;

        if let Some(sdump) = sdump {
            self.gb.put_sram_dump(sdump);
            self.save_loaded = true;
        }

        self.running = true;

        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn load_rom(&mut self, rom: &Rom) -> GbResult<()> {
        self.running = false;

        self.gb.load_rom(rom)?;

        let save_key = self.web_rom_save_key();

        let sdump = if let Some(window) = web_sys::window()
            && let Ok(Some(local_storage)) = window.local_storage()
            && let Ok(Some(data)) = local_storage.get_item(&save_key)
            && let Ok(sdump) = SDump::from_base64(&data)
        {
            Some(sdump)
        } else {
            None
        };

        if let Some(sdump) = sdump {
            self.gb.put_sram_dump(sdump);
            self.save_loaded = true;
        }

        self.running = true;

        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn handle_save(&mut self) -> GbResult<()> {
        if let Some(last_save) = self.last_save
            && last_save.elapsed().as_secs() < 5
        {
            return Ok(());
        }

        let Some(sdump) = self.gb.poll_sram_dump() else {
            return Ok(());
        };

        let save_key = self.web_rom_save_key();
        let data = sdump.to_base64()?;
        drop(sdump);

        if let Some(window) = web_sys::window()
            && let Ok(Some(local_storage)) = window.local_storage()
        {
            let _ = local_storage.set_item(&save_key, &data);
            self.last_save = Some(web_time::Instant::now());
        }

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn handle_save(&mut self) -> GbResult<()> {
        if let Some(last_save) = self.last_save
            && last_save.elapsed().as_secs() < 5
        {
            return Ok(());
        }

        let Some(rom_path) = self.rom_path.as_ref() else {
            return Ok(());
        };

        let Some(sdump) = self.gb.poll_sram_dump() else {
            return Ok(());
        };

        sdump.save(&rom_path.with_extension("sav"))?;
        self.last_save = Some(web_time::Instant::now());

        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    fn web_rom_save_key(&self) -> String {
        let rom_hash = self.gb.cartridge.header.sha256_hex_string();
        format!("sav-{}", rom_hash)
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
        let upscaled_width = GB_WIDTH * FRAME_SCALE;
        let upscaled_height = GB_HEIGHT * FRAME_SCALE;

        let current_raw = self.gb.frame().as_slice();
        let mut upscaled_data = vec![0u8; upscaled_width * upscaled_height * 4];

        let blend = if self.enable_ghosting {
            self.ghosting_blend
        } else {
            1.0
        };

        for y in 0..GB_HEIGHT {
            for x in 0..GB_WIDTH {
                let orig_idx = (y * GB_WIDTH + x) * 4;

                let r = (current_raw[orig_idx] as f32 * blend
                    + self.last_frame[orig_idx] as f32 * (1.0 - blend))
                    as u8;
                let g = (current_raw[orig_idx + 1] as f32 * blend
                    + self.last_frame[orig_idx + 1] as f32 * (1.0 - blend))
                    as u8;
                let b = (current_raw[orig_idx + 2] as f32 * blend
                    + self.last_frame[orig_idx + 2] as f32 * (1.0 - blend))
                    as u8;
                let a = 255;

                self.last_frame[orig_idx] = r;
                self.last_frame[orig_idx + 1] = g;
                self.last_frame[orig_idx + 2] = b;
                self.last_frame[orig_idx + 3] = a;

                for dy in 0..FRAME_SCALE {
                    for dx in 0..FRAME_SCALE {
                        let up_x = x * FRAME_SCALE + dx;
                        let up_y = y * FRAME_SCALE + dy;
                        let up_idx = (up_y * upscaled_width + up_x) * 4;

                        let mut pixel_r = r;
                        let mut pixel_g = g;
                        let mut pixel_b = b;

                        if self.enable_matrix {
                            let c0 = self.gb.ppu.dmg_theme.palette()[0];

                            if dx == FRAME_SCALE - 1 && dy == FRAME_SCALE - 1 {
                                let t = self.matrix_corner_brightness;
                                pixel_r = (pixel_r as f32 * t + c0.r() as f32 * (1.0 - t)) as u8;
                                pixel_g = (pixel_g as f32 * t + c0.g() as f32 * (1.0 - t)) as u8;
                                pixel_b = (pixel_b as f32 * t + c0.b() as f32 * (1.0 - t)) as u8;
                            } else if dx == FRAME_SCALE - 1 || dy == FRAME_SCALE - 1 {
                                let t = self.matrix_edge_brightness;
                                pixel_r = (pixel_r as f32 * t + c0.r() as f32 * (1.0 - t)) as u8;
                                pixel_g = (pixel_g as f32 * t + c0.g() as f32 * (1.0 - t)) as u8;
                                pixel_b = (pixel_b as f32 * t + c0.b() as f32 * (1.0 - t)) as u8;
                            }
                        }

                        upscaled_data[up_idx] = pixel_r;
                        upscaled_data[up_idx + 1] = pixel_g;
                        upscaled_data[up_idx + 2] = pixel_b;
                        upscaled_data[up_idx + 3] = a;
                    }
                }
            }
        }

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [upscaled_width, upscaled_height],
            &upscaled_data,
        );

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
        gilrs::Button::East | gilrs::Button::West => Some(JoypadState::A),
        gilrs::Button::South | gilrs::Button::North => Some(JoypadState::B),
        gilrs::Button::Start => Some(JoypadState::START),
        gilrs::Button::Select => Some(JoypadState::SELECT),
        _ => None,
    }
}
