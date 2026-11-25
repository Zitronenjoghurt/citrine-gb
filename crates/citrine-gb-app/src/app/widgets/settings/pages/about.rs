use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::SettingsTab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::info::InfoTab;
use crate::app::widgets::settings::content::{SettingsContent, groups, label, rows, view};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::Page;
use crate::icons;
use egui::Ui;

pub const ISSUES: &str = "https://github.com/Zitronenjoghurt/citrine-gb/issues";
pub const RELEASES: &str = "https://github.com/Zitronenjoghurt/citrine-gb/releases/latest";
pub const SOURCE: &str = "https://github.com/Zitronenjoghurt/citrine-gb";

#[derive(Clone, Copy)]
pub enum Pick {
    Doc(InfoTab),
    Link(&'static str),
}

pub fn pick(row: RowId) -> Option<(&'static str, Pick)> {
    match row {
        RowId::WhatsNew => Some((icons::SPARKLE, Pick::Doc(InfoTab::Changelog))),
        RowId::FeaturesAndPlans => Some((icons::LIST_CHECKS, Pick::Doc(InfoTab::Features))),
        RowId::ReportProblem => Some((icons::BUG, Pick::Link(ISSUES))),
        RowId::DesktopApp => Some((icons::DESKTOP, Pick::Link(RELEASES))),
        RowId::AndroidApp => Some((icons::DEVICE_MOBILE, Pick::Link(RELEASES))),
        RowId::AllVersions => Some((icons::STACK, Pick::Link(RELEASES))),
        RowId::Licences => Some((icons::SCROLL, Pick::Doc(InfoTab::Licenses))),
        RowId::SourceCode => Some((icons::GITHUB_LOGO, Pick::Link(SOURCE))),
        _ => None,
    }
}

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let view = view(s, SettingsTab::About);
    let mut picked = None;
    Page::show(ui, |page| {
        for group in groups(SettingsTab::About, view) {
            page.tree_group(group, None, |ui| {
                Form::show(ui, group, |form| {
                    for row in rows(group, view) {
                        if let Some((icon, target)) = pick(row) {
                            let text = format!("{icon} {}", label(row));
                            if form.custom(|ui| ui.button(text).clicked()) {
                                picked = Some(target);
                            }
                        } else if row == RowId::DevTools {
                            s.dirty |= form.toggle(label(row), &mut s.dev_mode);
                        }
                    }
                })
            });
        }
        let where_kept = format!(
            "Citrine keeps settings in {}, beside your saves.",
            crate::storage::SettingsStore::default().location()
        );
        page.footer(
            format!("Citrine {}", env!("CARGO_PKG_VERSION")),
            Some(&where_kept),
        );
    });
    match picked {
        Some(Pick::Doc(doc)) => {
            if let Some(info) = content.info {
                info.tab = doc;
            }
            if let Some(events) = content.events {
                events.open_tab(Tab::Info);
            }
        }
        Some(Pick::Link(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
        None => {}
    }
}
