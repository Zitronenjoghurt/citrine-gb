use super::lines::{card_name, ds_line, member_name, refused_line};
use super::room::{RoomFacts, adds_to_room, in_room, names};
use super::{Ended, Fit, PickFor, PlayingUi, fit_of, refresh_shelf};
use crate::app::ui_state::recent::RecentRoms;
use crate::emulator::Emulator;
use crate::online::Online;
use crate::play::shelf::ShelfIndex;
use crate::play::{
    GameFile, IncomingInvite, Look, MyGame, Play, PlaySession, Shelf, kind_of, plays_together,
    refusal, shares_games,
};
use citrine_gb_net::{Console, GameCard, OnlineId, Refusal};
use std::sync::Arc;

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

pub fn shelf_from(recent: &RecentRoms, index: &Arc<ShelfIndex>) -> Box<dyn Shelf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(crate::play::shelf::Files {
            paths: recent.entries().iter().map(|e| e.path.clone()).collect(),
            index: index.clone(),
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (recent, index);
        Box::new(crate::play::shelf::Homebrew)
    }
}

fn shelf_file(emulator: &Emulator, rom: Arc<[u8]>, card: &GameCard) -> GameFile {
    let save = emulator.battery_for(&rom);
    let mut file = GameFile::new(
        rom,
        card.hash,
        card_name(card).to_string(),
        kind_of(card.console),
    );
    file.save = save;
    file
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
        Look::Found(rom) => MyGame::Own(shelf_file(emulator, rom, &card)),
        Look::Missing => MyGame::Borrow {
            save: emulator.borrowed_save(&card.hash),
        },
        Look::Looking => return false,
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

fn refused_in_hardcore(emulator: &Emulator, playing: &mut PlayingUi) -> bool {
    playing.hardcore_refused |= emulator.hardcore();
    emulator.hardcore()
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
    if refused_in_hardcore(emulator, playing) {
        return true;
    }
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
    if refused_in_hardcore(emulator, playing) {
        return true;
    }
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
    if let Some(card) = game
        .as_ref()
        .and_then(GameFile::card)
        .filter(|card| !plays_together(card))
    {
        playing.ended = Some(Ended {
            friend,
            line: ds_line(card_name(&card)),
            paused: false,
        });
        return true;
    }
    prepare(emulator, playing);
    refresh_shelf(play, playing, recent);
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
    join_as(play, online, emulator, playing, recent, invite, true)
}

pub fn join_theirs(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    invite: &IncomingInvite,
) -> bool {
    join_as(play, online, emulator, playing, recent, invite, false)
}

fn join_as(
    play: &mut Play,
    online: &mut Online,
    emulator: &mut Emulator,
    playing: &mut PlayingUi,
    recent: &RecentRoms,
    invite: &IncomingInvite,
    asks_how: bool,
) -> bool {
    if refused_in_hardcore(emulator, playing) {
        return true;
    }
    if play.is_live() {
        playing.joining = Some(invite.clone());
        return true;
    }
    if declines_ds(play, online, playing, invite) {
        return true;
    }
    refresh_shelf(play, playing, recent);
    let Some(fit) = fit_of(playing, play, online, emulator, invite) else {
        playing.join_looking = Some(invite.clone());
        return true;
    };
    if asks_how
        && fit == Fit::Borrow
        && invite
            .game
            .as_ref()
            .is_some_and(|card| card.console.is_ds())
    {
        playing.picking = Some(PickFor::Join(invite.clone()));
        return true;
    }
    let game = match (fit, &invite.game) {
        (Fit::Open | Fit::Cable, _) => match emulator.game_file() {
            Some(file) => MyGame::Own(file),
            None => return false,
        },
        (Fit::Shelf, Some(card)) => match play.find(&card.hash) {
            Look::Found(rom) => MyGame::Own(shelf_file(emulator, rom, card)),
            Look::Missing | Look::Looking => return false,
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
    if declines_ds(play, online, playing, invite) {
        return true;
    }
    if let Some(card) = &invite.game
        && let Some(why) = ds_refusal(play, card, &game, shares_games(online))
    {
        let name = member_name(play, online, invite.from);
        if !play.refuse(online, invite.session, why) {
            return false;
        }
        playing.ended = Some(Ended {
            friend: invite.from,
            line: refused_line(&name, card_name(card), why),
            paused: false,
        });
        return true;
    }
    prepare(emulator, playing);
    if !play.accept(online, invite.session, game) {
        return false;
    }
    tune(play, emulator);
    true
}

fn declines_ds(
    play: &mut Play,
    online: &mut Online,
    playing: &mut PlayingUi,
    invite: &IncomingInvite,
) -> bool {
    let Some(card) = invite.game.as_ref().filter(|card| !plays_together(card)) else {
        return false;
    };
    play.decline(online, invite.session);
    playing.ended = Some(Ended {
        friend: invite.from,
        line: ds_line(card_name(card)),
        paused: false,
    });
    true
}

fn ds_refusal(play: &Play, card: &GameCard, game: &MyGame, sharing: bool) -> Option<Refusal> {
    let mine = match game {
        MyGame::Own(file) => file.card(),
        MyGame::Download => return None,
        MyGame::Borrow { .. } | MyGame::Nothing => None,
    };
    let other_copy = match card.console {
        Console::Nds { code } if mine.as_ref().is_none_or(|m| m.hash != card.hash) => {
            play.holds_ds_game(code)
        }
        _ => false,
    };
    refusal(card, mine.as_ref(), other_copy, sharing)
}
