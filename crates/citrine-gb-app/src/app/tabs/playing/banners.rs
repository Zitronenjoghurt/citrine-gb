use super::lines::{ask_line, full_name, short_name};
use super::{Ended, STRIP_AVATAR};
use crate::app::widgets::action::{action_height, dismiss_button, main_button};
use crate::app::widgets::avatar;
use crate::online::Online;
use crate::play::{IncomingInvite, Play};
use egui::{Label, Ui, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerAction {
    PlayAgain,
    Resume,
    Dismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Join,
    Pick,
    Decline,
}

pub fn ask_banner(
    ui: &mut Ui,
    play: &Play,
    online: &Online,
    invite: &IncomingInvite,
) -> Option<Answer> {
    let full_name = full_name(play, online, invite.from);
    let presence = online.friend_presence(invite.from);
    let line = ask_line(short_name(&full_name), invite);
    let mut answer = None;
    let height = action_height(ui);
    egui::Sides::new()
        .height(height)
        .shrink_left()
        .truncate()
        .show(
            ui,
            |ui| {
                avatar::avatar(ui, &full_name, invite.from.value(), STRIP_AVATAR, presence);
                ui.add(Label::new(line).truncate());
            },
            |ui| {
                if ui
                    .add(dismiss_button().min_size(vec2(height, height)))
                    .on_hover_text("Decline")
                    .clicked()
                {
                    answer = Some(Answer::Decline);
                }
                if ui
                    .add(main_button(ui, "Join").min_size(vec2(0.0, height)))
                    .clicked()
                {
                    answer = Some(Answer::Join);
                }
                if ui
                    .add(egui::Button::new("Pick another").min_size(vec2(0.0, height)))
                    .clicked()
                {
                    answer = Some(Answer::Pick);
                }
            },
        );
    answer
}

pub fn banner(ui: &mut Ui, ended: &Ended, can_play_again: bool) -> Option<BannerAction> {
    let mut action = None;
    egui::Sides::new().shrink_left().wrap().show(
        ui,
        |ui| {
            ui.label(&ended.line);
        },
        |ui| {
            if ended.paused {
                if ui
                    .add(main_button(ui, "Resume"))
                    .on_hover_text("Play on alone")
                    .clicked()
                {
                    action = Some(BannerAction::Resume);
                }
            } else if ui.add(dismiss_button()).on_hover_text("Dismiss").clicked() {
                action = Some(BannerAction::Dismiss);
            }
            if can_play_again && ui.button("Play again").clicked() {
                action = Some(BannerAction::PlayAgain);
            }
        },
    );
    action
}
