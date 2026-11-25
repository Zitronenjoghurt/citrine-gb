use super::Citrine;
use crate::app::mobile::kit;
use crate::app::tabs::Tab;
use crate::icons;
use crate::online::{Online, OnlineSettings};
use crate::platform::push::{self, Opened};
use crate::play::IncomingInvite;
use citrine_gb_net::{OnlineId, PushKinds, SessionId};
use egui::{RichText, Ui};
use std::time::Duration;
use web_time::Instant;

const INVITE_WAIT: Duration = Duration::from_secs(30);

pub const ASK_TITLE: &str = "Get notified";
pub const ASK_TEXT: &str =
    "Hear about messages, invites and friend requests while Citrine is closed.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    TurnOn,
    NotNow,
}

pub fn kinds(settings: &OnlineSettings, allowed: bool) -> PushKinds {
    if allowed {
        settings.push_kinds()
    } else {
        PushKinds::default()
    }
}

pub fn wants_ask(settings: &OnlineSettings, online: &Online) -> bool {
    asks(
        settings,
        push::available(),
        push::allowed(),
        online.is_connected() && !online.friends().is_empty(),
    )
}

fn asks(settings: &OnlineSettings, available: bool, allowed: bool, has_friends: bool) -> bool {
    available && !allowed && !settings.notify_asked && has_friends
}

pub fn ask_card(ui: &mut Ui) -> Option<Answer> {
    let mut answer = None;
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icons::BELL).size(22.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(ASK_TITLE).strong());
                    ui.label(RichText::new(ASK_TEXT).color(kit::value_color(ui.visuals())));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if kit::pill(ui, "Turn on", true).clicked() {
                    answer = Some(Answer::TurnOn);
                }
                if kit::pill(ui, "Not now", false).clicked() {
                    answer = Some(Answer::NotNow);
                }
            });
        })
    });
    answer
}

pub fn answer_ask(settings: &mut OnlineSettings, answer: Answer) {
    settings.notify_asked = true;
    if answer == Answer::TurnOn {
        push::ask();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenedInvite {
    from: OnlineId,
    session: SessionId,
    until: Instant,
}

impl OpenedInvite {
    pub fn new(from: OnlineId, session: SessionId, now: Instant) -> Self {
        Self {
            from,
            session,
            until: now + INVITE_WAIT,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Follow {
    Join(IncomingInvite),
    Profile(OnlineId),
}

pub fn follow_invite(
    opened: &mut Option<OpenedInvite>,
    invites: &[IncomingInvite],
    now: Instant,
) -> Option<Follow> {
    let waiting = (*opened)?;
    if let Some(invite) = invites.iter().find(|i| i.session == waiting.session) {
        *opened = None;
        return Some(Follow::Join(invite.clone()));
    }
    if now >= waiting.until {
        *opened = None;
        return Some(Follow::Profile(waiting.from));
    }
    None
}

impl Citrine {
    pub(super) fn update_push(&mut self) {
        let kinds = kinds(&self.ui.settings.online, push::allowed());
        push::with_token(|token| self.online.set_push(token, kinds));
    }

    pub(super) fn follow_push(&mut self, now: Instant) {
        push::keep_status_line(self.online.status_line());
        if let Some(opened) = push::take_opened() {
            self.open_notification(opened, now);
        }
    }

    pub(super) fn follow_opened_invite(&mut self, now: Instant) {
        let invites = self.play.invites();
        match follow_invite(&mut self.ui.playing.opened_invite, invites, now) {
            Some(Follow::Join(invite)) => {
                self.events.open_tab(Tab::GameBoy);
                self.answer_ask(&invite);
            }
            Some(Follow::Profile(from)) => {
                self.ui.friends.show_profile(from);
                self.events.open_tab(Tab::Friends);
            }
            None => {}
        }
    }

    fn open_notification(&mut self, opened: Opened, now: Instant) {
        match opened {
            Opened::Friend(id) => {
                self.ui.friends.show_profile(id);
                self.events.open_tab(Tab::Friends);
            }
            Opened::Chat(id) => {
                self.ui.friends.chat.open(id);
                self.events.open_tab(Tab::Friends);
            }
            Opened::Invite { from, session } => {
                self.ui.playing.opened_invite = Some(OpenedInvite::new(from, session, now));
                self.events.open_tab(Tab::GameBoy);
            }
            Opened::Walker => self.events.open_tab(Tab::Walker),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_step_notification_opens_the_walker() {
        assert_eq!(Opened::parse("walker", "", ""), Some(Opened::Walker));
    }

    #[test]
    fn a_tapped_invite_joins_once_it_arrives() {
        let from = OnlineId::random();
        let session = 4_242;
        let payload = Opened::parse("invite", &from.to_string(), "4242");
        assert_eq!(payload, Some(Opened::Invite { from, session }));
        let start = Instant::now();
        let mut opened = Some(OpenedInvite::new(from, session, start));
        let other = IncomingInvite {
            session: 7,
            from,
            game: None,
        };
        let later = start + Duration::from_secs(3);
        assert!(follow_invite(&mut opened, std::slice::from_ref(&other), later).is_none());
        let invite = IncomingInvite {
            session,
            from,
            game: None,
        };
        match follow_invite(&mut opened, &[other, invite], later) {
            Some(Follow::Join(joined)) => assert_eq!(joined.session, session),
            other => panic!("expected a join, got {other:?}"),
        }
        assert_eq!(opened, None);
    }

    #[test]
    fn a_tapped_invite_that_never_arrives_opens_the_profile() {
        let from = OnlineId::random();
        let start = Instant::now();
        let mut opened = Some(OpenedInvite::new(from, 9, start));
        assert!(follow_invite(&mut opened, &[], start + Duration::from_secs(29)).is_none());
        match follow_invite(&mut opened, &[], start + INVITE_WAIT) {
            Some(Follow::Profile(who)) => assert_eq!(who, from),
            other => panic!("expected the profile, got {other:?}"),
        }
        assert_eq!(opened, None);
    }

    #[test]
    fn no_permission_means_nothing_is_asked_of_the_server() {
        let settings = OnlineSettings::default();
        assert_eq!(kinds(&settings, false), PushKinds::default());
        assert_eq!(kinds(&settings, true), PushKinds::ALL);
    }

    #[test]
    fn each_switch_reaches_the_server() {
        let settings = OnlineSettings {
            notify_friend_requests: false,
            notify_invites: true,
            notify_chats: false,
            ..OnlineSettings::default()
        };
        assert_eq!(
            kinds(&settings, true),
            PushKinds {
                friend_requests: false,
                invites: true,
                chats: false,
            }
        );
    }

    #[test]
    fn the_ask_waits_for_a_friend_and_comes_once() {
        let mut settings = OnlineSettings::default();
        assert!(!asks(&settings, true, false, false), "not before a friend");
        assert!(asks(&settings, true, false, true));
        assert!(!asks(&settings, true, true, true), "not once allowed");
        assert!(!asks(&settings, false, false, true), "not without push");
        answer_ask(&mut settings, Answer::NotNow);
        assert!(!asks(&settings, true, false, true), "not after an answer");
    }
}
