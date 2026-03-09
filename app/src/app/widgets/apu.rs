use crate::app::events::AppEventQueue;
use crate::app::tabs::Tab;
use crate::emulator::Emulator;
use crate::icons;
use egui::{Response, Ui, Widget};

pub struct ApuWidget<'a> {
    emulator: &'a mut Emulator,
    events: &'a mut AppEventQueue,
}

impl<'a> ApuWidget<'a> {
    pub fn new(emulator: &'a mut Emulator, events: &'a mut AppEventQueue) -> Self {
        Self { emulator, events }
    }
}

impl Widget for ApuWidget<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button(icons::WAVE_SAWTOOTH).clicked() {
                    self.events.open_tab(Tab::ApuWaves);
                }

                let mut ch1_enabled = !self.emulator.gb.debugger.ch1_disabled;
                let mut ch2_enabled = !self.emulator.gb.debugger.ch2_disabled;
                let mut ch3_enabled = !self.emulator.gb.debugger.ch3_disabled;
                let mut ch4_enabled = !self.emulator.gb.debugger.ch4_disabled;

                if ui.checkbox(&mut ch1_enabled, "1").changed() {
                    self.emulator.gb.debugger.ch1_disabled = !ch1_enabled;
                }
                if ui.checkbox(&mut ch2_enabled, "2").changed() {
                    self.emulator.gb.debugger.ch2_disabled = !ch2_enabled;
                }
                if ui.checkbox(&mut ch3_enabled, "3").changed() {
                    self.emulator.gb.debugger.ch3_disabled = !ch3_enabled;
                }
                if ui.checkbox(&mut ch4_enabled, "4").changed() {
                    self.emulator.gb.debugger.ch4_disabled = !ch4_enabled;
                }
            });
        })
        .response
    }
}
