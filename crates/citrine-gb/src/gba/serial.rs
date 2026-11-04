use super::interrupt::Irq;

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
            self.outgoing()
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
mod tests;
