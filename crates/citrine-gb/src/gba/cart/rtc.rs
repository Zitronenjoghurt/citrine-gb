use super::date::{self, CENTURY, DateTime};

const CYCLES_PER_SECOND: u32 = 16_777_216;
const SCK: u8 = 1 << 0;
const SIO: u8 = 1 << 1;
const CS: u8 = 1 << 2;
const COMMAND_CODE: u8 = 0x6;
const STATUS_WRITABLE: u8 = 0x6A;
const STATUS_24H: u8 = 0x40;
const STATUS_POWER_OFF: u8 = 0x80;
const PM: u8 = 0x80;
const UNUSED: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Register {
    Reset,
    Status,
    DateTime,
    Time,
    ForceIrq,
    Unused,
}

impl Register {
    fn of(command: u8) -> Self {
        match (command >> 1) & 7 {
            0 => Self::Reset,
            1 => Self::Status,
            2 => Self::DateTime,
            3 => Self::Time,
            6 => Self::ForceIrq,
            _ => Self::Unused,
        }
    }

    fn len(self) -> usize {
        match self {
            Self::Reset | Self::ForceIrq => 0,
            Self::DateTime => 7,
            Self::Time => 3,
            Self::Status | Self::Unused => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Phase {
    Idle,
    Command,
    Write(Register),
    Read,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rtc {
    seconds: u64,
    cycles: u32,
    status: u8,
    weekday_shift: u8,
    phase: Phase,
    selected: bool,
    sck: bool,
    shift: u8,
    bits: u8,
    buffer: [u8; 7],
    index: usize,
    len: usize,
    sio: bool,
}

impl Default for Rtc {
    fn default() -> Self {
        Self {
            seconds: 0,
            cycles: 0,
            status: STATUS_24H,
            weekday_shift: 0,
            phase: Phase::Idle,
            selected: false,
            sck: false,
            shift: 0,
            bits: 0,
            buffer: [0; 7],
            index: 0,
            len: 0,
            sio: false,
        }
    }
}

impl Rtc {
    pub fn date_time(&self) -> DateTime {
        DateTime::from_seconds(self.seconds)
    }

    pub fn weekday(&self) -> u8 {
        (date::weekday(self.seconds) + self.weekday_shift) % 7
    }

    pub fn set_unix(&mut self, unix: i64) {
        self.seconds = date::from_unix(unix);
        self.cycles = 0;
        self.weekday_shift = 0;
    }

    pub fn tick(&mut self, cycles: u32) {
        self.cycles += cycles;
        while self.cycles >= CYCLES_PER_SECOND {
            self.cycles -= CYCLES_PER_SECOND;
            self.seconds = (self.seconds + 1) % CENTURY;
        }
    }

    pub fn output(&self) -> u8 {
        if self.sio { SIO } else { 0 }
    }

    pub fn update(&mut self, pins: u8) {
        let sck = pins & SCK != 0;
        let rising = sck && !self.sck;
        let falling = !sck && self.sck;
        self.sck = sck;
        if pins & CS == 0 {
            self.selected = false;
            self.phase = Phase::Idle;
            return;
        }
        if !self.selected {
            self.selected = true;
            self.phase = Phase::Command;
            self.shift = 0;
            self.bits = 0;
            return;
        }
        let bit = u8::from(pins & SIO != 0);
        match self.phase {
            Phase::Command if rising => {
                self.shift = (self.shift << 1) | bit;
                self.bits += 1;
                if self.bits == 8 {
                    self.start(self.shift);
                }
            }
            Phase::Write(register) if rising => {
                self.shift |= bit << self.bits;
                self.bits += 1;
                if self.bits == 8 {
                    self.buffer[self.index] = self.shift;
                    self.index += 1;
                    self.shift = 0;
                    self.bits = 0;
                    if self.index == self.len {
                        self.apply(register);
                        self.phase = Phase::Idle;
                    }
                }
            }
            Phase::Read if falling => {
                let byte = self.buffer.get(self.index).copied().unwrap_or(UNUSED);
                self.sio = (byte >> self.bits) & 1 != 0;
                self.bits += 1;
                if self.bits == 8 {
                    self.bits = 0;
                    self.index = (self.index + 1).min(self.len);
                }
            }
            _ => {}
        }
    }

    fn start(&mut self, command: u8) {
        self.shift = 0;
        self.bits = 0;
        self.index = 0;
        self.phase = Phase::Idle;
        if command >> 4 != COMMAND_CODE {
            return;
        }
        let register = Register::of(command);
        self.len = register.len();
        match register {
            Register::Reset => self.reset(),
            Register::ForceIrq => {}
            _ if command & 1 != 0 => {
                self.fill(register);
                self.phase = Phase::Read;
            }
            _ => self.phase = Phase::Write(register),
        }
    }

    fn fill(&mut self, register: Register) {
        match register {
            Register::Status => {
                self.buffer[0] = self.status;
                self.status &= !STATUS_POWER_OFF;
            }
            Register::DateTime => self.buffer = self.date_time_bytes(),
            Register::Time => {
                let bytes = self.date_time_bytes();
                self.buffer[..3].copy_from_slice(&bytes[4..]);
            }
            _ => self.buffer[0] = UNUSED,
        }
    }

    fn apply(&mut self, register: Register) {
        match register {
            Register::Status => {
                self.status = (self.buffer[0] & STATUS_WRITABLE) | (self.status & STATUS_POWER_OFF)
            }
            Register::DateTime => {
                let bytes = self.buffer;
                self.set_date_time(bytes);
            }
            Register::Time => {
                let mut bytes = self.date_time_bytes();
                bytes[4..].copy_from_slice(&self.buffer[..3]);
                let weekday = self.weekday();
                bytes[3] = weekday;
                self.set_date_time(bytes);
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.set_date_time([0, 1, 1, 0, 0, 0, 0]);
        self.status = 0;
    }

    fn date_time_bytes(&self) -> [u8; 7] {
        let now = self.date_time();
        let hour = if self.status & STATUS_24H != 0 {
            now.hour
        } else {
            now.hour % 12
        };
        let pm = if now.hour >= 12 { PM } else { 0 };
        [
            date::to_bcd(now.year),
            date::to_bcd(now.month),
            date::to_bcd(now.day),
            self.weekday(),
            date::to_bcd(hour) | pm,
            date::to_bcd(now.minute),
            date::to_bcd(now.second),
        ]
    }

    fn set_date_time(&mut self, bytes: [u8; 7]) {
        let raw_hour = date::from_bcd(bytes[4] & 0x3F);
        let hour = if self.status & STATUS_24H != 0 {
            raw_hour
        } else {
            raw_hour % 12 + if bytes[4] & PM != 0 { 12 } else { 0 }
        };
        self.seconds = DateTime {
            year: date::from_bcd(bytes[0]),
            month: date::from_bcd(bytes[1]),
            day: date::from_bcd(bytes[2]),
            hour,
            minute: date::from_bcd(bytes[5]),
            second: date::from_bcd(bytes[6]),
        }
        .to_seconds();
        self.cycles = 0;
        self.weekday_shift = (bytes[3] % 7 + 7 - date::weekday(self.seconds)) % 7;
    }
}

#[cfg(feature = "persistence")]
mod footer {
    use super::{Rtc, STATUS_24H};
    use crate::persistence::rtc_footer::EARLIEST_TIMESTAMP;
    use crate::persistence::seiko_footer::SeikoFooter;

    impl Rtc {
        pub fn footer(&self, now: u64) -> SeikoFooter {
            let status = self.status;
            let mut clock = self.clone();
            clock.status = status | STATUS_24H;
            SeikoFooter {
                time: clock.date_time_bytes(),
                status,
                timestamp: now,
            }
        }

        pub fn load(&mut self, footer: &SeikoFooter, now: u64) {
            self.status = footer.status | STATUS_24H;
            self.set_date_time(footer.time);
            self.status = footer.status;
            if footer.timestamp >= EARLIEST_TIMESTAMP {
                let elapsed = now.saturating_sub(footer.timestamp);
                self.seconds = (self.seconds + elapsed % super::CENTURY) % super::CENTURY;
            }
        }
    }
}
