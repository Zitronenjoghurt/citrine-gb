use crate::app::ui_state::settings::{GBA_SCREEN_NOTE, GbaScreen, Settings};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::content::{
    SettingsContent, groups, hidden_rows, label, rows, scope_bar, tagged, view,
};
#[cfg(feature = "gba")]
use crate::app::widgets::settings::file_row::file_row;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::emulator::gimmicks::LightSource;
use crate::emulator::gimmicks::settings::DEFAULT_LIGHT;
use citrine_gb::gb::ppu::types::color::ColorCorrection;
use egui::{Ui, Widget};

pub(in crate::app::widgets::settings) const GBA_CHANNELS_HINT: &str = "The Game Boy Advance's six sound channels. The first four are the Game Boy's. Direct sound A and B play recorded sound, often the music and voices.";

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let light_sensor = content.device.light_sensor;
    #[cfg(feature = "gba")]
    let bios = content.gba_bios;
    #[cfg(feature = "gba")]
    let mut events = content.events;
    #[cfg(feature = "nds")]
    let mut spare = crate::app::widgets::settings::storage::StorageUi::default();
    #[cfg(feature = "nds")]
    let compiled = content.storage.unwrap_or(&mut spare);
    let view = view(s, SettingsTab::Gba);
    #[cfg_attr(not(feature = "gba"), allow(unused_mut))]
    let mut draw = |form: &mut Form, s: &mut Settings, row: RowId| match row {
        #[cfg(feature = "gba")]
        RowId::GbaBios => file_row(
            form,
            crate::app::system_files::SystemFile::GbaBios,
            bios,
            &mut events,
        ),
        #[cfg(feature = "nds")]
        RowId::GbaJit => {
            s.dirty |= form.toggle_with_hint(
                label(row),
                crate::app::ui_state::settings::GBA_JIT_HINT,
                s.gba_jit_mut(),
            );
        }
        #[cfg(feature = "nds")]
        RowId::GbaCompiledCode => {
            let kind = crate::storage::kinds::Kind::CompiledCode;
            if let Some(clear) =
                crate::app::widgets::settings::storage::settings_row(form, compiled, kind)
            {
                compiled.request = Some(clear);
            }
        }
        _ => row_ui(form, s, light_sensor, row),
    };
    Page::show(ui, |page| {
        scope_bar(page, s, view);
        for group in groups(SettingsTab::Gba, view) {
            let hint = (group == GroupId::GbaChannels).then_some(GBA_CHANNELS_HINT);
            page.tree_group(group, hint, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        tagged(form, s, view, row, |form, s| draw(form, s, row));
                    }
                })
            });
        }
        hidden_rows(page, s, SettingsTab::Gba, view, |form, s, _, row| {
            draw(form, s, row)
        });
    });
}

fn row_ui(form: &mut Form, s: &mut Settings, light_sensor: bool, row: RowId) {
    let sensor_on = light_sensor && s.gimmicks.light_source == LightSource::Sensor;
    match row {
        RowId::GbaScreen => {
            form.row_with_hint(label(row), GBA_SCREEN_NOTE, |ui| {
                let mut screen = s.gba_screen();
                if GenericSelect::new(&mut screen, GbaScreen::ALL, "gba_screen")
                    .default_value(GbaScreen(ColorCorrection::Screen))
                    .ui(ui)
                    .changed()
                {
                    s.gba_color_correction = Some(screen.0);
                    s.dirty = true;
                }
            });
        }
        RowId::LightSensor if light_sensor => {
            let mut sensor = sensor_on;
            if form.toggle(label(row), &mut sensor) {
                s.gimmicks.light_source = if sensor {
                    LightSource::Sensor
                } else {
                    LightSource::Slider
                };
                s.dirty = true;
            }
        }
        RowId::Sunlight if !sensor_on => {
            form.row(label(row), |ui| {
                let mut percent = (s.gimmicks.light * 100.0).round() as u32;
                let response = ResetSlider::new(&mut percent, 0..=100)
                    .default_value((DEFAULT_LIGHT * 100.0) as u32)
                    .suffix("%")
                    .ui(ui);
                if response.changed() {
                    s.gimmicks.light = percent as f32 / 100.0;
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    s.dirty = true;
                }
            });
        }
        _ => {
            if let Some(index) = channel(row) {
                s.dirty |= form.toggle(label(row), &mut s.gba_channels[index]);
            }
        }
    }
}

fn channel(row: RowId) -> Option<usize> {
    match row {
        RowId::GbaPulse1 => Some(0),
        RowId::GbaPulse2 => Some(1),
        RowId::GbaWave => Some(2),
        RowId::GbaNoise => Some(3),
        RowId::DirectSoundA => Some(4),
        RowId::DirectSoundB => Some(5),
        _ => None,
    }
}
