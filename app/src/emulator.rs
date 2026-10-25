use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Captured, GbButton, KeyBindings, PadButton,
};
use crate::app::ui_state::settings::ScalingMode;
use crate::audio::{AudioLink, push_frames};
use crate::recorder::InputRecorder;
use crate::storage::SaveStore;
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb::utils::ema::EMA;
use gilrs::Axis;
use gilrs::EventType::{AxisChanged, ButtonPressed, ButtonReleased};
use ringbuf::HeapProd;
use ringbuf::traits::Observer;
use strum::IntoEnumIterator;

const FRAME_TIME: f64 = 1.0 / 59.7275;
const SAVE_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);

const MAX_RATE_DRIFT: f32 = 0.01;
const MAX_CATCHUP_FRAMES: f64 = 5.0;
const RATE_STEP: u32 = 8;

pub struct SaveStatus {
    pub battery_backed: bool,
    pub stored: bool,
    pub saved_at: Option<u64>,
    pub location: String,
    #[cfg(not(target_arch = "wasm32"))]
    pub legacy_path: Option<std::path::PathBuf>,
}
const GB_WIDTH: usize = 160;
const GB_HEIGHT: usize = 144;

pub struct Emulator {
    pub gb: GameBoy,
    pub running: bool,
    pub audio_producer: Option<HeapProd<f32>>,
    pub audio_overrun_samples: u128,
    audio_nominal_rate: u32,
    audio_target_frames: usize,
    audio_fill: EMA<f32>,
    pub enable_matrix: bool,
    /// 0.0 to 1.0 (e.g., 0.85 = 15% darker)
    pub matrix_edge_brightness: f32,
    /// 0.0 to 1.0 (e.g., 0.75 = 25% darker)
    pub matrix_corner_brightness: f32,
    pub enable_ghosting: bool,
    /// 0.0 (pure smear) to 1.0 (instant, no ghosting)
    pub ghosting_blend: f32,
    pub scaling: ScalingMode,
    held: JoypadState,
    texture: Option<egui::TextureHandle>,
    upscale_buffer: Vec<u8>,
    last_update: Option<web_time::Instant>,
    time_accumulator: f64,
    pub render_scale: usize,
    pub update_avg_timer: AvgTimer,
    pub frame_avg_timer: AvgTimer,
    pub total_frames: u128,
    last_frame: Vec<u8>,
    /// SHA-256 of the loaded ROM
    rom_key: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    rom_path: Option<std::path::PathBuf>,
    pub imported_legacy_save: bool,
    pub store: SaveStore,
    pub last_save: Option<web_time::Instant>,
    pub save_loaded: bool,
    pub recorder: InputRecorder,
}

impl Default for Emulator {
    fn default() -> Self {
        Self {
            gb: GameBoy::new_empty(GbModel::Dmg),
            running: true,
            audio_producer: None,
            audio_overrun_samples: 0,
            audio_nominal_rate: 44100,
            audio_target_frames: 0,
            audio_fill: EMA::default(),
            enable_matrix: true,
            matrix_edge_brightness: 0.85,
            matrix_corner_brightness: 0.75,
            enable_ghosting: true,
            ghosting_blend: 0.7,
            scaling: ScalingMode::default(),
            held: JoypadState::empty(),
            texture: None,
            upscale_buffer: Vec::new(),
            last_update: None,
            time_accumulator: 0.0,
            render_scale: 3,
            update_avg_timer: AvgTimer::default(),
            frame_avg_timer: AvgTimer::default(),
            total_frames: 0,
            last_frame: vec![0; GB_WIDTH * GB_HEIGHT * 4],
            rom_key: None,
            #[cfg(not(target_arch = "wasm32"))]
            rom_path: None,
            imported_legacy_save: false,
            store: SaveStore::default(),
            last_save: None,
            save_loaded: false,
            recorder: InputRecorder::default(),
        }
    }
}

impl Emulator {
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        gil: &mut gilrs::Gilrs,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
    ) -> GbResult<()> {
        self.update_avg_timer.start();

        if self.texture.is_none() {
            self.update_texture(ctx);
        }

        self.handle_input(ctx, gil, bindings, capture);

        if !self.running {
            self.gb.apu.audio_buffer.clear();
            self.last_update = Some(web_time::Instant::now());
            return Ok(());
        }

        let now = web_time::Instant::now();
        let dt = match self.last_update {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };

        self.last_update = Some(now);
        self.time_accumulator += dt;

        if self.time_accumulator > FRAME_TIME * MAX_CATCHUP_FRAMES {
            self.time_accumulator = FRAME_TIME * MAX_CATCHUP_FRAMES;
        }

        let mut ran_frame = false;
        while self.time_accumulator >= FRAME_TIME {
            self.frame_avg_timer.start();
            self.gb.run_frame();
            self.frame_avg_timer.stop();
            self.time_accumulator -= FRAME_TIME;
            ran_frame = true;

            if self.gb.debugger.hit_breakpoint {
                self.gb.debugger.hit_breakpoint = false;
                self.running = false;
                break;
            }
        }

        if ran_frame {
            self.update_texture(ctx);
            self.drain_audio();
            self.sync_audio_rate();
            self.total_frames = self.total_frames.wrapping_add(1);
        }

        self.handle_save()?;

        self.update_avg_timer.stop();
        Ok(())
    }

    pub fn attach_audio(&mut self, link: AudioLink) {
        self.gb.apu.set_sample_rate(link.sample_rate);
        self.audio_nominal_rate = link.sample_rate;
        self.audio_target_frames = link.target_frames;
        self.audio_fill = EMA::default();
        self.audio_producer = Some(link.producer);
    }

    fn drain_audio(&mut self) {
        let mut samples = std::mem::take(&mut self.gb.apu.audio_buffer);
        if let Some(producer) = &mut self.audio_producer {
            let pushed = push_frames(producer, &samples);
            if pushed < samples.len() {
                let dropped = samples.len() - pushed;
                self.audio_overrun_samples =
                    self.audio_overrun_samples.wrapping_add(dropped as u128);
            }
        }
        samples.clear();
        self.gb.apu.audio_buffer = samples;
    }

    fn sync_audio_rate(&mut self) {
        let Some(producer) = &self.audio_producer else {
            return;
        };
        if self.audio_target_frames == 0 {
            return;
        }

        let queued = (producer.occupied_len() / 2) as f32;
        self.audio_fill.update(queued, 20);

        let target = self.audio_target_frames as f32;
        let error = ((self.audio_fill.value() - target) / target).clamp(-1.0, 1.0);
        let rate = (self.audio_nominal_rate as f32 * (1.0 - MAX_RATE_DRIFT * error)) as u32;

        if rate.abs_diff(self.gb.apu.output_sample_rate) >= RATE_STEP {
            self.gb.apu.set_sample_rate(rate);
        }
    }

    pub fn audio_queued_frames(&self) -> usize {
        self.audio_producer
            .as_ref()
            .map(|p| p.occupied_len() / 2)
            .unwrap_or(0)
    }

    pub fn audio_target_frames(&self) -> usize {
        self.audio_target_frames
    }

    pub fn start_recording(&mut self) {
        self.recorder.start(&self.gb);
    }

    pub fn press(&mut self, button: JoypadState) {
        if self.held.contains(button) {
            return;
        }
        self.held.insert(button);
        let total_cycles = self.gb.debugger.total_cycles;
        self.gb.press_button(button);
        self.recorder.record(total_cycles, button, true);
    }

    pub fn release(&mut self, button: JoypadState) {
        if !self.held.contains(button) {
            return;
        }
        self.held.remove(button);
        let total_cycles = self.gb.debugger.total_cycles;
        self.gb.release_button(button);
        self.recorder.record(total_cycles, button, false);
    }

    pub fn set_held(&mut self, button: JoypadState, pressed: bool) {
        if pressed {
            self.press(button);
        } else {
            self.release(button);
        }
    }

    pub fn is_held(&self, button: JoypadState) -> bool {
        self.held.contains(button)
    }

    pub fn release_all(&mut self) {
        for button in GbButton::iter() {
            self.release(button.joypad());
        }
    }

    pub fn handle_input(
        &mut self,
        ctx: &egui::Context,
        gil: &mut gilrs::Gilrs,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
    ) {
        while let Some(gilrs::Event { event, .. }) = gil.next_event() {
            match event {
                ButtonPressed(btn, ..) => {
                    let Some(pad) = PadButton::from_gilrs(btn) else {
                        continue;
                    };
                    if capture.wants(BindingKind::Gamepad) {
                        capture.record(Captured::Pad(pad));
                        continue;
                    }
                    if let Some(button) = bindings.button_for_pad(pad) {
                        self.press(button.joypad());
                    }
                }
                ButtonReleased(btn, ..) => {
                    if let Some(button) =
                        PadButton::from_gilrs(btn).and_then(|pad| bindings.button_for_pad(pad))
                    {
                        self.release(button.joypad());
                    }
                }
                AxisChanged(axis, value, ..) if bindings.stick_as_dpad => match axis {
                    Axis::DPadX | Axis::LeftStickX => self.handle_x_axis(value),
                    Axis::DPadY | Axis::LeftStickY => self.handle_y_axis(value),
                    _ => {}
                },
                _ => {}
            }
        }

        if ctx.wants_keyboard_input() && !capture.wants(BindingKind::Keyboard) {
            return;
        }

        let events: Vec<(egui::Key, bool, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => Some((*key, *pressed, *modifiers)),
                    _ => None,
                })
                .collect()
        });

        for (key, pressed, modifiers) in events {
            if capture.wants(BindingKind::Keyboard) {
                if pressed && key != egui::Key::Escape {
                    capture.record(Captured::Key(key, modifiers));
                }
                continue;
            }
            if let Some(button) = bindings.button_for_key(key) {
                self.set_held(button.joypad(), pressed);
            }
        }
    }

    pub fn handle_x_axis(&mut self, value: f32) {
        if value < -0.5 {
            self.press(JoypadState::LEFT);
        } else if value > 0.5 {
            self.press(JoypadState::RIGHT);
        } else {
            self.release(JoypadState::LEFT);
            self.release(JoypadState::RIGHT);
        }
    }

    pub fn handle_y_axis(&mut self, value: f32) {
        if value < -0.5 {
            self.press(JoypadState::DOWN);
        } else if value > 0.5 {
            self.press(JoypadState::UP);
        } else {
            self.release(JoypadState::UP);
            self.release(JoypadState::DOWN);
        }
    }

    fn screen_layout(&self, available: egui::Vec2) -> (usize, egui::Vec2) {
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
        let (ideal_render_scale, display_size) = self.screen_layout(rect.size());

        if self.render_scale != ideal_render_scale {
            self.render_scale = ideal_render_scale;
            self.update_texture(ui.ctx());
        }

        let Some(tex) = &self.texture else {
            return;
        };

        let centre = rect.center();
        let top_left = egui::pos2(
            (centre.x - display_size.x / 2.0).round(),
            (centre.y - display_size.y / 2.0).round(),
        );
        let target = egui::Rect::from_min_size(top_left, display_size);

        egui::Image::new(egui::load::SizedTexture::new(tex.id(), display_size))
            .paint_at(ui, target);
    }

    pub fn screenshot(&self) -> (usize, usize, Vec<u8>) {
        let scale = self.render_scale.max(1);
        let expected = GB_WIDTH * scale * GB_HEIGHT * scale * 4;
        if self.upscale_buffer.len() == expected {
            (
                GB_WIDTH * scale,
                GB_HEIGHT * scale,
                self.upscale_buffer.clone(),
            )
        } else {
            (GB_WIDTH, GB_HEIGHT, self.gb.frame().as_slice().to_vec())
        }
    }

    pub fn load_rom(
        &mut self,
        rom: &Rom,
        #[cfg(not(target_arch = "wasm32"))] path: Option<&std::path::Path>,
    ) -> GbResult<()> {
        self.running = false;
        self.save_loaded = false;
        self.imported_legacy_save = false;
        self.gb.load_rom(rom)?;

        let key = self.gb.cartridge.header.sha256_hex_string();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.rom_path = path.map(|p| p.to_owned());
            if let Some(path) = path {
                self.imported_legacy_save = self.store.import_legacy_save_if_new(&key, path);
            }
        }

        if let Some(data) = self.store.load_battery(&key) {
            self.gb.put_sram_dump(SramDump::from_slice(&data));
            self.save_loaded = true;
        }

        self.rom_key = Some(key);
        self.last_save = None;
        self.running = true;
        Ok(())
    }

    pub fn handle_save(&mut self) -> GbResult<()> {
        if let Some(last_save) = self.last_save
            && last_save.elapsed() < SAVE_COOLDOWN
        {
            return Ok(());
        }
        self.flush_save()
    }

    pub fn flush_save(&mut self) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let Some(sdump) = self.gb.poll_sram_dump(false) else {
            return Ok(());
        };
        self.store.store_battery(&key, sdump.as_slice())?;
        self.last_save = Some(web_time::Instant::now());
        Ok(())
    }

    pub fn save_snapshot(&mut self, slot: usize) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let state = self.gb.dump_full()?;
        let frame = self.gb.frame().as_slice().to_vec();
        self.store.store_snapshot(&key, slot, &state, &frame)?;
        Ok(())
    }

    pub fn load_snapshot(&mut self, slot: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        let Some(state) = self.store.load_snapshot(&key, slot) else {
            return Ok(false);
        };
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let restored = citrine_gb::gb::GameBoy::from_dump(&state, &rom)?;
        let sample_rate = self.gb.apu.output_sample_rate;
        self.gb = restored;
        self.gb.apu.set_sample_rate(sample_rate);
        self.running = true;
        Ok(true)
    }

    pub fn delete_snapshot(&mut self, slot: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.delete_snapshot(&key, slot)?;
        }
        Ok(())
    }

    pub fn rename_snapshot(&mut self, slot: usize, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.set_snapshot_name(&key, slot, name)?;
        }
        Ok(())
    }

    pub fn rom_key(&self) -> Option<&str> {
        self.rom_key.as_deref()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rom_path(&self) -> Option<&std::path::Path> {
        self.rom_path.as_deref()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn reimport_legacy_save(&mut self) -> bool {
        let (Some(key), Some(path)) = (self.rom_key.clone(), self.rom_path.clone()) else {
            return false;
        };
        if !self.store.import_legacy_save(&key, &path) {
            return false;
        }
        if let Some(data) = self.store.load_battery(&key) {
            self.gb.put_sram_dump(SramDump::from_slice(&data));
            self.save_loaded = true;
            return true;
        }
        false
    }

    pub fn import_save_bytes(&mut self, data: &[u8]) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        self.store.store_battery(&key, data)?;
        self.gb.put_sram_dump(SramDump::from_slice(data));
        self.save_loaded = true;
        self.last_save = Some(web_time::Instant::now());
        Ok(true)
    }

    pub fn export_save_bytes(&mut self) -> Option<Vec<u8>> {
        self.gb
            .poll_sram_dump(true)
            .map(|dump| dump.as_slice().to_vec())
    }

    pub fn save_status(&self) -> SaveStatus {
        let key = self.rom_key.as_deref();
        SaveStatus {
            battery_backed: self.gb.cartridge.has_battery(),
            stored: key.is_some_and(|k| self.store.has_battery(k)),
            saved_at: key.and_then(|k| self.store.battery_saved_at(k)),
            location: self.store.location(),
            #[cfg(not(target_arch = "wasm32"))]
            legacy_path: self
                .rom_path
                .as_deref()
                .map(SaveStore::legacy_save_path)
                .filter(|p| p.is_file()),
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
        let scale = self.render_scale;
        let upscaled_width = GB_WIDTH * scale;
        let upscaled_height = GB_HEIGHT * scale;

        let current_raw = self.gb.frame().as_slice();
        let mut upscaled_data = std::mem::take(&mut self.upscale_buffer);
        upscaled_data.clear();
        upscaled_data.resize(upscaled_width * upscaled_height * 4, 0);

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

                for dy in 0..scale {
                    for dx in 0..scale {
                        let up_x = x * scale + dx;
                        let up_y = y * scale + dy;
                        let up_idx = (up_y * upscaled_width + up_x) * 4;

                        let mut pixel_r = r;
                        let mut pixel_g = g;
                        let mut pixel_b = b;

                        if self.enable_matrix {
                            let c0 = self.gb.ppu.dmg_theme.palette()[0];

                            if dx == scale - 1 && dy == scale - 1 {
                                let t = self.matrix_corner_brightness;
                                pixel_r = (pixel_r as f32 * t + c0.r() as f32 * (1.0 - t)) as u8;
                                pixel_g = (pixel_g as f32 * t + c0.g() as f32 * (1.0 - t)) as u8;
                                pixel_b = (pixel_b as f32 * t + c0.b() as f32 * (1.0 - t)) as u8;
                            } else if dx == scale - 1 || dy == scale - 1 {
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
