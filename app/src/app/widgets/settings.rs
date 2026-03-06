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
                let scale_result = Slider::new(&mut ui_scale, 0.5..=5.0).step_by(0.1).ui(ui);
                if scale_result.changed() && !scale_result.dragged() {
                    self.settings.set_ui_scale(ui_scale);
                }
                ui.end_row();

                let mut volume = self.settings.volume();
                ui.label("Volume");
                Slider::new(&mut volume, 0.0..=1.0).step_by(0.01).ui(ui);
                if volume != self.settings.volume() {
                    self.settings.set_volume(volume);
                }
                ui.end_row();

                let mut theme = self.settings.dmg_theme();
                ui.label("Theme");
                ThemeSelector::new(&mut theme).ui(ui);
                if theme != self.settings.dmg_theme() {
                    self.settings.set_dmg_theme(theme);
                };
                ui.end_row();

                let mut matrix_enabled = self.settings.matrix();
                ui.label("Matrix");
                ui.checkbox(&mut matrix_enabled, "");
                if matrix_enabled != self.settings.matrix() {
                    self.settings.set_matrix(matrix_enabled);
                }
                ui.end_row();

                let mut matrix_edge_darkness = self.settings.matrix_edge_darkness();
                ui.label("Matrix Edge Darkness");
                Slider::new(&mut matrix_edge_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .ui(ui);
                if matrix_edge_darkness != self.settings.matrix_edge_darkness() {
                    self.settings.set_matrix_edge_darkness(matrix_edge_darkness);
                }
                ui.end_row();

                let mut matrix_corner_darkness = self.settings.matrix_corner_darkness();
                ui.label("Matrix Corner Darkness");
                Slider::new(&mut matrix_corner_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .ui(ui);
                if matrix_corner_darkness != self.settings.matrix_corner_darkness() {
                    self.settings
                        .set_matrix_corner_darkness(matrix_corner_darkness);
                }
                ui.end_row();

                let mut ghosting_enabled = self.settings.ghosting();
                ui.label("Ghosting");
                ui.checkbox(&mut ghosting_enabled, "");
                if ghosting_enabled != self.settings.ghosting() {
                    self.settings.set_ghosting(ghosting_enabled);
                }
                ui.end_row();

                let mut ghosting_strength = self.settings.ghosting_strength();
                ui.label("Ghosting Strength");
                Slider::new(&mut ghosting_strength, 0.0..=1.0)
                    .step_by(0.01)
                    .ui(ui);
                if ghosting_strength != self.settings.ghosting_strength() {
                    self.settings.set_ghosting_strength(ghosting_strength);
                }
                ui.end_row();
            })
            .response
    }
}
