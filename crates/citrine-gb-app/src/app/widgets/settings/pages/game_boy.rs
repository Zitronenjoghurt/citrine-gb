use crate::app::boot_rom::BootRom;
use crate::app::events::AppEventQueue;
use crate::app::system_files::SystemFile;
use crate::app::ui_state::settings::{ConsoleChoice, GBC_SCREEN_NOTE, Settings};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::settings::content::{
    Device, SettingsContent, groups, hidden_rows, label, rows, scope_bar, tagged, view,
};
use crate::app::widgets::settings::file_row::file_row;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::app::widgets::settings::palette::{self, PaletteEditorState};
use crate::emulator::gimmicks::CameraSource;
use citrine_gb::gb::ppu::types::color::ColorCorrection;
use egui::{Ui, Widget};

const GB_BOOT_ROM: SystemFile = SystemFile::BootRom(BootRom::Gb);
const GBC_BOOT_ROM: SystemFile = SystemFile::BootRom(BootRom::Gbc);

pub(in crate::app::widgets::settings) const CHANNELS_HINT: &str = "The Game Boy's four sound channels. Pulse 1 and 2 usually carry the tune, Wave the bass and Noise the drums.";

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let editor = content.palette_editor;
    let device = content.device;
    let boot_roms = content.boot_roms;
    let mut events = content.events;
    let view = view(s, SettingsTab::GameBoy);
    let mut draw = |form: &mut Form, s: &mut Settings, row: RowId| match row {
        RowId::GbBootRom => file_row(form, GB_BOOT_ROM, boot_roms[0], &mut events),
        RowId::GbcBootRom => file_row(form, GBC_BOOT_ROM, boot_roms[1], &mut events),
        _ => row_ui(form, s, editor, device, &mut events, row),
    };
    Page::show(ui, |page| {
        scope_bar(page, s, view);
        for group in groups(SettingsTab::GameBoy, view) {
            let hint = (group == GroupId::GbChannels).then_some(CHANNELS_HINT);
            page.tree_group(group, hint, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        tagged(form, s, view, row, |form, s| draw(form, s, row));
                    }
                })
            });
        }
        hidden_rows(page, s, SettingsTab::GameBoy, view, |form, s, _, row| {
            draw(form, s, row)
        });
    });
}

fn row_ui(
    form: &mut Form,
    s: &mut Settings,
    editor: &mut PaletteEditorState,
    device: Device,
    events: &mut Option<&mut AppEventQueue>,
    row: RowId,
) {
    match row {
        RowId::RunGamesOn => {
            form.row_with_hint(
                label(row),
                "Automatic picks Game Boy Color for games made for it.",
                |ui| {
                    s.dirty |= GenericSelect::from_enum(&mut s.console, "enum_select_console")
                        .default_value(ConsoleChoice::default())
                        .ui(ui)
                        .changed();
                },
            );
        }
        RowId::CameraSees if device.live_camera => {
            form.row(label(row), |ui| {
                s.dirty |=
                    GenericSelect::from_enum(&mut s.gimmicks.camera, "enum_select_camera_source")
                        .default_value(CameraSource::default())
                        .ui(ui)
                        .changed();
            });
            if s.gimmicks.camera == CameraSource::Camera
                && let Some(error) = device.camera_error
            {
                form.hint(error);
            }
        }
        RowId::ChoosePicture
            if !device.live_camera || s.gimmicks.camera == CameraSource::Picture =>
        {
            let picked = form.row(label(row), |ui| ui.button("Choose").clicked());
            if picked && let Some(events) = events.as_deref_mut() {
                events.pick_camera_picture();
            }
        }
        RowId::Palette => {
            if let Some(action) = palette::palette_row(form, s, label(row)) {
                palette::apply(action, s, editor);
            }
        }
        RowId::OwnPalettes => {
            if let Some(action) = palette::own_palettes(form, s, editor, label(row)) {
                palette::apply(action, s, editor);
            }
        }
        RowId::GbcScreen => {
            form.row_with_hint(label(row), GBC_SCREEN_NOTE, |ui| {
                s.dirty |=
                    GenericSelect::from_enum(&mut s.color_correction, "enum_select_gbc_screen")
                        .default_value(ColorCorrection::Screen)
                        .ui(ui)
                        .changed();
            });
        }
        _ => {
            if let Some(index) = channel(row) {
                s.dirty |= form.toggle(label(row), &mut s.channels[index]);
            }
        }
    }
}

fn channel(row: RowId) -> Option<usize> {
    match row {
        RowId::GbPulse1 => Some(0),
        RowId::GbPulse2 => Some(1),
        RowId::GbWave => Some(2),
        RowId::GbNoise => Some(3),
        _ => None,
    }
}
