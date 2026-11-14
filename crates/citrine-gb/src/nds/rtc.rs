use crate::utils::date::{self, CENTURY, DateTime};

pub const CYCLES_PER_SECOND: u32 = 33_513_982;

const DATA: u16 = 1 << 0;
const CLOCK: u16 = 1 << 1;
const SELECT: u16 = 1 << 2;
const DATA_WRITE: u16 = 1 << 4;
const COMMAND_CODE: u8 = 0x6;

const STATUS1_RESET: u8 = 1 << 0;
const STATUS1_24H: u8 = 1 << 1;
const STATUS1_WRITABLE: u8 = 0x0E;
const STATUS1_INT1: u8 = 1 << 4;
const STATUS1_INT2: u8 = 1 << 5;
const STATUS1_FLAGS: u8 = 0xF0;

const INT1_MODE: u8 = 0x0F;
const INT1_FREQUENCY: u8 = 0x01;
const INT1_MINUTE_EDGE: u8 = 0x02;
const INT1_MINUTE_STEADY: u8 = 0x03;
const INT1_ALARM: u8 = 0x04;
const INT1_MINUTE_PULSE: u8 = 0x07;
const INT1_32K: u8 = 0x08;
const INT1_ALARM_BIT: u8 = 0x04;
const INT2_ALARM: u8 = 1 << 6;
const STATUS2_TEST: u8 = 1 << 7;

const PM: u8 = 1 << 6;
const COMPARE: u8 = 1 << 7;
const MINUTE_PULSE_CYCLES: u32 = CYCLES_PER_SECOND / 128;
const MINUTE_STEADY_LOW_SECONDS: u64 = 30;
const FREQUENCIES: [u32; 5] = [1, 2, 4, 8, 16];
const TONE_32K: u32 = 32_768;
const UNUSED: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Register {
    Status1,
    Status2,
    DateTime,
    Time,
    Alarm1,
    Alarm2,
    Adjust,
    Free,
}

impl Register {
    fn of(command: u8) -> Self {
        match (command >> 1) & 7 {
            0 => Self::Status1,
            1 => Self::Status2,
            2 => Self::DateTime,
            3 => Self::Time,
            4 => Self::Alarm1,
            5 => Self::Alarm2,
            6 => Self::Adjust,
            _ => Self::Free,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Phase {
    Idle,
    Command,
    Write(Register),
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rtc {
    seconds: u64,
    cycles: u32,
    weekday_shift: u8,
    status1: u8,
    status2: u8,
    alarm1: [u8; 3],
    alarm2: [u8; 3],
    adjust: u8,
    free: u8,
    pins: u16,
    phase: Phase,
    shift: u8,
    bits: u8,
    buffer: [u8; 7],
    index: usize,
    len: usize,
    output: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    set_by_game: bool,
}

impl Default for Rtc {
    fn default() -> Self {
        Self {
            seconds: 0,
            cycles: 0,
            weekday_shift: 0,
            status1: STATUS1_24H,
            status2: 0,
            alarm1: [0; 3],
            alarm2: [0; 3],
            adjust: 0,
            free: 0,
            pins: 0,
            phase: Phase::Idle,
            shift: 0,
            bits: 0,
            buffer: [0; 7],
            index: 0,
            len: 0,
            output: false,
            set_by_game: false,
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

    pub fn unix(&self) -> i64 {
        date::to_unix(self.seconds)
    }

    pub fn take_set_by_game(&mut self) -> bool {
        std::mem::take(&mut self.set_by_game)
    }

    pub fn hold(&mut self, secs: f64) -> f64 {
        let want = (secs.max(0.0) * f64::from(CYCLES_PER_SECOND)) as u32;
        let take = want.min(self.cycles);
        self.cycles -= take;
        f64::from(take) / f64::from(CYCLES_PER_SECOND)
    }

    #[inline]
    pub fn tick(&mut self, cycles: u32) {
        self.cycles += cycles;
        if self.cycles >= CYCLES_PER_SECOND {
            self.carry_seconds();
        }
    }

    #[inline(never)]
    fn carry_seconds(&mut self) {
        while self.cycles >= CYCLES_PER_SECOND {
            self.cycles -= CYCLES_PER_SECOND;
            let weekday = self.weekday();
            self.seconds = (self.seconds + 1) % CENTURY;
            if self.seconds == 0 {
                self.weekday_shift = ((weekday + 1) % 7 + 7 - date::weekday(0)) % 7;
            }
            if self.seconds.is_multiple_of(60) {
                self.minute_carry();
            }
        }
    }

    pub fn read(&self) -> u16 {
        if self.pins & DATA_WRITE != 0 {
            self.pins
        } else {
            (self.pins & !DATA) | u16::from(self.output)
        }
    }

    pub fn write(&mut self, value: u16, mask: u16) {
        let old = self.pins;
        self.pins = (old & !mask) | (value & mask);
        let selected = self.pins & SELECT != 0;
        if !selected {
            self.phase = Phase::Idle;
            return;
        }
        if old & SELECT == 0 {
            self.phase = Phase::Command;
            self.shift = 0;
            self.bits = 0;
            return;
        }
        let rising = self.pins & CLOCK != 0 && old & CLOCK == 0;
        let falling = self.pins & CLOCK == 0 && old & CLOCK != 0;
        let bit = u8::from(self.pins & DATA != 0);
        match self.phase {
            Phase::Command if rising => {
                self.shift = (self.shift << 1) | bit;
                self.bits += 1;
                if self.bits == 8 {
                    self.start(self.shift);
                }
            }
            Phase::Write(register) if rising => self.receive(register, bit),
            Phase::Read if falling => self.send(),
            _ => {}
        }
    }

    #[inline]
    pub fn interrupt(&self) -> bool {
        self.status2 & (INT1_MODE | INT2_ALARM) != 0 && self.raised()
    }

    #[inline(never)]
    fn raised(&self) -> bool {
        self.int1() || self.int2()
    }

    fn int1(&self) -> bool {
        let mode = self.status2 & INT1_MODE;
        if mode & INT1_32K != 0 {
            return self.tone_low(TONE_32K);
        }
        match mode {
            INT1_FREQUENCY => FREQUENCIES
                .iter()
                .enumerate()
                .any(|(bit, &hz)| self.alarm1[2] & (1 << bit) != 0 && self.tone_low(hz)),
            INT1_MINUTE_EDGE | INT1_ALARM => self.status1 & STATUS1_INT1 != 0,
            INT1_MINUTE_STEADY => self.seconds % 60 < MINUTE_STEADY_LOW_SECONDS,
            INT1_MINUTE_PULSE => {
                self.seconds.is_multiple_of(60) && self.cycles < MINUTE_PULSE_CYCLES
            }
            _ => false,
        }
    }

    fn int2(&self) -> bool {
        self.status2 & INT2_ALARM != 0 && self.status1 & STATUS1_INT2 != 0
    }

    fn tone_low(&self, hz: u32) -> bool {
        let phase = u64::from(self.cycles) * u64::from(hz) * 2 / u64::from(CYCLES_PER_SECOND);
        phase.is_multiple_of(2)
    }

    fn minute_carry(&mut self) {
        let mode = self.status2 & INT1_MODE;
        let edge = mode == INT1_MINUTE_EDGE;
        let alarm1 = mode == INT1_ALARM && self.alarm_matches(self.alarm1);
        if edge || alarm1 {
            self.status1 |= STATUS1_INT1;
        }
        if self.status2 & INT2_ALARM != 0 && self.alarm_matches(self.alarm2) {
            self.status1 |= STATUS1_INT2;
        }
    }

    fn alarm_matches(&self, alarm: [u8; 3]) -> bool {
        let now = self.date_time_bytes();
        let weekday = alarm[0] & COMPARE == 0 || alarm[0] & 7 == now[3];
        let hour = alarm[1] & COMPARE == 0 || alarm[1] & 0x7F == now[4];
        let minute = alarm[2] & COMPARE == 0 || alarm[2] & 0x7F == now[5];
        weekday && hour && minute
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
        self.len = self.len_of(register);
        if command & 1 != 0 {
            self.fill(register);
            self.phase = Phase::Read;
        } else {
            self.phase = Phase::Write(register);
        }
    }

    fn len_of(&self, register: Register) -> usize {
        match register {
            Register::DateTime => 7,
            Register::Time | Register::Alarm2 => 3,
            Register::Alarm1 if self.status2 & INT1_ALARM_BIT != 0 => 3,
            _ => 1,
        }
    }

    fn receive(&mut self, register: Register, bit: u8) {
        self.shift |= bit << self.bits;
        self.bits += 1;
        if self.bits < 8 {
            return;
        }
        self.buffer[self.index] = self.shift;
        self.index += 1;
        self.shift = 0;
        self.bits = 0;
        if self.index == self.len {
            self.apply(register);
            self.phase = Phase::Idle;
        }
    }

    fn send(&mut self) {
        let byte = if self.index < self.len {
            self.buffer[self.index]
        } else {
            UNUSED
        };
        self.output = (byte >> self.bits) & 1 != 0;
        self.bits += 1;
        if self.bits == 8 {
            self.bits = 0;
            self.index = (self.index + 1).min(self.len);
        }
    }

    fn fill(&mut self, register: Register) {
        match register {
            Register::Status1 => {
                self.buffer[0] = self.status1;
                self.status1 &= !STATUS1_FLAGS;
            }
            Register::Status2 => self.buffer[0] = self.status2,
            Register::DateTime => self.buffer = self.date_time_bytes(),
            Register::Time => {
                let now = self.date_time_bytes();
                self.buffer[..3].copy_from_slice(&now[4..]);
            }
            Register::Alarm1 if self.len == 1 => self.buffer[0] = self.alarm1[2],
            Register::Alarm1 => self.buffer[..3].copy_from_slice(&self.alarm1),
            Register::Alarm2 => self.buffer[..3].copy_from_slice(&self.alarm2),
            Register::Adjust => self.buffer[0] = self.adjust,
            Register::Free => self.buffer[0] = self.free,
        }
    }

    fn apply(&mut self, register: Register) {
        let bytes = self.buffer;
        match register {
            Register::Status1 if bytes[0] & STATUS1_RESET != 0 => self.reset(),
            Register::Status1 => {
                self.status1 = (self.status1 & !STATUS1_WRITABLE) | (bytes[0] & STATUS1_WRITABLE)
            }
            Register::Status2 => {
                self.status2 = bytes[0] & !STATUS2_TEST;
                if self.status2 & INT1_MODE != INT1_ALARM
                    && self.status2 & INT1_MODE != INT1_MINUTE_EDGE
                {
                    self.status1 &= !STATUS1_INT1;
                }
                if self.status2 & INT2_ALARM == 0 {
                    self.status1 &= !STATUS1_INT2;
                }
            }
            Register::DateTime => {
                self.set_date_time(bytes);
                self.set_by_game = true;
            }
            Register::Time => {
                let mut all = self.date_time_bytes();
                all[4..].copy_from_slice(&bytes[..3]);
                self.set_date_time(all);
                self.set_by_game = true;
            }
            Register::Alarm1 if self.len == 1 => self.alarm1[2] = bytes[0],
            Register::Alarm1 => self.alarm1.copy_from_slice(&bytes[..3]),
            Register::Alarm2 => self.alarm2.copy_from_slice(&bytes[..3]),
            Register::Adjust => self.adjust = bytes[0],
            Register::Free => self.free = bytes[0],
        }
    }

    fn reset(&mut self) {
        let pins = self.pins;
        *self = Self {
            status1: 0,
            pins,
            ..Self::default()
        };
        self.set_date_time([0, 1, 1, 0, 0, 0, 0]);
    }

    fn twenty_four_hours(&self) -> bool {
        self.status1 & STATUS1_24H != 0
    }

    fn date_time_bytes(&self) -> [u8; 7] {
        let now = self.date_time();
        let hour = if self.twenty_four_hours() {
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
        let hour = if self.twenty_four_hours() {
            raw_hour
        } else {
            raw_hour % 12 + if bytes[4] & PM != 0 { 12 } else { 0 }
        };
        self.seconds = DateTime {
            year: date::from_bcd(bytes[0]),
            month: date::from_bcd(bytes[1] & 0x1F),
            day: date::from_bcd(bytes[2] & 0x3F),
            hour,
            minute: date::from_bcd(bytes[5] & 0x7F),
            second: date::from_bcd(bytes[6] & 0x7F),
        }
        .to_seconds();
        self.cycles = 0;
        self.weekday_shift = (bytes[3] % 7 + 7 - date::weekday(self.seconds)) % 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIX_2025_03_14_15_09_26: i64 = 1_741_964_966;

    fn select(rtc: &mut Rtc) {
        rtc.write(CLOCK | DATA_WRITE, 0xFFFF);
        rtc.write(SELECT | CLOCK | DATA_WRITE, 0xFFFF);
    }

    fn deselect(rtc: &mut Rtc) {
        rtc.write(CLOCK | DATA_WRITE, 0xFFFF);
    }

    fn send_byte(rtc: &mut Rtc, byte: u8) {
        for bit in 0..8 {
            let data = u16::from((byte >> bit) & 1);
            rtc.write(SELECT | DATA_WRITE | data, 0xFFFF);
            rtc.write(SELECT | DATA_WRITE | CLOCK | data, 0xFFFF);
        }
    }

    fn receive_byte(rtc: &mut Rtc) -> u8 {
        let mut byte = 0;
        for bit in 0..8 {
            rtc.write(SELECT, 0xFFFF);
            rtc.write(SELECT | CLOCK, 0xFFFF);
            byte |= ((rtc.read() & DATA) as u8) << bit;
        }
        byte
    }

    fn command(register: u8, read: bool) -> u8 {
        0x06 | ((register.reverse_bits() >> 1) & 0x70) | if read { 0x80 } else { 0 }
    }

    fn read_register(rtc: &mut Rtc, register: u8, len: usize) -> Vec<u8> {
        select(rtc);
        send_byte(rtc, command(register, true));
        let bytes = (0..len).map(|_| receive_byte(rtc)).collect();
        deselect(rtc);
        bytes
    }

    fn write_register(rtc: &mut Rtc, register: u8, bytes: &[u8]) {
        select(rtc);
        send_byte(rtc, command(register, false));
        for &byte in bytes {
            send_byte(rtc, byte);
        }
        deselect(rtc);
    }

    fn clock() -> Rtc {
        let mut rtc = Rtc::default();
        rtc.set_unix(UNIX_2025_03_14_15_09_26);
        rtc
    }

    fn seconds(rtc: &mut Rtc, count: u32) {
        for _ in 0..count {
            rtc.tick(CYCLES_PER_SECOND);
        }
    }

    #[test]
    fn command_bytes_match_gbatek_numbering() {
        assert_eq!(command(0, true), 0x86);
        assert_eq!(command(1, false), 0x46);
        assert_eq!(command(2, true), 0xA6);
        assert_eq!(command(3, true), 0xE6);
        assert_eq!(command(4, false), 0x16);
        assert_eq!(command(7, true), 0xF6);
    }

    #[test]
    fn date_and_time_read_back_from_the_host_clock() {
        let mut rtc = clock();
        let bytes = read_register(&mut rtc, 2, 7);
        assert_eq!(bytes, [0x25, 0x03, 0x14, 5, 0x15 | PM, 0x09, 0x26]);
    }

    #[test]
    fn time_reads_three_bytes() {
        let mut rtc = clock();
        assert_eq!(read_register(&mut rtc, 3, 3), [0x15 | PM, 0x09, 0x26]);
    }

    #[test]
    fn twelve_hour_mode_reads_the_hour_with_pm() {
        let mut rtc = clock();
        write_register(&mut rtc, 0, &[0]);
        assert_eq!(read_register(&mut rtc, 3, 1), [0x03 | PM]);
    }

    #[test]
    fn morning_hours_read_without_pm() {
        let mut rtc = Rtc::default();
        rtc.set_unix(UNIX_2025_03_14_15_09_26 - 6 * 3600);
        assert_eq!(read_register(&mut rtc, 3, 1), [0x09]);
    }

    #[test]
    fn the_clock_runs_with_emulated_cycles() {
        let mut rtc = clock();
        rtc.tick(CYCLES_PER_SECOND - 1);
        assert_eq!(rtc.date_time().second, 26);
        rtc.tick(1);
        assert_eq!(rtc.date_time().second, 27);
        seconds(&mut rtc, 33);
        assert_eq!((rtc.date_time().minute, rtc.date_time().second), (10, 0));
    }

    #[test]
    fn writing_date_and_time_sets_the_clock() {
        let mut rtc = clock();
        write_register(&mut rtc, 2, &[0x99, 0x12, 0x31, 3, 0x23, 0x59, 0x58]);
        let now = rtc.date_time();
        assert_eq!((now.year, now.month, now.day), (99, 12, 31));
        assert_eq!((now.hour, now.minute, now.second), (23, 59, 58));
        assert_eq!(rtc.weekday(), 3);
        seconds(&mut rtc, 2);
        assert_eq!(
            read_register(&mut rtc, 2, 7),
            [0x00, 0x01, 0x01, 4, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn writing_the_time_keeps_the_date_and_weekday() {
        let mut rtc = clock();
        write_register(&mut rtc, 3, &[0x07, 0x30, 0x00]);
        assert_eq!(
            read_register(&mut rtc, 2, 7),
            [0x25, 0x03, 0x14, 5, 0x07, 0x30, 0x00]
        );
    }

    #[test]
    fn twelve_hour_writes_use_the_pm_bit() {
        let mut rtc = clock();
        write_register(&mut rtc, 0, &[0]);
        write_register(&mut rtc, 3, &[0x11 | PM, 0x00, 0x00]);
        assert_eq!(rtc.date_time().hour, 23);
        write_register(&mut rtc, 3, &[0x00, 0x00, 0x00]);
        assert_eq!(rtc.date_time().hour, 0);
    }

    #[test]
    fn status1_keeps_its_writable_bits() {
        let mut rtc = clock();
        write_register(&mut rtc, 0, &[0xFE]);
        assert_eq!(read_register(&mut rtc, 0, 1), [0x0E]);
    }

    #[test]
    fn status1_reset_clears_the_clock_and_registers() {
        let mut rtc = clock();
        write_register(&mut rtc, 7, &[0x5A]);
        write_register(&mut rtc, 1, &[0x40]);
        write_register(&mut rtc, 0, &[STATUS1_RESET]);
        assert_eq!(
            read_register(&mut rtc, 2, 7),
            [0x00, 0x01, 0x01, 0, 0x00, 0x00, 0x00]
        );
        assert_eq!(read_register(&mut rtc, 0, 1), [0]);
        assert_eq!(read_register(&mut rtc, 1, 1), [0]);
        assert_eq!(read_register(&mut rtc, 7, 1), [0]);
    }

    #[test]
    fn status2_drops_the_test_bit() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[0xF4]);
        assert_eq!(read_register(&mut rtc, 1, 1), [0x74]);
    }

    #[test]
    fn free_and_adjust_registers_hold_a_byte() {
        let mut rtc = clock();
        write_register(&mut rtc, 6, &[0x12]);
        write_register(&mut rtc, 7, &[0x34]);
        assert_eq!(read_register(&mut rtc, 6, 1), [0x12]);
        assert_eq!(read_register(&mut rtc, 7, 1), [0x34]);
    }

    #[test]
    fn a_wrong_command_code_is_ignored() {
        let mut rtc = clock();
        select(&mut rtc);
        send_byte(&mut rtc, 0xA7);
        send_byte(&mut rtc, 0x00);
        deselect(&mut rtc);
        assert_eq!(rtc.date_time().year, 25);
    }

    #[test]
    fn reading_past_the_register_returns_ff() {
        let mut rtc = clock();
        assert_eq!(read_register(&mut rtc, 3, 4)[3], 0xFF);
    }

    #[test]
    fn a_deselect_mid_write_drops_the_write() {
        let mut rtc = clock();
        select(&mut rtc);
        send_byte(&mut rtc, command(2, false));
        send_byte(&mut rtc, 0x99);
        deselect(&mut rtc);
        assert_eq!(rtc.date_time().year, 25);
    }

    #[test]
    fn data_reads_back_as_written_in_write_direction() {
        let mut rtc = Rtc::default();
        rtc.write(DATA | DATA_WRITE | 0x0F00, 0xFFFF);
        assert_eq!(rtc.read(), DATA | DATA_WRITE | 0x0F00);
    }

    #[test]
    fn alarm1_fires_at_its_minute_and_status1_read_clears_it() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_ALARM]);
        write_register(&mut rtc, 4, &[0, 0x15 | PM | COMPARE, 0x10 | COMPARE]);
        assert_eq!(
            read_register(&mut rtc, 4, 3),
            [0, 0x15 | PM | COMPARE, 0x10 | COMPARE]
        );
        seconds(&mut rtc, 33);
        assert!(!rtc.interrupt());
        seconds(&mut rtc, 1);
        assert!(rtc.interrupt());
        assert_eq!(
            read_register(&mut rtc, 0, 1)[0] & STATUS1_INT1,
            STATUS1_INT1
        );
        assert!(!rtc.interrupt());
    }

    #[test]
    fn alarm1_skips_a_minute_that_does_not_match() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_ALARM]);
        write_register(&mut rtc, 4, &[0, 0, 0x11 | COMPARE]);
        seconds(&mut rtc, 34);
        assert!(!rtc.interrupt());
        seconds(&mut rtc, 60);
        assert!(rtc.interrupt());
    }

    #[test]
    fn alarm1_compares_the_weekday() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_ALARM]);
        write_register(&mut rtc, 4, &[4 | COMPARE, 0, 0]);
        seconds(&mut rtc, 34);
        assert!(!rtc.interrupt());
        write_register(&mut rtc, 4, &[5 | COMPARE, 0, 0]);
        seconds(&mut rtc, 60);
        assert!(rtc.interrupt());
    }

    #[test]
    fn alarm2_fires_through_its_enable_bit() {
        let mut rtc = clock();
        write_register(&mut rtc, 5, &[0, 0, 0x10 | COMPARE]);
        seconds(&mut rtc, 34);
        assert!(!rtc.interrupt());
        write_register(&mut rtc, 1, &[INT2_ALARM]);
        seconds(&mut rtc, 3600);
        assert!(rtc.interrupt());
        assert_eq!(
            read_register(&mut rtc, 0, 1)[0] & STATUS1_INT2,
            STATUS1_INT2
        );
        assert!(!rtc.interrupt());
    }

    #[test]
    fn disabling_the_alarm_releases_the_line() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_ALARM]);
        seconds(&mut rtc, 34);
        assert!(rtc.interrupt());
        write_register(&mut rtc, 1, &[0]);
        assert!(!rtc.interrupt());
    }

    #[test]
    fn frequency_mode_shares_the_alarm1_minute_byte() {
        let mut rtc = clock();
        write_register(&mut rtc, 4, &[0x01]);
        write_register(&mut rtc, 1, &[INT1_ALARM]);
        assert_eq!(read_register(&mut rtc, 4, 3), [0, 0, 0x01]);
    }

    #[test]
    fn one_hz_frequency_is_low_for_half_a_second() {
        let mut rtc = clock();
        write_register(&mut rtc, 4, &[0x01]);
        write_register(&mut rtc, 1, &[INT1_FREQUENCY]);
        assert!(rtc.interrupt());
        rtc.tick(CYCLES_PER_SECOND / 2 + 1);
        assert!(!rtc.interrupt());
        rtc.tick(CYCLES_PER_SECOND / 2);
        assert!(rtc.interrupt());
    }

    #[test]
    fn enabled_frequencies_combine_low() {
        let mut rtc = clock();
        write_register(&mut rtc, 4, &[0x03]);
        write_register(&mut rtc, 1, &[INT1_FREQUENCY]);
        let tenth = CYCLES_PER_SECOND / 10;
        rtc.tick(3 * tenth);
        assert!(rtc.interrupt());
        rtc.tick(3 * tenth);
        assert!(rtc.interrupt());
        rtc.tick(2 * tenth);
        assert!(!rtc.interrupt());
    }

    #[test]
    fn minute_edge_latches_until_status1_is_read() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_MINUTE_EDGE]);
        seconds(&mut rtc, 33);
        assert!(!rtc.interrupt());
        seconds(&mut rtc, 2);
        assert!(rtc.interrupt());
        read_register(&mut rtc, 0, 1);
        assert!(!rtc.interrupt());
    }

    #[test]
    fn minute_steady_is_low_for_the_first_30_seconds() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_MINUTE_STEADY]);
        assert!(rtc.interrupt());
        seconds(&mut rtc, 4);
        assert!(!rtc.interrupt());
        seconds(&mut rtc, 30);
        assert!(rtc.interrupt());
    }

    #[test]
    fn minute_pulse_is_low_for_about_8_ms() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_MINUTE_PULSE]);
        seconds(&mut rtc, 34);
        assert!(rtc.interrupt());
        rtc.tick(MINUTE_PULSE_CYCLES);
        assert!(!rtc.interrupt());
    }

    #[test]
    fn the_32k_output_toggles() {
        let mut rtc = clock();
        write_register(&mut rtc, 1, &[INT1_32K]);
        let first = rtc.interrupt();
        rtc.tick(CYCLES_PER_SECOND / TONE_32K / 2 + 1);
        assert_ne!(rtc.interrupt(), first);
    }

    #[test]
    fn a_game_setting_the_clock_is_noticed_once() {
        let mut rtc = clock();
        assert_eq!(rtc.unix(), UNIX_2025_03_14_15_09_26);
        seconds(&mut rtc, 3);
        assert!(!rtc.take_set_by_game());
        write_register(&mut rtc, 3, &[0x08, 0x30, 0x00]);
        assert!(rtc.take_set_by_game());
        assert!(!rtc.take_set_by_game());
        assert_eq!(
            rtc.unix(),
            UNIX_2025_03_14_15_09_26 - 15 * 3600 - 9 * 60 - 26 + 8 * 3600 + 30 * 60
        );
        write_register(&mut rtc, 2, &[0x30, 0x01, 0x02, 3, 0x04, 0x05, 0x06]);
        assert!(rtc.take_set_by_game());
    }
}
