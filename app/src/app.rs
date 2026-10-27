use crate::app::tabs::Tab;
use crate::app::ui_state::UiState;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::settings::{LayoutMode, Settings};
use crate::app::widgets::snapshots::{SnapshotNews, snapshot_message};
use crate::audio::Audio;
use crate::emulator::{Emulator, SaveImport};
use crate::icons;
use crate::media::recording::{self, Availability, FinishedRecording, Outcome, StopReason};
use crate::media::{Frame as MediaFrame, MediaId, MediaItem, MediaKind, MediaStore, NewMedia};
use crate::storage::SettingsStore;
use crate::utils::file_channels::FileChannels;
use crate::utils::file_loader::FileLoader;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::utils::save_files::{FileKind, SaveFiles};
use citrine_gb::rom::Rom;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Color32, Context, FontDefinitions, TopBottomPanel};
use egui_commonmark::CommonMarkCache;
use egui_dock::{DockState, NodeIndex, SurfaceIndex};
use gilrs::Gilrs;
use strum::IntoEnumIterator;
use toasts::{Toast, Toasts};

const QUICK_LOAD_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

const MOBILE_BREAKPOINT: f32 = 560.0;

#[cfg(target_arch = "wasm32")]
const AUDIO_STALL_SECS: f32 = 1.0;

const SETTINGS_WRITE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

mod events;
mod mobile;
mod tabs;
mod toasts;
pub mod ui_state;
mod widgets;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Citrine {
    pub dock: DockState<Tab>,
    pub ui: UiState,
    #[serde(skip, default)]
    pub emulator: Emulator,
    #[serde(skip, default)]
    pub files: FileChannels,
    #[serde(skip, default = "default_gilrs")]
    pub gil: Gilrs,
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
    pub media: MediaStore,
}

const RESET_STOP_ASKING: &str = "Don't ask again until Citrine closes";

struct PendingReset {
    was_running: bool,
    stop_asking: bool,
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
            gil: default_gilrs(),
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
            media: MediaStore::default(),
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

fn default_gilrs() -> Gilrs {
    Gilrs::new().unwrap()
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        Self::setup_fonts(&cc.egui_ctx);
        catppuccin_egui::set_theme(&cc.egui_ctx, catppuccin_egui::MOCHA);

        let mut app = cc
            .storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default();

        if let Some(settings) = app.settings_store.load::<Settings>() {
            app.ui.settings = settings;
        }

        app.ui.settings.sanitize();
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
        self.render(ctx);
        if self.ui.settings.focus_mode && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.toggle_focus_mode();
        }

        self.handle_snapshot_hotkeys(ctx);
        self.quick_load_overlay(ctx);
        self.import_confirm(ctx);
        self.codec_confirm(ctx);
        self.reset_confirm(ctx);
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

        let Self {
            emulator, gil, ui, ..
        } = self;
        let result = emulator.update(
            ctx,
            gil,
            &ui.settings.bindings,
            &mut ui.capture,
            ui.controller_debug.log_mut(),
        );
        if let Err(err) = result {
            self.toasts.error(format!("Emulation error: {err}"));
        }
        match self.emulator.autosave_if_due() {
            Ok(Some(_)) => self.ui.snapshots.invalidate(),
            Ok(None) => {}
            Err(err) => {
                self.toasts.error(format!("Autosave failed: {err:?}"));
            }
        }
        self.finish_recordings();

        self.show_toasts(ctx);

        let mobile = self.is_mobile(ctx);
        self.handle_event_queue(mobile);
        if let Some(err) = self
            .ui
            .settings
            .apply(ctx, &mut self.audio, &mut self.emulator, mobile)
        {
            self.toasts.error(err);
        }

        self.flush_settings(false);

        self.ui.update_avg_timer.stop();
    }

    fn raw_input_hook(&mut self, _ctx: &Context, raw_input: &mut egui::RawInput) {
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
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn on_exit(&mut self) {
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
                gil: &self.gil,
                events: &mut self.events,
                files: &mut self.files,
                media: &mut self.media,
                ui: &mut self.ui,
            };

            egui_dock::DockArea::new(&mut self.dock)
                .style(egui_dock::Style::from_egui(ctx.style().as_ref()))
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);
        });
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
                    if self.ui.recent.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No recent ROMs"));
                    } else {
                        ui.label("Recent");
                        let mut chosen = None;
                        for entry in self.ui.recent.entries() {
                            if ui
                                .button(entry.display_name())
                                .on_hover_text(entry.path.display().to_string())
                                .clicked()
                            {
                                chosen = Some(entry.path.clone());
                            }
                        }
                        if let Some(path) = chosen {
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
            self.toasts
                .add(Toast::info("Press Esc to leave focus mode"))
                .duration(None);
        } else {
            self.toasts.dismiss_all_toasts();
        }
    }

    fn render_focus_mode(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                self.ui.toast_area = Some(ui.max_rect());
                ui.centered_and_justified(|ui| {
                    self.emulator.ui(ui);
                });
            });
    }

    fn tools_node(&self) -> Option<(SurfaceIndex, NodeIndex)> {
        Tab::iter()
            .filter(|t| *t != Tab::GameBoy)
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

    fn open_tab(&mut self, tab: Tab) {
        if let Some((surface_idx, node_idx, tab_idx)) = self.dock.find_tab(&tab) {
            self.dock.set_active_tab((surface_idx, node_idx, tab_idx));
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
        self.restart_audio(true);
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

        self.restart_audio(!stalled);
    }

    fn restart_audio(&mut self, announce: bool) {
        let Some(audio) = &mut self.audio else {
            return;
        };

        let device = self.ui.settings.audio_device.clone();
        match audio.start(device.as_deref(), self.ui.settings.audio_queue_ms) {
            Ok(link) => {
                let rate = link.sample_rate;
                self.emulator.attach_audio(link);
                self.ui.settings.dirty = true;
                if announce {
                    self.toasts.success(format!("Audio started ({rate} Hz)"));
                }
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
            HotkeyAction::ToggleTouchControls => {
                let show = &mut self.ui.settings.touch.show_controls;
                *show = !*show;
                self.ui.settings.dirty = true;
            }
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
        if let Err(err) = self.emulator.load_rom(
            &rom,
            #[cfg(not(target_arch = "wasm32"))]
            file.path.as_deref(),
        ) {
            self.toasts.error(format!("Failed to load ROM: {}", err));
        } else {
            self.toasts.success(format!("Loaded ROM '{}'", file.name));
            self.ui.settings.dirty = true;
            self.close_mobile_sheet();

            #[cfg(not(target_arch = "wasm32"))]
            if let Some(path) = file.path.as_deref() {
                let title = self.emulator.gb.cartridge.header.title.clone();
                self.ui.recent.record(path, &title);
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
            events::AppEvent::ShowMedia { id } => self.show_media(id, touch),
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
        #[cfg(not(target_arch = "wasm32"))]
        let result = self.emulator.load_rom(&rom, None);
        #[cfg(target_arch = "wasm32")]
        let result = self.emulator.load_rom(&rom);

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
            .load_rom(&citrine_gb::rom::Rom::new(&rom), None)
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
            .load_rom(&citrine_gb::rom::Rom::new(&rom), None)
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
}
