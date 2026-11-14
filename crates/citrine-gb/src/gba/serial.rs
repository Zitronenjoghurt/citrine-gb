use super::interrupt::Irq;
use crate::traffic::{Entry, Kind, Traffic};

pub const MAX_UNITS: usize = 4;
pub const REGISTERS_LEN: usize = 0x0C;

const MULTI_FIRST: usize = 0x0;
const MULTI_LAST: usize = 0x6;
const SIOCNT: usize = 0x8;
const SEND: usize = 0xA;

const INTERNAL_CLOCK: u16 = 1 << 0;
const FAST_CLOCK: u16 = 1 << 1;
const BAUD: u16 = 0b11;
const SI: u16 = 1 << 2;
const SO: u16 = 1 << 3;
const SD: u16 = 1 << 3;
const ID_SHIFT: u16 = 4;
const ERROR: u16 = 1 << 6;
const START: u16 = 1 << 7;
const LONG: u16 = 1 << 12;
const IRQ_ENABLE: u16 = 1 << 14;
const NORMAL_READ_ONLY: u16 = SI | 0x0070 | 0x8000;
const MULTI_READ_ONLY: u16 = SI | SD | 0x0030 | ERROR | START | 0x8000;
const OTHER_READ_ONLY: u16 = 0x8000;
const MODE_BITS: u16 = 0x3000;
const MODE_SHIFT: u16 = 12;

const RCNT_MASK: u16 = 0xC1FF;
const RCNT_PINS: u16 = 0x000F;
const RCNT_SPECIAL: u16 = 1 << 15;
const RCNT_JOY: u16 = 1 << 14;
const PIN_SC: u16 = 1 << 0;
const PIN_SD: u16 = 1 << 1;
const PIN_SI: u16 = 1 << 2;
const PIN_SO: u16 = 1 << 3;

const SLOW_BIT_CYCLES: u32 = 64;
const FAST_BIT_CYCLES: u32 = 8;
const START_DELAY: u32 = 6;
const CLOCK_HZ: u64 = 16_777_216;
const BAUDS: [u64; 4] = [9600, 38400, 57600, 115_200];
const UNIT_BITS: u64 = 18;
const MULTI_TIMEOUT: u32 = 512;
pub const CABLE_WINDOW: u32 = (CLOCK_HZ / BAUDS[3]) as u32;
const MISSING: u16 = 0xFFFF;
const SHIFTED_IN_ONES: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal8,
    Normal32,
    Multiplay,
    Uart,
    GeneralPurpose,
    JoyBus,
}

impl Mode {
    fn of(control: u16, rcnt: u16) -> Self {
        match (rcnt & RCNT_SPECIAL != 0, rcnt & RCNT_JOY != 0) {
            (true, false) => Self::GeneralPurpose,
            (true, true) => Self::JoyBus,
            (false, _) => match (control & MODE_BITS) >> MODE_SHIFT {
                0 => Self::Normal8,
                1 => Self::Normal32,
                2 => Self::Multiplay,
                _ => Self::Uart,
            },
        }
    }

    fn normal(self) -> bool {
        matches!(self, Self::Normal8 | Self::Normal32)
    }

    fn read_only(self) -> u16 {
        match self {
            Self::Normal8 | Self::Normal32 => NORMAL_READ_ONLY,
            Self::Multiplay => MULTI_READ_ONLY,
            _ => OTHER_READ_ONLY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LinkMessage {
    Lines { from: u8, so: bool, sd: bool },
    Normal { from: u8, data: u32, cycles: u32 },
    NormalReply { from: u8, data: u32 },
    Multi { data: u16, cycles: u32 },
    MultiReply { from: u8, data: Option<u16> },
}

impl LinkMessage {
    pub fn is_transfer(&self) -> bool {
        matches!(self, Self::Normal { .. } | Self::Multi { .. })
    }

    pub fn aged(self, late: u64) -> Self {
        match self {
            Self::Multi { data, cycles } => Self::Multi {
                data,
                cycles: cycles.saturating_sub(u32::try_from(late).unwrap_or(u32::MAX)),
            },
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Lines {
    so: bool,
    sd: bool,
}

impl Default for Lines {
    fn default() -> Self {
        Self {
            so: true,
            sd: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Link {
    unit: u8,
    units: u8,
    remote: [Lines; MAX_UNITS],
    sent: Lines,
}

impl Link {
    fn opponent(&self) -> u8 {
        self.unit ^ 1
    }

    fn everyone(&self) -> u8 {
        ((1u16 << self.units) - 1) as u8
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Transfer {
    #[default]
    Idle,
    Normal {
        cycles_left: u32,
        master: bool,
        incoming: Option<u32>,
    },
    Multi {
        cycles_left: u32,
        words: [u16; MAX_UNITS],
        answered: u8,
        sent: u8,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Serial {
    multi: [u16; MAX_UNITS],
    control: u16,
    send: u16,
    rcnt: u16,
    id: u8,
    error: bool,
    transfer: Transfer,
    link: Option<Link>,
    outbox: Vec<LinkMessage>,
    #[cfg_attr(feature = "serde", serde(skip))]
    outgoing: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    traffic: Traffic,
}

impl Serial {
    pub fn mode(&self) -> Mode {
        Mode::of(self.control, self.rcnt)
    }

    pub fn read(&self, offset: usize) -> u16 {
        match offset {
            MULTI_FIRST..=MULTI_LAST => self.multi[offset / 2],
            SIOCNT => self.siocnt(),
            SEND => self.send,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: usize, value: u16, mask: u16) {
        self.write_at(offset, value, mask, 0);
    }

    pub fn write_at(&mut self, offset: usize, value: u16, mask: u16, at: u32) {
        match offset {
            MULTI_FIRST..=MULTI_LAST => merge(&mut self.multi[offset / 2], value, mask),
            SIOCNT => self.write_siocnt(value, mask, at),
            SEND => merge(&mut self.send, value, mask),
            _ => {}
        }
    }

    pub fn rcnt(&self) -> u16 {
        match self.mode() {
            Mode::GeneralPurpose => self.rcnt,
            Mode::JoyBus => self.rcnt & !RCNT_PINS,
            _ => self.rcnt & !RCNT_PINS | self.pins(),
        }
    }

    pub fn write_rcnt(&mut self, value: u16, mask: u16) {
        merge(&mut self.rcnt, value & RCNT_MASK, mask);
        self.send_lines();
    }

    pub fn step(&mut self, cycles: u32) -> Irq {
        self.step_at(cycles).0
    }

    pub fn step_at(&mut self, cycles: u32) -> (Irq, u32) {
        let at = match &mut self.transfer {
            Transfer::Idle => return (Irq::empty(), 0),
            Transfer::Normal { cycles_left, .. } | Transfer::Multi { cycles_left, .. } => {
                let left = *cycles_left;
                *cycles_left = left.saturating_sub(cycles);
                left.min(cycles)
            }
        };
        (self.try_finish(), at)
    }

    pub fn until_done(&self) -> Option<u32> {
        match self.transfer {
            Transfer::Idle => None,
            Transfer::Normal { cycles_left, .. } | Transfer::Multi { cycles_left, .. } => {
                (cycles_left > 0).then_some(cycles_left)
            }
        }
    }

    pub fn active(&self) -> bool {
        self.transfer != Transfer::Idle
    }

    pub fn plugged(&self) -> bool {
        self.link.is_some()
    }

    pub fn waiting(&self) -> bool {
        match self.transfer {
            Transfer::Idle => false,
            Transfer::Normal {
                cycles_left,
                incoming,
                ..
            } => cycles_left == 0 && incoming.is_none(),
            Transfer::Multi {
                cycles_left,
                answered,
                ..
            } => cycles_left == 0 && !self.all_answered(answered),
        }
    }

    pub fn plug(&mut self, unit: usize, units: usize) {
        let units = units.clamp(1, MAX_UNITS);
        let lines = self.lines();
        self.link = Some(Link {
            unit: unit.min(units - 1) as u8,
            units: units as u8,
            remote: [Lines::default(); MAX_UNITS],
            sent: lines,
        });
        self.push(self.lines_message(lines));
    }

    pub fn unplug(&mut self) -> Irq {
        self.link = None;
        self.outbox.clear();
        match &mut self.transfer {
            Transfer::Idle => {}
            Transfer::Normal { incoming, .. } => {
                incoming.get_or_insert(SHIFTED_IN_ONES);
            }
            Transfer::Multi { answered, .. } => *answered = u8::MAX,
        }
        self.try_finish()
    }

    pub fn traffic(&self) -> &Traffic {
        &self.traffic
    }

    pub fn traffic_mut(&mut self) -> &mut Traffic {
        &mut self.traffic
    }

    pub fn copy_keeping_traffic(&mut self, source: &Serial) {
        let traffic = std::mem::take(&mut self.traffic);
        self.clone_from(source);
        self.traffic = traffic;
    }

    pub fn take_messages(&mut self) -> Vec<LinkMessage> {
        std::mem::take(&mut self.outbox)
    }

    pub fn receive(&mut self, message: LinkMessage) -> Irq {
        let Some(link) = self.link else {
            return Irq::empty();
        };
        match message {
            LinkMessage::Lines { from, so, sd } => {
                if let Some(lines) = self
                    .link
                    .as_mut()
                    .and_then(|link| link.remote.get_mut(usize::from(from)))
                {
                    *lines = Lines { so, sd };
                }
                Irq::empty()
            }
            LinkMessage::Normal { from, data, cycles } => {
                if from == link.opponent() {
                    let reply = self.answer_normal(data, cycles);
                    self.push(LinkMessage::NormalReply {
                        from: link.unit,
                        data: reply,
                    });
                }
                Irq::empty()
            }
            LinkMessage::NormalReply { from, data } => {
                if from == link.opponent()
                    && let Transfer::Normal {
                        master: true,
                        incoming,
                        ..
                    } = &mut self.transfer
                {
                    incoming.get_or_insert(data);
                }
                self.try_finish()
            }
            LinkMessage::Multi { data, cycles } => {
                if link.unit != 0 {
                    let reply = self.answer_multi(link.unit, data, cycles);
                    self.push(LinkMessage::MultiReply {
                        from: link.unit,
                        data: reply,
                    });
                }
                Irq::empty()
            }
            LinkMessage::MultiReply { from, data } => {
                if let Transfer::Multi {
                    words,
                    answered,
                    sent,
                    ..
                } = &mut self.transfer
                    && usize::from(from) < MAX_UNITS
                {
                    *answered |= 1 << from;
                    if let Some(word) = data {
                        words[usize::from(from)] = word;
                        *sent |= 1 << from;
                    }
                }
                self.try_finish()
            }
        }
    }

    fn siocnt(&self) -> u16 {
        let mode = self.mode();
        let stored = self.control & !mode.read_only();
        match mode {
            Mode::Normal8 | Mode::Normal32 => stored | flag(self.si(), SI),
            Mode::Multiplay => {
                stored
                    | flag(self.si(), SI)
                    | flag(self.sd(), SD)
                    | u16::from(self.id) << ID_SHIFT
                    | flag(self.error, ERROR)
                    | flag(self.active(), START)
            }
            _ => stored,
        }
    }

    fn write_siocnt(&mut self, value: u16, mask: u16, at: u32) {
        let was_started = self.control & START != 0;
        let mut control = self.control;
        merge(&mut control, value, mask);
        let mode = Mode::of(control, self.rcnt);
        self.control = control & !mode.read_only();
        self.traffic
            .push(Entry::new(Kind::SerialControl).sent(&self.control.to_le_bytes()));
        let started = self.control & START != 0;
        if mode.normal() {
            match (was_started, started) {
                (false, true) if self.control & INTERNAL_CLOCK != 0 => self.begin_normal(at),
                (true, false) => self.transfer = Transfer::Idle,
                _ => {}
            }
        } else if mode == Mode::Multiplay && mask & value & START != 0 {
            self.begin_multi();
        }
        self.send_lines();
    }

    fn begin_normal(&mut self, at: u32) {
        let cycles = self.normal_bits() * self.bit_cycles() + START_DELAY;
        let data = self.outgoing();
        self.outgoing = data;
        let incoming = match self.link {
            Some(link) => {
                self.push(LinkMessage::Normal {
                    from: link.unit,
                    data,
                    cycles,
                });
                None
            }
            None => Some(SHIFTED_IN_ONES),
        };
        self.transfer = Transfer::Normal {
            cycles_left: cycles + at,
            master: true,
            incoming,
        };
    }

    fn answer_normal(&mut self, data: u32, cycles: u32) -> u32 {
        let mode = self.mode();
        let armed = mode.normal()
            && self.control & START != 0
            && self.control & INTERNAL_CLOCK == 0
            && self.transfer == Transfer::Idle;
        if armed {
            self.transfer = Transfer::Normal {
                cycles_left: cycles,
                master: false,
                incoming: Some(data),
            };
            self.outgoing = self.outgoing();
            self.outgoing
        } else if mode.normal() && self.control & START == 0 && self.control & SO == 0 {
            0
        } else {
            SHIFTED_IN_ONES
        }
    }

    fn begin_multi(&mut self) {
        let Some(link) = self.link.filter(|link| link.unit == 0) else {
            return;
        };
        if self.active() {
            return;
        }
        let baud = BAUDS[usize::from(self.control & BAUD)];
        let mut cycles = (UNIT_BITS * u64::from(link.units) * CLOCK_HZ / baud) as u32;
        if usize::from(link.units) < MAX_UNITS {
            cycles += MULTI_TIMEOUT;
        }
        self.start_multi(0, self.send, cycles);
        self.push(LinkMessage::Multi {
            data: self.send,
            cycles,
        });
    }

    fn answer_multi(&mut self, unit: u8, data: u16, cycles: u32) -> Option<u16> {
        if self.mode() != Mode::Multiplay {
            return None;
        }
        self.start_multi(unit, data, cycles);
        Some(self.send)
    }

    fn start_multi(&mut self, unit: u8, parent: u16, cycles: u32) {
        self.multi = [MISSING; MAX_UNITS];
        let mut words = [MISSING; MAX_UNITS];
        words[0] = parent;
        words[usize::from(unit)] = self.send;
        let mask = 1 | 1 << unit;
        self.transfer = Transfer::Multi {
            cycles_left: cycles,
            words,
            answered: mask,
            sent: mask,
        };
    }

    fn try_finish(&mut self) -> Irq {
        match self.transfer {
            Transfer::Normal {
                cycles_left: 0,
                incoming: Some(data),
                ..
            } => {
                self.traffic.push(
                    Entry::new(Kind::SerialDone)
                        .sent(&self.outgoing.to_le_bytes())
                        .got(&data.to_le_bytes()),
                );
                if self.mode() == Mode::Normal32 {
                    self.multi[0] = data as u16;
                    self.multi[1] = (data >> 16) as u16;
                } else {
                    self.send = self.send & 0xFF00 | data as u16 & 0x00FF;
                }
                self.control &= !START;
            }
            Transfer::Multi {
                cycles_left: 0,
                words,
                answered,
                sent,
            } if self.all_answered(answered) => {
                let unit = self.link.map_or(0, |link| link.unit);
                let received: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
                self.traffic.push(
                    Entry::new(Kind::SerialDone)
                        .tag(1)
                        .sent(&self.send.to_le_bytes())
                        .got(&received),
                );
                self.multi = words;
                self.id = unit;
                self.error = (1..unit).any(|earlier| sent & 1 << earlier == 0);
            }
            _ => return Irq::empty(),
        }
        self.transfer = Transfer::Idle;
        self.send_lines();
        if self.control & IRQ_ENABLE != 0 {
            Irq::SERIAL
        } else {
            Irq::empty()
        }
    }

    fn all_answered(&self, answered: u8) -> bool {
        self.link
            .is_none_or(|link| answered & link.everyone() == link.everyone())
    }

    fn normal_bits(&self) -> u32 {
        if self.control & LONG != 0 { 32 } else { 8 }
    }

    fn bit_cycles(&self) -> u32 {
        if self.control & FAST_CLOCK != 0 {
            FAST_BIT_CYCLES
        } else {
            SLOW_BIT_CYCLES
        }
    }

    fn outgoing(&self) -> u32 {
        if self.mode() == Mode::Normal32 {
            u32::from(self.multi[0]) | u32::from(self.multi[1]) << 16
        } else {
            u32::from(self.send & 0x00FF)
        }
    }

    fn si(&self) -> bool {
        let Some(link) = self.link else {
            return true;
        };
        match self.mode() {
            Mode::Multiplay => link.unit != 0,
            _ => link
                .remote
                .get(usize::from(link.opponent()))
                .is_none_or(|lines| lines.so),
        }
    }

    fn sd(&self) -> bool {
        self.link.is_some_and(|link| {
            (0..link.units)
                .filter(|&unit| unit != link.unit)
                .all(|unit| link.remote[usize::from(unit)].sd)
        })
    }

    fn pins(&self) -> u16 {
        let sd = if self.mode() == Mode::Multiplay {
            self.sd()
        } else {
            true
        };
        flag(!self.active(), PIN_SC)
            | flag(sd, PIN_SD)
            | flag(self.si(), PIN_SI)
            | flag(self.lines().so, PIN_SO)
    }

    fn lines(&self) -> Lines {
        let mode = self.mode();
        Lines {
            so: !mode.normal() || self.control & SO != 0,
            sd: mode == Mode::Multiplay,
        }
    }

    fn lines_message(&self, lines: Lines) -> LinkMessage {
        LinkMessage::Lines {
            from: self.link.map_or(0, |link| link.unit),
            so: lines.so,
            sd: lines.sd,
        }
    }

    fn send_lines(&mut self) {
        let lines = self.lines();
        if let Some(link) = &mut self.link
            && link.sent != lines
        {
            link.sent = lines;
            self.push(self.lines_message(lines));
        }
    }

    fn push(&mut self, message: LinkMessage) {
        if self.link.is_some() {
            self.outbox.push(message);
        }
    }
}

fn merge(register: &mut u16, value: u16, mask: u16) {
    *register = *register & !mask | value & mask;
}

fn flag(on: bool, bit: u16) -> u16 {
    if on { bit } else { 0 }
}

#[cfg(test)]
mod tests {
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
}
