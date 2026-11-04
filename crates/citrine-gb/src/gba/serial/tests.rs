use super::*;
use crate::gba::bus::Bus;

const SIODATA32: u32 = 0x0400_0120;
const SIOMULTI: u32 = 0x0400_0120;
const SIOCNT_AT: u32 = 0x0400_0128;
const SIODATA8: u32 = 0x0400_012A;
const RCNT_AT: u32 = 0x0400_0134;
const JOYCNT: u32 = 0x0400_0140;
const JOY_RECV: u32 = 0x0400_0150;
const JOY_TRANS: u32 = 0x0400_0154;
const JOYSTAT: u32 = 0x0400_0158;

const MASTER: u16 = INTERNAL_CLOCK | START | IRQ_ENABLE;
const SLAVE: u16 = START | IRQ_ENABLE;
const MULTIPLAY: u16 = 0x2000 | IRQ_ENABLE;

struct Hub {
    units: Vec<Bus>,
    slice: u32,
}

impl Hub {
    fn new(count: usize) -> Self {
        Self::with_slice(count, 4)
    }

    fn with_slice(count: usize, slice: u32) -> Self {
        let mut hub = Self {
            units: (0..count).map(|_| Bus::new(&[])).collect(),
            slice,
        };
        for (unit, bus) in hub.units.iter_mut().enumerate() {
            bus.plug_link(unit, count);
        }
        hub.deliver();
        hub
    }

    fn write16(&mut self, unit: usize, addr: u32, value: u16) {
        self.units[unit].write16(addr, value);
        self.deliver();
    }

    fn run(&mut self, cycles: u32) {
        let mut left = cycles;
        while left > 0 {
            let slice = left.min(self.slice);
            for unit in 0..self.units.len() {
                self.units[unit].step(slice);
                self.deliver();
            }
            left -= slice;
        }
    }

    fn deliver(&mut self) {
        loop {
            let mut any = false;
            for from in 0..self.units.len() {
                for message in self.units[from].take_link_messages() {
                    any = true;
                    for (to, bus) in self.units.iter_mut().enumerate() {
                        if to != from {
                            bus.receive_link_message(message);
                        }
                    }
                }
            }
            if !any {
                return;
            }
        }
    }
}

fn alone() -> Bus {
    Bus::new(&[])
}

fn serial_irq(bus: &Bus) -> bool {
    bus.interrupts().raised() & Irq::SERIAL.bits() != 0
}

fn started(bus: &Bus) -> bool {
    bus.read16(SIOCNT_AT) & START != 0
}

#[test]
fn a_master_with_no_cable_shifts_in_ones_and_raises_the_irq_after_eight_bits() {
    let mut bus = alone();
    bus.write16(SIODATA8, 0x005A);
    bus.write16(SIOCNT_AT, MASTER);
    bus.step(511 + START_DELAY);
    assert!(started(&bus));
    assert!(!serial_irq(&bus));
    bus.step(1);
    assert!(!started(&bus));
    assert!(serial_irq(&bus));
    assert_eq!(bus.read16(SIODATA8), 0x00FF);
}

#[test]
fn normal_transfers_take_eight_or_sixty_four_cycles_a_bit_after_the_start_delay() {
    let cases = [
        (0, 512),
        (FAST_CLOCK, 64),
        (LONG, 2048),
        (LONG | FAST_CLOCK, 256),
    ];
    for (bits, cycles) in cases {
        let mut bus = alone();
        bus.write16(SIOCNT_AT, bits);
        bus.write16(SIOCNT_AT, MASTER | bits);
        bus.step(START_DELAY + cycles - 1);
        assert!(started(&bus), "{bits:#06x} ended early");
        bus.step(1);
        assert!(
            !started(&bus),
            "{bits:#06x} did not end at {}",
            START_DELAY + cycles
        );
    }
}

#[test]
fn the_irq_needs_the_enable_bit() {
    let mut bus = alone();
    bus.write16(SIOCNT_AT, INTERNAL_CLOCK | START);
    bus.step(512 + START_DELAY);
    assert!(!started(&bus));
    assert!(!serial_irq(&bus));
}

#[test]
fn eight_bit_normal_transfers_swap_bytes_both_ways() {
    let mut hub = Hub::new(2);
    hub.write16(1, SIODATA8, 0x003C);
    hub.write16(1, SIOCNT_AT, SLAVE);
    hub.write16(0, SIODATA8, 0x00A5);
    hub.write16(0, SIOCNT_AT, MASTER);
    hub.run(520 + START_DELAY);
    for bus in &hub.units {
        assert!(!started(bus));
        assert!(serial_irq(bus));
    }
    assert_eq!(hub.units[0].read16(SIODATA8), 0x003C);
    assert_eq!(hub.units[1].read16(SIODATA8), 0x00A5);
}

#[test]
fn thirty_two_bit_normal_transfers_swap_words_both_ways() {
    let mut hub = Hub::new(2);
    for unit in 0..2 {
        hub.write16(unit, SIOCNT_AT, LONG);
    }
    hub.units[0].write32(SIODATA32, 0x1234_5678);
    hub.units[1].write32(SIODATA32, 0x9ABC_DEF0);
    hub.write16(1, SIOCNT_AT, SLAVE | LONG);
    hub.write16(0, SIOCNT_AT, MASTER | LONG);
    hub.run(2056 + START_DELAY);
    assert_eq!(hub.units[0].read32(SIODATA32), 0x9ABC_DEF0);
    assert_eq!(hub.units[1].read32(SIODATA32), 0x1234_5678);
    assert!(hub.units.iter().all(serial_irq));
}

#[test]
fn the_slave_ends_on_the_master_clock_not_its_own() {
    let mut hub = Hub::with_slice(2, 1);
    hub.write16(1, SIOCNT_AT, SLAVE | FAST_CLOCK);
    hub.write16(0, SIOCNT_AT, MASTER);
    hub.run(510 + START_DELAY);
    assert!(started(&hub.units[1]));
    hub.run(4);
    assert!(!started(&hub.units[1]));
}

#[test]
fn an_unarmed_slave_answers_with_its_idle_so_level() {
    for (so, expected) in [(0, 0x0000), (SO, 0x00FF)] {
        let mut hub = Hub::new(2);
        hub.write16(1, SIODATA8, 0x0042);
        hub.write16(1, SIOCNT_AT, so);
        hub.write16(0, SIOCNT_AT, MASTER);
        hub.run(520 + START_DELAY);
        assert_eq!(hub.units[0].read16(SIODATA8), expected);
        assert_eq!(hub.units[1].read16(SIODATA8), 0x0042);
        assert!(!serial_irq(&hub.units[1]));
    }
}

#[test]
fn si_reads_the_other_units_so_and_high_with_no_cable() {
    assert_ne!(alone().read16(SIOCNT_AT) & SI, 0);
    let mut hub = Hub::new(2);
    hub.write16(1, SIOCNT_AT, SO);
    assert_ne!(hub.units[0].read16(SIOCNT_AT) & SI, 0);
    hub.write16(1, SIOCNT_AT, START);
    assert_eq!(hub.units[0].read16(SIOCNT_AT) & SI, 0);
    assert_eq!(hub.units[1].read16(SIOCNT_AT) & SI, 0);
    hub.write16(0, SIOCNT_AT, SO);
    assert_ne!(hub.units[1].read16(SIOCNT_AT) & SI, 0);
}

#[test]
fn clearing_the_start_bit_stops_a_transfer() {
    let mut bus = alone();
    bus.write16(SIOCNT_AT, MASTER);
    bus.step(100);
    bus.write16(SIOCNT_AT, MASTER & !START);
    bus.step(1000);
    assert!(!serial_irq(&bus));
    assert!(!bus.serial().active());
}

#[test]
fn a_master_waits_for_the_answer_and_takes_ones_when_the_cable_goes() {
    let mut bus = alone();
    bus.plug_link(0, 2);
    bus.write16(SIODATA8, 0x0012);
    bus.write16(SIOCNT_AT, MASTER);
    bus.step(600);
    assert!(bus.serial().waiting());
    assert!(started(&bus));
    bus.unplug_link();
    assert!(!started(&bus));
    assert!(serial_irq(&bus));
    assert_eq!(bus.read16(SIODATA8), 0x00FF);
}

fn multiplay(units: usize, connected: usize, baud: u16) -> Hub {
    let mut hub = Hub::new(units);
    for unit in 0..connected {
        hub.write16(unit, SIOCNT_AT, MULTIPLAY | baud);
        hub.write16(unit, SIODATA8, 0x1111 * (unit as u16 + 1));
    }
    hub
}

fn multi_words(bus: &Bus) -> [u16; 4] {
    [0, 2, 4, 6].map(|offset| bus.read16(SIOMULTI + offset))
}

#[test]
fn multiplay_parent_and_children_read_their_terminals() {
    let hub = multiplay(3, 3, 0);
    let siocnt: Vec<u16> = hub.units.iter().map(|bus| bus.read16(SIOCNT_AT)).collect();
    assert_eq!(siocnt[0] & SI, 0);
    assert_ne!(siocnt[1] & SI, 0);
    assert_ne!(siocnt[2] & SI, 0);
    assert!(siocnt.iter().all(|value| value & SD != 0));
}

#[test]
fn sd_reads_low_until_every_unit_is_in_multiplay() {
    let mut hub = Hub::new(2);
    hub.write16(0, SIOCNT_AT, MULTIPLAY);
    assert_eq!(hub.units[0].read16(SIOCNT_AT) & SD, 0);
    hub.write16(1, SIOCNT_AT, MULTIPLAY);
    assert_ne!(hub.units[0].read16(SIOCNT_AT) & SD, 0);
    assert_eq!(alone().read16(SIOCNT_AT) & SD, 0);
}

#[test]
fn multiplay_fills_every_slot_on_every_unit_and_missing_units_read_ffff() {
    let expected = [
        (2, [0x1111, 0x2222, 0xFFFF, 0xFFFF]),
        (3, [0x1111, 0x2222, 0x3333, 0xFFFF]),
        (4, [0x1111, 0x2222, 0x3333, 0x4444]),
    ];
    for (units, words) in expected {
        let mut hub = multiplay(units, units, 3);
        hub.write16(0, SIOCNT_AT, MULTIPLAY | 3 | START);
        assert!(hub.units.iter().all(started), "{units} units not busy");
        assert!(hub.units.iter().all(|bus| multi_words(bus) == [0xFFFF; 4]));
        hub.run(12_000);
        for (unit, bus) in hub.units.iter().enumerate() {
            let siocnt = bus.read16(SIOCNT_AT);
            assert_eq!(multi_words(bus), words, "{units} units, unit {unit}");
            assert_eq!(siocnt & START, 0);
            assert_eq!((siocnt >> ID_SHIFT) & 0b11, unit as u16);
            assert_eq!(siocnt & ERROR, 0);
            assert!(serial_irq(bus));
        }
    }
}

#[test]
fn a_child_out_of_multiplay_reads_ffff_and_sets_the_error_bit_behind_it() {
    let mut hub = multiplay(3, 3, 3);
    hub.write16(1, SIOCNT_AT, 0);
    hub.write16(0, SIOCNT_AT, MULTIPLAY | 3 | START);
    hub.run(12_000);
    assert_eq!(multi_words(&hub.units[0]), [0x1111, 0xFFFF, 0x3333, 0xFFFF]);
    assert_eq!(multi_words(&hub.units[2]), [0x1111, 0xFFFF, 0x3333, 0xFFFF]);
    assert_eq!(hub.units[0].read16(SIOCNT_AT) & ERROR, 0);
    assert_ne!(hub.units[2].read16(SIOCNT_AT) & ERROR, 0);
    assert!(!serial_irq(&hub.units[1]));
}

#[test]
fn multiplay_takes_eighteen_bits_a_unit_at_the_baud_rate() {
    let cases = [
        (2, 0, 63_426),
        (2, 1, 16_240),
        (2, 2, 10_997),
        (2, 3, 5_754),
        (3, 3, 8_376),
        (4, 3, 10_485),
    ];
    for (units, baud, cycles) in cases {
        let mut hub = Hub::with_slice(units, 1);
        for unit in 0..units {
            hub.write16(unit, SIOCNT_AT, MULTIPLAY | baud);
        }
        hub.write16(0, SIOCNT_AT, MULTIPLAY | baud | START);
        hub.run(cycles - 1);
        assert!(
            started(&hub.units[0]),
            "{units} units at baud {baud} ended early"
        );
        hub.run(1);
        assert!(
            !started(&hub.units[0]),
            "{units} units at baud {baud} did not end at {cycles}"
        );
    }
}

#[test]
fn only_a_plugged_parent_can_start_multiplay() {
    let mut bus = alone();
    bus.write16(SIOCNT_AT, MULTIPLAY | START);
    assert!(!started(&bus));
    let mut hub = multiplay(2, 2, 3);
    hub.write16(1, SIOCNT_AT, MULTIPLAY | 3 | START);
    assert!(!hub.units.iter().any(started));
}

#[test]
fn rcnt_general_purpose_keeps_its_pins_and_drops_bits_nine_to_thirteen() {
    let mut bus = alone();
    bus.write16(RCNT_AT, 0xBFFF);
    assert_eq!(bus.read16(RCNT_AT), 0x81FF);
    bus.write16(RCNT_AT, 0x8005);
    assert_eq!(bus.read16(RCNT_AT), 0x8005);
}

#[test]
fn joy_bus_and_uart_registers_are_plain_storage() {
    let mut bus = alone();
    bus.write16(RCNT_AT, 0xC000);
    bus.write16(JOYCNT, 0x0047);
    bus.write32(JOY_RECV, 0x0102_0304);
    bus.write32(JOY_TRANS, 0x0506_0708);
    bus.write16(JOYSTAT, 0x0030);
    assert_eq!(bus.read16(JOYCNT), 0x0047);
    assert_eq!(bus.read32(JOY_RECV), 0x0102_0304);
    assert_eq!(bus.read32(JOY_TRANS), 0x0506_0708);
    assert_eq!(bus.read16(JOYSTAT), 0x0030);
    assert_eq!(bus.read16(RCNT_AT), 0xC000);
    bus.write16(RCNT_AT, 0);
    bus.write16(SIOCNT_AT, 0x7F83);
    assert_eq!(bus.read16(SIOCNT_AT), 0x7F83);
    bus.step(100_000);
    assert!(!serial_irq(&bus));
}
