use crate::app::widgets::registers::flag_text;
use citrine_gb::gb::ppu::Ppu;
use egui::{Grid, Widget};

pub struct PpuRegisters<'a> {
    ppu: &'a Ppu,
}

impl<'a> PpuRegisters<'a> {
    pub fn new(ppu: &'a Ppu) -> Self {
        Self { ppu }
    }
}

impl Widget for PpuRegisters<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            Grid::new("ppu_registers_grid")
                .striped(true)
                .num_columns(4)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.style_mut().override_font_id = Some(egui::FontId::monospace(14.0));
                    ui.label("PPU");
                    ui.label(format!("{:?}", self.ppu.stat.ppu_mode));
                    ui.label("XXXX");
                    ui.label("DOTS");
                    ui.end_row();

                    ui.label("SCX");
                    ui.label(format!("{}", self.ppu.scx));
                    ui.label(format!("{}", self.ppu.scy));
                    ui.label("SCY");
                    ui.end_row();

                    ui.label("LY");
                    ui.label(format!("{}", self.ppu.ly));
                    ui.label(format!("{}", self.ppu.lyc));
                    ui.label("LYC");
                    ui.end_row();

                    ui.label("LYCI");
                    ui.label(flag_text(self.ppu.stat.lyc_interrupt));
                    ui.label(flag_text(self.ppu.stat.mode1_interrupt));
                    ui.label("VBKI");
                    ui.end_row();

                    ui.label("HBKI");
                    ui.label(flag_text(self.ppu.stat.mode0_interrupt));
                    ui.label(flag_text(self.ppu.stat.mode2_interrupt));
                    ui.label("OAMI");
                    ui.end_row();

                    ui.label("LCD");
                    ui.label(flag_text(self.ppu.lcdc.lcd_enabled));
                    ui.label(flag_text(self.ppu.lcdc.window_tilemap));
                    ui.label("WT");
                    ui.end_row();

                    ui.label("WE");
                    ui.label(flag_text(self.ppu.lcdc.window_enable));
                    ui.label(flag_text(self.ppu.lcdc.bg_window_tiles));
                    ui.label("BWT");
                    ui.end_row();

                    ui.label("BT");
                    ui.label(flag_text(self.ppu.lcdc.bg_tilemap));
                    ui.label(flag_text(self.ppu.lcdc.obj_size));
                    ui.label("OS");
                    ui.end_row();

                    ui.label("OE");
                    ui.label(flag_text(self.ppu.lcdc.obj_enable));
                    ui.label(flag_text(self.ppu.lcdc.bg_window_enable));
                    ui.label("BWE");
                    ui.end_row();

                    ui.label("WX");
                    ui.label(self.ppu.wx.to_string());
                    ui.label(self.ppu.wy.to_string());
                    ui.label("WY");
                    ui.end_row();

                    ui.label("WL");
                    ui.label(self.ppu.wl.to_string());
                    ui.label("");
                    ui.label("");
                    ui.end_row();
                });
        })
        .response
    }
}
