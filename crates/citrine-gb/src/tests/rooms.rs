use crate::console::{Console, ConsoleKind, Keys, Rollback, Timing, VideoSpec};
use crate::error::{GbError, GbResult};
use crate::netplay::pair::{self, Cable};
use crate::netplay::state;
use crate::netplay::{
    CableEvent, Check, Input, LinkPort, MAX_ROLLBACK_FRAMES, MemberError, NEVER, Player, Session,
    SessionConfig, Sync,
};
use crate::persistence::battery_save::BatterySave;
use crate::persistence::state::StateBody;
#[cfg(feature = "rom-id")]
use crate::rom::id::RomId;
use std::marker::PhantomData;

const LOCKSTEP: Sync = Sync::lockstep(2);
const ROLLBACK: Sync = Sync::new(30, 0);
const CHECK_EVERY: u32 = 10;

fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Unit {
    state: u64,
    keys: Keys,
    frames: u32,
    rank: Option<usize>,
    steps: u32,
    outbox: Vec<u64>,
    heard: u32,
}

impl Unit {
    fn new(seed: u64) -> Self {
        Self {
            state: mix(seed),
            ..Self::default()
        }
    }

    fn tick(&mut self) {
        self.state = mix(self.state ^ u64::from(self.keys.bits()));
    }
}

impl Console for Unit {
    fn kind(&self) -> ConsoleKind {
        ConsoleKind::Gb
    }

    fn video(&self) -> VideoSpec {
        VideoSpec {
            width: 0,
            height: 0,
        }
    }

    fn timing(&self) -> Timing {
        Timing {
            clock_hz: 4_194_304,
            cycles_per_frame: 70_224,
        }
    }

    fn run_frame(&mut self) {
        self.tick();
        self.frames += 1;
    }

    fn frame(&self) -> &[u8] {
        &[]
    }

    fn take_audio(&mut self, _out: &mut Vec<f32>) {}

    fn sample_rate(&self) -> u32 {
        44_100
    }

    fn set_sample_rate(&mut self, _rate: u32) {}

    fn keys(&self) -> Keys {
        self.keys
    }

    fn set_keys(&mut self, keys: Keys) {
        self.keys = keys;
    }

    fn soft_reset(&mut self) {
        unimplemented!()
    }

    fn audio_channels(&self) -> &'static [&'static str] {
        &[]
    }

    fn set_channel_enabled(&mut self, _index: usize, _enabled: bool) {}

    fn rom_bytes(&self) -> Vec<u8> {
        Vec::new()
    }

    fn title(&self) -> String {
        String::new()
    }

    fn has_battery(&self) -> bool {
        false
    }

    fn has_clock(&self) -> bool {
        false
    }

    fn battery(&mut self, _force: bool, _now: u64) -> Option<BatterySave> {
        None
    }

    fn battery_from_file(&self, _data: &[u8]) -> BatterySave {
        unimplemented!()
    }

    fn put_battery(&mut self, _save: BatterySave, _now: u64) {}

    fn save_state(&mut self, _now: u64) -> GbResult<Vec<u8>> {
        unimplemented!()
    }

    fn state_body(&mut self, _now: u64) -> GbResult<StateBody> {
        unimplemented!()
    }

    #[cfg(feature = "rom-id")]
    fn rom_id(&self) -> RomId {
        unimplemented!()
    }
}

impl Rollback for Unit {
    fn clone_state(&self) -> Self {
        self.clone()
    }

    fn copy_state_from(&mut self, other: &Self) {
        self.clone_from(other);
    }

    fn state_hash(&self, _scratch: &mut Self) -> u64 {
        let bytes = rmp_serde::to_vec(self).unwrap();
        bytes
            .iter()
            .fold(0, |hash, &byte| mix(hash ^ u64::from(byte)))
    }

    fn encode_state(&self) -> GbResult<Vec<u8>> {
        rmp_serde::to_vec(self).map_err(|error| GbError::IO(std::io::Error::other(error)))
    }

    fn decode_state(&self, data: &[u8]) -> GbResult<Self> {
        rmp_serde::from_slice(data).map_err(|error| GbError::IO(std::io::Error::other(error)))
    }

    fn set_blind(&mut self, _blind: bool) {}

    fn set_muted(&mut self, _muted: bool) {}
}

impl LinkPort for Unit {
    type Message = u64;

    fn plug_link(&mut self) {
        self.plug_link_as(0, 2);
    }

    fn plug_link_as(&mut self, unit: usize, _units: usize) {
        self.rank = Some(unit);
    }

    fn unplug_link(&mut self) {
        self.rank = None;
    }

    fn link_plugged(&self) -> bool {
        self.rank.is_some()
    }

    fn link_waiting(&self) -> bool {
        false
    }

    fn take_link_messages(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.outbox)
    }

    fn receive_link_message(&mut self, message: u64) {
        self.state = mix(self.state ^ message);
        self.heard += 1;
    }

    fn is_transfer(_message: &u64) -> bool {
        true
    }

    fn begin_linked_frame(&mut self) -> u64 {
        self.steps = 0;
        0
    }

    fn step_linked(&mut self) -> u64 {
        self.tick();
        self.steps += 1;
        if self.steps.is_multiple_of(3) {
            let rank = self.rank.unwrap_or_default() as u64;
            self.outbox.push(mix(self.state ^ rank));
        }
        1_000 + self.state % 512
    }

    fn linked_frame_done(&self) -> bool {
        self.steps >= 9
    }

    fn end_linked_frame(&mut self) {
        self.frames += 1;
    }
}

fn pad(unit: usize, frame: u32) -> Input {
    let held = mix(((unit as u64) << 32) | u64::from(frame / 7));
    let input = Input::new(Keys::from_bits_truncate(held as u16));
    if unit == 0 && frame == 0 {
        input.with_cable(CableEvent::Plug)
    } else {
        input
    }
}

fn units<const N: usize>() -> [Unit; N] {
    std::array::from_fn(|i| Unit::new(i as u64 + 1))
}

fn joiner() -> Unit {
    Unit::new(99)
}

trait Units: Rollback + LinkPort + Sized {
    fn units<const N: usize>() -> [Self; N];
    fn joiner() -> Self;
}

impl Units for Unit {
    fn units<const N: usize>() -> [Self; N] {
        units()
    }

    fn joiner() -> Self {
        joiner()
    }
}

struct Straight<const N: usize, C = Unit> {
    power_on: [u32; N],
    leave_at: [u32; N],
    join: Option<(u32, usize)>,
    units: PhantomData<C>,
}

impl<const N: usize, C: Units> Straight<N, C> {
    fn new() -> Self {
        Self {
            power_on: [0; N],
            leave_at: [NEVER; N],
            join: None,
            units: PhantomData,
        }
    }

    fn hashes(&self, frames: u32) -> Vec<[u64; N]> {
        let mut consoles = C::units::<N>();
        let mut cable = Cable::<C::Message, N>::default();
        let mut scratch = C::joiner();
        let mut hashes = Vec::new();
        for frame in 0..frames {
            if let Some((at, unit)) = self.join
                && at == frame
            {
                consoles[unit] = C::joiner();
                cable.reseat();
            }
            hashes.push(state::hash_consoles(&mut scratch, &consoles, &cable));
            let on: [bool; N] =
                std::array::from_fn(|i| frame >= self.power_on[i] && frame < self.leave_at[i]);
            let inputs = std::array::from_fn(|i| {
                if frame < self.power_on[i] || frame >= self.leave_at[i] {
                    Input::default()
                } else {
                    pad(i, frame)
                }
            });
            pair::run_frame(&mut consoles, &mut cable, &inputs, on);
        }
        hashes
    }
}

enum Payload<const N: usize> {
    Pad(u32, Input),
    Check(Check<N>),
}

struct Packet<const N: usize> {
    due: u32,
    from: usize,
    to: usize,
    payload: Payload<N>,
}

struct Room<const N: usize, C: Rollback + LinkPort = Unit> {
    sessions: [Option<Session<C, N>>; N],
    sent: [u32; N],
    silent: [bool; N],
    wire: Vec<Packet<N>>,
    checks: Vec<Check<N>>,
    tick: u32,
    rng: u64,
    lag: u32,
    jitter: u32,
    rollbacks: u32,
    deepest: u32,
    furthest: u32,
    waited: u32,
}

impl<const N: usize> Room<N> {
    fn new(sync: Sync, present: [bool; N], lag: u32, jitter: u32) -> Self {
        Self::with_units(sync, present, lag, jitter)
    }
}

impl<const N: usize, C: Units> Room<N, C> {
    fn with_units(sync: Sync, present: [bool; N], lag: u32, jitter: u32) -> Self {
        let sessions = std::array::from_fn(|i| {
            present[i].then(|| {
                let config = SessionConfig {
                    check_interval: CHECK_EVERY,
                    sync,
                    ..SessionConfig::new(Player::ALL[i], 7, 0)
                };
                let mut session = Session::with_consoles(config, C::units::<N>(), present);
                session.watch_remote(true);
                session
            })
        });
        Self {
            sessions,
            sent: [0; N],
            silent: [false; N],
            wire: Vec::new(),
            checks: Vec::new(),
            tick: 0,
            rng: 0x5EED,
            lag,
            jitter,
            rollbacks: 0,
            deepest: 0,
            furthest: 0,
            waited: 0,
        }
    }

    fn session(&mut self, player: Player) -> &mut Session<C, N> {
        self.sessions[player.index()].as_mut().unwrap()
    }

    fn send(&mut self, from: usize, payload: impl Fn() -> Payload<N>) {
        for to in (0..N).filter(|&to| to != from) {
            self.rng = mix(self.rng);
            let due = self.tick + self.lag + (self.rng % u64::from(self.jitter + 1)) as u32;
            self.wire.push(Packet {
                due,
                from,
                to,
                payload: payload(),
            });
        }
    }

    fn step(&mut self) {
        self.tick += 1;
        let tick = self.tick;
        let (due, wire) = std::mem::take(&mut self.wire)
            .into_iter()
            .partition::<Vec<_>, _>(|packet| packet.due <= tick);
        self.wire = wire;
        for packet in due {
            let Some(session) = self.sessions[packet.to].as_mut() else {
                continue;
            };
            match packet.payload {
                Payload::Pad(frame, input) => {
                    session.add_player_input(Player::ALL[packet.from], frame, input)
                }
                Payload::Check(check) => session.add_remote_check(check),
            }
        }
        for unit in 0..N {
            if self.silent[unit] {
                continue;
            }
            let Some(session) = self.sessions[unit].as_mut() else {
                continue;
            };
            let ahead = session.frame() + session.sync().delay;
            let mut pads = Vec::new();
            while self.sent[unit] <= ahead {
                let frame = self.sent[unit];
                session.add_local_input(frame, pad(unit, frame)).unwrap();
                pads.push(frame);
                self.sent[unit] += 1;
            }
            if session.advance() != crate::netplay::Advance::Ran {
                self.waited += 1;
            }
            self.rollbacks += session.last_rollback();
            self.deepest = self.deepest.max(session.last_rollback());
            self.furthest = self.furthest.max(session.frames_ahead());
            assert_eq!(session.desync(), None, "unit {unit} at tick {tick}");
            let mut checks = Vec::new();
            while let Some(check) = session.take_check() {
                checks.push(check);
            }
            for frame in pads {
                self.send(unit, || Payload::Pad(frame, pad(unit, frame)));
            }
            for check in checks {
                self.checks.push(check);
                self.send(unit, || Payload::Check(check));
            }
        }
    }

    fn run_until(&mut self, frame: u32) {
        let limit = self.tick + frame * 4 + 200;
        while (0..N).any(|i| {
            !self.silent[i] && self.sessions[i].as_ref().is_some_and(|s| s.frame() < frame)
        }) {
            assert!(self.tick < limit, "stuck at tick {}", self.tick);
            self.step();
        }
    }

    fn frames(&self) -> Vec<u32> {
        (0..N)
            .filter(|&i| !self.silent[i])
            .filter_map(|i| self.sessions[i].as_ref().map(Session::frame))
            .collect()
    }

    fn assert_matches(&self, straight: &Straight<N, C>, frames: u32) {
        let reference = straight.hashes(frames);
        let mut compared = 0;
        for check in self.checks.iter().filter(|c| c.frame < frames) {
            assert_eq!(
                check.hashes, reference[check.frame as usize],
                "check of frame {}",
                check.frame
            );
            compared += 1;
        }
        assert!(
            compared >= (frames / CHECK_EVERY) as usize,
            "{compared} checks"
        );
    }
}

#[test]
fn three_units_in_lockstep_match_a_straight_run() {
    let mut room = Room::<3>::new(LOCKSTEP, [true; 3], 1, 2);
    room.run_until(300);
    assert_eq!(room.rollbacks, 0);
    room.assert_matches(&Straight::new(), 300);
    let session = room.session(Player::Two);
    assert!(session.cable_plugged());
    assert!(session.console(Player::Three).heard > 0);
}

#[test]
fn four_units_with_rollback_match_a_straight_run() {
    let mut room = Room::<4>::new(ROLLBACK, [true; 4], 3, 4);
    room.run_until(300);
    assert!(room.rollbacks > 0);
    room.assert_matches(&Straight::new(), 300);
    let session = room.session(Player::One);
    assert!((0..4).all(|i| session.console(Player::ALL[i]).heard > 0));
}

#[test]
fn lockstep_never_runs_a_frame_it_has_not_heard() {
    let mut room = Room::<3>::new(LOCKSTEP, [true; 3], 6, 0);
    for _ in 0..200 {
        room.step();
        for i in 0..3 {
            let session = room.session(Player::ALL[i]);
            assert_eq!(session.frames_ahead(), 0);
        }
    }
    assert_eq!(room.rollbacks, 0);
}

#[test]
fn a_peer_late_past_the_budget_makes_the_others_wait_instead_of_rolling_back_deeper() {
    let budget = Sync::new(4, 0);
    let mut room = Room::<2>::new(budget, [true; 2], 12, 3);
    room.run_until(300);
    assert!(room.rollbacks > 0);
    assert_eq!(room.furthest, 4);
    assert!(room.deepest <= 4, "{}", room.deepest);
    room.assert_matches(&Straight::new(), 300);
}

#[test]
fn a_waiting_session_runs_again_once_the_late_input_arrives() {
    let mut room = Room::<2>::new(Sync::new(4, 0), [true; 2], 12, 0);
    for _ in 0..40 {
        room.step();
    }
    let session = room.session(Player::One);
    assert_eq!(session.frames_ahead(), 4);
    let frame = session.frame();
    session.add_local_input(frame, pad(0, frame)).unwrap();
    assert_eq!(session.advance(), crate::netplay::Advance::AwaitingRemote);
    assert_eq!(session.frame(), frame);
    assert_eq!(session.waiting_for(), Some(Player::Two));
    room.run_until(200);
    assert!(room.session(Player::One).frame() >= 200);
}

#[test]
fn rollback_within_the_budget_gives_the_frames_lockstep_gives() {
    let mut rolled = Room::<2>::new(Sync::new(8, 1), [true; 2], 3, 4);
    rolled.run_until(300);
    let mut stepped = Room::<2>::new(Sync::lockstep(1), [true; 2], 3, 4);
    stepped.run_until(300);
    assert!(rolled.rollbacks > 0);
    assert_eq!(stepped.rollbacks, 0);
    assert!(rolled.deepest <= 8);
    let checks = |room: &Room<2>| {
        let mut checks: Vec<_> = room
            .checks
            .iter()
            .filter(|c| c.frame < 300)
            .copied()
            .collect();
        checks.sort_by_key(|c| c.frame);
        checks.dedup();
        checks
    };
    assert_eq!(checks(&rolled), checks(&stepped));
    rolled.assert_matches(&Straight::new(), 300);
}

fn link_stats(sync: Sync, lag: u32, jitter: u32) -> (u32, u32) {
    let mut room = Room::<2>::new(sync, [true; 2], lag, jitter);
    room.run_until(600);
    room.assert_matches(&Straight::new(), 600);
    (room.waited, room.deepest)
}

#[test]
fn game_boy_links_at_80_and_150_ms_wait_less_with_the_full_window() {
    for (lag, jitter, delay) in [(2, 2, 0), (4, 3, 2)] {
        let full = link_stats(Sync::new(MAX_ROLLBACK_FRAMES, delay), lag, jitter);
        let tight = link_stats(Sync::new(2, delay), lag, jitter);
        assert!(full.0 <= tight.0, "{full:?} {tight:?}");
        assert!(full.1 <= MAX_ROLLBACK_FRAMES);
    }
}

fn drop_mid_session<const N: usize>(sync: Sync, lag: u32, jitter: u32) {
    let mut room = Room::<N>::new(sync, [true; N], lag, jitter);
    room.run_until(120);
    room.silent[2] = true;
    for _ in 0..80 {
        room.step();
    }
    let stalled = room.frames();
    for _ in 0..10 {
        room.step();
    }
    assert_eq!(room.frames(), stalled);
    let at = room.sent[2];
    for i in (0..N).filter(|&i| i != 2) {
        let session = room.session(Player::ALL[i]);
        assert_eq!(session.waiting_for(), Some(Player::Three));
        assert_eq!(session.confirmed_frame(), at);
        session.leave(Player::Three, at).unwrap();
        assert_eq!(
            session.leave(Player::Three, at),
            Err(MemberError::Absent(2))
        );
    }
    room.run_until(400);
    let mut straight = Straight::new();
    straight.leave_at[2] = at;
    room.assert_matches(&straight, 400);
    for i in (0..N).filter(|&i| i != 2) {
        let session = room.session(Player::ALL[i]);
        assert_eq!(session.console(Player::Three).frames, at);
        assert!(session.console(Player::One).heard > 0);
    }
}

#[test]
fn a_dropped_unit_stops_and_the_others_play_on_in_lockstep() {
    drop_mid_session::<3>(LOCKSTEP, 1, 2);
}

#[test]
fn a_dropped_unit_stops_and_the_others_play_on_with_rollback() {
    drop_mid_session::<4>(ROLLBACK, 3, 4);
}

fn late_joiner(sync: Sync) {
    const JOIN: u32 = 120;
    let mut room = Room::<3>::new(sync, [true, true, false], 2, 2);
    room.run_until(60);
    for i in 0..2 {
        room.session(Player::ALL[i])
            .join(Player::Three, JOIN, joiner())
            .unwrap();
    }
    assert_eq!(
        room.session(Player::One)
            .join(Player::Three, JOIN, joiner()),
        Err(MemberError::Present(2))
    );
    room.run_until(JOIN);
    let states = loop {
        let source = room.session(Player::One);
        let states: Option<Vec<Vec<u8>>> = (0..3)
            .map(|i| {
                let player = Player::ALL[i];
                if let Some(state) = source.checkpoint(JOIN, player) {
                    return Some(state);
                }
                let here = source.frame() == JOIN && source.confirmed_frame() == JOIN;
                let console = if i == 2 {
                    joiner()
                } else {
                    source.console(player).clone()
                };
                here.then(|| crate::netplay::encode_state(&console).unwrap())
            })
            .collect();
        if let Some(states) = states {
            break states;
        }
        assert!(room.tick < 1_000, "no state at frame {JOIN}");
        room.step();
    };
    let source = room.session(Player::One);
    let plugged = source.cable_plugged();
    let consoles = std::array::from_fn(|i| Unit::default().decode_state(&states[i]).unwrap());
    let config = SessionConfig {
        check_interval: CHECK_EVERY,
        sync,
        start_frame: JOIN,
        cable_plugged: plugged,
        power_on: [0, 0, JOIN],
        ..SessionConfig::new(Player::Three, 7, 0)
    };
    let mut session = Session::with_consoles(config, consoles, [true; 3]);
    session.watch_remote(true);
    room.sessions[2] = Some(session);
    room.sent[2] = JOIN;
    for unit in 0..2 {
        for frame in JOIN..room.sent[unit] {
            room.session(Player::Three).add_player_input(
                Player::ALL[unit],
                frame,
                pad(unit, frame),
            );
        }
    }
    room.run_until(400);
    let mut straight = Straight::new();
    straight.power_on[2] = JOIN;
    straight.join = Some((JOIN, 2));
    room.assert_matches(&straight, 400);
    for i in 0..3 {
        let session = room.session(Player::ALL[i]);
        let joined = session.console(Player::Three);
        assert!(joined.heard > 0);
        assert!(joined.frames >= 400 - JOIN - 40);
    }
}

#[test]
fn a_late_joiner_links_in_and_stays_in_step_in_lockstep() {
    late_joiner(LOCKSTEP);
}

#[test]
fn a_late_joiner_links_in_and_stays_in_step_with_rollback() {
    late_joiner(ROLLBACK);
}

#[test]
fn a_member_change_in_a_confirmed_frame_is_refused() {
    let mut room = Room::<3>::new(LOCKSTEP, [true; 3], 1, 0);
    room.run_until(50);
    let session = room.session(Player::One);
    assert_eq!(session.leave(Player::Two, 10), Err(MemberError::Past(10)));
    assert_eq!(session.leave(Player::One, 60), Err(MemberError::Local(0)));
}

#[cfg(feature = "gba")]
mod gba;
