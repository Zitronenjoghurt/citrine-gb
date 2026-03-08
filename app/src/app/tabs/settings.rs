use crate::app::tabs::TabViewer;
use crate::app::widgets::settings::bar::SettingsBar;
use crate::app::widgets::settings::content::SettingsContent;
use egui::Widget;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            let width = ui.available_width();
            let height = ui.available_height();
            let tabs_width = width * 0.2;

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(tabs_width);
                    ui.set_height(height);
                    SettingsBar::new(&mut viewer.ui.settings)
                        .spacing(2.0)
                        .ui(ui);
                });

                ui.separator();

                ui.vertical_centered(|ui| {
                    SettingsContent::new(&mut viewer.ui.settings).ui(ui);
                });
            });
        });
}
