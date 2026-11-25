use crate::app::widgets::settings::content::SettingsContent;
use egui::Ui;

#[cfg(feature = "nds")]
pub fn show(content: SettingsContent, ui: &mut Ui) {
    use crate::app::ui_state::settings;
    use crate::app::ui_state::settings::Settings;
    use crate::app::ui_state::settings_tree::rows::RowId;
    use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
    use crate::app::widgets::settings::content::{
        groups, hidden_rows, label, rows, scope_bar, tagged, view,
    };
    use crate::app::widgets::settings::form::Form;
    use crate::app::widgets::settings::page::Page;
    use crate::app::widgets::settings::{ds, ds_screens, storage};
    use crate::storage::kinds::Kind;

    let s = content.settings;
    let state = content.ds;
    let mut events = content.events;
    let mut spare = storage::StorageUi::default();
    let compiled = content.storage.unwrap_or(&mut spare);
    let view = view(s, SettingsTab::Ds);
    let mut draw = |form: &mut Form, s: &mut Settings, group: GroupId, row: RowId| match group {
        GroupId::DsScreens => ds_screens::screens_row(form, s, row),
        #[cfg(not(target_arch = "wasm32"))]
        GroupId::Ds3d => ds_screens::three_d_row(form, s, row),
        GroupId::DsUser => ds::user_row(form, s, state, row),
        GroupId::DsPerformance => match row {
            RowId::Jit => {
                s.dirty |= form.toggle_with_hint(label(row), settings::DS_JIT_HINT, &mut s.ds_jit);
            }
            RowId::CompiledCode => {
                let kind = Kind::CompiledCode;
                if let Some(clear) = storage::settings_row(form, compiled, kind) {
                    compiled.request = Some(clear);
                }
            }
            _ => {}
        },
        GroupId::DsSystem => ds::system_row(form, s, state, &mut events, row),
        GroupId::Walker => ds::walker_row(form, s, row),
        GroupId::DsMicrophone => mic_row(form, s, state.mic, row),
        _ => {}
    };
    Page::show(ui, |page| {
        scope_bar(page, s, view);
        for group in groups(SettingsTab::Ds, view) {
            let hint = (group == GroupId::DsUser).then_some(ds::HEADING_HINT);
            page.tree_group(group, hint, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        tagged(form, s, view, row, |form, s| draw(form, s, group, row));
                    }
                })
            });
        }
        hidden_rows(page, s, SettingsTab::Ds, view, &mut draw);
    });
}

#[cfg(not(feature = "nds"))]
pub fn show(_content: SettingsContent, _ui: &mut Ui) {}

#[cfg(feature = "nds")]
fn mic_row(
    form: &mut crate::app::widgets::settings::form::Form,
    s: &mut crate::app::ui_state::settings::Settings,
    test: Option<crate::audio::mic::MicTest>,
    row: crate::app::ui_state::settings_tree::rows::RowId,
) {
    use crate::app::ui_state::settings_tree::rows::RowId;
    use crate::app::widgets::mic_level::MicLevel;
    use crate::app::widgets::reset_slider::with_reset;
    use crate::app::widgets::settings::content::label;
    use crate::audio::mic::{MICROPHONE_HINT, MicTest, Microphone, PUSH_TO_TALK_HINT};
    match row {
        RowId::Microphone => {
            form.row_with_hint(label(row), MICROPHONE_HINT, |ui| {
                let off = Microphone::default();
                if with_reset(ui, s.microphone != off, off.label(), |ui| mic_picker(ui, s)) {
                    s.microphone = off;
                    s.dirty = true;
                }
            });
            s.mic_test_shown = true;
            if s.microphone != Microphone::Off {
                form.under_field(|ui| ui.add(MicLevel(test.unwrap_or(MicTest::Level(0.0)))));
            }
        }
        RowId::PushToTalk if s.microphone != Microphone::Off => {
            s.dirty |= form.toggle_with_hint(label(row), PUSH_TO_TALK_HINT, &mut s.push_to_talk);
        }
        _ => {}
    }
}

#[cfg(feature = "nds")]
fn mic_picker(ui: &mut Ui, s: &mut crate::app::ui_state::settings::Settings) {
    use crate::audio::mic::{Microphone, input_devices};
    let spacing = ui.spacing();
    let chrome = 2.0 * spacing.button_padding.x + spacing.icon_width + spacing.icon_spacing;
    let default = egui::WidgetText::from(Microphone::Default.label())
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

    egui::ComboBox::from_id_salt("mic_picker")
        .width(ui.available_width())
        .truncate()
        .selected_text(s.microphone.label())
        .show_ui(ui, |ui| {
            for choice in Microphone::choices(&input_devices()) {
                let chosen = s.microphone == choice;
                if ui.selectable_label(chosen, choice.label()).clicked() && !chosen {
                    s.microphone = choice;
                    s.dirty = true;
                }
            }
        });
}
