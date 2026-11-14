use crate::online::Online;
use citrine_gb::console::ConsoleKind;
use citrine_gb::gb::serial::LinkMessage;
use citrine_gb::machine::Machine;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb_net::{
    Ask, ClientMsg, Console, EndReason, GameCard, GameHash, GamePart, MAX_STATE_BYTES,
    MAX_TITLE_CHARS, OLDEST_PLAY_VERSION, Offer, OnlineId, Origin, PeerSealer, Pieces, PlayMsg,
    PublicKey, Refusal, Sealed, ServerMsg, SessionId, Start, plays_with,
};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

mod engine;
pub mod fetch;
mod links;
mod messages;
mod pace;
mod rejoin;
mod room;
mod rounds;
mod seen;
mod session;
pub mod shelf;

use citrine_gb_threads::{Lent, Pending, Threads};
use engine::Rollback;
pub use links::{LinkGroup, Links};
pub use rejoin::DropHistory;
pub use room::{Room, RoomLine, RoomMember};
pub use seen::Picture;
use seen::{Seen, Shown};
pub use shelf::{Look, Shelf};

pub const SLOW_ROUND_TRIP_MS: u32 = 120;
pub const SLOW_DELAY: u8 = 2;
pub const GUEST_POWER_ON: u32 = 60;
const PING_EVERY: Duration = Duration::from_secs(1);
const SETUP_PINGS: usize = 3;
const SETUP_WAIT: Duration = Duration::from_secs(3);
const PARTS_PER_UPDATE: usize = 4;
const RESEND_AFTER: Duration = Duration::from_secs(2);
const STALL_SHOWN: Duration = Duration::from_millis(750);
pub const UNLINK_AFTER: Duration = Duration::from_secs(10);
#[cfg(not(test))]
const SILENCE_LIMIT: Duration = Duration::from_secs(20);
#[cfg(test)]
const SILENCE_LIMIT: Duration = Duration::from_secs(3);
const KEPT_PINGS: usize = 8;
const KEPT_ROMS: usize = 4;
const DS_CODE: std::ops::Range<usize> = 0x0C..0x10;

#[derive(Clone)]
pub struct GameFile {
    pub rom: Arc<[u8]>,
    pub title: String,
    pub kind: ConsoleKind,
    pub save: Option<Vec<u8>>,
    pub running: bool,
    hash: GameHash,
}

impl GameFile {
    pub fn new(rom: Arc<[u8]>, hash: GameHash, title: String, kind: ConsoleKind) -> Self {
        Self {
            rom,
            title,
            kind,
            save: None,
            running: false,
            hash,
        }
    }

    pub fn hash(&self) -> GameHash {
        self.hash
    }

    pub fn card(&self) -> Option<GameCard> {
        if self.rom.is_empty() {
            return None;
        }
        Some(GameCard {
            hash: self.hash,
            size: u32::try_from(self.rom.len()).ok()?,
            title: self.title.chars().take(MAX_TITLE_CHARS).collect(),
            console: console_of(self.kind, &self.rom),
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
    pub downloaded: bool,
}

pub enum MyGame {
    Own(GameFile),
    Borrow { save: Option<Vec<u8>> },
    Download,
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
    NoCopy,
    OtherCopy,
    NoAnswer,
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
    Rollback(Lent<Rollback>),
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
    their_rom: Option<Arc<[u8]>>,
    their_origin: Option<Origin>,
    i_hold_theirs: Option<bool>,
    they_hold_mine: Option<bool>,
    incoming: Option<Incoming>,
    checking: Option<GameHash>,
    building: Option<rounds::Building>,
    early: Vec<PlayMsg>,
    sending: Option<usize>,
    acked: Option<(usize, Instant)>,
    start: Option<Start>,
    engine: Option<Engine>,
    seen: Seen,
    rejoin: Option<DropHistory>,
    their_rejoin: Option<citrine_gb_net::RejoinPoint>,
    their_user: Option<citrine_gb_net::DsUser>,
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
            checking: None,
            building: None,
            early: Vec::new(),
            sending: None,
            acked: None,
            start: None,
            engine: None,
            seen: Seen::default(),
            rejoin: None,
            their_rejoin: None,
            their_user: None,
        }
    }

    fn rollback(&self) -> Option<&Rollback> {
        match &self.engine {
            Some(Engine::Rollback(r)) => Some(r.get()),
            _ => None,
        }
    }

    fn shown(&self) -> Option<Shown<'_>> {
        match &self.engine {
            Some(Engine::Rollback(r)) => {
                Some(r.home().map_or(Shown::Away(&self.seen), Shown::Home))
            }
            _ => None,
        }
    }

    fn rollback_mut(&mut self) -> Option<&mut Rollback> {
        match &mut self.engine {
            Some(Engine::Rollback(r)) => Some(r.get_mut()),
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
    lanes: usize,
    input_delay: Option<u32>,
    role: Role,
    stage: Stage,
    borrowing: bool,
    downloading: bool,
    borrowed_save: Option<Vec<u8>>,
    lent: bool,
    round: Round,
    roms: Vec<(GameHash, Arc<[u8]>)>,
    wants_state: bool,
    returned: Option<Returned>,
    pings: VecDeque<(u32, Instant)>,
    next_ping: u32,
    round_trips: VecDeque<u32>,
    #[cfg(test)]
    spread_for_test: Option<u32>,
    pinged_at: Option<Instant>,
    started_at: Instant,
    heard_at: Instant,
    let_watch: bool,
    friend_lets_watch: bool,
    watch_friend: bool,
    watch_chosen: bool,
    my_cable: bool,
    friend_cable: bool,
    saves: Vec<LinkedSave>,
    sharing: Option<u32>,
    outbox: Vec<PlayMsg>,
    game_outbox: Vec<GamePart>,
    sealer: Option<Arc<PeerSealer>>,
    sealing: VecDeque<Pending<Vec<Sealed>>>,
    sealed: Vec<Sealed>,
    pieces: Pieces,
    last_piece: u32,
    sample_rate: u32,
    grouped: bool,
    stashed: Option<Box<Rollback>>,
    outbox_mark: Option<usize>,
    parked: VecDeque<PlayMsg>,
    failure: Option<String>,
    touch: Option<citrine_gb::console::Touch>,
    ds_user: citrine_gb_net::DsUser,
}

pub struct Play {
    invites: Vec<IncomingInvite>,
    room: Option<Room>,
    finder: shelf::Finder,
    seen_invites: bool,
    input_delay: Option<u32>,
    ds_user: citrine_gb_net::DsUser,
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
            finder: shelf::Finder::new(shelf),
            seen_invites: true,
            input_delay: None,
            ds_user: random_ds_user(),
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

    pub fn place_engines(&mut self, threads: Option<&citrine_gb_threads::Threads>) {
        if let Some(room) = &mut self.room {
            room.place_engines(threads);
        }
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

    pub fn holds(&self, hash: &GameHash) -> Option<bool> {
        match self.find(hash) {
            Look::Found(_) => Some(true),
            Look::Missing => Some(false),
            Look::Looking => None,
        }
    }

    pub fn holds_ds_game(&self, code: [u8; 4]) -> bool {
        self.finder.holds_ds_game(code)
    }

    pub fn find(&self, hash: &GameHash) -> Look {
        self.finder.known(hash)
    }

    pub fn set_shelf(&mut self, shelf: Box<dyn Shelf>) {
        self.finder = shelf::Finder::new(shelf);
    }

    pub fn set_input_delay(&mut self, frames: Option<u32>) {
        self.input_delay = frames;
    }

    pub fn ds_mac(&self) -> [u8; 6] {
        self.ds_user.mac
    }

    pub fn set_ds_user(&mut self, user: citrine_gb_net::DsUser) {
        if user == self.ds_user {
            return;
        }
        if let Some(room) = &mut self.room {
            room.set_ds_user(&user);
        }
        self.ds_user = user;
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
        let sharing = sharing_of(online);
        let mut play = PlaySession::new(friend, Role::Host, Stage::Inviting, sharing);
        play.ds_user = self.ds_user.clone();
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
        if online.protocol() < citrine_gb_net::ROOMS_PROTOCOL
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
        let sharing = sharing_of(online);
        let invite = &self.invites[index];
        let downloading = matches!(game, MyGame::Download);
        let (card, file, borrowing, save) = match game {
            MyGame::Own(file) => match file.card() {
                Some(card) => (Some(card), Some(file), false, None),
                None => return false,
            },
            MyGame::Borrow { save } if sharing.is_some() => match &invite.game {
                Some(game) => (Some(game.clone()), None, true, save),
                None => return false,
            },
            MyGame::Download if sharing.is_some() => match &invite.game {
                Some(game) if game.console.is_ds() => (Some(game.clone()), None, true, None),
                _ => return false,
            },
            MyGame::Borrow { .. } | MyGame::Download => return false,
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
        play.ds_user = self.ds_user.clone();
        play.id = Some(session);
        play.borrowing = borrowing;
        play.downloading = downloading;
        play.borrowed_save = save;
        play.round = Round::new(1, file, card);
        self.room = Some(Room::new(play));
        true
    }

    pub fn refuse(&mut self, online: &mut Online, session: SessionId, why: Refusal) -> bool {
        let known = self.invites.iter().any(|i| i.session == session);
        self.invites.retain(|i| i.session != session);
        known && online.send(ClientMsg::Refuse { session, why })
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

    pub fn update(&mut self, online: &mut Online, threads: &Threads, now: Instant) {
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
        let sharing = sharing_of(online);
        if let Some(room) = &mut self.room {
            let me = online.account().map(|a| a.online_id);
            room.set_input_delay(self.input_delay);
            room.step(&mut self.finder, threads, sharing, me, now);
            room.tell(online);
        }
        for game in self.invites.iter().filter_map(|i| i.game.as_ref()) {
            self.finder.look(&game.hash);
        }
        self.finder.run(threads);
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
        let finder = &mut self.finder;
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
        let play = room.pair_at(index);
        let Ok(opened) = play.pieces.open(keys, &key, sealed) else {
            return;
        };
        play.heard_at = now;
        let friend = play.friend;
        if let Some(msg) = opened
            && let Some(msg) = room.hear_watch(friend, msg)
            && let Some(msg) = room.hear_group(friend, msg)
        {
            room.pair_at(index).receive(msg, finder, now);
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
        let finder = &mut self.finder;
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
        if let Ok(part) = GamePart::open(keys, &key, sealed) {
            let play = room.pair_at(index);
            play.heard_at = now;
            play.receive_part(part, finder);
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
                        EndReason::NoCopy => Ending::NoCopy,
                        EndReason::OtherCopy => Ending::OtherCopy,
                        EndReason::NoAnswer => Ending::NoAnswer,
                    });
                }
            }
            ServerMsg::RoomJoined { room: id, members } => {
                let Some(me) = online.account().map(|a| a.online_id) else {
                    return;
                };
                let sharing = sharing_of(online);
                if let Some(room) = self.live_room(id) {
                    room.joined(me, members, sharing, now);
                }
            }
            ServerMsg::MemberUpdate { room: id, member } => {
                let sharing = sharing_of(online);
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

pub fn shares_games(online: &Online) -> bool {
    sharing_of(online).is_some()
}

fn sharing_of(online: &Online) -> Option<u32> {
    let on = online.info().is_some_and(|i| i.game_sharing);
    let paced = online.protocol() >= citrine_gb_net::GAME_BUCKET_PROTOCOL;
    on.then_some(if paced {
        citrine_gb_net::MAX_GAME_BYTES
    } else {
        citrine_gb_net::UNPACED_GAME_BYTES
    })
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

pub fn console_of(kind: ConsoleKind, rom: &[u8]) -> Console {
    match kind {
        ConsoleKind::Gb => Console::Gb,
        ConsoleKind::Gbc => Console::Gbc,
        ConsoleKind::Gba => Console::Gba,
        ConsoleKind::Nds => Console::Nds {
            code: rom
                .get(DS_CODE)
                .and_then(|code| code.try_into().ok())
                .unwrap_or_default(),
        },
    }
}

pub fn kind_of(console: Console) -> ConsoleKind {
    match console {
        Console::Gb => ConsoleKind::Gb,
        Console::Gbc => ConsoleKind::Gbc,
        Console::Gba => ConsoleKind::Gba,
        Console::Nds { .. } => ConsoleKind::Nds,
    }
}

pub fn plays_together(card: &GameCard) -> bool {
    cfg!(feature = "nds") || !card.console.is_ds()
}

pub fn refusal(
    card: &GameCard,
    mine: Option<&GameCard>,
    other_copy: bool,
    sharing: bool,
) -> Option<Refusal> {
    if sharing || !card.console.is_ds() || mine.is_some_and(|m| m.hash == card.hash) {
        return None;
    }
    let same_game = mine.is_some_and(|m| m.console.same_ds_game(card.console));
    Some(match same_game || other_copy {
        true => Refusal::OtherCopy,
        false => Refusal::NoCopy,
    })
}

fn unix_now() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn random_ds_user() -> citrine_gb_net::DsUser {
    let mut mac = [0x00, 0x09, 0xBF, 0, 0, 0];
    mac[3..].copy_from_slice(&random_seed().to_le_bytes()[..3]);
    ds_user_with(mac)
}

pub fn ds_user_with(mac: [u8; 6]) -> citrine_gb_net::DsUser {
    citrine_gb_net::DsUser {
        nickname: "Citrine".into(),
        message: String::new(),
        color: 0,
        birthday: (1, 1),
        language: 1,
        mac,
    }
}

fn random_seed() -> u64 {
    let mut bytes = [0; 8];
    let _ = getrandom::getrandom(&mut bytes);
    u64::from_le_bytes(bytes)
}

pub fn sha1_of(rom: &[u8]) -> Option<GameHash> {
    (!rom.is_empty()).then(|| citrine_gb::rom::id::game_sha1(rom))
}

fn packed(origin: Origin) -> Origin {
    match origin {
        Origin::Resume(state) => Origin::Resume(citrine_gb_net::pack(&state, true)),
        boot => boot,
    }
}

fn unpacked(origin: Origin, console: Option<Console>) -> Option<Origin> {
    let limit = console.map_or(MAX_STATE_BYTES, Console::max_state_bytes);
    match origin {
        Origin::Resume(state) => citrine_gb_net::unpack(&state, limit)
            .ok()
            .map(Origin::Resume),
        boot => Some(boot),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests {
    use super::*;
    use crate::online::address::ServerAddress;
    use crate::online::transport::{self, Connector, Transport, TransportEvent};
    use crate::online::{Phase, Trust};
    use citrine_gb::console::{Console, Keys, Rollback, Touch};
    use citrine_gb::gb::{GameBoy, GbModel};
    use citrine_gb::netplay::{
        Advance, ConsoleSetup, Input, Player, Session, SessionConfig, encode_state,
    };
    use citrine_gb::rom::Rom;
    use citrine_gb_net::{
        ErrorCode, FriendState, GAME_PART_BYTES, GAME_WINDOW_BYTES, InviteCode, Mode, NONCE_BYTES,
        Sealed,
    };
    use citrine_gb_server::{Config, Running, start};
    use citrine_gb_threads::Threads;
    use std::collections::HashMap;

    pub(crate) const SNAKE: &[u8] = include_bytes!("../homebrew/snake.gb");
    const PASSWORD: &str = "lemon tree";
    const FRAME: Duration = Duration::from_nanos(16_742_706);
    const SETTLE: Duration = Duration::from_secs(60);

    const PAD: [Keys; 8] = [
        Keys::empty(),
        Keys::LEFT,
        Keys::RIGHT,
        Keys::UP,
        Keys::DOWN,
        Keys::A,
        Keys::B,
        Keys::A.union(Keys::RIGHT),
    ];

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    type Meter = std::rc::Rc<std::cell::RefCell<Vec<(Instant, usize)>>>;
    type Cut = std::rc::Rc<std::cell::Cell<bool>>;
    type Now = std::rc::Rc<std::cell::Cell<Instant>>;
    type Pace = std::rc::Rc<std::cell::Cell<u32>>;
    type Nonce = [u8; NONCE_BYTES];
    type SharedWire = std::rc::Rc<std::cell::RefCell<Wire>>;
    type Presses = Vec<(u32, u32, Keys)>;
    type Touches = Vec<(u32, u32, Touch)>;

    const LOSS_WAIT: Duration = Duration::from_secs(1);

    #[derive(Default)]
    struct Wire {
        in_flight: Vec<(usize, Nonce, Instant)>,
        lossy: Vec<bool>,
    }

    impl Wire {
        fn join(&mut self) -> usize {
            self.lossy.push(false);
            self.lossy.len() - 1
        }

        fn reopened(&mut self, side: usize) {
            self.in_flight.retain(|&(from, ..)| from == side);
            self.lossy[side] = false;
        }

        fn awaits(&self, side: usize, now: Instant) -> bool {
            !self.lossy[side]
                && self
                    .in_flight
                    .iter()
                    .any(|&(from, _, handed)| from != side && handed < now)
        }

        fn arrived(&mut self, side: usize, nonce: &Nonce) -> Option<Instant> {
            let at = self
                .in_flight
                .iter()
                .position(|(from, sent, _)| *from != side && sent == nonce)?;
            self.lossy[side] = false;
            Some(self.in_flight.remove(at).2)
        }

        fn lost(&mut self, side: usize, now: Instant) {
            self.in_flight
                .retain(|&(from, _, handed)| from == side || handed >= now);
            self.lossy[side] = true;
        }
    }

    fn relayed_nonce(msg: &ServerMsg) -> Option<&Nonce> {
        match msg {
            ServerMsg::Play { sealed, .. }
            | ServerMsg::Game { sealed, .. }
            | ServerMsg::RoomPlay { sealed, .. }
            | ServerMsg::RoomGame { sealed, .. } => Some(&sealed.nonce),
            _ => None,
        }
    }

    struct Lag {
        inner: Box<dyn Transport>,
        clock: Now,
        meter: Meter,
        cut: Cut,
        down: Cut,
        pace: Pace,
        wire: Option<(SharedWire, usize)>,
        closed: bool,
        half: Duration,
        jitter_ms: u64,
        rng: Rng,
        out: VecDeque<(Instant, Vec<u8>, Option<Nonce>)>,
        inbound: VecDeque<(Instant, TransportEvent)>,
        last_out: Instant,
        last_in: Instant,
    }

    impl Lag {
        fn due(&mut self, last: Instant, now: Instant) -> Instant {
            let jitter = Duration::from_millis(self.rng.below(self.jitter_ms + 1));
            (now + self.half + jitter).max(last)
        }

        fn on_the_wire(&self, event: &TransportEvent) -> Duration {
            match (event, self.pace.get()) {
                (TransportEvent::Message(bytes), per_frame) if per_frame > 0 => {
                    FRAME * bytes.len() as u32 / per_frame
                }
                _ => Duration::ZERO,
            }
        }

        fn take_in(&mut self, event: TransportEvent, now: Instant) {
            let handed = match (&event, &self.wire) {
                (TransportEvent::Message(bytes), Some((wire, side))) => ServerMsg::decode(bytes)
                    .ok()
                    .as_ref()
                    .and_then(relayed_nonce)
                    .and_then(|nonce| wire.borrow_mut().arrived(*side, nonce)),
                _ => None,
            };
            let reached = handed.map_or(now, |handed| handed + FRAME);
            let due = self.due(self.last_in, reached) + self.on_the_wire(&event);
            self.last_in = due;
            self.inbound.push_back((due, event));
        }

        fn drain(&mut self, now: Instant) {
            let began = std::time::Instant::now();
            loop {
                for _ in 0..256 {
                    let Some(event) = self.inner.poll() else {
                        break;
                    };
                    self.take_in(event, now);
                }
                let Some((wire, side)) = &self.wire else {
                    return;
                };
                if !wire.borrow().awaits(*side, now) {
                    return;
                }
                if began.elapsed() > LOSS_WAIT {
                    wire.borrow_mut().lost(*side, now);
                    return;
                }
                std::thread::sleep(Duration::from_micros(100));
            }
        }
    }

    impl Transport for Lag {
        fn send(&mut self, bytes: Vec<u8>) {
            if self.closed {
                return;
            }
            let now = self.clock.get();
            let nonce = match ClientMsg::decode(&bytes) {
                Ok(
                    ClientMsg::Play { sealed, .. }
                    | ClientMsg::Game { sealed, .. }
                    | ClientMsg::RoomPlay { sealed, .. }
                    | ClientMsg::RoomGame { sealed, .. },
                ) => {
                    self.meter.borrow_mut().push((now, sealed.ciphertext.len()));
                    Some(sealed.nonce)
                }
                _ => None,
            };
            let due = self.due(self.last_out, now);
            self.last_out = due;
            self.out.push_back((due, bytes, nonce));
        }

        fn poll(&mut self) -> Option<TransportEvent> {
            if self.closed {
                return None;
            }
            if self.cut.replace(false) || self.down.get() {
                self.closed = true;
                return Some(TransportEvent::Closed);
            }
            let now = self.clock.get();
            while self.out.front().is_some_and(|(due, ..)| *due <= now) {
                if let Some((_, bytes, nonce)) = self.out.pop_front() {
                    if let (Some(nonce), Some((wire, side))) = (nonce, &self.wire) {
                        wire.borrow_mut().in_flight.push((*side, nonce, now));
                    }
                    self.inner.send(bytes);
                }
            }
            self.drain(now);
            match self.inbound.front() {
                Some((due, _)) if *due <= now => self.inbound.pop_front().map(|(_, event)| event),
                _ => None,
            }
        }
    }

    fn lagged(
        one_way_ms: u64,
        jitter_ms: u64,
        seed: u64,
        meter: Meter,
        [cut, down]: [Cut; 2],
        pace: Pace,
        hookup: Hookup,
    ) -> Connector {
        let mut websocket = transport::websocket();
        let mut opened = 0;
        Box::new(move |url| {
            opened += 1;
            let now = hookup.clock.get();
            if let Some((wire, side)) = &hookup.wire {
                wire.borrow_mut().reopened(*side);
            }
            Box::new(Lag {
                inner: websocket(url),
                clock: hookup.clock.clone(),
                meter: meter.clone(),
                cut: cut.clone(),
                down: down.clone(),
                pace: pace.clone(),
                wire: hookup.wire.clone(),
                closed: false,
                half: Duration::from_millis(one_way_ms) / 2,
                jitter_ms: jitter_ms / 2,
                rng: Rng(seed + opened),
                out: VecDeque::new(),
                inbound: VecDeque::new(),
                last_out: now,
                last_in: now,
            })
        })
    }

    struct Hookup {
        clock: Now,
        wire: Option<(SharedWire, usize)>,
    }

    struct Clock {
        now: Now,
        next: Instant,
        wire: Option<SharedWire>,
    }

    impl Clock {
        fn new() -> Self {
            let now = Instant::now();
            Self {
                now: Now::new(now.into()),
                next: now,
                wire: None,
            }
        }

        fn wired() -> Self {
            Self {
                wire: Some(SharedWire::default()),
                ..Self::new()
            }
        }

        fn hookup(&self) -> Hookup {
            Hookup {
                clock: self.now.clone(),
                wire: self.wire.clone().map(|wire| {
                    let side = wire.borrow_mut().join();
                    (wire, side)
                }),
            }
        }

        fn now(&self) -> Instant {
            self.now.get()
        }

        fn tick(&mut self) -> Instant {
            self.tick_by(FRAME)
        }

        fn tick_by(&mut self, step: Duration) -> Instant {
            self.next += step;
            let real = Instant::now();
            if self.next > real {
                std::thread::sleep(self.next - real);
            } else {
                self.next = real;
            }
            self.now.set(self.now.get() + step);
            self.now.get()
        }

        fn waited(&self, since: Instant, what: &str) {
            assert!(
                self.now().duration_since(since) < SETTLE,
                "timed out waiting until {what}"
            );
        }
    }

    struct Relay {
        running: Option<Running>,
        runtime: tokio::runtime::Runtime,
        address: ServerAddress,
        _dir: tempfile::TempDir,
    }

    impl Relay {
        fn start(sharing: bool) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mut vars: HashMap<&str, String> = HashMap::from([
                ("CITRINE_BIND", "127.0.0.1".into()),
                ("CITRINE_PORT", "0".into()),
                ("CITRINE_DISK_FLOOR_MB", "0".into()),
                ("CITRINE_DISK_FLOOR_PERCENT", "0".into()),
                (
                    "CITRINE_DATA_DIR",
                    dir.path().join("data").display().to_string(),
                ),
                (
                    "CITRINE_STATIC_DIR",
                    dir.path().join("static").display().to_string(),
                ),
                ("CITRINE_RELAY", "true".into()),
                ("CITRINE_ROOM_GRACE_SECS", "10".into()),
                ("CITRINE_IP_UNSIGNED", "8".into()),
                ("CITRINE_SIGN_UPS_PER_IP", "8".into()),
                ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
                ("CITRINE_WEB", "false".into()),
                (
                    "CITRINE_GAME_SHARING",
                    if sharing { "on" } else { "off" }.into(),
                ),
            ]);
            if let Ok(ip) = std::env::var("CITRINE_R4_PUBLIC_IP") {
                vars.insert("CITRINE_DS_NET_PUBLIC_IP", ip);
                vars.insert("CITRINE_DS_NET_STUN", "stun.l.google.com:19302".into());
            }
            let config = Config::from_vars(|k| vars.get(k).cloned()).unwrap();
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let running = runtime.block_on(start(config)).unwrap();
            let address =
                ServerAddress::parse(&format!("ws://127.0.0.1:{}", running.addr.port())).unwrap();
            Self {
                running: Some(running),
                runtime,
                address,
                _dir: dir,
            }
        }
    }

    impl Relay {
        fn sql(&self, sql: &'static str) {
            let path = self._dir.path().join("data").join("citrine.db");
            self.runtime.block_on(async {
                use sqlx::ConnectOptions;
                let mut conn = sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(path)
                    .connect()
                    .await
                    .unwrap();
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            });
        }

        fn invites(&self, required: bool) -> [Option<InviteCode>; 2] {
            let Some(running) = self.running.as_ref().filter(|_| required) else {
                return [None, None];
            };
            self.runtime.block_on(async {
                running.require_invites().await.unwrap();
                [
                    Some(running.create_invite().await.unwrap()),
                    Some(running.create_invite().await.unwrap()),
                ]
            })
        }
    }

    impl Drop for Relay {
        fn drop(&mut self) {
            if let Some(running) = self.running.take() {
                self.runtime.block_on(running.stop());
            }
        }
    }

    struct Nothing;

    impl Shelf for Nothing {
        fn find(&self, _hash: &GameHash) -> Option<std::sync::Arc<[u8]>> {
            None
        }
    }

    struct Watched {
        rom: std::sync::Arc<[u8]>,
        looked: std::sync::mpsc::Sender<std::thread::ThreadId>,
    }

    impl Shelf for Watched {
        fn find(&self, hash: &GameHash) -> Option<std::sync::Arc<[u8]>> {
            let _ = self.looked.send(std::thread::current().id());
            (sha1_of(&self.rom) == Some(*hash)).then(|| self.rom.clone())
        }
    }

    fn watched(
        rom: std::sync::Arc<[u8]>,
    ) -> (Watched, std::sync::mpsc::Receiver<std::thread::ThreadId>) {
        let (looked, lookups) = std::sync::mpsc::channel();
        (Watched { rom, looked }, lookups)
    }

    fn off_this_thread(lookups: &std::sync::mpsc::Receiver<std::thread::ThreadId>) -> bool {
        let here = std::thread::current().id();
        let threads: Vec<_> = lookups.try_iter().collect();
        !threads.is_empty() && threads.iter().all(|t| *t != here)
    }

    pub(crate) struct Side {
        pub(crate) online: Online,
        meter: Meter,
        cut: Cut,
        down: Cut,
        pace: Pace,
        pub(crate) play: Play,
        threads: Threads,
        rng: Rng,
        held: Keys,
        held_for: u64,
        pressed: u32,
        cable_gb: Option<GameBoy>,
        cable_frames: u32,
        received: bool,
        arrived: Vec<(u32, u32)>,
        sent: bool,
        presses_start: bool,
        own: Option<GameBoy>,
        #[cfg(feature = "gba")]
        own_gba: Option<Box<citrine_gb::gba::machine::Gba>>,
        #[cfg(feature = "nds")]
        own_nds: Option<Box<citrine_gb::nds::machine::Nds>>,
        gave: Option<Vec<u8>>,
        runs_own: bool,
        start_only: bool,
        shoulders: bool,
        history: Option<DropHistory>,
        handed_back: Option<(OnlineId, bool)>,
        padding: Vec<u8>,
        state_progress: Vec<(u64, u64)>,
        script: Option<Presses>,
        touches: Touches,
        runs: Vec<bool>,
        #[cfg(feature = "nds")]
        app: Option<Box<AppLoop>>,
    }

    impl Side {
        fn new(lag: (u64, u64), seed: u64, shelf: Box<dyn Shelf>, clock: &Clock) -> Self {
            let meter = Meter::default();
            let (cut, down) = (Cut::default(), Cut::default());
            let line = [cut.clone(), down.clone()];
            let pace = Pace::default();
            Self {
                online: Online::new(lagged(
                    lag.0,
                    lag.1,
                    seed,
                    meter.clone(),
                    line,
                    pace.clone(),
                    clock.hookup(),
                )),
                meter,
                cut,
                down,
                pace,
                play: Play::new(shelf),
                threads: Threads::new(citrine_gb_threads::Mode::from_env()),
                rng: Rng(seed),
                held: Keys::empty(),
                held_for: 0,
                pressed: 0,
                cable_gb: None,
                cable_frames: 0,
                received: false,
                arrived: Vec::new(),
                sent: false,
                presses_start: false,
                own: None,
                #[cfg(feature = "gba")]
                own_gba: None,
                #[cfg(feature = "nds")]
                own_nds: None,
                gave: None,
                runs_own: false,
                start_only: false,
                shoulders: false,
                history: None,
                handed_back: None,
                padding: Vec::new(),
                state_progress: Vec::new(),
                script: None,
                touches: Vec::new(),
                runs: Vec::new(),
                #[cfg(feature = "nds")]
                app: None,
            }
        }

        pub(crate) fn id(&self) -> OnlineId {
            self.online.account().unwrap().online_id
        }

        pub(crate) fn session(&self) -> &PlaySession {
            self.play.session().unwrap()
        }

        pub(crate) fn stage(&self) -> Option<Stage> {
            self.play.session().map(|s| s.stage())
        }

        fn frame(&self) -> u32 {
            self.play.session().map_or(0, |s| s.frame())
        }

        fn whereabouts(&self) -> String {
            format!(
                "{:?}, {:?} at frame {}, live {}",
                self.online.phase(),
                self.stage(),
                self.frame(),
                self.play.is_live()
            )
        }

        fn running(&mut self, file: GameFile) -> GameFile {
            let mut gb = GameBoy::new_empty(GbModel::try_from(file.kind).unwrap());
            gb.load_rom(&Rom::new(&file.rom)).unwrap();
            for _ in 0..90 {
                gb.run_frame();
            }
            self.own = Some(gb);
            GameFile {
                running: true,
                ..file
            }
        }

        fn own_state(&self) -> Option<Vec<u8>> {
            if let Some(gb) = &self.own {
                let mut state = encode_state(gb).ok()?;
                state.extend_from_slice(&self.padding);
                return Some(state);
            }
            #[cfg(feature = "gba")]
            if let Some(gba) = self.own_gba.as_deref() {
                return encode_state(gba).ok();
            }
            #[cfg(feature = "nds")]
            if let Some(nds) = self.own_nds.as_deref() {
                return encode_state(nds).ok();
            }
            None
        }

        fn session_mut(&mut self) -> &mut PlaySession {
            self.play.session_mut().unwrap()
        }

        fn buttons(&mut self) -> Keys {
            let pad = self.pad_buttons();
            match self.shoulders && self.pressed % 40 < 10 {
                true => pad | Keys::L | Keys::R,
                false => pad,
            }
        }

        fn pad_buttons(&mut self) -> Keys {
            self.pressed += 1;
            if let Some(script) = &self.script {
                let frame = self.frame();
                return script
                    .iter()
                    .filter(|(from, until, _)| (*from..*until).contains(&frame))
                    .fold(Keys::empty(), |keys, (_, _, more)| keys | *more);
            }
            if self.start_only {
                return match self.presses_start && (30..35).contains(&(self.frame() % 60)) {
                    true => Keys::START,
                    false => Keys::empty(),
                };
            }
            if self.presses_start && self.pressed % 90 < 5 {
                return Keys::START;
            }
            if self.held_for == 0 {
                self.held = PAD[self.rng.below(PAD.len() as u64) as usize];
                self.held_for = 1 + self.rng.below(24);
            }
            self.held_for -= 1;
            self.held
        }

        fn step(&mut self, now: Instant) {
            self.play.place_engines(Some(&self.threads));
            self.online.update(now);
            self.play.update(&mut self.online, &self.threads, now);
            if let Some(back) = self.play.session_mut().and_then(PlaySession::take_returned) {
                self.handed_back = Some((back.friend, back.paused));
                self.history = back.history;
                self.own = match back.console {
                    #[cfg(feature = "gba")]
                    Machine::Gba(gba) => {
                        self.own_gba = Some(gba);
                        None
                    }
                    #[cfg(feature = "nds")]
                    Machine::Nds(nds) => {
                        self.own_nds = Some(nds);
                        None
                    }
                    console => game_boy(console),
                };
            }
            if self.play.session().is_some_and(PlaySession::wants_state)
                && let Some(state) = self.own_state()
            {
                self.gave = Some(state.clone());
                let history = self.history.take();
                self.session_mut().give_state(state, history);
            }
            let buttons = self.buttons();
            if self.runs_own
                && !self.play.is_live()
                && let Some(gb) = &mut self.own
            {
                gb.release_button(buttons.complement().into());
                gb.press_button(buttons.into());
                gb.run_frame();
            }
            if let Some(play) = self.play.session_mut()
                && play.stage() == Stage::Playing
            {
                match play.mode() {
                    Some(Mode::Rollback) => {
                        if !self.touches.is_empty() {
                            let frame = play.frame();
                            play.set_touch(
                                self.touches
                                    .iter()
                                    .find(|(from, until, _)| (*from..*until).contains(&frame))
                                    .map(|(_, _, touch)| *touch),
                            );
                        }
                        #[cfg(feature = "nds")]
                        if let Some(app) = &mut self.app {
                            app.update(play, now);
                        } else {
                            let ran = play.run_frame(buttons, now);
                            self.runs.push(ran);
                        }
                        #[cfg(not(feature = "nds"))]
                        {
                            let ran = play.run_frame(buttons, now);
                            self.runs.push(ran);
                        }
                    }
                    Some(Mode::Cable) => {
                        if let Some(gb) = &mut self.cable_gb {
                            play.pump_link(gb);
                            gb.run_frame();
                            self.cable_frames += 1;
                        }
                    }
                    Some(Mode::Apart) | None => {}
                }
            }
            if let Some(progress) = self.play.session().and_then(PlaySession::state_received) {
                self.state_progress.push(progress);
            }
            if let Some(Stage::Receiving { got, total }) = self.stage() {
                self.received = true;
                self.arrived.push((got, total));
            }
            self.sent |= self.play.session().is_some_and(PlaySession::sends_game);
            self.play.tick_watch(now);
            self.play.flush(&mut self.online);
        }
    }

    pub(crate) struct Pair {
        pub(crate) host: Side,
        pub(crate) guest: Side,
        relay: Relay,
        clock: Clock,
    }

    impl Pair {
        pub(crate) fn new(
            sharing: bool,
            one_way_ms: u64,
            jitter_ms: u64,
            shelves: [Box<dyn Shelf>; 2],
        ) -> Self {
            let relay = Relay::start(sharing);
            let [host_shelf, guest_shelf] = shelves;
            let clock = Clock::wired();
            let mut pair = Self {
                host: Side::new((one_way_ms, jitter_ms), 11, host_shelf, &clock),
                guest: Side::new((one_way_ms, jitter_ms), 29, guest_shelf, &clock),
                relay,
                clock,
            };
            pair.guest.presses_start = true;
            let [host_code, guest_code] = pair.relay.invites(sharing);
            let now = pair.clock.now();
            for side in [&mut pair.host, &mut pair.guest] {
                side.online.probe(
                    pair.relay.address.clone(),
                    PASSWORD.into(),
                    Trust::default(),
                    now,
                );
            }
            pair.until("both probed", |p| {
                p.host.online.phase() == Phase::Probed && p.guest.online.phase() == Phase::Probed
            });
            let now = pair.clock.now();
            pair.host
                .online
                .create_invited_account("Clara".into(), host_code, now);
            pair.guest
                .online
                .create_invited_account("Mia".into(), guest_code, now);
            pair.until("both signed up", |p| {
                p.host.online.is_connected()
                    && p.guest.online.is_connected()
                    && p.host.online.account().is_some()
                    && p.guest.online.account().is_some()
            });
            let (host_id, guest_id) = (pair.host.id(), pair.guest.id());
            assert!(pair.host.online.add_friend(guest_id));
            pair.until("the request arrives", |p| {
                p.guest.online.friend(host_id).map(|f| f.state) == Some(FriendState::Incoming)
            });
            assert!(pair.guest.online.answer(host_id, true));
            pair.until("both are friends", |p| {
                p.host.online.friend(guest_id).map(|f| f.state) == Some(FriendState::Friend)
                    && p.guest.online.friend(host_id).map(|f| f.state) == Some(FriendState::Friend)
            });
            pair
        }

        pub(crate) fn relay_sql(&self, sql: &'static str) {
            self.relay.sql(sql);
        }

        fn pace(&mut self, bytes_per_frame: u32) {
            self.host.pace.set(bytes_per_frame);
            self.guest.pace.set(bytes_per_frame);
        }

        pub(crate) fn cut(&mut self) {
            self.host.cut.set(true);
            self.guest.cut.set(true);
        }

        pub(crate) fn tick(&mut self) {
            let now = self.clock.tick();
            self.host.step(now);
            self.guest.step(now);
        }

        #[cfg(feature = "nds")]
        fn tick_by(&mut self, step: Duration) {
            let now = self.clock.tick_by(step);
            self.host.step(now);
            self.guest.step(now);
        }

        pub(crate) fn stored_messages(&self) -> u64 {
            let running = self.relay.running.as_ref().unwrap();
            self.relay
                .runtime
                .block_on(running.stored_messages())
                .unwrap()
        }

        pub(crate) fn until(&mut self, what: &str, mut done: impl FnMut(&Pair) -> bool) {
            let begun = self.clock.now();
            while !done(self) {
                assert!(
                    self.clock.now().duration_since(begun) < SETTLE,
                    "timed out waiting until {what}: host {}, guest {}",
                    self.host.whereabouts(),
                    self.guest.whereabouts()
                );
                self.tick();
            }
        }

        pub(crate) fn invite(&mut self, game: GameFile) -> SessionId {
            self.invite_with(Some(game))
        }

        pub(crate) fn invite_with(&mut self, game: Option<GameFile>) -> SessionId {
            let guest_id = self.guest.id();
            assert!(self.host.play.invite(&mut self.host.online, guest_id, game));
            self.until("the invite arrives", |p| !p.guest.play.invites().is_empty());
            self.guest.play.invites()[0].session
        }

        fn run_host_alone(&mut self, frames: u32) {
            for _ in 0..frames {
                let now = self.clock.tick();
                self.host.step(now);
            }
        }

        fn run_guest_alone(&mut self, frames: u32) {
            for _ in 0..frames {
                let now = self.clock.tick();
                self.guest.step(now);
            }
        }

        pub(crate) fn accept(&mut self, session: SessionId, game: MyGame) {
            assert!(
                self.guest
                    .play
                    .accept(&mut self.guest.online, session, game)
            );
            self.until("both are playing", |p| {
                p.host.stage() == Some(Stage::Playing) && p.guest.stage() == Some(Stage::Playing)
            });
        }

        pub(crate) fn until_checked(&mut self) {
            self.until("both logged three checks", |p| {
                [&p.host, &p.guest].iter().all(|side| {
                    side.session()
                        .log()
                        .is_some_and(|log| log.checks.len() >= 3)
                })
            });
        }

        pub(crate) fn play_frames(&mut self, frames: u32) {
            let target = self.host.frame().max(self.guest.frame()) + frames;
            self.until("both ran the frames", |p| {
                p.host.frame() >= target && p.guest.frame() >= target
            });
        }
    }

    fn game_boy(machine: Machine) -> Option<GameBoy> {
        match machine {
            Machine::Gb(gb) => Some(*gb),
            #[cfg(feature = "gba")]
            Machine::Gba(_) => None,
            #[cfg(feature = "nds")]
            Machine::Nds(_) => None,
            #[cfg(not(feature = "nds"))]
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    fn game(rom: &[u8], title: &str) -> GameFile {
        GameFile::new(
            rom.into(),
            sha1_of(rom).unwrap(),
            title.into(),
            GbModel::for_rom(&Rom::new(rom)).unwrap().into(),
        )
    }

    pub(crate) fn snake() -> GameFile {
        game(SNAKE, "Snake")
    }

    fn custom() -> GameFile {
        let mut rom = SNAKE.to_vec();
        if let Some(last) = rom.last_mut() {
            *last ^= 0xFF;
        }
        game(&rom, "My Snake")
    }

    fn hash_of(file: &GameFile) -> GameHash {
        file.card().unwrap().hash
    }

    pub(crate) fn homebrew() -> [Box<dyn Shelf>; 2] {
        [Box::new(shelf::Homebrew), Box::new(shelf::Homebrew)]
    }

    pub(crate) fn empty() -> [Box<dyn Shelf>; 2] {
        [Box::new(Nothing), Box::new(Nothing)]
    }

    fn reference_checks(play: &PlaySession, roms: [&[u8]; 2]) -> HashMap<u32, [u64; 2]> {
        let start = play.start().unwrap();
        let log = play.log().unwrap();
        let roms = roms.map(Rom::new);
        let setups = [0, 1].map(|i| {
            Some(ConsoleSetup {
                rom: &roms[i],
                kind: GbModel::for_rom(&roms[i]).unwrap().into(),
                save: None,
                state: None,
            })
        });
        let config = SessionConfig {
            power_on: start.power_on.as_slice().try_into().unwrap(),
            ..SessionConfig::new(Player::One, start.seed, start.start_time)
        };
        let mut session = Session::new(config, setups).unwrap();
        let mut want = HashMap::new();
        let (one, two) = match play.role {
            Role::Host => (&log.local, &log.remote),
            Role::Guest => (&log.remote, &log.local),
        };
        let inputs: Vec<(Input, Input)> = one
            .iter()
            .zip(two)
            .map_while(|(one, two)| one.zip(*two))
            .collect();
        let next = (Input::default(), Input::default());
        for (frame, (one, two)) in inputs.into_iter().chain([next]).enumerate() {
            session.add_remote_input(frame as u32, two);
            session.add_local_input(frame as u32, one).unwrap();
            assert_eq!(session.advance(), Advance::Ran);
            while let Some(check) = session.take_check() {
                want.insert(check.frame, check.hashes);
            }
        }
        want
    }

    fn assert_matches_reference(play: &PlaySession, roms: [&[u8]; 2]) {
        let want = reference_checks(play, roms);
        let log = play.log().unwrap();
        let mut compared = 0;
        for check in &log.checks {
            if let Some(hashes) = want.get(&check.frame) {
                assert_eq!(
                    *hashes, check.hashes,
                    "{:?} check of frame {}, resyncs at {:?}",
                    play.role, check.frame, log.resynced
                );
                compared += 1;
            }
        }
        assert!(compared >= 3, "only {compared} checks to compare");
    }

    fn assert_same_inputs(host: &PlaySession, guest: &PlaySession) {
        let (host, guest) = (host.log().unwrap(), guest.log().unwrap());
        for (sent, got) in [(&host.local, &guest.remote), (&guest.local, &host.remote)] {
            for (frame, (a, b)) in sent.iter().zip(got).enumerate() {
                if let (Some(a), Some(b)) = (a, b) {
                    assert_eq!(a, b, "input of frame {frame}");
                }
            }
        }
    }

    fn plays_snake(one_way_ms: u64, jitter_ms: u64) {
        let mut pair = Pair::new(false, one_way_ms, jitter_ms, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(480);
        pair.until_checked();
        let host = pair.host.session();
        let guest = pair.guest.session();
        assert_eq!(host.mode(), Some(Mode::Rollback));
        let slow = one_way_ms * 2 > u64::from(SLOW_ROUND_TRIP_MS);
        let delay = u32::from(host.delay().unwrap());
        if slow {
            assert!((u32::from(SLOW_DELAY)..=links::MAX_INPUT_DELAY).contains(&delay));
        } else {
            assert_eq!(delay, 0);
        }
        assert_eq!(host.sync().map(|s| s.delay), Some(delay));
        assert_eq!(guest.delay(), host.delay());
        assert_same_inputs(host, guest);
        assert_matches_reference(host, [SNAKE, SNAKE]);
        assert_matches_reference(guest, [SNAKE, SNAKE]);
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert!(host.cable_plugged() && guest.cable_plugged());
    }

    #[test]
    fn snake_on_one_machine_plays_like_a_local_session() {
        plays_snake(0, 0);
    }

    #[test]
    fn snake_at_75_ms_plays_like_a_local_session() {
        plays_snake(75, 20);
    }

    #[test]
    fn snake_at_250_ms_plays_like_a_local_session() {
        plays_snake(250, 40);
    }

    #[test]
    fn l_and_r_pressed_on_one_side_arrive_on_the_other() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        pair.host.shoulders = true;
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(240);
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_same_inputs(host, guest);
        let shoulders = guest
            .log()
            .unwrap()
            .remote
            .iter()
            .flatten()
            .filter(|input| input.buttons.contains(Keys::L | Keys::R))
            .count();
        assert!(shoulders >= 40, "{shoulders} frames with L and R");
    }

    #[test]
    fn the_hosts_input_delay_setting_reaches_both_sessions() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        pair.host.play.set_input_delay(Some(3));
        pair.guest.play.set_input_delay(Some(1));
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(240);
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.delay(), Some(3));
        assert_eq!(guest.delay(), Some(3));
        assert_eq!(host.sync().map(|s| s.delay), Some(3));
        assert_eq!(guest.sync().map(|s| s.delay), Some(3));
        assert_same_inputs(host, guest);
        assert_matches_reference(host, [SNAKE, SNAKE]);
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
    }

    fn joined_pair(guest_runs: bool) -> (Pair, Vec<u8>, Option<Vec<u8>>) {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let mine = pair.host.running(snake());
        let host_before = encode_state(pair.host.own.as_ref().unwrap()).unwrap();
        let session = pair.invite(mine);
        let (theirs, guest_before) = if guest_runs {
            let theirs = pair.guest.running(snake());
            let gb = pair.guest.own.as_mut().unwrap();
            for _ in 0..40 {
                gb.run_frame();
            }
            (theirs, Some(encode_state(gb).unwrap()))
        } else {
            (snake(), None)
        };
        pair.accept(session, MyGame::Own(theirs));
        (pair, host_before, guest_before)
    }

    #[test]
    fn joining_starts_from_both_running_games_as_they_were() {
        let (mut pair, host_before, guest_before) = joined_pair(true);
        assert_ne!(Some(&host_before), guest_before.as_ref());
        assert_eq!(pair.host.gave.as_ref(), Some(&host_before));
        assert_eq!(pair.guest.gave, guest_before);
        let want = [Some(host_before), guest_before];
        for side in [pair.host.session(), pair.guest.session()] {
            assert_eq!(side.start().unwrap().power_on, [0, 0]);
            assert_eq!(side.log().unwrap().started, want);
        }
        pair.play_frames(240);
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    #[test]
    fn a_friend_without_a_running_game_powers_on_beside_the_running_one() {
        let (mut pair, host_before, _) = joined_pair(false);
        assert_eq!(pair.guest.gave, None);
        for side in [pair.host.session(), pair.guest.session()] {
            assert_eq!(side.start().unwrap().power_on, [0, GUEST_POWER_ON]);
            let started = &side.log().unwrap().started;
            assert_eq!(started[0].as_ref(), Some(&host_before));
            assert!(started[1].is_some());
        }
        pair.play_frames(240);
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    #[test]
    fn either_player_plugs_and_pulls_the_cable() {
        let mut pair = Pair::new(false, 40, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(60);
        pair.host.play.session_mut().unwrap().set_cable(false);
        pair.until("the cable is out on both", |p| {
            !p.host.session().cable_plugged() && !p.guest.session().cable_plugged()
        });
        pair.play_frames(60);
        pair.host.play.session_mut().unwrap().set_cable(true);
        pair.until("the cable is in on both", |p| {
            p.host.session().cable_plugged() && p.guest.session().cable_plugged()
        });
        pair.play_frames(60);
        assert_eq!(
            pair.host.session().resyncs() + pair.guest.session().resyncs(),
            0
        );
        pair.until_checked();
        assert_matches_reference(pair.host.session(), [SNAKE, SNAKE]);
    }

    fn transfers_over(pair: &mut Pair, frames: u32) -> [u64; 2] {
        let before = [pair.host.session(), pair.guest.session()].map(PlaySession::cable_transfers);
        pair.play_frames(frames);
        let after = [pair.host.session(), pair.guest.session()].map(PlaySession::cable_transfers);
        [after[0] - before[0], after[1] - before[1]]
    }

    fn linked_snakes() -> Pair {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        for side in [&mut pair.host, &mut pair.guest] {
            side.runs_own = true;
            side.start_only = true;
        }
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair
    }

    fn dropped_after(frames: u32) -> (Pair, [Returned; 2]) {
        let mut pair = linked_snakes();
        let linked = transfers_over(&mut pair, frames);
        assert!(
            linked.iter().all(|&n| n >= 20),
            "before the drop: {linked:?}"
        );
        pair.guest.down.set(true);
        pair.until("both lost the session", |p| {
            !p.host.play.is_live() && !p.guest.play.is_live()
        });
        pair.guest.down.set(false);
        pair.guest.online.retry_now(pair.clock.now());
        let returned = [&mut pair.host, &mut pair.guest].map(|side| {
            assert_eq!(side.stage(), Some(Stage::Ended(Ending::Dropped)));
            side.play.close().and_then(PlaySession::finish).unwrap()
        });
        (pair, returned)
    }

    fn running_snake() -> GameFile {
        GameFile {
            running: true,
            ..snake()
        }
    }

    #[test]
    fn a_dropped_snake_pair_links_again_when_both_come_back() {
        for frames in [300, 900] {
            let (mut pair, returned) = dropped_after(frames);
            for (side, back) in [&mut pair.host, &mut pair.guest].into_iter().zip(returned) {
                side.runs_own = !back.paused;
                side.history = back.history;
                side.own = game_boy(back.console);
            }
            pair.until("the guest is back", |p| p.guest.online.is_connected());
            let session = pair.invite(running_snake());
            pair.accept(session, MyGame::Own(running_snake()));
            let moved = transfers_over(&mut pair, 600);
            assert!(
                moved.iter().all(|&n| n >= 30),
                "after {frames} frames: {moved:?}"
            );
            let (host, guest) = (pair.host.session(), pair.guest.session());
            assert_eq!(host.resyncs() + guest.resyncs(), 0);
            assert_same_inputs(host, guest);
        }
    }

    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut rng = Rng(seed);
        (0..len).map(|_| rng.below(256) as u8).collect()
    }

    const OVER_A_FRAME: usize = 3 << 19;
    const LINK_BYTES_PER_FRAME: u32 = 256 << 10;

    fn over_a_frame(side: &mut Side, seed: u64) {
        side.padding = noise(OVER_A_FRAME, seed);
    }

    fn assert_fills_by_bytes(side: &Side) {
        let progress = &side.state_progress;
        assert!(!progress.is_empty(), "no progress shown");
        let total = progress[0].1;
        assert!(total as usize > 1 << 20, "{total} bytes");
        assert!(progress.iter().all(|&(got, t)| t == total && got < total));
        assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0));
        assert_eq!(side.session().state_received(), None);
    }

    #[test]
    fn a_running_game_over_a_frame_crosses_on_join() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        pair.pace(LINK_BYTES_PER_FRAME);
        over_a_frame(&mut pair.host, 3);
        let mine = pair.host.running(snake());
        let session = pair.invite(mine);
        pair.accept(session, MyGame::Own(snake()));
        let gave = pair.host.gave.clone().unwrap();
        let state = gave.len() - pair.host.padding.len();
        assert!(gave.len() > OVER_A_FRAME);
        let started = pair.guest.session().log().unwrap().started[0].clone();
        assert_eq!(started.as_deref(), Some(&gave[..state]));
        assert_fills_by_bytes(&pair.guest);
        pair.play_frames(240);
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    #[test]
    fn a_heartgold_sized_resync_reaches_the_friend() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(120);
        let mut padding = noise(2 << 20, 5);
        padding.resize(7 << 20, 0);
        let host = pair.host.session_mut().round.rollback_mut().unwrap();
        let sent = host.send_resync(&padding).unwrap();
        assert!(sent > 7 << 20);
        pair.until("the resync arrives", |p| p.guest.session().resyncs() == 1);
        pair.play_frames(120);
        assert_eq!(pair.host.session().resyncs(), 0);
        assert_same_inputs(pair.host.session(), pair.guest.session());
    }

    #[test]
    fn a_dropped_pair_with_states_over_a_frame_rejoins() {
        let (mut pair, returned) = dropped_after(300);
        for (side, back) in [&mut pair.host, &mut pair.guest].into_iter().zip(returned) {
            side.runs_own = !back.paused;
            side.history = back.history;
            side.own = game_boy(back.console);
        }
        pair.pace(LINK_BYTES_PER_FRAME);
        over_a_frame(&mut pair.host, 7);
        over_a_frame(&mut pair.guest, 9);
        pair.until("the guest is back", |p| p.guest.online.is_connected());
        let session = pair.invite(running_snake());
        pair.accept(session, MyGame::Own(running_snake()));
        for side in [&pair.host, &pair.guest] {
            assert!(side.gave.as_ref().unwrap().len() > OVER_A_FRAME);
            assert_fills_by_bytes(side);
        }
        let moved = transfers_over(&mut pair, 600);
        assert!(moved.iter().all(|&n| n >= 30), "{moved:?}");
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    #[test]
    fn leaving_hands_back_a_running_console_and_a_drop_a_paused_one() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(60);
        pair.host.play.leave(&mut pair.host.online);
        pair.until("the friend sees it", |p| {
            p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
        });
        for side in [&mut pair.host, &mut pair.guest] {
            let back = side.play.close().and_then(PlaySession::finish).unwrap();
            assert!(!back.paused);
        }
        let (_, returned) = dropped_after(300);
        assert!(returned.iter().all(|back| back.paused));
    }

    #[test]
    fn leaving_ends_the_session_for_both_and_keeps_the_console() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(120);
        pair.host.play.leave(&mut pair.host.online);
        assert_eq!(pair.host.stage(), Some(Stage::Ended(Ending::Left)));
        pair.until("the friend sees it", |p| {
            p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
        });
        let screen = pair.host.session().console(true).unwrap().frame().to_vec();
        let console = pair.host.play.close().unwrap().finish().unwrap().console;
        assert!(console.frame() == screen.as_slice());
        assert!(pair.host.play.session().is_none());
        assert!(pair.guest.play.close().unwrap().finish().is_some());
    }

    #[test]
    fn a_lost_connection_drops_the_session_for_the_friend() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(60);
        pair.guest.online.leave();
        pair.until("the host sees the drop", |p| {
            p.host.stage() == Some(Stage::Ended(Ending::Dropped))
        });
        assert_eq!(pair.guest.stage(), Some(Stage::Ended(Ending::Dropped)));
    }

    #[test]
    fn declining_ends_the_invite_for_the_host() {
        let mut pair = Pair::new(false, 0, 0, homebrew());
        let session = pair.invite(snake());
        pair.guest.play.decline(&mut pair.guest.online, session);
        pair.until("the host hears", |p| {
            p.host.stage() == Some(Stage::Ended(Ending::Declined))
        });
        assert!(pair.guest.play.invites().is_empty());
    }

    #[test]
    fn a_borrowed_game_crosses_when_the_relay_shares_games() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let mine = custom();
        let hash = hash_of(&mine);
        let session = pair.invite(mine);
        pair.accept(session, MyGame::Borrow { save: None });
        assert!(pair.guest.received);
        pair.play_frames(120);
        let guest = pair.guest.session();
        assert!(guest.borrowing());
        assert_eq!(guest.mode(), Some(Mode::Rollback));
        assert_eq!(guest.my_game().unwrap().hash, hash);
        assert_eq!(pair.host.session().resyncs() + guest.resyncs(), 0);
        assert_eq!(pair.guest.play.holds(&hash), Some(false));
        let custom = custom().rom;
        pair.until_checked();
        assert_matches_reference(pair.host.session(), [&custom, &custom]);
    }

    #[test]
    fn missing_games_cross_both_ways_when_the_relay_shares_games() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(custom());
        pair.accept(session, MyGame::Own(snake()));
        assert!(pair.guest.received && pair.host.received);
        assert!(pair.guest.sent && pair.host.sent);
        for side in [&pair.host, &pair.guest] {
            assert!(side.arrived.windows(2).all(|w| w[0].0 <= w[1].0));
            assert!(side.arrived.iter().all(|(got, total)| got <= total));
            assert_eq!(side.arrived.first().map(|a| a.0), Some(0));
        }
        assert!(!pair.host.session().sends_game() && !pair.guest.session().sends_game());
        pair.play_frames(120);
        assert_eq!(pair.host.session().mode(), Some(Mode::Rollback));
        assert_eq!(
            pair.host.session().resyncs() + pair.guest.session().resyncs(),
            0
        );
        pair.until_checked();
        assert_matches_reference(pair.host.session(), [&custom().rom, SNAKE]);
    }

    #[test]
    fn without_game_sharing_different_games_play_over_the_cable() {
        let mut pair = Pair::new(false, 30, 10, empty());
        let session = pair.invite(custom());
        assert!(!pair.guest.play.accept(
            &mut pair.guest.online,
            session,
            MyGame::Borrow { save: None }
        ));
        let mine = custom();
        let theirs = snake();
        for (side, file) in [(&mut pair.host, &mine), (&mut pair.guest, &theirs)] {
            let mut gb = GameBoy::new_empty(GbModel::try_from(file.kind).unwrap());
            gb.load_rom(&Rom::new(&file.rom)).unwrap();
            side.cable_gb = Some(gb);
        }
        pair.accept(session, MyGame::Own(snake()));
        assert!(!pair.guest.received);
        assert_eq!(pair.host.session().mode(), Some(Mode::Cable));
        assert_eq!(pair.guest.session().mode(), Some(Mode::Cable));
        pair.until("both consoles ran", |p| {
            p.host.cable_frames >= 120 && p.guest.cable_frames >= 120
        });
        assert!(pair.host.session().cable_plugged() && pair.guest.session().cable_plugged());
        pair.guest.play.session_mut().unwrap().set_cable(false);
        pair.until("the cable is out on both", |p| {
            !p.host.session().cable_plugged() && !p.guest.session().cable_plugged()
        });
        pair.guest.online.send(ClientMsg::Game {
            session,
            sealed: Sealed {
                nonce: [7; NONCE_BYTES],
                ciphertext: vec![7; 40],
            },
        });
        pair.until("the relay refuses the game", |p| {
            p.guest.online.request_error() == Some(ErrorCode::GameSharingOff)
        });
    }

    #[test]
    fn friend_rows_follow_an_invite_until_both_play() {
        use crate::app::tabs::playing::{PlayButton, play_button, play_subtitle};
        let mut pair = Pair::new(false, 0, 0, homebrew());
        let (host_id, guest_id) = (pair.host.id(), pair.guest.id());
        let host = &pair.host;
        assert_eq!(
            play_button(&host.play, &host.online, guest_id),
            Some(PlayButton::Ready)
        );

        let session = pair.invite(snake());
        assert_eq!(
            play_subtitle(&pair.host.play, guest_id).as_deref(),
            Some("Invite sent")
        );
        assert_eq!(
            play_subtitle(&pair.guest.play, host_id).as_deref(),
            Some("Wants to play Snake")
        );
        assert_eq!(
            play_button(&pair.guest.play, &pair.guest.online, host_id),
            None
        );
        assert!(pair.guest.play.has_unseen_invite());
        pair.guest.play.mark_invites_seen();
        assert!(!pair.guest.play.has_unseen_invite());

        pair.accept(session, MyGame::Own(snake()));
        for (side, friend) in [(&pair.host, guest_id), (&pair.guest, host_id)] {
            assert_eq!(
                play_subtitle(&side.play, friend).as_deref(),
                Some("Playing with you")
            );
            assert_eq!(play_button(&side.play, &side.online, friend), None);
        }
    }

    fn assert_watcher_sees_the_player(watcher: &PlaySession, player: &PlaySession) {
        let want: HashMap<u32, [u64; 2]> = player
            .log()
            .unwrap()
            .checks
            .iter()
            .map(|c| (c.frame, c.hashes))
            .collect();
        let mut compared = 0;
        for check in &watcher.log().unwrap().checks {
            if let Some(hashes) = want.get(&check.frame) {
                assert_eq!(*hashes, check.hashes, "check of frame {}", check.frame);
                compared += 1;
            }
        }
        assert!(compared >= 3, "only {compared} checks to compare");
    }

    fn watching_pair(sharing: bool, shelves: [Box<dyn Shelf>; 2], game: GameFile) -> Pair {
        let mut pair = Pair::new(sharing, 30, 10, shelves);
        let session = pair.invite_with(None);
        assert!(pair.guest.play.invites()[0].game.is_none());
        assert_eq!(
            crate::app::tabs::playing::ask_line("Clara", &pair.guest.play.invites()[0]),
            "Clara wants to join you"
        );
        let game = pair.guest.running(game);
        let before = encode_state(pair.guest.own.as_ref().unwrap()).unwrap();
        pair.accept(session, MyGame::Own(game));
        pair.until("the player's game goes in as it was", |p| {
            p.guest.gave.is_some()
        });
        assert_eq!(pair.guest.gave.as_ref(), Some(&before));
        pair
    }

    #[test]
    fn a_friend_with_no_game_watches_the_player() {
        let mut pair = watching_pair(false, homebrew(), snake());
        pair.play_frames(240);
        let (watcher, player) = (pair.host.session(), pair.guest.session());
        assert_eq!(watcher.mode(), Some(Mode::Rollback));
        assert!(watcher.watching_only() && !player.watching_only());
        assert_eq!(watcher.friend_plays(), Some(true));
        assert_eq!(player.friend_plays(), Some(false));
        assert!(watcher.console(true).is_none() && watcher.console(false).is_some());
        assert!(player.console(true).is_some() && player.console(false).is_none());
        assert!(player.lets_friend_watch() && watcher.friend_lets_me_watch());
        assert_eq!(watcher.resyncs() + player.resyncs(), 0);
        assert_watcher_sees_the_player(watcher, player);
        use crate::app::tabs::playing::{play_subtitle, screen_note, watched_line};
        assert_eq!(screen_note("Mia", watcher), None);
        assert!(watched_line("Clara", player).ends_with("Clara is watching you"));
        let (host_id, guest_id) = (pair.host.id(), pair.guest.id());
        assert_eq!(
            play_subtitle(&pair.host.play, guest_id).as_deref(),
            Some("You're watching")
        );
        assert_eq!(
            play_subtitle(&pair.guest.play, host_id).as_deref(),
            Some("Watching you")
        );
        use crate::app::tabs::playing::{Switch, bar_switches};
        let kinds = |session| {
            bar_switches(session, "Mia")
                .iter()
                .map(|s| (s.switch, s.on))
                .collect::<Vec<_>>()
        };
        let (watcher, player) = (pair.host.session(), pair.guest.session());
        assert_eq!(kinds(watcher), vec![(Switch::Watch, true)]);
        assert_eq!(kinds(player), vec![(Switch::Show, true)]);
    }

    #[test]
    fn a_player_never_waits_for_the_watcher() {
        let mut pair = watching_pair(false, homebrew(), snake());
        pair.play_frames(60);
        let before = pair.guest.frame();
        pair.run_guest_alone(120);
        assert!(pair.guest.frame() >= before + 100);
        assert!(!pair.guest.session().waiting_for_friend(pair.clock.now()));
    }

    #[test]
    fn the_player_stops_showing_and_shows_again() {
        let mut pair = watching_pair(false, homebrew(), snake());
        pair.play_frames(120);
        pair.guest.session_mut().set_let_watch(false);
        pair.until("the watcher hears", |p| {
            !p.host.session().friend_lets_me_watch()
        });
        let heard = pair.guest.frame();
        pair.until("the last pads are in", |p| p.guest.frame() >= heard + 30);
        let seen = pair.host.frame();
        let played = pair.guest.frame();
        pair.until("the player plays on", |p| p.guest.frame() >= played + 120);
        assert_eq!(pair.host.frame(), seen);
        assert!(pair.host.session().is_live() && pair.guest.session().is_live());
        assert_eq!(
            crate::app::tabs::playing::screen_note("Mia", pair.host.session()).as_deref(),
            Some("Mia isn't showing their screen.")
        );

        pair.guest.session_mut().set_let_watch(true);
        pair.until("both are in the next round", |p| {
            p.host.session().round() == 2
                && p.guest.session().round() == 2
                && p.host.stage() == Some(Stage::Playing)
                && p.guest.stage() == Some(Stage::Playing)
        });
        pair.play_frames(180);
        assert!(pair.host.session().friend_lets_me_watch());
        assert_watcher_sees_the_player(pair.host.session(), pair.guest.session());
    }

    #[test]
    fn the_watcher_leaves_and_the_player_keeps_the_game() {
        let mut pair = watching_pair(false, homebrew(), snake());
        pair.play_frames(60);
        pair.host.play.leave(&mut pair.host.online);
        pair.until("the player sees it", |p| {
            p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
        });
        assert!(pair.host.play.close().unwrap().finish().is_none());
        let back = pair.guest.play.close().unwrap().finish().unwrap();
        assert!(back.borrowed.is_none());
        assert_eq!(&*back.console.rom_bytes(), SNAKE);
    }

    #[test]
    fn a_watcher_without_the_game_and_no_sharing_watches_nothing() {
        let pair = watching_pair(false, empty(), custom());
        let watcher = pair.host.session();
        assert_eq!(watcher.mode(), Some(Mode::Apart));
        assert_eq!(watcher.holds_theirs(), Some(false));
        assert_eq!(watcher.friend_game().unwrap().title, "My Snake");
        assert!(watcher.console(false).is_none());
        assert!(!pair.host.received);
        assert_eq!(pair.guest.session().mode(), Some(Mode::Apart));
        use crate::app::tabs::playing::{screen_note, watched_line};
        assert_eq!(
            screen_note("Mia", watcher).as_deref(),
            Some("You need My Snake to watch Mia. This relay doesn't share games.")
        );
        use crate::app::tabs::playing::{Switch, bar_switches, needs_copy, play_subtitle};
        assert!(needs_copy(watcher));
        let switches = bar_switches(watcher, "Mia");
        assert_eq!(switches.len(), 1);
        assert_eq!(switches[0].switch, Switch::Watch);
        assert_eq!(switches[0].why_off.as_deref(), Some("Needs My Snake"));
        assert_eq!(
            play_subtitle(&pair.host.play, pair.guest.id()).as_deref(),
            Some("Needs My Snake")
        );
        assert_eq!(
            watched_line("Clara", pair.guest.session()),
            "Clara needs My Snake to watch you."
        );
    }

    #[test]
    fn a_watcher_hands_over_their_own_copy_and_watches() {
        let mut pair = watching_pair(false, empty(), custom());
        assert_eq!(pair.host.session().mode(), Some(Mode::Apart));
        assert!(!pair.host.session_mut().hand_rom(SNAKE.into()));
        assert!(pair.host.session_mut().hand_rom(custom().rom));
        pair.until("the watcher sees the game", |p| {
            p.host.session().mode() == Some(Mode::Rollback)
        });
        pair.play_frames(180);
        let (watcher, player) = (pair.host.session(), pair.guest.session());
        assert!(watcher.watching_only());
        assert!(!pair.host.received);
        assert_watcher_sees_the_player(watcher, player);
    }

    #[test]
    fn a_watcher_borrows_the_game_when_the_relay_shares_games() {
        let mut pair = watching_pair(true, empty(), custom());
        assert!(pair.host.received);
        pair.play_frames(180);
        let (watcher, player) = (pair.host.session(), pair.guest.session());
        assert_eq!(watcher.mode(), Some(Mode::Rollback));
        assert!(watcher.console(false).is_some());
        assert_watcher_sees_the_player(watcher, player);
    }

    #[test]
    fn a_watcher_brings_a_game_and_both_play() {
        let mut pair = watching_pair(false, homebrew(), snake());
        pair.play_frames(120);
        assert!(pair.host.session_mut().bring(MyGame::Own(snake())));
        pair.until("both play the next round", |p| {
            p.host.session().round() == 2
                && p.guest.session().round() == 2
                && p.host.stage() == Some(Stage::Playing)
                && p.guest.stage() == Some(Stage::Playing)
        });
        pair.play_frames(240);
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.mode(), Some(Mode::Rollback));
        assert!(!host.watching_only() && !guest.watching_only());
        assert!(host.console(true).is_some() && guest.console(true).is_some());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    #[test]
    fn hiding_my_screen_pauses_my_game_until_i_bring_it_back() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(120);
        assert!(pair.host.session_mut().bring(MyGame::Nothing));
        pair.until("the host watches in the next round", |p| {
            p.host.own.is_some()
                && p.guest.session().round() == 2
                && p.host.stage() == Some(Stage::Playing)
                && p.guest.stage() == Some(Stage::Playing)
        });
        let paused = encode_state(pair.host.own.as_ref().unwrap()).unwrap();
        pair.until("the friend plays on", |p| p.guest.frame() >= 240);
        assert!(pair.host.session().watching_only());
        assert!(pair.host.session().console(false).is_some());
        assert_eq!(
            encode_state(pair.host.own.as_ref().unwrap()).unwrap(),
            paused
        );

        let mine = GameFile {
            running: true,
            ..snake()
        };
        pair.host.gave = None;
        assert!(pair.host.session_mut().bring(MyGame::Own(mine)));
        pair.until("both play the third round", |p| {
            p.host.session().round() == 3
                && p.host.stage() == Some(Stage::Playing)
                && p.guest.stage() == Some(Stage::Playing)
        });
        assert_eq!(pair.host.gave.as_ref(), Some(&paused));
        pair.play_frames(120);
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert!(host.console(true).is_some() && host.console(false).is_some());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    fn both_play(pair: &mut Pair) {
        pair.until("both play", |p| {
            p.host.stage() == Some(Stage::Playing) && p.guest.stage() == Some(Stage::Playing)
        });
    }

    #[test]
    fn both_play_the_same_game_and_each_watches_the_other() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(240);
        for side in [pair.host.session(), pair.guest.session()] {
            assert_eq!(side.mode(), Some(Mode::Rollback));
            assert!(side.lets_friend_watch() && side.friend_lets_me_watch());
            assert!(side.sees_friend() && side.has_friend_tab());
            assert!(side.console(true).is_some() && side.console(false).is_some());
        }
        pair.guest.session_mut().set_watch_friend(false);
        assert!(!pair.guest.session().has_friend_tab());
        pair.host.session_mut().set_let_watch(false);
        pair.until("the guest hears the host stopped showing", |p| {
            !p.guest.session().friend_lets_me_watch()
        });
        pair.play_frames(120);
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert!(!guest.sees_friend() && host.sees_friend());
        assert_eq!((host.round(), guest.round()), (1, 1));
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
        assert_matches_reference(host, [SNAKE, SNAKE]);
    }

    #[test]
    fn different_games_link_only_while_both_cables_are_in() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(custom());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(120);
        let linked = |p: &Pair| {
            (
                p.host.session().cable_plugged(),
                p.guest.session().cable_plugged(),
            )
        };
        assert_eq!(pair.host.session().mode(), Some(Mode::Rollback));
        assert_eq!(linked(&pair), (true, true));
        pair.guest.session_mut().set_cable(false);
        pair.until("the cable is out on both", |p| linked(p) == (false, false));
        pair.host.session_mut().set_cable(false);
        pair.play_frames(30);
        pair.host.session_mut().set_cable(true);
        pair.play_frames(120);
        assert_eq!(linked(&pair), (false, false));
        assert!(!pair.host.session().friend_cable());
        pair.guest.session_mut().set_cable(true);
        pair.until("the cable is in on both", |p| linked(p) == (true, true));
        pair.play_frames(60);
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!((host.round(), guest.round()), (1, 1));
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
        assert_matches_reference(host, [&custom().rom, SNAKE]);
    }

    #[test]
    fn the_guest_picks_another_game_on_join() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(custom()));
        pair.play_frames(120);
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(guest.my_game().unwrap().title, "My Snake");
        assert_eq!(host.friend_game().unwrap().title, "My Snake");
        assert_eq!(host.mode(), Some(Mode::Rollback));
        assert!(host.sees_friend() && guest.sees_friend());
        assert_matches_reference(host, [SNAKE, &custom().rom]);
    }

    #[test]
    fn a_player_changes_games_and_the_friend_plays_on() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let mine = pair.host.running(snake());
        let session = pair.invite(mine);
        let theirs = pair.guest.running(snake());
        pair.accept(session, MyGame::Own(theirs));
        pair.play_frames(120);
        pair.guest.gave = None;
        assert!(pair.host.session_mut().bring(MyGame::Own(custom())));
        pair.until("both play the next round", |p| {
            p.host.session().round() == 2 && p.guest.session().round() == 2
        });
        both_play(&mut pair);
        pair.play_frames(120);
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.my_game().unwrap().title, "My Snake");
        assert_eq!(guest.friend_game().unwrap().title, "My Snake");
        let kept = pair.guest.gave.clone();
        assert!(kept.is_some());
        assert_eq!(guest.log().unwrap().started[1], kept);
        assert_eq!(guest.start().unwrap().power_on[1], 0);
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
    }

    #[test]
    fn a_guest_joins_with_no_game_and_just_watches() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Nothing);
        pair.play_frames(240);
        let (player, watcher) = (pair.host.session(), pair.guest.session());
        assert!(watcher.watching_only() && watcher.sees_friend());
        assert!(!watcher.has_friend_tab() && !player.has_friend_tab());
        assert_eq!(player.friend_plays(), Some(false));
        assert_watcher_sees_the_player(watcher, player);
        assert!(pair.guest.session_mut().bring(MyGame::Own(snake())));
        pair.until("both play the next round", |p| {
            p.host.session().round() == 2 && p.guest.session().round() == 2
        });
        both_play(&mut pair);
        pair.play_frames(120);
        assert!(pair.host.session().sees_friend() && pair.guest.session().sees_friend());
        assert!(pair.host.session().has_friend_tab() && pair.guest.session().has_friend_tab());
    }

    #[derive(Clone, Copy, Debug)]
    enum Close {
        Leave,
        Drop,
        Timeout,
    }

    #[test]
    fn every_ending_keeps_both_games_where_they_were() {
        for close in [Close::Leave, Close::Drop, Close::Timeout] {
            let mut pair = Pair::new(false, 30, 10, homebrew());
            let mine = pair.host.running(snake());
            let session = pair.invite(mine);
            let theirs = pair.guest.running(snake());
            pair.accept(session, MyGame::Own(theirs));
            pair.play_frames(180);
            match close {
                Close::Leave => pair.host.play.leave(&mut pair.host.online),
                Close::Drop => pair.guest.online.leave(),
                Close::Timeout => pair.run_host_alone(240),
            }
            pair.until("both sessions ended", |p| {
                !p.host.session().is_live() && !p.guest.session().is_live()
            });
            for side in [&mut pair.host, &mut pair.guest] {
                let session = side.session();
                assert!(session.frame() > 180, "{close:?}");
                let gb = session.game_boy(true).unwrap();
                let mut scratch = Rollback::clone_state(gb);
                let hash = |state: &GameBoy, scratch: &mut GameBoy| {
                    let mut state = Rollback::clone_state(state);
                    state.unplug_link();
                    Rollback::state_hash(&state, scratch)
                };
                let shown = hash(gb, &mut scratch);
                let confirmed: Vec<u64> = session
                    .round
                    .rollback()
                    .and_then(|r| r.confirmed_states(true, usize::MAX))
                    .unwrap()
                    .into_iter()
                    .map(|(_, state)| {
                        hash(&Rollback::decode_state(gb, &state).unwrap(), &mut scratch)
                    })
                    .collect();
                let back = game_boy(side.play.close().unwrap().finish().unwrap().console).unwrap();
                let now = hash(&back, &mut scratch);
                match close {
                    Close::Leave => assert!(now == shown, "{close:?} changed the game"),
                    Close::Drop | Close::Timeout => assert!(
                        now == shown || confirmed.contains(&now),
                        "{close:?} moved the game on"
                    ),
                }
                assert!(!back.link_plugged());
            }
        }
    }

    mod real_games {
        use super::*;
        use crate::real_roms::{rom_of, save_from};
        use citrine_gb_gamedata::Version;
        use citrine_gb_net::Body;

        fn real(
            test: &str,
            version: Version,
            title: &str,
            save: Option<Vec<u8>>,
        ) -> Option<GameFile> {
            let (rom, _) = rom_of(test, version)?;
            Some(GameFile {
                save,
                ..game(&rom, title)
            })
        }

        fn texts(side: &Side, friend: OnlineId) -> Vec<String> {
            side.online
                .conversation(friend)
                .map(|c| {
                    c.entries
                        .iter()
                        .filter_map(|e| match &e.body {
                            Body::Text(text) => Some(text.clone()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        fn assert_same_checks(one: &PlaySession, other: &PlaySession) -> usize {
            let want: HashMap<u32, [u64; 2]> = one
                .log()
                .unwrap()
                .checks
                .iter()
                .map(|c| (c.frame, c.hashes))
                .collect();
            let mut compared = 0;
            for check in &other.log().unwrap().checks {
                if let Some(hashes) = want.get(&check.frame) {
                    assert_eq!(*hashes, check.hashes, "check of frame {}", check.frame);
                    compared += 1;
                }
            }
            assert!(compared >= 3, "only {compared} checks to compare");
            compared
        }

        fn both_leave(pair: &mut Pair) {
            pair.host.play.leave(&mut pair.host.online);
            pair.until("the friend sees it", |p| {
                p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
            });
            assert!(pair.host.play.close().is_some());
            assert!(pair.guest.play.close().is_some());
            pair.host.own = None;
            pair.guest.own = None;
            pair.host.cable_gb = None;
            pair.guest.cable_gb = None;
        }

        #[test]
        #[ignore]
        fn two_friends_chat_play_silver_watch_and_link_gold_with_silver() {
            let test = "two_friends_chat_play_silver_watch_and_link_gold_with_silver";
            let save = save_from(test, "CITRINE_SILVER_SAVE");
            let (Some(silver), Some(gold)) = (
                real(test, Version::Silver, "Pokémon Silver", save.clone()),
                real(test, Version::Gold, "Pokémon Gold", None),
            ) else {
                return;
            };
            let started = Instant::now();
            let mut pair = Pair::new(false, 30, 10, empty());
            let (host_id, guest_id) = (pair.host.id(), pair.guest.id());
            println!("signed in and friends after {:?}", started.elapsed());

            assert!(pair.host.online.send_text(guest_id, "silver tonight?"));
            pair.until("the line arrives", |p| !texts(&p.guest, host_id).is_empty());
            assert_eq!(texts(&pair.guest, host_id), ["silver tonight?"]);
            assert!(pair.guest.online.send_text(host_id, "yes"));
            pair.until("the answer arrives", |p| {
                texts(&p.host, guest_id).len() == 2
            });
            println!("chatted after {:?}", started.elapsed());

            let session = pair.invite(silver.clone());
            pair.accept(session, MyGame::Own(silver.clone()));
            pair.play_frames(900);
            let (host, guest) = (pair.host.session(), pair.guest.session());
            assert_eq!(host.mode(), Some(Mode::Rollback));
            assert_eq!(guest.mode(), Some(Mode::Rollback));
            assert_same_inputs(host, guest);
            let compared = assert_same_checks(host, guest);
            assert_eq!(host.resyncs() + guest.resyncs(), 0);
            println!(
                "played Silver {} frames, {compared} checks matched, after {:?}",
                host.frame(),
                started.elapsed()
            );
            both_leave(&mut pair);

            let session = pair.invite_with(None);
            let mine = pair.guest.running(silver.clone());
            pair.accept(session, MyGame::Own(mine));
            assert_eq!(pair.host.session().mode(), Some(Mode::Apart));
            assert!(pair.host.session_mut().hand_rom(silver.rom.clone()));
            pair.until("the watcher sees the game", |p| {
                p.host.session().mode() == Some(Mode::Rollback)
            });
            pair.play_frames(600);
            let (watcher, player) = (pair.host.session(), pair.guest.session());
            assert!(watcher.watching_only() && !player.watching_only());
            assert!(watcher.console(true).is_none() && watcher.console(false).is_some());
            let compared = assert_same_checks(player, watcher);
            assert_eq!(watcher.resyncs() + player.resyncs(), 0);
            println!(
                "watched {compared} matching checks after {:?}",
                started.elapsed()
            );
            pair.host.play.leave(&mut pair.host.online);
            pair.until("the player sees the watcher go", |p| {
                p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
            });
            assert!(pair.host.play.close().unwrap().finish().is_none());
            let back = pair.guest.play.close().unwrap().finish().unwrap();
            assert_eq!(back.console.rom_bytes(), silver.rom);
            pair.guest.own = None;

            let session = pair.invite(gold.clone());
            for (side, file) in [(&mut pair.host, &gold), (&mut pair.guest, &silver)] {
                let mut gb = GameBoy::new_empty(GbModel::try_from(file.kind).unwrap());
                gb.load_rom(&Rom::new(&file.rom)).unwrap();
                side.cable_gb = Some(gb);
            }
            pair.accept(session, MyGame::Own(silver.clone()));
            assert_eq!(pair.host.session().mode(), Some(Mode::Cable));
            assert_eq!(pair.guest.session().mode(), Some(Mode::Cable));
            pair.until("both consoles ran", |p| {
                p.host.cable_frames >= 600 && p.guest.cable_frames >= 600
            });
            assert!(pair.host.session().cable_plugged() && pair.guest.session().cable_plugged());
            println!(
                "Gold and Silver ran on the cable after {:?}",
                started.elapsed()
            );
            both_leave(&mut pair);
            assert!(pair.host.play.session().is_none() && pair.guest.play.session().is_none());
            println!("took {:?}", started.elapsed());
        }

        #[cfg(feature = "nds")]
        fn heartgold_from(
            state_var: &str,
        ) -> Option<(GameFile, Box<citrine_gb::nds::machine::Nds>)> {
            use citrine_gb::nds::machine::{Nds, NdsFirmware};
            let test = "heartgold_resumes_and_rejoins_over_local_wireless";
            let rom = save_from(test, "CITRINE_HG_ROM")?;
            let state = save_from(test, state_var)?;
            let blank = Nds::new(rom.clone().into(), NdsFirmware::default()).ok()?;
            let nds = blank.decode_state(&state).ok()?;
            let file = GameFile::new(
                rom.clone().into(),
                sha1_of(&rom).unwrap(),
                "Pokémon HeartGold".into(),
                ConsoleKind::Nds,
            );
            Some((
                GameFile {
                    running: true,
                    ..file
                },
                Box::new(nds),
            ))
        }

        #[cfg(feature = "nds")]
        fn heartgold_together(pair: &mut Pair, file: &GameFile) {
            for side in [&mut pair.host, &mut pair.guest] {
                side.state_progress.clear();
                side.gave = None;
            }
            let session = pair.invite(file.clone());
            pair.accept(session, MyGame::Own(file.clone()));
            for side in [&pair.host, &pair.guest] {
                assert!(side.gave.as_ref().unwrap().len() > citrine_gb_net::WHOLE_BYTES);
                if !side.state_progress.is_empty() {
                    assert_fills_by_bytes(side);
                }
            }
            let seen = [&pair.host, &pair.guest].map(|side| side.state_progress.len());
            println!("state progress steps seen {seen:?}");
            pair.play_frames(900);
            pair.until_checked();
            let (host, guest) = (pair.host.session(), pair.guest.session());
            assert_eq!(host.resyncs() + guest.resyncs(), 0);
            assert!(host.failure().is_none() && guest.failure().is_none());
            assert_same_inputs(host, guest);
            let compared = assert_same_checks(host, guest);
            let sizes = [&pair.host, &pair.guest].map(|side| side.gave.as_ref().unwrap().len());
            println!(
                "states {sizes:?} bytes, {} frames, {compared} checks matched",
                host.frame()
            );
        }

        #[cfg(feature = "nds")]
        #[test]
        #[ignore]
        fn heartgold_resumes_and_rejoins_over_local_wireless() {
            let guest_var = match std::env::var_os("CITRINE_HG_STATE_GUEST") {
                Some(_) => "CITRINE_HG_STATE_GUEST",
                None => "CITRINE_HG_STATE",
            };
            let (Some((file, host_nds)), Some((_, guest_nds))) = (
                heartgold_from("CITRINE_HG_STATE"),
                heartgold_from(guest_var),
            ) else {
                return;
            };
            let started = Instant::now();
            let mut pair = Pair::new(true, 30, 10, empty());
            pair.host.own_nds = Some(host_nds);
            pair.guest.own_nds = Some(guest_nds);
            heartgold_together(&mut pair, &file);
            println!("resumed after {:?}", started.elapsed());

            pair.guest.down.set(true);
            pair.until("both lost the session", |p| {
                !p.host.play.is_live() && !p.guest.play.is_live()
            });
            pair.guest.down.set(false);
            pair.guest.online.retry_now(pair.clock.now());
            for side in [&mut pair.host, &mut pair.guest] {
                assert_eq!(side.stage(), Some(Stage::Ended(Ending::Dropped)));
                let back = side.play.close().and_then(PlaySession::finish).unwrap();
                side.history = back.history;
                let Machine::Nds(nds) = back.console else {
                    panic!("a DS game came back as another console");
                };
                side.own_nds = Some(nds);
            }
            pair.until("the guest is back", |p| p.guest.online.is_connected());
            heartgold_together(&mut pair, &file);
            println!("rejoined after {:?}", started.elapsed());
        }

        #[cfg(feature = "nds")]
        struct Roms(Vec<std::sync::Arc<[u8]>>);

        #[cfg(feature = "nds")]
        impl Shelf for Roms {
            fn find(&self, hash: &GameHash) -> Option<std::sync::Arc<[u8]>> {
                self.0
                    .iter()
                    .find(|rom| sha1_of(rom) == Some(*hash))
                    .cloned()
            }

            fn holds_ds_game(&self, code: [u8; 4]) -> bool {
                self.0
                    .iter()
                    .any(|rom| rom.get(0x0C..0x0F) == Some(&code[..3]))
            }
        }

        #[cfg(feature = "nds")]
        struct Lane {
            rom: &'static str,
            state: String,
            script: Vec<(u32, u32, Keys)>,
            nickname: &'static str,
        }

        #[cfg(feature = "nds")]
        struct Distance {
            dir: std::path::PathBuf,
            out: std::path::PathBuf,
            one_way_ms: u64,
            jitter_ms: u64,
            every: u32,
        }

        #[cfg(feature = "nds")]
        impl Distance {
            fn from_env(name: &str) -> Option<Self> {
                let dir = citrine_gb_test_roms::env_path(name, "CITRINE_WDIST_DIR")?;
                let out = std::env::var_os("CITRINE_WDIST_OUT")
                    .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
                    .join(name);
                std::fs::create_dir_all(&out).ok()?;
                let lag = std::env::var("CITRINE_WDIST_LAG").unwrap_or_else(|_| "40,20".into());
                let (one_way, jitter) = lag.split_once(',')?;
                Some(Self {
                    dir,
                    out,
                    one_way_ms: one_way.parse().ok()?,
                    jitter_ms: jitter.parse().ok()?,
                    every: std::env::var("CITRINE_WDIST_EVERY")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(300),
                })
            }

            fn read(&self, path: &str) -> Vec<u8> {
                std::fs::read(self.dir.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
            }

            fn shot(&self, pair: &Pair, name: &str) {
                let lanes = [&pair.host, &pair.guest].map(|side| {
                    side.session()
                        .console(true)
                        .map(|console| console.frame().to_vec())
                        .unwrap_or_default()
                });
                let (width, height) = (256, 384);
                let mut rgba = vec![0u8; width * 2 * height * 4];
                for (i, frame) in lanes.iter().enumerate() {
                    if frame.len() < width * height * 4 {
                        continue;
                    }
                    for y in 0..height {
                        let from = &frame[y * width * 4..(y + 1) * width * 4];
                        let at = (y * width * 2 + i * width) * 4;
                        rgba[at..at + width * 4].copy_from_slice(from);
                    }
                }
                self.write_png(&rgba, width as u32 * 2, height as u32, name);
            }

            fn write_png(&self, rgba: &[u8], width: u32, height: u32, name: &str) {
                let file = std::fs::File::create(self.out.join(format!("{name}.png"))).unwrap();
                let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(rgba)
                    .unwrap();
            }

            fn file(&self, rom: &[u8], title: &str) -> GameFile {
                GameFile {
                    running: true,
                    ..GameFile::new(
                        rom.into(),
                        sha1_of(rom).unwrap(),
                        title.into(),
                        ConsoleKind::Nds,
                    )
                }
            }

            fn run(&self, lanes: [Lane; 2], frames: u32) -> Pair {
                use citrine_gb::nds::machine::{Nds, NdsFirmware};
                let roms: Vec<std::sync::Arc<[u8]>> = ["mk.nds", "pt.nds", "hg.nds"]
                    .iter()
                    .filter_map(|name| std::fs::read(self.dir.join(name)).ok())
                    .map(Into::into)
                    .collect();
                let shelves: [Box<dyn Shelf>; 2] =
                    [Box::new(Roms(roms.clone())), Box::new(Roms(roms.clone()))];
                let mut pair = Pair::new(true, self.one_way_ms, self.jitter_ms, shelves);
                let mut files = Vec::new();
                for (lane, (side, unit)) in lanes
                    .iter()
                    .zip([(&mut pair.host, 0u8), (&mut pair.guest, 1)])
                {
                    let rom = self.read(lane.rom);
                    let blank = Nds::new(rom.clone().into(), NdsFirmware::default()).unwrap();
                    let nds = blank.decode_state(&self.read(&lane.state)).unwrap();
                    side.own_nds = Some(Box::new(nds));
                    side.script = Some(lane.script.clone());
                    let mut user = ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, unit]);
                    user.nickname = lane.nickname.into();
                    side.play.set_ds_user(user);
                    files.push(self.file(&rom, lane.rom));
                }
                let session = pair.invite(files[0].clone());
                pair.accept(session, MyGame::Own(files[1].clone()));
                let started = Instant::now();
                let mut done = 0;
                while done < frames {
                    let step = self.every.min(frames - done);
                    pair.play_frames(step);
                    done += step;
                    self.shot(&pair, &format!("f{:05}", pair.host.frame()));
                    println!("frame {} after {:?}", pair.host.frame(), started.elapsed());
                }
                self.shot(&pair, "end");
                let (host, guest) = (pair.host.session(), pair.guest.session());
                println!(
                    "delay {:?}, round trip {:?} ms, lag {} ms one way + up to {} ms",
                    host.delay(),
                    host.round_trip_ms(),
                    self.one_way_ms,
                    self.jitter_ms
                );
                for (who, side) in [("host", &pair.host), ("guest", &pair.guest)] {
                    let start = side.runs.len().min(240);
                    println!(
                        "PACING {}",
                        Pacing::of(&side.runs[..start]).line(&format!("{who} start"))
                    );
                    println!("PACING {}", Pacing::of(&side.runs[start..]).line(who));
                }
                assert_eq!(host.resyncs() + guest.resyncs(), 0);
                assert!(host.failure().is_none() && guest.failure().is_none());
                assert_same_inputs(host, guest);
                pair
            }

            fn download(&self, script: &str, frames: u32) -> Pair {
                self.download_from(script, frames, 0)
            }

            fn download_from(&self, script: &str, frames: u32, running_for: u32) -> Pair {
                use citrine_gb::nds::machine::Nds;
                let rom: std::sync::Arc<[u8]> = self.read("mk.nds").into();
                let shelves: [Box<dyn Shelf>; 2] = [
                    Box::new(Roms(vec![rom.clone()])),
                    Box::new(Roms(Vec::new())),
                ];
                let mut pair = Pair::new(true, self.one_way_ms, self.jitter_ms, shelves);
                let (keys, touches) = lab_script(script);
                let later = |at: u32| at.checked_sub(running_for);
                pair.host.script = Some(
                    keys.iter()
                        .filter_map(|&(from, until, key)| Some((later(from)?, later(until)?, key)))
                        .collect(),
                );
                pair.host.touches = touches
                    .iter()
                    .filter_map(|&(from, until, touch)| Some((later(from)?, later(until)?, touch)))
                    .collect();
                pair.guest.script = Some(vec![(
                    later(3000).unwrap_or(0),
                    later(4100).unwrap(),
                    Keys::A,
                )]);
                let mut users = Vec::new();
                for (side, unit, nickname) in [
                    (&mut pair.host, 0u8, "Clara"),
                    (&mut pair.guest, 1, "Arhum"),
                ] {
                    let mut user = ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, unit]);
                    user.nickname = nickname.into();
                    side.play.set_ds_user(user.clone());
                    users.push(user);
                }
                if running_for > 0 {
                    let firmware = crate::play::engine::ds_firmware(Some(&users[0]));
                    let mut nds = Nds::new(rom.to_vec().into(), firmware).unwrap();
                    for frame in 0..running_for {
                        let held = keys
                            .iter()
                            .filter(|(from, until, _)| (*from..*until).contains(&frame))
                            .fold(Keys::empty(), |held, (_, _, key)| held | *key);
                        nds.set_keys(held);
                        nds.set_touch(
                            touches
                                .iter()
                                .find(|(from, until, _)| (*from..*until).contains(&frame))
                                .map(|(_, _, touch)| *touch),
                        );
                        nds.run_frame();
                    }
                    pair.host.own_nds = Some(Box::new(nds));
                }
                let file = GameFile {
                    running: running_for > 0,
                    ..self.file(&rom, "mk.nds")
                };
                let session = pair.invite(file);
                pair.accept(session, MyGame::Download);
                let started = Instant::now();
                let mut seen = Vec::new();
                let mut screens = std::collections::VecDeque::new();
                let mut done = 0;
                while done < frames {
                    pair.play_frames(1);
                    done += 1;
                    let fetch = pair.guest.session().fetch();
                    if seen.last() != Some(&fetch) {
                        seen.push(fetch.clone());
                    }
                    if done % self.every != 0 && done < frames {
                        continue;
                    }
                    self.shot(&pair, &format!("f{:05}", pair.host.frame()));
                    println!(
                        "frame {} after {:?}: {fetch:?}",
                        pair.host.frame(),
                        started.elapsed()
                    );
                    let screen = pair
                        .guest
                        .session()
                        .console(true)
                        .map(|console| console.frame().to_vec())
                        .unwrap_or_default();
                    screens.push_back(screen);
                    if screens.len() > 4 {
                        screens.pop_front();
                    }
                }
                self.shot(&pair, "end");
                assert!(
                    screens.iter().all(|screen| lit_share(screen) > 0.3),
                    "the guest's screen went dark before the race"
                );
                assert!(
                    screens
                        .iter()
                        .zip(screens.iter().skip(1))
                        .all(|(a, b)| a != b),
                    "the guest's screen stopped before the race"
                );
                let (host, guest) = (pair.host.session(), pair.guest.session());
                assert!(guest.downloading());
                assert!(seen.iter().any(|fetch| matches!(
                    fetch,
                    Some(crate::play::fetch::Fetch::Receiving { .. })
                )));
                assert!(
                    seen.iter()
                        .any(|fetch| matches!(fetch, Some(crate::play::fetch::Fetch::Ready(_))))
                );
                assert_eq!(host.resyncs() + guest.resyncs(), 0);
                assert!(host.failure().is_none() && guest.failure().is_none());
                assert_same_inputs(host, guest);
                pair
            }

            fn saves(&self, pair: &mut Pair) {
                pair.host.play.leave(&mut pair.host.online);
                pair.until("the friend sees it", |p| {
                    p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
                });
                for (tag, side) in [("a", &mut pair.host), ("b", &mut pair.guest)] {
                    let back = side.play.close().and_then(PlaySession::finish);
                    let nds = match back.map(|back| back.console) {
                        Some(Machine::Nds(nds)) => nds,
                        _ => side.own_nds.take().expect("the DS came back"),
                    };
                    let save = nds
                        .buses()
                        .cart()
                        .backup()
                        .expect("a save")
                        .bytes()
                        .to_vec();
                    std::fs::write(self.out.join(format!("{tag}.sav")), save).unwrap();
                }
            }
        }

        #[cfg(feature = "nds")]
        const MK_DOWNLOAD_HOST: &str = "412 A 6
700 DOWN 4
760 A 6
1400 A 6
1760 A 6
2000 A 6
2400 DOWN 4
2460 A 6
2700 touch 135 157 6
3000 DOWN 4
3060 A 6
4200 A 6
4800 A 6
5400 A 6
6000 A 6
6600 A 6
7560 touch 140 28 6";

        #[cfg(feature = "nds")]
        fn lab_script(text: &str) -> (Presses, Touches) {
            let (mut keys, touches) = lab_lines(text);
            keys.extend((7700..=10460).step_by(120).map(|at| (at, at + 6, Keys::A)));
            (keys, touches)
        }

        #[cfg(feature = "nds")]
        fn lab_lines(text: &str) -> (Presses, Touches) {
            let (mut keys, mut touches) = (Vec::new(), Vec::new());
            for line in text.lines() {
                let words: Vec<&str> = line.split_whitespace().collect();
                if words.is_empty() {
                    continue;
                }
                let at: u32 = words[0].parse().unwrap();
                let until = at + words[words.len() - 1].parse::<u32>().unwrap();
                let key = match words[1] {
                    "touch" => {
                        let touch = citrine_gb::console::Touch {
                            x: words[2].parse().unwrap(),
                            y: words[3].parse().unwrap(),
                        };
                        touches.push((at, until, touch));
                        continue;
                    }
                    "A" => Keys::A,
                    "B" => Keys::B,
                    "UP" => Keys::UP,
                    "DOWN" => Keys::DOWN,
                    "LEFT" => Keys::LEFT,
                    "RIGHT" => Keys::RIGHT,
                    "START" => Keys::START,
                    other => panic!("unknown key {other}"),
                };
                keys.push((at, until, key));
            }
            (keys, touches)
        }

        #[cfg(feature = "nds")]
        fn lit_share(screen: &[u8]) -> f64 {
            let pixels = screen.len() / 4;
            if pixels == 0 {
                return 0.0;
            }
            let lit = screen
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3].iter().any(|&channel| channel > 16))
                .count();
            lit as f64 / pixels as f64
        }

        #[cfg(feature = "nds")]
        #[test]
        #[ignore]
        fn a_friend_without_mario_kart_downloads_it_and_races() {
            let Some(distance) = Distance::from_env("mkdl") else {
                println!("no CITRINE_WDIST_DIR, skipped");
                return;
            };
            let frames = std::env::var("CITRINE_WDIST_FRAMES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(11000);
            distance.download(MK_DOWNLOAD_HOST, frames);
        }

        #[cfg(feature = "nds")]
        #[test]
        #[ignore]
        fn a_friend_joins_a_running_mario_kart_by_download_play() {
            let Some(distance) = Distance::from_env("mkdl-running") else {
                println!("no CITRINE_WDIST_DIR, skipped");
                return;
            };
            let running_for = std::env::var("CITRINE_WDIST_RUNNING")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2760);
            let frames = std::env::var("CITRINE_WDIST_FRAMES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(11000 - running_for);
            distance.download_from(MK_DOWNLOAD_HOST, frames, running_for);
        }

        #[cfg(feature = "nds")]
        #[test]
        #[ignore]
        fn a_two_ds_mario_kart_race_costs_per_frame_on_one_device() {
            use citrine_gb::netplay::{
                Advance, ConsoleSetup, Input, NdsBoot, NdsSetup, Player, Session, SessionConfig,
                Sync,
            };
            let Some(dir) = citrine_gb_test_roms::env_path("race", "CITRINE_WDIST_DIR") else {
                return;
            };
            let read = |name: &str| std::fs::read(dir.join(name)).unwrap();
            let rom = citrine_gb::rom::Rom::new(&read("mk.nds"));
            let states = [read("race/a.state"), read("race/b.state")];
            let frames: u32 = std::env::var("CITRINE_WDIST_FRAMES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600);
            let mut ends = Vec::new();
            for watching in [false, true] {
                let setups = [0, 1].map(|unit| {
                    Some(NdsSetup {
                        boot: NdsBoot::Card(ConsoleSetup {
                            rom: &rom,
                            kind: ConsoleKind::Nds,
                            save: None,
                            state: Some(&states[unit]),
                        }),
                        firmware: crate::play::engine::ds_firmware(None),
                    })
                });
                let config = SessionConfig {
                    sync: Sync::lockstep(0),
                    ..SessionConfig::new(Player::One, 0x0D5_2026, 1_700_000_000)
                };
                let mut session = Session::new_nds(config, setups).unwrap();
                session.watch_remote(watching);
                let mut rng = 0x5EED_u64;
                let mut costs = Vec::new();
                for frame in 0..frames {
                    rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let turn = match rng >> 62 {
                        0 => Keys::LEFT,
                        1 => Keys::RIGHT,
                        _ => Keys::empty(),
                    };
                    session
                        .add_local_input(frame, Input::new(Keys::A | turn))
                        .unwrap();
                    session.add_remote_input(frame, Input::new(Keys::A | turn));
                    let started = Instant::now();
                    assert!(matches!(session.advance(), Advance::Ran));
                    costs.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                let skip = 60.min(costs.len());
                let warm = &mut costs[skip..];
                warm.sort_by(f64::total_cmp);
                let at = |q: f64| warm[((warm.len() as f64 - 1.0) * q).round() as usize];
                println!(
                    "RACE watching {watching}: {} frames, mean {:.2} ms, p50 {:.2} p99 {:.2} max {:.2} ms per shown frame, lit {:.2}",
                    warm.len(),
                    warm.iter().sum::<f64>() / warm.len() as f64,
                    at(0.5),
                    at(0.99),
                    at(1.0),
                    lit_share(session.console(Player::Two).frame()),
                );
                ends.push([Player::One, Player::Two].map(|player| {
                    let console = session.console(player);
                    let mut scratch = citrine_gb::console::Rollback::clone_state(console);
                    citrine_gb::console::Rollback::state_hash(console, &mut scratch)
                }));
            }
            println!("RACE same states watched or not: {}", ends[0] == ends[1]);
            assert!(ends[0] == ends[1]);
        }

        #[cfg(feature = "nds")]
        #[test]
        #[ignore]
        fn mario_kart_balloon_battle_across_the_atlantic() {
            let Some(distance) = Distance::from_env("mk") else {
                return;
            };
            let lanes = [
                Lane {
                    rom: "mk.nds",
                    state: "w10/states/m16/a1500.state".into(),
                    script: Vec::new(),
                    nickname: "Clara",
                },
                Lane {
                    rom: "mk.nds",
                    state: "w10/states/m16/b1500.state".into(),
                    script: vec![(300, 2100, Keys::A)],
                    nickname: "Arhum",
                },
            ];
            let frames = std::env::var("CITRINE_WDIST_FRAMES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2700);
            let mut pair = distance.run(lanes, frames);
            distance.saves(&mut pair);
        }

        #[cfg(feature = "nds")]
        #[test]
        #[ignore]
        fn union_room_trade_across_the_atlantic() {
            let Some(distance) = Distance::from_env("union") else {
                return;
            };
            let lanes = [
                Lane {
                    rom: "pt.nds",
                    state: "w10/states/u29/a500.state".into(),
                    script: vec![(20, 24, Keys::A)],
                    nickname: "Clara",
                },
                Lane {
                    rom: "hg.nds",
                    state: "w10/states/u29/b500.state".into(),
                    script: vec![(20, 24, Keys::A)],
                    nickname: "Arhum",
                },
            ];
            let frames = std::env::var("CITRINE_WDIST_FRAMES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600);
            let mut pair = distance.run(lanes, frames);
            distance.saves(&mut pair);
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        #[derive(Default)]
        struct Tally {
            wire_up: u64,
            wire_down: u64,
            up: u64,
            down: u64,
            datagrams_up: u32,
            datagrams_down: u32,
            opens: u32,
            opened: u32,
            failed: u32,
            closed: u32,
            errors: Vec<ErrorCode>,
            first: Option<(&'static str, std::net::SocketAddrV4)>,
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        impl Tally {
            fn sent(&mut self, bytes: &[u8]) {
                self.wire_up += bytes.len() as u64;
                match ClientMsg::decode(bytes) {
                    Ok(ClientMsg::NetSend { to, data, .. }) => {
                        self.up += data.len() as u64;
                        self.datagrams_up += 1;
                        if to.port != 53 && self.first.is_none() {
                            self.first = Some(("udp", to.socket()));
                        }
                    }
                    Ok(ClientMsg::NetOpen { to, .. }) => {
                        self.opens += 1;
                        if self.first.is_none() {
                            self.first = Some(("tcp", to.socket()));
                        }
                    }
                    Ok(ClientMsg::NetWrite { data, .. }) => self.up += data.len() as u64,
                    _ => {}
                }
            }

            fn heard(&mut self, bytes: &[u8]) {
                self.wire_down += bytes.len() as u64;
                match ServerMsg::decode(bytes) {
                    Ok(ServerMsg::NetHeard { data, .. }) => {
                        self.down += data.len() as u64;
                        self.datagrams_down += 1;
                    }
                    Ok(ServerMsg::NetRead { data, .. }) => self.down += data.len() as u64,
                    Ok(ServerMsg::NetOpened { ok: true, .. }) => self.opened += 1,
                    Ok(ServerMsg::NetOpened { ok: false, .. }) => self.failed += 1,
                    Ok(ServerMsg::NetClosed { .. }) => self.closed += 1,
                    Ok(ServerMsg::Error(code)) if !self.errors.contains(&code) => {
                        self.errors.push(code);
                    }
                    _ => {}
                }
            }
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        struct Counted {
            inner: Box<dyn Transport>,
            tally: std::rc::Rc<std::cell::RefCell<Tally>>,
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        impl Transport for Counted {
            fn send(&mut self, bytes: Vec<u8>) {
                self.tally.borrow_mut().sent(&bytes);
                self.inner.send(bytes);
            }

            fn poll(&mut self) -> Option<TransportEvent> {
                let event = self.inner.poll();
                if let Some(TransportEvent::Message(bytes)) = &event {
                    self.tally.borrow_mut().heard(bytes);
                }
                event
            }

            fn arrived(&self) -> Option<Instant> {
                self.inner.arrived()
            }

            fn clock(&self) -> Option<Instant> {
                self.inner.clock()
            }

            fn relay_key(&self) -> Option<citrine_gb_net::RelayKey> {
                self.inner.relay_key()
            }

            fn side_line(&self) -> Option<transport::SideLine> {
                self.inner.side_line()
            }
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn counted(tally: std::rc::Rc<std::cell::RefCell<Tally>>) -> Connector {
            let mut websocket = transport::websocket();
            Box::new(move |url| {
                Box::new(Counted {
                    inner: websocket(url),
                    tally: tally.clone(),
                })
            })
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn wait_online(online: &mut Online, what: &str, done: impl Fn(&Online) -> bool) {
            let until = Instant::now() + Duration::from_secs(30);
            while !done(online) {
                assert!(
                    Instant::now() < until,
                    "timed out waiting until {what}: {:?}",
                    online.phase()
                );
                online.update(Instant::now());
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        struct Wfc {
            n: usize,
            emulator: crate::emulator::Emulator,
            online: Online,
            tally: std::rc::Rc<std::cell::RefCell<Tally>>,
            mac: [u8; 6],
            keys: Presses,
            touches: Touches,
            held: Keys,
            touch: Option<Touch>,
            told_start: bool,
            peers: Vec<citrine_gb_nds_net::gateway::UdpPeer>,
            qr2: Qr2Watch,
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        #[derive(Default)]
        struct Qr2Watch {
            up: std::collections::BTreeMap<u8, u32>,
            down: std::collections::BTreeMap<u8, u32>,
            replied: Option<u32>,
            registered: Option<u32>,
            keys: String,
            tcp_failed: Option<String>,
            others: std::collections::BTreeMap<(u16, std::net::SocketAddrV4), (u32, u32)>,
            opened: std::collections::BTreeMap<(u16, std::net::SocketAddrV4), u32>,
            silent: Option<String>,
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        struct R4Log;

        #[cfg(all(feature = "nds", ds_wifi))]
        impl log::Log for R4Log {
            fn enabled(&self, metadata: &log::Metadata) -> bool {
                metadata.target().starts_with("citrine_gb_server::ds_net")
            }

            fn log(&self, record: &log::Record) {
                if self.enabled(record.metadata()) {
                    println!("R4 server {}", record.args());
                }
            }

            fn flush(&self) {}
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn counts(map: &std::collections::BTreeMap<u8, u32>) -> String {
            map.iter()
                .map(|(kind, count)| format!("{kind:02x}:{count}"))
                .collect::<Vec<_>>()
                .join(" ")
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        const NATNEG_MAGIC: [u8; 6] = [0xFD, 0xFC, 0x1E, 0x66, 0x6A, 0xB2];
        #[cfg(all(feature = "nds", ds_wifi))]
        const QR2_REGISTERED: u8 = 0x0A;
        #[cfg(all(feature = "nds", ds_wifi))]
        const QR2_LATE: u32 = 600;
        #[cfg(all(feature = "nds", ds_wifi))]
        const TCP_SILENT: u32 = 600;

        #[cfg(all(feature = "nds", ds_wifi))]
        fn qr2_name(kind: u8) -> &'static str {
            match kind {
                0x00 => "query",
                0x01 => "challenge",
                0x02 => "echo",
                0x03 => "heartbeat",
                0x04 => "adderror",
                0x05 => "echo-response",
                0x06 => "client-message",
                0x07 => "client-message-ack",
                0x08 => "keepalive",
                0x09 => "available",
                0x0A => "registered",
                _ => "unknown",
            }
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn qr2_kind(up: bool, data: &[u8]) -> Option<u8> {
            if up {
                data.first().copied()
            } else if data.starts_with(&[0xFE, 0xFD]) {
                data.get(2).copied()
            } else {
                None
            }
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn qr2_keys(body: &[u8]) -> String {
            let fields: Vec<String> = body
                .split(|byte| *byte == 0)
                .map(|field| String::from_utf8_lossy(field).escape_debug().to_string())
                .collect();
            fields
                .chunks(2)
                .take_while(|pair| pair.len() == 2 && !pair[0].is_empty())
                .filter(|pair| pair[0] != "statechanged")
                .map(|pair| {
                    let value: String = pair[1].chars().take(32).collect();
                    format!("{}={value}", pair[0])
                })
                .collect::<Vec<_>>()
                .join(" ")
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn master_view(challenge: &str) -> Option<std::net::SocketAddrV4> {
            if challenge.len() != 20 || challenge.get(6..8) != Some("00") {
                return None;
            }
            let ip = u32::from_str_radix(challenge.get(8..16)?, 16).ok()?;
            let port = u16::from_str_radix(challenge.get(16..20)?, 16).ok()?;
            Some(std::net::SocketAddrV4::new(ip.into(), port))
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        impl Wfc {
            fn frame(&mut self, frame: u32, now: Instant) {
                use crate::held::InputSource;
                let held = self
                    .keys
                    .iter()
                    .filter(|(from, until, _)| (*from..*until).contains(&frame))
                    .fold(Keys::empty(), |keys, (_, _, more)| keys | *more);
                let up = self.held.difference(held);
                if !up.is_empty() {
                    self.emulator.release(InputSource::Keyboard, up);
                }
                let down = held.difference(self.held);
                if !down.is_empty() {
                    self.emulator.press(InputSource::Keyboard, down);
                }
                self.held = held;
                let touch = self
                    .touches
                    .iter()
                    .find(|(from, until, _)| (*from..*until).contains(&frame))
                    .map(|(_, _, touch)| *touch);
                if touch != self.touch {
                    self.emulator.set_touch(touch);
                    self.touch = touch;
                }
                self.emulator.advance(now, None);
                self.online.update(Instant::now());
                self.emulator.ds_wifi.pump(&mut self.online);
                let refusal = self.emulator.ds_wifi.take_refusal();
                assert!(
                    refusal.is_none(),
                    "R4 p{} f{frame}: the relay refused the DS: {refusal:?}",
                    self.n
                );
                let first = self.tally.borrow().first;
                if !self.told_start
                    && let Some((kind, to)) = first
                {
                    self.told_start = true;
                    println!("R4 p{} CONNECT START f{frame} {kind} {to}", self.n);
                }
                let (trace, flows) = self
                    .emulator
                    .core_mut()
                    .wifi
                    .as_mut()
                    .map(|network| {
                        let gateway = network.gateway_mut();
                        gateway.trace();
                        (gateway.take_trace(), gateway.tcp_peers())
                    })
                    .unwrap_or_default();
                for item in trace {
                    self.watch(frame, item);
                }
                for flow in flows {
                    if let Some(&opened) = self.qr2.opened.get(&(flow.ds_port, flow.to))
                        && flow.up == 0
                        && flow.down == 0
                        && frame > opened + TCP_SILENT
                        && self.qr2.silent.is_none()
                    {
                        self.qr2.silent = Some(format!(
                            "tcp ds {} stream {:?} to {} silent since f{opened}: up 0 B, relayed in {} B, down 0 B, state {:?}, ds segments {:?}",
                            flow.ds_port,
                            flow.stream,
                            flow.to,
                            flow.relayed,
                            flow.state,
                            flow.segments
                        ));
                    }
                }
            }

            fn watch(&mut self, frame: u32, item: citrine_gb_nds_net::gateway::Trace) {
                use citrine_gb_nds_net::gateway::Trace;
                let n = self.n;
                match item {
                    Trace::Datagram {
                        ds_port,
                        peer,
                        up,
                        data,
                    } => {
                        let way = if up { "up" } else { "down" };
                        let natneg = data.starts_with(&NATNEG_MAGIC);
                        if peer.port() != 27_900 && !natneg {
                            let seen = self.qr2.others.entry((ds_port, peer)).or_default();
                            if *seen == (0, 0) {
                                println!(
                                    "R4 p{n} f{frame} udp first {way} ds {ds_port} peer {peer} len {} first byte {:02x?}",
                                    data.len(),
                                    data.first()
                                );
                            }
                            if up {
                                seen.0 += 1;
                            } else {
                                seen.1 += 1;
                            }
                            return;
                        }
                        let what = if natneg {
                            format!("natneg {:02x?}", data.get(7))
                        } else if let Some(kind) = qr2_kind(up, &data) {
                            let seen = if up {
                                &mut self.qr2.up
                            } else {
                                &mut self.qr2.down
                            };
                            *seen.entry(kind).or_default() += 1;
                            if up && kind == 0x01 && self.qr2.replied.is_none() {
                                self.qr2.replied = Some(frame);
                            }
                            if !up && kind == QR2_REGISTERED && self.qr2.registered.is_none() {
                                self.qr2.registered = Some(frame);
                            }
                            format!("qr2 {kind:02x} {}", qr2_name(kind))
                        } else {
                            format!("first byte {:02x?}", data.first())
                        };
                        println!(
                            "R4 p{n} f{frame} udp {way} ds {ds_port} peer {peer} len {} {what}",
                            data.len()
                        );
                        if peer.port() != 27_900 {
                            return;
                        }
                        if up && data.first() == Some(&0x03) {
                            let keys = qr2_keys(data.get(5..).unwrap_or_default());
                            if keys != self.qr2.keys {
                                println!("R4 p{n} f{frame} qr2 keys {keys}");
                                self.qr2.keys = keys;
                            }
                        }
                        if !up && data.starts_with(&[0xFE, 0xFD, 0x01]) {
                            let body = data.get(7..).unwrap_or_default();
                            let end = body.iter().position(|b| *b == 0).unwrap_or(body.len());
                            let challenge = String::from_utf8_lossy(&body[..end]).to_string();
                            println!(
                                "R4 p{n} f{frame} qr2 challenge {} master sees {:?}",
                                challenge.escape_debug(),
                                master_view(&challenge)
                            );
                        }
                        if !up && data.starts_with(&[0xFE, 0xFD, 0x06]) {
                            let inner = data.get(11..).unwrap_or_default();
                            if inner.starts_with(&NATNEG_MAGIC) {
                                println!(
                                    "R4 p{n} f{frame} qr2 client message natneg {:02x?}",
                                    inner.get(7)
                                );
                            }
                        }
                    }
                    Trace::TcpOpen { ds_port, to } => {
                        println!("R4 p{n} f{frame} tcp open ds {ds_port} to {to}");
                    }
                    Trace::TcpOpened { ds_port, to, ok } => {
                        println!("R4 p{n} f{frame} tcp opened ds {ds_port} to {to} ok {ok}");
                        if !ok && self.qr2.tcp_failed.is_none() {
                            self.qr2.tcp_failed = Some(format!("tcp to {to} failed at f{frame}"));
                        }
                        if ok {
                            self.qr2.opened.insert((ds_port, to), frame);
                        }
                    }
                    Trace::TcpState {
                        ds_port,
                        to,
                        state,
                        segments,
                    } => {
                        println!(
                            "R4 p{n} f{frame} tcp state ds {ds_port} to {to} {state:?} after ds segments {segments:?}"
                        );
                    }
                    Trace::TcpClosed {
                        ds_port,
                        to,
                        up,
                        relayed,
                        down,
                    } => {
                        self.qr2.opened.remove(&(ds_port, to));
                        println!(
                            "R4 p{n} f{frame} tcp closed ds {ds_port} to {to} up {up} B relayed in {relayed} B down {down} B"
                        );
                    }
                }
            }

            fn alarm(&self, frame: u32) -> Option<String> {
                if let Some(failed) = &self.qr2.tcp_failed {
                    return Some(failed.clone());
                }
                if let Some(silent) = &self.qr2.silent {
                    return Some(silent.clone());
                }
                match (self.qr2.replied, self.qr2.registered) {
                    (Some(replied), None) if frame > replied + QR2_LATE => Some(format!(
                        "no qr2 registered {QR2_LATE} frames after the challenge reply at f{replied}"
                    )),
                    _ => None,
                }
            }

            fn summary(&mut self) {
                let n = self.n;
                println!(
                    "R4 p{n} SUMMARY qr2 up {} down {} challenge reply {:?} registered {:?}",
                    counts(&self.qr2.up),
                    counts(&self.qr2.down),
                    self.qr2.replied,
                    self.qr2.registered
                );
                for ((ds_port, peer), (up, down)) in &self.qr2.others {
                    println!("R4 p{n} SUMMARY udp ds {ds_port} peer {peer} up {up} down {down}");
                }
                let open = self
                    .emulator
                    .core_mut()
                    .wifi
                    .as_ref()
                    .map(|network| network.gateway().tcp_peers())
                    .unwrap_or_default();
                for flow in open {
                    println!(
                        "R4 p{n} SUMMARY tcp still open ds {} stream {:?} to {} {:?} up {} B relayed in {} B down {} B ds segments {:?}",
                        flow.ds_port,
                        flow.stream,
                        flow.to,
                        flow.state,
                        flow.up,
                        flow.relayed,
                        flow.down,
                        flow.segments
                    );
                }
            }

            fn shot(&mut self, distance: &Distance, frame: u32) {
                const BYTES: usize = 256 * 384 * 4;
                let name = format!("p{}-f{frame:05}", self.n);
                let rgba = self.emulator.frame();
                if rgba.len() >= BYTES {
                    distance.write_png(&rgba[..BYTES], 256, 384, &name);
                }
            }

            fn lease(&mut self) -> Option<std::net::Ipv4Addr> {
                let mac = self.mac;
                let network = self.emulator.core_mut().wifi.as_ref()?;
                network.gateway().lease(mac)
            }

            fn report(&mut self, frame: u32) {
                let mac = self.mac;
                let net = self.emulator.core_mut().wifi.as_ref().map_or_else(
                    || "no network".to_string(),
                    |network| {
                        let gateway = network.gateway();
                        format!(
                            "lease {} dhcp {} lookups {} flows {} relayed {}",
                            gateway
                                .lease(mac)
                                .map_or_else(|| "none".to_string(), |at| at.to_string()),
                            gateway.dhcp_replies(),
                            gateway.nintendo_lookups(),
                            gateway.flows(),
                            gateway.relayed()
                        )
                    },
                );
                let tally = self.tally.borrow();
                let account = self
                    .online
                    .account()
                    .map_or_else(|| "none".to_string(), |a| a.online_id.to_string());
                println!(
                    "R4 p{} f{frame} {net} opens {} ok {} failed {} closed {} up {} B in {} datagrams down {} B in {} datagrams wire {}/{} B errors {:?} online {:?} account {account}",
                    self.n,
                    tally.opens,
                    tally.opened,
                    tally.failed,
                    tally.closed,
                    tally.up,
                    tally.datagrams_up,
                    tally.down,
                    tally.datagrams_down,
                    tally.wire_up,
                    tally.wire_down,
                    tally.errors,
                    self.online.phase()
                );
                drop(tally);
                let peers = self
                    .emulator
                    .core_mut()
                    .wifi
                    .as_ref()
                    .map(|network| network.gateway().udp_peers())
                    .unwrap_or_default();
                for peer in &peers {
                    if !self.peers.contains(peer) {
                        println!(
                            "R4 p{} f{frame} udp ds port {} peer {} up {} down {}",
                            self.n, peer.ds_port, peer.peer, peer.up, peer.down
                        );
                    }
                }
                self.peers = peers;
            }
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        fn stranger(
            peer: &citrine_gb_nds_net::gateway::UdpPeer,
            public_ip: Option<std::net::Ipv4Addr>,
            players: &[std::net::Ipv4Addr],
        ) -> bool {
            const SERVER_PORTS: [u16; 5] = [53, 27_900, 27_901, 27_902, 28_910];
            peer.down > 0
                && Some(*peer.peer.ip()) != public_ip
                && !players.contains(peer.peer.ip())
                && !SERVER_PORTS.contains(&peer.peer.port())
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        #[test]
        fn the_r4_stranger_rule_takes_both_leases_as_players_and_fires_on_any_other_address() {
            use std::net::{Ipv4Addr, SocketAddrV4};
            let mine = Ipv4Addr::new(108, 53, 152, 211);
            let public = Some(mine);
            let leases = [Ipv4Addr::new(10, 77, 1, 1), Ipv4Addr::new(10, 77, 1, 2)];
            let peer = |ip: Ipv4Addr, port: u16, down: u32| citrine_gb_nds_net::gateway::UdpPeer {
                ds_port: 57_495,
                peer: SocketAddrV4::new(ip, port),
                up: 3,
                down,
            };
            assert!(!stranger(&peer(leases[1], 60_603, 2), public, &leases));
            assert!(!stranger(&peer(leases[0], 57_495, 2), public, &leases));
            assert!(!stranger(&peer(mine, 49_509, 2), public, &leases));
            let master = Ipv4Addr::new(95, 217, 77, 151);
            assert!(!stranger(&peer(master, 27_900, 4), public, &leases));
            let other = Ipv4Addr::new(10, 77, 1, 3);
            assert!(!stranger(&peer(other, 60_603, 0), public, &leases));
            assert!(stranger(&peer(other, 60_603, 2), public, &leases));
            let far = Ipv4Addr::new(203, 0, 113, 9);
            assert!(stranger(&peer(far, 60_603, 1), public, &leases));
            assert!(stranger(&peer(far, 60_603, 1), None, &leases));
            assert!(stranger(&peer(leases[1], 60_603, 2), public, &[]));
        }

        #[cfg(all(feature = "nds", ds_wifi))]
        #[test]
        #[ignore]
        fn mario_kart_goes_online_through_the_relay() {
            use crate::app::ui_state::ds_online::{DsOnline, OnlineService};
            const DS_FRAME: Duration = Duration::from_nanos(16_715_000);
            const REPORT_EVERY: u32 = 300;
            const STRANGER_EVERY: u32 = 30;
            let Some(distance) = Distance::from_env("mkwfc") else {
                println!("no CITRINE_WDIST_DIR, skipped");
                return;
            };
            if !distance.dir.join("mk.nds").is_file() {
                println!("no mk.nds in CITRINE_WDIST_DIR, skipped");
                return;
            }
            let var = |name: &str| std::env::var(name).ok();
            let players = var("CITRINE_R4_PLAYERS")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 2);
            let frames = var("CITRINE_R4_FRAMES")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(12000);
            let every = var("CITRINE_R4_SHOT_EVERY")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(100)
                .max(1);
            let mut ds_online = DsOnline::default();
            ds_online.set_service(OnlineService::Kaeru);
            let dns = ds_online.dns();
            assert!(dns.is_some(), "Kaeru has no DNS address");
            let mk = Rom::new(&distance.read("mk.nds"));
            let public_ip =
                var("CITRINE_R4_PUBLIC_IP").and_then(|ip| ip.parse::<std::net::Ipv4Addr>().ok());
            if log::set_logger(&R4Log).is_ok() {
                log::set_max_level(log::LevelFilter::Debug);
            }
            println!("R4 public ip {public_ip:?}");
            let relay = Relay::start(false);
            let threads = Threads::default();
            let mut wfc = Vec::new();
            for (n, (tag, name)) in [("A", "Clara"), ("B", "Mia")]
                .into_iter()
                .enumerate()
                .take(players)
            {
                let n = n + 1;
                let tally = std::rc::Rc::new(std::cell::RefCell::new(Tally::default()));
                let mut online = Online::new(counted(tally.clone()));
                online.probe(
                    relay.address.clone(),
                    PASSWORD.into(),
                    Trust::default(),
                    Instant::now(),
                );
                wait_online(&mut online, &format!("p{n} reaches the relay"), |o| {
                    o.phase() == Phase::Probed
                });
                online.create_account(name.into(), Instant::now());
                wait_online(&mut online, &format!("p{n} signs up"), |o| {
                    o.is_connected() && o.account().is_some()
                });
                let identity = var(format!("CITRINE_R4_IDENTITY_{tag}").as_str())
                    .and_then(|file| crate::ds_wifi::Identity::from_bytes(&distance.read(&file)))
                    .unwrap_or_else(crate::ds_wifi::Identity::random);
                let mac = identity.mac;
                let mut emulator = stored_emulator(&format!("mkwfc-p{n}"));
                emulator.firmware.nds.online = identity.online(dns, false, false);
                emulator.ds_wifi.identity = Some(identity);
                emulator.ds_wifi.spawner = Some(threads.spawner());
                emulator.ds_wifi.online = var("CITRINE_R4_OFFLINE").is_none();
                emulator.ds_wifi.direct = false;
                emulator
                    .load_game(&mk, ConsoleKind::Nds, None)
                    .expect("Mario Kart loads");
                let save = var(format!("CITRINE_R4_SAVE_{tag}").as_str());
                if let Some(save) = &save {
                    let imported = emulator
                        .import_save(Some(&distance.read(save)), None)
                        .expect("the save loads");
                    assert_eq!(imported, crate::emulator::SaveImport::Imported);
                }
                let script = var(format!("CITRINE_R4_SCRIPT_{tag}").as_str())
                    .unwrap_or_else(|| "r4/mk.txt".into());
                let text = String::from_utf8(distance.read(&script)).expect("a text script");
                let (keys, touches) = lab_lines(&text);
                println!(
                    "R4 p{n} {name} account {} mac {mac:02x?} script {script} save {save:?}",
                    online.account().map(|a| a.online_id.value()).unwrap_or(0)
                );
                wfc.push(Wfc {
                    n,
                    emulator,
                    online,
                    tally,
                    mac,
                    keys,
                    touches,
                    held: Keys::empty(),
                    touch: None,
                    told_start: false,
                    peers: Vec::new(),
                    qr2: Qr2Watch::default(),
                });
            }
            let started = Instant::now();
            let mut now = started;
            for frame in 1..=frames {
                now += DS_FRAME;
                for player in &mut wfc {
                    player.frame(frame, now);
                }
                let last = frame == frames;
                if frame % every == 0 || last {
                    for player in &mut wfc {
                        player.shot(&distance, frame);
                    }
                }
                if frame % REPORT_EVERY == 0 || last {
                    for player in &mut wfc {
                        player.report(frame);
                    }
                    println!("R4 f{frame} after {:?}", started.elapsed());
                }
                let ours: Vec<_> = wfc.iter_mut().filter_map(Wfc::lease).collect();
                if frame % STRANGER_EVERY == 0
                    && let Some((n, peer)) = wfc.iter_mut().find_map(|player| {
                        let peers = player
                            .emulator
                            .core_mut()
                            .wifi
                            .as_ref()
                            .map(|network| network.gateway().udp_peers())
                            .unwrap_or_default();
                        peers
                            .into_iter()
                            .find(|peer| stranger(peer, public_ip, &ours))
                            .map(|peer| (player.n, peer))
                    })
                {
                    println!(
                        "R4 p{n} f{frame} STRANGER {} up {} down {}, leaving",
                        peer.peer, peer.up, peer.down
                    );
                    for player in &mut wfc {
                        player.shot(&distance, frame);
                        player.report(frame);
                    }
                    break;
                }
                if let Some((n, why)) = wfc
                    .iter()
                    .find_map(|player| player.alarm(frame).map(|why| (player.n, why)))
                {
                    println!("R4 p{n} f{frame} STOP {why}, leaving");
                    for player in &mut wfc {
                        player.shot(&distance, frame);
                        player.report(frame);
                    }
                    break;
                }
                let real = Instant::now();
                if now > real {
                    std::thread::sleep(now - real);
                }
            }
            for player in &mut wfc {
                player.summary();
                let n = player.n;
                if let Some(identity) = &player.emulator.ds_wifi.identity {
                    let file = distance.out.join(format!("p{n}.identity"));
                    std::fs::write(file, identity.to_bytes()).unwrap();
                }
                if let Some(save) = player.emulator.export_save_bytes() {
                    std::fs::write(distance.out.join(format!("p{n}.sav")), save).unwrap();
                }
            }
        }
    }
    mod rooms {
        use super::*;
        #[cfg(feature = "gba")]
        use crate::app::tabs::playing::group_link;
        use crate::app::tabs::playing::{
            MemberAction, PlayingUi, RoomAction, RoomFacts, invite_closed, invite_note,
            member_menu, settle_room, switch_lines,
        };
        use crate::emulator::Emulator;
        use citrine_gb_net::{Body, OLDEST_PROTOCOL_VERSION};

        struct Group {
            sides: Vec<Side>,
            _relay: Relay,
            clock: Clock,
        }

        impl Group {
            fn new(names: &[&str], old: &[usize]) -> Self {
                let relay = Relay::start(false);
                let clock = Clock::new();
                let mut sides: Vec<Side> = names
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        Side::new(
                            (30, 10),
                            11 + 18 * i as u64,
                            Box::new(shelf::Homebrew),
                            &clock,
                        )
                    })
                    .collect();
                for &i in old {
                    sides[i].online.speak(OLDEST_PROTOCOL_VERSION);
                }
                let mut group = Self {
                    sides,
                    _relay: relay,
                    clock,
                };
                let now = group.clock.now();
                for side in &mut group.sides {
                    side.online.probe(
                        group._relay.address.clone(),
                        PASSWORD.into(),
                        Trust::default(),
                        now,
                    );
                }
                group.until("all probed", |g| {
                    g.sides.iter().all(|s| s.online.phase() == Phase::Probed)
                });
                let now = group.clock.now();
                for (side, name) in group.sides.iter_mut().zip(names) {
                    side.online
                        .create_invited_account((*name).into(), None, now);
                }
                group.until("all signed up", |g| {
                    g.sides
                        .iter()
                        .all(|s| s.online.is_connected() && s.online.account().is_some())
                });
                for i in 1..group.sides.len() {
                    group.befriend(0, i);
                }
                group
            }

            fn id(&self, i: usize) -> OnlineId {
                self.sides[i].id()
            }

            fn befriend(&mut self, a: usize, b: usize) {
                let (a_id, b_id) = (self.id(a), self.id(b));
                assert!(self.sides[a].online.add_friend(b_id));
                self.until("the request arrives", |g| {
                    g.sides[b].online.friend(a_id).map(|f| f.state) == Some(FriendState::Incoming)
                });
                assert!(self.sides[b].online.answer(a_id, true));
                self.until("they are friends", |g| {
                    g.sides[a].online.friend(b_id).map(|f| f.state) == Some(FriendState::Friend)
                        && g.sides[b].online.friend(a_id).map(|f| f.state)
                            == Some(FriendState::Friend)
                });
            }

            fn until(&mut self, what: &str, mut done: impl FnMut(&Group) -> bool) {
                let begun = self.clock.now();
                while !done(self) {
                    self.clock.waited(begun, what);
                    let now = self.clock.tick();
                    for side in &mut self.sides {
                        side.step(now);
                    }
                }
            }

            fn invite(&mut self, from: usize, to: usize, game: Option<GameFile>) {
                self.invite_as(from, to, game, snake());
            }

            fn invite_as(
                &mut self,
                from: usize,
                to: usize,
                game: Option<GameFile>,
                theirs: GameFile,
            ) {
                let to_id = self.id(to);
                let side = &mut self.sides[from];
                assert!(side.play.invite(&mut side.online, to_id, game));
                self.until("the invite arrives", |g| {
                    !g.sides[to].play.invites().is_empty()
                });
                let session = self.sides[to].play.invites()[0].session;
                let side = &mut self.sides[to];
                assert!(
                    side.play
                        .accept(&mut side.online, session, MyGame::Own(theirs))
                );
            }

            fn pair_of_two(names: &[&str], old: &[usize]) -> Self {
                let mut group = Self::new(names, old);
                group.invite(0, 1, Some(snake()));
                group.until("both play", |g| {
                    g.sides[0].stage() == Some(Stage::Playing)
                        && g.sides[1].stage() == Some(Stage::Playing)
                });
                group
            }

            fn room(&self, i: usize) -> &Room {
                self.sides[i].play.room().unwrap()
            }

            fn members(&self, i: usize) -> Vec<OnlineId> {
                let mut who: Vec<OnlineId> = self.room(i).others().collect();
                who.sort();
                who
            }

            fn others(&self, i: usize) -> Vec<OnlineId> {
                let mut who: Vec<OnlineId> = (0..self.sides.len())
                    .filter(|j| *j != i)
                    .map(|j| self.id(j))
                    .collect();
                who.sort();
                who
            }

            fn frames_run(&mut self, frames: u32) {
                self.frames_run_by(0, 1, frames);
            }

            fn frames_run_by(&mut self, a: usize, b: usize, frames: u32) {
                let target = self.sides[a].frame().max(self.sides[b].frame()) + frames;
                self.until("the pair ran the frames", |g| {
                    g.sides[a].frame() >= target && g.sides[b].frame() >= target
                });
            }

            fn linked(&self, a: usize, b: usize) -> bool {
                [(a, b), (b, a)].iter().all(|&(i, j)| {
                    self.sides[i].stage() == Some(Stage::Playing)
                        && self.sides[i].session().friend() == self.id(j)
                        && self.sides[i].session().mode() == Some(Mode::Rollback)
                })
            }

            fn ask(&mut self, from: usize, to: usize) {
                let to_id = self.id(to);
                let from_id = self.id(from);
                assert!(self.sides[from].play.link(to_id));
                self.until("the request is held", |g| {
                    g.room(to).links().asking().contains(&from_id)
                });
            }
        }

        fn three() -> Group {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[]);
            group.frames_run(60);
            group.invite(0, 2, None);
            group.until("everyone sees three", |g| {
                (0..3).all(|i| g.sides[i].play.room().is_some() && g.members(i) == g.others(i))
            });
            group
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_gba_member_plays_the_game_boy_game_to_link_and_gets_their_own_back() {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[]);
            group.frames_run(60);
            group.invite_as(0, 2, None, super::link_words());
            let (mia, jonas) = (group.id(1), group.id(2));
            group.until("Jonas plays his GBA game apart", |g| {
                g.sides[2].stage() == Some(Stage::Playing)
                    && g.room(2)
                        .pair(mia)
                        .is_some_and(|p| p.friend_game().is_some())
            });
            let own = super::link_words().card().unwrap();
            assert!(group.sides[2].play.play_theirs(mia, MyGame::Own(snake())));
            assert!(group.sides[0].play.unlink());
            group.until("Mia and Jonas link in Snake", |g| g.linked(1, 2));
            let snake_card = snake().card().unwrap();
            assert_eq!(group.sides[2].session().my_game(), Some(&snake_card));
            assert_eq!(group.sides[2].play.shelved(), Some(&own));
            assert!(group.sides[2].session().shelves_mine());
            group.frames_run_by(1, 2, 120);
            assert_same_inputs(group.sides[1].session(), group.sides[2].session());
            group.sides[2].handed_back = None;
            assert!(group.sides[2].play.unlink());
            group.until("Jonas is back in his own game", |g| {
                g.room(2).links().group_of(jonas).is_none()
                    && g.room(2)
                        .pair(mia)
                        .is_some_and(|p| p.is_apart() && p.my_game() == Some(&own))
            });
            assert!(group.sides[2].play.shelved().is_none());
            assert_eq!(group.sides[2].handed_back, None);
            group.until("Mia sees Jonas's own game again", |g| {
                g.room(1)
                    .pair(jonas)
                    .is_some_and(|p| p.friend_game() == Some(&own))
            });
        }

        #[cfg(feature = "gba")]
        fn gba_group() -> Group {
            gba_group_of(&["Clara", "Mia", "Jonas"])
        }

        #[cfg(feature = "gba")]
        fn gba_group_of(names: &[&str]) -> Group {
            let mut group = Group::new(names, &[]);
            for side in &mut group.sides {
                let rom = super::link_words().rom;
                side.own_gba = Some(Box::new(citrine_gb::gba::machine::Gba::new(&rom)));
            }
            group.invite_as(0, 1, Some(super::link_words()), super::link_words());
            group.until("Clara and Mia link", |g| g.linked(0, 1));
            group.invite_as(0, 2, None, super::link_words());
            group.until("everyone sees three", |g| {
                (0..3).all(|i| {
                    g.sides[i]
                        .play
                        .room()
                        .is_some_and(|r| r.others().count() == 2)
                })
            });
            let clara = group.id(0);
            assert!(group.sides[2].play.link(clara));
            group.until("all three run one group engine", |g| {
                (0..3).all(|i| {
                    g.room(i)
                        .group_engine()
                        .is_some_and(|r| r.units() == [0, 1, 2])
                })
            });
            for i in 0..3 {
                let linked = group.room(i).links().group_of(clara).unwrap();
                assert_eq!(linked.members(), [clara, group.id(1), group.id(2)]);
            }
            for i in 3..names.len() {
                group.invite_as(0, i, None, super::link_words());
            }
            group.until("everyone sees everyone", |g| {
                (0..g.sides.len())
                    .all(|i| g.sides[i].play.room().is_some() && g.members(i) == g.others(i))
            });
            group
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_group_start_builds_its_engine_while_the_members_step() {
            let mut group = Group::new(&["Clara", "Mia", "Jonas"], &[]);
            for side in &mut group.sides {
                let rom = super::link_words().rom;
                side.own_gba = Some(Box::new(citrine_gb::gba::machine::Gba::new(&rom)));
                side.threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
            }
            group.invite_as(0, 1, Some(super::link_words()), super::link_words());
            group.until("Clara and Mia link", |g| g.linked(0, 1));
            group.invite_as(0, 2, None, super::link_words());
            group.until("everyone sees three", |g| {
                (0..3).all(|i| {
                    g.sides[i]
                        .play
                        .room()
                        .is_some_and(|r| r.others().count() == 2)
                })
            });
            let clara = group.id(0);
            assert!(group.sides[2].play.link(clara));
            let mut building = [0; 3];
            group.until("all three run one group engine", |g| {
                for (i, count) in building.iter_mut().enumerate() {
                    if g.room(i).builds_group() {
                        *count += 1;
                    }
                }
                (0..3).all(|i| {
                    g.room(i)
                        .group_engine()
                        .is_some_and(|r| r.units() == [0, 1, 2])
                })
            });
            assert!(building.iter().all(|count| *count > 0), "{building:?}");
            group.group_runs(&[0, 1, 2], 60);
        }

        #[cfg(feature = "gba")]
        struct Held(std::sync::Arc<[u8]>);

        #[cfg(feature = "gba")]
        impl Shelf for Held {
            fn find(&self, hash: &citrine_gb_net::GameHash) -> Option<std::sync::Arc<[u8]>> {
                (super::sha1_of(&self.0) == Some(*hash)).then(|| self.0.clone())
            }
        }

        #[cfg(feature = "gba")]
        #[test]
        fn drawing_a_watch_never_calls_its_engine_home() {
            let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
            group.group_runs(&[0, 1, 2], 60);
            group.sides[3]
                .play
                .set_shelf(Box::new(Held(super::link_words().rom)));
            let mia = group.id(1);
            assert!(group.sides[3].play.watch_group(mia, true));
            group.until("Lena sees Mia's screen", |g| {
                g.room(3).watched_picture().is_some()
            });
            let threads = citrine_gb_threads::Mode::from_env() == citrine_gb_threads::Mode::Threads;
            for frame in 0..30 {
                let now = group.clock.tick();
                for side in &mut group.sides {
                    side.step(now);
                }
                let room = group.room(3);
                let home = room.watch_home();
                assert!(!(threads && home), "frame {frame}");
                assert!(room.watched_picture().is_some(), "frame {frame}");
                assert_eq!(room.watch_home(), home, "frame {frame}");
            }
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_watch_looks_up_its_game_off_the_ui_thread() {
            let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
            group.group_runs(&[0, 1, 2], 60);
            let (shelf, lookups) = super::watched(super::link_words().rom);
            group.sides[3].threads =
                citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
            group.sides[3].play.set_shelf(Box::new(shelf));
            let mia = group.id(1);
            assert!(group.sides[3].play.watch_group(mia, true));
            group.until("Lena sees Mia's screen", |g| {
                g.room(3).watched_picture().is_some()
            });
            assert!(!group.room(3).watch_missing());
            assert!(super::off_this_thread(&lookups));
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_watch_of_a_game_off_the_shelf_shows_it_missing() {
            let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
            group.group_runs(&[0, 1, 2], 60);
            group.sides[3].play.set_shelf(Box::new(super::Nothing));
            let mia = group.id(1);
            assert!(group.sides[3].play.watch_group(mia, true));
            group.until("Lena's watch lacks the game", |g| g.room(3).watch_missing());
            assert!(group.room(3).watched_picture().is_none());
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_member_outside_the_group_watches_it_and_sees_the_same_frames() {
            let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
            group.group_runs(&[0, 1, 2], 60);
            group.sides[3]
                .play
                .set_shelf(Box::new(Held(super::link_words().rom)));
            let mia = group.id(1);
            let lena = group.id(3);
            assert!(group.sides[3].play.watch_group(mia, true));
            group.until("Lena sees Mia's screen", |g| {
                g.room(3).watched_picture().is_some()
            });
            group.until("Clara lists Lena as a watcher", |g| {
                g.room(0)
                    .links()
                    .group_of(g.id(0))
                    .is_some_and(|l| l.watchers().contains(&lena))
            });
            let start = group.room(3).watch_engine().unwrap().frame();
            group.until("Lena follows 180 frames", |g| {
                g.room(3)
                    .watch_engine()
                    .is_some_and(|r| r.frame() >= start + 180)
            });
            let theirs: HashMap<u32, Vec<u64>> = group
                .room(1)
                .group_engine()
                .unwrap()
                .log
                .group_checks
                .iter()
                .cloned()
                .collect();
            let mut compared = 0;
            for (frame, hashes) in &group.room(3).watch_engine().unwrap().log.group_checks {
                if let Some(mia_saw) = theirs.get(frame) {
                    assert_eq!(hashes[..3], mia_saw[..3], "check of frame {frame}");
                    compared += 1;
                }
            }
            assert!(compared >= 2, "{compared} checks compared");
            assert_eq!(group.room(3).watching(), Some(mia));
            assert!(group.sides[3].play.watch_group(mia, false));
            group.until("Clara drops Lena from the watchers", |g| {
                g.room(0)
                    .links()
                    .group_of(g.id(0))
                    .is_some_and(|l| !l.watchers().contains(&lena))
            });
        }

        #[cfg(feature = "gba")]
        impl Group {
            fn group_frame(&self, i: usize) -> u32 {
                self.room(i).group_engine().map_or(0, |r| r.frame())
            }

            fn group_confirmed(&self, i: usize) -> u32 {
                self.room(i)
                    .group_engine()
                    .map_or(0, |r| r.confirmed_frame())
            }

            fn group_resyncs(&self, i: usize) -> u32 {
                self.room(i).group_engine().map_or(0, |r| r.resyncs())
            }

            fn leave_at_confirmed(&mut self, i: usize, unit: u8) -> Option<u32> {
                let engine = self.sides[i].play.room.as_mut()?.group_engine_mut()?;
                let at = engine.confirmed_frame();
                engine.leave_unit(unit, at)
            }

            fn until_on(
                &mut self,
                sides: &[usize],
                what: &str,
                mut done: impl FnMut(&Group) -> bool,
            ) {
                let begun = self.clock.now();
                while !done(self) {
                    self.clock.waited(begun, what);
                    let now = self.clock.tick();
                    for &i in sides {
                        self.sides[i].step(now);
                    }
                }
            }

            fn group_runs(&mut self, sides: &[usize], frames: u32) {
                let target = sides
                    .iter()
                    .map(|&i| self.group_frame(i))
                    .max()
                    .unwrap_or(0)
                    + frames;
                self.until("the group ran the frames", |g| {
                    sides.iter().all(|&i| g.group_frame(i) >= target)
                });
            }
        }

        #[cfg(feature = "gba")]
        #[test]
        fn the_group_engine_draws_the_partner_screens_a_member_watches() {
            let mut group = gba_group();
            group.group_runs(&[0, 1, 2], 30);
            for i in 0..3 {
                let session = group.sides[i].session();
                assert!(session.sees_friend(), "member {i} sees a partner");
                assert!(
                    group.room(i).group_engine().unwrap().watches(),
                    "member {i}"
                );
            }
            group.sides[0].session_mut().set_watch_friend(false);
            assert!(!group.room(0).group_engine().unwrap().watches());
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_group_member_names_every_partner_and_the_slowest_link() {
            let mut group = gba_group();
            group.group_runs(&[0, 1, 2], 30);
            let side = &group.sides[1];
            let (partners, slowest) = group_link(&side.play, &side.online).unwrap();
            assert_eq!(partners, "Clara and Jonas");
            let facts = RoomFacts::of(&side.play, &side.online, 0).unwrap();
            assert_eq!(slowest, facts.partners().iter().filter_map(|p| p.ms).max());
        }

        #[cfg(feature = "gba")]
        #[test]
        fn three_gba_members_link_into_one_group_and_play_one_game() {
            let mut group = gba_group();
            group.until("the group ran 240 frames", |g| {
                (0..3).all(|i| g.room(i).group_engine().is_some_and(|r| r.frame() >= 240))
            });
            let logs: Vec<engine::Log> = (0..3)
                .map(|i| group.room(i).group_engine().unwrap().log.clone())
                .collect();
            for unit in 0..3 {
                for frame in 0..240 {
                    let inputs: Vec<_> = logs
                        .iter()
                        .filter_map(|log| log.units[unit].get(frame).cloned().flatten())
                        .collect();
                    assert!(
                        inputs.windows(2).all(|w| w[0] == w[1]),
                        "unit {unit} frame {frame}"
                    );
                }
            }
            let checks: Vec<HashMap<u32, Vec<u64>>> = logs
                .iter()
                .map(|log| log.group_checks.iter().cloned().collect())
                .collect();
            let mut compared = 0;
            for (frame, hashes) in &checks[0] {
                for other in &checks[1..] {
                    if let Some(theirs) = other.get(frame) {
                        assert_eq!(hashes, theirs, "check of frame {frame}");
                        compared += 1;
                    }
                }
            }
            assert!(compared > 0);
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_member_who_unlinks_gets_their_game_back_and_the_rest_play_on() {
            let mut group = gba_group();
            group.group_runs(&[0, 1, 2], 60);
            let jonas = group.id(2);
            assert!(group.sides[2].play.unlink());
            assert!(group.room(2).group_engine().is_none());
            assert!(!group.sides[2].session().in_group());
            group.until("Clara and Mia see Jonas unlinked", |g| {
                (0..2).all(|i| {
                    g.room(i)
                        .links()
                        .group_of(g.id(0))
                        .is_some_and(|l| !l.holds(jonas))
                })
            });
            group.group_runs(&[0, 1], 120);
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_member_who_stops_answering_can_be_unlinked_by_anyone() {
            let mut group = gba_group();
            group.group_runs(&[0, 1, 2], 60);
            let jonas = group.id(2);
            group.sides[2].down.set(true);
            group.until("Clara waits for Jonas", |g| {
                g.room(0)
                    .group_waiting_for(g.clock.now())
                    .is_some_and(|(who, _)| who == jonas)
            });
            assert!(group.sides[1].play.unlink_member(jonas));
            group.until("Clara sees Jonas unlinked", |g| {
                g.room(0)
                    .links()
                    .group_of(g.id(0))
                    .is_some_and(|l| !l.holds(jonas))
            });
            group.group_runs(&[0, 1], 120);
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_member_who_pauses_shows_as_paused_to_the_group() {
            let mut group = gba_group();
            let mia = group.id(1);
            group.sides[1].play.set_status(true, false);
            group.until("Clara sees Mia paused", |g| {
                g.room(0).paused_members().contains(&mia)
            });
            group.sides[1].play.set_status(false, true);
            group.until("Clara sees Mia away", |g| {
                !g.room(0).paused_members().contains(&mia)
                    && g.room(0).away_members().contains(&mia)
            });
        }

        #[test]
        fn a_third_member_joins_while_the_pair_keeps_playing() {
            let mut group = three();
            let round = group.sides[0].session().round();
            let jonas = group.id(2);
            group.until("Jonas plays apart with Clara", |g| {
                g.sides[2].stage() == Some(Stage::Playing)
            });
            assert_eq!(group.sides[2].session().friend(), group.id(0));
            assert_eq!(group.sides[2].session().mode(), Some(Mode::Apart));
            assert_eq!(group.room(2).member(group.id(1)).unwrap().name, "Mia");
            assert_eq!(group.room(1).member(jonas).unwrap().name, "Jonas");
            group.frames_run(120);
            assert_eq!(group.sides[0].session().round(), round);
            assert_eq!(group.sides[1].session().round(), round);
            assert_eq!(group.sides[0].session().friend(), group.id(1));
            assert_eq!(group.sides[0].session().mode(), Some(Mode::Rollback));
            assert_same_inputs(group.sides[0].session(), group.sides[1].session());
        }

        #[test]
        fn a_game_change_reaches_every_member() {
            let mut group = three();
            let clara = group.id(0);
            group.until("Jonas plays apart", |g| {
                g.sides[2].stage() == Some(Stage::Playing)
            });
            let card = super::custom().card().unwrap();
            assert!(group.sides[0].play.bring(MyGame::Own(super::custom())));
            group.until("Mia and Jonas see the new game", |g| {
                (1..3).all(|i| {
                    g.room(i)
                        .pair(clara)
                        .is_some_and(|p| p.friend_game() == Some(&card))
                })
            });
        }

        #[test]
        fn joining_from_a_second_device_ends_the_room_on_the_first() {
            let mut group = three();
            let room = group.room(2).id().unwrap();
            let jonas = group.id(2);
            let side = &mut group.sides[2];
            side.play.receive(
                &mut side.online,
                ServerMsg::MemberGone {
                    room,
                    who: jonas,
                    reason: EndReason::Left,
                },
                group.clock.now(),
            );
            assert_eq!(group.sides[2].stage(), Some(Stage::Ended(Ending::Moved)));
            assert!(!group.sides[2].play.is_live());
        }

        #[test]
        fn hiding_my_screen_hides_it_from_everyone_in_the_room() {
            let mut group = three();
            let clara = group.id(0);
            group.until("both see Clara's screen", |g| {
                [1, 2].iter().all(|&i| {
                    g.room(i)
                        .pair(clara)
                        .is_some_and(|p| p.friend_lets_me_watch())
                })
            });
            group.sides[0].play.set_let_watch(false);
            group.until("nobody sees Clara's screen", |g| {
                [1, 2].iter().all(|&i| {
                    g.room(i)
                        .pair(clara)
                        .is_some_and(|p| !p.friend_lets_me_watch())
                })
            });
            let room = group.room(0);
            assert!(
                room.others()
                    .all(|w| !room.pair(w).unwrap().lets_friend_watch())
            );
        }

        #[test]
        fn an_invite_from_another_room_asks_to_leave_this_one_first() {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas", "Ana"], &[]);
            group.frames_run(60);
            group.invite(0, 2, None);
            let clara = group.id(0);
            group.until("everyone sees three", |g| {
                (0..3).all(|i| {
                    g.sides[i]
                        .play
                        .room()
                        .is_some_and(|r| r.members().len() == 2)
                })
            });
            let side = &mut group.sides[3];
            assert!(side.play.invite(&mut side.online, clara, Some(snake())));
            group.until("Ana's invite arrives", |g| {
                !g.sides[0].play.invites().is_empty()
            });
            let side = &mut group.sides[0];
            let invite = side.play.invites()[0].clone();
            assert!(
                !side
                    .play
                    .accept(&mut side.online, invite.session, MyGame::Own(snake()))
            );
            let lines = switch_lines(&side.play, &side.online, "Ana").unwrap();
            assert_eq!(lines.0, "Switch to Ana?");
            assert!(
                lines.1 == "Leave the room with Mia and Jonas?"
                    || lines.1 == "Leave the room with Jonas and Mia?",
                "{}",
                lines.1
            );
            side.play.leave(&mut side.online);
            assert!(!side.play.is_live());
            side.play.close();
            assert!(
                side.play
                    .accept(&mut side.online, invite.session, MyGame::Own(snake()))
            );
            group.until("Clara plays with Ana", |g| {
                g.sides[0].stage() == Some(Stage::Playing)
                    && g.sides[0].session().friend() == g.id(3)
            });
            group.until("Mia and Jonas stay together", |g| {
                [1, 2]
                    .iter()
                    .all(|&i| g.room(i).members().len() == 1 && g.sides[i].play.is_live())
            });
        }

        #[test]
        fn a_member_who_is_not_a_friend_can_be_added_from_the_room() {
            let mut group = three();
            let (mia, jonas) = (group.id(1), group.id(2));
            let side = &group.sides[2];
            let facts = RoomFacts::of(&side.play, &side.online, 0).unwrap();
            let person = facts.person(mia).unwrap();
            assert_eq!((person.name.as_str(), person.friend), ("Mia", None));
            let menu = member_menu(&facts, mia);
            assert!(menu.add_friend && !menu.message && !menu.profile);
            let side = &mut group.sides[2];
            let go = settle_room(
                RoomAction::Member(mia, MemberAction::AddFriend),
                &mut side.play,
                &mut side.online,
                &Emulator::default(),
                &mut PlayingUi::default(),
            );
            assert!(go.is_none());
            group.until("Mia gets Jonas's request", |g| {
                g.sides[1].online.friend(jonas).map(|f| f.state) == Some(FriendState::Incoming)
            });
        }

        #[test]
        fn watching_a_member_whose_game_i_lack_asks_for_my_copy() {
            let mut group = Group::new(&["Clara", "Mia", "Jonas"], &[]);
            group.invite_as(0, 1, Some(snake()), super::custom());
            group.until("both play", |g| {
                (0..2).all(|i| g.sides[i].stage() == Some(Stage::Playing))
            });
            group.invite(0, 2, None);
            let mia = group.id(1);
            group.until("Clara sees the room", |g| g.room(0).members().len() == 2);
            let side = &group.sides[0];
            let facts = RoomFacts::of(&side.play, &side.online, 0).unwrap();
            let watch = member_menu(&facts, mia).watch.unwrap();
            assert!(watch.missing && watch.copy);
            assert_eq!(watch.why_off.as_deref(), Some("Needs My Snake"));
            let side = &mut group.sides[0];
            let mut ui = PlayingUi::default();
            settle_room(
                RoomAction::Member(mia, MemberAction::OpenCopy),
                &mut side.play,
                &mut side.online,
                &Emulator::default(),
                &mut ui,
            );
            assert!(ui.wants_copy);
        }

        #[test]
        fn room_chat_reaches_every_member() {
            let mut group = three();
            let jonas_side = &mut group.sides[2];
            assert!(
                jonas_side
                    .play
                    .say(&mut jonas_side.online, "hello room".into())
            );
            let jonas = group.id(2);
            group.until("Clara and Mia read it", |g| {
                [0, 1].iter().all(|&i| {
                    g.room(i).chat().iter().any(|line| {
                        line.from == jonas && line.envelope.body == Body::Text("hello room".into())
                    })
                })
            });
            assert_eq!(group.room(2).chat().len(), 1);
        }

        #[test]
        fn a_dropped_member_rejoins_and_the_pair_plays_on() {
            let mut group = Group::pair_of_two(&["Clara", "Mia"], &[]);
            group.frames_run(60);
            let round = group.sides[0].session().round();
            let mia = group.id(1);
            group.sides[1].cut.set(true);
            group.until("Clara sees Mia reconnecting", |g| {
                g.room(0).member(mia).is_some_and(|m| m.state.reconnecting)
            });
            assert_eq!(group.sides[0].stage(), Some(Stage::Playing));
            group.until("Mia is back", |g| {
                !g.room(1).rejoining()
                    && g.room(0).member(mia).is_some_and(|m| !m.state.reconnecting)
            });
            group.until("both play a new round", |g| {
                (0..2).all(|i| {
                    g.sides[i].stage() == Some(Stage::Playing)
                        && g.sides[i].session().round() > round
                })
            });
            group.frames_run(60);
            assert_same_inputs(group.sides[0].session(), group.sides[1].session());
        }

        #[test]
        fn a_pair_dropped_in_a_room_of_three_hands_back_a_paused_console() {
            let mut group = three();
            group.until("Clara and Mia play linked", |g| g.linked(0, 1));
            group.frames_run(60);
            let mia = group.id(1);
            group.sides[0].handed_back = None;
            group.sides[1].down.set(true);
            group.until("Clara gets her console back", |g| {
                g.sides[0].handed_back.is_some_and(|(_, paused)| paused)
            });
            assert_eq!(group.sides[0].handed_back, Some((mia, true)));
            assert!(group.sides[0].play.is_live());
        }

        #[test]
        fn an_old_partner_still_plays_a_pair() {
            let mut group = Group::pair_of_two(&["Clara", "Mia"], &[1]);
            group.frames_run(120);
            assert_eq!(group.sides[0].session().mode(), Some(Mode::Rollback));
            assert_same_inputs(group.sides[0].session(), group.sides[1].session());
            let mia = &mut group.sides[1];
            mia.play.leave(&mut mia.online);
            group.until("Clara sees Mia leave", |g| {
                g.sides[0].stage() == Some(Stage::Ended(Ending::FriendLeft))
            });
        }

        #[test]
        fn a_link_request_waits_until_the_pair_unlinks() {
            let mut group = three();
            group.until("Jonas plays apart", |g| {
                g.sides[2].stage() == Some(Stage::Playing)
            });
            group.ask(2, 1);
            group.frames_run(60);
            assert!(group.linked(0, 1));
            let (mia, jonas) = (group.id(1), group.id(2));
            assert!(group.sides[0].play.unlink());
            group.until("Mia and Jonas link", |g| g.linked(1, 2));
            for i in 0..3 {
                let room = group.room(i);
                assert_eq!(room.links().groups().len(), 1);
                assert_eq!(room.links().group_of(jonas).unwrap().authority(), mia);
            }
            assert_eq!(group.sides[1].session().role(), Role::Host);
            assert_eq!(group.sides[2].session().role(), Role::Guest);
            assert!(group.room(0).pair(mia).unwrap().is_apart());
            group.frames_run_by(1, 2, 120);
            assert_same_inputs(group.sides[1].session(), group.sides[2].session());
        }

        #[test]
        fn the_next_in_line_links_when_the_authority_leaves() {
            let mut group = three();
            group.until("Jonas plays apart", |g| {
                g.sides[2].stage() == Some(Stage::Playing)
            });
            group.ask(2, 1);
            let clara = group.id(0);
            let mia = group.id(1);
            let side = &mut group.sides[0];
            side.play.leave(&mut side.online);
            group.until("Mia and Jonas link", |g| g.linked(1, 2));
            assert!(group.room(1).member(clara).is_none());
            assert_eq!(
                group.room(2).links().group_of(mia).unwrap().authority(),
                mia
            );
            group.frames_run_by(1, 2, 120);
            assert_same_inputs(group.sides[1].session(), group.sides[2].session());
        }

        #[test]
        fn taking_a_waiting_request_unlinks_and_links_with_the_asker() {
            let mut group = three();
            group.until("Jonas plays apart", |g| {
                g.sides[2].stage() == Some(Stage::Playing)
            });
            group.ask(2, 1);
            let jonas = group.id(2);
            assert!(group.sides[1].play.take_link(jonas));
            group.until("Mia and Jonas link", |g| g.linked(1, 2));
            assert!(group.room(1).links().asking().is_empty());
            assert!(group.room(0).links().group_of(group.id(0)).is_none());
        }

        #[test]
        fn an_invite_an_old_partner_blocks_names_the_friend_it_was_for() {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[1]);
            group.frames_run(60);
            let jonas = group.id(2);
            let side = &mut group.sides[0];
            assert!(side.play.invite(&mut side.online, jonas, None));
            group.until("the relay answers", |g| g.room(0).invite_error().is_some());
            assert_eq!(group.room(0).invite_error(), Some(ErrorCode::NeedsUpdate));
            assert_eq!(group.room(0).invite_failed(), Some(jonas));
            assert!(group.room(0).inviting().is_empty());
        }

        #[test]
        fn a_room_of_two_keeps_the_pair_bar_and_three_get_the_room_bar() {
            let pair = Group::pair_of_two(&["Clara", "Mia"], &[]);
            assert!(RoomFacts::of(&pair.sides[0].play, &pair.sides[0].online, 0).is_none());
            let group = three();
            let facts = RoomFacts::of(&group.sides[0].play, &group.sides[0].online, 0).unwrap();
            assert_eq!(facts.people.len(), 3);
        }

        #[test]
        fn an_old_member_closes_the_invite_before_anyone_tries() {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[1]);
            let (mia, jonas) = (group.id(1), group.id(2));
            group.until("Clara knows Mia's client is old", |g| {
                g.room(0).old_client() == Some(mia)
            });
            assert_eq!(group.room(0).invite_error(), None);
            let side = &group.sides[0];
            assert_eq!(
                invite_note(&side.play, &side.online, jonas).as_deref(),
                Some("Can't invite until Mia updates Citrine")
            );
            assert!(invite_closed(&side.play, &side.online, jonas));
        }

        #[test]
        fn a_full_room_greys_play_together_before_anyone_tries() {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[]);
            let jonas = group.id(2);
            let side = &group.sides[0];
            assert_eq!(invite_note(&side.play, &side.online, jonas), None);
            let me = side.id();
            let mut list = group.room(0).members().to_vec();
            let template = list[0].clone();
            while list.len() + 1 < citrine_gb_net::ROOM_MEMBERS {
                list.push(RoomMember {
                    who: OnlineId::new(1000 + list.len() as u64).unwrap(),
                    ..template.clone()
                });
            }
            let members: Vec<citrine_gb_net::Member> = list
                .into_iter()
                .map(|m| citrine_gb_net::Member {
                    who: m.who,
                    display_name: m.name,
                    public_key: m.key,
                    state: m.state,
                })
                .collect();
            let side = &mut group.sides[0];
            let room = side.play.room.as_mut().unwrap();
            room.joined(me, members, None, group.clock.now());
            let side = &group.sides[0];
            assert_eq!(
                invite_note(&side.play, &side.online, jonas).as_deref(),
                Some("Room is full")
            );
            assert!(invite_closed(&side.play, &side.online, jonas));
        }

        #[test]
        fn a_blocked_invite_says_why_on_the_friend_it_was_for() {
            let mut group = Group::pair_of_two(&["Clara", "Mia", "Jonas"], &[1]);
            group.frames_run(60);
            let jonas = group.id(2);
            let side = &mut group.sides[0];
            assert!(side.play.invite(&mut side.online, jonas, None));
            group.until("the relay answers", |g| g.room(0).invite_error().is_some());
            assert_eq!(
                invite_note(&group.sides[0].play, &group.sides[0].online, jonas).as_deref(),
                Some("Can't invite until Mia updates Citrine")
            );
            assert!(invite_closed(
                &group.sides[0].play,
                &group.sides[0].online,
                jonas
            ));
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_watcher_outside_the_friend_list_sees_the_watched_members_name() {
            let mut group = gba_group_of(&["Clara", "Mia", "Jonas", "Lena"]);
            group.group_runs(&[0, 1, 2], 30);
            group.sides[3]
                .play
                .set_shelf(Box::new(Held(super::link_words().rom)));
            let mia = group.id(1);
            assert!(group.sides[3].online.friend(mia).is_none());
            assert!(group.sides[3].play.watch_group(mia, true));
            group.until("Lena sees Mia's screen", |g| {
                g.room(3).watched_picture().is_some()
            });
            let lena = &group.sides[3];
            let session = lena.play.session().expect("Lena's session");
            assert_eq!(
                crate::app::tabs::playing::watched_name(&lena.play, &lena.online, session),
                "Mia"
            );
        }

        #[cfg(feature = "gba")]
        #[test]
        fn an_unlink_from_behind_the_authority_keeps_the_group_in_step() {
            let mut group = gba_group();
            group.group_runs(&[0, 1, 2], 60);
            let jonas = group.id(2);
            group.until_on(&[0, 2], "Clara confirms past Mia", |g| {
                g.group_confirmed(0) > g.group_confirmed(1)
            });
            group.sides[2].down.set(true);
            assert!(group.sides[1].play.unlink_member(jonas));
            group.until("Clara sees Jonas unlinked", |g| {
                g.room(0)
                    .links()
                    .group_of(g.id(0))
                    .is_some_and(|l| !l.holds(jonas))
            });
            group.group_runs(&[0, 1], 120);
            assert_eq!((group.group_resyncs(0), group.group_resyncs(1)), (0, 0));
        }

        #[cfg(feature = "gba")]
        #[test]
        fn a_split_leave_heals_with_one_resync_per_unit() {
            let mut group = gba_group();
            group.group_runs(&[0, 1, 2], 60);
            group.until_on(&[0, 2], "Clara confirms past Mia", |g| {
                g.group_confirmed(0) > g.group_confirmed(1)
            });
            group.sides[2].down.set(true);
            let mia = group.leave_at_confirmed(1, 2).unwrap();
            let clara = group.leave_at_confirmed(0, 2).unwrap();
            assert!(clara > mia);
            group.group_runs(&[0, 1], 600);
            let healed = group.group_resyncs(1);
            assert!(healed > 0);
            group.group_runs(&[0, 1], 1200);
            assert_eq!(group.group_resyncs(1), healed);
        }
    }

    #[test]
    fn watching_the_friend_brings_their_sound_and_watch_off_leaves_mine() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(240);
        let (mut mine, mut friend) = (Vec::new(), Vec::new());
        pair.guest.session_mut().drain_audio(&mut mine, &mut friend);
        assert!(!mine.is_empty() && !friend.is_empty());
        pair.guest.session_mut().set_watch_friend(false);
        pair.play_frames(1);
        pair.guest.session_mut().drain_audio(&mut mine, &mut friend);
        mine.clear();
        friend.clear();
        pair.play_frames(120);
        pair.guest.session_mut().drain_audio(&mut mine, &mut friend);
        assert!(!mine.is_empty());
        assert!(friend.is_empty(), "the friend's game played unwatched");
    }

    struct Spend {
        bytes: usize,
        seconds: f64,
        peak_second: usize,
        lowest_room: f64,
    }

    fn spend(meter: &Meter, per_second: f64, burst: f64) -> Spend {
        let log = meter.borrow();
        let first = log.first().map_or_else(Instant::now, |(at, _)| *at);
        let last = log.last().map_or(first, |(at, _)| *at);
        let mut room = burst;
        let mut lowest_room = burst;
        let mut at = first;
        let mut peak_second = 0;
        for (i, (when, bytes)) in log.iter().enumerate() {
            room = (room + when.duration_since(at).as_secs_f64() * per_second).min(burst);
            at = *when;
            room -= *bytes as f64;
            lowest_room = lowest_room.min(room);
            let second: usize = log[i..]
                .iter()
                .take_while(|(t, _)| t.duration_since(*when) < Duration::from_secs(1))
                .map(|(_, n)| n)
                .sum();
            peak_second = peak_second.max(second);
        }
        Spend {
            bytes: log.iter().map(|(_, n)| n).sum(),
            seconds: last.duration_since(first).as_secs_f64(),
            peak_second,
            lowest_room,
        }
    }

    #[test]
    fn a_link_session_stays_inside_the_relay_play_budget() {
        let (per_second, burst) = (256.0 * 1024.0, 12.0 * 1024.0 * 1024.0);
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(custom());
        pair.accept(session, MyGame::Borrow { save: None });
        pair.play_frames(600);
        assert_eq!(pair.host.session().mode(), Some(Mode::Rollback));
        for (who, side) in [("host", &pair.host), ("guest", &pair.guest)] {
            let s = spend(&side.meter, per_second, burst);
            eprintln!(
                "play bytes, {who}: {} B in {:.1} s, {:.0} B/s on average, {} B in the busiest second, {:.0} B of the 12 MB burst left at the lowest",
                s.bytes,
                s.seconds,
                s.bytes as f64 / s.seconds.max(0.001),
                s.peak_second,
                s.lowest_room,
            );
            assert!(s.lowest_room > 0.0);
            assert!((s.bytes as f64 / s.seconds) < per_second);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    fn solo(frame: &[u8]) -> Option<crate::discord::Unit<'_>> {
        Some(crate::discord::Unit {
            game: "solo".into(),
            running: true,
            frame,
        })
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[test]
    fn the_discord_picture_in_a_session_is_my_own_unit_and_keeps_moving() {
        use crate::discord::{frame_key, my_unit};
        let (mut pair, _, _) = joined_pair(true);
        pair.host.presses_start = true;
        pair.play_frames(60);
        let before = [&pair.host, &pair.guest]
            .map(|side| frame_key(my_unit(Some(side.session()), None).unwrap().frame));
        pair.play_frames(300);
        for (side, before) in [&pair.host, &pair.guest].into_iter().zip(before) {
            let session = side.session();
            let mine = session.game_boy(true).unwrap();
            let unit = my_unit(Some(session), solo(&[1, 2, 3])).unwrap();
            assert!(std::ptr::eq(unit.frame, mine.frame().as_slice()));
            assert!(unit.running);
            assert_eq!(
                unit.game,
                format!("together {}", mine.cartridge.header.sha256_hex_string())
            );
            assert_ne!(frame_key(unit.frame), before);
        }
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[test]
    fn a_watcher_sends_no_discord_picture_of_the_player() {
        let mut pair = watching_pair(false, homebrew(), snake());
        pair.play_frames(120);
        let watcher = pair.host.session();
        assert!(watcher.console(false).is_some());
        assert!(crate::discord::my_unit(Some(watcher), solo(&[1])).is_none());
        let player = pair.guest.session();
        let unit = crate::discord::my_unit(Some(player), solo(&[1])).unwrap();
        assert!(std::ptr::eq(
            unit.frame,
            player.console(true).unwrap().frame()
        ));
    }

    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    #[test]
    fn after_a_session_the_discord_picture_is_the_solo_game_again() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(60);
        pair.host.play.leave(&mut pair.host.online);
        let frame = [7];
        let unit = crate::discord::my_unit(pair.host.play.session(), solo(&frame)).unwrap();
        assert_eq!(unit.game, "solo");
        assert!(crate::discord::my_unit(None, solo(&frame)).is_some_and(|u| u.game == "solo"));
    }

    #[cfg(feature = "gba")]
    fn link_words() -> GameFile {
        let rom: Vec<u8> = [
            0xE3A0_0404u32,
            0xE280_2C01,
            0xE280_3C02,
            0xE3A0_4000,
            0xE1C2_43B4,
            0xE3A0_5A06,
            0xE385_5083,
            0xE3C5_A080,
            0xE1C2_A2B8,
            0xE3A0_8402,
            0xE3A0_9080,
            0xE1D2_13B0,
            0xE021_1004,
            0xE1C2_12BA,
            0xE1C3_90B2,
            0xE1D2_62B8,
            0xE316_0008,
            0x0AFF_FFFC,
            0xE1C2_52B8,
            0xE1D3_60B2,
            0xE316_0080,
            0x0AFF_FFFC,
            0xE592_6020,
            0xE592_7024,
            0xE8A8_00C0,
            0xE3C8_8701,
            0xE284_4001,
            0xEAFF_FFEE,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        let hash = sha1_of(&rom).unwrap();
        GameFile::new(rom.into(), hash, "Link Words".into(), ConsoleKind::Gba)
    }

    #[cfg(feature = "gba")]
    fn runs_a_gba(session: &PlaySession, local: bool) -> bool {
        session
            .console(local)
            .is_some_and(|console| console.kind() == ConsoleKind::Gba)
    }

    #[cfg(feature = "gba")]
    #[test]
    fn two_gba_players_join_and_trade_link_words() {
        let mut pair = Pair::new(false, 30, 10, empty());
        let session = pair.invite(link_words());
        assert_eq!(
            pair.guest.play.invites()[0]
                .game
                .as_ref()
                .map(|g| g.console),
            Some(citrine_gb_net::Console::Gba)
        );
        pair.accept(session, MyGame::Own(link_words()));
        for side in [&pair.host, &pair.guest] {
            assert_eq!(side.session().mode(), Some(Mode::Rollback));
            assert!(runs_a_gba(side.session(), true) && runs_a_gba(side.session(), false));
        }
        let moved = transfers_over(&mut pair, 240);
        assert!(pair.host.session().cable_plugged() && pair.guest.session().cable_plugged());
        assert!(moved.iter().all(|&n| n >= 100), "{moved:?}");
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.resyncs() + guest.resyncs(), 0);
        assert_same_inputs(host, guest);
        assert_watcher_sees_the_player(host, guest);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_game_crosses_to_a_friend_without_it_and_both_play() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(link_words());
        pair.accept(session, MyGame::Borrow { save: None });
        assert!(pair.guest.received);
        assert!(runs_a_gba(pair.guest.session(), true));
        let moved = transfers_over(&mut pair, 120);
        assert!(moved.iter().all(|&n| n >= 50), "{moved:?}");
        pair.host.play.leave(&mut pair.host.online);
        pair.until("the borrower sees the host go", |p| {
            p.guest.stage() == Some(Stage::Ended(Ending::FriendLeft))
        });
        let back = pair.guest.play.close().unwrap().finish().unwrap();
        assert_eq!(back.borrowed, Some(hash_of(&link_words())));
        assert_eq!(back.console.kind(), ConsoleKind::Gba);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_friend_with_no_game_watches_a_gba_player() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite_with(None);
        pair.accept(session, MyGame::Own(link_words()));
        pair.play_frames(240);
        let (watcher, player) = (pair.host.session(), pair.guest.session());
        assert!(watcher.watching_only() && !player.watching_only());
        assert!(watcher.console(true).is_none() && runs_a_gba(watcher, false));
        assert!(runs_a_gba(player, true) && player.console(false).is_none());
        assert_eq!(watcher.resyncs() + player.resyncs(), 0);
        assert_watcher_sees_the_player(watcher, player);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_game_boy_and_a_gba_player_play_apart() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(link_words()));
        for side in [&pair.host, &pair.guest] {
            assert_eq!(side.session().mode(), Some(Mode::Apart));
            assert!(side.session().console(true).is_none());
        }
    }

    #[test]
    fn a_game_on_the_shelf_is_looked_up_off_the_ui_thread() {
        let mine = custom();
        let (shelf, lookups) = watched(mine.rom.clone());
        let mut pair = Pair::new(true, 30, 10, [Box::new(Nothing), Box::new(shelf)]);
        pair.guest.threads = Threads::new(citrine_gb_threads::Mode::Threads);
        let session = pair.invite(mine);
        pair.accept(session, MyGame::Borrow { save: None });
        pair.play_frames(120);
        assert!(!pair.guest.received);
        assert_eq!(pair.guest.session().mode(), Some(Mode::Rollback));
        assert!(off_this_thread(&lookups));
        pair.until_checked();
        let custom = custom().rom;
        assert_matches_reference(pair.host.session(), [&custom, &custom]);
    }

    #[test]
    fn a_game_file_cards_the_hash_it_keeps() {
        let file = GameFile::new(snake().rom, [9; 20], "Snake".into(), snake().kind);
        assert_eq!(file.card().unwrap().hash, [9; 20]);
        assert_eq!(file.clone().card().unwrap().hash, [9; 20]);
        assert_eq!(snake().card().unwrap().hash, sha1_of(SNAKE).unwrap());
    }

    #[test]
    fn find_and_holds_answer_from_the_background_lookup() {
        let mine = custom();
        let hash = hash_of(&mine);
        let (shelf, lookups) = watched(mine.rom.clone());
        let alone = Play::new(Box::new(watched(mine.rom.clone()).0));
        assert_eq!(alone.holds(&hash), None);
        assert!(matches!(alone.find(&hash), Look::Looking));
        let mut pair = Pair::new(false, 30, 10, [Box::new(Nothing), Box::new(shelf)]);
        pair.guest.threads = Threads::new(citrine_gb_threads::Mode::Threads);
        pair.invite(mine.clone());
        pair.until("the lookup answers", |p| {
            p.guest.play.holds(&hash).is_some()
        });
        assert_eq!(pair.guest.play.holds(&hash), Some(true));
        assert!(matches!(pair.guest.play.find(&hash), Look::Found(rom) if rom == mine.rom));
        assert!(off_this_thread(&lookups));
        let other = sha1_of(SNAKE).unwrap();
        assert_eq!(pair.host.play.holds(&other), None);
    }

    #[test]
    fn a_received_game_shows_after_its_check() {
        let mut pair = Pair::new(true, 30, 10, empty());
        let mine = custom();
        let total = mine.rom.len() as u32;
        let session = pair.invite(mine);
        assert!(pair.guest.play.accept(
            &mut pair.guest.online,
            session,
            MyGame::Borrow { save: None }
        ));
        pair.until("the last part arrives", |p| {
            p.guest.arrived.last().is_some_and(|(got, _)| *got == total)
        });
        assert_eq!(
            pair.guest.stage(),
            Some(Stage::Receiving { got: total, total })
        );
        assert!(pair.guest.session().round.checking.is_some());
        pair.until("both are playing", |p| {
            p.host.stage() == Some(Stage::Playing) && p.guest.stage() == Some(Stage::Playing)
        });
        assert!(pair.guest.session().round.checking.is_none());
        assert_eq!(pair.guest.session().round.i_hold_theirs, Some(true));
    }

    #[test]
    fn a_pair_start_builds_its_engine_while_the_guest_steps() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        pair.guest.threads = Threads::new(citrine_gb_threads::Mode::Threads);
        let session = pair.invite(snake());
        assert!(
            pair.guest
                .play
                .accept(&mut pair.guest.online, session, MyGame::Own(snake()))
        );
        let mut building = 0;
        pair.until("both are playing", |p| {
            if let Some(s) = p.guest.play.session()
                && s.round.building.is_some()
            {
                assert_ne!(s.stage(), Stage::Playing);
                building += 1;
            }
            p.host.stage() == Some(Stage::Playing) && p.guest.stage() == Some(Stage::Playing)
        });
        assert!(building > 0);
        assert!(pair.guest.session().round.building.is_none());
        pair.play_frames(60);
    }

    #[test]
    fn pads_that_arrive_while_the_engine_builds_still_reach_it() {
        let mut pair = Pair::new(false, 30, 10, homebrew());
        pair.guest.threads = Threads::new(citrine_gb_threads::Mode::Inline);
        let session = pair.invite(snake());
        assert!(
            pair.guest
                .play
                .accept(&mut pair.guest.online, session, MyGame::Own(snake()))
        );
        pair.until("the guest plans its engine", |p| {
            p.guest
                .play
                .session()
                .is_some_and(|s| s.round.building.is_some())
        });
        let begun = pair.clock.now();
        let mut ahead = 0;
        while ahead < 20 {
            pair.clock.waited(begun, "the host runs ahead");
            let now = pair.clock.tick();
            pair.host.step(now);
            if pair.host.frame() > 0 {
                ahead += 1;
            }
        }
        pair.play_frames(60);
    }

    #[test]
    fn unacknowledged_game_parts_go_again_from_the_last_acknowledged_byte() {
        let file = GameFile::new(
            vec![7; 12 << 20].into(),
            [1; 20],
            "Big".into(),
            ConsoleKind::Gba,
        );
        let cap = Some(citrine_gb_net::MAX_GAME_BYTES);
        let mut session = PlaySession::new(OnlineId::random(), Role::Host, Stage::Preparing, cap);
        session.round.my_card = file.card();
        session.round.mine = Some(file);
        session.round.sending = Some(0);
        session.send_parts(Instant::now());
        assert!(session.game_outbox.is_empty());
        let start = Instant::now();
        session.round.acked = Some((0, start));
        for _ in 0..10 {
            session.send_parts(start);
        }
        let window = GAME_WINDOW_BYTES as usize;
        let sent = session.round.sending.unwrap();
        assert!((window..window + GAME_PART_BYTES).contains(&sent));
        session.game_outbox.clear();
        session.send_parts(start + RESEND_AFTER);
        assert_eq!(session.game_outbox[0].offset, 0);
        session.round.acked = Some((12 << 20, start + RESEND_AFTER));
        session.game_outbox.clear();
        session.send_parts(start + RESEND_AFTER);
        assert!(session.game_outbox.is_empty());
        assert_eq!(session.round.sending, None);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_32_mb_gba_game_crosses_in_paced_parts_with_byte_progress() {
        let mut rom = link_words().rom.to_vec();
        rom.resize(32 << 20, 0);
        let hash = sha1_of(&rom).unwrap();
        let mine = GameFile::new(rom.into(), hash, "Big".into(), ConsoleKind::Gba);
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(mine);
        assert!(pair.guest.play.accept(
            &mut pair.guest.online,
            session,
            MyGame::Borrow { save: None }
        ));
        let total = 32u64 << 20;
        let (mut got, mut sent, mut ahead) = (Vec::new(), Vec::new(), 0);
        pair.until("the guest plays", |p| {
            if let Some(Stage::Receiving { got: done, .. }) = p.guest.stage() {
                got.push(u64::from(done));
            }
            let host = p.host.session();
            if host.sends_game() {
                sent.push(host.game_sent().unwrap());
            }
            if let (Some(offset), Some((acked, _))) = (host.round.sending, host.round.acked) {
                ahead = ahead.max(offset.saturating_sub(acked));
            }
            p.guest.stage() == Some(Stage::Playing)
        });
        assert!(got.windows(2).all(|w| w[0] <= w[1]));
        assert!(got.iter().any(|&done| done > 0 && done < total));
        assert!(sent.iter().all(|&(_, of)| of == total));
        assert!(sent.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(sent.iter().any(|&(done, _)| done > 0 && done < total));
        let window = (GAME_WINDOW_BYTES as usize) + PARTS_PER_UPDATE * GAME_PART_BYTES;
        assert!(ahead > 0 && ahead <= window, "{ahead} bytes ahead");
        assert_eq!(pair.guest.session().my_game().unwrap().hash, hash);
        assert_eq!(pair.guest.play.holds(&hash), Some(false));
    }

    #[cfg(feature = "gba")]
    fn longest_guest_frame(bytes: usize, borrow: bool) -> f64 {
        let mut rom = link_words().rom.to_vec();
        rom.resize(bytes, 0);
        let hash = sha1_of(&rom).unwrap();
        let mine = GameFile::new(rom.into(), hash, "Big".into(), ConsoleKind::Gba);
        longest_frames(mine, borrow)[1]
    }

    #[cfg(any(feature = "gba", feature = "nds"))]
    fn longest_frames(mine: GameFile, borrow: bool) -> [f64; 2] {
        let bytes = mine.rom.len();
        let mut pair = Pair::new(true, 30, 10, empty());
        for side in [&mut pair.host, &mut pair.guest] {
            side.threads = Threads::new(citrine_gb_threads::Mode::Threads);
        }
        let session = pair.invite(mine.clone());
        let game = match borrow {
            true => MyGame::Borrow { save: None },
            false => MyGame::Own(mine),
        };
        assert!(
            pair.guest
                .play
                .accept(&mut pair.guest.online, session, game)
        );
        let mut frames = [Vec::new(), Vec::new()];
        let begun = pair.clock.now();
        while pair.guest.stage() != Some(Stage::Playing) {
            pair.clock.waited(begun, "the guest plays");
            let now = pair.clock.tick();
            let started = std::time::Instant::now();
            pair.host.step(now);
            frames[0].push(started.elapsed().as_secs_f64() * 1000.0);
            let before = pair.guest.stage();
            let started = std::time::Instant::now();
            pair.guest.step(now);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            if before != pair.guest.stage() && pair.guest.stage() == Some(Stage::Playing) {
                println!("{} MB start frame {ms:.2} ms", bytes >> 20);
            }
            frames[1].push(ms);
        }
        frames.map(|frames| {
            let longest = frames.iter().copied().fold(0.0, f64::max);
            let mean = frames.iter().sum::<f64>() / frames.len() as f64;
            println!(
                "{} MB frames {} longest {longest:.2} ms mean {mean:.3} ms",
                bytes >> 20,
                frames.len()
            );
            longest
        })
    }

    #[cfg(feature = "gba")]
    #[test]
    #[ignore = "frame times, run in release"]
    fn receiving_the_biggest_gba_game_keeps_frames_short() {
        let longest = longest_guest_frame(citrine_gb_net::MAX_GAME_BYTES as usize, true);
        println!(
            "receiving {} MB, longest {longest:.3} ms",
            citrine_gb_net::MAX_GAME_BYTES >> 20
        );
        assert!(longest < 16.7, "longest frame {longest:.2} ms");
    }

    #[cfg(feature = "gba")]
    #[test]
    #[ignore = "frame times, run in release"]
    fn starting_a_32_mb_gba_game_keeps_frames_short() {
        let longest = longest_guest_frame(32 << 20, false);
        assert!(longest < 16.7, "longest frame {longest:.2} ms");
    }

    fn ds_game(code: &[u8; 4], version: u8) -> GameFile {
        let mut rom = vec![0; 0x200];
        rom[0x0C..0x10].copy_from_slice(code);
        rom[0x1E] = version;
        GameFile::new(
            rom.as_slice().into(),
            sha1_of(&rom).unwrap(),
            "Pong".into(),
            ConsoleKind::Nds,
        )
    }

    #[test]
    fn a_ds_card_carries_the_game_code() {
        let card = ds_game(b"APNE", 0).card().unwrap();
        assert_eq!(
            card.console,
            citrine_gb_net::Console::Nds { code: *b"APNE" }
        );
    }

    #[test]
    fn a_ds_game_needs_the_same_copy_unless_the_relay_shares_games() {
        let theirs = ds_game(b"APNE", 0).card().unwrap();
        let same = ds_game(b"APNE", 0).card().unwrap();
        let revised = ds_game(b"APNE", 1).card().unwrap();
        let european = ds_game(b"APNP", 0).card().unwrap();
        let other = ds_game(b"AXYE", 0).card().unwrap();
        assert_eq!(refusal(&theirs, Some(&same), false, false), None);
        assert_eq!(refusal(&theirs, None, false, false), Some(Refusal::NoCopy));
        assert_eq!(
            refusal(&theirs, None, true, false),
            Some(Refusal::OtherCopy)
        );
        assert_eq!(refusal(&theirs, None, false, true), None);
        assert_eq!(refusal(&theirs, None, true, true), None);
        assert_eq!(
            refusal(&theirs, Some(&revised), false, false),
            Some(Refusal::OtherCopy)
        );
        assert_eq!(
            refusal(&theirs, Some(&european), false, false),
            Some(Refusal::OtherCopy)
        );
        assert_eq!(
            refusal(&theirs, Some(&other), false, false),
            Some(Refusal::NoCopy)
        );
        let snake = snake().card().unwrap();
        assert_eq!(refusal(&snake, None, false, false), None);
    }

    #[test]
    fn a_refused_ds_invite_tells_the_host_why() {
        for (why, ending) in [
            (Refusal::NoCopy, Ending::NoCopy),
            (Refusal::OtherCopy, Ending::OtherCopy),
        ] {
            let mut pair = Pair::new(true, 0, 0, empty());
            let session = pair.invite(ds_game(b"APNE", 0));
            let card = pair.guest.play.invites()[0].game.clone().unwrap();
            assert!(card.console.is_ds());
            assert!(pair.guest.play.refuse(&mut pair.guest.online, session, why));
            pair.until("the host hears why", |p| {
                p.host.stage() == Some(Stage::Ended(ending))
            });
            assert!(pair.guest.play.invites().is_empty());
        }
    }

    #[cfg(feature = "nds")]
    #[expect(
        clippy::disallowed_types,
        reason = "test threads share no worker, so the DS pair tests take turns behind one global lock"
    )]
    pub(crate) fn one_ds_pair_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        static DS_PAIRS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        DS_PAIRS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(feature = "nds")]
    pub(crate) fn nickname() -> GameFile {
        const NICKNAME: &[u8] = include_bytes!("../fixtures/nds/nickname.nds");
        GameFile::new(
            NICKNAME.into(),
            sha1_of(NICKNAME).unwrap(),
            "Nickname".into(),
            ConsoleKind::Nds,
        )
    }

    #[cfg(feature = "nds")]
    pub(crate) fn lighting_cube() -> GameFile {
        const CUBE: &[u8] = include_bytes!("../fixtures/nds/blocksds-lighting_cube.nds");
        GameFile::new(
            CUBE.into(),
            sha1_of(CUBE).unwrap(),
            "Lighting cube".into(),
            ConsoleKind::Nds,
        )
    }

    #[cfg(feature = "nds")]
    #[test]
    #[ignore = "frame times, run in release"]
    fn sending_a_32_mb_ds_game_keeps_the_hosts_frames_short() {
        let _one = one_ds_pair_at_a_time();
        let mut rom = nickname().rom.to_vec();
        let mut state = 0x9E37_79B9u32;
        rom.extend((rom.len()..32 << 20).map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        }));
        let hash = sha1_of(&rom).unwrap();
        let mine = GameFile::new(rom.into(), hash, "Big".into(), ConsoleKind::Nds);
        let [host, _] = longest_frames(mine, true);
        assert!(host < 16.7, "longest host frame {host:.2} ms");
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_game_crosses_to_a_friend_without_it_when_the_relay_shares_games() {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, 30, 10, empty());
        let hash = hash_of(&nickname());
        let session = pair.invite(nickname());
        pair.accept(session, MyGame::Borrow { save: None });
        assert!(pair.guest.received && pair.host.sent);
        assert!(pair.guest.arrived.iter().all(|(got, total)| got <= total));
        pair.play_frames(120);
        let guest = pair.guest.session();
        assert!(guest.borrowing());
        assert_eq!(guest.mode(), Some(Mode::Rollback));
        assert_eq!(guest.my_game().unwrap().hash, hash);
        assert_eq!(pair.guest.play.holds(&hash), Some(false));
        pair.until_checked();
        for side in [&pair.host, &pair.guest] {
            assert_eq!(side.session().resyncs(), 0);
            assert!(side.session().failure().is_none());
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_guest_without_the_game_waits_on_an_empty_console_for_download_play() {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, 30, 10, empty());
        let session = pair.invite(nickname());
        pair.accept(session, MyGame::Download);
        pair.play_frames(120);
        pair.until_checked();
        for side in [&pair.host, &pair.guest] {
            assert_eq!(side.session().resyncs(), 0);
            assert!(side.session().failure().is_none());
        }
        let guest = pair.guest.session();
        assert!(guest.downloading());
        assert_eq!(guest.mode(), Some(Mode::Rollback));
        assert_eq!(guest.fetch(), Some(super::fetch::Fetch::Listening));
        assert_eq!(pair.host.session().fetch(), None);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn joining_a_ds_game_you_lack_asks_how_before_lending_it() {
        use crate::app::tabs::playing::{PickFor, PlayingUi, join, join_theirs, join_with};
        use crate::app::ui_state::recent::RecentRoms;
        let _one = one_ds_pair_at_a_time();
        for lend in [false, true] {
            let mut pair = Pair::new(true, 0, 0, empty());
            pair.invite(nickname());
            let invite = pair.guest.play.invites()[0].clone();
            let mut emulator = stored_emulator(&format!("ds-join-asks-{lend}"));
            let mut ui = PlayingUi::default();
            let recent = RecentRoms::default();
            for tick in 0.. {
                let guest = &mut pair.guest;
                assert!(join(
                    &mut guest.play,
                    &mut guest.online,
                    &mut emulator,
                    &mut ui,
                    &recent,
                    &invite,
                ));
                if ui.join_looking.take().is_none() {
                    break;
                }
                assert!(tick < 2_000, "the guest never looked for the game");
                pair.tick();
            }
            let guest = &mut pair.guest;
            assert!(!guest.play.is_live(), "joined before the guest chose");
            assert!(
                matches!(&ui.picking, Some(PickFor::Join(asked)) if asked.session == invite.session),
                "no picker"
            );
            let joined = match lend {
                true => join_theirs(
                    &mut guest.play,
                    &mut guest.online,
                    &mut emulator,
                    &mut ui,
                    &recent,
                    &invite,
                ),
                false => join_with(
                    &mut guest.play,
                    &mut guest.online,
                    &mut emulator,
                    &mut ui,
                    &invite,
                    MyGame::Download,
                ),
            };
            assert!(joined);
            let session = guest.session();
            assert_eq!(session.downloading(), !lend);
            assert!(session.borrowing());
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn two_friends_play_a_ds_game_together() {
        let _one = one_ds_pair_at_a_time();
        let rom = nickname;
        let mut pair = Pair::new(true, 0, 0, empty());
        let mut guest_user = super::random_ds_user();
        guest_user.nickname = "Mia".into();
        pair.guest.play.set_ds_user(guest_user.clone());
        let session = pair.invite(rom());
        pair.accept(session, MyGame::Own(rom()));
        assert_eq!(pair.host.session().round.their_user, Some(guest_user));
        pair.play_frames(120);
        pair.until_checked();
        for side in [&pair.host, &pair.guest] {
            assert_eq!(side.session().resyncs(), 0);
            assert!(side.session().failure().is_none());
        }
        let [host, guest] =
            [&pair.host, &pair.guest].map(|side| side.session().log().unwrap().checks.clone());
        for check in &host {
            if let Some(theirs) = guest.iter().find(|c| c.frame == check.frame) {
                assert_eq!(theirs.hashes, check.hashes, "frame {}", check.frame);
            }
        }
    }

    #[cfg(feature = "nds")]
    #[derive(Debug)]
    pub(crate) struct Pacing {
        ticks: usize,
        frames: usize,
        longest_gap: usize,
        gaps: [usize; 4],
        intervals_ms: [f64; 3],
    }

    #[cfg(feature = "nds")]
    impl Pacing {
        pub(crate) fn of(runs: &[bool]) -> Self {
            let mut gaps = [0; 4];
            let mut intervals = Vec::new();
            let mut since = 0;
            let mut started = false;
            for &ran in runs {
                since += 1;
                if ran {
                    if since > 1 {
                        gaps[(since - 2).min(3)] += 1;
                    }
                    if started {
                        intervals.push(since);
                    }
                    started = true;
                    since = 0;
                }
            }
            intervals.sort_unstable();
            let at = |q: f64| {
                let i = ((intervals.len() as f64 - 1.0) * q).round() as usize;
                intervals
                    .get(i)
                    .map_or(0.0, |&n| n as f64 * FRAME.as_secs_f64() * 1000.0)
            };
            Self {
                ticks: runs.len(),
                frames: runs.iter().filter(|&&ran| ran).count(),
                longest_gap: intervals.last().map_or(0, |&n| n - 1),
                gaps,
                intervals_ms: [at(0.5), at(0.99), at(1.0)],
            }
        }

        pub(crate) fn stalls_per_minute(&self) -> f64 {
            let minutes = self.ticks as f64 * FRAME.as_secs_f64() / 60.0;
            self.gaps[1..].iter().sum::<usize>() as f64 / minutes.max(f64::EPSILON)
        }

        pub(crate) fn line(&self, who: &str) -> String {
            format!(
                "{who}: {} frames in {} ticks, gaps of 1/2/3/4+ ticks {:?}, {:.2} stalls over a frame per minute, {:.0}% speed, frame interval p50 {:.1} ms p99 {:.1} ms max {:.1} ms",
                self.frames,
                self.ticks,
                self.gaps,
                self.stalls_per_minute(),
                self.frames as f64 * 100.0 / self.ticks.max(1) as f64,
                self.intervals_ms[0],
                self.intervals_ms[1],
                self.intervals_ms[2],
            )
        }
    }

    #[cfg(feature = "nds")]
    fn airtalk() -> GameFile {
        const AIRTALK: &[u8] = include_bytes!("../fixtures/nds/airtalk.nds");
        GameFile::new(
            AIRTALK.into(),
            sha1_of(AIRTALK).unwrap(),
            "Airtalk".into(),
            ConsoleKind::Nds,
        )
    }

    #[cfg(feature = "nds")]
    const DEVICE_RATE: u32 = 44_100;
    #[cfg(feature = "nds")]
    const DEVICE_READ: Duration = Duration::from_millis(10);
    #[cfg(feature = "nds")]
    const QUEUE_MS: u32 = 80;
    #[cfg(feature = "nds")]
    const PRESS_EVERY: Duration = Duration::from_millis(700);
    #[cfg(feature = "nds")]
    const PRESS_FRAMES: u32 = 4;

    #[cfg(feature = "nds")]
    pub(crate) struct AppLoop {
        clock: crate::audio::even_clock::EvenClock,
        feed: crate::audio::pacer::AudioFeed,
        reader: crate::audio::queue::QueueReader,
        speed: f64,
        every: Duration,
        last: Option<Instant>,
        since: Duration,
        device: Duration,
        out: Vec<f32>,
        mine: Vec<f32>,
        friend: Vec<f32>,
        press: Option<(Instant, u32)>,
        next_press: Option<Instant>,
        held: u32,
        press_frames: Vec<f64>,
    }

    #[cfg(feature = "nds")]
    impl AppLoop {
        fn new(refresh_hz: f64, speed: f64) -> Self {
            use crate::audio::queue::{QueueConfig, audio_queue, latency_frames};
            let target = latency_frames(DEVICE_RATE, QUEUE_MS);
            let (writer, reader) = audio_queue(QueueConfig::new(DEVICE_RATE, target));
            let read = (DEVICE_RATE as u64 * DEVICE_READ.as_millis() as u64 / 1000) as usize;
            Self {
                clock: Default::default(),
                feed: crate::audio::pacer::AudioFeed::new(writer, DEVICE_RATE, target),
                reader,
                speed,
                every: Duration::from_secs_f64(1.0 / refresh_hz),
                last: None,
                since: Duration::ZERO,
                device: Duration::ZERO,
                out: vec![0.0; read * 2],
                mine: Vec::new(),
                friend: Vec::new(),
                press: None,
                next_press: None,
                held: 0,
                press_frames: Vec::new(),
            }
        }

        fn update(&mut self, play: &mut PlaySession, now: Instant) {
            let Some(last) = self.last.replace(now) else {
                return;
            };
            let step = now.duration_since(last);
            self.device += step;
            while self.device >= DEVICE_READ {
                self.device -= DEVICE_READ;
                self.reader.read(&mut self.out, 2);
            }
            self.since += step;
            if self.since + Duration::from_micros(100) < self.every {
                return;
            }
            let dt = std::mem::take(&mut self.since);
            let timing = ConsoleKind::Nds.timing();
            let due = self.clock.advance(dt.as_secs_f64() * self.speed, timing);
            let next = *self.next_press.get_or_insert(now + PRESS_EVERY);
            if self.press.is_none() && self.held == 0 && now >= next && due > 0 {
                let delay = u32::from(play.delay().unwrap_or(0));
                self.press = Some((now, play.frame() + delay));
                self.held = PRESS_FRAMES;
                self.next_press = Some(now + PRESS_EVERY);
            }
            let buttons = if self.held > 0 {
                self.held = self.held.saturating_sub(due as u32);
                Keys::A
            } else {
                Keys::empty()
            };
            play.run_frames(vec![buttons; due], now);
            play.settle_frames();
            if let Some((at, target)) = self.press
                && play.frame() > target
            {
                self.press_frames
                    .push(now.duration_since(at).as_secs_f64() / timing.frame_secs());
                self.press = None;
            }
            play.drain_audio(&mut self.mine, &mut self.friend);
            self.feed.set_session(true);
            self.feed.push_at(&self.mine, now);
            self.mine.clear();
            self.friend.clear();
        }

        fn start_counting(&mut self) {
            self.feed.stats().reset();
            self.press_frames.clear();
        }

        fn underrun_ms(&self) -> f64 {
            self.feed.stats().underrun_frames() as f64 * 1000.0 / f64::from(DEVICE_RATE)
        }

        fn press_at(&self, q: f64) -> f64 {
            let mut sorted = self.press_frames.clone();
            sorted.sort_by(f64::total_cmp);
            let i = ((sorted.len() as f64 - 1.0) * q).round() as usize;
            sorted.get(i).copied().unwrap_or(f64::NAN)
        }
    }

    #[cfg(feature = "nds")]
    #[derive(Debug)]
    pub(crate) struct Heard {
        underrun_ms_a_minute: f64,
        press_p50: f64,
        press_max: f64,
        delay: Option<u8>,
        start_delay: Option<u8>,
    }

    #[cfg(feature = "nds")]
    fn ds_sound_on_the_device(
        one_way_ms: u64,
        jitter_ms: u64,
        guest_speed: f64,
        secs: u64,
    ) -> [Heard; 2] {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, one_way_ms, jitter_ms, empty());
        pair.host
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x00]));
        pair.guest
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x01]));
        let session = pair.invite(airtalk());
        pair.accept(session, MyGame::Own(airtalk()));
        pair.play_frames(240);
        let start_delay = pair.host.session().delay();
        pair.host.app = Some(Box::new(AppLoop::new(180.0, 1.0)));
        pair.guest.app = Some(Box::new(AppLoop::new(60.0, guest_speed)));
        let tick = Duration::from_secs_f64(1.0 / 180.0);
        let ticks = |secs: u64| (secs as f64 / tick.as_secs_f64()) as u64;
        for _ in 0..ticks(5) {
            pair.tick_by(tick);
        }
        for side in [&mut pair.host, &mut pair.guest] {
            side.app.as_mut().unwrap().start_counting();
        }
        for _ in 0..ticks(secs) {
            pair.tick_by(tick);
        }
        let minutes = secs as f64 / 60.0;
        let heard = [&pair.host, &pair.guest].map(|side| {
            let app = side.app.as_ref().unwrap();
            Heard {
                underrun_ms_a_minute: app.underrun_ms() / minutes,
                press_p50: app.press_at(0.5),
                press_max: app.press_at(1.0),
                delay: side.session().delay(),
                start_delay,
            }
        });
        println!(
            "one way {one_way_ms} ms, jitter {jitter_ms} ms, guest at {:.1}% speed, round trip {:?} ms",
            guest_speed * 100.0,
            pair.host.session().round_trip_ms()
        );
        for (who, h) in ["host", "guest"].iter().zip(&heard) {
            println!(
                "HEARD {who}: underrun {:.0} ms a minute, press to screen p50 {:.1} max {:.1} frames, delay {:?}",
                h.underrun_ms_a_minute, h.press_p50, h.press_max, h.delay
            );
        }
        for side in [pair.host.session(), pair.guest.session()] {
            assert_eq!(side.resyncs(), 0);
            assert!(side.failure().is_none());
        }
        heard
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_sound_plays_on_through_a_slow_friend() {
        let heard = ds_sound_on_the_device(40, 20, 0.966, 30);
        for (who, h) in ["host", "guest"].iter().zip(&heard) {
            assert!(
                h.underrun_ms_a_minute < 300.0,
                "{who} heard {:.0} ms of silence a minute",
                h.underrun_ms_a_minute
            );
            assert_eq!(
                h.delay, h.start_delay,
                "{who}: a slow friend raised the delay"
            );
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_sound_plays_on_at_full_speed() {
        let heard = ds_sound_on_the_device(40, 20, 1.0, 30);
        for (who, h) in ["host", "guest"].iter().zip(&heard) {
            assert!(
                h.underrun_ms_a_minute < 300.0,
                "{who} heard {:.0} ms of silence a minute",
                h.underrun_ms_a_minute
            );
        }
    }

    #[cfg(feature = "nds")]
    fn ds_wireless_pacing(
        one_way_ms: u64,
        jitter_ms: u64,
        frames: u32,
    ) -> ([Pacing; 2], Option<u32>) {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, one_way_ms, jitter_ms, empty());
        pair.host
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x00]));
        pair.guest
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x01]));
        let session = pair.invite(airtalk());
        pair.accept(session, MyGame::Own(airtalk()));
        pair.play_frames(240);
        let start = [&pair.host, &pair.guest].map(|side| Pacing::of(&side.runs));
        println!("{}", start[0].line("start, host"));
        println!("{}", start[1].line("start, guest"));
        for side in [&mut pair.host, &mut pair.guest] {
            side.runs.clear();
        }
        pair.play_frames(frames);
        pair.until_checked();
        let (host, guest) = (pair.host.session(), pair.guest.session());
        assert_eq!(host.delay(), guest.delay());
        println!(
            "delay {:?}, round trip {:?} ms",
            host.delay(),
            host.round_trip_ms()
        );
        for side in [host, guest] {
            assert_eq!(side.resyncs(), 0);
            assert!(side.failure().is_none());
        }
        assert_same_inputs(host, guest);
        let pacing = [&pair.host, &pair.guest].map(|side| Pacing::of(&side.runs));
        println!("{}", pacing[0].line("host"));
        println!("{}", pacing[1].line("guest"));
        (pacing, pair.host.session().round_trip_ms())
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_wireless_at_150_ms_never_stalls_over_a_frame() {
        let (pacing, round_trip) = ds_wireless_pacing(40, 20, 1200);
        assert!(round_trip.is_some_and(|ms| ms >= 150), "{round_trip:?}");
        for side in pacing {
            assert!(side.longest_gap <= 1, "{side:?}");
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_wireless_at_200_ms_never_stalls_over_a_frame() {
        let (pacing, round_trip) = ds_wireless_pacing(55, 20, 1200);
        assert!(round_trip.is_some_and(|ms| ms >= 180), "{round_trip:?}");
        for side in pacing {
            assert!(side.longest_gap <= 1, "{side:?}");
        }
    }

    #[cfg(feature = "nds")]
    fn ds_sound_against_the_clock(
        one_way_ms: u64,
        jitter_ms: u64,
        spread_ms: Option<u32>,
        ticks: usize,
    ) -> ([f64; 2], [Pacing; 2], [Option<u8>; 2]) {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, one_way_ms, jitter_ms, empty());
        pair.host
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x00]));
        pair.guest
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x01]));
        let session = pair.invite(airtalk());
        pair.host.session_mut().spread_for_test = spread_ms;
        assert!(
            pair.guest
                .play
                .accept(&mut pair.guest.online, session, MyGame::Own(airtalk()))
        );
        pair.guest.session_mut().spread_for_test = spread_ms;
        pair.until("both are playing", |p| {
            p.host.stage() == Some(Stage::Playing) && p.guest.stage() == Some(Stage::Playing)
        });
        let at_start = pair.host.session().delay();
        pair.play_frames(240);
        let (mut mine, mut friend) = (Vec::new(), Vec::new());
        let mut drain = |side: &mut Side| {
            side.session_mut().drain_audio(&mut mine, &mut friend);
            let heard = mine.len();
            mine.clear();
            friend.clear();
            heard
        };
        drain(&mut pair.host);
        drain(&mut pair.guest);
        let from = [pair.host.frame(), pair.guest.frame()];
        let mut heard = [0usize; 2];
        for _ in 0..240 {
            pair.tick();
            heard[0] += drain(&mut pair.host);
            heard[1] += drain(&mut pair.guest);
        }
        let ran = [pair.host.frame() - from[0], pair.guest.frame() - from[1]];
        let per_frame: [f64; 2] = std::array::from_fn(|i| heard[i] as f64 / ran[i] as f64);
        for side in [&mut pair.host, &mut pair.guest] {
            side.runs.clear();
        }
        let mut heard = [0usize; 2];
        for _ in 0..ticks {
            pair.tick();
            heard[0] += drain(&mut pair.host);
            heard[1] += drain(&mut pair.guest);
        }
        let pacing = [&pair.host, &pair.guest].map(|side| Pacing::of(&side.runs));
        println!(
            "delay {at_start:?} at the start, {:?} at the end, round trip {:?} ms, spread {:?} ms",
            pair.host.session().delay(),
            pair.host.session().round_trip_ms(),
            pair.host.session().round_trip_spread_ms()
        );
        let shares = std::array::from_fn(|i| {
            let share = heard[i] as f64 / (ticks as f64 * per_frame[i]);
            println!(
                "{}, {:.1} samples a frame, sound {:.2}% of the clock",
                pacing[i].line(["host", "guest"][i]),
                per_frame[i],
                share * 100.0
            );
            share
        });
        assert_eq!(pair.host.session().delay(), pair.guest.session().delay());
        (shares, pacing, [at_start, pair.host.session().delay()])
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_sound_keeps_up_with_the_clock_without_jitter() {
        let (shares, pacing, delay) = ds_sound_against_the_clock(40, 0, None, 1200);
        assert_eq!(delay, [Some(6); 2], "a steady link keeps its delay");
        assert_eq!(pacing.map(|p| p.gaps), [[0; 4]; 2]);
        for share in shares {
            assert!(share >= 0.995, "sound {:.2}% of the clock", share * 100.0);
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_sound_keeps_up_with_the_clock_through_stalls() {
        let (shares, pacing, delay) = ds_sound_against_the_clock(40, 90, None, 1200);
        assert_eq!(delay[1], Some(10), "a jittery link gets two frames more");
        for share in shares {
            assert!(share >= 0.995, "sound {:.2}% of the clock", share * 100.0);
        }
        for (who, p) in ["host", "guest"].iter().zip(&pacing) {
            assert!(p.gaps[0] <= 2, "{who} waited a tick {} times", p.gaps[0]);
            assert_eq!(p.gaps[1..], [0; 3], "{who} stalled over a frame");
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_waits_add_the_frame_a_low_spread_reading_missed() {
        let (shares, pacing, delay) = ds_sound_against_the_clock(40, 90, Some(16), 1200);
        assert_eq!(
            delay,
            [Some(9), Some(10)],
            "waits add one frame and keep it"
        );
        for share in shares {
            assert!(share >= 0.995, "sound {:.2}% of the clock", share * 100.0);
        }
        for (who, p) in ["host", "guest"].iter().zip(&pacing) {
            assert!(p.gaps[0] <= 2, "{who} waited a tick {} times", p.gaps[0]);
            assert_eq!(p.gaps[1..], [0; 3], "{who} stalled over a frame");
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_guest_stops_asking_for_frames_at_the_cap() {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, 40, 0, empty());
        pair.host
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x00]));
        pair.guest
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x01]));
        let session = pair.invite(airtalk());
        pair.host.session_mut().spread_for_test = Some(90);
        assert!(
            pair.guest
                .play
                .accept(&mut pair.guest.online, session, MyGame::Own(airtalk()))
        );
        pair.until("both are playing", |p| {
            p.host.stage() == Some(Stage::Playing) && p.guest.stage() == Some(Stage::Playing)
        });
        pair.play_frames(30);
        let at_start = pair.host.session().delay();
        fn guest(pair: &mut Pair) -> &mut super::engine::Rollback {
            pair.guest.session_mut().round.rollback_mut().unwrap()
        }
        assert_eq!(guest(&mut pair).delay_room(), 2);
        guest(&mut pair).wait_past_the_limit();
        pair.play_frames(60);
        assert_eq!(guest(&mut pair).log.asks, 1);
        assert_eq!(
            guest(&mut pair).delay_room(),
            0,
            "the host said it is at its cap"
        );
        guest(&mut pair).wait_past_the_limit();
        pair.play_frames(60);
        assert_eq!(guest(&mut pair).log.asks, 1, "no asks once at the cap");
        assert_eq!(pair.host.session().delay(), at_start);
        assert_eq!(pair.guest.session().delay(), at_start);
    }

    #[cfg(feature = "nds")]
    fn ds_pair_playing() -> Pair {
        let mut pair = Pair::new(true, 30, 0, empty());
        pair.host
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x00]));
        pair.guest
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x01]));
        let session = pair.invite(airtalk());
        pair.accept(session, MyGame::Own(airtalk()));
        pair.play_frames(60);
        pair
    }

    #[cfg(feature = "nds")]
    fn starts_without_the_friends_screen(side: &mut Side) {
        let session = side.session_mut();
        assert!(session.friend_lets_me_watch());
        assert!(!session.watches_friend() && !session.has_friend_tab());
        assert!(!session.round.rollback_mut().unwrap().watches());
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_host_plays_without_the_friends_screen_until_asked() {
        let _one = one_ds_pair_at_a_time();
        let mut pair = ds_pair_playing();
        starts_without_the_friends_screen(&mut pair.host);
        pair.host.session_mut().set_watch_friend(true);
        pair.play_frames(30);
        assert!(pair.host.session().sees_friend() && pair.host.session().has_friend_tab());
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_guest_plays_without_the_friends_screen_until_asked() {
        let _one = one_ds_pair_at_a_time();
        let mut pair = ds_pair_playing();
        starts_without_the_friends_screen(&mut pair.guest);
        pair.guest.session_mut().set_watch_friend(true);
        pair.play_frames(30);
        assert!(pair.guest.session().sees_friend() && pair.guest.session().has_friend_tab());
    }

    #[cfg(feature = "nds")]
    #[test]
    fn ds_delay_comes_back_down_once_the_waits_stop() {
        let _one = one_ds_pair_at_a_time();
        let mut pair = Pair::new(true, 40, 0, empty());
        pair.host
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x00]));
        pair.guest
            .play
            .set_ds_user(super::ds_user_with([0x00, 0x09, 0xBF, 0x12, 0x34, 0x01]));
        let session = pair.invite(airtalk());
        pair.accept(session, MyGame::Own(airtalk()));
        pair.play_frames(30);
        let at_start = pair.host.session().delay().unwrap();
        pair.guest
            .session_mut()
            .round
            .rollback_mut()
            .unwrap()
            .wait_past_the_limit();
        pair.play_frames(60);
        assert_eq!(pair.host.session().delay(), Some(at_start + 1));
        assert_eq!(pair.guest.session().delay(), Some(at_start + 1));
        pair.play_frames(1300);
        assert_eq!(pair.host.session().delay(), Some(at_start));
        assert_eq!(pair.guest.session().delay(), Some(at_start));
    }

    #[cfg(feature = "nds")]
    #[test]
    #[ignore]
    fn ds_wireless_pacing_by_round_trip() {
        let lags: Vec<(u64, u64)> = std::env::var("CITRINE_WDIST_LAGS")
            .unwrap_or_else(|_| "55,20 75,20 95,20".into())
            .split_whitespace()
            .filter_map(|lag| {
                let (one_way, jitter) = lag.split_once(',')?;
                Some((one_way.parse().ok()?, jitter.parse().ok()?))
            })
            .collect();
        for (one_way, jitter) in lags {
            println!("LAG one way {one_way} ms, jitter {jitter} ms");
            ds_wireless_pacing(one_way, jitter, 1800);
        }
    }

    fn stored_emulator(name: &str) -> crate::emulator::Emulator {
        let root = std::env::temp_dir().join(format!("citrine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut emulator = crate::emulator::Emulator::default();
        emulator.store = crate::storage::SaveStore::at(root);
        emulator
    }

    #[cfg(not(feature = "nds"))]
    #[test]
    fn a_ds_invite_is_declined_on_join_and_says_why() {
        use crate::app::tabs::playing::{PlayingUi, join};
        use crate::app::ui_state::recent::RecentRoms;
        let mut pair = Pair::new(true, 0, 0, empty());
        pair.invite(ds_game(b"APNE", 0));
        let invite = pair.guest.play.invites()[0].clone();
        let mut emulator = stored_emulator("ds-join");
        let mut ui = PlayingUi::default();
        assert!(join(
            &mut pair.guest.play,
            &mut pair.guest.online,
            &mut emulator,
            &mut ui,
            &RecentRoms::default(),
            &invite,
        ));
        assert!(pair.guest.play.invites().is_empty());
        assert_eq!(
            ui.ended.expect("a line").line,
            "Pong is a DS game, and DS games can't be played together yet."
        );
        pair.until("the host hears no", |p| {
            p.host.stage() == Some(Stage::Ended(Ending::Declined))
        });
    }
}
