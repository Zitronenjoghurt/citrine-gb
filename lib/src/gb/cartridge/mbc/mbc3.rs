use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

pub const RTC_CYCLES_PER_SECOND: u32 = 1 << 20;

const SECONDS: usize = 0;
const MINUTES: usize = 1;
const HOURS: usize = 2;
const DAYS_LOW: usize = 3;
const DAYS_HIGH: usize = 4;

const REGISTER_MASKS: [u8; 5] = [0x3F, 0x3F, 0x1F, 0xFF, 0xC1];

pub const DAYS_HIGH_BIT: u8 = 0x01;
pub const HALT_BIT: u8 = 0x40;
pub const DAY_CARRY_BIT: u8 = 0x80;

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RtcRegisters {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days_low: u8,
    pub days_high: u8,
}

impl RtcRegisters {
    pub fn from_bytes(bytes: [u8; 5]) -> Self {
        Self {
            seconds: bytes[SECONDS] & REGISTER_MASKS[SECONDS],
            minutes: bytes[MINUTES] & REGISTER_MASKS[MINUTES],
            hours: bytes[HOURS] & REGISTER_MASKS[HOURS],
            days_low: bytes[DAYS_LOW] & REGISTER_MASKS[DAYS_LOW],
            days_high: bytes[DAYS_HIGH] & REGISTER_MASKS[DAYS_HIGH],
        }
    }

    pub fn to_bytes(self) -> [u8; 5] {
        [
            self.seconds,
            self.minutes,
            self.hours,
            self.days_low,
            self.days_high,
        ]
    }

    pub fn days(&self) -> u16 {
        self.days_low as u16 | (((self.days_high & DAYS_HIGH_BIT) as u16) << 8)
    }

    pub fn halted(&self) -> bool {
        self.days_high & HALT_BIT != 0
    }

    fn get(&self, index: usize) -> u8 {
        self.to_bytes()[index]
    }

    fn set(&mut self, index: usize, value: u8) {
        let mut bytes = self.to_bytes();
        bytes[index] = value;
        *self = Self::from_bytes(bytes);
    }

    fn tick(&mut self) {
        if self.seconds != 59 {
            self.seconds = (self.seconds + 1) & REGISTER_MASKS[SECONDS];
            return;
        }
        self.seconds = 0;

        if self.minutes != 59 {
            self.minutes = (self.minutes + 1) & REGISTER_MASKS[MINUTES];
            return;
        }
        self.minutes = 0;

        if self.hours != 23 {
            self.hours = (self.hours + 1) & REGISTER_MASKS[HOURS];
            return;
        }
        self.hours = 0;
        self.add_day();
    }

    fn add_day(&mut self) {
        let (days_low, overflow) = self.days_low.overflowing_add(1);
        self.days_low = days_low;
        if !overflow {
            return;
        }
        if self.days_high & DAYS_HIGH_BIT == 0 {
            self.days_high |= DAYS_HIGH_BIT;
        } else {
            self.days_high = (self.days_high & !DAYS_HIGH_BIT) | DAY_CARRY_BIT;
        }
    }

    fn time_in_range(&self) -> bool {
        self.seconds < 60 && self.minutes < 60 && self.hours < 24
    }
}

#[derive(Debug, Default, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Rtc {
    pub real: RtcRegisters,
    pub latched: RtcRegisters,
    pub sub_second_cycles: u32,
}

impl Rtc {
    pub fn cycle(&mut self) {
        if self.real.halted() {
            return;
        }
        self.sub_second_cycles += 1;
        if self.sub_second_cycles >= RTC_CYCLES_PER_SECOND {
            self.sub_second_cycles = 0;
            self.real.tick();
        }
    }

    pub fn advance_seconds(&mut self, seconds: u64) {
        if self.real.halted() {
            return;
        }
        let mut seconds = seconds;
        while seconds > 0 && !self.real.time_in_range() {
            self.real.tick();
            seconds -= 1;
        }

        let mut days = seconds / SECONDS_PER_DAY;
        if days > 2 * DAY_COUNTER_PERIOD {
            days = 2 * DAY_COUNTER_PERIOD + days % DAY_COUNTER_PERIOD;
        }
        for _ in 0..days {
            self.real.add_day();
        }

        for _ in 0..seconds % SECONDS_PER_DAY {
            self.real.tick();
        }
    }

    pub fn latch(&mut self) {
        self.latched = self.real;
    }

    fn read(&self, register: usize) -> u8 {
        self.latched.get(register)
    }

    fn write(&mut self, register: usize, value: u8) {
        if register == SECONDS {
            self.sub_second_cycles = 0;
        }
        self.real.set(register, value);
        self.latched.set(register, value);
    }
}

const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
const DAY_COUNTER_PERIOD: u64 = 512;

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mbc3 {
    pub has_rtc: bool,
    pub ram_rtc_enabled: bool,
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub rom_bank_register: u8,
    pub ram_rtc_select: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    pub latch_armed: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub rtc: Rtc,
}

impl Mbc3 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize, has_rtc: bool) -> Self {
        Self {
            has_rtc,
            ram_rtc_enabled: false,
            rom_bank_count,
            ram_bank_count,
            rom_bank_register: 0,
            ram_rtc_select: 0,
            latch_armed: false,
            rtc: Rtc::default(),
        }
    }

    pub fn cycle(&mut self) {
        if self.has_rtc {
            self.rtc.cycle();
        }
    }

    fn selected_rtc_register(&self) -> Option<usize> {
        if !self.has_rtc || !self.ram_rtc_enabled {
            return None;
        }
        match self.ram_rtc_select {
            0x08..=0x0C => Some((self.ram_rtc_select - 0x08) as usize),
            _ => None,
        }
    }

    fn write_ram_rtc_enabled(&mut self, value: u8) {
        self.ram_rtc_enabled = value & 0x0F == 0x0A;
    }

    fn write_rom_bank_number(&mut self, value: u8) {
        self.rom_bank_register = value & 0x7F;
    }

    fn write_ram_rtc_select(&mut self, value: u8) {
        self.ram_rtc_select = value;
    }

    fn write_clock_latch(&mut self, value: u8) {
        if !self.has_rtc {
            return;
        };

        if self.latch_armed && value == 1 {
            self.rtc.latch();
        }
        self.latch_armed = value == 0;
    }
}

impl MbcInterface for Mbc3 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.write_ram_rtc_enabled(value),
            0x2000..=0x3FFF => self.write_rom_bank_number(value),
            0x4000..=0x5FFF => self.write_ram_rtc_select(value),
            0x6000..=0x7FFF => self.write_clock_latch(value),
            0xA000..=0xBFFF => {
                if let Some(register) = self.selected_rtc_register() {
                    self.rtc.write(register, value);
                    return true;
                }
            }
            _ => {}
        }

        false
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        if !(0xA000..=0xBFFF).contains(&addr) {
            return None;
        }
        self.selected_rtc_register()
            .map(|register| self.rtc.read(register))
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        mask_bank_number(self.rom_bank_register as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        if !self.ram_rtc_enabled || self.ram_rtc_select > 7 {
            None
        } else {
            Some(mask_bank_number(
                self.ram_rtc_select as usize,
                self.ram_bank_count,
            ))
        }
    }

    fn soft_reset(&mut self) {
        let rtc = std::mem::take(&mut self.rtc);
        *self = Self::new(self.rom_bank_count, self.ram_bank_count, self.has_rtc);
        self.rtc = rtc;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtc_cart() -> Mbc3 {
        let mut mbc = Mbc3::new(2, 1, true);
        mbc.on_write(0x0000, 0x0A);
        mbc
    }

    fn write_register(mbc: &mut Mbc3, register: u8, value: u8) {
        mbc.on_write(0x4000, register);
        mbc.on_write(0xA000, value);
    }

    fn latch(mbc: &mut Mbc3) {
        mbc.on_write(0x6000, 0x00);
        mbc.on_write(0x6000, 0x01);
    }

    fn read_register(mbc: &mut Mbc3, register: u8) -> u8 {
        mbc.on_write(0x4000, register);
        mbc.on_read(0xA000).expect("rtc register mapped")
    }

    fn read_all(mbc: &mut Mbc3) -> [u8; 5] {
        latch(mbc);
        [0x08, 0x09, 0x0A, 0x0B, 0x0C].map(|r| read_register(mbc, r))
    }

    fn write_all(mbc: &mut Mbc3, values: [u8; 5]) {
        write_register(mbc, 0x0C, HALT_BIT);
        for (register, value) in [0x08, 0x09, 0x0A, 0x0B, 0x0C].into_iter().zip(values) {
            write_register(mbc, register, value);
        }
    }

    fn run_seconds(mbc: &mut Mbc3, seconds: u32) {
        for _ in 0..seconds * RTC_CYCLES_PER_SECOND {
            mbc.cycle();
        }
    }

    #[test]
    fn counts_one_second_per_million_cycles() {
        let mut mbc = rtc_cart();
        for _ in 0..RTC_CYCLES_PER_SECOND - 1 {
            mbc.cycle();
        }
        assert_eq!(read_all(&mut mbc)[0], 0);
        mbc.cycle();
        assert_eq!(read_all(&mut mbc)[0], 1);
    }

    #[test]
    fn reads_return_the_latched_time_until_the_next_latch() {
        let mut mbc = rtc_cart();
        latch(&mut mbc);
        run_seconds(&mut mbc, 3);
        assert_eq!(read_register(&mut mbc, 0x08), 0);
        latch(&mut mbc);
        assert_eq!(read_register(&mut mbc, 0x08), 3);
    }

    #[test]
    fn latching_needs_zero_then_one() {
        let mut mbc = rtc_cart();
        latch(&mut mbc);
        run_seconds(&mut mbc, 2);
        mbc.on_write(0x6000, 0x01);
        assert_eq!(read_register(&mut mbc, 0x08), 0);
        mbc.on_write(0x6000, 0x00);
        mbc.on_write(0x6000, 0x05);
        mbc.on_write(0x6000, 0x01);
        assert_eq!(read_register(&mut mbc, 0x08), 0);
    }

    #[test]
    fn halt_stops_the_clock() {
        let mut mbc = rtc_cart();
        write_register(&mut mbc, 0x0C, HALT_BIT);
        run_seconds(&mut mbc, 2);
        assert_eq!(read_all(&mut mbc), [0, 0, 0, 0, HALT_BIT]);
    }

    #[test]
    fn rolls_over_into_the_next_day() {
        let mut mbc = rtc_cart();
        write_all(&mut mbc, [59, 59, 23, 0xFF, 0]);
        run_seconds(&mut mbc, 1);
        assert_eq!(read_all(&mut mbc), [0, 0, 0, 0x00, DAYS_HIGH_BIT]);
    }

    #[test]
    fn day_overflow_sets_a_sticky_carry() {
        let mut mbc = rtc_cart();
        write_all(&mut mbc, [59, 59, 23, 0xFF, DAYS_HIGH_BIT]);
        run_seconds(&mut mbc, 1);
        assert_eq!(read_all(&mut mbc), [0, 0, 0, 0, DAY_CARRY_BIT]);

        mbc.rtc.advance_seconds(24 * 60 * 60);
        assert_eq!(read_all(&mut mbc), [0, 0, 0, 1, DAY_CARRY_BIT]);
    }

    #[test]
    fn unused_bits_read_as_zero() {
        let mut mbc = rtc_cart();
        write_all(&mut mbc, [0xFF; 5]);
        assert_eq!(read_all(&mut mbc), [0x3F, 0x3F, 0x1F, 0xFF, 0xC1]);
    }

    #[test]
    fn out_of_range_values_count_up_and_wrap_without_carry() {
        let mut mbc = rtc_cart();
        write_all(&mut mbc, [60, 63, 28, 0, 0]);
        run_seconds(&mut mbc, 1);
        assert_eq!(read_all(&mut mbc), [61, 63, 28, 0, 0]);

        write_all(&mut mbc, [63, 63, 31, 0, 0]);
        run_seconds(&mut mbc, 1);
        assert_eq!(read_all(&mut mbc), [0, 63, 31, 0, 0]);
    }

    #[test]
    fn high_minutes_and_hours_still_carry_from_59() {
        let mut mbc = rtc_cart();
        write_all(&mut mbc, [59, 61, 0, 0, 0]);
        run_seconds(&mut mbc, 1);
        assert_eq!(read_all(&mut mbc), [0, 62, 0, 0, 0]);

        write_all(&mut mbc, [59, 59, 31, 0, 0]);
        run_seconds(&mut mbc, 1);
        assert_eq!(read_all(&mut mbc), [0, 0, 0, 0, 0]);
    }

    #[test]
    fn writing_seconds_restarts_the_second() {
        let mut mbc = rtc_cart();
        for _ in 0..RTC_CYCLES_PER_SECOND / 2 {
            mbc.cycle();
        }
        write_register(&mut mbc, 0x08, 10);
        for _ in 0..RTC_CYCLES_PER_SECOND - 1 {
            mbc.cycle();
        }
        assert_eq!(read_all(&mut mbc)[0], 10);
        mbc.cycle();
        assert_eq!(read_all(&mut mbc)[0], 11);
    }

    #[test]
    fn writing_minutes_keeps_the_second_running() {
        let mut mbc = rtc_cart();
        for _ in 0..RTC_CYCLES_PER_SECOND / 2 {
            mbc.cycle();
        }
        write_register(&mut mbc, 0x09, 5);
        for _ in 0..RTC_CYCLES_PER_SECOND / 2 {
            mbc.cycle();
        }
        assert_eq!(read_all(&mut mbc)[..2], [1, 5]);
    }

    #[test]
    fn registers_are_unmapped_without_ram_enable() {
        let mut mbc = Mbc3::new(2, 1, true);
        mbc.on_write(0x4000, 0x08);
        assert_eq!(mbc.on_read(0xA000), None);
        assert_eq!(mbc.ram_bank(), None);
    }

    #[test]
    fn carts_without_a_clock_ignore_rtc_selects() {
        let mut mbc = Mbc3::new(2, 1, false);
        mbc.on_write(0x0000, 0x0A);
        mbc.on_write(0x4000, 0x08);
        assert!(!mbc.on_write(0xA000, 0x12));
        assert_eq!(mbc.on_read(0xA000), None);
    }

    #[test]
    fn catching_up_skips_a_halted_clock() {
        let mut rtc = Rtc::default();
        rtc.advance_seconds(90);
        assert_eq!(rtc.real.to_bytes(), [30, 1, 0, 0, 0]);

        rtc.real.days_high = HALT_BIT;
        rtc.advance_seconds(90);
        assert_eq!(rtc.real.to_bytes(), [30, 1, 0, 0, HALT_BIT]);
    }

    #[test]
    fn catching_up_whole_days_matches_ticking_each_second() {
        let start = RtcRegisters::from_bytes([12, 34, 5, 0xFE, DAYS_HIGH_BIT]);
        let elapsed = 3 * SECONDS_PER_DAY + 7 * 3600 + 123;

        let mut fast = Rtc {
            real: start,
            ..Rtc::default()
        };
        fast.advance_seconds(elapsed);

        let mut slow = start;
        for _ in 0..elapsed {
            slow.tick();
        }
        assert_eq!(fast.real, slow);
        assert_eq!(fast.real.days_high & DAY_CARRY_BIT, DAY_CARRY_BIT);
    }

    #[test]
    fn catching_up_years_keeps_the_day_carry_and_the_day_of_the_cycle() {
        let mut rtc = Rtc::default();
        rtc.advance_seconds(5000 * SECONDS_PER_DAY + 61);
        assert_eq!(rtc.real.days(), (5000 % 512) as u16);
        assert_eq!(rtc.real.to_bytes()[..3], [1, 1, 0]);
        assert_ne!(rtc.real.days_high & DAY_CARRY_BIT, 0);
    }

    #[test]
    fn soft_reset_keeps_the_time() {
        let mut mbc = rtc_cart();
        write_all(&mut mbc, [1, 2, 3, 4, 0]);
        mbc.soft_reset();
        assert_eq!(mbc.rtc.real.to_bytes(), [1, 2, 3, 4, 0]);
        assert!(!mbc.ram_rtc_enabled);
    }
}
