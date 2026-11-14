use super::{Sheet, pick};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::widgets::settings::online::{
    self, ALLOW, ANDROID_SETTINGS, CLOUD_HINT, DELAY_HINT, FRIEND_REQUESTS, INVITES, MESSAGES,
    SYNC_HINT, TYPING, TYPING_HINT, cloud_problems, no_storage, storage,
};
use crate::app::widgets::settings::page::{Heading, Page};
use crate::icons;
use egui::{Context, Ui};
use web_time::Instant;

impl Citrine {
    pub(super) fn online_page(&mut self, ui: &mut Ui) {
        let mut page = Page::begin(ui);
        if !self.ui.settings.online.is_joined() {
            page.plain(|ui| self.signed_out_rows(ui, false));
            page.end();
            self.signed_out_sheet(ui);
            return;
        }
        let s = &mut self.ui.settings;
        let client = &mut self.online;
        let form = &mut self.ui.join;
        let now = Instant::now();
        online::track(s, client, form, now);
        if s.dev_mode
            && let Some(relay) = s.online.joined_relay()
        {
            page.section(Heading::new(icons::IDENTIFICATION_CARD, "Account"), |ui| {
                kit::list(ui, |list| {
                    list.row(Row::new("Server").value(relay.address.key()).no_chevron());
                })
            });
        }
        let mut dirty = false;
        let mut open = None;
        page.section(Heading::new(icons::USERS, "Friends"), |ui| {
            kit::list(ui, |list| {
                dirty |= list.switch(
                    SwitchRow::new(TYPING, &mut s.online.typing_indicators).info(TYPING_HINT),
                );
            })
        });
        if crate::platform::push::available() {
            page.section(Heading::new(icons::BELL, "Notifications"), |ui| {
                kit::list(ui, |list| {
                    dirty |= notification_rows(list, &mut s.online);
                })
            });
        }
        page.section(
            Heading::new(icons::GAME_CONTROLLER, "Play together"),
            |ui| {
                kit::list(ui, |list| {
                    let row = Row::new("Input delay")
                        .info(DELAY_HINT)
                        .value(s.online.input_delay.to_string());
                    if list.row(row).clicked() {
                        open = Some(Sheet::InputDelay);
                    }
                })
            },
        );
        page.section(Heading::new(icons::CLOUD, "Cloud"), |ui| {
            kit::list(ui, |list| {
                dirty |= list.switch(
                    SwitchRow::new("Cloud saves", &mut s.online.cloud_saves).info(CLOUD_HINT),
                );
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
            })
        });
        s.dirty |= dirty;
        for text in no_storage(s, client)
            .into_iter()
            .chain(cloud_problems(s, client))
        {
            page.note(text);
        }
        if storage::has_storage(client) {
            page.section(Heading::new(icons::HARD_DRIVES, "Storage"), |ui| {
                storage::storage_list(ui, client, form, now)
            });
        }
        page.end();
        self.open_sheet(open);
    }

    pub(super) fn online_sheet(&mut self, ctx: &Context) -> bool {
        let s = &mut self.ui.settings;
        let (changed, done) = pick(ctx, "Input delay", None, &mut s.online.input_delay);
        s.dirty |= changed;
        done
    }
}

fn notification_rows(list: &mut kit::List<'_>, s: &mut crate::online::OnlineSettings) -> bool {
    let allowed = crate::platform::push::allowed();
    if !allowed && list.row(Row::new(ALLOW).value("Off")).clicked() {
        crate::platform::push::ask();
    }
    let mut dirty = false;
    for (label, on) in [
        (FRIEND_REQUESTS, &mut s.notify_friend_requests),
        (INVITES, &mut s.notify_invites),
        (MESSAGES, &mut s.notify_chats),
    ] {
        if allowed {
            dirty |= list.switch(SwitchRow::new(label, on));
        } else {
            list.switch(SwitchRow::new(label, &mut false).enabled(false));
        }
    }
    if allowed && list.row(Row::new(ANDROID_SETTINGS)).clicked() {
        crate::platform::push::open_settings();
    }
    dirty
}
