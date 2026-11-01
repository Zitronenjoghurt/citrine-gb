use super::cgb::{boot, read, write};
use crate::gb::serial::LinkMessage;
use crate::gb::{GameBoy, GbModel};

const CYCLES_PER_MS: u64 = 1_048_576 / 1000;
const DONE: u16 = 0xC0FF;
const SWITCH_SPEED: [u8; 6] = [0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00];

fn exchange_program(prefix: &[u8], sc: u8, first: u8, count: u8) -> Vec<u8> {
    let mut code = prefix.to_vec();
    code.extend_from_slice(&[
        0xAF, 0xEA, 0xFF, 0xC0, 0x21, 0x00, 0xC0, 0x06, count, 0x3E, first, 0xE0, 0x01, 0x4F, 0x3E,
        sc, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA, 0xF0, 0x01, 0x22, 0x79, 0x3C, 0x05,
        0x20, 0xEB, 0x3E, 0xAA, 0xEA, 0xFF, 0xC0,
    ]);
    code
}

fn transfer(id: u16, byte: u8) -> LinkMessage {
    LinkMessage::Transfer {
        id,
        byte,
        seq: 0,
        since: 0,
    }
}

fn outgoing(gb: &mut GameBoy) -> Vec<LinkMessage> {
    gb.take_link_messages()
        .into_iter()
        .map(|message| match message {
            LinkMessage::Transfer { id, byte, .. } => transfer(id, byte),
            reply => reply,
        })
        .collect()
}

fn plugged(model: GbModel, cgb_flag: u8, code: &[u8]) -> GameBoy {
    let mut gb = boot(model, cgb_flag, code);
    gb.plug_link();
    gb
}

struct Pair {
    gbs: [GameBoy; 2],
    clocks: [u64; 2],
    waited: [u64; 2],
    longest_wait: [u64; 2],
    wait_run: [u64; 2],
    sent: [u64; 2],
    transfers: [Vec<(u64, u8)>; 2],
    in_flight: Vec<(u64, usize, LinkMessage)>,
    latency: u64,
    jitter: u64,
    noise: u64,
    last_due: [u64; 2],
}

impl Pair {
    fn new(a: GameBoy, b: GameBoy, latency_ms: u64) -> Self {
        Self {
            gbs: [a, b],
            clocks: [0; 2],
            waited: [0; 2],
            longest_wait: [0; 2],
            wait_run: [0; 2],
            sent: [0; 2],
            transfers: [Vec::new(), Vec::new()],
            in_flight: Vec::new(),
            latency: latency_ms * CYCLES_PER_MS,
            jitter: 0,
            noise: 0x9E37_79B9_7F4A_7C15,
            last_due: [0; 2],
        }
    }

    fn step(&mut self) {
        let i = if self.clocks[0] <= self.clocks[1] {
            0
        } else {
            1
        };
        let now = self.clocks[i];
        let mut arrived = Vec::new();
        self.in_flight.retain(|&(due, to, message)| {
            let ready = to == i && due <= now;
            if ready {
                arrived.push(message);
            }
            !ready
        });
        for message in arrived {
            self.gbs[i].receive_link_message(message);
        }

        let gb = &mut self.gbs[i];
        let waiting = gb.link_waiting();
        let before = gb.cycle_counter;
        gb.step();
        let spent = gb.cycle_counter.wrapping_sub(before) as u64;
        self.clocks[i] += spent;
        if waiting {
            self.waited[i] += spent;
            self.wait_run[i] += spent;
            self.longest_wait[i] = self.longest_wait[i].max(self.wait_run[i]);
        } else {
            self.wait_run[i] = 0;
        }
        let played = self.clocks[i] - self.waited[i];
        for message in gb.take_link_messages() {
            self.sent[i] += 1;
            if let LinkMessage::Transfer { byte, .. } = message {
                self.transfers[i].push((played, byte));
            }
            let mut due = self.clocks[i] + self.latency;
            if self.jitter > 0 {
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 7;
                self.noise ^= self.noise << 17;
                due = due - self.jitter + self.noise % (2 * self.jitter + 1);
            }
            let due = due.max(self.last_due[1 - i]);
            self.last_due[1 - i] = due;
            self.in_flight.push((due, 1 - i, message));
        }
    }

    fn run_until(&mut self, limit: u64, mut done: impl FnMut(&mut Pair) -> bool) {
        while !done(self) {
            assert!(
                self.clocks[0].min(self.clocks[1]) < limit,
                "pair did not finish within {limit} cycles"
            );
            self.step();
        }
    }

    fn with_jitter(mut self, jitter_ms: u64) -> Self {
        self.jitter = jitter_ms * CYCLES_PER_MS;
        self
    }

    fn played(&self, side: usize) -> u64 {
        self.clocks[side] - self.waited[side]
    }

    fn both_done(&mut self) -> bool {
        self.gbs.iter_mut().all(|gb| read(gb, DONE) == 0xAA)
    }
}

fn received(gb: &mut GameBoy, count: u8) -> Vec<u8> {
    (0..count as u16).map(|i| read(gb, 0xC000 + i)).collect()
}

fn run_exchange(master: GameBoy, slave: GameBoy, latency_ms: u64, count: u8) -> Pair {
    let mut pair = Pair::new(master, slave, latency_ms);
    let limit = (count as u64 + 4) * (2 * latency_ms * CYCLES_PER_MS + 20_000) + 200_000;
    pair.run_until(limit, Pair::both_done);
    assert_eq!(
        received(&mut pair.gbs[0], count),
        (0..count).map(|i| 0x80 + i).collect::<Vec<_>>()
    );
    assert_eq!(
        received(&mut pair.gbs[1], count),
        (0..count).map(|i| 0x10 + i).collect::<Vec<_>>()
    );
    assert!(pair.in_flight.is_empty());
    pair
}

#[test]
fn scripted_bytes_cross_both_ways_at_any_latency() {
    for latency in [0, 16, 100, 250] {
        let count = 6;
        let master = plugged(
            GbModel::Dmg,
            0x00,
            &exchange_program(&[], 0x81, 0x10, count),
        );
        let slave = plugged(
            GbModel::Dmg,
            0x00,
            &exchange_program(&[], 0x80, 0x80, count),
        );
        let pair = run_exchange(master, slave, latency, count);

        let round_trip = 2 * latency * CYCLES_PER_MS;
        let transfer = 8 * 128;
        if latency == 0 {
            assert!(
                pair.waited[0] < transfer,
                "master paused {} cycles with no latency",
                pair.waited[0]
            );
        } else {
            let least = count as u64 * (round_trip - transfer);
            let most = count as u64 * (round_trip + 5_000);
            assert!(
                (least..most).contains(&pair.waited[0]),
                "latency {latency}: master waited {} cycles, expected {least}..{most}",
                pair.waited[0]
            );
        }
        assert_eq!(pair.waited[1], 0, "the slave never pauses");
    }
}

#[test]
fn cgb_fast_clock_and_double_speed_exchange() {
    let count = 5;
    for (prefix, sc) in [
        (&[][..], 0x83),
        (&SWITCH_SPEED[..], 0x81),
        (&SWITCH_SPEED[..], 0x83),
    ] {
        for latency in [0, 16] {
            let master = plugged(
                GbModel::Cgb,
                0x80,
                &exchange_program(prefix, sc, 0x10, count),
            );
            let slave = plugged(
                GbModel::Cgb,
                0x80,
                &exchange_program(&[], 0x80, 0x80, count),
            );
            let pair = run_exchange(master, slave, latency, count);
            assert_eq!(pair.gbs[0].cgb.double_speed, !prefix.is_empty());
        }
    }
}

#[test]
fn master_pauses_at_the_transfer_end_until_the_reply() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF01, 0x42);
    write(&mut gb, 0xFF02, 0x81);
    assert_eq!(outgoing(&mut gb), vec![transfer(0, 0x42)]);
    while !gb.link_waiting() {
        gb.step();
    }
    let pc = gb.cpu.pc;
    for _ in 0..10_000 {
        gb.step();
    }
    assert_eq!(gb.cpu.pc, pc);
    assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x00);

    gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x3C });
    assert!(!gb.link_waiting());
    assert_eq!(read(&mut gb, 0xFF01), 0x3C);
    assert_eq!(read(&mut gb, 0xFF02), 0x7F);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
}

#[test]
fn early_reply_shifts_in_on_the_normal_clock() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF01, 0x42);
    write(&mut gb, 0xFF02, 0x81);
    gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0xA5 });
    super::cgb::cycles(&mut gb, 8 * 128 - 130);
    assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
    super::cgb::cycles(&mut gb, 130);
    assert_eq!(read(&mut gb, 0xFF02), 0x7F);
    assert_eq!(read(&mut gb, 0xFF01), 0xA5);
    assert!(!gb.link_waiting());
}

#[test]
fn run_frame_returns_while_waiting_and_times_out_to_ff() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    gb.set_link_timeout_frames(3);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF02, 0x81);
    while !gb.link_waiting() {
        gb.step();
    }
    let pc = gb.cpu.pc;
    gb.run_frame();
    gb.run_frame();
    assert!(gb.link_waiting());
    assert_eq!(gb.cpu.pc, pc);
    assert!(!gb.ppu.frame_ready);
    gb.run_frame();
    assert!(!gb.link_waiting());
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);

    gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x12 });
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
}

#[test]
fn step_timeout_counts_frames_of_cycles() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    gb.set_link_timeout_frames(1);
    write(&mut gb, 0xFF02, 0x81);
    while !gb.link_waiting() {
        gb.step();
    }
    let mut steps = 0;
    while gb.link_waiting() {
        gb.step();
        steps += 1;
    }
    assert_eq!(steps, 17556);
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
}

#[test]
fn unplugged_reads_ff_and_sends_nothing() {
    let mut gb = boot(GbModel::Dmg, 0x00, &[]);
    assert!(!gb.link_plugged());
    write(&mut gb, 0xFF01, 0x55);
    write(&mut gb, 0xFF02, 0x81);
    super::cgb::cycles(&mut gb, 8 * 128);
    assert!(!gb.link_waiting());
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    assert!(gb.take_link_messages().is_empty());

    gb.receive_link_message(transfer(3, 0x10));
    assert!(gb.take_link_messages().is_empty());
}

#[test]
fn plugging_in_mid_transfer_still_reads_ff() {
    let mut gb = boot(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF02, 0x81);
    super::cgb::cycles(&mut gb, 300);
    gb.plug_link();
    super::cgb::cycles(&mut gb, 8 * 128);
    assert!(!gb.link_waiting());
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    assert!(gb.take_link_messages().is_empty());
}

#[test]
fn pulling_the_cable_mid_transfer_keeps_the_bits_already_in() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF02, 0x81);
    gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x0A });
    super::cgb::cycles(&mut gb, 4 * 128 + 10);
    gb.unplug_link();
    super::cgb::cycles(&mut gb, 4 * 128);
    assert!(!gb.link_waiting());
    assert_eq!(read(&mut gb, 0xFF01), 0x0F);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
}

#[test]
fn pulling_the_cable_while_waiting_reads_ff_at_once() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF02, 0x81);
    while !gb.link_waiting() {
        gb.step();
    }
    gb.unplug_link();
    assert!(!gb.link_waiting());
    assert_eq!(read(&mut gb, 0xFF01), 0xFF);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
    assert!(gb.take_link_messages().is_empty());
}

#[test]
fn slave_waits_forever_once_the_cable_is_pulled() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF01, 0x77);
    write(&mut gb, 0xFF02, 0x80);
    gb.unplug_link();
    gb.receive_link_message(transfer(0, 0x11));
    for _ in 0..5 {
        gb.run_frame();
    }
    assert_eq!(read(&mut gb, 0xFF02) & 0x80, 0x80);
    assert_eq!(read(&mut gb, 0xFF01), 0x77);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x00);

    gb.plug_link();
    gb.receive_link_message(transfer(1, 0x11));
    super::cgb::cycles(&mut gb, 8 * 128);
    assert_eq!(read(&mut gb, 0xFF01), 0x11);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
    assert_eq!(
        gb.take_link_messages(),
        vec![LinkMessage::Reply { id: 1, byte: 0x77 }]
    );
}

#[test]
fn slave_answers_with_its_sb_and_raises_the_interrupt() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF01, 0x99);
    write(&mut gb, 0xFF02, 0x80);
    gb.receive_link_message(transfer(7, 0x21));
    super::cgb::cycles(&mut gb, 8 * 128);
    assert_eq!(read(&mut gb, 0xFF01), 0x21);
    assert_eq!(read(&mut gb, 0xFF02), 0x7E);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x08);
    assert_eq!(
        gb.take_link_messages(),
        vec![LinkMessage::Reply { id: 7, byte: 0x99 }]
    );
}

#[test]
fn early_transfer_waits_briefly_for_the_slave_to_arm() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF01, 0x99);
    gb.receive_link_message(transfer(1, 0x21));
    super::cgb::cycles(&mut gb, 1000);
    assert!(gb.take_link_messages().is_empty());
    write(&mut gb, 0xFF02, 0x80);
    super::cgb::cycles(&mut gb, 8 * 128 - 10);
    assert!(gb.take_link_messages().is_empty());
    super::cgb::cycles(&mut gb, 10);
    assert_eq!(read(&mut gb, 0xFF01), 0x21);
    assert_eq!(
        gb.take_link_messages(),
        vec![LinkMessage::Reply { id: 1, byte: 0x99 }]
    );
}

#[test]
fn a_slave_that_arms_after_the_byte_was_due_still_takes_a_full_byte_time() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF01, 0x99);
    write(&mut gb, 0xFF02, 0x80);
    gb.receive_link_message(transfer(1, 0x10));
    super::cgb::cycles(&mut gb, 8 * 128);
    assert_eq!(
        gb.take_link_messages(),
        vec![LinkMessage::Reply { id: 1, byte: 0x99 }]
    );
    write(&mut gb, 0xFF01, 0x24);
    gb.receive_link_message(LinkMessage::Transfer {
        id: 2,
        byte: 0x4D,
        seq: 1,
        since: 0,
    });
    super::cgb::cycles(&mut gb, 2000);
    assert!(gb.take_link_messages().is_empty());
    write(&mut gb, 0xFF02, 0x80);
    super::cgb::cycles(&mut gb, 8 * 128 - 10);
    assert!(gb.take_link_messages().is_empty());
    assert_eq!(read(&mut gb, 0xFF01), 0x24);
    super::cgb::cycles(&mut gb, 10);
    assert_eq!(read(&mut gb, 0xFF01), 0x4D);
    assert_eq!(
        gb.take_link_messages(),
        vec![LinkMessage::Reply { id: 2, byte: 0x24 }]
    );
}

#[test]
fn a_slave_that_never_arms_answers_zero() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF0F, 0x00);
    write(&mut gb, 0xFF01, 0x99);
    gb.receive_link_message(transfer(2, 0x21));
    super::cgb::cycles(&mut gb, 5000);
    assert_eq!(
        gb.take_link_messages(),
        vec![LinkMessage::Reply { id: 2, byte: 0x00 }]
    );
    assert_eq!(read(&mut gb, 0xFF01), 0x99);
    assert_eq!(read(&mut gb, 0xFF0F) & 0x08, 0x00);
}

fn election_program(wait: u16) -> Vec<u8> {
    let [lo, hi] = wait.to_le_bytes();
    vec![
        0xAF, 0xEA, 0xFF, 0xC0, 0x3E, 0x02, 0xE0, 0x01, 0x3E, 0x80, 0xE0, 0x02, 0x01, lo, hi, 0xF0,
        0x02, 0xCB, 0x7F, 0x28, 0x13, 0x0B, 0x78, 0xB1, 0x20, 0xF5, 0x3E, 0x01, 0xE0, 0x01, 0x3E,
        0x81, 0xE0, 0x02, 0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA, 0xF0, 0x01, 0xEA, 0x00, 0xC0, 0x3E,
        0xAA, 0xEA, 0xFF, 0xC0,
    ]
}

#[test]
fn the_side_that_times_out_first_becomes_master() {
    for latency in [0, 16, 100, 250] {
        let mut pair = Pair::new(
            plugged(GbModel::Dmg, 0x00, &election_program(0x0100)),
            plugged(GbModel::Dmg, 0x00, &election_program(0xFFFF)),
            latency,
        );
        pair.run_until(2_000_000, Pair::both_done);
        assert_eq!(read(&mut pair.gbs[0], 0xC000), 0x02, "latency {latency}");
        assert_eq!(read(&mut pair.gbs[1], 0xC000), 0x01, "latency {latency}");
        assert_eq!(read(&mut pair.gbs[1], 0xFF02) & 0x01, 0x00);
    }
}

#[test]
fn both_masters_read_ff() {
    let count = 3;
    for latency in [0, 100] {
        let mut pair = Pair::new(
            plugged(
                GbModel::Dmg,
                0x00,
                &exchange_program(&[], 0x81, 0x10, count),
            ),
            plugged(
                GbModel::Dmg,
                0x00,
                &exchange_program(&[], 0x81, 0x80, count),
            ),
            latency,
        );
        pair.run_until(2_000_000, Pair::both_done);
        for gb in &mut pair.gbs {
            assert_eq!(received(gb, count), vec![0xFF; count as usize]);
        }
    }
}

#[test]
fn a_late_reply_after_a_restart_is_ignored() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF01, 0x01);
    write(&mut gb, 0xFF02, 0x81);
    write(&mut gb, 0xFF01, 0x02);
    write(&mut gb, 0xFF02, 0x81);
    assert_eq!(
        outgoing(&mut gb),
        vec![transfer(0, 0x01), transfer(1, 0x02),]
    );
    gb.receive_link_message(LinkMessage::Reply { id: 0, byte: 0x33 });
    while !gb.link_waiting() {
        gb.step();
    }
    gb.receive_link_message(LinkMessage::Reply { id: 1, byte: 0x44 });
    assert_eq!(read(&mut gb, 0xFF01), 0x44);
}

#[test]
fn soft_reset_keeps_the_cable_plugged() {
    let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
    write(&mut gb, 0xFF02, 0x81);
    while !gb.link_waiting() {
        gb.step();
    }
    gb.soft_reset();
    assert!(gb.link_plugged());
    assert!(!gb.link_waiting());
    assert!(gb.take_link_messages().is_empty());
}

#[cfg(feature = "persistence")]
mod snapshot {
    use super::*;
    use crate::gb::serial::Serial;

    #[test]
    fn a_snapshot_taken_while_waiting_restores_unplugged() {
        let code = exchange_program(&[], 0x81, 0x10, 1);
        let mut gb = plugged(GbModel::Dmg, 0x00, &code);
        while !gb.link_waiting() {
            gb.step();
        }
        let pc = gb.cpu.pc;
        let dump = gb.dump_full(0).expect("dump");
        let rom = super::super::cgb::rom(0x00, &code);
        let mut restored = GameBoy::from_dump(&dump, &rom, 0).expect("restore");

        assert!(!restored.link_plugged());
        assert!(!restored.link_waiting());
        assert_eq!(restored.cpu.pc, pc);
        restored.step();
        assert!(!restored.link_waiting());
        assert_eq!(read(&mut restored, 0xFF01), 0xFF);
        assert_eq!(read(&mut restored, 0xFF0F) & 0x08, 0x08);
        for _ in 0..10 {
            restored.run_frame();
        }
        assert_eq!(read(&mut restored, DONE), 0xAA);
        assert_eq!(read(&mut restored, 0xC000), 0xFF);
    }

    #[test]
    fn serial_state_round_trips() {
        let mut gb = plugged(GbModel::Dmg, 0x00, &[]);
        write(&mut gb, 0xFF01, 0x5A);
        write(&mut gb, 0xFF02, 0x81);
        super::super::cgb::cycles(&mut gb, 3 * 128 + 70);
        let bytes = rmp_serde::to_vec_named(&gb.serial).expect("encode");
        let restored: Serial = rmp_serde::from_slice(&bytes).expect("decode");
        assert_eq!(restored.sb, gb.serial.sb);
        assert_eq!(restored.read_sc(false), gb.serial.read_sc(false));
        assert!(restored.transfer_active());
        assert!(restored.plugged());
    }

    #[test]
    fn serial_state_from_before_the_link_cable_loads() {
        #[derive(serde::Serialize)]
        struct OldSerial {
            sb: u8,
            sc: u8,
            bits: u8,
            clock_high: bool,
        }
        let old = OldSerial {
            sb: 0x12,
            sc: 0x81,
            bits: 3,
            clock_high: true,
        };
        let bytes = rmp_serde::to_vec_named(&old).expect("encode");
        let serial: Serial = rmp_serde::from_slice(&bytes).expect("decode");
        assert_eq!(serial.sb, 0x12);
        assert!(serial.transfer_active());
        assert!(!serial.waiting());
        assert!(!serial.plugged());
    }
}

fn serial_send(sb: u8, sc: u8) -> [u8; 8] {
    [0x3E, sb, 0xE0, 0x01, 0x3E, sc, 0xE0, 0x02]
}

const SERIAL_WAIT: [u8; 6] = [0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA];

fn store_sb(lo: u8) -> [u8; 5] {
    [0xF0, 0x01, 0xEA, lo, 0xC0]
}

fn delay(loops: u16) -> [u8; 8] {
    let [lo, hi] = loops.to_le_bytes();
    [0x01, lo, hi, 0x0B, 0x78, 0xB1, 0x20, 0xFB]
}

fn program(parts: &[&[u8]]) -> Vec<u8> {
    let mut code = vec![0xAF, 0xEA, 0xFF, 0xC0];
    for part in parts {
        code.extend_from_slice(part);
    }
    code.extend_from_slice(&[0x3E, 0xAA, 0xEA, 0xFF, 0xC0]);
    code
}

#[test]
fn a_slave_that_turns_master_before_the_byte_lands_keeps_the_order() {
    for latency in [0, 2, 16, 50, 100, 250] {
        let initiator = program(&[
            &serial_send(0x11, 0x81),
            &SERIAL_WAIT,
            &store_sb(0x00),
            &serial_send(0x22, 0x80),
            &SERIAL_WAIT,
            &store_sb(0x01),
        ]);
        let switcher = program(&[
            &serial_send(0x33, 0x80),
            &delay(900),
            &store_sb(0x00),
            &serial_send(0x44, 0x81),
            &SERIAL_WAIT,
            &store_sb(0x01),
        ]);
        let mut pair = Pair::new(
            plugged(GbModel::Dmg, 0x00, &initiator),
            plugged(GbModel::Dmg, 0x00, &switcher),
            latency,
        );
        pair.run_until(4_000_000, Pair::both_done);
        let seen = if latency < 6 { 0x11 } else { 0x33 };
        assert_eq!(
            received(&mut pair.gbs[0], 2),
            vec![0x33, 0x44],
            "{latency} ms"
        );
        assert_eq!(
            received(&mut pair.gbs[1], 2),
            vec![seen, 0x22],
            "{latency} ms"
        );
        assert!(pair.in_flight.is_empty());
    }
}

#[test]
fn a_slave_that_arms_a_little_late_still_gets_the_byte_at_any_latency() {
    for latency in [0, 16, 100, 250] {
        let master = program(&[&serial_send(0x5A, 0x81), &SERIAL_WAIT, &store_sb(0x00)]);
        let slave = program(&[
            &delay(300),
            &serial_send(0xA5, 0x80),
            &SERIAL_WAIT,
            &store_sb(0x00),
        ]);
        let mut pair = Pair::new(
            plugged(GbModel::Dmg, 0x00, &master),
            plugged(GbModel::Dmg, 0x00, &slave),
            latency,
        );
        pair.run_until(2_000_000, Pair::both_done);
        assert_eq!(read(&mut pair.gbs[0], 0xC000), 0xA5, "{latency} ms");
        assert_eq!(read(&mut pair.gbs[1], 0xC000), 0x5A, "{latency} ms");
    }
}

#[test]
fn two_sides_both_listening_count_their_idle_frames() {
    let listener = program(&[&serial_send(0x00, 0x80), &SERIAL_WAIT, &store_sb(0x00)]);
    let mut pair = Pair::new(
        plugged(GbModel::Dmg, 0x00, &listener),
        plugged(GbModel::Dmg, 0x00, &listener),
        16,
    );
    let timeout = crate::gb::serial::DEFAULT_LINK_TIMEOUT_FRAMES;
    let end = (timeout as u64 + 10) * FRAME_CYCLES;
    while pair.played(0).min(pair.played(1)) < end {
        pair.step();
    }
    for gb in &mut pair.gbs {
        assert!(gb.link_listening_frames() > timeout);
        assert!(!gb.link_waiting());
    }
    assert_eq!(pair.sent, [0, 0]);

    write(&mut pair.gbs[0], 0xFF02, 0x81);
    assert_eq!(pair.gbs[0].link_listening_frames(), 0);
    pair.gbs[1].unplug_link();
    assert_eq!(pair.gbs[1].link_listening_frames(), 0);
}

const SNAKE: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/snake.gb");
const FRAME_CYCLES: u64 = 17_556;
const SNAKE_GO: u8 = 0x06;

fn snake() -> GameBoy {
    let rom = crate::rom::Rom::new(SNAKE);
    let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
    gb.load_rom(&rom).unwrap();
    gb
}

fn play_linked_snakes(latency_ms: u64, jitter_ms: u64) {
    use crate::gb::joypad::JoypadState;
    let timeout = crate::gb::serial::DEFAULT_LINK_TIMEOUT_FRAMES as u64 * FRAME_CYCLES;
    let mut first = snake();
    for _ in 0..60 {
        first.run_frame();
    }
    first.plug_link();
    let mut second = snake();
    second.plug_link();
    let mut pair = Pair::new(first, second, latency_ms).with_jitter(jitter_ms);
    for frame in 0..900u64 {
        let start = frame >= 60 && frame % 60 < 5;
        if start {
            pair.gbs[1].joypad.press(JoypadState::START);
        } else {
            pair.gbs[1].joypad.release(JoypadState::START);
        }
        let end = (frame + 1) * FRAME_CYCLES;
        while pair.played(1) < end {
            pair.step();
        }
    }
    let go = pair.transfers[1]
        .iter()
        .filter(|&&(_, byte)| byte == SNAKE_GO)
        .count();
    assert!(go >= 5, "{latency_ms}±{jitter_ms} ms: {go} GO bytes");
    for side in 0..2 {
        let last = pair.transfers[side].last().map_or(0, |&(played, _)| played);
        assert!(
            last >= 750 * FRAME_CYCLES,
            "{latency_ms}±{jitter_ms} ms: side {side} went quiet at frame {}",
            last / FRAME_CYCLES
        );
        assert!(
            pair.longest_wait[side] < timeout / 2,
            "{latency_ms}±{jitter_ms} ms: side {side} waited {} cycles at once",
            pair.longest_wait[side]
        );
    }
}

#[test]
fn two_linked_snakes_play_rounds_back_to_back_over_the_cable() {
    for latency in [0, 16, 50, 100, 250] {
        play_linked_snakes(latency, 0);
    }
    play_linked_snakes(100, 40);
}

const PETRIS: &[u8] = include_bytes!("../../../citrine-gb-app/homebrew/petris.gbc");
fn petris() -> GameBoy {
    let rom = crate::rom::Rom::new(PETRIS);
    let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).unwrap());
    gb.load_rom(&rom).unwrap();
    gb.plug_link();
    gb
}

fn petris_keys(side: usize, frame: u64) -> crate::gb::joypad::JoypadState {
    use crate::gb::joypad::JoypadState;
    let tap = |at: u64| (at..at + 6).contains(&frame);
    let mut keys = JoypadState::empty();
    if tap(600) || (side == 0 && tap(900)) {
        keys |= JoypadState::START;
    }
    if side == 0 && tap(700) {
        keys |= JoypadState::DOWN;
    }
    if side == 0 && tap(750) {
        keys |= JoypadState::RIGHT;
    }
    if side == 0 && tap(800) {
        keys |= JoypadState::UP;
    }
    keys
}

fn play_linked_petris(latency_ms: u64, jitter_ms: u64) {
    use crate::gb::joypad::JoypadState;
    let mut early = petris();
    for _ in 0..37 {
        early.run_frame();
    }
    let mut pair = Pair::new(early, petris(), latency_ms).with_jitter(jitter_ms);
    let all = [
        JoypadState::START,
        JoypadState::DOWN,
        JoypadState::RIGHT,
        JoypadState::UP,
    ];
    while pair.played(0).min(pair.played(1)) < 1500 * FRAME_CYCLES {
        for side in 0..2 {
            let keys = petris_keys(side, pair.played(side) / FRAME_CYCLES);
            for key in all {
                if keys.contains(key) {
                    pair.gbs[side].joypad.press(key);
                } else {
                    pair.gbs[side].joypad.release(key);
                }
            }
        }
        pair.step();
    }
    let kinds = |side: usize| -> Vec<u8> {
        let mut kinds: Vec<u8> = pair.transfers[side]
            .iter()
            .map(|&(_, byte)| byte & 0xF0)
            .collect();
        kinds.dedup();
        kinds
    };
    let (host, guest) = (kinds(0), kinds(1));
    let label = format!("{latency_ms}±{jitter_ms} ms");
    let seed = host.iter().position(|&kind| kind == 0x30);
    let start = host.iter().rposition(|&kind| kind == 0x50);
    assert!(
        seed.zip(start).is_some_and(|(seed, start)| seed < start),
        "{label}: host sent {host:x?}"
    );
    let joined = guest.iter().position(|&kind| kind == 0x40);
    assert!(
        joined.is_some_and(|joined| !guest[joined..].contains(&0x10)),
        "{label}: guest sent {guest:x?}"
    );
}

#[test]
fn two_linked_petris_start_a_versus_game() {
    for latency in [0, 16, 50, 100, 250] {
        play_linked_petris(latency, 0);
    }
    play_linked_petris(100, 40);
}
