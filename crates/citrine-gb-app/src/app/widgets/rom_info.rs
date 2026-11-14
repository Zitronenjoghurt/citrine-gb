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
        let rows = gb_rows(self.header);
        let column = ValueColumn::new(ui, rows.iter().map(|(name, _)| *name).chain(["Playing on"]));
        let mut console = self.console;
        info_grid("rom_info")
            .show(ui, |ui| {
                for (index, (name, value)) in rows.into_iter().enumerate() {
                    if index == 3
                        && let Some((model, events)) = console.take()
                    {
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
                    ui.label(name);
                    column.show(ui, value);
                    ui.end_row();
                }
            })
            .response
    }
}

fn gb_rows(header: &RomHeader) -> Vec<(&'static str, String)> {
    let maker = match header.licensee {
        citrine_gb::rom::header::RomLicensee::Unknown => "Unknown".to_owned(),
        licensee => licensee.to_string(),
    };
    let cartridge = header
        .cartridge_type
        .map(|ct| ct.to_string())
        .unwrap_or("Unknown".to_string());
    let pick = |yes: bool, shown: &str, hidden: &str| if yes { shown } else { hidden }.to_owned();
    vec![
        ("Title", header.title.clone()),
        ("Maker", maker),
        ("Works on", header.cgb_mode.to_string()),
        ("Cartridge type", cartridge),
        (
            "ROM size",
            format!("{} ({})", header.rom_size_pretty(), banks(header.rom_banks)),
        ),
        (
            "Save RAM",
            format!("{} ({})", header.ram_size_pretty(), banks(header.ram_banks)),
        ),
        (
            "Region",
            pick(header.overseas_only, "Outside Japan", "Japan"),
        ),
        (
            "Super Game Boy",
            pick(header.sgb_support, "Supported", "Not supported"),
        ),
        (
            "Nintendo logo",
            pick(header.valid_nintendo_logo, "OK", "Changed"),
        ),
        ("Version", header.version_number.to_string()),
        (
            "Checksum",
            checksum(
                header.provided_header_checksum.into(),
                header.actual_header_checksum.into(),
                2,
            ),
        ),
        (
            "File checksum",
            checksum(
                header.provided_global_checksum.into(),
                header.actual_global_checksum.into(),
                4,
            ),
        ),
    ]
}

#[cfg(feature = "gba")]
fn unknown_maker(code: &str) -> String {
    if code.is_empty() {
        "Unknown".to_owned()
    } else {
        format!("Unknown ({code})")
    }
}

fn banks(count: usize) -> String {
    if count == 1 {
        "1 bank".to_owned()
    } else {
        format!("{count} banks")
    }
}

struct ValueColumn {
    font: egui::FontId,
    room: f32,
}

impl ValueColumn {
    fn new<'n>(ui: &Ui, names: impl Iterator<Item = &'n str>) -> Self {
        let font = egui::TextStyle::Body.resolve(ui.style());
        let mut column = Self { font, room: 0.0 };
        let pad = ui.spacing().item_spacing.x;
        let names = names.map(|name| column.width(ui, name)).fold(0.0, f32::max);
        column.room = (ui.available_width() - names - pad).max(pad);
        column
    }

    fn width(&self, ui: &Ui, text: &str) -> f32 {
        ui.painter()
            .layout_no_wrap(
                text.to_owned(),
                self.font.clone(),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
    }

    fn show(&self, ui: &mut Ui, value: String) {
        if self.width(ui, &value) <= self.room {
            ui.label(value);
            return;
        }
        let value = if value.contains(", ") {
            value.replacen(", ", ",\n", 1)
        } else {
            value.replacen(" (", "\n(", 1)
        };
        ui.vertical(|ui| {
            ui.set_max_width(self.room);
            ui.add(egui::Label::new(value).wrap());
        });
    }
}

fn info_grid(id: &str) -> Grid {
    Grid::new(id).striped(true).num_columns(2)
}

fn checksum(provided: u32, actual: u32, digits: usize) -> String {
    if provided == actual {
        format!("0x{provided:0digits$X}, OK")
    } else {
        format!("0x{provided:0digits$X}, expected 0x{actual:0digits$X}")
    }
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
        HeaderRows::new("gba_rom_info", gba_rows(self.header, self.save)).ui(ui)
    }
}

#[cfg(feature = "gba")]
struct HeaderRows {
    id: &'static str,
    rows: Vec<(&'static str, String)>,
}

#[cfg(feature = "gba")]
impl HeaderRows {
    fn new(id: &'static str, rows: Vec<(&'static str, String)>) -> Self {
        Self { id, rows }
    }
}

#[cfg(feature = "gba")]
impl Widget for HeaderRows {
    fn ui(self, ui: &mut Ui) -> Response {
        let column = ValueColumn::new(ui, self.rows.iter().map(|(name, _)| *name));
        info_grid(self.id)
            .show(ui, |ui| {
                for (name, value) in self.rows {
                    ui.label(name);
                    column.show(ui, value);
                    ui.end_row();
                }
            })
            .response
    }
}

#[cfg(feature = "nds")]
pub struct NdsRomInfo<'a> {
    rom: &'a [u8],
}

#[cfg(feature = "nds")]
impl<'a> NdsRomInfo<'a> {
    pub fn new(rom: &'a [u8]) -> Self {
        Self { rom }
    }
}

#[cfg(feature = "nds")]
impl Widget for NdsRomInfo<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let Some(header) = citrine_gb::rom::nds::NdsHeader::parse(self.rom) else {
            return ui.small("No ROM loaded");
        };
        let banner = header.banner(self.rom);
        let icon = banner
            .as_ref()
            .and_then(|banner| crate::ds_banner::texture(ui.ctx(), &banner.icon));
        ui.vertical(|ui| {
            if let Some(icon) = icon {
                let (space, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(NDS_ICON_SIZE), egui::Sense::hover());
                let rect = crate::ds_banner::icon_rect(ui.ctx(), space.center(), NDS_ICON_SIZE);
                ui.painter().image(
                    icon.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                ui.add_space(ui.spacing().item_spacing.y);
            }
            HeaderRows::new("nds_rom_info", nds_rows(&header, banner.as_ref())).ui(ui);
        })
        .response
    }
}

#[cfg(feature = "nds")]
const NDS_ICON_SIZE: f32 = 64.0;

#[cfg(feature = "nds")]
pub fn nds_rows(
    header: &citrine_gb::rom::nds::NdsHeader,
    banner: Option<&citrine_gb::rom::nds::NdsBanner>,
) -> Vec<(&'static str, String)> {
    let language = crate::ds_banner::language();
    let title = banner
        .and_then(|banner| banner.title(language))
        .unwrap_or_else(|| header.title.clone());
    let shown = |text: String| {
        if text.is_empty() {
            "None".to_owned()
        } else {
            text
        }
    };
    let mut rows = vec![("Title", shown(title))];
    let maker = match header.maker_licensee() {
        citrine_gb::rom::header::RomLicensee::Unknown => unknown_maker(&header.maker()),
        maker => maker.to_string(),
    };
    let code = header.game_code();
    let game_code = if header.is_homebrew() && !code.is_empty() {
        format!("None ({code})")
    } else {
        shown(code)
    };
    let save = if header.is_homebrew() {
        "None".to_owned()
    } else {
        use citrine_gb::nds::save::{Listing, listing};
        match listing(header.game_code) {
            Listing::Chip(save) => nds_save_type(save).to_owned(),
            Listing::NoSave => "None".to_owned(),
            Listing::Unlisted => "Unknown".to_owned(),
        }
    };
    rows.extend([
        ("Game code", game_code),
        ("Maker", maker),
        ("Save type", save),
        ("Version", header.version.to_string()),
        (
            "Checksum",
            checksum(
                header.provided_checksum.into(),
                header.actual_checksum.into(),
                4,
            ),
        ),
    ]);
    rows
}

#[cfg(feature = "nds")]
fn nds_save_type(save: citrine_gb::nds::save::SaveType) -> &'static str {
    use citrine_gb::nds::save::SaveType;
    match save {
        SaveType::Eeprom512 => "EEPROM, 512 bytes",
        SaveType::Eeprom8K => "EEPROM, 8 KB",
        SaveType::Eeprom64K => "EEPROM, 64 KB",
        SaveType::Eeprom128K => "EEPROM, 128 KB",
        SaveType::Flash256K => "Flash, 256 KB",
        SaveType::Flash512K => "Flash, 512 KB",
        SaveType::Flash1M => "Flash, 1 MB",
        SaveType::Flash8M => "Flash, 8 MB",
        SaveType::Fram8K => "FRAM, 8 KB",
        SaveType::Fram32K => "FRAM, 32 KB",
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
        citrine_gb::rom::header::RomLicensee::Unknown => unknown_maker(&header.maker_code),
        maker => maker.to_string(),
    };
    vec![
        ("Title", shown(&header.title)),
        ("Game code", shown(&header.game_code)),
        ("Maker", maker),
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
        assert_eq!(value("Maker"), "None");
        assert_eq!(value("Save type"), "SRAM, 32 KB");
        assert!(rows.iter().all(|(row, _)| *row != "Playing on"));
        let checksum = if header.provided_checksum == header.actual_checksum {
            format!("0x{:02X}, OK", header.provided_checksum)
        } else {
            format!(
                "0x{:02X}, expected 0x{:02X}",
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
            let mut data = file.data.to_vec();
            data[0xBD] ^= 0xFF;
            file.data = data.into();
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
                if ["expected", "SRAM, 32 KB", "Checksum"]
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
                    if shown.contains("expected") {
                        let text = text.galley.text();
                        assert!(
                            !text.contains('\n') || text.split_once(",\n").is_some(),
                            "{text:?} at {width}"
                        );
                        if width >= 640.0 {
                            assert_eq!(rows, 1, "{text:?} at {width}");
                        }
                    }
                    seen.push(shown);
                }
            }
            assert!(
                seen.iter().any(|text| text.contains("expected")),
                "{seen:?} at {width}"
            );
        }
    }
}

#[cfg(all(test, feature = "nds"))]
mod nds_tests {
    use crate::app::files::nds_tests::{GAME, Opened, TITLE, fixture, icon_texture, shows_texture};
    use crate::app::tabs::Tab;
    use crate::app::tests::{info_frame_out, text_rect};
    use crate::app::ui_state::settings::LayoutMode;
    use egui::Context;

    #[test]
    fn rom_info_shows_the_ds_header_and_banner() {
        let data = fixture();
        let header = citrine_gb::rom::nds::NdsHeader::parse(&data).expect("a header");
        let rows = super::nds_rows(&header, header.banner(&data).as_ref());
        let value = |name: &str| {
            rows.iter()
                .find(|(row, _)| *row == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or_else(|| panic!("no {name} row"))
        };
        assert_eq!(value("Title"), TITLE);
        assert!(rows.iter().all(|(row, _)| *row != "Publisher"));
        assert_eq!(value("Game code"), "None (####)");
        assert!(rows.iter().all(|(row, _)| *row != "Playing on"));
        assert_eq!(value("Save type"), "None");
        assert_eq!(value("Checksum"), "0xD486, OK");
        assert!(GAME.ends_with(".nds"));
    }

    #[test]
    fn the_rom_info_tab_shows_the_ds_banner_on_desktop_and_phone() {
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
            for text in [TITLE, "None (####)", "0xD486, OK"] {
                assert!(text_rect(&output, text).is_some(), "{text} at {width}");
            }
            assert!(
                shows_texture(&output, icon_texture(&ctx)),
                "the icon at {width}"
            );
        }
    }
}

#[cfg(test)]
mod gb_tests {
    use super::*;
    use citrine_gb::rom::header::{RomCartridgeType, RomCgbMode};

    #[test]
    #[cfg(feature = "gba")]
    fn an_unknown_maker_says_so_and_keeps_its_code() {
        assert_eq!(unknown_maker("GC"), "Unknown (GC)");
        assert_eq!(unknown_maker(""), "Unknown");
    }

    #[test]
    fn gb_sizes_read_in_whole_kilobytes() {
        let header = RomHeader {
            rom_banks: 16,
            ram_banks: 1,
            ..Default::default()
        };
        let rows = gb_rows(&header);
        let value = |name: &str| &rows.iter().find(|(row, _)| *row == name).unwrap().1;
        assert_eq!(value("ROM size"), "256 KB (16 banks)");
        assert_eq!(value("Save RAM"), "8 KB (1 bank)");
    }

    #[test]
    fn long_gb_values_wrap_inside_a_narrow_dock() {
        let header = RomHeader {
            cgb_mode: RomCgbMode::CgbAndGb,
            cartridge_type: Some(RomCartridgeType::Mbc1RamBattery),
            ram_banks: 1,
            ..Default::default()
        };
        let width = 200.0;
        let ctx = egui::Context::default();
        let mut left = 0.0;
        let mut texts = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    left = ui.min_rect().left();
                    ui.allocate_ui(egui::vec2(width, 800.0), |ui| {
                        RomInfo::new(&header).ui(ui);
                    });
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().replace('\n', " "),
                        text.visual_bounding_rect(),
                    )),
                    _ => None,
                })
                .collect();
        }
        for wanted in [
            "Game Boy and Game Boy Color",
            "MBC1 + RAM + Battery",
            "(1 bank)",
        ] {
            assert!(
                texts.iter().any(|(text, _)| text.contains(wanted)),
                "{wanted}: {texts:?}"
            );
        }
        let cut: Vec<_> = texts
            .iter()
            .filter(|(_, rect)| rect.max.x > left + width + 0.5)
            .collect();
        assert!(cut.is_empty(), "past {}: {cut:?}", left + width);
        for (index, (text, rect)) in texts.iter().enumerate() {
            for (other, other_rect) in &texts[index + 1..] {
                assert!(
                    !rect.shrink(0.5).intersects(other_rect.shrink(0.5)),
                    "{text:?} overlaps {other:?}"
                );
            }
        }
    }
}
