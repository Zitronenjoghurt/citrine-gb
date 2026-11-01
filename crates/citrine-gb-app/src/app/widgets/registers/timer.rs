use citrine_gb::gb::timer::Timer;
use egui::{Grid, Widget};

pub struct TimerRegisters<'a> {
    timer: &'a Timer,
}

impl<'a> TimerRegisters<'a> {
    pub fn new(timer: &'a Timer) -> Self {
        Self { timer }
    }
}

impl Widget for TimerRegisters<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            Grid::new("timer_registers_grid")
                .striped(true)
                .num_columns(4)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.style_mut().override_font_id = Some(egui::FontId::monospace(14.0));
                    ui.label("DIV");
                    ui.label(format!("0x{:04X}", self.timer.div));
                    ui.label(format!("0x{:02X}", self.timer.tima));
                    ui.label("TIMA");
                    ui.end_row();

                    ui.label("TMA");
                    ui.label(format!("0x{:02X}", self.timer.tma));
                    ui.label(format!("0x{:02X}", self.timer.tac));
                    ui.label("TAC");
                    ui.end_row();
                });
        })
        .response
    }
}
