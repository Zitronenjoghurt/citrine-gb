use crate::app::ui_state::keybinds::{BindingCapture, KeyBindings, PadButton};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::audio::mix::{Mixer, SoundLevels, level};
use crate::audio::pacer::{AudioFeed, FrameClock};
use crate::autosave::{AutosaveInterval, LastGood, PlayClock};
use crate::controller_debug::PadLog;
use crate::held::{Change, HeldButtons, InputSource};
use crate::local_link::LocalLink;
use crate::media::recording::ScreenRecorder;
use crate::online::OnlineSettings;
use crate::pad_input::PadSource;
use crate::partner_link::PartnerLink;
use crate::play::{DropHistory, GameFile, LinkedSave, PlaySession, SaveKey};
use crate::recorder::InputRecorder;
use crate::storage::{SaveStore, now_unix};
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::console::{Console, ConsoleKind, Keys, Timing, VideoSpec};
use citrine_gb::debug::Debug;
use citrine_gb::debug::step::{Step, Stop};
use citrine_gb::error::GbResult;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;
use link::borrowed_key;

mod audio;
pub mod gimmicks;
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
const FAST_FORWARD: usize = 3;

pub struct Emulator {
    machine: Machine,
    audio: Vec<f32>,
    pub running: bool,
    pub fast_forward: bool,
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
    pub reserve: f32,
    held: HeldButtons,
    pressed_by: Option<InputSource>,
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
    linked_video: VideoSpec,
    stepped: bool,
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
    drop_history: Option<DropHistory>,
    handed_back: Option<citrine_gb_net::OnlineId>,
    console_switched: Option<GbModel>,
    pub link: Option<LocalLink>,
    pub partner: Option<PartnerLink>,
    stat_events: StatEvents,
    pub gimmicks: gimmicks::Gimmicks,
    gimmick_settings: gimmicks::GimmickSettings,
    pub firmware: Firmware,
    pub gba_bios_name: String,
}

impl Default for Emulator {
    fn default() -> Self {
        let machine = Machine::Gb(Box::new(GameBoy::new_empty(GbModel::Dmg)));
        let video = machine.video();
        Self {
            machine,
            audio: Vec::new(),
            running: true,
            fast_forward: false,
            audio_feed: None,
            enable_matrix: true,
            matrix_edge_brightness: 0.85,
            matrix_corner_brightness: 0.75,
            enable_ghosting: true,
            ghosting_blend: 0.7,
            scaling: ScalingMode::default(),
            reserve: 0.0,
            held: HeldButtons::default(),
            pressed_by: None,
            texture: None,
            upscale_buffer: Vec::new(),
            last_update: None,
            clock: FrameClock::default(),
            render_scale: 3,
            update_avg_timer: AvgTimer::default(),
            frame_avg_timer: AvgTimer::default(),
            total_frames: 0,
            last_frame: vec![0; video.width * video.height * 4],
            linked_frame: None,
            linked_video: video,
            stepped: false,
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
            drop_history: None,
            handed_back: None,
            console_switched: None,
            link: None,
            partner: None,
            stat_events: StatEvents::default(),
            gimmicks: gimmicks::Gimmicks::default(),
            gimmick_settings: gimmicks::GimmickSettings::default(),
            firmware: Firmware::default(),
            gba_bios_name: String::new(),
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
        if !self.has_game() {
            return None;
        }
        Some(GameFile {
            rom: self.machine.rom_bytes(),
            title: self.game_name.clone(),
            kind: self.machine.kind(),
            save: self
                .machine
                .battery(true, now_unix())
                .map(|save| save.to_file()),
            running: true,
        })
    }

    pub fn borrowed_save(&self, hash: &citrine_gb_net::GameHash) -> Option<Vec<u8>> {
        self.store.load_battery(&borrowed_key(hash))
    }

    pub fn battery_for(&self, rom: &[u8]) -> Option<Vec<u8>> {
        let key = citrine_gb::rom::id::RomId::new(rom).sha256_hex_string();
        self.store.load_battery(&key)
    }

    pub fn open_game(&self) -> Option<citrine_gb_net::GameHash> {
        self.has_game().then(|| self.machine.rom_id().sha1)
    }

    pub(crate) fn advance(
        &mut self,
        now: web_time::Instant,
        mut cable: Option<&mut PlaySession>,
    ) -> bool {
        if self.last_good.take_written() {
            self.play.copy_written();
        }
        if !self.running {
            self.gimmicks.stop_rumble();
            self.drop_audio();
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

        self.gimmicks.follow_clock(&mut self.machine, now_unix());
        self.gimmicks.feed(&mut self.machine);
        let mut ran_frame = false;
        let mut heard = self.audio.len();
        let due = self.clock.advance(dt, self.machine.timing());
        let frames = if self.fast_forward {
            due * FAST_FORWARD
        } else {
            due
        };
        for _ in 0..frames {
            if let Some(session) = cable.as_deref_mut()
                && let Some(gb) = self.machine.as_gb_mut()
            {
                session.pump_link(gb);
            }
            self.feed_replay();
            self.frame_avg_timer.start();
            match (self.link.as_mut(), self.partner.as_mut()) {
                (Some(link), _) => link.run_frame(&mut self.machine, self.held.held()),
                (None, Some(partner)) => match self.machine.as_gb_mut() {
                    Some(gb) => partner.run_frame(gb),
                    None => self.machine.run_frame(),
                },
                (None, None) => self.machine.run_frame(),
            }
            self.frame_avg_timer.stop();
            self.gimmicks.collect(&mut self.machine);
            let owed = self.held.take_owed();
            self.apply(Change {
                pressed: Keys::empty(),
                released: owed,
            });
            ran_frame = true;
            heard = self.capture_frame(heard);

            if self.take_breakpoint_hit() {
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
        self.load_game(
            rom,
            model.into(),
            #[cfg(not(target_arch = "wasm32"))]
            path,
        )
    }

    pub fn load_game(
        &mut self,
        rom: &Rom,
        kind: ConsoleKind,
        #[cfg(not(target_arch = "wasm32"))] path: Option<&std::path::Path>,
    ) -> GbResult<()> {
        self.screen_recorder.stop();
        self.replay = None;
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave before switching games: {err:?}");
        }
        self.running = false;
        self.fast_forward = false;
        self.save_loaded = false;
        self.imported_legacy_save = false;
        self.console_switched = None;
        if let (Some(partner), Some(gb)) = (self.partner.take(), self.machine.as_gb_mut()) {
            partner.finish(gb);
        }
        match (GbModel::try_from(kind), self.machine.as_gb_mut()) {
            (Ok(model), Some(gb)) => gb.load_rom_with_model(rom, model)?,
            _ => self.replace_machine(Machine::load(rom, kind, &self.firmware)?),
        }
        self.sync_joypad();

        let key = self.machine.rom_id().sha256_hex_string();
        if let Err(err) = self.store.set_game_console(&key, self.machine.kind()) {
            log::warn!("could not note the console of the game: {err}");
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.rom_path = path.map(|p| p.to_owned());
            if let Some(path) = path {
                self.imported_legacy_save = self.store.import_legacy_save_if_new(&key, path);
            }
        }

        if let Some(save) = self.store.load_battery_save(&key, &self.machine) {
            self.machine.put_battery(save, now_unix());
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
        self.start_gimmicks();
        Ok(())
    }

    pub fn kind(&self) -> ConsoleKind {
        self.machine.kind()
    }

    pub fn console(&self) -> GbModel {
        self.as_gb().map_or(GbModel::Dmg, |gb| gb.model)
    }

    pub fn debug(&self) -> &Machine {
        &self.machine
    }

    pub fn debug_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }

    #[cfg(all(test, feature = "gba"))]
    pub fn load_gba(&mut self, data: &[u8]) -> GbResult<()> {
        self.screen_recorder.stop();
        self.replay = None;
        self.running = false;
        self.fast_forward = false;
        self.console_switched = None;
        let machine = Machine::load(&Rom::new(data), ConsoleKind::Gba, &self.firmware)?;
        self.replace_machine(machine);
        Ok(())
    }

    #[cfg(feature = "gba")]
    pub fn gba_bios_name(&self) -> Option<&str> {
        self.firmware
            .gba_bios
            .as_ref()
            .map(|_| self.gba_bios_name.as_str())
    }

    #[cfg(feature = "gba")]
    pub fn gba_bios_kind(&self) -> citrine_gb::gba::bios::BiosKind {
        self.firmware
            .gba_bios
            .as_ref()
            .map_or(citrine_gb::gba::bios::BiosKind::BuiltIn, |bios| bios.kind())
    }

    #[cfg(feature = "gba")]
    pub fn as_gba(&self) -> Option<&citrine_gb::gba::machine::Gba> {
        match &self.machine {
            Machine::Gba(gba) => Some(gba),
            Machine::Gb(_) => None,
        }
    }

    #[cfg(feature = "gba")]
    pub fn set_gba_color_correction(
        &mut self,
        correction: citrine_gb::gb::ppu::types::color::ColorCorrection,
    ) {
        if let Machine::Gba(gba) = &mut self.machine {
            gba.bus_mut().ppu_mut().set_color_correction(correction);
        }
    }

    pub fn as_gb(&self) -> Option<&GameBoy> {
        self.machine.as_gb()
    }

    pub fn as_gb_mut(&mut self) -> Option<&mut GameBoy> {
        self.machine.as_gb_mut()
    }

    pub fn debug_console(&self) -> &dyn Debug {
        &self.machine
    }

    pub fn has_game(&self) -> bool {
        self.as_gb().is_none_or(|gb| gb.cartridge.has_rom_loaded)
    }

    pub fn video(&self) -> VideoSpec {
        match self.linked_frame {
            Some(_) => self.linked_video,
            None => self.machine.video(),
        }
    }

    pub fn timing(&self) -> Timing {
        self.machine.timing()
    }

    pub fn frame(&self) -> &[u8] {
        self.machine.frame()
    }

    #[cfg(test)]
    pub(crate) fn gb(&self) -> &GameBoy {
        self.as_gb().expect("a game boy")
    }

    #[cfg(test)]
    pub(crate) fn gb_mut(&mut self) -> &mut GameBoy {
        self.as_gb_mut().expect("a game boy")
    }

    #[cfg(all(test, feature = "gba"))]
    pub(crate) fn put_gba(&mut self, rom: &[u8], rom_key: &str) {
        let gba = citrine_gb::gba::machine::Gba::new(rom);
        self.replace_machine(Machine::Gba(Box::new(gba)));
        self.rom_key = Some(rom_key.to_owned());
        self.running = true;
    }

    fn take_breakpoint_hit(&mut self) -> bool {
        self.machine.take_breakpoint_hit()
    }

    pub fn take_console_switch(&mut self) -> Option<GbModel> {
        self.console_switched.take()
    }

    pub fn restart_on(&mut self, model: GbModel) -> GbResult<()> {
        if self.rom_key.is_none() {
            return Ok(());
        }
        self.flush_save()?;
        let Some(gb) = self.as_gb() else {
            return Ok(());
        };
        let rom = Rom::new(&gb.cartridge.rom_bytes());
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
            gimmick_settings: std::mem::take(&mut self.gimmick_settings),
            firmware: std::mem::take(&mut self.firmware),
            gba_bios_name: std::mem::take(&mut self.gba_bios_name),
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

    pub fn is_running(&self) -> bool {
        self.running || self.step_pending()
    }

    pub fn toggle_running(&mut self) {
        if self.machine.step_pending() {
            self.machine.cancel_step();
            return;
        }
        self.running = !self.running;
        if !self.running {
            self.release_all();
        }
    }

    pub fn step(&mut self, step: Step) -> Stop {
        if self.running {
            self.running = false;
            self.release_all();
        }
        let stop = Debug::step(&mut self.machine, step);
        self.stepped = true;
        stop
    }

    fn continue_step(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.stepped) {
            self.update_texture(ctx);
        }
        if self.running || !self.step_pending() {
            return;
        }
        self.machine.resume_step();
        self.update_texture(ctx);
        ctx.request_repaint();
    }

    fn step_pending(&self) -> bool {
        self.machine.step_pending()
    }

    fn replace_machine(&mut self, machine: Machine) {
        let enabled: [bool; 4] = std::array::from_fn(|index| {
            self.as_gb().is_none_or(|gb| gb.apu.channel_enabled(index))
        });
        let same_channels = self.machine.audio_channels().len() == machine.audio_channels().len();
        self.machine = machine;
        self.drop_history = None;
        if same_channels {
            for (index, enabled) in enabled.into_iter().enumerate() {
                self.machine.set_channel_enabled(index, enabled);
            }
        }
        self.start_gimmicks();
    }

    pub fn soft_reset(&mut self, ctx: &egui::Context) {
        self.machine.soft_reset();
        self.update_texture(ctx);
    }

    pub fn reset_to(&mut self, ctx: &egui::Context, start_cycles: u32) {
        self.machine.soft_reset();
        self.started_at = Some(web_time::Instant::now());
        if let Some(gb) = self.as_gb_mut() {
            gb.run_for(start_cycles);
        } else {
            Debug::step(&mut self.machine, Step::Cycles(start_cycles));
        }
        self.update_texture(ctx);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
