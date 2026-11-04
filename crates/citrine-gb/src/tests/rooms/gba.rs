use super::{CHECK_EVERY, LOCKSTEP, Room, Straight, Units, pad};
use crate::console::{Console, Rollback};
use crate::gba::machine::Gba;
use crate::netplay::Sync;
use crate::netplay::pair::{self, Cable};
use crate::netplay::{LinkPort, Player, Session, SessionConfig};

const EWRAM: u32 = 0x0200_0000;
const MISSING: u16 = 0xFFFF;
const FRAMES: u32 = 240;
const CHANGE: u32 = 120;

fn program() -> Vec<u8> {
    [
        0xE3A0_0404,
        0xE280_2C01,
        0xE280_3C02,
        0xE3A0_4000,
        0xE1C2_43B4,
        0xE3A0_5A06,
        0xE385_5083,
        0xE3A0_8402,
        0xE3A0_9080,
        0xE1D2_13B0,
        0xE021_1004,
        0xE1C2_12BA,
        0xE1C3_90B2,
        0xE1C2_52B8,
        0xE1D3_60B2,
        0xE316_0080,
        0x0AFF_FFFC,
        0xE592_6020,
        0xE592_7024,
        0xE8A8_00C0,
        0xE3C8_8701,
        0xE284_4001,
        0xEAFF_FFF1u32,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect()
}

impl Units for Gba {
    fn units<const N: usize>() -> [Self; N] {
        std::array::from_fn(|_| Gba::new(&program()))
    }

    fn joiner() -> Self {
        Gba::new(&program())
    }
}

fn transfers(gba: &Gba) -> Vec<[u16; 4]> {
    let count = gba.cpu().reg(4);
    (0..count)
        .map(|i| {
            let low = gba.bus().read32(EWRAM + 8 * i);
            let high = gba.bus().read32(EWRAM + 8 * i + 4);
            [
                low as u16,
                (low >> 16) as u16,
                high as u16,
                (high >> 16) as u16,
            ]
        })
        .collect()
}

fn assert_same_transfers(logs: &[Vec<[u16; 4]>], linked: usize) {
    let longest = logs.iter().max_by_key(|log| log.len()).unwrap();
    assert!(longest.len() > 100, "{} transfers", longest.len());
    for log in logs {
        assert!(!log.is_empty());
        let fits = (0..=1).any(|behind| {
            let end = longest.len() - behind;
            end >= log.len() && longest[end - log.len()..end] == log[..]
        });
        assert!(fits, "a unit saw other words");
    }
    let last = longest.last().unwrap();
    for (id, word) in last.iter().enumerate() {
        assert_eq!(*word == MISSING, id >= linked, "word {id} of {last:04X?}");
    }
}

fn session_logs<const N: usize>(session: &Session<Gba, N>, units: &[usize]) -> Vec<Vec<[u16; 4]>> {
    units
        .iter()
        .map(|&i| transfers(session.console(Player::ALL[i])))
        .collect()
}

fn linked<const N: usize>(sync: Sync, lag: u32, jitter: u32) {
    let mut room = Room::<N, Gba>::with_units(sync, [true; N], lag, jitter);
    room.run_until(FRAMES);
    if sync.budget == 0 {
        assert_eq!(room.rollbacks, 0);
    } else {
        assert!(room.rollbacks > 0);
        assert!(room.furthest <= sync.budget);
        assert!(room.deepest <= sync.budget);
    }
    room.assert_matches(&Straight::new(), FRAMES);
    let all: Vec<usize> = (0..N).collect();
    for i in 0..N {
        let session = room.session(Player::ALL[i]);
        assert!(session.cable_plugged());
        assert_same_transfers(&session_logs(session, &all), N);
    }
}

#[test]
fn two_gbas_trade_multiplay_words_in_lockstep() {
    linked::<2>(LOCKSTEP, 1, 2);
}

#[test]
fn three_gbas_trade_multiplay_words_in_lockstep() {
    linked::<3>(LOCKSTEP, 1, 2);
}

#[test]
fn four_gbas_trade_multiplay_words_in_lockstep() {
    linked::<4>(LOCKSTEP, 1, 2);
}

#[test]
fn two_gbas_trade_multiplay_words_with_rollback_inside_the_budget() {
    linked::<2>(Sync::new(4, 0), 6, 3);
}

#[test]
fn four_gbas_trade_multiplay_words_with_rollback_inside_the_budget() {
    linked::<4>(Sync::new(2, 1), 4, 2);
}

#[test]
fn a_gba_that_leaves_at_a_frame_unplugs_and_the_others_trade_on() {
    let mut room = Room::<4, Gba>::with_units(LOCKSTEP, [true; 4], 1, 2);
    room.run_until(CHANGE / 2);
    for i in 0..3 {
        room.session(Player::ALL[i])
            .leave(Player::Four, CHANGE)
            .unwrap();
    }
    room.run_until(CHANGE);
    room.silent[3] = true;
    room.run_until(FRAMES);
    let mut straight = Straight::new();
    straight.leave_at[3] = CHANGE;
    room.assert_matches(&straight, FRAMES);
    for i in 0..3 {
        let session = room.session(Player::ALL[i]);
        assert_same_transfers(&session_logs(session, &[0, 1, 2]), 3);
    }
}

#[test]
fn a_gba_that_joins_at_a_frame_links_in_from_a_state() {
    let mut room = Room::<4, Gba>::with_units(LOCKSTEP, [true, true, true, false], 1, 2);
    room.run_until(CHANGE / 2);
    for i in 0..3 {
        room.session(Player::ALL[i])
            .join(Player::Four, CHANGE, Gba::joiner())
            .unwrap();
    }
    room.run_until(CHANGE);
    let states = loop {
        let source = room.session(Player::One);
        let states: Option<Vec<Vec<u8>>> = (0..4)
            .map(|i| {
                let player = Player::ALL[i];
                if let Some(state) = source.checkpoint(CHANGE, player) {
                    return Some(state);
                }
                let here = source.frame() == CHANGE && source.confirmed_frame() == CHANGE;
                let console = if i == 3 {
                    Gba::joiner()
                } else {
                    source.console(player).clone_state()
                };
                here.then(|| crate::netplay::encode_state(&console).unwrap())
            })
            .collect();
        if let Some(states) = states {
            break states;
        }
        assert!(room.tick < 1_000, "no state at frame {CHANGE}");
        room.step();
    };
    let template = Gba::joiner();
    let consoles = std::array::from_fn(|i| template.decode_state(&states[i]).unwrap());
    let config = SessionConfig {
        check_interval: CHECK_EVERY,
        sync: LOCKSTEP,
        start_frame: CHANGE,
        cable_plugged: room.session(Player::One).cable_plugged(),
        power_on: [0, 0, 0, CHANGE],
        ..SessionConfig::new(Player::Four, 7, 0)
    };
    let mut session = Session::with_consoles(config, consoles, [true; 4]);
    session.watch_remote(true);
    room.sessions[3] = Some(session);
    room.sent[3] = CHANGE;
    for unit in 0..3 {
        for frame in CHANGE..room.sent[unit] {
            room.session(Player::Four)
                .add_player_input(Player::ALL[unit], frame, pad(unit, frame));
        }
    }
    room.run_until(FRAMES);
    let mut straight = Straight::new();
    straight.power_on[3] = CHANGE;
    straight.join = Some((CHANGE, 3));
    room.assert_matches(&straight, FRAMES);
    for i in 0..4 {
        let session = room.session(Player::ALL[i]);
        assert_same_transfers(&session_logs(session, &[0, 1, 2, 3]), 4);
    }
}

fn watcher_checks(
    room: &mut Room<4, Gba>,
    at: u32,
    states: &[Vec<u8>],
    cable: Option<&[u8]>,
) -> Vec<crate::netplay::Check<4>> {
    let template = Gba::joiner();
    let consoles = std::array::from_fn(|i| match states.get(i) {
        Some(state) => template.decode_state(state).unwrap(),
        None => Gba::joiner(),
    });
    let config = SessionConfig {
        check_interval: CHECK_EVERY,
        sync: LOCKSTEP,
        start_frame: at,
        ..SessionConfig::new(Player::Four, 7, 0)
    };
    let mut watcher = Session::with_consoles(config, consoles, [true, true, true, false]);
    if let Some(cable) = cable {
        watcher.restore_cable(cable).unwrap();
    }
    watcher.watch_remote(true);
    room.run_until(FRAMES);
    let mut checks = Vec::new();
    for frame in at..FRAMES {
        for unit in 0..3 {
            watcher.add_player_input(Player::ALL[unit], frame, pad(unit, frame));
        }
        while watcher.advance() == crate::netplay::Advance::Ran {}
        while let Some(check) = watcher.take_check() {
            checks.push(check);
        }
    }
    checks
}

#[test]
fn a_watcher_follows_a_running_group_from_a_checkpoint_and_its_cable() {
    let mut room = Room::<4, Gba>::with_units(LOCKSTEP, [true, true, true, false], 1, 2);
    room.run_until(CHANGE);
    let source = room.session(Player::One);
    let at = source.latest_checkpoint().unwrap();
    let cable = source.checkpoint_cable(at).unwrap();
    let states: Vec<Vec<u8>> = (0..3)
        .map(|i| source.checkpoint(at, Player::ALL[i]).unwrap())
        .collect();
    let compare = |room: &Room<4, Gba>, checks: &[crate::netplay::Check<4>]| {
        checks
            .iter()
            .filter_map(|check| {
                let theirs = room.checks.iter().find(|c| c.frame == check.frame)?;
                Some(theirs.hashes[..3] == check.hashes[..3])
            })
            .collect::<Vec<bool>>()
    };
    let followed = watcher_checks(&mut room, at, &states, Some(&cable));
    let matched = compare(&room, &followed);
    assert!(matched.len() >= ((FRAMES - at) / CHECK_EVERY - 1) as usize);
    assert!(matched.iter().all(|same| *same));
    let blind = watcher_checks(&mut room, at, &states, None);
    assert!(compare(&room, &blind).iter().any(|same| !same));
}

fn linked_pair(frames: u32) -> [Gba; 2] {
    let mut consoles = Gba::units::<2>();
    let mut cable = Cable::default();
    for frame in 0..frames {
        let inputs = [pad(0, frame), pad(1, frame)];
        pair::run_frame(&mut consoles, &mut cable, &inputs, [true; 2]);
    }
    consoles
}

#[test]
fn a_gba_state_round_trip_keeps_the_hash_and_the_run() {
    let [mut one, _] = linked_pair(30);
    one.unplug_link();
    let mut scratch = Gba::joiner();
    let mut two = Gba::joiner()
        .decode_state(&one.encode_state().unwrap())
        .unwrap();
    assert_eq!(two.state_hash(&mut scratch), one.state_hash(&mut scratch));
    let mut three = Gba::joiner();
    three.copy_state_from(&one);
    for frame in 0..20 {
        for gba in [&mut one, &mut two, &mut three] {
            gba.set_keys(pad(0, frame).buttons);
            gba.run_frame();
        }
    }
    let hash = one.state_hash(&mut scratch);
    assert_eq!(two.state_hash(&mut scratch), hash);
    assert_eq!(three.state_hash(&mut scratch), hash);
    assert_eq!(two.frame(), one.frame());
    assert_eq!(three.frame(), one.frame());
}

#[test]
fn a_gba_state_leaves_the_game_out() {
    let mut rom = program();
    rom.resize(4 << 20, 0);
    let gba = Gba::new(&rom);
    let state = gba.encode_state().unwrap();
    assert!(state.len() < 1 << 20, "{} bytes", state.len());
    let back = Gba::new(&rom).decode_state(&state).unwrap();
    assert_eq!(back.bus().rom().len(), rom.len());
}

#[test]
fn a_blind_and_muted_gba_hashes_like_a_seen_one() {
    let mut seen = Gba::joiner();
    let mut blind = Gba::joiner();
    blind.set_blind(true);
    blind.set_muted(true);
    let mut audio = Vec::new();
    for frame in 0..10 {
        for gba in [&mut seen, &mut blind] {
            gba.set_keys(pad(1, frame).buttons);
            gba.run_frame();
        }
        blind.take_audio(&mut audio);
    }
    let mut scratch = Gba::joiner();
    assert_eq!(
        blind.state_hash(&mut scratch),
        seen.state_hash(&mut scratch)
    );
    assert!(audio.is_empty());
}
