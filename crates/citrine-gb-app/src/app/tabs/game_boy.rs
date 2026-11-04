use crate::app::tabs::linked;
use crate::app::tabs::playing::{self, Answer, BannerAction, PickFor, PlayButton};
use crate::app::tabs::{Tab, TabViewer};
use crate::app::widgets::tilt;
use crate::local_link::Game;
use egui::RichText;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    let top = ui.available_height();
    if let Some(invite) = playing::pending_ask(viewer.play) {
        let answer = banner_frame(ui, |ui| {
            playing::ask_banner(ui, viewer.play, viewer.online, &invite)
        });
        match answer {
            Some(Answer::Join) => {
                let joined = playing::join(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    &invite,
                );
                if !joined {
                    viewer.events.notify_error("Couldn't answer the invite");
                }
            }
            Some(Answer::Pick) => viewer.ui.playing.picking = Some(PickFor::Join(invite)),
            Some(Answer::Decline) => viewer.play.decline(viewer.online, invite.session),
            None => {}
        }
    }
    if let Some(ended) = viewer.ui.playing.ended.clone() {
        let again = playing::play_button(viewer.play, viewer.online, ended.friend)
            == Some(PlayButton::Ready);
        let action = banner_frame(ui, |ui| playing::banner(ui, &ended, again));
        match action {
            Some(BannerAction::PlayAgain) => {
                let invited = playing::invite(
                    viewer.play,
                    viewer.online,
                    viewer.emulator,
                    &mut viewer.ui.playing,
                    &viewer.ui.recent,
                    ended.friend,
                );
                if !invited {
                    viewer.events.notify_error("Couldn't send the invite");
                }
            }
            Some(BannerAction::Resume) => {
                viewer.emulator.running = true;
                viewer.ui.playing.ended = None;
            }
            Some(BannerAction::Dismiss) => viewer.ui.playing.ended = None,
            None => {}
        }
    }
    let banners = (top - ui.available_height()).max(0.0);
    let sound = playing::FriendSound::of(&viewer.ui.settings.online);
    if let Some(facts) = playing::room_facts(viewer.play, viewer.online, &viewer.ui.playing)
        && let Some(mine) = playing::my_menu(viewer.play, viewer.online, sound)
    {
        let action = banner_frame(ui, |ui| {
            let row = playing::member_row(ui, &facts, &mine, false);
            playing::request_card(ui, &facts, false).or(row)
        });
        if let Some(action) = action
            && let Some(go) = playing::settle_room(
                action,
                viewer.play,
                viewer.online,
                viewer.emulator,
                &mut viewer.ui.playing,
            )
        {
            room_go(viewer, go);
        }
    } else if let Some(session) = viewer.play.session().filter(|s| s.is_live()) {
        let facts = playing::BarFacts {
            muted: viewer.ui.settings.muted,
            compact: false,
            friend_sound: sound,
        };
        let action = banner_frame(ui, |ui| {
            playing::bar(ui, viewer.play, viewer.online, session, facts)
        });
        if let Some(action) = action {
            playing::settle_bar(action, viewer.play, viewer.online, &mut viewer.ui.playing);
        }
    }
    viewer.ui.toast_area = Some(ui.max_rect());
    if let Some(session) = viewer.play.session()
        && let Some((label, progress)) = playing::transfer(
            session,
            &playing::member_name(viewer.play, viewer.online, session.friend()),
        )
    {
        playing::arriving(ui, ui.available_rect_before_wrap(), &label, progress);
        return;
    }
    if let Some(session) = viewer.play.session()
        && let Some(line) = playing::screen_note(
            &playing::member_name(viewer.play, viewer.online, session.friend()),
            session,
        )
    {
        let copy = playing::needs_copy(session).then_some("Open my copy");
        if playing::note_over(ui, ui.available_rect_before_wrap(), &line, copy) {
            viewer.ui.playing.wants_copy = true;
        }
        return;
    }
    if let Some(link) = &viewer.emulator.link {
        let line = linked::driver(link, Game::First, false);
        ui.vertical_centered(|ui| ui.label(RichText::new(line).weak()));
    }
    let area = ui.available_rect_before_wrap();
    let tilting = viewer.emulator.cart().motion();
    let room = tilt::screen_area(area, tilting);
    viewer.emulator.reserve += banners;
    let screen = ui
        .scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
            ui.vertical_centered(|ui| viewer.emulator.ui(ui)).inner
        })
        .inner;
    viewer.emulator.reserve -= banners;
    let playing =
        viewer.emulator.has_game() || viewer.play.session().is_some_and(|s| s.runs_engine());
    if let Some(screen) = screen
        && playing
    {
        let badge = tilt::badge_rect(screen, area);
        viewer.ui.toast_picture = Some(if tilting { screen.union(badge) } else { screen });
    }
    if let Some(screen) = screen
        && tilting
        && tilt::badge(ui, viewer.emulator.gimmicks.motion, screen, area)
    {
        viewer.events.centre_tilt();
    }
    let Some(link) = &viewer.emulator.link else {
        return;
    };
    if link.takes_clicks() && link.control() == Game::Second {
        let clicked = ui
            .interact(area, egui::Id::new("first_screen"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked();
        if clicked {
            viewer.emulator.set_link_control(Game::First);
        }
    }
}

fn room_go(viewer: &mut TabViewer, go: playing::RoomGo) {
    match go {
        playing::RoomGo::Chat => viewer.events.open_tab(Tab::RoomChat),
        playing::RoomGo::Add => viewer.events.open_tab(Tab::Friends),
        playing::RoomGo::Message(who) => {
            viewer.ui.friends.chat.open(who);
            viewer.events.open_tab(Tab::Friends);
        }
        playing::RoomGo::Profile(who) => {
            viewer.ui.friends.show_profile(who);
            viewer.events.open_tab(Tab::Friends);
        }
    }
}

fn banner_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, add)
        .inner
}
