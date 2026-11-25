use crate::app::ui_state::settings::{self, ScalingMode, Settings};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{SettingsTab, Surface};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::{ResetSlider, percent_slider};
use crate::app::widgets::settings::content::{
    SettingsContent, groups, hidden_rows, label, rows, scope_bar, tagged, view,
};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use egui::{Ui, Widget};

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let view = view(s, SettingsTab::Screen);
    Page::show(ui, |page| {
        scope_bar(page, s, view);
        for group in groups(SettingsTab::Screen, view) {
            page.tree_group(group, None, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        form.tree_row(row, |form| {
                            tagged(form, s, view, row, |form, s| row_ui(form, s, row))
                        });
                    }
                })
            });
        }
        hidden_rows(page, s, SettingsTab::Screen, view, |form, s, _, row| {
            form.tree_row(row, |form| row_ui(form, s, row))
        });
    });
}

fn row_ui(form: &mut Form, s: &mut Settings, row: RowId) {
    let integer = s.scaling == ScalingMode::Integer;
    match row {
        RowId::Scaling => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.scaling, "enum_select_scaling")
                    .default_value(ScalingMode::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::SameSizePixels if !integer => {
            s.dirty |= form.toggle_with_hint(
                label(row),
                settings::SAME_SIZE_PIXELS_NOTE,
                &mut s.same_size_pixels,
            );
        }
        RowId::MatchRefresh => {
            s.dirty |=
                form.toggle_with_hint(label(row), settings::MATCH_SCREEN_NOTE, &mut s.match_screen);
        }
        RowId::LcdGrid => {
            let forced = s.matrix && !integer && !s.same_size_pixels;
            let note = if forced {
                settings::grid_note(s.scaling, Surface::Desktop)
            } else {
                settings::GRID_NOTE.to_owned()
            };
            s.dirty |= form.toggle_with_hint(label(row), &note, &mut s.matrix);
        }
        RowId::GridLines if s.matrix => {
            darkness(
                form,
                row,
                &mut s.matrix_edge_darkness,
                Settings::DEFAULT_MATRIX_EDGE_DARKNESS,
                &mut s.dirty,
            );
        }
        RowId::GridCorners if s.matrix => {
            darkness(
                form,
                row,
                &mut s.matrix_corner_darkness,
                Settings::DEFAULT_MATRIX_CORNER_DARKNESS,
                &mut s.dirty,
            );
        }
        RowId::Ghosting => {
            s.dirty |= form.toggle_with_hint(label(row), settings::GHOSTING_NOTE, &mut s.ghosting);
        }
        RowId::GhostingStrength if s.ghosting => {
            darkness(
                form,
                row,
                &mut s.ghosting_strength,
                Settings::DEFAULT_GHOSTING_STRENGTH,
                &mut s.dirty,
            );
        }
        RowId::ShowHuds => {
            s.dirty |= form.toggle(label(row), &mut s.hud.shown);
        }
        RowId::HudOpacity => {
            form.row_with_hint(label(row), crate::app::hud::editor::CARD_HINT, |ui| {
                let mut card = (s.hud.opacity() * 100.0).round();
                let changed = ui
                    .add(
                        ResetSlider::new(&mut card, 0.0..=100.0)
                            .step_by(5.0)
                            .suffix("%")
                            .default_value((crate::app::hud::DEFAULT_OPACITY * 100.0).round()),
                    )
                    .changed();
                if changed {
                    s.hud.opacity = card / 100.0;
                    s.dirty = true;
                }
            });
        }
        _ => {}
    }
}

fn darkness(form: &mut Form, row: RowId, value: &mut f32, default: f32, dirty: &mut bool) {
    form.row(label(row), |ui| {
        *dirty |= percent_slider(ui, value, 0.0..=1.0, 0.01, default).changed();
    });
}
