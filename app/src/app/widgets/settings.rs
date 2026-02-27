use crate::app::settings::Settings;
use crate::app::widgets::theme_selector::ThemeSelector;
use egui::{Grid, Response, Slider, Ui, Widget};

pub struct SettingsWidget<'a> {
    settings: &'a mut Settings,
}

impl<'a> SettingsWidget<'a> {
    pub fn new(settings: &'a mut Settings) -> Self {
        Self { settings }
    }
}

impl Widget for SettingsWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        Grid::new("settings_grid")
            .num_columns(2)
            .show(ui, |ui| {
                let mut ui_scale = self.settings.ui_scale();
                ui.label("UI Scale");
                Slider::new(&mut ui_scale, 0.5..=5.0).step_by(0.1).ui(ui);
                if ui_scale != self.settings.ui_scale() {
                    self.settings.set_ui_scale(ui_scale);
                }
                ui.end_row();

                let mut theme = self.settings.dmg_theme();
                ui.label("Theme");
                ThemeSelector::new(&mut theme).ui(ui);
                if theme != self.settings.dmg_theme() {
                    self.settings.set_dmg_theme(theme);
                };
                ui.end_row();
            })
            .response
    }
}
