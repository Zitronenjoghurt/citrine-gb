use crate::app::widgets::registers::flag_text;
use citrine_gb::gb::cpu::Cpu;
use citrine_gb::gb::ic::InterruptController;
use egui::{Grid, Widget};

pub struct InterruptRegisters<'a> {
    cpu: &'a Cpu,
    ic: &'a InterruptController,
}

impl<'a> InterruptRegisters<'a> {
    pub fn new(cpu: &'a Cpu, ic: &'a InterruptController) -> Self {
        Self { cpu, ic }
    }
}

impl Widget for InterruptRegisters<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            Grid::new("interrupt_registers_grid")
                .striped(true)
                .num_columns(4)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.style_mut().override_font_id = Some(egui::FontId::monospace(14.0));
                    ui.label("IR");
                    ui.label("FLAG");
                    ui.label("ENABLE");
                    ui.label("");
                    ui.end_row();

                    ui.label("VBK");
                    ui.label(flag_text(self.ic.flag.vblank));
                    ui.label(flag_text(self.ic.enable.vblank));
                    ui.label("");
                    ui.end_row();

                    ui.label("LCD");
                    ui.label(flag_text(self.ic.flag.lcd));
                    ui.label(flag_text(self.ic.enable.lcd));
                    ui.label("");
                    ui.end_row();

                    ui.label("TIM");
                    ui.label(flag_text(self.ic.flag.timer));
                    ui.label(flag_text(self.ic.enable.timer));
                    ui.label("");
                    ui.end_row();

                    ui.label("SRL");
                    ui.label(flag_text(self.ic.flag.serial));
                    ui.label(flag_text(self.ic.enable.serial));
                    ui.label("");
                    ui.end_row();

                    ui.label("JPD");
                    ui.label(flag_text(self.ic.flag.joypad));
                    ui.label(flag_text(self.ic.enable.joypad));
                    ui.label("");
                    ui.end_row();

                    ui.label("IME");
                    ui.label(flag_text(self.cpu.ime));
                    ui.label(flag_text(self.cpu.halted));
                    ui.label("HLT");
                    ui.end_row();
                });
        })
        .response
    }
}
