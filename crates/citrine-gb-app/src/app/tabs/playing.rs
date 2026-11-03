use crate::app::tabs::TabViewer;
use crate::app::ui_state::recent::RecentRoms;
use crate::emulator::Emulator;
use crate::online::{Online, Seen, seen};
use crate::play::{IncomingInvite, Play, PlaySession, Stage};
use citrine_gb_net::{GameHash, Mode, OnlineId, SessionId};
use egui::Ui;
use std::collections::HashMap;

mod actions;
mod banners;
mod bar;
mod confirms;
mod lines;
mod picker;
mod screen;
mod sound;

pub use actions::{invite, join, join_with, shelf_from, watch};
pub use banners::{Answer, BannerAction, ask_banner, banner};
pub use bar::{BarFacts, Switch, bar, bar_switches, latency, settle_bar};
pub use confirms::{stop_confirm, switch_confirm};
pub use lines::{
    ending_line, friend_name, friend_tab_line, game_title, play_subtitle, quality, screen_note,
    short_name,
};
pub use picker::{Pick, picker};
pub use screen::{arriving, note_over, paint_friend_screen};
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
    pub file_for: Option<PickFor>,
    pub swapped: bool,
    pub wants_copy: bool,
    pub copy_for: bool,
    pub pick_query: String,
    pick_shown: bool,
    fits: HashMap<(SessionId, Option<GameHash>), Fit>,
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
    if session.stage() == Stage::Playing && watching(session) {
        let sound = FriendSound::of(&viewer.ui.settings.online);
        if let Some(sound) = friend_sound_bar(ui, sound) {
            sound.store(&mut viewer.ui.settings.online);
            viewer.ui.settings.dirty = true;
        }
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, egui::Sense::hover());
        paint_friend_screen(&mut viewer.ui.playing, ui, session, area);
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let name = friend_name(viewer.online, session.friend());
    let line = friend_tab_line(&name, session);
    note_over(ui, area, &line, None);
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

pub fn receiving(session: &PlaySession) -> Option<(u32, u32)> {
    match session.stage() {
        Stage::Receiving { got, total } => Some((got, total)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
