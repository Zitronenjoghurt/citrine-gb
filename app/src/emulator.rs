use crate::app::ui_state::keybinds::{
    BindingCapture, BindingKind, Captured, GbButton, KeyBindings, PadButton,
};
use crate::app::ui_state::settings::{ConsoleChoice, ScalingMode};
use crate::audio::AudioLink;
use crate::audio::pacer::{AudioFeed, FrameClock};
use crate::autosave::{AutosaveInterval, LastGood, PlayClock, StateCopy};
use crate::media::recording::ScreenRecorder;
use crate::recorder::InputRecorder;
use crate::storage::{SaveStore, now_unix};
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::full_dump::FullDumpRef;
use citrine_gb::persistence::rtc_footer::RtcFooter;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use gilrs::Axis;
use gilrs::EventType::{AxisChanged, ButtonPressed, ButtonReleased};
use strum::IntoEnumIterator;

const SAVE_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);
const PLAYING_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_PLAY_STEP: f64 = 0.25;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SaveImport {
    Imported,
    NoRom,
    NoClock,
    Nothing,
}

pub struct SaveStatus {
    pub name: String,
    pub battery_backed: bool,
    pub stored: bool,
    pub saved_at: Option<u64>,
    pub location: String,
    pub has_clock: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub legacy_path: Option<std::path::PathBuf>,
}
const GB_WIDTH: usize = 160;
const GB_HEIGHT: usize = 144;

pub struct Emulator {
    pub gb: GameBoy,
    pub running: bool,
    audio_feed: Option<AudioFeed>,
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
    clock: FrameClock,
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
    started_at: Option<web_time::Instant>,
    pub recorder: InputRecorder,
    pub screen_recorder: ScreenRecorder,
    pub console: ConsoleChoice,
    pub autosave_interval: AutosaveInterval,
    pub autosave_keep: usize,
    play: PlayClock,
    pub last_good: LastGood,
}

impl Default for Emulator {
    fn default() -> Self {
        Self {
            gb: GameBoy::new_empty(GbModel::Dmg),
            running: true,
            audio_feed: None,
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
            clock: FrameClock::default(),
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
            started_at: None,
            recorder: InputRecorder::default(),
            screen_recorder: ScreenRecorder::default(),
            console: ConsoleChoice::default(),
            autosave_interval: AutosaveInterval::default(),
            autosave_keep: crate::autosave::DEFAULT_KEEP,
            play: PlayClock::default(),
            last_good: LastGood::default(),
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
            self.advance(web_time::Instant::now());
            return Ok(());
        }

        if self.advance(web_time::Instant::now()) {
            self.update_texture(ctx);
            self.drain_audio();
            self.total_frames = self.total_frames.wrapping_add(1);
        }

        self.handle_save()?;

        self.update_avg_timer.stop();
        Ok(())
    }

    fn advance(&mut self, now: web_time::Instant) -> bool {
        if !self.running {
            self.gb.apu.audio_buffer.clear();
            if let Some(feed) = &self.audio_feed {
                feed.set_paused(true);
            }
            self.last_update = Some(now);
            if self.play.copy_due(true) {
                self.copy_last_good();
            }
            return false;
        }

        let dt = match self.last_update {
            Some(last) => now.duration_since(last).as_secs_f64(),
            None => 0.0,
        };
        self.last_update = Some(now);

        let mut ran_frame = false;
        let mut heard = self.gb.apu.audio_buffer.len();
        for _ in 0..self.clock.advance(dt) {
            self.frame_avg_timer.start();
            self.gb.run_frame();
            self.frame_avg_timer.stop();
            ran_frame = true;
            heard = self.capture_frame(heard);

            if self.gb.debugger.hit_breakpoint {
                self.gb.debugger.hit_breakpoint = false;
                self.running = false;
                break;
            }
        }

        self.play
            .play(std::time::Duration::from_secs_f64(dt.min(MAX_PLAY_STEP)));
        if self.play.copy_due(false) {
            self.copy_last_good();
        }
        ran_frame
    }

    fn capture_frame(&mut self, heard: usize) -> usize {
        let samples = &self.gb.apu.audio_buffer;
        if self.screen_recorder.is_recording() {
            let fresh = &samples[heard.min(samples.len())..];
            self.screen_recorder.capture(
                self.gb.frame().as_slice(),
                fresh,
                self.gb.apu.output_sample_rate,
            );
        }
        samples.len()
    }

    pub fn attach_audio(&mut self, link: AudioLink) {
        self.gb.apu.set_sample_rate(link.sample_rate);
        self.audio_feed = Some(link.feed);
    }

    fn drain_audio(&mut self) {
        let mut samples = std::mem::take(&mut self.gb.apu.audio_buffer);
        if let Some(feed) = &mut self.audio_feed {
            feed.set_paused(false);
            feed.push(&samples);
        }
        samples.clear();
        self.gb.apu.audio_buffer = samples;
    }

    pub fn audio_feed(&self) -> Option<&AudioFeed> {
        self.audio_feed.as_ref()
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

    pub fn screenshot(&self, scale: usize) -> (usize, usize, Vec<u8>) {
        let scale = scale.max(1);
        let source = if self.upscale_buffer.is_empty() {
            self.gb.frame().as_slice()
        } else {
            &self.last_frame
        };
        let mut rgba = Vec::new();
        self.paint(source, scale, &mut rgba);
        (GB_WIDTH * scale, GB_HEIGHT * scale, rgba)
    }

    pub fn load_rom(
        &mut self,
        rom: &Rom,
        #[cfg(not(target_arch = "wasm32"))] path: Option<&std::path::Path>,
    ) -> GbResult<()> {
        self.screen_recorder.stop();
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave before switching games: {err:?}");
        }
        self.running = false;
        self.save_loaded = false;
        self.imported_legacy_save = false;
        self.gb
            .load_rom_with_model(rom, self.console.model_for(rom))?;

        let key = self.gb.cartridge.header.sha256_hex_string();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.rom_path = path.map(|p| p.to_owned());
            if let Some(path) = path {
                self.imported_legacy_save = self.store.import_legacy_save_if_new(&key, path);
            }
        }

        if let Some(data) = self.store.load_battery(&key) {
            self.gb
                .put_sram_dump(SramDump::from_slice(&data), now_unix());
            self.save_loaded = true;
        }

        self.rom_key = Some(key);
        self.last_save = None;
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.last_good.clear();
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
        let Some(sdump) = self.gb.poll_sram_dump(false, now_unix()) else {
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
        let state = self.gb.dump_full(now_unix())?;
        let frame = self.gb.frame().as_slice().to_vec();
        self.store.store_snapshot(&key, slot, &state, &frame)?;
        self.started_at = Some(web_time::Instant::now());
        Ok(())
    }

    pub fn load_snapshot(&mut self, slot: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        let Some(state) = self.store.load_snapshot(&key, slot) else {
            return Ok(false);
        };
        self.restore(&state)?;
        Ok(true)
    }

    pub fn load_autosave(&mut self, id: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        let Some(state) = self.store.load_autosave(&key, id) else {
            return Ok(false);
        };
        self.restore(&state)?;
        Ok(true)
    }

    fn restore(&mut self, state: &[u8]) -> GbResult<()> {
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let restored = citrine_gb::gb::GameBoy::from_dump(state, &rom, now_unix())?;
        let sample_rate = self.gb.apu.output_sample_rate;
        self.gb = restored;
        self.gb.apu.set_sample_rate(sample_rate);
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.copy_last_good();
        Ok(())
    }

    pub fn delete_autosave(&mut self, id: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.delete_autosave(&key, id)?;
        }
        Ok(())
    }

    pub fn autosave(&mut self) -> GbResult<Option<usize>> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(None);
        };
        self.play.attempted();
        self.flush_save()?;
        let state = self.gb.dump_full(now_unix())?;
        let frame = self.gb.frame().as_slice().to_vec();
        let id = self
            .store
            .store_autosave(&key, &state, &frame, self.autosave_keep)?;
        self.play.saved();
        self.last_good.mark_saved();
        self.started_at = Some(web_time::Instant::now());
        Ok(Some(id))
    }

    pub fn autosave_if_played(&mut self) -> GbResult<Option<usize>> {
        if !self.play.has_unsaved_play() {
            return Ok(None);
        }
        self.autosave()
    }

    pub fn autosave_if_due(&mut self) -> GbResult<Option<usize>> {
        if !self.play.autosave_due(self.autosave_interval) {
            return Ok(None);
        }
        self.autosave()
    }

    fn copy_last_good(&mut self) {
        self.play.copied();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let sram = self.gb.poll_sram_dump(true, now_unix());
        let battery = sram.as_ref().map(|dump| dump.as_slice().to_vec());
        let state = match (FullDumpRef { gb: &self.gb, sram }).to_rmp() {
            Ok(state) => state,
            Err(err) => {
                log::error!("could not copy the state for autosave: {err:?}");
                return;
            }
        };
        self.last_good.keep(StateCopy {
            store: self.store.clone(),
            rom_key: key,
            state,
            frame: self.gb.frame().as_slice().to_vec(),
            battery,
            keep: self.autosave_keep,
            unsaved: self.play.has_unsaved_play(),
        });
    }

    pub fn delete_snapshot(&mut self, slot: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.delete_snapshot(&key, slot)?;
        }
        Ok(())
    }

    pub fn snapshot_name(&self, slot: usize) -> String {
        self.rom_key
            .as_deref()
            .and_then(|key| self.store.snapshot_name(key, slot))
            .unwrap_or_else(|| crate::storage::default_slot_name(slot))
    }

    pub fn rename_snapshot(&mut self, slot: usize, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.set_snapshot_name(&key, slot, name)?;
        }
        Ok(())
    }

    pub fn default_battery_name(&self) -> String {
        let title = self.gb.cartridge.header.title.trim();
        if title.is_empty() {
            "Battery save".to_string()
        } else {
            title.to_string()
        }
    }

    pub fn rename_battery(&mut self, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store
                .set_battery_name(&key, name, &self.default_battery_name())?;
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
            self.gb
                .put_sram_dump(SramDump::from_slice(&data), now_unix());
            self.save_loaded = true;
            return true;
        }
        false
    }

    pub fn import_save(
        &mut self,
        ram: Option<&[u8]>,
        clock: Option<RtcFooter>,
    ) -> GbResult<SaveImport> {
        if let Some(blocked) = self.import_blocker(clock.is_some()) {
            return Ok(blocked);
        }
        let Some(key) = self.rom_key.clone() else {
            return Ok(SaveImport::NoRom);
        };
        let now = now_unix();
        match (ram, clock) {
            (Some(ram), None) => {
                self.store.store_battery(&key, ram)?;
                self.gb.put_sram_dump(SramDump::from_slice(ram), now);
            }
            (ram, Some(footer)) => {
                if let Some(ram) = ram {
                    self.gb.put_sram_dump(SramDump::from_slice(ram), now);
                }
                self.gb.cartridge.put_rtc_footer(footer, now);
                if let Some(dump) = self.gb.poll_sram_dump(true, now) {
                    self.store.store_battery(&key, dump.as_slice())?;
                }
            }
            (None, None) => return Ok(SaveImport::Nothing),
        }
        self.save_loaded = true;
        self.last_save = Some(web_time::Instant::now());
        Ok(SaveImport::Imported)
    }

    pub fn import_blocker(&self, with_clock: bool) -> Option<SaveImport> {
        if self.rom_key.is_none() {
            Some(SaveImport::NoRom)
        } else if with_clock && self.gb.cartridge.rtc().is_none() {
            Some(SaveImport::NoClock)
        } else {
            None
        }
    }

    pub fn has_been_playing(&self) -> bool {
        self.started_at
            .is_some_and(|started| started.elapsed() >= PLAYING_AFTER)
    }

    pub fn restart(&mut self) {
        self.gb.soft_reset();
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
    }

    pub fn export_rtc_files(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        let footer = self.gb.cartridge.rtc_footer(now_unix())?;
        Some((
            self.gb.cartridge.battery_ram(),
            footer.encode_rtc_file().to_vec(),
        ))
    }

    pub fn export_save_bytes(&mut self) -> Option<Vec<u8>> {
        self.gb
            .poll_sram_dump(true, now_unix())
            .map(|dump| dump.as_slice().to_vec())
    }

    pub fn save_status(&self) -> SaveStatus {
        let key = self.rom_key.as_deref();
        SaveStatus {
            name: key
                .and_then(|k| self.store.battery_name(k))
                .unwrap_or_else(|| self.default_battery_name()),
            battery_backed: self.gb.cartridge.has_battery(),
            stored: key.is_some_and(|k| self.store.has_battery(k)),
            saved_at: key.and_then(|k| self.store.battery_saved_at(k)),
            location: self.store.location(),
            has_clock: self.gb.cartridge.rtc().is_some(),
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
        self.started_at = Some(web_time::Instant::now());
        self.gb.run_cycles(start_cycles);
        self.update_texture(ctx);
    }

    pub fn clear_frame(&mut self, ctx: &egui::Context) {
        self.gb.ppu.clear_frame();
        self.update_texture(ctx);
    }

    fn blend_frame(&mut self) {
        let current_raw = self.gb.frame().as_slice();
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

    fn paint(&self, source: &[u8], scale: usize, out: &mut Vec<u8>) {
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

    fn update_texture(&mut self, ctx: &egui::Context) {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const RAM_LEN: usize = 0x2000;

    fn emulator(name: &str) -> Emulator {
        let root =
            std::env::temp_dir().join(format!("citrine-import-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Emulator {
            store: SaveStore::at(root),
            ..Emulator::default()
        }
    }

    fn rom(cartridge_type: u8) -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = cartridge_type;
        data[0x0149] = 0x02;
        Rom::new(&data)
    }

    fn bytes(seed: u8) -> Vec<u8> {
        (0..RAM_LEN)
            .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
            .collect()
    }

    fn running(emulator: &mut Emulator, cartridge_type: u8) {
        emulator.load_rom(&rom(cartridge_type), None).expect("load");
        emulator.gb.run_cycles(100_000);
    }

    fn played(emulator: &mut Emulator, seconds: f64) {
        let start = web_time::Instant::now();
        emulator.advance(start);
        let mut at = 0.0;
        while at < seconds {
            at += 0.1;
            emulator.advance(start + std::time::Duration::from_secs_f64(at));
        }
    }

    fn write_sram(emulator: &mut Emulator, value: u8) {
        use citrine_gb::WriteMemory;
        emulator.gb.cartridge.write_naive(0x0000, 0x0A);
        emulator.gb.cartridge.write_naive(0xA000, value);
    }

    fn autosaves(emulator: &Emulator) -> Vec<usize> {
        emulator
            .store
            .autosave_ids(emulator.rom_key().expect("key"))
    }

    #[test]
    fn paused_time_does_not_count() {
        let mut emulator = emulator("auto-paused");
        running(&mut emulator, 0x03);
        emulator.running = false;
        played(&mut emulator, 3.0);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);

        emulator.running = true;
        played(&mut emulator, 0.5);
        emulator.autosave_interval = AutosaveInterval::OneMinute;
        assert_eq!(emulator.autosave_if_due().expect("autosave"), None);
        emulator.play.play(std::time::Duration::from_secs(60));
        assert_eq!(emulator.autosave_if_due().expect("autosave"), Some(0));
        assert_eq!(emulator.autosave_if_due().expect("autosave"), None);
    }

    #[test]
    fn closing_autosaves_only_after_play() {
        let mut emulator = emulator("auto-close");
        running(&mut emulator, 0x03);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
        played(&mut emulator, 0.3);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), Some(0));
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
        assert_eq!(autosaves(&emulator), vec![0]);
    }

    #[test]
    fn switching_games_autosaves_the_old_one() {
        let mut emulator = emulator("auto-switch");
        running(&mut emulator, 0x03);
        let first = emulator.rom_key().expect("key").to_owned();
        played(&mut emulator, 0.3);

        let mut other = vec![0u8; 0x8000];
        other[0x0134] = b'B';
        other[0x0147] = 0x03;
        other[0x0149] = 0x02;
        emulator.load_rom(&Rom::new(&other), None).expect("load");

        assert_ne!(emulator.rom_key(), Some(first.as_str()));
        assert_eq!(emulator.store.autosave_ids(&first), vec![0]);
        assert!(autosaves(&emulator).is_empty());
    }

    #[test]
    fn the_keep_limit_drops_the_oldest() {
        let mut emulator = emulator("auto-keep");
        running(&mut emulator, 0x03);
        emulator.autosave_keep = 2;
        for _ in 0..4 {
            emulator.autosave().expect("autosave");
        }
        assert_eq!(autosaves(&emulator), vec![2, 3]);
    }

    #[test]
    fn an_autosave_loads_back_without_touching_snapshots() {
        let mut emulator = emulator("auto-load");
        running(&mut emulator, 0x03);
        emulator.save_snapshot(0).expect("snapshot");
        let key = emulator.rom_key().expect("key").to_owned();
        let snapshot = emulator.store.load_snapshot(&key, 0);

        played(&mut emulator, 0.3);
        write_sram(&mut emulator, 0x11);
        let id = emulator.autosave().expect("autosave").expect("id");
        played(&mut emulator, 0.3);
        write_sram(&mut emulator, 0x22);
        assert!(emulator.load_autosave(id).expect("load"));

        assert_eq!(emulator.gb.cartridge.battery_ram()[0], 0x11);
        assert_eq!(emulator.autosave_if_played().expect("autosave"), None);
        assert_eq!(emulator.store.load_snapshot(&key, 0), snapshot);
        assert_eq!(emulator.store.used_slots(&key), vec![0]);
    }

    #[test]
    fn a_crash_saves_the_last_frame_boundary() {
        let mut emulator = emulator("auto-crash");
        running(&mut emulator, 0x03);
        write_sram(&mut emulator, 0x42);
        played(&mut emulator, 1.2);
        emulator.running = false;
        played(&mut emulator, 0.1);

        assert_eq!(emulator.last_good.write_out(), Ok(Some(0)));
        let key = emulator.rom_key().expect("key").to_owned();
        let state = emulator.store.load_autosave(&key, 0).expect("autosave");
        let rom = Rom::new(&emulator.gb.cartridge.rom_bytes());
        let restored = GameBoy::from_dump(&state, &rom, now_unix()).expect("restores");
        assert_eq!(restored.cpu.pc, emulator.gb.cpu.pc);
        assert_eq!(restored.cartridge.battery_ram()[0], 0x42);
        let battery = emulator.store.load_battery(&key).expect("battery");
        assert_eq!(battery[0], 0x42);
    }

    #[test]
    fn an_imported_save_survives_the_restart() {
        let mut emulator = emulator("save");
        running(&mut emulator, 0x03);
        emulator.import_save(Some(&bytes(1)), None).expect("first");
        emulator.restart();
        emulator.gb.run_cycles(100_000);

        let result = emulator.import_save(Some(&bytes(2)), None).expect("second");
        assert_eq!(result, SaveImport::Imported);
        emulator.restart();
        emulator.gb.run_cycles(100_000);

        assert_eq!(emulator.gb.cartridge.battery_ram(), bytes(2));
        let key = emulator.rom_key().expect("key").to_owned();
        assert_eq!(emulator.store.load_battery(&key), Some(bytes(2)));
        assert!(emulator.running);
        assert!(!emulator.has_been_playing());
    }

    #[test]
    fn an_imported_clock_survives_the_restart() {
        let mut emulator = emulator("clock");
        running(&mut emulator, 0x10);
        let mut footer = emulator.gb.cartridge.rtc_footer(now_unix()).expect("clock");
        footer.real.seconds = 12;
        footer.real.minutes = 34;
        footer.real.hours = 5;
        footer.real.days_low = 6;
        footer.real.days_high = 0x40;
        footer.latched = footer.real;

        let result = emulator
            .import_save(Some(&bytes(3)), Some(footer))
            .expect("import");
        assert_eq!(result, SaveImport::Imported);
        emulator.restart();
        emulator.gb.run_cycles(100_000);

        assert_eq!(emulator.gb.cartridge.battery_ram(), bytes(3));
        let clock = emulator.gb.cartridge.rtc_footer(now_unix()).expect("clock");
        assert_eq!(clock.real, footer.real);
    }

    #[test]
    fn a_clock_needs_a_cart_with_one() {
        let mut emulator = emulator("no-clock");
        assert_eq!(emulator.import_blocker(false), Some(SaveImport::NoRom));
        running(&mut emulator, 0x03);
        assert_eq!(emulator.import_blocker(false), None);
        assert_eq!(emulator.import_blocker(true), Some(SaveImport::NoClock));
    }

    #[test]
    fn a_screenshot_lands_in_the_media_store() {
        use crate::media::{MediaKind, MediaStore, save_screenshot};
        let root = std::env::temp_dir().join(format!("citrine-shot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut media = MediaStore::at(root.clone());
        let mut emulator = emulator("screenshot");
        assert!(save_screenshot(&emulator, &mut media, 3).is_err());

        running(&mut emulator, 0x00);
        let item = save_screenshot(&emulator, &mut media, 3).expect("saved");
        assert_eq!(item.info.kind, MediaKind::Screenshot);
        assert_eq!((item.info.width, item.info.height), (480, 432));
        assert!(media.path(&item.id).starts_with(root.join("screenshots")));
        assert!(media.path(&item.id).is_file());
        assert!(item.thumbnail.is_some());
        assert_eq!(MediaStore::at(root.clone()).items().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }
}
