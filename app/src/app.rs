use crate::app::file_picker::{FileIntent, FilePicker, FileResult};
use crate::app::panels::PanelKind;
use crate::app::ui_state::UiState;
use crate::app::widgets::panel_menu::PanelMenu;
use crate::emulator::Emulator;
use crate::icons;
use citrine_gb::rom::Rom;
use eframe::{Frame, Storage};
use egui::{CentralPanel, Context, FontDefinitions, SidePanel, TopBottomPanel, Widget};
use egui_notify::Toasts;

mod file_picker;
mod panels;
mod ui_state;
mod widgets;
mod windows;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Citrine {
    pub ui: UiState,
    #[serde(skip, default)]
    pub emulator: Emulator,
    #[serde(skip, default)]
    pub file_picker: FilePicker,
    #[serde(skip, default)]
    pub toasts: Toasts,
}

impl Citrine {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        #[cfg(target_arch = "wasm32")]
        cc.egui_ctx.set_pixels_per_point(3.0);

        #[cfg(not(target_arch = "wasm32"))]
        cc.egui_ctx.set_pixels_per_point(1.5);

        Self::setup_fonts(&cc.egui_ctx);
        let mut app = cc
            .storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default();
        app.file_picker.set_drop_intent(FileIntent::LoadRom);
        app.ui.panels.right = Some(PanelKind::Registers);
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
            }
        }

        self.emulator.update(ctx);
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
        });
    }

    fn central_panel(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            self.emulator.ui(ui);
        });
    }
}

// File handling
impl Citrine {
    fn handle_load_rom(&mut self, fr: FileResult) {
        let rom = Rom::new(&fr.data);
        if let Err(err) = self.emulator.gb.load_rom(&rom) {
            self.toasts.error(format!("Failed to load ROM: {}", err));
        } else {
            self.toasts.success(format!("Loaded ROM '{}'", fr.name));
        }
    }

    fn handle_load_boot_rom(&mut self, fr: FileResult) {
        self.emulator.gb.load_boot_rom(&fr.data);
        self.toasts.success("Boot ROM loaded");
    }
}
