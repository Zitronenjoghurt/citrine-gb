use crate::{icons, Citrine};
use egui::{Grid, Ui};

pub fn debug(ui: &mut Ui, app: &mut Citrine) {
    ui.vertical(|ui| {
        ui.style_mut().override_font_id = Some(egui::FontId::monospace(14.0));
        Grid::new("registers_grid")
            .striped(true)
            .num_columns(4)
            .show(ui, |ui| {
                ui.label("A");
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.a));
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.b));
                ui.label("B");
                ui.end_row();

                ui.label("C");
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.c));
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.d));
                ui.label("D");
                ui.end_row();

                ui.label("E");
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.e));
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.h));
                ui.label("H");
                ui.end_row();

                ui.label("L");
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.l));
                ui.label(format!("0x{:02X}", app.emulator.gb.cpu.ir));
                ui.label("IR");
                ui.end_row();

                ui.label("PC");
                ui.label(format!("0x{:04X}", app.emulator.gb.cpu.pc));
                ui.label(format!("0x{:04X}", app.emulator.gb.cpu.sp));
                ui.label("SP");
                ui.end_row();

                ui.style_mut().override_font_id = None;

                ui.label("Z");
                ui.label(flag_text(app.emulator.gb.cpu.f.zero));
                ui.label(flag_text(app.emulator.gb.cpu.f.subtract));
                ui.label("N");
                ui.end_row();

                ui.label("H");
                ui.label(flag_text(app.emulator.gb.cpu.f.half_carry));
                ui.label(flag_text(app.emulator.gb.cpu.f.carry));
                ui.label("C");
                ui.end_row();
            });
    });
}

fn flag_text(value: bool) -> &'static str {
    if value { icons::CHECK } else { icons::X }
}
