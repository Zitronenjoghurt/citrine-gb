use crate::app::background::InBackground;
use crate::app::ui_state::hotkeys::QuickLoadKey;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::content::{
    SettingsContent, groups, hidden_rows, label, rows, scope_bar, tagged, view,
};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::autosave::{AutosaveInterval, DEFAULT_KEEP, MAX_KEEP};
use crate::emulator::fast_forward::{FastForwardKey, FastForwardSpeed};
use crate::emulator::rewind::{REWIND_NOTE, RewindLength};
use egui::{Ui, Widget};

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub(in crate::app::widgets::settings) const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you close it or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";
#[cfg(target_arch = "wasm32")]
pub(in crate::app::widgets::settings) const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you leave the page or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";
#[cfg(target_os = "android")]
pub(in crate::app::widgets::settings) const AUTOSAVE_NOTE: &str = "Citrine also autosaves when you leave the app or switch games, and after a crash. Autosaves show in the Saves tab, apart from your snapshots.";

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let view = view(s, SettingsTab::Playing);
    Page::show(ui, |page| {
        scope_bar(page, s, view);
        for group in groups(SettingsTab::Playing, view) {
            let hint = (group == GroupId::Saves).then_some(AUTOSAVE_NOTE);
            page.tree_group(group, hint, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        tagged(form, s, view, row, |form, s| row_ui(form, s, row));
                    }
                })
            });
        }
        hidden_rows(page, s, SettingsTab::Playing, view, |form, s, _, row| {
            row_ui(form, s, row)
        });
    });
}

fn row_ui(form: &mut Form, s: &mut Settings, row: RowId) {
    match row {
        RowId::FastForward => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(
                    &mut s.fast_forward_speed,
                    "enum_select_fast_forward_speed",
                )
                .default_value(FastForwardSpeed::default())
                .ui(ui)
                .changed();
            });
        }
        RowId::FastForwardKey => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(
                    &mut s.fast_forward_key,
                    "enum_select_fast_forward_key",
                )
                .default_value(FastForwardKey::default())
                .ui(ui)
                .changed();
            });
        }
        RowId::QuickLoad => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.quick_load, "enum_select_quick_load")
                    .default_value(QuickLoadKey::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::InBackground => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(
                    &mut s.background_pause,
                    "enum_select_background_pause",
                )
                .default_value(InBackground::default())
                .ui(ui)
                .changed();
            });
        }
        RowId::Rewind => {
            form.row_with_hint(label(row), REWIND_NOTE, |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.rewind, "enum_select_rewind")
                    .default_value(RewindLength::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::AutosaveEvery => {
            form.row_with_hint(
                label(row),
                "Minutes of play between autosaves. Paused time doesn't count.",
                |ui| {
                    s.dirty |= GenericSelect::from_enum(
                        &mut s.autosave_interval,
                        "enum_select_autosave_interval",
                    )
                    .default_value(AutosaveInterval::default())
                    .ui(ui)
                    .changed();
                },
            );
        }
        RowId::AutosavesKept => {
            form.row_with_hint(
                label(row),
                "When there are more, the oldest autosave goes.",
                |ui| {
                    let response = ResetSlider::new(&mut s.autosave_keep, 1..=MAX_KEEP)
                        .default_value(DEFAULT_KEEP)
                        .ui(ui);
                    if response.drag_stopped() || (response.changed() && !response.dragged()) {
                        s.dirty = true;
                    }
                },
            );
        }
        _ => {}
    }
}
