use crate::app::ui_state::library::scan::LibraryDepth;
use crate::app::ui_state::settings::{LayoutMode, Settings};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::ui_state::ui_theme::UiTheme;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::percent_slider;
use crate::app::widgets::settings::content::{SettingsContent, groups, label, rows, view};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use egui::{Ui, Widget};

pub(in crate::app::widgets::settings) const LAYOUT_HINT: &str =
    "Automatic uses the touch layout on small screens.";
pub(crate) const HIDE_POINTER_HINT: &str = "Hides the mouse pointer while it is over the game.";
pub(crate) const DISCORD_NOTE: &str =
    "Your Discord profile shows what you play while Discord runs on this computer.";
pub(crate) const DISCORD_DETAILS_HINT: &str = "In Pokémon games: badges, Pokédex and play time. Hovering over the image shows your first Pokémon and money.";
pub(crate) const DISCORD_SCREENSHOT_HINT: &str =
    "Your Discord profile shows your game screen instead of the Citrine icon.";
pub(crate) const DISCORD_PICTURES_OFF: &str = "Off on this server.";
pub(crate) const COVERS_NOTE: &str = "Citrine downloads each game's box art from libretro's thumbnail collection on GitHub the first time you play it, and keeps it on this device.";
pub(crate) const PROGRESS_NOTE: &str = "Shows under each game in the library how many of its RetroAchievements you've earned, while you're signed in to RetroAchievements.";
pub(crate) const UPDATES_NOTE: &str = "Citrine asks GitHub for the newest release when it starts and once a day. Updates install only when you choose to.";

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    let discord = content.discord;
    let view = view(s, SettingsTab::App);
    Page::show(ui, |page| {
        for group in groups(SettingsTab::App, view) {
            let hint = (group == GroupId::Discord).then_some(DISCORD_NOTE);
            page.tree_group(group, hint, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        form.tree_row(row, |form| {
                            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
                            if group == GroupId::Discord {
                                let pictures_off = discord.as_ref().is_some_and(|(_, _, off)| *off);
                                super::super::discord::row(form, s, row, pictures_off);
                                return;
                            }
                            row_ui(form, s, row);
                        });
                    }
                    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
                    if group == GroupId::Discord
                        && let Some((now, screen, pictures_off)) = &discord
                    {
                        super::super::discord::preview(
                            form,
                            s,
                            now.as_ref(),
                            *screen,
                            *pictures_off,
                        );
                    }
                })
            });
        }
    });
}

fn row_ui(form: &mut Form, s: &mut Settings, row: RowId) {
    match row {
        RowId::Theme => {
            form.row(label(row), |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.ui_theme, "enum_select_ui_theme")
                    .default_value(UiTheme::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::AppLayout => {
            form.row_with_hint(label(row), LAYOUT_HINT, |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.layout, "enum_select_layout")
                    .default_value(LayoutMode::default())
                    .ui(ui)
                    .changed();
            });
            if s.layout == LayoutMode::Desktop && LayoutMode::too_narrow_for_desktop(form.ctx()) {
                form.hint(
                    "This window is too narrow for the desktop layout, so it uses the touch layout.",
                );
            }
        }
        RowId::UiScale => {
            form.row(label(row), |ui| {
                let response = percent_slider(
                    ui,
                    &mut s.ui_scale,
                    0.5..=5.0,
                    0.1,
                    Settings::DEFAULT_UI_SCALE,
                );
                s.note_scale_drag(&response);
            });
        }
        RowId::Fullscreen => s.dirty |= form.toggle(label(row), &mut s.fullscreen),
        RowId::HidePointer => {
            s.dirty |= form.toggle_with_hint(label(row), HIDE_POINTER_HINT, &mut s.hide_pointer)
        }
        RowId::Subfolders => {
            form.row(label(row), |ui| {
                s.dirty |=
                    GenericSelect::from_enum(&mut s.library_depth, "enum_select_library_depth")
                        .default_value(LibraryDepth::default())
                        .ui(ui)
                        .changed();
            });
        }
        RowId::Covers => {
            s.dirty |= form.toggle_with_hint(label(row), COVERS_NOTE, &mut s.covers);
        }
        #[cfg(not(target_arch = "wasm32"))]
        RowId::AchievementProgress => {
            s.dirty |=
                form.toggle_with_hint(label(row), PROGRESS_NOTE, &mut s.achievements.in_library);
        }
        #[cfg(not(target_arch = "wasm32"))]
        RowId::CheckUpdates => {
            s.dirty |= form.toggle_with_hint(label(row), UPDATES_NOTE, &mut s.check_updates);
        }
        _ => {}
    }
}
