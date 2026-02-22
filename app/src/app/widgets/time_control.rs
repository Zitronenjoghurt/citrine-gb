use crate::emulator::Emulator;
use crate::icons;
use egui::{Response, Ui, Widget};

pub struct TimeControl<'a> {
    emulator: &'a mut Emulator,
}

impl<'a> TimeControl<'a> {
    pub fn new(emulator: &'a mut Emulator) -> Self {
        Self { emulator }
    }
}

impl Widget for TimeControl<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            let start_stop = if self.emulator.running {
                icons::STOP
            } else {
                icons::PLAY
            };

            if ui.button(start_stop).clicked() {
                self.emulator.running = !self.emulator.running;
            }

            if ui.button(icons::STEPS).clicked() {
                self.emulator.force_step(ui.ctx());
            }
        })
        .response
    }
}
