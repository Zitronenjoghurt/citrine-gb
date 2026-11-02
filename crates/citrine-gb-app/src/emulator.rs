use crate::app::ui_state::keybinds::{BindingCapture, KeyBindings, PadButton};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::audio::mix::{Mixer, SoundLevels, level};
use crate::audio::pacer::{AudioFeed, FrameClock};
use crate::autosave::{AutosaveInterval, LastGood, PlayClock};
use crate::controller_debug::PadLog;
use crate::held::{Change, HeldButtons};
use crate::local_link::LocalLink;
use crate::media::recording::ScreenRecorder;
use crate::online::OnlineSettings;
use crate::pad_input::PadSource;
use crate::partner_link::PartnerLink;
use crate::play::{GameFile, LinkedSave, PlaySession, SaveKey};
use crate::recorder::InputRecorder;
use crate::storage::{SaveStore, now_unix};
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::error::GbResult;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use link::borrowed_key;

mod audio;
mod input;
mod link;
mod replay;
mod saves;
mod screen;
mod snapshots;

const SAVE_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);
const PLAYING_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_PLAY_STEP: f64 = 0.25;

#[derive(Debug, Default)]
pub struct StatEvents {
    pub launched: bool,
    pub snapshots: u32,
    pub loads: u32,
    pub played: std::time::Duration,
    pub together: Option<TogetherPlay>,
}

#[derive(Debug)]
pub struct TogetherPlay {
    pub rom: String,
    pub name: String,
    pub played: std::time::Duration,
}

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
    held: HeldButtons,
    texture: Option<egui::TextureHandle>,
    upscale_buffer: Vec<u8>,
    last_update: Option<web_time::Instant>,
    clock: FrameClock,
    pub render_scale: usize,
    pub update_avg_timer: AvgTimer,
    pub frame_avg_timer: AvgTimer,
    pub total_frames: u128,
    last_frame: Vec<u8>,
    linked_frame: Option<Vec<u8>>,
    linked_audio: Vec<f32>,
    friend_audio: Vec<f32>,
    mixer: Mixer,
    pub sound: SoundLevels,
    /// SHA-256 of the loaded ROM
    rom_key: Option<String>,
    game_name: String,
    loose_presses: Vec<PadButton>,
    #[cfg(not(target_arch = "wasm32"))]
    rom_path: Option<std::path::PathBuf>,
    pub imported_legacy_save: bool,
    pub store: SaveStore,
    pub last_save: Option<web_time::Instant>,
    pub save_loaded: bool,
    started_at: Option<web_time::Instant>,
    pub recorder: InputRecorder,
    pub replay: Option<crate::replay::ReplayPlayer>,
    pub screen_recorder: ScreenRecorder,
    pub autosave_interval: AutosaveInterval,
    pub autosave_keep: usize,
    play: PlayClock,
    pub last_good: LastGood,
    console_switched: Option<GbModel>,
    pub link: Option<LocalLink>,
    pub partner: Option<PartnerLink>,
    stat_events: StatEvents,
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
            held: HeldButtons::default(),
            texture: None,
            upscale_buffer: Vec::new(),
            last_update: None,
            clock: FrameClock::default(),
            render_scale: 3,
            update_avg_timer: AvgTimer::default(),
            frame_avg_timer: AvgTimer::default(),
            total_frames: 0,
            last_frame: vec![0; GB_WIDTH * GB_HEIGHT * 4],
            linked_frame: None,
            linked_audio: Vec::new(),
            friend_audio: Vec::new(),
            mixer: Mixer::default(),
            sound: SoundLevels {
                mine: level(Settings::DEFAULT_VOLUME, false),
                friend: level(OnlineSettings::DEFAULT_FRIEND_VOLUME, false),
            },
            rom_key: None,
            game_name: String::new(),
            loose_presses: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            rom_path: None,
            imported_legacy_save: false,
            store: SaveStore::default(),
            last_save: None,
            save_loaded: false,
            started_at: None,
            recorder: InputRecorder::default(),
            replay: None,
            screen_recorder: ScreenRecorder::default(),
            autosave_interval: AutosaveInterval::default(),
            autosave_keep: crate::autosave::DEFAULT_KEEP,
            play: PlayClock::default(),
            last_good: LastGood::default(),
            console_switched: None,
            link: None,
            partner: None,
            stat_events: StatEvents::default(),
        }
    }
}

impl Emulator {
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
    ) -> GbResult<()> {
        self.update_linked(ctx, pads, bindings, capture, log, None)
    }

    fn store_saves(&mut self, saves: Vec<LinkedSave>) -> GbResult<()> {
        let mut stored: Vec<&SaveKey> = Vec::new();
        for save in saves.iter().rev() {
            if stored.contains(&&save.key) {
                continue;
            }
            stored.push(&save.key);
            let key = match &save.key {
                SaveKey::Borrowed(hash) => borrowed_key(hash),
                SaveKey::Rom(key) => key.clone(),
            };
            self.store.store_battery(&key, save.dump.as_slice())?;
        }
        Ok(())
    }

    pub fn game_file(&mut self) -> Option<GameFile> {
        if !self.gb.cartridge.has_rom_loaded {
            return None;
        }
        Some(GameFile {
            rom: self.gb.cartridge.rom_bytes(),
            title: self.game_name.clone(),
            model: self.console(),
            save: self
                .gb
                .poll_sram_dump(true, now_unix())
                .map(|dump| dump.as_slice().to_vec()),
            running: true,
        })
    }

    pub fn borrowed_save(&self, hash: &citrine_gb_net::GameHash) -> Option<Vec<u8>> {
        self.store.load_battery(&borrowed_key(hash))
    }

    pub fn battery_for(&self, rom: &[u8]) -> Option<Vec<u8>> {
        let header = Rom::new(rom).header().ok()?;
        self.store.load_battery(&header.sha256_hex_string())
    }

    pub fn open_game(&self) -> Option<citrine_gb_net::GameHash> {
        let cartridge = &self.gb.cartridge;
        cartridge.has_rom_loaded.then_some(cartridge.header.sha1)
    }

    pub(crate) fn advance(
        &mut self,
        now: web_time::Instant,
        mut cable: Option<&mut PlaySession>,
    ) -> bool {
        if !self.running {
            self.gb.apu.audio_buffer.clear();
            if let Some(link) = &mut self.link {
                link.clear_audio();
            }
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
            if let Some(session) = cable.as_deref_mut() {
                session.pump_link(&mut self.gb);
            }
            self.feed_replay();
            self.frame_avg_timer.start();
            match (self.link.as_mut(), self.partner.as_mut()) {
                (Some(link), _) => link.run_frame(&mut self.gb, self.held.held()),
                (None, Some(partner)) => partner.run_frame(&mut self.gb),
                (None, None) => self.gb.run_frame(),
            }
            self.frame_avg_timer.stop();
            let owed = self.held.take_owed();
            self.apply(Change {
                pressed: JoypadState::empty(),
                released: owed,
            });
            ran_frame = true;
            heard = self.capture_frame(heard);

            if self.gb.debugger.hit_breakpoint {
                self.gb.debugger.hit_breakpoint = false;
                self.running = false;
                break;
            }
        }

        let step = std::time::Duration::from_secs_f64(dt.min(MAX_PLAY_STEP));
        self.play.play(step);
        self.stat_events.played += step;
        if self.play.copy_due(false) {
            self.copy_last_good();
        }
        ran_frame
    }

    pub fn load_rom(
        &mut self,
        rom: &Rom,
        model: GbModel,
        #[cfg(not(target_arch = "wasm32"))] path: Option<&std::path::Path>,
    ) -> GbResult<()> {
        self.screen_recorder.stop();
        self.replay = None;
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave before switching games: {err:?}");
        }
        self.running = false;
        self.save_loaded = false;
        self.imported_legacy_save = false;
        self.console_switched = None;
        if let Some(partner) = self.partner.take() {
            partner.finish(&mut self.gb);
        }
        self.gb.load_rom_with_model(rom, model)?;
        self.sync_joypad();

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
        self.name_game(None);
        self.stat_events.launched = true;
        self.last_save = None;
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.last_good.clear();
        Ok(())
    }

    pub fn console(&self) -> GbModel {
        self.gb.model
    }

    pub fn take_console_switch(&mut self) -> Option<GbModel> {
        self.console_switched.take()
    }

    pub fn restart_on(&mut self, model: GbModel) -> GbResult<()> {
        if self.rom_key.is_none() {
            return Ok(());
        }
        self.flush_save()?;
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let name = self.game_name.clone();
        let launched = self.stat_events.launched;
        #[cfg(not(target_arch = "wasm32"))]
        let path = self.rom_path.clone();
        self.load_rom(
            &rom,
            model,
            #[cfg(not(target_arch = "wasm32"))]
            path.as_deref(),
        )?;
        self.game_name = name;
        self.stat_events.launched = launched;
        Ok(())
    }

    pub fn unload(&mut self) {
        self.screen_recorder.stop();
        self.last_good.clear();
        *self = Self {
            store: self.store.clone(),
            last_good: self.last_good.clone(),
            audio_feed: self.audio_feed.take(),
            sound: self.sound,
            running: false,
            ..Self::default()
        };
    }

    pub fn rom_key(&self) -> Option<&str> {
        self.rom_key.as_deref()
    }

    pub fn screen_texture(&self) -> Option<egui::TextureId> {
        self.texture.as_ref().map(|t| t.id())
    }

    pub fn take_stat_events(&mut self) -> StatEvents {
        std::mem::take(&mut self.stat_events)
    }

    pub fn force_step(&mut self, ctx: &egui::Context, cycle_count: u32) {
        self.gb.step_cycles(cycle_count);
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
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
