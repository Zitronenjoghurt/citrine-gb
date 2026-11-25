use super::kit;
use super::nav::Place;
use crate::app::Citrine;
use crate::app::ui_state::ds_screens::ScreenContext;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::search::search;
use crate::app::ui_state::settings_tree::{Section, SettingsTab, Surface};
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::settings::page_list::by_page;
use crate::icons;
use egui::{RichText, Ui};
use strum::IntoEnumIterator;

pub fn sections(dev_mode: bool) -> Vec<(Section, Vec<SettingsTab>)> {
    Section::iter()
        .map(|section| {
            let pages = section
                .pages()
                .iter()
                .copied()
                .filter(|page| page.available(dev_mode))
                .collect::<Vec<_>>();
            (section, pages)
        })
        .filter(|(_, pages)| !pages.is_empty())
        .collect()
}

pub fn value(settings: &Settings, page: SettingsTab, screens: ScreenContext) -> String {
    match page {
        SettingsTab::Screen => settings.touch.scaling.to_string(),
        SettingsTab::Sound if settings.muted => "Muted".to_owned(),
        SettingsTab::Sound => format!("{:.0}%", settings.volume * 100.0),
        SettingsTab::Controls if settings.touch.show_controls => "On-screen".to_owned(),
        SettingsTab::Controls => "Hidden".to_owned(),
        SettingsTab::GameBoy => settings
            .custom_palette
            .clone()
            .unwrap_or_else(|| settings.dmg_theme.to_string()),
        SettingsTab::Gba => settings.gba_screen().to_string(),
        SettingsTab::Ds => settings
            .ds_screens
            .placement(screens)
            .arrangement
            .to_string(),
        SettingsTab::App => settings.ui_theme.to_string(),
        SettingsTab::Online if settings.online.is_joined() => "Signed in".to_owned(),
        SettingsTab::Online => "Signed out".to_owned(),
        _ => String::new(),
    }
}

impl Citrine {
    pub(super) fn settings_place(&mut self, ui: &mut Ui) {
        let focus = std::mem::take(&mut self.ui.settings.focus_search);
        if let Some(page) = self.mobile.nav.settings_page() {
            self.settings_page(ui, page);
            return;
        }
        let dev_mode = self.ui.settings.dev_mode;
        let sections = sections(dev_mode);
        let screens = self.emulator.screen_context.on_phone();
        let mut picked = None;
        let mut found = None;
        let mut query = std::mem::take(&mut self.ui.settings.search);
        let field_id = egui::Id::new("settings_search");
        let show_field = !self.big_screen_on()
            || focus
            || !query.is_empty()
            || ui.ctx().memory(|m| m.has_focus(field_id));
        let update = crate::app::widgets::update::texts_now(&self.ui.update);
        let mut update_tapped = false;
        kit::place(ui, "settings_place", |ui| {
            kit::place_heading(ui, Place::Settings.label());
            let mut page = Page::begin(ui);
            if show_field {
                page.plain(|ui| {
                    let field = kit::field(
                        ui,
                        egui::TextEdit::singleline(&mut query)
                            .id(field_id)
                            .hint_text("Search settings"),
                    );
                    if focus {
                        field.request_focus();
                    }
                });
            }
            if !query.trim().is_empty() {
                found = hits(&mut page, &query, dev_mode);
                page.end();
                return;
            }
            if let Some(update) = &update {
                update_tapped = page.plain(|ui| update_row(ui, update));
            }
            for (section, pages) in &sections {
                let rows = pages
                    .iter()
                    .map(|tab| {
                        kit::Row::new(tab.title()).icon(tab.icon()).value(value(
                            &self.ui.settings,
                            *tab,
                            screens,
                        ))
                    })
                    .collect();
                let tapped =
                    page.section(Heading::new("", section.title()), |ui| kit::rows(ui, rows));
                if let Some(index) = tapped {
                    picked = Some(pages[index]);
                }
            }
            page.plain(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(format!("Citrine {}", env!("CARGO_PKG_VERSION")))
                            .size(13.0)
                            .weak(),
                    );
                })
            });
            page.end();
        });
        if let Some(row) = found {
            self.find_setting(row, true);
            self.ui.settings.search = query;
            return;
        }
        self.ui.settings.search = query;
        if update_tapped {
            let tap = self.ui.update.tap(&self.files.spawner);
            crate::app::widgets::update::follow(ui.ctx(), tap);
        }
        if let Some(page) = picked {
            self.ui.settings.current_tab = page;
            self.mobile.nav.open_settings(page);
            self.release_touch();
        }
    }

    fn settings_page(&mut self, ui: &mut Ui, page: SettingsTab) {
        self.ui.settings.current_tab = page;
        if self.page_title(ui, page.title()) {
            self.page_back();
            return;
        }
        self.phone_settings_page(ui, page);
    }
}

fn hits(page: &mut Page, query: &str, dev_mode: bool) -> Option<RowId> {
    let pages = by_page(search(query, Surface::Phone, dev_mode));
    if pages.is_empty() {
        page.plain(|ui| ui.label(RichText::new("No settings found").weak()));
        return None;
    }
    let mut picked = None;
    for (tab, rows) in &pages {
        let list = rows
            .iter()
            .map(|row| kit::Row::new(row.label(Surface::Phone)))
            .collect();
        let tapped = page.section(Heading::new(tab.icon(), tab.title()), |ui| {
            kit::rows(ui, list)
        });
        if let Some(index) = tapped {
            picked = Some(rows[index]);
        }
    }
    picked
}

pub(super) fn update_row(ui: &mut Ui, update: &crate::app::widgets::update::Texts) -> bool {
    kit::card(ui, |ui| {
        let row = kit::Row::new(&update.title)
            .icon(icons::ARROW_CIRCLE_UP)
            .detail(&update.detail)
            .accent();
        let row = match update.action {
            Some(action) => row.value(action),
            None => row.no_chevron(),
        };
        let tapped = row.show(ui).clicked() && update.action.is_some();
        if let Some(progress) = update.progress {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(14, 0))
                .show(ui, |ui| crate::app::widgets::update::bar(ui, progress));
            ui.add_space(12.0);
        }
        tapped
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::settings::ScalingMode;

    fn listed(dev_mode: bool) -> Vec<SettingsTab> {
        sections(dev_mode)
            .into_iter()
            .flat_map(|(_, pages)| pages)
            .collect()
    }

    #[test]
    fn the_list_follows_the_tree_and_developer_shows_only_in_dev_mode() {
        let plain = listed(false);
        assert_eq!(plain.first(), Some(&SettingsTab::Screen));
        assert_eq!(plain.last(), Some(&SettingsTab::About));
        assert!(!plain.contains(&SettingsTab::Developer));
        assert_eq!(listed(true).last(), Some(&SettingsTab::Developer));
        assert_eq!(plain.contains(&SettingsTab::Ds), cfg!(feature = "nds"));
        for (section, pages) in sections(true) {
            assert!(!section.title().is_empty());
            for page in pages {
                assert_eq!(page.section(), section, "{page:?}");
            }
        }
    }

    #[test]
    fn rows_show_the_value_each_page_holds() {
        let screens = ScreenContext::Portrait;
        let settings = Settings {
            volume: 0.8,
            ..Settings::default()
        };
        assert_eq!(value(&settings, SettingsTab::Sound, screens), "80%");
        let muted = Settings {
            muted: true,
            ..Settings::default()
        };
        assert_eq!(value(&muted, SettingsTab::Sound, screens), "Muted");
        assert_eq!(value(&settings, SettingsTab::Online, screens), "Signed out");
        assert_eq!(value(&settings, SettingsTab::Storage, screens), "");
        assert_eq!(value(&settings, SettingsTab::About, screens), "");
    }

    #[test]
    fn no_row_value_depends_on_the_running_console() {
        let screens = ScreenContext::Portrait;
        let mut settings = Settings::default();
        settings.touch.scaling = ScalingMode::Fit;
        assert_eq!(
            value(&settings, SettingsTab::Screen, screens),
            ScalingMode::Fit.to_string()
        );
        assert_eq!(
            value(&settings, SettingsTab::Gba, screens),
            settings.gba_screen().to_string()
        );
        assert_eq!(
            value(&settings, SettingsTab::GameBoy, screens),
            settings.dmg_theme.to_string()
        );
        assert_eq!(
            value(&settings, SettingsTab::Ds, screens),
            settings.ds_screens.portrait.arrangement.to_string()
        );
    }
}
