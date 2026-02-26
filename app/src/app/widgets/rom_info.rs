use citrine_gb::rom::header::RomHeader;
use egui::{Grid, Response, Ui, Widget};

pub struct RomInfo<'a> {
    header: &'a RomHeader,
}

impl<'a> RomInfo<'a> {
    pub fn new(header: &'a RomHeader) -> Self {
        Self { header }
    }
}

impl Widget for RomInfo<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        Grid::new("rom_info")
            .striped(true)
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Title");
                ui.label(&self.header.title);
                ui.end_row();

                ui.label("Licensee");
                ui.label(self.header.licensee.to_string());
                ui.end_row();

                ui.label("Compatibility");
                ui.label(self.header.cgb_mode.to_string());
                ui.end_row();

                ui.label("Cartridge Type");
                ui.label(
                    self.header
                        .cartridge_type
                        .map(|ct| ct.to_string())
                        .unwrap_or("Unknown".to_string()),
                );
                ui.end_row();

                ui.label("Expected ROM Size");
                ui.label(format!(
                    "{} ({} banks)",
                    self.header.rom_size_pretty(),
                    self.header.rom_banks
                ));
                ui.end_row();

                ui.label("Expected RAM Size");
                ui.label(format!(
                    "{} ({} banks)",
                    self.header.ram_size_pretty(),
                    self.header.ram_banks
                ));
                ui.end_row();

                ui.label("Overseas Only");
                ui.label(yes_no(self.header.overseas_only));
                ui.end_row();

                ui.label("SGB Support");
                ui.label(yes_no(self.header.sgb_support));
                ui.end_row();

                ui.label("Valid Nintendo Logo");
                ui.label(yes_no(self.header.valid_nintendo_logo));
                ui.end_row();

                ui.label("Version Number");
                ui.label(format!("{}", self.header.version_number));
                ui.end_row();

                ui.label("Header Checksum");
                ui.label(format!(
                    "Provided: 0x{:02X} | Calculated: 0x{:02X}",
                    self.header.provided_header_checksum, self.header.actual_header_checksum
                ));
                ui.end_row();

                ui.label("Global Checksum");
                ui.label(format!(
                    "Provided: 0x{:04X} | Calculated: 0x{:04X}",
                    self.header.provided_global_checksum, self.header.actual_global_checksum
                ));
                ui.end_row();

                ui.label("Entrypoint");
                ui.label(format!("{}", self.header.entrypoint));
                ui.end_row();
            })
            .response
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}
