use crate::app::ui_state::UiState;
use crate::emulator::Emulator;
use egui::{Grid, Response, Widget};

pub struct PerformanceWidget<'a> {
    emulator: &'a Emulator,
    ui: &'a UiState,
}

impl<'a> PerformanceWidget<'a> {
    pub fn new(emulator: &'a Emulator, ui: &'a UiState) -> Self {
        Self { emulator, ui }
    }
}

impl Widget for PerformanceWidget<'_> {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        Grid::new("performance_grid")
            .num_columns(4)
            .show(ui, |ui| {
                ui.strong("Type");
                ui.strong("Average");
                ui.strong("Frequency");
                ui.strong("Load");
                ui.end_row();

                ui.label("App updates");
                ui.label(self.ui.update_avg_timer.display_average_secs());
                ui.label(self.ui.update_avg_timer.display_updates_per_sec());
                ui.label(self.ui.update_avg_timer.display_budget());
                ui.end_row();

                ui.label("Emulator updates");
                ui.label(self.emulator.update_avg_timer.display_average_secs());
                ui.label(self.emulator.update_avg_timer.display_updates_per_sec());
                ui.label(self.emulator.update_avg_timer.display_budget());
                ui.end_row();

                ui.label("Emulator frames");
                ui.label(self.emulator.frame_avg_timer.display_average_secs());
                ui.label(self.emulator.frame_avg_timer.display_updates_per_sec());
                ui.label(self.emulator.frame_avg_timer.display_budget());
                ui.end_row();
            })
            .response
    }
}
