use crate::app::file_picker::{FileIntent, FilePicker, FileResult};
use crate::app::ui_state::UiState;
use crate::app::widgets::panel_menu::PanelMenu;
use crate::audio::Audio;
use crate::emulator::Emulator;
use crate::icons;
use citrine_gb::rom::Rom;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Context, FontDefinitions, SidePanel, TopBottomPanel, Widget};
use egui_notify::Toasts;
use gilrs::Gilrs;

mod file_picker;
mod panels;
mod settings;
mod ui_state;
mod widgets;
mod windows;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Citrine {
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
    audio: Option<Audio>,
}

impl Default for Citrine {
    fn default() -> Self {
        Self {
            ui: UiState::default(),
            emulator: Emulator::default(),
            file_picker: FilePicker::default(),
            gil: default_gilrs(),
            toasts: Toasts::default(),
            audio: None,
        }
    }
}

fn default_gilrs() -> Gilrs {
    Gilrs::new().unwrap()
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        Self::setup_fonts(&cc.egui_ctx);
        let mut app = cc
            .storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default();
        app.file_picker.set_drop_intent(FileIntent::LoadRom);

        let (audio, producer) = Audio::new();
        app.audio = Some(audio);
        app.emulator.audio_producer = Some(producer);

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
        if let Some(result) = self.file_picker.poll(ctx) {
            match result.intent {
                FileIntent::LoadRom => self.handle_load_rom(result),
                FileIntent::LoadBootRom => self.handle_load_boot_rom(result),
                FileIntent::ExportE2E => self.handle_export_e2e(result),
            }
        }

        if let Err(err) = self.emulator.update(ctx, &mut self.gil) {
            self.toasts.error(format!("Emulation Error: {}", err));
        }

        TopBottomPanel::top("top_panel").show(ctx, |ui| self.top_panel(ui));

        if let Some(panel) = self.ui.panels.left {
            SidePanel::left("left_panel").show(ctx, |ui| panel.ui(ui, self));
        }

        if let Some(panel) = self.ui.panels.right {
            SidePanel::right("right_panel").show(ctx, |ui| panel.ui(ui, self));
        }

        CentralPanel::default().show(ctx, |ui| self.central_panel(ui));

        let active_windows = self.ui.windows.active;
        active_windows.show_all(ctx, self);

        self.file_picker.show_drop_overlay(ctx);
        self.toasts.show(ctx);

        self.ui
            .settings
            .apply(ctx, &mut self.audio, &mut self.emulator);
    }

    fn save(&mut self, storage: &mut dyn Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }
}

// Rendering
impl Citrine {
    fn top_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Citrine");
            ui.separator();

            ui.menu_button(icons::FOLDER, |ui| {
                if ui.button("Load ROM").clicked() {
                    self.file_picker.open(FileIntent::LoadRom);
                    ui.close_kind(egui::UiKind::Menu);
                }
                if ui.button("Load Boot ROM").clicked() {
                    self.file_picker.open(FileIntent::LoadBootRom);
                    ui.close_kind(egui::UiKind::Menu);
                }
            });

            self.ui.windows.active.toggle_menu(ui);

            PanelMenu::new(icons::ALIGN_LEFT_SIMPLE, &mut self.ui.panels.left).ui(ui);
            PanelMenu::new(icons::ALIGN_RIGHT_SIMPLE, &mut self.ui.panels.right).ui(ui);

            ui.separator();

            ui.label(format!("{:.02}ms", self.emulator.last_frame_secs * 1000.0));

            ui.label(format!("{} Cycles", self.emulator.gb.debugger.total_cycles));

            if let Some(last_save) = self.emulator.last_save {
                ui.separator();

                let elapsed = web_time::Instant::now() - last_save;
                let label = match elapsed.as_secs() {
                    0 if elapsed.subsec_micros() == 0 => {
                        format!("{}ns ago", elapsed.subsec_nanos())
                    }
                    0 if elapsed.subsec_millis() == 0 => {
                        format!("{}µs ago", elapsed.subsec_micros())
                    }
                    0 => format!("{}ms ago", elapsed.subsec_millis()),
                    1..=59 => format!("{}s ago", elapsed.as_secs()),
                    60..=3599 => format!("{}m ago", elapsed.as_secs() / 60),
                    _ => format!("{}h ago", elapsed.as_secs() / 3600),
                };
                ui.label(format!(
                    "Last SRAM dump: {} ({} always save in-game if possible)",
                    label,
                    icons::WARNING
                ));
            } else if self.emulator.gb.cartridge.supports_sram_saves() {
                ui.separator();
                if self.emulator.save_loaded {
                    ui.label("Save file loaded");
                } else {
                    ui.label("Game did not save anything yet");
                }
            } else if !self.emulator.gb.cartridge.supports_sram_saves() {
                ui.separator();
                ui.label("No saves (cartridge has no battery)");
            }
        });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            self.emulator.ui(ui);
        });
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
