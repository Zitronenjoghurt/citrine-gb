use crate::online::Online;
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::serial::LinkMessage;
use citrine_gb::machine::Machine;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::id::RomId;
use citrine_gb_net::{
    Ask, ClientMsg, Console, EndReason, GameCard, GameHash, GamePart, MAX_STATE_BYTES,
    MAX_TITLE_CHARS, OLDEST_PLAY_VERSION, Offer, OnlineId, Origin, PlayMsg, PublicKey, ServerMsg,
    SessionId, Start, plays_with,
};
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

mod engine;
mod links;
mod messages;
mod rejoin;
mod room;
mod rounds;
mod session;
pub mod shelf;

use engine::Rollback;
pub use links::{LinkGroup, Links};
pub use rejoin::DropHistory;
pub use room::{Room, RoomLine, RoomMember};
pub use shelf::Shelf;

pub const SLOW_ROUND_TRIP_MS: u32 = 120;
pub const SLOW_DELAY: u8 = 2;
pub const GUEST_POWER_ON: u32 = 60;
const PING_EVERY: Duration = Duration::from_secs(1);
const SETUP_PINGS: usize = 3;
const SETUP_WAIT: Duration = Duration::from_secs(3);
const PARTS_PER_UPDATE: usize = 4;
const STALL_SHOWN: Duration = Duration::from_millis(750);
pub const UNLINK_AFTER: Duration = Duration::from_secs(10);
#[cfg(not(test))]
const SILENCE_LIMIT: Duration = Duration::from_secs(20);
#[cfg(test)]
const SILENCE_LIMIT: Duration = Duration::from_secs(3);
const KEPT_PINGS: usize = 8;
const KEPT_ROMS: usize = 4;

#[derive(Clone)]
pub struct GameFile {
    pub rom: Vec<u8>,
    pub title: String,
    pub kind: ConsoleKind,
    pub save: Option<Vec<u8>>,
    pub running: bool,
}

impl GameFile {
    pub fn card(&self) -> Option<GameCard> {
        Some(GameCard {
            hash: sha1_of(&self.rom)?,
            size: u32::try_from(self.rom.len()).ok()?,
            title: self.title.chars().take(MAX_TITLE_CHARS).collect(),
            console: console_of(self.kind),
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
    pub console: Machine,
    pub borrowed: Option<GameHash>,
    pub paused: bool,
    pub friend: OnlineId,
    pub history: Option<DropHistory>,
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
    Moved,
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
    rejoin: Option<DropHistory>,
    their_rejoin: Option<citrine_gb_net::RejoinPoint>,
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
            rejoin: None,
            their_rejoin: None,
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
    key: Option<PublicKey>,
    apart: bool,
    began: bool,
    links_in: Vec<PlayMsg>,
    sync: citrine_gb::netplay::Sync,
    input_delay: Option<u32>,
    role: Role,
    stage: Stage,
    borrowing: bool,
    borrowed_save: Option<Vec<u8>>,
    lent: bool,
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
    grouped: bool,
    stashed: Option<Box<Rollback>>,
}

pub struct Play {
    invites: Vec<IncomingInvite>,
    room: Option<Room>,
    shelf: Box<dyn Shelf>,
    seen_invites: bool,
    input_delay: Option<u32>,
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
            room: None,
            shelf,
            seen_invites: true,
            input_delay: None,
        }
    }

    pub fn invites(&self) -> &[IncomingInvite] {
        &self.invites
    }

    #[cfg(test)]
    pub fn push_invite(&mut self, invite: IncomingInvite) {
        self.invites.push(invite);
    }

    pub fn has_unseen_invite(&self) -> bool {
        !self.seen_invites && !self.invites.is_empty()
    }

    pub fn mark_invites_seen(&mut self) {
        self.seen_invites = true;
    }

    pub fn session(&self) -> Option<&PlaySession> {
        self.room.as_ref().and_then(Room::view)
    }

    pub fn session_mut(&mut self) -> Option<&mut PlaySession> {
        self.room.as_mut().and_then(Room::view_mut)
    }

    pub fn room(&self) -> Option<&Room> {
        self.room.as_ref()
    }

    pub fn is_live(&self) -> bool {
        self.room.as_ref().is_some_and(Room::is_live)
    }

    pub fn bring(&mut self, game: MyGame) -> bool {
        self.room.as_mut().is_some_and(|room| room.bring(game))
    }

    pub fn set_status(&mut self, paused: bool, away: bool) {
        if let Some(room) = self.room.as_mut().filter(|r| r.is_live()) {
            room.set_status(paused, away);
        }
    }

    pub fn unlink_member(&mut self, who: OnlineId) -> bool {
        self.room
            .as_mut()
            .is_some_and(|room| room.unlink_member(who))
    }

    pub fn link(&mut self, who: OnlineId) -> bool {
        self.room.as_mut().is_some_and(|room| room.link(who))
    }

    pub fn watch_group(&mut self, who: OnlineId, on: bool) -> bool {
        self.room
            .as_mut()
            .is_some_and(|room| room.watch_group(who, on))
    }

    pub fn tick_watch(&mut self, now: Instant) {
        if let Some(room) = self.room.as_mut().filter(|r| r.is_live()) {
            room.tick_watch(now);
        }
    }

    pub fn set_let_watch(&mut self, on: bool) {
        if let Some(room) = self.room.as_mut() {
            room.set_let_watch(on);
        }
    }

    pub fn play_theirs(&mut self, who: OnlineId, game: MyGame) -> bool {
        self.room
            .as_mut()
            .is_some_and(|room| room.play_theirs(who, game))
    }

    pub fn shelved(&self) -> Option<&GameCard> {
        self.room.as_ref().and_then(Room::shelved)
    }

    pub fn unlink(&mut self) -> bool {
        self.room.as_mut().is_some_and(Room::unlink)
    }

    pub fn take_link(&mut self, who: OnlineId) -> bool {
        self.room.as_mut().is_some_and(|room| room.take_link(who))
    }

    pub fn drop_link_asks(&mut self) {
        if let Some(room) = self.room.as_mut() {
            room.drop_asks();
        }
    }

    pub fn say(&mut self, online: &mut Online, text: String) -> bool {
        self.room
            .as_mut()
            .is_some_and(|room| room.say(online, text))
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

    pub fn set_input_delay(&mut self, frames: Option<u32>) {
        self.input_delay = frames;
    }

    pub fn invite(
        &mut self,
        online: &mut Online,
        friend: OnlineId,
        game: Option<GameFile>,
    ) -> bool {
        if self.is_live() {
            return self.invite_to_room(online, friend);
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
            version: OLDEST_PLAY_VERSION,
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
        self.room = Some(Room::new(play));
        true
    }

    fn invite_to_room(&mut self, online: &mut Online, friend: OnlineId) -> bool {
        let Some(room) = self.room.as_mut() else {
            return false;
        };
        let Some(id) = room.id() else {
            return false;
        };
        if online.protocol() < citrine_gb_net::PROTOCOL_VERSION
            || room.rejoining()
            || room.others().any(|w| w == friend)
        {
            return false;
        }
        let Some(view) = room.view().filter(|v| v.began) else {
            return false;
        };
        let Some(public_key) = online.friend(friend).map(|f| f.public_key) else {
            return false;
        };
        let Some(keys) = online.keys() else {
            return false;
        };
        let offer = Offer {
            version: OLDEST_PLAY_VERSION,
            ask: Ask::Play,
            game: view.my_game().cloned(),
        };
        let sealed = offer.seal(keys, &public_key);
        if !online.send(ClientMsg::RoomInvite {
            room: id,
            to: friend,
            sealed,
        }) {
            return false;
        }
        room.invited(friend);
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
        self.room = Some(Room::new(play));
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
        if let Some(room) = &mut self.room {
            room.leave(online);
        }
    }

    pub fn close(&mut self) -> Option<PlaySession> {
        match &self.room {
            Some(room) if !room.is_live() => self.room.take().and_then(Room::into_view),
            _ => None,
        }
    }

    pub fn update(&mut self, online: &mut Online, now: Instant) {
        if !online.is_connected() {
            self.invites.clear();
            if let Some(room) = &mut self.room
                && room.is_live()
            {
                if room.can_rejoin() && online.comes_back() {
                    room.lose(now);
                } else {
                    room.end(Ending::Dropped);
                }
            }
            return;
        }
        if let Some(room) = &mut self.room
            && room.is_live()
        {
            room.rejoin(online);
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
        if let Some(room) = &mut self.room {
            let me = online.account().map(|a| a.online_id);
            room.set_input_delay(self.input_delay);
            room.step(&*self.shelf, sharing, me, now);
            room.tell(online);
        }
        self.flush(online);
    }

    pub fn flush(&mut self, online: &mut Online) {
        if let Some(room) = &mut self.room {
            room.flush(online);
        }
    }

    fn live_room(&mut self, id: SessionId) -> Option<&mut Room> {
        self.room
            .as_mut()
            .filter(|r| r.id() == Some(id) && r.is_live())
    }

    fn hear(
        &mut self,
        online: &Online,
        id: SessionId,
        from: Option<OnlineId>,
        sealed: &citrine_gb_net::Sealed,
        now: Instant,
    ) {
        let shelf = &*self.shelf;
        let Some(room) = self
            .room
            .as_mut()
            .filter(|r| r.id() == Some(id) && r.is_live())
        else {
            return;
        };
        let Some((index, key)) = room.route(from, online) else {
            return;
        };
        let Some(keys) = online.keys() else {
            return;
        };
        if let Ok(msg) = PlayMsg::open(keys, &key, sealed) {
            let play = room.pair_at(index);
            play.heard_at = now;
            let friend = play.friend;
            if let Some(msg) = room.hear_watch(friend, msg)
                && let Some(msg) = room.hear_group(friend, msg)
            {
                room.pair_at(index).receive(msg, shelf, now);
            }
        }
    }

    fn hear_part(
        &mut self,
        online: &Online,
        id: SessionId,
        from: Option<OnlineId>,
        sealed: &citrine_gb_net::Sealed,
        now: Instant,
    ) {
        let Some(room) = self.live_room(id) else {
            return;
        };
        let Some((index, key)) = room.route(from, online) else {
            return;
        };
        let Some(keys) = online.keys() else {
            return;
        };
        if let Ok(part) = GamePart::open(keys, &key, sealed) {
            let play = room.pair_at(index);
            play.heard_at = now;
            play.receive_part(part);
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
                    Some(offer) if plays_with(offer.version) => {
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
                let Some(room) = &mut self.room else {
                    online.send(ClientMsg::Leave(session));
                    return;
                };
                if room.id() == Some(session) {
                    room.invited(to);
                    return;
                }
                match room.first_mut() {
                    Some(play)
                        if play.friend == to
                            && play.id.is_none()
                            && play.stage == Stage::Inviting =>
                    {
                        room.set_id(session);
                    }
                    _ => {
                        online.send(ClientMsg::Leave(session));
                    }
                }
            }
            ServerMsg::SessionStarted { session, with } => {
                if let Some(room) = &mut self.room
                    && room.id() == Some(session)
                    && let Some(play) = room.first_mut()
                    && play.friend == with
                {
                    play.begin(now);
                }
            }
            ServerMsg::SessionEnded { session, reason } => {
                self.invites.retain(|i| i.session != session);
                if let Some(room) = &mut self.room
                    && room.id() == Some(session)
                    && room.is_live()
                {
                    room.end(match reason {
                        EndReason::Declined => Ending::Declined,
                        EndReason::Left => Ending::FriendLeft,
                        EndReason::Disconnected => Ending::Dropped,
                    });
                }
            }
            ServerMsg::RoomJoined { room: id, members } => {
                let Some(me) = online.account().map(|a| a.online_id) else {
                    return;
                };
                let sharing = online.info().is_some_and(|i| i.game_sharing);
                if let Some(room) = self.live_room(id) {
                    room.joined(me, members, sharing, now);
                }
            }
            ServerMsg::MemberUpdate { room: id, member } => {
                let sharing = online.info().is_some_and(|i| i.game_sharing);
                if let Some(me) = online.account().map(|a| a.online_id)
                    && member.who != me
                    && let Some(room) = self.live_room(id)
                {
                    room.update_member(member, sharing, now);
                }
            }
            ServerMsg::MemberGone {
                room: id,
                who,
                reason,
            } => {
                let me = online.account().map(|a| a.online_id);
                if let Some(room) = self.live_room(id) {
                    match me == Some(who) {
                        true => room.end(Ending::Moved),
                        false => room.member_gone(who, reason, online),
                    }
                }
            }
            ServerMsg::RoomChat {
                room: id,
                from,
                at,
                sealed,
            } => {
                if let Some(room) = self.live_room(id) {
                    room.hear_chat(from, at, &sealed, online);
                }
            }
            ServerMsg::Error(code) => {
                if let Some(room) = self.room.as_mut().filter(|r| r.is_live()) {
                    room.error(code);
                }
            }
            ServerMsg::Play { session, sealed } => self.hear(online, session, None, &sealed, now),
            ServerMsg::RoomPlay {
                room: id,
                from,
                sealed,
            } => self.hear(online, id, Some(from), &sealed, now),
            ServerMsg::Game { session, sealed } => {
                self.hear_part(online, session, None, &sealed, now)
            }
            ServerMsg::RoomGame {
                room: id,
                from,
                sealed,
            } => self.hear_part(online, id, Some(from), &sealed, now),
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
            | ServerMsg::RoomJoined { .. }
            | ServerMsg::MemberUpdate { .. }
            | ServerMsg::MemberGone { .. }
            | ServerMsg::RoomPlay { .. }
            | ServerMsg::RoomGame { .. }
            | ServerMsg::RoomChat { .. }
            | ServerMsg::Error(_)
    )
}

pub fn console_of(kind: ConsoleKind) -> Console {
    match kind {
        ConsoleKind::Gb => Console::Gb,
        ConsoleKind::Gbc => Console::Gbc,
        ConsoleKind::Gba => Console::Gba,
    }
}

pub fn kind_of(console: Console) -> ConsoleKind {
    match console {
        Console::Gb => ConsoleKind::Gb,
        Console::Gbc => ConsoleKind::Gbc,
        Console::Gba => ConsoleKind::Gba,
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

pub fn sha1_of(rom: &[u8]) -> Option<GameHash> {
    (!rom.is_empty()).then(|| RomId::new(rom).sha1)
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
