use crate::ReadMemory;
use crate::gb::joypad::JoypadState;
use crate::gb::{GameBoy, GbModel};
use crate::netplay::pair::{self, Cable};
use crate::netplay::state::{self, Snapshot};
use crate::netplay::{
    Advance, CableEvent, Check, ConsoleSetup, Input, MAX_ROLLBACK_FRAMES, Player, Session,
    SessionConfig, new_console,
};
use crate::persistence::rtc_footer::RtcFooter;
use crate::persistence::sram_dump::SramDump;
use crate::rom::Rom;
use std::collections::VecDeque;

const SNAKE: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/snake.gb");
const PETRIS: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/petris.gbc");
const TOBU: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/tobu.gb");
const LIBBET: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/libbet.gb");
const UCITY: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/ucity.gbc");
const POSTIE: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/postie.gbc");
const FRAME_MS: f64 = 70_224.0 * 1000.0 / 4_194_304.0;
const START_TIME: u64 = 1_790_000_000;
const PLAYERS: [Player; 2] = [Player::One, Player::Two];
const IDLE: [Input; 2] = [Input::new(JoypadState::empty()); 2];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[derive(Debug, Clone, Copy)]
enum Delay {
    Fixed(f64),
    Jitter(f64, f64),
}

struct Wire<T> {
    delay: Delay,
    rng: Rng,
    queue: VecDeque<(f64, T)>,
    last: f64,
}

impl<T> Wire<T> {
    fn new(delay: Delay, seed: u64) -> Self {
        Self {
            delay,
            rng: Rng(seed),
            queue: VecDeque::new(),
            last: 0.0,
        }
    }

    fn send(&mut self, now: f64, item: T) {
        let delay = match self.delay {
            Delay::Fixed(ms) => ms,
            Delay::Jitter(mean, spread) => mean + (self.rng.unit() * 2.0 - 1.0) * spread,
        };
        self.last = (now + delay).max(self.last);
        self.queue.push_back((self.last, item));
    }

    fn arrived(&mut self, now: f64) -> Option<T> {
        if self.queue.front()?.0 <= now {
            self.queue.pop_front().map(|(_, item)| item)
        } else {
            None
        }
    }
}

fn roms(one: &[u8], two: &[u8]) -> [Rom; 2] {
    [Rom::new(one), Rom::new(two)]
}

fn setups(roms: &[Rom; 2]) -> [Option<ConsoleSetup<'_>>; 2] {
    [0, 1].map(|i| {
        Some(ConsoleSetup {
            rom: &roms[i],
            model: GbModel::for_rom(&roms[i]).unwrap(),
            save: None,
            state: None,
        })
    })
}

fn config(local: Player, check_interval: u32) -> SessionConfig {
    SessionConfig {
        check_interval,
        ..SessionConfig::new(local, 0x0C17_2026, START_TIME)
    }
}

fn session(roms: &[Rom; 2], local: Player, check_interval: u32) -> Session {
    let mut session = Session::new(config(local, check_interval), setups(roms)).unwrap();
    session.watch_remote(true);
    session
}

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

fn random_inputs(seed: u64, frames: usize, longest: u64) -> Vec<Input> {
    let mut rng = Rng(seed);
    let mut inputs = Vec::with_capacity(frames);
    while inputs.len() < frames {
        let held = Input::new(PAD[rng.below(PAD.len() as u64) as usize]);
        let length = 1 + rng.below(longest) as usize;
        inputs.extend(std::iter::repeat_n(held, length));
    }
    inputs.truncate(frames);
    inputs
}

fn press(inputs: &mut [Input], at: usize, buttons: JoypadState, frames: usize) {
    for input in &mut inputs[at..at + frames] {
        input.buttons = buttons;
    }
}

fn cable(inputs: &mut [Input], at: usize, event: CableEvent) {
    inputs[at].cable = Some(event);
}

struct Online {
    sessions: [Session; 2],
    checks: [Vec<Check>; 2],
    deepest: u32,
    rollbacks: u32,
    resyncs: u32,
    resynced: Vec<u32>,
    advance_ms: Vec<f64>,
}

fn play_online(
    new_session: impl Fn(Player) -> Session,
    inputs: &[Vec<Input>; 2],
    delay: Delay,
    mut corrupt: impl FnMut(u32, &mut Session),
) -> Online {
    let frames = inputs[0].len() as u32;
    let mut online = Online {
        sessions: PLAYERS.map(new_session),
        checks: [Vec::new(), Vec::new()],
        deepest: 0,
        rollbacks: 0,
        resyncs: 0,
        resynced: Vec::new(),
        advance_ms: Vec::new(),
    };
    let mut wires = [1, 2].map(|seed| Wire::new(delay, seed));
    let mut check_wires = [3, 4].map(|seed| Wire::<Check>::new(delay, seed));
    let mut resync_wire = Wire::<(u32, Player, Vec<u8>)>::new(delay, 5);
    let mut answered = None;
    let mut sent = [0u32; 2];
    let limit = frames * 4 + 400;
    for tick in 0..limit {
        let now = tick as f64 * FRAME_MS;
        for side in 0..2 {
            let session = &mut online.sessions[side];
            while let Some((frame, input)) = wires[1 - side].arrived(now) {
                session.add_remote_input(frame, input);
            }
            while let Some(check) = check_wires[1 - side].arrived(now) {
                session.add_remote_check(check);
            }
            if side == 1 {
                while let Some((frame, player, data)) = resync_wire.arrived(now) {
                    session.resync(frame, player, &data).unwrap();
                    online.resyncs += 1;
                    online.resynced.push(frame);
                }
            }
            let frame = session.frame();
            if frame == sent[side] && frame < frames {
                let input = inputs[side][frame as usize];
                session.add_local_input(frame, input).unwrap();
                wires[side].send(now, (frame, input));
                sent[side] += 1;
            }
            corrupt(frame, session);
            let mut advanced = Advance::AwaitingLocal;
            let ms = time_ms(|| advanced = session.advance());
            if side == 0 {
                online.advance_ms.push(ms);
            }
            if advanced == Advance::Ran && session.last_rollback() > 0 {
                online.rollbacks += 1;
            }
            online.deepest = online.deepest.max(session.last_rollback());
            while let Some(check) = session.take_check() {
                online.checks[side].push(check);
                check_wires[side].send(now, check);
            }
            if side == 0
                && let Some(desync) = session.desync()
                && answered != Some(desync.frame)
            {
                answered = Some(desync.frame);
                for player in PLAYERS.into_iter().filter(|&p| desync.differs(p)) {
                    let data = session.checkpoint(desync.frame, player).unwrap();
                    resync_wire.send(now, (desync.frame, player, data));
                }
            }
        }
        let settled = online
            .sessions
            .iter()
            .all(|s| s.frame() == frames && s.confirmed_frame() == frames);
        let quiet = wires.iter().all(|w| w.queue.is_empty())
            && check_wires.iter().all(|w| w.queue.is_empty())
            && resync_wire.queue.is_empty();
        if settled && quiet {
            return online;
        }
    }
    panic!("sessions did not settle within {limit} ticks");
}

fn play_reference(session: Session, inputs: &[Vec<Input>; 2]) -> (Session, Vec<Check>) {
    let mut session = session;
    let mut checks = Vec::new();
    for (frame, (&local, &remote)) in inputs[0].iter().zip(&inputs[1]).enumerate() {
        session.add_remote_input(frame as u32, remote);
        session.add_local_input(frame as u32, local).unwrap();
        assert_eq!(session.advance(), Advance::Ran);
        assert_eq!(session.last_rollback(), 0);
        checks.extend(std::iter::from_fn(|| session.take_check()));
    }
    (session, checks)
}

struct Plain {
    consoles: [GameBoy; 2],
    cable: Cable,
    hashes: Vec<[u64; 2]>,
}

fn play_plain(roms: &[Rom; 2], inputs: &[Vec<Input>; 2], power_on: [u32; 2]) -> Plain {
    let config = config(Player::One, 1);
    let [one, two] = setups(roms);
    let mut consoles = [
        new_console(one.unwrap(), &config, Player::One).unwrap(),
        new_console(two.unwrap(), &config, Player::Two).unwrap(),
    ];
    let mut cable = Cable::default();
    let mut scratch = consoles[0].clone_state();
    let mut hashes = Vec::new();
    for (frame, (&one, &two)) in inputs[0].iter().zip(&inputs[1]).enumerate() {
        hashes.push(state::hash_consoles(&mut scratch, &consoles, &cable));
        let on = power_on.map(|at| frame as u32 >= at);
        pair::run_frame(&mut consoles, &mut cable, &[one, two], on);
    }
    Plain {
        consoles,
        cable,
        hashes,
    }
}

fn by_frame(checks: &[Check]) -> Vec<[u64; 2]> {
    let mut sorted = checks.to_vec();
    sorted.sort_by_key(|c| c.frame);
    sorted.dedup_by_key(|c| c.frame);
    for (frame, check) in sorted.iter().enumerate() {
        assert_eq!(check.frame, frame as u32, "check frames have a gap");
    }
    sorted.iter().map(|c| c.hashes).collect()
}

fn assert_same_consoles(a: &GameBoy, b: &GameBoy, what: &str) {
    assert!(
        state::encode_console(&a.clone_state()).unwrap()
            == state::encode_console(&b.clone_state()).unwrap(),
        "{what}: console state differs"
    );
    assert!(
        a.frame().as_slice() == b.frame().as_slice(),
        "{what}: screen differs"
    );
}

fn snake_inputs(frames: usize) -> [Vec<Input>; 2] {
    let mut one = random_inputs(11, frames, 24);
    let mut two = random_inputs(29, frames, 24);
    for at in (60..frames - 10).step_by(90) {
        press(&mut two, at, JoypadState::START, 5);
    }
    cable(&mut one, 20, CableEvent::Plug);
    cable(&mut two, frames / 2, CableEvent::Pull);
    cable(&mut one, frames / 2 + 40, CableEvent::Plug);
    [one, two]
}

#[test]
fn delayed_sessions_match_a_run_without_delay_every_frame() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = snake_inputs(540);
    let (_, reference) = play_reference(session(&roms, Player::One, 1), &inputs);
    let reference = by_frame(&reference);
    assert_eq!(reference.len(), inputs[0].len());

    for delay in [
        Delay::Fixed(0.0),
        Delay::Fixed(50.0),
        Delay::Fixed(125.0),
        Delay::Fixed(250.0),
        Delay::Jitter(100.0, 40.0),
    ] {
        let online = play_online(|local| session(&roms, local, 1), &inputs, delay, |_, _| {});
        for side in 0..2 {
            let hashes = by_frame(&online.checks[side]);
            assert_eq!(hashes.len(), reference.len(), "{delay:?}");
            if let Some(frame) = (0..hashes.len()).find(|&f| hashes[f] != reference[f]) {
                panic!("{delay:?}: side {side} differs from the reference at frame {frame}");
            }
            assert!(online.sessions[side].desync().is_none(), "{delay:?}");
        }
        if !matches!(delay, Delay::Fixed(ms) if ms == 0.0) {
            assert!(
                online.rollbacks > 20,
                "{delay:?}: {} rollbacks",
                online.rollbacks
            );
        }
        assert!(online.deepest <= MAX_ROLLBACK_FRAMES, "{delay:?}");
    }
}

#[test]
fn a_cgb_game_stays_in_step_under_jitter() {
    let roms = roms(PETRIS, PETRIS);
    let mut inputs = [random_inputs(5, 360, 12), random_inputs(6, 360, 12)];
    cable(&mut inputs[1], 0, CableEvent::Plug);
    let (_, reference) = play_reference(session(&roms, Player::One, 1), &inputs);
    let online = play_online(
        |local| session(&roms, local, 1),
        &inputs,
        Delay::Jitter(100.0, 40.0),
        |_, _| {},
    );
    for side in 0..2 {
        assert!(
            by_frame(&online.checks[side]) == by_frame(&reference),
            "side {side}"
        );
    }
    assert!(online.rollbacks > 20);
}

fn linked_round(
    roms: &[Rom; 2],
    inputs: &[Vec<Input>; 2],
    power_on: [u32; 2],
    least_messages: u64,
) -> Cable {
    let plain = play_plain(roms, inputs, power_on);
    assert!(
        plain.cable.messages.iter().all(|&m| m >= least_messages),
        "the round sent only {:?} link messages, {:?} of them transfers",
        plain.cable.messages,
        plain.cable.transfers
    );
    let online = play_online(
        |local| {
            let config = SessionConfig {
                power_on,
                ..config(local, 1)
            };
            let mut session = Session::new(config, setups(roms)).unwrap();
            session.watch_remote(true);
            session
        },
        inputs,
        Delay::Jitter(100.0, 40.0),
        |_, _| {},
    );
    assert!(online.rollbacks > 10, "{} rollbacks", online.rollbacks);
    for (side, session) in online.sessions.iter().enumerate() {
        assert!(
            by_frame(&online.checks[side]) == plain.hashes,
            "side {side}"
        );
        assert_eq!(
            session.link_messages(),
            (plain.cable.messages, plain.cable.transfers)
        );
        for player in PLAYERS {
            assert_same_consoles(
                session.console(player),
                &plain.consoles[player.index()],
                &format!("side {side}, {player:?}"),
            );
        }
    }
    plain.cable
}

#[test]
fn a_linked_petris_match_through_sessions_matches_a_local_pair() {
    let frames = 1500;
    let mut inputs = [
        vec![Input::default(); frames],
        vec![Input::default(); frames],
    ];
    cable(&mut inputs[0], 0, CableEvent::Plug);
    press(&mut inputs[0], 450, JoypadState::START, 5);
    press(&mut inputs[1], 450, JoypadState::START, 5);
    press(&mut inputs[0], 600, JoypadState::DOWN, 5);
    press(&mut inputs[0], 640, JoypadState::RIGHT, 5);
    press(&mut inputs[0], 700, JoypadState::START, 5);
    let pad = [
        JoypadState::LEFT,
        JoypadState::RIGHT,
        JoypadState::A,
        JoypadState::DOWN,
        JoypadState::empty(),
    ];
    let mut rng = Rng(9);
    for side in &mut inputs {
        let mut at = 800;
        while at < frames {
            let length = (4 + rng.below(20) as usize).min(frames - at);
            press(side, at, pad[rng.below(pad.len() as u64) as usize], length);
            at += length;
        }
    }
    linked_round(&roms(PETRIS, PETRIS), &inputs, [0; 2], 20);
}

#[test]
fn snake_links_through_sessions_when_one_console_starts_later() {
    let frames = 1200;
    let mut inputs = [
        vec![Input::default(); frames],
        vec![Input::default(); frames],
    ];
    cable(&mut inputs[0], 0, CableEvent::Plug);
    let mut rng = Rng(3);
    for side in &mut inputs {
        let mut at = 300;
        while at < frames {
            let length = (6 + rng.below(30) as usize).min(frames - at);
            press(side, at, PAD[1 + rng.below(4) as usize], length);
            at += length;
        }
    }
    for at in (120..frames - 10).step_by(60) {
        press(&mut inputs[1], at, JoypadState::START, 5);
    }
    let cable = linked_round(&roms(SNAKE, SNAKE), &inputs, [0, 60], 20);
    let [one, two] = cable.transfers;
    assert!(one > 0 && two > 0, "{:?}", cable.transfers);
    assert_eq!(cable.messages, [one + two; 2]);
}

#[test]
fn a_console_handed_back_comes_from_the_last_confirmed_frame() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = snake_inputs(120);
    let mut ahead = session(&roms, Player::One, 1);
    for frame in 0..120 {
        if frame < 90 {
            ahead.add_remote_input(frame, inputs[1][frame as usize]);
        }
        ahead
            .add_local_input(frame, inputs[0][frame as usize])
            .unwrap();
        assert_eq!(ahead.advance(), Advance::Ran);
    }
    assert_eq!(ahead.confirmed_frame(), 90);
    let shared = [0, 1].map(|side| inputs[side][..90].to_vec());
    let (reference, _) = play_reference(session(&roms, Player::One, 1), &shared);
    let handed = ahead.into_confirmed_console(Player::One);
    assert_same_consoles(&handed, reference.console(Player::One), "confirmed frame");
}

fn exchange_program(sc: u8, first: u8, step: u8, count: u16) -> Vec<u8> {
    let [low, high] = count.to_le_bytes();
    let mut code = vec![0xAF, 0xEA, 0x00, 0xDF, 0x21, 0x00, 0xC0];
    code.extend_from_slice(&[0x11, low, high, 0x06, first]);
    let top = code.len();
    code.extend_from_slice(&[0x78, 0xE0, 0x01]);
    if sc == 0x81 {
        code.extend_from_slice(&[0x0E, 0x28, 0x0D, 0x20, 0xFD]);
    }
    code.extend_from_slice(&[0x3E, sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA]);
    code.extend_from_slice(&[0xF0, 0x01, 0x22, 0x78, 0xC6, step, 0x47]);
    code.extend_from_slice(&[0x1B, 0x7A, 0xB3, 0x20]);
    let back = top as i32 - (code.len() as i32 + 1);
    code.push(back as i8 as u8);
    code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0x00, 0xDF]);
    code
}

fn exchanged(gb: &GameBoy, count: u16) -> Vec<u8> {
    (0..count)
        .map(|i| gb.memory.read_naive(0xC000 + i))
        .collect()
}

#[test]
fn a_scripted_trade_exchanges_the_same_bytes_as_a_local_link() {
    let count = 420u16;
    let roms = [
        super::cgb::rom(0x00, &exchange_program(0x81, 0x10, 1, count)),
        super::cgb::rom(0x00, &exchange_program(0x80, 0x40, 3, count)),
    ];
    let mut inputs = [random_inputs(7, 240, 6), random_inputs(8, 240, 6)];
    cable(&mut inputs[0], 0, CableEvent::Plug);

    let plain = play_plain(&roms, &inputs, [0; 2]);
    let master: Vec<u8> = (0..count).map(|i| 0x10u8.wrapping_add(i as u8)).collect();
    let slave: Vec<u8> = (0..count)
        .map(|i| 0x40u8.wrapping_add((i as u8).wrapping_mul(3)))
        .collect();
    for gb in &plain.consoles {
        assert_eq!(
            gb.memory.read_naive(0xDF00),
            0xAA,
            "the local trade did not finish"
        );
    }
    assert_eq!(exchanged(&plain.consoles[0], count), slave);
    assert_eq!(exchanged(&plain.consoles[1], count), master);

    for delay in [Delay::Fixed(125.0), Delay::Jitter(100.0, 40.0)] {
        let online = play_online(|local| session(&roms, local, 1), &inputs, delay, |_, _| {});
        assert!(online.rollbacks > 10, "{delay:?}");
        for session in &online.sessions {
            assert_eq!(exchanged(session.console(Player::One), count), slave);
            assert_eq!(exchanged(session.console(Player::Two), count), master);
            for player in PLAYERS {
                assert_same_consoles(
                    session.console(player),
                    &plain.consoles[player.index()],
                    &format!("{delay:?}, {player:?}"),
                );
            }
        }
    }
}

#[test]
fn a_desync_is_detected_and_resync_recovers() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = snake_inputs(600);
    let mut corrupted = false;
    let online = play_online(
        |local| session(&roms, local, 30),
        &inputs,
        Delay::Fixed(50.0),
        |frame, session| {
            if frame >= 200 && !corrupted && session.local() == Player::Two {
                corrupted = true;
                session.corrupt(Player::One, |gb| {
                    for addr in 0xC000..0xC100u16 {
                        let value = gb.memory.read_naive(addr);
                        crate::WriteMemory::write_naive(&mut gb.memory, addr, value ^ 0x5A);
                    }
                });
            }
        },
    );
    assert!(corrupted);
    assert!(online.resyncs >= 1, "the difference was never detected");
    let (reference, _) = play_reference(session(&roms, Player::One, 30), &inputs);
    for session in &online.sessions {
        assert!(session.desync().is_none());
        for player in PLAYERS {
            assert_same_consoles(
                session.console(player),
                reference.console(player),
                &format!("{player:?} after the resync"),
            );
        }
    }
}

#[test]
fn a_resync_in_the_middle_of_a_trade_keeps_the_trade() {
    let count = 420u16;
    let roms = [
        super::cgb::rom(0x00, &exchange_program(0x81, 0x10, 1, count)),
        super::cgb::rom(0x00, &exchange_program(0x80, 0x40, 3, count)),
    ];
    let mut inputs = [random_inputs(7, 240, 6), random_inputs(8, 240, 6)];
    cable(&mut inputs[0], 0, CableEvent::Plug);
    let plain = play_plain(&roms, &inputs, [0; 2]);

    let config = config(Player::One, 1);
    let [one, two] = setups(&roms);
    let mut consoles = [
        new_console(one.unwrap(), &config, Player::One).unwrap(),
        new_console(two.unwrap(), &config, Player::Two).unwrap(),
    ];
    let mut cable = Cable::default();
    let mut trading = Vec::new();
    for (&one, &two) in inputs[0].iter().zip(&inputs[1]) {
        trading.push(consoles[0].serial.transfer_active());
        pair::run_frame(&mut consoles, &mut cable, &[one, two], [true; 2]);
    }
    let interval = 4;
    let checks: Vec<usize> = (0..trading.len())
        .step_by(interval as usize)
        .filter(|&frame| trading[frame])
        .collect();
    let during: Vec<usize> = (0..trading.len()).filter(|&f| trading[f]).collect();
    assert!(checks.len() > 2, "the trade runs in frames {during:?}");
    let at = checks[checks.len() / 2] as u32;

    let mut corrupted = false;
    let online = play_online(
        |local| session(&roms, local, interval),
        &inputs,
        Delay::Fixed(50.0),
        |frame, session| {
            if frame >= at && !corrupted && session.local() == Player::Two {
                corrupted = true;
                session.corrupt(Player::One, |gb| {
                    for addr in 0xD000..0xD100u16 {
                        let value = gb.memory.read_naive(addr);
                        crate::WriteMemory::write_naive(&mut gb.memory, addr, value ^ 0x5A);
                    }
                });
            }
        },
    );
    assert!(corrupted);
    assert!(online.resyncs >= 1, "the difference was never detected");
    let first = online.resynced[0];
    assert!(
        trading[first as usize],
        "the first resync, frame {first}, is not mid-trade"
    );
    for session in &online.sessions {
        assert!(session.desync().is_none());
        assert_eq!(
            session.link_messages(),
            (plain.cable.messages, plain.cable.transfers)
        );
        for player in PLAYERS {
            assert_same_consoles(
                session.console(player),
                &plain.consoles[player.index()],
                &format!("{player:?} after the resync"),
            );
        }
    }
}

fn rtc_program() -> Vec<u8> {
    let mut code = vec![0x3E, 0x0A, 0xEA, 0x00, 0x00];
    let top = code.len();
    code.extend_from_slice(&[0xAF, 0xEA, 0x00, 0x60, 0x3C, 0xEA, 0x00, 0x60]);
    for (register, target) in [(0x08u8, 0x00u8), (0x09, 0x01), (0x0A, 0x02), (0x0B, 0x03)] {
        code.extend_from_slice(&[0x3E, register, 0xEA, 0x00, 0x40, 0xFA, 0x00, 0xA0]);
        code.extend_from_slice(&[0xEA, target, 0xC0]);
    }
    code.extend_from_slice(&[
        0x3E, 0x00, 0xEA, 0x00, 0x40, 0xFA, 0x00, 0xA0, 0x3C, 0xEA, 0x00, 0xA0,
    ]);
    let back = top as i32 - (code.len() as i32 + 2);
    code.extend_from_slice(&[0x18, back as i8 as u8]);
    code
}

fn rtc_rom() -> Rom {
    let mut data = super::cgb::rom(0x00, &rtc_program()).data;
    data[0x0147] = 0x10;
    data[0x0149] = 0x02;
    Rom::new(&data)
}

fn rtc_save(ram: u8, timestamp: u64) -> SramDump {
    let mut data = vec![ram; 0x2000];
    let footer = RtcFooter {
        real: crate::gb::cartridge::RtcRegisters::from_bytes([10, 20, 3, 4, 0]),
        latched: crate::gb::cartridge::RtcRegisters::default(),
        timestamp,
    };
    data.extend_from_slice(&footer.encode());
    SramDump::from_slice(&data)
}

fn rtc_session(rom: &Rom, local: Player, start_time: u64) -> Session {
    let saved = START_TIME - 3_600;
    let setups = [0x11, 0x22].map(|ram| {
        Some(ConsoleSetup {
            rom,
            model: GbModel::Dmg,
            save: Some(rtc_save(ram, saved)),
            state: None,
        })
    });
    Session::new(
        SessionConfig {
            check_interval: 1,
            ..SessionConfig::new(local, 7, start_time)
        },
        setups,
    )
    .unwrap()
}

fn clock(gb: &GameBoy) -> [u8; 4] {
    [0, 1, 2, 3].map(|i| gb.memory.read_naive(0xC000 + i))
}

#[test]
fn rtc_carts_run_on_session_time() {
    let rom = rtc_rom();
    let frames = 400usize;
    let inputs = [random_inputs(1, frames, 10), random_inputs(2, frames, 10)];
    let (_, reference) = play_reference(rtc_session(&rom, Player::One, START_TIME), &inputs);
    let mut online = play_online(
        |local| rtc_session(&rom, local, START_TIME),
        &inputs,
        Delay::Jitter(100.0, 40.0),
        |_, _| {},
    );
    assert!(online.rollbacks > 10);
    for side in 0..2 {
        assert!(
            by_frame(&online.checks[side]) == by_frame(&reference),
            "side {side}"
        );
        for player in PLAYERS {
            assert_eq!(clock(online.sessions[side].console(player)), [16, 20, 4, 4]);
        }
    }

    let elapsed = frames as u64 * 70_224 / 4_194_304;
    for (side, ram) in [(0, 0x11), (1, 0x22)] {
        let session = &mut online.sessions[side];
        let saved = session.local_save(false).unwrap();
        let data = saved.as_slice();
        let footer_at = data.len() - crate::persistence::rtc_footer::RTC_FOOTER_LEN;
        let footer = RtcFooter::decode(&data[footer_at..]).unwrap();
        assert_eq!(footer.timestamp, START_TIME + elapsed);
        assert_eq!(data[1], ram);
        assert!(session.local_save(false).is_none());
        assert!(session.local_save(true).is_some());
    }

    let mut later = rtc_session(&rom, Player::One, START_TIME + 86_400);
    later.add_local_input(0, inputs[0][0]).unwrap();
    later.add_remote_input(0, inputs[1][0]);
    later.advance();
    assert_ne!(later.take_check().unwrap().hashes, reference[0].hashes);
}

#[test]
fn inputs_arrive_late_out_of_order_and_twice() {
    let roms = roms(SNAKE, SNAKE);
    let frames = 160u32;
    let inputs = snake_inputs(frames as usize);
    let (_, reference) = play_reference(session(&roms, Player::One, 1), &inputs);
    let mut session = session(&roms, Player::One, 1);
    let mut checks = Vec::new();
    let mut rng = Rng(99);
    let mut pending: Vec<u32> = Vec::new();
    let mut delivered: Vec<u32> = Vec::new();
    let deliver = |session: &mut Session, frame: u32| {
        session.add_remote_input(frame, inputs[1][frame as usize]);
    };
    for frame in 0..frames {
        session
            .add_local_input(frame, inputs[0][frame as usize])
            .unwrap();
        pending.push(frame);
        for i in (1..pending.len()).rev() {
            pending.swap(i, rng.below(i as u64 + 1) as usize);
        }
        let mut kept = Vec::new();
        for remote in pending.drain(..) {
            if frame - remote >= 20 || rng.below(4) == 0 {
                deliver(&mut session, remote);
                deliver(&mut session, remote);
                delivered.push(remote);
            } else {
                kept.push(remote);
            }
        }
        pending = kept;
        if !delivered.is_empty() && rng.below(3) == 0 {
            let again = delivered[rng.below(delivered.len() as u64) as usize];
            deliver(&mut session, again);
        }
        assert_eq!(session.advance(), Advance::Ran);
        checks.extend(std::iter::from_fn(|| session.take_check()));
    }
    for remote in pending.drain(..) {
        deliver(&mut session, remote);
    }
    assert_eq!(session.advance(), Advance::AwaitingLocal);
    checks.extend(std::iter::from_fn(|| session.take_check()));
    assert!(by_frame(&checks) == by_frame(&reference));
    assert_eq!(
        session.add_local_input(3, Input::default()),
        Err(crate::netplay::InputError::Past(3))
    );
}

#[test]
fn a_session_waits_when_the_remote_falls_too_far_behind() {
    let roms = roms(SNAKE, SNAKE);
    let mut session = session(&roms, Player::One, 60);
    for frame in 0..MAX_ROLLBACK_FRAMES {
        session.add_local_input(frame, Input::default()).unwrap();
        assert_eq!(session.advance(), Advance::Ran);
    }
    session
        .add_local_input(MAX_ROLLBACK_FRAMES, Input::default())
        .unwrap();
    assert_eq!(session.advance(), Advance::AwaitingRemote);
    session.add_remote_input(0, Input::new(JoypadState::A));
    assert_eq!(session.advance(), Advance::Ran);
    assert_eq!(session.last_rollback(), MAX_ROLLBACK_FRAMES);
    assert_eq!(session.advance(), Advance::AwaitingLocal);
}

#[test]
fn a_rewind_never_replays_sound_already_heard() {
    let roms = roms(SNAKE, SNAKE);
    let frames = 300u32;
    let inputs = snake_inputs(frames as usize);
    let mut delayed = session(&roms, Player::One, 60);
    let mut direct = session(&roms, Player::One, 60);
    let (mut heard, mut expected) = (Vec::new(), Vec::new());
    let mut most = 0;
    let mut rollbacks = 0;
    for frame in 0..frames {
        for session in [&mut delayed, &mut direct] {
            session
                .add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
        }
        direct.add_remote_input(frame, inputs[1][frame as usize]);
        if frame >= 10 {
            delayed.add_remote_input(frame - 10, inputs[1][frame as usize - 10]);
        }
        assert_eq!(delayed.advance(), Advance::Ran);
        assert_eq!(direct.advance(), Advance::Ran);
        rollbacks += u32::from(delayed.last_rollback() > 0);
        let before = heard.len();
        delayed.drain_audio(&mut heard);
        most = most.max(heard.len() - before);
        direct.drain_audio(&mut expected);
    }
    assert!(rollbacks > 10);
    let (heard, expected) = (heard.len() as f64, expected.len() as f64);
    assert!(
        (heard - expected).abs() < expected * 0.01,
        "heard {heard} samples, a run without rewinds makes {expected}"
    );
    assert!(most < 4_000, "one frame gave {most} samples");
}

#[test]
fn a_rewind_never_replays_the_friends_sound() {
    let roms = roms(SNAKE, SNAKE);
    let frames = 300u32;
    let inputs = snake_inputs(frames as usize);
    let mut delayed = session(&roms, Player::One, 60);
    let mut direct = session(&roms, Player::One, 60);
    let (mut heard, mut expected) = (Vec::new(), Vec::new());
    let mut rollbacks = 0;
    for frame in 0..frames {
        for session in [&mut delayed, &mut direct] {
            session
                .add_local_input(frame, inputs[0][frame as usize])
                .unwrap();
        }
        direct.add_remote_input(frame, inputs[1][frame as usize]);
        if frame >= 10 {
            delayed.add_remote_input(frame - 10, inputs[1][frame as usize - 10]);
        }
        assert_eq!(delayed.advance(), Advance::Ran);
        assert_eq!(direct.advance(), Advance::Ran);
        rollbacks += u32::from(delayed.last_rollback() > 0);
        delayed.drain_remote_audio(&mut heard);
        direct.drain_remote_audio(&mut expected);
    }
    assert!(rollbacks > 10);
    assert!(!expected.is_empty());
    let (heard, expected) = (heard.len() as f64, expected.len() as f64);
    assert!(
        (heard - expected).abs() < expected * 0.01,
        "heard {heard} of the friend's samples, a run without rewinds makes {expected}"
    );
}

#[test]
fn the_friends_sound_plays_only_while_watching() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = snake_inputs(240);
    let mut session = session(&roms, Player::One, 60);
    session.watch_remote(false);
    let mut heard = Vec::new();
    for frame in 0..240u32 {
        if frame == 120 {
            session.drain_remote_audio(&mut heard);
            assert!(heard.is_empty(), "the friend's game made sound unwatched");
            session.watch_remote(true);
        }
        session
            .add_local_input(frame, inputs[0][frame as usize])
            .unwrap();
        session.add_remote_input(frame, inputs[1][frame as usize]);
        assert_eq!(session.advance(), Advance::Ran);
    }
    session.drain_remote_audio(&mut heard);
    assert!(
        !heard.is_empty(),
        "the friend's game made no sound while watched"
    );
    let mut mine = Vec::new();
    session.drain_audio(&mut mine);
    assert!(!mine.is_empty());
}

#[test]
fn snapshots_round_trip_the_whole_pair() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = snake_inputs(200);
    let mut plain = play_plain(
        &roms,
        &[inputs[0][..150].to_vec(), inputs[1][..150].to_vec()],
        [0; 2],
    );
    let mut snapshot = Snapshot::new(&plain.consoles);
    snapshot.save(150, &plain.consoles, &plain.cable);
    let mut scratch = plain.consoles[0].clone_state();
    let saved = state::hash_consoles(&mut scratch, &plain.consoles, &plain.cable);
    for _ in 0..30 {
        pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
    }
    let moved = state::hash_consoles(&mut scratch, &plain.consoles, &plain.cable);
    assert_ne!(saved, moved);
    snapshot.load(&mut plain.consoles, &mut plain.cable);
    assert_eq!(
        state::hash_consoles(&mut scratch, &plain.consoles, &plain.cable),
        saved
    );
}

#[test]
fn a_console_taken_out_of_a_session_runs_on_like_the_session_copy() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = [random_inputs(3, 400, 20), random_inputs(4, 400, 20)];
    let play = |frames: usize| {
        let mut session = session(&roms, Player::One, 60);
        for (frame, (&local, &remote)) in inputs[0].iter().zip(&inputs[1]).take(frames).enumerate()
        {
            session.add_remote_input(frame as u32, remote);
            session.add_local_input(frame as u32, local).unwrap();
            assert_eq!(session.advance(), Advance::Ran);
        }
        session
    };
    let whole = play(400);
    for player in PLAYERS {
        let mut gb = play(250).into_console(player);
        for input in &inputs[player.index()][250..] {
            gb.release_button(input.buttons.complement());
            gb.press_button(input.buttons);
            gb.run_frame();
        }
        assert_same_consoles(&gb, whole.console(player), &format!("{player:?}"));
    }
}

fn play_rewinding(
    roms: &[Rom; 2],
    config: SessionConfig,
    inputs: &[Vec<Input>; 2],
    lag: usize,
    rewind_to: impl Fn(usize) -> Option<u32>,
) -> (Session, Vec<Check>) {
    let local = config.local;
    let mut session = Session::new(config, setups(roms)).unwrap();
    session.watch_remote(true);
    let remote = local.other().index();
    let frames = inputs[0].len();
    let mut checks = Vec::new();
    for frame in 0..frames {
        if frame >= lag {
            session.add_remote_input((frame - lag) as u32, inputs[remote][frame - lag]);
        }
        session
            .add_local_input(frame as u32, inputs[local.index()][frame])
            .unwrap();
        if let Some(to) = rewind_to(frame) {
            session.rewind_to(to);
        }
        assert_eq!(session.advance(), Advance::Ran);
        checks.extend(std::iter::from_fn(|| session.take_check()));
    }
    let start = frames.saturating_sub(lag);
    for (frame, &input) in inputs[remote].iter().enumerate().skip(start) {
        session.add_remote_input(frame as u32, input);
    }
    assert_eq!(session.advance(), Advance::AwaitingLocal);
    checks.extend(std::iter::from_fn(|| session.take_check()));
    (session, checks)
}

fn sorted_checks(checks: &[Check]) -> Vec<Check> {
    let mut sorted = checks.to_vec();
    sorted.sort_by_key(|c| c.frame);
    sorted.dedup_by_key(|c| c.frame);
    sorted
}

fn differing_parts(a: &GameBoy, b: &GameBoy) -> Vec<&'static str> {
    fn bytes<T: serde::Serialize>(part: &T) -> Vec<u8> {
        rmp_serde::to_vec(part).unwrap()
    }
    let parts = [
        ("cpu", bytes(&a.cpu) != bytes(&b.cpu)),
        ("cartridge", bytes(&a.cartridge) != bytes(&b.cartridge)),
        ("cgb", bytes(&a.cgb) != bytes(&b.cgb)),
        ("dma", bytes(&a.dma) != bytes(&b.dma)),
        ("hdma", bytes(&a.hdma) != bytes(&b.hdma)),
        ("ic", bytes(&a.ic) != bytes(&b.ic)),
        ("memory", bytes(&a.memory) != bytes(&b.memory)),
        ("timer", bytes(&a.timer) != bytes(&b.timer)),
        ("ppu", bytes(&a.ppu) != bytes(&b.ppu)),
        ("apu", bytes(&a.apu) != bytes(&b.apu)),
        ("joypad", bytes(&a.joypad) != bytes(&b.joypad)),
        ("serial", bytes(&a.serial) != bytes(&b.serial)),
        ("cycles", a.cycle_counter != b.cycle_counter),
    ];
    parts
        .into_iter()
        .filter(|(_, differs)| *differs)
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn rewinds_replay_to_the_same_state_as_a_straight_run() {
    let roms = roms(SNAKE, SNAKE);
    let inputs = snake_inputs(600);
    let power_on = 60;
    let rewind_to = |frame: usize| match frame {
        f if f > power_on as usize && f <= power_on as usize + 12 => Some(power_on),
        f if f % 7 == 0 => Some(f.saturating_sub(3 + f / 7 % 6) as u32),
        _ => None,
    };
    for local in PLAYERS {
        let mapped = match local {
            Player::One => inputs.clone(),
            Player::Two => [inputs[1].clone(), inputs[0].clone()],
        };
        for (interval, power_on) in [(1, [0, 0]), (60, [0, 0]), (1, [0, power_on])] {
            let config = SessionConfig {
                power_on,
                ..config(local, interval)
            };
            let mut reference = Session::new(config, setups(&roms)).unwrap();
            reference.watch_remote(true);
            let (reference, straight) = play_reference(reference, &mapped);
            for lag in [3, 8, 12] {
                let (session, rewound) = play_rewinding(&roms, config, &inputs, lag, rewind_to);
                let ours = sorted_checks(&rewound);
                let theirs = sorted_checks(&straight);
                let first = theirs
                    .iter()
                    .enumerate()
                    .find(|&(i, check)| ours.get(i) != Some(check))
                    .map(|(_, check)| check.frame);
                let parts =
                    PLAYERS.map(|p| differing_parts(session.console(p), reference.console(p)));
                let case = format!(
                    "{local:?} local, interval {interval}, power on {power_on:?}, lag {lag}"
                );
                assert!(
                    first.is_none(),
                    "{case}: first differing check at frame {first:?}, \
                     differing parts at the end {parts:?}"
                );
                for player in PLAYERS {
                    assert_same_consoles(
                        session.console(player),
                        reference.console(player),
                        &format!("{case}, {player:?}"),
                    );
                }
            }
        }
    }
}

#[test]
fn a_session_keeps_its_consoles_off_the_stack() {
    let size = std::mem::size_of::<Session>();
    assert!(size < 4096, "a session is {size} bytes");
}

fn median_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn p95_ms(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() * 95 / 100]
}

fn time_ms(mut f: impl FnMut()) -> f64 {
    let start = std::time::Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1000.0
}

fn measure(name: &str, roms: &[Rom; 2], plug: bool) {
    let frames = 600;
    let mut inputs = [random_inputs(1, frames, 20), random_inputs(2, frames, 20)];
    if plug {
        cable(&mut inputs[0], 0, CableEvent::Plug);
    }
    let mut plain = play_plain(roms, &inputs, [0; 2]);
    let mut snapshot = Snapshot::new(&plain.consoles);
    let mut scratch = plain.consoles[0].clone_state();
    let (mut save, mut load, mut rerun, mut frame, mut hash) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..200 {
        save.push(time_ms(|| snapshot.save(0, &plain.consoles, &plain.cable)));
        frame.push(time_ms(|| {
            pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2])
        }));
        load.push(time_ms(|| {
            snapshot.load(&mut plain.consoles, &mut plain.cable)
        }));
        rerun.push(time_ms(|| {
            for _ in 0..8 {
                pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
            }
        }));
        hash.push(time_ms(|| {
            std::hint::black_box(state::hash_consoles(
                &mut scratch,
                &plain.consoles,
                &plain.cable,
            ));
        }));
    }
    let mut catch_up = Vec::new();
    for _ in 0..200 {
        snapshot.load(&mut plain.consoles, &mut plain.cable);
        for gb in &mut plain.consoles {
            gb.ppu.blind = true;
            gb.apu.muted = true;
        }
        catch_up.push(time_ms(|| {
            for _ in 0..8 {
                pair::run_frame(&mut plain.consoles, &mut plain.cable, &IDLE, [true; 2]);
            }
        }));
        for gb in &mut plain.consoles {
            gb.ppu.blind = false;
            gb.apu.muted = false;
        }
    }
    let (catch_up, catch_up_p95) = (median_ms(catch_up.clone()), p95_ms(catch_up));
    let rerun_p95 = p95_ms(rerun.clone());
    let (save, load, rerun, frame, hash) = (
        median_ms(save),
        median_ms(load),
        median_ms(rerun),
        median_ms(frame),
        median_ms(hash),
    );

    let mut session = Session::new(config(Player::One, 60), setups(roms)).unwrap();
    let mut worst = Vec::new();
    for f in 0..frames as u32 {
        session.add_local_input(f, inputs[0][f as usize]).unwrap();
        if f >= 8 {
            let flipped = Input::new(JoypadState::from_bits_truncate(f as u8));
            session.add_remote_input(f - 8, flipped);
        }
        let ms = time_ms(|| {
            session.advance();
        });
        if f >= 60 {
            worst.push(ms);
        }
    }
    worst.sort_by(f64::total_cmp);
    let online = play_online(
        |player| Session::new(config(player, 60), setups(roms)).unwrap(),
        &inputs,
        Delay::Jitter(75.0, 20.0),
        |_, _| {},
    );
    let mut played = online.advance_ms;
    played.sort_by(f64::total_cmp);
    eprintln!(
        "{name}: snapshot {save:.3} ms, restore {load:.3} ms, 8 frames of two consoles {rerun:.3} ms (p95 {rerun_p95:.3} ms), \
         total {:.3} ms; 8 catch-up frames without picture or sound {catch_up:.3} ms (p95 {catch_up_p95:.3} ms); one frame of two consoles {frame:.3} ms; state hash {hash:.3} ms; \
         advance with an 8-frame rewind every frame: median {:.3} ms, p95 {:.3} ms, max {:.3} ms; \
         online at 150 ms RTT: advance median {:.3} ms, p95 {:.3} ms, max {:.3} ms, {} rewinds, deepest {} frames",
        save + load + rerun,
        worst[worst.len() / 2],
        worst[worst.len() * 95 / 100],
        worst[worst.len() - 1],
        played[played.len() / 2],
        played[played.len() * 95 / 100],
        played[played.len() - 1],
        online.rollbacks,
        online.deepest,
    );
}

#[test]
#[ignore]
fn rollback_cost() {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            measure("DMG Snake, unplugged", &roms(SNAKE, SNAKE), false);
            measure("DMG Snake, linked", &roms(SNAKE, SNAKE), true);
            measure("CGB Petris, unplugged", &roms(PETRIS, PETRIS), false);
            measure("CGB Petris, linked", &roms(PETRIS, PETRIS), true);
        })
        .unwrap()
        .join()
        .unwrap();
}

fn long_run(roms: &[Rom; 2], frames: usize, plug: bool, blind: bool) -> ([u64; 2], [u64; 2]) {
    use std::io::Write;
    let mut inputs = [random_inputs(5, frames, 30), random_inputs(6, frames, 30)];
    for side in &mut inputs {
        for at in (100..frames.saturating_sub(10)).step_by(240) {
            press(side, at, JoypadState::START, 5);
        }
    }
    if plug {
        cable(&mut inputs[0], 0, CableEvent::Plug);
    }
    let config = config(Player::One, 1);
    let [one, two] = setups(roms);
    let mut consoles = [
        new_console(one.unwrap(), &config, Player::One).unwrap(),
        new_console(two.unwrap(), &config, Player::Two).unwrap(),
    ];
    for gb in &mut consoles {
        gb.ppu.blind = blind;
        gb.apu.muted = blind;
    }
    let mut cable = Cable::default();
    let mut scratch = consoles[0].clone_state();
    let mut states = [state::Fnv::new(), state::Fnv::new()];
    let mut pictures = [state::Fnv::new(), state::Fnv::new()];
    for (&one, &two) in inputs[0].iter().zip(&inputs[1]) {
        pair::run_frame(&mut consoles, &mut cable, &[one, two], [true; 2]);
        let hashes = state::hash_consoles(&mut scratch, &consoles, &cable);
        for i in 0..2 {
            states[i].write_all(&hashes[i].to_le_bytes()).unwrap();
            pictures[i]
                .write_all(consoles[i].frame().as_slice())
                .unwrap();
        }
    }
    (states.map(|f| f.finish()), pictures.map(|f| f.finish()))
}

const LONG_RUNS: [(&str, &[u8], &[u8], bool); 4] = [
    ("DMG Snake, linked", SNAKE, SNAKE, true),
    ("CGB Petris, linked", PETRIS, PETRIS, true),
    ("DMG Tobu and CGB uCity", TOBU, UCITY, false),
    ("DMG libbet and CGB Postie", LIBBET, POSTIE, false),
];

#[test]
fn catch_up_frames_end_in_the_same_state() {
    for (name, one, two, plug) in LONG_RUNS {
        let roms = roms(one, two);
        assert_eq!(
            long_run(&roms, 300, plug, true).0,
            long_run(&roms, 300, plug, false).0,
            "{name}"
        );
    }
}

#[test]
#[ignore]
fn long_run_hashes() {
    for (name, one, two, plug) in LONG_RUNS {
        let roms = roms(one, two);
        let (states, pictures) = long_run(&roms, 6000, plug, false);
        let (blind, _) = long_run(&roms, 6000, plug, true);
        assert_eq!(states, blind, "{name}");
        eprintln!("{name}: states {states:016x?}, pictures {pictures:016x?}");
    }
}

fn alone<'a>(rom: &'a Rom, state: Option<&'a [u8]>) -> [Option<ConsoleSetup<'a>>; 2] {
    [
        Some(ConsoleSetup {
            rom,
            model: GbModel::for_rom(rom).unwrap(),
            save: None,
            state,
        }),
        None,
    ]
}

fn watched(rom: &Rom, local: Player, state: Option<&[u8]>) -> Session {
    let mut session = Session::new(config(local, 60), alone(rom, state)).unwrap();
    session.watch_remote(true);
    session
}

fn run_alone(gb: &mut GameBoy, inputs: &[Input]) {
    for input in inputs {
        gb.release_button(input.buttons.complement());
        gb.press_button(input.buttons);
        gb.run_frame();
    }
}

#[test]
fn a_player_never_waits_for_a_watcher() {
    let rom = Rom::new(SNAKE);
    let inputs = random_inputs(7, 400, 20);
    let mut player = watched(&rom, Player::One, None);
    assert!(player.present(Player::One));
    assert!(!player.present(Player::Two));
    for (frame, &input) in inputs.iter().enumerate() {
        player.add_local_input(frame as u32, input).unwrap();
        player.add_remote_input(frame as u32, Input::new(JoypadState::A));
        assert_eq!(player.advance(), Advance::Ran);
        assert_eq!(player.confirmed_frame(), player.frame());
        assert_eq!(player.last_rollback(), 0);
    }
    let config = config(Player::One, 60);
    let [setup, _] = alone(&rom, None);
    let mut plain = new_console(setup.unwrap(), &config, Player::One).unwrap();
    run_alone(&mut plain, &inputs);
    assert_same_consoles(player.console(Player::One), &plain, "player");
    let mut sound = Vec::new();
    player.drain_audio(&mut sound);
    assert!(!sound.is_empty());
}

#[test]
fn a_watcher_runs_the_players_console_only_on_inputs_that_arrived() {
    let rom = Rom::new(SNAKE);
    let frames = 600u32;
    let inputs = random_inputs(9, frames as usize, 20);
    let mut player = watched(&rom, Player::One, None);
    let mut watcher = watched(&rom, Player::Two, None);
    let mut pads = Wire::new(Delay::Jitter(90.0, 40.0), 1);
    let mut checks = Wire::<Check>::new(Delay::Jitter(90.0, 40.0), 2);
    let (mut arrived, mut heard) = (0u32, Vec::new());
    for tick in 0..frames * 3 {
        let now = tick as f64 * FRAME_MS;
        if player.frame() < frames {
            let frame = player.frame();
            player
                .add_local_input(frame, inputs[frame as usize])
                .unwrap();
            assert_eq!(player.advance(), Advance::Ran);
            pads.send(now, (frame, inputs[frame as usize]));
            while let Some(check) = player.take_check() {
                checks.send(now, check);
            }
        }
        while let Some((frame, input)) = pads.arrived(now) {
            watcher.add_remote_input(frame, input);
            arrived = arrived.max(frame + 1);
        }
        while let Some(check) = checks.arrived(now) {
            watcher.add_remote_check(check);
        }
        let before = watcher.frame();
        let advanced = watcher.advance();
        assert_eq!(advanced == Advance::Ran, before < arrived);
        assert!(watcher.frame() <= arrived);
        assert_eq!(watcher.last_rollback(), 0);
        while watcher.take_check().is_some() {}
        watcher.drain_audio(&mut heard);
    }
    assert_eq!(watcher.frame(), frames);
    assert_eq!(watcher.desync(), None);
    assert_same_consoles(
        watcher.console(Player::One),
        player.console(Player::One),
        "watcher",
    );
    assert!(!heard.is_empty(), "the watcher hears the player's game");
    assert!(watcher.local_save(true).is_none());
}

#[test]
fn a_console_resumes_from_its_running_state() {
    let rom = Rom::new(SNAKE);
    let inputs = random_inputs(5, 500, 20);
    let config = config(Player::One, 60);
    let [setup, _] = alone(&rom, None);
    let mut plain = new_console(setup.unwrap(), &config, Player::One).unwrap();
    run_alone(&mut plain, &inputs[..250]);
    let state = crate::netplay::encode_state(&plain).unwrap();
    run_alone(&mut plain, &inputs[250..]);
    for local in PLAYERS {
        let mut session = watched(&rom, local, Some(&state));
        for (frame, &input) in inputs[250..].iter().enumerate() {
            match local {
                Player::One => session.add_local_input(frame as u32, input).unwrap(),
                Player::Two => session.add_remote_input(frame as u32, input),
            }
            assert_eq!(session.advance(), Advance::Ran);
        }
        assert_same_consoles(session.console(Player::One), &plain, &format!("{local:?}"));
    }
}
