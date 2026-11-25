use super::label;
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::tabs::Tab;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, Surface};
use crate::app::widgets::settings::page::Page;
use crate::app::widgets::settings::pages::about::{Pick, pick};
use egui::{RichText, Ui, vec2};

const ICON: f32 = 72.0;

impl Citrine {
    pub(in crate::app::mobile) fn about_page(&mut self, ui: &mut Ui) {
        let dev_mode = self.ui.settings.dev_mode;
        let picks: Vec<(RowId, &'static str, Pick)> = GroupId::About
            .rows_on(Surface::Phone, dev_mode)
            .filter_map(|row| pick(row).map(|(icon, target)| (row, icon, target)))
            .collect();
        let mut picked = None;
        let update = crate::app::widgets::update::texts_now(&self.ui.update);
        let mut update_tapped = false;
        let s = &mut self.ui.settings;
        kit::place(ui, "about_page", |ui| {
            let mut page = Page::begin(ui);
            page.plain(|ui| {
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
            });
            if let Some(update) = &update {
                update_tapped =
                    page.plain(|ui| crate::app::mobile::settings_list::update_row(ui, update));
            }
            page.plain(|ui| {
                kit::list(ui, |list| {
                    for (row, icon, target) in &picks {
                        if list.row(Row::new(label(*row)).icon(icon)).clicked() {
                            picked = Some(*target);
                        }
                    }
                    if RowId::DevTools.shown_on(Surface::Phone, dev_mode) {
                        s.dirty |=
                            list.switch(SwitchRow::new(label(RowId::DevTools), &mut s.dev_mode));
                    }
                })
            });
            page.end();
        });
        if update_tapped {
            let tap = self.ui.update.tap(&self.files.spawner);
            crate::app::widgets::update::follow(ui.ctx(), tap);
        }
        match picked {
            Some(Pick::Doc(doc)) => {
                self.ui.info.tab = doc;
                self.mobile.about_doc = Some(doc);
                self.mobile.open(Tab::Info);
                self.release_touch();
            }
            Some(Pick::Link(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            None => {}
        }
    }
}
