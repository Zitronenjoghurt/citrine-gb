use crate::app::widgets::registers::flag_text;
use citrine_gb::gb::cpu::Cpu;
use egui::{Grid, Widget};

pub struct CpuRegisters<'a> {
    cpu: &'a Cpu,
}

impl<'a> CpuRegisters<'a> {
    pub fn new(cpu: &'a Cpu) -> Self {
        Self { cpu }
    }
}

impl Widget for CpuRegisters<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            Grid::new("cpu_registers_grid")
                .striped(true)
                .num_columns(4)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.style_mut().override_font_id = Some(egui::FontId::monospace(14.0));
                    ui.label("A");
                    ui.label(format!("0x{:02X}", self.cpu.a));
                    ui.label(format!("0x{:02X}", self.cpu.b));
                    ui.label("B");
                    ui.end_row();

                    ui.label("C");
                    ui.label(format!("0x{:02X}", self.cpu.c));
                    ui.label(format!("0x{:02X}", self.cpu.d));
                    ui.label("D");
                    ui.end_row();

                    ui.label("E");
                    ui.label(format!("0x{:02X}", self.cpu.e));
                    ui.label(format!("0x{:02X}", self.cpu.h));
                    ui.label("H");
                    ui.end_row();

                    ui.label("L");
                    ui.label(format!("0x{:02X}", self.cpu.l));
                    ui.label(format!("0x{:02X}", self.cpu.ir));
                    ui.label("IR");
                    ui.end_row();

                    ui.label("Z");
                    ui.label(flag_text(self.cpu.f.zero));
                    ui.label(flag_text(self.cpu.f.subtract));
                    ui.label("N");
                    ui.end_row();

                    ui.label("H");
                    ui.label(flag_text(self.cpu.f.half_carry));
                    ui.label(flag_text(self.cpu.f.carry));
                    ui.label("C");
                    ui.end_row();

                    ui.label("PC");
                    ui.label(format!("0x{:04X}", self.cpu.pc));
                    ui.label(format!("0x{:04X}", self.cpu.sp));
                    ui.label("SP");
                    ui.end_row();
                });
        })
        .response
    }
}
