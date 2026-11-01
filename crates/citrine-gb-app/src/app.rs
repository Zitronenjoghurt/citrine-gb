use crate::app::tabs::Tab;
use crate::app::ui_state::UiState;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::keybinds::PadButton;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::settings::other_console;
use crate::app::ui_state::settings::{LayoutMode, Settings, console_name, plays_on_both};
use crate::app::ui_state::synced;
use crate::app::widgets::snapshots::{SnapshotNews, snapshot_message};
use crate::audio::Audio;
use crate::emulator::{Emulator, SaveImport};
use crate::icons;
use crate::media::recording::{self, Availability, FinishedRecording, Outcome, StopReason};
use crate::media::{Frame as MediaFrame, MediaId, MediaItem, MediaKind, MediaStore, NewMedia};
use crate::online::Online;
use crate::pad_input::PadSource;
use crate::play::{MyGame, Play, Stage};
use crate::storage::SettingsStore;
use crate::utils::file_channels::FileChannels;
use crate::utils::file_loader::FileLoader;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::utils::save_files::{FileKind, SaveFiles};
use citrine_gb::gb::GbModel;
use citrine_gb::rom::Rom;
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb::rom::header::RomCgbMode;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Color32, Context, FontDefinitions, TopBottomPanel};
use egui_commonmark::CommonMarkCache;
use egui_dock::{DockState, NodeIndex, SurfaceIndex};
use strum::IntoEnumIterator;
use toasts::{Toast, Toasts};

const QUICK_LOAD_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

const MOBILE_BREAKPOINT: f32 = 560.0;

#[cfg(target_arch = "wasm32")]
const AUDIO_STALL_SECS: f32 = 1.0;

const FRIENDS_DOCK_SHARE: f32 = 0.34;
const GAME_INFO_DOCK_SHARE: f32 = 0.42;

const SETTINGS_WRITE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

mod canvas;
mod events;
pub mod focus;
mod link;
mod mobile;
pub(crate) mod tabs;
mod toasts;
pub mod ui_state;
pub mod viewer;
mod widgets;
mod your_data;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Citrine {
    pub dock: DockState<Tab>,
    pub ui: UiState,
    #[serde(skip, default)]
    pub emulator: Emulator,
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
    pub play: Play,
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
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(skip, default)]
    discord: crate::discord::Rpc,
    #[cfg(not(target_arch = "wasm32"))]
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
            ui: UiState::default(),
            emulator: Emulator::default(),
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
            play: Play::default(),
            activity: Activity::default(),
            cloud_rom: None,
            cloud_hold: None,
            game_info: Default::default(),
            stats: crate::stats::Stats::default(),
            #[cfg(not(target_arch = "wasm32"))]
            discord: crate::discord::Rpc::default(),
            #[cfg(not(target_arch = "wasm32"))]
            shots: crate::discord::Shots::default(),
        };
        app.open_tab(Tab::Info);
        app
    }
}

fn device_size(ctx: &Context) -> egui::Vec2 {
    let physical = ctx.content_rect().size() * ctx.pixels_per_point();
    physical / ctx.native_pixels_per_point().unwrap_or(1.0)
}

pub fn recording_time(recorder: &recording::ScreenRecorder) -> String {
    crate::media::format_length(recorder.elapsed().as_millis() as u64)
}

pub fn mobile_layout(layout: LayoutMode, ctx: &Context) -> bool {
    touch_layout_for(layout, device_size(ctx))
}

pub fn desktop_too_narrow(ctx: &Context) -> bool {
    device_size(ctx).x < MOBILE_BREAKPOINT
}

fn touch_layout_for(layout: LayoutMode, size: egui::Vec2) -> bool {
    match layout {
        LayoutMode::Desktop => size.x < MOBILE_BREAKPOINT,
        LayoutMode::Mobile => true,
        LayoutMode::Auto => size.x.min(size.y) < MOBILE_BREAKPOINT,
    }
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        Self::setup_fonts(&cc.egui_ctx);
        catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MOCHA);
        #[cfg(target_arch = "wasm32")]
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);

        let mut app = cc
            .storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default();

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

        app
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
            || self.mobile.sheet == Some(mobile::Sheet::Panel(Tab::ControllerDebug));
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
        #[cfg(not(target_arch = "wasm32"))]
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

        self.ui.update_avg_timer.stop();
    }

    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        canvas::fit(ctx, raw_input);
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

    fn save(&mut self, storage: &mut dyn Storage) {
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
    }
}

// Rendering
impl Citrine {
    pub fn is_mobile(&self, ctx: &Context) -> bool {
        mobile_layout(self.ui.settings.layout, ctx)
    }

    fn render(&mut self, ctx: &Context) {
        if self.is_mobile(ctx) {
            self.render_mobile(ctx);
        } else if self.ui.settings.focus_mode {
            self.render_focus_mode(ctx);
        } else {
            self.render_normal_mode(ctx);
        }
    }

    fn render_normal_mode(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));
        self.toast_strip(ctx);

        self.ui.tab_fit = tabs::fit_titles(ctx, &self.dock);
        CentralPanel::default().show(ctx, |ui| {
            let mut viewer = tabs::TabViewer {
                audio: &mut self.audio,
                commonmark: &mut self.commonmark,
                emulator: &mut self.emulator,
                game_info: &mut self.game_info,
                pads: self.pads.as_ref(),
                events: &mut self.events,
                files: &mut self.files,
                media: &mut self.media,
                online: &mut self.online,
                play: &mut self.play,
                stats: &self.stats,
                ui: &mut self.ui,
            };

            egui_dock::DockArea::new(&mut self.dock)
                .style(egui_dock::Style::from_egui(ctx.style().as_ref()))
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);
        });
    }

    fn media_viewer(&mut self, ctx: &Context) {
        let mut viewer = tabs::TabViewer {
            audio: &mut self.audio,
            commonmark: &mut self.commonmark,
            emulator: &mut self.emulator,
            game_info: &mut self.game_info,
            pads: self.pads.as_ref(),
            events: &mut self.events,
            files: &mut self.files,
            media: &mut self.media,
            online: &mut self.online,
            play: &mut self.play,
            stats: &self.stats,
            ui: &mut self.ui,
        };
        viewer::show(&mut viewer, ctx);
    }

    fn show_toasts(&mut self, ctx: &Context) {
        if let Some(area) = self.ui.toast_area.take() {
            self.toasts.place(area);
        }
        for event in self.toasts.show(ctx) {
            self.events.push(event);
        }
    }

    fn toast_strip(&mut self, ctx: &Context) {
        if !self.toasts.wants_strip() {
            return;
        }
        TopBottomPanel::bottom("toast_strip")
            .exact_height(Toasts::strip_height(ctx))
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| self.toasts.place_strip(ui.max_rect()));
    }

    fn top_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Citrine");
            ui.separator();

            ui.menu_button(icons::FOLDER, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.set_min_width(180.0);

                if ui.button("Load ROM").clicked() {
                    FileLoader::new()
                        .title("Load ROM")
                        .add_filter("Game Boy ROMs", &["gb", "gbc"])
                        .dispatch(self.files.rom_tx.clone());
                }
                if ui.button("Load boot ROM").clicked() {
                    FileLoader::new()
                        .title("Load boot ROM")
                        .add_filter("Game Boy boot ROM", &["bin"])
                        .dispatch(self.files.boot_rom_tx.clone());
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    self.ui.recent.prune();
                    self.ui.recent.name_from_files();
                    if self.ui.recent.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No recent ROMs"));
                    } else {
                        ui.label("Recent");
                        let mut chosen = None;
                        for entry in self.ui.recent.entries() {
                            let name = entry.display_name();
                            let clicked = match entry.rom_key.as_deref() {
                                Some(key) => {
                                    let model = other_console(
                                        self.ui.settings.console_for(key, RomCgbMode::CgbAndGb),
                                    );
                                    let button = egui::Button::new(name).right_text(
                                        egui::containers::menu::SubMenuButton::RIGHT_ARROW,
                                    );
                                    egui::containers::menu::SubMenuButton::from_button(button)
                                        .ui(ui, |ui| {
                                            if ui
                                                .button(format!("Play on {}", console_name(model)))
                                                .clicked()
                                            {
                                                chosen = Some((
                                                    entry.path.clone(),
                                                    Some((key.to_owned(), model)),
                                                ));
                                            }
                                        })
                                        .0
                                        .clicked()
                                }
                                None => ui
                                    .button(name)
                                    .on_hover_text(entry.path.display().to_string())
                                    .clicked(),
                            };
                            if clicked {
                                chosen = Some((entry.path.clone(), None));
                            }
                        }
                        if let Some((path, console)) = chosen {
                            if let Some((key, model)) = console {
                                self.ui.settings.set_game_console(&key, model);
                            }
                            self.load_recent(path);
                        }
                    }
                }
            })
            .response
            .on_hover_text("Files");

            ui.separator();
            self.transport_buttons(ui);
            ui.separator();

            if self.game_info.recognised()
                && ui
                    .button(icons::IDENTIFICATION_CARD)
                    .on_hover_text(Tab::GameInfo.label())
                    .clicked()
            {
                self.open_tab(Tab::GameInfo);
                self.widen_for_game_info();
            }

            if ui
                .button(icons::JOYSTICK)
                .on_hover_text(Tab::Homebrew.label())
                .clicked()
            {
                self.open_tab(Tab::Homebrew);
            }

            let saves_hint = match self.ui.settings.hotkeys.quick_keys() {
                Some(keys) => format!("{} ({keys} quick save / load)", Tab::Saves.label()),
                None => Tab::Saves.label().to_owned(),
            };
            if ui
                .button(icons::FLOPPY_DISK)
                .on_hover_text(saves_hint)
                .clicked()
            {
                self.open_tab(Tab::Saves);
            }

            if ui
                .button(icons::IMAGES)
                .on_hover_text("Screenshots and recordings")
                .clicked()
            {
                self.open_tab(Tab::Media);
            }

            if ui.button(icons::CHART_BAR).on_hover_text("Stats").clicked() {
                self.open_tab(Tab::Stats);
            }

            let entry = self.link_entry();
            let hint = match entry {
                link::LinkEntry::Blocked(why) => why,
                link::LinkEntry::Open if self.emulator.partner.is_some() => "Event partner",
                link::LinkEntry::Open => "Linked game",
                link::LinkEntry::Ready => "Link a second game",
            };
            let enabled = !matches!(entry, link::LinkEntry::Blocked(_));
            if ui
                .add_enabled(enabled, egui::Button::new(tabs::linked::LINK_ICON))
                .on_hover_text(hint)
                .on_disabled_hover_text(hint)
                .clicked()
            {
                self.pick_second_game(false);
            }

            if self.ui.settings.online.is_joined() {
                let invited = self.play.has_unseen_invite();
                let hint = tabs::chat::attention_hint(&self.online, invited);
                let response = ui.button(icons::USERS).on_hover_text(hint);
                if self.online.needs_attention() || invited {
                    widgets::avatar::status_dot(ui, response.rect);
                }
                if response.clicked() {
                    self.open_tab(Tab::Friends);
                }
            }

            if ui.button(icons::GEAR).on_hover_text("Settings").clicked() {
                self.open_tab(Tab::Settings);
            }

            ui.menu_button(icons::INFO, |ui| {
                if ui.button(Tab::Info.label()).clicked() {
                    self.open_tab(Tab::Info);
                }
                if ui.button(Tab::RomInfo.label()).clicked() {
                    self.open_tab(Tab::RomInfo);
                }
            })
            .response
            .on_hover_text("About");

            let focus = HotkeyAction::ToggleFocusMode;
            let focus_hint = match self.ui.settings.hotkeys.key_name(focus) {
                Some(key) => format!("{} ({key})", focus.label()),
                None => focus.label().to_owned(),
            };
            if ui
                .button(icons::FRAME_CORNERS)
                .on_hover_text(focus_hint)
                .clicked()
            {
                self.toggle_focus_mode();
            }

            if self.ui.settings.dev_mode {
                ui.menu_button(icons::CIRCUITRY, |ui| {
                    for tab in [Tab::Disassembly, Tab::Apu, Tab::Registers] {
                        if ui.button(tab.label()).clicked() {
                            self.open_tab(tab);
                        }
                    }
                })
                .response
                .on_hover_text("Hardware views");

                ui.menu_button(icons::BRACKETS_CURLY, |ui| {
                    for tab in [
                        Tab::TimeControl,
                        Tab::Performance,
                        Tab::DebugActions,
                        Tab::AudioDebug,
                        Tab::ControllerDebug,
                        Tab::E2ETest,
                    ] {
                        if ui.button(tab.label()).clicked() {
                            self.open_tab(tab);
                        }
                    }
                })
                .response
                .on_hover_text("Debug tools");
            }

            ui.separator();

            if ui
                .button("Reset layout")
                .on_hover_text("Restore the default tab layout")
                .clicked()
            {
                self.dock = DockState::new(vec![Tab::GameBoy]);
                if self.ui.settings.online.is_joined() {
                    self.open_tab(Tab::Friends);
                }
            }
        });
    }

    fn transport_buttons(&mut self, ui: &mut egui::Ui) {
        let loaded = self.emulator.gb.cartridge.has_rom_loaded;
        let running = self.emulator.running;

        let play_icon = if running { icons::PAUSE } else { icons::PLAY };
        let play_hint = if running { "Pause" } else { "Resume" };
        if self
            .shortcut_button(ui, play_icon, play_hint, HotkeyAction::TogglePause, loaded)
            .clicked()
        {
            self.run_hotkey(HotkeyAction::TogglePause);
        }

        if self
            .shortcut_button(
                ui,
                icons::ARROW_COUNTER_CLOCKWISE,
                "Reset the game",
                HotkeyAction::SoftReset,
                loaded,
            )
            .clicked()
        {
            self.run_hotkey(HotkeyAction::SoftReset);
        }

        if self
            .shortcut_button(
                ui,
                icons::CAMERA,
                "Screenshot",
                HotkeyAction::Screenshot,
                loaded,
            )
            .clicked()
        {
            self.run_hotkey(HotkeyAction::Screenshot);
        }

        if self.recording_button(ui, None).clicked() {
            self.run_hotkey(HotkeyAction::ToggleRecording);
        }

        let muted = self.ui.settings.muted;
        let mute_icon = if muted {
            icons::SPEAKER_SLASH
        } else {
            icons::SPEAKER_HIGH
        };
        let mute_hint = if muted { "Unmute" } else { "Mute" };
        if self
            .shortcut_button(ui, mute_icon, mute_hint, HotkeyAction::ToggleMute, true)
            .clicked()
        {
            self.run_hotkey(HotkeyAction::ToggleMute);
        }
    }

    fn shortcut_button(
        &self,
        ui: &mut egui::Ui,
        icon: &str,
        hint: &str,
        action: HotkeyAction,
        enabled: bool,
    ) -> egui::Response {
        let keys = self.ui.settings.hotkeys.keys_for(action);
        let hint = match keys.first() {
            Some(hotkey) => format!("{hint} ({hotkey})"),
            None => hint.to_string(),
        };
        ui.add_enabled(enabled, egui::Button::new(icon))
            .on_hover_text(hint)
    }

    fn toggle_focus_mode(&mut self) {
        self.ui.settings.focus_mode = !self.ui.settings.focus_mode;
        if self.ui.settings.focus_mode {
            let names = self.focus_exit_names();
            let keys = if names.is_empty() {
                "Esc".to_owned()
            } else {
                names.join(" or ")
            };
            self.toasts.add(Toast::hint(
                format!("Press {keys} to leave focus mode"),
                crate::utils::reduced_motion(),
            ));
        } else {
            self.toasts.dismiss_all_toasts();
        }
    }

    fn focus_exit_names(&self) -> Vec<String> {
        let hotkeys = &self.ui.settings.hotkeys;
        let toggle = HotkeyAction::ToggleFocusMode;
        let mut names: Vec<String> = hotkeys
            .keys_for(HotkeyAction::LeaveFocusMode)
            .iter()
            .chain(&hotkeys.keys_for(toggle))
            .map(ToString::to_string)
            .collect();
        names.extend(hotkeys.pads_for(toggle).iter().map(ToString::to_string));
        names.dedup();
        names
    }

    fn run_pad_shortcuts(&mut self, presses: Vec<PadButton>) {
        for pad in presses {
            if let Some(action) = self.ui.settings.hotkeys.action_for_pad(pad)
                && !action.is_hold()
            {
                self.run_hotkey(action);
            }
        }
    }

    fn render_focus_mode(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                let area = ui.max_rect();
                self.ui.toast_area = Some(area);
                let place = self.ui.settings.focus_place;
                let screen = self
                    .emulator
                    .ui_placed(ui, area, |size| place.rect(area, size));
                let response = ui
                    .interact(
                        screen,
                        ui.id().with("focus-screen"),
                        egui::Sense::click_and_drag(),
                    )
                    .on_hover_cursor(egui::CursorIcon::Move);
                if response.double_clicked() {
                    self.ui.settings.focus_place = focus::ScreenPlace::default();
                    self.ui.settings.dirty = true;
                } else if response.dragged() {
                    self.ui.settings.focus_place =
                        place.dragged(response.drag_delta(), area, screen.size());
                }
                if response.drag_stopped() {
                    self.ui.settings.dirty = true;
                }
            });
    }

    fn tools_node(&self) -> Option<(SurfaceIndex, NodeIndex)> {
        Tab::iter()
            .filter(|t| *t != Tab::GameBoy && *t != Tab::Friends)
            .find_map(|t| self.dock.find_tab(&t))
            .map(|(surface_idx, node_idx, _)| (surface_idx, node_idx))
    }

    fn media_would_cover_game(&self) -> bool {
        let Some((gb_surface, gb_node, _)) = self.dock.find_tab(&Tab::GameBoy) else {
            return false;
        };
        let media = match self.dock.find_tab(&Tab::Media) {
            Some((surface_idx, node_idx, _)) => Some((surface_idx, node_idx)),
            None => self.tools_node(),
        };
        media == Some((gb_surface, gb_node))
    }

    fn sync_friend_tab(&mut self, wanted: bool) {
        let found = self.dock.find_tab(&Tab::Playing);
        match (wanted, found) {
            (false, Some(location)) => {
                self.dock.remove_tab(location);
            }
            (true, None) => match self.dock.find_tab(&Tab::GameBoy) {
                Some((surface, gb_node, _)) if surface == SurfaceIndex::main() => {
                    self.dock
                        .main_surface_mut()
                        .split_right(gb_node, 0.5, vec![Tab::Playing]);
                }
                _ => self.open_tab(Tab::Playing),
            },
            _ => {}
        }
    }

    fn open_tab(&mut self, tab: Tab) {
        if let Some((surface_idx, node_idx, tab_idx)) = self.dock.find_tab(&tab) {
            self.dock.set_active_tab((surface_idx, node_idx, tab_idx));
            return;
        }

        if tab == Tab::Friends
            && let Some((surface, gb_node, _)) = self.dock.find_tab(&Tab::GameBoy)
            && surface == SurfaceIndex::main()
        {
            self.dock
                .main_surface_mut()
                .split_left(gb_node, FRIENDS_DOCK_SHARE, vec![tab]);
            return;
        }

        let tools_node = self.tools_node();
        let gb_loc = self.dock.find_tab(&Tab::GameBoy);
        if let Some((surface_idx, node_idx)) = tools_node {
            self.dock
                .set_focused_node_and_surface((surface_idx, node_idx));
            self.dock.main_surface_mut().push_to_focused_leaf(tab);
        } else if let Some((_gb_surface, gb_node, _)) = gb_loc {
            self.dock
                .main_surface_mut()
                .split_right(gb_node, 0.6, vec![tab]);
        } else {
            self.dock.main_surface_mut().push_to_focused_leaf(tab);
        }
    }
}

impl Citrine {
    fn watch_game_info(&mut self) {
        let key = self.emulator.rom_key().map(str::to_owned);
        if key.as_deref() != self.game_info.rom_key() {
            let rom = self.emulator.gb.cartridge.rom_bytes();
            let saved_at = key
                .as_deref()
                .and_then(|key| self.emulator.store.battery_saved_at(key));
            self.game_info.set_rom(key.as_deref(), &rom, saved_at);
            if !self.game_info.recognised() {
                self.close_game_info();
            }
        }
        let open = self.dock.find_tab(&Tab::GameInfo).is_some()
            || self.mobile.sheet == Some(mobile::Sheet::Panel(Tab::GameInfo));
        let now = web_time::Instant::now();
        let read = open && self.game_info.due(now);
        let progress = self.game_info.progress_due(now);
        if !read && !progress {
            return;
        }
        let gb = &self.emulator.gb;
        let battery = gb.cartridge.battery_ram();
        let wram: Vec<u8> = (0..2)
            .filter_map(|bank| gb.wram_bank(bank))
            .flatten()
            .copied()
            .collect();
        let unix = crate::storage::now_unix();
        self.game_info.read(&battery, &wram, unix);
        if progress
            && let (Some(rom), Some(state)) = (self.game_info.rom_key(), self.game_info.progress())
        {
            let day = crate::stats::day_key(crate::stats::local_date(unix));
            self.stats.recorder().progress(rom, &day, state);
        }
    }

    fn widen_for_game_info(&mut self) {
        let Some((surface, node, _)) = self.dock.find_tab(&Tab::GameInfo) else {
            return;
        };
        let Some(parent) = node.parent() else {
            return;
        };
        if surface == SurfaceIndex::main()
            && node.is_right()
            && let egui_dock::Node::Horizontal(split) = &mut self.dock.main_surface_mut()[parent]
            && split.fraction > GAME_INFO_DOCK_SHARE
        {
            split.fraction = GAME_INFO_DOCK_SHARE;
        }
    }

    fn close_game_info(&mut self) {
        if let Some(location) = self.dock.find_tab(&Tab::GameInfo) {
            self.dock.remove_tab(location);
        }
        if self.mobile.sheet == Some(mobile::Sheet::Panel(Tab::GameInfo)) {
            self.mobile.close();
        }
    }

    fn watch_cloud_game(&mut self) {
        let rom = self.emulator.rom_key().map(str::to_owned);
        if rom == self.cloud_rom {
            return;
        }
        self.cloud_rom = rom.clone();
        let title = self.emulator.game_name().to_owned();
        self.online.cloud_game(rom.clone().map(|rom| (rom, title)));
        let Some(rom) = rom else {
            return;
        };
        if !self.online.cloud_enabled()
            || (self.online.cloud_settled(&rom) && !self.online.is_connected())
        {
            return;
        }
        self.online.cloud_hold(&rom);
        let was_running = match self.cloud_hold.take() {
            Some(hold) => hold.was_running,
            None => self.emulator.running,
        };
        self.cloud_hold = Some(CloudHold {
            since: web_time::Instant::now(),
            was_running,
            told: false,
        });
        self.emulator.running = false;
    }

    fn cloud_conflict(&mut self, ctx: &Context) {
        let Some(conflict) = self.online.cloud_ask() else {
            return;
        };
        let answer = widgets::cloud_conflict::show(ctx, conflict);
        let (rom, slot) = (conflict.rom.clone(), conflict.slot.clone());
        match answer {
            None => {}
            Some(widgets::cloud_conflict::Answer::NotNow) => self.online.cloud_dismiss(&rom, &slot),
            Some(widgets::cloud_conflict::Answer::Choose(choice)) => {
                if let Err(err) = self.online.cloud_resolve(&rom, &slot, choice) {
                    self.toasts
                        .error(format!("Could not keep that copy: {err}"));
                }
                self.ui.snapshots.invalidate();
            }
        }
    }

    fn settle_cloud_game(&mut self, now: web_time::Instant) {
        if let Some(hold) = &mut self.cloud_hold {
            let settled = self
                .cloud_rom
                .as_deref()
                .is_none_or(|rom| self.online.cloud_settled(rom));
            let waited = now.duration_since(hold.since);
            if !settled && waited < CLOUD_HOLD_LIMIT {
                if waited >= CLOUD_HOLD_TOAST && !hold.told {
                    hold.told = true;
                    self.toasts.info("Getting your save from the cloud");
                }
                return;
            }
            let was_running = hold.was_running;
            self.cloud_hold = None;
            self.online.cloud_release();
            if self.online.cloud_take_reload()
                && let Err(err) = self.emulator.reload_battery()
            {
                self.toasts
                    .error(format!("Could not load the cloud save: {err}"));
            }
            self.emulator.running = was_running;
            return;
        }
        if self.online.cloud_take_reload() {
            match self.emulator.reload_battery() {
                Ok(()) => {
                    self.toasts.success("Loaded the save from the cloud");
                }
                Err(err) => {
                    self.toasts
                        .error(format!("Could not load the cloud save: {err}"));
                }
            }
        }
        if self.online.cloud_take_changed() {
            self.ui.snapshots.invalidate();
        }
    }

    fn update_online(&mut self, ctx: &Context) {
        let now = web_time::Instant::now();
        let touched = ctx.input(|i| !i.events.is_empty() || i.pointer.any_down());
        let held = self.emulator.held_buttons();
        if touched || held.bits() != self.activity.held.bits() {
            self.activity.last_input = now;
        }
        self.activity.held = held;
        let away = now.duration_since(self.activity.last_input) >= crate::online::AWAY_AFTER;
        let cartridge = &self.emulator.gb.cartridge;
        let game = (self.ui.settings.online.show_playing && cartridge.has_rom_loaded)
            .then_some(cartridge.header.sha1);
        self.online.set_activity(away, game);
        let settings = &self.ui.settings.online;
        self.online
            .set_cloud(settings.cloud_owner(), settings.cloud_saves);
        let sync = settings.cloud_saves && settings.sync_settings;
        self.online.update(now);
        if self
            .online
            .sync_settings(&mut self.ui.settings.synced, sync)
        {
            let values = self.ui.settings.synced.values();
            synced::adopt(&mut self.ui.settings, &values);
            self.ui.settings.unsaved = true;
        }
        self.settle_cloud_game(now);
        self.play.update(&mut self.online, now);

        if let Some(joined) = self.online.take_joined() {
            let online = &mut self.ui.settings.online;
            online.remember(
                &joined.address,
                &joined.relay_id,
                &joined.password,
                &joined.account,
            );
            self.ui.settings.dirty = true;
            self.ui.join = Default::default();
            if !self.is_mobile(ctx) {
                self.open_tab(Tab::Friends);
            }
        }
        if let Some(account) = self.online.take_account_change() {
            self.ui.settings.online.note_account(account);
            self.ui.settings.dirty = true;
        }
        if !self.ui.settings.online.is_joined() {
            if let Some(location) = self.dock.find_tab(&Tab::Friends) {
                self.dock.remove_tab(location);
            }
            if self.mobile.sheet == Some(mobile::Sheet::Panel(Tab::Friends)) {
                self.mobile.close();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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
        let running = emulator.running && emulator.gb.cartridge.has_rom_loaded;
        if self
            .shots
            .due(std::time::Instant::now(), settings.every, running, || {
                crate::discord::frame_key(emulator.gb.frame().as_slice())
            })
        {
            let (width, height, rgba) = emulator.screenshot(crate::discord::SHOT_SCALE);
            if let Some(png) = crate::utils::screenshot::encode_png(width, height, &rgba)
                .filter(|png| png.len() <= citrine_gb_net::MAX_SCREEN_BYTES)
            {
                self.online.send_screen(png);
            }
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
                fast_forward: false,
            };
            let rom = self.emulator.rom_key().map(str::to_string);
            let name = self.emulator.game_name().to_string();
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
                }
            }
            if let Some(play) = events.together {
                let activity = crate::stats::Activity {
                    together: true,
                    ..Default::default()
                };
                recorder.play(&play.rom, &play.name, play.played, activity, now, &day);
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

    fn settle_play(&mut self, ctx: &Context) {
        if let Some(ended) = &self.ui.playing.ended
            && self.play.invites().iter().any(|i| i.from == ended.friend)
        {
            self.ui.playing.ended = None;
        }
        match self.play.session() {
            Some(session) if session.is_live() => {
                let friend_tab = session.has_friend_tab();
                if session.stage() != Stage::Inviting && !self.ui.playing.shown {
                    self.ui.playing.shown = true;
                    if self.is_mobile(ctx) {
                        self.close_mobile_sheet();
                    }
                }
                if !self.is_mobile(ctx) {
                    self.sync_friend_tab(friend_tab);
                }
                return;
            }
            Some(session) => {
                let friend = session.friend();
                let played = session.mode().is_some();
                if let Stage::Ended(ending) = session.stage() {
                    let name = tabs::playing::friend_name(&self.online, friend);
                    self.ui.playing.ended = tabs::playing::ending_line(&name, ending, played)
                        .map(|line| tabs::playing::Ended { friend, line });
                }
                if let Some(session) = self.play.close()
                    && let Err(err) = self.emulator.finish_session(session)
                {
                    self.toasts.error(format!("Couldn't keep the game: {err}"));
                }
                self.ui.playing.asking_stop = false;
                self.ui.playing.shown = false;
                if let Some(next) = self.ui.playing.switch_to.take() {
                    self.play_again(next);
                    return;
                }
            }
            None => {}
        }
        if self.keeps_link_tab() {
            return;
        }
        if let Some(location) = self.dock.find_tab(&Tab::Playing) {
            self.dock.remove_tab(location);
        }
        if self.mobile.sheet == Some(mobile::Sheet::Panel(Tab::Playing)) {
            self.close_mobile_sheet();
        }
    }

    fn play_picker(&mut self, ctx: &Context) {
        if std::mem::take(&mut self.ui.playing.wants_copy) {
            self.ui.playing.copy_for = true;
            self.try_start_audio();
            crate::utils::file_loader::FileLoader::new()
                .title("Open your copy of the game")
                .add_filter("Game Boy ROMs", &["gb", "gbc"])
                .dispatch(self.files.play_rom_tx.clone());
        }
        let Some((target, pick)) = tabs::playing::picker(
            ctx,
            &mut self.ui.playing,
            &self.online,
            &self.ui.recent,
            &self.play,
        ) else {
            return;
        };
        match pick {
            tabs::playing::Pick::Homebrew(game) => {
                self.picked_for(target, |app| app.handle_load_rom_data(game.data()));
            }
            #[cfg(not(target_arch = "wasm32"))]
            tabs::playing::Pick::Recent(path) => {
                self.picked_for(target, |app| app.load_recent(path));
            }
            tabs::playing::Pick::File => {
                self.ui.playing.file_for = Some(target);
                self.try_start_audio();
                crate::utils::file_loader::FileLoader::new()
                    .title("Open a game to play together")
                    .add_filter("Game Boy ROMs", &["gb", "gbc"])
                    .dispatch(self.files.play_rom_tx.clone());
            }
            tabs::playing::Pick::Theirs => {
                if let tabs::playing::PickFor::Join(invite) = target {
                    self.answer_ask(&invite);
                }
            }
            tabs::playing::Pick::Nothing => match target {
                tabs::playing::PickFor::Join(invite) => self.join_with(&invite, MyGame::Nothing),
                tabs::playing::PickFor::Change => self.put_my_game_away(),
                tabs::playing::PickFor::Invite(_) => {}
            },
        }
    }

    fn picked_for(&mut self, target: tabs::playing::PickFor, load: impl FnOnce(&mut Self)) {
        match target {
            tabs::playing::PickFor::Invite(friend) => {
                load(self);
                if self.emulator.open_game().is_some() {
                    self.play_again(friend);
                }
            }
            tabs::playing::PickFor::Join(invite) => {
                load(self);
                if let Some(file) = self.emulator.game_file() {
                    self.join_with(&invite, MyGame::Own(file));
                }
            }
            tabs::playing::PickFor::Change => {
                if !self.play.is_live() {
                    return;
                }
                self.put_my_game_away();
                load(self);
                let file = self.emulator.game_file();
                if let (Some(session), Some(file)) = (self.play.session_mut(), file)
                    && !session.bring(MyGame::Own(file))
                {
                    self.toasts.error("Couldn't change the game");
                }
            }
        }
    }

    fn put_my_game_away(&mut self) {
        let Some(session) = self.play.session_mut() else {
            return;
        };
        if session.my_game().is_none() || !session.bring(MyGame::Nothing) {
            return;
        }
        if let Some(back) = session.take_returned() {
            self.emulator.keep_returned(back);
        }
    }

    fn join_with(&mut self, invite: &crate::play::IncomingInvite, game: MyGame) {
        let joined = tabs::playing::join_with(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            invite,
            game,
        );
        if !joined {
            self.toasts.error("Couldn't answer the invite");
        }
    }

    fn answer_ask(&mut self, invite: &crate::play::IncomingInvite) {
        let joined = tabs::playing::join(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            &self.ui.recent,
            invite,
        );
        if !joined {
            self.toasts.error("Couldn't answer the invite");
        }
    }

    fn play_again(&mut self, friend: citrine_gb_net::OnlineId) {
        let invited = tabs::playing::invite(
            &mut self.play,
            &mut self.online,
            &mut self.emulator,
            &mut self.ui.playing,
            &self.ui.recent,
            friend,
        );
        if !invited {
            self.toasts.error("Couldn't send the invite");
        }
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

// Audio
impl Citrine {
    pub fn try_start_audio(&mut self) {
        let Some(audio) = &self.audio else {
            return;
        };
        if audio.is_running() {
            return;
        }
        self.restart_audio();
    }

    fn unlock_audio_on_interaction(&mut self, ctx: &Context) {
        let interacted = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key { pressed: true, .. }
                            | egui::Event::Touch {
                                phase: egui::TouchPhase::Start,
                                ..
                            }
                    )
                })
        });
        if !interacted {
            return;
        }

        let stalled = std::mem::take(&mut self.audio_stalled);
        let Some(audio) = self.audio.as_mut() else {
            return;
        };

        audio.mark_user_activated();

        if audio.is_running() && !stalled {
            audio.resume();
            return;
        }

        self.restart_audio();
    }

    fn restart_audio(&mut self) {
        let Some(audio) = &mut self.audio else {
            return;
        };

        let device = self.ui.settings.audio_device.clone();
        match audio.start(device.as_deref(), self.ui.settings.audio_queue_ms) {
            Ok(link) => {
                self.emulator.attach_audio(link);
                self.ui.settings.dirty = true;
            }
            Err(err) => {
                self.toasts.error(format!("Failed to start audio: {err}"));
            }
        }
    }
}

impl Citrine {
    fn drain_file_channels(&mut self) {
        while let Ok(file) = self.files.rom_rx.try_recv() {
            self.handle_load_rom(file);
        }
        while let Ok(file) = self.files.play_rom_rx.try_recv() {
            if std::mem::take(&mut self.ui.playing.copy_for) {
                let Some(session) = self.play.session_mut() else {
                    continue;
                };
                let title = tabs::playing::game_title(session);
                if !session.hand_rom(&file.data) {
                    self.toasts.error(format!("That file isn't {title}"));
                }
                continue;
            }
            match self.ui.playing.file_for.take() {
                Some(target) => self.picked_for(target, |app| app.handle_load_rom(file)),
                None => self.handle_load_rom(file),
            }
        }
        while let Ok(file) = self.files.second_rom_rx.try_recv() {
            self.link_picked(file);
        }
        while let Ok(file) = self.files.boot_rom_rx.try_recv() {
            self.handle_load_boot_rom(file);
        }
        while let Ok(files) = self.files.sav_rx.try_recv() {
            self.handle_import_saves(files);
        }
        #[cfg(not(target_arch = "wasm32"))]
        while let Ok(dir) = self.files.folder_rx.try_recv() {
            self.handle_export_e2e(&dir);
        }
        while let Ok(outcome) = self.files.save_rx.try_recv() {
            match outcome {
                SaveOutcome::Saved(name) => self.toasts.success(format!("Saved to '{name}'")),
                SaveOutcome::SavedPair(first, second) => self
                    .toasts
                    .success(format!("Saved to '{first}' and '{second}'")),
                SaveOutcome::Failed(err) => self.toasts.error(format!("Failed to save: {err}")),
                SaveOutcome::Cancelled => continue,
            };
        }
    }

    fn handle_snapshot_hotkeys(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() || self.ui.capture.is_waiting() {
            self.quick_load_hold = None;
            return;
        }

        let (pressed, modifiers, held_keys) = ctx.input(|i| {
            let pressed: Vec<egui::Key> = i
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => Some(*key),
                    _ => None,
                })
                .collect();
            let held: Vec<egui::Key> = egui::Key::ALL
                .iter()
                .copied()
                .filter(|key| i.key_down(*key))
                .collect();
            (pressed, i.modifiers, held)
        });

        for key in pressed {
            if let Some(action) = self.ui.settings.hotkeys.action_for(key, modifiers)
                && !action.is_hold()
            {
                self.run_hotkey(action);
            }
        }

        let hold_active = held_keys.iter().any(|key| {
            self.ui
                .settings
                .hotkeys
                .action_for(*key, modifiers)
                .is_some_and(|action| action.is_hold())
        });

        if !hold_active {
            self.quick_load_hold = None;
            return;
        }

        let started = *self
            .quick_load_hold
            .get_or_insert_with(web_time::Instant::now);
        if started.elapsed() >= QUICK_LOAD_HOLD {
            self.quick_load_hold = None;
            self.run_hotkey(HotkeyAction::QuickLoad);
        }
    }

    pub fn run_hotkey(&mut self, action: HotkeyAction) {
        let needs_rom = matches!(
            action,
            HotkeyAction::QuickSave
                | HotkeyAction::QuickSaveNewSlot
                | HotkeyAction::QuickLoad
                | HotkeyAction::Screenshot
                | HotkeyAction::SoftReset
        );
        if needs_rom && self.emulator.rom_key().is_none() {
            self.toasts.error("No ROM loaded");
            return;
        }

        match action {
            HotkeyAction::QuickSave => self.save_to_slot(self.ui.settings.quick_slot),
            HotkeyAction::QuickSaveNewSlot => {
                let key = self.emulator.rom_key().unwrap_or_default().to_owned();
                self.save_to_slot(self.emulator.store.next_free_slot(&key));
            }
            HotkeyAction::QuickLoad => {
                let slot = self.ui.settings.quick_slot;
                match self.emulator.load_snapshot(slot) {
                    Ok(true) => self.toasts.success(snapshot_message(
                        SnapshotNews::Loaded,
                        &self.emulator.snapshot_name(slot),
                    )),
                    Ok(false) => self.toasts.info(snapshot_message(
                        SnapshotNews::Empty,
                        &self.emulator.snapshot_name(slot),
                    )),
                    Err(err) => self.toasts.error(format!("Quick load failed: {err:?}")),
                };
                self.ui.snapshots.invalidate();
            }
            HotkeyAction::Screenshot => self.take_screenshot(),
            HotkeyAction::ToggleRecording => self.toggle_recording(),
            HotkeyAction::TogglePause => {
                self.emulator.running = !self.emulator.running;
                if !self.emulator.running {
                    self.emulator.release_all();
                }
            }
            HotkeyAction::SoftReset => self.ask_reset(),
            HotkeyAction::ToggleMute => {
                self.ui.settings.muted = !self.ui.settings.muted;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::ToggleFocusMode => self.toggle_focus_mode(),
            HotkeyAction::LeaveFocusMode => {
                if self.ui.settings.focus_mode {
                    self.toggle_focus_mode();
                }
            }
            HotkeyAction::ToggleTouchControls => {
                let show = &mut self.ui.settings.touch.show_controls;
                *show = !*show;
                self.ui.settings.dirty = true;
            }
            HotkeyAction::SwitchGame => self.switch_game(),
        }
    }

    fn save_to_slot(&mut self, slot: usize) {
        match self.emulator.save_snapshot(slot) {
            Ok(()) => self.toasts.success(snapshot_message(
                SnapshotNews::Saved,
                &self.emulator.snapshot_name(slot),
            )),
            Err(err) => self.toasts.error(format!("Quick save failed: {err:?}")),
        };
        self.ui.snapshots.invalidate();
    }

    fn name_games(&mut self) {
        if let Some(key) = self.emulator.rom_key()
            && self
                .ui
                .settings
                .game_names
                .learn(key, self.emulator.game_name())
        {
            self.ui.settings.dirty = true;
        }
        self.media.name_games(&self.ui.settings.game_names);
        if self.online.cloud_enabled() {
            let names = &self.ui.settings.game_names;
            for recent in self.ui.recent.entries() {
                if let Some(key) = &recent.rom_key {
                    let name = recent.display_name();
                    self.online.cloud_name(key, names.name_or(Some(key), &name));
                }
            }
            for (key, name) in names.iter() {
                self.online.cloud_name(key, name);
            }
        }
    }

    fn quick_load_overlay(&self, ctx: &Context) {
        let Some(started) = self.quick_load_hold else {
            return;
        };
        let progress =
            (started.elapsed().as_secs_f32() / QUICK_LOAD_HOLD.as_secs_f32()).clamp(0.0, 1.0);
        let name = self.emulator.snapshot_name(self.ui.settings.quick_slot);

        egui::Area::new("quick_load_hold".into())
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(220.0);
                    ui.label(format!("Hold to load '{name}'"));
                    ui.add(egui::ProgressBar::new(progress).desired_height(6.0));
                });
            });
    }

    fn handle_load_rom(&mut self, file: PickedFile) {
        self.try_start_audio();
        let rom = Rom::new(&file.data);
        let model = self.ui.settings.console_for_rom(&rom);
        if let Err(err) = self.emulator.load_rom(
            &rom,
            model,
            #[cfg(not(target_arch = "wasm32"))]
            file.path.as_deref(),
        ) {
            self.toasts.error(format!("Failed to load ROM: {}", err));
        } else {
            self.emulator.name_game(Some(&file.name));
            self.toasts.success(format!("Loaded ROM '{}'", file.name));
            self.ui.settings.dirty = true;
            self.close_mobile_sheet();

            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = file.path.as_deref() {
                let title = self.emulator.game_name().to_string();
                let header = &self.emulator.gb.cartridge.header;
                let both = plays_on_both(header.cgb_mode);
                let key = both.then(|| header.sha256_hex_string());
                self.ui.recent.record(path, &title, key);
            }

            if self.emulator.imported_legacy_save {
                self.toasts
                    .success("Imported the save file found next to the ROM");
            } else if self.emulator.save_loaded {
                self.toasts.success("Loaded save data");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_recent(&mut self, path: std::path::PathBuf) {
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(err) => {
                self.toasts
                    .error(format!("Could not open {}: {err}", path.display()));
                self.ui.recent.remove(&path);
                return;
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.handle_load_rom(PickedFile {
            name,
            data,
            path: Some(path),
        });
    }

    fn handle_import_saves(&mut self, files: Vec<PickedFile>) {
        let sorted = match crate::utils::save_files::sort(files) {
            Ok(sorted) => sorted,
            Err(err) => {
                self.toasts.error(err);
                return;
            }
        };
        let blocked = self.emulator.import_blocker(sorted.clock.is_some());
        if blocked.is_none() && self.emulator.has_been_playing() {
            self.pending_import = Some(PendingImport {
                files: sorted,
                rom_key: self.emulator.rom_key().map(str::to_owned),
                was_running: self
                    .pending_import
                    .as_ref()
                    .map_or(self.emulator.running, |pending| pending.was_running),
            });
            self.emulator.running = false;
            self.emulator.release_all();
            return;
        }
        self.import_saves(sorted);
    }

    fn ask_reset(&mut self) {
        if self.reset_without_asking {
            self.reset_game();
            return;
        }
        if self.pending_reset.is_none() {
            self.pending_reset = Some(PendingReset {
                was_running: self.emulator.running,
                stop_asking: false,
            });
            self.emulator.running = false;
            self.emulator.release_all();
        }
    }

    fn reset_game(&mut self) {
        self.emulator.restart();
        self.toasts.success("Game reset");
    }

    fn reset_confirm(&mut self, ctx: &Context) {
        let Some(pending) = &mut self.pending_reset else {
            return;
        };
        if self.emulator.rom_key().is_none() {
            self.pending_reset = None;
            return;
        }
        let choice = widgets::confirm::confirm_with_option(
            ctx,
            "reset_confirm",
            "Reset the game?",
            "Reset",
            (RESET_STOP_ASKING, &mut pending.stop_asking),
        );
        if let Some(confirmed) = choice {
            self.finish_reset(confirmed);
        }
    }

    fn finish_reset(&mut self, confirmed: bool) {
        let Some(pending) = self.pending_reset.take() else {
            return;
        };
        self.emulator.running = pending.was_running;
        if confirmed {
            self.reset_without_asking = pending.stop_asking;
            self.reset_game();
        }
    }

    fn ask_play_on(&mut self, model: GbModel) {
        if self.emulator.rom_key().is_none()
            || self.emulator.console() == model
            || !plays_on_both(self.emulator.gb.cartridge.header.cgb_mode)
        {
            return;
        }
        if !self.emulator.has_been_playing() {
            self.play_on(model);
            return;
        }
        if self.pending_console.is_none() {
            self.pending_console = Some(PendingConsole {
                model,
                rom_key: self.emulator.rom_key().map(str::to_owned),
                was_running: self.emulator.running,
            });
            self.emulator.running = false;
            self.emulator.release_all();
        }
    }

    fn console_confirm(&mut self, ctx: &Context) {
        let Some(pending) = &self.pending_console else {
            return;
        };
        if pending.rom_key.as_deref() != self.emulator.rom_key() {
            self.pending_console = None;
            return;
        }
        let choice = widgets::confirm::confirm(
            ctx,
            "console_confirm",
            &format!("Restart the game on {}?", console_name(pending.model)),
            "Restart",
        );
        if let Some(confirmed) = choice {
            self.finish_play_on(confirmed);
        }
    }

    fn finish_play_on(&mut self, confirmed: bool) {
        let Some(pending) = self.pending_console.take() else {
            return;
        };
        if confirmed {
            self.play_on(pending.model);
        } else {
            self.emulator.running = pending.was_running;
        }
    }

    fn play_on(&mut self, model: GbModel) {
        let Some(key) = self.emulator.rom_key().map(str::to_owned) else {
            return;
        };
        self.ui.settings.set_game_console(&key, model);
        match self.emulator.restart_on(model) {
            Ok(()) => {
                self.toasts
                    .success(format!("Restarted on {}", console_name(model)));
            }
            Err(err) => {
                self.toasts
                    .error(format!("Could not restart the game: {err:?}"));
            }
        }
        self.ui.settings.dirty = true;
        self.ui.snapshots.invalidate();
    }

    fn note_console_switch(&mut self) {
        if let Some(model) = self.emulator.take_console_switch() {
            self.toasts.success(format!(
                "Switched to {}, where this save was made",
                console_name(model)
            ));
            self.ui.settings.dirty = true;
        }
    }

    fn import_confirm(&mut self, ctx: &Context) {
        let Some(pending) = &self.pending_import else {
            return;
        };
        if pending.rom_key.as_deref() != self.emulator.rom_key() {
            self.pending_import = None;
            return;
        }
        let choice = widgets::confirm::confirm(
            ctx,
            "import_confirm",
            "Restart the game with this save? Anything not saved in the game is lost.",
            "Import",
        );
        let Some(confirmed) = choice else {
            return;
        };
        let Some(pending) = self.pending_import.take() else {
            return;
        };
        if confirmed {
            self.import_saves(pending.files);
        } else {
            self.emulator.running = pending.was_running;
        }
    }

    fn import_saves(&mut self, sorted: SaveFiles) {
        let save_name = sorted.save.as_ref().map(|file| file.name.clone());
        let (clock_name, clock) = sorted.clock.unzip();
        let result = self
            .emulator
            .import_save(sorted.save.as_ref().map(|file| file.data.as_slice()), clock);
        if let Ok(SaveImport::Imported) = result {
            self.emulator.restart();
        }
        match (result, save_name, clock_name) {
            (Ok(SaveImport::Imported), Some(save), Some(clock)) => {
                self.toasts
                    .success(format!("Restarted with '{save}' and '{clock}'"));
            }
            (Ok(SaveImport::Imported), Some(save), None) => {
                self.toasts.success(format!("Restarted with '{save}'"));
            }
            (Ok(SaveImport::Imported), None, Some(clock)) => {
                self.toasts
                    .success(format!("Restarted with the clock from '{clock}'"));
            }
            (Ok(SaveImport::NoClock), _, Some(clock)) => {
                self.toasts.error(format!(
                    "This game has no clock, so '{clock}' can't be used"
                ));
            }
            (Ok(SaveImport::NoRom), _, _) => {
                self.toasts.error("Load a ROM before importing a save");
            }
            (Ok(_), _, _) => {}
            (Err(err), _, _) => {
                self.toasts.error(format!("Failed to import save: {err:?}"));
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        let mut saves = Vec::new();
        for file in dropped {
            let Some(file) = picked_from_drop(file) else {
                continue;
            };
            match crate::utils::save_files::kind_of(&file.name) {
                FileKind::Rom if self.ui.linked.picking => self.link_picked(file),
                FileKind::Rom => self.handle_load_rom(file),
                FileKind::Save | FileKind::Clock => saves.push(file),
                FileKind::Other => {
                    self.toasts
                        .error(format!("'{}' is not a ROM or a save file", file.name));
                }
            }
        }
        if !saves.is_empty() {
            self.handle_import_saves(saves);
        }
    }

    fn handle_load_boot_rom(&mut self, file: PickedFile) {
        self.try_start_audio();
        self.emulator.gb.load_boot_rom(&file.data);
        self.ui.settings.dirty = true;
        self.toasts.success("Boot ROM loaded");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_export_e2e(&mut self, dir: &std::path::Path) {
        if self.ui.e2e.title.is_empty() {
            self.toasts.error("Please enter a title for the E2E test");
            return;
        }

        let e2e = self
            .emulator
            .gb
            .create_e2e_test(&self.ui.e2e.title, &self.ui.e2e.description);

        if let Err(err) = e2e.export(dir) {
            self.toasts.error(format!("Failed to export E2E: {}", err));
        } else {
            self.toasts
                .success(format!("Exported E2E to '{}'", dir.display()));
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
        }
    }

    fn media_saved(&mut self, id: MediaId, touch: bool) {
        let kind = id.kind().unwrap_or(MediaKind::Screenshot);
        let in_play = touch || self.ui.settings.focus_mode || self.media_would_cover_game();
        let toast = self
            .toasts
            .success(format!("{} saved to Media", kind.label()));
        if in_play {
            toast.button("Open", events::AppEvent::ShowMedia { id });
        } else {
            self.show_media(id, touch);
        }
    }

    fn media_added(&mut self, id: MediaId) {
        let kind = id.kind().unwrap_or(MediaKind::Screenshot);
        self.toasts
            .success(format!("{} saved to Media", kind.label()))
            .button("Open", events::AppEvent::ShowMedia { id });
    }

    fn show_media(&mut self, id: MediaId, touch: bool) {
        if self.ui.settings.focus_mode && !touch {
            self.toggle_focus_mode();
        }
        self.ui.media.highlight(id);
        self.show_tab(Tab::Media, touch);
    }

    fn take_screenshot(&mut self) {
        match crate::media::save_screenshot(
            &self.emulator,
            &mut self.media,
            self.ui.settings.screenshot_scale,
        ) {
            Ok(item) => self
                .events
                .push(events::AppEvent::MediaSaved { id: item.id }),
            Err(message) => {
                self.toasts.error(message);
            }
        };
    }

    pub(super) fn recording_button(
        &self,
        ui: &mut egui::Ui,
        size: Option<egui::Vec2>,
    ) -> egui::Response {
        let keys = self
            .ui
            .settings
            .hotkeys
            .keys_for(HotkeyAction::ToggleRecording);
        let hint = |text: &str| match keys.first() {
            Some(hotkey) => format!("{text} ({hotkey})"),
            None => text.to_string(),
        };
        let icon_size = size.map(|_| 18.0);
        let recorder = &self.emulator.screen_recorder;
        if recorder.is_recording() {
            let label = match size {
                Some(_) => format!("{}  {}", icons::RECORD, recording_time(recorder)),
                None => icons::STOP.to_string(),
            };
            let mut text = egui::RichText::new(label).color(ui.visuals().error_fg_color);
            if let Some(px) = icon_size {
                text = text.size(px);
            }
            let button = egui::Button::new(text).min_size(size.unwrap_or_default());
            return ui.add(button).on_hover_text(hint("Stop recording"));
        }
        let mut text = egui::RichText::new(icons::RECORD);
        if let Some(px) = icon_size {
            text = text.size(px);
        }
        let button = egui::Button::new(text).min_size(size.unwrap_or_default());
        let loaded = self.emulator.gb.cartridge.has_rom_loaded;
        match recording::availability(
            self.emulator.gb.apu.output_sample_rate,
            self.ui.settings.recording_scale,
        ) {
            Availability::Ready | Availability::Checking | Availability::Download => ui
                .add_enabled(loaded, button)
                .on_hover_text(hint("Record video")),
            Availability::Missing(reason) => {
                ui.add_enabled(false, button).on_disabled_hover_text(reason)
            }
        }
    }

    fn toggle_recording(&mut self) {
        if self.emulator.screen_recorder.is_recording() {
            self.emulator.screen_recorder.stop();
            return;
        }
        if !self.emulator.gb.cartridge.has_rom_loaded {
            self.toasts.error("Load a ROM before recording");
            return;
        }
        let rate = self.emulator.gb.apu.output_sample_rate;
        let scale = self.ui.settings.recording_scale;
        match recording::availability(rate, scale) {
            Availability::Ready => {}
            Availability::Checking => {
                self.record_when_ready = true;
                return;
            }
            Availability::Download => {
                self.codec_prompt = true;
                return;
            }
            Availability::Missing(reason) => {
                self.toasts.error(reason);
                return;
            }
        }
        let new = NewMedia::current(MediaKind::Recording, &self.emulator);
        if let Err(err) = self.emulator.screen_recorder.start(new, rate, scale) {
            self.toasts
                .error(format!("Could not start recording: {err}"));
        }
    }

    fn codec_confirm(&mut self, ctx: &Context) {
        if !self.codec_prompt {
            return;
        }
        let choice = widgets::confirm::confirm_with_detail(
            ctx,
            "codec_confirm",
            "Recording needs Cisco's video codec, a one-time download of about 600 KB.",
            Some(recording::CODEC_NOTICE),
            "Download",
        );
        let Some(download) = choice else {
            return;
        };
        self.codec_prompt = false;
        if recording::download_codec(download) {
            self.record_when_ready = true;
            self.toasts.info("Downloading the video codec");
        } else {
            self.toasts
                .info("Recording needs the video codec, so nothing was recorded");
        }
    }

    fn finish_recordings(&mut self) {
        if let Some(err) = recording::take_codec_failure() {
            let pending = std::mem::take(&mut self.record_when_ready);
            self.toasts.error(if pending {
                format!("Could not download the video codec, so nothing was recorded: {err}")
            } else {
                format!("Could not download the video codec: {err}")
            });
        }
        if self.record_when_ready
            && !matches!(
                recording::availability(
                    self.emulator.gb.apu.output_sample_rate,
                    self.ui.settings.recording_scale,
                ),
                Availability::Checking
            )
        {
            self.record_when_ready = false;
            self.toggle_recording();
        }
        match self.emulator.screen_recorder.take_stopped() {
            Some(StopReason::SoundChanged) => {
                self.toasts
                    .info("Recording stopped because the sound output changed");
            }
            Some(StopReason::TooSlow) => {
                self.toasts
                    .error("Recording stopped because this device couldn't keep up");
            }
            Some(StopReason::TooLong) => {
                let minutes = recording::max_length().map_or(0, |max| max.as_secs() / 60);
                self.toasts.info(format!(
                    "Recording stopped at the browser's limit of {minutes} minutes"
                ));
            }
            Some(StopReason::Asked | StopReason::Failed) | None => {}
        }
        self.store_recordings();
    }

    fn store_recordings(&mut self) {
        while let Some(outcome) = self.emulator.screen_recorder.poll() {
            match outcome {
                Outcome::Saved(done) => match self.store_recording(*done) {
                    Ok(item) => {
                        self.events
                            .push(events::AppEvent::MediaSaved { id: item.id });
                        continue;
                    }
                    Err(err) => self
                        .toasts
                        .error(format!("Could not save the recording: {err}")),
                },
                Outcome::Empty => self
                    .toasts
                    .info("Nothing was recorded while the game was paused"),
                Outcome::Failed(err) => self
                    .toasts
                    .error(format!("Could not save the recording: {err}")),
            };
        }
    }

    fn store_recording(&mut self, done: FinishedRecording) -> std::io::Result<MediaItem> {
        let mut new = done.info;
        new.width = done.width as u32;
        new.height = done.height as u32;
        new.length_ms = Some(done.length.as_millis() as u64);
        if done.thumbnail.len() == new.frame.rgba.len() {
            new.frame = MediaFrame {
                rgba: done.thumbnail,
                ..new.frame
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        let item = self.media.add_recording(&done.output.path, new)?;
        #[cfg(target_arch = "wasm32")]
        let item = self.media.add_recording_blob(done.output.blob, new)?;
        Ok(item)
    }

    fn handle_load_rom_data(&mut self, data: Vec<u8>) {
        self.try_start_audio();
        let rom = Rom::new(&data);
        let model = self.ui.settings.console_for_rom(&rom);
        #[cfg(not(target_arch = "wasm32"))]
        let result = self.emulator.load_rom(&rom, model, None);
        #[cfg(target_arch = "wasm32")]
        let result = self.emulator.load_rom(&rom, model);

        match result {
            Ok(()) => self.close_mobile_sheet(),
            Err(err) => {
                self.toasts.error(format!("Failed to load ROM: {err}"));
            }
        }
        self.ui.settings.dirty = true;
    }
}

fn picked_from_drop(file: egui::DroppedFile) -> Option<PickedFile> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = file.path {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let data = std::fs::read(&path).ok()?;
        return Some(PickedFile {
            name,
            data,
            path: Some(path),
        });
    }
    let data = file.bytes?.to_vec();
    Some(PickedFile {
        name: file.name,
        data,
        #[cfg(not(target_arch = "wasm32"))]
        path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::settings::SettingsTab;
    use mobile::{Sheet, TOUCH_TARGET_PX};

    fn phone_app(name: &str) -> Citrine {
        let root =
            std::env::temp_dir().join(format!("citrine-touch-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = Citrine::default();
        app.emulator.store = crate::storage::SaveStore::at(root);
        app.ui.settings.layout = LayoutMode::Mobile;
        app
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
                            sheet_name(app),
                            widget.rect.height(),
                            label
                        )
                    })
                })
                .collect()
        })
    }

    fn sheet_name(app: &Citrine) -> String {
        match app.mobile.sheet {
            Some(Sheet::Panel(Tab::Settings)) => {
                format!("Settings/{}", app.ui.settings.current_tab.title())
            }
            other => format!("{other:?}"),
        }
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

        let sheets = [
            None,
            Some(Sheet::Menu),
            Some(Sheet::Panel(Tab::Homebrew)),
            Some(Sheet::Panel(Tab::Saves)),
            Some(Sheet::Panel(Tab::Media)),
            Some(Sheet::Panel(Tab::Info)),
        ];
        for sheet in sheets {
            app.mobile.sheet = sheet;
            small.extend(small_targets(&mut app, &ctx));
        }
        let first_game =
            crate::app::widgets::homebrew::details_id(crate::homebrew::HOMEBREW_GAMES[0].id);
        let mut details = egui::collapsing_header::CollapsingState::load_with_default_open(
            &ctx, first_game, true,
        );
        details.set_open(true);
        details.store(&ctx);
        app.mobile.sheet = Some(Sheet::Panel(Tab::Homebrew));
        small.extend(small_targets(&mut app, &ctx));
        app.mobile.sheet = Some(Sheet::Panel(Tab::Settings));
        for tab in SettingsTab::iter_with_dev_mode(false) {
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
        assert!(app.ui.settings.game_consoles.is_empty(), "and the choice");

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
        assert!(app.ui.settings.game_consoles.is_empty());
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
    fn opening_a_tab_in_the_touch_layout_opens_its_sheet() {
        let mut app = phone_app("open-tab");
        app.events.open_tab(Tab::Media);
        app.handle_event_queue(true);
        assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::Media)));
        assert!(
            app.dock.find_tab(&Tab::Media).is_none(),
            "no hidden dock tab"
        );

        let mut desktop = phone_app("open-tab-desktop");
        desktop.events.open_tab(Tab::Media);
        desktop.handle_event_queue(false);
        assert_eq!(desktop.mobile.sheet, None);
        assert!(desktop.dock.find_tab(&Tab::Media).is_some());
    }

    fn media_app(name: &str, layout: LayoutMode) -> Citrine {
        let mut app = phone_app(name);
        app.ui.settings.layout = layout;
        let root =
            std::env::temp_dir().join(format!("citrine-media-test-{}-{name}", std::process::id()));
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

    fn play_frame(app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>) -> egui::FullOutput {
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

    fn text_rect(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
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

    fn load_game(app: &mut Citrine, rom: &[u8], save: Option<&[u8]>) {
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

    fn text_containing(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
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

    fn tap(at: egui::Pos2) -> Vec<egui::Event> {
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
        app.mobile.open(Sheet::Menu);
        let output = play_frame(&mut app, &ctx, vec![]);
        assert!(text_containing(&output, Tab::GameInfo.label()).is_some());

        let ctx = Context::default();
        let mut other = media_app("info-not-offered", LayoutMode::Mobile);
        other.watch_game_info();
        assert!(!other.game_info.recognised());
        other.mobile.open(Sheet::Menu);
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
            phone.mobile.open(Sheet::Panel(Tab::GameInfo));
            load_game(&mut phone, &rom, None);
            assert_eq!(phone.mobile.sheet, None, "{name}");
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
            app.mobile.open(Sheet::Panel(Tab::GameInfo));
            app.watch_game_info();
            let output = play_frame(&mut app, &ctx, vec![]);
            assert_eq!(badge_count(&output), shown, "reached Kanto: {reached}");
        }
    }

    #[test]
    fn game_info_back_on_a_phone_closes_the_details_before_the_sheet() {
        let save = citrine_gb_gamedata::fixtures::crystal_save();
        let ctx = Context::default();
        let mut app = crystal_app("info-back", LayoutMode::Mobile, Some(&save));
        app.mobile.open(Sheet::Menu);
        app.mobile.open(Sheet::Panel(Tab::GameInfo));
        app.watch_game_info();
        app.game_info.view.details = true;
        assert!(app.game_info.details_open());

        let output = play_frame(&mut app, &ctx, vec![]);
        let back = text_containing(&output, "Party").expect("back to the party");
        play_frame(&mut app, &ctx, tap(back.center()));
        assert!(!app.game_info.details_open());
        assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::GameInfo)));

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
        assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::GameInfo)));
        press(&mut app, escape());
        assert_eq!(app.mobile.sheet, Some(Sheet::Menu));
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
        use citrine_gb::gb::joypad::JoypadState;
        for _ in 0..60 {
            app.emulator.gb.run_frame();
        }
        for step in 0..120 {
            let button = if step % 2 == 0 {
                JoypadState::START
            } else {
                JoypadState::A
            };
            app.emulator.gb.press_button(button);
            for _ in 0..6 {
                app.emulator.gb.run_frame();
            }
            app.emulator.gb.release_button(button);
            for _ in 0..54 {
                app.emulator.gb.run_frame();
            }
            app.game_info.next_read_now();
            app.watch_game_info();
            if app.game_info.game().is_some_and(|game| game.live.is_some()) {
                return true;
            }
        }
        false
    }

    #[test]
    #[ignore]
    fn game_info_opens_for_every_real_pokemon_game() {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        let dir = std::env::var_os("CITRINE_ROMS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join("roms/gb"));
        let silver_save =
            std::env::var_os("CITRINE_SILVER_SAVE").and_then(|p| std::fs::read(p).ok());
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
                    app.mobile.open(Sheet::Panel(Tab::GameInfo));
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

    fn info_frame_out(
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
        assert_eq!(app.mobile.sheet, None);
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
        for _ in 0..10 {
            output = play_frame(&mut app, &ctx, Vec::new());
        }
        assert_eq!(app.mobile.sheet, None);
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
        assert_eq!(app.mobile.sheet, Some(Sheet::Panel(Tab::Media)));
        let id = app.media.items()[0].id.clone();
        assert_eq!(app.ui.media.highlighted(), Some(&id));
    }

    #[test]
    fn a_forced_desktop_layout_falls_back_to_touch_on_a_narrow_window() {
        let phone = egui::vec2(412.0, 915.0);
        let landscape_phone = egui::vec2(915.0, 412.0);
        let desktop = egui::vec2(1280.0, 800.0);
        assert!(touch_layout_for(LayoutMode::Desktop, phone));
        assert!(!touch_layout_for(LayoutMode::Desktop, landscape_phone));
        assert!(!touch_layout_for(LayoutMode::Desktop, desktop));
        assert!(touch_layout_for(LayoutMode::Auto, landscape_phone));
        assert!(touch_layout_for(LayoutMode::Mobile, desktop));
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
                    if ["Load a ROM", "Homebrew games"]
                        .iter()
                        .any(|label| text.galley.text().ends_with(label)) =>
                {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .collect();
        let css_per_point = ctx.pixels_per_point() / pixel_ratio;
        let bar = egui::containers::panel::PanelState::load(ctx, egui::Id::new("mobile_bar"))
            .expect("the phone bar is drawn")
            .rect;
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
        assert_eq!(empty_state.len(), 2, "{at}: Load a ROM and Homebrew games");
        let tallest_icon = if scale > 1.5 { 48.0 } else { TOUCH_TARGET_PX };
        for rect in &bar {
            assert!(
                (TOUCH_TARGET_PX - 0.5..tallest_icon + 0.5).contains(&rect.height()),
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
                (height - TOUCH_TARGET_PX).abs() < 0.5,
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
    fn a_channel_muted_in_the_apu_tab_stays_muted_after_a_volume_change() {
        let ctx = Context::default();
        let mut app = media_app("apu-mute", LayoutMode::Desktop);
        app.open_tab(Tab::Apu);
        let mut output = desk_frame(&mut app, &ctx, Vec::new());
        for _ in 0..3 {
            output = desk_frame(&mut app, &ctx, Vec::new());
        }
        let second = text_rect(&output, "2").expect("the APU tab's channel 2 box");

        click_at(&mut app, &ctx, second.center());
        desk_frame(&mut app, &ctx, Vec::new());
        assert!(app.emulator.gb.debugger.ch2_disabled);

        app.ui.settings.volume = 0.5;
        app.ui.settings.dirty = true;
        desk_frame(&mut app, &ctx, Vec::new());

        assert!(app.emulator.gb.debugger.ch2_disabled);
        assert_eq!(app.ui.settings.channels, [true, false, true, true]);
    }

    #[test]
    fn a_controller_button_bound_to_focus_mode_turns_it_on_and_off() {
        let mut app = Citrine::default();
        app.ui
            .settings
            .hotkeys
            .bind_pad(PadButton::Mode, HotkeyAction::ToggleFocusMode);
        app.run_pad_shortcuts(vec![PadButton::Mode]);
        assert!(app.ui.settings.focus_mode);
        assert!(
            app.focus_exit_names()
                .contains(&PadButton::Mode.to_string())
        );
        app.run_pad_shortcuts(vec![PadButton::Mode]);
        assert!(!app.ui.settings.focus_mode);
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
                crate::media::NewMedia::current(crate::media::MediaKind::Screenshot, &app.emulator),
                &png,
            )
            .unwrap();
        app.ui.settings.volume = 0.2;
        app.ui.settings.matrix = true;
        app.ui.settings.online.joined = Some("relay.example".into());
        app.ui.settings.current_tab = SettingsTab::YourData;
        app.ui
            .recent
            .record(std::path::Path::new("/roms/game.gb"), "Game", None);
        app.open_tab(Tab::Settings);
        app.mobile.open(Sheet::Panel(Tab::Settings));
        assert!(!app.emulator.store.files().is_empty());

        app.delete_everything();

        let fresh = Citrine::default();
        let tabs = |app: &Citrine| {
            app.dock
                .iter_all_tabs()
                .map(|(_, tab)| *tab)
                .collect::<Vec<_>>()
        };
        assert_eq!(tabs(&app), tabs(&fresh), "the first-launch tabs");
        assert_eq!(app.mobile.sheet, None);
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
}
