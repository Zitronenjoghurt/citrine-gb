use crate::online::Online;
use citrine_gb::gb::serial::LinkMessage;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_net::{
    Ask, ClientMsg, Console, EndReason, GameCard, GameHash, GamePart, MAX_STATE_BYTES,
    MAX_TITLE_CHARS, Offer, OnlineId, Origin, PLAY_VERSION, PlayMsg, ServerMsg, SessionId, Start,
};
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

mod engine;
mod messages;
mod rounds;
mod session;
pub mod shelf;

use engine::Rollback;
pub use shelf::Shelf;

pub const SLOW_ROUND_TRIP_MS: u32 = 120;
pub const SLOW_DELAY: u8 = 2;
pub const GUEST_POWER_ON: u32 = 60;
const PING_EVERY: Duration = Duration::from_secs(1);
const SETUP_PINGS: usize = 3;
const SETUP_WAIT: Duration = Duration::from_secs(3);
const PARTS_PER_UPDATE: usize = 4;
const STALL_SHOWN: Duration = Duration::from_millis(750);
#[cfg(not(test))]
const SILENCE_LIMIT: Duration = Duration::from_secs(20);
#[cfg(test)]
const SILENCE_LIMIT: Duration = Duration::from_secs(3);
const KEPT_PINGS: usize = 8;
const KEPT_ROMS: usize = 4;

pub struct GameFile {
    pub rom: Vec<u8>,
    pub title: String,
    pub model: GbModel,
    pub save: Option<Vec<u8>>,
    pub running: bool,
}

impl GameFile {
    pub fn card(&self) -> Option<GameCard> {
        let header = Rom::new(&self.rom).header().ok()?;
        Some(GameCard {
            hash: header.sha1,
            size: u32::try_from(self.rom.len()).ok()?,
            title: self.title.chars().take(MAX_TITLE_CHARS).collect(),
            console: console_of(self.model),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveKey {
    Borrowed(GameHash),
    Rom(String),
}

pub struct LinkedSave {
    pub key: SaveKey,
    pub dump: SramDump,
}

pub struct Returned {
    pub console: GameBoy,
    pub borrowed: Option<GameHash>,
    pub paused: bool,
}

pub enum MyGame {
    Own(GameFile),
    Borrow { save: Option<Vec<u8>> },
    Nothing,
}

#[derive(Debug, Clone)]
pub struct IncomingInvite {
    pub session: SessionId,
    pub from: OnlineId,
    pub game: Option<GameCard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Declined,
    Left,
    FriendLeft,
    Dropped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Inviting,
    Preparing,
    Receiving { got: u32, total: u32 },
    Playing,
    Ended(Ending),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Guest,
}

enum Engine {
    Rollback(Box<Rollback>),
    Cable(CableLink),
    Apart,
}

#[derive(Default)]
struct CableLink {
    plugged: bool,
    applied: Option<bool>,
    incoming: VecDeque<LinkMessage>,
}

struct Incoming {
    hash: GameHash,
    total: u32,
    data: Vec<u8>,
}

struct Round {
    number: u32,
    mine: Option<GameFile>,
    my_card: Option<GameCard>,
    my_origin: Option<Origin>,
    resume: bool,
    joined: bool,
    set_up: bool,
    theirs: Option<Option<GameCard>>,
    their_rom: Option<Vec<u8>>,
    their_origin: Option<Origin>,
    i_hold_theirs: Option<bool>,
    they_hold_mine: Option<bool>,
    incoming: Option<Incoming>,
    sending: Option<usize>,
    start: Option<Start>,
    engine: Option<Engine>,
}

impl Round {
    fn new(number: u32, mine: Option<GameFile>, my_card: Option<GameCard>) -> Self {
        Self {
            number,
            mine,
            my_card,
            my_origin: None,
            resume: false,
            joined: false,
            set_up: false,
            theirs: None,
            their_rom: None,
            their_origin: None,
            i_hold_theirs: None,
            they_hold_mine: None,
            incoming: None,
            sending: None,
            start: None,
            engine: None,
        }
    }

    fn rollback(&self) -> Option<&Rollback> {
        match &self.engine {
            Some(Engine::Rollback(r)) => Some(r),
            _ => None,
        }
    }

    fn rollback_mut(&mut self) -> Option<&mut Rollback> {
        match &mut self.engine {
            Some(Engine::Rollback(r)) => Some(r),
            _ => None,
        }
    }
}

pub struct PlaySession {
    id: Option<SessionId>,
    friend: OnlineId,
    role: Role,
    stage: Stage,
    borrowing: bool,
    borrowed_save: Option<Vec<u8>>,
    round: Round,
    roms: Vec<(GameHash, Vec<u8>)>,
    wants_state: bool,
    returned: Option<Returned>,
    pings: VecDeque<(u32, Instant)>,
    next_ping: u32,
    round_trips: VecDeque<u32>,
    pinged_at: Option<Instant>,
    started_at: Instant,
    heard_at: Instant,
    let_watch: bool,
    friend_lets_watch: bool,
    watch_friend: bool,
    my_cable: bool,
    friend_cable: bool,
    saves: Vec<LinkedSave>,
    sharing: bool,
    outbox: Vec<PlayMsg>,
    game_outbox: Vec<GamePart>,
    sample_rate: u32,
}

pub struct Play {
    invites: Vec<IncomingInvite>,
    session: Option<PlaySession>,
    shelf: Box<dyn Shelf>,
    seen_invites: bool,
}

impl Default for Play {
    fn default() -> Self {
        Self::new(Box::new(shelf::Homebrew))
    }
}

impl Play {
    pub fn new(shelf: Box<dyn Shelf>) -> Self {
        Self {
            invites: Vec::new(),
            session: None,
            shelf,
            seen_invites: true,
        }
    }

    pub fn invites(&self) -> &[IncomingInvite] {
        &self.invites
    }

    pub fn has_unseen_invite(&self) -> bool {
        !self.seen_invites && !self.invites.is_empty()
    }

    pub fn mark_invites_seen(&mut self) {
        self.seen_invites = true;
    }

    pub fn session(&self) -> Option<&PlaySession> {
        self.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Option<&mut PlaySession> {
        self.session.as_mut()
    }

    pub fn is_live(&self) -> bool {
        self.session.as_ref().is_some_and(PlaySession::is_live)
    }

    pub fn holds(&self, hash: &GameHash) -> bool {
        self.find(hash).is_some()
    }

    pub fn find(&self, hash: &GameHash) -> Option<Vec<u8>> {
        self.shelf
            .find(hash)
            .filter(|rom| sha1_of(rom) == Some(*hash))
    }

    pub fn set_shelf(&mut self, shelf: Box<dyn Shelf>) {
        self.shelf = shelf;
    }

    pub fn invite(
        &mut self,
        online: &mut Online,
        friend: OnlineId,
        game: Option<GameFile>,
    ) -> bool {
        if self.is_live() {
            return false;
        }
        let card = match &game {
            Some(game) => match game.card() {
                Some(card) => Some(card),
                None => return false,
            },
            None => None,
        };
        let Some(public_key) = online.friend(friend).map(|f| f.public_key) else {
            return false;
        };
        let Some(keys) = online.keys() else {
            return false;
        };
        let offer = Offer {
            version: PLAY_VERSION,
            ask: Ask::Play,
            game: card.clone(),
        };
        let sealed = offer.seal(keys, &public_key);
        if !online.send(ClientMsg::Invite { to: friend, sealed }) {
            return false;
        }
        let sharing = online.info().is_some_and(|i| i.game_sharing);
        let mut play = PlaySession::new(friend, Role::Host, Stage::Inviting, sharing);
        play.round = Round::new(1, game, card);
        self.session = Some(play);
        true
    }

    pub fn accept(&mut self, online: &mut Online, session: SessionId, game: MyGame) -> bool {
        if self.is_live() {
            return false;
        }
        let Some(index) = self.invites.iter().position(|i| i.session == session) else {
            return false;
        };
        let sharing = online.info().is_some_and(|i| i.game_sharing);
        let invite = &self.invites[index];
        let (card, file, borrowing, save) = match game {
            MyGame::Own(file) => match file.card() {
                Some(card) => (Some(card), Some(file), false, None),
                None => return false,
            },
            MyGame::Borrow { save } if sharing => match &invite.game {
                Some(game) => (Some(game.clone()), None, true, save),
                None => return false,
            },
            MyGame::Borrow { .. } => return false,
            MyGame::Nothing => (None, None, false, None),
        };
        if !online.send(ClientMsg::AnswerInvite {
            session,
            accept: true,
        }) {
            return false;
        }
        let invite = self.invites.remove(index);
        let mut play = PlaySession::new(invite.from, Role::Guest, Stage::Preparing, sharing);
        play.id = Some(session);
        play.borrowing = borrowing;
        play.borrowed_save = save;
        play.round = Round::new(1, file, card);
        self.session = Some(play);
        true
    }

    pub fn decline(&mut self, online: &mut Online, session: SessionId) {
        self.invites.retain(|i| i.session != session);
        online.send(ClientMsg::AnswerInvite {
            session,
            accept: false,
        });
    }

    pub fn leave(&mut self, online: &mut Online) {
        let Some(play) = &mut self.session else {
            return;
        };
        if let Some(id) = play.id
            && play.is_live()
        {
            online.send(ClientMsg::Leave(id));
        }
        if play.is_live() {
            play.end(Ending::Left);
        }
    }

    pub fn close(&mut self) -> Option<PlaySession> {
        match &self.session {
            Some(play) if !play.is_live() => self.session.take(),
            _ => None,
        }
    }

    pub fn update(&mut self, online: &mut Online, now: Instant) {
        if !online.is_connected() {
            self.invites.clear();
            if let Some(play) = &mut self.session
                && play.is_live()
            {
                play.end(Ending::Dropped);
            }
            return;
        }
        let msgs: Vec<ServerMsg> = online
            .inbox()
            .iter()
            .filter(|m| is_session_msg(m))
            .cloned()
            .collect();
        for msg in msgs {
            self.receive(online, msg, now);
        }
        let sharing = online.info().is_some_and(|i| i.game_sharing);
        if let Some(play) = &mut self.session {
            play.sharing = sharing;
            play.step(&*self.shelf, now);
        }
        self.flush(online);
    }

    pub fn flush(&mut self, online: &mut Online) {
        if let Some(play) = &mut self.session {
            play.flush(online);
        }
    }

    fn receive(&mut self, online: &mut Online, msg: ServerMsg, now: Instant) {
        match msg {
            ServerMsg::Invite {
                session,
                from,
                sealed,
            } => {
                let offer = online
                    .friend(from)
                    .map(|f| f.public_key)
                    .zip(online.keys())
                    .and_then(|(key, keys)| {
                        Offer::open(keys, &key, &sealed)
                            .inspect_err(|err| log::warn!("declined an invite: {err}"))
                            .ok()
                    });
                match offer {
                    Some(offer) if offer.version == PLAY_VERSION => {
                        self.invites.retain(|i| i.from != from);
                        self.invites.push(IncomingInvite {
                            session,
                            from,
                            game: offer.game,
                        });
                        self.seen_invites = false;
                    }
                    _ => {
                        online.send(ClientMsg::AnswerInvite {
                            session,
                            accept: false,
                        });
                    }
                }
            }
            ServerMsg::Invited { session, to } => {
                if let Some(play) = &mut self.session
                    && play.friend == to
                    && play.id.is_none()
                    && play.stage == Stage::Inviting
                {
                    play.id = Some(session);
                } else {
                    online.send(ClientMsg::Leave(session));
                }
            }
            ServerMsg::SessionStarted { session, with } => {
                if let Some(play) = &mut self.session
                    && play.id == Some(session)
                    && play.friend == with
                {
                    play.begin(now);
                }
            }
            ServerMsg::SessionEnded { session, reason } => {
                self.invites.retain(|i| i.session != session);
                if let Some(play) = &mut self.session
                    && play.id == Some(session)
                    && play.is_live()
                {
                    play.end(match reason {
                        EndReason::Declined => Ending::Declined,
                        EndReason::Left => Ending::FriendLeft,
                        EndReason::Disconnected => Ending::Dropped,
                    });
                }
            }
            ServerMsg::Play { session, sealed } => {
                let Some(play) = self.session.as_mut().filter(|p| p.id == Some(session)) else {
                    return;
                };
                let Some(key) = online.friend(play.friend).map(|f| f.public_key) else {
                    return;
                };
                let Some(keys) = online.keys() else {
                    return;
                };
                if let Ok(msg) = PlayMsg::open(keys, &key, &sealed) {
                    play.heard_at = now;
                    play.receive(msg, &*self.shelf, now);
                }
            }
            ServerMsg::Game { session, sealed } => {
                let Some(play) = self.session.as_mut().filter(|p| p.id == Some(session)) else {
                    return;
                };
                let Some(key) = online.friend(play.friend).map(|f| f.public_key) else {
                    return;
                };
                let Some(keys) = online.keys() else {
                    return;
                };
                if let Ok(part) = GamePart::open(keys, &key, &sealed) {
                    play.heard_at = now;
                    play.receive_part(part);
                }
            }
            _ => {}
        }
    }
}

fn is_session_msg(msg: &ServerMsg) -> bool {
    matches!(
        msg,
        ServerMsg::Invite { .. }
            | ServerMsg::Invited { .. }
            | ServerMsg::SessionStarted { .. }
            | ServerMsg::SessionEnded { .. }
            | ServerMsg::Play { .. }
            | ServerMsg::Game { .. }
    )
}

pub fn console_of(model: GbModel) -> Console {
    match model {
        GbModel::Dmg => Console::Gb,
        GbModel::Cgb => Console::Gbc,
    }
}

pub fn model_of(console: Console) -> GbModel {
    match console {
        Console::Gb => GbModel::Dmg,
        Console::Gbc => GbModel::Cgb,
    }
}

fn unix_now() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn random_seed() -> u64 {
    let mut bytes = [0; 8];
    let _ = getrandom::getrandom(&mut bytes);
    u64::from_le_bytes(bytes)
}

fn sha1_of(rom: &[u8]) -> Option<GameHash> {
    Rom::new(rom).header().ok().map(|h| h.sha1)
}

fn packed(origin: Origin) -> Origin {
    match origin {
        Origin::Resume(state) => Origin::Resume(citrine_gb_net::pack(&state, true)),
        boot => boot,
    }
}

fn unpacked(origin: Origin) -> Option<Origin> {
    match origin {
        Origin::Resume(state) => citrine_gb_net::unpack(&state, MAX_STATE_BYTES)
            .ok()
            .map(Origin::Resume),
        boot => Some(boot),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests;
