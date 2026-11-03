use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row};
use crate::app::widgets::info::InfoTab;
use crate::icons;
use egui::{RichText, Ui, vec2};

const ISSUES: &str = "https://github.com/Zitronenjoghurt/citrine-gb/issues";
const RELEASES: &str = "https://github.com/Zitronenjoghurt/citrine-gb/releases/latest";
const ICON: f32 = 72.0;

#[derive(Clone, Copy)]
enum Pick {
    Doc(InfoTab),
    Link(&'static str),
}

fn entries() -> Vec<(&'static str, &'static str, Pick)> {
    let mut entries = vec![
        (icons::SPARKLE, "What's new", Pick::Doc(InfoTab::Changelog)),
        (
            icons::LIST_CHECKS,
            "Features and plans",
            Pick::Doc(InfoTab::Features),
        ),
        (icons::BUG, "Report a problem", Pick::Link(ISSUES)),
    ];
    #[cfg(target_arch = "wasm32")]
    entries.push((icons::DESKTOP, "Desktop app", Pick::Link(RELEASES)));
    #[cfg(target_arch = "wasm32")]
    entries.push((icons::DEVICE_MOBILE, "Android app", Pick::Link(RELEASES)));
    #[cfg(not(target_arch = "wasm32"))]
    entries.push((icons::STACK, "All versions", Pick::Link(RELEASES)));
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "openh264-source")))]
    entries.push((icons::SCROLL, "Licences", Pick::Doc(InfoTab::Licenses)));
    entries
}

impl Citrine {
    pub(in crate::app::mobile) fn about_page(&mut self, ui: &mut Ui) {
        let entries = entries();
        let mut picked = None;
        let update = crate::app::widgets::update::texts_now(&self.ui.update);
        let mut update_tapped = false;
        kit::place(ui, "about_page", |ui| {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                if let Some(icon) = crate::app::widgets::app_icon::texture(ui.ctx()) {
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::from_handle(&icon))
                            .fit_to_exact_size(vec2(ICON, ICON))
                            .corner_radius(16),
                    );
                    ui.add_space(8.0);
                }
                ui.label(RichText::new("Citrine").size(22.0).strong());
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(14.0)
                        .weak(),
                );
            });
            ui.add_space(16.0);
            if let Some(update) = &update {
                update_tapped = crate::app::mobile::settings_list::update_row(ui, update);
                ui.add_space(14.0);
            }
            let rows = entries
                .iter()
                .map(|(icon, label, _)| Row::new(label).icon(icon))
                .collect();
            picked = kit::rows(ui, rows).map(|index| entries[index].2);
        });
        if update_tapped {
            let tap = self.ui.update.tap();
            crate::app::widgets::update::follow(ui.ctx(), tap);
        }
        match picked {
            Some(Pick::Doc(doc)) => {
                self.ui.info.tab = doc;
                self.mobile.about_doc = Some(doc);
                self.release_touch();
            }
            Some(Pick::Link(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            None => {}
        }
    }
}
