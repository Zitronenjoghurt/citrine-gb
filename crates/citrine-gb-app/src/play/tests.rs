use super::*;
use crate::online::address::ServerAddress;
use crate::online::transport::{self, Connector, Transport, TransportEvent};
use crate::online::{Phase, Trust};
use citrine_gb::console::{Console, Keys};
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::netplay::{
    Advance, ConsoleSetup, Input, Player, Session, SessionConfig, encode_state,
};
use citrine_gb::rom::Rom;
use citrine_gb_net::{ErrorCode, FriendState, InviteCode, Mode, NONCE_BYTES, Sealed};
use citrine_gb_server::{Config, Running, start};
use std::collections::HashMap;

pub(crate) const SNAKE: &[u8] = include_bytes!("../../homebrew/snake.gb");
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

struct Lag {
    inner: Box<dyn Transport>,
    clock: Now,
    meter: Meter,
    cut: Cut,
    down: Cut,
    closed: bool,
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
        if self.closed {
            return;
        }
        let now = self.clock.get();
        if let Ok(
            ClientMsg::Play { sealed, .. }
            | ClientMsg::Game { sealed, .. }
            | ClientMsg::RoomPlay { sealed, .. }
            | ClientMsg::RoomGame { sealed, .. },
        ) = ClientMsg::decode(&bytes)
        {
            self.meter.borrow_mut().push((now, sealed.ciphertext.len()));
        }
        let due = self.due(self.last_out, now);
        self.last_out = due;
        self.out.push_back((due, bytes));
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

fn lagged(
    one_way_ms: u64,
    jitter_ms: u64,
    seed: u64,
    meter: Meter,
    [cut, down]: [Cut; 2],
    clock: Now,
) -> Connector {
    let mut websocket = transport::websocket();
    let mut opened = 0;
    Box::new(move |url| {
        opened += 1;
        let now = clock.get();
        Box::new(Lag {
            inner: websocket(url),
            clock: clock.clone(),
            meter: meter.clone(),
            cut: cut.clone(),
            down: down.clone(),
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

struct Clock {
    now: Now,
    next: Instant,
}

impl Clock {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            now: Now::new(now.into()),
            next: now,
        }
    }

    fn now(&self) -> Instant {
        self.now.get()
    }

    fn tick(&mut self) -> Instant {
        self.next += FRAME;
        let real = Instant::now();
        if self.next > real {
            std::thread::sleep(self.next - real);
        } else {
            self.next = real;
        }
        self.now.set(self.now.get() + FRAME);
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
    fn find(&self, _hash: &GameHash) -> Option<Vec<u8>> {
        None
    }
}

pub(crate) struct Side {
    pub(crate) online: Online,
    meter: Meter,
    cut: Cut,
    down: Cut,
    pub(crate) play: Play,
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
    gave: Option<Vec<u8>>,
    runs_own: bool,
    start_only: bool,
    shoulders: bool,
    history: Option<DropHistory>,
    handed_back: Option<(OnlineId, bool)>,
}

impl Side {
    fn new(lag: (u64, u64), seed: u64, shelf: Box<dyn Shelf>, clock: &Clock) -> Self {
        let meter = Meter::default();
        let (cut, down) = (Cut::default(), Cut::default());
        let line = [cut.clone(), down.clone()];
        Self {
            online: Online::new(lagged(
                lag.0,
                lag.1,
                seed,
                meter.clone(),
                line,
                clock.now.clone(),
            )),
            meter,
            cut,
            down,
            play: Play::new(shelf),
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
            gave: None,
            runs_own: false,
            start_only: false,
            shoulders: false,
            history: None,
            handed_back: None,
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
            return encode_state(gb).ok();
        }
        #[cfg(feature = "gba")]
        if let Some(gba) = self.own_gba.as_deref() {
            return encode_state(gba).ok();
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
        self.online.update(now);
        self.play.update(&mut self.online, now);
        if let Some(back) = self.play.session_mut().and_then(PlaySession::take_returned) {
            self.handed_back = Some((back.friend, back.paused));
            self.history = back.history;
            self.own = match back.console {
                #[cfg(feature = "gba")]
                Machine::Gba(gba) => {
                    self.own_gba = Some(gba);
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
        let clock = Clock::new();
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

    pub(crate) fn cut(&mut self) {
        self.host.cut.set(true);
        self.guest.cut.set(true);
    }

    fn tick(&mut self) {
        let now = self.clock.tick();
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
            self.clock.waited(begun, what);
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

    fn until_checked(&mut self) {
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
    }
}

fn game(rom: &[u8], title: &str) -> GameFile {
    GameFile {
        rom: rom.to_vec(),
        title: title.into(),
        kind: GbModel::for_rom(&Rom::new(rom)).unwrap().into(),
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
    assert!(!pair.guest.play.holds(&hash));
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
    assert_eq!(back.console.rom_bytes(), SNAKE);
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
            let was = (gb.frame().as_slice().to_vec(), gb.cpu.pc, gb.cycle_counter);
            let back = game_boy(side.play.close().unwrap().finish().unwrap().console).unwrap();
            let now = (
                back.frame().as_slice().to_vec(),
                back.cpu.pc,
                back.cycle_counter,
            );
            match close {
                Close::Leave => assert!(now == was, "{close:?} changed the game"),
                Close::Drop | Close::Timeout => {
                    assert!(now.2 <= was.2, "{close:?} moved the game on")
                }
            }
            assert!(!back.link_plugged());
        }
    }
}

mod real_games;
mod rooms;

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
    GameFile {
        rom,
        title: "Link Words".into(),
        kind: ConsoleKind::Gba,
        save: None,
        running: false,
    }
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
