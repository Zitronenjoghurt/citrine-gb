use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::settings::content::{SettingsContent, groups, label, rows, view};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use egui::Ui;

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let view = view(s, SettingsTab::Developer);
    let mut files = false;
    Page::show(ui, |page| {
        for group in groups(SettingsTab::Developer, view) {
            page.tree_group(group, None, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        match row {
                            RowId::RandomRam => {
                                s.dirty |= form.toggle(label(row), &mut s.randomized_ram);
                            }
                            RowId::DsFiles => {
                                files |= form.row(label(row), |ui| ui.button("Open").clicked());
                            }
                            _ => {}
                        }
                    }
                })
            });
        }
    });
    if files && let Some(events) = content.events {
        events.open_tab(Tab::DsFiles);
    }
}
