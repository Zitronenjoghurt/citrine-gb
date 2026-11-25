use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::settings::content::{SettingsContent, groups, label, rows, view};
use crate::app::widgets::settings::controls;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use egui::Ui;

const TOUCH_HINT: &str = "The touch layout shows these on phones and small windows. With menus hidden, Auto hides them when you press a controller button or a key, and shows them again when you tap beside the game.";

const BIG_SCREEN_HINT: &str = "With Confirm button on Automatic, Nintendo controllers confirm with the right button and others with the bottom one.";

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let capture = content.capture;
    let view = view(s, SettingsTab::Controls);
    let mut test = false;
    Page::show(ui, |page| {
        controls::prepare(page, s, capture);
        for group in groups(SettingsTab::Controls, view) {
            let hint = match group {
                GroupId::Touch => Some(TOUCH_HINT),
                GroupId::BigScreen => Some(BIG_SCREEN_HINT),
                _ => None,
            };
            page.tree_group(group, hint, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        form.tree_row(row, |form| match row {
                            RowId::GameButtons => {
                                controls::game_buttons(form, s, capture, Some(label(row)))
                            }
                            RowId::Shortcuts => {
                                controls::shortcuts(form, s, capture, Some(label(row)))
                            }
                            RowId::SplitKeys => {
                                controls::split_keys(form, s, capture, Some(label(row)))
                            }
                            RowId::ControllerTest => test |= controls::controller_test(form),
                            RowId::TiltKeys => {
                                controls::tilt_keys(form, s, capture, Some(label(row)))
                            }
                            _ if group == GroupId::Controller => {
                                controls::controller_row(form, s, row)
                            }
                            _ if group == GroupId::Touch => controls::touch_row(form, s, row),
                            _ if group == GroupId::BigScreen => {
                                controls::big_screen_row(form, s, row)
                            }
                            _ => {}
                        });
                    }
                })
            });
        }
    });
    if test && let Some(events) = content.events {
        events.open_tab(Tab::ControllerDebug);
    }
}
