use super::engine::Rollback;
use super::messages::to_byte;
use super::{
    Engine, LinkedSave, MyGame, PlaySession, Returned, Role, Round, STALL_SHOWN, SaveKey, Stage,
    sha1_of,
};
use citrine_gb::gb::GameBoy;
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb_net::{CableByte, GameCard, Mode, OnlineId, Origin, PlayMsg, SessionId};
use std::collections::VecDeque;
use web_time::Instant;

impl PlaySession {
    pub(super) fn new(friend: OnlineId, role: Role, stage: Stage, sharing: bool) -> Self {
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

    pub fn shares_games(&self) -> bool {
        self.sharing
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
    pub(crate) fn log(&self) -> Option<&super::engine::Log> {
        self.round.rollback().map(|r| &r.log)
    }

    #[cfg(test)]
    pub(crate) fn start(&self) -> Option<citrine_gb_net::Start> {
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

    pub(super) fn join_cables(&mut self) {
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

    pub(super) fn keep_saves(&mut self, dumps: impl IntoIterator<Item = SramDump>) {
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
}
