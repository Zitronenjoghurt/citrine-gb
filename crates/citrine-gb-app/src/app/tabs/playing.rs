use crate::app::tabs::TabViewer;
use crate::app::ui_state::recent::RecentRoms;
use crate::emulator::Emulator;
use crate::icons;
use crate::online::{Online, Seen, seen};
use crate::play::{Ending, IncomingInvite, Play, PlaySession, Stage};
use crate::progress::Progress;
use citrine_gb_net::{GameHash, Mode, OnlineId, SessionId};
use egui::Ui;
use std::collections::HashMap;

mod actions;
mod banners;
mod bar;
mod confirms;
mod lines;
mod picker;
mod room;
mod room_bar;
mod screen;
mod sound;

pub use actions::{
    group_link, group_partners, invite, join, join_with, link_theirs, shelf_from, unlink_group,
    watch,
};
pub use banners::{Answer, BannerAction, ask_banner, banner};
pub use bar::{BarFacts, Switch, bar, bar_switches, server_latency, settle_bar};
#[cfg(test)]
pub use confirms::switch_lines;
pub use confirms::{join_confirm, relink_confirm, stop_confirm, switch_confirm};
pub use lines::{
    ending_line, friend_tab_line, full_name, game_title, member_name, play_subtitle, quality,
    screen_note, short_name,
};
pub use picker::{Pick, picker};
#[cfg(test)]
pub use room::member_menu;
pub use room::{RoomFacts, adds_to_room, in_room, invite_closed, invite_note};
pub use room_bar::{
    MemberAction, MyMenu, RoomAction, header_ui, member_menu_ui, member_row, request_card,
};
pub use screen::{
    WatchCaption, WatchTap, arriving, friend_console, note_over, paint_friend_screen,
    watch_thumbnail, watched_friend, watched_name,
};
pub use sound::{FriendSound, friend_sound, friend_sound_bar};

#[cfg(test)]
pub(crate) use lines::{ask_line, watched_line};

const STRIP_AVATAR: f32 = 22.0;

#[derive(Default)]
pub struct PlayingUi {
    friend_screen: Option<egui::TextureHandle>,
    painted_pass: u64,
    pub asking_stop: bool,
    pub ended: Option<Ended>,
    pub shown: bool,
    pub picking: Option<PickFor>,
    pub switching: Option<OnlineId>,
    pub switch_to: Option<OnlineId>,
    pub joining: Option<IncomingInvite>,
    pub join_after: Option<IncomingInvite>,
    pub file_for: Option<PickFor>,
    pub swapped: bool,
    pub wants_copy: bool,
    pub copy_for: bool,
    pub pick_query: String,
    pub relinking: Option<Relink>,
    pub unlink_load: Option<usize>,
    pub room_sheet: Option<RoomSheet>,
    pub room_seen: usize,
    pub room_draft: String,
    pick_shown: bool,
    fits: HashMap<(SessionId, Option<GameHash>), Fit>,
}

impl PlayingUi {
    pub fn handed_back(&mut self, friend: OnlineId, name: &str) {
        self.ended = ending_line(name, Ending::Dropped, true).map(|line| Ended {
            friend,
            line,
            paused: true,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relink {
    pub target: OnlineId,
    pub take: bool,
    pub theirs: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomSheet {
    Member(OnlineId),
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomGo {
    Chat,
    Add,
    Message(OnlineId),
    Profile(OnlineId),
}

#[derive(Debug, Clone)]
pub enum PickFor {
    Invite(OnlineId),
    Join(IncomingInvite),
    Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub friend: OnlineId,
    pub line: String,
    pub paused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Open,
    Shelf,
    Borrow,
    Cable,
    Watch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayButton {
    Ready,
    Switch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BarAction {
    Leave,
    ChangeGame,
    Show(bool),
    Watch(bool),
    Cable(bool),
    Mute,
    Chat,
    FriendSound(FriendSound),
}

pub fn fit(open_matches: bool, on_shelf: bool, sharing: bool, game_open: bool) -> Fit {
    if open_matches {
        Fit::Open
    } else if on_shelf {
        Fit::Shelf
    } else if sharing {
        Fit::Borrow
    } else if game_open {
        Fit::Cable
    } else {
        Fit::Watch
    }
}

pub fn play_button(play: &Play, online: &Online, friend: OnlineId) -> Option<PlayButton> {
    let person = online.friend(friend)?;
    if !online.is_connected() || matches!(seen(person.presence), Seen::Offline { .. }) {
        return None;
    }
    if play.invites().iter().any(|i| i.from == friend) {
        return None;
    }
    match play.session().filter(|s| s.is_live()) {
        Some(session) if session.friend() == friend => None,
        Some(_) if in_room(play, friend) => None,
        Some(_) if adds_to_room(play, online) => Some(PlayButton::Ready),
        Some(_) => Some(PlayButton::Switch),
        None => Some(PlayButton::Ready),
    }
}

pub fn can_watch(play: &Play, online: &Online, friend: OnlineId, game_open: bool) -> bool {
    !game_open
        && !play.is_live()
        && play_button(play, online, friend).is_some()
        && online.playing(friend).is_some()
}

pub fn fit_of(
    playing: &mut PlayingUi,
    play: &mut Play,
    online: &Online,
    emulator: &Emulator,
    recent: &RecentRoms,
    invite: &IncomingInvite,
) -> Fit {
    let open = emulator.open_game();
    let key = (invite.session, open);
    if let Some(found) = playing.fits.get(&key) {
        return *found;
    }
    play.set_shelf(shelf_from(recent));
    let sharing = online.info().is_some_and(|i| i.game_sharing);
    let found = match &invite.game {
        None if open.is_some() => Fit::Open,
        None => Fit::Watch,
        Some(game) => fit(
            open == Some(game.hash),
            play.holds(&game.hash),
            sharing,
            open.is_some(),
        ),
    };
    playing.fits.insert(key, found);
    found
}

pub fn watching(session: &PlaySession) -> bool {
    session.sees_friend()
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let Some(session) = viewer.play.session() else {
        crate::app::tabs::linked::show(viewer, ui);
        return;
    };
    let group_watch = viewer
        .play
        .room()
        .is_some_and(|r| r.watched_console().is_some());
    if group_watch || (session.stage() == Stage::Playing && watching(session)) {
        if !group_watch {
            let sound = FriendSound::of(&viewer.ui.settings.online);
            if let Some(sound) = friend_sound_bar(ui, sound) {
                sound.store(&mut viewer.ui.settings.online);
                viewer.ui.settings.dirty = true;
            }
        }
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        let console = friend_console(viewer.play, session);
        paint_friend_screen(&mut viewer.ui.playing, ui, console, area);
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let name = member_name(viewer.play, viewer.online, session.friend());
    let line = friend_tab_line(&name, session);
    note_over(ui, area, &line, None);
}

pub fn room_facts(play: &Play, online: &Online, playing: &PlayingUi) -> Option<RoomFacts> {
    RoomFacts::of(play, online, playing.room_seen)
}

pub fn my_menu(play: &Play, online: &Online, sound: FriendSound) -> Option<MyMenu> {
    let session = play.session()?;
    Some(MyMenu::of(
        session,
        &lines::member_name(play, online, session.friend()),
        sound,
    ))
}

pub fn settle_room(
    action: RoomAction,
    play: &mut Play,
    online: &mut Online,
    emulator: &Emulator,
    playing: &mut PlayingUi,
) -> Option<RoomGo> {
    if matches!(action, RoomAction::Member(..)) {
        playing.room_sheet = None;
    }
    match action {
        RoomAction::Open(who) => playing.room_sheet = Some(RoomSheet::Member(who)),
        RoomAction::Member(who, MemberAction::Watch(on)) => {
            let other = play.room().is_some_and(|r| r.in_other_group(who));
            play.watch_group(who, on);
            if !other && let Some(session) = play.session_mut() {
                session.set_watch_friend(on);
            }
        }
        RoomAction::Member(
            _,
            MemberAction::Link {
                target,
                moves_me,
                theirs,
            },
        ) => {
            if moves_me {
                playing.relinking = Some(Relink {
                    target,
                    take: false,
                    theirs,
                });
            } else if theirs {
                link_theirs(play, emulator, target);
            } else {
                play.link(target);
            }
        }
        RoomAction::Member(_, MemberAction::Unlink) => {
            play.unlink();
        }
        RoomAction::Member(_, MemberAction::OpenCopy) => playing.wants_copy = true,
        RoomAction::Member(who, MemberAction::Message) => return Some(RoomGo::Message(who)),
        RoomAction::Member(who, MemberAction::Profile) => return Some(RoomGo::Profile(who)),
        RoomAction::Member(who, MemberAction::AddFriend) => {
            online.add_friend(who);
        }
        RoomAction::Bar(bar) => bar::settle_bar(bar, play, online, playing),
        RoomAction::Add => return Some(RoomGo::Add),
        RoomAction::Chat => return Some(RoomGo::Chat),
        RoomAction::TakeLink(who) => {
            let linked = room_facts(play, online, playing).is_some_and(|f| f.mine.is_some());
            if linked {
                playing.relinking = Some(Relink {
                    target: who,
                    take: true,
                    theirs: false,
                });
            } else {
                play.take_link(who);
            }
        }
        RoomAction::DropAsks => play.drop_link_asks(),
        RoomAction::Unlink(who) => {
            play.unlink_member(who);
        }
    }
    None
}

pub fn needs_copy(session: &PlaySession) -> bool {
    session.watching_only()
        && session.stage() == Stage::Playing
        && session.friend_plays() == Some(true)
        && session.mode() == Some(Mode::Apart)
}

pub fn pending_ask(play: &Play) -> Option<IncomingInvite> {
    if play.is_live() {
        return None;
    }
    play.invites().last().cloned()
}

pub fn transfer(session: &PlaySession, name: &str) -> Option<(String, Progress)> {
    if let Stage::Receiving { got, total } = session.stage() {
        let title = game_title(session);
        let label = format!("{}  Getting {title} from {name}", icons::DOWNLOAD_SIMPLE);
        return Some((label, Progress::bytes(u64::from(got), u64::from(total))));
    }
    if session.sends_game() {
        let title = session
            .my_game()
            .map(lines::card_name)
            .filter(|title| !title.is_empty())
            .unwrap_or("your game");
        let label = format!("{}  Sending {title} to {name}", icons::UPLOAD_SIMPLE);
        return Some((label, Progress::unknown()));
    }
    None
}

#[cfg(test)]
mod tests;
