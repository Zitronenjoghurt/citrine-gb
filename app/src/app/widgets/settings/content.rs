use crate::app::settings::{Settings, SettingsTab};
use crate::app::widgets::enum_select::EnumSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use citrine_gb::gb::ppu::types::theme::DmgTheme;
use egui::{Grid, Response, ScrollArea, Ui, Widget};

pub struct SettingsContent<'a> {
    settings: &'a mut Settings,
}

impl<'a> SettingsContent<'a> {
    pub fn new(settings: &'a mut Settings) -> Self {
        Self { settings }
    }
}

impl Widget for SettingsContent<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ScrollArea::vertical()
            .show(ui, |ui| {
                ui.vertical_centered(|ui| match self.settings.current_tab {
                    SettingsTab::General => self.general(ui),
                    SettingsTab::Sound => self.sound(ui),
                    SettingsTab::Style => self.style(ui),
                    SettingsTab::Developer => self.developer(ui),
                })
                .response
            })
            .inner
    }
}

impl SettingsContent<'_> {
    pub fn general(mut self, ui: &mut Ui) {
        ui.heading("General");
        ui.separator();

        let s = &mut self.settings;

        Grid::new("settings_general_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("UI Scale");

                let response = ResetSlider::new(&mut s.ui_scale, 0.5..=5.0)
                    .step_by(0.1)
                    .default_value(Settings::DEFAULT_UI_SCALE)
                    .ui(ui);

                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    s.dirty = true;
                }

                ui.end_row();
            });
    }

    pub fn sound(mut self, ui: &mut Ui) {
        ui.heading("Sound");
        ui.separator();

        let s = &mut self.settings;

        Grid::new("settings_sound_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Volume");
                s.dirty |= ResetSlider::new(&mut s.volume, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_VOLUME)
                    .ui(ui)
                    .changed();
                ui.end_row();
            });
    }

    pub fn style(mut self, ui: &mut Ui) {
        ui.heading("Style");
        ui.separator();

        let s = &mut self.settings;

        Grid::new("settings_style_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Theme");
                s.dirty |= EnumSelect::new(&mut s.dmg_theme, "enum_select_dmg_theme")
                    .default_value(DmgTheme::Citrine)
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.separator();
                ui.separator();
                ui.end_row();

                ui.label("Matrix");
                s.dirty |= ui.checkbox(&mut s.matrix, "").changed();
                ui.end_row();

                ui.label("Edge Darkness");
                s.dirty |= ResetSlider::new(&mut s.matrix_edge_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_MATRIX_EDGE_DARKNESS)
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.label("Corner Darkness");
                s.dirty |= ResetSlider::new(&mut s.matrix_corner_darkness, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_MATRIX_CORNER_DARKNESS)
                    .ui(ui)
                    .changed();
                ui.end_row();

                ui.separator();
                ui.separator();
                ui.end_row();

                ui.label("Ghosting");
                s.dirty |= ui.checkbox(&mut s.ghosting, "").changed();
                ui.end_row();

                ui.label("Strength");
                s.dirty |= ResetSlider::new(&mut s.ghosting_strength, 0.0..=1.0)
                    .step_by(0.01)
                    .default_value(Settings::DEFAULT_GHOSTING_STRENGTH)
                    .ui(ui)
                    .changed();
                ui.end_row();
            });
    }

    pub fn developer(self, ui: &mut Ui) {}
}
