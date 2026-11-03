use crate::app::tabs::Tab;
use crate::app::ui_state::UiState;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::synced;
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::media::MediaStore;
#[cfg(not(target_arch = "wasm32"))]
use crate::media::recording::Outcome;
use crate::online::Online;
use crate::pad_input::PadSource;
use crate::play::Play;
use crate::storage::SettingsStore;
use crate::utils::file_channels::FileChannels;
use crate::utils::save_files::SaveFiles;
use citrine_gb::gb::GbModel;
use eframe::{Frame, Storage};
use egui::{Context, FontDefinitions};
use egui_commonmark::CommonMarkCache;
use egui_dock::DockState;
use toasts::Toasts;

const QUICK_LOAD_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(target_arch = "wasm32")]
const AUDIO_STALL_SECS: f32 = 1.0;

const FRIENDS_DOCK_SHARE: f32 = 0.34;
const GAME_INFO_DOCK_SHARE: f32 = 0.42;

const SETTINGS_WRITE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

mod audio;
mod canvas;
mod capture;
mod cloud;
mod console;
mod events;
mod files;
pub mod focus;
mod hotkeys;
mod info_dock;
mod link;
mod mobile;
mod online;
mod render;
mod reset;
pub(crate) mod tabs;
mod toasts;
pub mod ui_state;
pub mod viewer;
mod widgets;
mod your_data;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Citrine {
    pub dock: DockState<Tab>,
    pub ui: Box<UiState>,
    #[serde(skip, default)]
    pub emulator: Box<Emulator>,
    #[serde(skip, default)]
    pub files: FileChannels,
    #[serde(skip, default = "crate::pad_input::open_source")]
    pub pads: Box<dyn PadSource>,
    #[serde(skip, default)]
    pub toasts: Toasts,
    #[serde(skip, default)]
    pub audio: Option<Audio>,
    #[serde(skip, default)]
    pub events: events::AppEventQueue,
    #[serde(skip, default)]
    pub commonmark: CommonMarkCache,
    #[serde(skip, default)]
    quick_load_hold: Option<web_time::Instant>,
    #[serde(skip, default)]
    mobile: mobile::MobileState,
    #[serde(skip, default)]
    audio_stalled: bool,
    #[serde(skip, default)]
    record_when_ready: bool,
    #[serde(skip, default)]
    codec_prompt: bool,
    #[serde(skip, default)]
    settings_store: SettingsStore,
    #[serde(skip, default)]
    settings_saved_at: Option<web_time::Instant>,
    #[serde(skip, default)]
    pending_import: Option<PendingImport>,
    #[serde(skip, default)]
    pending_reset: Option<PendingReset>,
    #[serde(skip, default)]
    reset_without_asking: bool,
    #[serde(skip, default)]
    pending_console: Option<PendingConsole>,
    #[serde(skip, default)]
    pub media: MediaStore,
    #[serde(skip, default)]
    pub online: Online,
    #[serde(skip, default)]
    pub play: Box<Play>,
    #[serde(skip, default)]
    activity: Activity,
    #[serde(skip, default)]
    cloud_rom: Option<String>,
    #[serde(skip, default)]
    pub game_info: crate::game_info::GameInfo,
    #[serde(skip, default)]
    cloud_hold: Option<CloudHold>,
    #[serde(skip, default)]
    pub stats: crate::stats::Stats,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[serde(skip, default)]
    discord: crate::discord::Rpc,
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[serde(skip, default)]
    shots: crate::discord::Shots,
}

struct CloudHold {
    since: web_time::Instant,
    was_running: bool,
    told: bool,
}

const CLOUD_HOLD_LIMIT: std::time::Duration = std::time::Duration::from_secs(4);
const CLOUD_HOLD_TOAST: std::time::Duration = std::time::Duration::from_millis(400);

struct Activity {
    last_input: web_time::Instant,
    held: citrine_gb::gb::joypad::JoypadState,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            last_input: web_time::Instant::now(),
            held: citrine_gb::gb::joypad::JoypadState::empty(),
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

impl Default for Citrine {
    fn default() -> Self {
        let dock = DockState::new(vec![Tab::GameBoy]);
        let mut app = Self {
            dock,
            ui: Box::default(),
            emulator: Box::default(),
            files: FileChannels::default(),
            pads: crate::pad_input::open_source(),
            toasts: Toasts::default(),
            audio: None,
            events: events::AppEventQueue::default(),
            commonmark: CommonMarkCache::default(),
            quick_load_hold: None,
            mobile: mobile::MobileState::default(),
            audio_stalled: false,
            record_when_ready: false,
            codec_prompt: false,
            settings_store: SettingsStore::default(),
            settings_saved_at: None,
            pending_import: None,
            pending_reset: None,
            reset_without_asking: false,
            pending_console: None,
            media: MediaStore::default(),
            online: Online::default(),
            play: Box::default(),
            activity: Activity::default(),
            cloud_rom: None,
            cloud_hold: None,
            game_info: Default::default(),
            stats: crate::stats::Stats::default(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            discord: crate::discord::Rpc::default(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            shots: crate::discord::Shots::default(),
        };
        app.open_tab(Tab::Info);
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
        app.online.persist_chats();
        app.online.cloud_attach(app.emulator.store.clone());
        app.stats = crate::stats::Stats::open(crate::stats::StatsStore::default());
        if let Some(relay) = app.ui.settings.online.joined_relay() {
            app.online.sign_in(relay, web_time::Instant::now());
        }
        app.audio = Some(Audio::new());
        app.emulator.running = false;
        app.mobile.listen_for_back();
        crate::autosave::install_panic_hook(app.emulator.last_good.clone());
        #[cfg(target_arch = "wasm32")]
        crate::autosave::save_when_hidden(app.emulator.last_good.clone());
        #[cfg(target_os = "android")]
        crate::platform::android::lifecycle::save_on_pause(app.emulator.last_good.clone());

        app
    }

    fn restore(storage: Option<&dyn Storage>) -> Self {
        storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default()
    }

    fn setup_fonts(ctx: &Context) {
        let mut fonts = FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        crate::app::widgets::emoji::add_font(&mut fonts);
        ctx.set_fonts(fonts);
    }
}

impl eframe::App for Citrine {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.ui.update_avg_timer.start();
        #[cfg(target_os = "android")]
        if let Some(audio) = &mut self.audio {
            crate::platform::android::lifecycle::follow_visibility(audio);
        }

        // ToDo: Handle this more efficiently => e.g. pause emulator if not visible
        ctx.request_repaint();

        #[cfg(target_arch = "wasm32")]
        if ctx.input(|i| i.unstable_dt) > AUDIO_STALL_SECS {
            self.audio_stalled = true;
        }

        self.emulator.gb.debugger.record_channels = self.dock.find_tab(&Tab::ApuWaves).is_some();

        mobile::touch_spacing(ctx, self.is_mobile(ctx));
        self.name_games();
        self.update_online(ctx);
        self.ui.update.tick(self.ui.settings.check_updates);
        self.count_your_data();
        self.render(ctx);
        if self.ui.settings.focus_mode
            && self.focus_exit_names().is_empty()
            && !self.ui.viewer.is_open()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.toggle_focus_mode();
        }
        self.media_viewer(ctx);

        self.handle_snapshot_hotkeys(ctx);
        self.quick_load_overlay(ctx);
        self.import_confirm(ctx);
        self.codec_confirm(ctx);
        self.reset_confirm(ctx);
        self.load_state_confirm(ctx);
        self.console_confirm(ctx);
        self.cloud_conflict(ctx);
        self.handle_your_data(ctx);
        tabs::playing::stop_confirm(ctx, &mut self.play, &mut self.online, &mut self.ui.playing);
        self.play_picker(ctx);
        if let Some(next) =
            tabs::playing::switch_confirm(ctx, &mut self.ui.playing, &self.online, &self.play)
        {
            self.play.leave(&mut self.online);
            self.ui.playing.switch_to = Some(next);
        }
        self.note_console_switch();
        self.mobile_escape(ctx);
        self.ui.capture.end_frame();
        let debug_open = self.dock.find_tab(&Tab::ControllerDebug).is_some()
            || self.mobile.showing(Tab::ControllerDebug);
        let now = web_time::Instant::now();
        self.ui.controller_debug.set_open(debug_open, now);
        self.ui
            .controller_debug
            .note_focus(ctx.input(|i| i.viewport().focused), now);

        self.drain_file_channels();
        self.handle_dropped_files(ctx);
        self.unlock_audio_on_interaction(ctx);
        self.watch_cloud_game();

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
        let presses = self.emulator.take_loose_presses();
        self.run_pad_shortcuts(presses);
        self.play.flush(&mut self.online);
        self.record_stats();
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            let game = crate::discord::game_now(&self.emulator, &self.stats, &self.game_info);
            let image = self.discord_screen();
            let presence =
                crate::discord::presence(&self.ui.settings.discord, game.as_ref(), image);
            self.discord.show(presence);
        }
        self.settle_play(ctx);
        match self.emulator.autosave_if_due() {
            Ok(Some(_)) => self.ui.snapshots.invalidate(),
            Ok(None) => {}
            Err(err) => {
                self.toasts.error(format!("Autosave failed: {err:?}"));
            }
        }
        self.finish_recordings();
        self.watch_game_info();

        self.show_toasts(ctx);

        let mobile = self.is_mobile(ctx);
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

        self.flush_settings(false);
        #[cfg(target_os = "android")]
        crate::platform::android::frame_end(ctx);

        self.ui.update_avg_timer.stop();
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        canvas::fit(ctx, raw_input);
        self.raw_keys(ctx, raw_input);
        #[cfg(target_os = "android")]
        crate::platform::android::input(ctx, raw_input);
        crate::app::ui_state::settings::note_window(ctx, raw_input);
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

    fn save(&mut self, storage: &mut dyn Storage) {
        #[cfg(target_os = "android")]
        if let Some(audio) = &mut self.audio {
            crate::platform::android::lifecycle::follow_visibility(audio);
        }
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save: {err:?}");
        }
        self.flush_settings(true);
        self.online.flush_chats();
        self.stats.flush();
        self.online.cloud_flush();
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn on_exit(&mut self) {
        if let Err(err) = self.emulator.end_link() {
            log::error!("failed to save the second game on close: {err:?}");
        }
        if let Err(err) = self.emulator.autosave_if_played() {
            log::error!("failed to autosave on close: {err:?}");
        }
        self.emulator.screen_recorder.stop();
        for outcome in self.emulator.screen_recorder.wait() {
            if let Outcome::Saved(done) = outcome
                && let Err(err) = self.store_recording(*done)
            {
                log::error!("failed to save the recording: {err}");
            }
        }
        if self.ui.update.take_restart() {
            crate::update::relaunch();
        }
    }
}

impl Citrine {
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
        let emulator = &self.emulator;
        let solo = emulator.rom_key().map(|key| crate::discord::Unit {
            game: key.to_string(),
            running: emulator.running && emulator.gb.cartridge.has_rom_loaded,
            frame: emulator.gb.frame().as_slice(),
        });
        let unit = crate::discord::my_unit(self.play.session(), solo);
        let shot = self.shots.due(
            std::time::Instant::now(),
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
                let (width, height, rgba) =
                    emulator.screenshot_of(unit.frame, crate::discord::SHOT_SCALE);
                if let Some(png) = crate::utils::screenshot::encode_png(width, height, &rgba)
                    .filter(|png| png.len() <= citrine_gb_net::MAX_SCREEN_BYTES)
                {
                    self.online.send_screen(png);
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
                linked: self.emulator.link.is_some(),
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
        if let Some(link) = self.emulator.link.as_ref().filter(|link| link.plugged()) {
            return Some((
                CablePartner::Game(link.rom_key().to_string()),
                link.title().to_string(),
            ));
        }
        let partner = self.emulator.partner.as_ref().filter(|p| p.plugged())?;
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
            events::AppEvent::OpenTab { tab } => self.show_tab(tab, touch),
            events::AppEvent::MediaSaved { id } => self.media_saved(id, touch),
            events::AppEvent::MediaAdded { id } => self.media_added(id),
            events::AppEvent::ShowMedia { id } => self.show_media(id, touch),
            events::AppEvent::PlayOn { model } => self.ask_play_on(model),
            events::AppEvent::PickSecondGame => self.pick_second_game(touch),
            events::AppEvent::LinkGame { data, name } => self.link_game(data, name),
            events::AppEvent::EndLink => self.end_link(),
            events::AppEvent::StartPartner { event } => self.start_partner(event),
            events::AppEvent::OpenSettings { tab } => self.open_settings(tab, touch),
            events::AppEvent::UndoSnapshot { rom_key, slot } => self.undo_snapshot(&rom_key, slot),
        }
    }
}

#[cfg(test)]
mod tests;
