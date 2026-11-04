use crate::console::Keys;
use crate::gb::{GameBoy, GbModel};
use crate::netplay::LocalCable;
use crate::rom::Rom;
use crate::{ReadMemory, WriteMemory};

pub(super) const COUNT: u16 = 300;
const NOW: u64 = 1_790_000_000;
const IDLE: [Keys; 2] = [Keys::empty(); 2];
const GO: [Keys; 2] = [Keys::A; 2];

pub(super) fn trade_program(sc: u8, first: u8, step: u8) -> Vec<u8> {
    let [low, high] = COUNT.to_le_bytes();
    let mut code = vec![0x3E, 0x0A, 0xEA, 0x00, 0x00, 0xAF, 0xEA, 0x00, 0xDF];
    code.extend_from_slice(&[0x3E, 0x10, 0xE0, 0x00, 0xF0, 0x00, 0xCB, 0x47, 0x20, 0xFA]);
    code.extend_from_slice(&[0x21, 0x00, 0xA0, 0x11, low, high, 0x06, first]);
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

pub(super) fn cart(cgb_flag: u8, kind: u8, code: &[u8]) -> Rom {
    let mut data = super::cgb::rom(cgb_flag, code).data;
    data[0x0147] = kind;
    data[0x0149] = 0x02;
    Rom::new(&data)
}

pub(super) fn red() -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Dmg);
    gb.load_rom(&cart(0x00, 0x03, &trade_program(0x81, 0x10, 1)))
        .unwrap();
    gb
}

fn silver() -> GameBoy {
    let mut gb = GameBoy::new_empty(GbModel::Cgb);
    gb.load_rom(&cart(0x80, 0x10, &trade_program(0x80, 0x40, 3)))
        .unwrap();
    gb
}

pub(super) fn sent(first: u8, step: u8) -> Vec<u8> {
    (0..COUNT)
        .map(|i| first.wrapping_add((i as u8).wrapping_mul(step)))
        .collect()
}

pub(super) fn done(gb: &GameBoy) -> bool {
    gb.memory.read_naive(0xDF00) == 0xAA
}

pub(super) fn saved(gb: &mut GameBoy) -> Vec<u8> {
    let dump = gb.poll_sram_dump(true, NOW).expect("a battery save");
    dump.as_slice()[..COUNT as usize].to_vec()
}

fn lines(gb: &mut GameBoy) -> [u8; 2] {
    [0x10, 0x20].map(|select| {
        gb.joypad.write_naive(0xFF00, select);
        gb.joypad.read_naive(0xFF00) & 0x0F
    })
}

fn trade(red: &mut GameBoy, silver: &mut GameBoy, cable: &mut LocalCable<GameBoy>) -> usize {
    (1..=600)
        .find(|_| {
            cable.run_frame([red, silver], GO);
            done(red) && done(silver)
        })
        .expect("the trade did not finish in 10 seconds")
}

#[test]
fn red_and_silver_trade_over_a_local_cable_and_keep_it_in_their_saves() {
    let (mut red, mut silver) = (red(), silver());
    assert_eq!(red.model, GbModel::Dmg);
    assert_eq!(silver.model, GbModel::Cgb);
    for _ in 0..30 {
        red.run_frame();
    }
    let mut cable = LocalCable::new();
    cable.set_plugged(true);
    trade(&mut red, &mut silver, &mut cable);

    assert!(red.link_plugged() && silver.link_plugged());
    assert_eq!(saved(&mut red), sent(0x40, 3));
    assert_eq!(saved(&mut silver), sent(0x10, 1));

    cable.unplug([&mut red, &mut silver]);
    assert!(!cable.plugged());
    assert!(!red.link_plugged() && !silver.link_plugged());
}

#[test]
fn without_the_cable_the_master_reads_ff_and_the_other_game_keeps_waiting() {
    let (mut red, mut silver) = (red(), silver());
    let mut cable = LocalCable::new();
    for _ in 0..120 {
        cable.run_frame([&mut red, &mut silver], GO);
    }
    assert!(!red.link_plugged());
    assert!(done(&red) && !done(&silver));
    assert_eq!(saved(&mut red), vec![0xFF; COUNT as usize]);
}

#[test]
fn a_cable_plugged_in_after_both_games_ran_alone_still_trades() {
    let (mut red, mut silver) = (red(), silver());
    let mut cable = LocalCable::new();
    for _ in 0..120 {
        cable.run_frame([&mut red, &mut silver], IDLE);
    }
    assert!(!done(&red) && !done(&silver));
    cable.set_plugged(true);
    trade(&mut red, &mut silver, &mut cable);
    assert_eq!(saved(&mut red), sent(0x40, 3));
    assert_eq!(saved(&mut silver), sent(0x10, 1));
}

#[test]
fn buttons_reach_each_console_on_its_own() {
    let (mut red, mut silver) = (red(), silver());
    let mut cable = LocalCable::new();
    cable.run_frame([&mut red, &mut silver], [Keys::A, Keys::START | Keys::LEFT]);
    assert_eq!(lines(&mut red), [0x0E, 0x0F]);
    assert_eq!(lines(&mut silver), [0x07, 0x0D]);
}
