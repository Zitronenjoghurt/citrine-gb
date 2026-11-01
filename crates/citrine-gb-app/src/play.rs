use crate::online::Online;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::serial::LinkMessage;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::netplay::NEVER;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_net::{
    Ask, CableByte, ClientMsg, Console, EndReason, GAME_PART_BYTES, GameCard, GameHash, GamePart,
    MAX_GAME_BYTES, MAX_STATE_BYTES, MAX_TITLE_CHARS, Mode, Offer, OnlineId, Origin, PLAY_VERSION,
    PlayMsg, PublicKey, ServerMsg, SessionId, Start,
};
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

mod engine;
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

impl PlaySession {
    fn new(friend: OnlineId, role: Role, stage: Stage, sharing: bool) -> Self {
        let now = Instant::now();
        Self {
            id: None,
            friend,
            role,
            stage,
            borrowing: false,
            borrowed_save: None,
            round: Round::new(1, None, None),
            roms: Vec::new(),
            wants_state: false,
            returned: None,
            pings: VecDeque::new(),
            next_ping: 0,
            round_trips: VecDeque::new(),
            pinged_at: None,
            started_at: now,
            heard_at: now,
            let_watch: true,
            friend_lets_watch: false,
            watch_friend: true,
            my_cable: true,
            friend_cable: true,
            saves: Vec::new(),
            sharing,
            outbox: Vec::new(),
            game_outbox: Vec::new(),
            sample_rate: 44_100,
        }
    }

    pub fn id(&self) -> Option<SessionId> {
        self.id
    }

    pub fn friend(&self) -> OnlineId {
        self.friend
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn is_live(&self) -> bool {
        !matches!(self.stage, Stage::Ended(_))
    }

    pub fn my_game(&self) -> Option<&GameCard> {
        self.round.my_card.as_ref()
    }

    pub fn friend_game(&self) -> Option<&GameCard> {
        self.round.theirs.as_ref().and_then(Option::as_ref)
    }

    pub fn friend_plays(&self) -> Option<bool> {
        self.round.theirs.as_ref().map(Option::is_some)
    }

    pub fn borrowing(&self) -> bool {
        self.borrowing
    }

    pub fn holds_theirs(&self) -> Option<bool> {
        self.round.i_hold_theirs
    }

    pub fn round(&self) -> u32 {
        self.round.number
    }

    pub fn mode(&self) -> Option<Mode> {
        self.round.start.map(|s| s.mode)
    }

    pub fn delay(&self) -> Option<u8> {
        self.round.start.map(|s| s.delay)
    }

    pub fn round_trip_ms(&self) -> Option<u32> {
        let mut sorted: Vec<u32> = self.round_trips.iter().copied().collect();
        sorted.sort_unstable();
        sorted.get(sorted.len() / 2).copied()
    }

    pub fn lets_friend_watch(&self) -> bool {
        self.let_watch
    }

    pub fn friend_lets_me_watch(&self) -> bool {
        self.friend_lets_watch
    }

    pub fn watches_friend(&self) -> bool {
        self.watch_friend
    }

    pub fn sees_friend(&self) -> bool {
        self.watch_friend && self.friend_lets_watch && self.console(false).is_some()
    }

    pub fn my_cable(&self) -> bool {
        self.my_cable
    }

    pub fn friend_cable(&self) -> bool {
        self.friend_cable
    }

    pub fn cable_plugged(&self) -> bool {
        match &self.round.engine {
            Some(Engine::Rollback(r)) => r.cable_plugged(),
            Some(Engine::Cable(c)) => c.plugged,
            Some(Engine::Apart) => false,
            None => self.round.start.is_some_and(|s| s.plugged),
        }
    }

    pub fn waiting_for_friend(&self, now: Instant) -> bool {
        self.round
            .rollback()
            .is_some_and(|r| r.stalled_for(now) >= STALL_SHOWN)
    }

    pub fn runs_engine(&self) -> bool {
        self.round.rollback().is_some()
    }

    pub fn console(&self, local: bool) -> Option<&GameBoy> {
        self.round.rollback().and_then(|r| r.console(local))
    }

    pub fn wants_state(&self) -> bool {
        self.wants_state
    }

    pub fn give_state(&mut self, state: Vec<u8>) {
        if !self.wants_state {
            return;
        }
        self.wants_state = false;
        self.round.my_origin = Some(Origin::Resume(state));
        self.send_setup();
    }

    pub fn take_returned(&mut self) -> Option<Returned> {
        self.returned.take()
    }

    pub fn holds_my_game(&self) -> bool {
        let resuming = self.wants_state || matches!(self.round.my_origin, Some(Origin::Resume(_)));
        resuming && self.is_live() && self.round.start.is_none() && self.stage != Stage::Inviting
    }

    pub fn watching_only(&self) -> bool {
        self.is_live() && self.stage != Stage::Inviting && self.round.my_card.is_none()
    }

    pub fn has_friend_tab(&self) -> bool {
        self.is_live()
            && self.stage != Stage::Inviting
            && self.round.my_card.is_some()
            && self.friend_plays() == Some(true)
            && self.watch_friend
    }

    pub fn frame(&self) -> u32 {
        self.round.rollback().map_or(0, Rollback::frame)
    }

    pub fn resyncs(&self) -> u32 {
        self.round.rollback().map_or(0, Rollback::resyncs)
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate;
    }

    #[cfg(test)]
    pub(crate) fn log(&self) -> Option<&engine::Log> {
        self.round.rollback().map(|r| &r.log)
    }

    #[cfg(test)]
    pub(crate) fn start(&self) -> Option<Start> {
        self.round.start
    }

    pub fn set_let_watch(&mut self, on: bool) {
        if self.let_watch == on {
            return;
        }
        self.let_watch = on;
        self.outbox.push(PlayMsg::Watch(on));
        let hidden = self.round.rollback().is_some_and(Rollback::hidden);
        if let Some(r) = self.round.rollback_mut() {
            r.show(on);
        }
        if on && hidden && self.is_live() {
            let mine = self.round.mine.take();
            let card = self.round.my_card.clone();
            self.next_round(self.round.number + 1, mine, card);
        }
    }

    pub fn set_watch_friend(&mut self, on: bool) {
        self.watch_friend = on;
        let shown = self.friend_lets_watch && on;
        if let Some(r) = self.round.rollback_mut() {
            r.watch(shown);
        }
    }

    pub fn set_cable(&mut self, plugged: bool) {
        if self.my_cable == plugged {
            return;
        }
        self.my_cable = plugged;
        self.outbox.push(PlayMsg::Cable(plugged));
        self.join_cables();
    }

    fn join_cables(&mut self) {
        let linked = self.my_cable && self.friend_cable;
        match &mut self.round.engine {
            Some(Engine::Rollback(r)) => r.request_cable(linked),
            Some(Engine::Cable(c)) => c.plugged = linked,
            Some(Engine::Apart) | None => {}
        }
    }

    pub fn bring(&mut self, game: MyGame) -> bool {
        if !self.is_live() || self.stage == Stage::Inviting || self.id.is_none() {
            return false;
        }
        let (mine, card) = match game {
            MyGame::Own(file) => match file.card() {
                Some(card) => (Some(file), Some(card)),
                None => return false,
            },
            MyGame::Borrow { .. } => return false,
            MyGame::Nothing => (None, None),
        };
        self.borrowing = false;
        self.next_round(self.round.number + 1, mine, card);
        true
    }

    pub fn hand_rom(&mut self, rom: &[u8]) -> bool {
        if !self.is_live() || self.id.is_none() {
            return false;
        }
        let Some(hash) = sha1_of(rom).filter(|h| self.friend_game().map(|g| g.hash) == Some(*h))
        else {
            return false;
        };
        self.keep_rom(hash, rom);
        let mine = self.round.mine.take();
        let card = self.round.my_card.clone();
        self.next_round(self.round.number + 1, mine, card);
        true
    }

    pub fn take_saves(&mut self) -> Vec<LinkedSave> {
        std::mem::take(&mut self.saves)
    }

    fn save_key(&self) -> Option<SaveKey> {
        if self.borrowing
            && let Some(card) = &self.round.my_card
        {
            return Some(SaveKey::Borrowed(card.hash));
        }
        self.console(true)
            .map(|gb| SaveKey::Rom(gb.cartridge.header.sha256_hex_string()))
    }

    fn keep_saves(&mut self, dumps: impl IntoIterator<Item = SramDump>) {
        let Some(key) = self.save_key() else {
            return;
        };
        self.saves.extend(dumps.into_iter().map(|dump| LinkedSave {
            key: key.clone(),
            dump,
        }));
    }

    pub fn drain_audio(&mut self, mine: &mut Vec<f32>, friend: &mut Vec<f32>) {
        if let Some(r) = self.round.rollback_mut() {
            r.drain_audio(mine, friend);
        }
    }

    pub fn run_frame(&mut self, buttons: JoypadState, now: Instant) -> bool {
        if self.stage != Stage::Playing {
            return false;
        }
        let round_trip = self.round_trip_ms().unwrap_or(0);
        let Some(r) = self.round.rollback_mut() else {
            return false;
        };
        if !r.cable_settling() && r.cable_plugged() != (self.my_cable && self.friend_cable) {
            r.request_cable(self.my_cable && self.friend_cable);
        }
        let ran = r.tick(buttons, round_trip, now, &mut self.outbox);
        let save = r.take_save();
        self.keep_saves(save);
        ran
    }

    pub fn pump_link(&mut self, gb: &mut GameBoy) {
        let Some(Engine::Cable(c)) = &mut self.round.engine else {
            return;
        };
        let live = self.stage == Stage::Playing;
        let want = live && c.plugged;
        if c.applied != Some(want) {
            if want {
                gb.plug_link();
            } else {
                gb.unplug_link();
            }
            c.applied = Some(want);
        }
        if !want {
            c.incoming.clear();
            return;
        }
        while let Some(msg) = c.incoming.pop_front() {
            gb.receive_link_message(msg);
        }
        let bytes: Vec<CableByte> = gb.take_link_messages().into_iter().map(to_byte).collect();
        if !bytes.is_empty() {
            self.outbox.push(PlayMsg::Bytes(bytes));
        }
    }

    pub fn finish(mut self) -> Option<Returned> {
        self.retire_round();
        self.returned.take()
    }

    pub fn finish_saves(&mut self) -> Vec<LinkedSave> {
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
        self.take_saves()
    }

    fn end(&mut self, ending: Ending) {
        self.stage = Stage::Ended(ending);
        self.round.sending = None;
        self.round.incoming = None;
        self.wants_state = false;
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
    }

    fn begin(&mut self, now: Instant) {
        if self.stage != Stage::Inviting && self.stage != Stage::Preparing {
            return;
        }
        self.stage = Stage::Preparing;
        self.started_at = now;
        self.heard_at = now;
        self.outbox.push(PlayMsg::Watch(self.let_watch));
        self.outbox.push(PlayMsg::Cable(self.my_cable));
        self.join_round();
        self.ping(now);
    }

    fn retire_round(&mut self) {
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
        let Some(engine) = self.round.engine.take() else {
            return;
        };
        if let Engine::Rollback(r) = engine
            && let Some(mut gb) = r.into_console()
        {
            gb.unplug_link();
            let borrowed = self
                .borrowing
                .then(|| self.round.my_card.as_ref().map(|c| c.hash))
                .flatten();
            self.returned = Some(Returned {
                console: gb,
                borrowed,
            });
        }
    }

    fn next_round(&mut self, number: u32, mine: Option<GameFile>, card: Option<GameCard>) {
        let ran_mine = self
            .round
            .rollback()
            .is_some_and(|r| r.console(true).is_some());
        let same_game =
            card.as_ref().map(|c| c.hash) == self.round.my_card.as_ref().map(|c| c.hash);
        self.retire_round();
        self.round = Round::new(number, mine, card);
        self.round.resume = ran_mine && same_game && self.round.my_card.is_some();
        self.stage = Stage::Preparing;
        self.join_round();
    }

    fn join_round(&mut self) {
        if self.round.joined {
            return;
        }
        self.round.joined = true;
        self.outbox.push(PlayMsg::Join {
            version: PLAY_VERSION,
            round: self.round.number,
            game: self.round.my_card.clone(),
        });
        let resume = self.round.my_card.is_some()
            && (self.round.resume || self.round.mine.as_ref().is_some_and(|m| m.running));
        self.round.my_origin = match &self.round.mine {
            _ if resume => None,
            Some(mine) => Some(Origin::Boot(mine.save.clone())),
            None if self.borrowing => Some(Origin::Boot(self.borrowed_save.clone())),
            None => None,
        };
        self.wants_state = resume;
        self.send_setup();
    }

    fn send_setup(&mut self) {
        if self.round.set_up || self.round.my_card.is_none() {
            return;
        }
        let Some(origin) = self.round.my_origin.clone() else {
            return;
        };
        self.round.set_up = true;
        self.outbox.push(PlayMsg::Setup {
            round: self.round.number,
            from: packed(origin),
        });
    }

    fn ping(&mut self, now: Instant) {
        let n = self.next_ping;
        self.next_ping = self.next_ping.wrapping_add(1);
        self.pings.push_back((n, now));
        while self.pings.len() > KEPT_PINGS {
            self.pings.pop_front();
        }
        self.pinged_at = Some(now);
        self.outbox.push(PlayMsg::Ping(n));
    }

    fn known_rom(&self, hash: &GameHash, shelf: &dyn Shelf) -> Option<Vec<u8>> {
        if let Some(mine) = &self.round.mine
            && sha1_of(&mine.rom) == Some(*hash)
        {
            return Some(mine.rom.clone());
        }
        if let Some((_, rom)) = self.roms.iter().find(|(h, _)| h == hash) {
            return Some(rom.clone());
        }
        shelf.find(hash).filter(|rom| sha1_of(rom) == Some(*hash))
    }

    fn keep_rom(&mut self, hash: GameHash, rom: &[u8]) {
        self.roms.retain(|(h, _)| *h != hash);
        self.roms.push((hash, rom.to_vec()));
        while self.roms.len() > KEPT_ROMS {
            self.roms.remove(0);
        }
    }

    fn receive(&mut self, msg: PlayMsg, shelf: &dyn Shelf, now: Instant) {
        if !self.is_live() {
            return;
        }
        match msg {
            PlayMsg::Join {
                version,
                round,
                game,
            } => {
                if version != PLAY_VERSION {
                    self.end(Ending::Failed);
                    return;
                }
                if round < self.round.number {
                    return;
                }
                if round > self.round.number {
                    let mine = self.round.mine.take();
                    let card = self.round.my_card.clone();
                    self.next_round(round, mine, card);
                }
                if self.round.theirs.is_some() {
                    return;
                }
                if let Some(game) = &game {
                    let rom = self.known_rom(&game.hash, shelf);
                    let holds = rom.is_some();
                    if let Some(rom) = &rom {
                        self.keep_rom(game.hash, rom);
                    }
                    self.round.their_rom = rom;
                    self.round.i_hold_theirs = Some(holds);
                    self.outbox.push(PlayMsg::Holds {
                        game: game.hash,
                        holds,
                    });
                    if !holds && self.sharing {
                        self.stage = Stage::Receiving {
                            got: 0,
                            total: game.size,
                        };
                    }
                }
                self.round.theirs = Some(game);
            }
            PlayMsg::Holds { game, holds } => {
                if self.round.my_card.as_ref().map(|c| c.hash) != Some(game) {
                    return;
                }
                self.round.they_hold_mine = Some(holds);
                if !holds && self.sharing && self.round.mine.is_some() {
                    self.round.sending = Some(0);
                }
            }
            PlayMsg::Setup { round, from } => {
                if round == self.round.number {
                    match unpacked(from) {
                        Some(origin) => self.round.their_origin = Some(origin),
                        None => self.end(Ending::Failed),
                    }
                }
            }
            PlayMsg::Ping(n) => self.outbox.push(PlayMsg::Pong(n)),
            PlayMsg::Pong(n) => {
                if let Some(index) = self.pings.iter().position(|(sent, _)| *sent == n) {
                    let (_, at) = self.pings.remove(index).unwrap_or((n, now));
                    let ms = now.duration_since(at).as_millis().min(u32::MAX as u128) as u32;
                    self.round_trips.push_back(ms);
                    while self.round_trips.len() > KEPT_PINGS {
                        self.round_trips.pop_front();
                    }
                }
            }
            PlayMsg::Start(start) => {
                if self.role == Role::Guest
                    && start.round == self.round.number
                    && self.round.start.is_none()
                {
                    self.launch(start, now);
                }
            }
            PlayMsg::Watch(on) => {
                self.friend_lets_watch = on;
                let shown = on && self.watch_friend;
                if let Some(r) = self.round.rollback_mut() {
                    r.watch(shown);
                }
            }
            PlayMsg::Cable(plugged) => {
                self.friend_cable = plugged;
                self.join_cables();
            }
            PlayMsg::Bytes(bytes) => {
                if let Some(Engine::Cable(c)) = &mut self.round.engine
                    && c.plugged
                {
                    c.incoming.extend(bytes.into_iter().map(from_byte));
                }
            }
            msg @ (PlayMsg::Pads { .. } | PlayMsg::Check { .. } | PlayMsg::Resync { .. }) => {
                if let Some(r) = self.round.rollback_mut() {
                    r.receive(msg);
                }
            }
        }
    }

    fn receive_part(&mut self, part: GamePart) {
        let Some(Some(theirs)) = &self.round.theirs else {
            return;
        };
        if self.round.i_hold_theirs != Some(false)
            || part.hash != theirs.hash
            || part.total != theirs.size
            || part.total > MAX_GAME_BYTES
        {
            return;
        }
        let incoming = self.round.incoming.get_or_insert_with(|| Incoming {
            hash: part.hash,
            total: part.total,
            data: Vec::with_capacity(part.total as usize),
        });
        if part.offset as usize != incoming.data.len()
            || incoming.data.len() + part.data.len() > incoming.total as usize
        {
            return;
        }
        incoming.data.extend_from_slice(&part.data);
        let got = incoming.data.len() as u32;
        self.stage = Stage::Receiving {
            got,
            total: incoming.total,
        };
        if got == incoming.total {
            let Some(done) = self.round.incoming.take() else {
                return;
            };
            if sha1_of(&done.data) == Some(done.hash) {
                self.keep_rom(done.hash, &done.data);
                self.round.their_rom = Some(done.data);
                self.round.i_hold_theirs = Some(true);
                self.stage = Stage::Preparing;
                self.outbox.push(PlayMsg::Holds {
                    game: done.hash,
                    holds: true,
                });
            } else {
                self.end(Ending::Failed);
            }
        }
    }

    fn step(&mut self, shelf: &dyn Shelf, now: Instant) {
        if !self.is_live() || self.stage == Stage::Inviting {
            return;
        }
        if now.duration_since(self.heard_at) > SILENCE_LIMIT {
            self.end(Ending::Dropped);
            return;
        }
        if self
            .pinged_at
            .is_none_or(|at| now.duration_since(at) >= PING_EVERY)
        {
            self.ping(now);
        }
        if self.borrowing && self.round.mine.is_none() {
            self.fill_borrowed(shelf);
        }
        self.send_parts();
        if self.role == Role::Host && self.round.start.is_none() {
            self.try_start(now);
        }
    }

    fn fill_borrowed(&mut self, shelf: &dyn Shelf) {
        let Some(card) = self.round.my_card.clone() else {
            return;
        };
        if let Some(rom) = self.known_rom(&card.hash, shelf) {
            self.keep_rom(card.hash, &rom);
        }
    }

    fn send_parts(&mut self) {
        let (Some(offset), Some(mine), Some(card)) =
            (self.round.sending, &self.round.mine, &self.round.my_card)
        else {
            return;
        };
        let total = mine.rom.len();
        let mut offset = offset;
        for _ in 0..PARTS_PER_UPDATE {
            if offset >= total {
                break;
            }
            let end = (offset + GAME_PART_BYTES).min(total);
            self.game_outbox.push(GamePart {
                hash: card.hash,
                offset: offset as u32,
                total: total as u32,
                data: mine.rom[offset..end].to_vec(),
            });
            offset = end;
        }
        self.round.sending = (offset < total).then_some(offset);
    }

    fn try_start(&mut self, now: Instant) {
        let Some(theirs) = self.round.theirs.clone() else {
            return;
        };
        let mine = self.round.my_card.is_some();
        if mine && !self.round.set_up {
            return;
        }
        if theirs.is_some() && self.round.their_origin.is_none() {
            return;
        }
        let i_hold = theirs.is_none() || self.round.i_hold_theirs == Some(true);
        let they_hold = !mine || self.round.they_hold_mine == Some(true);
        if theirs.is_some() && self.round.i_hold_theirs.is_none() {
            return;
        }
        if mine && theirs.is_some() && self.round.they_hold_mine.is_none() {
            return;
        }
        if mine && theirs.is_none() && self.round.they_hold_mine.is_none() {
            return;
        }
        let both = mine && theirs.is_some();
        if !(i_hold && they_hold) && self.sharing {
            return;
        }
        if both {
            let waited = now.duration_since(self.started_at) >= SETUP_WAIT;
            if self.round_trips.len() < SETUP_PINGS && !waited {
                return;
            }
        }
        let mode = if !mine && theirs.is_none() {
            Mode::Apart
        } else if i_hold && they_hold {
            Mode::Rollback
        } else if both {
            Mode::Cable
        } else {
            Mode::Apart
        };
        let slow = self
            .round_trip_ms()
            .is_some_and(|ms| ms > SLOW_ROUND_TRIP_MS);
        let power = |origin: Option<&Origin>, later: u32| match origin {
            None => NEVER,
            Some(Origin::Boot(_)) => later,
            Some(Origin::Resume(_)) => 0,
        };
        let start = Start {
            round: self.round.number,
            mode,
            seed: random_seed(),
            start_time: unix_now(),
            delay: if slow && both { SLOW_DELAY } else { 0 },
            power_on: [
                power(self.round.my_origin.as_ref().filter(|_| mine), 0),
                power(
                    self.round
                        .their_origin
                        .as_ref()
                        .filter(|_| theirs.is_some()),
                    if mine { GUEST_POWER_ON } else { 0 },
                ),
            ],
            plugged: self.my_cable && self.friend_cable,
        };
        self.outbox.push(PlayMsg::Start(start));
        self.launch(start, now);
    }

    fn launch(&mut self, start: Start, now: Instant) {
        self.round.start = Some(start);
        let engine = match start.mode {
            Mode::Cable => Some(Engine::Cable(CableLink {
                plugged: self.my_cable && self.friend_cable,
                ..CableLink::default()
            })),
            Mode::Apart => Some(Engine::Apart),
            Mode::Rollback => self
                .rollback(start, now)
                .map(|r| Engine::Rollback(Box::new(r))),
        };
        match engine {
            Some(engine) => {
                self.round.engine = Some(engine);
                self.stage = Stage::Playing;
                self.heard_at = now;
            }
            None => self.end(Ending::Failed),
        }
    }

    fn my_rom(&self) -> Option<Vec<u8>> {
        if let Some(mine) = &self.round.mine {
            return Some(mine.rom.clone());
        }
        let card = self.round.my_card.as_ref()?;
        self.roms
            .iter()
            .find(|(h, _)| *h == card.hash)
            .map(|(_, rom)| rom.clone())
            .or_else(|| self.round.their_rom.clone())
    }

    fn rollback(&mut self, start: Start, now: Instant) -> Option<Rollback> {
        let mine = match (&self.round.my_card, &self.round.my_origin) {
            (Some(card), Some(origin)) => {
                Some((self.my_rom()?, model_of(card.console), origin.clone()))
            }
            (Some(_), None) => return None,
            (None, _) => None,
        };
        let theirs = match (&self.round.theirs, &self.round.their_origin) {
            (Some(Some(card)), Some(origin)) => Some((
                self.round.their_rom.clone()?,
                model_of(card.console),
                origin.clone(),
            )),
            (Some(Some(_)), None) => return None,
            _ => None,
        };
        let local = match self.role {
            Role::Host => citrine_gb::netplay::Player::One,
            Role::Guest => citrine_gb::netplay::Player::Two,
        };
        let mut games = [mine, theirs];
        if local == citrine_gb::netplay::Player::Two {
            games.swap(0, 1);
        }
        let mut r = Rollback::new(local, start, games, self.sample_rate, now).ok()?;
        r.watch(self.friend_lets_watch && self.watch_friend);
        r.show(self.let_watch);
        Some(r)
    }

    fn flush(&mut self, online: &mut Online) {
        let Some(id) = self.id else {
            return;
        };
        if let Some(r) = self.round.rollback_mut() {
            r.flush(&mut self.outbox);
        }
        if self.outbox.is_empty() && self.game_outbox.is_empty() {
            return;
        }
        let Some(key) = online.friend(self.friend).map(|f| f.public_key) else {
            return;
        };
        send_all(
            online,
            id,
            &key,
            std::mem::take(&mut self.outbox),
            std::mem::take(&mut self.game_outbox),
        );
    }
}

fn send_all(
    online: &mut Online,
    session: SessionId,
    key: &PublicKey,
    msgs: Vec<PlayMsg>,
    parts: Vec<GamePart>,
) {
    let sealed: Vec<ClientMsg> = match online.keys() {
        Some(keys) => msgs
            .iter()
            .map(|msg| ClientMsg::Play {
                session,
                sealed: msg.seal(keys, key),
            })
            .chain(parts.iter().map(|part| ClientMsg::Game {
                session,
                sealed: part.seal(keys, key),
            }))
            .collect(),
        None => return,
    };
    for msg in sealed {
        online.send(msg);
    }
}

fn to_byte(msg: LinkMessage) -> CableByte {
    match msg {
        LinkMessage::Transfer {
            id,
            byte,
            seq,
            since,
        } => CableByte::Transfer {
            id,
            byte,
            seq,
            since,
        },
        LinkMessage::Reply { id, byte } => CableByte::Reply { id, byte },
    }
}

fn from_byte(byte: CableByte) -> LinkMessage {
    match byte {
        CableByte::Transfer {
            id,
            byte,
            seq,
            since,
        } => LinkMessage::Transfer {
            id,
            byte,
            seq,
            since,
        },
        CableByte::Reply { id, byte } => LinkMessage::Reply { id, byte },
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests;
