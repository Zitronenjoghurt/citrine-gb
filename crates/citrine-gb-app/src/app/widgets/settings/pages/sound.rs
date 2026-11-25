use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::{ResetSlider, percent_slider};
use crate::app::widgets::settings::content::{
    SettingsContent, groups, hidden_rows, label, rows, scope_bar, tagged, view,
};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::audio::pacer::{FastForwardSound, SlowSound};
use crate::audio::{DEFAULT_LATENCY_MS, MAX_LATENCY_MS, MIN_LATENCY_MS};
use egui::{Ui, Widget};

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let audio = content.audio;
    let view = view(s, SettingsTab::Sound);
    Page::show(ui, |page| {
        scope_bar(page, s, view);
        for group in groups(SettingsTab::Sound, view) {
            page.tree_group(group, None, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        tagged(form, s, view, row, |form, s| row_ui(form, s, row));
                    }
                })
            });
        }
        hidden_rows(page, s, SettingsTab::Sound, view, |form, s, _, row| {
            row_ui(form, s, row)
        });
        if let Some(audio) = audio {
            if audio.is_running() {
                page.note(format!(
                    "Playing at {} Hz on {}.",
                    audio.get_sample_rate(),
                    audio.device_name().unwrap_or("the default device")
                ));
            } else {
                page.note("Sound starts when you load a game or tap the screen.");
            }
        }
    });
}

fn row_ui(form: &mut Form, s: &mut Settings, row: RowId) {
    match row {
        RowId::Volume => {
            form.row(label(row), |ui| {
                s.dirty |=
                    percent_slider(ui, &mut s.volume, 0.0..=1.0, 0.01, Settings::DEFAULT_VOLUME)
                        .changed();
            });
        }
        RowId::Mute => {
            s.dirty |= form.toggle(label(row), &mut s.muted);
        }
        #[cfg(not(target_arch = "wasm32"))]
        RowId::OutputDevice => {
            form.row_with_hint(
                label(row),
                "Changing this reopens the sound device.",
                |ui| device_picker(ui, s),
            );
        }
        RowId::Latency => {
            form.row_with_hint(
                label(row),
                "Lower reacts faster. Higher crackles less.",
                |ui| {
                    let response =
                        ResetSlider::new(&mut s.audio_queue_ms, MIN_LATENCY_MS..=MAX_LATENCY_MS)
                            .default_value(DEFAULT_LATENCY_MS)
                            .suffix(" ms")
                            .ui(ui);
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        s.dirty = true;
                    }
                },
            );
        }
        RowId::SlowSound => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.slow_sound, "enum_select_slow_sound")
                    .default_value(SlowSound::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::FastForwardSound => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(
                    &mut s.fast_forward_sound,
                    "enum_select_fast_forward_sound",
                )
                .default_value(FastForwardSound::default())
                .ui(ui)
                .changed();
            });
        }
        _ => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn device_picker(ui: &mut Ui, s: &mut Settings) {
    use crate::audio::Audio;
    let selected = s
        .audio_device
        .clone()
        .unwrap_or_else(|| crate::audio::SAME_AS_SYSTEM.to_string());
    let spacing = ui.spacing();
    let chrome = 2.0 * spacing.button_padding.x + spacing.icon_width + spacing.icon_spacing;
    let default = egui::WidgetText::from(crate::audio::SAME_AS_SYSTEM)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x;
    let reset = crate::app::widgets::reset_slider::reset_width(ui)
        + crate::app::widgets::reset_slider::RESET_GAP;
    crate::app::widgets::settings::form::need(ui, default + chrome + reset);

    egui::ComboBox::from_id_salt("audio_device_picker")
        .width(ui.available_width() - reset)
        .truncate()
        .selected_text(selected)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(s.audio_device.is_none(), crate::audio::SAME_AS_SYSTEM)
                .clicked()
            {
                s.audio_device = None;
                s.dirty = true;
            }
            for name in Audio::output_devices() {
                let chosen = s.audio_device.as_deref() == Some(name.as_str());
                if ui.selectable_label(chosen, &name).clicked() {
                    s.audio_device = Some(name);
                    s.dirty = true;
                }
            }
        });
}
