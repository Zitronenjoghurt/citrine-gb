use crate::app::events::AppEventQueue;
use crate::app::ui_state::settings::{console_name, other_console, plays_on_both};
use citrine_gb::gb::GbModel;
use citrine_gb::rom::header::RomHeader;
use egui::{Grid, Response, Ui, Widget};

pub struct RomInfo<'a> {
    header: &'a RomHeader,
    console: Option<(GbModel, &'a mut AppEventQueue)>,
}

impl<'a> RomInfo<'a> {
    pub fn new(header: &'a RomHeader) -> Self {
        Self {
            header,
            console: None,
        }
    }

    pub fn console(mut self, model: GbModel, events: &'a mut AppEventQueue) -> Self {
        self.console = Some((model, events));
        self
    }
}

impl Widget for RomInfo<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        info_grid("rom_info")
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

                if let Some((model, events)) = self.console {
                    ui.label("Playing on");
                    ui.vertical(|ui| {
                        ui.label(console_name(model));
                        let other = other_console(model);
                        if plays_on_both(self.header.cgb_mode)
                            && ui
                                .button(format!("Play on {}", console_name(other)))
                                .clicked()
                        {
                            events.play_on(other);
                        }
                    });
                    ui.end_row();
                }

                ui.label("Cartridge type");
                ui.label(
                    self.header
                        .cartridge_type
                        .map(|ct| ct.to_string())
                        .unwrap_or("Unknown".to_string()),
                );
                ui.end_row();

                ui.label("Expected ROM size");
                ui.label(format!(
                    "{} ({} banks)",
                    self.header.rom_size_pretty(),
                    self.header.rom_banks
                ));
                ui.end_row();

                ui.label("Expected RAM size");
                ui.label(format!(
                    "{} ({} banks)",
                    self.header.ram_size_pretty(),
                    self.header.ram_banks
                ));
                ui.end_row();

                ui.label("Overseas only");
                ui.label(yes_no(self.header.overseas_only));
                ui.end_row();

                ui.label("SGB support");
                ui.label(yes_no(self.header.sgb_support));
                ui.end_row();

                ui.label("Valid Nintendo logo");
                ui.label(yes_no(self.header.valid_nintendo_logo));
                ui.end_row();

                ui.label("Version");
                ui.label(format!("{}", self.header.version_number));
                ui.end_row();

                ui.label("Header checksum");
                ui.label(checksum(
                    self.header.provided_header_checksum.into(),
                    self.header.actual_header_checksum.into(),
                    2,
                ));
                ui.end_row();

                ui.label("Global checksum");
                ui.label(checksum(
                    self.header.provided_global_checksum.into(),
                    self.header.actual_global_checksum.into(),
                    4,
                ));
                ui.end_row();
            })
            .response
    }
}

fn info_grid(id: &str) -> Grid {
    Grid::new(id).striped(true).num_columns(2)
}

fn checksum(provided: u32, actual: u32, digits: usize) -> String {
    if provided == actual {
        format!("0x{provided:0digits$X}, correct")
    } else {
        format!("0x{provided:0digits$X}, should be 0x{actual:0digits$X}")
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

#[cfg(feature = "gba")]
pub struct GbaRomInfo<'a> {
    header: &'a citrine_gb::gba::header::GbaHeader,
    save: citrine_gb::gba::save::SaveType,
}

#[cfg(feature = "gba")]
impl<'a> GbaRomInfo<'a> {
    pub fn new(
        header: &'a citrine_gb::gba::header::GbaHeader,
        save: citrine_gb::gba::save::SaveType,
    ) -> Self {
        Self { header, save }
    }
}

#[cfg(feature = "gba")]
impl Widget for GbaRomInfo<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let rows = gba_rows(self.header, self.save);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let width = |ui: &Ui, text: &str| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .size()
                .x
        };
        let names = rows
            .iter()
            .map(|(name, _)| width(ui, name))
            .fold(0.0, f32::max);
        let pad = ui.spacing().item_spacing.x;
        let room = (ui.available_width() - names - pad).max(pad);
        info_grid("gba_rom_info")
            .show(ui, |ui| {
                for (name, value) in rows {
                    ui.label(name);
                    let value = if width(ui, &value) > room {
                        value.replacen(", ", ",\n", 1)
                    } else {
                        value
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let shown = ui
                            .scope(|ui| {
                                ui.set_max_width(room);
                                ui.add(egui::Label::new(value).wrap())
                            })
                            .inner;
                        ui.add_space(pad.min(room - shown.rect.width()).max(0.0));
                    });
                    ui.end_row();
                }
            })
            .response
    }
}

#[cfg(feature = "gba")]
pub fn gba_rows(
    header: &citrine_gb::gba::header::GbaHeader,
    save: citrine_gb::gba::save::SaveType,
) -> Vec<(&'static str, String)> {
    let shown = |text: &str| {
        if text.is_empty() {
            "None".to_owned()
        } else {
            text.to_owned()
        }
    };
    let maker = match header.maker {
        citrine_gb::rom::header::RomLicensee::Unknown => shown(&header.maker_code),
        maker => format!("{maker} ({})", header.maker_code),
    };
    vec![
        ("Title", shown(&header.title)),
        ("Game code", shown(&header.game_code)),
        ("Maker", maker),
        ("Playing on", "Game Boy Advance".to_owned()),
        ("Save type", save_type(save).to_owned()),
        ("Version", header.version.to_string()),
        (
            "Checksum",
            checksum(
                header.provided_checksum.into(),
                header.actual_checksum.into(),
                2,
            ),
        ),
    ]
}

#[cfg(feature = "gba")]
fn save_type(save: citrine_gb::gba::save::SaveType) -> &'static str {
    use citrine_gb::gba::save::SaveType;
    match save {
        SaveType::Auto => "Not known until the game saves",
        SaveType::Sram32K => "SRAM, 32 KB",
        SaveType::Sram64K => "SRAM, 64 KB",
        SaveType::Flash64K => "Flash, 64 KB",
        SaveType::Flash128K => "Flash, 128 KB",
        SaveType::Eeprom => "EEPROM, size not known until the game saves",
        SaveType::Eeprom512 => "EEPROM, 512 bytes",
        SaveType::Eeprom8K => "EEPROM, 8 KB",
    }
}

#[cfg(all(test, feature = "gba"))]
mod tests {
    use crate::app::files::tests::Opened;
    use crate::app::tabs::Tab;
    use crate::app::tests::{info_frame_out, text_rect};
    use crate::app::ui_state::settings::LayoutMode;
    use egui::Context;

    #[test]
    fn rom_info_shows_the_gba_header() {
        let mut opened = Opened::new("rom-info");
        let file = opened.picked();
        opened.app.handle_load_rom(file);
        let gba = opened.app.emulator.as_gba().expect("a gba");
        let header = citrine_gb::gba::header::GbaHeader::parse(gba.bus().rom());
        let rows = crate::app::widgets::rom_info::gba_rows(&header, gba.bus().backup().kind());
        let value = |name: &str| {
            rows.iter()
                .find(|(row, _)| *row == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or_else(|| panic!("no {name} row"))
        };
        assert_eq!(value("Title"), "CITRINE SAVE");
        assert_eq!(value("Game code"), "ZCSE");
        assert_eq!(value("Maker"), "None (00)");
        assert_eq!(value("Save type"), "SRAM, 32 KB");
        assert_eq!(value("Playing on"), "Game Boy Advance");
        let checksum = if header.provided_checksum == header.actual_checksum {
            format!("0x{:02X}, correct", header.provided_checksum)
        } else {
            format!(
                "0x{:02X}, should be 0x{:02X}",
                header.provided_checksum, header.actual_checksum
            )
        };
        assert_eq!(value("Checksum"), checksum);
    }

    #[test]
    fn the_rom_info_tab_shows_the_gba_header_on_desktop_and_phone() {
        for (width, layout) in [(1280.0, LayoutMode::Desktop), (412.0, LayoutMode::Mobile)] {
            let mut opened = Opened::new(&format!("rom-info-tab-{width}"));
            opened.app.ui.settings.layout = layout;
            let file = opened.picked();
            opened.app.handle_load_rom(file);
            opened.app.open_tab(Tab::RomInfo);
            opened.app.mobile.open(Tab::RomInfo);
            let ctx = Context::default();
            info_frame_out(&mut opened.app, &ctx, width, 900.0);
            let output = info_frame_out(&mut opened.app, &ctx, width, 900.0);
            for text in ["CITRINE SAVE", "ZCSE", "SRAM, 32 KB"] {
                assert!(text_rect(&output, text).is_some(), "{text} at {width}");
            }
        }
    }

    #[test]
    fn rom_info_values_stay_inside_the_narrowest_desktop_dock() {
        for width in [560.0, 640.0, 800.0, 1280.0] {
            let mut opened = Opened::new(&format!("rom-info-fit-{width}"));
            opened.app.ui.settings.layout = LayoutMode::Desktop;
            let mut file = opened.picked();
            file.data[0xBD] ^= 0xFF;
            opened.app.handle_load_rom(file);
            opened.app.open_tab(Tab::RomInfo);
            let ctx = Context::default();
            info_frame_out(&mut opened.app, &ctx, width, 900.0);
            info_frame_out(&mut opened.app, &ctx, width, 900.0);
            let output = info_frame_out(&mut opened.app, &ctx, width, 900.0);
            let mut seen = Vec::new();
            for clipped in &output.shapes {
                let egui::Shape::Text(text) = &clipped.shape else {
                    continue;
                };
                let shown = text.galley.text().replace('\n', " ");
                if ["Game Boy Advance", "should be", "SRAM, 32 KB", "Checksum"]
                    .iter()
                    .any(|wanted| shown.contains(wanted))
                {
                    let rect = text.visual_bounding_rect();
                    assert!(
                        rect.max.x <= clipped.clip_rect.max.x + 0.5,
                        "{shown:?} ends at {} past {} at {width}",
                        rect.max.x,
                        clipped.clip_rect.max.x
                    );
                    let rows = text.galley.rows.len();
                    if shown.contains("should be") {
                        let text = text.galley.text();
                        assert!(
                            !text.contains('\n') || text.split_once(",\n").is_some(),
                            "{text:?} at {width}"
                        );
                        if width >= 640.0 {
                            assert_eq!(rows, 1, "{text:?} at {width}");
                        }
                    }
                    if shown == "Game Boy Advance" && width >= 640.0 {
                        assert_eq!(rows, 1, "Game Boy Advance at {width}");
                    }
                    seen.push(shown);
                }
            }
            assert!(
                seen.iter().any(|text| text.contains("should be")),
                "{seen:?} at {width}"
            );
        }
    }
}
