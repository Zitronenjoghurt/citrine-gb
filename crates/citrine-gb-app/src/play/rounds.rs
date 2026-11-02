use super::engine::Rollback;
use super::{
    CableLink, Ending, Engine, GUEST_POWER_ON, GameFile, KEPT_PINGS, KEPT_ROMS, PlaySession,
    Returned, Role, Round, SETUP_PINGS, SETUP_WAIT, SLOW_DELAY, SLOW_ROUND_TRIP_MS, Shelf, Stage,
    model_of, packed, random_seed, sha1_of, unix_now,
};
use citrine_gb::netplay::NEVER;
use citrine_gb_net::{GameCard, GameHash, Mode, Origin, PLAY_VERSION, PlayMsg, Start};
use web_time::Instant;

impl PlaySession {
    pub(super) fn end(&mut self, ending: Ending) {
        self.stage = Stage::Ended(ending);
        self.round.sending = None;
        self.round.incoming = None;
        self.wants_state = false;
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
    }

    pub(super) fn begin(&mut self, now: Instant) {
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

    pub(super) fn retire_round(&mut self) {
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

    pub(super) fn next_round(
        &mut self,
        number: u32,
        mine: Option<GameFile>,
        card: Option<GameCard>,
    ) {
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

    pub(super) fn send_setup(&mut self) {
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

    pub(super) fn ping(&mut self, now: Instant) {
        let n = self.next_ping;
        self.next_ping = self.next_ping.wrapping_add(1);
        self.pings.push_back((n, now));
        while self.pings.len() > KEPT_PINGS {
            self.pings.pop_front();
        }
        self.pinged_at = Some(now);
        self.outbox.push(PlayMsg::Ping(n));
    }

    pub(super) fn known_rom(&self, hash: &GameHash, shelf: &dyn Shelf) -> Option<Vec<u8>> {
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

    pub(super) fn keep_rom(&mut self, hash: GameHash, rom: &[u8]) {
        self.roms.retain(|(h, _)| *h != hash);
        self.roms.push((hash, rom.to_vec()));
        while self.roms.len() > KEPT_ROMS {
            self.roms.remove(0);
        }
    }

    pub(super) fn try_start(&mut self, now: Instant) {
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

    pub(super) fn launch(&mut self, start: Start, now: Instant) {
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

    pub(super) fn rollback(&mut self, start: Start, now: Instant) -> Option<Rollback> {
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
}
