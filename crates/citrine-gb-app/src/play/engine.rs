use super::links::{CostMeter, DsCost};
use super::pace;
use super::rejoin::KEPT_FRAMES;
use crate::utils::avg_timer::AvgTimer;
use citrine_gb::console::{Console, ConsoleKind, Keys, Timing, Touch};
use citrine_gb::error::{GbError, GbResult};
use citrine_gb::gb::GameBoy;
#[cfg(feature = "gba")]
use citrine_gb::gba::machine::Gba;
use citrine_gb::machine::Machine;
#[cfg(feature = "nds")]
use citrine_gb::nds::machine::{Nds, NdsFirmware};
use citrine_gb::netplay::{
    Advance, CableEvent, CablePace, Check, ConsoleSetup, DEFAULT_CHECK_INTERVAL, Input, InputError,
    LinkPort, NEVER, Player, ResyncError, Session, SessionConfig, Sync,
};
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_net::{DsUser, Origin, Pad, PlayMsg, Start};
use citrine_gb_threads::{Lendable, Split};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

const SAVE_EVERY: u32 = 60;
const SKIP_EVERY: u32 = 6;
const AHEAD_LIMIT: f64 = 1.5;
const MAX_OWED: u32 = 8;
const REPAY_ROOM: f64 = 1.0;
const REPAY_SPARE: u32 = 1;
const REPAY_GAP: u32 = 3;
const WATCH_BEHIND: u32 = 6;
const WAIT_WINDOW: u32 = 300;
const WAIT_LIMIT: usize = 5;
pub(super) const CALM_FRAMES: u32 = 1200;
const QUICK_CALM_FRAMES: u32 = 60;
const QUICK_GAP: u32 = 2;
const EASE_SLACK: u32 = 2;
const LONG_STALL: u32 = 3;
const RISE_STALL: u32 = 2;
const SPARE_FRAMES: u32 = 60;
const SPELL_DROP: u32 = 2;
const SPREAD_TICKS: u32 = 15;
const PACE_STEP: u32 = 30;
const PACE_SAMPLES: usize = 9;
const SLOW_PACE: f64 = 0.98;

pub type Setup = (Arc<[u8]>, ConsoleKind, Origin);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub ran: u64,
    pub waited: u64,
    pub skipped: u64,
    pub repaid: u64,
}

pub struct Ticks {
    pub pads: Vec<Keys>,
    pub touch: Option<Touch>,
    pub cable: Option<bool>,
    pub saves: bool,
    pub round_trip_ms: u32,
    pub spread_ms: u32,
    pub aim: Option<u32>,
    pub rise_aim: Option<u32>,
    pub now: Instant,
}

#[derive(Default)]
pub struct Ticked {
    pub ran: bool,
    pub outbox: Vec<PlayMsg>,
    pub saves: Vec<SramDump>,
}

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
    #[cfg(feature = "nds")]
    Nds(Box<Session<Nds>>),
    #[cfg(feature = "nds")]
    NdsGroup(Box<Session<Nds, GROUP_UNITS>>),
}

macro_rules! each {
    ($linked:expr, $session:ident => $body:expr) => {
        match $linked {
            Linked::Gb($session) => $body,
            #[cfg(feature = "gba")]
            Linked::Gba($session) => $body,
            #[cfg(feature = "gba")]
            Linked::Group($session) => $body,
            #[cfg(feature = "nds")]
            Linked::Nds($session) => $body,
            #[cfg(feature = "nds")]
            Linked::NdsGroup($session) => $body,
        }
    };
}

impl Linked {
    fn new(
        config: SessionConfig,
        setups: [Option<ConsoleSetup>; 2],
        users: &[Option<DsUser>; 2],
        downloads: [bool; 2],
    ) -> GbResult<Self> {
        if ds_session(&setups)? {
            #[cfg(feature = "nds")]
            return Ok(Self::Nds(Box::new(Session::new_nds(
                config,
                nds_setups(setups, users, downloads),
            )?)));
        }
        let _ = (users, downloads);
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
        users: &[Option<DsUser>; GROUP_UNITS],
        downloads: [bool; GROUP_UNITS],
    ) -> GbResult<Self> {
        if ds_session(&setups)? {
            #[cfg(feature = "nds")]
            return Ok(Self::NdsGroup(Box::new(Session::new_nds(
                config,
                nds_setups(setups, users, downloads),
            )?)));
        }
        let _ = (users, downloads);
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

    fn fetch(&self, player: Player) -> Option<super::fetch::Fetch> {
        match self {
            #[cfg(feature = "nds")]
            Self::Nds(s) => s.console(player).download().map(super::fetch::Fetch::of),
            _ => {
                let _ = player;
                None
            }
        }
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
            #[cfg(feature = "nds")]
            Self::Nds(s) => Machine::Nds(Box::new(unplugged(*s, player, confirmed))),
            #[cfg(feature = "nds")]
            Self::NdsGroup(s) => Machine::Nds(Box::new(unplugged(*s, player, confirmed))),
        }
    }

    fn sync(&self) -> Sync {
        each!(self, s => s.sync())
    }

    fn set_budget(&mut self, budget: u32) {
        each!(self, s => s.set_budget(budget))
    }

    fn last_rollback(&self) -> u32 {
        each!(self, s => s.last_rollback())
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
            #[cfg(feature = "nds")]
            Self::Nds(_) => 2,
            #[cfg(feature = "nds")]
            Self::NdsGroup(_) => GROUP_UNITS,
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

    fn cable_pace(&self) -> Option<CablePace> {
        each!(self, s => s.cable_pace())
    }

    fn add_player_input(&mut self, player: Player, frame: u32, input: Input) {
        each!(self, s => s.add_player_input(player, frame, input))
    }

    fn leave(&mut self, player: Player, at: u32) -> Option<u32> {
        each!(self, s => {
            let at = at.max(s.confirmed_frame());
            s.leave(player, at).ok().map(|()| at)
        })
    }

    fn leave_from(&mut self, player: Player, at: u32) -> Option<u32> {
        let exact = each!(self, s => s.leave_from(player, at).is_ok());
        if exact {
            Some(at)
        } else {
            self.leave(player, at)
        }
    }

    fn advance(&mut self, split: &mut Split<'_>) -> Advance {
        each!(self, s => s.advance_on(split))
    }

    fn take_check(&mut self) -> Option<(u32, Vec<u64>)> {
        each!(self, s => s.take_check().map(|c| (c.frame, c.hashes.to_vec())))
    }

    fn add_remote_check(&mut self, frame: u32, hashes: &[u64]) {
        each!(self, s => if let Ok(hashes) = hashes.try_into() {
            s.add_remote_check(Check { frame, hashes });
        })
    }

    fn resync(
        &mut self,
        frame: u32,
        player: Player,
        data: &[u8],
        cable: Option<&[u8]>,
    ) -> Result<(), ResyncError> {
        each!(self, s => s.resync_with_cable(frame, player, data, cable))
    }

    fn checkpoint_cable(&self, frame: u32) -> Option<Vec<u8>> {
        each!(self, s => s.checkpoint_cable(frame))
    }

    fn local_from_peer(&self) -> bool {
        each!(self, s => s.local_from_peer())
    }

    fn input(&self, player: Player, frame: u32) -> Option<Input> {
        each!(self, s => s.input(player, frame))
    }

    #[cfg(test)]
    fn cable_transfers(&self) -> Vec<u64> {
        each!(self, s => s.cable_transfers().to_vec())
    }

    #[cfg(test)]
    fn latest_checkpoint(&self) -> Option<u32> {
        each!(self, s => s.latest_checkpoint())
    }

    fn keeping_confirmed(self, frames: u32) -> Self {
        match self {
            Self::Gb(s) => Self::Gb(s.keeping_confirmed(frames)),
            #[cfg(feature = "gba")]
            Self::Gba(s) => Self::Gba(s.keeping_confirmed(frames)),
            #[cfg(feature = "gba")]
            Self::Group(s) => Self::Group(Box::new(s.keeping_confirmed(frames))),
            #[cfg(feature = "nds")]
            Self::Nds(s) => Self::Nds(Box::new(s.keeping_confirmed(frames))),
            #[cfg(feature = "nds")]
            Self::NdsGroup(s) => Self::NdsGroup(Box::new(s.keeping_confirmed(frames))),
        }
    }

    fn trailing_checks(self, frames: u32) -> Self {
        match self {
            Self::Gb(s) => Self::Gb(s.trailing_checks(frames)),
            #[cfg(feature = "gba")]
            Self::Gba(s) => Self::Gba(s.trailing_checks(frames)),
            #[cfg(feature = "gba")]
            Self::Group(s) => Self::Group(Box::new(s.trailing_checks(frames))),
            #[cfg(feature = "nds")]
            Self::Nds(s) => Self::Nds(Box::new(s.trailing_checks(frames))),
            #[cfg(feature = "nds")]
            Self::NdsGroup(s) => Self::NdsGroup(Box::new(s.trailing_checks(frames))),
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

    #[cfg(feature = "nds")]
    fn nds_mut(&mut self, player: Player) -> Option<&mut Nds> {
        match self {
            Linked::Nds(s) if s.present(player) => Some(s.console_mut(player)),
            Linked::NdsGroup(s) if s.present(player) => Some(s.console_mut(player)),
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
    least_delay: u32,
    aim: u32,
    rise_aim: Option<u32>,
    quick: bool,
    delay_room: u32,
    delay_cap: u32,
    spell_from: Option<u32>,
    spell_top: u32,
    rose: Option<(u32, u32)>,
    spread_ms: u32,
    stall_run: u32,
    stall_base: u32,
    heard_frame: u32,
    heard_tick: u32,
    stall_quiet: bool,
    echo: Option<(u32, u32)>,
    held_delay: Option<u32>,
    last_wait: u32,
    least_spare: u32,
    calm_since: u32,
    least_slack: u32,
    ticks: u32,
    waits: VecDeque<u32>,
    paces: VecDeque<(u32, u32)>,
    filled: u32,
    unsent_from: u32,
    unsent: Vec<Pad>,
    cable: Option<CableEvent>,
    cable_at: Option<u32>,
    plug_at_start: bool,
    ahead: f64,
    skips: u32,
    stalled_since: Option<Instant>,
    owed: u32,
    since_repay: u32,
    counts: Counts,
    answered: Option<u32>,
    resyncs: u32,
    outgoing: Vec<PlayMsg>,
    save: Option<SramDump>,
    ticked: Ticked,
    frame_timer: AvgTimer,
    meter: Option<CostMeter>,
    #[cfg(test)]
    pub log: Log,
}

impl Lendable for Rollback {
    type Plan = Ticks;

    fn work(&mut self, plan: Ticks, split: &mut Split<'_>) {
        let due = u32::try_from(plan.pads.len()).unwrap_or(u32::MAX);
        self.ticks = self.ticks.saturating_add(due);
        let mut ran = false;
        for &buttons in &plan.pads {
            ran = self.run_tick(buttons, &plan, false, split);
        }
        self.since_repay = self.since_repay.saturating_add(due);
        if ran
            && self.owed > 0
            && self.spell_from.is_none()
            && self.since_repay >= REPAY_GAP
            && self.ahead < AHEAD_LIMIT - REPAY_ROOM
            && self.session.remote_frame() > self.session.frame() + REPAY_SPARE
            && let Some(&buttons) = plan.pads.last()
            && self.run_tick(buttons, &plan, true, split)
        {
            self.owed -= 1;
            self.since_repay = 0;
        }
        self.spread_ms = plan.spread_ms;
        self.aim_at(plan.aim);
        self.rise_aim = plan.rise_aim;
        self.ease_delay();
    }
}

impl Rollback {
    fn run_tick(
        &mut self,
        buttons: Keys,
        plan: &Ticks,
        repay: bool,
        split: &mut Split<'_>,
    ) -> bool {
        if let Some(plugged) = plan.cable
            && !self.cable_settling()
            && self.cable_plugged() != plugged
        {
            self.request_cable(plugged);
        }
        let mut outbox = std::mem::take(&mut self.ticked.outbox);
        self.frame_timer.start();
        let ran = if repay {
            self.step(buttons, plan.touch, plan.now, &mut outbox, split)
        } else {
            self.tick(
                buttons,
                plan.touch,
                plan.round_trip_ms,
                plan.now,
                &mut outbox,
                split,
            )
        };
        self.ticked.ran |= ran;
        if ran && repay {
            self.counts.repaid += 1;
        } else if ran {
            self.counts.ran += 1;
        }
        self.frame_timer.stop();
        self.ticked.outbox = outbox;
        if plan.saves
            && let Some(save) = self.take_save()
        {
            self.ticked.saves.push(save);
        }
        ran
    }
}

const _: fn() = || {
    fn send<T: Send>() {}
    send::<Rollback>();
};

const KEPT_GROUP_CHECKS: usize = 8;
const RESYNC_WINDOW: u32 = KEPT_GROUP_CHECKS as u32 * DEFAULT_CHECK_INTERVAL;
const KEPT_PADS: usize = 180;

#[derive(Default)]
struct GroupWire {
    authority: u8,
    watched: Option<Player>,
    sent: VecDeque<(u32, Pad)>,
    addressed: Vec<(Option<u8>, PlayMsg)>,
    mine: VecDeque<(u32, Vec<u64>)>,
    theirs: Vec<(u8, u32, Vec<u64>)>,
    answered: Vec<(u8, usize, u32)>,
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
    #[cfg(feature = "nds")]
    pub asks: u32,
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

fn roms_of<const N: usize>(games: &[Option<Setup>; N]) -> [Option<Rom>; N] {
    let mut made: Vec<Rom> = Vec::new();
    games.each_ref().map(|game| {
        let (data, kind, _) = game.as_ref()?;
        if let Some(rom) = made.iter().find(|rom| rom.data == *data) {
            if matches!(kind, ConsoleKind::Gba | ConsoleKind::Nds) {
                rom.ids();
            }
            return Some(rom.clone());
        }
        let rom = Rom::from(data.clone());
        made.push(rom.clone());
        Some(rom)
    })
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
            Origin::Download => (None, None),
        };
        Some(ConsoleSetup {
            rom,
            kind: *kind,
            save,
            state,
        })
    })
}

fn ds_session(setups: &[Option<ConsoleSetup>]) -> GbResult<bool> {
    let ds = setups
        .iter()
        .flatten()
        .filter(|setup| setup.kind == ConsoleKind::Nds)
        .count();
    match ds {
        0 => Ok(false),
        _ if ds < setups.iter().flatten().count() || cfg!(not(feature = "nds")) => {
            Err(GbError::NdsLink)
        }
        _ => Ok(true),
    }
}

#[cfg(feature = "nds")]
fn nds_setups<'a, const N: usize>(
    setups: [Option<ConsoleSetup<'a>>; N],
    users: &[Option<DsUser>; N],
    downloads: [bool; N],
) -> [Option<citrine_gb::netplay::NdsSetup<'a>>; N] {
    use citrine_gb::netplay::{NdsBoot, NdsSetup};
    let mut users = users.iter();
    let mut downloads = downloads.into_iter();
    setups.map(|setup| {
        let firmware = ds_firmware(users.next().and_then(Option::as_ref));
        let download = downloads.next().unwrap_or(false);
        let setup = setup?;
        let boot = match download {
            true => NdsBoot::DownloadPlay,
            false => NdsBoot::Card(setup),
        };
        Some(NdsSetup { boot, firmware })
    })
}

fn downloads<const N: usize>(games: &[Option<Setup>; N]) -> [bool; N] {
    games
        .each_ref()
        .map(|game| matches!(game, Some((_, _, Origin::Download))))
}

#[cfg(feature = "nds")]
pub fn ds_firmware(user: Option<&DsUser>) -> NdsFirmware {
    use citrine_gb::nds::spi::firmware::online::Online;
    use citrine_gb::nds::spi::firmware::{Language, UserSettings};
    let Some(user) = user else {
        return NdsFirmware::default();
    };
    let defaults = UserSettings::default();
    NdsFirmware {
        user_settings: Some(UserSettings {
            nickname: user.nickname.clone(),
            message: user.message.clone(),
            favorite_color: user.color & 0x0F,
            birthday: user.birthday,
            language: [
                Language::Japanese,
                Language::English,
                Language::French,
                Language::German,
                Language::Italian,
                Language::Spanish,
            ]
            .get(usize::from(user.language))
            .copied()
            .unwrap_or(defaults.language),
            ..defaults
        }),
        online: Online {
            mac: Some(user.mac),
            ..Online::default()
        },
        ..NdsFirmware::default()
    }
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
        users: [Option<DsUser>; 2],
        sample_rate: u32,
        sync: Sync,
        now: Instant,
    ) -> GbResult<Self> {
        let roms = roms_of(&games);
        let setups = setups_of(&roms, &games);
        let present = [setups[0].is_some(), setups[1].is_some()];
        let config = SessionConfig {
            power_on: start.power_on.as_slice().try_into().unwrap_or([NEVER; 2]),
            sample_rate,
            sync: sync.with_delay(u32::from(start.delay)),
            ..SessionConfig::new(local, start.seed, start.start_time)
        };
        let session =
            Linked::new(config, setups, &users, downloads(&games))?.keeping_confirmed(KEPT_FRAMES);
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
        users: [Option<DsUser>; GROUP_UNITS],
        sample_rate: u32,
        sync: Sync,
        authority: u8,
    ) -> GbResult<Self> {
        let roms = roms_of(&games);
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
        let session =
            Linked::group(config, setups, &users, downloads(&games))?.trailing_checks(KEPT_FRAMES);
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
            least_delay: start.delay as u32,
            aim: start.delay as u32,
            rise_aim: None,
            quick: false,
            delay_room: 0,
            delay_cap: 0,
            spell_from: None,
            spell_top: 0,
            rose: None,
            spread_ms: 0,
            stall_run: 0,
            stall_base: 0,
            heard_frame: 0,
            heard_tick: 0,
            stall_quiet: false,
            echo: None,
            held_delay: None,
            last_wait: 0,
            least_spare: u32::MAX,
            calm_since: 0,
            least_slack: u32::MAX,
            ticks: 0,
            waits: VecDeque::new(),
            paces: VecDeque::new(),
            filled: 0,
            unsent_from: 0,
            unsent: Vec::new(),
            cable: None,
            cable_at: start.plugged.then_some(0),
            plug_at_start: start.plugged,
            ahead: 0.0,
            skips: 0,
            stalled_since: None,
            owed: 0,
            since_repay: 0,
            counts: Counts::default(),
            answered: None,
            resyncs: 0,
            outgoing: Vec::new(),
            save: None,
            ticked: Ticked::default(),
            frame_timer: AvgTimer::default(),
            meter: None,
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

    pub fn leave_unit(&mut self, unit: u8, at: u32) -> Option<u32> {
        let player = Player::ALL[usize::from(unit) % GROUP_UNITS];
        if player == self.session.local() || !self.session.present(player) {
            return None;
        }
        self.session.leave(player, at)
    }

    pub fn follow_leave(&mut self, unit: u8, at: u32) -> Option<u32> {
        let player = Player::ALL[usize::from(unit) % GROUP_UNITS];
        if player == self.session.local() || !self.session.present(player) {
            return None;
        }
        self.session.leave_from(player, at)
    }

    pub fn confirmed_frame(&self) -> u32 {
        self.session.confirmed_frame()
    }

    pub fn pads_before(&self, unit: u8, upto: u32) -> Option<PlayMsg> {
        let player = Player::ALL[usize::from(unit) % GROUP_UNITS];
        let mut pads = Vec::new();
        let mut first = upto;
        while first > 0 && pads.len() < KEPT_PADS {
            let Some(input) = self.session.input(player, first - 1) else {
                break;
            };
            pads.push(pad_of(input));
            first -= 1;
        }
        if pads.is_empty() {
            return None;
        }
        pads.reverse();
        Some(PlayMsg::Pads {
            round: self.round,
            unit: Some(unit),
            first,
            pads,
        })
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

    pub fn measure_cost(&mut self) {
        self.meter = Some(CostMeter::default());
    }

    pub fn measured_cost(&self) -> Option<DsCost> {
        self.meter.as_ref().and_then(CostMeter::cost)
    }

    fn note_cost(&mut self, spent: Duration) {
        let Some(meter) = &mut self.meter else {
            return;
        };
        meter.note(spent, self.session.last_rollback());
        if let Some(cost) = meter.cost() {
            self.session.set_budget(cost.budget());
        }
    }

    pub fn frame(&self) -> u32 {
        self.session.frame()
    }

    pub fn resyncs(&self) -> u32 {
        self.resyncs
    }

    pub fn local_from_peer(&self) -> bool {
        self.session.local_from_peer()
    }

    #[cfg(test)]
    pub fn send_resync(&mut self, padding: &[u8]) -> Option<usize> {
        let frame = self.session.latest_checkpoint()?;
        let mut data = self.session.checkpoint(frame, Player::Two)?;
        data.extend_from_slice(padding);
        let len = data.len();
        self.outgoing.push(PlayMsg::Resync {
            round: self.round,
            frame,
            unit: 1,
            data,
            cable: None,
        });
        Some(len)
    }

    #[cfg(test)]
    pub fn send_resync_with(&mut self, edit: impl FnOnce(&mut GameBoy)) -> Option<()> {
        use citrine_gb::console::Rollback as _;
        let frame = self.session.latest_checkpoint()?;
        let data = self.session.checkpoint(frame, Player::Two)?;
        let mut gb = match &self.session {
            Linked::Gb(s) => s.console(Player::Two).decode_state(&data).ok()?,
            #[cfg(feature = "gba")]
            _ => return None,
        };
        edit(&mut gb);
        let data = gb.encode_state().ok()?;
        self.session.resync(frame, Player::Two, &data, None).ok()?;
        self.outgoing.push(PlayMsg::Resync {
            round: self.round,
            frame,
            unit: 1,
            data,
            cable: None,
        });
        Some(())
    }

    pub fn cable_plugged(&self) -> bool {
        match self.cable {
            Some(CableEvent::Plug) => true,
            Some(CableEvent::Pull) => false,
            None => self.session.cable_plugged(),
        }
    }

    pub fn timing(&self) -> Timing {
        self.console(true)
            .or(self.console(false))
            .map_or(ConsoleKind::Gb.timing(), |console| console.timing())
    }

    pub fn fetch(&self) -> Option<super::fetch::Fetch> {
        self.session.fetch(self.session.local())
    }

    pub fn console(&self, local: bool) -> Option<&dyn Console> {
        self.session.console(self.shown_player(local))
    }

    fn shown_player(&self, local: bool) -> Player {
        let me = self.session.local();
        match (local, self.watched_unit()) {
            (true, _) => me,
            (false, Some(unit)) => Player::ALL[usize::from(unit)],
            (false, None) => me.other(),
        }
    }

    #[cfg(feature = "nds")]
    pub fn nds_mut(&mut self, local: bool) -> Option<&mut Nds> {
        let player = self.shown_player(local);
        self.session.nds_mut(player)
    }

    pub fn stalled_for(&self, now: Instant) -> Duration {
        self.stalled_since
            .map_or(Duration::ZERO, |at| now.duration_since(at))
    }

    pub fn stalled_since(&self) -> Option<Instant> {
        self.stalled_since
    }

    pub fn watch(&mut self, on: bool) {
        self.session.watch_remote(on);
    }

    pub fn watches(&self) -> bool {
        self.session.watching()
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    pub fn ahead(&self) -> f64 {
        self.ahead
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

    pub fn take_ticked(&mut self) -> Ticked {
        std::mem::take(&mut self.ticked)
    }

    pub fn frame_timer(&self) -> &AvgTimer {
        &self.frame_timer
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
        touch: Option<Touch>,
        round_trip_ms: u32,
        now: Instant,
        outbox: &mut Vec<PlayMsg>,
        split: &mut Split<'_>,
    ) -> bool {
        if self.watching_only {
            return self.follow(now, split);
        }
        let frame = self.session.frame();
        let heard = self.session.remote_frame();
        self.note_pace(heard);
        if heard > 0 {
            let remote_now = heard as f64 - self.delay as f64 - 1.0
                + round_trip_ms as f64 / pace::frame_ms(self.timing()) / 2.0;
            self.ahead = self.ahead * 0.9 + (frame as f64 - remote_now) * 0.1;
            if self.ahead > AHEAD_LIMIT {
                self.skips += 1;
                if self.skips.is_multiple_of(SKIP_EVERY) {
                    self.counts.skipped += 1;
                    return false;
                }
            }
        }
        self.step(buttons, touch, now, outbox, split)
    }

    fn step(
        &mut self,
        buttons: Keys,
        touch: Option<Touch>,
        now: Instant,
        outbox: &mut Vec<PlayMsg>,
        split: &mut Split<'_>,
    ) -> bool {
        let frame = self.session.frame();
        while self.filled <= frame + self.delay {
            let at = self.filled;
            let mut input = if at < self.delay {
                Input::default()
            } else {
                Input::new(buttons).with_touch(touch)
            };
            if at == 0 && self.host && self.plug_at_start {
                input = input.with_cable(CableEvent::Plug);
            } else if at >= self.delay
                && let Some(event) = self.cable.take()
            {
                input = input.with_cable(event);
                self.cable_at = Some(at);
            }
            if at >= self.delay
                && let Some(pace) = self.session.cable_pace()
            {
                input = input.with_pace(pace);
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
        if self.session.remote_frame() != self.heard_frame {
            self.heard_frame = self.session.remote_frame();
            self.heard_tick = self.ticks;
        }
        let slack = (self.session.remote_frame() + self.session.sync().budget())
            .saturating_sub(self.session.frame());
        self.least_spare = self.least_spare.min(slack);
        if self.ahead <= AHEAD_LIMIT && !self.partner_slow() {
            self.least_slack = self.least_slack.min(slack);
        }
        if let Some((_, until)) = self.echo
            && self.ticks >= until
        {
            self.echo = None;
            if let Some(delay) = self.held_delay.take() {
                self.follow_lower(delay);
            }
        }
        let started = self.meter.is_some().then(Instant::now);
        let ran = self.session.advance(split) == Advance::Ran;
        if ran && let Some(started) = started {
            self.note_cost(started.elapsed());
        }
        if ran {
            self.stalled_since = None;
            let run = std::mem::take(&mut self.stall_run);
            if run >= LONG_STALL {
                self.stalled_long(run);
            }
            if self.session.frame().is_multiple_of(SAVE_EVERY)
                && let Some(save) = self.session.local_save(false)
            {
                self.save = Some(save);
            }
        } else {
            self.stalled_since.get_or_insert(now);
            self.counts.waited += 1;
            if self.spell_from.is_none() {
                self.owed = (self.owed + 1).min(MAX_OWED);
            }
            if self.stall_run == 0 {
                self.stall_base = self.delay;
            }
            self.stall_run += 1;
            if self.stall_run == 1 {
                self.stall_quiet = self.went_quiet();
            }
            if self.stall_run >= RISE_STALL || self.stall_quiet {
                self.rise_in_stall();
            }
            self.last_wait = self.ticks;
            self.least_spare = u32::MAX;
            if self.aim >= self.delay + QUICK_GAP
                || (self.ahead <= AHEAD_LIMIT && !self.partner_slow())
            {
                self.note_wait();
            }
        }
        self.collect();
        self.flush(outbox);
        ran
    }

    fn note_pace(&mut self, heard: u32) {
        if self
            .paces
            .back()
            .is_some_and(|&(at, _)| self.ticks < at + PACE_STEP)
        {
            return;
        }
        self.paces.push_back((self.ticks, heard));
        if self.paces.len() > PACE_SAMPLES {
            self.paces.pop_front();
        }
    }

    fn partner_slow(&self) -> bool {
        let (Some(&(from, _)), Some(&(to, _))) = (self.paces.front(), self.paces.back()) else {
            return false;
        };
        if to - from < PACE_STEP * (PACE_SAMPLES as u32 - 1) {
            return false;
        }
        let count = self.paces.len() as f64;
        let (sum_at, sum_heard) = self.paces.iter().fold((0.0, 0.0), |(a, h), &(at, heard)| {
            (a + f64::from(at), h + f64::from(heard))
        });
        let (mean_at, mean_heard) = (sum_at / count, sum_heard / count);
        let (mut across, mut spread) = (0.0, 0.0);
        for &(at, heard) in &self.paces {
            let dx = f64::from(at) - mean_at;
            across += dx * (f64::from(heard) - mean_heard);
            spread += dx * dx;
        }
        spread > 0.0 && across / spread < SLOW_PACE
    }

    fn note_wait(&mut self) {
        self.calm();
        if !self.can_rise() || self.group.is_some() || self.ticks <= WAIT_WINDOW {
            return;
        }
        if self.delay_cap > 0
            && self
                .rise_aim
                .is_some_and(|aim| aim >= self.delay + QUICK_GAP)
        {
            self.waits.clear();
            self.rise_to_aim();
            return;
        }
        let now = self.ticks;
        self.waits.push_back(now);
        while self
            .waits
            .front()
            .is_some_and(|&at| now - at >= WAIT_WINDOW)
        {
            self.waits.pop_front();
        }
        if self.waits.len() > WAIT_LIMIT {
            self.waits.clear();
            if self.host {
                self.step_up();
            } else {
                #[cfg(all(test, feature = "nds"))]
                {
                    self.log.asks += 1;
                }
                self.outgoing.push(PlayMsg::Delay {
                    round: self.round,
                    delay: (self.delay + 1) as u8,
                });
            }
        }
    }

    fn rise_to_aim(&mut self) {
        let aim = self.rise_aim.unwrap_or(self.aim);
        if self.host {
            self.jump_to(aim);
            return;
        }
        #[cfg(all(test, feature = "nds"))]
        {
            self.log.asks += 1;
        }
        self.outgoing.push(PlayMsg::Delay {
            round: self.round,
            delay: aim.min(self.delay_cap) as u8,
        });
    }

    fn can_rise(&self) -> bool {
        self.delay_room > 0 || (self.spell_from.is_some() && self.delay < self.delay_cap)
    }

    fn step_up(&mut self) {
        if !self.can_rise() {
            return;
        }
        let from = self.rise_from().unwrap_or(self.delay);
        self.rose = Some((from, self.ticks));
        if self.delay_cap > 0 && self.delay + 1 >= from + SPELL_DROP {
            self.start_spell(from);
        }
        self.delay += 1;
        self.owed = 0;
        self.delay_room = match self.spell_from {
            Some(_) => self.spell_room(self.delay),
            None => self.delay_room - 1,
        };
        self.quick = false;
        self.calm();
        self.outgoing.push(PlayMsg::Delay {
            round: self.round,
            delay: self.delay as u8,
        });
    }

    fn stalled_long(&mut self, frames: u32) {
        if !self.may_jump() {
            return;
        }
        let to = (self.stall_base + frames)
            .max(self.rise_aim.unwrap_or(0))
            .min(self.delay_cap);
        if self.host {
            self.jump_to(to);
        } else if to > self.delay {
            #[cfg(all(test, feature = "nds"))]
            {
                self.log.asks += 1;
            }
            self.outgoing.push(PlayMsg::Delay {
                round: self.round,
                delay: to as u8,
            });
        }
    }

    fn went_quiet(&self) -> bool {
        let frame_ms = pace::frame_ms(self.timing());
        let quiet = self.ticks.saturating_sub(self.heard_tick + 1);
        f64::from(quiet) * frame_ms > f64::from(self.spread_ms) + frame_ms
    }

    fn may_jump(&self) -> bool {
        self.delay_cap > 0
            && self.group.is_none()
            && self.ticks > WAIT_WINDOW
            && (!self.partner_slow() || self.aim >= self.stall_base + QUICK_GAP)
    }

    fn rise_in_stall(&mut self) {
        if !self.may_jump() {
            return;
        }
        let to = (self.stall_base + self.stall_run).min(self.delay_cap);
        if to <= self.delay {
            return;
        }
        if self.host {
            self.jump_to(to);
            return;
        }
        if !self.can_rise() {
            return;
        }
        #[cfg(all(test, feature = "nds"))]
        {
            self.log.asks += 1;
        }
        let from = self
            .echo
            .map_or(self.stall_base, |(from, _)| from.min(self.stall_base));
        self.echo = Some((from, self.ticks + SPARE_FRAMES));
        self.follow_rise(to);
        self.outgoing.push(PlayMsg::Delay {
            round: self.round,
            delay: to as u8,
        });
    }

    fn follow_lower(&mut self, delay: u32) {
        if delay < self.delay {
            let spell = self.spell_from.is_some();
            if self.spell_from.is_some_and(|from| delay <= from) {
                self.spell_from = None;
            }
            self.delay_room = if spell {
                self.spell_room(delay)
            } else {
                self.delay_room + (self.delay - delay)
            };
            self.delay = delay;
        } else if delay == self.delay {
            self.delay_room = 0;
        }
    }

    fn follow_rise(&mut self, delay: u32) {
        let from = self.rise_from().unwrap_or(self.delay);
        self.rose = Some((from, self.ticks));
        if self.delay_cap > 0
            && (delay > self.delay + self.delay_room || delay >= from + SPELL_DROP)
        {
            self.start_spell(from);
        }
        self.delay_room = match self.spell_from {
            Some(_) => self.spell_room(delay),
            None => self.delay_room.saturating_sub(delay - self.delay),
        };
        self.delay = delay;
        self.owed = 0;
    }

    fn jump_to(&mut self, to: u32) {
        let to = to.min(self.delay_cap);
        if to <= self.delay {
            return;
        }
        self.start_spell(self.rise_from().unwrap_or(self.delay));
        self.delay = to;
        self.owed = 0;
        self.delay_room = self.spell_room(to);
        self.quick = false;
        self.last_wait = self.ticks;
        self.least_spare = u32::MAX;
        self.calm();
        self.outgoing.push(PlayMsg::Delay {
            round: self.round,
            delay: self.delay as u8,
        });
    }

    fn start_spell(&mut self, from: u32) {
        if self.spell_from.is_none() {
            self.spell_from = Some(from.min(self.delay));
            self.spell_top = self.delay + self.delay_room;
        }
    }

    fn spell_room(&self, delay: u32) -> u32 {
        match self.spell_from {
            Some(from) if delay > from => self.delay_cap.saturating_sub(delay),
            _ => self.spell_top.saturating_sub(delay),
        }
    }

    fn spare_window(&self) -> u32 {
        let spread = f64::from(self.spread_ms) / pace::frame_ms(self.timing());
        SPARE_FRAMES + SPREAD_TICKS * spread.ceil() as u32
    }

    fn rise_from(&self) -> Option<u32> {
        self.rose
            .filter(|&(_, at)| self.ticks.saturating_sub(at) < WAIT_WINDOW)
            .map(|(from, _)| from)
    }

    fn ease_spell(&mut self) -> bool {
        let Some(from) = self.spell_from else {
            return false;
        };
        if self.delay <= from {
            self.spell_from = None;
            return false;
        }
        if self.ticks.saturating_sub(self.last_wait) < self.spare_window() {
            return false;
        }
        if self.least_spare == u32::MAX || self.least_spare < EASE_SLACK + SPELL_DROP {
            self.last_wait = self.ticks;
            self.least_spare = u32::MAX;
            return false;
        }
        let mut to = self
            .delay
            .saturating_sub(self.least_spare - EASE_SLACK)
            .max(from);
        if to <= from + 1 {
            to = from;
        }
        self.delay = to;
        self.delay_room = self.spell_room(to);
        self.last_wait = self.ticks;
        self.least_spare = u32::MAX;
        if to <= from {
            self.spell_from = None;
        }
        self.calm();
        self.outgoing.push(PlayMsg::Delay {
            round: self.round,
            delay: self.delay as u8,
        });
        true
    }

    fn calm(&mut self) {
        self.calm_since = self.ticks;
        self.least_slack = u32::MAX;
    }

    fn aim_at(&mut self, aim: Option<u32>) {
        let aim = aim.unwrap_or(self.least_delay);
        if aim != self.aim {
            self.aim = aim;
            self.quick = aim + QUICK_GAP <= self.delay;
        }
    }

    fn floor(&self) -> u32 {
        if self.aim + QUICK_GAP <= self.least_delay {
            self.aim
        } else {
            self.least_delay
        }
    }

    fn ease_delay(&mut self) {
        let calm = if self.quick && self.delay > self.aim && self.spell_from.is_none() {
            QUICK_CALM_FRAMES
        } else {
            CALM_FRAMES
        };
        if !self.host || self.group.is_some() || self.ease_spell() {
            return;
        }
        if self.delay <= self.floor() || self.ticks.saturating_sub(self.calm_since) < calm {
            return;
        }
        if self.least_slack < EASE_SLACK && !self.partner_slow() {
            self.calm();
            return;
        }
        self.delay -= 1;
        self.delay_room = match self.spell_from {
            Some(_) => self.spell_room(self.delay),
            None => self.delay_room + 1,
        };
        self.calm();
        self.outgoing.push(PlayMsg::Delay {
            round: self.round,
            delay: self.delay as u8,
        });
    }

    pub fn set_delay_room(&mut self, room: u32) {
        self.delay_room = room;
    }

    pub fn set_delay_cap(&mut self, cap: u32) {
        self.delay_cap = cap;
    }

    #[cfg(all(test, feature = "nds"))]
    pub fn delay_room(&self) -> u32 {
        self.delay_room
    }

    #[cfg(all(test, feature = "nds"))]
    pub fn wait_past_the_limit(&mut self) {
        self.ticks = self.ticks.max(WAIT_WINDOW + 1);
        for _ in 0..=WAIT_LIMIT {
            self.note_wait();
        }
    }

    pub fn delay(&self) -> u32 {
        self.delay
    }

    fn follow(&mut self, now: Instant, split: &mut Split<'_>) -> bool {
        let behind = self
            .session
            .remote_frame()
            .saturating_sub(self.session.frame());
        let runs = if behind > WATCH_BEHIND { 2 } else { 1 };
        let mut ran = false;
        for _ in 0..runs {
            if self.session.advance(split) != Advance::Ran {
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
                    cable: None,
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
            let Some(g) = &mut self.group else {
                return;
            };
            let last = g
                .answered
                .iter()
                .position(|(t, u, _)| *t == to && *u == unit);
            if last.is_some_and(|i| frame < g.answered[i].2.saturating_add(RESYNC_WINDOW)) {
                continue;
            }
            let Some(data) = self.session.checkpoint(frame, Player::ALL[unit]) else {
                continue;
            };
            let cable = self.session.checkpoint_cable(frame);
            match last {
                Some(i) => g.answered[i].2 = frame,
                None => g.answered.push((to, unit, frame)),
            }
            g.addressed.push((
                Some(to),
                PlayMsg::Resync {
                    round: self.round,
                    frame,
                    unit: unit as u8,
                    data,
                    cable,
                },
            ));
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
                cable,
            } if round == self.round && !self.authority && from == g.authority => {
                let player = Player::ALL[usize::from(unit) % GROUP_UNITS];
                if self
                    .session
                    .resync(frame, player, &data, cable.as_deref())
                    .is_ok()
                {
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
                ..
            } if round == self.round && !self.authority => {
                let player = if unit == 0 { Player::One } else { Player::Two };
                if self.session.resync(frame, player, &data, None).is_ok() {
                    self.resyncs += 1;
                    #[cfg(test)]
                    self.log.resynced.push(frame);
                }
            }
            PlayMsg::Delay { round, delay } if round == self.round => {
                let delay = u32::from(delay);
                if self.host {
                    if delay > self.delay {
                        self.calm();
                    }
                    if self.delay_cap > 0 && delay >= self.delay + LONG_STALL {
                        self.jump_to(delay);
                    } else if delay > self.delay && !self.can_rise() {
                        self.outgoing.push(PlayMsg::Delay {
                            round: self.round,
                            delay: self.delay as u8,
                        });
                    } else if delay > self.delay {
                        self.step_up();
                    }
                } else if delay > self.delay {
                    self.held_delay = None;
                    self.follow_rise(delay);
                } else if self
                    .echo
                    .is_some_and(|(from, until)| delay > from && self.ticks < until)
                {
                    self.held_delay = (delay < self.delay).then_some(delay);
                    if delay == self.delay {
                        self.delay_room = 0;
                    }
                } else {
                    self.held_delay = None;
                    self.follow_lower(delay);
                }
            }
            _ => {}
        }
    }
}

fn pad_of(input: Input) -> Pad {
    let pace = match input.pace {
        Some(CablePace::Exact) => Pad::PACED | Pad::EXACT,
        Some(CablePace::Windows) => Pad::PACED,
        None => 0,
    };
    Pad {
        buttons: (input.buttons.bits() & Pad::BUTTONS) | pace,
        cable: input.cable.map(|event| event == CableEvent::Plug),
        touch: input.touch.map(|touch| [touch.x, touch.y]),
    }
}

fn input_of(pad: Pad) -> Input {
    let input = Input::new(Keys::from_bits_retain(pad.buttons & Pad::BUTTONS))
        .with_touch(pad.touch.map(|[x, y]| Touch { x, y }));
    let input = match pad.cable {
        Some(true) => input.with_cable(CableEvent::Plug),
        Some(false) => input.with_cable(CableEvent::Pull),
        None => input,
    };
    match (pad.buttons & Pad::PACED != 0, pad.buttons & Pad::EXACT != 0) {
        (true, true) => input.with_pace(CablePace::Exact),
        (true, false) => input.with_pace(CablePace::Windows),
        (false, _) => input,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_of_one_game_share_its_rom() {
        let game = |data: &[u8], kind| Some((Arc::from(data), kind, Origin::Boot(None)));
        let roms = roms_of(&[
            game(&[1; 64], ConsoleKind::Gba),
            game(&[1; 64], ConsoleKind::Gba),
        ]);
        let [Some(one), Some(two)] = &roms else {
            panic!("both players have a game");
        };
        assert!(Arc::ptr_eq(&one.data, &two.data));
        let roms = roms_of(&[
            game(&[1; 64], ConsoleKind::Gb),
            None,
            game(&[2; 64], ConsoleKind::Gb),
        ]);
        assert!(roms[1].is_none());
        assert_ne!(
            roms[0].as_ref().unwrap().data,
            roms[2].as_ref().unwrap().data
        );
    }

    #[test]
    fn pads_carry_buttons_cable_events_and_paces_both_ways() {
        for input in [
            Input::default(),
            Input::new(Keys::A | Keys::LEFT),
            Input::new(Keys::all()).with_cable(CableEvent::Plug),
            Input::new(Keys::L | Keys::R),
            Input::new(Keys::DOWN).with_cable(CableEvent::Pull),
            Input::new(Keys::all()).with_pace(CablePace::Exact),
            Input::new(Keys::B)
                .with_cable(CableEvent::Plug)
                .with_pace(CablePace::Windows),
            Input::new(Keys::Y).with_touch(Some(Touch { x: 255, y: 191 })),
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
                6, 3, 210, 9, 5, 0, 0, 33, 0, 255, 63, 1, 1, 136, 1, 1, 0, 128, 6, 0
            ]
        );
    }
}
