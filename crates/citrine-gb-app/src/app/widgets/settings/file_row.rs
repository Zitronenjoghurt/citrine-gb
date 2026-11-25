use crate::app::boot_rom::BootRom;
#[cfg(feature = "nds")]
use crate::app::ds_firmware::DsFile;
use crate::app::events::AppEventQueue;
use crate::app::system_files::{FileChoice, SystemFile};
use crate::app::ui_state::settings_tree::Surface;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::settings::form::Form;
use egui::{Ui, Widget};

pub fn file_row(
    form: &mut Form,
    file: SystemFile,
    name: Option<&str>,
    events: &mut Option<&mut AppEventQueue>,
) {
    let row = |ui: &mut Ui| {
        let mut choice = FileChoice::current(file, name);
        let response =
            GenericSelect::new(&mut choice, FileChoice::all(file, name), select_id(file))
                .default_value(file.empty())
                .middle_ellipsis()
                .ui(ui);
        if let Some(name) = name {
            response.clone().on_hover_text(name);
        }
        if response.changed()
            && let Some(events) = events.as_deref_mut()
        {
            choice.apply(file, events);
        }
    };
    let label = file.row().label(Surface::Desktop);
    form.row_with_hint(label, file.hint(), row);
}

fn select_id(file: SystemFile) -> &'static str {
    match file {
        SystemFile::BootRom(BootRom::Gb) => "enum_select_gb_boot_rom",
        SystemFile::BootRom(BootRom::Gbc) => "enum_select_gbc_boot_rom",
        #[cfg(feature = "gba")]
        SystemFile::GbaBios => "enum_select_gba_bios",
        #[cfg(feature = "nds")]
        SystemFile::Ds(DsFile::Arm9Bios) => "enum_select_ds_arm9_bios",
        #[cfg(feature = "nds")]
        SystemFile::Ds(DsFile::Arm7Bios) => "enum_select_ds_arm7_bios",
        #[cfg(feature = "nds")]
        SystemFile::Ds(DsFile::Firmware) => "enum_select_ds_firmware",
    }
}
