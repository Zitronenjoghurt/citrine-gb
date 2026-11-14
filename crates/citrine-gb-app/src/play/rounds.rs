use super::engine::Rollback;
use super::shelf::{Finder, Look};
use super::{
    CableLink, DropHistory, Ending, Engine, GUEST_POWER_ON, GameFile, KEPT_PINGS, KEPT_ROMS,
    PlaySession, Returned, Role, Round, SETUP_PINGS, SETUP_WAIT, Stage, kind_of, links, packed,
    random_seed, unix_now,
};
use citrine_gb::error::GbResult;
use citrine_gb::netplay::{NEVER, Sync};
use citrine_gb_net::{
    Console, GameCard, GameHash, Mode, OLDEST_PLAY_VERSION, Origin, PlayMsg, Start,
};
use citrine_gb_threads::{Lent, Pending, Threads};
use std::sync::Arc;
use web_time::Instant;

type Build = Box<dyn FnOnce() -> GbResult<Rollback> + Send>;

pub(super) enum Building {
    Planned(Build),
    Running(Pending<GbResult<Rollback>>),
}

impl PlaySession {
    pub(super) fn end(&mut self, ending: Ending) {
        self.stage = Stage::Ended(ending);
        self.round.sending = None;
        self.round.incoming = None;
        self.wants_state = false;
        self.settle_frames();
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
    }

    pub(super) fn begin(&mut self, now: Instant) {
        if self.stage != Stage::Inviting && self.stage != Stage::Preparing {
            return;
        }
        self.stage = Stage::Preparing;
        self.began = true;
        self.started_at = now;
        self.heard_at = now;
        self.outbox.push(PlayMsg::Watch(self.let_watch));
        self.outbox.push(PlayMsg::Cable(self.my_cable));
        self.join_round();
        self.ping(now);
    }

    pub(super) fn retire_round(&mut self) {
        self.settle_frames();
        if self.round.rollback().is_some_and(Rollback::is_group)
            && let Some(Engine::Rollback(r)) = self.round.engine.take()
        {
            self.stashed = Some(r.into_inner());
            return;
        }
        let save = self.round.rollback_mut().and_then(Rollback::final_save);
        self.keep_saves(save);
        let Some(engine) = self.round.engine.take() else {
            return;
        };
        let dropped = self.stage == Stage::Ended(Ending::Dropped);
        let history = match (&engine, self.id, &self.round.my_card) {
            (Engine::Rollback(r), Some(id), Some(card)) if dropped => {
                DropHistory::of(r.get(), id, self.round.number, self.friend, card.hash)
            }
            _ => None,
        };
        if let Engine::Rollback(r) = engine
            && !self.lent
            && let Some(console) = r.into_inner().into_console(dropped)
        {
            let borrowed = self
                .borrowing
                .then(|| self.round.my_card.as_ref().map(|c| c.hash))
                .flatten();
            self.returned = Some(Returned {
                console,
                borrowed,
                paused: dropped,
                friend: self.friend,
                history,
                downloaded: self.downloading,
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

    pub(super) fn restart(&mut self, now: Instant) {
        let mine = self.round.mine.take();
        let card = self.round.my_card.clone();
        self.restart_with(now, mine, card);
    }

    pub(super) fn restart_with(
        &mut self,
        now: Instant,
        mine: Option<GameFile>,
        card: Option<GameCard>,
    ) {
        self.started_at = now;
        self.heard_at = now;
        self.next_round(self.round.number + 1, mine, card);
    }

    fn join_round(&mut self) {
        if self.round.joined {
            return;
        }
        self.round.joined = true;
        self.outbox.push(PlayMsg::Join {
            version: OLDEST_PLAY_VERSION,
            round: self.round.number,
            game: self.round.my_card.clone(),
        });
        if self
            .round
            .my_card
            .as_ref()
            .is_some_and(|c| c.console.is_ds())
        {
            self.outbox.push(PlayMsg::DsUser {
                round: self.round.number,
                user: self.ds_user.clone(),
            });
        }
        if self.apart {
            return;
        }
        let resume = self.round.my_card.is_some()
            && (self.round.resume || self.round.mine.as_ref().is_some_and(|m| m.running));
        self.round.my_origin = match &self.round.mine {
            _ if resume => None,
            _ if self.downloading => Some(Origin::Download),
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

    pub(super) fn known_rom(&self, hash: &GameHash, finder: &mut Finder) -> Look {
        if let (Some(mine), Some(card)) = (&self.round.mine, &self.round.my_card)
            && card.hash == *hash
            && mine.rom.len() == card.size as usize
        {
            return Look::Found(mine.rom.clone());
        }
        if let Some((_, rom)) = self.roms.iter().find(|(h, _)| h == hash) {
            return Look::Found(rom.clone());
        }
        finder.look(hash)
    }

    pub(super) fn keep_rom(&mut self, hash: GameHash, rom: Arc<[u8]>) {
        self.roms.retain(|(h, _)| *h != hash);
        self.roms.push((hash, rom));
        while self.roms.len() > KEPT_ROMS {
            self.roms.remove(0);
        }
    }

    pub(super) fn try_start(&mut self, now: Instant) {
        let Some(theirs) = self.round.theirs.clone() else {
            return;
        };
        if self.apart {
            let start = Start {
                round: self.round.number,
                mode: Mode::Apart,
                seed: random_seed(),
                start_time: unix_now(),
                delay: 0,
                power_on: vec![NEVER, NEVER],
                plugged: false,
                start_frame: 0,
            };
            self.outbox.push(PlayMsg::Start(start.clone()));
            self.launch(start, now);
            return;
        }
        let mine = self.round.my_card.is_some();
        if mine && !self.round.set_up {
            return;
        }
        if theirs.is_some() && self.round.their_origin.is_none() {
            return;
        }
        if theirs.as_ref().is_some_and(|c| c.console.is_ds()) && self.round.their_user.is_none() {
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
        if !(i_hold && they_hold) && self.sharing.is_some() {
            return;
        }
        if both {
            let waited = now.duration_since(self.started_at) >= SETUP_WAIT;
            if self.round_trips.len() < SETUP_PINGS && !waited {
                return;
            }
        }
        let family = |card: &GameCard| match card.console {
            Console::Gb | Console::Gbc => 0,
            Console::Gba => 1,
            Console::Nds { .. } => 2,
        };
        let mixed = theirs
            .as_ref()
            .zip(self.round.my_card.as_ref())
            .is_some_and(|(t, m)| family(t) != family(m));
        let mode = if (!mine && theirs.is_none()) || mixed {
            Mode::Apart
        } else if i_hold && they_hold {
            Mode::Rollback
        } else if both {
            Mode::Cable
        } else {
            Mode::Apart
        };
        let power = |origin: Option<&Origin>, later: u32| match origin {
            None => NEVER,
            Some(Origin::Boot(_)) => later,
            Some(Origin::Resume(_) | Origin::Download) => 0,
        };
        let start = Start {
            round: self.round.number,
            mode,
            seed: random_seed(),
            start_time: unix_now(),
            delay: if both {
                (links::delay_for(
                    self.round_console(),
                    self.input_delay,
                    self.round_trip_ms(),
                    self.round_sync().budget(),
                ) + links::jitter_margin(
                    self.round_console(),
                    self.input_delay,
                    self.round_trip_spread_ms(),
                )) as u8
            } else {
                0
            },
            power_on: vec![
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
            start_frame: 0,
        };
        self.outbox.push(PlayMsg::Start(start.clone()));
        self.launch(start, now);
    }

    pub(super) fn launch(&mut self, start: Start, now: Instant) {
        self.round.start = Some(start.clone());
        if !self.watch_chosen && matches!(self.round_console(), Some(Console::Nds { .. })) {
            self.watch_friend = false;
        }
        let engine = match start.mode {
            Mode::Cable => Some(Engine::Cable(CableLink {
                plugged: self.my_cable && self.friend_cable,
                ..CableLink::default()
            })),
            Mode::Apart => Some(Engine::Apart),
            Mode::Rollback => match self.rollback(start, now) {
                Some(build) => {
                    self.round.building = Some(Building::Planned(build));
                    return;
                }
                None => None,
            },
        };
        self.seat(engine, now);
    }

    pub(super) fn build_engine(&mut self, threads: &Threads, now: Instant) {
        match self.round.building.take() {
            Some(Building::Planned(build)) => {
                let pending = threads.background("engine build", build);
                self.round.building = Some(Building::Running(pending));
            }
            Some(Building::Running(mut pending)) => match pending.try_take() {
                Some(built) => {
                    let early = std::mem::take(&mut self.round.early);
                    let room = match self.role {
                        Role::Host => links::delay_room(
                            self.round_console(),
                            self.input_delay,
                            self.round_trip_spread_ms(),
                        ),
                        Role::Guest => {
                            links::delay_room(self.round_console(), self.input_delay, None)
                        }
                    };
                    let built = match built {
                        Ok(Ok(rollback)) => Some(rollback),
                        Ok(Err(err)) => {
                            self.failure = Some(err.to_string());
                            None
                        }
                        Err(_) => None,
                    };
                    let engine = built.map(|mut r| {
                        r.set_delay_room(room);
                        for msg in early {
                            r.receive(msg);
                        }
                        r.watch(self.friend_lets_watch && self.watch_friend);
                        r.show(self.let_watch);
                        Engine::Rollback(Lent::new(Box::new(r)))
                    });
                    self.seat(engine, now);
                }
                None => self.round.building = Some(Building::Running(pending)),
            },
            None => {}
        }
    }

    fn seat(&mut self, engine: Option<Engine>, now: Instant) {
        match engine {
            Some(engine) => {
                self.round.engine = Some(engine);
                self.stage = Stage::Playing;
                self.heard_at = now;
            }
            None => self.end(Ending::Failed),
        }
    }

    fn round_console(&self) -> Option<Console> {
        let mut cards = self
            .round
            .my_card
            .iter()
            .chain(self.round.theirs.iter().flatten())
            .map(|card| card.console);
        let first = cards.next()?;
        Some(match cards.any(|console| console == Console::Gba) {
            true => Console::Gba,
            false => first,
        })
    }

    fn round_sync(&self) -> Sync {
        match self.round_console() {
            Some(console @ (Console::Gba | Console::Nds { .. })) => {
                links::sync_for(Some(console), 2, self.lanes)
            }
            _ => self.sync,
        }
    }

    pub(super) fn my_rom(&self) -> Option<Arc<[u8]>> {
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

    pub(super) fn rollback(&mut self, start: Start, now: Instant) -> Option<Build> {
        self.meet_at_common_frame();
        let mine = match (&self.round.my_card, &self.round.my_origin) {
            (Some(card), Some(origin)) => {
                Some((self.my_rom()?, kind_of(card.console), origin.clone()))
            }
            (Some(_), None) => return None,
            (None, _) => None,
        };
        let theirs = match (&self.round.theirs, &self.round.their_origin) {
            (Some(Some(card)), Some(origin)) => Some((
                self.round.their_rom.clone()?,
                kind_of(card.console),
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
        let mut users = [Some(self.ds_user.clone()), self.round.their_user.clone()];
        if local == citrine_gb::netplay::Player::Two {
            games.swap(0, 1);
            users.swap(0, 1);
        }
        let sample_rate = self.sample_rate;
        let sync = self.round_sync();
        Some(Box::new(move || {
            Rollback::new(local, start, games, users, sample_rate, sync, now)
        }))
    }
}
