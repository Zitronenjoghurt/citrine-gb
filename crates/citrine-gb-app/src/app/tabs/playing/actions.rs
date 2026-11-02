use super::lines::card_name;
use super::{Fit, PickFor, PlayingUi, fit_of};
use crate::app::ui_state::recent::RecentRoms;
use crate::emulator::Emulator;
use crate::online::Online;
use crate::play::{GameFile, IncomingInvite, MyGame, Play, Shelf, model_of};
use citrine_gb_net::OnlineId;

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
    let rate = emulator.gb.apu.output_sample_rate;
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
                model: model_of(card.console),
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
