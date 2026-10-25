use crate::app::tabs::TabViewer;
use crate::app::ui_state::settings::SettingsTab;
use crate::app::widgets::settings::bar::SettingsBar;
use crate::app::widgets::settings::content::{LoadedGame, SettingsContent};
use egui::Widget;

const SIDEBAR_MIN_WIDTH: f32 = 420.0;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            if ui.available_width() < SIDEBAR_MIN_WIDTH {
                stacked(viewer, ui);
            } else {
                side_by_side(viewer, ui);
            }
        });
}

fn side_by_side(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let width = ui.available_width();
    let height = ui.available_height();
    let tabs_width = (width * 0.2).max(120.0);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(tabs_width);
            ui.set_height(height);
            SettingsBar::new(&mut viewer.ui.settings)
                .spacing(2.0)
                .ui(ui);
        });

        ui.separator();

        ui.vertical_centered(|ui| content(viewer, ui));
    });
}

fn stacked(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let settings = &mut viewer.ui.settings;
    let dev_mode = settings.dev_mode;

    ui.horizontal_wrapped(|ui| {
        for tab in SettingsTab::iter_with_dev_mode(dev_mode) {
            ui.selectable_value(
                &mut settings.current_tab,
                tab,
                format!("{} {}", tab.icon(), tab.title()),
            );
        }
    });
    ui.separator();
    content(viewer, ui);
}

fn content(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let game = viewer.emulator.rom_key().map(|rom_key| LoadedGame {
        rom_key,
        title: viewer.emulator.gb.cartridge.header.title.trim(),
    });

    SettingsContent::new(
        &mut viewer.ui.settings,
        &mut viewer.ui.capture,
        &mut viewer.ui.palette_editor,
        viewer.audio.as_ref(),
        game,
    )
    .ui(ui);
}
