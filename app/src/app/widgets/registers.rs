use crate::app::widgets::registers::cpu::CpuRegisters;
use crate::app::widgets::toggle_button::ToggleButton;
use citrine_gb::gb::GameBoy;
use egui::{Response, Ui, Widget};

mod cpu;
mod ir;
mod ppu;
mod timer;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct RegistersState {
    pub cpu: bool,
    pub ir: bool,
    pub ppu: bool,
    pub timer: bool,
}

pub struct Registers<'a> {
    gb: &'a GameBoy,
    state: &'a mut RegistersState,
}

impl<'a> Registers<'a> {
    pub fn new(gb: &'a GameBoy, state: &'a mut RegistersState) -> Self {
        Self { gb, state }
    }
}

impl Widget for Registers<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("REGISTERS");
                ToggleButton::new(&mut self.state.cpu, "CPU").ui(ui);
                ToggleButton::new(&mut self.state.ir, "IR").ui(ui);
                ToggleButton::new(&mut self.state.ppu, "PPU").ui(ui);
                ToggleButton::new(&mut self.state.timer, "TIMER").ui(ui);
            });

            if self.state.cpu {
                ui.separator();
                CpuRegisters::new(&self.gb.cpu).ui(ui);
            }

            if self.state.ir {
                ui.separator();
                ir::InterruptRegisters::new(&self.gb.cpu, &self.gb.ic).ui(ui);
            }

            if self.state.ppu {
                ui.separator();
                ppu::PpuRegisters::new(&self.gb.ppu).ui(ui);
            }

            if self.state.timer {
                ui.separator();
                timer::TimerRegisters::new(&self.gb.timer).ui(ui);
            }
        })
        .response
    }
}

fn flag_text(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}
