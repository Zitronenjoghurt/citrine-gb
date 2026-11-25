use crate::app::tabs::Tab;
use crate::app::ui_state::UiState;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::synced;
use crate::app::widgets::snapshots::{FailedStep, failed_message};
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::media::MediaStore;
#[cfg(not(target_arch = "wasm32"))]
use crate::media::recording::Outcome;
use crate::online::Online;
use crate::play::Play;
use crate::storage::{SettingsStore, Written};
use crate::utils::file_channels::FileChannels;
use crate::utils::save_files::SaveFiles;
use citrine_gb::debug::Debug;
use citrine_gb::gb::GbModel;
use eframe::{Frame, Storage};
use egui::{Context, FontDefinitions};
use egui_commonmark::CommonMarkCache;
use egui_dock::DockState;
use toasts::Toasts;

const QUICK_LOAD_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(target_arch = "wasm32")]
const AUDIO_STALL_SECS: f32 = 1.0;

const DOCK_PANE_MIN: f32 = 120.0;

const FRIENDS_DOCK_SHARE: f32 = 0.34;
const FRIENDS_DOCK_WIDTH: f32 = 180.0;
const FRIENDS_STACK_SHARE: f32 = 0.6;
const GAME_INFO_DOCK_SHARE: f32 = 0.42;

const SETTINGS_WRITE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const AUTO_SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

const IDLE_TICK: std::time::Duration = std::time::Duration::from_millis(50);
#[cfg(not(target_arch = "wasm32"))]
const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(not(target_arch = "wasm32"))]
mod achievements;
mod archive;
mod audio;
mod background;
pub(crate) mod big_screen;
pub mod boot_rom;
mod canvas;
mod capture;
mod clip;
mod cloud;
mod console;
mod covers;
#[cfg(feature = "nds")]
pub mod ds_firmware;
mod ds_screens;
mod events;
mod fast_forward;
mod files;
pub mod focus;
#[cfg(feature = "gba")]
mod gba_bios;
mod gimmicks;
pub(crate) mod hardcore;
mod hotkeys;
pub mod hud;
mod info_dock;
#[cfg(not(target_arch = "wasm32"))]
mod lan;
pub(crate) mod layout_edit;
mod layout_picks;
pub(crate) mod layout_sheet;
mod library;
mod link;
#[cfg(feature = "nds")]
mod mic;
#[cfg(feature = "nds")]
pub(crate) mod mic_mark;
mod mobile;
pub(crate) mod monitor;
#[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
mod nearby_walkers;
mod online;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub mod pacing_log;
mod peer_saves;
mod push;
pub(crate) mod recording;
mod render;
mod repaint;
mod replay_show;
mod report;
mod reset;
pub(crate) mod screen_extras;
mod second_screens;
mod shelf;
mod storage_page;
pub mod system_files;
pub(crate) mod tabs;
mod toasts;
mod toolbar;
pub mod ui_state;
mod video_render;
pub mod viewer;
pub(crate) mod widgets;
mod your_data;
mod zips;

#[derive(serde::Deserialize)]
struct Saved {
    dock: DockState<Tab>,
    ui: Box<UiState>,
}

#[derive(serde::Serialize)]
pub struct Citrine {
    pub dock: DockState<Tab>,
    pub ui: Box<UiState>,
    #[serde(skip, default)]
    pub emulator: Box<Emulator>,
    #[serde(skip)]
    threads: citrine_gb_threads::Threads,
    #[serde(skip, default)]
    pub files: FileChannels,
    #[serde(skip, default)]
    rom_ids: crate::rom_ids::RomIds,
    #[serde(skip, default)]
    pub pads: big_screen::PadGate,
    #[serde(skip, default)]
    big: big_screen::BigScreen,
    #[serde(skip, default)]
    pub toasts: Toasts,
    #[serde(skip, default)]
    pub focus_popups: focus::popups::FocusPopups,
    #[serde(skip, default)]
    pub audio: Option<Audio>,
    #[serde(skip, default)]
    pub events: events::AppEventQueue,
    #[serde(skip, default)]
    pub commonmark: CommonMarkCache,
    #[serde(skip, default)]
    quick_load_hold: Option<web_time::Instant>,
    #[serde(skip, default)]
    away: background::Away,
    rewind_pressed: bool,
    #[serde(skip, default)]
    fast_forward: fast_forward::FastForwardInput,
    #[serde(skip, default)]
    mobile: mobile::MobileState,
    #[serde(skip, default)]
    audio_stalled: bool,
    #[serde(skip, default)]
    record_when_ready: bool,
    #[serde(skip, default)]
    tape_media: Option<crate::media::NewMedia>,
    #[serde(skip, default)]
    parked_replays: Vec<capture::ParkedReplay>,
    #[serde(skip, default)]
    video_replays: capture::VideoReplays,
    #[serde(skip, default)]
    video_renders: video_render::VideoRenders,
    #[serde(skip, default)]
    replay_show: replay_show::ReplayShow,
    #[serde(skip, default)]
    clip_work: clip::ClipWork,
    #[serde(skip, default)]
    codec_prompt: bool,
    #[serde(skip, default)]
    settings_store: SettingsStore,
    #[serde(skip, default)]
    settings_saved_at: Option<web_time::Instant>,
    #[serde(skip, default)]
    pending_import: Option<PendingImport>,
    #[serde(skip, default)]
    zip: Option<zips::ZipOpen>,
    #[serde(skip, default)]
    digested: Option<(std::sync::Arc<[u8]>, crate::rom_ids::Ids)>,
    #[serde(skip, default)]
    pending_reset: Option<PendingReset>,
    #[serde(skip, default)]
    reset_without_asking: bool,
    #[serde(skip, default)]
    pending_console: Option<PendingConsole>,
    #[serde(skip, default)]
    pending_peer_discard: Option<crate::emulator::peer_saves::PeerNotice>,
    #[serde(skip, default)]
    pub media: MediaStore,
    #[serde(skip, default)]
    pub(crate) senses: crate::senses::Senses,
    #[cfg(feature = "nds")]
    #[serde(skip, default)]
    pub(crate) mic: crate::audio::mic::Mic,
    #[cfg(feature = "nds")]
    #[serde(skip, default)]
    pub(crate) talk_pressed: bool,
    #[serde(skip, default)]
    pub online: Online,
    #[serde(skip, default)]
    pub play: Box<Play>,
    #[serde(skip, default)]
    activity: Activity,
    #[serde(skip, default)]
    cloud_rom: Option<String>,
    #[serde(skip, default)]
    cloud_keep: Option<(u64, crate::online::KeepSaves)>,
    #[serde(skip, default)]
    pub game_info: crate::game_info::GameInfo,
    #[serde(skip, default)]
    cloud_hold: Option<CloudHold>,
    #[serde(skip, default)]
    pub stats: crate::stats::Stats,
    #[serde(skip, default)]
    shelf: crate::homebrew::loader::ShelfLoader,
    #[serde(skip, default)]
    archive: archive::ArchiveShelf,
    #[serde(skip, default)]
    covers: crate::covers::Finder,
    #[serde(skip, default)]
    game_facts: crate::game_facts::Finder,
    #[serde(skip, default)]
    achievements: Box<crate::achievements::Achievements>,
    #[serde(skip, default)]
    second: Box<crate::second_screens::SecondScreens>,
    #[serde(skip, default)]
    surface: second_screens::Surfaces,
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip, default)]
    lan: lan::LanPeers,
    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[serde(skip, default)]
    nearby_walkers: nearby_walkers::NearbyWalkers,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[serde(skip, default)]
    discord: crate::discord::Rpc,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[serde(skip, default)]
    shots: crate::discord::Shots,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[serde(skip, default)]
    pacing_log: Option<pacing_log::PacingLog<std::io::BufWriter<std::fs::File>>>,
    #[serde(skip, default)]
    repaint: repaint::Repaint,
    #[serde(skip, default)]
    monitor: monitor::WindowMonitor,
    #[serde(skip, default)]
    present_pace: monitor::PresentPace,
}

struct CloudHold {
    since: web_time::Instant,
    was_running: bool,
}

const CLOUD_HOLD_LIMIT: std::time::Duration = std::time::Duration::from_secs(4);

struct Activity {
    last_input: web_time::Instant,
    held: citrine_gb::console::Keys,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            last_input: web_time::Instant::now(),
            held: citrine_gb::console::Keys::empty(),
        }
    }
}

const RESET_STOP_ASKING: &str = "Don't ask again until Citrine closes";

struct PendingReset {
    was_running: bool,
    stop_asking: bool,
}

struct PendingConsole {
    model: GbModel,
    rom_key: Option<String>,
    was_running: bool,
}

struct PendingImport {
    files: SaveFiles,
    rom_key: Option<String>,
    was_running: bool,
}

fn app_threads() -> citrine_gb_threads::Threads {
    let mode = if cfg!(test) || cfg!(not(threaded)) {
        citrine_gb_threads::Mode::Inline
    } else {
        citrine_gb_threads::Mode::Threads
    };
    citrine_gb_threads::Threads::new(mode)
}

impl Default for Citrine {
    fn default() -> Self {
        let dock = DockState::new(vec![Tab::GameBoy]);
        let threads = app_threads();
        let mut app = Self {
            dock,
            ui: Box::default(),
            emulator: Box::default(),
            files: FileChannels::new(threads.spawner()),
            covers: crate::covers::Finder::new(crate::covers::net::platform(&threads.spawner())),
            game_facts: crate::game_facts::Finder::new(crate::covers::net::platform(
                &threads.spawner(),
            )),
            achievements: Box::new(crate::achievements::Achievements::new(&threads.spawner())),
            threads,
            rom_ids: crate::rom_ids::RomIds::default(),
            pads: big_screen::PadGate::default(),
            big: big_screen::BigScreen::default(),
            toasts: Toasts::default(),
            focus_popups: focus::popups::FocusPopups::default(),
            audio: None,
            events: events::AppEventQueue::default(),
            commonmark: CommonMarkCache::default(),
            quick_load_hold: None,
            away: background::Away::default(),
            rewind_pressed: false,
            fast_forward: Default::default(),
            mobile: mobile::MobileState::default(),
            audio_stalled: false,
            record_when_ready: false,
            tape_media: None,
            parked_replays: Vec::new(),
            video_replays: capture::VideoReplays::default(),
            video_renders: video_render::VideoRenders::default(),
            replay_show: replay_show::ReplayShow::default(),
            clip_work: clip::ClipWork::default(),
            codec_prompt: false,
            settings_store: SettingsStore::default(),
            settings_saved_at: None,
            pending_import: None,
            zip: None,
            digested: None,
            pending_reset: None,
            reset_without_asking: false,
            pending_console: None,
            pending_peer_discard: None,
            media: MediaStore::default(),
            senses: crate::senses::Senses::default(),
            #[cfg(feature = "nds")]
            mic: crate::audio::mic::Mic::default(),
            #[cfg(feature = "nds")]
            talk_pressed: false,
            online: Online::default(),
            play: Box::default(),
            activity: Activity::default(),
            cloud_rom: None,
            cloud_keep: None,
            cloud_hold: None,
            game_info: Default::default(),
            stats: crate::stats::Stats::default(),
            shelf: Default::default(),
            archive: Default::default(),
            second: Box::default(),
            surface: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            lan: Default::default(),
            #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
            nearby_walkers: Default::default(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            discord: crate::discord::Rpc::default(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            shots: crate::discord::Shots::default(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            pacing_log: None,
            repaint: repaint::Repaint::default(),
            monitor: monitor::WindowMonitor::default(),
            present_pace: monitor::PresentPace::default(),
        };
        app.open_tab(Tab::Info);
        app.emulator.writes = crate::storage::Writes::start(&app.threads);
        #[cfg(not(target_arch = "wasm32"))]
        app.second.set_spawner(app.threads.spawner());
        #[cfg(threaded)]
        app.emulator.run_on(&app.threads);
        app
    }
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        Self::setup_fonts(&cc.egui_ctx);
        catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MOCHA);
        #[cfg(target_arch = "wasm32")]
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);

        let mut app = Self::restore(cc.storage);

        if let Some(settings) = app.settings_store.load::<Settings>() {
            app.ui.settings = settings;
        }

        app.ui.settings.sanitize();
        app.load_boot_roms();
        #[cfg(feature = "gba")]
        app.load_gba_bios();
        #[cfg(feature = "nds")]
        app.load_ds_files();
        #[cfg(feature = "nds")]
        app.sync_ds_user();
        app.emulator.store.move_cache_out();
        app.emulator.reopen_peer_notices();
        #[cfg(not(target_arch = "wasm32"))]
        crate::storage::tidy_folders();
        app.online.persist_chats();
        app.online.cloud_attach(app.emulator.store.clone());
        app.online.cloud_spawner(app.threads.spawner());
        app.stats = crate::stats::Stats::open(crate::stats::StatsStore::default());
        if let Some(relay) = app.ui.settings.online.joined_relay() {
            app.online.sign_in(relay, web_time::Instant::now());
        }
        app.audio = Some(Audio::new());
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            app.pacing_log = pacing_log::PacingLog::from_env();
        }
        app.emulator.running = false;
        app.mobile.listen_for_back();
        crate::autosave::install_panic_hook(app.emulator.last_good.crash_copy());
        #[cfg(target_arch = "wasm32")]
        crate::autosave::save_when_hidden(app.emulator.last_good.crash_copy());
        #[cfg(all(feature = "dev-route", target_arch = "wasm32"))]
        crate::dev_route::open(&mut app);
        #[cfg(target_arch = "wasm32")]
        app.join_from_link();
        #[cfg(target_arch = "wasm32")]
        crate::rom_ids::warm_hashes();
        #[cfg(target_os = "android")]
        crate::platform::android::lifecycle::save_on_pause(app.emulator.last_good.crash_copy());

        app
    }

    fn restore(storage: Option<&dyn eframe::Storage>) -> Self {
        let mut app = Self::default();
        if let Some(saved) =
            storage.and_then(|storage| eframe::get_value::<Saved>(storage, eframe::APP_KEY))
        {
            app.dock = saved.dock;
            app.ui = saved.ui;
        }
        app.ui.seed_library();
        tabs::retire_old_tabs(&mut app.dock);
        app
    }

    #[cfg(threaded)]
    fn follow_visibility(&mut self, visible: bool) {
        self.emulator.follow_visibility(visible, &self.threads);
        #[cfg(feature = "nds")]
        if !visible {
            self.emulator.save_jit_cache();
        }
        self.play.place_engines(visible.then_some(&self.threads));
    }

    fn run_side_jobs(&mut self) {
        for panicked in self.threads.run_side_jobs() {
            log::error!("a side job panicked: {}", panicked.0);
        }
    }

    fn needs_every_frame(&self) -> bool {
        (self.emulator.has_game() && self.emulator.is_running())
            || self.play.session().is_some()
            || self.play.room().is_some()
            || !self.toasts.texts().is_empty()
    }

    fn idle_refreshes(&self) -> Option<u64> {
        if self.play.session().is_some()
            || self.play.room().is_some()
            || !self.toasts.texts().is_empty()
        {
            return None;
        }
        self.emulator.idle_refreshes()
    }

    fn plan_repaint(&mut self, ctx: &Context, started: web_time::Instant) {
        let busy = self.needs_every_frame();
        let idle = self.idle_refreshes();
        let measured = self.emulator.refresh_period();
        let hz = self.monitor.rate().map(|rate| rate.hz);
        let not_before = self
            .present_pace
            .plan(started, hz, web_time::Instant::now());
        self.repaint
            .finish(ctx, busy, idle, measured, IDLE_TICK, not_before);
    }

    fn follow_monitor(&mut self, frame: &Frame, now: web_time::Instant) {
        if let Some(mut screens) = crate::platform::monitor::OsScreens::of(frame) {
            self.monitor.follow(&mut screens, now);
        }
        self.repaint
            .follow_display(self.monitor.monitor().map(|monitor| monitor.id));
        self.emulator.set_display(crate::emulator::Display {
            monitor: self.monitor.rate(),
            paced: self.present_pace.paced(),
        });
    }

    pub(crate) fn setup_fonts(ctx: &Context) {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        crate::icons::add_font(&mut fonts);
        crate::app::widgets::emoji::add_font(&mut fonts);
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for Citrine {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        crate::profile::frame(ctx);
        crate::profile::scope!("update");
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        if let Some(state) = _frame.wgpu_render_state() {
            self.emulator.set_gpu(ctx, state);
        }
        let frame_start = web_time::Instant::now();
        self.follow_monitor(_frame, frame_start);
        #[cfg(not(target_arch = "wasm32"))]
        self.ui
            .record_watch
            .calibrate(_frame, &self.emulator, &self.ui.frame_log);
        self.ui.update_avg_timer.start();
        #[cfg(all(feature = "dev-route", target_arch = "wasm32"))]
        crate::dev_route::tick(self, ctx);
        #[cfg(target_os = "android")]
        if let Some(audio) = &mut self.audio {
            crate::platform::android::lifecycle::follow_visibility(audio);
        }
        #[cfg(target_os = "android")]
        let visible = crate::platform::android::lifecycle::visible();
        #[cfg(not(target_os = "android"))]
        let visible = true;
        #[cfg(threaded)]
        self.follow_visibility(visible);
        let refreshes = self.repaint.begin(ctx);
        self.emulator.set_refreshes(refreshes);

        #[cfg(target_arch = "wasm32")]
        if ctx.input(|i| i.unstable_dt) > AUDIO_STALL_SECS {
            self.audio_stalled = true;
        }

        let apu_open = self.dock.find_tab(&Tab::Apu).is_some();
        self.emulator.debug_mut().record_waves(apu_open);

        mobile::touch_spacing(ctx, self.is_mobile(ctx));
        self.name_games();
        {
            crate::profile::scope!("online");
            self.update_online(ctx);
        }
        self.emulator.set_hotkeys(&self.ui.settings.hotkeys);
        self.begin_console(ctx);
        self.run_side_jobs();
        #[cfg(not(target_arch = "wasm32"))]
        self.media
            .follow_folder(self.ui.settings.capture_folder.as_deref());
        self.find_covers();
        self.follow_library(ctx);
        self.follow_archive(ctx);
        self.ui
            .update
            .tick(&self.files.spawner, self.ui.settings.check_updates);
        self.count_your_data();
        self.count_storage();
        #[cfg(feature = "nds")]
        self.follow_walker(ctx);
        self.step_second_screens(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.follow_lan();
        #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
        self.follow_nearby_walkers(ctx);
        {
            crate::profile::scope!("render");
            if self.joined_as_surface() {
                self.render_surface(ctx);
            } else {
                self.render(ctx);
            }
        }
        self.surface_windows(ctx);
        self.second_screens_input();
        self.ui.join.save_asked_codes(&self.files.spawner);
        if !self.layout_edit_keys(ctx)
            && self.ui.focus_mode
            && self.focus_exit_names().is_empty()
            && !self.ui.viewer.is_open()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.toggle_focus_mode();
        }
        self.media_viewer(ctx);

        self.follow_quick_load(ctx);
        self.follow_rewind(ctx);
        self.follow_fast_forward(ctx);
        self.quick_load_overlay(ctx);
        self.import_confirm(ctx);
        self.codec_confirm(ctx);
        self.reset_confirm(ctx);
        self.load_state_confirm(ctx);
        self.unlink_load_confirm(ctx);
        self.console_confirm(ctx);
        self.peer_discard_confirm(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.unsent_confirm(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.hardcore_confirm(ctx);
        self.cloud_conflict(ctx);
        self.handle_your_data(ctx);
        self.handle_storage(ctx);
        tabs::playing::stop_confirm(ctx, &mut self.play, &mut self.online, &mut self.ui.playing);
        tabs::playing::relink_confirm(
            ctx,
            &mut self.play,
            &self.online,
            &self.emulator,
            &mut self.ui.playing,
        );
        self.play_picker(ctx);
        if let Some(next) =
            tabs::playing::switch_confirm(ctx, &mut self.ui.playing, &self.online, &self.play)
        {
            self.play.leave(&mut self.online);
            self.ui.playing.switch_to = Some(next);
        }
        if let Some(invite) =
            tabs::playing::join_confirm(ctx, &mut self.ui.playing, &self.online, &self.play)
        {
            self.play.leave(&mut self.online);
            self.ui.playing.join_after = Some(invite);
        }
        self.note_console_switch();
        self.mobile_escape(ctx);
        self.ui.capture.end_frame();
        self.ui.performance.take_seen();
        self.ui.performance.update(
            web_time::Instant::now(),
            self.emulator.running,
            self.emulator.load(),
        );
        let watching = self.ui.threads_debug.take_seen();
        if self.threads.watching() != watching {
            self.threads.watch(watching);
        }
        let debug_open = self.dock.find_tab(&Tab::ControllerDebug).is_some()
            || self.mobile.showing(Tab::ControllerDebug);
        let now = web_time::Instant::now();
        self.ui.controller_debug.set_open(debug_open, now);
        self.ui
            .controller_debug
            .note_focus(ctx.input(|i| i.viewport().focused), now);

        self.drain_file_channels();
        #[cfg(feature = "nds")]
        self.sync_ds_user();
        self.handle_dropped_files(ctx);
        self.zip_frame(ctx);
        self.unlock_audio_on_interaction(ctx);
        self.watch_cloud_game();
        self.sense_gimmicks(ctx);
        #[cfg(feature = "nds")]
        self.follow_mic(ctx);

        let Self {
            emulator,
            pads,
            ui,
            play,
            ..
        } = self;
        let result = emulator.update_linked(
            ctx,
            pads.as_mut(),
            &ui.settings.bindings,
            &mut ui.capture,
            ui.controller_debug.log_mut(),
            play.session_mut().filter(|s| s.is_live()),
        );
        if let Err(err) = result {
            self.toasts.error(format!("Emulation error: {err}"));
        }
        #[cfg(all(threaded, target_arch = "wasm32"))]
        if let Some(audio) = &self.audio {
            audio.pump();
        }
        if let Some(friend) = self.emulator.take_handed_back() {
            let name = tabs::playing::member_name(&self.play, &self.online, friend);
            self.ui.playing.handed_back(friend, &name);
        }
        self.tell_peer_saves();
        self.publish_second_screens();
        self.use_gimmicks();
        self.run_shortcuts();
        self.play.flush(&mut self.online, web_time::Instant::now());
        self.record_stats();
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            let game = crate::discord::game_now(&self.emulator, &self.stats, &self.game_info);
            let image = self.discord_screen();
            let presence =
                crate::discord::presence(&self.ui.settings.discord, game.as_ref(), image);
            self.discord.show(&self.files.spawner, presence);
        }
        self.settle_play(ctx);
        if let Err(err) = self.emulator.autosave_if_due() {
            self.toasts
                .error(failed_message(FailedStep::Save, "the autosave", &err));
        }
        #[cfg(feature = "nds")]
        self.emulator.jit_cache_due();
        self.show_save_news();
        self.finish_recordings(ctx);
        self.show_loads();
        self.watch_shelf();
        self.watch_game_info();

        self.show_toasts(ctx);
        self.big_screen_end(ctx);

        let mobile = self.is_mobile(ctx);
        if !mobile {
            crate::app::ui_state::ui_theme::paint_focus_ring(ctx);
        }
        self.handle_event_queue(mobile);
        let changed = self.ui.settings.dirty;
        if let Some(err) = self
            .ui
            .settings
            .apply(ctx, &mut self.audio, &mut self.emulator, mobile)
        {
            self.toasts.error(err);
        }
        if changed {
            synced::note(&mut self.ui.settings, crate::online::synced::now_millis());
        }
        self.follow_background(ctx, visible);
        self.emulator.rest_pointer(ctx);

        self.flush_settings(false);
        #[cfg(target_os = "android")]
        crate::platform::android::frame_end(ctx);

        let spent = self.ui.update_avg_timer.stop();
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        if let Some(log) = self.pacing_log.as_mut() {
            let pace = self.emulator.pace();
            log.frame(pacing_log::PacingSample {
                start: frame_start,
                cpu: _frame.info().cpu_usage,
                update: spent,
                game_frames: self.emulator.total_frames,
                shown_twice: pace.repeated_per_sec(),
                dropped: pace.skipped_per_sec(),
                refresh_period: self.emulator.refresh_period(),
                load: self.emulator.load(),
                surface_errors: crate::platform::surface_errors(),
                window: pacing_log::WindowFacts::of(ctx),
            });
        }
        self.ui.threads_debug.note_main(spent);
        self.ui.record_watch.follow(
            ctx,
            _frame,
            frame_start,
            &mut self.emulator,
            &self.ui.settings,
            &mut self.ui.frame_log,
        );
        self.ui.frame_log.note(
            web_time::Instant::now(),
            spent,
            crate::frame_log::Totals {
                audio: self.audio_totals(),
                session: self.play.session().and_then(|s| s.frame_totals()),
                ..self.emulator.frame_totals()
            },
            self.emulator.frame_budget(),
            || self.emulator.run_context(),
        );
        #[cfg(all(feature = "dev-route", target_arch = "wasm32"))]
        crate::dev_route::tock(self);
        if visible {
            self.plan_repaint(ctx, frame_start);
        }
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        canvas::fit(ctx, raw_input);
        self.raw_keys(ctx, raw_input);
        #[cfg(target_os = "android")]
        crate::platform::android::input(ctx, raw_input);
        crate::app::ui_state::settings::note_window(ctx, raw_input);
        self.big_screen_input(ctx, raw_input);
        for _ in 0..self.browser_backs() {
            for pressed in [true, false] {
                raw_input.events.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
        }
    }

    #[cfg(target_os = "android")]
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn auto_save_interval(&self) -> std::time::Duration {
        if self.needs_every_frame() && self.emulator.has_game() {
            std::time::Duration::MAX
        } else {
            AUTO_SAVE_EVERY
        }
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        #[cfg(target_os = "android")]
        if let Some(audio) = &mut self.audio {
            crate::platform::android::lifecycle::follow_visibility(audio);
        }
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save: {err:?}");
        }
        self.flush_parked_save();
        self.flush_settings(true);
        self.online.flush_chats();
        self.stats.flush();
        self.online.cloud_flush();
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn on_exit(&mut self) {
        self.close_replay_show();
        self.settle_pending_download();
        self.second.close();
        #[cfg(feature = "nds")]
        self.emulator.detach_ir();
        if let Err(err) = self.emulator.end_link() {
            log::error!("failed to save the second game on close: {err:?}");
        }
        if let Err(err) = self.emulator.autosave_if_played() {
            log::error!("failed to autosave on close: {err:?}");
        }
        #[cfg(feature = "nds")]
        self.emulator.save_jit_cache();
        if self.emulator.is_taping() {
            self.stop_tape_recording();
        }
        if let Some(replay) = self.emulator.take_cut_recording() {
            self.keep_replay(replay);
        }
        self.store_parked_replays(None);
        self.emulator.screen_recorder.stop();
        for outcome in self.emulator.screen_recorder.wait() {
            if let Outcome::Saved(done) = outcome {
                self.store_recording(*done, true);
            }
        }
        self.store_video_replays();
        let threads = std::mem::take(&mut self.threads);
        for name in threads.shutdown(EXIT_WAIT).unfinished {
            log::warn!("the {name} thread did not stop in time");
        }
        self.store_video_replays();
        if self.ui.update.take_restart() {
            crate::update::relaunch();
        }
    }
}

impl Citrine {
    fn begin_console(&mut self, ctx: &Context) {
        #[cfg(not(target_arch = "wasm32"))]
        self.follow_achievements(ctx);
        crate::profile::scope!("console");
        let Self {
            emulator,
            pads,
            ui,
            play,
            ..
        } = self;
        emulator.begin(
            ctx,
            pads.as_mut(),
            &ui.settings.bindings,
            &mut ui.capture,
            ui.controller_debug.log_mut(),
            play.session_mut(),
        );
    }

    fn show_save_news(&mut self) {
        for outcome in self.emulator.take_save_news() {
            match outcome {
                Ok(Written::Battery | Written::Cache) => {}
                Ok(Written::Autosave(_) | Written::Snapshot(_)) => self.ui.snapshots.invalidate(),
                Err(err) => {
                    self.toasts.error(err);
                    self.ui.snapshots.invalidate();
                }
            }
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn discord_screen(&mut self) -> Option<String> {
        let settings = &self.ui.settings.discord;
        if !settings.enabled
            || !settings.screenshot
            || !self.online.is_connected()
            || self.online.pictures_off()
        {
            self.shots = crate::discord::Shots::default();
            return None;
        }
        let now = std::time::Instant::now();
        if self.shots.may_take(now, settings.every) {
            self.emulator.finish_picture();
        }
        let emulator = &self.emulator;
        let solo = emulator.rom_key().map(|key| crate::discord::Unit {
            game: key.to_string(),
            running: emulator.running && emulator.has_game(),
            frame: emulator.published_frame(),
        });
        let unit = crate::discord::my_unit(self.play.session(), solo);
        let shot = self.shots.due(
            now,
            settings.every,
            unit.as_ref().map(|u| (u.game.as_str(), u.running)),
            || {
                unit.as_ref()
                    .map_or(0, |u| crate::discord::frame_key(u.frame))
            },
        );
        match (shot, &unit) {
            (crate::discord::Shot::Forget, _) => self.online.forget_screen(),
            (crate::discord::Shot::Take, Some(unit)) => {
                let only = settings.screens.only();
                match crate::discord::screen_png(|scale| {
                    emulator.screenshot_of_screen(unit.frame, scale, only)
                }) {
                    Ok(png) => {
                        self.online.send_screen(png);
                    }
                    Err(bytes) => log::warn!(
                        "the Discord screen stays unsent: its smallest picture is {bytes} bytes, over the relay's {}",
                        citrine_gb_net::MAX_SCREEN_BYTES
                    ),
                }
            }
            _ => {}
        }
        let (path, version) = self.online.screen()?;
        let address = self.online.address()?;
        Some(crate::discord::screen_url(address, path, version))
    }

    fn record_stats(&mut self) {
        let events = self.emulator.take_stat_events();
        let sent = self.online.take_sent_messages();
        let busy = events.launched
            || events.snapshots > 0
            || events.loads > 0
            || !events.played.is_zero()
            || events.together.is_some()
            || sent > 0;
        if busy {
            let now = crate::storage::now_unix();
            let day = crate::stats::day_key(crate::stats::local_date(now));
            let together = self.play.session().is_some_and(|s| s.is_live());
            let activity = crate::stats::Activity {
                linked: self.emulator.linked(),
                together,
                fast_forward: self.emulator.fast_forward,
            };
            let rom = self.emulator.rom_key().map(str::to_string);
            let name = self.emulator.game_name().to_string();
            let local_cable = self.local_cable();
            let online_cable = self.online_cable();
            let recorder = self.stats.recorder();
            if let Some(rom) = &rom {
                if events.launched {
                    recorder.launch(rom, &name, now, &day);
                }
                for _ in 0..events.snapshots {
                    recorder.snapshot(rom, &day);
                }
                for _ in 0..events.loads {
                    recorder.load(rom, &day);
                }
                if !events.played.is_zero() {
                    recorder.play(rom, &name, events.played, activity, now, &day);
                    if let Some((partner, partner_name)) = &local_cable {
                        recorder.cable(rom, partner, partner_name, events.played, now, &day);
                    }
                }
            }
            if let Some(play) = events.together {
                let activity = crate::stats::Activity {
                    together: true,
                    ..Default::default()
                };
                recorder.play(&play.rom, &play.name, play.played, activity, now, &day);
                if let Some((partner, partner_name)) = &online_cable {
                    recorder.cable(&play.rom, partner, partner_name, play.played, now, &day);
                }
            }
            for _ in 0..sent {
                recorder.chat_message(&day);
            }
        }
        let now = web_time::Instant::now();
        self.stats.tick(now);
        let online = &self.ui.settings.online;
        let sync = online.cloud_saves && online.cloud_owner().is_some();
        self.stats.sync(&mut self.online, sync, now);
    }

    fn local_cable(&self) -> Option<(crate::stats::CablePartner, String)> {
        use crate::stats::CablePartner;
        if let Some(link) = self.emulator.link().filter(|link| link.plugged()) {
            return Some((
                CablePartner::Game(link.rom_key().to_string()),
                link.title().to_string(),
            ));
        }
        let partner = self.emulator.partner().filter(|p| p.plugged())?;
        Some((
            CablePartner::Event(partner.event().key.to_string()),
            partner.name().to_string(),
        ))
    }

    fn online_cable(&self) -> Option<(crate::stats::CablePartner, String)> {
        let session = self.play.session().filter(|s| s.cable_plugged())?;
        let friend = session.friend();
        let name = self
            .online
            .friend(friend)
            .map(|f| f.display_name.clone())
            .unwrap_or_default();
        Some((crate::stats::CablePartner::Friend(friend.to_string()), name))
    }

    fn flush_settings(&mut self, force: bool) {
        if !self.ui.settings.unsaved {
            return;
        }

        let now = web_time::Instant::now();
        if !force
            && let Some(last) = self.settings_saved_at
            && now.duration_since(last) < SETTINGS_WRITE_INTERVAL
        {
            return;
        }

        match self.settings_store.store(&self.ui.settings) {
            Ok(()) => {
                self.ui.settings.unsaved = false;
                self.settings_saved_at = Some(now);
            }
            Err(err) => log::error!("Failed to save settings: {err}"),
        }
    }
}

// Events
impl Citrine {
    fn handle_event_queue(&mut self, touch: bool) {
        for event in self.events.take() {
            self.handle_event(event, touch);
        }
    }

    fn handle_event(&mut self, event: events::AppEvent, touch: bool) {
        match event {
            events::AppEvent::Notify { message, error } => {
                if error {
                    self.toasts.error(message);
                } else {
                    self.toasts.success(message);
                }
            }
            events::AppEvent::LoadRomData { data } => {
                self.handle_load_rom_data(data);
            }
            events::AppEvent::Games(action) => self.apply_games(Some(action)),
            events::AppEvent::PlayShelfGame { game } => self.play_shelf_game(game),
            events::AppEvent::PrefetchShelfGame { game } => self.shelf.prefetch(game),
            events::AppEvent::OpenTab { tab } => self.show_tab(tab, touch),
            events::AppEvent::MediaSaved { id } => self.media_saved(id, touch),
            events::AppEvent::Record(action) => self.record_bar_action(action),
            events::AppEvent::Replay(action) => self.replay_bar_action(action),
            events::AppEvent::Clip(action) => self.clip_action(action),
            events::AppEvent::MediaAdded { id } => self.media_added(id),
            events::AppEvent::ShowMedia { id } => self.show_media(id, touch),
            events::AppEvent::PlayOn { model } => self.ask_play_on(model),
            events::AppEvent::PickSecondGame => self.pick_second_game(touch),
            events::AppEvent::LinkGame { data, name } => {
                self.link_game(data, name);
            }
            events::AppEvent::EndLink => self.end_link(),
            events::AppEvent::StartPartner { event } => self.start_partner(event),
            events::AppEvent::OpenSettings { tab } => self.open_settings(tab, touch),
            events::AppEvent::FindSetting { row } => self.find_setting(row, touch),
            events::AppEvent::UndoSnapshot { rom_key, slot } => self.undo_snapshot(&rom_key, slot),
            events::AppEvent::KeepPeerSaves(notice) => self.keep_peer_saves(&notice),
            events::AppEvent::AskDiscardPeerSaves(notice) => {
                self.pending_peer_discard = Some(notice)
            }
            events::AppEvent::SetPeerSavesAside(notice) => self.set_peer_saves_aside(&notice),
            events::AppEvent::UndoKeptSaves(kept) => self.undo_kept_saves(&kept),
            events::AppEvent::PickCameraPicture => self.pick_camera_picture(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            events::AppEvent::PickCaptureFolder => {
                crate::utils::file_loader::FolderPicker::new()
                    .title("Choose where screenshots and recordings go")
                    .dispatch(&self.files.spawner, self.files.capture_folder_tx.clone());
            }
            #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
            events::AppEvent::DownloadCodec => {
                crate::media::recording::download_codec(&self.files.spawner, true);
            }
            events::AppEvent::CentreTilt => self.centre_tilt(),
            events::AppEvent::HardcoreOff => hardcore::turn_off(&mut self.ui.settings),
            events::AppEvent::SaveGameReport => self.save_game_report(),
            events::AppEvent::Screens(action) => self.screens_action(action),
            events::AppEvent::EditLayout(start) => self.start_layout_edit_with(start),
            events::AppEvent::ToggleLid => {
                self.run_hotkey(crate::app::ui_state::hotkeys::HotkeyAction::ToggleLid);
            }
            events::AppEvent::ScreenSettings { phone } => self.open_screen_settings(phone),
            events::AppEvent::FindGroup { group } => self.find_group(group, touch),
            events::AppEvent::RemoveHud { id } => self.remove_hud(id),
            events::AppEvent::RestoreHud { index, hud } => {
                self.ui.settings.hud.restore(index, hud);
                self.ui.settings.dirty = true;
            }
            events::AppEvent::PickFile(file) => self.pick_system_file(file),
            events::AppEvent::RemoveFile(file) => self.remove_system_file(file),
            #[cfg(feature = "nds")]
            events::AppEvent::FindDsBios => self.find_ds_bios(touch),
            #[cfg(feature = "nds")]
            events::AppEvent::FindDsDirect => self.find_ds_direct(touch),
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::disallowed_methods,
        reason = "the tests run apps on threads with a given stack size"
    )]

    use super::*;
    use crate::app::ui_state::hotkeys::HotkeyAction;
    use crate::app::ui_state::keybinds::PadButton;
    use crate::app::ui_state::settings::LayoutMode;
    use crate::app::ui_state::settings_tree::SettingsTab;
    use citrine_gb::rom::Rom;
    use citrine_gb::rom::header::RomCgbMode;
    use mobile::TOUCH_TARGET_PX;
    use mobile::nav::Place;

    mod chat_keys {
        use super::*;
        use crate::app::ui_state::keybinds::GbButton;
        use citrine_gb_net::{Friend, FriendState, OnlineId, Presence, PublicKey};

        fn ana() -> OnlineId {
            OnlineId::new(7).unwrap()
        }

        fn chatting_while_playing(name: &str) -> Citrine {
            let mut app = phone_app(name);
            app.ui.settings.layout = LayoutMode::Desktop;
            playing(&mut app);
            app.online.befriend_for_test(Friend {
                online_id: ana(),
                display_name: "Ana".into(),
                public_key: PublicKey([4; 32]),
                state: FriendState::Friend,
                presence: Some(Presence::Online { away: false }),
            });
            app.ui.friends.chat.open(ana());
            app.open_tab(Tab::Friends);
            app
        }

        fn frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            app.raw_keys(ctx, &mut input);
            ctx.run(input, |ctx| {
                mobile::touch_spacing(ctx, false);
                app.render(ctx);
                app.take_input(ctx);
                app.handle_event_queue(false);
            })
        }

        fn key(key: egui::Key, pressed: bool) -> egui::Event {
            egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }
        }

        fn text(text: &str) -> egui::Event {
            egui::Event::Text(text.into())
        }

        fn up_held(app: &Citrine) -> bool {
            app.emulator.is_held(GbButton::Up.key())
        }

        fn in_field(ctx: &Context) -> bool {
            ctx.wants_keyboard_input()
        }

        fn draft(app: &Citrine) -> Option<String> {
            app.ui
                .friends
                .chat
                .composing()
                .map(|(_, draft)| draft.to_owned())
        }

        fn press_tab(app: &mut Citrine, ctx: &Context) {
            frame(app, ctx, vec![key(egui::Key::Tab, true)]);
            frame(app, ctx, vec![key(egui::Key::Tab, false)]);
            frame(app, ctx, vec![]);
        }

        #[test]
        fn tab_moves_the_cursor_to_the_chat_and_its_hint_says_so() {
            let mut app = chatting_while_playing("chat-key-tab");
            let ctx = Context::default();
            frame(&mut app, &ctx, vec![]);
            let out = frame(&mut app, &ctx, vec![]);
            assert!(text_containing(&out, "Tab to chat").is_some());
            assert!(text_containing(&out, "go back to the game").is_none());
            assert!(!in_field(&ctx));
            press_tab(&mut app, &ctx);
            assert!(in_field(&ctx));
            assert_eq!(draft(&app).as_deref(), Some(""));
            let out = frame(&mut app, &ctx, vec![]);
            assert!(text_containing(&out, "to chat").is_none());
            assert!(text_containing(&out, "Tab or Esc to go back to the game").is_some());
            assert_eq!(
                app.ui.settings.hotkeys.key_name(HotkeyAction::FocusChat),
                Some("Tab".to_owned())
            );
        }

        #[test]
        fn enter_sends_and_hands_the_keys_straight_back_to_the_game() {
            let mut app = chatting_while_playing("chat-key-enter");
            let ctx = Context::default();
            frame(&mut app, &ctx, vec![]);
            press_tab(&mut app, &ctx);
            frame(&mut app, &ctx, vec![text("gg")]);
            assert_eq!(draft(&app).as_deref(), Some("gg"));
            frame(&mut app, &ctx, vec![key(egui::Key::Enter, true)]);
            assert!(!in_field(&ctx));
            assert_eq!(draft(&app).as_deref(), Some(""));
            let sent = app.online.conversation(ana()).unwrap().entries.len();
            assert_eq!(sent, 1);
            assert!(
                !app.emulator.is_held(GbButton::Start.key()),
                "the Enter that sent never reaches the game"
            );
            frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
            assert!(up_held(&app));
        }

        #[test]
        fn esc_leaves_the_field_and_keeps_the_draft_and_the_chat() {
            let mut app = chatting_while_playing("chat-key-esc");
            let ctx = Context::default();
            frame(&mut app, &ctx, vec![]);
            press_tab(&mut app, &ctx);
            frame(&mut app, &ctx, vec![text("half a")]);
            frame(&mut app, &ctx, vec![key(egui::Key::Escape, true)]);
            assert!(!in_field(&ctx));
            assert_eq!(draft(&app).as_deref(), Some("half a"));
            assert_eq!(app.ui.friends.chat.current(), Some(ana()));
            assert!(!app.ui.focus_mode);
            frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
            assert!(up_held(&app));
        }

        #[test]
        fn a_key_held_while_moving_to_the_chat_lets_go_of_the_game() {
            let mut app = chatting_while_playing("chat-key-held");
            let ctx = Context::default();
            frame(&mut app, &ctx, vec![]);
            frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
            assert!(up_held(&app));
            press_tab(&mut app, &ctx);
            assert!(in_field(&ctx));
            assert!(!up_held(&app), "Up lets go once the chat has the keys");
            frame(&mut app, &ctx, vec![key(egui::Key::W, false)]);
            frame(&mut app, &ctx, vec![key(egui::Key::Escape, true)]);
            assert!(!in_field(&ctx));
            frame(&mut app, &ctx, vec![]);
            assert!(!up_held(&app));
            let repeat = egui::Event::Key {
                key: egui::Key::W,
                physical_key: None,
                pressed: true,
                repeat: true,
                modifiers: Default::default(),
            };
            frame(&mut app, &ctx, vec![repeat]);
            assert!(!up_held(&app), "a key repeat is not a new press");
        }

        #[test]
        fn clicking_the_game_leaves_the_field() {
            let mut app = chatting_while_playing("chat-key-click");
            let ctx = Context::default();
            frame(&mut app, &ctx, vec![]);
            press_tab(&mut app, &ctx);
            frame(&mut app, &ctx, vec![text("brb")]);
            frame(&mut app, &ctx, tap(egui::pos2(1000.0, 400.0)));
            frame(&mut app, &ctx, vec![]);
            assert!(!in_field(&ctx));
            assert_eq!(draft(&app).as_deref(), Some("brb"));
        }

        #[test]
        fn tab_again_goes_back_to_the_game_with_the_draft_kept() {
            let mut app = chatting_while_playing("chat-key-tab-out");
            let ctx = Context::default();
            frame(&mut app, &ctx, vec![]);
            press_tab(&mut app, &ctx);
            assert!(in_field(&ctx));
            frame(&mut app, &ctx, vec![text("one sec")]);
            press_tab(&mut app, &ctx);
            assert!(!in_field(&ctx));
            assert_eq!(draft(&app).as_deref(), Some("one sec"));
            frame(&mut app, &ctx, vec![key(egui::Key::W, true)]);
            assert!(up_held(&app));
            press_tab(&mut app, &ctx);
            assert!(in_field(&ctx));
            assert_eq!(draft(&app).as_deref(), Some("one sec"));
        }
    }
    #[cfg(feature = "nds")]
    mod ds_session {
        use super::*;
        use crate::app::tabs::playing::Ended;
        use crate::play::tests::{Pair, homebrew};
        use citrine_gb::console::ConsoleKind;

        fn ended_frame(app: &mut Citrine, ctx: &Context) -> egui::FullOutput {
            let mut output = None;
            for _ in 0..3 {
                output = Some(match app.ui.settings.layout {
                    LayoutMode::Mobile => play_frame(app, ctx, Vec::new()),
                    _ => desk_frame(app, ctx, Vec::new()),
                });
            }
            output.unwrap()
        }

        #[test]
        fn a_ds_game_offers_play_again_once_playing_together_ends() {
            let mut pair = Pair::new(false, 0, 0, homebrew());
            let friend = pair.guest.id();
            for layout in [LayoutMode::Mobile, LayoutMode::Desktop] {
                let mut app = phone_app(&format!("ds-again-{layout:?}"));
                app.ui.settings.layout = layout;
                app.emulator
                    .load_nds(include_bytes!(
                        "../fixtures/nds/blocksds-console-both-screens.nds"
                    ))
                    .unwrap();
                app.mobile.nav.show_game();
                std::mem::swap(&mut *app.play, &mut pair.host.play);
                std::mem::swap(&mut app.online, &mut pair.host.online);
                app.ui.playing.ended = Some(Ended {
                    friend,
                    line: "Mia stopped playing".into(),
                    paused: false,
                });
                let output = ended_frame(&mut app, &Context::default());
                std::mem::swap(&mut *app.play, &mut pair.host.play);
                std::mem::swap(&mut app.online, &mut pair.host.online);
                assert!(
                    text_rect(&output, "Mia stopped playing").is_some(),
                    "{layout:?}"
                );
                assert!(text_rect(&output, "Play again").is_some(), "{layout:?}");
            }
        }

        fn gba_solo(name: &str, layout: LayoutMode, solo: bool) -> Citrine {
            use crate::app::hud::{Hud, Value};
            let mut app = phone_app(name);
            app.ui.settings.layout = layout;
            app.ui.settings.check_updates = false;
            if solo {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("fixtures/gba/save-sram.gba");
                app.handle_load_rom_data(std::fs::read(path).expect("the GBA fixture"));
                app.emulator.running = true;
                assert_eq!(app.emulator.kind(), ConsoleKind::Gba);
            }
            app.ui.settings.hud.shown = true;
            app.ui.settings.hud.huds = vec![Hud {
                values: vec![Value::Fps, Value::Jit],
                ..Hud::default()
            }];
            app
        }

        fn full_frames(
            app: &mut Citrine,
            ctx: &Context,
            size: egui::Vec2,
            partner: &mut dyn FnMut(),
        ) -> Vec<String> {
            let mut frame = eframe::Frame::_new_kittest();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let mut output = None;
            for _ in 0..6 {
                partner();
                output = Some(ctx.run(input.clone(), |ctx| {
                    eframe::App::update(app, ctx, &mut frame);
                }));
            }
            output
                .expect("a frame")
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().trim().to_owned()),
                    _ => None,
                })
                .collect()
        }

        fn walk(
            app: &mut Citrine,
            layout: LayoutMode,
            partner: &mut dyn FnMut(),
        ) -> Vec<(&'static str, Vec<String>)> {
            let ctx = Context::default();
            let mut pages = Vec::new();
            if layout == LayoutMode::Desktop {
                let size = egui::vec2(1280.0, 800.0);
                pages.push(("game", full_frames(app, &ctx, size, partner)));
                let card = app.game_card().map_or_else(Vec::new, |card| {
                    let icon = card.icon.map(|_| "an icon".to_owned());
                    [card.system, card.title].into_iter().chain(icon).collect()
                });
                pages.push(("second screen", card));
                app.open_tab(Tab::ControllerDebug);
                pages.push(("controller debug", full_frames(app, &ctx, size, partner)));
            } else {
                let size = egui::vec2(412.0, 915.0);
                app.mobile.nav.show_game();
                pages.push(("game", full_frames(app, &ctx, size, partner)));
                let id = app.ui.settings.hud.huds[0].id;
                app.mobile.hud_sheet = Some(crate::app::mobile::hud::HudSheet::Edit(id));
                pages.push(("HUD sheet", full_frames(app, &ctx, size, partner)));
                app.mobile.hud_sheet = None;
                app.mobile.open(Tab::ControllerDebug);
                pages.push(("controller debug", full_frames(app, &ctx, size, partner)));
            }
            pages
        }

        #[test]
        fn a_ds_session_after_a_gba_game_shows_the_ds_everywhere() {
            use crate::play::MyGame;
            use crate::play::tests::{empty, nickname, one_ds_pair_at_a_time};
            let _one = one_ds_pair_at_a_time();
            let wants = [
                ("game", "JIT"),
                ("second screen", "Nintendo DS"),
                ("second screen", "Nickname"),
                ("second screen", "an icon"),
                ("HUD sheet", "JIT"),
                ("controller debug", "Nintendo DS buttons held"),
            ];
            let mut wrong = Vec::new();
            for (host, solo) in [(true, true), (false, true), (false, false)] {
                for layout in [LayoutMode::Desktop, LayoutMode::Mobile] {
                    let who = match (host, solo) {
                        (true, _) => "host",
                        (false, true) => "guest",
                        (false, false) => "guest with no game",
                    };
                    let seat = format!("{who} {layout:?}");
                    let mut app = gba_solo(&format!("ds-after-gba-{seat}"), layout, solo);
                    let mut pair = Pair::new(true, 0, 0, empty());
                    let session = pair.invite(nickname());
                    pair.accept(session, MyGame::Own(nickname()));
                    pair.play_frames(60);
                    pair.hand_over_at_real_time();
                    pair.swap_seat(host, &mut app.play, &mut app.online);
                    let pages = walk(&mut app, layout, &mut || pair.run_partner_alone(host));
                    let shown = app.emulator.played_kind();
                    let together = app.emulator.shows_session();
                    pair.swap_seat(host, &mut app.play, &mut app.online);
                    if !together {
                        wrong.push(format!("{seat}: the game together ended"));
                    }
                    if shown != ConsoleKind::Nds {
                        wrong.push(format!("{seat}: the console on screen is {shown:?}"));
                    }
                    for (page, texts) in pages {
                        if let Some(gba) = texts.iter().find(|t| t.contains("Game Boy Advance")) {
                            wrong.push(format!("{seat} {page}: shows {gba:?}"));
                        }
                        for (_, want) in wants.iter().filter(|(p, _)| *p == page) {
                            if !texts.iter().any(|t| t.starts_with(want)) {
                                wrong.push(format!("{seat} {page}: no {want:?}"));
                            }
                        }
                    }
                }
            }
            assert!(wrong.is_empty(), "{wrong:#?}");
        }
    }
    #[cfg(feature = "gba")]
    mod gba_panels {
        use super::*;

        const SPIN: [u8; 4] = 0xEAFF_FFFE_u32.to_le_bytes();

        fn gba_rom() -> Vec<u8> {
            let mut rom = vec![0u8; 0x400];
            rom[..4].copy_from_slice(&SPIN);
            rom
        }

        fn both_consoles_rom() -> Vec<u8> {
            let mut rom = vec![0u8; 0x8000];
            rom[0x0143] = 0x80;
            rom
        }

        fn app_on(name: &str, layout: LayoutMode, gba: bool) -> Citrine {
            let mut app = phone_app(name);
            app.ui.settings.layout = layout;
            app.ui.settings.dev_mode = true;
            if gba {
                app.emulator.put_gba(&gba_rom(), &format!("gba-{name}"));
            } else {
                load_game(&mut app, &both_consoles_rom(), None);
                app.emulator.running = true;
            }
            app
        }

        fn frames(app: &mut Citrine, ctx: &Context, width: f32) -> egui::FullOutput {
            info_frame_out(app, ctx, width, 900.0);
            info_frame_out(app, ctx, width, 900.0)
        }

        fn shows(output: &egui::FullOutput, text: &str) -> bool {
            text_rect(output, text).is_some() || text_containing(output, text).is_some()
        }

        fn desktop_tab(name: &str, gba: bool, tab: Tab) -> egui::FullOutput {
            let mut app = app_on(name, LayoutMode::Desktop, gba);
            app.open_tab(tab);
            frames(&mut app, &Context::default(), 1280.0)
        }

        #[test]
        fn the_palettes_stay_in_settings_with_a_gba() {
            for gba in [false, true] {
                let mut app = app_on(&format!("palettes-{gba}"), LayoutMode::Desktop, gba);
                app.ui.settings.current_tab = SettingsTab::GameBoy;
                app.open_tab(Tab::Settings);
                let output = frames(&mut app, &Context::default(), 1280.0);
                assert!(
                    text_rect(&output, "Palette").is_some(),
                    "desktop, gba {gba}"
                );
                app.ui.settings.current_tab = SettingsTab::Screen;
                let output = frames(&mut app, &Context::default(), 1280.0);
                assert!(shows(&output, "Scaling"), "desktop, gba {gba}");

                let mut phone = app_on(&format!("phone-palettes-{gba}"), LayoutMode::Mobile, gba);
                phone.mobile.nav.open_settings(SettingsTab::GameBoy);
                let output = frames(&mut phone, &Context::default(), 412.0);
                assert!(shows(&output, "Own palettes"), "phone, gba {gba}");
                phone.mobile.nav.open_settings(SettingsTab::Screen);
                let output = frames(&mut phone, &Context::default(), 412.0);
                assert!(shows(&output, "Scaling"), "phone, gba {gba}");
            }
        }

        #[test]
        fn a_gba_hides_the_json_dump_in_debug_actions() {
            for gba in [false, true] {
                let output = desktop_tab(&format!("dump-{gba}"), gba, Tab::DebugActions);
                assert_eq!(shows(&output, "Export as JSON"), !gba, "gba {gba}");
                assert!(shows(&output, "Input recording"), "gba {gba}");
            }
        }

        #[test]
        fn a_gba_offers_no_other_console() {
            for gba in [false, true] {
                let mut phone = app_on(&format!("play-on-{gba}"), LayoutMode::Mobile, gba);
                phone.mobile.open(Tab::ThisGame);
                let output = frames(&mut phone, &Context::default(), 412.0);
                let play_on = crate::app::widgets::settings::this_game::PLAY_ON;
                assert_eq!(
                    text_rect(&output, play_on).is_some(),
                    !gba,
                    "this game, gba {gba}"
                );
            }
        }

        #[test]
        fn a_gba_runs_through_every_dev_tool_and_console_panel() {
            let tools = crate::app::widgets::dev_menu::DEV_SECTIONS
                .iter()
                .flat_map(|section| section.tools)
                .map(|(_, tab)| *tab);
            let panels = [
                Tab::GameBoy,
                Tab::RomInfo,
                Tab::Settings,
                Tab::GameInfo,
                Tab::Saves,
            ];
            for tab in tools.chain(panels) {
                for width in [412.0, 1280.0] {
                    let layout = if width < 500.0 {
                        LayoutMode::Mobile
                    } else {
                        LayoutMode::Desktop
                    };
                    let mut app = app_on(&format!("tool-{tab:?}-{width}"), layout, true);
                    app.open_tab(tab);
                    app.mobile.open(tab);
                    let ctx = Context::default();
                    for _ in 0..3 {
                        let _ = play_frame(&mut app, &ctx, Vec::new());
                        info_frame_out(&mut app, &ctx, width, 900.0);
                    }
                    assert!(app.emulator.as_gb().is_none(), "{tab:?}");
                }
            }
        }

        #[test]
        fn a_gba_autosave_row_keeps_delete_inside_the_dock() {
            let mut app = app_on("autosave-delete", LayoutMode::Desktop, true);
            app.emulator.autosave().unwrap();
            app.emulator.save_snapshot(0).unwrap();
            app.open_tab(Tab::Saves);
            let web_1280_css = 1280.0 / 2.2;
            let output = frames(&mut app, &Context::default(), web_1280_css);
            let deletes: Vec<(egui::Rect, egui::Rect)> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().contains("Delete") => {
                        Some((text.visual_bounding_rect(), clipped.clip_rect))
                    }
                    _ => None,
                })
                .collect();
            assert!(!deletes.is_empty(), "no Delete button in the Saves tab");
            for (text, clip) in deletes {
                assert!(
                    text.max.x <= clip.max.x,
                    "Delete ends at {} past the dock edge {}",
                    text.max.x,
                    clip.max.x
                );
            }
        }
    }
    mod hud {
        use super::*;
        use crate::app::mobile::hud::HudSheet;

        fn focus_frame(
            app: &mut Citrine,
            ctx: &Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                mobile::touch_spacing(ctx, false);
                app.render(ctx);
                app.take_input(ctx);
            })
        }

        fn focus_with_hud(name: &str) -> (Citrine, u32) {
            let mut app = phone_app(name);
            app.ui.settings.layout = LayoutMode::Desktop;
            playing(&mut app);
            app.ui.focus_mode = true;
            let id = app.ui.settings.hud.add();
            (app, id)
        }

        fn pointer(at: egui::Pos2, pressed: Option<bool>) -> Vec<egui::Event> {
            let mut events = vec![egui::Event::PointerMoved(at)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
            events
        }

        #[test]
        fn esc_closes_the_hud_editor_and_stays_in_focus_mode() {
            let ctx = Context::default();
            let (mut app, id) = focus_with_hud("hud-esc");
            app.ui.hud.editor = Some(id);
            focus_frame(&mut app, &ctx, vec![]);
            let output = focus_frame(&mut app, &ctx, vec![]);
            assert!(
                text_containing(&output, "Add a value").is_some(),
                "the editor shows"
            );
            focus_frame(&mut app, &ctx, escape());
            assert_eq!(app.ui.hud.editor, None);
            assert!(app.ui.focus_mode, "Esc only closed the editor");
            let mut release = escape();
            if let egui::Event::Key { pressed, .. } = &mut release[0] {
                *pressed = false;
            }
            focus_frame(&mut app, &ctx, release);
            focus_frame(&mut app, &ctx, escape());
            assert!(!app.ui.focus_mode, "the next Esc leaves focus mode");
        }

        #[test]
        fn a_dragged_hud_moves_and_is_saved() {
            let ctx = Context::default();
            let (mut app, id) = focus_with_hud("hud-drag");
            let output = focus_frame(&mut app, &ctx, vec![]);
            let label = text_containing(&output, "FPS").expect("the HUD shows its values");
            let start = label.center();
            let end = start + egui::vec2(200.0, 150.0);
            app.ui.settings.dirty = false;
            focus_frame(&mut app, &ctx, pointer(start, Some(true)));
            for step in 1..=10 {
                let at = start + (end - start) * (step as f32 / 10.0);
                focus_frame(&mut app, &ctx, pointer(at, None));
            }
            focus_frame(&mut app, &ctx, pointer(end, Some(false)));
            focus_frame(&mut app, &ctx, vec![]);
            let hud = app.ui.settings.hud.get(id).expect("still there");
            let moved = egui::vec2(hud.offset[0], hud.offset[1]) - egui::vec2(12.0, 12.0);
            assert!((moved.x - 200.0).abs() <= 8.0, "{moved:?}");
            assert!((moved.y - 150.0).abs() <= 8.0, "{moved:?}");
            assert!(app.ui.settings.dirty, "the new place is saved");
        }

        #[test]
        fn shift_f3_hides_and_shows_the_huds() {
            let (mut app, _) = focus_with_hud("hud-key");
            assert!(app.ui.settings.hud.shown);
            app.run_hotkey(HotkeyAction::ToggleHuds);
            assert!(!app.ui.settings.hud.shown);
            assert!(app.ui.settings.dirty);
            app.run_hotkey(HotkeyAction::ToggleHuds);
            assert!(app.ui.settings.hud.shown);
        }

        #[test]
        fn removing_a_hud_offers_undo_that_puts_it_back() {
            let (mut app, id) = focus_with_hud("hud-undo");
            let other = app.ui.settings.hud.add();
            app.remove_hud(id);
            assert!(app.ui.settings.hud.get(id).is_none());
            assert!(app.toasts.button_labels().contains(&"Undo"));
            app.ui.settings.hud.restore(
                0,
                crate::app::hud::Hud {
                    id,
                    ..Default::default()
                },
            );
            let order: Vec<u32> = app.ui.settings.hud.huds.iter().map(|hud| hud.id).collect();
            assert_eq!(order, [id, other]);
        }

        #[test]
        fn back_on_a_phone_closes_the_hud_editor_sheet_and_stays_in_the_game() {
            let ctx = Context::default();
            let mut app = phone_app("hud-back");
            playing(&mut app);
            app.mobile.nav.show_game();
            let id = app.ui.settings.hud.add();
            app.mobile.hud_sheet = Some(HudSheet::Size(id));
            let press = |app: &mut Citrine, events: Vec<egui::Event>| {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(412.0, 915.0),
                    )),
                    events,
                    ..Default::default()
                };
                ctx.run(input, |ctx| {
                    mobile::touch_spacing(ctx, true);
                    app.render(ctx);
                    app.mobile_escape(ctx);
                })
            };
            press(&mut app, vec![]);
            press(&mut app, escape());
            assert_eq!(
                app.mobile.hud_sheet,
                Some(HudSheet::Edit(id)),
                "Back steps out of Size first"
            );
            press(&mut app, vec![]);
            let output = press(&mut app, vec![]);
            assert!(text_containing(&output, "Add a value").is_some());
            press(&mut app, escape());
            assert_eq!(app.mobile.hud_sheet, None);
            assert!(app.mobile.nav.on_game());
        }

        #[test]
        fn the_more_sheet_hud_tile_opens_the_hud_sheet() {
            let ctx = Context::default();
            let mut app = phone_app("hud-tile");
            playing(&mut app);
            app.mobile.nav.show_game();
            app.mobile.nav.set_more(true);
            play_frame(&mut app, &ctx, vec![]);
            let output = play_frame(&mut app, &ctx, vec![]);
            let tile = text_containing(&output, "HUD").expect("a HUD tile");
            play_frame(&mut app, &ctx, tap(tile.center()));
            assert_eq!(app.mobile.hud_sheet, Some(HudSheet::List));
            play_frame(&mut app, &ctx, vec![]);
            let output = play_frame(&mut app, &ctx, vec![]);
            assert!(text_containing(&output, "New HUD").is_some());
        }
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[test]
    fn the_window_and_layout_are_kept_every_30_seconds_but_not_while_a_game_runs() {
        use std::time::Duration;
        let mut app = test_app();
        let every = Duration::from_secs(30);
        assert_eq!(eframe::App::auto_save_interval(&app), every);
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        app.emulator
            .load_rom(&Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app.emulator.running = true;
        assert_eq!(eframe::App::auto_save_interval(&app), Duration::MAX);
        app.emulator.running = false;
        assert_eq!(eframe::App::auto_save_interval(&app), every);
    }

    mod idle_repaint {
        use super::*;
        use std::time::Duration;

        const REFRESH_HZ: f64 = 180.0;

        fn desktop_input() -> egui::RawInput {
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                predicted_dt: (1.0 / REFRESH_HZ) as f32,
                ..Default::default()
            }
        }

        fn repaint_causes(app: &mut Citrine, ctx: &Context, frame: &mut eframe::Frame) -> String {
            let mut causes = std::collections::BTreeSet::new();
            for _ in 0..3 {
                let _ = ctx.run(desktop_input(), |ctx| eframe::App::update(app, ctx, frame));
                causes.extend(ctx.repaint_causes().iter().map(ToString::to_string));
            }
            causes.into_iter().collect::<Vec<_>>().join(", ")
        }

        fn updates_in_a_second(app: &mut Citrine, ctx: &Context, frame: &mut eframe::Frame) -> u32 {
            let input = desktop_input();
            let refresh = Duration::from_secs_f64(1.0 / REFRESH_HZ);
            let mut at = Duration::ZERO;
            let mut updates = 0;
            while at < Duration::from_secs(1) {
                let output = ctx.run(input.clone(), |ctx| eframe::App::update(app, ctx, frame));
                let delay = output
                    .viewport_output
                    .values()
                    .map(|viewport| viewport.repaint_delay)
                    .min()
                    .unwrap_or(Duration::MAX);
                at += delay.max(refresh);
                updates += 1;
            }
            updates
        }

        #[test]
        fn a_paused_game_redraws_a_few_times_a_second_and_resume_goes_back_to_every_refresh() {
            let mut app = crystal_app("idle-repaint", LayoutMode::Desktop, None);
            app.ui.settings.check_updates = false;
            let ctx = Context::default();
            #[cfg(feature = "profile")]
            crate::profile::set_open(&ctx, false);
            let mut frame = eframe::Frame::_new_kittest();
            app.emulator.running = true;
            let _ = updates_in_a_second(&mut app, &ctx, &mut frame);
            app.toasts.dismiss_all_toasts();

            app.emulator.running = false;
            let _ = updates_in_a_second(&mut app, &ctx, &mut frame);
            let paused = updates_in_a_second(&mut app, &ctx, &mut frame);
            assert!(paused <= 24, "{paused} updates a second while paused");

            app.emulator.running = true;
            let running = updates_in_a_second(&mut app, &ctx, &mut frame);
            assert!(running >= 175, "{running} updates a second after resume");
        }

        fn chat_app(name: &str, messages: u64) -> Citrine {
            use crate::online::chat::{Change, ChatEntry, Delivery};
            use citrine_gb_net::{Body, Friend, FriendState, OnlineId, Presence, PublicKey};
            let ana = OnlineId::new(7).unwrap();
            let mut app = phone_app(name);
            app.ui.settings.layout = LayoutMode::Desktop;
            app.ui.settings.check_updates = false;
            remember_account(&mut app);
            app.online.befriend_for_test(Friend {
                online_id: ana,
                display_name: "Ana".into(),
                public_key: PublicKey([4; 32]),
                state: FriendState::Friend,
                presence: Some(Presence::Online { away: false }),
            });
            let words = [
                "see you at the lemon tree",
                "did you beat the second castle yet? I keep falling into the lava right before the boss door",
                "😋",
                "ok",
                "my brother says the secret room behind the waterfall has an extra life but nobody ever found the switch for it, so I spent the whole evening jumping at walls",
                "trade tomorrow? 🍋🍋",
            ];
            let entries = (0..messages)
                .map(|id| ChatEntry {
                    id: id + 1,
                    mine: id % 3 == 0,
                    at: 1_761_800_000 + id * 600,
                    body: Body::Text(words[id as usize % words.len()].into()),
                    delivery: if id % 3 == 0 {
                        Delivery::Read(1_761_900_000)
                    } else {
                        Delivery::Received
                    },
                    change: Change::Kept,
                    reply: None,
                })
                .collect();
            app.online.add_chat_for_test(ana, entries);
            app.ui.friends.chat.open(ana);
            app.open_tab(Tab::Friends);
            app
        }

        #[test]
        fn an_idle_open_chat_sleeps_like_the_rest_of_the_idle_app() {
            let mut app = chat_app("idle-chat", 2000);
            let ctx = Context::default();
            #[cfg(feature = "profile")]
            crate::profile::set_open(&ctx, false);
            let mut frame = eframe::Frame::_new_kittest();
            let _ = updates_in_a_second(&mut app, &ctx, &mut frame);
            app.toasts.dismiss_all_toasts();
            let _ = updates_in_a_second(&mut app, &ctx, &mut frame);
            let idle = updates_in_a_second(&mut app, &ctx, &mut frame);
            let causes = repaint_causes(&mut app, &ctx, &mut frame);
            assert!(
                idle <= 24,
                "{idle} updates a second with the chat open, asked by {causes}"
            );
        }

        #[test]
        fn a_long_open_chat_draws_only_the_messages_in_view() {
            let mut app = chat_app("long-chat", 2000);
            app.ui.friends.chat.show_older_for_test(9);
            let ctx = Context::default();
            #[cfg(feature = "profile")]
            crate::profile::set_open(&ctx, false);
            let mut frame = eframe::Frame::_new_kittest();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            fn lemon_trees(shape: &egui::Shape) -> usize {
                match shape {
                    egui::Shape::Text(text) => text.galley.text().contains("lemon tree") as usize,
                    egui::Shape::Vec(shapes) => shapes.iter().map(lemon_trees).sum(),
                    _ => 0,
                }
            }
            let mut texts = 0;
            for _ in 0..4 {
                let output = ctx.run(input.clone(), |ctx| {
                    eframe::App::update(&mut app, ctx, &mut frame)
                });
                texts = output
                    .shapes
                    .iter()
                    .map(|clipped| lemon_trees(&clipped.shape))
                    .sum();
            }
            assert!((1..40).contains(&texts), "{texts} messages drawn");
        }

        #[test]
        #[ignore = "frame cost bench, run by hand"]
        fn open_chat_frame_cost() {
            let frames: u32 = std::env::var("CHAT_COST_FRAMES")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(200);
            let mut app = chat_app("chat-cost", 2000);
            if std::env::var_os("CHAT_COST_ALL").is_some() {
                app.ui.friends.chat.show_older_for_test(9);
            }
            let ctx = Context::default();
            #[cfg(feature = "profile")]
            crate::profile::set_open(&ctx, false);
            let mut frame = eframe::Frame::_new_kittest();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            for _ in 0..10 {
                let _ = ctx.run(input.clone(), |ctx| {
                    eframe::App::update(&mut app, ctx, &mut frame)
                });
            }
            app.toasts.dismiss_all_toasts();
            let start = std::time::Instant::now();
            for _ in 0..frames {
                let _ = ctx.run(input.clone(), |ctx| {
                    eframe::App::update(&mut app, ctx, &mut frame)
                });
            }
            let per_frame = start.elapsed() / frames;
            let open = updates_in_a_second(&mut app, &ctx, &mut frame);
            app.ui.friends.chat.close();
            let _ = updates_in_a_second(&mut app, &ctx, &mut frame);
            let closed = updates_in_a_second(&mut app, &ctx, &mut frame);
            let causes = repaint_causes(&mut app, &ctx, &mut frame);
            println!("CHAT UPDATES {open} a second open, {closed} closed");
            println!("CHAT CAUSES closed {causes}");
            println!(
                "CHAT FRAME {frames} frames, {} us a frame",
                per_frame.as_micros()
            );
        }
    }
    mod saves_on_open {
        use super::*;
        use citrine_gb::console::{Console as _, ConsoleKind};

        fn pattern(len: usize, seed: u8) -> Vec<u8> {
            (0..len)
                .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
                .collect()
        }

        fn gba_rom() -> Vec<u8> {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/gba/save-flash128.gba");
            std::fs::read(path).expect("fixture")
        }

        fn app_with(name: &str, cloud: bool) -> (Citrine, std::path::PathBuf) {
            let mut app = phone_app(name);
            if cloud {
                remember_account(&mut app);
                app.ui.settings.online.cloud_saves = true;
            }
            let root = crate::storage::data_dir().join("phone-saves").join(name);
            (app, root)
        }

        fn lay_out_v012(root: &std::path::Path, key: &str, save: &[u8]) {
            let dir = root.join(key);
            std::fs::create_dir_all(&dir).expect("game folder");
            std::fs::write(dir.join("battery.sav"), save).expect("battery.sav");
            std::fs::write(dir.join("battery.meta"), 1_700_000_000u64.to_le_bytes())
                .expect("battery.meta");
        }

        fn open(app: &mut Citrine, rom: &[u8], kind: ConsoleKind, path: Option<&std::path::Path>) {
            app.emulator
                .load_game(&citrine_gb::rom::Rom::new(rom), kind, path)
                .expect("load");
            let ctx = Context::default();
            for _ in 0..5 {
                play_frame(app, &ctx, vec![]);
            }
            if let Some(hold) = &mut app.cloud_hold {
                hold.since -= std::time::Duration::from_secs(10);
            }
            for _ in 0..5 {
                play_frame(app, &ctx, vec![]);
            }
            assert!(app.cloud_hold.is_none(), "the cloud hold ends");
        }

        fn ram(app: &mut Citrine) -> Vec<u8> {
            app.emulator
                .debug_mut()
                .battery(true, 0)
                .expect("a battery")
                .ram
        }

        #[test]
        fn a_save_from_v012_loads_when_its_game_opens() {
            let games = [
                (
                    "gb",
                    citrine_gb_gamedata::fixtures::crystal_cartridge(),
                    ConsoleKind::Gbc,
                    0x8000,
                ),
                ("gba", gba_rom(), ConsoleKind::Gba, 0x20000),
            ];
            for (console, rom, kind, size) in games {
                for cloud in [false, true] {
                    let case = format!("v012-{console}-cloud-{cloud}");
                    let (mut app, root) = app_with(&case, cloud);
                    let key = citrine_gb::rom::id::RomId::new(&rom).sha256_hex_string();
                    let save = pattern(size, 7);
                    lay_out_v012(&root, &key, &save);
                    open(&mut app, &rom, kind, None);
                    assert_eq!(app.emulator.rom_key(), Some(key.as_str()), "{case}");
                    assert!(app.emulator.save_loaded, "{case}: the save loaded");
                    assert!(ram(&mut app) == save, "{case}: the game has the save");
                    assert_eq!(
                        std::fs::read(root.join(&key).join("battery.sav")).ok(),
                        Some(save),
                        "{case}: the file is untouched"
                    );
                    let _ = std::fs::remove_dir_all(&root);
                }
            }
        }

        #[test]
        fn a_sav_beside_the_rom_loads_when_its_game_opens() {
            for cloud in [false, true] {
                let case = format!("beside-rom-cloud-{cloud}");
                let (mut app, root) = app_with(&case, cloud);
                let games = root.join("games");
                std::fs::create_dir_all(&games).expect("games folder");
                let rom = citrine_gb_gamedata::fixtures::crystal_cartridge();
                let save = pattern(0x8000, 3);
                std::fs::write(games.join("crystal.gbc"), &rom).expect("rom");
                std::fs::write(games.join("crystal.sav"), &save).expect("sav");
                open(
                    &mut app,
                    &rom,
                    ConsoleKind::Gbc,
                    Some(&games.join("crystal.gbc")),
                );
                assert!(app.emulator.save_loaded, "{case}: the save loaded");
                assert!(ram(&mut app) == save, "{case}: the game has the save");
                assert!(games.join("crystal.sav").exists(), "{case}: the .sav stays");
                let _ = std::fs::remove_dir_all(&root);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    mod saves_on_the_network {
        use super::*;

        const ROM: &str = "2222222222222222222222222222222222222222222222222222222222222222";

        fn device(name: &str) -> Citrine {
            let mut app = phone_app(name);
            app.online.cloud_attach(app.emulator.store.clone());
            app.ui.settings.online.keep_saves = crate::online::KeepSaves::All;
            remember_account(&mut app);
            app.ui.settings.online.cloud_saves = false;
            app
        }

        fn update(app: &mut Citrine, ctx: &Context, frame: &mut eframe::Frame) {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 915.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| eframe::App::update(app, ctx, frame));
        }

        fn trusted(app: &Citrine) -> usize {
            app.lan
                .lan
                .as_ref()
                .map_or(0, |lan| lan.trusted_peers().count())
        }

        #[test]
        fn on_a_phone_a_device_row_is_one_line_under_a_head_on_the_rows_edges() {
            let mut me = crate::lan::tests::lan("Pixel", 1);
            let mut deck = crate::lan::tests::lan("Steam Deck", 2);
            let mut laptop = crate::lan::tests::lan("Clara's laptop", 3);
            crate::lan::tests::connect(&mut deck, &mut me, Some([9; 32]));
            crate::lan::tests::connect(&mut laptop, &mut me, None);
            for _ in 0..6 {
                me.step(0.0);
                deck.step(0.0);
                laptop.step(0.0);
            }
            me.remember(laptop.key());
            let mut app = device("network-rows");
            app.ui.settings.online.network_saves = true;
            app.lan.lan = Some(me);
            app.open_settings(SettingsTab::Sync, true);
            let ctx = Context::default();
            app.follow_lan();
            let mut output = play_frame(&mut app, &ctx, Vec::new());
            for _ in 0..5 {
                app.follow_lan();
                output = play_frame(&mut app, &ctx, Vec::new());
            }
            assert_eq!(app.online.network_devices().len(), 2);
            let texts: Vec<(String, egui::Rect)> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                    )),
                    _ => None,
                })
                .collect();
            let line = |wanted: &str| {
                texts
                    .iter()
                    .find(|(text, _)| text == wanted)
                    .map(|(_, rect)| *rect)
                    .unwrap_or_else(|| panic!("no {wanted} in {texts:?}"))
            };
            for (name, on) in [("Steam Deck", true), ("Clara's laptop", false)] {
                let row = line(name);
                let status = texts.iter().find(|(text, rect)| {
                    ["Syncing", "Not syncing"].contains(&text.as_str())
                        && (rect.center().y - row.center().y).abs() < 2.0
                });
                assert_eq!(
                    status.is_some(),
                    on,
                    "{name}: a status only beside an on switch, in {texts:?}"
                );
            }
            let devices = line("Devices");
            let head = line("Sync saves");
            let name = line("Steam Deck");
            assert!(
                (devices.left() - name.left()).abs() < 0.5,
                "{devices:?} {name:?}"
            );
            let switch = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect)
                        if (rect.rect.width() - 44.0).abs() < 0.5
                            && (rect.rect.height() - 26.0).abs() < 0.5 =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .find(|rect| rect.top() > name.top() - 20.0)
                .expect("a switch");
            assert!(
                (head.right() - switch.right()).abs() < 0.5,
                "{head:?} {switch:?}"
            );
        }

        #[test]
        fn signed_out_sync_settings_sits_under_this_network_and_signed_in_under_cloud() {
            for joined in [false, true] {
                let name = format!("sync-settings-place-{joined}");
                let mut app = phone_app(&name);
                if joined {
                    remember_account(&mut app);
                }
                app.ui.settings.online.network_saves = true;
                app.open_settings(SettingsTab::Sync, true);
                let ctx = Context::default();
                let mut output = play_frame(&mut app, &ctx, Vec::new());
                for _ in 0..3 {
                    output = play_frame(&mut app, &ctx, Vec::new());
                }
                let top = |wanted: &str| {
                    output
                        .shapes
                        .iter()
                        .find_map(|clipped| match &clipped.shape {
                            egui::Shape::Text(text) if text.galley.text() == wanted => {
                                Some(text.pos.y)
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("{name}: no {wanted}"))
                };
                let sync = top("Sync settings");
                let cloud = top("Cloud saves");
                let network = top("This network");
                let saves = top("Saves on this network");
                if joined {
                    assert!(cloud < sync && sync < network, "{name}: under Cloud");
                } else {
                    assert!(
                        network < saves && saves < sync,
                        "{name}: under This network"
                    );
                }
            }
        }

        #[test]
        fn two_apps_on_one_network_share_a_battery_save_and_a_synced_setting_with_no_server() {
            let mut deck_lan = crate::lan::tests::lan("Steam Deck", 1);
            let mut pixel_lan = crate::lan::tests::lan("Pixel", 2);
            crate::lan::tests::connect(&mut pixel_lan, &mut deck_lan, Some([9; 32]));
            for _ in 0..6 {
                deck_lan.step(0.0);
                pixel_lan.step(0.0);
            }
            assert_eq!(deck_lan.trusted_peers().count(), 1);
            assert_eq!(pixel_lan.trusted_peers().count(), 1);
            let mut deck = device("network-deck");
            let mut pixel = device("network-pixel");
            deck.lan.lan = Some(deck_lan);
            pixel.lan.lan = Some(pixel_lan);
            deck.emulator
                .store
                .store_battery(ROM, b"battery from the deck")
                .unwrap();
            let ghosting = !pixel.ui.settings.ghosting;
            let volume = pixel.ui.settings.volume;
            deck.ui.settings.ghosting = ghosting;
            deck.ui.settings.volume = volume / 2.0;
            deck.ui.settings.dirty = true;
            let deck_ctx = Context::default();
            let pixel_ctx = Context::default();
            let mut deck_frame = eframe::Frame::_new_kittest();
            let mut pixel_frame = eframe::Frame::_new_kittest();
            let mut copied = None;
            for _ in 0..200 {
                update(&mut deck, &deck_ctx, &mut deck_frame);
                update(&mut pixel, &pixel_ctx, &mut pixel_frame);
                copied = pixel.emulator.store.load_battery(ROM);
                if copied.is_some() && pixel.ui.settings.ghosting == ghosting {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(
                copied.as_deref(),
                Some(&b"battery from the deck"[..]),
                "deck trusts {} and sees {:?}, pixel trusts {} and sees {:?}, pixel elsewhere {:?}",
                trusted(&deck),
                deck.online.network_devices(),
                trusted(&pixel),
                pixel.online.network_devices(),
                pixel.online.saves_elsewhere(),
            );
            let devices = pixel.online.network_devices();
            assert_eq!(devices.len(), 1, "{devices:?}");
            assert_eq!(devices[0].sync, crate::online::cloud::PeerSync::Syncing);
            assert_eq!(pixel.ui.settings.ghosting, ghosting);
            assert!((pixel.ui.settings.volume - volume).abs() < f32::EPSILON);
            pixel.ui.settings.online.leave();
            for _ in 0..20 {
                update(&mut pixel, &pixel_ctx, &mut pixel_frame);
                update(&mut deck, &deck_ctx, &mut deck_frame);
            }
            assert_eq!(pixel.online.network_devices(), []);
            assert_eq!(deck.online.network_devices(), []);
        }

        #[test]
        fn signed_out_saves_elsewhere_lists_only_paired_devices_saves() {
            const PAIRED_ROM: &str =
                "3333333333333333333333333333333333333333333333333333333333333333";
            let mut pixel_lan = crate::lan::tests::lan("Pixel", 1);
            let mut deck_lan = crate::lan::tests::lan("Steam Deck", 2);
            let mut laptop_lan = crate::lan::tests::lan("Clara's laptop", 3);
            crate::lan::tests::connect(&mut deck_lan, &mut pixel_lan, Some([9; 32]));
            crate::lan::tests::connect(&mut laptop_lan, &mut pixel_lan, None);
            for _ in 0..6 {
                pixel_lan.step(0.0);
                deck_lan.step(0.0);
                laptop_lan.step(0.0);
            }
            pixel_lan.remember(laptop_lan.key());
            laptop_lan.remember(pixel_lan.key());
            let mut pixel = device("elsewhere-pixel");
            let mut deck = device("elsewhere-deck");
            let mut laptop = device("elsewhere-laptop");
            laptop.ui.settings.online.leave();
            pixel.ui.settings.online.keep_saves = crate::online::KeepSaves::Library;
            pixel
                .ui
                .settings
                .online
                .network_allowed
                .push(laptop_lan.key());
            laptop
                .ui
                .settings
                .online
                .network_allowed
                .push(pixel_lan.key());
            pixel.lan.lan = Some(pixel_lan);
            deck.lan.lan = Some(deck_lan);
            laptop.lan.lan = Some(laptop_lan);
            deck.emulator
                .store
                .store_battery(ROM, b"battery from the deck")
                .unwrap();
            laptop
                .emulator
                .store
                .store_battery(PAIRED_ROM, b"battery from the laptop")
                .unwrap();
            let contexts = [Context::default(), Context::default(), Context::default()];
            let mut frames = [
                eframe::Frame::_new_kittest(),
                eframe::Frame::_new_kittest(),
                eframe::Frame::_new_kittest(),
            ];
            let mut step = |pixel: &mut Citrine, deck: &mut Citrine, laptop: &mut Citrine| {
                for (at, app) in [pixel, deck, laptop].into_iter().enumerate() {
                    update(app, &contexts[at], &mut frames[at]);
                }
            };
            let sources = |app: &Citrine| {
                let mut found: Vec<(Option<String>, Vec<String>)> = app
                    .online
                    .saves_elsewhere()
                    .into_iter()
                    .map(|game| (game.rom, game.devices))
                    .collect();
                found.sort();
                found
            };
            let both = vec![
                (Some(ROM.to_owned()), vec!["Steam Deck".to_owned()]),
                (
                    Some(PAIRED_ROM.to_owned()),
                    vec!["Clara's laptop".to_owned()],
                ),
            ];
            for _ in 0..200 {
                step(&mut pixel, &mut deck, &mut laptop);
                if sources(&pixel) == both {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(
                sources(&pixel),
                both,
                "pixel sees {:?}",
                pixel.online.network_devices()
            );
            pixel.ui.settings.online.leave();
            for _ in 0..20 {
                step(&mut pixel, &mut deck, &mut laptop);
            }
            assert_eq!(sources(&pixel), both[1..]);
        }

        #[test]
        fn a_paired_device_that_leaves_the_network_takes_its_saves_along() {
            const PAIRED_ROM: &str =
                "3333333333333333333333333333333333333333333333333333333333333333";
            let mut pixel_lan = crate::lan::tests::lan("Pixel", 1);
            let mut laptop_lan = crate::lan::tests::lan("Clara's laptop", 3);
            crate::lan::tests::connect(&mut laptop_lan, &mut pixel_lan, None);
            for _ in 0..6 {
                pixel_lan.step(0.0);
                laptop_lan.step(0.0);
            }
            pixel_lan.remember(laptop_lan.key());
            laptop_lan.remember(pixel_lan.key());
            let mut pixel = device("leaving-pixel");
            let mut laptop = device("leaving-laptop");
            laptop.ui.settings.online.leave();
            pixel.ui.settings.online.keep_saves = crate::online::KeepSaves::Library;
            pixel
                .ui
                .settings
                .online
                .network_allowed
                .push(laptop_lan.key());
            laptop
                .ui
                .settings
                .online
                .network_allowed
                .push(pixel_lan.key());
            pixel.lan.lan = Some(pixel_lan);
            laptop.lan.lan = Some(laptop_lan);
            laptop
                .emulator
                .store
                .store_battery(PAIRED_ROM, b"battery from the laptop")
                .unwrap();
            let contexts = [Context::default(), Context::default()];
            let mut frames = [eframe::Frame::_new_kittest(), eframe::Frame::_new_kittest()];
            let roms = |app: &Citrine| {
                app.online
                    .saves_elsewhere()
                    .into_iter()
                    .map(|game| game.rom)
                    .collect::<Vec<_>>()
            };
            for _ in 0..200 {
                update(&mut pixel, &contexts[0], &mut frames[0]);
                update(&mut laptop, &contexts[1], &mut frames[1]);
                if roms(&pixel) == [Some(PAIRED_ROM.to_owned())] {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(roms(&pixel), [Some(PAIRED_ROM.to_owned())]);
            laptop.lan.lan = None;
            for _ in 0..20 {
                update(&mut pixel, &contexts[0], &mut frames[0]);
            }
            assert!(
                pixel.online.network_devices().is_empty(),
                "{:?}",
                pixel.online.network_devices()
            );
            assert!(roms(&pixel).is_empty(), "{:?}", roms(&pixel));
        }
    }
    mod restart {
        use super::*;
        use std::collections::HashMap;

        const ANDROID_MAIN_HEADROOM: usize = 256 * 1024;

        #[derive(Default)]
        struct Saved(HashMap<String, String>);

        impl Storage for Saved {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }

            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.to_string(), value);
            }

            fn flush(&mut self) {}
        }

        fn first_run() -> Saved {
            let mut app = crystal_app("restart", LayoutMode::Mobile, None);
            let key = app.emulator.rom_key().map(str::to_string);
            let _ = app
                .ui
                .recent
                .record(std::path::Path::new("crystal.gbc"), "Crystal", key);
            let ctx = Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 915.0),
                )),
                ..Default::default()
            };
            for _ in 0..3 {
                let _ = ctx.run(input.clone(), |ctx| app.render(ctx));
            }
            let mut saved = Saved::default();
            eframe::set_value(&mut saved, eframe::APP_KEY, &app);
            saved
        }

        fn on_small_stack<T: Send + 'static>(start: impl FnOnce() -> T + Send + 'static) -> T {
            std::thread::Builder::new()
                .stack_size(ANDROID_MAIN_HEADROOM)
                .spawn(start)
                .unwrap()
                .join()
                .unwrap()
        }

        #[test]
        fn second_start_restores_what_the_first_saved_within_an_android_stack() {
            let saved = first_run();
            let recent: Vec<String> = Citrine::restore(Some(&saved))
                .ui
                .recent
                .entries()
                .iter()
                .map(crate::app::ui_state::recent::RecentRom::display_name)
                .collect();
            assert!(!recent.is_empty());
            let restored = on_small_stack(move || {
                Citrine::restore(Some(&saved))
                    .ui
                    .recent
                    .entries()
                    .iter()
                    .map(crate::app::ui_state::recent::RecentRom::display_name)
                    .collect::<Vec<_>>()
            });
            assert_eq!(restored, recent);
        }

        #[test]
        fn first_start_builds_within_an_android_stack() {
            let fresh = on_small_stack(|| Citrine::restore(None).ui.recent.is_empty());
            assert!(fresh);
        }

        #[test]
        fn app_state_stays_small_on_the_stack() {
            assert!(std::mem::size_of::<Citrine>() < 16 * 1024);
        }
    }
    mod rotation {
        use super::*;
        use eframe::App as _;

        struct Phone {
            size: egui::Vec2,
            insets: [f32; 4],
        }

        const NATIVE: f32 = 3.5;

        fn portrait() -> Phone {
            Phone {
                size: egui::vec2(1440.0, 3120.0),
                insets: [0.0, 130.0, 0.0, 70.0],
            }
        }

        fn landscape() -> Phone {
            Phone {
                size: egui::vec2(3120.0, 1440.0),
                insets: [130.0, 0.0, 0.0, 70.0],
            }
        }

        #[derive(Debug, PartialEq)]
        struct Shown {
            points: f32,
            content: egui::Rect,
            landscape: bool,
        }

        fn phone_frame(app: &mut Citrine, ctx: &Context, phone: &Phone) -> Shown {
            let points = ctx.zoom_factor() * NATIVE;
            let [left, top, right, bottom] = phone.insets.map(|px| px / points);
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    phone.size / points,
                )),
                safe_area_insets: Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
                    left,
                    right,
                    top,
                    bottom,
                })),
                ..Default::default()
            };
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .native_pixels_per_point = Some(NATIVE);
            app.raw_input_hook(ctx, &mut input);
            let _ = ctx.run(input, |ctx| {
                mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
                let mobile = app.is_mobile(ctx);
                let Citrine {
                    ui,
                    audio,
                    emulator,
                    ..
                } = app;
                ui.settings.apply(ctx, audio, emulator, mobile);
            });
            Shown {
                points: ctx.pixels_per_point(),
                content: ctx.content_rect(),
                landscape: mobile::landscape(ctx),
            }
        }

        fn settles(app: &mut Citrine, ctx: &Context, phone: &Phone) -> Shown {
            for _ in 0..3 {
                phone_frame(app, ctx, phone);
            }
            let first = phone_frame(app, ctx, phone);
            for frame in 0..20 {
                assert_eq!(
                    phone_frame(app, ctx, phone),
                    first,
                    "frame {frame} after the turn shows the same layout"
                );
            }
            first
        }

        #[test]
        fn turning_the_phone_settles_on_one_layout_each_way() {
            let mut app = phone_app("rotation");
            let ctx = Context::default();
            let upright = settles(&mut app, &ctx, &portrait());
            assert!(!upright.landscape);
            let turned = settles(&mut app, &ctx, &landscape());
            assert!(turned.landscape);
            assert_eq!(turned.points, upright.points, "turning keeps the text size");
            assert!(turned.content.width() > turned.content.height());
            assert_eq!(settles(&mut app, &ctx, &portrait()), upright);
        }
    }

    pub(crate) fn test_app() -> Citrine {
        Citrine::default()
    }

    pub(crate) fn phone_app(name: &str) -> Citrine {
        let root = crate::storage::data_dir().join("phone-saves").join(name);
        let _ = std::fs::remove_dir_all(&root);
        let mut app = test_app();
        app.emulator.store = crate::storage::SaveStore::at(root);
        app.ui.settings.layout = LayoutMode::Mobile;
        app
    }

    fn counted(app: &mut Citrine) -> Vec<(&'static str, usize)> {
        app.data_parts()
            .tallies(&crate::storage::kinds::Usage::new())
            .into_iter()
            .map(|(kind, tally)| (kind.name, tally.count))
            .collect()
    }

    fn fill_every_part(app: &mut Citrine) {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        let key = app.emulator.rom_key().unwrap().to_owned();
        app.emulator.store.store_battery(&key, b"battery").unwrap();
        let png = crate::utils::screenshot::encode_png(160, 144, &vec![9; 160 * 144 * 4]).unwrap();
        app.media
            .add_screenshot(
                crate::media::NewMedia::current(
                    crate::media::MediaKind::Screenshot,
                    &mut app.emulator,
                ),
                &png,
            )
            .unwrap();
        for (file, size) in [
            (crate::app::boot_rom::BootRom::Gb, 0x100),
            (crate::app::boot_rom::BootRom::Gbc, 0x900),
        ] {
            app.handle_load_boot_rom(
                file,
                crate::utils::file_loader::PickedFile {
                    name: "boot.bin".to_owned(),
                    data: vec![1; size].into(),
                    path: None,
                    seen: None,
                },
            );
        }
        #[cfg(feature = "gba")]
        app.handle_load_gba_bios(crate::utils::file_loader::PickedFile {
            name: "gba_bios.bin".to_owned(),
            data: vec![1; 0x4000].into(),
            path: None,
            seen: None,
        });
        #[cfg(feature = "nds")]
        {
            use crate::app::ds_firmware::DsFile;
            for (file, size) in [
                (DsFile::Arm9Bios, 0x1000),
                (DsFile::Arm7Bios, 0x4000),
                (DsFile::Firmware, 0x4_0000),
            ] {
                app.handle_load_ds_file(
                    file,
                    crate::utils::file_loader::PickedFile {
                        name: format!("{}.bin", file.label()),
                        data: vec![1; size].into(),
                        path: None,
                        seen: None,
                    },
                );
            }
            let walker = app.emulator.pokewalker.walker().unwrap().clone();
            app.emulator.pokewalker.save(&walker);
        }
    }

    #[test]
    fn a_test_app_starts_with_no_data_even_after_another_test_filled_every_part() {
        let filled = std::thread::spawn(|| {
            let mut app = test_app();
            fill_every_part(&mut app);
            counted(&mut app)
        })
        .join()
        .unwrap();
        let mut wanted = vec!["Saves", "Screenshots", "GB boot ROM", "GBC boot ROM"];
        if cfg!(feature = "gba") {
            wanted.push("GBA BIOS");
        }
        if cfg!(feature = "nds") {
            wanted.push("Pokéwalker");
        }
        for name in wanted {
            assert!(
                filled
                    .iter()
                    .any(|&(part, count)| part == name && count > 0),
                "the other test filled {name}: {filled:?}"
            );
        }
        let fresh = counted(&mut test_app());
        assert!(fresh.iter().all(|&(_, count)| count == 0), "{fresh:?}");
    }

    fn small_targets(app: &mut Citrine, ctx: &Context) -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 3000.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input.clone(), |ctx| {
                mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
            }));
        }
        let link = ctx.style().visuals.hyperlink_color;
        let texts: Vec<(egui::Rect, String, bool)> = output
            .unwrap()
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.visual_bounding_rect(),
                    text.galley.text().trim().to_owned(),
                    text.fallback_color == link,
                )),
                _ => None,
            })
            .collect();
        ctx.viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|widget| widget.sense.senses_click() && widget.sense.is_focusable())
                .filter(|widget| widget.rect.area() > 0.0)
                .filter(|widget| widget.rect.height() < TOUCH_TARGET_PX - 0.5)
                .filter_map(|widget| {
                    let inside: Vec<&(egui::Rect, String, bool)> = texts
                        .iter()
                        .filter(|(rect, _, _)| widget.rect.contains(rect.center()))
                        .collect();
                    let inline_link =
                        !inside.is_empty() && inside.iter().all(|(_, _, is_link)| *is_link);
                    (!inline_link).then(|| {
                        let label: Vec<&str> =
                            inside.iter().map(|(_, text, _)| text.as_str()).collect();
                        format!(
                            "{} {:.0}px {:?}",
                            nav_name(app),
                            widget.rect.height(),
                            label
                        )
                    })
                })
                .collect()
        })
    }

    fn nav_name(app: &Citrine) -> String {
        let nav = &app.mobile.nav;
        format!(
            "{:?}/{:?}/{:?}/more {}",
            nav.place(),
            nav.page(),
            nav.settings_page(),
            nav.more_open()
        )
    }

    #[test]
    fn every_touch_control_is_at_least_44_px_tall() {
        let ctx = Context::default();
        let mut app = phone_app("targets");
        let mut small = Vec::new();

        small.extend(small_targets(&mut app, &ctx));
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        rom[0x0149] = 0x02;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app.emulator.save_snapshot(0).unwrap();
        app.ui.snapshots.invalidate();

        app.mobile.nav.show_game();
        small.extend(small_targets(&mut app, &ctx));
        app.mobile.nav.set_more(true);
        small.extend(small_targets(&mut app, &ctx));
        for place in Place::ALL {
            app.mobile.nav = Default::default();
            app.mobile.nav.go(place);
            small.extend(small_targets(&mut app, &ctx));
        }
        for tab in [Tab::Homebrew, Tab::Saves, Tab::Info, Tab::Stats] {
            app.mobile.nav = Default::default();
            app.mobile.open(tab);
            small.extend(small_targets(&mut app, &ctx));
        }
        let first_game =
            crate::app::widgets::homebrew::details_id(crate::homebrew::HOMEBREW_GAMES[0].id);
        let mut details = egui::collapsing_header::CollapsingState::load_with_default_open(
            &ctx, first_game, true,
        );
        details.set_open(true);
        details.store(&ctx);
        app.mobile.nav = Default::default();
        app.mobile.open(Tab::Homebrew);
        small.extend(small_targets(&mut app, &ctx));
        for tab in SettingsTab::iter_with_dev_mode(false) {
            app.mobile.nav.open_settings(tab);
            app.ui.settings.current_tab = tab;
            small.extend(small_targets(&mut app, &ctx));
        }

        assert!(small.is_empty(), "too small to tap:\n{}", small.join("\n"));
    }

    fn playing(app: &mut Citrine) {
        app.emulator
            .load_rom(
                &Rom::new(&[0u8; 0x8000]),
                GbModel::Dmg,
                #[cfg(not(target_arch = "wasm32"))]
                None,
            )
            .expect("load");
        app.emulator.running = true;
    }

    #[test]
    fn reset_asks_first_and_cancel_keeps_playing() {
        let mut app = phone_app("reset-cancel");
        playing(&mut app);
        app.run_hotkey(HotkeyAction::SoftReset);
        assert!(app.pending_reset.is_some(), "reset waits for an answer");
        assert!(!app.emulator.running, "the game holds still while asking");
        app.finish_reset(false);
        assert!(app.pending_reset.is_none());
        assert!(app.emulator.running);
        app.run_hotkey(HotkeyAction::SoftReset);
        assert!(
            app.pending_reset.is_some(),
            "a cancel doesn't stop the next ask"
        );
    }

    fn playing_both(app: &mut Citrine) -> String {
        let mut data = vec![0u8; 0x8000];
        data[0x0143] = 0x80;
        data[0x0147] = 0x03;
        let rom = Rom::new(&data);
        let model = app.ui.settings.console_for_rom(&rom);
        app.emulator.load_rom(&rom, model, None).expect("load");
        app.emulator.running = true;
        app.emulator.rom_key().expect("key").to_owned()
    }

    fn wait_until_playing(app: &mut Citrine) {
        std::thread::sleep(std::time::Duration::from_millis(5100));
        assert!(app.emulator.has_been_playing());
    }

    #[test]
    fn a_fresh_game_switches_console_without_asking() {
        let mut app = phone_app("console-fresh");
        let key = playing_both(&mut app);
        assert_eq!(app.emulator.console(), GbModel::Cgb);
        app.events.play_on(GbModel::Dmg);
        app.handle_event_queue(true);
        assert!(app.pending_console.is_none());
        assert_eq!(app.emulator.console(), GbModel::Dmg);
        assert_eq!(
            app.ui.settings.console_for(&key, RomCgbMode::CgbAndGb),
            GbModel::Dmg,
            "the game remembers its console"
        );

        let mut again = phone_app("console-fresh-again");
        again.ui.settings = std::mem::take(&mut app.ui.settings);
        playing_both(&mut again);
        assert_eq!(
            again.emulator.console(),
            GbModel::Dmg,
            "and starts there next time"
        );
    }

    #[test]
    fn switching_a_running_game_asks_first() {
        let mut app = phone_app("console-ask");
        let key = playing_both(&mut app);
        wait_until_playing(&mut app);
        app.events.play_on(GbModel::Dmg);
        app.handle_event_queue(true);
        assert!(
            app.pending_console.is_some(),
            "the switch waits for an answer"
        );
        assert!(!app.emulator.running, "the game holds still while asking");

        app.finish_play_on(false);
        assert!(app.emulator.running);
        assert_eq!(
            app.emulator.console(),
            GbModel::Cgb,
            "cancel keeps the console"
        );
        assert!(
            app.ui.settings.own_value(&key, "console").is_none(),
            "and the choice"
        );

        app.events.play_on(GbModel::Dmg);
        app.handle_event_queue(true);
        app.finish_play_on(true);
        assert_eq!(app.emulator.console(), GbModel::Dmg);
        assert_eq!(
            app.ui.settings.console_for(&key, RomCgbMode::CgbAndGb),
            GbModel::Dmg
        );
    }

    #[test]
    fn a_game_for_one_console_offers_no_switch() {
        let mut app = phone_app("console-one");
        playing(&mut app);
        app.events.play_on(GbModel::Cgb);
        app.handle_event_queue(true);
        assert_eq!(app.emulator.console(), GbModel::Dmg);
        let key = app.emulator.rom_key().expect("key").to_owned();
        assert_eq!(app.ui.settings.own_values(&key).count(), 0);
    }

    #[test]
    fn dont_ask_again_lasts_until_citrine_closes() {
        let mut app = phone_app("reset-stop-asking");
        playing(&mut app);
        app.run_hotkey(HotkeyAction::SoftReset);
        app.pending_reset.as_mut().unwrap().stop_asking = true;
        app.finish_reset(true);
        app.run_hotkey(HotkeyAction::SoftReset);
        assert!(app.pending_reset.is_none(), "no second ask this session");
        assert!(app.emulator.running);

        let saved = serde_json::to_string(&app).unwrap();
        assert!(
            !saved.contains("without_asking") && !saved.contains("stop_asking"),
            "the choice is never saved"
        );
    }

    #[test]
    fn opening_a_tab_in_the_touch_layout_goes_to_its_place() {
        let mut app = phone_app("open-tab");
        app.events.open_tab(Tab::Media);
        app.handle_event_queue(true);
        assert_eq!(app.mobile.nav.place(), Place::Media);
        assert!(app.mobile.showing(Tab::Media));
        assert!(
            app.dock.find_tab(&Tab::Media).is_none(),
            "no hidden dock tab"
        );

        let mut desktop = phone_app("open-tab-desktop");
        desktop.events.open_tab(Tab::Media);
        desktop.handle_event_queue(false);
        assert!(!desktop.mobile.showing(Tab::Media));
        assert!(desktop.dock.find_tab(&Tab::Media).is_some());
    }

    fn media_app(name: &str, layout: LayoutMode) -> Citrine {
        let mut app = phone_app(name);
        app.ui.settings.layout = layout;
        let root = crate::storage::data_dir().join("media-app").join(name);
        let _ = std::fs::remove_dir_all(&root);
        app.media = MediaStore::at(root);
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app
    }

    fn media_in_front(app: &Citrine) -> bool {
        let Some((surface, node, tab)) = app.dock.find_tab(&Tab::Media) else {
            return false;
        };
        app.dock[surface][node]
            .get_leaf()
            .is_some_and(|leaf| leaf.active == tab)
    }

    fn game_in_view(app: &Citrine) -> bool {
        let (surface, node, tab) = app.dock.find_tab(&Tab::GameBoy).unwrap();
        app.dock[surface][node]
            .get_leaf()
            .is_some_and(|leaf| leaf.active == tab)
    }

    pub(crate) fn play_frame(
        app: &mut Citrine,
        ctx: &Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
            app.show_toasts(ctx);
            app.handle_event_queue(touch);
        })
    }

    #[cfg(feature = "gba")]
    pub(crate) fn inside_its_clip(output: &egui::FullOutput, wanted: &str) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                text.visual_bounding_rect().max.x <= clipped.clip_rect.max.x + 0.5
            }
            _ => false,
        })
    }

    pub(crate) fn text_rect(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
    }

    pub(crate) fn shown_whole(output: &egui::FullOutput, wanted: &str) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().trim() == wanted => clipped
                .clip_rect
                .expand(0.5)
                .contains_rect(text.visual_bounding_rect()),
            _ => false,
        })
    }

    pub(crate) fn assert_in_front(app: &Citrine, tab: Tab) {
        let (surface, node, index) = app.dock.find_tab(&tab).expect("the tab is open");
        let egui_dock::Node::Leaf(leaf) = &app.dock[surface][node] else {
            panic!("{tab:?} is not in a leaf");
        };
        assert_eq!(leaf.active, index, "{tab:?} shows in its pane");
        assert_eq!(
            app.dock.focused_leaf(),
            Some((surface, node)),
            "{tab:?} has the focus"
        );
    }

    pub(crate) fn load_game(app: &mut Citrine, rom: &[u8], save: Option<&[u8]>) {
        let rom = citrine_gb::rom::Rom::new(rom);
        if let Some(save) = save {
            let key = rom.header().unwrap().sha256_hex_string();
            app.emulator.store.store_battery(&key, save).unwrap();
        }
        app.emulator.load_rom(&rom, GbModel::Cgb, None).unwrap();
        app.watch_game_info();
    }

    fn crystal_app(name: &str, layout: LayoutMode, save: Option<&[u8]>) -> Citrine {
        let mut app = phone_app(name);
        app.ui.settings.layout = layout;
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            save,
        );
        app
    }

    pub(crate) fn text_containing(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().contains(wanted) => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
    }

    fn badge_count(output: &egui::FullOutput) -> usize {
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Path(path) => {
                    let p = &path.points;
                    p.len() == 4 && p[0].x == p[2].x && p[1].y == p[3].y && p[0].y < p[2].y
                }
                _ => false,
            })
            .count()
    }

    pub(crate) fn tap(at: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]
    }

    fn escape() -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]
    }

    #[test]
    fn game_info_is_offered_only_for_a_recognised_game() {
        let ctx = Context::default();
        let mut app = crystal_app("info-offered", LayoutMode::Mobile, None);
        assert!(app.game_info.recognised());
        app.mobile.nav.show_game();
        app.mobile.nav.set_more(true);
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_containing(&output, Tab::GameInfo.label()).is_some());

        let ctx = Context::default();
        let mut other = media_app("info-not-offered", LayoutMode::Mobile);
        other.watch_game_info();
        assert!(!other.game_info.recognised());
        other.mobile.nav.show_game();
        other.mobile.nav.set_more(true);
        play_frame(&mut other, &ctx, vec![]);
        let output = play_frame(&mut other, &ctx, vec![]);
        assert!(text_containing(&output, Tab::GameInfo.label()).is_none());
    }

    #[test]
    fn game_info_closes_when_an_unsupported_game_loads() {
        let mut zero = vec![0u8; 0x8000];
        zero[0x0147] = 0x03;
        let latin = citrine_gb_gamedata::fixtures::japanese_cartridge_in_latin();
        for (name, rom) in [("zero", zero), ("latin", latin)] {
            let mut desktop =
                crystal_app(&format!("info-closes-{name}"), LayoutMode::Desktop, None);
            desktop.open_tab(Tab::GameInfo);
            assert!(desktop.dock.find_tab(&Tab::GameInfo).is_some());
            load_game(&mut desktop, &rom, None);
            assert!(!desktop.game_info.recognised(), "{name}");
            assert!(desktop.dock.find_tab(&Tab::GameInfo).is_none(), "{name}");

            let mut phone = crystal_app(&format!("info-sheet-{name}"), LayoutMode::Mobile, None);
            phone.mobile.open(Tab::GameInfo);
            load_game(&mut phone, &rom, None);
            assert!(!phone.mobile.showing(Tab::GameInfo), "{name}");
        }
    }

    #[test]
    fn game_info_shows_eight_badges_in_gold_and_silver_until_kanto() {
        use citrine_gb_gamedata::fixtures::crystal_save_with_badges;
        for (reached, shown) in [(false, 8), (true, 16)] {
            let save = crystal_save_with_badges(0xFF, 0, reached);
            let ctx = Context::default();
            let mut app = crystal_app(
                &format!("info-badges-{reached}"),
                LayoutMode::Mobile,
                Some(&save),
            );
            app.mobile.open(Tab::GameInfo);
            app.watch_game_info();
            let output = play_frame(&mut app, &ctx, vec![]);
            assert_eq!(badge_count(&output), shown, "reached Kanto: {reached}");
        }
    }

    #[test]
    fn game_info_back_on_a_phone_closes_the_details_before_the_page() {
        let save = citrine_gb_gamedata::fixtures::crystal_save();
        let ctx = Context::default();
        let mut app = crystal_app("info-back", LayoutMode::Mobile, Some(&save));
        app.mobile.nav.show_game();
        app.mobile.open(Tab::GameInfo);
        app.watch_game_info();
        app.game_info.view.details = true;
        assert!(app.game_info.details_open());

        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(
            text_containing(&output, "Party").is_some(),
            "the details name their page"
        );
        let back = text_containing(&output, crate::icons::ARROW_LEFT).expect("a back arrow");
        play_frame(&mut app, &ctx, tap(back.center()));
        assert!(!app.game_info.details_open());
        assert!(app.mobile.showing(Tab::GameInfo));

        app.game_info.view.details = true;
        let press = |app: &mut Citrine, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 915.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                app.render(ctx);
                app.mobile_escape(ctx);
            });
        };
        press(&mut app, vec![]);
        press(&mut app, escape());
        assert!(!app.game_info.details_open());
        assert!(app.mobile.showing(Tab::GameInfo));
        press(&mut app, escape());
        assert!(app.mobile.nav.on_game());
    }

    fn info_frame(app: &mut Citrine, ctx: &Context, width: f32) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 915.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
        });
    }

    fn walk_game_info(app: &mut Citrine, ctx: &Context, width: f32) {
        use crate::game_info::Page;
        for page in [Page::Party, Page::Boxes, Page::Bag, Page::Pokedex] {
            app.game_info.view.page = page;
            app.game_info.view.details = false;
            info_frame(app, ctx, width);
        }
        app.game_info.view.page = Page::Party;
        for pick in 0..6 {
            app.game_info.view.party_pick = pick;
            app.game_info.view.details = true;
            info_frame(app, ctx, width);
        }
        app.game_info.view.page = Page::Boxes;
        for pc_box in 0..14 {
            app.game_info.view.pc_box = Some(pc_box);
            for pick in [0, 5, 19] {
                app.game_info.view.box_pick = pick;
                app.game_info.view.details = true;
                info_frame(app, ctx, width);
            }
        }
        app.game_info.view.page = Page::Pokedex;
        app.game_info.view.details = false;
        for (search, caught) in [("", true), ("a", false), ("zz", false)] {
            app.game_info.view.dex_search = search.to_owned();
            app.game_info.view.dex_caught = caught;
            info_frame(app, ctx, width);
        }
    }

    fn reach_overworld(app: &mut Citrine) -> bool {
        use citrine_gb::console::Keys;
        for _ in 0..60 {
            app.emulator.gb_mut().run_frame();
        }
        for step in 0..120 {
            let button = if step % 2 == 0 { Keys::START } else { Keys::A };
            app.emulator.gb_mut().press_button(button.into());
            for _ in 0..6 {
                app.emulator.gb_mut().run_frame();
            }
            app.emulator.gb_mut().release_button(button.into());
            for _ in 0..54 {
                app.emulator.gb_mut().run_frame();
            }
            app.game_info.next_read_now();
            app.watch_game_info();
            if app.game_info.game().is_some_and(|game| game.live.is_some()) {
                return true;
            }
        }
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore]
    fn game_info_opens_for_every_real_pokemon_game() {
        let test = "game_info_opens_for_every_real_pokemon_game";
        let dir = crate::real_roms::rom_dir();
        let silver_save = crate::real_roms::save_from(test, "CITRINE_SILVER_SAVE");
        for name in [
            "Pokemon - Red Version (USA, Europe).gb",
            "Pokemon Yellow.gb",
            "Pokemon - Gold Version (USA, Europe).gbc",
            "Pokemon - Silver Version (USA, Europe).gbc",
            "Pokemon Crystal.gbc",
        ] {
            let rom = std::fs::read(dir.join(name)).expect(name);
            let save = silver_save.as_deref().filter(|_| name.contains("Silver"));
            for (layout, width) in [(LayoutMode::Desktop, 1280.0), (LayoutMode::Mobile, 412.0)] {
                let ctx = Context::default();
                let mut app = phone_app(&format!("real-{}-{width}", name.len()));
                app.ui.settings.layout = layout;
                load_game(&mut app, &rom, save);
                assert!(app.game_info.recognised(), "{name}");
                if layout == LayoutMode::Desktop {
                    app.open_tab(Tab::GameInfo);
                } else {
                    app.mobile.open(Tab::GameInfo);
                }
                app.watch_game_info();
                walk_game_info(&mut app, &ctx, width);
                let shown = app.game_info.game().and_then(|game| game.state()).is_some();
                println!("{name} at {width}: opened, save shown: {shown}");
                if save.is_some() {
                    assert!(shown, "{name}");
                    assert!(
                        reach_overworld(&mut app),
                        "{name} never reached the overworld"
                    );
                    walk_game_info(&mut app, &ctx, width);
                    if layout == LayoutMode::Desktop {
                        time_game_info(&mut app);
                    }
                }
            }
        }
    }

    fn time_game_info(app: &mut Citrine) {
        let rounds = 200;
        let start = web_time::Instant::now();
        for _ in 0..rounds {
            app.game_info.next_read_now();
            app.watch_game_info();
        }
        let read = start.elapsed() / rounds;
        let start = web_time::Instant::now();
        let idle_rounds = 100_000;
        for _ in 0..idle_rounds {
            app.watch_game_info();
        }
        let idle = start.elapsed() / idle_rounds;
        println!("a frame with a read: {read:?}; a frame without: {idle:?}");
    }

    #[test]
    fn game_info_on_desktop_has_room_for_details_beside_the_list() {
        let save = citrine_gb_gamedata::fixtures::crystal_save();
        let ctx = Context::default();
        let mut app = crystal_app("info-room", LayoutMode::Desktop, Some(&save));
        app.open_tab(Tab::GameInfo);
        app.widen_for_game_info();
        app.watch_game_info();
        let _ = info_frame_out(&mut app, &ctx, 1280.0, 800.0);
        let output = info_frame_out(&mut app, &ctx, 1280.0, 800.0);
        let egg = text_rect(&output, "Egg").expect("party list");
        let stats = text_rect(&output, "Stats").expect("details");
        assert!(stats.left() > egg.right(), "details beside the list");
        let right = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.visual_bounding_rect().right()),
                _ => None,
            })
            .filter(|right| right.is_finite())
            .fold(0.0, f32::max);
        assert!(right <= 1280.0, "text reaches {right}");
    }

    #[test]
    fn game_info_pages_fit_the_panel_at_every_desktop_width() {
        use crate::game_info::Page;
        let save = citrine_gb_gamedata::fixtures::crystal_save();
        for width in [560.0, 600.0, 640.0, 670.0, 720.0, 800.0, 1000.0, 1280.0] {
            for page in [Page::Map, Page::Party, Page::Journey] {
                let ctx = Context::default();
                let mut app = crystal_app("info-fit", LayoutMode::Desktop, Some(&save));
                app.open_tab(Tab::GameInfo);
                app.widen_for_game_info();
                app.watch_game_info();
                app.game_info.view.page = page;
                let _ = info_frame_out(&mut app, &ctx, width, 800.0);
                let output = info_frame_out(&mut app, &ctx, width, 800.0);
                let status = output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(text)
                            if text.galley.text().starts_with("Save file")
                                || text.galley.text().starts_with("Playing") =>
                        {
                            Some(text.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .expect("status line");
                let left = output
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(text) => Some(text.visual_bounding_rect()),
                        _ => None,
                    })
                    .filter(|rect| (rect.center().y - status.center().y).abs() < 4.0)
                    .map(|rect| rect.left())
                    .fold(f32::INFINITY, f32::min);
                for clipped in &output.shapes {
                    let rect = match &clipped.shape {
                        egui::Shape::Text(text) => text.visual_bounding_rect(),
                        egui::Shape::Rect(rect) => rect.rect,
                        _ => continue,
                    };
                    if rect.top() > status.top()
                        && rect.left() >= left - 0.5
                        && rect.left() < status.right()
                        && rect.right().is_finite()
                    {
                        assert!(
                            rect.right() <= status.right() + 1.0,
                            "a shape on {page:?} ends at {}, the panel at {} (screen {width})",
                            rect.right(),
                            status.right()
                        );
                    }
                }
            }
        }
    }

    pub(crate) fn info_frame_out(
        app: &mut Citrine,
        ctx: &Context,
        width: f32,
        height: f32,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
        })
    }

    #[test]
    fn a_screenshot_brings_the_media_tab_to_front_on_desktop() {
        let mut app = media_app("shot-desktop", LayoutMode::Desktop);
        app.open_tab(Tab::Settings);
        assert!(!media_in_front(&app));

        app.take_screenshot();
        app.handle_event_queue(false);
        assert!(media_in_front(&app));
        assert!(game_in_view(&app));
        let first = app.ui.media.highlighted().cloned().expect("highlighted");

        let layout = format!("{:?}", app.dock.iter_all_tabs().collect::<Vec<_>>());
        app.take_screenshot();
        app.handle_event_queue(false);
        assert!(media_in_front(&app));
        assert_eq!(
            layout,
            format!("{:?}", app.dock.iter_all_tabs().collect::<Vec<_>>())
        );
        let second = app.ui.media.highlighted().cloned().expect("highlighted");
        assert_ne!(first, second);
        assert!(!app.mobile.showing(Tab::Media));
    }

    #[test]
    fn a_screenshot_never_hides_the_game_behind_the_media_tab() {
        let mut app = media_app("shot-shared-leaf", LayoutMode::Desktop);
        app.dock = DockState::new(vec![Tab::GameBoy, Tab::Settings]);
        let (surface, node, tab) = app.dock.find_tab(&Tab::GameBoy).unwrap();
        app.dock.set_active_tab((surface, node, tab));

        app.take_screenshot();
        app.handle_event_queue(false);
        assert!(game_in_view(&app));
        assert!(app.dock.find_tab(&Tab::Media).is_none());
        assert_eq!(app.ui.media.highlighted(), None);
    }

    #[test]
    fn a_phone_screenshot_keeps_the_game_and_offers_to_open_media() {
        let ctx = Context::default();
        let mut app = media_app("shot-phone", LayoutMode::Mobile);
        play_frame(&mut app, &ctx, Vec::new());
        app.take_screenshot();
        let mut output = play_frame(&mut app, &ctx, Vec::new());
        for _ in 0..40 {
            output = play_frame(&mut app, &ctx, Vec::new());
        }
        assert!(!app.mobile.showing(Tab::Media));
        assert!(text_rect(&output, "Screenshot saved to Media").is_some());
        let open = text_rect(&output, "Open").expect("an Open button on the toast");

        let at = open.center();
        play_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        play_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.mobile.showing(Tab::Media));
        let id = app.media.items()[0].id.clone();
        assert_eq!(app.ui.media.highlighted(), Some(&id));
    }

    #[test]
    fn a_forced_desktop_layout_falls_back_to_touch_on_a_narrow_window() {
        let phone = egui::vec2(412.0, 915.0);
        let landscape_phone = egui::vec2(915.0, 412.0);
        let desktop = egui::vec2(1280.0, 800.0);
        assert!(LayoutMode::Desktop.is_touch_at(phone));
        assert!(!LayoutMode::Desktop.is_touch_at(landscape_phone));
        assert!(!LayoutMode::Desktop.is_touch_at(desktop));
        assert!(LayoutMode::Auto.is_touch_at(landscape_phone));
        assert!(LayoutMode::Mobile.is_touch_at(desktop));
    }

    fn frame_at(
        app: &mut Citrine,
        ctx: &Context,
        css: egui::Vec2,
        pixel_ratio: f32,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                css / ctx.zoom_factor(),
            )),
            viewports: std::iter::once((
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    native_pixels_per_point: Some(pixel_ratio),
                    ..Default::default()
                },
            ))
            .collect(),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
            app.ui
                .settings
                .apply(ctx, &mut app.audio, &mut app.emulator, touch);
        })
    }

    fn settle(
        app: &mut Citrine,
        ctx: &Context,
        css: egui::Vec2,
        pixel_ratio: f32,
    ) -> egui::FullOutput {
        for _ in 0..3 {
            frame_at(app, ctx, css, pixel_ratio);
        }
        frame_at(app, ctx, css, pixel_ratio)
    }

    fn touch_heights_in_css_px(
        ctx: &Context,
        output: egui::FullOutput,
        pixel_ratio: f32,
    ) -> (Vec<egui::Rect>, Vec<f32>) {
        let labels: Vec<egui::Pos2> = output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text)
                    if ["Open a ROM", "Homebrew games"]
                        .iter()
                        .any(|label| text.galley.text().ends_with(label)) =>
                {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .collect();
        let css_per_point = ctx.pixels_per_point() / pixel_ratio;
        let bar = egui::containers::panel::PanelState::load(ctx, egui::Id::new("place_bar"))
            .expect("the phone bar is drawn")
            .rect
            .expand(1.0 / ctx.pixels_per_point());
        ctx.viewport(|viewport| {
            let clickable: Vec<egui::Rect> = viewport
                .prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|widget| widget.sense.senses_click() && widget.sense.is_focusable())
                .filter(|widget| widget.rect.area() > 0.0)
                .map(|widget| widget.rect)
                .collect();
            let bar_buttons = clickable
                .iter()
                .filter(|rect| bar.contains_rect(**rect))
                .map(|rect| *rect * css_per_point)
                .collect();
            let empty_state = labels
                .iter()
                .filter_map(|label| {
                    clickable
                        .iter()
                        .filter(|rect| rect.contains(*label))
                        .map(|rect| rect.height() * css_per_point)
                        .reduce(f32::min)
                })
                .collect();
            (bar_buttons, empty_state)
        })
    }

    fn assert_touch_sized(ctx: &Context, output: egui::FullOutput, pixel_ratio: f32, scale: f32) {
        let (bar, empty_state) = touch_heights_in_css_px(ctx, output, pixel_ratio);
        let at = format!("pixel ratio {pixel_ratio}, ui scale {scale}");
        assert!(bar.len() >= 3, "{at}: {} phone bar buttons", bar.len());
        assert_eq!(empty_state.len(), 2, "{at}: Open a ROM and Homebrew games");
        for rect in &bar {
            assert!(
                rect.height() >= TOUCH_TARGET_PX - 0.5,
                "{at}: phone bar button {:.1} css px tall",
                rect.height()
            );
        }
        for (i, a) in bar.iter().enumerate() {
            for b in &bar[i + 1..] {
                assert!(
                    a.intersect(*b).area() < 1.0,
                    "{at}: phone bar buttons {a:?} and {b:?} overlap"
                );
            }
        }
        for height in empty_state {
            assert!(
                height >= TOUCH_TARGET_PX - 0.5,
                "{at}: empty state button {height:.1} css px tall"
            );
        }
    }

    #[test]
    fn touch_controls_are_44_css_px_at_every_pixel_ratio_and_ui_scale() {
        let phone = egui::vec2(412.0, 915.0);
        for pixel_ratio in [1.0, 2.0, 2.625, 3.0, 3.5] {
            for scale in [0.6, 1.0, 1.5, 2.0] {
                let ctx = Context::default();
                let mut app = phone_app(&format!("dpr-{pixel_ratio}-{scale}"));
                app.ui.settings.touch.ui_scale = scale;
                app.ui.settings.ui_scale = scale;
                app.ui.settings.touch.show_controls = false;
                let output = settle(&mut app, &ctx, phone, pixel_ratio);
                let wanted = TOUCH_TARGET_PX * pixel_ratio / ctx.pixels_per_point();
                assert!(
                    (ctx.style().spacing.interact_size.y - wanted).abs() < 0.01,
                    "pixel ratio {pixel_ratio}, ui scale {scale}: interact size {} points, wanted {wanted}",
                    ctx.style().spacing.interact_size.y
                );
                assert_touch_sized(&ctx, output, pixel_ratio, scale);
            }
        }
    }

    #[test]
    fn a_new_pixel_ratio_or_ui_scale_resizes_touch_controls_without_a_restart() {
        let phone = egui::vec2(412.0, 915.0);
        let ctx = Context::default();
        let mut app = phone_app("dpr-change");
        app.ui.settings.touch.show_controls = false;
        let output = settle(&mut app, &ctx, phone, 1.0);
        assert_touch_sized(&ctx, output, 1.0, 1.0);
        let output = settle(&mut app, &ctx, phone, 3.0);
        assert_touch_sized(&ctx, output, 3.0, 1.0);
        app.ui.settings.touch.ui_scale = 1.5;
        let output = settle(&mut app, &ctx, phone / 1.25, 3.75);
        assert_touch_sized(&ctx, output, 3.75, 1.5);
    }

    fn desk_frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            app.render(ctx);
            app.handle_event_queue(false);
            app.ui
                .settings
                .apply(ctx, &mut app.audio, &mut app.emulator, false);
        })
    }

    fn click_at(app: &mut Citrine, ctx: &Context, at: egui::Pos2) -> egui::FullOutput {
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        desk_frame(app, ctx, vec![egui::Event::PointerMoved(at), press(true)]);
        desk_frame(app, ctx, vec![press(false)])
    }

    #[test]
    fn on_desktop_a_mouse_click_on_hide_closes_a_load_toast_over_the_game() {
        let ctx = Context::default();
        let mut app = media_app("hide-over-game", LayoutMode::Desktop);
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        let frame = |app: &mut Citrine, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                app.render(ctx);
                app.show_toasts(ctx);
                app.handle_event_queue(false);
            })
        };
        let start = web_time::Instant::now();
        let progress = crate::progress::Progress::unknown();
        app.toasts
            .load("hold", "Getting your save", progress, start);
        app.toasts.load(
            "hold",
            "Getting your save",
            progress,
            start + std::time::Duration::from_secs(1),
        );
        app.toasts
            .load_note("hold", "The game starts when it arrives");
        let mut output = frame(&mut app, Vec::new());
        for _ in 0..4 {
            output = frame(&mut app, Vec::new());
        }
        let toast = app.toasts.rects(&ctx)[0];
        let picture = picture_rect(&output).expect("the game is drawn");
        let (_, games, _) = app.dock.find_tab(&Tab::GameBoy).expect("the game's pane");
        let pane = app.dock.main_surface()[games]
            .get_leaf()
            .expect("a leaf")
            .viewport;
        let edge = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(border)
                    if border.stroke.width > 0.0
                        && border.rect.width() >= pane.width() - 4.0
                        && (border.rect.bottom() - pane.bottom()).abs() <= 2.0 =>
                {
                    Some(border.rect.bottom() - border.stroke.width)
                }
                _ => None,
            })
            .expect("the pane's border");
        let above = toast.top() - picture.bottom();
        let below = edge - toast.bottom();
        let centred = above > 2.0 && (above - below).abs() <= 1.0;
        let roomy = above >= 8.0 && (below - 8.0).abs() <= 1.0;
        assert!(
            centred || roomy,
            "{above} above the toast, {below} to the pane's edge"
        );
        let close = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape)
                    if shape.galley.text() == crate::icons::X
                        && toast.contains_rect(clipped.shape.visual_bounding_rect()) =>
                {
                    Some(clipped.shape.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("the toast's close button");
        let press = |pressed| egui::Event::PointerButton {
            pos: close,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut app, vec![egui::Event::PointerMoved(close)]);
        assert_eq!(
            ctx.layer_id_at(close).map(|layer| layer.order),
            Some(egui::Order::Middle),
            "nothing lies over the toast's close button"
        );
        frame(&mut app, vec![press(true)]);
        frame(&mut app, vec![press(false)]);
        assert!(app.toasts.dismissed("hold"), "{:?}", app.toasts.texts());
    }

    #[test]
    fn a_channel_muted_in_the_apu_tab_stays_muted_after_a_volume_change() {
        let ctx = Context::default();
        let mut app = media_app("apu-mute", LayoutMode::Desktop);
        app.open_tab(Tab::Apu);
        let mut output = desk_frame(&mut app, &ctx, Vec::new());
        for _ in 0..3 {
            output = desk_frame(&mut app, &ctx, Vec::new());
        }
        let second = text_rect(&output, "Pulse 2").expect("the APU tab's Pulse 2 box");

        click_at(&mut app, &ctx, second.center());
        desk_frame(&mut app, &ctx, Vec::new());
        assert!(!app.emulator.gb().apu.channel_enabled(1));

        app.ui.settings.volume = 0.5;
        app.ui.settings.dirty = true;
        desk_frame(&mut app, &ctx, Vec::new());

        assert!(!app.emulator.gb().apu.channel_enabled(1));
        assert_eq!(app.ui.settings.channels, [true, false, true, true]);
    }

    #[test]
    fn a_channel_muted_in_settings_stays_muted_after_loading_a_snapshot() {
        let ctx = Context::default();
        let mut app = media_app("mute-snapshot", LayoutMode::Desktop);
        app.ui.settings.channels = [true, true, false, true];
        app.ui.settings.dirty = true;
        desk_frame(&mut app, &ctx, Vec::new());
        app.emulator.save_snapshot(0).unwrap();

        assert!(app.emulator.load_snapshot(0).unwrap());
        desk_frame(&mut app, &ctx, Vec::new());

        assert!(!app.emulator.gb().apu.channel_enabled(2));
        assert!(app.emulator.gb().apu.channel_enabled(0));
    }

    #[test]
    fn a_controller_button_bound_to_focus_mode_turns_it_on_and_off() {
        let mut app = test_app();
        app.ui
            .settings
            .hotkeys
            .bind_pad(PadButton::Mode, HotkeyAction::ToggleFocusMode);
        app.emulator.set_hotkeys(&app.ui.settings.hotkeys);
        let tap = |app: &mut Citrine| {
            use crate::pad_input::{PadInput, PadKey, fake};
            let mode = PadKey::Known(PadButton::Mode);
            let mut pads = fake(&[(0, PadInput::Pressed(mode)), (0, PadInput::Released(mode))]);
            let Citrine { emulator, ui, .. } = app;
            emulator.handle_pads(&mut pads, &ui.settings.bindings, &mut ui.capture, None);
            app.run_shortcuts();
        };
        tap(&mut app);
        assert!(app.ui.focus_mode);
        assert!(
            app.focus_exit_names()
                .contains(&PadButton::Mode.to_string())
        );
        tap(&mut app);
        assert!(!app.ui.focus_mode);
    }

    #[test]
    fn delete_everything_leaves_the_app_as_a_first_launch_does() {
        let mut app = media_app("delete-everything", LayoutMode::Desktop);
        app.emulator.save_snapshot(0).unwrap();
        let key = app.emulator.rom_key().unwrap().to_owned();
        app.emulator.store.store_battery(&key, b"battery").unwrap();
        let png = crate::utils::screenshot::encode_png(160, 144, &vec![9; 160 * 144 * 4]).unwrap();
        app.media
            .add_screenshot(
                crate::media::NewMedia::current(
                    crate::media::MediaKind::Screenshot,
                    &mut app.emulator,
                ),
                &png,
            )
            .unwrap();
        app.ui.settings.volume = 0.2;
        app.ui.settings.matrix = true;
        app.ui.settings.online.joined = Some("relay.example".into());
        app.ui.settings.current_tab = SettingsTab::Storage;
        app.ui
            .recent
            .record(std::path::Path::new("/roms/game.gb"), "Game", None);
        app.open_tab(Tab::Settings);
        app.mobile.nav.open_settings(SettingsTab::Storage);
        assert!(!app.emulator.store.files().is_empty());

        app.delete_everything();

        let fresh = test_app();
        let tabs = |app: &Citrine| {
            app.dock
                .iter_all_tabs()
                .map(|(_, tab)| *tab)
                .collect::<Vec<_>>()
        };
        assert_eq!(tabs(&app), tabs(&fresh), "the first-launch tabs");
        assert_eq!(app.mobile.nav, mobile::nav::Nav::default());
        assert!(app.emulator.rom_key().is_none(), "no game is loaded");
        assert!(app.emulator.store.files().is_empty(), "no saves");
        assert!(app.media.items().is_empty(), "no media");
        assert!(app.ui.recent.is_empty(), "no recent games");
        let without_account = |settings: &Settings| {
            let mut value = serde_json::to_value(settings).unwrap();
            value.as_object_mut().unwrap().remove("online");
            value
        };
        assert_eq!(
            without_account(&app.ui.settings),
            without_account(&fresh.ui.settings),
            "default settings"
        );
        assert_eq!(
            app.ui.settings.online.joined.as_deref(),
            Some("relay.example"),
            "the online account stays"
        );
    }

    fn partner_frame(
        app: &mut Citrine,
        ctx: &Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
            app.handle_event_queue(touch);
        })
    }

    fn partner_cartridge() -> Vec<u8> {
        let mut rom = citrine_gb_gamedata::fixtures::crystal_cartridge();
        let moves: Vec<u8> = (1..=251u8)
            .flat_map(|id| {
                let pp = match id {
                    1 => 35,
                    2 => 25,
                    3 => 10,
                    _ => 20,
                };
                [id, 0, 40, 0, 100, pp, 0]
            })
            .collect();
        citrine_gb_gamedata::fixtures::put(&mut rom, 0x100000, &moves);
        let base: Vec<u8> = (1..=251u8)
            .flat_map(|dex| {
                let mut record = [50u8; 32];
                record[0] = dex;
                record[13] = 0x1F;
                record[22] = 0;
                record
            })
            .collect();
        citrine_gb_gamedata::fixtures::put(&mut rom, 0x110000, &base);
        rom
    }

    #[test]
    fn a_picked_event_partner_goes_back_to_the_event_list() {
        let ctx = Context::default();
        let mut app = phone_app("partner-back");
        app.ui.settings.layout = LayoutMode::Desktop;
        let rom = partner_cartridge();
        load_game(&mut app, &rom, None);
        let game = citrine_gb_gamedata::identify(&rom).expect("the fixture is a known game");
        let event = citrine_gb_partner::events_for(game)
            .find(|event| event.generation == citrine_gb_gamedata::Generation::Two && !event.egg)
            .expect("a Gold and Silver event");
        app.open_tab(Tab::Playing);
        app.ui.linked.listing = true;
        app.start_partner(event);
        assert!(app.emulator.partnered(), "the partner started");
        let _ = partner_frame(&mut app, &ctx, vec![]);
        let output = partner_frame(&mut app, &ctx, vec![]);
        let back = text_containing(&output, "All events").expect("a way back to the list");
        let _ = partner_frame(&mut app, &ctx, tap(back.center()));
        assert!(app.ui.linked.listing, "the list is open again");

        let _ = partner_frame(&mut app, &ctx, escape());
        assert!(
            !app.ui.linked.listing,
            "Esc returns to the partner it came from"
        );
        let _ = partner_frame(&mut app, &ctx, escape());
        let _ = partner_frame(&mut app, &ctx, vec![]);
        assert!(
            !app.ui.linked.listing,
            "Esc stops on the partner opened from the list"
        );

        app.ui.linked.listing = true;
        app.start_partner(event);
        let _ = partner_frame(&mut app, &ctx, vec![]);
        let _ = partner_frame(&mut app, &ctx, escape());
        assert!(
            app.ui.linked.listing,
            "Esc after a pick goes back to the list"
        );
    }

    #[derive(Default)]
    struct MemoryStorage(std::collections::HashMap<String, String>);

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }

        fn flush(&mut self) {}
    }

    fn stored_with(tabs: Vec<Tab>) -> Vec<Tab> {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || {
                let mut saved = phone_app("stored-tabs");
                saved.dock = DockState::new(tabs);
                let mut storage = MemoryStorage::default();
                eframe::set_value(&mut storage, eframe::APP_KEY, &saved);
                drop(saved);
                let app = Citrine::restore(Some(&storage));
                app.dock.iter_all_tabs().map(|(_, tab)| *tab).collect()
            })
            .expect("thread")
            .join()
            .expect("restored")
    }

    #[test]
    fn a_layout_saved_with_apu_waves_opens_the_apu_tab() {
        let tabs = stored_with(vec![Tab::GameBoy, Tab::ApuWaves, Tab::Settings]);
        assert_eq!(tabs, [Tab::GameBoy, Tab::Apu, Tab::Settings]);
    }

    #[test]
    fn a_layout_saved_with_both_apu_tabs_keeps_one() {
        let tabs = stored_with(vec![Tab::GameBoy, Tab::Apu, Tab::ApuWaves]);
        assert_eq!(tabs, [Tab::GameBoy, Tab::Apu]);
    }

    #[test]
    fn a_gba_game_picked_as_the_second_game_says_what_it_is() {
        let mut app = phone_app("gba-link");
        let mut data = vec![0; 0x200];
        data[0..4].copy_from_slice(&[0x2E, 0x00, 0x00, 0xEA]);
        data[0xB2] = 0x96;
        app.ui.linked.picking = true;
        app.link_picked(crate::utils::file_loader::PickedFile {
            name: "Test.gba".to_string(),
            data: data.into(),
            path: None,
            seen: None,
        });
        assert_eq!(
            app.toasts.texts(),
            ["Game Boy Advance games link only with each other"]
        );
        assert!(!app.emulator.linked());
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_game_picked_to_play_online_goes_to_the_invite() {
        let mut app = phone_app("gba-online");
        let mut data = vec![0; 0x200];
        data[0..4].copy_from_slice(&[0x2E, 0x00, 0x00, 0xEA]);
        data[0xB2] = 0x96;
        let friend = citrine_gb_net::OnlineId::new(7).expect("id");
        app.picked_for(super::tabs::playing::PickFor::Invite(friend), |app| {
            app.handle_load_rom_data(data)
        });
        assert_eq!(app.emulator.kind(), citrine_gb::console::ConsoleKind::Gba);
        assert_eq!(app.toasts.texts(), ["Couldn't send the invite"]);
        let card = app
            .emulator
            .game_file()
            .and_then(|file| file.card())
            .unwrap();
        assert_eq!(card.console, citrine_gb_net::Console::Gba);
    }

    #[test]
    fn the_more_sheet_shows_close_game_on_phone_screens() {
        for (css, pixel_ratio) in [
            (egui::vec2(412.0, 915.0), 2.625),
            (egui::vec2(360.0, 800.0), 3.0),
        ] {
            let ctx = Context::default();
            let mut app = phone_app(&format!("more-{}", css.x));
            let mut rom = vec![0u8; 0x8000];
            rom[0x0147] = 0x03;
            rom[0x0149] = 0x02;
            app.emulator
                .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
                .unwrap();
            app.emulator.save_snapshot(0).unwrap();
            app.ui.snapshots.invalidate();
            app.mobile.nav.show_game();
            app.mobile.nav.set_more(true);
            let output = settle(&mut app, &ctx, css, pixel_ratio);
            let bottom = ctx.content_rect().bottom();
            let close = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Close game" => {
                        let rect = text.visual_bounding_rect();
                        Some((rect.intersect(clipped.clip_rect).height(), rect.bottom()))
                    }
                    _ => None,
                })
                .expect("the sheet draws Close game");
            assert!(
                close.0 > 0.0 && close.1 <= bottom,
                "{css:?}: Close game ends at {} under a screen bottom of {bottom}",
                close.1
            );
        }
    }

    #[test]
    fn a_layout_saved_with_time_control_opens_the_debugger_in_its_place() {
        let tabs = stored_with(vec![Tab::GameBoy, Tab::TimeControl, Tab::Registers]);
        assert_eq!(tabs, [Tab::GameBoy, Tab::Disassembly, Tab::Registers]);
    }

    #[test]
    fn a_layout_saved_with_both_old_debugger_tabs_keeps_one() {
        let tabs = stored_with(vec![Tab::GameBoy, Tab::Disassembly, Tab::TimeControl]);
        assert_eq!(tabs, [Tab::GameBoy, Tab::Disassembly]);
    }

    fn debugger_text_positions(css: egui::Vec2, beside_game: bool) -> (egui::Pos2, egui::Pos2) {
        let ctx = Context::default();
        let mut app = phone_app(&format!("debugger-{}", css.x));
        app.ui.settings.layout = LayoutMode::Desktop;
        load_tobu(&mut app);
        app.dock = DockState::new(vec![Tab::Disassembly]);
        if beside_game {
            app.dock = DockState::new(vec![Tab::GameBoy]);
            app.dock.main_surface_mut().split_right(
                egui_dock::NodeIndex::root(),
                0.6,
                vec![Tab::Disassembly],
            );
        }
        let output = settle(&mut app, &ctx, css, 1.0);
        let find = |wanted: &dyn Fn(&str) -> bool| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if wanted(text.galley.text()) => Some(text.pos),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("text is drawn at {css:?}"))
        };
        (find(&is_code_address), find(&|text| text == "CPU"))
    }

    fn is_code_address(text: &str) -> bool {
        text.len() == 7
            && text.as_bytes()[2] == b':'
            && text
                .chars()
                .filter(|c| *c != ':')
                .all(|c| c.is_ascii_hexdigit())
    }

    fn load_tobu(app: &mut Citrine) {
        let tobu = include_bytes!("../homebrew/tobu.gb");
        app.emulator
            .load_rom(&Rom::new(tobu), citrine_gb::gb::GbModel::Dmg, None)
            .expect("tobu loads");
        app.emulator.gb_mut().debugger.static_analysis_enabled = true;
        for _ in 0..30 {
            app.emulator.gb_mut().run_frame();
        }
    }

    #[test]
    fn a_debugger_beside_the_game_puts_the_cpu_beside_the_code() {
        let (code, cpu) = debugger_text_positions(egui::vec2(760.0, 800.0), false);
        assert!(cpu.x > code.x + 200.0, "code at {code:?}, CPU at {cpu:?}");
    }

    #[test]
    fn a_debugger_in_the_side_dock_puts_the_code_above_the_cpu() {
        let (code, cpu) = debugger_text_positions(egui::vec2(1280.0, 800.0), true);
        assert!(code.y < cpu.y, "code at {code:?}, CPU at {cpu:?}");
    }

    fn docked_code_rows(ui_scale: f32, cpu_open: bool) -> (usize, String) {
        let ctx = Context::default();
        let mut app = phone_app(&format!("code-rows-{ui_scale}-{cpu_open}"));
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.ui_scale = ui_scale;
        app.ui.registers.docked_cpu = cpu_open;
        load_tobu(&mut app);
        app.open_tab(Tab::Disassembly);
        let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
        let texts: Vec<(String, egui::Rect, egui::Rect)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.visual_bounding_rect(),
                    clipped.clip_rect,
                )),
                _ => None,
            })
            .collect();
        let rows: Vec<&(String, egui::Rect, egui::Rect)> = texts
            .iter()
            .filter(|(text, rect, clip)| is_code_address(text) && clip.contains_rect(*rect))
            .collect();
        let layout = format!(
            "screen {:?}, rows {:?}, texts {:?}",
            ctx.content_rect(),
            rows.iter()
                .map(|(_, rect, _)| rect.top())
                .collect::<Vec<_>>(),
            texts
                .iter()
                .filter(|(text, rect, _)| rect.left() > 330.0 && !is_code_address(text))
                .map(|(text, rect, _)| (text.chars().take(12).collect::<String>(), rect.top()))
                .collect::<Vec<_>>()
        );
        (rows.len(), layout)
    }

    #[test]
    fn the_docked_debugger_shows_ten_code_rows_at_web_zoom() {
        for (ui_scale, cpu_open) in [(1.5, false), (1.5, true), (2.2, false)] {
            let (rows, layout) = docked_code_rows(ui_scale, cpu_open);
            assert!(
                rows >= 10,
                "{rows} rows at {ui_scale}, CPU open {cpu_open}: {layout}"
            );
        }
    }

    const DOCK_BEFORE_DEV_TOOLS: &str = include_str!("../fixtures/dock-v0.11.0.ron");

    fn load_dock(text: &str) -> Option<DockState<Tab>> {
        let mut storage = MemoryStorage::default();
        eframe::Storage::set_string(&mut storage, "dock", text.trim().to_owned());
        eframe::get_value(&storage, "dock")
    }

    #[test]
    fn a_dock_layout_saved_before_the_dev_tools_menu_still_loads() {
        let mut dock = load_dock(DOCK_BEFORE_DEV_TOOLS).expect("loads");
        tabs::retire_old_tabs(&mut dock);
        let tabs: Vec<Tab> = dock.iter_all_tabs().map(|(_, tab)| *tab).collect();
        assert!(tabs.contains(&Tab::Disassembly));
        assert!(tabs.contains(&Tab::AudioDebug));
        assert!(!tabs.contains(&Tab::TimeControl));
        assert_eq!(tabs.len(), 19);
    }

    #[test]
    fn a_tool_from_a_newer_build_loads_and_closes() {
        let newer = DOCK_BEFORE_DEV_TOOLS.replace("Performance", "Tool(Hologram)");
        let mut dock = load_dock(&newer).expect("loads");
        assert!(dock.find_tab(&Tab::Tool(tabs::Tool::Unknown)).is_some());
        tabs::retire_old_tabs(&mut dock);
        assert!(dock.find_tab(&Tab::Tool(tabs::Tool::Unknown)).is_none());
        assert!(dock.find_tab(&Tab::Settings).is_some());
    }

    const APP_STATES: [(&str, &str); 2] = [
        ("v0.12.0", include_str!("../fixtures/app-v0.12.0.ron")),
        (
            "v0.13.0-c12",
            include_str!("../fixtures/app-v0.13.0-c12.ron"),
        ),
    ];

    #[test]
    fn app_state_from_each_release_keeps_its_recent_games() {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(|| {
                for (tag, text) in APP_STATES {
                    let mut storage = MemoryStorage::default();
                    eframe::Storage::set_string(&mut storage, eframe::APP_KEY, text.to_owned());
                    let saved = eframe::get_value::<Saved>(&storage, eframe::APP_KEY)
                        .unwrap_or_else(|| panic!("{tag} app state no longer loads"));
                    assert!(
                        saved
                            .ui
                            .recent
                            .entries()
                            .iter()
                            .any(|rom| rom.title == "Pokémon Crystal"),
                        "{tag} keeps its recent games"
                    );
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    fn settings_page(name: &str, page: SettingsTab) -> (Citrine, Context) {
        let ctx = Context::default();
        let mut app = phone_app(name);
        app.mobile.nav.go(Place::Settings);
        app.mobile.nav.open_settings(page);
        play_frame(&mut app, &ctx, vec![]);
        (app, ctx)
    }

    #[test]
    fn every_phone_settings_page_draws_its_rows_with_the_shared_widgets() {
        use crate::app::widgets::settings::page::tests::{after_pass, assert_unified, quiet};
        for page in SettingsTab::iter_with_dev_mode(true) {
            let ctx = Context::default();
            quiet(&ctx);
            let mut app = phone_app(&format!("unified-{page:?}"));
            app.ui.settings.dev_mode = true;
            app.ui.settings.matrix = true;
            app.ui.settings.ghosting = true;
            app.mobile.nav.go(Place::Settings);
            app.mobile.nav.open_settings(page);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 3000.0),
                )),
                ..Default::default()
            };
            let mut laid = Default::default();
            for _ in 0..3 {
                let _ = ctx.run(input.clone(), |ctx| {
                    mobile::touch_spacing(ctx, app.is_mobile(ctx));
                    app.render(ctx);
                });
                laid = after_pass(&ctx);
            }
            let (marks, controls) = laid;
            assert_unified(&format!("phone {page:?}"), &marks, &controls);
        }
    }

    pub(crate) fn phone_settings_output(
        app: &mut Citrine,
        ctx: &Context,
        page: SettingsTab,
    ) -> egui::FullOutput {
        app.mobile.nav.go(Place::Settings);
        app.mobile.nav.open_settings(page);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 5000.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input.clone(), |ctx| {
                mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
            }));
        }
        output.unwrap()
    }

    fn phone_label(row: crate::app::ui_state::settings_tree::rows::RowId) -> &'static str {
        row.label(crate::app::ui_state::settings_tree::Surface::Phone)
    }

    #[test]
    fn the_phone_playing_page_shows_the_fast_forward_speed_above_rewind() {
        use crate::app::ui_state::settings_tree::rows::RowId;
        let ctx = Context::default();
        let mut app = phone_app("fast-forward-row");
        app.ui.settings.fast_forward_speed = crate::emulator::fast_forward::FastForwardSpeed::Ten;
        let output = phone_settings_output(&mut app, &ctx, SettingsTab::Playing);
        let speed_row = text_rect(&output, phone_label(RowId::FastForward)).expect("speed row");
        let speed = text_rect(&output, "10x").expect("speed value");
        let rewind = text_rect(&output, phone_label(RowId::Rewind)).expect("rewind row");
        let autosave = text_rect(&output, phone_label(RowId::AutosaveEvery)).expect("autosave");
        assert!((speed_row.center().y - speed.center().y).abs() < 2.0);
        assert!(speed.top() < rewind.top());
        assert!(rewind.top() < autosave.top());
    }

    #[test]
    fn the_phone_capture_page_shows_the_clip_length() {
        use crate::app::ui_state::settings_tree::rows::RowId;
        let ctx = Context::default();
        let mut app = phone_app("clip-length-row");
        app.ui.settings.clip_length = crate::emulator::rewind::ClipLength::TwoMinutes;
        let output = phone_settings_output(&mut app, &ctx, SettingsTab::Capture);
        let row = text_rect(&output, phone_label(RowId::ClipLength)).expect("clip length row");
        let value = text_rect(&output, "2 min").expect("clip length value");
        assert!((row.center().y - value.center().y).abs() < 2.0);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_phone_console_pages_show_compiled_game_code_under_their_jit_switch() {
        use crate::app::ui_state::settings_tree::rows::RowId;
        if !citrine_gb::nds::machine::JIT_AVAILABLE {
            return;
        }
        let ctx = Context::default();
        let mut app = phone_app("clear-jit-row");
        let clear_label = phone_label(RowId::CompiledCode);
        assert_eq!(phone_label(RowId::GbaCompiledCode), clear_label);
        for page in SettingsTab::iter_with_dev_mode(true) {
            let output = phone_settings_output(&mut app, &ctx, page);
            let clears = output
                .shapes
                .iter()
                .filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text().trim() == clear_label)
                })
                .count();
            if page == SettingsTab::Storage {
                assert_eq!(clears, 1);
                continue;
            }
            let (switch, next) = match page {
                SettingsTab::Ds => (RowId::Jit, Some(RowId::Arm9Bios)),
                SettingsTab::Gba => (RowId::GbaJit, None),
                _ => {
                    assert_eq!(clears, 0, "{page:?}");
                    continue;
                }
            };
            assert_eq!(clears, 1, "{page:?}");
            let switch = text_rect(&output, phone_label(switch)).expect("jit switch");
            let clear = text_rect(&output, clear_label).expect("clear row");
            assert!(switch.top() < clear.top(), "{page:?}");
            if let Some(next) = next {
                let next = text_rect(&output, phone_label(next)).expect("next row");
                assert!(clear.top() < next.top(), "{page:?}");
            }
        }
    }

    #[test]
    fn the_phone_settings_pages_leave_the_game_report_to_this_game() {
        let ctx = Context::default();
        let mut app = phone_app("report-row");
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        for page in [SettingsTab::App, SettingsTab::GameBoy, SettingsTab::Playing] {
            let output = phone_settings_output(&mut app, &ctx, page);
            assert!(
                text_rect(&output, super::report::SAVE_GAME_REPORT).is_none(),
                "{page:?}"
            );
        }
    }

    #[cfg(all(
        feature = "nds",
        not(any(target_arch = "wasm32", target_os = "android"))
    ))]
    #[test]
    fn the_phone_app_page_offers_the_ds_screen_choice_whatever_the_game() {
        use crate::app::ui_state::settings_tree::rows::RowId;
        use crate::discord::ShotScreens;
        let ctx = Context::default();
        let mut app = phone_app("discord-rows");
        let output = phone_settings_output(&mut app, &ctx, SettingsTab::App);
        assert!(text_rect(&output, phone_label(RowId::DiscordActivity)).is_some());
        assert!(text_rect(&output, phone_label(RowId::DiscordShot)).is_none());

        app.ui.settings.discord.enabled = true;
        app.ui.settings.discord.screenshot = true;
        for (screens, shown) in [
            (ShotScreens::Both, "Both"),
            (ShotScreens::Top, "Top"),
            (ShotScreens::Bottom, "Bottom"),
        ] {
            app.ui.settings.discord.screens = screens;
            let output = phone_settings_output(&mut app, &ctx, SettingsTab::App);
            let shot = text_rect(&output, phone_label(RowId::DiscordShot)).expect("screenshot");
            let row =
                text_rect(&output, phone_label(RowId::DiscordDsScreens)).expect("screens row");
            let value = text_rect(&output, shown).expect(shown);
            assert!(shot.top() < row.top());
            assert!((row.center().y - value.center().y).abs() < 4.0, "{shown}");
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn every_phone_ds_3d_row_shows_its_label_and_value_whole_on_one_line() {
        use crate::app::ui_state::ds_3d::{
            DistantTextures, Resolution, SmoothEdges, TextureFilter,
        };
        use strum::IntoEnumIterator;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 3000.0),
            )),
            ..Default::default()
        };
        for (step, distant) in DistantTextures::iter().enumerate() {
            let filter = TextureFilter::iter().cycle().nth(step).unwrap();
            let edges = SmoothEdges::iter().cycle().nth(step).unwrap();
            let resolution = Resolution::iter().next_back().unwrap();
            let ctx = Context::default();
            let mut app = phone_app("ds-3d-one-line");
            app.ui.settings.ds_3d.resolution = resolution;
            app.ui.settings.ds_3d.smooth_edges = edges;
            app.ui.settings.ds_3d.texture_filter = filter;
            app.ui.settings.ds_3d.distant_textures = distant;
            app.mobile.nav.go(Place::Settings);
            app.mobile.nav.open_settings(SettingsTab::Ds);
            let mut output = Default::default();
            for _ in 0..3 {
                output = ctx.run(input.clone(), |ctx| {
                    mobile::touch_spacing(ctx, app.is_mobile(ctx));
                    app.render(ctx);
                });
            }
            let texts: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.clone()),
                    _ => None,
                })
                .collect();
            let shown = [
                "3D resolution".to_owned(),
                "Anti-aliasing".to_owned(),
                "Texture filter".to_owned(),
                "Anisotropic filtering".to_owned(),
                resolution.to_string(),
                edges.to_string(),
                filter.to_string(),
                distant.to_string(),
            ];
            for wanted in shown {
                let text = texts
                    .iter()
                    .find(|text| text.galley.text() == wanted)
                    .unwrap_or_else(|| panic!("{wanted:?} shows with {distant:?}"));
                assert!(!text.galley.elided, "{wanted:?} is cut with {distant:?}");
                assert_eq!(text.galley.rows.len(), 1, "{wanted:?} takes one line");
                assert!(
                    text.visual_bounding_rect().max.x <= 412.0,
                    "{wanted:?} runs off the screen"
                );
            }
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_phone_ds_page_lists_the_screen_rows_with_the_shared_widgets() {
        use crate::app::ui_state::ds_screens::Arrangement;
        use crate::app::ui_state::settings_tree::rows::RowId;
        use crate::app::widgets::settings::page::tests::{after_pass, assert_unified, quiet};
        let ctx = Context::default();
        quiet(&ctx);
        let mut app = phone_app("ds-screens-page");
        app.ui.settings.ds_screens.portrait.arrangement = Arrangement::BigAndSmall;
        app.mobile.nav.go(Place::Settings);
        app.mobile.nav.open_settings(SettingsTab::Ds);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 3000.0),
            )),
            ..Default::default()
        };
        let mut laid = Default::default();
        let mut texts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
            });
            laid = after_pass(&ctx);
            texts = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().trim().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let (marks, controls) = laid;
        assert_unified("phone DS screens", &marks, &controls);
        let mut wanted = vec!["Screens"];
        wanted.extend(
            [
                RowId::SmallCorner,
                RowId::SmallSize,
                RowId::Gap,
                RowId::Rotation,
            ]
            .map(phone_label),
        );
        for row in wanted {
            assert!(texts.iter().any(|text| text == row), "{row}: {texts:?}");
        }
        let tabs = RowId::TwoTabs.label(crate::app::ui_state::settings_tree::Surface::Desktop);
        assert!(!texts.iter().any(|text| text == tabs), "{texts:?}");
    }

    #[test]
    fn every_tree_row_shows_on_its_phone_page() {
        use crate::app::ui_state::settings_tree::rows::RowId;
        use crate::app::ui_state::settings_tree::{SettingsTab, Surface};
        let ctx = Context::default();
        let mut app = phone_app("phone-tree-rows");
        let s = &mut app.ui.settings;
        s.dev_mode = true;
        s.matrix = true;
        s.ghosting = true;
        s.gimmicks.camera = crate::emulator::gimmicks::settings::CameraSource::Picture;
        s.touch.scaling = crate::app::ui_state::settings::ScalingMode::Fit;
        s.discord.enabled = true;
        s.discord.screenshot = true;
        s.ds_3d.resolution = crate::app::ui_state::ds_3d::Resolution::Double;
        s.ds_screens.portrait.arrangement =
            crate::app::ui_state::ds_screens::Arrangement::BigAndSmall;
        s.ds_screens.landscape.arrangement =
            crate::app::ui_state::ds_screens::Arrangement::BigAndSmall;
        s.ds_online.service = crate::app::ui_state::ds_online::OnlineService::Kaeru;
        s.microphone = crate::audio::mic::Microphone::Default;
        s.push_to_talk = true;
        s.gimmicks.camera = crate::emulator::gimmicks::CameraSource::Picture;
        let needs_a_device_or_file = [
            RowId::CameraSees,
            RowId::LightSensor,
            RowId::KeepFirmware,
            RowId::NetworkDevices,
        ];
        let whole_page_widgets = [SettingsTab::Online, SettingsTab::Storage];
        for page in SettingsTab::iter_with_dev_mode(true) {
            if whole_page_widgets.contains(&page) {
                continue;
            }
            let output = phone_settings_output(&mut app, &ctx, page);
            for row in page.rows() {
                if !row.shown_on(Surface::Phone, true) || needs_a_device_or_file.contains(&row) {
                    continue;
                }
                let label = phone_label(row);
                assert!(
                    text_rect(&output, label).is_some(),
                    "{page:?} misses {row:?} ({label:?})"
                );
            }
        }
    }

    #[test]
    fn picking_a_phone_search_hit_opens_the_page_and_scrolls_to_the_row() {
        use crate::app::ui_state::settings_tree::rows::RowId;
        let ctx = Context::default();
        let mut app = phone_app("phone-search-pick");
        app.mobile.nav.go(Place::Settings);
        app.ui.settings.search = "updates".to_owned();
        play_frame(&mut app, &ctx, vec![]);
        let label = phone_label(RowId::CheckUpdates);
        let output = play_frame(&mut app, &ctx, vec![]);
        let hit = text_rect(&output, label).expect("the hit for the update row");
        play_frame(&mut app, &ctx, tap(hit.center()));
        assert_eq!(
            app.mobile.nav.settings_page(),
            Some(RowId::CheckUpdates.page())
        );
        let mut output = play_frame(&mut app, &ctx, vec![]);
        for _ in 0..30 {
            output = play_frame(&mut app, &ctx, vec![]);
        }
        assert!(app.ui.settings.find.is_none());
        let row = text_rect(&output, label).expect("the update row on its page");
        assert!(row.min.y >= 0.0 && row.max.y <= 915.0, "{row:?}");
    }

    fn page_texts(output: &egui::FullOutput, anchor: &str) -> Option<Vec<String>> {
        let clip = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == anchor => {
                    Some(clipped.clip_rect)
                }
                _ => None,
            })?;
        Some(
            output
                .shapes
                .iter()
                .filter(|clipped| clipped.clip_rect == clip)
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().trim().to_owned()),
                    _ => None,
                })
                .filter(|text| !text.is_empty())
                .collect(),
        )
    }

    fn settings_texts(app: &mut Citrine, ctx: &Context) -> Vec<(String, Vec<String>)> {
        use crate::app::ui_state::settings_tree::{SettingsTab, Surface};
        let whole_page_widgets = [SettingsTab::Online, SettingsTab::Storage];
        let mut pages = Vec::new();
        for surface in [Surface::Phone, Surface::Desktop] {
            for page in SettingsTab::iter_with_dev_mode(true) {
                if whole_page_widgets.contains(&page) {
                    continue;
                }
                let Some(anchor) = page
                    .groups_on(surface, true)
                    .map(|group| group.title())
                    .find(|title| !title.is_empty())
                else {
                    continue;
                };
                let output = match surface {
                    Surface::Phone => {
                        app.ui.settings.layout = LayoutMode::Mobile;
                        phone_settings_output(app, ctx, page)
                    }
                    Surface::Desktop => {
                        app.ui.settings.layout = LayoutMode::Desktop;
                        app.ui.settings.current_tab = page;
                        app.open_tab(crate::app::tabs::Tab::Settings);
                        let mut output = info_frame_out(app, ctx, 1280.0, 5000.0);
                        for _ in 0..2 {
                            output = info_frame_out(app, ctx, 1280.0, 5000.0);
                        }
                        output
                    }
                };
                let texts = page_texts(&output, anchor)
                    .unwrap_or_else(|| panic!("{page:?} on {surface:?} misses {anchor:?}"));
                pages.push((format!("{page:?} on {surface:?}"), without_previews(texts)));
            }
        }
        pages
    }

    fn without_previews(texts: Vec<String>) -> Vec<String> {
        let icon = |text: &str| text.starts_with(|c: char| ('\u{e000}'..='\u{f8ff}').contains(&c));
        let mut kept = Vec::new();
        let mut in_preview = false;
        for text in texts {
            if text == "Your Discord profile" {
                in_preview = true;
            } else if in_preview && icon(&text) {
                in_preview = false;
            }
            if !in_preview {
                kept.push(text);
            }
        }
        kept
    }

    fn settings_app(name: &str) -> Citrine {
        let mut app = phone_app(name);
        let s = &mut app.ui.settings;
        s.dev_mode = true;
        s.check_updates = false;
        s.discord.enabled = true;
        s.discord.screenshot = true;
        app
    }

    fn same_text(a: &str, b: &str) -> bool {
        fn stem(text: &str) -> Option<&str> {
            text.strip_suffix('\u{2026}').map(str::trim_end)
        }
        match (stem(a), stem(b)) {
            (None, None) => a == b,
            (Some(cut), None) => b.starts_with(cut),
            (None, Some(cut)) => a.starts_with(cut),
            (Some(x), Some(y)) => x.starts_with(y) || y.starts_with(x),
        }
    }

    #[test]
    fn a_value_cut_shorter_reads_as_the_same_text() {
        assert!(same_text(
            "Integer (sharpe\u{2026}",
            "Integer (sharp\u{2026}"
        ));
        assert!(same_text("Beside the screen", "Beside the scre\u{2026}"));
        assert!(!same_text(
            "Integer (sharpe\u{2026}",
            "Integer (smooth\u{2026}"
        ));
        assert!(!same_text("Beside the screen", "Below the scre\u{2026}"));
        assert!(!same_text("Fit", "Fill"));
    }

    fn assert_same_pages(
        seen: &[(String, Vec<String>)],
        with: &[(String, Vec<String>)],
        game: &str,
    ) {
        assert_eq!(seen.len(), with.len(), "{game}");
        let mut changed = Vec::new();
        for ((page, before), (_, after)) in seen.iter().zip(with) {
            let differs = before.iter().zip(after).position(|(a, b)| !same_text(a, b));
            if differs.is_none() && before.len() == after.len() {
                continue;
            }
            let at = differs.unwrap_or(before.len().min(after.len()));
            let near = |texts: &Vec<String>| {
                texts
                    .iter()
                    .skip(at.saturating_sub(3))
                    .take(8)
                    .cloned()
                    .collect::<Vec<_>>()
            };
            changed.push(format!(
                "{page}: {:?} became {:?}",
                near(before),
                near(after)
            ));
        }
        assert!(changed.is_empty(), "with {game}: {changed:#?}");
    }

    #[test]
    fn settings_pages_are_the_same_with_no_game_gb_gba_and_ds() {
        let ctx = Context::default();
        let mut none = settings_app("same-pages-none");
        let base = settings_texts(&mut none, &ctx);
        assert!(base.len() > 10, "{}", base.len());

        let mut gb = settings_app("same-pages-gb");
        load_game(
            &mut gb,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        let ctx = Context::default();
        assert_same_pages(&base, &settings_texts(&mut gb, &ctx), "a GB game");

        #[cfg(feature = "gba")]
        {
            let mut gba = settings_app("same-pages-gba");
            gba.handle_load_rom_data(crate::app::files::tests::fixture());
            assert_eq!(gba.emulator.kind(), citrine_gb::console::ConsoleKind::Gba);
            let ctx = Context::default();
            assert_same_pages(&base, &settings_texts(&mut gba, &ctx), "a GBA game");
        }

        #[cfg(feature = "nds")]
        {
            let mut ds = settings_app("same-pages-ds");
            ds.handle_load_rom_data(crate::app::files::nds_tests::fixture());
            assert_eq!(ds.emulator.kind(), citrine_gb::console::ConsoleKind::Nds);
            let ctx = Context::default();
            assert_same_pages(&base, &settings_texts(&mut ds, &ctx), "a DS game");
        }
    }

    #[cfg(all(feature = "gba", feature = "nds"))]
    #[test]
    fn settings_pages_stay_the_same_in_a_ds_pair_after_a_gba_game_on_both_seats() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, empty, nickname, one_ds_pair_at_a_time};
        let _one = one_ds_pair_at_a_time();
        let ctx = Context::default();
        let without_sync_pages = |pages: Vec<(String, Vec<String>)>| {
            pages
                .into_iter()
                .filter(|(page, _)| !page.starts_with("Sync on "))
                .collect::<Vec<_>>()
        };
        let mut none = settings_app("pair-pages-none");
        let base = without_sync_pages(settings_texts(&mut none, &ctx));
        for host in [true, false] {
            let mut app = settings_app(&format!("pair-pages-{host}"));
            app.handle_load_rom_data(crate::app::files::tests::fixture());
            app.emulator.running = true;
            let mut pair = Pair::new(true, 0, 0, empty());
            let session = pair.invite(nickname());
            pair.accept(session, MyGame::Own(nickname()));
            pair.play_frames(60);
            pair.hand_over_at_real_time();
            pair.swap_seat(host, &mut app.play, &mut app.online);
            let ctx = Context::default();
            let mut frame = eframe::Frame::_new_kittest();
            for _ in 0..6 {
                pair.run_partner_alone(host);
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(412.0, 900.0),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let _ = ctx.run(input, |ctx| {
                    eframe::App::update(&mut app, ctx, &mut frame);
                });
            }
            let kind = app.emulator.played_kind();
            let together = app.emulator.shows_session();
            let key = app.ui.settings.playing_key().map(str::to_owned);
            let pages = without_sync_pages(settings_texts(&mut app, &ctx));
            pair.swap_seat(host, &mut app.play, &mut app.online);
            let seat = if host {
                "the host seat"
            } else {
                "the guest seat"
            };
            assert!(together, "the game together still shows on {seat}");
            assert_eq!(kind, citrine_gb::console::ConsoleKind::Nds, "{seat}");
            assert_eq!(key, None, "no game scope in a game together on {seat}");
            assert_same_pages(
                &base,
                &pages,
                &format!("a DS pair after a GBA game on {seat}"),
            );
        }
    }

    fn last_text_rect(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
        output
            .shapes
            .iter()
            .rev()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
    }

    #[test]
    fn back_closes_a_settings_sheet_before_the_page() {
        let (mut app, ctx) = settings_page("sheet-back", SettingsTab::Sound);
        let output = play_frame(&mut app, &ctx, vec![]);
        let volume = text_rect(&output, "Volume").expect("a volume row");
        play_frame(&mut app, &ctx, tap(volume.center()));
        play_frame(&mut app, &ctx, vec![]);
        assert!(app.mobile.sheet_open());
        play_frame(&mut app, &ctx, escape());
        play_frame(&mut app, &ctx, vec![]);
        assert!(!app.mobile.sheet_open());
        assert_eq!(app.mobile.nav.settings_page(), Some(SettingsTab::Sound));
    }

    #[test]
    fn a_picked_value_applies_and_closes_the_sheet() {
        let (mut app, ctx) = settings_page("sheet-pick", SettingsTab::App);
        app.ui.settings.ui_theme = crate::app::ui_state::ui_theme::UiTheme::Light;
        let output = play_frame(&mut app, &ctx, vec![]);
        let theme = text_rect(&output, "Theme").expect("a theme row");
        play_frame(&mut app, &ctx, tap(theme.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        let dark = last_text_rect(&output, "Dark").expect("a dark choice");
        play_frame(&mut app, &ctx, tap(dark.center()));
        play_frame(&mut app, &ctx, vec![]);
        assert_eq!(
            app.ui.settings.ui_theme,
            crate::app::ui_state::ui_theme::UiTheme::Dark
        );
        assert!(!app.mobile.sheet_open());
    }

    #[test]
    fn switch_labels_sit_mid_row_and_a_tap_on_them_flips_the_switch() {
        let (mut app, ctx) = settings_page("switch-row", SettingsTab::Controls);
        let size = egui::vec2(412.0, 3000.0);
        let mut output = sized_frame(&mut app, &ctx, size, Vec::new());
        for _ in 0..2 {
            output = sized_frame(&mut app, &ctx, size, Vec::new());
        }
        let show = text_rect(&output, "Show").expect("a show row").center().y;
        let size_row = text_rect(&output, "Size").expect("a size row").center().y;
        let opacity = text_rect(&output, "Opacity")
            .expect("an opacity row")
            .center()
            .y;
        assert!(
            ((opacity - size_row) - (size_row - show)).abs() < 3.0,
            "switch label at {show}, plain rows at {size_row} and {opacity}"
        );
        let before = app.ui.settings.touch.show_controls;
        let label = text_rect(&output, "Show").unwrap();
        sized_frame(&mut app, &ctx, size, tap(label.center()));
        sized_frame(&mut app, &ctx, size, Vec::new());
        assert_ne!(app.ui.settings.touch.show_controls, before);
    }

    fn signed_in_you(name: &str) -> (Citrine, Context) {
        let ctx = Context::default();
        let mut app = phone_app(name);
        remember_account(&mut app);
        app.mobile.nav.go(Place::You);
        play_frame(&mut app, &ctx, vec![]);
        (app, ctx)
    }

    pub(crate) fn remember_account(app: &mut Citrine) {
        let account = citrine_gb_net::Account {
            online_id: citrine_gb_net::OnlineId::new(0x1234_5678).unwrap(),
            display_name: "Clara".into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        };
        app.ui.settings.online.remember(
            &crate::online::address::ServerAddress::parse("relay.example").unwrap(),
            &citrine_gb_net::RelayId::random(),
            "pw",
            &account,
            &Default::default(),
        );
    }

    #[test]
    fn the_relay_id_sheet_fits_on_the_screen() {
        let (mut app, ctx) = signed_in_you("you-key-fits");
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Relay ID").expect("a relay ID row");
        play_frame(&mut app, &ctx, tap(row.center()));
        for _ in 0..3 {
            play_frame(&mut app, &ctx, vec![]);
        }
        let output = play_frame(&mut app, &ctx, vec![]);
        let copy = output
            .shapes
            .iter()
            .rev()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().ends_with("Copy relay ID") => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .expect("a copy button");
        assert!(
            copy.bottom() <= ctx.content_rect().bottom(),
            "the copy button ends at {} under a screen bottom of {}",
            copy.bottom(),
            ctx.content_rect().bottom()
        );
    }

    #[test]
    fn a_sheet_note_starts_under_its_title_and_the_button_label_is_centred() {
        let (mut app, ctx) = signed_in_you("you-key-lines-up");
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Relay ID").expect("a relay ID row");
        play_frame(&mut app, &ctx, tap(row.center()));
        for _ in 0..3 {
            play_frame(&mut app, &ctx, vec![]);
        }
        let output = play_frame(&mut app, &ctx, vec![]);
        let title = last_text_rect(&output, "Relay ID").expect("a sheet title");
        let find = |wanted: fn(&str) -> bool| {
            output
                .shapes
                .iter()
                .rev()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if wanted(text.galley.text()) => {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                })
        };
        let note = find(|text| text.starts_with("Use it to sign in")).expect("a sheet note");
        let copy = find(|text| text.ends_with("Copy relay ID")).expect("a copy button");
        assert!(
            (note.left() - title.left()).abs() < 3.0,
            "the note starts at {} and the title at {}",
            note.left(),
            title.left()
        );
        assert!(
            2.0 * (note.top() - title.bottom()) < copy.top() - note.bottom(),
            "the note sits {} under its title and {} over the button",
            note.top() - title.bottom(),
            copy.top() - note.bottom()
        );
        let middle = ctx.content_rect().center().x;
        assert!(
            (copy.center().x - middle).abs() < 3.0,
            "the button label is centred on {} in a sheet centred on {}",
            copy.center().x,
            middle
        );
    }

    #[test]
    fn signing_out_on_a_phone_asks_in_a_sheet() {
        let (mut app, ctx) = signed_in_you("you-sign-out-sheet");
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Sign out").expect("a sign out row");
        play_frame(&mut app, &ctx, tap(row.center()));
        for _ in 0..3 {
            play_frame(&mut app, &ctx, vec![]);
        }
        let question = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text)
                        if text.galley.text().starts_with("Sign out on this device") =>
                    {
                        Some(text.visual_bounding_rect())
                    }
                    _ => None,
                })
        };
        let output = play_frame(&mut app, &ctx, vec![]);
        let asked = question(&output).expect("a sign out question");
        assert!(
            asked.top() > ctx.content_rect().center().y,
            "the question starts at {}, above the screen middle, so it isn't a bottom sheet",
            asked.top()
        );
        let cancel = last_text_rect(&output, "Cancel").expect("a cancel button");
        play_frame(&mut app, &ctx, tap(cancel.center()));
        for _ in 0..2 {
            play_frame(&mut app, &ctx, vec![]);
        }
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(question(&output).is_none(), "cancel closes the question");
    }

    #[test]
    fn tapping_the_online_id_copies_it() {
        let (mut app, ctx) = signed_in_you("you-copy");
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Online ID").expect("an online id row");
        let output = play_frame(&mut app, &ctx, tap(row.center()));
        let copied = output.platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if !text.is_empty()),
        );
        assert!(copied, "the online id goes to the clipboard");
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, crate::icons::CHECK_FAT).is_some());
        assert!(text_rect(&output, crate::icons::COPY).is_none());
    }

    #[test]
    fn back_closes_the_relay_id_sheet_and_stays_on_you() {
        let (mut app, ctx) = signed_in_you("you-key");
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Relay ID").expect("a relay ID row");
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        assert!(app.mobile.sheet_open());
        play_frame(&mut app, &ctx, escape());
        play_frame(&mut app, &ctx, vec![]);
        assert!(!app.mobile.sheet_open());
        assert_eq!(app.mobile.nav.place(), Place::You);
    }

    #[test]
    fn the_docked_debugger_shows_breakpoints_above_a_visible_cpu_header() {
        use citrine_gb::debug::breakpoints::{Access, Watch};
        use citrine_gb::debug::code::{Address, BlockId};
        for cpu_open in [false, true] {
            let ctx = Context::default();
            let mut app = phone_app(&format!("debugger-stops-{cpu_open}"));
            app.ui.settings.layout = LayoutMode::Desktop;
            app.ui.settings.ui_scale = 2.2;
            app.ui.registers.docked_cpu = cpu_open;
            load_tobu(&mut app);
            let gb = app.emulator.as_gb_mut().unwrap();
            gb.debugger
                .breakpoints
                .insert(Address::at(BlockId::Rom, 0, 0x0150));
            gb.debugger.breakpoints.watch(Watch {
                from: Address::at(BlockId::Wram, 0, 0),
                len: 1,
                access: Access::Write,
            });
            app.open_tab(Tab::Disassembly);
            let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
            let shown = |wanted: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                            let rect = text.visual_bounding_rect();
                            Some((rect, clipped.clip_rect))
                        }
                        _ => None,
                    })
            };
            let (list, list_clip) = shown("Breakpoints").expect("a breakpoint list");
            let (cpu, cpu_clip) = shown("CPU").expect("a CPU header");
            assert!(
                list_clip.contains_rect(list),
                "CPU open {cpu_open}: the list at {list:?} is cut by {list_clip:?}"
            );
            assert!(
                cpu_clip.contains_rect(cpu),
                "CPU open {cpu_open}: the CPU header at {cpu:?} is cut by {cpu_clip:?}"
            );
            assert!(
                list.bottom() <= cpu.top(),
                "CPU open {cpu_open}: {list:?} {cpu:?}"
            );
            for entry in ["00:0150", "00:C000"] {
                let listed = output.shapes.iter().any(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().trim() == entry => {
                        let rect = text.visual_bounding_rect();
                        let side = ctx.style().spacing.interact_size.y;
                        clipped.clip_rect.contains_rect(rect)
                            && clipped.clip_rect.height() >= side - 0.5
                            && rect.top() >= list.bottom()
                            && rect.bottom() <= cpu.top()
                    }
                    _ => false,
                });
                let found: Vec<(egui::Rect, egui::Rect)> = output
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(text) if text.galley.text().trim() == entry => {
                            Some((text.visual_bounding_rect(), clipped.clip_rect))
                        }
                        _ => None,
                    })
                    .collect();
                assert!(
                    listed,
                    "CPU open {cpu_open}: {entry} is not in view: {found:?}, side {}, list {list:?} in {list_clip:?}, cpu {cpu:?} in {cpu_clip:?}, floor {:?}",
                    ctx.style().spacing.interact_size.y,
                    ctx.data(
                        |data| data.get_temp::<(f32, f32)>(egui::Id::new("debugger_stops_height"))
                    )
                );
            }
        }
    }

    fn signed_out_you(name: &str, server: bool) -> (Citrine, Context) {
        let (mut app, ctx) = signed_in_you(name);
        app.ui.settings.online.leave();
        if !server {
            app.ui.settings.online.address.clear();
            app.ui.settings.online.relays.clear();
        }
        play_frame(&mut app, &ctx, vec![]);
        (app, ctx)
    }

    #[test]
    fn signed_out_you_shows_no_account_and_no_avatar() {
        let (mut app, ctx) = signed_out_you("you-signed-out", true);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "Sign in").is_some());
        assert!(text_rect(&output, "Create account").is_some());
        for gone in ["Clara", "C", "Sign out", "Online ID"] {
            assert!(
                text_rect(&output, gone).is_none(),
                "{gone} shows signed out"
            );
        }
    }

    #[test]
    fn signed_out_the_phone_cloud_card_signs_in_right_there() {
        let (mut app, ctx) = signed_out_you("sync-sign-in", true);
        app.open_settings(SettingsTab::Sync, true);
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Sign in").expect("a sign in row in the Cloud card");
        let cloud = text_rect(&output, "Cloud saves").expect("the cloud saves row");
        assert!(
            (row.left() - cloud.left()).abs() <= 1.0,
            "sign in starts at {}, cloud saves at {}",
            row.left(),
            cloud.left()
        );
        assert!(text_containing(&output, "Sign in under").is_none());
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        play_frame(&mut app, &ctx, vec![]);
        assert!(app.mobile.sheet_open(), "the sign in sheet opens");
        assert_eq!(app.ui.settings.current_tab, SettingsTab::Sync);
    }

    #[test]
    fn signed_out_the_desktop_cloud_group_opens_the_sign_in_on_online() {
        let ctx = Context::default();
        let mut app = media_app("sync-sign-in-desk", LayoutMode::Desktop);
        app.ui.settings.online.leave();
        app.open_settings(SettingsTab::Sync, false);
        let mut output = desk_frame(&mut app, &ctx, Vec::new());
        for _ in 0..3 {
            output = desk_frame(&mut app, &ctx, Vec::new());
        }
        let button = text_rect(&output, &format!("{} Sign in", crate::icons::SIGN_IN))
            .expect("a sign in button");
        assert!(text_containing(&output, "Sign in under").is_none());
        let cloud = text_rect(&output, "Cloud saves").expect("the cloud saves row");
        let account = text_rect(&output, "Account").expect("an account row label");
        let rects: Vec<egui::Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => Some(shape.rect),
                _ => None,
            })
            .collect();
        let check = rects
            .iter()
            .filter(|rect| {
                (rect.width() - rect.height()).abs() < 0.5
                    && (8.0..24.0).contains(&rect.width())
                    && (rect.center().y - cloud.center().y).abs() < 8.0
                    && rect.right() <= cloud.left()
                    && rect.left() > cloud.left() - 40.0
            })
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .expect("the cloud saves check box");
        assert!(
            (account.left() - check.left()).abs() <= 1.0,
            "account starts at {}, the cloud saves box at {}",
            account.left(),
            check.left()
        );
        let frame = rects
            .iter()
            .filter(|rect| rect.contains_rect(button) && rect.height() < 40.0)
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("the sign in button's frame");
        assert!(
            frame.left() > account.right(),
            "the button starts at {}, inside the account label ending at {}",
            frame.left(),
            account.right()
        );
        assert!(
            frame.top() > check.bottom(),
            "the button sits under the checks"
        );
        click_at(&mut app, &ctx, button.center());
        desk_frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.ui.settings.current_tab, SettingsTab::Online);
    }

    #[test]
    fn signed_out_you_without_a_server_says_so_in_one_row() {
        let (mut app, ctx) = signed_out_you("you-no-server", false);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "Not set up").is_some());
        assert!(text_rect(&output, "Sign in").is_none());
        let row = text_rect(&output, "Server").expect("a server row");
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(app.mobile.sheet_open());
        assert!(text_rect(&output, "Address").is_some());
    }

    #[test]
    fn the_sign_in_sheet_closes_with_back_and_stays_on_you() {
        let (mut app, ctx) = signed_out_you("you-sign-in-sheet", true);
        for row_label in ["Sign in", "Create account", "Server"] {
            let output = play_frame(&mut app, &ctx, vec![]);
            let row = text_rect(&output, row_label).expect("a signed out row");
            play_frame(&mut app, &ctx, tap(row.center()));
            play_frame(&mut app, &ctx, vec![]);
            assert!(app.mobile.sheet_open(), "{row_label} opens a sheet");
            play_frame(&mut app, &ctx, escape());
            play_frame(&mut app, &ctx, vec![]);
            assert!(
                !app.mobile.sheet_open(),
                "back closes the {row_label} sheet"
            );
            assert_eq!(app.mobile.nav.place(), Place::You);
        }
    }

    fn with_second_account(app: &mut Citrine) {
        let account = citrine_gb_net::Account {
            online_id: citrine_gb_net::OnlineId::new(0x2345_6789).unwrap(),
            display_name: "Zitrone".into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        };
        let relay = crate::online::address::ServerAddress::parse("relay.example").unwrap();
        app.ui.settings.online.leave();
        app.ui.settings.online.remember(
            &relay,
            &citrine_gb_net::RelayId::random(),
            "pw",
            &account,
            &Default::default(),
        );
    }

    #[test]
    fn the_sign_in_sheet_offers_the_saved_accounts_and_signs_in_with_one_tap() {
        let (mut app, ctx) = signed_out_you("you-saved-accounts", true);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(
            text_rect(&output, "Clara").is_none(),
            "no account on the signed out page"
        );
        let row = text_rect(&output, "Sign in").expect("a sign in row");
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        let saved = text_rect(&output, "Clara").expect("the saved account in the sheet");
        play_frame(&mut app, &ctx, tap(saved.center()));
        play_frame(&mut app, &ctx, vec![]);
        assert!(app.ui.settings.online.is_joined());
        assert_eq!(app.ui.settings.online.address, "relay.example");
    }

    #[test]
    fn switch_account_lists_the_accounts_switches_and_adds_one() {
        let (mut app, ctx) = signed_in_you("you-switch");
        with_second_account(&mut app);
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Switch account").expect("a switch account row");
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(app.mobile.sheet_open());
        assert!(text_rect(&output, "Add account").is_some());
        play_frame(&mut app, &ctx, escape());
        play_frame(&mut app, &ctx, vec![]);
        assert!(!app.mobile.sheet_open(), "back closes the accounts sheet");

        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Switch account").unwrap();
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        let other = text_rect(&output, "Clara").expect("the other account");
        play_frame(&mut app, &ctx, tap(other.center()));
        play_frame(&mut app, &ctx, vec![]);
        let joined = app.ui.settings.online.joined_entry().unwrap();
        assert_eq!(joined.display_name.as_deref(), Some("Clara"));
        assert_eq!(joined.others.len(), 1);
        assert!(!app.mobile.sheet_open());

        let output = play_frame(&mut app, &ctx, vec![]);
        let row = text_rect(&output, "Switch account").unwrap();
        play_frame(&mut app, &ctx, tap(row.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        let add = text_rect(&output, "Add account").unwrap();
        play_frame(&mut app, &ctx, tap(add.center()));
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(
            app.ui.settings.online.is_joined(),
            "add account keeps the session"
        );
        assert!(app.mobile.sheet_open());
        assert!(text_rect(&output, "Create account").is_some());
        let sign_in = text_rect(&output, "Sign in").expect("a sign in row");
        play_frame(&mut app, &ctx, tap(sign_in.center()));
        play_frame(&mut app, &ctx, vec![]);
        play_frame(&mut app, &ctx, vec![]);
        assert!(!app.ui.settings.online.is_joined());
        assert!(app.mobile.sheet_open(), "the sign in sheet opens");
        play_frame(&mut app, &ctx, escape());
        play_frame(&mut app, &ctx, vec![]);
        play_frame(&mut app, &ctx, vec![]);
        assert!(!app.mobile.sheet_open());
        let joined = app
            .ui
            .settings
            .online
            .joined_entry()
            .expect("back on the account");
        assert_eq!(joined.display_name.as_deref(), Some("Clara"));
        assert_eq!(app.ui.settings.online.accounts_on("relay.example").len(), 2);
    }

    #[test]
    fn the_online_page_signed_out_shows_the_server_row() {
        let (mut app, ctx) = settings_page("online-signed-out", SettingsTab::Online);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "Not set up").is_some());
        assert!(text_rect(&output, "Relay password").is_none());
    }

    fn phone_online_marks(
        app: &mut Citrine,
    ) -> Vec<(crate::app::widgets::settings::page::Mark, egui::Rect)> {
        use crate::app::widgets::settings::page::tests::{after_pass, quiet};
        let ctx = Context::default();
        quiet(&ctx);
        app.mobile.nav.go(Place::Settings);
        app.mobile.nav.open_settings(SettingsTab::Online);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 3000.0),
            )),
            ..Default::default()
        };
        let mut marks = Vec::new();
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
            });
            marks = after_pass(&ctx).0;
        }
        marks
    }

    #[test]
    fn the_phone_online_page_shows_the_typing_text_under_typing_indicators() {
        let (mut app, ctx) = signed_in_you("typing-text-row");
        app.ui.settings.online.typing_text = "is meowing".into();
        app.mobile.nav.go(Place::Settings);
        app.mobile.nav.open_settings(SettingsTab::Online);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 3000.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input.clone(), |ctx| {
                mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
            }));
        }
        let output = output.unwrap();
        let switch = text_rect(&output, "Typing indicators").expect("typing switch");
        let row = text_rect(&output, "Typing text").expect("typing text row");
        let value = text_rect(&output, "is meowing").expect("typing text value");
        assert!(switch.top() < row.top());
        assert!((row.center().y - value.center().y).abs() < 2.0);
    }

    #[test]
    fn the_phone_online_page_has_no_empty_card() {
        use crate::app::widgets::settings::page::tests::assert_no_empty_cards;
        let (mut app, _) = signed_in_you("online-cards-in");
        assert!(!app.online.is_connected());
        let marks = phone_online_marks(&mut app);
        assert_no_empty_cards("signed in", &marks);
        let mut app = phone_app("online-cards-out");
        let marks = phone_online_marks(&mut app);
        assert_no_empty_cards("signed out", &marks);
    }

    #[test]
    fn the_frame_shows_friends_only_while_signed_in() {
        let (mut app, ctx) = signed_in_you("frame-friends");
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "Friends").is_some());
        app.ui.settings.online.leave();
        play_frame(&mut app, &ctx, vec![]);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "Friends").is_none());
        app.mobile.nav.go(Place::Friends);
        play_frame(&mut app, &ctx, vec![]);
        play_frame(&mut app, &ctx, vec![]);
        assert_eq!(app.mobile.nav.place(), Place::You);
    }

    #[test]
    fn an_offer_names_its_download_size() {
        let offer = crate::update::Offer {
            version: "0.11.0".to_owned(),
            page: String::new(),
            installable: true,
            size: 48_200_000,
        };
        let texts =
            super::widgets::update::texts(&crate::update::Phase::Available(offer), false).unwrap();
        assert_eq!(texts.title, "Citrine 0.11.0 is out");
        assert_eq!(texts.detail, "48.2 MB download");
        assert_eq!(texts.action, Some("Update"));
    }

    #[test]
    fn the_video_tool_shows_every_view_of_a_game_boy_inside_its_tab() {
        use crate::app::widgets::video::VideoTab;
        let ctx = Context::default();
        let mut app = phone_app("video-views");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.ui_scale = 2.2;
        load_tobu(&mut app);
        app.open_tab(Tab::Tool(tabs::Tool::Video));
        app.ui.video.show_hidden = true;
        for tab in VideoTab::ALL {
            app.ui.video.tab = tab;
            let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
            let images: Vec<_> = output
                .shapes
                .iter()
                .filter(|clipped| match &clipped.shape {
                    egui::Shape::Mesh(mesh) => mesh.texture_id != egui::TextureId::default(),
                    _ => false,
                })
                .collect();
            let shows_pictures = tab != VideoTab::Palettes;
            assert_eq!(!images.is_empty(), shows_pictures, "{tab:?}");
            for clipped in images {
                let egui::Shape::Mesh(mesh) = &clipped.shape else {
                    continue;
                };
                let rect = mesh.calc_bounds();
                assert!(
                    rect.left() >= clipped.clip_rect.left() - 0.5
                        && rect.right() <= clipped.clip_rect.right() + 0.5,
                    "{tab:?}: picture {rect:?} is wider than its tab {:?}",
                    clipped.clip_rect
                );
            }
        }
        assert_eq!(app.ui.video.sprites().len(), 40);
        assert_eq!(app.ui.video.maps().len(), 2);
        assert_eq!(app.ui.video.palettes().len(), 2);
        app.ui.video.tab = VideoTab::Sprites;
        let source = app.ui.video.sprites()[0].source;
        app.ui.video.select(source);
        assert_eq!(app.ui.video.tab, VideoTab::Tiles);
        assert_eq!(app.ui.video.selected, Some(source));
        assert_eq!(app.ui.video.back, Some(VideoTab::Sprites));
    }

    #[cfg(feature = "gba")]
    #[test]
    fn the_video_tool_shows_a_gba_with_its_own_sheets_and_sprites() {
        use crate::app::widgets::video::VideoTab;
        let ctx = Context::default();
        let mut app = phone_app("video-gba");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.emulator.load_gba(&[0; 0x200]).expect("a gba loads");
        app.open_tab(Tab::Tool(tabs::Tool::Video));
        for tab in VideoTab::ALL {
            app.ui.video.tab = tab;
            settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
        }
        assert_eq!(app.ui.video.sprites().len(), 128);
        assert_eq!(app.ui.video.palettes()[0].colors.len(), 256);
        app.ui.video.select(app.ui.video.sprites()[0].source);
        settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
        assert_eq!(app.ui.video.sheet, 2);
        assert_eq!(
            app.ui.video.colors,
            citrine_gb::debug::video::SheetColors::Palette { group: 1, row: 0 }
        );
    }

    #[cfg(feature = "gba")]
    mod gba_tools {
        use super::*;
        use citrine_gb::console::Console;
        use citrine_gb::debug::Debug;
        use citrine_gb::debug::code::{Address, BlockId};
        use citrine_gb::debug::step::Step;

        const END_LOOP: u32 = 0x0C;

        fn program() -> Vec<u8> {
            [
                0xE3A0_C000_u32,
                0xEB00_0001,
                0xE3A0_1002,
                0xEAFF_FFFE,
                0xE3A0_2003,
                0xE12F_FF1E,
            ]
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
        }

        fn cart(offset: u32) -> Address {
            Address::gba(BlockId::GamePak, offset)
        }

        const PRINT_HI: [u32; 16] = [
            0xE3A0_0404,
            0xE280_08FF,
            0xE280_0CF6,
            0xE3A0_1CC0,
            0xE281_10DE,
            0xE280_2E18,
            0xE1C2_10B0,
            0xE3A0_3048,
            0xE5C0_3000,
            0xE3A0_3069,
            0xE5C0_3001,
            0xE280_2C01,
            0xE3A0_1C01,
            0xE281_1003,
            0xE1C2_10B0,
            0xEAFF_FFFE,
        ];

        fn words(words: &[u32]) -> Vec<u8> {
            words.iter().flat_map(|word| word.to_le_bytes()).collect()
        }

        pub(super) fn gba_app(name: &str) -> Citrine {
            gba_app_with(name, &program())
        }

        fn gba_app_with(name: &str, rom: &[u8]) -> Citrine {
            let mut app = phone_app(name);
            app.ui.settings.layout = LayoutMode::Desktop;
            app.emulator.load_gba(rom).expect("the program loads");
            app.emulator.debug_mut().set_code_analysis(true);
            app
        }

        fn drawn(output: &egui::FullOutput, wanted: &str) -> Option<(egui::Rect, egui::Rect)> {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().trim() == wanted => {
                        Some((text.visual_bounding_rect(), clipped.clip_rect))
                    }
                    _ => None,
                })
        }

        fn in_view(output: &egui::FullOutput, wanted: &str) -> bool {
            drawn(output, wanted).is_some_and(|(rect, clip)| clip.contains_rect(rect))
        }

        fn pc(app: &Citrine) -> Option<Address> {
            app.emulator.debug().pc()
        }

        #[test]
        fn the_debugger_shows_arm_code_and_every_cpu_bank_of_a_gba_game() {
            let ctx = Context::default();
            let mut app = gba_app("gba-debugger-code");
            app.emulator.debug_mut().run_frame();
            app.ui.registers.docked_cpu = true;
            app.open_tab(Tab::Disassembly);

            let output = settle(&mut app, &ctx, egui::vec2(1280.0, 1440.0), 1.0);

            for wanted in ["08000004", "BL", "R12", "CPSR", "Mode System", "FIQ", "IRQ"] {
                assert!(in_view(&output, wanted), "{wanted} is in view");
            }
            assert!(drawn(&output, "Log").is_none(), "no log before a print");
            assert!(
                drawn(&output, "ARM7TDMI").is_none(),
                "no picker for one CPU"
            );
            let texts: Vec<String> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            assert!(texts.iter().any(|text| text.ends_with(" cycles, ")));
            assert!(!texts.iter().any(|text| text.contains("M-cycle")));
        }

        #[test]
        fn step_over_runs_a_bl_and_step_out_returns_from_it() {
            let mut app = gba_app("gba-debugger-steps");

            app.emulator.step(Step::Instruction);
            assert_eq!(pc(&app), Some(cart(0x04)));
            app.emulator.step(Step::Over);
            assert_eq!(pc(&app), Some(cart(0x08)));

            let mut app = gba_app("gba-debugger-out");
            app.emulator.step(Step::Instruction);
            app.emulator.step(Step::Instruction);
            assert_eq!(pc(&app), Some(cart(0x10)));
            app.emulator.step(Step::Out);
            assert_eq!(pc(&app), Some(cart(0x08)));
        }

        #[test]
        fn a_breakpoint_on_the_end_loop_stops_with_r12_in_view() {
            let ctx = Context::default();
            let mut app = gba_app("gba-debugger-end");
            app.emulator
                .debug_mut()
                .breakpoints()
                .insert(cart(END_LOOP));
            app.emulator.debug_mut().run_frame();
            assert!(app.emulator.debug_mut().take_breakpoint_hit());
            assert_eq!(pc(&app), Some(cart(END_LOOP)));
            app.open_tab(Tab::Disassembly);

            let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);

            assert!(in_view(&output, "R12"));
            assert!(in_view(&output, "0800000C"));
        }

        #[test]
        fn lines_a_game_prints_show_in_the_debugger_log() {
            let ctx = Context::default();
            let mut app = gba_app_with("gba-debugger-log", &words(&PRINT_HI));
            app.emulator.debug_mut().run_frame();
            app.open_tab(Tab::Disassembly);

            for height in [800.0, 1440.0] {
                let output = settle(&mut app, &ctx, egui::vec2(1280.0, height), 1.0);

                for wanted in ["Log", "Info", "Hi"] {
                    assert!(in_view(&output, wanted), "{wanted} is in view at {height}");
                }
            }
        }

        #[test]
        fn four_printed_lines_fit_under_the_code_in_a_short_debugger() {
            use citrine_gb::debug::print::{PrintLevel, PrintLine};
            let ctx = Context::default();
            let mut app = gba_app_with("gba-debugger-log-lines", &words(&PRINT_HI));
            let log = app
                .emulator
                .debug_mut()
                .print_log()
                .expect("a GBA has a log");
            let texts = ["first line", "second line", "third line", "fourth line"];
            for text in texts {
                log.push(PrintLine {
                    level: PrintLevel::Warn,
                    text: text.into(),
                });
            }
            app.open_tab(Tab::Disassembly);

            for (size, ratio) in [
                (egui::vec2(1280.0, 800.0), 2.0),
                (egui::vec2(1280.0, 560.0), 1.0),
            ] {
                let output = settle(&mut app, &ctx, size, ratio);

                let shown: Vec<String> = output
                    .shapes
                    .iter()
                    .filter_map(|clipped| match &clipped.shape {
                        egui::Shape::Text(text)
                            if clipped.clip_rect.contains_rect(text.visual_bounding_rect()) =>
                        {
                            Some(text.galley.text().to_owned())
                        }
                        _ => None,
                    })
                    .collect();
                let newest = if size.y < 600.0 {
                    &texts[1..]
                } else {
                    &texts[..]
                };
                for text in newest {
                    assert!(
                        shown.iter().any(|line| line.ends_with(text)),
                        "{text} is in view at {size:?}"
                    );
                }
                let (clear, clip) = drawn(&output, "Clear").expect("Clear is drawn");
                assert!(
                    clear.right() + 2.0 <= clip.right(),
                    "Clear and its button edge are in view at {size:?}"
                );
            }
        }

        #[test]
        fn the_sound_tool_lists_six_channels_and_mutes_them_from_its_own_setting() {
            use citrine_gb::machine::Machine;
            let ctx = Context::default();
            let mut app = gba_app("gba-sound-tool");
            app.open_tab(Tab::Apu);
            app.ui.settings.gba_channels[5] = false;

            let output = settle(&mut app, &ctx, egui::vec2(1280.0, 1440.0), 1.0);

            for name in ["Pulse 1", "Noise", "Direct Sound A", "Direct Sound B"] {
                assert!(in_view(&output, name), "{name} is in view");
            }
            let Machine::Gba(gba) = app.emulator.debug() else {
                panic!("a GBA is loaded");
            };
            let sound = gba.bus().sound();
            assert!(!sound.channel_enabled(5));
            assert!((0..5).all(|channel| sound.channel_enabled(channel)));
            assert_eq!(app.ui.settings.channels, [true; 4]);
            app.emulator.debug_mut().record_waves(true);
            app.emulator.debug_mut().run_frame();
            assert_eq!(app.emulator.debug().waves().len(), 6);
        }

        #[test]
        fn register_fields_wrap_as_tight_text_lines_on_a_phone() {
            let ctx = Context::default();
            let mut app = gba_app("gba-registers-phone");
            app.ui.settings.layout = LayoutMode::Mobile;
            *app.ui.registers.open_mut("Display") = true;
            app.mobile.open(Tab::Registers);

            let output = settle(&mut app, &ctx, egui::vec2(412.0, 915.0), 1.0);

            assert!(in_view(&output, "DISPCNT"), "DISPCNT is in view");
            let mut tops: Vec<f32> = ["OAM in HBlank", "Forced blank", "BG3", "OBJ window"]
                .iter()
                .map(|name| {
                    drawn(&output, name)
                        .unwrap_or_else(|| panic!("{name} is drawn"))
                        .0
                        .top()
                })
                .collect();
            tops.sort_by(f32::total_cmp);
            tops.dedup_by(|a, b| (*a - *b).abs() < 1.0);
            assert!(tops.len() > 1, "the fields wrap at 412");
            let line = ctx.style().text_styles[&egui::TextStyle::Small].size * 2.0;
            for pair in tops.windows(2) {
                assert!(
                    pair[1] - pair[0] < line,
                    "field lines sit {} apart",
                    pair[1] - pair[0]
                );
            }
        }
    }

    #[test]
    fn the_debugger_lays_out_side_by_side_in_the_right_pane_beside_another_tab() {
        for (scale, pane) in [(1.0, 505.0), (1.113, 452.9)] {
            let ctx = Context::default();
            let mut app = phone_app(&format!("debugger-right-pane-{scale}"));
            app.ui.settings.layout = LayoutMode::Desktop;
            app.ui.settings.ui_scale = scale;
            load_tobu(&mut app);
            app.open_tab(Tab::Disassembly);
            settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);
            let output = settle(&mut app, &ctx, egui::vec2(1280.0, 800.0), 1.0);

            let (_, node, _) = app.dock.find_tab(&Tab::Disassembly).expect("docked");
            let egui_dock::Node::Leaf(leaf) = &app.dock.main_surface()[node] else {
                panic!("the debugger sits in a leaf");
            };
            assert!(leaf.tabs().len() > 1, "the debugger shares its pane");
            assert!((leaf.viewport.width() - pane).abs() < 1.0);
            let right = leaf.viewport.right();
            let af = output.shapes.iter().any(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    text.galley.text() == "AF"
                        && text.pos.x > leaf.viewport.left()
                        && clipped.clip_rect.contains_rect(text.visual_bounding_rect())
                }
                _ => false,
            });
            assert!(af, "the CPU column draws beside the code at {pane}");
            for clipped in &output.shapes {
                if let egui::Shape::Text(text) = &clipped.shape {
                    assert!(
                        text.pos.x <= right,
                        "{:?} starts at {} past the pane's edge {right} at {pane}",
                        text.galley.text(),
                        text.pos.x
                    );
                }
            }
        }
    }

    pub(crate) fn picture_rect(output: &egui::FullOutput) -> Option<egui::Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    Some(clipped.shape.visual_bounding_rect())
                }
                egui::Shape::Rect(rect) if rect.fill_texture_id() != egui::TextureId::default() => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .max_by(|a, b| a.area().total_cmp(&b.area()))
    }

    fn toast_frames(app: &mut Citrine, ctx: &Context, size: egui::Vec2) -> egui::FullOutput {
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut output = None;
        let mut room = None;
        for step in 0..80 {
            output = Some(ctx.run(input(), |ctx| {
                mobile::touch_spacing(ctx, false);
                app.render(ctx);
                app.show_toasts(ctx);
            }));
            let now = crate::app::toasts::strip_room(ctx);
            if step >= 3 && room == Some(now) {
                break;
            }
            room = Some(now);
        }
        output.unwrap()
    }

    #[test]
    fn on_desktop_a_toast_never_moves_the_game_picture_at_any_window_size() {
        let sizes = [
            (1024.0, 768.0),
            (1280.0, 800.0),
            (1920.0, 1080.0),
            (800.0, 600.0),
            (640.0, 480.0),
            (1280.0 / 2.2, 720.0 / 2.2),
            (1024.0 / 2.2, 640.0 / 2.2),
            (1920.0 / 2.2, 1000.0 / 2.2),
        ];
        let progress = crate::progress::Progress::bytes(700 * 1024, 2500 * 1024);
        let start = web_time::Instant::now();
        let later = start + std::time::Duration::from_secs(1);
        for (width, height) in sizes {
            for two_lines in [false, true] {
                for homebrew in [false, true] {
                    let name = format!("toast-{width}-{two_lines}-{homebrew}");
                    let mut app = crystal_app(&name, LayoutMode::Desktop, None);
                    app.emulator.running = true;
                    if homebrew {
                        app.open_tab(Tab::Homebrew);
                    }
                    let ctx = Context::default();
                    app.emulator.clear_frame(&ctx);
                    let bare = toast_frames(&mut app, &ctx, egui::vec2(width, height));
                    let bare = picture_rect(&bare).expect("the game is drawn");
                    if two_lines {
                        for now in [start, later] {
                            app.toasts
                                .load("shelf", "Downloading Hyperspace Roll", progress, now);
                        }
                    }
                    app.toasts.error("Couldn't download Pentris").button(
                        "Try again",
                        events::AppEvent::OpenTab { tab: Tab::Homebrew },
                    );
                    let output = toast_frames(&mut app, &ctx, egui::vec2(width, height));
                    let picture = picture_rect(&output).expect("the game is drawn");
                    assert_eq!(picture, bare, "{name}: the picture moved");
                    let toasts = app.toasts.rects(&ctx);
                    assert_eq!(toasts.len(), 1 + two_lines as usize, "{name}");
                    for toast in toasts {
                        assert!(
                            !toast.contains(picture.center()),
                            "{name}: {toast:?} covers the middle of {picture:?}"
                        );
                        assert!(
                            ctx.content_rect().contains_rect(toast),
                            "{name}: {toast:?} leaves the window"
                        );
                    }
                }
            }
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn on_a_phone_a_toast_covers_no_button_and_no_screen() {
        let games: [(&str, &[u8]); 3] = [
            ("gb", include_bytes!("../homebrew/tobu.gb")),
            ("gba", include_bytes!("../homebrew/gba/pentris.gba.br")),
            ("ds", include_bytes!("../homebrew/nds/pong_nds.nds.br")),
        ];
        let sizes = [
            egui::vec2(412.0, 915.0),
            egui::vec2(360.0, 800.0),
            egui::vec2(915.0, 412.0),
            egui::vec2(1280.0, 800.0),
        ];
        for (console, rom) in games {
            for size in sizes {
                let name = format!("phone-toast-{console}-{}", size.x);
                let portrait = size.x < size.y;
                let mut app = phone_app(&name);
                let rom = if console == "gb" {
                    rom.to_vec()
                } else {
                    crate::storage::decompress(rom).expect("rom unpacks")
                };
                app.handle_load_rom_data(rom);
                app.emulator.running = true;
                let ctx = Context::default();
                app.emulator.clear_frame(&ctx);
                let bare = picture_rects(&toast_frames(&mut app, &ctx, size));
                assert!(!bare.is_empty(), "{name}: the game is drawn");
                app.toasts.success("Saved to 'pentris'");
                app.toasts.error("Couldn't download Pentris").button(
                    "Try again",
                    events::AppEvent::OpenTab { tab: Tab::Homebrew },
                );
                let screens = picture_rects(&toast_frames(&mut app, &ctx, size));
                if !portrait {
                    assert_eq!(screens, bare, "{name}: the picture moved");
                }
                let (free, spare, buttons) = free_of_pad(&mut app, &ctx, size);
                let toasts = app.toasts.rects(&ctx);
                assert_eq!(toasts.len(), 2, "{name}");
                for toast in toasts {
                    let room = spare.filter(|spare| spare.expand(1.0).contains_rect(toast));
                    assert!(
                        free.expand(1.0).contains_rect(toast) || room.is_some(),
                        "{name}: {toast:?} leaves the room between the pad buttons {free:?}"
                    );
                    for screen in &screens {
                        assert!(
                            !toast.contains(screen.center()),
                            "{name}: {toast:?} covers the middle of {screen:?}"
                        );
                    }
                    if portrait {
                        for screen in &screens {
                            assert!(
                                !toast.shrink(1.0).intersects(*screen),
                                "{name}: {toast:?} covers part of {screen:?}"
                            );
                        }
                        let buttons = buttons.expect("a phone in portrait shows the pad");
                        assert!(
                            !toast.shrink(1.0).intersects(buttons),
                            "{name}: {toast:?} covers the buttons {buttons:?}"
                        );
                    }
                    if let Some(eye) = app.mobile.hidden.eye {
                        assert!(
                            !toast.shrink(1.0).intersects(eye),
                            "{name}: {toast:?} covers the eye {eye:?}"
                        );
                    }
                    #[cfg(feature = "nds")]
                    if !portrait {
                        for target in &app.ui.touch_targets {
                            assert!(
                                !toast.shrink(1.0).intersects(*target),
                                "{name}: {toast:?} covers the button {target:?}"
                            );
                        }
                    }
                }
                app.toasts = Toasts::default();
                let after = picture_rects(&toast_frames(&mut app, &ctx, size));
                assert_eq!(
                    after, bare,
                    "{name}: the layout stays changed after the toast"
                );
            }
        }
    }

    #[cfg(feature = "nds")]
    fn picture_rects(output: &egui::FullOutput) -> Vec<egui::Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    Some(clipped.shape.visual_bounding_rect())
                }
                egui::Shape::Rect(rect) if rect.fill_texture_id() != egui::TextureId::default() => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .filter(|rect| rect.area() > 64.0 * 64.0)
            .collect()
    }

    #[cfg(feature = "nds")]
    fn free_of_pad(
        app: &mut Citrine,
        ctx: &Context,
        size: egui::Vec2,
    ) -> (egui::Rect, Option<egui::Rect>, Option<egui::Rect>) {
        let mut free = None;
        let mut spare = None;
        let mut buttons = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            },
            |ctx| {
                mobile::touch_spacing(ctx, false);
                app.ui.touch_bounds = None;
                app.render(ctx);
                free = app.ui.toast_area;
                spare = app.ui.toast_spare;
                buttons = app.ui.touch_bounds;
                app.show_toasts(ctx);
            },
        );
        (
            free.expect("the game page sets the toast room"),
            spare,
            buttons,
        )
    }

    #[test]
    fn on_desktop_the_invite_banner_never_moves_the_game_picture() {
        for zoom in [1.0, 2.2] {
            let name = format!("invite-picture-{zoom}");
            let mut app = crystal_app(&name, LayoutMode::Desktop, None);
            app.emulator.running = true;
            let ctx = Context::default();
            app.emulator.clear_frame(&ctx);
            let heights: Vec<f32> = (0..32)
                .map(|step| (560.0 + step as f32 * 16.0) / zoom)
                .collect();
            let size = |height: f32| egui::vec2(1280.0 / zoom, height);
            let bare: Vec<egui::Rect> = heights
                .iter()
                .map(|&height| {
                    let output = toast_frames(&mut app, &ctx, size(height));
                    picture_rect(&output).expect("the game is drawn")
                })
                .collect();
            app.play.push_invite(crate::play::IncomingInvite {
                session: 1,
                from: citrine_gb_net::OnlineId::new(7).expect("id"),
                game: None,
            });
            for (&height, bare) in heights.iter().zip(bare) {
                let output = toast_frames(&mut app, &ctx, size(height));
                let picture = picture_rect(&output).expect("the game is drawn");
                assert_eq!(picture, bare, "{name} at {height}: the picture moved");
                let banner = ctx
                    .memory(|mem| mem.area_rect(egui::Id::new("ask_banner")))
                    .expect("the banner shows");
                assert!(
                    !banner.contains(picture.center()),
                    "{name} at {height}: {banner:?} covers the middle of {picture:?}"
                );
                assert!(
                    ctx.content_rect().contains_rect(banner),
                    "{name} at {height}: {banner:?} leaves the window"
                );
            }
        }
    }

    #[test]
    fn on_desktop_a_notice_waiting_for_an_answer_never_moves_the_game_picture() {
        for zoom in [1.0, 2.2] {
            let name = format!("waiting-notice-{zoom}");
            let mut app = crystal_app(&name, LayoutMode::Desktop, None);
            app.emulator.running = true;
            let ctx = Context::default();
            app.emulator.clear_frame(&ctx);
            for step in 0..24 {
                let height = (560.0 + step as f32 * 24.0) / zoom;
                let size = egui::vec2(1280.0 / zoom, height);
                let bare = picture_rect(&toast_frames(&mut app, &ctx, size)).expect("drawn");
                app.toasts
                    .info("Mika's saves didn't replace yours.")
                    .button(
                        "Use Mika's, keep mine in Saves",
                        events::AppEvent::OpenTab { tab: Tab::Media },
                    )
                    .or(
                        "Delete Mika's saves",
                        events::AppEvent::OpenTab { tab: Tab::Media },
                    )
                    .needs_action();
                let picture = picture_rect(&toast_frames(&mut app, &ctx, size)).expect("drawn");
                assert_eq!(picture, bare, "{name} at {height}: the picture moved");
                let toasts = app.toasts.rects(&ctx);
                assert_eq!(toasts.len(), 1, "{name} at {height}");
                for toast in toasts {
                    assert!(
                        !toast.contains(picture.center()),
                        "{name} at {height}: {toast:?} covers the middle of {picture:?}"
                    );
                    assert!(
                        ctx.content_rect().contains_rect(toast),
                        "{name} at {height}: {toast:?} leaves the window"
                    );
                    let pane = app.toasts.shown_in().expect("placed in the game pane");
                    let room = pane.bottom() - picture.bottom();
                    assert!(
                        room < toast.height() || !toast.intersects(picture),
                        "{name} at {height}: {toast:?} covers {picture:?} with {room} free below it"
                    );
                }
                app.toasts = Toasts::default();
                let after = picture_rect(&toast_frames(&mut app, &ctx, size)).expect("drawn");
                assert_eq!(after, bare, "{name} at {height}: the picture moved back");
            }
        }
    }

    #[test]
    fn on_desktop_the_notice_and_the_toast_after_use_sit_the_same_gap_above_the_pane_foot() {
        let mut app = crystal_app("same-gap", LayoutMode::Desktop, None);
        app.emulator.running = true;
        let ctx = Context::default();
        app.emulator.clear_frame(&ctx);
        let tab = || events::AppEvent::OpenTab { tab: Tab::Media };
        let mut checked = 0;
        for step in 0..24 {
            let size = egui::vec2(1280.0, 560.0 + step as f32 * 24.0);
            let mut gaps = Vec::new();
            for waiting in [true, false] {
                if waiting {
                    app.toasts
                        .info("Mika's saves didn't replace yours.")
                        .button("Use Mika's, keep mine in Saves", tab())
                        .or("Delete Mika's saves", tab())
                        .needs_action();
                } else {
                    app.toasts
                        .success("You're playing Mika's save. Yours is kept in Saves.")
                        .button("Undo", tab());
                }
                let picture = picture_rect(&toast_frames(&mut app, &ctx, size)).expect("drawn");
                let toast = app.toasts.rects(&ctx)[0];
                let pane = app.toasts.shown_in().expect("placed in the game pane");
                let above = toast.top() - picture.bottom();
                let below = pane.bottom() - toast.bottom();
                if above >= 0.0 && pane.bottom() - picture.bottom() >= toast.height() + 16.0 {
                    gaps.push(below);
                } else if above > 0.0 {
                    assert!((above - below).abs() <= 1.0, "{above} {below} at {size:?}");
                }
                app.toasts = Toasts::default();
            }
            if let [notice, after_use] = gaps[..] {
                checked += 1;
                assert_eq!(notice, after_use, "at {size:?}");
                assert!((notice - 8.0).abs() < 0.5, "{notice} at {size:?}");
            }
        }
        assert!(checked > 0, "no window had room for both under the picture");
    }

    #[test]
    fn on_desktop_the_picture_stays_put_through_the_notice_delete_and_cancel() {
        let frames = |app: &mut Citrine, ctx: &Context, size: egui::Vec2| {
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let mut output = None;
            for _ in 0..4 {
                output = Some(ctx.run(input(), |ctx| {
                    mobile::touch_spacing(ctx, false);
                    app.render(ctx);
                    app.peer_discard_confirm(ctx);
                    app.show_toasts(ctx);
                }));
            }
            picture_rect(&output.expect("ran")).expect("the game is drawn")
        };
        for zoom in [1.0, 2.2] {
            let name = format!("notice-size-{zoom}");
            let mut app = crystal_app(&name, LayoutMode::Desktop, None);
            app.emulator.running = true;
            let ctx = Context::default();
            app.emulator.clear_frame(&ctx);
            for step in 0..6 {
                let height = (560.0 + step as f32 * 96.0) / zoom;
                let size = egui::vec2(1280.0 / zoom, height);
                let bare = frames(&mut app, &ctx, size);
                app.show_peer_notice(crate::emulator::peer_saves::PeerNotice {
                    friend: citrine_gb_net::OnlineId::new(7).expect("id"),
                    divert: crate::emulator::peer_saves::Divert {
                        own: "own".to_owned(),
                        borrowed: "borrowed-mika".to_owned(),
                    },
                    name: Some("Mika".to_owned()),
                });
                let notice = frames(&mut app, &ctx, size);
                assert_eq!(
                    notice, bare,
                    "{name} at {height}: the notice moved the picture"
                );
                let delete = app
                    .toasts
                    .press("Delete Mika's saves")
                    .expect("a delete button");
                app.handle_event(delete, false);
                assert!(app.pending_peer_discard.is_some());
                let asking = frames(&mut app, &ctx, size);
                assert_eq!(
                    asking, notice,
                    "{name} at {height}: the picture moved while asking"
                );
                app.finish_peer_discard(false);
                let back = frames(&mut app, &ctx, size);
                assert_eq!(
                    back, notice,
                    "{name} at {height}: the picture moved after cancel"
                );
                let toasts = app.toasts.rects(&ctx);
                assert_eq!(toasts.len(), 1, "{name} at {height}");
                assert!(
                    !toasts[0].contains(back.center()),
                    "{name} at {height}: {:?} covers the middle of {back:?}",
                    toasts[0]
                );
                let pane = app.toasts.shown_in().expect("placed in the game pane");
                let room = pane.bottom() - back.bottom();
                assert!(
                    room < toasts[0].height() || !toasts[0].intersects(back),
                    "{name} at {height}: {:?} covers {back:?} with {room} free below it",
                    toasts[0]
                );
                app.toasts = Toasts::default();
            }
        }
    }

    fn shows_text(app: &mut Citrine, wanted: &str) -> bool {
        let ctx = Context::default();
        settle(app, &ctx, egui::vec2(412.0, 915.0), 2.0)
        .shapes
        .into_iter()
        .any(|clipped| matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text() == wanted))
    }

    #[test]
    fn more_games_gets_a_heading_only_below_other_games() {
        let mut first = phone_app("more-games-first");
        assert!(
            !shows_text(&mut first, "More games"),
            "a first visit has nothing for More to follow"
        );
        let mut playing = phone_app("more-games-playing");
        load_game(
            &mut playing,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        playing.mobile.nav.go(Place::Games);
        assert!(shows_text(&mut playing, "Now playing"));
        assert!(shows_text(&mut playing, "More games"));
    }

    #[test]
    fn the_resume_label_sits_in_the_middle_of_its_pill() {
        let ctx = Context::default();
        let mut app = phone_app("resume-centred");
        load_game(
            &mut app,
            &citrine_gb_gamedata::fixtures::crystal_cartridge(),
            None,
        );
        app.mobile.nav.go(Place::Games);
        let mut output = play_frame(&mut app, &ctx, vec![]);
        for _ in 0..2 {
            output = play_frame(&mut app, &ctx, vec![]);
        }
        let label =
            text_rect(&output, &format!("{} Resume", crate::icons::PLAY)).expect("the label");
        let fill = crate::app::ui_state::ui_theme::brand(&ctx.style().visuals);
        let pill = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect)
                    if rect.fill == fill && rect.rect.contains(label.center()) =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .expect("the pill");
        assert!(
            (pill.center().x - label.center().x).abs() < 4.0,
            "pill {pill:?}, label {label:?}"
        );
    }

    fn hidden_game(name: &str) -> (Citrine, Context) {
        let ctx = Context::default();
        let mut app = phone_app(name);
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        rom[0x0149] = 0x02;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app.mobile.nav.show_game();
        let mut output = play_frame(&mut app, &ctx, vec![]);
        for _ in 0..2 {
            output = play_frame(&mut app, &ctx, vec![]);
        }
        let hide = text_containing(&output, crate::icons::EYE_SLASH)
            .expect("the strip has the hide button");
        play_frame(&mut app, &ctx, tap(hide.center()));
        (app, ctx)
    }

    fn frames(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
        play_frame(app, ctx, events);
        play_frame(app, ctx, vec![]);
        play_frame(app, ctx, vec![])
    }

    fn escape_frames(app: &mut Citrine, ctx: &Context) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            events: escape(),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            app.render(ctx);
            app.mobile_escape(ctx);
        });
        frames(app, ctx, vec![])
    }

    #[test]
    fn the_eye_slash_hides_the_bar_and_the_strip_and_back_brings_them_back() {
        let (mut app, ctx) = hidden_game("hide-back");
        let output = frames(&mut app, &ctx, vec![]);
        assert!(app.mobile.ui_hidden());
        assert!(text_rect(&output, "Games").is_none());
        assert!(text_containing(&output, crate::icons::EYE_SLASH).is_none());
        assert!(text_containing(&output, crate::icons::EYE).is_some());
        let output = escape_frames(&mut app, &ctx);
        assert!(!app.mobile.ui_hidden());
        assert!(app.mobile.nav.on_game());
        assert!(text_rect(&output, "Games").is_some());
    }

    #[test]
    fn the_eye_button_brings_the_ui_back() {
        let (mut app, ctx) = hidden_game("hide-eye");
        let output = frames(&mut app, &ctx, vec![]);
        let eye = text_containing(&output, crate::icons::EYE).expect("the eye button shows");
        let output = frames(&mut app, &ctx, tap(eye.center()));
        assert!(!app.mobile.ui_hidden());
        assert!(text_rect(&output, "Games").is_some());
    }

    #[test]
    fn a_controller_press_hides_the_touch_buttons_until_a_touch_below_the_picture() {
        let (mut app, ctx) = hidden_game("hide-pad");
        let output = frames(&mut app, &ctx, vec![]);
        let select = text_rect(&output, "SELECT").expect("touch buttons show at first");
        app.emulator.press(
            crate::held::InputSource::PadButtons(0),
            citrine_gb::console::Keys::A,
        );
        app.emulator.release(
            crate::held::InputSource::PadButtons(0),
            citrine_gb::console::Keys::A,
        );
        let output = frames(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "SELECT").is_none());
        assert!(app.mobile.ui_hidden());
        let output = frames(&mut app, &ctx, tap(egui::pos2(30.0, select.center().y)));
        assert!(text_rect(&output, "SELECT").is_some());
    }

    #[test]
    fn a_key_press_hides_the_touch_buttons_too() {
        let (mut app, ctx) = hidden_game("hide-key");
        let output = frames(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "SELECT").is_some());
        let key = |pressed| egui::Event::Key {
            key: egui::Key::ArrowLeft,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        for event in [key(true), key(false)] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 915.0),
                )),
                events: vec![event],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                app.render(ctx);
                let Citrine {
                    emulator, pads, ui, ..
                } = &mut app;
                emulator.handle_input(
                    ctx,
                    pads.as_mut(),
                    &ui.settings.bindings,
                    &mut ui.capture,
                    None,
                );
            });
        }
        let output = frames(&mut app, &ctx, vec![]);
        assert!(text_rect(&output, "SELECT").is_none());
    }

    #[test]
    fn the_select_and_start_labels_fit_their_pills_at_the_biggest_size_in_landscape() {
        let ctx = Context::default();
        let mut app = phone_app("pill-labels");
        app.ui.settings.touch.scale = crate::app::ui_state::touch::TouchSettings::MAX_SCALE;
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        rom[0x0149] = 0x02;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app.mobile.nav.show_game();
        let output = settle(&mut app, &ctx, egui::vec2(640.0, 360.0), 1.0);
        for label in ["SELECT", "START"] {
            let text = text_rect(&output, label).expect("the pill label shows");
            let pill = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.rect.contains(text.center()) => Some(rect.rect),
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("the pill shows behind its label");
            assert!(
                pill.contains_rect(text),
                "{label}: label {text:?} runs past its pill {pill:?}"
            );
        }
    }

    fn sized_frame(
        app: &mut Citrine,
        ctx: &Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            mobile::touch_spacing(ctx, touch);
            app.render(ctx);
        })
    }

    fn game_at(name: &str, size: egui::Vec2, scale: f32, hide: bool) -> (Citrine, Context) {
        let ctx = Context::default();
        let mut app = phone_app(name);
        app.ui.settings.touch.scale = scale;
        app.emulator.scaling = app.ui.settings.touch.scaling;
        app.emulator.enable_matrix = app.ui.settings.matrix;
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        rom[0x0149] = 0x02;
        app.emulator
            .load_rom(&citrine_gb::rom::Rom::new(&rom), GbModel::Dmg, None)
            .unwrap();
        app.mobile.nav.show_game();
        let mut output = sized_frame(&mut app, &ctx, size, vec![]);
        for _ in 0..2 {
            output = sized_frame(&mut app, &ctx, size, vec![]);
        }
        if hide {
            let slash = text_containing(&output, crate::icons::EYE_SLASH)
                .expect("the strip has the hide button");
            sized_frame(&mut app, &ctx, size, tap(slash.center()));
            assert!(app.mobile.ui_hidden());
        }
        (app, ctx)
    }

    fn textured(shape: &egui::Shape) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                Some(mesh.calc_bounds())
            }
            egui::Shape::Rect(rect) if rect.fill_texture_id() != egui::TextureId::default() => {
                Some(rect.rect)
            }
            _ => None,
        }
    }

    fn covering(output: &egui::FullOutput, picture: egui::Rect) -> Vec<egui::Rect> {
        let inside = picture.shrink(0.5);
        output
            .shapes
            .iter()
            .filter(|clipped| textured(&clipped.shape).is_none())
            .map(|clipped| clipped.shape.visual_bounding_rect())
            .filter(|rect| rect.is_positive() && !rect.contains_rect(picture))
            .filter(|rect| rect.intersects(inside))
            .collect()
    }

    fn hidden_picture(name: &str, size: egui::Vec2, scale: f32) -> (egui::Rect, Vec<egui::Rect>) {
        let (mut app, ctx) = game_at(name, size, scale, true);
        let mut output = sized_frame(&mut app, &ctx, size, vec![]);
        for _ in 0..3 {
            output = sized_frame(&mut app, &ctx, size, vec![]);
        }
        assert!(
            text_containing(&output, crate::icons::EYE).is_some(),
            "the eye shows"
        );
        let picture = picture_rect(&output).expect("the game picture shows");
        (picture, covering(&output, picture))
    }

    #[test]
    fn hidden_in_landscape_the_picture_takes_the_full_height_centred_with_nothing_over_it() {
        let size = egui::vec2(640.0, 360.0);
        for scale in [1.0, 1.6] {
            let (picture, over) = hidden_picture("fill-landscape", size, scale);
            assert!(
                (picture.height() - size.y).abs() <= 1.0,
                "Size {scale}: picture {picture:?} is not full height"
            );
            assert!((picture.center().y - size.y / 2.0).abs() <= 1.0);
            assert!((picture.center().x - size.x / 2.0).abs() <= 1.0);
            assert!(over.is_empty(), "Size {scale}: {over:?} cover {picture:?}");
        }
    }

    #[test]
    fn hidden_in_portrait_the_picture_spans_the_full_width_at_the_top_with_nothing_over_it() {
        let size = egui::vec2(360.0, 640.0);
        for scale in [1.0, 1.6] {
            let (picture, over) = hidden_picture("fill-portrait", size, scale);
            assert!(
                (picture.width() - size.x).abs() <= 1.0,
                "Size {scale}: picture {picture:?} is not full width"
            );
            assert!(picture.top().abs() <= 1.0);
            assert!((picture.center().x - size.x / 2.0).abs() <= 1.0);
            assert!(over.is_empty(), "Size {scale}: {over:?} cover {picture:?}");
        }
    }

    #[test]
    fn a_picture_that_cannot_take_the_full_height_sits_centred_shown_or_hidden() {
        let size = egui::vec2(480.0, 400.0);
        for hide in [false, true] {
            for scale in [1.0, 1.6] {
                let (mut app, ctx) = game_at("fill-centred", size, scale, hide);
                let mut output = sized_frame(&mut app, &ctx, size, vec![]);
                for _ in 0..3 {
                    output = sized_frame(&mut app, &ctx, size, vec![]);
                }
                let picture = picture_rect(&output).expect("the game picture shows");
                assert!(picture.height() < size.y - 1.0);
                let top = if hide { 0.0 } else { 48.0 };
                let middle = (top + size.y) / 2.0;
                assert!(
                    (picture.center().y - middle).abs() <= 1.0,
                    "hidden {hide}, Size {scale}: picture {picture:?} not centred on {middle}"
                );
                let over = covering(&output, picture);
                assert!(
                    over.is_empty(),
                    "hidden {hide}, Size {scale}: {over:?} cover {picture:?}"
                );
            }
        }
    }

    #[test]
    fn the_landscape_picture_keeps_its_place_when_a_key_press_hides_the_touch_buttons() {
        let size = egui::vec2(640.0, 360.0);
        let (mut app, ctx) = game_at("fill-keep", size, 1.0, true);
        sized_frame(&mut app, &ctx, size, vec![]);
        let output = sized_frame(&mut app, &ctx, size, vec![]);
        let before = picture_rect(&output).expect("the game picture shows");
        assert!(text_rect(&output, "SELECT").is_some());
        app.emulator.press(
            crate::held::InputSource::PadButtons(0),
            citrine_gb::console::Keys::A,
        );
        app.emulator.release(
            crate::held::InputSource::PadButtons(0),
            citrine_gb::console::Keys::A,
        );
        let mut output = sized_frame(&mut app, &ctx, size, vec![]);
        for _ in 0..2 {
            output = sized_frame(&mut app, &ctx, size, vec![]);
        }
        assert!(text_rect(&output, "SELECT").is_none());
        assert_eq!(
            picture_rect(&output).expect("the game picture shows"),
            before
        );
    }

    fn wheel_over(
        app: &mut Citrine,
        ctx: &Context,
        size: egui::Vec2,
        at: egui::Pos2,
    ) -> egui::FullOutput {
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -40.0),
            modifiers: egui::Modifiers::NONE,
        };
        sized_frame(app, ctx, size, vec![egui::Event::PointerMoved(at)]);
        let mut output = sized_frame(app, ctx, size, vec![wheel]);
        for _ in 0..40 {
            output = sized_frame(app, ctx, size, Vec::new());
        }
        output
    }

    fn settings_scroll_app(name: &str) -> (Citrine, Context) {
        let mut app = media_app(name, LayoutMode::Desktop);
        app.dock = DockState::new(vec![Tab::Settings]);
        app.ui.settings.current_tab = SettingsTab::App;
        (app, Context::default())
    }

    fn bar_and_page(
        app: &mut Citrine,
        ctx: &Context,
        size: egui::Vec2,
        over: Option<egui::Pos2>,
    ) -> (Option<egui::Rect>, Option<egui::Rect>) {
        let general = crate::app::widgets::settings::page_list::text(SettingsTab::Screen);
        let output = match over {
            Some(at) => wheel_over(app, ctx, size, at),
            None => {
                let mut output = sized_frame(app, ctx, size, Vec::new());
                for _ in 0..3 {
                    output = sized_frame(app, ctx, size, Vec::new());
                }
                output
            }
        };
        (text_rect(&output, &general), text_rect(&output, "Theme"))
    }

    #[test]
    fn the_settings_bar_and_page_scroll_on_their_own() {
        let (mut app, ctx) = settings_scroll_app("settings-scroll-page");
        let size = egui::vec2(1280.0, 400.0);
        let (tab, label) = bar_and_page(&mut app, &ctx, size, None);
        let (tab, label) = (tab.expect("the bar's first tab"), label.expect("the label"));
        let (tab_after, label_after) = bar_and_page(&mut app, &ctx, size, Some(label.center()));
        assert_eq!(tab_after.map(|r| r.top()), Some(tab.top()));
        let moved = label_after.map_or(f32::INFINITY, |r| label.top() - r.top());
        assert!(moved > 20.0, "page moved {moved}");

        let (mut app, ctx) = settings_scroll_app("settings-scroll-bar");
        let size = egui::vec2(1280.0, 140.0);
        let (tab, label) = bar_and_page(&mut app, &ctx, size, None);
        let (tab, label) = (tab.expect("the bar's first tab"), label.expect("the label"));
        let (tab_after, label_after) = bar_and_page(&mut app, &ctx, size, Some(tab.center()));
        assert_eq!(label_after.map(|r| r.top()), Some(label.top()));
        let moved = tab_after.map_or(f32::INFINITY, |r| tab.top() - r.top());
        assert!(moved > 20.0, "bar moved {moved}");
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_jit_switch_reaches_a_ds_game_on_the_console_thread() {
        use citrine_gb::nds::machine::JIT_AVAILABLE;

        let mut app = test_app();
        app.ui.settings.check_updates = false;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        app.handle_load_rom_data(std::fs::read(path).expect("the DS fixture"));
        app.emulator.running = true;
        assert!(app.emulator.has_worker());
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let mut play = |app: &mut Citrine| {
            let shown = app.emulator.total_frames;
            for _ in 0..6 {
                let _ = ctx.run(input.clone(), |ctx| {
                    eframe::App::update(app, ctx, &mut frame);
                });
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(app.emulator.total_frames > shown, "the console ran");
        };

        app.ui.settings.ds_jit = false;
        play(&mut app);
        assert!(
            !app.emulator.ds_jit_on(),
            "off goes back to the interpreter"
        );

        app.ui.settings.ds_jit = true;
        play(&mut app);
        assert_eq!(app.emulator.ds_jit_on(), JIT_AVAILABLE);
        eframe::App::on_exit(&mut app);
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    struct DsRun {
        app: Citrine,
        ctx: Context,
        frame: eframe::Frame,
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    impl DsRun {
        fn new(fixture: &str) -> Self {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/nds")
                .join(fixture);
            Self::from_rom(std::fs::read(path).expect("the DS fixture"))
        }

        fn from_rom(rom: Vec<u8>) -> Self {
            let mut app = test_app();
            app.ui.settings.check_updates = false;
            app.handle_load_rom_data(rom);
            app.emulator.running = true;
            assert!(app.emulator.has_worker());
            Self {
                app,
                ctx: Context::default(),
                frame: eframe::Frame::_new_kittest(),
            }
        }

        fn on_pool_threads(mut self) -> Self {
            let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
            self.app.emulator.run_on(&threads);
            self.app.threads = threads;
            self
        }

        fn play(&mut self, updates: usize) {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            };
            let shown = self.app.emulator.total_frames;
            for _ in 0..updates {
                let _ = self.ctx.run(input.clone(), |ctx| {
                    eframe::App::update(&mut self.app, ctx, &mut self.frame);
                });
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(self.app.emulator.total_frames > shown, "the console ran");
        }

        fn nds(&self) -> &citrine_gb::nds::machine::Nds {
            self.app.emulator.as_nds().expect("a DS game")
        }
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    impl Drop for DsRun {
        fn drop(&mut self) {
            eframe::App::on_exit(&mut self.app);
        }
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_jit_compiles_arm_and_thumb_blocks_of_a_ds_game_on_the_console_thread() {
        use citrine_gb::nds::machine::JIT_AVAILABLE;

        let mut run = DsRun::new("blocksds-lighting_cube.nds");
        run.app.ui.settings.ds_jit = false;
        run.play(30);
        let off = run.nds().jit_blocks();
        assert_eq!(off, Default::default(), "no blocks without the JIT");

        run.app.ui.settings.ds_jit = true;
        run.play(60);
        let on = run.nds().jit_blocks();
        if JIT_AVAILABLE {
            assert!(on.arm > 0 && on.thumb > 0, "blocks {on:?}");
        } else {
            assert_eq!(on, Default::default());
        }
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_ds_render_threads_draw_on_the_pool_from_the_console_thread() {
        let mut inline = DsRun::new("blocksds-lighting_cube.nds");
        inline.play(30);
        assert_eq!(
            inline.nds().side_work(),
            Default::default(),
            "an inline app draws on the console thread"
        );

        let mut pooled = DsRun::new("blocksds-lighting_cube.nds").on_pool_threads();
        let painters = pooled.app.threads.pool_size() > 1;
        let mut side = pooled.nds().side_work();
        for _ in 0..20 {
            pooled.play(30);
            side = pooled.nds().side_work();
            if side.lines_3d > 0 && (side.lines_2d > 0) == painters {
                break;
            }
        }
        assert!(
            side.lines_3d > 0 && (side.lines_2d > 0) == painters,
            "pool work {side:?}, 2d painters {painters}"
        );
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_idle_loop_skip_runs_in_a_ds_game_on_the_console_thread() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds/blocksds-console-both-screens.nds");
        let mut rom = std::fs::read(path).expect("the DS fixture");
        for header in [0x20, 0x30] {
            let word = |at: usize| u32::from_le_bytes(rom[at..at + 4].try_into().unwrap()) as usize;
            let entry = word(header) + word(header + 4) - word(header + 8);
            rom[entry..entry + 4].copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        }
        for jit in [false, true] {
            let mut run = DsRun::from_rom(rom.clone());
            run.app.ui.settings.ds_jit = jit;
            run.play(30);
            let skips = run.nds().idle_skips();
            assert!(skips > 0, "jit {jit}: {skips} idle skips");
        }
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn rewind_keeps_a_tape_of_a_ds_game_on_the_console_thread() {
        use crate::emulator::rewind::{ClipLength, RewindLength};

        let mut run = DsRun::new("blocksds-lighting_cube.nds");
        run.app.ui.settings.rewind = RewindLength::Off;
        run.app.ui.settings.clip_length = ClipLength::Off;
        run.app.ui.settings.dirty = true;
        run.play(20);
        assert_eq!(run.app.emulator.rewind_kept(), (0, 0), "off keeps nothing");

        run.app.ui.settings.rewind = RewindLength::default();
        run.app.ui.settings.dirty = true;
        run.play(60);
        let (frames, bytes) = run.app.emulator.rewind_kept();
        assert!(frames > 30, "{frames} frames kept");
        assert!(bytes < 8 << 20, "{bytes} bytes for {frames} frames");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn record_keeps_a_replay_in_media_and_start_over_goes_back_to_the_mark() {
        use crate::app::widgets::record_bar::{RecordBarAction, RecordBarFacts};

        let mut app = media_app("tape-record", LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        app.emulator.running = true;
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let mut play =
            |app: &mut Citrine, updates: usize| run_updates(app, &ctx, &mut frame, updates);
        play(&mut app, 5);
        assert!(app.emulator.can_tape());
        app.emulator.fast_forward = true;
        app.toggle_recording();
        assert!(app.emulator.is_taping());
        assert!(
            !app.emulator.fast_forward,
            "recording turns fast forward off"
        );
        assert!(
            !app.emulator.screen_recorder.is_recording(),
            "no live video"
        );
        let facts = RecordBarFacts::of(&app.emulator, &app.ui, false).expect("the bar shows");
        assert!(facts.start_over);
        assert_eq!(facts.frame_times, None, "the setting is off");
        assert!(!app.ui.frame_log.is_recording());

        play(&mut app, 40);
        let before = app.emulator.tape_recording_secs().unwrap();
        assert!(before > 0.1, "{before} s recorded");
        app.record_bar_action(RecordBarAction::StartOver);
        let after = app.emulator.tape_recording_secs().unwrap();
        assert!(
            after < before,
            "start over clears the timer: {after} s after {before} s"
        );
        assert!(app.emulator.is_taping());

        play(&mut app, 20);
        app.record_bar_action(RecordBarAction::Stop);
        app.handle_event_queue(false);
        assert!(!app.emulator.is_recording());
        assert!(RecordBarFacts::of(&app.emulator, &app.ui, false).is_none());
        let item = app
            .media
            .items()
            .iter()
            .find(|item| item.info.kind == crate::media::MediaKind::Replay)
            .expect("the recording is in Media")
            .clone();
        assert!(item.info.length_ms.is_some_and(|ms| ms > 0));
        assert!(item.thumbnail.is_some(), "the row has a picture");
        assert!(!item.info.frame_times);
        let bytes = std::fs::read(app.media.path(&item.id)).unwrap();
        let replay = citrine_gb_replay::Replay::read(&bytes).unwrap();
        assert!(replay.perf.is_none(), "no frame times without the setting");
        assert!(replay.header.frames > 5, "{} frames", replay.header.frames);
        let length = (replay.header.frames as f64 * 1000.0 / replay.header.frame_rate) as u64;
        assert_eq!(item.info.length_ms, Some(length));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tape_recording(
        name: &str,
        render: crate::media::render::RenderVideo,
    ) -> (Citrine, Context, eframe::Frame, crate::media::MediaId) {
        tape_recording_with(name, include_bytes!("../homebrew/tobu.gb"), |app| {
            app.ui.settings.render_video = render;
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tape_recording_with(
        name: &str,
        rom: &[u8],
        set: impl FnOnce(&mut Citrine),
    ) -> (Citrine, Context, eframe::Frame, crate::media::MediaId) {
        let mut app = media_app(name, LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        set(&mut app);
        load_game(&mut app, rom, None);
        app.emulator.running = true;
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        run_updates(&mut app, &ctx, &mut frame, 5);
        app.toggle_recording();
        assert!(app.emulator.is_taping());
        run_updates(&mut app, &ctx, &mut frame, 30);
        app.toggle_recording();
        if app.ui.settings.record_frame_times {
            app.finish_recordings(&ctx);
        }
        app.handle_event_queue(false);
        let id = app
            .media
            .items()
            .iter()
            .find(|item| item.info.kind == crate::media::MediaKind::Replay)
            .expect("the recording is in Media")
            .id
            .clone();
        (app, ctx, frame, id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn media_item(app: &Citrine, id: &crate::media::MediaId) -> crate::media::MediaItem {
        app.media
            .items()
            .iter()
            .find(|item| &item.id == id)
            .expect("the recording stays in Media")
            .clone()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn until_rendered(
        app: &mut Citrine,
        ctx: &Context,
        frame: &mut eframe::Frame,
        id: &crate::media::MediaId,
    ) -> crate::media::MediaItem {
        for _ in 0..250 {
            run_updates(app, ctx, frame, 2);
            let item = media_item(app, id);
            if item.info.video {
                return item;
            }
        }
        panic!("the video was not rendered");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_stopped_recording_renders_its_video_into_the_same_replay() {
        use crate::media::render::RenderVideo;
        use citrine_gb_replay::Replay;

        let (mut app, ctx, mut frame, id) = tape_recording("render-now", RenderVideo::RightAway);
        let before = std::fs::read(app.media.path(&id)).unwrap();
        let replay = Replay::read(&before).unwrap();
        assert!(replay.video.is_none(), "the tape keeps inputs only");
        let item = until_rendered(&mut app, &ctx, &mut frame, &id);
        let replays = app
            .media
            .items()
            .iter()
            .filter(|item| item.info.kind == crate::media::MediaKind::Replay)
            .count();
        assert_eq!(replays, 1, "the video goes into the same recording");
        let bytes = std::fs::read(app.media.path(&id)).unwrap();
        assert_eq!(item.info.bytes, bytes.len() as u64);
        let rendered = Replay::read(&bytes).unwrap();
        assert_eq!(rendered.header.frames, replay.header.frames);
        assert_eq!(rendered.events, replay.events, "the inputs stay");
        assert!(rendered.thumb.is_some());
        assert!(rendered.video.is_some_and(|video| video.len() > 1000));
        assert!(item.thumbnail.is_some());
        run_updates(&mut app, &ctx, &mut frame, 2);
        assert!(app.ui.media.rendering.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn when_i_ask_the_video_renders_from_its_button_and_stop_cancels_it() {
        use crate::media::render::RenderVideo;

        let (mut app, ctx, mut frame, id) = tape_recording("render-ask", RenderVideo::WhenIAsk);
        run_updates(&mut app, &ctx, &mut frame, 5);
        assert!(
            app.ui.media.rendering.is_none(),
            "nothing renders by itself"
        );
        assert!(!media_item(&app, &id).info.video);

        app.ui.media.render = Some(id.clone());
        app.ui.media.stop_render = Some(id.clone());
        run_updates(&mut app, &ctx, &mut frame, 10);
        assert!(app.ui.media.rendering.is_none(), "stop drops the render");
        assert!(!media_item(&app, &id).info.video);

        app.ui.media.render = Some(id.clone());
        let item = until_rendered(&mut app, &ctx, &mut frame, &id);
        assert!(item.info.length_ms.is_some_and(|ms| ms > 0));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[track_caller]
    fn update_until(
        app: &mut Citrine,
        ctx: &Context,
        frame: &mut eframe::Frame,
        mut done: impl FnMut(&Citrine) -> bool,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(60) {
            if done(app) {
                return;
            }
            let _ = ctx.run(input.clone(), |ctx| {
                eframe::App::update(app, ctx, frame);
            });
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the app never got there");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn playing_replay(name: &str) -> (Citrine, Context, eframe::Frame, crate::media::MediaId) {
        use crate::media::render::RenderVideo;
        let (mut app, ctx, mut frame, id) = tape_recording(name, RenderVideo::WhenIAsk);
        app.ui.media.play = Some(id.clone());
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.is_playing_tape()
        });
        (app, ctx, frame, id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn tape_at(app: &Citrine) -> crate::emulator::tape_play::TapeFacts {
        app.emulator.tape_facts().expect("a replay plays")
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn play_opens_the_replay_and_close_leaves_the_live_game_as_it_was() {
        use crate::app::widgets::replay_bar::{ReplayBarAction, ReplayBarFacts};
        use crate::media::render::RenderVideo;

        let (mut app, ctx, mut frame, id) = tape_recording("replay-play", RenderVideo::WhenIAsk);
        app.ui.media.play = Some(id);
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.is_playing_tape()
        });
        assert!(app.replay_show.is_open());
        let live = app.replay_show.live_mut().expect("the live game waits");
        let live_key = live.rom_key().map(str::to_owned);
        let live_hash = live.core_mut().machine.state_hash();
        let live_frames = live.total_frames;
        assert_eq!(app.emulator.rom_key(), None, "the replay saves nothing");
        assert!(!app.ui.viewer.is_open());
        update_until(&mut app, &ctx, &mut frame, |app| tape_at(app).played > 10);
        let facts = ReplayBarFacts::of(&app.emulator, false).expect("the bar shows");
        assert!(facts.tape.running);
        assert!(facts.tape.frames > facts.tape.played);
        assert!(!facts.tape.diverged);

        app.replay_bar_action(ReplayBarAction::Close);
        assert!(!app.replay_show.is_open());
        assert!(!app.emulator.is_playing_tape());
        assert_eq!(app.emulator.rom_key().map(str::to_owned), live_key);
        assert_eq!(app.emulator.total_frames, live_frames);
        assert_eq!(
            app.emulator.core_mut().machine.state_hash(),
            live_hash,
            "the live game did not move"
        );
        run_updates(&mut app, &ctx, &mut frame, 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_replay_pauses_at_its_end_and_seeks_back_and_forth_to_the_frame() {
        use crate::app::widgets::replay_bar::ReplayBarAction;

        let (mut app, ctx, mut frame, _) = playing_replay("replay-seek");
        update_until(&mut app, &ctx, &mut frame, |app| {
            let tape = tape_at(app);
            tape.played >= tape.frames
        });
        run_updates_quiet(&mut app, &ctx, &mut frame, 3);
        let end = tape_at(&app);
        assert!(!end.running, "the end pauses");
        assert!(!app.emulator.running);
        assert_eq!(end.played, end.frames);

        app.replay_bar_action(ReplayBarAction::Seek(10));
        update_until(&mut app, &ctx, &mut frame, |app| {
            tape_at(app).seeking.is_none()
        });
        let back = tape_at(&app);
        assert_eq!(back.played, 10);
        assert!(!back.running, "a seek keeps the pause");

        let ahead = end.frames - 5;
        app.replay_bar_action(ReplayBarAction::Seek(ahead));
        update_until(&mut app, &ctx, &mut frame, |app| {
            tape_at(app).seeking.is_none()
        });
        assert_eq!(tape_at(&app).played, ahead);
        assert!(!tape_at(&app).running);

        app.replay_bar_action(ReplayBarAction::Toggle);
        assert!(tape_at(&app).running);
        update_until(&mut app, &ctx, &mut frame, |app| !tape_at(app).running);
        assert_eq!(tape_at(&app).played, end.frames);

        app.replay_bar_action(ReplayBarAction::Toggle);
        let again = tape_at(&app);
        assert!(again.running, "play at the end starts over");
        assert!(again.played < 10 || again.seeking.is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_replay_whose_game_is_gone_opens_its_video() {
        use crate::media::render::RenderVideo;

        let mut gone = include_bytes!("../homebrew/tobu.gb").to_vec();
        let last = gone.len() - 1;
        gone[last] ^= 0xff;
        let (mut app, ctx, mut frame, id) = tape_recording_with("replay-no-game", &gone, |app| {
            app.ui.settings.render_video = RenderVideo::WhenIAsk;
        });
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        load_game(&mut app, &rom, None);
        app.video_renders.forget_tape_rom();
        app.ui.media.play = Some(id);
        update_until(&mut app, &ctx, &mut frame, |app| app.ui.viewer.is_open());
        assert!(!app.emulator.is_playing_tape());
        assert!(!app.replay_show.is_open());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_replay_with_frame_times_shows_them_over_its_seek_bar() {
        use crate::app::widgets::replay_graph::seek_span_id;
        use crate::media::render::RenderVideo;

        let tobu = include_bytes!("../homebrew/tobu.gb");
        let (mut app, ctx, mut frame, id) = tape_recording_with("replay-graph", tobu, |app| {
            app.ui.settings.render_video = RenderVideo::WhenIAsk;
            app.ui.settings.record_frame_times = true;
        });
        app.ui.media.play = Some(id);
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.is_playing_tape()
        });
        run_updates_quiet(&mut app, &ctx, &mut frame, 3);
        let tape = tape_at(&app);
        let times = app
            .emulator
            .tape_times()
            .expect("the replay has frame times");
        assert!(times.game.len() >= 10, "{}", times.game.len());
        assert!(
            times
                .game
                .iter()
                .all(|(x, _)| (1..=tape.frames).contains(x))
        );
        assert!(times.game.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(times.budget_ms > 0.0);
        let span = ctx
            .data(|d| d.get_temp::<egui::Rangef>(seek_span_id()))
            .expect("the seek bar is drawn");
        assert!(span.min > 0.0 && span.max < 1280.0, "{span:?}");

        app.replay_bar_action(crate::app::widgets::replay_bar::ReplayBarAction::Close);
        assert!(app.emulator.tape_times().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_replay_leaves_pause_rewind_and_saves_out_of_the_toolbar() {
        use crate::app::toolbar::Item;
        use crate::app::widgets::replay_bar::ReplayBarAction;

        let (mut app, _ctx, _frame, _) = playing_replay("replay-toolbar");
        let items = app.items();
        for gone in [Item::Pause, Item::Rewind, Item::Saves] {
            assert!(!items.contains(&gone), "{gone:?}");
        }
        assert!(items.contains(&Item::FastForward));
        app.replay_bar_action(ReplayBarAction::Close);
        let items = app.items();
        assert!(items.contains(&Item::Pause));
        assert!(items.contains(&Item::Saves));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_replay_without_frame_times_has_no_graph() {
        let (mut app, ctx, mut frame, _) = playing_replay("replay-no-graph");
        run_updates_quiet(&mut app, &ctx, &mut frame, 2);
        assert!(app.emulator.tape_times().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_phone_replay_draws_the_game_below_its_bar() {
        let (mut app, ctx, mut frame, _) = playing_replay("replay-phone-game");
        app.ui.settings.layout = LayoutMode::Mobile;
        app.mobile.nav.show_game();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                eframe::App::update(&mut app, ctx, &mut frame);
            });
        }
        assert!(app.emulator.is_playing_tape());
        let picture = app
            .emulator
            .placed_screens()
            .iter()
            .copied()
            .reduce(egui::Rect::union)
            .expect("the game shows");
        assert!(picture.right() <= 412.0, "{picture:?}");
        assert!(picture.height() > 300.0, "{picture:?}");
        let (bar, room) = app.ui.replay_room.expect("the replay bar shows");
        let above = picture.top() - bar;
        let below = room.bottom() - picture.bottom();
        assert!(above > 0.0 && below > 0.0, "{above} {below}");
        assert!((above - below).abs() <= 0.5, "{above} {below}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn live_game(name: &str) -> (Citrine, Context, eframe::Frame) {
        let mut app = media_app(name, LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        app.emulator.running = true;
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.rewind_kept().0 > 150
        });
        (app, ctx, frame)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_clip(app: &mut Citrine, ctx: &Context, frame: &mut eframe::Frame) {
        app.save_a_clip();
        update_until(app, ctx, frame, |app| {
            app.ui
                .clip
                .as_ref()
                .is_some_and(|view| !view.pictures.is_empty())
                && app
                    .emulator
                    .tape_facts()
                    .is_some_and(|tape| tape.seeking.is_none())
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn replays_in_media(app: &Citrine) -> Vec<crate::media::MediaItem> {
        app.media
            .items()
            .iter()
            .filter(|item| item.info.kind == crate::media::MediaKind::Replay)
            .cloned()
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn save_a_clip_opens_the_last_seconds_paused_at_the_end_and_cancel_goes_back() {
        use crate::app::widgets::clip_panel::ClipAction;

        let (mut app, ctx, mut frame) = live_game("clip-open");
        assert_eq!(app.clip_blocked(), None);
        open_clip(&mut app, &ctx, &mut frame);
        assert!(app.replay_show.is_open());
        let live = app.replay_show.live_mut().expect("the live game waits");
        let live_hash = live.core_mut().machine.state_hash();
        let live_frames = live.total_frames;
        let tape = tape_at(&app);
        assert!(tape.frames >= 60, "{} frames", tape.frames);
        assert_eq!(tape.played, tape.frames, "the clip opens at its end");
        assert!(!tape.running, "the clip opens paused");
        let view = app.ui.clip.as_ref().unwrap();
        assert!(view.live);
        assert_eq!((view.from, view.to), (0, tape.frames));
        let facts = view.facts(&app.emulator, false).expect("the panel shows");
        assert_eq!(facts.at, tape.frames);
        run_updates_quiet(&mut app, &ctx, &mut frame, 3);
        assert!(replays_in_media(&app).is_empty(), "nothing saved yet");

        app.clip_action(ClipAction::Cancel);
        assert!(app.ui.clip.is_none());
        assert!(!app.replay_show.is_open());
        assert!(!app.emulator.is_playing_tape());
        assert!(app.emulator.running, "the live game runs again");
        assert_eq!(app.emulator.total_frames, live_frames);
        assert_eq!(app.emulator.core_mut().machine.state_hash(), live_hash);
        assert!(replays_in_media(&app).is_empty(), "cancel keeps nothing");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_clip_saves_its_picked_range_as_a_replay_that_plays() {
        use crate::app::widgets::clip_panel::ClipAction;
        use citrine_gb_replay::Replay;

        let (mut app, ctx, mut frame) = live_game("clip-save");
        app.emulator.running = false;
        run_updates_quiet(&mut app, &ctx, &mut frame, 2);
        open_clip(&mut app, &ctx, &mut frame);
        let frames = tape_at(&app).frames;
        let (from, to) = (12, frames - 8);
        app.clip_action(ClipAction::Range(from, frames));
        update_until(&mut app, &ctx, &mut frame, |app| {
            tape_at(app).seeking.is_none()
        });
        assert_eq!(tape_at(&app).played, from, "a moved start shows its frame");
        app.clip_action(ClipAction::Range(from, to));
        update_until(&mut app, &ctx, &mut frame, |app| {
            tape_at(app).seeking.is_none()
        });
        assert_eq!(tape_at(&app).played, to, "a moved end shows its frame");

        app.clip_action(ClipAction::Save);
        assert!(
            app.ui.clip.as_ref().is_some_and(|view| view.saving),
            "the save starts: {:?}",
            app.toasts.texts()
        );
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.ui.clip.as_ref().is_none_or(|view| !view.saving)
        });
        assert!(app.ui.clip.is_none(), "{:?}", app.toasts.texts());
        app.handle_event_queue(false);
        assert!(!app.replay_show.is_open());
        assert!(!app.emulator.running, "a paused game stays paused");
        let saved = replays_in_media(&app);
        assert_eq!(saved.len(), 1);
        assert!(saved[0].thumbnail.is_some(), "the row has a picture");
        let replay = Replay::read(&std::fs::read(app.media.path(&saved[0].id)).unwrap()).unwrap();
        assert_eq!(replay.header.frames, to - from);

        app.ui.media.play = Some(saved[0].id.clone());
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.is_playing_tape()
        });
        for _ in 0..2000 {
            let tape = tape_at(&app);
            if tape.played >= tape.frames {
                break;
            }
            run_updates_quiet(&mut app, &ctx, &mut frame, 1);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let tape = tape_at(&app);
        assert!(tape.played >= tape.frames, "{tape:?}");
        assert!(!tape.diverged, "the clip plays as it was played");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_media_replay_crops_into_a_new_replay_and_keeps_the_original() {
        use crate::app::widgets::clip_panel::ClipAction;
        use citrine_gb_replay::Replay;

        let (mut app, ctx, mut frame, id) = playing_replay("clip-crop");
        let frames = tape_at(&app).frames;
        app.replay_bar_action(crate::app::widgets::replay_bar::ReplayBarAction::Clip);
        let view = app.ui.clip.as_ref().expect("the crop panel opens");
        assert!(!view.live);
        assert_eq!((view.from, view.to), (0, frames));
        app.clip_action(ClipAction::Range(5, frames - 5));
        update_until(&mut app, &ctx, &mut frame, |app| {
            tape_at(app).seeking.is_none()
        });
        app.clip_action(ClipAction::Save);
        update_until(&mut app, &ctx, &mut frame, |app| app.ui.clip.is_none());
        app.handle_event_queue(false);
        assert!(app.replay_show.is_open(), "the replay stays open");
        let saved = replays_in_media(&app);
        assert_eq!(saved.len(), 2);
        assert!(saved.iter().any(|item| item.id == id), "the original stays");
        let crop = saved.iter().find(|item| item.id != id).unwrap();
        let replay = Replay::read(&std::fs::read(app.media.path(&crop.id)).unwrap()).unwrap();
        assert_eq!(replay.header.frames, frames - 10);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_crop_keeps_the_frame_times_of_its_range() {
        use crate::app::widgets::clip_panel::ClipAction;
        use crate::frame_log::TapeTimes;
        use crate::media::render::RenderVideo;
        use citrine_gb_replay::Replay;

        let rom = include_bytes!("../homebrew/tobu.gb");
        let (mut app, ctx, mut frame, id) = tape_recording_with("clip-crop-times", rom, |app| {
            app.ui.settings.render_video = RenderVideo::WhenIAsk;
            app.ui.settings.record_frame_times = true;
        });
        let rows = |replay: &Replay| {
            let perf = replay.perf.as_ref().expect("frame times");
            let info: serde_json::Value = serde_json::from_slice(&perf.info).unwrap();
            info["recording"]["rows"].as_u64().unwrap()
        };
        let original = Replay::read(&std::fs::read(app.media.path(&id)).unwrap()).unwrap();
        app.ui.media.play = Some(id.clone());
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.is_playing_tape()
        });
        let frames = tape_at(&app).frames;
        app.replay_bar_action(crate::app::widgets::replay_bar::ReplayBarAction::Clip);
        app.clip_action(ClipAction::Range(5, frames - 5));
        update_until(&mut app, &ctx, &mut frame, |app| {
            tape_at(app).seeking.is_none()
        });
        app.clip_action(ClipAction::Save);
        update_until(&mut app, &ctx, &mut frame, |app| app.ui.clip.is_none());
        app.handle_event_queue(false);
        let saved = replays_in_media(&app);
        let crop = saved.iter().find(|item| item.id != id).expect("the crop");
        assert!(crop.info.frame_times, "the crop's row has frame times");
        let replay = Replay::read(&std::fs::read(app.media.path(&crop.id)).unwrap()).unwrap();
        let csv = std::str::from_utf8(&replay.perf.as_ref().expect("frame times").frames).unwrap();
        let times = TapeTimes::read(csv).expect("the crop's frame times follow its tape");
        assert!(
            times.game.len() > 5,
            "{} frames with times",
            times.game.len()
        );
        assert!(
            times.game.iter().all(|(at, _)| *at <= frames - 10),
            "{times:?}"
        );
        assert!(rows(&replay) < rows(&original));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn clip_length_off_turns_clips_off_and_says_where_to_turn_them_on() {
        use crate::emulator::rewind::ClipLength;

        let (mut app, ctx, mut frame) = live_game("clip-off");
        app.ui.settings.clip_length = ClipLength::Off;
        app.ui.settings.dirty = true;
        run_updates(&mut app, &ctx, &mut frame, 3);
        assert_eq!(app.clip_blocked().as_deref(), Some(super::clip::CLIPS_OFF));
        assert!(super::clip::CLIPS_OFF.contains("Settings, Capture, Clip length"));
        app.save_a_clip();
        run_updates(&mut app, &ctx, &mut frame, 3);
        assert!(app.ui.clip.is_none());
        assert!(!app.replay_show.is_open());
        assert!(app.emulator.running);
        assert!(
            app.toasts.texts().contains(&super::clip::CLIPS_OFF),
            "{:?}",
            app.toasts.texts()
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rewind_off_still_saves_a_clip_of_the_last_seconds() {
        use crate::emulator::rewind::RewindLength;

        let mut app = media_app("clip-no-rewind", LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        app.ui.settings.rewind = RewindLength::Off;
        app.ui.settings.dirty = true;
        app.emulator.running = true;
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        update_until(&mut app, &ctx, &mut frame, |app| {
            app.emulator.rewind_kept().0 > 150
        });
        assert!(!app.emulator.can_rewind());
        assert_eq!(app.clip_blocked(), None);
        open_clip(&mut app, &ctx, &mut frame);
        assert!(app.replay_show.is_open());
        let tape = tape_at(&app);
        assert!(tape.frames > 150, "{} frames", tape.frames);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn play_together_shows_the_replays_video() {
        use crate::media::render::RenderVideo;

        let (mut app, _ctx, _frame, id) = tape_recording("replay-together", RenderVideo::WhenIAsk);
        app.emulator.together = true;
        app.ui.media.play = Some(id);
        app.tick_replay_show();
        assert!(app.ui.viewer.is_open());
        assert!(!app.emulator.is_playing_tape());
        assert!(!app.replay_show.is_open());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn hardcore_does_not_play_replays() {
        use crate::media::render::RenderVideo;

        let (mut app, _ctx, _frame, id) = tape_recording("replay-hardcore", RenderVideo::WhenIAsk);
        app.emulator.follow_hardcore(true);
        app.ui.media.play = Some(id);
        for _ in 0..20 {
            app.tick_replay_show();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!app.emulator.is_playing_tape());
        assert!(!app.replay_show.is_open());
        assert!(app.ui.media.play.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_updates_quiet(
        app: &mut Citrine,
        ctx: &Context,
        frame: &mut eframe::Frame,
        updates: usize,
    ) {
        let mut left = updates;
        update_until(app, ctx, frame, move |_| {
            let done = left == 0;
            left = left.saturating_sub(1);
            done
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_updates(app: &mut Citrine, ctx: &Context, frame: &mut eframe::Frame, updates: usize) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let shown = app.emulator.total_frames;
        for _ in 0..updates {
            let _ = ctx.run(input.clone(), |ctx| {
                eframe::App::update(app, ctx, frame);
            });
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(app.emulator.total_frames > shown, "the console ran");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn putting_in_another_game_saves_the_recording_so_far() {
        use citrine_gb::console::Console as _;
        let mut app = media_app("tape-swap", LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        app.ui.settings.record_frame_times = true;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        app.emulator.running = true;
        let game = app.emulator.game_name().to_string();
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        run_updates(&mut app, &ctx, &mut frame, 3);
        app.toggle_recording();
        assert!(app.emulator.is_taping());
        run_updates(&mut app, &ctx, &mut frame, 20);
        let crc = format!("{:08x}", app.emulator.core_mut().machine.rom_id().crc32);
        let mut rom = vec![0u8; 0x8000];
        rom[0x0147] = 0x03;
        load_game(&mut app, &rom, None);
        app.finish_recordings(&ctx);
        app.handle_event_queue(false);
        let item = app
            .media
            .items()
            .iter()
            .find(|item| item.info.kind == crate::media::MediaKind::Replay)
            .expect("the cut recording is in Media")
            .clone();
        assert_eq!(item.info.game, game);
        assert!(!app.emulator.is_taping());
        assert!(!app.ui.frame_log.is_recording());
        let bytes = std::fs::read(app.media.path(&item.id)).unwrap();
        let replay = citrine_gb_replay::Replay::read(&bytes).unwrap();
        let perf = replay
            .perf
            .expect("the cut recording keeps its frame times");
        let info: serde_json::Value = serde_json::from_slice(&perf.info).unwrap();
        assert_eq!(info["game"]["crc32"], crc.as_str(), "{info}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn frame_times_go_in_the_recording_and_export_as_a_csv() {
        use crate::app::recording::{FRAMES, INFO, frame_times_name, frame_times_zip};
        use crate::app::widgets::record_bar::{RecordBarAction, RecordBarFacts};
        use std::io::Read;

        let mut app = media_app("tape-frame-times", LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        app.ui.settings.record_frame_times = true;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        app.emulator.running = true;
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        run_updates(&mut app, &ctx, &mut frame, 3);
        assert!(!app.ui.frame_log.is_recording());
        app.toggle_recording();
        assert!(app.ui.frame_log.is_recording());
        let facts = RecordBarFacts::of(&app.emulator, &app.ui, false).expect("the bar shows");
        assert_eq!(facts.frame_times, Some(true));
        run_updates(&mut app, &ctx, &mut frame, 30);
        app.record_bar_action(RecordBarAction::Stop);
        assert!(!app.ui.frame_log.is_recording());
        run_updates(&mut app, &ctx, &mut frame, 3);
        app.handle_event_queue(false);
        let item = app
            .media
            .items()
            .iter()
            .find(|item| item.info.kind == crate::media::MediaKind::Replay)
            .expect("the recording is in Media")
            .clone();
        assert!(item.info.frame_times);
        let bytes = std::fs::read(app.media.path(&item.id)).unwrap();
        let replay = citrine_gb_replay::Replay::read(&bytes).unwrap();
        let perf = replay
            .perf
            .as_ref()
            .expect("the frame times are in the recording");
        let info: serde_json::Value = serde_json::from_slice(&perf.info).unwrap();
        assert!(info["recording"]["started_unix"].is_u64(), "{info}");
        let csv = std::str::from_utf8(&perf.frames).unwrap();
        let mut lines = csv.lines();
        assert!(lines.next().unwrap().ends_with(",tape_frame"));
        let frames: Vec<u64> = lines
            .filter_map(|line| line.rsplit(',').next()?.parse().ok())
            .collect();
        assert!(frames.len() > 10, "{} rows", frames.len());
        assert!(
            frames.windows(2).all(|pair| pair[0] <= pair[1]),
            "{frames:?}"
        );
        let last = *frames.last().unwrap();
        assert!(
            last > 0 && last <= replay.header.frames,
            "{last} of {}",
            replay.header.frames
        );

        let zipped = frame_times_zip(&bytes).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zipped)).unwrap();
        let mut exported = String::new();
        zip.by_name(FRAMES)
            .unwrap()
            .read_to_string(&mut exported)
            .unwrap();
        assert_eq!(exported, csv);
        assert!(zip.by_name(INFO).is_ok());
        let name = frame_times_name(&item.id);
        assert!(
            name.ends_with(" performance.zip") && !name.contains(".replay"),
            "{name}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_play_together_recording_is_a_replay_with_its_video_and_reason() {
        use crate::media::MediaKind;
        use crate::media::recording::headless;

        let mut app = media_app("video-replay", LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        assert_eq!(app.emulator.tape_blocker(), None);
        app.emulator.together = true;
        assert!(!app.emulator.can_tape());
        assert_eq!(app.emulator.tape_blocker(), Some("Play together"));
        let header = app.emulator.video_header().expect("a game is in");
        assert_eq!(header.no_inputs.as_deref(), Some("Play together"));
        assert!(!header.sha1.is_empty());

        let mut recorded =
            headless::record(include_bytes!("../homebrew/tobu.gb"), "together.mp4", 60);
        let temp = recorded.done.output.path.clone();
        let mp4 = std::fs::read(&temp).unwrap();
        let frames = recorded.done.frames;
        recorded.done.info.kind = MediaKind::Replay;
        recorded.done.info.video = true;
        recorded.done.info.replay = Some(Box::new(citrine_gb_replay::Replay {
            header,
            perf: Some(citrine_gb_replay::Perf {
                frames: b"tape_frame\n\n".to_vec(),
                info: b"{}".to_vec(),
                gpu: None,
            }),
            ..citrine_gb_replay::Replay::default()
        }));
        app.store_recording(recorded.done, true);
        app.store_video_replays();
        app.handle_event_queue(false);
        assert!(!temp.exists(), "the loose video is gone");

        let item = app
            .media
            .items()
            .iter()
            .find(|item| item.info.kind == MediaKind::Replay)
            .expect("the recording is in Media")
            .clone();
        assert!(item.info.video && item.info.frame_times);
        assert!(item.thumbnail.is_some());
        let length = item.info.length_ms.unwrap();
        assert!((900..1100).contains(&length), "{length} ms");
        assert!(item.id.file_name().ends_with(".replay"), "{:?}", item.id);
        let bytes = std::fs::read(app.media.path(&item.id)).unwrap();
        let replay = citrine_gb_replay::Replay::read(&bytes).unwrap();
        assert_eq!(replay.video.as_deref(), Some(&mp4[..]));
        assert!(replay.thumb.is_some());
        assert_eq!(replay.header.frames, frames);
        assert_eq!(replay.header.no_inputs.as_deref(), Some("Play together"));
        assert!(replay.events.is_empty() && replay.start.is_empty());
        assert!(replay.perf.is_some());
        assert_eq!(
            citrine_gb_replay::Replay::video(&bytes).unwrap().as_deref(),
            Some(&mp4[..])
        );
        let name = crate::app::recording::video_name(&item.id);
        assert!(
            name.ends_with(".mp4") && !name.contains(".replay"),
            "{name}"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn frame_times_turned_off_while_recording_leave_none() {
        use crate::app::widgets::record_bar::{RecordBarAction, RecordBarFacts};

        let mut app = media_app("tape-frame-times-off", LayoutMode::Desktop);
        app.ui.settings.check_updates = false;
        app.ui.settings.record_frame_times = true;
        load_game(&mut app, include_bytes!("../homebrew/tobu.gb"), None);
        app.emulator.running = true;
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        run_updates(&mut app, &ctx, &mut frame, 3);
        app.toggle_recording();
        run_updates(&mut app, &ctx, &mut frame, 10);
        app.record_bar_action(RecordBarAction::FrameTimes(false));
        assert!(!app.ui.frame_log.is_recording());
        let facts = RecordBarFacts::of(&app.emulator, &app.ui, false).expect("the bar shows");
        assert_eq!(facts.frame_times, Some(false));
        run_updates(&mut app, &ctx, &mut frame, 10);
        app.record_bar_action(RecordBarAction::Stop);
        app.handle_event_queue(false);
        let item = app
            .media
            .items()
            .iter()
            .find(|item| item.info.kind == crate::media::MediaKind::Replay)
            .expect("the recording is in Media")
            .clone();
        assert!(!item.info.frame_times);
        let bytes = std::fs::read(app.media.path(&item.id)).unwrap();
        let replay = citrine_gb_replay::Replay::read(&bytes).unwrap();
        assert!(replay.perf.is_none());
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn recording_a_ds_game_with_rewind_off_keeps_a_replay() {
        use crate::emulator::rewind::{ClipLength, RewindLength};

        let mut run = DsRun::new("blocksds-lighting_cube.nds");
        run.app.ui.settings.rewind = RewindLength::Off;
        run.app.ui.settings.clip_length = ClipLength::Off;
        run.app.ui.settings.dirty = true;
        run.play(10);
        assert!(run.app.emulator.start_tape_recording());
        run.play(60);
        let secs = run.app.emulator.tape_recording_secs().unwrap();
        assert!(secs > 0.5, "{secs} s recorded");
        let replay = run.app.emulator.stop_tape_recording().unwrap();
        assert!(replay.header.frames > 30, "{} frames", replay.header.frames);
        assert_eq!(run.app.emulator.tape_recording_secs(), None);
        run.play(10);
        assert_eq!(
            run.app.emulator.rewind_kept(),
            (0, 0),
            "off keeps nothing again"
        );
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn a_recording_ds_game_draws_on_the_pool_from_the_console_thread() {
        let mut run = DsRun::new("blocksds-lighting_cube.nds").on_pool_threads();
        run.play(10);
        assert!(run.app.emulator.start_tape_recording());
        let before = run.nds().side_work();
        run.play(60);
        let mut side = run.nds().side_work();
        for _ in 0..20 {
            if side.lines_3d > before.lines_3d {
                break;
            }
            run.play(30);
            side = run.nds().side_work();
        }
        assert!(
            side.lines_3d > before.lines_3d,
            "pool work while recording {side:?}, before {before:?}"
        );
        assert!(run.app.emulator.tape_recording_secs().unwrap() > 0.5);
        let replay = run.app.emulator.stop_tape_recording().unwrap();
        assert!(replay.header.frames > 30, "{} frames", replay.header.frames);
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn upscaling_draws_a_running_ds_game_on_the_gpu() {
        use crate::app::ui_state::ds_3d::{Ds3d, Resolution, SmoothEdges};

        let Some(gpu) = crate::emulator::Emulator::test_gpu() else {
            return;
        };
        let mut run = DsRun::new("blocksds-lighting_cube.nds");
        run.app.emulator.set_gpu(&run.ctx, &gpu);
        run.app.ui.settings.ds_3d = Ds3d {
            resolution: Resolution::Native,
            smooth_edges: SmoothEdges::Off,
            ..Ds3d::default()
        };
        run.play(20);
        assert!(!run.app.emulator.is_upscaled(), "native draws no upscale");

        run.app.ui.settings.ds_3d = Ds3d {
            resolution: Resolution::Quadruple,
            smooth_edges: SmoothEdges::On,
            ..Ds3d::default()
        };
        run.play(30);
        assert!(run.app.emulator.is_upscaled(), "4x draws on the GPU");
        assert!(
            run.nds().composite().is_some(),
            "the console keeps a composite"
        );
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_3d_gpu_time_reaches_a_hud_with_the_performance_tab_closed() {
        use crate::app::hud::Value;
        use crate::app::ui_state::ds_3d::{Ds3d, Resolution, SmoothEdges};
        use crate::app::widgets::performance::GpuView;

        let Some(gpu) = crate::emulator::Emulator::test_gpu() else {
            return;
        };
        let mut run = DsRun::new("blocksds-lighting_cube.nds");
        run.app.emulator.set_gpu(&run.ctx, &gpu);
        run.app.ui.settings.ds_3d = Ds3d {
            resolution: Resolution::Quadruple,
            smooth_edges: SmoothEdges::On,
            ..Ds3d::default()
        };
        let id = run.app.ui.settings.hud.add();
        run.app.ui.settings.hud.get_mut(id).unwrap().values = vec![Value::Gpu];
        run.play(30);
        assert!(
            !run.app
                .dock
                .iter_all_tabs()
                .any(|(_, tab)| *tab == Tab::Performance),
            "the Performance tab is closed"
        );
        let mut views = Vec::new();
        for _ in 0..40 {
            run.play(10);
            let gpu = run.app.ui.performance.view().gpu;
            if matches!(gpu, GpuView::Timed(..)) {
                return;
            }
            views.push(gpu);
        }
        panic!("{} never timed: {views:?}", gpu.adapter.get_info().name);
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn a_gba_game_follows_the_gba_jit_switch_only() {
        use citrine_gb::nds::machine::JIT_AVAILABLE;

        let mut app = test_app();
        app.ui.settings.check_updates = false;
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/gba/save-sram.gba");
        app.handle_load_rom_data(std::fs::read(path).expect("the GBA fixture"));
        app.emulator.running = true;
        assert!(app.emulator.has_worker());
        let ctx = Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            ..Default::default()
        };
        let mut play = |app: &mut Citrine| {
            let shown = app.emulator.total_frames;
            for _ in 0..6 {
                let _ = ctx.run(input.clone(), |ctx| {
                    eframe::App::update(app, ctx, &mut frame);
                });
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(app.emulator.total_frames > shown, "the console ran");
        };

        app.ui.settings.ds_jit = false;
        app.ui.settings.gba_jit = Some(true);
        play(&mut app);
        assert_eq!(
            app.emulator.gba_jit_on(),
            JIT_AVAILABLE,
            "the GBA switch alone runs the GBA JIT"
        );

        app.ui.settings.ds_jit = true;
        app.ui.settings.gba_jit = Some(false);
        play(&mut app);
        assert!(!app.emulator.gba_jit_on(), "the DS switch leaves it off");

        app.ui.settings.gba_jit = Some(true);
        play(&mut app);
        assert_eq!(app.emulator.gba_jit_on(), JIT_AVAILABLE);
        eframe::App::on_exit(&mut app);
    }

    #[cfg(all(feature = "nds", not(target_arch = "wasm32")))]
    #[test]
    fn the_jit_switch_sits_on_each_console_page_on_both_surfaces() {
        use crate::app::ui_state::settings_tree::Surface;
        use crate::app::ui_state::settings_tree::rows::RowId;
        if !citrine_gb::nds::machine::JIT_AVAILABLE {
            return;
        }
        assert_eq!(phone_label(RowId::Jit), phone_label(RowId::GbaJit));
        assert_eq!(
            RowId::Jit.label(Surface::Desktop),
            RowId::GbaJit.label(Surface::Desktop)
        );
        assert_eq!(RowId::Jit.page(), SettingsTab::Ds);
        assert_eq!(RowId::GbaJit.page(), SettingsTab::Gba);
        let home = |page: SettingsTab| page != SettingsTab::App;
        for page in [SettingsTab::Ds, SettingsTab::Gba, SettingsTab::App] {
            let (mut app, ctx) = settings_page(&format!("jit-row-phone-{page:?}"), page);
            let size = egui::vec2(412.0, 3000.0);
            let mut output = sized_frame(&mut app, &ctx, size, Vec::new());
            for _ in 0..3 {
                output = sized_frame(&mut app, &ctx, size, Vec::new());
            }
            let shown = text_rect(&output, phone_label(RowId::Jit)).is_some();
            assert_eq!(shown, home(page), "phone {page:?}");
        }
        for page in [SettingsTab::Ds, SettingsTab::Gba, SettingsTab::App] {
            let mut app = media_app(&format!("jit-row-desktop-{page:?}"), LayoutMode::Desktop);
            app.dock = DockState::new(vec![Tab::Settings]);
            app.ui.settings.current_tab = page;
            let ctx = Context::default();
            let size = egui::vec2(1280.0, 3000.0);
            let mut output = sized_frame(&mut app, &ctx, size, Vec::new());
            for _ in 0..3 {
                output = sized_frame(&mut app, &ctx, size, Vec::new());
            }
            let shown = text_rect(&output, RowId::Jit.label(Surface::Desktop)).is_some();
            assert_eq!(shown, home(page), "{page:?}");
        }
    }

    #[test]
    fn a_hidden_app_stops_its_console_worker_and_starts_one_when_shown() {
        let mut app = test_app();
        assert!(app.emulator.has_worker());
        app.follow_visibility(false);
        assert!(!app.emulator.has_worker());
        app.follow_visibility(true);
        assert!(app.emulator.has_worker());
    }

    #[test]
    fn drawing_a_session_never_calls_its_engine_home() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, homebrew, snake};
        use citrine_gb_threads::{Mode, Threads};
        let threads = Threads::new(Mode::Threads);
        let mut pair = Pair::new(false, 0, 0, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(90);
        for layout in [LayoutMode::Desktop, LayoutMode::Mobile] {
            let mut app = phone_app("drawing-session");
            app.ui.settings.layout = layout;
            std::mem::swap(&mut *app.play, &mut pair.host.play);
            std::mem::swap(&mut app.online, &mut pair.host.online);
            app.play.place_engines(Some(&threads));
            let ctx = Context::default();
            for frame in 0..3 {
                let session = app.play.session_mut().unwrap();
                session.set_watch_friend(true);
                session.run_frames(
                    vec![citrine_gb::console::Keys::empty()],
                    web_time::Instant::now(),
                );
                assert!(!session.engine_home(), "{layout:?} frame {frame}");
                assert!(
                    session.friend_picture().is_some(),
                    "{layout:?} frame {frame}"
                );
                play_frame(&mut app, &ctx, Vec::new());
                let session = app.play.session_mut().unwrap();
                assert!(!session.engine_home(), "{layout:?} frame {frame} drawn");
                session.settle_frames();
                assert!(session.engine_home(), "{layout:?} frame {frame} settled");
            }
            app.play.place_engines(None);
            std::mem::swap(&mut *app.play, &mut pair.host.play);
            std::mem::swap(&mut app.online, &mut pair.host.online);
        }
    }

    #[test]
    fn in_focus_mode_an_invite_pops_up_beside_the_game_unless_turned_off() {
        let mut app = crystal_app("focus-popups", LayoutMode::Desktop, None);
        app.emulator.running = true;
        app.ui.focus_mode = true;
        let ctx = Context::default();
        app.emulator.clear_frame(&ctx);
        app.play.push_invite(crate::play::IncomingInvite {
            session: 1,
            from: citrine_gb_net::OnlineId::new(7).expect("id"),
            game: None,
        });
        let size = egui::vec2(1600.0, 800.0);
        let output = toast_frames(&mut app, &ctx, size);
        let picture = picture_rect(&output).expect("the game is drawn");
        let line = text_containing(&output, "Wants to join you").expect("the invite pops up");
        assert!(!line.intersects(picture), "{line:?} covers {picture:?}");
        assert!(text_containing(&output, "Join").is_some());

        app.ui.settings.online.focus_popups = false;
        let output = toast_frames(&mut app, &ctx, size);
        assert!(text_containing(&output, "Wants to join you").is_none());
    }

    #[cfg(feature = "nds")]
    fn heartgold_rom() -> Rom {
        let mut rom = vec![0; 0x1000];
        rom[..4].copy_from_slice(b"TEST");
        rom[0x0C..0x10].copy_from_slice(b"IPKE");
        for (at, offset, ram) in [(0x20, 0x200u32, 0x0200_0000u32), (0x30, 0x800, 0x0380_0000)] {
            for (index, value) in [offset, ram, ram, 4].into_iter().enumerate() {
                rom[at + 4 * index..at + 4 * index + 4].copy_from_slice(&value.to_le_bytes());
            }
            rom[offset as usize..offset as usize + 4]
                .copy_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        }
        Rom::from(rom)
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_pokewalker_page_keeps_heartgold_running_on_a_phone() {
        let mut app = phone_app("walker-page");
        let root = crate::storage::scratch_dir()
            .join(format!("citrine-walker-page-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        app.emulator.pokewalker =
            crate::walker::Pokewalker::new(Box::new(crate::walker::FileStore::at(root)));
        app.emulator
            .load_game(
                &heartgold_rom(),
                citrine_gb::console::ConsoleKind::Nds,
                None,
            )
            .expect("load");
        app.emulator.running = true;
        app.resume_game();
        let ctx = Context::default();
        play_frame(&mut app, &ctx, Vec::new());
        app.mobile.open(Tab::Walker);
        play_frame(&mut app, &ctx, Vec::new());
        assert!(
            app.emulator.running,
            "the game answers the walker while its page is open"
        );
        app.mobile.open(Tab::Saves);
        play_frame(&mut app, &ctx, Vec::new());
        assert!(!app.emulator.running, "other pages still hold the game");
    }

    #[cfg(feature = "nds")]
    pub(crate) fn heartgold_in_its_overworld(name: &str) -> Citrine {
        let rom = std::env::var("CITRINE_HG").expect("CITRINE_HG");
        let save = std::env::var("CITRINE_HG_SAVE").expect("CITRINE_HG_SAVE");
        let mut app = phone_app(name);
        app.handle_load_rom_data(std::fs::read(rom).expect("rom"));
        let imported = app
            .emulator
            .import_save(Some(&std::fs::read(save).expect("save")), None)
            .expect("import");
        println!("import {imported:?}");
        let keyboard = crate::held::InputSource::Keyboard;
        let presses = [
            (600, citrine_gb::console::Keys::START),
            (900, citrine_gb::console::Keys::START),
            (1300, citrine_gb::console::Keys::A),
        ];
        for frame in 1..=2600 {
            for (at, key) in presses {
                if frame == at {
                    app.emulator.press(keyboard, key);
                }
                if frame == at + 6 {
                    app.emulator.release(keyboard, key);
                }
            }
            assert_eq!(app.emulator.lend_one_frame(), 1, "frame {frame}");
        }
        app
    }

    #[cfg(feature = "nds")]
    #[test]
    #[ignore = "needs HeartGold in CITRINE_HG and a save with a boxed Pokémon in CITRINE_HG_SAVE"]
    fn real_heartgold_shows_the_pokemon_in_its_pc_boxes() {
        use crate::game_info::Page;
        let mut app = heartgold_in_its_overworld("real-hg-boxes");
        app.mobile.open(Tab::GameInfo);
        app.watch_game_info();
        app.game_info.next_read_now();
        app.watch_game_info();
        let game = app.game_info.game().expect("game info");
        let state = game.live.as_ref().expect("a live read");
        let boxes: Vec<(Option<&str>, Vec<u16>)> = state
            .boxes
            .iter()
            .map(|pc| {
                let species = pc
                    .pokemon
                    .iter()
                    .filter_map(|p| match p {
                        citrine_gb_gamedata::Pokemon::Hatched(mon) => Some(mon.species),
                        citrine_gb_gamedata::Pokemon::Egg => None,
                    })
                    .collect();
                (pc.name.as_deref(), species)
            })
            .collect();
        println!("boxes {boxes:?} current {}", state.current_box);
        assert_eq!(state.boxes.len(), 18);
        assert!(state.boxes.iter().all(|pc| pc.name.is_some()));
        assert_eq!(boxes[0].1, vec![155]);
        let citrine_gb_gamedata::Pokemon::Hatched(feuri) = &state.boxes[0].pokemon[0] else {
            panic!("egg");
        };
        println!("box 1 slot 1: {} level {}", feuri.nickname, feuri.level);
        assert!(feuri.level > 1);
        assert!(Page::available(state).contains(&Page::Boxes));
    }

    #[cfg(feature = "nds")]
    fn side_share(app: &Citrine, tab: Tab) -> Option<f32> {
        let (_, node, _) = app.dock.find_tab(&tab)?;
        match &app.dock.main_surface()[node.parent()?] {
            egui_dock::Node::Horizontal(split) => Some(1.0 - split.fraction),
            _ => None,
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn the_walker_opens_as_wide_as_the_other_side_panes() {
        let rom = crate::storage::decompress(include_bytes!("../homebrew/nds/pong_nds.nds.br"))
            .expect("rom unpacks");
        let ctx = Context::default();
        let mut shares = Vec::new();
        for tab in [Tab::Saves, Tab::Walker] {
            let mut app = phone_app(&format!("walker-share-{tab:?}"));
            app.ui.settings.layout = LayoutMode::Desktop;
            app.handle_load_rom_data(rom.clone());
            let _ = info_frame_out(&mut app, &ctx, 1280.0, 800.0);
            if tab == Tab::Walker {
                app.open_walker();
            } else {
                app.open_tab(tab);
            }
            let _ = info_frame_out(&mut app, &ctx, 1280.0, 800.0);
            shares.push(side_share(&app, tab).expect("beside the game"));
        }
        assert_eq!(
            shares[1], shares[0],
            "walker {} vs saves {}",
            shares[1], shares[0]
        );
        assert!(shares[1] < 0.5, "the game keeps most of the window");
    }
}
