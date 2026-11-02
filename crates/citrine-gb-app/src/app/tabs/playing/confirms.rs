use super::PlayingUi;
use super::lines::friend_name;
use crate::app::widgets::action::main_button;
use crate::online::Online;
use crate::play::Play;
use citrine_gb_net::OnlineId;
use egui::RichText;

pub fn stop_confirm(
    ctx: &egui::Context,
    play: &mut Play,
    online: &mut Online,
    playing: &mut PlayingUi,
) {
    if !playing.asking_stop {
        return;
    }
    let Some(session) = play.session().filter(|s| s.is_live()) else {
        playing.asking_stop = false;
        return;
    };
    let name = friend_name(online, session.friend());
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::new("stop_playing")).show(ctx, |ui| {
        ui.set_max_width(300.0);
        ui.label(format!("Stop playing with {name}?"));
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            if columns[0]
                .add_sized(size, egui::Button::new("Stop playing"))
                .clicked()
            {
                choice = Some(true);
            }
            let stay = main_button(&columns[1], "Stay");
            if columns[1].add_sized(size, stay).clicked() {
                choice = Some(false);
            }
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(false);
    }
    if let Some(stop) = choice {
        playing.asking_stop = false;
        if stop {
            play.leave(online);
        }
    }
}

pub fn switch_confirm(
    ctx: &egui::Context,
    playing: &mut PlayingUi,
    online: &Online,
    play: &Play,
) -> Option<OnlineId> {
    let next = playing.switching?;
    let Some(session) = play.session().filter(|s| s.is_live()) else {
        playing.switching = None;
        return Some(next);
    };
    let current = friend_name(online, session.friend());
    let name = friend_name(online, next);
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::new("switch_friend")).show(ctx, |ui| {
        ui.set_max_width(320.0);
        ui.label(format!("Switch to {name}?"));
        ui.label(
            RichText::new(format!("{current} leaves your game."))
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            let invite = main_button(&columns[0], format!("Invite {name}"));
            if columns[0].add_sized(size, invite).clicked() {
                choice = Some(true);
            }
            if columns[1]
                .add_sized(size, egui::Button::new("Cancel"))
                .clicked()
            {
                choice = Some(false);
            }
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(false);
    }
    let choice = choice?;
    playing.switching = None;
    choice.then_some(next)
}
