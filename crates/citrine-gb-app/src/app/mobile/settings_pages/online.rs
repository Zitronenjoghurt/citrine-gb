use super::{Sheet, pick};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::widgets::settings::online::{
    self, CLOUD_HINT, DELAY_HINT, SYNC_HINT, TYPING, TYPING_HINT, cloud_problems, no_storage,
    storage,
};
use egui::{Context, RichText, Ui};
use web_time::Instant;

fn note(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(text).size(13.0).weak()).wrap());
    });
}

impl Citrine {
    pub(super) fn online_page(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        if !self.ui.settings.online.is_joined() {
            self.signed_out_rows(ui, false);
            self.signed_out_sheet(ui);
            return;
        }
        let s = &mut self.ui.settings;
        let client = &mut self.online;
        let form = &mut self.ui.join;
        let now = Instant::now();
        online::track(s, client, form, now);
        kit::list(ui, |list| {
            if s.dev_mode
                && let Some(relay) = s.online.joined_relay()
            {
                list.row(Row::new("Server").value(relay.address.key()).no_chevron());
            }
            if client.is_connected() {
                list.row(Row::new("Connection").value("Connected").no_chevron());
            } else {
                list.custom(|ui| {
                    kit::padded(ui, |ui| {
                        online::connection_line(ui, client, now);
                        online::problems(ui, s, client, form, now);
                    })
                });
            }
        });
        ui.add_space(12.0);
        let mut dirty = false;
        kit::list(ui, |list| {
            dirty |= list
                .switch(SwitchRow::new(TYPING, &mut s.online.typing_indicators).info(TYPING_HINT));
        });
        ui.add_space(12.0);
        kit::list(ui, |list| {
            dirty |= list
                .switch(SwitchRow::new("Cloud saves", &mut s.online.cloud_saves).info(CLOUD_HINT));
            let cloud = s.online.cloud_saves;
            let mut sync = cloud && s.online.sync_settings;
            if list.switch(
                SwitchRow::new("Sync settings", &mut sync)
                    .info(SYNC_HINT)
                    .enabled(cloud),
            ) {
                s.online.sync_settings = sync;
                dirty = true;
            }
        });
        ui.add_space(12.0);
        let mut open = None;
        kit::list(ui, |list| {
            let row = Row::new("Input delay")
                .info(DELAY_HINT)
                .value(s.online.input_delay.to_string());
            if list.row(row).clicked() {
                open = Some(Sheet::InputDelay);
            }
        });
        s.dirty |= dirty;
        for text in no_storage(s, client)
            .into_iter()
            .chain(cloud_problems(s, client))
        {
            note(ui, text);
        }
        if storage::has_storage(client) {
            ui.add_space(12.0);
            storage::storage_list(ui, client, form, now);
        }
        self.open_sheet(open);
    }

    pub(super) fn online_sheet(&mut self, ctx: &Context) -> bool {
        let s = &mut self.ui.settings;
        let (changed, done) = pick(ctx, "Input delay", None, &mut s.online.input_delay);
        s.dirty |= changed;
        done
    }
}
