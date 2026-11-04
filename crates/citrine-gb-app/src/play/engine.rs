use super::rejoin::KEPT_FRAMES;
use citrine_gb::console::{Console, ConsoleKind, Keys};
use citrine_gb::error::{GbError, GbResult};
use citrine_gb::gb::GameBoy;
#[cfg(feature = "gba")]
use citrine_gb::gba::machine::Gba;
use citrine_gb::machine::Machine;
use citrine_gb::netplay::{
    Advance, CableEvent, Check, ConsoleSetup, Input, InputError, LinkPort, NEVER, Player,
    ResyncError, Session, SessionConfig, Sync,
};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_net::{Origin, Pad, PlayMsg, Start};
use std::collections::VecDeque;
use std::time::Duration;
use web_time::Instant;

const FRAME_MS: f64 = 70_224.0 * 1000.0 / 4_194_304.0;
const SAVE_EVERY: u32 = 60;
const SKIP_EVERY: u32 = 6;
const AHEAD_LIMIT: f64 = 1.5;
const WATCH_BEHIND: u32 = 6;

pub type Setup = (Vec<u8>, ConsoleKind, Origin);

pub struct WatchPoint {
    pub frame: u32,
    pub cable: Vec<u8>,
    pub states: Vec<(u8, Vec<u8>)>,
}

pub const GROUP_UNITS: usize = citrine_gb::netplay::MAX_UNITS;

enum Linked {
    Gb(Session<GameBoy>),
    #[cfg(feature = "gba")]
    Gba(Session<Gba>),
    #[cfg(feature = "gba")]
    Group(Box<Session<Gba, GROUP_UNITS>>),
}

macro_rules! each {
    ($linked:expr, $session:ident => $body:expr) => {
        match $linked {
            Linked::Gb($session) => $body,
            #[cfg(feature = "gba")]
            Linked::Gba($session) => $body,
            #[cfg(feature = "gba")]
            Linked::Group($session) => $body,
        }
    };
}

impl Linked {
    fn new(config: SessionConfig, setups: [Option<ConsoleSetup>; 2]) -> GbResult<Self> {
        let kinds: Vec<bool> = setups
            .iter()
            .flatten()
            .map(|setup| setup.kind == ConsoleKind::Gba)
            .collect();
        match (kinds.contains(&true), kinds.contains(&false)) {
            (true, true) => Err(GbError::GbaGame),
            #[cfg(feature = "gba")]
            (true, false) => Ok(Self::Gba(Session::new_gba(config, setups)?)),
            #[cfg(not(feature = "gba"))]
            (true, false) => Err(GbError::GbaGame),
            _ => Ok(Self::Gb(Session::new(config, setups)?)),
        }
    }

    fn group(
        config: SessionConfig<GROUP_UNITS>,
        setups: [Option<ConsoleSetup>; GROUP_UNITS],
    ) -> GbResult<Self> {
        if setups.iter().flatten().any(|s| s.kind != ConsoleKind::Gba) {
            return Err(GbError::GbaGame);
        }
        #[cfg(feature = "gba")]
        return Ok(Self::Group(Box::new(Session::new_gba(config, setups)?)));
        #[cfg(not(feature = "gba"))]
        {
            let _ = config;
            Err(GbError::GbaGame)
        }
    }

    fn console(&self, player: Player) -> Option<&dyn Console> {
        each!(self, s => s.present(player).then(|| s.console(player) as &dyn Console))
    }

    #[cfg(test)]
    fn encoded(&self, player: Player) -> Option<Vec<u8>> {
        each!(self, s => s
            .present(player)
            .then(|| citrine_gb::netplay::encode_state(s.console(player)).ok())
            .flatten())
    }

    fn into_machine(self, player: Player, confirmed: bool) -> Machine {
        match self {
            Self::Gb(s) => Machine::Gb(Box::new(unplugged(s, player, confirmed))),
            #[cfg(feature = "gba")]
            Self::Gba(s) => Machine::Gba(Box::new(unplugged(s, player, confirmed))),
            #[cfg(feature = "gba")]
            Self::Group(s) => Machine::Gba(Box::new(unplugged(*s, player, confirmed))),
        }
    }

    fn sync(&self) -> Sync {
        each!(self, s => s.sync())
    }

    fn frame(&self) -> u32 {
        each!(self, s => s.frame())
    }

    fn confirmed_frame(&self) -> u32 {
        each!(self, s => s.confirmed_frame())
    }

    fn local(&self) -> Player {
        each!(self, s => s.local())
    }

    fn present(&self, player: Player) -> bool {
        player.index() < self.units() && each!(self, s => s.present(player))
    }

    fn units(&self) -> usize {
        match self {
            Self::Gb(_) => 2,
            #[cfg(feature = "gba")]
            Self::Gba(_) => 2,
            #[cfg(feature = "gba")]
            Self::Group(_) => GROUP_UNITS,
        }
    }

    fn cable_plugged(&self) -> bool {
        each!(self, s => s.cable_plugged())
    }

    fn remote_frame(&self) -> u32 {
        each!(self, s => s.remote_frame())
    }

    fn waiting_for(&self) -> Option<Player> {
        each!(self, s => s.waiting_for())
    }

    fn desync(&self) -> Option<(u32, Vec<bool>)> {
        each!(self, s => s.desync().map(|d| (d.frame, d.consoles.to_vec())))
    }

    fn checkpoint(&self, frame: u32, player: Player) -> Option<Vec<u8>> {
        each!(self, s => s.checkpoint(frame, player))
    }

    fn watch_remote(&mut self, on: bool) {
        each!(self, s => s.watch_remote(on))
    }

    fn watching(&self) -> bool {
        each!(self, s => s.watching())
    }

    fn watch_point(&self) -> Option<(u32, Vec<u8>)> {
        each!(self, s => {
            let at = s.latest_checkpoint()?;
            Some((at, s.checkpoint_cable(at)?))
        })
    }

    fn restore_cable(&mut self, data: &[u8]) -> GbResult<()> {
        each!(self, s => s.restore_cable(data))
    }

    fn left_by(&self, player: Player, frame: u32) -> bool {
        each!(self, s => s.left_by(player, frame))
    }

    fn drain_audio(&mut self, out: &mut Vec<f32>) {
        each!(self, s => s.drain_audio(out))
    }

    fn drain_remote_audio(&mut self, out: &mut Vec<f32>) {
        each!(self, s => s.drain_remote_audio(out))
    }

    fn local_save(&mut self, force: bool) -> Option<SramDump> {
        each!(self, s => s.local_save(force))
    }

    fn add_local_input(&mut self, frame: u32, input: Input) -> Result<(), InputError> {
        each!(self, s => s.add_local_input(frame, input))
    }

    fn add_remote_input(&mut self, frame: u32, input: Input) {
        each!(self, s => s.add_remote_input(frame, input))
    }

    fn add_player_input(&mut self, player: Player, frame: u32, input: Input) {
        each!(self, s => s.add_player_input(player, frame, input))
    }

    fn leave(&mut self, player: Player, at: u32) -> bool {
        each!(self, s => s.leave(player, at.max(s.confirmed_frame())).is_ok())
    }

    fn advance(&mut self) -> Advance {
        each!(self, s => s.advance())
    }

    fn take_check(&mut self) -> Option<(u32, Vec<u64>)> {
        each!(self, s => s.take_check().map(|c| (c.frame, c.hashes.to_vec())))
    }

    fn add_remote_check(&mut self, frame: u32, hashes: &[u64]) {
        each!(self, s => if let Ok(hashes) = hashes.try_into() {
            s.add_remote_check(Check { frame, hashes });
        })
    }

    fn resync(&mut self, frame: u32, player: Player, data: &[u8]) -> Result<(), ResyncError> {
        each!(self, s => s.resync(frame, player, data))
    }

    #[cfg(test)]
    fn cable_transfers(&self) -> Vec<u64> {
        each!(self, s => s.cable_transfers().to_vec())
    }

    fn keeping_confirmed(self, frames: u32) -> Self {
        match self {
            Self::Gb(s) => Self::Gb(s.keeping_confirmed(frames)),
            #[cfg(feature = "gba")]
            Self::Gba(s) => Self::Gba(s.keeping_confirmed(frames)),
            #[cfg(feature = "gba")]
            Self::Group(s) => Self::Group(Box::new(s.keeping_confirmed(frames))),
        }
    }

    fn confirmed_states(&self, player: Player, keep: usize) -> Option<Vec<(u32, Vec<u8>)>> {
        each!(self, s => encoded_states(s.confirmed_states(player), keep))
    }

    #[cfg(test)]
    fn game_boy(&self, player: Player) -> Option<&GameBoy> {
        match self {
            Linked::Gb(s) => s.present(player).then(|| s.console(player)),
            #[cfg(feature = "gba")]
            _ => None,
        }
    }
}

pub struct Rollback {
    session: Linked,
    round: u32,
    host: bool,
    authority: bool,
    group: Option<Box<GroupWire>>,
    watching_only: bool,
    watched_only: bool,
    hidden: bool,
    delay: u32,
    filled: u32,
    unsent_from: u32,
    unsent: Vec<Pad>,
    cable: Option<CableEvent>,
    cable_at: Option<u32>,
    plug_at_start: bool,
    ahead: f64,
    skips: u32,
    stalled_since: Option<Instant>,
    answered: Option<u32>,
    resyncs: u32,
    outgoing: Vec<PlayMsg>,
    save: Option<SramDump>,
    #[cfg(test)]
    pub log: Log,
}

const KEPT_GROUP_CHECKS: usize = 8;
const KEPT_PADS: usize = 180;

#[derive(Default)]
struct GroupWire {
    authority: u8,
    watched: Option<Player>,
    sent: VecDeque<(u32, Pad)>,
    addressed: Vec<(Option<u8>, PlayMsg)>,
    mine: VecDeque<(u32, Vec<u64>)>,
    theirs: Vec<(u8, u32, Vec<u64>)>,
}

#[cfg(test)]
#[derive(Default, Clone)]
pub struct Log {
    pub local: Vec<Option<Input>>,
    pub remote: Vec<Option<Input>>,
    pub units: [Vec<Option<Input>>; GROUP_UNITS],
    pub checks: Vec<Check>,
    pub group_checks: Vec<(u32, Vec<u64>)>,
    pub resynced: Vec<u32>,
    pub started: Vec<Option<Vec<u8>>>,
}

#[cfg(test)]
impl Log {
    fn put(list: &mut Vec<Option<Input>>, frame: u32, input: Input) {
        let at = frame as usize;
        if list.len() <= at {
            list.resize(at + 1, None);
        }
        list[at] = Some(input);
    }
}

fn encoded_states<C: citrine_gb::console::Rollback + LinkPort>(
    mut states: Vec<(u32, C)>,
    keep: usize,
) -> Option<Vec<(u32, Vec<u8>)>> {
    states.drain(..states.len().saturating_sub(keep));
    states
        .into_iter()
        .map(|(at, mut console)| {
            console.unplug_link();
            Some((at, citrine_gb::netplay::encode_state(&console).ok()?))
        })
        .collect()
}

fn setups_of<'a, const N: usize>(
    roms: &'a [Option<Rom>; N],
    games: &'a [Option<Setup>; N],
) -> [Option<ConsoleSetup<'a>>; N] {
    std::array::from_fn(|i| {
        let (Some(rom), Some((_, kind, origin))) = (&roms[i], &games[i]) else {
            return None;
        };
        let (save, state) = match origin {
            Origin::Boot(save) => (save.as_deref().map(SramDump::from_slice), None),
            Origin::Resume(state) => (None, Some(state.as_slice())),
        };
        Some(ConsoleSetup {
            rom,
            kind: *kind,
            save,
            state,
        })
    })
}

fn unplugged<C: citrine_gb::console::Rollback + LinkPort, const N: usize>(
    session: Session<C, N>,
    player: Player,
    confirmed: bool,
) -> C {
    let mut console = match confirmed {
        true => session.into_confirmed_console(player),
        false => session.into_console(player),
    };
    console.unplug_link();
    console
}

impl Rollback {
    pub fn new(
        local: Player,
        start: Start,
        games: [Option<Setup>; 2],
        sample_rate: u32,
        sync: Sync,
        now: Instant,
    ) -> GbResult<Self> {
        let roms = games
            .each_ref()
            .map(|game| game.as_ref().map(|(rom, _, _)| Rom::new(rom)));
        let setups = setups_of(&roms, &games);
        let present = [setups[0].is_some(), setups[1].is_some()];
        let config = SessionConfig {
            power_on: start.power_on.as_slice().try_into().unwrap_or([NEVER; 2]),
            sample_rate,
            sync: sync.with_delay(u32::from(start.delay)),
            ..SessionConfig::new(local, start.seed, start.start_time)
        };
        let session = Linked::new(config, setups)?.keeping_confirmed(KEPT_FRAMES);
        let _ = now;
        let me = present[local.index()];
        let friend = present[local.other().index()];
        let authority = if me && friend {
            local == Player::One
        } else {
            me
        };
        Ok(Self::with(session, &start, authority, None, me && !friend))
    }

    pub fn group(
        local: Player,
        start: Start,
        games: [Option<Setup>; GROUP_UNITS],
        sample_rate: u32,
        sync: Sync,
        authority: u8,
    ) -> GbResult<Self> {
        let roms = games
            .each_ref()
            .map(|game| game.as_ref().map(|(rom, _, _)| Rom::new(rom)));
        let setups = setups_of(&roms, &games);
        let mut power_on = [NEVER; GROUP_UNITS];
        for (at, from) in power_on.iter_mut().zip(&start.power_on) {
            *at = *from;
        }
        let config = SessionConfig {
            power_on,
            sample_rate,
            start_frame: start.start_frame,
            sync: sync.with_delay(u32::from(start.delay)),
            ..SessionConfig::new(local, start.seed, start.start_time)
        };
        let session = Linked::group(config, setups)?;
        let wire = GroupWire {
            authority,
            ..GroupWire::default()
        };
        let mine = local.index() == usize::from(authority);
        let mut r = Self::with(session, &start, mine, Some(Box::new(wire)), false);
        r.filled = start.start_frame;
        r.watching_only |= start.start_frame > 0;
        Ok(r)
    }

    fn with(
        session: Linked,
        start: &Start,
        authority: bool,
        group: Option<Box<GroupWire>>,
        watched_only: bool,
    ) -> Self {
        #[cfg(test)]
        let started = (0..session.units())
            .map(|i| session.encoded(Player::ALL[i]))
            .collect();
        let local = session.local();
        let watching_only = !session.present(local);
        Self {
            session,
            round: start.round,
            host: local == Player::One,
            authority,
            group,
            watching_only,
            watched_only,
            hidden: false,
            delay: start.delay as u32,
            filled: 0,
            unsent_from: 0,
            unsent: Vec::new(),
            cable: None,
            cable_at: start.plugged.then_some(0),
            plug_at_start: start.plugged,
            ahead: 0.0,
            skips: 0,
            stalled_since: None,
            answered: None,
            resyncs: 0,
            outgoing: Vec::new(),
            save: None,
            #[cfg(test)]
            log: Log {
                started,
                ..Log::default()
            },
        }
    }

    pub fn is_group(&self) -> bool {
        self.group.is_some()
    }

    pub fn group_round(&self) -> Option<u32> {
        self.group.as_ref().map(|_| self.round)
    }

    pub fn local_unit(&self) -> u8 {
        self.session.local().index() as u8
    }

    pub fn units(&self) -> Vec<u8> {
        (0..self.session.units())
            .filter(|&i| self.session.present(Player::ALL[i]))
            .map(|i| i as u8)
            .collect()
    }

    pub fn set_authority(&mut self, unit: u8) {
        let local = self.local_unit();
        if let Some(g) = &mut self.group {
            g.authority = unit;
            self.authority = unit == local && !self.watching_only;
        }
    }

    pub fn watch_unit(&mut self, unit: Option<u8>) {
        if let Some(g) = &mut self.group {
            g.watched = unit.map(|u| Player::ALL[usize::from(u) % GROUP_UNITS]);
        }
    }

    pub fn watched_unit(&self) -> Option<u8> {
        let g = self.group.as_ref()?;
        let local = self.session.local();
        g.watched
            .filter(|p| *p != local && self.session.present(*p))
            .or_else(|| {
                (0..self.session.units())
                    .map(|i| Player::ALL[i])
                    .find(|p| *p != local && self.session.present(*p))
            })
            .map(|p| p.index() as u8)
    }

    pub fn unit_console(&self, unit: u8) -> Option<&dyn Console> {
        self.session
            .console(Player::ALL[usize::from(unit) % GROUP_UNITS])
    }

    pub fn waiting_unit(&self) -> Option<u8> {
        self.session.waiting_for().map(|p| p.index() as u8)
    }

    pub fn stop_frame(&self) -> u32 {
        self.filled.max(self.session.frame())
    }

    pub fn leave_unit(&mut self, unit: u8, at: u32) -> bool {
        let player = Player::ALL[usize::from(unit) % GROUP_UNITS];
        if player == self.session.local() || !self.session.present(player) {
            return false;
        }
        self.session.leave(player, at)
    }

    pub fn confirmed_frame(&self) -> u32 {
        self.session.confirmed_frame()
    }

    pub fn recent_pads(&self) -> Option<PlayMsg> {
        let g = self.group.as_ref()?;
        let first = g.sent.front()?.0;
        Some(PlayMsg::Pads {
            round: self.round,
            unit: Some(self.local_unit()),
            first,
            pads: g.sent.iter().map(|(_, pad)| *pad).collect(),
        })
    }

    pub fn watch_point(&self) -> Option<WatchPoint> {
        self.group.as_ref()?;
        let (frame, cable) = self.session.watch_point().filter(|(at, _)| *at > 0)?;
        let mut states = Vec::new();
        for unit in self.units() {
            let player = Player::ALL[usize::from(unit)];
            if self.session.left_by(player, frame) {
                continue;
            }
            states.push((unit, self.session.checkpoint(frame, player)?));
        }
        Some(WatchPoint {
            frame,
            cable,
            states,
        })
    }

    pub fn restore_cable(&mut self, data: &[u8]) -> bool {
        self.session.restore_cable(data).is_ok()
    }

    pub fn take_addressed(&mut self) -> Vec<(Option<u8>, PlayMsg)> {
        self.group
            .as_mut()
            .map(|g| std::mem::take(&mut g.addressed))
            .unwrap_or_default()
    }

    pub fn sync(&self) -> Sync {
        self.session.sync()
    }

    pub fn frame(&self) -> u32 {
        self.session.frame()
    }

    pub fn resyncs(&self) -> u32 {
        self.resyncs
    }

    pub fn cable_plugged(&self) -> bool {
        match self.cable {
            Some(CableEvent::Plug) => true,
            Some(CableEvent::Pull) => false,
            None => self.session.cable_plugged(),
        }
    }

    pub fn console(&self, local: bool) -> Option<&dyn Console> {
        let me = self.session.local();
        let player = match (local, self.watched_unit()) {
            (true, _) => me,
            (false, Some(unit)) => Player::ALL[usize::from(unit)],
            (false, None) => me.other(),
        };
        self.session.console(player)
    }

    pub fn stalled_for(&self, now: Instant) -> Duration {
        self.stalled_since
            .map_or(Duration::ZERO, |at| now.duration_since(at))
    }

    pub fn watch(&mut self, on: bool) {
        self.session.watch_remote(on);
    }

    pub fn watches(&self) -> bool {
        self.session.watching()
    }

    pub fn show(&mut self, on: bool) {
        self.hidden = self.watched_only && !on;
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    pub fn cable_settling(&self) -> bool {
        self.cable.is_some() || self.cable_at.is_some_and(|at| at >= self.session.frame())
    }

    pub fn request_cable(&mut self, plugged: bool) {
        if plugged == self.session.cable_plugged() {
            self.cable = None;
        } else {
            self.cable = Some(if plugged {
                CableEvent::Plug
            } else {
                CableEvent::Pull
            });
        }
    }

    pub fn drain_audio(&mut self, mine: &mut Vec<f32>, friend: &mut Vec<f32>) {
        self.session.drain_audio(mine);
        self.session.drain_remote_audio(friend);
    }

    pub fn take_save(&mut self) -> Option<SramDump> {
        self.save.take()
    }

    pub fn final_save(&mut self) -> Option<SramDump> {
        self.save.take().or_else(|| self.session.local_save(false))
    }

    #[cfg(test)]
    pub fn cable_transfers(&self) -> Vec<u64> {
        self.session.cable_transfers()
    }

    pub fn confirmed_states(&self, local: bool, keep: usize) -> Option<Vec<(u32, Vec<u8>)>> {
        let me = self.session.local();
        let player = if local { me } else { me.other() };
        match self.session.present(player) {
            true => self.session.confirmed_states(player, keep),
            false => Some(Vec::new()),
        }
    }

    #[cfg(test)]
    pub fn game_boy(&self, local: bool) -> Option<&GameBoy> {
        let me = self.session.local();
        self.session.game_boy(if local { me } else { me.other() })
    }

    pub fn into_console(self, confirmed: bool) -> Option<Machine> {
        let local = self.session.local();
        let present = self.session.present(local);
        present.then(|| self.session.into_machine(local, confirmed))
    }

    pub fn tick(
        &mut self,
        buttons: Keys,
        round_trip_ms: u32,
        now: Instant,
        outbox: &mut Vec<PlayMsg>,
    ) -> bool {
        if self.watching_only {
            return self.follow(now);
        }
        let frame = self.session.frame();
        let heard = self.session.remote_frame();
        if heard > 0 {
            let remote_now =
                heard as f64 - self.delay as f64 - 1.0 + round_trip_ms as f64 / FRAME_MS / 2.0;
            self.ahead = self.ahead * 0.9 + (frame as f64 - remote_now) * 0.1;
            if self.ahead > AHEAD_LIMIT {
                self.skips += 1;
                if self.skips.is_multiple_of(SKIP_EVERY) {
                    return false;
                }
            }
        }
        while self.filled <= frame + self.delay {
            let at = self.filled;
            let mut input = if at < self.delay {
                Input::default()
            } else {
                Input::new(buttons)
            };
            if at == 0 && self.host && self.plug_at_start {
                input = input.with_cable(CableEvent::Plug);
            } else if at >= self.delay
                && let Some(event) = self.cable.take()
            {
                input = input.with_cable(event);
                self.cable_at = Some(at);
            }
            if self.session.add_local_input(at, input).is_err() {
                break;
            }
            #[cfg(test)]
            {
                Log::put(&mut self.log.local, at, input);
                let unit = self.session.local().index();
                Log::put(&mut self.log.units[unit], at, input);
            }
            if self.unsent.is_empty() {
                self.unsent_from = at;
            }
            self.unsent.push(pad_of(input));
            if let Some(g) = &mut self.group {
                g.sent.push_back((at, pad_of(input)));
                while g.sent.len() > KEPT_PADS {
                    g.sent.pop_front();
                }
            }
            self.filled += 1;
        }
        let ran = self.session.advance() == Advance::Ran;
        if ran {
            self.stalled_since = None;
            if self.session.frame().is_multiple_of(SAVE_EVERY)
                && let Some(save) = self.session.local_save(false)
            {
                self.save = Some(save);
            }
        } else {
            self.stalled_since.get_or_insert(now);
        }
        self.collect();
        self.flush(outbox);
        ran
    }

    fn follow(&mut self, now: Instant) -> bool {
        let behind = self
            .session
            .remote_frame()
            .saturating_sub(self.session.frame());
        let runs = if behind > WATCH_BEHIND { 2 } else { 1 };
        let mut ran = false;
        for _ in 0..runs {
            if self.session.advance() != Advance::Ran {
                break;
            }
            ran = true;
        }
        if ran {
            self.stalled_since = None;
        } else {
            self.stalled_since.get_or_insert(now);
        }
        self.collect();
        ran
    }

    fn collect(&mut self) {
        while let Some((frame, hashes)) = self.session.take_check() {
            #[cfg(test)]
            {
                if let Ok(hashes) = hashes.as_slice().try_into() {
                    self.log.checks.push(Check { frame, hashes });
                }
                self.log.group_checks.push((frame, hashes.clone()));
            }
            let check = PlayMsg::Check {
                round: self.round,
                frame,
                hashes: hashes.clone(),
            };
            match &mut self.group {
                None => self.outgoing.push(check),
                Some(g) if self.authority => {
                    g.mine.push_back((frame, hashes));
                    while g.mine.len() > KEPT_GROUP_CHECKS {
                        g.mine.pop_front();
                    }
                }
                Some(g) if !self.watching_only => g.addressed.push((Some(g.authority), check)),
                Some(_) => {}
            }
        }
        if !self.authority {
            return;
        }
        if self.group.is_some() {
            self.compare_group();
            return;
        }
        let Some((frame, consoles)) = self.session.desync() else {
            return;
        };
        if self.answered == Some(frame) {
            return;
        }
        self.answered = Some(frame);
        for player in [Player::One, Player::Two] {
            if consoles[player.index()]
                && let Some(data) = self.session.checkpoint(frame, player)
            {
                self.outgoing.push(PlayMsg::Resync {
                    round: self.round,
                    frame,
                    unit: player.index() as u8,
                    data,
                });
            }
        }
    }

    fn compare_group(&mut self) {
        let Some(g) = &mut self.group else {
            return;
        };
        let newest = g.mine.back().map(|(frame, _)| *frame);
        let mut answers = Vec::new();
        g.theirs.retain(|(from, frame, hashes)| {
            let Some((_, mine)) = g.mine.iter().find(|(f, _)| f == frame) else {
                return newest.is_none_or(|newest| *frame > newest);
            };
            for (unit, (a, b)) in mine.iter().zip(hashes).enumerate() {
                if a != b {
                    answers.push((*from, *frame, unit));
                }
            }
            false
        });
        for (to, frame, unit) in answers {
            if let Some(data) = self.session.checkpoint(frame, Player::ALL[unit]) {
                let msg = PlayMsg::Resync {
                    round: self.round,
                    frame,
                    unit: unit as u8,
                    data,
                };
                if let Some(g) = &mut self.group {
                    g.addressed.push((Some(to), msg));
                }
            }
        }
    }

    pub fn flush(&mut self, outbox: &mut Vec<PlayMsg>) {
        if let Some(g) = &mut self.group {
            if !self.unsent.is_empty() && !self.watching_only {
                g.addressed.push((
                    None,
                    PlayMsg::Pads {
                        round: self.round,
                        unit: Some(self.session.local().index() as u8),
                        first: self.unsent_from,
                        pads: std::mem::take(&mut self.unsent),
                    },
                ));
            }
            self.unsent.clear();
            g.addressed
                .extend(self.outgoing.drain(..).map(|msg| (None, msg)));
            return;
        }
        if self.hidden {
            self.unsent.clear();
            self.outgoing.clear();
            return;
        }
        if !self.unsent.is_empty() {
            outbox.push(PlayMsg::Pads {
                round: self.round,
                unit: None,
                first: self.unsent_from,
                pads: std::mem::take(&mut self.unsent),
            });
        }
        outbox.append(&mut self.outgoing);
    }

    pub fn receive_from(&mut self, from: u8, msg: PlayMsg) {
        let Some(g) = &mut self.group else {
            return;
        };
        match msg {
            PlayMsg::Pads {
                round,
                unit: Some(unit),
                first,
                pads,
            } if round == self.round && usize::from(unit) < GROUP_UNITS => {
                let player = Player::ALL[usize::from(unit)];
                let local = player == self.session.local();
                for (i, pad) in pads.into_iter().enumerate() {
                    let frame = first.wrapping_add(i as u32);
                    let input = input_of(pad);
                    #[cfg(test)]
                    Log::put(&mut self.log.units[usize::from(unit)], frame, input);
                    if !local {
                        self.session.add_player_input(player, frame, input);
                    } else if self.watching_only {
                        let _ = self.session.add_local_input(frame, input);
                    }
                }
            }
            PlayMsg::Check {
                round,
                frame,
                hashes,
            } if round == self.round && self.authority => {
                g.theirs.push((from, frame, hashes));
                while g.theirs.len() > KEPT_GROUP_CHECKS * GROUP_UNITS {
                    g.theirs.remove(0);
                }
                self.collect();
            }
            PlayMsg::Resync {
                round,
                frame,
                unit,
                data,
            } if round == self.round && !self.authority && from == g.authority => {
                let player = Player::ALL[usize::from(unit) % GROUP_UNITS];
                if self.session.resync(frame, player, &data).is_ok() {
                    self.resyncs += 1;
                    #[cfg(test)]
                    self.log.resynced.push(frame);
                }
            }
            _ => {}
        }
    }

    pub fn receive(&mut self, msg: PlayMsg) {
        if self.group.is_some() {
            return;
        }
        match msg {
            PlayMsg::Pads {
                round,
                unit: None,
                first,
                pads,
            } if round == self.round => {
                for (i, pad) in pads.into_iter().enumerate() {
                    let frame = first.wrapping_add(i as u32);
                    let input = input_of(pad);
                    #[cfg(test)]
                    Log::put(&mut self.log.remote, frame, input);
                    self.session.add_remote_input(frame, input);
                }
            }
            PlayMsg::Check {
                round,
                frame,
                hashes,
            } if round == self.round => {
                self.session.add_remote_check(frame, &hashes);
                self.collect();
            }
            PlayMsg::Resync {
                round,
                frame,
                unit,
                data,
            } if round == self.round && !self.authority => {
                let player = if unit == 0 { Player::One } else { Player::Two };
                if self.session.resync(frame, player, &data).is_ok() {
                    self.resyncs += 1;
                    #[cfg(test)]
                    self.log.resynced.push(frame);
                }
            }
            _ => {}
        }
    }
}

fn pad_of(input: Input) -> Pad {
    Pad {
        buttons: input.buttons.bits(),
        cable: input.cable.map(|event| event == CableEvent::Plug),
    }
}

fn input_of(pad: Pad) -> Input {
    let input = Input::new(Keys::from_bits_retain(pad.buttons));
    match pad.cable {
        Some(true) => input.with_cable(CableEvent::Plug),
        Some(false) => input.with_cable(CableEvent::Pull),
        None => input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_carry_buttons_and_cable_events_both_ways() {
        for input in [
            Input::default(),
            Input::new(Keys::A | Keys::LEFT),
            Input::new(Keys::all()).with_cable(CableEvent::Plug),
            Input::new(Keys::L | Keys::R),
            Input::new(Keys::DOWN).with_cable(CableEvent::Pull),
        ] {
            assert_eq!(input_of(pad_of(input)), input);
        }
    }

    #[test]
    fn encoded_pads_keep_their_bytes() {
        let pads = [
            Input::default(),
            Input::new(Keys::A | Keys::LEFT),
            Input::new(Keys::all()).with_cable(CableEvent::Plug),
            Input::new(Keys::START | Keys::DOWN).with_cable(CableEvent::Pull),
            Input::new(Keys::L | Keys::R),
        ]
        .map(pad_of)
        .to_vec();
        let message = PlayMsg::Pads {
            round: 3,
            unit: None,
            first: 1_234,
            pads,
        };

        let bytes = postcard::to_allocvec(&message).unwrap();

        assert_eq!(
            bytes,
            [
                6, 3, 210, 9, 5, 0, 0, 33, 0, 255, 7, 1, 1, 136, 1, 1, 0, 128, 6, 0
            ]
        );
    }
}
