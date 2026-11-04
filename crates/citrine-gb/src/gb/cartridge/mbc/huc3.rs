use crate::gb::cartridge::mbc::mbc3::RTC_CYCLES_PER_SECOND;
use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

pub const HUC3_FOOTER_LEN: usize = 17;
const MINUTE_CYCLES: u32 = 60 * RTC_CYCLES_PER_SECOND;
const MINUTES_PER_DAY: u16 = 1440;
const DAY_MASK: u16 = 0x0FFF;

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Huc3Clock {
    pub minutes: u16,
    pub days: u16,
    pub alarm_minutes: u16,
    pub alarm_days: u16,
    pub alarm_enabled: bool,
    pub sub_minute_cycles: u32,
}

impl Huc3Clock {
    pub fn cycle(&mut self) {
        self.sub_minute_cycles += 1;
        if self.sub_minute_cycles >= MINUTE_CYCLES {
            self.sub_minute_cycles = 0;
            self.add_minutes(1);
        }
    }

    pub fn advance_seconds(&mut self, seconds: u64) {
        let cycles = self.sub_minute_cycles as u64 + (seconds % 60) * RTC_CYCLES_PER_SECOND as u64;
        let minutes = seconds / 60 + cycles / MINUTE_CYCLES as u64;
        self.sub_minute_cycles = (cycles % MINUTE_CYCLES as u64) as u32;
        self.add_minutes(minutes);
    }

    fn add_minutes(&mut self, minutes: u64) {
        let total = self.minutes as u64 + minutes;
        let days = total / MINUTES_PER_DAY as u64;
        self.minutes = (total % MINUTES_PER_DAY as u64) as u16;
        self.days = ((self.days as u64 + days) & DAY_MASK as u64) as u16;
    }

    pub fn encode(&self, now: u64) -> [u8; HUC3_FOOTER_LEN] {
        let mut out = [0u8; HUC3_FOOTER_LEN];
        out[0..8].copy_from_slice(&now.to_le_bytes());
        out[8..10].copy_from_slice(&self.minutes.to_le_bytes());
        out[10..12].copy_from_slice(&self.days.to_le_bytes());
        out[12..14].copy_from_slice(&self.alarm_minutes.to_le_bytes());
        out[14..16].copy_from_slice(&self.alarm_days.to_le_bytes());
        out[16] = u8::from(self.alarm_enabled);
        out
    }

    pub fn decode(data: &[u8], now: u64) -> Option<Self> {
        if data.len() < HUC3_FOOTER_LEN {
            return None;
        }
        let word = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]);
        let saved = u64::from_le_bytes(data[0..8].try_into().ok()?);
        if saved > now {
            return None;
        }
        let mut clock = Self {
            minutes: word(8) % MINUTES_PER_DAY,
            days: word(10) & DAY_MASK,
            alarm_minutes: word(12),
            alarm_days: word(14),
            alarm_enabled: data[16] != 0,
            sub_minute_cycles: 0,
        };
        clock.advance_seconds(now - saved);
        Some(clock)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Huc3 {
    pub rom_bank_count: usize,
    pub ram_bank_count: usize,
    pub rom_bank_register: u8,
    pub ram_bank_register: u8,
    pub mode: u8,
    pub mailbox: u8,
    pub last_command: u8,
    pub result: u8,
    pub pointer: u8,
    pub memory: Vec<u8>,
    pub tone_requests: u8,
    pub clock: Huc3Clock,
    pub led: bool,
    pub light: bool,
}

impl Huc3 {
    pub fn new(rom_bank_count: usize, ram_bank_count: usize) -> Self {
        Self {
            rom_bank_count,
            ram_bank_count,
            rom_bank_register: 1,
            ram_bank_register: 0,
            mode: 0,
            mailbox: 0,
            last_command: 0,
            result: 0,
            pointer: 0,
            memory: vec![0; 0x100],
            tone_requests: 0,
            clock: Huc3Clock::default(),
            led: false,
            light: false,
        }
    }

    pub fn cycle(&mut self) {
        self.clock.cycle();
    }

    fn nibble(&self, address: u8) -> u8 {
        let shift = |value: u16, index: u8| ((value >> (4 * index)) & 0x0F) as u8;
        match address {
            0x10..=0x12 => shift(self.clock.minutes, address - 0x10),
            0x13..=0x15 => shift(self.clock.days, address - 0x13),
            0x58..=0x5A => shift(self.clock.alarm_minutes, address - 0x58),
            0x5B..=0x5D => shift(self.clock.alarm_days, address - 0x5B),
            _ => self.memory[address as usize],
        }
    }

    fn set_nibble(&mut self, address: u8, value: u8) {
        let put = |field: &mut u16, index: u8| {
            let shift = 4 * index;
            *field = (*field & !(0x0F << shift)) | ((value as u16 & 0x0F) << shift);
        };
        match address {
            0x10..=0x12 => put(&mut self.clock.minutes, address - 0x10),
            0x13..=0x15 => put(&mut self.clock.days, address - 0x13),
            0x58..=0x5A => put(&mut self.clock.alarm_minutes, address - 0x58),
            0x5B..=0x5D => put(&mut self.clock.alarm_days, address - 0x5B),
            _ => self.memory[address as usize] = value & 0x0F,
        }
    }

    fn execute(&mut self) {
        let command = (self.mailbox >> 4) & 0x07;
        let argument = self.mailbox & 0x0F;
        self.last_command = command;
        match command {
            0x1 => {
                self.result = self.nibble(self.pointer);
                self.pointer = self.pointer.wrapping_add(1);
            }
            0x3 => {
                self.set_nibble(self.pointer, argument);
                self.pointer = self.pointer.wrapping_add(1);
            }
            0x4 => self.pointer = (self.pointer & 0xF0) | argument,
            0x5 => self.pointer = (self.pointer & 0x0F) | (argument << 4),
            0x6 => self.extended(argument),
            _ => {}
        }
    }

    fn extended(&mut self, argument: u8) {
        match argument {
            0x0 => {
                for index in 0..6 {
                    self.memory[index as usize] = self.nibble(0x10 + index);
                }
            }
            0x1 => {
                for index in 0..6 {
                    self.set_nibble(0x10 + index, self.memory[index as usize]);
                }
                self.clock.minutes %= MINUTES_PER_DAY;
                self.clock.days &= DAY_MASK;
            }
            0x2 => self.result = 0x1,
            0xE => self.tone_requests = self.tone_requests.wrapping_add(1),
            _ => {}
        }
    }
}

impl MbcInterface for Huc3 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        match addr {
            0x0000..=0x1FFF => self.mode = value & 0x0F,
            0x2000..=0x3FFF => self.rom_bank_register = value & 0x7F,
            0x4000..=0x5FFF => self.ram_bank_register = value & 0x03,
            0xA000..=0xBFFF => match self.mode {
                0x0B => self.mailbox = value & 0x7F,
                0x0D if value & 0x01 == 0 => {
                    self.execute();
                    return true;
                }
                0x0E => self.led = value & 0x01 != 0,
                _ => {}
            },
            _ => {}
        }
        false
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        if !(0xA000..=0xBFFF).contains(&addr) {
            return None;
        }
        match self.mode {
            0x00 | 0x0A => None,
            0x0C => Some(0x80 | (self.last_command << 4) | self.result),
            0x0D => Some(0x81),
            0x0E => Some(0xC0 | u8::from(self.light)),
            _ => Some(0xFF),
        }
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        mask_bank_number(self.rom_bank_register as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        match self.mode {
            0x00 | 0x0A => Some(mask_bank_number(
                self.ram_bank_register as usize,
                self.ram_bank_count.max(1),
            )),
            _ => None,
        }
    }

    fn ram_write_offset(&self, addr: u16) -> Option<usize> {
        if self.mode == 0x0A {
            self.ram_offset(addr)
        } else {
            None
        }
    }

    fn soft_reset(&mut self) {
        let clock = std::mem::take(&mut self.clock);
        let memory = std::mem::take(&mut self.memory);
        let light = self.light;
        *self = Self::new(self.rom_bank_count, self.ram_bank_count);
        self.clock = clock;
        self.memory = memory;
        self.light = light;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mbc: &mut Huc3, command: u8, argument: u8) -> u8 {
        mbc.on_write(0x0000, 0x0D);
        assert_eq!(mbc.on_read(0xA000).unwrap() & 1, 1);
        mbc.on_write(0x0000, 0x0B);
        mbc.on_write(0xA000, (command << 4) | argument);
        mbc.on_write(0x0000, 0x0D);
        mbc.on_write(0xA000, 0xFE);
        mbc.on_write(0x0000, 0x0C);
        mbc.on_read(0xA000).unwrap() & 0x0F
    }

    fn point(mbc: &mut Huc3, address: u8) {
        run(mbc, 0x4, address & 0x0F);
        run(mbc, 0x5, address >> 4);
    }

    fn read_time(mbc: &mut Huc3) -> (u16, u16) {
        run(mbc, 0x6, 0x0);
        point(mbc, 0x00);
        let nibbles: Vec<u16> = (0..6).map(|_| run(mbc, 0x1, 0) as u16).collect();
        let minutes = nibbles[0] | nibbles[1] << 4 | nibbles[2] << 8;
        let days = nibbles[3] | nibbles[4] << 4 | nibbles[5] << 8;
        (minutes, days)
    }

    #[test]
    fn ram_writes_need_mode_a_and_reads_work_in_mode_0() {
        let mut mbc = Huc3::new(128, 4);
        mbc.on_write(0x4000, 0x02);
        assert_eq!(mbc.ram_offset(0xA010), Some(2 * 0x2000 + 0x10));
        assert_eq!(mbc.ram_write_offset(0xA010), None);
        mbc.on_write(0x0000, 0x0A);
        assert_eq!(mbc.ram_write_offset(0xA010), Some(2 * 0x2000 + 0x10));
        mbc.on_write(0x0000, 0x07);
        assert_eq!(mbc.ram_offset(0xA010), None);
        assert_eq!(mbc.on_read(0xA010), Some(0xFF));
    }

    #[test]
    fn rom_bank_zero_is_mappable() {
        let mut mbc = Huc3::new(128, 4);
        mbc.on_write(0x2000, 0x00);
        assert_eq!(mbc.rom_bank_high(), 0);
        mbc.on_write(0x2000, 0xFF);
        assert_eq!(mbc.rom_bank_high(), 0x7F);
    }

    #[test]
    fn status_query_answers_one() {
        let mut mbc = Huc3::new(128, 4);
        assert_eq!(run(&mut mbc, 0x6, 0x2), 0x1);
        mbc.on_write(0x0000, 0x0C);
        assert_eq!(mbc.on_read(0xA000), Some(0x80 | 0x60 | 0x1));
    }

    #[test]
    fn nibbles_write_and_read_back_through_the_pointer() {
        let mut mbc = Huc3::new(128, 4);
        point(&mut mbc, 0x40);
        run(&mut mbc, 0x3, 0x7);
        run(&mut mbc, 0x3, 0xC);
        point(&mut mbc, 0x40);
        assert_eq!(run(&mut mbc, 0x1, 0), 0x7);
        assert_eq!(run(&mut mbc, 0x1, 0), 0xC);
        assert_eq!(mbc.pointer, 0x42);
    }

    #[test]
    fn set_time_then_read_it_back_after_a_minute() {
        let mut mbc = Huc3::new(128, 4);
        point(&mut mbc, 0x00);
        for nibble in [0xF, 0x5, 0x5, 0x3, 0x0, 0x0] {
            run(&mut mbc, 0x3, nibble);
        }
        run(&mut mbc, 0x6, 0x1);
        assert_eq!((mbc.clock.minutes, mbc.clock.days), (0x55F, 3));
        (0..MINUTE_CYCLES).for_each(|_| mbc.cycle());
        assert_eq!(read_time(&mut mbc), (0x560, 3));
    }

    #[test]
    fn the_day_rolls_over_at_1440_minutes() {
        let mut clock = Huc3Clock {
            minutes: 1439,
            days: 9,
            ..Huc3Clock::default()
        };
        clock.advance_seconds(60);
        assert_eq!((clock.minutes, clock.days), (0, 10));
        clock.advance_seconds(3 * 86_400 + 30);
        assert_eq!((clock.minutes, clock.days), (0, 13));
        clock.advance_seconds(30);
        assert_eq!((clock.minutes, clock.days), (1, 13));
    }

    #[test]
    fn footer_is_the_sameboy_layout_and_catches_up() {
        let clock = Huc3Clock {
            minutes: 600,
            days: 42,
            alarm_minutes: 0x123,
            alarm_days: 7,
            alarm_enabled: true,
            sub_minute_cycles: 0,
        };
        let bytes = clock.encode(1_700_000_000);
        assert_eq!(bytes.len(), 17);
        assert_eq!(&bytes[0..8], &1_700_000_000u64.to_le_bytes());
        assert_eq!(&bytes[8..10], &600u16.to_le_bytes());
        let back = Huc3Clock::decode(&bytes, 1_700_000_000 + 86_400 + 120).unwrap();
        assert_eq!((back.minutes, back.days), (602, 43));
        assert_eq!((back.alarm_minutes, back.alarm_days), (0x123, 7));
        assert!(back.alarm_enabled);
        assert!(Huc3Clock::decode(&bytes, 1_600_000_000).is_none());
    }

    #[test]
    fn ir_mode_reads_light_and_sets_led() {
        let mut mbc = Huc3::new(128, 4);
        mbc.on_write(0x0000, 0x0E);
        assert_eq!(mbc.on_read(0xA000), Some(0xC0));
        mbc.light = true;
        assert_eq!(mbc.on_read(0xA000), Some(0xC1));
        mbc.on_write(0xA000, 0x01);
        assert!(mbc.led);
    }
}
