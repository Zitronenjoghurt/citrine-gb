use super::kit;
use super::nav::Place;
use crate::app::Citrine;
use crate::app::tabs::Tab;
use crate::app::ui_state::settings::{GbaScreen, Settings, SettingsTab};
use crate::app::widgets::settings::page::Page;
use crate::icons;
use citrine_gb::console::ConsoleKind;
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

pub fn value(settings: &Settings, page: SettingsTab, console: ConsoleKind) -> String {
    match page {
        SettingsTab::Style if console == ConsoleKind::Gba => {
            GbaScreen(settings.color_correction).to_string()
        }
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
        let console = self.emulator.kind();
        let mut picked = None;
        let update = crate::app::widgets::update::texts_now(&self.ui.update);
        let mut update_tapped = false;
        kit::place(ui, "settings_place", |ui| {
            kit::place_title(ui, Place::Settings.label(), |_| {});
            let mut page = Page::begin(ui);
            if let Some(update) = &update {
                update_tapped = page.plain(|ui| update_row(ui, update));
            }
            let rows = entries
                .iter()
                .map(|entry| match entry {
                    Entry::Page(page) => kit::Row::new(page.title_for(console))
                        .icon(page.icon())
                        .value(value(&self.ui.settings, *page, console)),
                    Entry::About => kit::Row::new("About").icon(icons::INFO),
                })
                .collect();
            picked = page.plain(|ui| kit::rows(ui, rows).map(|index| entries[index]));
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
        if update_tapped {
            let tap = self.ui.update.tap(&self.files.spawner);
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
        if self.page_title(ui, page.title_for(self.emulator.kind())) {
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
mod tests {
    use super::*;

    #[test]
    fn about_follows_your_data_and_developer_shows_only_in_dev_mode() {
        let plain = entries(false);
        assert_eq!(plain.last(), Some(&Entry::About));
        assert!(!plain.contains(&Entry::Page(SettingsTab::Developer)));
        assert_eq!(
            entries(true).last(),
            Some(&Entry::Page(SettingsTab::Developer))
        );
    }

    #[test]
    fn rows_show_the_value_each_page_holds() {
        let settings = Settings {
            volume: 0.8,
            ..Settings::default()
        };
        assert_eq!(value(&settings, SettingsTab::Sound, ConsoleKind::Gb), "80%");
        let muted = Settings {
            muted: true,
            ..Settings::default()
        };
        assert_eq!(value(&muted, SettingsTab::Sound, ConsoleKind::Gb), "Muted");
        assert_eq!(
            value(&settings, SettingsTab::Online, ConsoleKind::Gb),
            "Signed out"
        );
        assert_eq!(value(&settings, SettingsTab::YourData, ConsoleKind::Gb), "");
    }

    #[test]
    fn a_gba_game_names_the_style_page_after_its_console_and_shows_the_screen() {
        let settings = Settings::default();
        assert_eq!(SettingsTab::Style.title_for(ConsoleKind::Gb), "GB style");
        assert_eq!(SettingsTab::Style.title_for(ConsoleKind::Gba), "GBA style");
        assert_eq!(SettingsTab::Style.title_for(ConsoleKind::Nds), "DS style");
        assert_eq!(SettingsTab::Sound.title_for(ConsoleKind::Gba), "Sound");
        assert_eq!(
            value(&settings, SettingsTab::Style, ConsoleKind::Gba),
            GbaScreen(settings.color_correction).to_string()
        );
        assert_eq!(
            value(&settings, SettingsTab::Style, ConsoleKind::Gb),
            settings.dmg_theme.to_string()
        );
    }
}
