use crate::Citrine;
use egui::{Grid, Ui};

pub fn debug(ui: &mut Ui, app: &mut Citrine) {
    ui.vertical(|ui| {
        ui.style_mut().override_font_id = Some(egui::FontId::monospace(14.0));
        Grid::new("registers_grid")
            .striped(true)
            .num_columns(4)
            .min_col_width(60.0)
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

                ui.label("PC");
                ui.label(format!("0x{:04X}", app.emulator.gb.cpu.pc));
                ui.label(format!("0x{:04X}", app.emulator.gb.cpu.sp));
                ui.label("SP");
                ui.end_row();

                ui.label("");
                ui.label("");
                ui.label("");
                ui.label("");
                ui.end_row();

                ui.label("DIV");
                ui.label(format!("0x{:04X}", app.emulator.gb.timer.div));
                ui.label(format!("0x{:02X}", app.emulator.gb.timer.tima));
                ui.label("TIMA");
                ui.end_row();

                ui.label("TMA");
                ui.label(format!("0x{:02X}", app.emulator.gb.timer.tma));
                ui.label(format!("0x{:02X}", app.emulator.gb.timer.tac));
                ui.label("TAC");
                ui.end_row();

                ui.label("");
                ui.label("");
                ui.label("");
                ui.label("");
                ui.end_row();

                ui.label("PPU");
                ui.label(format!("{:?}", app.emulator.gb.ppu.stat.ppu_mode));
                ui.label(format!("{}", app.emulator.gb.ppu.dot_counter));
                ui.label("DOTS");
                ui.end_row();

                ui.label("SCX");
                ui.label(format!("{}", app.emulator.gb.ppu.scx));
                ui.label(format!("{}", app.emulator.gb.ppu.scy));
                ui.label("SCY");
                ui.end_row();

                ui.label("LY");
                ui.label(format!("{}", app.emulator.gb.ppu.ly));
                ui.label(format!("{}", app.emulator.gb.ppu.lyc));
                ui.label("LYC");
                ui.end_row();

                ui.label("LYCI");
                ui.label(flag_text(app.emulator.gb.ppu.stat.lyc_interrupt));
                ui.label(flag_text(app.emulator.gb.ppu.stat.mode1_interrupt));
                ui.label("VBKI");
                ui.end_row();

                ui.label("HBKI");
                ui.label(flag_text(app.emulator.gb.ppu.stat.mode0_interrupt));
                ui.label(flag_text(app.emulator.gb.ppu.stat.mode2_interrupt));
                ui.label("OAMI");
                ui.end_row();

                ui.label("LCD");
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.lcd_enabled));
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.window_tilemap));
                ui.label("WT");
                ui.end_row();

                ui.label("WE");
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.window_enable));
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.bg_window_tiles));
                ui.label("BWT");
                ui.end_row();

                ui.label("BT");
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.bg_tilemap));
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.obj_size));
                ui.label("OS");
                ui.end_row();

                ui.label("OE");
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.obj_enable));
                ui.label(flag_text(app.emulator.gb.ppu.lcdc.bg_window_enable));
                ui.label("BWE");
                ui.end_row();

                ui.label("");
                ui.label("");
                ui.label("");
                ui.label("");
                ui.end_row();

                ui.label("IR");
                ui.label("FLAG");
                ui.label("ENABLE");
                ui.label("");
                ui.end_row();

                ui.label("VBK");
                ui.label(flag_text(app.emulator.gb.ic.flag.vblank));
                ui.label(flag_text(app.emulator.gb.ic.enable.vblank));
                ui.label("");
                ui.end_row();

                ui.label("LCD");
                ui.label(flag_text(app.emulator.gb.ic.flag.lcd));
                ui.label(flag_text(app.emulator.gb.ic.enable.lcd));
                ui.label("");
                ui.end_row();

                ui.label("TIM");
                ui.label(flag_text(app.emulator.gb.ic.flag.timer));
                ui.label(flag_text(app.emulator.gb.ic.enable.timer));
                ui.label("");
                ui.end_row();

                ui.label("SRL");
                ui.label(flag_text(app.emulator.gb.ic.flag.serial));
                ui.label(flag_text(app.emulator.gb.ic.enable.serial));
                ui.label("");
                ui.end_row();

                ui.label("JPD");
                ui.label(flag_text(app.emulator.gb.ic.flag.joypad));
                ui.label(flag_text(app.emulator.gb.ic.enable.joypad));
                ui.label("");
                ui.end_row();

                ui.label("IME");
                ui.label(flag_text(app.emulator.gb.cpu.ime));
                ui.label(flag_text(app.emulator.gb.cpu.halted));
                ui.label("HLT");
                ui.end_row();
            });
    });
}

fn flag_text(value: bool) -> &'static str {
    if value { "1" } else { "0" }
}
