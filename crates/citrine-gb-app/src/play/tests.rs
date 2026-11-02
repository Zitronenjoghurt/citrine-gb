use super::*;
use crate::online::address::ServerAddress;
use crate::online::transport::{self, Connector, Transport, TransportEvent};
use crate::online::{Phase, Trust};
use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::netplay::{
    Advance, ConsoleSetup, Input, Player, Session, SessionConfig, encode_state,
};
use citrine_gb_net::{ErrorCode, FriendState, InviteCode, Mode, NONCE_BYTES, Sealed};
use citrine_gb_server::{Config, Running, start};
use std::collections::HashMap;

pub(crate) const SNAKE: &[u8] = include_bytes!("../../homebrew/snake.gb");
const PASSWORD: &str = "lemon tree";
const FRAME: Duration = Duration::from_nanos(16_742_706);
const SETTLE: Duration = Duration::from_secs(60);
const PAD: [JoypadState; 8] = [
    JoypadState::empty(),
    JoypadState::LEFT,
    JoypadState::RIGHT,
    JoypadState::UP,
    JoypadState::DOWN,
    JoypadState::A,
    JoypadState::B,
    JoypadState::A.union(JoypadState::RIGHT),
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

struct Lag {
    inner: Box<dyn Transport>,
    meter: Meter,
    half: Duration,
    jitter_ms: u64,
    rng: Rng,
    out: VecDeque<(Instant, Vec<u8>)>,
    inbound: VecDeque<(Instant, TransportEvent)>,
    last_out: Instant,
    last_in: Instant,
}

impl Lag {
    fn due(&mut self, last: Instant, now: Instant) -> Instant {
        let jitter = Duration::from_millis(self.rng.below(self.jitter_ms + 1));
        (now + self.half + jitter).max(last)
    }
}

impl Transport for Lag {
    fn send(&mut self, bytes: Vec<u8>) {
        let now = Instant::now();
        if let Ok(ClientMsg::Play { sealed, .. } | ClientMsg::Game { sealed, .. }) =
            ClientMsg::decode(&bytes)
        {
            self.meter.borrow_mut().push((now, sealed.ciphertext.len()));
        }
        let due = self.due(self.last_out, now);
        self.last_out = due;
        self.out.push_back((due, bytes));
    }

    fn poll(&mut self) -> Option<TransportEvent> {
        let now = Instant::now();
        while self.out.front().is_some_and(|(due, _)| *due <= now) {
            if let Some((_, bytes)) = self.out.pop_front() {
                self.inner.send(bytes);
            }
        }
        for _ in 0..256 {
            let Some(event) = self.inner.poll() else {
                break;
            };
            let due = self.due(self.last_in, now);
            self.last_in = due;
            self.inbound.push_back((due, event));
        }
        match self.inbound.front() {
            Some((due, _)) if *due <= now => self.inbound.pop_front().map(|(_, event)| event),
            _ => None,
        }
    }
}

fn lagged(one_way_ms: u64, jitter_ms: u64, seed: u64, meter: Meter) -> Connector {
    let mut websocket = transport::websocket();
    let mut opened = 0;
    Box::new(move |url| {
        opened += 1;
        let now = Instant::now();
        Box::new(Lag {
            inner: websocket(url),
            meter: meter.clone(),
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

struct Relay {
    running: Option<Running>,
    runtime: tokio::runtime::Runtime,
    address: ServerAddress,
    _dir: tempfile::TempDir,
}

impl Relay {
    fn start(sharing: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vars: HashMap<&str, String> = HashMap::from([
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
            ("CITRINE_RELAY_PASSWORD", PASSWORD.into()),
            ("CITRINE_WEB", "false".into()),
            (
                "CITRINE_GAME_SHARING",
                if sharing { "on" } else { "off" }.into(),
            ),
        ]);
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
    fn find(&self, _hash: &GameHash) -> Option<Vec<u8>> {
        None
    }
}

pub(crate) struct Side {
    pub(crate) online: Online,
    meter: Meter,
    pub(crate) play: Play,
    rng: Rng,
    held: JoypadState,
    held_for: u64,
    pressed: u32,
    cable_gb: Option<GameBoy>,
    cable_frames: u32,
    received: bool,
    presses_start: bool,
    own: Option<GameBoy>,
    gave: Option<Vec<u8>>,
}

impl Side {
    fn new(lag: (u64, u64), seed: u64, shelf: Box<dyn Shelf>) -> Self {
        let meter = Meter::default();
        Self {
            online: Online::new(lagged(lag.0, lag.1, seed, meter.clone())),
            meter,
            play: Play::new(shelf),
            rng: Rng(seed),
            held: JoypadState::empty(),
            held_for: 0,
            pressed: 0,
            cable_gb: None,
            cable_frames: 0,
            received: false,
            presses_start: false,
            own: None,
            gave: None,
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

    fn running(&mut self, file: GameFile) -> GameFile {
        let mut gb = GameBoy::new_empty(file.model);
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

    fn session_mut(&mut self) -> &mut PlaySession {
        self.play.session_mut().unwrap()
    }

    fn buttons(&mut self) -> JoypadState {
        self.pressed += 1;
        if self.presses_start && self.pressed % 90 < 5 {
            return JoypadState::START;
        }
        if self.held_for == 0 {
            self.held = PAD[self.rng.below(PAD.len() as u64) as usize];
            self.held_for = 1 + self.rng.below(24);
        }
        self.held_for -= 1;
        self.held
    }

    fn step(&mut self, now: Instant) {
        self.online.update(now);
        self.play.update(&mut self.online, now);
        if let Some(play) = self.play.session_mut() {
            if let Some(back) = play.take_returned() {
                self.own = Some(back.console);
            }
            if play.wants_state()
                && let Some(gb) = &self.own
            {
                let state = encode_state(gb).unwrap();
                self.gave = Some(state.clone());
                play.give_state(state);
            }
        }
        let buttons = self.buttons();
        if let Some(play) = self.play.session_mut()
            && play.stage() == Stage::Playing
        {
            match play.mode() {
                Some(Mode::Rollback) => {
                    play.run_frame(buttons, now);
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
        if matches!(self.stage(), Some(Stage::Receiving { .. })) {
            self.received = true;
        }
        self.play.flush(&mut self.online);
    }
}

pub(crate) struct Pair {
    pub(crate) host: Side,
    pub(crate) guest: Side,
    relay: Relay,
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
        let mut pair = Self {
            host: Side::new((one_way_ms, jitter_ms), 11, host_shelf),
            guest: Side::new((one_way_ms, jitter_ms), 29, guest_shelf),
            relay,
        };
        pair.guest.presses_start = true;
        let [host_code, guest_code] = pair.relay.invites(sharing);
        let now = Instant::now();
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
        let now = Instant::now();
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

    fn tick(&mut self) {
        let now = Instant::now();
        self.host.step(now);
        self.guest.step(now);
    }

    pub(crate) fn until(&mut self, what: &str, mut done: impl FnMut(&Pair) -> bool) {
        let begun = Instant::now();
        let mut next = begun;
        while !done(self) {
            assert!(begun.elapsed() < SETTLE, "timed out waiting until {what}");
            self.tick();
            next += FRAME;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
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
        let mut next = Instant::now();
        for _ in 0..frames {
            self.host.step(Instant::now());
            next += FRAME;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
        }
    }

    fn run_guest_alone(&mut self, frames: u32) {
        let mut next = Instant::now();
        for _ in 0..frames {
            self.guest.step(Instant::now());
            next += FRAME;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
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

    pub(crate) fn play_frames(&mut self, frames: u32) {
        let target = self.host.frame().max(self.guest.frame()) + frames;
        self.until("both ran the frames", |p| {
            p.host.frame() >= target && p.guest.frame() >= target
        });
    }
}

fn game(rom: &[u8], title: &str) -> GameFile {
    GameFile {
        rom: rom.to_vec(),
        title: title.into(),
        model: GbModel::for_rom(&Rom::new(rom)).unwrap(),
        save: None,
        running: false,
    }
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

fn empty() -> [Box<dyn Shelf>; 2] {
    [Box::new(Nothing), Box::new(Nothing)]
}

fn reference_checks(play: &PlaySession, roms: [&[u8]; 2]) -> HashMap<u32, [u64; 2]> {
    let start = play.start().unwrap();
    let log = play.log().unwrap();
    let roms = roms.map(Rom::new);
    let setups = [0, 1].map(|i| {
        Some(ConsoleSetup {
            rom: &roms[i],
            model: GbModel::for_rom(&roms[i]).unwrap(),
            save: None,
            state: None,
        })
    });
    let config = SessionConfig {
        power_on: start.power_on,
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
    for (frame, (one, two)) in inputs.into_iter().enumerate() {
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
    let host = pair.host.session();
    let guest = pair.guest.session();
    assert_eq!(host.mode(), Some(Mode::Rollback));
    let slow = one_way_ms * 2 > u64::from(SLOW_ROUND_TRIP_MS);
    assert_eq!(host.delay(), Some(if slow { SLOW_DELAY } else { 0 }));
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
    assert_matches_reference(pair.host.session(), [SNAKE, SNAKE]);
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
    let screen = pair
        .host
        .session()
        .console(true)
        .unwrap()
        .frame()
        .as_slice()
        .to_vec();
    let gb = pair.host.play.close().unwrap().finish().unwrap().console;
    assert!(gb.frame().as_slice() == screen.as_slice());
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
    assert!(!pair.guest.play.holds(&hash));
    let custom = custom().rom;
    assert_matches_reference(pair.host.session(), [&custom, &custom]);
}

#[test]
fn missing_games_cross_both_ways_when_the_relay_shares_games() {
    let mut pair = Pair::new(true, 30, 10, empty());
    let session = pair.invite(custom());
    pair.accept(session, MyGame::Own(snake()));
    assert!(pair.guest.received);
    pair.play_frames(120);
    assert_eq!(pair.host.session().mode(), Some(Mode::Rollback));
    assert_eq!(
        pair.host.session().resyncs() + pair.guest.session().resyncs(),
        0
    );
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
        let mut gb = GameBoy::new_empty(file.model);
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
    assert!(!pair.guest.session().waiting_for_friend(Instant::now()));
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
    assert_eq!(back.console.cartridge.rom_bytes(), SNAKE);
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
    assert_eq!(
        switches[0].why_off.as_deref(),
        Some("You don't have My Snake")
    );
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
    assert!(!pair.host.session_mut().hand_rom(SNAKE));
    assert!(pair.host.session_mut().hand_rom(&custom().rom));
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
            let gb = session.console(true).unwrap();
            let was = (gb.frame().as_slice().to_vec(), gb.cpu.pc, gb.cycle_counter);
            let back = side.play.close().unwrap().finish().unwrap().console;
            let now = (
                back.frame().as_slice().to_vec(),
                back.cpu.pc,
                back.cycle_counter,
            );
            assert!(now == was, "{close:?} changed the game");
            assert!(!back.link_plugged());
        }
    }
}

mod real_games;

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
