use crate::app::tabs::Tab;
use crate::app::ui_state::UiState;
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::icons;
use crate::utils::file_channels::FileChannels;
use crate::utils::file_loader::FileLoader;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use citrine_gb::rom::Rom;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Color32, Context, FontDefinitions, TopBottomPanel};
use egui_commonmark::CommonMarkCache;
use egui_dock::DockState;
use egui_notify::{Toast, Toasts};
use gilrs::Gilrs;
use strum::IntoEnumIterator;

const QUICK_LOAD_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

mod events;
mod tabs;
mod ui_state;
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
        };
        app.open_tab(Tab::Info);
        app
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

        let (audio, producer) = Audio::new();
        app.audio = Some(audio);
        app.emulator.audio_producer = Some(producer);
        app.emulator.running = false;

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

        self.render(ctx);
        if self.ui.settings.focus_mode && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.toggle_focus_mode();
        }

        self.handle_snapshot_hotkeys(ctx);
        self.quick_load_overlay(ctx);

        self.drain_file_channels();

        if let Err(err) = self.emulator.update(ctx, &mut self.gil) {
            self.toasts.error(format!("Emulation Error: {}", err));
        }

        self.toasts.show(ctx);

        self.handle_event_queue();
        self.ui
            .settings
            .apply(ctx, &mut self.audio, &mut self.emulator);

        self.ui.update_avg_timer.stop();
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save: {err:?}");
        }
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

// Rendering
impl Citrine {
    fn render(&mut self, ctx: &Context) {
        if self.ui.settings.focus_mode {
            self.render_focus_mode(ctx);
        } else {
            self.render_normal_mode(ctx);
        }
    }

    fn render_normal_mode(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));

        CentralPanel::default().show(ctx, |ui| {
            let mut viewer = tabs::TabViewer {
                audio: &mut self.audio,
                commonmark: &mut self.commonmark,
                emulator: &mut self.emulator,
                events: &mut self.events,
                files: &mut self.files,
                ui: &mut self.ui,
            };

            egui_dock::DockArea::new(&mut self.dock)
                .style(egui_dock::Style::from_egui(ctx.style().as_ref()))
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut viewer);
        });
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
                if ui.button("Load Boot ROM").clicked() {
                    FileLoader::new()
                        .title("Load Boot ROM")
                        .add_filter("Game Boy Boot ROM", &["bin"])
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
            .on_hover_text("File Menu");

            if ui
                .button(icons::JOYSTICK)
                .on_hover_text("Homebrew Games")
                .clicked()
            {
                self.open_tab(Tab::Homebrew);
            }

            if ui
                .button(icons::FLOPPY_DISK)
                .on_hover_text("Saves & Snapshots (F8 quick save / F9 quick load)")
                .clicked()
            {
                self.open_tab(Tab::Saves);
            }

            if ui.button(icons::GEAR).on_hover_text("Settings").clicked() {
                self.open_tab(Tab::Settings);
            }

            ui.menu_button(icons::INFO, |ui| {
                if ui.button("General").clicked() {
                    self.open_tab(Tab::Info);
                }
                if ui.button("ROM").clicked() {
                    self.open_tab(Tab::RomInfo);
                }
            })
            .response
            .on_hover_text("Information & Details");

            if ui
                .button(icons::FRAME_CORNERS)
                .on_hover_text("Toggle Focus Mode (Hide UI)")
                .clicked()
            {
                self.toggle_focus_mode();
            }

            if self.ui.settings.dev_mode {
                ui.menu_button(icons::CIRCUITRY, |ui| {
                    if ui.button("Disassembly").clicked() {
                        self.open_tab(Tab::Disassembly);
                    }
                    if ui.button("APU").clicked() {
                        self.open_tab(Tab::Apu);
                    }
                    if ui.button("Registers").clicked() {
                        self.open_tab(Tab::Registers);
                    }
                })
                .response
                .on_hover_text("Hardware Views");

                ui.menu_button(icons::BRACKETS_CURLY, |ui| {
                    if ui.button("Time Control").clicked() {
                        self.open_tab(Tab::TimeControl);
                    }
                    if ui.button("Performance").clicked() {
                        self.open_tab(Tab::Performance);
                    }
                    if ui.button("Actions").clicked() {
                        self.open_tab(Tab::DebugActions)
                    }
                    if ui.button("Audio Debug").clicked() {
                        self.open_tab(Tab::AudioDebug);
                    }
                    if ui.button("E2E Tests").clicked() {
                        self.open_tab(Tab::E2ETest);
                    }
                })
                .response
                .on_hover_text("Debug Tools");
            }

            ui.separator();

            if ui
                .button("Reset Layout")
                .on_hover_text("Restore default tab layout")
                .clicked()
            {
                self.dock = DockState::new(vec![Tab::GameBoy]);
            }
        });
    }

    fn toggle_focus_mode(&mut self) {
        self.ui.settings.focus_mode = !self.ui.settings.focus_mode;
        if self.ui.settings.focus_mode {
            self.toasts
                .add(Toast::info("Focus mode enabled. Press ESC to exit."))
                .duration(None);
        } else {
            self.toasts.dismiss_all_toasts();
        }
    }

    fn render_focus_mode(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::BLACK))
            .show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    self.emulator.ui(ui);
                });
            });
    }

    fn open_tab(&mut self, tab: Tab) {
        if let Some((surface_idx, node_idx, tab_idx)) = self.dock.find_tab(&tab) {
            self.dock.set_active_tab((surface_idx, node_idx, tab_idx));
            return;
        }

        let mut tools_node = None;
        for t in Tab::iter().filter(|t| *t != Tab::GameBoy) {
            if let Some((surface_idx, node_idx, _)) = self.dock.find_tab(&t) {
                tools_node = Some((surface_idx, node_idx));
                break;
            }
        }

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

// Audio
impl Citrine {
    pub fn try_start_audio(&mut self) {
        let Some(audio) = &mut self.audio else {
            return;
        };

        if audio.stream.is_some() {
            return;
        };

        match audio.try_start() {
            Ok(sample_rate) => {
                self.emulator.gb.apu.set_sample_rate(sample_rate);
                self.toasts
                    .success(format!("Audio started ({} Hz)", sample_rate));
            }
            Err(err) => {
                self.toasts.error(format!("Failed to start audio: {}", err));
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
        while let Ok(file) = self.files.sav_rx.try_recv() {
            self.handle_import_save(file);
        }
        #[cfg(not(target_arch = "wasm32"))]
        while let Ok(dir) = self.files.folder_rx.try_recv() {
            self.handle_export_e2e(&dir);
        }
        while let Ok(outcome) = self.files.save_rx.try_recv() {
            match outcome {
                SaveOutcome::Saved(name) => self.toasts.success(format!("Saved to '{name}'")),
                SaveOutcome::Failed(err) => self.toasts.error(format!("Failed to save: {err}")),
                SaveOutcome::Cancelled => continue,
            };
        }
    }

    fn handle_snapshot_hotkeys(&mut self, ctx: &Context) {
        if ctx.wants_keyboard_input() {
            self.quick_load_hold = None;
            return;
        }

        let (save, shift, load_held) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F8),
                i.modifiers.shift,
                i.key_down(egui::Key::F9),
            )
        });

        if self.emulator.rom_key().is_none() {
            self.quick_load_hold = None;
            if save || load_held {
                self.toasts.error("No ROM loaded");
            }
            return;
        }

        if save {
            let slot = if shift {
                self.emulator
                    .store
                    .next_free_slot(self.emulator.rom_key().unwrap_or_default())
            } else {
                self.ui.settings.quick_slot
            };
            match self.emulator.save_snapshot(slot) {
                Ok(()) => self
                    .toasts
                    .success(format!("Saved snapshot to slot {slot}")),
                Err(err) => self.toasts.error(format!("Quick save failed: {err:?}")),
            };
            self.ui.snapshots.invalidate();
        }

        if load_held {
            let started = *self
                .quick_load_hold
                .get_or_insert_with(web_time::Instant::now);
            if started.elapsed() >= QUICK_LOAD_HOLD {
                self.quick_load_hold = None;
                let slot = self.ui.settings.quick_slot;
                match self.emulator.load_snapshot(slot) {
                    Ok(true) => self.toasts.success(format!("Loaded slot {slot}")),
                    Ok(false) => self.toasts.info(format!("Slot {slot} is empty")),
                    Err(err) => self.toasts.error(format!("Quick load failed: {err:?}")),
                };
                self.ui.snapshots.invalidate();
            }
        } else {
            self.quick_load_hold = None;
        }
    }

    fn quick_load_overlay(&self, ctx: &Context) {
        let Some(started) = self.quick_load_hold else {
            return;
        };
        let progress =
            (started.elapsed().as_secs_f32() / QUICK_LOAD_HOLD.as_secs_f32()).clamp(0.0, 1.0);
        let slot = self.ui.settings.quick_slot;

        egui::Area::new("quick_load_hold".into())
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(220.0);
                    ui.label(format!("Hold to load slot {slot}"));
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

    fn handle_import_save(&mut self, file: PickedFile) {
        match self.emulator.import_save_bytes(&file.data) {
            Ok(true) => {
                self.toasts
                    .success(format!("Imported save from '{}'", file.name));
            }
            Ok(false) => {
                self.toasts.error("Load a ROM before importing a save");
            }
            Err(err) => {
                self.toasts.error(format!("Failed to import save: {err:?}"));
            }
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
    fn handle_event_queue(&mut self) {
        for event in self.events.take() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: events::AppEvent) {
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
            events::AppEvent::OpenTab { tab } => self.open_tab(tab),
        }
    }

    fn handle_load_rom_data(&mut self, data: Vec<u8>) {
        self.try_start_audio();
        let rom = Rom::new(&data);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.emulator.load_rom(&rom, None);
        #[cfg(target_arch = "wasm32")]
        let _ = self.emulator.load_rom(&rom);
        self.ui.settings.dirty = true;
    }
}
