use super::PlayingUi;
use super::lines::member_name;
use super::room::{RoomFacts, names};
use crate::app::widgets::action::main_button;
use crate::online::Online;
use crate::play::{IncomingInvite, Play};
use citrine_gb_net::OnlineId;

pub fn leave_question(play: &Play, online: &Online) -> Option<String> {
    let session = play.session().filter(|s| s.is_live())?;
    let name = member_name(play, online, session.friend());
    let others = play
        .room()
        .map_or(0, |r| r.members().len().saturating_sub(1));
    match others {
        0 => None,
        1 => {
            let other = play
                .room()
                .and_then(|r| r.members().iter().find(|m| m.who != session.friend()))
                .map(|m| super::short_name(&m.name).to_owned())
                .unwrap_or_default();
            Some(format!("Leave the room with {name} and {other}?"))
        }
        n => Some(format!("Leave the room with {name} and {n} others?")),
    }
}

pub fn switch_lines(play: &Play, online: &Online, next: &str) -> Option<(String, String)> {
    let session = play.session().filter(|s| s.is_live())?;
    let current = member_name(play, online, session.friend());
    let detail =
        leave_question(play, online).unwrap_or_else(|| format!("{current} leaves your game."));
    Some((format!("Switch to {next}?"), detail))
}

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
    let name = member_name(play, online, session.friend());
    let (question, button) = match leave_question(play, online) {
        Some(question) => (question, "Leave"),
        None => (format!("Stop playing with {name}?"), "Stop playing"),
    };
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::new("stop_playing")).show(ctx, |ui| {
        ui.set_max_width(300.0);
        ui.label(question);
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            if columns[0]
                .add_sized(size, egui::Button::new(button))
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

pub(super) fn switch_modal(
    ctx: &egui::Context,
    id: &str,
    lines: (String, String),
    button: String,
) -> Option<bool> {
    crate::app::widgets::confirm::confirm_with_detail(ctx, id, &lines.0, Some(&lines.1), &button)
}

pub fn switch_confirm(
    ctx: &egui::Context,
    playing: &mut PlayingUi,
    online: &Online,
    play: &Play,
) -> Option<OnlineId> {
    let next = playing.switching?;
    let name = member_name(play, online, next);
    let Some(lines) = switch_lines(play, online, &name) else {
        playing.switching = None;
        return Some(next);
    };
    let choice = switch_modal(ctx, "switch_friend", lines, format!("Invite {name}"))?;
    playing.switching = None;
    choice.then_some(next)
}

pub fn join_confirm(
    ctx: &egui::Context,
    playing: &mut PlayingUi,
    online: &Online,
    play: &Play,
) -> Option<IncomingInvite> {
    let next = playing.joining.as_ref()?;
    let name = member_name(play, online, next.from);
    let Some(lines) = switch_lines(play, online, &name) else {
        return playing.joining.take();
    };
    let choice = switch_modal(ctx, "switch_join", lines, format!("Join {name}"))?;
    let next = playing.joining.take()?;
    choice.then_some(next)
}

pub fn relink_question(facts: &RoomFacts, target: &str) -> String {
    let partners: Vec<&str> = facts.partners().iter().map(|p| p.short()).collect();
    format!("Unlink from {} and link with {target}?", names(&partners))
}

pub fn relink_confirm(
    ctx: &egui::Context,
    play: &mut Play,
    online: &Online,
    emulator: &crate::emulator::Emulator,
    playing: &mut PlayingUi,
) {
    let Some(relink) = playing.relinking else {
        return;
    };
    let facts = RoomFacts::of(play, online, 0).filter(|f| f.mine.is_some());
    let Some((facts, target)) = facts.and_then(|f| {
        f.person(relink.target)
            .map(|p| p.short().to_owned())
            .map(|t| (f, t))
    }) else {
        playing.relinking = None;
        return;
    };
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::new("relink")).show(ctx, |ui| {
        ui.set_max_width(320.0);
        ui.label(relink_question(&facts, &target));
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            let size = egui::vec2(columns[0].available_width(), 32.0);
            let link = main_button(&columns[0], "Link");
            if columns[0].add_sized(size, link).clicked() {
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
    let Some(link) = choice else {
        return;
    };
    playing.relinking = None;
    if !link {
        return;
    }
    if relink.take {
        play.take_link(relink.target);
    } else if relink.theirs {
        super::link_theirs(play, emulator, relink.target);
    } else {
        play.link(relink.target);
    }
}
