use super::kit;
use super::nav::Place;
use crate::app::Citrine;
use crate::app::tabs::Tab;
use crate::app::ui_state::settings::{Settings, SettingsTab};
use crate::icons;
use egui::{RichText, Ui};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Page(SettingsTab),
    About,
}

pub fn entries(dev_mode: bool) -> Vec<Entry> {
    let mut entries: Vec<Entry> = [
        SettingsTab::Controls,
        SettingsTab::Style,
        SettingsTab::Sound,
        SettingsTab::General,
        SettingsTab::Online,
        SettingsTab::YourData,
    ]
    .into_iter()
    .map(Entry::Page)
    .collect();
    entries.push(Entry::About);
    if dev_mode {
        entries.push(Entry::Page(SettingsTab::Developer));
    }
    entries
}

pub fn value(settings: &Settings, page: SettingsTab) -> String {
    match page {
        SettingsTab::General => settings.ui_theme.to_string(),
        SettingsTab::Sound if settings.muted => "Muted".to_owned(),
        SettingsTab::Sound => format!("{:.0}%", settings.volume * 100.0),
        SettingsTab::Controls if settings.touch.show_controls => "On-screen".to_owned(),
        SettingsTab::Controls => "Hidden".to_owned(),
        SettingsTab::Style => settings
            .custom_palette
            .clone()
            .unwrap_or_else(|| settings.dmg_theme.to_string()),
        SettingsTab::Online if settings.online.is_joined() => "Signed in".to_owned(),
        SettingsTab::Online => "Signed out".to_owned(),
        SettingsTab::YourData | SettingsTab::Developer => String::new(),
    }
}

impl Citrine {
    pub(super) fn settings_place(&mut self, ui: &mut Ui) {
        if let Some(page) = self.mobile.nav.settings_page() {
            self.settings_page(ui, page);
            return;
        }
        let entries = entries(self.ui.settings.dev_mode);
        let mut picked = None;
        let update = crate::app::widgets::update::texts_now(&self.ui.update);
        let mut update_tapped = false;
        kit::place(ui, "settings_place", |ui| {
            kit::place_title(ui, Place::Settings.label(), |_| {});
            if let Some(update) = &update {
                update_tapped = update_row(ui, update);
                ui.add_space(14.0);
            }
            let rows = entries
                .iter()
                .map(|entry| match entry {
                    Entry::Page(page) => kit::Row::new(page.title())
                        .icon(page.icon())
                        .value(value(&self.ui.settings, *page)),
                    Entry::About => kit::Row::new("About").icon(icons::INFO),
                })
                .collect();
            picked = kit::rows(ui, rows).map(|index| entries[index]);
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(format!("Citrine {}", env!("CARGO_PKG_VERSION")))
                        .size(13.0)
                        .weak(),
                );
            });
        });
        if update_tapped {
            let tap = self.ui.update.tap();
            crate::app::widgets::update::follow(ui.ctx(), tap);
        }
        match picked {
            Some(Entry::Page(page)) => {
                self.ui.settings.current_tab = page;
                self.mobile.nav.open_settings(page);
                self.release_touch();
            }
            Some(Entry::About) => {
                self.mobile.about_doc = None;
                self.mobile.open(Tab::Info);
                self.release_touch();
            }
            None => {}
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
mod tests;
