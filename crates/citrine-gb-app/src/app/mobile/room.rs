use super::nav::Place;
use super::sheet;
use crate::app::Citrine;
use crate::app::tabs::{Tab, playing, room_chat};
use egui::{Context, Id, Margin, Ui};

impl Citrine {
    pub(super) fn room_row(&mut self, ui: &mut Ui) {
        let sound = playing::FriendSound::of(&self.ui.settings.online);
        let Some(facts) = playing::room_facts(&self.play, &self.online, &self.ui.playing) else {
            return;
        };
        let Some(mine) = playing::my_menu(&self.play, &self.online, sound) else {
            return;
        };
        let action = egui::Frame::new()
            .inner_margin(Margin::symmetric(6, 0))
            .show(ui, |ui| {
                let row = playing::member_row(ui, &facts, &mine, true);
                playing::request_card(ui, &facts, true).or(row)
            })
            .inner;
        if let Some(action) = action {
            self.settle_phone_room(action);
        }
        self.room_sheets(ui.ctx(), &facts);
    }

    fn settle_phone_room(&mut self, action: playing::RoomAction) {
        let go = playing::settle_room(
            action,
            &mut self.play,
            &mut self.online,
            &self.emulator,
            &mut self.ui.playing,
        );
        let Some(go) = go else {
            return;
        };
        self.release_touch();
        match go {
            playing::RoomGo::Chat => self.ui.playing.room_sheet = Some(playing::RoomSheet::Chat),
            playing::RoomGo::Add => self.mobile.nav.go(Place::Friends),
            playing::RoomGo::Message(who) => {
                self.ui.friends.chat.open(who);
                self.mobile.nav.go(Place::Friends);
            }
            playing::RoomGo::Profile(who) => {
                self.ui.friends.show_profile(who);
                self.mobile.open(Tab::Friends);
            }
        }
    }

    fn room_sheets(&mut self, ctx: &Context, facts: &playing::RoomFacts) {
        match self.ui.playing.room_sheet {
            Some(playing::RoomSheet::Member(who)) => {
                let Some(person) = facts.person(who) else {
                    self.ui.playing.room_sheet = None;
                    return;
                };
                let shown = sheet::show(ctx, Id::new(("room_member", who)), "", |ui| {
                    sheet::inset(ui, |ui| playing::header_ui(ui, person, 40.0, true));
                    ui.add_space(8.0);
                    playing::member_menu_ui(ui, facts, who, true)
                });
                if shown.closed {
                    self.ui.playing.room_sheet = None;
                }
                if let Some(picked) = shown.inner {
                    self.settle_phone_room(playing::RoomAction::Member(who, picked));
                }
            }
            Some(playing::RoomSheet::Chat) => {
                let lines = room_chat::lines(&self.play, &self.online);
                self.ui.playing.room_seen = self.play.room().map_or(0, |r| r.chat().len());
                let height = sheet::area(ctx).unwrap_or(ctx.content_rect()).height() * 0.7;
                let draft = &mut self.ui.playing.room_draft;
                let shown = sheet::show(ctx, Id::new("room_chat"), "Room chat", |ui| {
                    ui.set_height(height);
                    room_chat::body(ui, &lines, draft, true)
                });
                if shown.closed {
                    self.ui.playing.room_sheet = None;
                }
                if let Some(text) = shown.inner
                    && !room_chat::send(
                        &mut self.play,
                        &mut self.online,
                        &mut self.ui.playing.room_draft,
                        text,
                    )
                {
                    self.toasts.error("Couldn't send the message".to_owned());
                }
            }
            None => {}
        }
    }
}
