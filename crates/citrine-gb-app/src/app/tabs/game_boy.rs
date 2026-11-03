use crate::app::tabs::TabViewer;
use crate::app::tabs::linked;
use crate::app::tabs::playing::{self, Answer, BannerAction, PickFor, PlayButton};
use crate::local_link::Game;
use egui::RichText;

pub fn show(viewer: &mut TabViewer, ui: &mut egui::Ui) {
    if let Some(invite) = playing::pending_ask(viewer.play) {
        let answer = banner_frame(ui, |ui| playing::ask_banner(ui, viewer.online, &invite));
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
    if let Some(session) = viewer.play.session().filter(|s| s.is_live()) {
        let facts = playing::BarFacts {
            muted: viewer.ui.settings.muted,
            compact: false,
            friend_sound: playing::FriendSound::of(&viewer.ui.settings.online),
        };
        let action = banner_frame(ui, |ui| playing::bar(ui, viewer.online, session, facts));
        if let Some(action) = action {
            playing::settle_bar(action, viewer.play, viewer.online, &mut viewer.ui.playing);
        }
    }
    viewer.ui.toast_area = Some(ui.max_rect());
    if let Some(session) = viewer.play.session()
        && let Some((got, total)) = playing::receiving(session)
    {
        let title = playing::game_title(session);
        let name = playing::friend_name(viewer.online, session.friend());
        playing::arriving(
            ui,
            ui.available_rect_before_wrap(),
            &title,
            &name,
            got,
            total,
        );
        return;
    }
    if let Some(session) = viewer.play.session()
        && let Some(line) = playing::screen_note(
            &playing::friend_name(viewer.online, session.friend()),
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
    ui.vertical_centered(|ui| {
        viewer.emulator.ui(ui);
    });
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

fn banner_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, add)
        .inner
}
