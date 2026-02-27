use crate::emulator::Emulator;
use crate::icons;
use citrine_gb::gb::GbModel;
use egui::{Response, Slider, Ui, Widget};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TimeControlState {
    pub step_cycles: u32,
    pub start_cycles: u32,
}

impl Default for TimeControlState {
    fn default() -> Self {
        Self {
            step_cycles: 1,
            start_cycles: 0,
        }
    }
}

pub struct TimeControl<'a> {
    emulator: &'a mut Emulator,
    state: &'a mut TimeControlState,
}

impl<'a> TimeControl<'a> {
    pub fn new(emulator: &'a mut Emulator, state: &'a mut TimeControlState) -> Self {
        Self { emulator, state }
    }
}

impl Widget for TimeControl<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let start_stop = if self.emulator.running {
                    icons::STOP
                } else {
                    icons::PLAY
                };

                if ui.button(start_stop).clicked() {
                    self.emulator.running = !self.emulator.running;
                }
            });

            if !self.emulator.running {
                ui.separator();

                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("Step");
                        if ui.button(icons::STEPS).clicked() {
                            self.emulator.force_step(ui.ctx(), self.state.step_cycles);
                        }

                        if ui.button("DMG").clicked() {
                            self.state.step_cycles = GbModel::Dmg.frame_cycles();
                        }

                        if ui.button("CGB").clicked() {
                            self.state.step_cycles = GbModel::Cgb.frame_cycles();
                        }

                        if ui.button("100").clicked() {
                            self.state.step_cycles = 100;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Cycles");
                        Slider::new(&mut self.state.step_cycles, 1..=1_000_000)
                            .logarithmic(true)
                            .ui(ui);
                    });
                });
            }

            ui.separator();
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Reset");
                    if ui.button(icons::CLOCK_COUNTER_CLOCKWISE).clicked() {
                        self.emulator.reset_to(ui.ctx(), self.state.start_cycles);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Start cycles");
                    Slider::new(&mut self.state.start_cycles, 0..=100_000_000)
                        .logarithmic(true)
                        .ui(ui);
                });
            });
        })
        .response
    }
}
