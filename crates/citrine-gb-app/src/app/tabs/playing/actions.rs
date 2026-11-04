use super::lines::card_name;
use super::room::{RoomFacts, adds_to_room, in_room, names};
use super::{Fit, PickFor, PlayingUi, fit_of};
use crate::app::ui_state::recent::RecentRoms;
use crate::emulator::Emulator;
use crate::online::Online;
use crate::play::{GameFile, IncomingInvite, MyGame, Play, PlaySession, Shelf, kind_of};
use citrine_gb_net::OnlineId;

pub fn group_partners(play: &Play, online: &Online) -> Option<String> {
    group_link(play, online).map(|(partners, _)| partners)
}

pub fn group_link(play: &Play, online: &Online) -> Option<(String, Option<u16>)> {
    if !play.session().is_some_and(PlaySession::in_group) {
        return None;
    }
    let facts = RoomFacts::of(play, online, 0)?;
    let partners = facts.partners();
    let short: Vec<&str> = partners.iter().map(|p| p.short()).collect();
    let slowest = partners.iter().filter_map(|p| p.ms).max();
    Some((names(&short), slowest))
}

pub fn unlink_group(play: &mut Play, emulator: &mut Emulator) -> bool {
    if !play.session().is_some_and(PlaySession::in_group) || !play.unlink() {
        return false;
    }
    if let Some(back) = play.session_mut().and_then(PlaySession::take_returned) {
        emulator.keep_returned(back);
    }
    true
}

pub fn shelf_from(recent: &RecentRoms) -> Box<dyn Shelf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(crate::play::shelf::Files {
            paths: recent.entries().iter().map(|e| e.path.clone()).collect(),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = recent;
        Box::new(crate::play::shelf::Homebrew)
    }
}

pub fn link_theirs(play: &mut Play, emulator: &Emulator, who: OnlineId) -> bool {
    let Some(card) = play
        .room()
        .and_then(|r| r.pair(who))
        .and_then(|p| p.friend_game())
        .cloned()
    else {
        return false;
    };
    let game = match play.find(&card.hash) {
        Some(rom) => MyGame::Own(GameFile {
            save: emulator.battery_for(&rom),
            rom,
            title: card_name(&card).to_string(),
            kind: kind_of(card.console),
            running: false,
        }),
        None => MyGame::Borrow {
            save: emulator.borrowed_save(&card.hash),
        },
    };
    play.play_theirs(who, game)
}

fn prepare(emulator: &mut Emulator, playing: &mut PlayingUi) {
    if let Err(err) = emulator.end_link() {
        log::error!("could not save the second game before playing together: {err:?}");
    }
    if let Err(err) = emulator.autosave_if_played() {
        log::error!("could not autosave before playing together: {err:?}");
    }
    playing.ended = None;
    playing.shown = false;
    playing.swapped = false;
}

fn tune(play: &mut Play, emulator: &Emulator) {
    let rate = emulator.sample_rate();
    if let Some(session) = play.session_mut() {
        session.set_sample_rate(rate);
    }
}

pub fn invite(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    friend: OnlineId,
) -> bool {
    if adds_to_room(play, online) && !in_room(play, friend) {
        return play.invite(online, friend, None);
    }
    if play
        .session()
        .is_some_and(|s| s.is_live() && s.friend() != friend)
    {
        playing.switching = Some(friend);
        return true;
    }
    let Some(game) = emulator.game_file() else {
        playing.picking = Some(PickFor::Invite(friend));
        return true;
    };
    send_invite(play, online, emulator, playing, recent, friend, Some(game))
}

pub fn watch(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    friend: OnlineId,
) -> bool {
    if play.is_live() {
        return false;
    }
    send_invite(play, online, emulator, playing, recent, friend, None)
}

fn send_invite(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    friend: OnlineId,
    game: Option<GameFile>,
) -> bool {
    prepare(emulator, playing);
    play.set_shelf(shelf_from(recent));
    if !play.invite(online, friend, game) {
        return false;
    }
    tune(play, emulator);
    true
}

pub fn join(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    invite: &IncomingInvite,
) -> bool {
    if play.is_live() {
        playing.joining = Some(invite.clone());
        return true;
    }
    let fit = fit_of(playing, play, online, emulator, recent, invite);
    let game = match (fit, &invite.game) {
        (Fit::Open | Fit::Cable, _) => match emulator.game_file() {
            Some(file) => MyGame::Own(file),
            None => return false,
        },
        (Fit::Shelf, Some(card)) => match play.find(&card.hash) {
            Some(rom) => MyGame::Own(GameFile {
                save: emulator.battery_for(&rom),
                rom,
                title: card_name(card).to_string(),
                kind: kind_of(card.console),
                running: false,
            }),
            None => return false,
        },
        (Fit::Borrow, Some(card)) => MyGame::Borrow {
            save: emulator.borrowed_save(&card.hash),
        },
        (Fit::Watch, _) => MyGame::Nothing,
        (Fit::Shelf | Fit::Borrow, None) => return false,
    };
    join_with(play, online, emulator, playing, invite, game)
}

pub fn join_with(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    invite: &IncomingInvite,
    game: MyGame,
) -> bool {
    prepare(emulator, playing);
    if !play.accept(online, invite.session, game) {
        return false;
    }
    tune(play, emulator);
    true
}
