use crate::app::ui_state::keybinds::{BindingCapture, KeyBindings, PadButton};
use crate::app::ui_state::settings::{ScalingMode, Settings};
use crate::audio::even_clock::EvenClock;
use crate::audio::mix::{SoundLevels, level};
use crate::autosave::{AutosaveInterval, LastGood, PlayClock};
use crate::controller_debug::PadLog;
use crate::held::{HeldButtons, InputSource};
use crate::local_link::LocalLink;
use crate::media::recording::ScreenRecorder;
use crate::online::OnlineSettings;
use crate::pad_input::PadSource;
use crate::partner_link::PartnerLink;
use crate::play::{DropHistory, GameFile, LinkedSave, PlaySession, SaveKey};
use crate::recorder::InputRecorder;
use crate::storage::{Outcome, SaveStore, Write, Writes, now_unix};
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::console::{Console, ConsoleKind, Keys, Timing, VideoSpec};
use citrine_gb::debug::Debug;
use citrine_gb::debug::step::{Step, Stop};
use citrine_gb::error::GbResult;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::machine::{Firmware, Machine};
use citrine_gb::rom::Rom;
use link::borrowed_key;
use runner::{Core, Effects, Hooks, Plan, Ran, Taken};
use seat::Seat;

#[cfg(not(target_arch = "wasm32"))]
mod achievements;
mod audio;
pub mod fast_forward;
pub mod gimmicks;
#[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
mod gpu_clock;
mod input;
#[cfg(feature = "nds")]
pub mod jit_cache;
pub mod layout;
mod link;
pub mod load;
mod replay;
pub mod rewind;
mod runner;
mod saves;
mod screen;
mod seat;
#[cfg(threaded)]
mod show_lag;
mod snapshots;
#[cfg(threaded)]
mod ui_wait;
#[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
mod upscale;
#[cfg(feature = "nds")]
mod walker;

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
    ClockInSave,
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
#[cfg(all(threaded, not(target_arch = "wasm32")))]
const FRAME_WAIT: std::time::Duration = std::time::Duration::from_millis(4);
#[cfg(all(threaded, target_arch = "wasm32"))]
const FRAME_WAIT: std::time::Duration = std::time::Duration::ZERO;
#[cfg(all(threaded, not(target_arch = "wasm32")))]
const SOLO_WAIT: std::time::Duration = std::time::Duration::from_micros(16_667);
#[cfg(all(threaded, target_arch = "wasm32"))]
const SOLO_WAIT: std::time::Duration = std::time::Duration::ZERO;

pub struct Emulator {
    seat: Seat,
    lent: Option<f64>,
    lending: bool,
    want_copy: bool,
    ready: Option<Taken>,
    session_lent: Option<bool>,
    #[cfg(threaded)]
    frame_wait: std::time::Duration,
    #[cfg(threaded)]
    solo_wait: std::time::Duration,
    #[cfg(threaded)]
    ui_wait: ui_wait::UiWait,
    #[cfg(threaded)]
    late_frames: u64,
    #[cfg(threaded)]
    console_waited: std::time::Duration,
    #[cfg(threaded)]
    show_pace: show_lag::ShowPace,
    #[cfg(threaded)]
    away_keys: Vec<(egui::Key, bool, egui::Modifiers)>,
    away_dt: f64,
    refreshes: Option<u64>,
    away_due: usize,
    #[cfg(all(threaded, feature = "nds"))]
    jit_spawner: Option<citrine_gb_threads::Spawner>,
    pub running: bool,
    pub fast_forward: bool,
    pub fast_forward_speed: fast_forward::FastForwardSpeed,
    pub match_screen: bool,
    pub rewind_length: rewind::RewindLength,
    pub rewind_held: bool,
    pub together: bool,
    rewinding: bool,
    pub enable_matrix: bool,
    /// 0.0 to 1.0 (e.g., 0.85 = 15% darker)
    pub matrix_edge_brightness: f32,
    /// 0.0 to 1.0 (e.g., 0.75 = 25% darker)
    pub matrix_corner_brightness: f32,
    pub enable_ghosting: bool,
    /// 0.0 (pure smear) to 1.0 (instant, no ghosting)
    pub ghosting_blend: f32,
    pub scaling: ScalingMode,
    pub same_size_pixels: bool,
    pub ds_screens: crate::app::ui_state::ds_screens::DsScreens,
    pub screen_context: crate::app::ui_state::ds_screens::ScreenContext,
    shown_layout: Option<layout::ScreenLayout>,
    placed_screens: Vec<egui::Rect>,
    placed_touch: Option<egui::Rect>,
    pub ds_3d: crate::app::ui_state::ds_3d::Ds3d,
    pub ds_jit: bool,
    load: load::Load,
    #[cfg(feature = "nds")]
    jit_cache: Option<std::sync::Arc<citrine_gb::nds::machine::JitCache>>,
    #[cfg(feature = "nds")]
    jit_saved: Option<web_time::Instant>,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    gpu: Option<(egui::Context, eframe::egui_wgpu::RenderState)>,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    upscaler: Option<upscale::Upscaler>,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    upscaled: Option<upscale::Upscaled>,
    held: HeldButtons,
    pressed_by: Option<InputSource>,
    texture: Option<egui::TextureHandle>,
    screen_image: Option<std::sync::Arc<egui::ColorImage>>,
    linear: bool,
    last_update: Option<web_time::Instant>,
    clock: EvenClock,
    pub render_scale: usize,
    pub update_avg_timer: AvgTimer,
    pub frame_avg_timer: AvgTimer,
    pub total_frames: u128,
    touch_seen: Option<Option<citrine_gb::console::Touch>>,
    touch: Option<citrine_gb::console::Touch>,
    touch_owed: bool,
    pub screen_away: Option<usize>,
    linked_frame: Option<Vec<u8>>,
    linked_video: VideoSpec,
    linked_kind: ConsoleKind,
    painted_early: bool,
    stepped: bool,
    pub sound: SoundLevels,
    /// SHA-256 of the loaded ROM
    rom_key: Option<String>,
    download_key: Option<String>,
    cheats_for: Option<(String, Vec<crate::cheats::SavedCheat>)>,
    game_name: String,
    loose_presses: Vec<PadButton>,
    loose_held: Vec<PadButton>,
    #[cfg(not(target_arch = "wasm32"))]
    rom_path: Option<std::path::PathBuf>,
    pub imported_legacy_save: bool,
    pub store: SaveStore,
    pub writes: Writes,
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
    stat_events: StatEvents,
    pub gimmicks: gimmicks::Gimmicks,
    gimmick_settings: gimmicks::GimmickSettings,
    #[cfg(feature = "nds")]
    pub pokewalker: crate::walker::Pokewalker,
    #[cfg(feature = "nds")]
    walker_linked: bool,
    #[cfg(feature = "nds")]
    walker_ms: Option<u64>,
    #[cfg(feature = "nds")]
    walker_copy: Option<citrine_gb_walker::Walker>,
    #[cfg(feature = "nds")]
    walker_watched: bool,
    #[cfg(feature = "nds")]
    ds_clock: i64,
    #[cfg(feature = "nds")]
    ds_clock_set: Option<i64>,
    steps_in_game: bool,
    #[cfg(feature = "nds")]
    game_steps: crate::walker::GameSteps,
    #[cfg(feature = "nds")]
    walker_clock: fn() -> u64,
    #[cfg(ds_wifi)]
    pub ds_wifi: crate::ds_wifi::DsWifi,
    pub firmware: Firmware,
    pub gba_bios_name: String,
    #[cfg(feature = "nds")]
    pub ds_file_names: [String; 3],
}

impl Default for Emulator {
    fn default() -> Self {
        let seat = Seat::default();
        let video = seat.facts().video;
        Self {
            seat,
            lent: None,
            lending: false,
            want_copy: false,
            ready: None,
            session_lent: None,
            #[cfg(threaded)]
            frame_wait: FRAME_WAIT,
            #[cfg(threaded)]
            solo_wait: SOLO_WAIT,
            #[cfg(threaded)]
            ui_wait: ui_wait::UiWait::default(),
            #[cfg(threaded)]
            late_frames: 0,
            #[cfg(threaded)]
            console_waited: std::time::Duration::ZERO,
            #[cfg(threaded)]
            show_pace: show_lag::ShowPace::default(),
            #[cfg(threaded)]
            away_keys: Vec::new(),
            away_dt: 0.0,
            refreshes: None,
            away_due: 0,
            #[cfg(all(threaded, feature = "nds"))]
            jit_spawner: None,
            running: true,
            fast_forward: false,
            fast_forward_speed: fast_forward::FastForwardSpeed::default(),
            match_screen: true,
            rewind_length: rewind::RewindLength::default(),
            rewind_held: false,
            together: false,
            rewinding: false,
            enable_matrix: true,
            matrix_edge_brightness: 0.85,
            matrix_corner_brightness: 0.75,
            enable_ghosting: true,
            ghosting_blend: 0.7,
            scaling: ScalingMode::default(),
            same_size_pixels: false,
            ds_screens: Default::default(),
            screen_context: Default::default(),
            shown_layout: None,
            placed_screens: Vec::new(),
            placed_touch: None,
            ds_3d: Default::default(),
            ds_jit: crate::app::ui_state::settings::DS_JIT_DEFAULT,
            load: load::Load::default(),
            #[cfg(feature = "nds")]
            jit_cache: None,
            #[cfg(feature = "nds")]
            jit_saved: None,
            #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
            gpu: None,
            #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
            upscaler: None,
            #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
            upscaled: None,
            held: HeldButtons::default(),
            pressed_by: None,
            texture: None,
            screen_image: None,
            linear: false,
            last_update: None,
            clock: EvenClock::default(),
            render_scale: 3,
            update_avg_timer: AvgTimer::default(),
            frame_avg_timer: AvgTimer::default(),
            total_frames: 0,
            touch_seen: None,
            touch: None,
            touch_owed: false,
            screen_away: None,
            linked_frame: None,
            linked_video: video,
            linked_kind: ConsoleKind::Gb,
            painted_early: false,
            stepped: false,
            sound: SoundLevels {
                mine: level(Settings::DEFAULT_VOLUME, false),
                friend: level(OnlineSettings::DEFAULT_FRIEND_VOLUME, false),
                slow: crate::audio::pacer::SlowSound::default(),
            },
            rom_key: None,
            download_key: None,
            cheats_for: None,
            game_name: String::new(),
            loose_presses: Vec::new(),
            loose_held: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            rom_path: None,
            imported_legacy_save: false,
            store: SaveStore::default(),
            writes: Writes::default(),
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
            stat_events: StatEvents::default(),
            gimmicks: gimmicks::Gimmicks::default(),
            gimmick_settings: gimmicks::GimmickSettings::default(),
            #[cfg(feature = "nds")]
            pokewalker: crate::walker::Pokewalker::default(),
            #[cfg(feature = "nds")]
            walker_linked: false,
            #[cfg(feature = "nds")]
            walker_ms: None,
            #[cfg(feature = "nds")]
            walker_copy: None,
            #[cfg(feature = "nds")]
            walker_watched: false,
            #[cfg(feature = "nds")]
            ds_clock: 0,
            #[cfg(feature = "nds")]
            ds_clock_set: None,
            steps_in_game: false,
            #[cfg(feature = "nds")]
            game_steps: Default::default(),
            #[cfg(feature = "nds")]
            walker_clock: crate::storage::now_unix,
            #[cfg(ds_wifi)]
            ds_wifi: Default::default(),
            firmware: Firmware::default(),
            gba_bios_name: String::new(),
            #[cfg(feature = "nds")]
            ds_file_names: Default::default(),
        }
    }
}

impl Emulator {
    #[cfg(feature = "nds")]
    pub fn ds_user(&self, mac: [u8; 6]) -> citrine_gb_net::DsUser {
        let firmware = &self.firmware.nds;
        let mut user = crate::play::ds_user_with(firmware.online.mac.unwrap_or(mac));
        if let Some(settings) = &firmware.user_settings {
            user.nickname = settings.nickname.clone();
            user.message = settings.message.clone();
            user.color = settings.favorite_color;
            user.birthday = settings.birthday;
            user.language = settings.language as u8;
        }
        user
    }

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
            self.writes.send(Write::BatteryFile {
                store: self.store.clone(),
                key,
                ram: save.dump.as_slice().to_vec(),
            });
        }
        Ok(())
    }

    pub fn game_file(&mut self) -> Option<GameFile> {
        if !self.has_game() || self.holds_download() {
            return None;
        }
        let machine = &self.seat.core().machine;
        let mut file = GameFile::new(
            machine.rom_bytes(),
            machine.game_sha1(),
            self.game_name.clone(),
            self.seat.facts().kind,
        );
        file.save = self
            .seat
            .core_mut()
            .machine
            .battery(true, now_unix())
            .map(|save| save.to_file());
        file.running = true;
        Some(file)
    }

    pub fn borrowed_save(&self, hash: &citrine_gb_net::GameHash) -> Option<Vec<u8>> {
        self.store.load_battery(&borrowed_key(hash))
    }

    pub fn battery_for(&self, rom: &[u8]) -> Option<Vec<u8>> {
        let key = citrine_gb::rom::id::RomId::new(rom).sha256_hex_string();
        self.store.load_battery(&key)
    }

    pub fn open_game(&self) -> Option<citrine_gb_net::GameHash> {
        self.has_game()
            .then(|| self.seat.core().machine.game_sha1())
    }

    pub(crate) fn advance(
        &mut self,
        now: web_time::Instant,
        cable: Option<&mut PlaySession>,
    ) -> bool {
        let Some((plan, dt)) = self.prepare(now) else {
            return false;
        };
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        let sharp = self.records_3d();
        let mut hooks = InlineHooks {
            cable,
            replay: &mut self.replay,
            held: plan.held,
            recorder: &mut self.recorder,
            screen: &mut self.screen_recorder,
            #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
            sharp,
        };
        let ran =
            self.seat
                .core_mut()
                .run(&plan, &mut hooks, &mut citrine_gb_threads::Split::inline());
        self.after_run(ran, dt)
    }

    fn prepare(&mut self, now: web_time::Instant) -> Option<(Plan, f64)> {
        self.give_touch();
        if self.last_good.take_written() {
            self.play.copy_written();
        }
        if !self.running {
            self.gimmicks.stop_rumble();
            let core = self.seat.core_mut();
            core.drop_audio();
            if let Some(link) = &mut core.link {
                link.clear_audio();
            }
            if !core.play_preview(self.sound)
                && let Some(feed) = &core.audio_feed
            {
                feed.set_paused(true);
            }
            self.last_update = Some(now);
            if self.play.copy_due(true) {
                self.copy_last_good();
            }
            return None;
        }

        if self.cheats_for.as_ref().map(|(key, _)| key.as_str()) != self.rom_key.as_deref() {
            self.cheats_for = None;
            self.seat.core_mut().cheats.clear();
        }
        let rewind = self.rewind_plan();
        self.rewinding = rewind.back;
        let machine = &mut self.seat.core_mut().machine;
        #[cfg(feature = "nds")]
        match machine {
            Machine::Nds(nds) => {
                nds.set_jit(self.ds_jit);
                #[cfg(threaded)]
                if let Some(spawner) = &self.jit_spawner {
                    nds.compile_jit_on(spawner);
                }
            }
            Machine::Gba(gba) => {
                gba.set_jit(crate::app::ui_state::settings::GBA_USES_JIT && self.ds_jit);
                #[cfg(threaded)]
                if let Some(spawner) = &self.jit_spawner {
                    gba.compile_jit_on(spawner);
                }
            }
            _ => {}
        }
        self.gimmicks.follow_clock(machine, now_unix());
        self.gimmicks.feed(machine);
        let timing = machine.timing();
        let (dt, fed) = self.advance_clock(now, timing);
        let due = fed + std::mem::take(&mut self.away_due);
        let dt = dt + std::mem::take(&mut self.away_dt);
        let fast = self.fast_forward && !rewind.back;
        let frames = if fast {
            self.fast_forward_speed.frames(due)
        } else {
            due
        };
        let plan = Plan {
            frames,
            held: self.held.held(),
            owed: if frames > 0 {
                self.held.take_owed()
            } else {
                Keys::empty()
            },
            effects: self.effects(),
            sound: self.sound,
            fast_forward: self.fast_forward,
            take: std::mem::take(&mut self.want_copy),
            rewind,
            budget: fast.then(|| fast_forward::budget(dt)),
            defer_3d: self.defers_3d(),
            pace: self.clock.pace(timing),
        };
        Some((plan, dt))
    }

    pub fn set_refreshes(&mut self, refreshes: Option<u64>) {
        self.refreshes = refreshes;
    }

    fn advance_clock(&mut self, now: web_time::Instant, timing: Timing) -> (f64, usize) {
        match self.refreshes.take() {
            Some(0) => (0.0, 0),
            Some(refreshes) => {
                let dt = self.since_last_update(now);
                (dt, self.clock.advance_over(dt, refreshes, timing))
            }
            None => {
                let dt = self.since_last_update(now);
                let due = if dt > 0.0 {
                    self.clock.advance(dt, timing)
                } else {
                    0
                };
                (dt, due)
            }
        }
    }

    fn since_last_update(&mut self, now: web_time::Instant) -> f64 {
        let dt = self
            .last_update
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.last_update = Some(now);
        dt
    }

    #[cfg(threaded)]
    fn tick_away(&mut self, ctx: &egui::Context, now: web_time::Instant) {
        let keys = input::key_events(ctx);
        self.away_keys.extend(keys);
        if !self.running {
            return;
        }
        let (dt, due) = self.advance_clock(now, self.seat.facts().timing);
        self.away_dt += dt;
        self.away_due += due;
    }

    #[cfg(threaded)]
    fn show_lag_basis(&self) -> Option<(std::time::Duration, std::time::Duration)> {
        if self.fast_forward || self.rewinding {
            return None;
        }
        let frame = std::time::Duration::from_secs_f64(self.seat.facts().timing.frame_secs());
        Some((self.show_refresh(), frame))
    }

    #[cfg(threaded)]
    fn show_refresh(&self) -> std::time::Duration {
        self.clock
            .refresh_period()
            .map_or(self.ui_wait.refresh(), std::time::Duration::from_secs_f64)
    }

    fn after_run(&mut self, ran: Ran, dt: f64) -> bool {
        let core = self.seat.core_mut();
        self.gimmicks.take_catch(&mut core.catch);
        self.frame_avg_timer = core.frame_timer.clone();
        #[cfg(feature = "nds")]
        self.follow_walker();
        #[cfg(ds_wifi)]
        {
            let core = self.seat.core_mut();
            self.ds_wifi.follow(&mut core.machine, &mut core.wifi);
        }
        if ran.hit {
            self.running = false;
        }
        let step = std::time::Duration::from_secs_f64(dt.min(MAX_PLAY_STEP));
        self.play.play(step);
        self.stat_events.played += step;
        if self.play.copy_due(false) {
            self.copy_in_play();
        }
        ran.frames > 0
    }

    fn effects(&self) -> Effects {
        Effects {
            matrix: screen::draws_grid(self.enable_matrix, self.render_scale),
            edge: self.matrix_edge_brightness,
            corner: self.matrix_corner_brightness,
            ghosting: self.enable_ghosting,
            blend: self.ghosting_blend,
            scale: self.render_scale,
        }
    }

    fn can_lend(&self) -> bool {
        self.seat.can_lend()
            && self.running
            && self.replay.is_none()
            && !self.recorder.is_active()
            && !self.screen_recorder.is_recording()
            && self.texture.is_some()
            && self.linked_frame.is_none()
            && !self.seat.facts().step_pending
    }

    pub fn begin(
        &mut self,
        ctx: &egui::Context,
        pads: &mut dyn PadSource,
        bindings: &KeyBindings,
        capture: &mut BindingCapture,
        log: Option<&mut PadLog>,
        session: Option<&mut PlaySession>,
    ) {
        self.ready = None;
        self.lending = false;
        let now = web_time::Instant::now();
        #[cfg(threaded)]
        {
            self.ui_wait.begin(now, self.refreshes);
            self.show_pace.begin(self.refreshes);
            if self.lent.is_some() {
                let early = !self.show_pace.due();
                if early {
                    self.tick_away(ctx, now);
                }
                let next_due = self.away_due > 0;
                if (early && !next_due) || !self.seat.wait(std::time::Duration::ZERO) {
                    if !early {
                        self.tick_away(ctx, now);
                    }
                    return;
                }
            }
        }
        self.settle(ctx);
        let session = session.filter(|s| s.is_live());
        if let Some(session) = session {
            self.begin_rollback(ctx, pads, bindings, capture, log, session);
            return;
        }
        if !self.can_lend() || capture.is_waiting() {
            #[cfg(threaded)]
            self.away_keys.clear();
            return;
        }
        self.update_avg_timer.start();
        self.handle_input(ctx, pads, bindings, capture, log);
        if !self.can_lend() || capture.is_waiting() {
            self.update_avg_timer.stop();
            return;
        }
        self.drop_history = None;
        let Some((plan, dt)) = self.prepare(now) else {
            self.update_avg_timer.stop();
            return;
        };
        #[cfg(feature = "nds")]
        {
            self.copy_walker();
            self.catch_ds_clock_set();
        }
        #[cfg(threaded)]
        {
            let lag = self.show_lag_basis();
            self.show_pace
                .lent(web_time::Instant::now(), plan.frames, lag);
        }
        self.seat.lend(plan);
        self.lending = true;
        self.lent = Some(dt);
    }

    fn finish_lent(&mut self, ctx: &egui::Context) -> GbResult<()> {
        #[cfg(threaded)]
        let due = self.show_pace.due() || self.away_due > 0;
        #[cfg(not(threaded))]
        let due = true;
        #[cfg(threaded)]
        if due {
            let waited = web_time::Instant::now();
            let done = self.seat.wait(self.wait_left());
            self.console_waited += waited.elapsed();
            if !done {
                self.late_frames += 1;
            }
        }
        if due && self.seat.is_home() {
            self.settle(ctx);
            self.handle_save()?;
        }
        self.update_avg_timer.stop();
        Ok(())
    }

    fn settle(&mut self, ctx: &egui::Context) {
        let Some(dt) = self.lent.take() else {
            return;
        };
        let core = self.seat.core_mut();
        let ran = core.ran.take().unwrap_or_default();
        self.ready = core.taken.take();
        #[cfg(threaded)]
        if self
            .show_pace
            .shown(ran.finished, self.ui_wait.first(), self.show_refresh())
        {
            self.clock.held();
        }
        if self.after_run(ran, dt) {
            self.upload(ctx);
            self.total_frames = self.total_frames.wrapping_add(1);
        }
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
        self.ready = None;
        if let Err(err) = self.autosave_if_played() {
            log::error!("could not autosave before switching games: {err:?}");
        }
        self.running = false;
        self.fast_forward = false;
        self.save_loaded = false;
        self.imported_legacy_save = false;
        self.console_switched = None;
        let core = self.seat.core_mut();
        if let (Some(partner), Some(gb)) = (core.partner.take(), core.machine.as_gb_mut()) {
            partner.finish(gb);
        }
        match (
            GbModel::try_from(kind),
            self.seat.core_mut().machine.as_gb_mut(),
        ) {
            (Ok(model), Some(gb)) => gb.load_rom_with_model(rom, model)?,
            _ => self.replace_machine(Machine::load(rom, kind, &self.firmware)?),
        }
        #[cfg(feature = "nds")]
        if let Some(header) =
            citrine_gb::rom::nds::NdsHeader::parse(&rom.data).filter(|_| kind == ConsoleKind::Nds)
        {
            self.attach_ir(header.game_code);
        }
        self.sync_joypad();

        let key = self.seat.core_mut().machine.rom_id().sha256_hex_string();
        self.writes.settle();
        if let Err(err) = self
            .store
            .set_game_console(&key, self.seat.core_mut().machine.kind())
        {
            log::warn!("could not note the console of the game: {err}");
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.rom_path = path.map(|p| p.to_owned());
            if let Some(path) = path {
                self.imported_legacy_save = self.store.import_legacy_save_if_new(&key, path);
            }
        }

        if let Some(save) = self
            .store
            .load_battery_save(&key, &self.seat.core_mut().machine)
        {
            self.seat.core_mut().machine.put_battery(save, now_unix());
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
        #[cfg(not(target_arch = "wasm32"))]
        self.achievements_follow_game();
        Ok(())
    }

    #[cfg(feature = "nds")]
    pub fn rom_bytes(&self) -> std::sync::Arc<[u8]> {
        self.seat.core().machine.rom_bytes()
    }

    pub fn kind(&self) -> ConsoleKind {
        self.seat.facts().kind
    }

    pub fn console(&self) -> GbModel {
        self.seat.facts().model.unwrap_or(GbModel::Dmg)
    }

    pub fn debug(&self) -> &Machine {
        &self.seat.core().machine
    }

    pub fn debug_mut(&mut self) -> &mut Machine {
        &mut self.seat.core_mut().machine
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

    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn pen_down(&self) -> bool {
        match &self.seat.core().machine {
            Machine::Nds(nds) => {
                let rcnt = nds.buses().peek_io(citrine_gb::nds::Cpu::Arm7, 0x0400_0134);
                rcnt >> 16 & (1 << 6) == 0
            }
            _ => false,
        }
    }

    #[cfg(all(test, feature = "nds"))]
    pub fn load_nds(&mut self, data: &[u8]) -> GbResult<()> {
        self.running = false;
        self.console_switched = None;
        let machine = Machine::load(&Rom::new(data), ConsoleKind::Nds, &self.firmware)?;
        self.replace_machine(machine);
        Ok(())
    }

    #[cfg(feature = "nds")]
    pub fn ds_file_name(&self, file: crate::app::ds_firmware::DsFile) -> Option<&str> {
        self.ds_slot(file)
            .is_some()
            .then(|| self.ds_file_names[file.index()].as_str())
    }

    #[cfg(feature = "nds")]
    pub fn set_ds_file(
        &mut self,
        file: crate::app::ds_firmware::DsFile,
        value: Option<(Vec<u8>, String)>,
    ) {
        let (image, name) = value.unzip();
        *self.ds_slot_mut(file) = image;
        self.ds_file_names[file.index()] = name.unwrap_or_default();
    }

    #[cfg(feature = "nds")]
    fn ds_slot(&self, file: crate::app::ds_firmware::DsFile) -> &Option<Vec<u8>> {
        use crate::app::ds_firmware::DsFile;
        let nds = &self.firmware.nds;
        match file {
            DsFile::Arm9Bios => &nds.arm9_bios,
            DsFile::Arm7Bios => &nds.arm7_bios,
            DsFile::Firmware => &nds.firmware,
        }
    }

    #[cfg(feature = "nds")]
    fn ds_slot_mut(&mut self, file: crate::app::ds_firmware::DsFile) -> &mut Option<Vec<u8>> {
        use crate::app::ds_firmware::DsFile;
        let nds = &mut self.firmware.nds;
        match file {
            DsFile::Arm9Bios => &mut nds.arm9_bios,
            DsFile::Arm7Bios => &mut nds.arm7_bios,
            DsFile::Firmware => &mut nds.firmware,
        }
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

    #[cfg(feature = "nds")]
    pub fn as_nds(&self) -> Option<&citrine_gb::nds::machine::Nds> {
        match &self.seat.core().machine {
            Machine::Nds(nds) => Some(nds),
            _ => None,
        }
    }

    #[cfg(all(test, feature = "nds"))]
    pub fn as_nds_mut(&mut self) -> Option<&mut citrine_gb::nds::machine::Nds> {
        match &mut self.seat.core_mut().machine {
            Machine::Nds(nds) => Some(nds),
            _ => None,
        }
    }

    #[cfg(feature = "gba")]
    pub fn as_gba(&self) -> Option<&citrine_gb::gba::machine::Gba> {
        match &self.seat.core().machine {
            Machine::Gba(gba) => Some(gba),
            _ => None,
        }
    }

    #[cfg(feature = "gba")]
    pub fn set_gba_color_correction(
        &mut self,
        correction: citrine_gb::gb::ppu::types::color::ColorCorrection,
    ) {
        if let Machine::Gba(gba) = &mut self.seat.core_mut().machine {
            gba.bus_mut().ppu_mut().set_color_correction(correction);
        }
    }

    pub fn as_gb(&self) -> Option<&GameBoy> {
        self.seat.core().machine.as_gb()
    }

    pub fn as_gb_mut(&mut self) -> Option<&mut GameBoy> {
        self.seat.core_mut().machine.as_gb_mut()
    }

    pub fn debug_console(&self) -> &dyn Debug {
        &self.seat.core().machine
    }

    pub fn has_game(&self) -> bool {
        self.seat.facts().has_game
    }

    pub fn video(&self) -> VideoSpec {
        match self.linked_frame {
            Some(_) => self.linked_video,
            None => self.seat.facts().video,
        }
    }

    pub fn shows_console(&self) -> bool {
        self.linked_frame.is_some() || self.has_game()
    }

    pub fn played_kind(&self) -> ConsoleKind {
        match self.linked_frame {
            Some(_) => self.linked_kind,
            None => self.kind(),
        }
    }

    #[cfg(any(test, not(any(target_arch = "wasm32", target_os = "android"))))]
    pub fn pace(&self) -> &crate::audio::even_clock::PaceCount {
        self.clock.count()
    }

    pub fn refresh_period(&self) -> Option<f64> {
        self.clock.refresh_period()
    }

    pub fn timing(&self) -> Timing {
        self.seat.facts().timing
    }

    pub fn link(&self) -> Option<&LocalLink> {
        if !self.linked() {
            return None;
        }
        self.seat.core().link.as_ref()
    }

    pub fn link_mut(&mut self) -> Option<&mut LocalLink> {
        if !self.linked() {
            return None;
        }
        self.seat.core_mut().link.as_mut()
    }

    pub fn linked(&self) -> bool {
        self.seat.facts().linked
    }

    pub fn partner(&self) -> Option<&PartnerLink> {
        if !self.partnered() {
            return None;
        }
        self.seat.core().partner.as_ref()
    }

    pub fn partner_mut(&mut self) -> Option<&mut PartnerLink> {
        if !self.partnered() {
            return None;
        }
        self.seat.core_mut().partner.as_mut()
    }

    pub fn partnered(&self) -> bool {
        self.seat.facts().partnered
    }

    #[cfg(threaded)]
    fn wait_left(&self) -> std::time::Duration {
        self.ui_wait.left(web_time::Instant::now(), self.solo_wait)
    }

    #[cfg(threaded)]
    pub fn set_frame_wait(&mut self, wait: std::time::Duration) {
        self.frame_wait = wait;
        self.solo_wait = wait;
    }

    #[cfg(threaded)]
    pub fn console_waited(&self) -> std::time::Duration {
        self.console_waited
    }

    #[cfg(threaded)]
    pub fn late_frames(&self) -> u64 {
        self.late_frames
    }

    #[cfg(threaded)]
    pub fn run_on(&mut self, threads: &citrine_gb_threads::Threads) {
        self.seat.run_on(threads);
        #[cfg(feature = "nds")]
        {
            self.jit_spawner = Some(threads.spawner());
        }
    }

    #[cfg(threaded)]
    pub fn follow_visibility(&mut self, visible: bool, threads: &citrine_gb_threads::Threads) {
        self.seat.follow_visibility(visible, threads);
    }

    #[cfg(any(test, all(feature = "dev-route", target_arch = "wasm32")))]
    pub(crate) fn has_worker(&self) -> bool {
        self.seat.has_worker()
    }

    pub fn frame(&mut self) -> &[u8] {
        self.finish_picture();
        self.published_frame()
    }

    pub(crate) fn published_frame(&self) -> &[u8] {
        self.seat.core().machine.frame()
    }

    #[cfg(test)]
    pub(crate) fn put_machine(&mut self, machine: Machine) {
        self.replace_machine(machine);
        self.running = true;
    }

    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn painted(&self) -> (&[u8], [usize; 2]) {
        let screen = &self.seat.core().screen;
        (&screen.painted, screen.size)
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub(crate) fn test_gpu() -> Option<eframe::egui_wgpu::RenderState> {
        upscale::tests::gpu()
    }

    #[cfg(all(test, feature = "nds", not(target_arch = "wasm32")))]
    pub(crate) fn rewind_kept(&self) -> (usize, usize, usize) {
        self.seat.core().rewind.kept()
    }

    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn ds_jit_on(&self) -> bool {
        match &self.seat.core().machine {
            Machine::Nds(nds) => nds.jit_on(),
            _ => false,
        }
    }

    #[cfg(all(test, feature = "nds"))]
    pub(crate) fn gba_jit_on(&self) -> bool {
        match &self.seat.core().machine {
            Machine::Gba(gba) => gba.jit_on(),
            _ => false,
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn lend_one_frame(&mut self) -> usize {
        self.give_touch();
        let plan = Plan {
            frames: 1,
            held: self.held.held(),
            owed: self.held.take_owed(),
            effects: self.effects(),
            sound: self.sound,
            fast_forward: false,
            take: std::mem::take(&mut self.want_copy),
            rewind: self.rewind_plan(),
            budget: None,
            defer_3d: self.defers_3d(),
            pace: 1.0,
        };
        self.seat.lend(plan);
        let core = self.seat.core_mut();
        self.ready = core.taken.take();
        core.ran.take().map_or(0, |ran| ran.frames)
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
        let rom = Rom::from(gb.cartridge.rom_bytes());
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

    #[cfg(feature = "nds")]
    pub fn restart_ds(&mut self) -> GbResult<()> {
        let machine = &self.seat.core().machine;
        let rom = Rom::from(machine.rom_bytes()).with_ids((machine.rom_id(), machine.game_sha1()));
        let name = self.game_name.clone();
        let launched = self.stat_events.launched;
        #[cfg(not(target_arch = "wasm32"))]
        let path = self.rom_path.clone();
        self.load_game(
            &rom,
            ConsoleKind::Nds,
            #[cfg(not(target_arch = "wasm32"))]
            path.as_deref(),
        )?;
        self.game_name = name;
        self.stat_events.launched = launched;
        Ok(())
    }

    pub fn unload(&mut self) {
        #[cfg(feature = "nds")]
        self.detach_ir();
        #[cfg(feature = "nds")]
        self.save_jit_cache();
        self.screen_recorder.stop();
        self.last_good.clear();
        let mut seat = std::mem::take(&mut self.seat);
        let feed = seat.core_mut().audio_feed.take();
        #[cfg(not(target_arch = "wasm32"))]
        let achievements = seat.core_mut().achievements.take();
        let mut core = Core::new(Machine::Gb(Box::new(GameBoy::new_empty(GbModel::Dmg))));
        core.audio_feed = feed;
        #[cfg(not(target_arch = "wasm32"))]
        {
            core.achievements = achievements.map(|mut client| {
                client.unload_game();
                client
            });
        }
        core.follow_feed_rate();
        seat.replace(core);
        *self = Self {
            seat,
            store: self.store.clone(),
            writes: std::mem::take(&mut self.writes),
            last_good: std::mem::take(&mut self.last_good),
            sound: self.sound,
            match_screen: self.match_screen,
            running: false,
            gimmick_settings: std::mem::take(&mut self.gimmick_settings),
            #[cfg(feature = "nds")]
            pokewalker: std::mem::take(&mut self.pokewalker),
            #[cfg(feature = "nds")]
            walker_linked: false,
            #[cfg(feature = "nds")]
            walker_ms: None,
            #[cfg(feature = "nds")]
            ds_clock: self.ds_clock,
            steps_in_game: self.steps_in_game,
            #[cfg(feature = "nds")]
            game_steps: Default::default(),
            #[cfg(feature = "nds")]
            walker_clock: self.walker_clock,
            #[cfg(ds_wifi)]
            ds_wifi: std::mem::take(&mut self.ds_wifi),
            firmware: std::mem::take(&mut self.firmware),
            gba_bios_name: std::mem::take(&mut self.gba_bios_name),
            ..Self::default()
        };
    }

    pub fn take_save_news(&mut self) -> Vec<Outcome> {
        self.writes.take_news()
    }

    pub fn next_free_slot(&mut self, rom_key: &str) -> usize {
        self.writes.settle();
        self.store.next_free_slot(rom_key)
    }

    pub fn rom_key(&self) -> Option<&str> {
        self.rom_key.as_deref()
    }

    pub fn follow_cheats(&mut self, book: &crate::cheats::CheatBook) {
        let saved = self.rom_key.as_deref().map(|key| (key, book.list(key)));
        let seen = self
            .cheats_for
            .as_ref()
            .map(|(key, list)| (key.as_str(), list.as_slice()));
        if saved == seen {
            return;
        }
        let cheats = match saved {
            Some((_, list)) => crate::cheats::compile(self.kind(), list),
            None => Vec::new(),
        };
        self.cheats_for = saved.map(|(key, list)| (key.to_owned(), list.to_vec()));
        self.seat.core_mut().cheats = cheats;
    }

    pub fn screen_texture(&self) -> Option<egui::TextureId> {
        self.texture.as_ref().map(|t| t.id())
    }

    pub fn take_stat_events(&mut self) -> StatEvents {
        std::mem::take(&mut self.stat_events)
    }

    pub fn idle_refreshes(&self) -> Option<u64> {
        let busy = !self.running
            || self.fast_forward
            || self.rewinding
            || self.replay.is_some()
            || self.recorder.is_active()
            || self.screen_recorder.is_recording()
            || self.linked()
            || self.partnered()
            || self.step_pending()
            || self.away_due > 0;
        if busy {
            return None;
        }
        if self.lent.is_some() {
            return self.refreshes_to_show();
        }
        self.clock.refreshes_to_due(self.seat.facts().timing)
    }

    #[cfg(threaded)]
    fn refreshes_to_show(&self) -> Option<u64> {
        self.show_pace
            .refreshes_to_show()
            .filter(|&refreshes| refreshes > 0)
    }

    #[cfg(not(threaded))]
    fn refreshes_to_show(&self) -> Option<u64> {
        None
    }

    pub fn is_running(&self) -> bool {
        self.running || self.step_pending()
    }

    pub fn toggle_running(&mut self) {
        if self.seat.core().machine.step_pending() {
            self.seat.core_mut().machine.cancel_step();
            return;
        }
        self.running = !self.running;
        if !self.running {
            self.release_all();
            #[cfg(feature = "nds")]
            self.save_jit_cache();
        }
    }

    pub fn step(&mut self, step: Step) -> Stop {
        if self.running {
            self.running = false;
            self.release_all();
        }
        let stop = Debug::step(&mut self.seat.core_mut().machine, step);
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
        self.seat.core_mut().machine.resume_step();
        self.update_texture(ctx);
        ctx.request_repaint();
    }

    pub(crate) fn core_mut(&mut self) -> &mut runner::Core {
        self.seat.core_mut()
    }

    fn step_pending(&self) -> bool {
        self.seat.facts().step_pending
    }

    fn replace_machine(&mut self, machine: Machine) {
        #[cfg(feature = "nds")]
        self.detach_ir();
        let enabled: [bool; 4] = std::array::from_fn(|index| {
            self.as_gb().is_none_or(|gb| gb.apu.channel_enabled(index))
        });
        let same_channels =
            self.seat.core().machine.audio_channels().len() == machine.audio_channels().len();
        self.seat.core_mut().put_machine(machine);
        #[cfg(feature = "nds")]
        self.follow_jit_cache();
        self.ready = None;
        self.drop_history = None;
        if same_channels {
            for (index, enabled) in enabled.into_iter().enumerate() {
                self.seat
                    .core_mut()
                    .machine
                    .set_channel_enabled(index, enabled);
            }
        }
        self.start_gimmicks();
    }

    pub fn soft_reset(&mut self, ctx: &egui::Context) {
        let core = self.seat.core_mut();
        core.machine.soft_reset();
        core.flight.note_reset();
        #[cfg(not(target_arch = "wasm32"))]
        self.achievements_reset();
        self.update_texture(ctx);
    }

    pub fn reset_to(&mut self, ctx: &egui::Context, start_cycles: u32) {
        self.seat.core_mut().machine.soft_reset();
        self.seat.core_mut().flight.restart();
        #[cfg(not(target_arch = "wasm32"))]
        self.achievements_reset();
        self.started_at = Some(web_time::Instant::now());
        if let Some(gb) = self.as_gb_mut() {
            gb.run_for(start_cycles);
        } else {
            Debug::step(
                &mut self.seat.core_mut().machine,
                Step::Cycles(start_cycles),
            );
        }
        self.update_texture(ctx);
    }
}

struct InlineHooks<'a> {
    cable: Option<&'a mut PlaySession>,
    replay: &'a mut Option<crate::replay::ReplayPlayer>,
    held: Keys,
    recorder: &'a mut InputRecorder,
    screen: &'a mut ScreenRecorder,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    sharp: bool,
}

impl Hooks for InlineHooks<'_> {
    fn before_frame(&mut self, machine: &mut Machine) {
        if let Some(session) = self.cable.as_deref_mut()
            && let Some(gb) = machine.as_gb_mut()
        {
            session.pump_link(gb);
        }
        replay::feed(self.replay, self.held, machine);
    }

    fn record(&mut self, cycles: u128, key: Keys, pressed: bool) {
        self.recorder.record(cycles, key, pressed);
    }

    fn captured(&mut self, machine: &Machine, fresh: &[f32]) {
        if !self.screen.is_recording() {
            return;
        }
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        if self.sharp {
            self.screen.hold(fresh, machine.sample_rate());
            return;
        }
        self.screen
            .capture(machine.frame(), fresh, machine.sample_rate());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::ui_state::keybinds::{
        BindingKind, BindingTarget, Captured, GbButton, PadButton,
    };
    use crate::held::InputSource;
    use crate::pad_input::PadEvent;
    use crate::pad_input::{PadAxis, PadInput, PadKey};
    use crate::storage::Written;
    use citrine_gb::console::ConsoleKind;
    use strum::IntoEnumIterator;

    const RAM_LEN: usize = 0x2000;
    const KEYS: InputSource = InputSource::Keyboard;

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
        emulator
            .load_rom(&rom(cartridge_type), GbModel::Dmg, None)
            .expect("load");
        emulator.gb_mut().run_cycles(100_000);
    }

    fn autosaved(emulator: &mut Emulator) -> Option<usize> {
        emulator
            .take_save_news()
            .into_iter()
            .find_map(|outcome| match outcome {
                Ok(crate::storage::Written::Autosave(id)) => Some(id),
                _ => None,
            })
    }

    fn played(emulator: &mut Emulator, seconds: f64) {
        let start = web_time::Instant::now();
        emulator.advance(start, None);
        let mut at = 0.0;
        while at < seconds {
            at += 0.1;
            emulator.advance(start + std::time::Duration::from_secs_f64(at), None);
        }
    }

    fn write_sram(emulator: &mut Emulator, value: u8) {
        use citrine_gb::WriteMemory;
        emulator.gb_mut().cartridge.write_naive(0x0000, 0x0A);
        emulator.gb_mut().cartridge.write_naive(0xA000, value);
    }

    fn autosaves(emulator: &Emulator) -> Vec<usize> {
        emulator
            .store
            .autosave_ids(emulator.rom_key().expect("key"))
    }

    fn gb_held(emulator: &mut Emulator) -> u16 {
        use citrine_gb::{ReadMemory, WriteMemory};
        let joypad = &mut emulator.gb_mut().joypad;
        joypad.write_naive(0xFF00, 0x10);
        let buttons = !joypad.read_naive(0xFF00) & 0x0F;
        joypad.write_naive(0xFF00, 0x20);
        let dpad = !joypad.read_naive(0xFF00) & 0x0F;
        u16::from(buttons | (dpad << 4))
    }

    #[test]
    fn the_game_boy_sees_what_is_held_after_a_load() {
        let mut emulator = emulator("held-sync");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, Keys::A);
        emulator.save_snapshot(0).expect("save");
        emulator.release(KEYS, Keys::A);
        emulator.press(KEYS, Keys::RIGHT);

        assert!(emulator.load_snapshot(0).expect("load"));
        assert_eq!(
            gb_held(&mut emulator),
            Keys::RIGHT.bits(),
            "A was let go before the load and Right is still down"
        );
        assert!(emulator.release(KEYS, Keys::RIGHT));
        assert_eq!(gb_held(&mut emulator), 0);

        emulator.press(KEYS, Keys::START);
        running(&mut emulator, 0x03);
        assert_eq!(gb_held(&mut emulator), Keys::START.bits());
        assert!(
            emulator.release(KEYS, Keys::START),
            "the release reaches the game"
        );
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn paused_time_does_not_count() {
        let mut emulator = emulator("auto-paused");
        running(&mut emulator, 0x03);
        emulator.running = false;
        played(&mut emulator, 3.0);
        assert!(!emulator.autosave_if_played().expect("autosave"));

        emulator.running = true;
        played(&mut emulator, 0.5);
        emulator.autosave_interval = AutosaveInterval::OneMinute;
        assert!(!emulator.autosave_if_due().expect("autosave"));
        emulator.play.play(std::time::Duration::from_secs(60));
        assert!(emulator.autosave_if_due().expect("autosave"));
        assert_eq!(autosaved(&mut emulator), Some(0));
        assert!(!emulator.autosave_if_due().expect("autosave"));
    }

    #[test]
    fn a_hidden_page_autosave_counts_as_saved() {
        let mut emulator = emulator("auto-hidden");
        running(&mut emulator, 0x03);
        played(&mut emulator, 1.5);
        emulator.running = false;
        played(&mut emulator, 0.2);
        assert_eq!(emulator.last_good.write_out(), Ok(Some(0)));
        played(&mut emulator, 0.2);
        assert!(!emulator.autosave_if_played().expect("autosave"));
        assert_eq!(autosaves(&emulator), vec![0]);

        emulator.running = true;
        played(&mut emulator, 1.5);
        assert!(emulator.autosave_if_played().expect("autosave"));
        assert_eq!(autosaved(&mut emulator), Some(1));
    }

    #[test]
    fn closing_autosaves_only_after_play() {
        let mut emulator = emulator("auto-close");
        running(&mut emulator, 0x03);
        assert!(!emulator.autosave_if_played().expect("autosave"));
        played(&mut emulator, 0.3);
        assert!(emulator.autosave_if_played().expect("autosave"));
        assert_eq!(autosaved(&mut emulator), Some(0));
        assert!(!emulator.autosave_if_played().expect("autosave"));
        assert_eq!(autosaves(&emulator), vec![0]);
    }

    #[test]
    fn an_exit_right_after_saving_keeps_every_save_written_on_the_worker() {
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        let mut emulator = emulator("exit-flush");
        emulator.writes = Writes::start(&threads);
        running(&mut emulator, 0x03);
        write_sram(&mut emulator, 0x5A);
        played(&mut emulator, 0.3);
        emulator.flush_save().expect("battery");
        emulator.save_snapshot(2).expect("snapshot");
        assert!(emulator.autosave_if_played().expect("autosave"));
        let key = emulator.rom_key().expect("key").to_owned();

        let report = threads.shutdown(std::time::Duration::from_secs(10));

        assert!(report.unfinished.is_empty(), "{:?}", report.unfinished);
        assert!(emulator.writes.is_idle());
        let news = emulator.take_save_news();
        assert!(news.iter().all(Result::is_ok), "{news:?}");
        assert!(news.contains(&Ok(Written::Snapshot(2))));
        assert!(news.contains(&Ok(Written::Autosave(0))));
        assert_eq!(emulator.store.autosave_ids(&key), vec![0]);
        assert_eq!(
            emulator.store.load_battery(&key).map(|ram| ram[0]),
            Some(0x5A)
        );
        assert!(emulator.load_snapshot(2).expect("load"));
    }

    #[test]
    fn a_due_autosave_on_a_lent_core_waits_one_frame_for_the_console_thread_copy() {
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        let mut emulator = emulator("lent-copy");
        emulator.writes = Writes::start(&threads);
        running(&mut emulator, 0x03);
        emulator.run_on(&threads);
        emulator.autosave_interval = AutosaveInterval::OneMinute;
        emulator.play.play(std::time::Duration::from_secs(61));
        emulator.lending = true;

        assert!(!emulator.autosave_if_due().expect("asked"));
        assert!(emulator.want_copy);
        assert!(emulator.ready.is_none());
        assert_eq!(emulator.lend_one_frame(), 1);
        assert!(emulator.ready.is_some());
        assert!(emulator.autosave_if_due().expect("autosave"));
        assert!(emulator.ready.is_none());
        assert!(!emulator.want_copy);
        emulator.writes.settle();

        assert_eq!(autosaved(&mut emulator), Some(0));
        assert!(emulator.load_autosave(0).expect("load"));
        assert!(
            threads
                .shutdown(std::time::Duration::from_secs(10))
                .unfinished
                .is_empty()
        );
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
        emulator
            .load_rom(&Rom::new(&other), GbModel::Dmg, None)
            .expect("load");

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
        assert!(emulator.autosave().expect("autosave"));
        let id = autosaved(&mut emulator).expect("id");
        played(&mut emulator, 0.3);
        write_sram(&mut emulator, 0x22);
        assert!(emulator.load_autosave(id).expect("load"));

        assert_eq!(emulator.gb().cartridge.battery_ram()[0], 0x11);
        assert!(!emulator.autosave_if_played().expect("autosave"));
        assert_eq!(emulator.store.load_snapshot(&key, 0), snapshot);
        assert_eq!(emulator.store.used_slots(&key), vec![0]);
    }

    fn dual(emulator: &mut Emulator, model: GbModel) {
        let mut data = rom(0x03).data.to_vec();
        data[0x0143] = 0x80;
        emulator
            .load_rom(&Rom::new(&data), model, None)
            .expect("load");
        emulator.gb_mut().run_cycles(100_000);
    }

    #[test]
    fn switching_consoles_keeps_the_battery_save() {
        let mut emulator = emulator("switch-battery");
        dual(&mut emulator, GbModel::Cgb);
        write_sram(&mut emulator, 0x5A);
        emulator.restart_on(GbModel::Dmg).expect("switch");
        assert_eq!(emulator.console(), GbModel::Dmg);
        assert_eq!(emulator.gb().cartridge.battery_ram()[0], 0x5A);

        write_sram(&mut emulator, 0x6B);
        emulator.restart_on(GbModel::Cgb).expect("switch back");
        assert_eq!(emulator.console(), GbModel::Cgb);
        assert_eq!(emulator.gb().cartridge.battery_ram()[0], 0x6B);
        assert_eq!(
            emulator.take_console_switch(),
            None,
            "a restart is no surprise"
        );
    }

    #[test]
    fn switching_consoles_keeps_the_game_name() {
        let mut emulator = emulator("switch-name");
        dual(&mut emulator, GbModel::Cgb);
        emulator.name_game(Some("My Game (USA).gbc"));
        let name = emulator.game_name().to_owned();
        emulator.restart_on(GbModel::Dmg).expect("switch");
        assert_eq!(emulator.game_name(), name);
    }

    #[test]
    fn a_known_game_names_its_battery_save_by_its_proper_name() {
        let mut emulator = emulator("proper-name");
        let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/homebrew/libbet.gb"))
            .expect("libbet");
        emulator
            .load_rom(&Rom::new(&rom), GbModel::Dmg, None)
            .expect("load");
        emulator.name_game(Some("libbet.gb"));
        assert_ne!(
            emulator.gb().cartridge.header.title.trim(),
            "Libbet and the Magic Floor"
        );
        assert_eq!(emulator.game_name(), "Libbet and the Magic Floor");
        assert_eq!(emulator.save_status().name, "Libbet and the Magic Floor");
    }

    #[test]
    fn a_snapshot_loads_on_the_console_it_was_made_on() {
        let mut emulator = emulator("snapshot-console");
        dual(&mut emulator, GbModel::Dmg);
        emulator.save_snapshot(0).expect("snapshot");
        let key = emulator.rom_key().expect("key").to_owned();
        let info = emulator.store.snapshot_info(&key, 0).expect("info");
        assert_eq!(info.console, Some(ConsoleKind::Gb));

        emulator.restart_on(GbModel::Cgb).expect("switch");
        assert!(emulator.load_snapshot(0).expect("load"));
        assert_eq!(emulator.console(), GbModel::Dmg);
        assert_eq!(emulator.take_console_switch(), Some(GbModel::Dmg));
        assert_eq!(emulator.take_console_switch(), None);

        assert!(emulator.load_snapshot(0).expect("load again"));
        assert_eq!(
            emulator.take_console_switch(),
            None,
            "same console, no notice"
        );
    }

    #[test]
    fn an_autosave_records_its_console() {
        let mut emulator = emulator("autosave-console");
        dual(&mut emulator, GbModel::Cgb);
        played(&mut emulator, 0.3);
        assert!(emulator.autosave().expect("autosave"));
        let id = autosaved(&mut emulator).expect("id");
        let key = emulator.rom_key().expect("key").to_owned();
        let infos = emulator.store.autosave_infos(&key);
        assert_eq!(infos[0].console, Some(ConsoleKind::Gbc));

        emulator.restart_on(GbModel::Dmg).expect("switch");
        assert!(emulator.load_autosave(id).expect("load"));
        assert_eq!(emulator.console(), GbModel::Cgb);
        assert_eq!(emulator.take_console_switch(), Some(GbModel::Cgb));
    }

    #[test]
    fn a_snapshot_from_before_consoles_were_recorded_still_loads() {
        let name = "old-snapshot";
        let mut emulator = emulator(name);
        dual(&mut emulator, GbModel::Dmg);
        write_sram(&mut emulator, 0x33);
        emulator.save_snapshot(0).expect("snapshot");
        let key = emulator.rom_key().expect("key").to_owned();
        let meta = std::env::temp_dir()
            .join(format!("citrine-import-test-{}-{name}", std::process::id()))
            .join(&key)
            .join("slot0.meta");
        std::fs::write(&meta, 1_700_000_000u64.to_le_bytes()).expect("old meta");

        let info = emulator.store.snapshot_info(&key, 0).expect("info");
        assert_eq!(info.saved_at, 1_700_000_000);
        assert_eq!(
            info.console,
            Some(ConsoleKind::Gb),
            "read from the snapshot"
        );
        assert_eq!(std::fs::read(&meta).expect("meta").len(), 9, "noted once");

        write_sram(&mut emulator, 0x44);
        assert!(emulator.load_snapshot(0).expect("load"));
        assert_eq!(emulator.gb().cartridge.battery_ram()[0], 0x33);
        assert_eq!(emulator.console(), GbModel::Dmg);
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
        let rom = Rom::new(&emulator.gb().cartridge.rom_bytes());
        let restored = GameBoy::from_dump(&state, &rom, now_unix()).expect("restores");
        assert_eq!(restored.cpu.pc, emulator.gb().cpu.pc);
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
        emulator.gb_mut().run_cycles(100_000);

        let result = emulator.import_save(Some(&bytes(2)), None).expect("second");
        assert_eq!(result, SaveImport::Imported);
        emulator.restart();
        emulator.gb_mut().run_cycles(100_000);

        assert_eq!(emulator.gb().cartridge.battery_ram(), bytes(2));
        let key = emulator.rom_key().expect("key").to_owned();
        assert_eq!(emulator.store.load_battery(&key), Some(bytes(2)));
        assert!(emulator.running);
        assert!(!emulator.has_been_playing());
    }

    #[test]
    fn an_imported_clock_survives_the_restart() {
        let mut emulator = emulator("clock");
        running(&mut emulator, 0x10);
        let mut footer = emulator
            .gb()
            .cartridge
            .rtc_footer(now_unix())
            .expect("clock");
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
        emulator.gb_mut().run_cycles(100_000);

        assert_eq!(emulator.gb().cartridge.battery_ram(), bytes(3));
        let clock = emulator
            .gb()
            .cartridge
            .rtc_footer(now_unix())
            .expect("clock");
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
        assert!(save_screenshot(&mut emulator, &mut media, 3).is_err());

        running(&mut emulator, 0x00);
        let item = save_screenshot(&mut emulator, &mut media, 3).expect("saved");
        assert_eq!(item.info.kind, MediaKind::Screenshot);
        assert_eq!((item.info.width, item.info.height), (480, 432));
        assert!(media.path(&item.id).starts_with(root.join("screenshots")));
        assert!(media.path(&item.id).is_file());
        assert!(item.thumbnail.is_some());
        assert_eq!(MediaStore::at(root.clone()).items().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    struct FakePads(std::collections::VecDeque<PadEvent>);

    impl PadSource for FakePads {
        fn next_event(&mut self) -> Option<PadEvent> {
            self.0.pop_front()
        }

        fn pads(&self, _seen: &[usize]) -> Vec<crate::controller_debug::PadInfo> {
            Vec::new()
        }

        fn api(&self) -> String {
            "fake".to_string()
        }
    }

    fn fake(events: &[(usize, PadInput)]) -> FakePads {
        FakePads(
            events
                .iter()
                .map(|&(pad, input)| PadEvent {
                    pad,
                    input,
                    code: None,
                })
                .collect(),
        )
    }

    #[test]
    fn any_pad_source_drives_the_bindings() {
        let mut emulator = emulator("pad-source");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut log = PadLog::new(web_time::Instant::now());
        let mut pads = fake(&[
            (3, PadInput::Connected),
            (3, PadInput::Pressed(PadKey::Known(PadButton::East))),
            (3, PadInput::Moved(PadAxis::LeftStickX, -0.9)),
        ]);
        emulator.handle_pads(&mut pads, &bindings, &mut capture, Some(&mut log));
        assert!(emulator.is_held(GbButton::A.key()));
        assert!(emulator.is_held(GbButton::Left.key()));
        assert_eq!(log.len(), 3);
        assert_eq!(log.seen(), &[3]);

        let mut pads = fake(&[
            (3, PadInput::Released(PadKey::Known(PadButton::East))),
            (3, PadInput::Moved(PadAxis::LeftStickX, 0.0)),
        ]);
        emulator.handle_pads(&mut pads, &bindings, &mut capture, None);
        assert!(!emulator.is_held(GbButton::A.key()));
        assert!(!emulator.is_held(GbButton::Left.key()));
    }

    #[test]
    fn any_pad_source_feeds_a_waiting_binding() {
        let mut emulator = emulator("pad-capture");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        capture.start(BindingTarget::Button(GbButton::B), BindingKind::Gamepad);
        let mut pads = fake(&[(
            1,
            PadInput::Pressed(PadKey::Known(PadButton::RightTrigger2)),
        )]);
        emulator.handle_pads(&mut pads, &bindings, &mut capture, None);
        assert_eq!(
            capture.take(),
            Some((
                BindingTarget::Button(GbButton::B),
                Captured::Pad(PadButton::RightTrigger2)
            ))
        );
        assert!(!emulator.is_held(GbButton::B.key()));
    }

    fn d_pad(button: PadButton, pressed: bool) -> PadInput {
        if pressed {
            PadInput::Pressed(PadKey::Known(button))
        } else {
            PadInput::Released(PadKey::Known(button))
        }
    }

    fn stick_x(value: f32) -> PadInput {
        PadInput::Moved(PadAxis::LeftStickX, value)
    }

    fn stick_y(value: f32) -> PadInput {
        PadInput::Moved(PadAxis::LeftStickY, value)
    }

    fn noise(count: usize) -> Vec<(usize, PadInput)> {
        (0..count)
            .map(|i| {
                let value = if i % 2 == 0 { 0.02 } else { -0.02 };
                let input = if i % 3 == 0 {
                    stick_y(value)
                } else {
                    stick_x(value)
                };
                (0, input)
            })
            .collect()
    }

    fn feed(emulator: &mut Emulator, bindings: &KeyBindings, events: &[(usize, PadInput)]) {
        let mut capture = BindingCapture::default();
        emulator.handle_pads(&mut fake(events), bindings, &mut capture, None);
    }

    fn pads_in(emulator: &mut Emulator, events: &[(usize, PadInput)]) {
        feed(emulator, &KeyBindings::default(), events);
    }

    fn play_a_frame(emulator: &mut Emulator) {
        emulator.running = true;
        let start = web_time::Instant::now();
        emulator.advance(start, None);
        emulator.advance(start + std::time::Duration::from_millis(50), None);
    }

    fn holds(emulator: &Emulator, button: GbButton) -> bool {
        emulator.is_held(button.key())
    }

    #[test]
    fn stick_noise_never_lets_go_of_a_held_d_pad() {
        let mut emulator = emulator("pad-noise");
        running(&mut emulator, 0x03);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::DPadLeft, true))]);
        pads_in(&mut emulator, &noise(50));
        assert!(holds(&emulator, GbButton::Left));
        play_a_frame(&mut emulator);
        assert_eq!(gb_held(&mut emulator), Keys::LEFT.bits());
    }

    #[test]
    fn a_d_pad_tap_reaches_the_game_for_a_frame() {
        let mut emulator = emulator("pad-tap");
        running(&mut emulator, 0x03);
        let mut tap = vec![(0, d_pad(PadButton::DPadLeft, true))];
        tap.extend(noise(6));
        tap.push((0, d_pad(PadButton::DPadLeft, false)));
        pads_in(&mut emulator, &tap);
        assert!(!holds(&emulator, GbButton::Left), "the player let go");
        assert_eq!(
            gb_held(&mut emulator),
            Keys::LEFT.bits(),
            "the game still sees the tap until a frame runs"
        );
        play_a_frame(&mut emulator);
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn a_stick_flick_reaches_the_game_for_a_frame() {
        let mut emulator = emulator("pad-flick");
        running(&mut emulator, 0x03);
        pads_in(
            &mut emulator,
            &[(0, stick_x(0.95)), (0, stick_x(0.3)), (0, stick_x(0.01))],
        );
        assert!(!holds(&emulator, GbButton::Right));
        assert_eq!(gb_held(&mut emulator), Keys::RIGHT.bits());
        play_a_frame(&mut emulator);
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn a_centred_stick_lets_go_of_only_what_it_held() {
        let mut emulator = emulator("pad-centre");
        running(&mut emulator, 0x03);
        pads_in(
            &mut emulator,
            &[(0, d_pad(PadButton::DPadUp, true)), (0, stick_x(-0.9))],
        );
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, stick_x(0.0))]);
        assert!(!holds(&emulator, GbButton::Left));
        assert!(holds(&emulator, GbButton::Up));
        assert_eq!(gb_held(&mut emulator), Keys::UP.bits());
    }

    #[test]
    fn the_d_pad_and_the_stick_hold_the_same_button_together() {
        let mut emulator = emulator("pad-both");
        running(&mut emulator, 0x03);
        pads_in(
            &mut emulator,
            &[(0, d_pad(PadButton::DPadLeft, true)), (0, stick_x(-0.9))],
        );
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, stick_x(0.0))]);
        assert!(holds(&emulator, GbButton::Left), "the d-pad still holds it");
        pads_in(&mut emulator, &[(0, stick_x(-0.9))]);
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::DPadLeft, false))]);
        assert!(holds(&emulator, GbButton::Left), "the stick still holds it");
        assert_eq!(gb_held(&mut emulator), Keys::LEFT.bits());
        pads_in(&mut emulator, &[(0, stick_x(0.0))]);
        assert!(!holds(&emulator, GbButton::Left));
        assert_eq!(gb_held(&mut emulator), 0);
    }

    #[test]
    fn the_keyboard_and_a_pad_do_not_let_go_of_each_other() {
        let mut emulator = emulator("pad-keys");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, Keys::A);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::East, true))]);
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, d_pad(PadButton::East, false))]);
        assert!(holds(&emulator, GbButton::A));
        pads_in(&mut emulator, &[(0, d_pad(PadButton::East, true))]);
        emulator.release(KEYS, Keys::A);
        assert!(holds(&emulator, GbButton::A));
    }

    #[test]
    fn stick_diagonals_act_on_each_axis_alone() {
        let mut emulator = emulator("pad-diagonal");
        running(&mut emulator, 0x03);
        pads_in(&mut emulator, &[(0, stick_x(-0.8)), (0, stick_y(-0.8))]);
        assert!(holds(&emulator, GbButton::Left));
        assert!(holds(&emulator, GbButton::Down));
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, stick_y(0.02))]);
        assert!(holds(&emulator, GbButton::Left));
        assert!(!holds(&emulator, GbButton::Down));
        pads_in(&mut emulator, &[(0, stick_y(0.8)), (0, stick_x(0.01))]);
        assert!(holds(&emulator, GbButton::Up));
        assert!(!holds(&emulator, GbButton::Left));
    }

    #[test]
    fn a_stick_resting_near_the_dead_zone_does_not_flicker() {
        let mut emulator = emulator("pad-edge");
        running(&mut emulator, 0x03);
        let mut log = PadLog::new(web_time::Instant::now());
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut send = |emulator: &mut Emulator, log: &mut PadLog, values: &[f32]| {
            let events: Vec<(usize, PadInput)> = values.iter().map(|&v| (0, stick_x(v))).collect();
            emulator.handle_pads(&mut fake(&events), &bindings, &mut capture, Some(log));
        };
        send(&mut emulator, &mut log, &[0.46, 0.49, 0.47, 0.5, 0.48]);
        assert!(!holds(&emulator, GbButton::Right), "not past the dead zone");
        send(&mut emulator, &mut log, &[0.53]);
        assert!(holds(&emulator, GbButton::Right));
        let before = log.len();
        let jitter: Vec<f32> = (0..40)
            .map(|i| if i % 2 == 0 { 0.52 } else { 0.42 })
            .collect();
        send(&mut emulator, &mut log, &jitter);
        assert!(holds(&emulator, GbButton::Right), "held through the jitter");
        assert_eq!(log.len(), before + 1, "the jitter is one log line");
        send(&mut emulator, &mut log, &[0.38]);
        assert!(!holds(&emulator, GbButton::Right));
    }

    #[test]
    fn the_dead_zone_setting_moves_where_the_stick_presses() {
        let mut emulator = emulator("pad-deadzone");
        running(&mut emulator, 0x03);
        let mut bindings = KeyBindings::default();
        bindings.stick_deadzone = 30;
        feed(&mut emulator, &bindings, &[(0, stick_x(-0.35))]);
        assert!(holds(&emulator, GbButton::Left));
        feed(&mut emulator, &bindings, &[(0, stick_x(-0.22))]);
        assert!(holds(&emulator, GbButton::Left), "lets go only below 0.20");
        feed(&mut emulator, &bindings, &[(0, stick_x(-0.15))]);
        assert!(!holds(&emulator, GbButton::Left));
    }

    #[test]
    fn an_unplugged_pad_lets_go_of_what_it_held() {
        let mut emulator = emulator("pad-unplug");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, Keys::A);
        pads_in(
            &mut emulator,
            &[
                (0, d_pad(PadButton::East, true)),
                (0, d_pad(PadButton::Start, true)),
                (0, stick_x(-0.9)),
                (1, d_pad(PadButton::South, true)),
            ],
        );
        play_a_frame(&mut emulator);
        pads_in(&mut emulator, &[(0, PadInput::Disconnected)]);
        assert!(holds(&emulator, GbButton::A), "the keyboard still holds it");
        assert!(holds(&emulator, GbButton::B), "pad 1 still holds it");
        assert!(!holds(&emulator, GbButton::Start));
        assert!(!holds(&emulator, GbButton::Left));
        assert_eq!(gb_held(&mut emulator), (Keys::A | Keys::B).bits());
    }

    #[test]
    fn letting_go_of_everything_clears_every_source() {
        let mut emulator = emulator("pad-release-all");
        running(&mut emulator, 0x03);
        emulator.press(KEYS, Keys::A);
        emulator.press(InputSource::Touch, Keys::B);
        pads_in(
            &mut emulator,
            &[(0, d_pad(PadButton::DPadUp, true)), (0, stick_x(0.9))],
        );
        emulator.release_all();
        assert!(GbButton::iter().all(|b| !holds(&emulator, b)));
        assert_eq!(gb_held(&mut emulator), 0, "taps owed a frame go too");
        pads_in(&mut emulator, &[(0, stick_x(0.95))]);
        assert!(
            holds(&emulator, GbButton::Right),
            "a stick still pushed holds again on its next move"
        );
    }

    #[test]
    fn stick_noise_folds_into_one_log_line_per_axis() {
        let mut emulator = emulator("pad-log");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut log = PadLog::new(web_time::Instant::now());
        let mut events = vec![(0, d_pad(PadButton::DPadUp, true))];
        events.extend(noise(90));
        events.push((0, PadInput::Moved(PadAxis::RightStickX, 0.01)));
        events.push((0, d_pad(PadButton::DPadUp, false)));
        events.extend(noise(9));
        emulator.handle_pads(&mut fake(&events), &bindings, &mut capture, Some(&mut log));
        let lines: Vec<String> = log.entries().map(|e| e.event()).collect();
        assert_eq!(lines.len(), 7, "{lines:#?}");
        assert!(lines[0].contains("DPadUp pressed"));
        assert!(
            lines[1].contains("LeftStickY moved 30 times between -0.02 and +0.02"),
            "{}",
            lines[1]
        );
        assert!(
            lines[2].contains("LeftStickX moved 60 times"),
            "{}",
            lines[2]
        );
        assert!(lines[3].contains("RightStickX at +0.01"), "{}", lines[3]);
        assert!(lines[4].contains("DPadUp released"));
    }

    #[test]
    fn a_finished_session_hands_its_console_to_the_emulator() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, SNAKE, homebrew, snake};
        let mut emu = emulator("finish-session");
        let model = GbModel::for_rom(&Rom::new(SNAKE)).unwrap();
        emu.load_rom(&Rom::new(SNAKE), model, None).unwrap();
        let name = emu.game_name().to_owned();
        let mut pair = Pair::new(false, 0, 0, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(90);

        let now = web_time::Instant::now();
        emu.last_update = Some(now - std::time::Duration::from_millis(50));
        let session = pair.host.play.session_mut().unwrap();
        emu.advance_rollback(now, session);
        let shown = session.console(true).unwrap().frame().to_vec();
        assert!(emu.linked_frame.as_deref() == Some(shown.as_slice()));

        pair.host.play.leave(&mut pair.host.online);
        let screen = pair.host.session().console(true).unwrap().frame().to_vec();
        let session = pair.host.play.close().unwrap();
        emu.finish_session(session).unwrap();
        assert!(emu.gb().frame().as_slice() == screen.as_slice());
        assert!(emu.seat.core().screen.last_frame == screen);
        assert!(emu.linked_frame.is_none());
        assert!(emu.running);
        assert!(!emu.gb().link_plugged());
        assert_eq!(emu.game_name(), name);
    }

    #[cfg(feature = "nds")]
    fn shades(frame: &[u8]) -> usize {
        frame
            .as_chunks::<4>()
            .0
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    #[cfg(feature = "nds")]
    #[test]
    fn both_players_see_the_3d_of_a_ds_game_they_play_together() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, empty, lighting_cube};
        use citrine_gb::nds::machine::{Nds, NdsFirmware};
        let _one = crate::play::tests::one_ds_pair_at_a_time();
        let mut alone = Nds::new(lighting_cube().rom, NdsFirmware::default()).unwrap();
        for _ in 0..240 {
            alone.run_frame();
        }
        let lit = shades(alone.frame());
        assert!(lit >= 32, "the cube alone shows {lit} shades");
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(lighting_cube());
        pair.accept(session, MyGame::Own(lighting_cube()));
        pair.play_frames(240);
        for (who, side) in [("host", &mut pair.host), ("guest", &mut pair.guest)] {
            let mut emu = emulator(&format!("cube-{who}"));
            let now = web_time::Instant::now();
            emu.last_update = Some(now - std::time::Duration::from_millis(50));
            emu.advance_rollback(now, side.play.session_mut().unwrap());
            let shown = shades(emu.linked_frame.as_deref().unwrap_or_default());
            assert!(
                shown * 2 >= lit,
                "the {who} sees {shown} shades, the cube alone {lit}"
            );
        }
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    fn off_by(upscaled: &[u8], scale: usize, frame: &[u8], screen_y: usize) -> usize {
        const WIDTH: usize = 256;
        const HEIGHT: usize = 192;
        let start = screen_y * WIDTH * 4;
        let screen = &frame[start..start + WIDTH * HEIGHT * 4];
        screen
            .chunks(4)
            .enumerate()
            .filter(|(index, cpu)| {
                let x = index % WIDTH * scale + scale / 2;
                let y = index / WIDTH * scale + scale / 2;
                let at = (y * WIDTH * scale + x) * 4;
                let gpu = &upscaled[at..at + 4];
                gpu.iter().zip(cpu.iter()).any(|(a, b)| a.abs_diff(*b) > 24)
            })
            .count()
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn upscaled_3d_in_a_ds_pair_shows_the_game_played_together() {
        use crate::app::ui_state::ds_3d::{Ds3d, Resolution, SmoothEdges};
        use crate::play::MyGame;
        use crate::play::tests::{Pair, empty, lighting_cube};
        const CUBE: &[u8] = include_bytes!("../fixtures/nds/blocksds-lighting_cube.nds");
        let Some(state) = Emulator::test_gpu() else {
            return;
        };
        let _one = crate::play::tests::one_ds_pair_at_a_time();
        let ctx = egui::Context::default();
        let mut emus = [emulator("upscaled-host"), emulator("upscaled-guest")];
        for emu in &mut emus {
            emu.load_nds(CUBE).unwrap();
            emu.gpu = Some((ctx.clone(), state.clone()));
            emu.ds_3d = Ds3d {
                resolution: Resolution::Double,
                smooth_edges: SmoothEdges::Off,
            };
            emu.update_texture(&ctx);
            for _ in 0..60 {
                emu.seat.core_mut().machine.run_frame();
            }
            emu.update_texture(&ctx);
            assert!(emu.upscaled.is_some(), "the paused cube is upscaled");
        }
        let paused: Vec<Vec<u8>> = emus
            .iter()
            .map(|emu| emu.seat.core().machine.frame().to_vec())
            .collect();
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(lighting_cube());
        pair.accept(session, MyGame::Own(lighting_cube()));
        pair.play_frames(200);
        for _ in 0..4 {
            pair.play_frames(20);
            for (emu, side) in emus.iter_mut().zip([&mut pair.host, &mut pair.guest]) {
                let now = web_time::Instant::now();
                emu.last_update = Some(now - std::time::Duration::from_millis(50));
                emu.advance_rollback(now, side.play.session_mut().unwrap());
                emu.update_texture(&ctx);
            }
        }
        for ((who, emu), paused) in ["host", "guest"].iter().zip(&emus).zip(&paused) {
            let shown = emu.linked_frame.as_deref().expect("a session frame");
            let upscaled = emu.upscaled.expect("the 3d is upscaled");
            let (scale, pixels) = emu.upscaler.as_ref().unwrap().read().expect("readback");
            assert_eq!(scale, 2);
            let screen_y = upscaled.screen_y();
            let to_session = off_by(&pixels, 2, shown, screen_y);
            let to_paused = off_by(&pixels, 2, paused, screen_y);
            assert!(
                to_session * 10 <= 256 * 192 && to_session < to_paused,
                "the {who}'s upscaled screen is {to_session} pixels off the session \
                 and {to_paused} off the game paused in the seat"
            );
        }
    }

    #[cfg(feature = "nds")]
    fn a_ds_session(name: &str) -> (Emulator, crate::play::tests::Pair) {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, empty, nickname};
        let emu = emulator(name);
        let mut pair = Pair::new(true, 0, 0, empty());
        let session = pair.invite(nickname());
        pair.accept(session, MyGame::Own(nickname()));
        pair.play_frames(60);
        (emu, pair)
    }

    #[cfg(feature = "nds")]
    fn host_frame(
        emu: &mut Emulator,
        ctx: &egui::Context,
        mut session: Option<&mut PlaySession>,
    ) -> GbResult<()> {
        let mut result = Ok(());
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let now = web_time::Instant::now();
            emu.last_update = Some(now - std::time::Duration::from_millis(17));
            result = emu.update_linked(
                ctx,
                &mut fake(&[]),
                &KeyBindings::default(),
                &mut BindingCapture::default(),
                None,
                session.as_deref_mut(),
            );
        });
        result
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_friend_joining_a_running_ds_game_picks_it_up_where_it_is() {
        use crate::play::tests::{Pair, empty, nickname};
        use crate::play::{MyGame, Stage};
        let _one = crate::play::tests::one_ds_pair_at_a_time();
        let ctx = egui::Context::default();
        let mut emu = emulator("ds-host-join");
        emu.load_game(&Rom::new(&nickname().rom), ConsoleKind::Nds, None)
            .unwrap();
        for _ in 0..30 {
            host_frame(&mut emu, &ctx, None).unwrap();
        }
        assert!(emu.total_frames > 0);
        let mut pair = Pair::new(true, 0, 0, empty());
        let session = pair.invite(emu.game_file().unwrap());
        assert!(
            pair.guest
                .play
                .accept(&mut pair.guest.online, session, MyGame::Download)
        );
        let playing = |pair: &Pair| {
            pair.host.stage() == Some(Stage::Playing) && pair.guest.stage() == Some(Stage::Playing)
        };
        for tick in 0.. {
            pair.tick();
            if playing(&pair) {
                break;
            }
            assert!(tick < 20_000, "the game never started");
            let session = pair.host.play.session_mut().filter(|s| s.is_live());
            if let Err(err) = host_frame(&mut emu, &ctx, session) {
                panic!("the host showed \"{err}\" on tick {tick}");
            }
        }
        assert!(
            pair.host.session().resumed_mine(),
            "the host's game restarted"
        );
        pair.play_frames(60);
        pair.until_checked();
        for side in [&pair.host, &pair.guest] {
            assert_eq!(side.session().resyncs(), 0);
            assert!(side.session().failure().is_none());
        }
    }

    #[cfg(feature = "nds")]
    fn guest_frames(emu: &mut Emulator, pair: &mut crate::play::tests::Pair) {
        let now = web_time::Instant::now();
        emu.last_update = Some(now - std::time::Duration::from_millis(50));
        emu.advance_rollback(now, pair.guest.play.session_mut().unwrap());
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_pad_follows_the_console_a_session_plays() {
        let _one = crate::play::tests::one_ds_pair_at_a_time();
        let (mut emu, mut pair) = a_ds_session("pad-follows-session");
        assert_eq!(emu.played_kind(), ConsoleKind::Gb);

        guest_frames(&mut emu, &mut pair);
        assert_eq!(emu.kind(), ConsoleKind::Gb);
        assert_eq!(emu.played_kind(), ConsoleKind::Nds);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_lid_closes_on_the_ds_a_session_plays() {
        let _one = crate::play::tests::one_ds_pair_at_a_time();
        let (mut emu, mut pair) = a_ds_session("lid-in-session");
        assert_eq!(emu.lid(), None);

        guest_frames(&mut emu, &mut pair);
        assert_eq!(emu.kind(), ConsoleKind::Gb);
        assert_eq!(emu.lid(), Some(false));
        emu.toggle_lid();
        assert_eq!(emu.lid(), Some(true));
        assert!(emu.held_buttons().contains(Keys::LID));
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_game_pane_is_named_after_the_console_a_session_plays() {
        use crate::app::tabs::game_title;
        let _one = crate::play::tests::one_ds_pair_at_a_time();
        let (mut emu, mut pair) = a_ds_session("title-in-session");
        assert_eq!(game_title(&emu), "Game Boy");

        guest_frames(&mut emu, &mut pair);
        assert_eq!(game_title(&emu), "Nintendo DS");
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_game_got_by_download_play_is_never_offered_to_a_friend() {
        use crate::app::tabs::playing::can_play_again;
        use crate::play::tests::{Pair, homebrew, nickname};
        let pair = Pair::new(false, 0, 0, homebrew());
        let friend = pair.guest.id();
        let host = &pair.host;
        for downloaded in [false, true] {
            let mut emu = emulator(&format!("downloaded-{downloaded}"));
            let console =
                Machine::load(&Rom::new(&nickname().rom), ConsoleKind::Nds, &emu.firmware).unwrap();
            emu.adopt(crate::play::Returned {
                console,
                borrowed: Some([7; 20]),
                paused: true,
                friend,
                history: None,
                downloaded,
            });
            assert_eq!(emu.game_file().is_some(), !downloaded);
            assert_eq!(
                can_play_again(&host.play, &host.online, &emu, friend),
                !downloaded
            );
        }
    }

    #[test]
    fn resetting_to_a_cycle_count_keeps_the_frame_position() {
        let mut emulator = emulator("reset-to");
        running(&mut emulator, 0x03);

        emulator.reset_to(&egui::Context::default(), 100_000);

        let counter = emulator.gb().cycle_counter;
        assert!((12_220..12_230).contains(&counter), "counter {counter}");
    }

    #[test]
    fn resetting_to_a_cycle_count_stops_at_a_breakpoint() {
        let mut emulator = emulator("reset-to-breakpoint");
        running(&mut emulator, 0x03);
        let target = emulator.gb().code_address(0x0200).expect("rom");
        emulator.gb_mut().debugger.breakpoints.insert(target);

        emulator.reset_to(&egui::Context::default(), 100_000);

        assert_eq!(emulator.gb().cpu.pc.wrapping_sub(1), 0x0200);
    }

    #[test]
    fn a_lent_frame_lets_go_of_a_tap_and_of_a_press_it_saw() {
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        let mut emulator = emulator("lent-release");
        running(&mut emulator, 0x03);
        emulator.run_on(&threads);

        emulator.press(KEYS, Keys::START);
        emulator.release(KEYS, Keys::START);
        assert_eq!(gb_held(&mut emulator), Keys::START.bits());
        assert_eq!(emulator.lend_one_frame(), 1);
        assert_eq!(gb_held(&mut emulator), 0, "the tap ends after its frame");

        emulator.press(KEYS, Keys::START);
        assert_eq!(emulator.lend_one_frame(), 1);
        emulator.release(KEYS, Keys::START);
        assert_eq!(emulator.lend_one_frame(), 1);
        assert_eq!(
            gb_held(&mut emulator),
            0,
            "a press a frame saw ends at once"
        );
        assert!(
            threads
                .shutdown(std::time::Duration::from_secs(10))
                .unfinished
                .is_empty()
        );
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    fn samples_per_second(emulator: &mut Emulator) -> f64 {
        let machine = &mut emulator.seat.core_mut().machine;
        let mut out = Vec::new();
        machine.run_frame();
        machine.take_audio(&mut out);
        out.clear();
        for _ in 0..60 {
            machine.run_frame();
            machine.take_audio(&mut out);
        }
        out.len() as f64 / 2.0 / (60.0 * machine.timing().frame_secs())
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn every_console_makes_samples_at_the_device_rate() {
        use crate::audio::AudioLink;
        use crate::audio::pacer::AudioFeed;
        use crate::audio::queue::{QueueConfig, audio_queue};

        let rate = 96_000;
        let fixture = |path: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
            Rom::new(&std::fs::read(path).expect("fixture"))
        };
        let mut emulator = emulator("device-rate");
        let (writer, _reader) = audio_queue(QueueConfig::new(rate, 4096));
        emulator.attach_audio(AudioLink {
            sample_rate: rate,
            feed: AudioFeed::new(writer, rate, 4096),
        });
        let near = |made: f64| (made / f64::from(rate) - 1.0).abs() < 0.002;

        let ds = fixture("fixtures/nds/blocksds-console-both-screens.nds");
        emulator
            .load_game(&ds, ConsoleKind::Nds, None)
            .expect("ds loads");
        let made = samples_per_second(&mut emulator);
        assert!(near(made), "ds made {made:.0} samples a second for {rate}");

        let gba = fixture("fixtures/gba/save-sram.gba");
        emulator
            .load_game(&gba, ConsoleKind::Gba, None)
            .expect("gba loads");
        let made = samples_per_second(&mut emulator);
        assert!(near(made), "gba made {made:.0} samples a second for {rate}");

        emulator.unload();
        emulator
            .load_rom(&rom(0x00), GbModel::Dmg, None)
            .expect("gb loads");
        let made = samples_per_second(&mut emulator);
        assert!(near(made), "gb made {made:.0} samples a second for {rate}");
    }

    #[cfg(feature = "nds")]
    fn ds_x_and_y_down(emulator: &mut Emulator) -> (bool, bool) {
        emulator.debug_mut().run_frame();
        let nds = emulator.as_nds_mut().expect("ds");
        let extkeyin = nds.buses().peek_io(citrine_gb::nds::Cpu::Arm7, 0x0400_0134) >> 16;
        let owed = emulator.held.take_owed();
        emulator.apply(crate::held::Change {
            pressed: Keys::empty(),
            released: owed,
        });
        (extkeyin & 1 == 0, extkeyin & 2 == 0)
    }

    #[cfg(feature = "nds")]
    #[test]
    fn keys_and_pad_buttons_bound_to_x_and_y_press_them_on_the_ds() {
        use crate::app::ui_state::keybinds::{DsButton, KeyBindings};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        let mut emulator = emulator("ds-x-y");
        emulator
            .load_game(
                &Rom::new(&std::fs::read(path).expect("fixture")),
                ConsoleKind::Nds,
                None,
            )
            .expect("ds loads");
        let mut bindings = KeyBindings::default();
        bindings.bind_ds_key(egui::Key::K, DsButton::X);
        bindings.bind_ds_key(egui::Key::J, DsButton::Y);
        let mut capture = BindingCapture::default();
        let ctx = egui::Context::default();
        let mut key = |emulator: &mut Emulator, key: egui::Key, pressed: bool| {
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                emulator.handle_input(ctx, &mut fake(&[]), &bindings, &mut capture, None);
            });
        };
        assert_eq!(ds_x_and_y_down(&mut emulator), (false, false));
        key(&mut emulator, egui::Key::K, true);
        assert_eq!(ds_x_and_y_down(&mut emulator), (true, false));
        key(&mut emulator, egui::Key::J, true);
        assert_eq!(ds_x_and_y_down(&mut emulator), (true, true));
        key(&mut emulator, egui::Key::K, false);
        key(&mut emulator, egui::Key::J, false);
        assert_eq!(ds_x_and_y_down(&mut emulator), (false, false));
        key(&mut emulator, egui::Key::C, true);
        assert_eq!(ds_x_and_y_down(&mut emulator), (true, false));
        key(&mut emulator, egui::Key::C, false);

        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let press = |pad: PadButton, down: bool| {
            let key = PadKey::Known(pad);
            let input = if down {
                PadInput::Pressed(key)
            } else {
                PadInput::Released(key)
            };
            fake(&[(0, input)])
        };
        for (pad, expected) in [
            (PadButton::North, (true, false)),
            (PadButton::West, (false, true)),
        ] {
            emulator.handle_pads(&mut press(pad, true), &bindings, &mut capture, None);
            assert_eq!(ds_x_and_y_down(&mut emulator), expected, "{pad:?}");
            assert!(
                !emulator.held_buttons().intersects(Keys::A | Keys::B),
                "{pad:?} is no Game Boy button on the ds"
            );
            emulator.handle_pads(&mut press(pad, false), &bindings, &mut capture, None);
            assert_eq!(ds_x_and_y_down(&mut emulator), (false, false));
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn north_presses_x_and_not_b_in_a_ds_game() {
        use crate::app::ui_state::keybinds::KeyBindings;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        let mut emulator = emulator("ds-north");
        emulator
            .load_game(
                &Rom::new(&std::fs::read(path).expect("fixture")),
                ConsoleKind::Nds,
                None,
            )
            .expect("ds loads");
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let mut pad = |emulator: &mut Emulator, pad: PadButton, down: bool| {
            let key = PadKey::Known(pad);
            let input = if down {
                PadInput::Pressed(key)
            } else {
                PadInput::Released(key)
            };
            emulator.handle_pads(&mut fake(&[(0, input)]), &bindings, &mut capture, None);
            emulator.debug_mut().run_frame();
            let owed = emulator.held.take_owed();
            emulator.apply(crate::held::Change {
                pressed: Keys::empty(),
                released: owed,
            });
        };
        let a_and_b_down = |emulator: &mut Emulator| {
            let nds = emulator.as_nds_mut().expect("ds");
            let keyinput = nds.buses().peek_io(citrine_gb::nds::Cpu::Arm9, 0x0400_0130);
            (keyinput & 1 == 0, keyinput & 2 == 0)
        };
        pad(&mut emulator, PadButton::South, true);
        assert_eq!(a_and_b_down(&mut emulator), (false, true));
        pad(&mut emulator, PadButton::South, false);
        pad(&mut emulator, PadButton::North, true);
        assert_eq!(a_and_b_down(&mut emulator), (false, false));
        assert_eq!(ds_x_and_y_down(&mut emulator), (true, false));
        pad(&mut emulator, PadButton::North, false);
        pad(&mut emulator, PadButton::West, true);
        assert_eq!(a_and_b_down(&mut emulator), (false, false));
        assert_eq!(ds_x_and_y_down(&mut emulator), (false, true));
    }

    #[test]
    fn the_ds_buttons_leave_game_boy_play_alone() {
        use crate::app::ui_state::keybinds::KeyBindings;
        let mut emulator = emulator("ds-x-y-on-gb");
        running(&mut emulator, 0x00);
        let bindings = KeyBindings::default();
        let mut capture = BindingCapture::default();
        let north = PadInput::Pressed(PadKey::Known(PadButton::North));
        emulator.handle_pads(&mut fake(&[(0, north)]), &bindings, &mut capture, None);
        assert_eq!(emulator.held_buttons(), Keys::B);
    }

    #[cfg(feature = "gba")]
    mod gba {
        use super::*;
        use citrine_gb::gba::save::SaveType;
        use citrine_gb::persistence::state::GBA_STATE_MAGIC;

        const FRAMES: usize = 60;
        const SAVES: [(&str, SaveType); 5] = [
            ("sram", SaveType::Sram32K),
            ("flash64", SaveType::Flash64K),
            ("flash128", SaveType::Flash128K),
            ("eeprom512", SaveType::Eeprom512),
            ("eeprom8k", SaveType::Eeprom8K),
        ];

        fn fixture(name: &str) -> Vec<u8> {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/gba")
                .join(name);
            std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        }

        fn save_rom(kind: &str) -> Rom {
            Rom::new(&fixture(&format!("save-{kind}.gba")))
        }

        fn mgba_save(kind: &str) -> Vec<u8> {
            crate::storage::decompress(&fixture(&format!("mgba-{kind}.sav.br"))).expect("brotli")
        }

        fn on_store(root: &std::path::Path) -> Emulator {
            Emulator {
                store: SaveStore::at(root.to_path_buf()),
                ..Emulator::default()
            }
        }

        fn store_root(name: &str) -> std::path::PathBuf {
            let root = std::env::temp_dir()
                .join(format!("citrine-gba-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            root
        }

        fn start(emulator: &mut Emulator, rom: &Rom) {
            emulator
                .load_game(rom, ConsoleKind::Gba, None)
                .expect("load");
            assert_eq!(emulator.kind(), ConsoleKind::Gba);
        }

        fn run(emulator: &mut Emulator, frames: usize) {
            for _ in 0..frames {
                emulator.debug_mut().run_frame();
            }
        }

        fn chip(emulator: &mut Emulator) -> Option<SaveType> {
            emulator
                .debug_mut()
                .battery(true, 0)
                .and_then(|save| save.chip)
        }

        #[test]
        fn each_save_type_comes_back_with_its_chip_after_a_restart() {
            for (kind, expected) in SAVES {
                let root = store_root(&format!("type-{kind}"));
                let mut emulator = on_store(&root);
                start(&mut emulator, &save_rom(kind));
                run(&mut emulator, FRAMES);
                emulator.flush_save().expect("flush");
                let written = emulator.export_save_bytes().expect("save");
                assert_eq!(chip(&mut emulator), Some(expected), "{kind}");

                let mut again = on_store(&root);
                start(&mut again, &save_rom(kind));
                assert!(again.save_loaded, "{kind}");
                assert_eq!(again.export_save_bytes(), Some(written), "{kind}");
                assert_eq!(chip(&mut again), Some(expected), "{kind}");
            }
        }

        #[test]
        fn a_cartridge_without_a_library_string_takes_its_chip_from_the_store() {
            let root = store_root("undetected");
            let mut emulator = on_store(&root);
            start(&mut emulator, &save_rom("flash64"));
            run(&mut emulator, FRAMES);
            emulator.flush_save().expect("flush");
            let written = emulator.export_save_bytes().expect("save");

            let mut bare = save_rom("flash64").data.to_vec();
            let at = bare
                .windows(8)
                .position(|window| window == b"FLASH512")
                .expect("library string");
            bare[at] = 0;
            let mut again = on_store(&root);
            start(&mut again, &Rom::new(&bare));
            let bare_key = again.rom_key().expect("key").to_owned();
            let original_key = emulator.rom_key().expect("key").to_owned();
            for name in ["battery.sav", "battery.meta"] {
                let data = emulator.store.read_file(&original_key, name).expect(name);
                again.store.put_file(&bare_key, name, &data).expect(name);
            }
            again.reload_battery().expect("reload");
            assert_eq!(again.kind(), ConsoleKind::Gba);
            assert_eq!(chip(&mut again), Some(SaveType::Flash64K));
            assert_eq!(again.export_save_bytes(), Some(written));
        }

        #[test]
        fn a_save_written_by_mgba_imports_for_each_type() {
            for (kind, expected) in SAVES {
                let file = mgba_save(kind);
                let mut emulator = on_store(&store_root(&format!("mgba-{kind}")));
                start(&mut emulator, &save_rom(kind));
                let result = emulator.import_save(Some(&file), None).expect("import");
                assert_eq!(result, SaveImport::Imported, "{kind}");
                assert_eq!(
                    emulator.export_save_bytes().as_deref(),
                    Some(&file[..]),
                    "{kind}"
                );
                let key = emulator.rom_key().expect("key").to_owned();
                assert_eq!(
                    emulator.store.load_battery(&key).as_deref(),
                    Some(&file[..]),
                    "{kind}"
                );
                assert_eq!(chip(&mut emulator), Some(expected), "{kind}");
            }
        }

        #[test]
        fn citrine_writes_the_same_save_as_mgba() {
            for (kind, _) in SAVES {
                let mut emulator = on_store(&store_root(&format!("same-{kind}")));
                start(&mut emulator, &save_rom(kind));
                run(&mut emulator, FRAMES);
                let ours = emulator.export_save_bytes().expect("save");
                let theirs = mgba_save(kind);
                assert_eq!(ours.len(), theirs.len(), "{kind}");
                let fresh_bank = |at: usize| match kind {
                    "flash128" => at >= 0x1_0000,
                    "eeprom8k" => at >= 0x200,
                    _ => false,
                };
                let first = ours
                    .iter()
                    .zip(&theirs)
                    .enumerate()
                    .position(|(at, (&a, &b))| a != b && !(fresh_bank(at) && a == 0xFF && b == 0));
                assert_eq!(first, None, "{kind}");
            }
        }

        #[test]
        fn a_gba_snapshot_carries_the_magic_and_loads_back() {
            let mut emulator = on_store(&store_root("snapshot"));
            start(&mut emulator, &save_rom("sram"));
            run(&mut emulator, 10);
            emulator.save_snapshot(0).expect("snapshot");
            run(&mut emulator, 1);
            let frame = emulator.frame().to_vec();
            let key = emulator.rom_key().expect("key").to_owned();
            let state = emulator.store.load_snapshot(&key, 0).expect("stored");
            assert_eq!(state[..4], GBA_STATE_MAGIC);
            let info = emulator.store.snapshot_info(&key, 0).expect("info");
            assert_eq!(info.console, Some(ConsoleKind::Gba));
            assert_eq!(info.thumb_size(), [120, 80]);

            run(&mut emulator, FRAMES);
            assert!(emulator.load_snapshot(0).expect("load"));
            assert_eq!(emulator.kind(), ConsoleKind::Gba);
            run(&mut emulator, 1);
            assert_eq!(emulator.frame(), &frame[..]);
            assert_eq!(emulator.take_console_switch(), None);
        }

        #[test]
        fn a_gba_autosave_loads_back() {
            let mut emulator = on_store(&store_root("autosave"));
            start(&mut emulator, &save_rom("flash64"));
            run(&mut emulator, FRAMES);
            assert!(emulator.autosave().expect("autosave"));
            let id = autosaved(&mut emulator).expect("id");
            let saved = emulator.export_save_bytes().expect("save");
            run(&mut emulator, 5);
            assert!(emulator.load_autosave(id).expect("load"));
            assert_eq!(emulator.kind(), ConsoleKind::Gba);
            assert_eq!(emulator.export_save_bytes(), Some(saved));
        }

        #[test]
        fn a_state_with_another_version_byte_says_older_or_newer() {
            use crate::app::widgets::snapshots::{FailedStep, failed_message};
            let mut emulator = on_store(&store_root("version"));
            start(&mut emulator, &save_rom("sram"));
            run(&mut emulator, 10);
            emulator.save_snapshot(0).expect("snapshot");
            emulator.writes.settle();
            let key = emulator.rom_key().expect("key").to_owned();
            let state = emulator.store.load_snapshot(&key, 0).expect("stored");

            let told = |emulator: &mut Emulator, version: u8| {
                let mut changed = state.clone();
                changed[4] = version;
                let err = emulator.restore(&changed).expect_err("refused");
                failed_message(FailedStep::Load, "'Boss fight'", &err)
            };
            assert_eq!(
                told(&mut emulator, state[4] + 1),
                "Couldn't load 'Boss fight'. It needs a newer version of Citrine."
            );
            assert_eq!(
                told(&mut emulator, state[4] - 1),
                "Couldn't load 'Boss fight'. It was saved by an older version of Citrine."
            );
        }

        #[test]
        fn a_crash_during_a_gba_game_writes_its_state_and_battery() {
            let root = store_root("crash");
            let mut emulator = on_store(&root);
            start(&mut emulator, &save_rom("eeprom8k"));
            played(&mut emulator, 1.2);
            emulator.running = false;
            played(&mut emulator, 0.1);
            assert_eq!(emulator.last_good.write_out(), Ok(Some(0)));

            let key = emulator.rom_key().expect("key").to_owned();
            let state = emulator.store.load_autosave(&key, 0).expect("autosave");
            assert_eq!(state[..4], GBA_STATE_MAGIC);
            let mut again = on_store(&root);
            start(&mut again, &save_rom("eeprom8k"));
            assert_eq!(chip(&mut again), Some(SaveType::Eeprom8K));
            assert!(again.load_autosave(0).expect("load"));
            run(&mut again, 1);
            run(&mut emulator, 1);
            assert_eq!(again.frame(), emulator.frame());
        }

        #[test]
        fn a_second_gba_game_runs_linked_and_keeps_its_own_save() {
            let root = store_root("link");
            let mut emulator = on_store(&root);
            let rom = save_rom("sram");
            start(&mut emulator, &rom);
            let first_key = emulator.rom_key().expect("key").to_owned();
            emulator
                .start_link(&rom, ConsoleKind::Gba, Some("save-sram.gba"))
                .expect("link");
            let link = emulator.link().expect("a linked game");
            assert_eq!(link.kind(), ConsoleKind::Gba);
            assert_eq!(link.save_key(), format!("{first_key}-link2"));
            let started = web_time::Instant::now();
            for step in 1..=20 {
                emulator.advance(started + std::time::Duration::from_millis(step * 50), None);
            }
            let ours = emulator.frame().to_vec();
            let link = emulator.link().expect("a linked game");
            assert_eq!(link.machine.frame().len(), 240 * 160 * 4);
            assert_eq!(link.machine.frame(), ours);
            emulator.flush_save().expect("flush");
            assert!(
                emulator
                    .store
                    .load_battery(&format!("{first_key}-link2"))
                    .is_some()
            );
            emulator.end_link().expect("end");
            assert!(!emulator.linked());
        }

        const KEYINPUT: u32 = 0x0400_0130;

        fn shoulders_down(emulator: &mut Emulator) -> (bool, bool) {
            emulator.debug_mut().run_frame();
            let keyinput = emulator.as_gba().expect("gba").bus().read16(KEYINPUT);
            let owed = emulator.held.take_owed();
            emulator.apply(crate::held::Change {
                pressed: Keys::empty(),
                released: owed,
            });
            (
                keyinput & Keys::L.bits() == 0,
                keyinput & Keys::R.bits() == 0,
            )
        }

        fn gba_emulator(name: &str) -> Emulator {
            let mut emulator = on_store(&store_root(name));
            start(&mut emulator, &save_rom("flash64"));
            emulator
        }

        fn feed(emulator: &mut Emulator, events: Vec<PadEvent>) {
            let mut capture = BindingCapture::default();
            let mut pads = FakePads(events.into());
            emulator.handle_pads(&mut pads, &KeyBindings::default(), &mut capture, None);
        }

        fn pad_buttons(button: PadButton, down: bool) -> Vec<PadEvent> {
            let key = PadKey::Known(button);
            let input = if down {
                PadInput::Pressed(key)
            } else {
                PadInput::Released(key)
            };
            vec![PadEvent {
                pad: 0,
                input,
                code: None,
            }]
        }

        #[test]
        fn the_keyboard_presses_l_and_r_on_the_gba() {
            let mut emulator = gba_emulator("shoulder-keys");
            let bindings = KeyBindings::default();
            let mut capture = BindingCapture::default();
            let ctx = egui::Context::default();
            let mut key = |emulator: &mut Emulator, key: egui::Key, pressed: bool| {
                let input = egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    emulator.handle_input(
                        ctx,
                        &mut FakePads(Default::default()),
                        &bindings,
                        &mut capture,
                        None,
                    );
                });
            };
            for _ in 0..3 {
                key(&mut emulator, egui::Key::Num1, true);
                assert_eq!(shoulders_down(&mut emulator), (true, false));
                key(&mut emulator, egui::Key::Num3, true);
                assert_eq!(shoulders_down(&mut emulator), (true, true));
                key(&mut emulator, egui::Key::Num1, false);
                key(&mut emulator, egui::Key::Num3, false);
                assert_eq!(shoulders_down(&mut emulator), (false, false));
            }
        }

        #[test]
        fn bumpers_and_triggers_both_press_l_and_r() {
            let mut emulator = gba_emulator("shoulder-pads");
            for (button, expected) in [
                (PadButton::LeftTrigger, (true, false)),
                (PadButton::LeftTrigger2, (true, false)),
                (PadButton::RightTrigger, (false, true)),
                (PadButton::RightTrigger2, (false, true)),
            ] {
                for _ in 0..3 {
                    feed(&mut emulator, pad_buttons(button, true));
                    assert_eq!(shoulders_down(&mut emulator), expected, "{button:?}");
                    feed(&mut emulator, pad_buttons(button, false));
                    assert_eq!(shoulders_down(&mut emulator), (false, false), "{button:?}");
                }
            }
        }

        #[test]
        fn an_analog_trigger_presses_l_past_its_threshold_and_lets_go_below_the_lower_one() {
            use crate::pad_input::sdl_map::{SdlInput, Translator};
            let mut emulator = gba_emulator("shoulder-axis");
            let mut translator = Translator::default();
            let mut pull = |emulator: &mut Emulator, raw: i16| {
                let events = translator.translate(0, SdlInput::Axis(4, raw));
                feed(emulator, events);
                shoulders_down(emulator).0
            };
            for _ in 0..3 {
                assert!(!pull(&mut emulator, 16_000));
                assert!(pull(&mut emulator, 26_000));
                assert!(
                    pull(&mut emulator, 22_000),
                    "a hand that eases off a little keeps L"
                );
                assert!(pull(&mut emulator, 25_000));
                assert!(!pull(&mut emulator, 20_000));
                assert!(
                    !pull(&mut emulator, 23_000),
                    "below the press line L stays up"
                );
                assert!(!pull(&mut emulator, 0));
            }
        }

        #[test]
        fn a_shoulder_held_on_the_keyboard_and_a_pad_stays_down_until_both_let_go() {
            let mut emulator = gba_emulator("shoulder-both");
            emulator.press(InputSource::Keyboard, Keys::L);
            feed(&mut emulator, pad_buttons(PadButton::LeftTrigger, true));
            assert_eq!(shoulders_down(&mut emulator), (true, false));
            feed(&mut emulator, pad_buttons(PadButton::LeftTrigger, false));
            assert_eq!(shoulders_down(&mut emulator), (true, false));
            emulator.release(InputSource::Keyboard, Keys::L);
            assert_eq!(shoulders_down(&mut emulator), (false, false));
        }

        fn emerald_rom() -> Rom {
            let mut data = fixture("save-flash128.gba");
            data[0xAC..0xB0].copy_from_slice(b"BPEE");
            Rom::new(&data)
        }

        fn clock_of(emulator: &mut Emulator) -> citrine_gb::utils::date::DateTime {
            let Machine::Gba(gba) = emulator.debug_mut() else {
                panic!("not a GBA");
            };
            gba.clock()
        }

        #[test]
        fn a_clock_game_exports_its_clock_at_the_end_of_the_sav_for_mgba() {
            let mut emulator = on_store(&store_root("clock-export"));
            start(&mut emulator, &emerald_rom());
            assert!(emulator.save_status().has_clock);
            let Machine::Gba(gba) = emulator.debug_mut() else {
                panic!("not a GBA");
            };
            gba.set_clock(1_798_761_600 + 13 * 3600 + 37 * 60);
            let set = clock_of(&mut emulator);
            run(&mut emulator, FRAMES);
            emulator.flush_save().expect("flush");

            let file = emulator.export_save_bytes().expect("save");
            assert_eq!(file.len(), 128 * 1024 + 16);
            let footer =
                citrine_gb::persistence::seiko_footer::SeikoFooter::decode(&file[128 * 1024..])
                    .expect("footer");
            assert_eq!(&footer.time[..3], &[0x27, 0x01, 0x01]);
            assert_eq!(footer.time[5], 0x37);

            let mut other = on_store(&store_root("clock-import"));
            start(&mut other, &emerald_rom());
            assert_eq!(
                other.import_save(Some(&file), None).expect("import"),
                SaveImport::Imported
            );
            other.restart();
            let back = clock_of(&mut other);
            assert_eq!(
                (back.year, back.month, back.day, back.hour, back.minute),
                (set.year, set.month, set.day, set.hour, set.minute)
            );
            assert_eq!(other.export_save_bytes().map(|f| f.len()), Some(file.len()));
        }

        #[test]
        fn a_clock_file_for_a_gba_game_is_refused_with_the_reason() {
            let mut emulator = on_store(&store_root("clock-rtc"));
            start(&mut emulator, &emerald_rom());
            assert_eq!(emulator.import_blocker(true), Some(SaveImport::ClockInSave));
            let mut plain = on_store(&store_root("clock-none"));
            start(&mut plain, &save_rom("sram"));
            assert_eq!(plain.import_blocker(true), Some(SaveImport::NoClock));
        }
    }

    mod fast_forward {
        use super::*;
        use crate::emulator::fast_forward::FastForwardSpeed;
        use std::time::Duration;

        const TICK: f64 = 1.0 / 60.0;
        const TICKS: usize = 120;

        fn ticked(speed: Option<FastForwardSpeed>) -> Vec<Plan> {
            let mut emulator = emulator("fast-forward");
            running(&mut emulator, 0x00);
            emulator.running = true;
            if let Some(speed) = speed {
                emulator.fast_forward = true;
                emulator.fast_forward_speed = speed;
            }
            let start = web_time::Instant::now();
            (0..=TICKS)
                .filter_map(|tick| {
                    let now = start + Duration::from_secs_f64(tick as f64 * TICK);
                    emulator.prepare(now).map(|(plan, _)| plan)
                })
                .collect()
        }

        fn frames(plans: &[Plan]) -> usize {
            plans.iter().map(|plan| plan.frames).sum()
        }

        #[test]
        fn each_fixed_speed_runs_that_many_frames_per_game_frame() {
            let normal = frames(&ticked(None));
            assert!(normal > 0);
            for (speed, times) in [
                (FastForwardSpeed::Two, 2),
                (FastForwardSpeed::Three, 3),
                (FastForwardSpeed::Five, 5),
                (FastForwardSpeed::Ten, 10),
            ] {
                assert_eq!(frames(&ticked(Some(speed))), normal * times, "{speed}");
            }
        }

        #[test]
        fn three_times_is_the_default() {
            assert_eq!(FastForwardSpeed::default(), FastForwardSpeed::Three);
            assert_eq!(
                Emulator::default().fast_forward_speed,
                FastForwardSpeed::Three
            );
        }

        #[test]
        fn normal_play_has_no_time_budget() {
            assert!(ticked(None).iter().all(|plan| plan.budget.is_none()));
        }

        #[test]
        fn max_runs_until_its_share_of_each_display_frame_is_spent() {
            let plans = ticked(Some(FastForwardSpeed::Max));
            for plan in &plans[1..] {
                assert_eq!(plan.frames, usize::MAX);
                let budget = plan.budget.expect("a time budget");
                assert!(budget > Duration::ZERO);
                assert!(budget < Duration::from_secs_f64(TICK));
            }
        }

        #[test]
        fn a_fixed_speed_is_capped_by_the_same_budget() {
            let plans = ticked(Some(FastForwardSpeed::Ten));
            let max = ticked(Some(FastForwardSpeed::Max));
            for (ten, max) in plans.iter().zip(&max).skip(1) {
                assert_eq!(ten.budget, max.budget);
            }
        }

        #[test]
        fn a_stall_never_buys_a_longer_budget() {
            let mut emulator = emulator("fast-forward-stall");
            running(&mut emulator, 0x00);
            emulator.running = true;
            emulator.fast_forward = true;
            emulator.fast_forward_speed = FastForwardSpeed::Max;
            let start = web_time::Instant::now();
            emulator.prepare(start);
            let (plan, _) = emulator
                .prepare(start + Duration::from_secs(2))
                .expect("running");
            assert!(plan.budget.expect("a time budget") < Duration::from_millis(50));
        }

        fn budget_plan(emulator: &Emulator, frames: usize, budget: Option<Duration>) -> Plan {
            Plan {
                frames,
                held: Keys::empty(),
                owed: Keys::empty(),
                effects: emulator.effects(),
                sound: emulator.sound,
                fast_forward: true,
                take: false,
                rewind: rewind::RewindPlan::OFF,
                budget,
                defer_3d: false,
                pace: 1.0,
            }
        }

        fn run(emulator: &mut Emulator, plan: &Plan) -> usize {
            emulator
                .seat
                .core_mut()
                .run(
                    plan,
                    &mut runner::NoHooks,
                    &mut citrine_gb_threads::Split::inline(),
                )
                .frames
        }

        #[test]
        fn a_spent_budget_still_runs_one_frame() {
            let mut emulator = emulator("fast-forward-spent");
            running(&mut emulator, 0x00);
            let plan = budget_plan(&emulator, usize::MAX, Some(Duration::ZERO));
            assert_eq!(run(&mut emulator, &plan), 1);
            let plan = budget_plan(&emulator, 10, Some(Duration::ZERO));
            assert_eq!(run(&mut emulator, &plan), 1);
        }

        #[test]
        fn max_stops_once_its_budget_is_spent() {
            let mut emulator = emulator("fast-forward-max");
            running(&mut emulator, 0x00);
            let plan = budget_plan(&emulator, usize::MAX, Some(Duration::from_millis(20)));
            let started = web_time::Instant::now();
            let ran = run(&mut emulator, &plan);
            assert!(ran >= 1);
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn a_reachable_speed_runs_every_frame_it_asked_for() {
            let mut emulator = emulator("fast-forward-plenty");
            running(&mut emulator, 0x00);
            let plan = budget_plan(&emulator, 10, Some(Duration::from_secs(600)));
            assert_eq!(run(&mut emulator, &plan), 10);
        }
    }

    mod screen_match {
        use super::*;
        use crate::emulator::load::ScreenMatch;
        use std::time::Duration;

        fn shown_at(hz: f64, matching: bool) -> Emulator {
            let mut emulator = emulator("screen-match");
            running(&mut emulator, 0x00);
            emulator.running = true;
            emulator.clock.set_matching(matching);
            let start = web_time::Instant::now();
            for refresh in 0..=(hz as usize * 5) {
                let now = start + Duration::from_secs_f64(refresh as f64 / hz);
                let _ = emulator.prepare(now);
            }
            emulator
        }

        #[test]
        fn a_matched_screen_gives_the_game_a_60_hz_budget() {
            let emulator = shown_at(120.0, true);
            assert_eq!(emulator.screen_match(), Some(ScreenMatch::Matched));
            let rate = 1.0 / emulator.frame_budget().game.as_secs_f64();
            assert!((rate - 60.0).abs() < 0.01, "{rate}");
        }

        #[test]
        fn a_139_hz_screen_names_its_rate_and_keeps_the_console_budget() {
            let emulator = shown_at(139.0, true);
            let Some(ScreenMatch::Unmatched(hz)) = emulator.screen_match() else {
                panic!("{:?}", emulator.screen_match());
            };
            assert!((hz - 139.0).abs() < 0.5, "{hz}");
            assert_eq!(
                emulator.frame_budget().game,
                Duration::from_secs_f64(emulator.timing().frame_secs())
            );
        }

        #[test]
        fn matching_held_off_or_turned_off_says_so() {
            let mut emulator = shown_at(120.0, false);
            assert_eq!(emulator.screen_match(), Some(ScreenMatch::Shared));
            emulator.match_screen = false;
            assert_eq!(emulator.screen_match(), Some(ScreenMatch::Off));
            assert_eq!(Emulator::default().screen_match(), None);
        }
    }

    #[cfg(threaded)]
    mod refresh_skip {
        use super::*;
        use std::time::Duration;

        const REFRESH_180: Duration = Duration::from_nanos(5_555_556);
        const DS_FRAME: Duration = Duration::from_nanos(16_715_000);

        fn lent_with_cost(cost: Duration) -> Emulator {
            let mut emulator = emulator("refresh-skip");
            running(&mut emulator, 0x00);
            emulator.running = true;
            let start = web_time::Instant::now();
            let basis = Some((REFRESH_180, DS_FRAME));
            emulator.show_pace.begin(None);
            emulator.show_pace.lent(start, 1, basis);
            emulator
                .show_pace
                .shown(Some(start + cost), REFRESH_180, REFRESH_180);
            emulator.show_pace.begin(None);
            emulator.show_pace.lent(start, 1, basis);
            emulator.lent = Some(0.0);
            emulator
        }

        #[test]
        fn a_frame_shown_two_refreshes_after_its_lend_lets_the_app_sleep_until_then() {
            let emulator = lent_with_cost(Duration::from_millis(12));
            assert_eq!(emulator.show_pace.refreshes_to_show(), Some(2));
            assert_eq!(emulator.idle_refreshes(), Some(2));
        }

        #[test]
        fn a_frame_shown_at_the_next_refresh_wakes_at_the_next_refresh() {
            let emulator = lent_with_cost(Duration::from_millis(9));
            assert_eq!(emulator.idle_refreshes(), Some(1));
        }

        #[test]
        fn a_frame_that_fits_its_refresh_keeps_every_refresh() {
            let emulator = lent_with_cost(Duration::from_millis(3));
            assert_eq!(emulator.idle_refreshes(), None);
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_screenshot_and_recording_follow_the_screen_layout() {
        use crate::app::ui_state::ds_screens::{Arrangement, Placement};
        use crate::media::recording::Source;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        let rom = Rom::new(&std::fs::read(path).expect("fixture"));
        let mut emulator = emulator("ds-layouts");
        emulator
            .load_game(&rom, ConsoleKind::Nds, None)
            .expect("ds loads");
        for _ in 0..60 {
            emulator.debug_mut().run_frame();
        }
        let stacked = Placement::default();
        for (name, placement, size) in [
            ("stacked", stacked, (256, 384)),
            (
                "swapped",
                Placement {
                    swapped: true,
                    ..stacked
                },
                (256, 384),
            ),
            (
                "side-by-side",
                Placement {
                    arrangement: Arrangement::SideBySide,
                    ..stacked
                },
                (512, 192),
            ),
            (
                "one-screen",
                Placement {
                    one_screen: true,
                    ..stacked
                },
                (256, 192),
            ),
        ] {
            emulator.ds_screens.desktop = placement;
            let (width, height, rgba) = emulator.screenshot(2);
            assert_eq!((width, height), (size.0 * 2, size.1 * 2), "{name}");
            assert_eq!(rgba.len(), width * height * 4, "{name}");
            let source = Source::laid_out(emulator.shown_layout(), emulator.timing());
            assert_eq!((source.width, source.height), size, "{name}");
            if let Some(dir) = std::env::var_os("CITRINE_DS_SHOTS") {
                let png = crate::utils::screenshot::encode_png(width, height, &rgba).expect("png");
                std::fs::create_dir_all(&dir).expect("dir");
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png)
                    .expect("write");
            }
        }
        let swapped = {
            emulator.ds_screens.desktop = Placement {
                swapped: true,
                ..stacked
            };
            emulator.screenshot(1).2
        };
        emulator.ds_screens.desktop = stacked;
        let upright = emulator.screenshot(1).2;
        let half = upright.len() / 2;
        assert_eq!(swapped[..half], upright[half..]);
        assert_eq!(swapped[half..], upright[..half]);
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn a_gba_game_has_a_jit_cache_only_when_its_jit_runs() {
        use crate::app::ui_state::settings::GBA_USES_JIT;
        use citrine_gb::nds::machine::JIT_AVAILABLE;

        let fixture = |path: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
            Rom::new(&std::fs::read(path).expect("fixture"))
        };
        let mut emulator = emulator("gba-jit-cache");
        let gba = fixture("fixtures/gba/save-sram.gba");
        emulator
            .load_game(&gba, ConsoleKind::Gba, None)
            .expect("gba loads");
        let name = emulator.jit_cache.as_ref().map(|cache| cache.file_name());
        assert_eq!(name.is_some(), JIT_AVAILABLE && GBA_USES_JIT);

        let ds = fixture("fixtures/nds/blocksds-console-both-screens.nds");
        emulator
            .load_game(&ds, ConsoleKind::Nds, None)
            .expect("ds loads");
        let ds_name = emulator.jit_cache.as_ref().map(|cache| cache.file_name());
        if JIT_AVAILABLE {
            assert_ne!(ds_name, name, "each game has its own cache file");
        }

        emulator.unload();
        emulator
            .load_rom(&rom(0x00), GbModel::Dmg, None)
            .expect("gb loads");
        assert!(
            emulator.jit_cache.is_none(),
            "a Game Boy game has no JIT cache"
        );
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_jit_switch_reaches_a_running_ds_game() {
        use citrine_gb::nds::machine::JIT_AVAILABLE;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        let rom = Rom::new(&std::fs::read(path).expect("the DS fixture"));
        let mut emulator = emulator("native-code");
        emulator
            .load_game(&rom, ConsoleKind::Nds, None)
            .expect("ds loads");
        let jit_on = |emulator: &Emulator| match &emulator.seat.core().machine {
            Machine::Nds(nds) => nds.jit_on(),
            _ => panic!("a DS game"),
        };
        assert_eq!(
            emulator.ds_jit,
            crate::app::ui_state::settings::DS_JIT_DEFAULT
        );
        play_a_frame(&mut emulator);
        assert_eq!(
            jit_on(&emulator),
            JIT_AVAILABLE && crate::app::ui_state::settings::DS_JIT_DEFAULT
        );

        emulator.ds_jit = false;
        play_a_frame(&mut emulator);
        assert!(!jit_on(&emulator), "off goes back to the interpreter");

        emulator.ds_jit = true;
        play_a_frame(&mut emulator);
        assert_eq!(jit_on(&emulator), JIT_AVAILABLE);
    }

    #[cfg(threaded)]
    #[test]
    fn an_update_that_ticks_away_and_then_lends_feeds_the_clock_once() {
        let mut emulator = emulator("tick-away-once");
        running(&mut emulator, 0x00);
        let ctx = egui::Context::default();
        let start = web_time::Instant::now();
        let refresh = 1.0 / 180.0;
        for n in 0..240 {
            let now = start + std::time::Duration::from_secs_f64(n as f64 * refresh);
            emulator.tick_away(&ctx, now);
            if emulator.away_due > 0 {
                emulator.prepare(now);
            }
        }

        let period = emulator.refresh_period().expect("settled");
        assert!((period - refresh).abs() < refresh * 0.01, "{period}");
    }
}
