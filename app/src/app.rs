use crate::app::file_picker::{FileIntent, FilePicker, FileResult, SaveResult};
use crate::app::tabs::Tab;
use crate::app::ui_state::UiState;
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::icons;
use citrine_gb::rom::Rom;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Color32, Context, FontDefinitions, TopBottomPanel};
use egui_commonmark::CommonMarkCache;
use egui_dock::DockState;
use egui_notify::{Toast, Toasts};
use gilrs::Gilrs;
use std::io::Write;
use strum::IntoEnumIterator;

mod events;
mod file_picker;
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
    pub file_picker: FilePicker,
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
}

impl Default for Citrine {
    fn default() -> Self {
        let dock = DockState::new(vec![Tab::GameBoy]);
        let mut app = Self {
            dock,
            ui: UiState::default(),
            emulator: Emulator::default(),
            file_picker: FilePicker::default(),
            gil: default_gilrs(),
            toasts: Toasts::default(),
            audio: None,
            events: events::AppEventQueue::default(),
            commonmark: CommonMarkCache::default(),
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

        if let Some(result) = self.file_picker.poll() {
            match result.intent {
                FileIntent::LoadRom => self.handle_load_rom(result),
                FileIntent::LoadBootRom => self.handle_load_boot_rom(result),
                FileIntent::ExportE2E => self.handle_export_e2e(result),
            }
        }

        if let Some(save_result) = self.file_picker.poll_save() {
            match save_result {
                SaveResult::Success(filename) => {
                    self.toasts.success(format!("Saved to '{}'", filename));
                }
                SaveResult::Error(err) => {
                    self.toasts.error(format!("Failed to save: {}", err));
                }
                SaveResult::Cancelled => {}
            }
        }

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
                file_picker: &mut self.file_picker,
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
                if ui.button("Load ROM").clicked() {
                    self.file_picker.open(FileIntent::LoadRom);
                }
                if ui.button("Load Boot ROM").clicked() {
                    self.file_picker.open(FileIntent::LoadBootRom);
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

// File handling
impl Citrine {
    fn handle_load_rom(&mut self, fr: FileResult) {
        self.try_start_audio();
        let rom = Rom::new(fr.data().unwrap());
        if let Err(err) = self.emulator.load_rom(
            &rom,
            #[cfg(not(target_arch = "wasm32"))]
            Some(&fr.path),
        ) {
            self.toasts.error(format!("Failed to load ROM: {}", err));
        } else {
            self.toasts.success(format!("Loaded ROM '{}'", fr.name));
            self.ui.settings.dirty = true;

            if self.emulator.save_loaded {
                self.toasts.success("Loaded save data");
            }
        }
    }

    fn handle_load_boot_rom(&mut self, fr: FileResult) {
        self.try_start_audio();
        self.emulator.gb.load_boot_rom(fr.data().unwrap());
        self.ui.settings.dirty = true;
        self.toasts.success("Boot ROM loaded");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn handle_export_e2e(&mut self, fr: FileResult) {
        let dir = fr.directory_path().unwrap();

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

    #[cfg(target_arch = "wasm32")]
    fn handle_export_e2e(&mut self, _fr: FileResult) {}
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
