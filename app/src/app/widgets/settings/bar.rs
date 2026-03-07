use crate::app::settings::{Settings, SettingsTab};
use egui::{Response, ScrollArea, Ui, Widget};
use strum::IntoEnumIterator;

pub struct SettingsBar<'a> {
    settings: &'a mut Settings,
    spacing: f32,
}

impl<'a> SettingsBar<'a> {
    pub fn new(settings: &'a mut Settings) -> Self {
        Self {
            settings,
            spacing: 0.0,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}

impl Widget for SettingsBar<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Categories");
            });
            ui.separator();
            ScrollArea::vertical().show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for tab in SettingsTab::iter() {
                        ui.selectable_value(
                            &mut self.settings.current_tab,
                            tab,
                            format!("{} {}", tab.icon(), tab.title()),
                        );
                        ui.add_space(self.spacing);
                    }
                });
            });
        })
        .response
    }
}
