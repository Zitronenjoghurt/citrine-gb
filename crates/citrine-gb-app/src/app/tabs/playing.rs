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
    group_link, group_partners, invite, join, join_theirs, join_with, link_theirs, shelf_from,
    unlink_group, watch,
};
pub use banners::{Answer, BannerAction, ask_banner, banner};
pub use bar::{BarFacts, Switch, bar, bar_switches, server_latency, settle_bar};
#[cfg(test)]
pub use confirms::switch_lines;
pub use confirms::{join_confirm, relink_confirm, stop_confirm, switch_confirm};
pub use lines::{
    ending_line, failed_line, friend_tab_line, full_name, game_title, invite_line, member_name,
    path_line, path_mark, play_subtitle, quality, screen_note, short_name,
};
pub use picker::{Pick, picker};
#[cfg(test)]
pub use room::member_menu;
pub use room::{RoomFacts, adds_to_room, in_room, invite_closed, invite_note};
pub use room_bar::{
    MemberAction, MyMenu, RoomAction, header_ui, member_menu_ui, member_row, request_card,
};
#[cfg(test)]
pub use screen::fetch_screen;
pub use screen::{
    WatchCaption, WatchTap, arriving, download_screen, friend_picture, friend_video, note_over,
    paint_friend_screen, watch_thumbnail, watched_friend, watched_name,
};
pub use sound::{FriendSound, friend_sound, friend_sound_bar};

#[cfg(test)]
pub(crate) use lines::{ask_line, watched_line};

const STRIP_AVATAR: f32 = 22.0;

#[derive(Default)]
pub struct PlayingUi {
    friend_screen: Option<egui::TextureHandle>,
    painted_pass: u64,
    fetch_icon: Option<(Vec<u8>, egui::TextureHandle)>,
    fetch_pressed: bool,
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
    pub join_looking: Option<IncomingInvite>,
    pub opened_invite: Option<crate::app::push::OpenedInvite>,
    pub hardcore_refused: bool,
    pick_shown: bool,
    fits: HashMap<(SessionId, Option<GameHash>), Fit>,
    shelf_paths: Option<Vec<std::path::PathBuf>>,
    shelf_index: std::sync::Arc<crate::play::shelf::ShelfIndex>,
}

impl PlayingUi {
    pub fn shelf(&self, recent: &RecentRoms) -> Box<dyn crate::play::shelf::Shelf> {
        shelf_from(recent, &self.shelf_index)
    }

    pub fn handed_back(&mut self, friend: OnlineId, name: &str) {
        self.ended = ending_line(name, "", Ending::Dropped, true).map(|line| Ended {
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
    if !online.is_connected()
        || online.key_changed(friend)
        || matches!(seen(person.presence), Seen::Offline { .. })
    {
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

pub fn can_play_again(play: &Play, online: &Online, emulator: &Emulator, friend: OnlineId) -> bool {
    play_button(play, online, friend) == Some(PlayButton::Ready) && !emulator.holds_download()
}

pub fn can_watch(play: &Play, online: &Online, friend: OnlineId, game_open: bool) -> bool {
    !game_open
        && !play.is_live()
        && play_button(play, online, friend).is_some()
        && online.playing(friend).is_some()
}

pub fn fit_of(
    playing: &mut PlayingUi,
    play: &Play,
    online: &Online,
    emulator: &Emulator,
    invite: &IncomingInvite,
) -> Option<Fit> {
    let open = emulator.open_game();
    let key = (invite.session, open);
    if let Some(found) = playing.fits.get(&key) {
        return Some(*found);
    }
    let sharing = online.info().is_some_and(|i| i.game_sharing);
    let found = match &invite.game {
        None if open.is_some() => Fit::Open,
        None => Fit::Watch,
        Some(game) if open == Some(game.hash) => Fit::Open,
        Some(game) => fit(false, play.holds(&game.hash)?, sharing, open.is_some()),
    };
    playing.fits.insert(key, found);
    Some(found)
}

pub fn refresh_shelf(play: &mut Play, playing: &mut PlayingUi, recent: &RecentRoms) {
    let paths = recent.entries().iter().map(|e| &e.path);
    if playing
        .shelf_paths
        .as_ref()
        .is_some_and(|kept| kept.iter().eq(paths.clone()))
    {
        return;
    }
    playing.shelf_paths = Some(paths.cloned().collect());
    playing.fits.clear();
    play.set_shelf(shelf_from(recent, &playing.shelf_index));
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
        .is_some_and(|r| r.watched_picture().is_some());
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
        let picture = friend_picture(viewer.play, session);
        paint_friend_screen(&mut viewer.ui.playing, ui, picture, area);
        return;
    }
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    let name = member_name(viewer.play, viewer.online, session.friend());
    let line = friend_tab_line(&name, session);
    note_over(ui, area, friend_video(viewer.play, session), &line, None);
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
    if let Some((got, total)) = session.state_received() {
        let label = format!("{}  Getting {name}'s running game", icons::DOWNLOAD_SIMPLE);
        return Some((label, Progress::bytes(got, total)));
    }
    if session.sends_game() {
        let title = session
            .my_game()
            .map(lines::card_name)
            .filter(|title| !title.is_empty())
            .unwrap_or("your game");
        let label = format!("{}  Sending {title} to {name}", icons::UPLOAD_SIMPLE);
        let progress = match session.game_sent() {
            Some((done, total)) => Progress::bytes(done, total),
            None => Progress::unknown(),
        };
        return Some((label, progress));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::bar::{Tail, bar_line};
    use super::*;
    use crate::play::Ending;

    #[test]
    fn the_friends_game_comes_first_then_borrowing_then_the_cable_then_watching() {
        assert_eq!(fit(true, true, true, true), Fit::Open);
        assert_eq!(fit(false, true, true, true), Fit::Shelf);
        assert_eq!(fit(false, false, true, true), Fit::Borrow);
        assert_eq!(fit(false, false, false, true), Fit::Cable);
        assert_eq!(fit(false, false, false, false), Fit::Watch);
    }

    #[test]
    fn an_ending_says_only_what_happened() {
        assert_eq!(
            ending_line("Mia", "Pong", Ending::FriendLeft, true).as_deref(),
            Some("Mia left.")
        );
        assert_eq!(
            ending_line("Mia", "Pong", Ending::Dropped, true).as_deref(),
            Some("Lost the connection to Mia.")
        );
        assert_eq!(
            ending_line("Mia", "Pong", Ending::Failed, true).as_deref(),
            Some("Playing with Mia stopped.")
        );
        assert_eq!(
            ending_line("Mia", "Pong", Ending::FriendLeft, false).as_deref(),
            Some("Mia left.")
        );
        assert_eq!(ending_line("Mia", "Pong", Ending::Left, true), None);
        assert_eq!(
            ending_line("Mia", "Pong", Ending::Declined, false).as_deref(),
            Some("Mia declined your invite.")
        );
    }

    #[test]
    fn a_console_handed_back_in_a_room_shows_the_resume_banner() {
        let mut playing = PlayingUi::default();
        let jonas = OnlineId::random();
        playing.ended = Some(Ended {
            friend: jonas,
            line: "Jonas left.".into(),
            paused: false,
        });
        let mia = OnlineId::random();
        playing.handed_back(mia, "Mia");
        assert_eq!(
            playing.ended,
            Some(Ended {
                friend: mia,
                line: "Lost the connection to Mia.".into(),
                paused: true,
            })
        );
    }

    fn line_right_edge(width: f32, name: &str, title: &str) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut right = 0.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width + 100.0, 200.0),
            )),
            ..Default::default()
        };
        let mut start = 0.0;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_max_width(width);
                start = ui.min_rect().left();
                ui.horizontal(|ui| {
                    ui.set_max_width(width);
                    let tail = Tail {
                        full: "Good \u{b7} 42 ms".into(),
                        short: "42 ms".into(),
                        hover: None,
                        short_hover: Some("Good".into()),
                    };
                    bar_line(ui, name, Some(title), tail);
                    right = ui.min_rect().right();
                });
            });
        });
        (start, right)
    }

    #[test]
    fn the_bar_line_never_runs_past_its_room() {
        for width in [20.0, 60.0, 120.0, 200.0, 320.0, 900.0] {
            let (start, right) =
                line_right_edge(width, "Maximiliane", "Pok\u{e9}mon - Goldene Edition");
            assert!(
                right - start <= width + 0.5,
                "{width}: line ends at {}",
                right - start
            );
        }
    }

    #[test]
    fn the_friends_mute_toggles_and_a_silent_slider_comes_back_at_half() {
        let playing = FriendSound {
            volume: 0.7,
            muted: false,
        };
        assert_eq!(
            playing.toggled(),
            FriendSound {
                volume: 0.7,
                muted: true
            }
        );
        assert_eq!(playing.toggled().toggled(), playing);
        let zero = FriendSound {
            volume: 0.0,
            muted: false,
        };
        assert_eq!(
            zero.toggled(),
            FriendSound {
                volume: 0.5,
                muted: false
            }
        );
    }

    fn fetch_frame(
        ctx: &egui::Context,
        app: &mut crate::app::Citrine,
        fetch: Option<&crate::play::fetch::Fetch>,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 915.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                assert_eq!(
                    fetch_screen(
                        &mut app.ui.playing,
                        ui,
                        area,
                        fetch,
                        "Mia",
                        &mut app.emulator
                    ),
                    fetch.is_some()
                );
            });
        })
    }

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    fn mario_kart_offer() -> crate::play::fetch::Offered {
        crate::play::fetch::Offered {
            title: "MARIO KART DS\nNintendo".into(),
            host: "Mia".into(),
            icon: None,
        }
    }

    #[test]
    fn the_download_button_holds_a_until_the_download_starts() {
        use crate::play::fetch::Fetch;
        use citrine_gb::console::Keys;
        let mut app = crate::app::tests::phone_app("download-button");
        let ctx = egui::Context::default();
        let offer = mario_kart_offer();
        let offered = Fetch::Offered(offer.clone());
        let shown = fetch_frame(&ctx, &mut app, Some(&offered), Vec::new());
        let label = format!("{}  Download", crate::icons::DOWNLOAD_SIMPLE);
        let button = crate::app::tests::text_rect(&shown, &label).expect("a Download button");
        assert!(crate::app::tests::text_rect(&shown, "MARIO KART DS").is_some());
        fetch_frame(&ctx, &mut app, Some(&offered), click(button.center()));
        assert!(app.emulator.is_held(Keys::A));
        fetch_frame(&ctx, &mut app, Some(&offered), Vec::new());
        assert!(app.emulator.is_held(Keys::A));
        fetch_frame(
            &ctx,
            &mut app,
            Some(&Fetch::Joining(offer.clone())),
            Vec::new(),
        );
        assert!(!app.emulator.is_held(Keys::A));
        let waiting = Fetch::Receiving {
            offer,
            got: 0,
            total: 1 << 20,
        };
        let shown = fetch_frame(&ctx, &mut app, Some(&waiting), Vec::new());
        assert!(crate::app::tests::text_rect(&shown, "Waiting for Mia to start").is_some());
        assert!(crate::app::tests::text_rect(&shown, "Downloading").is_none());
        fetch_frame(&ctx, &mut app, None, Vec::new());
        assert!(!app.emulator.is_held(Keys::A));
    }

    #[test]
    fn a_stopped_download_holds_a_on_try_again_until_the_console_joins_again() {
        use crate::play::fetch::Fetch;
        use citrine_gb::console::Keys;
        let mut app = crate::app::tests::phone_app("download-again");
        let ctx = egui::Context::default();
        let offer = mario_kart_offer();
        let failed = Fetch::Failed(offer.clone());
        let shown = fetch_frame(&ctx, &mut app, Some(&failed), Vec::new());
        assert!(crate::app::tests::text_rect(&shown, "The download stopped").is_some());
        let label = format!("{}  Try again", crate::icons::ARROW_CLOCKWISE);
        let again = crate::app::tests::text_rect(&shown, &label).expect("a Try again button");
        fetch_frame(&ctx, &mut app, Some(&failed), click(again.center()));
        assert!(app.emulator.is_held(Keys::A));
        fetch_frame(&ctx, &mut app, Some(&failed), Vec::new());
        assert!(app.emulator.is_held(Keys::A));
        fetch_frame(&ctx, &mut app, Some(&Fetch::Joining(offer)), Vec::new());
        assert!(!app.emulator.is_held(Keys::A));
    }
}
