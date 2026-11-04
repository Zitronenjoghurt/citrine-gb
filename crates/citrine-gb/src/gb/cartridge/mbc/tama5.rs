use crate::gb::cartridge::mbc::mbc3::RTC_CYCLES_PER_SECOND;
use crate::gb::cartridge::mbc::{MbcInterface, mask_bank_number};

pub const TAMA5_RAM_SIZE: usize = 32;
pub const TAMA5_SAVE_LEN: usize = TAMA5_RAM_SIZE + 4 * 8 + 8;
const TIMER: usize = 0;
const ALARM: usize = 1;
const MAX_CATCH_UP_SECONDS: u64 = 10 * 366 * 86_400;

const ROM_LOW: u8 = 0x0;
const ROM_HIGH: u8 = 0x1;
const WRITE_LOW: u8 = 0x4;
const WRITE_HIGH: u8 = 0x5;
const ADDRESS_HIGH: u8 = 0x6;
const ADDRESS_LOW: u8 = 0x7;
const READ_LOW: u8 = 0xC;
const READ_HIGH: u8 = 0xD;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tama5 {
    pub rom_bank_count: usize,
    pub selected: u8,
    pub registers: [u8; 8],
    pub output: u8,
    pub ram: Vec<u8>,
    pub pages: [[u8; 16]; 4],
    pub timer_enabled: bool,
    pub alarm_enabled: bool,
    pub sub_second_cycles: u32,
}

impl Tama5 {
    pub fn new(rom_bank_count: usize) -> Self {
        let mut pages = [[0u8; 16]; 4];
        pages[TIMER][7] = 1;
        pages[TIMER][9] = 1;
        pages[ALARM][0xA] = 1;
        Self {
            rom_bank_count,
            selected: 0,
            registers: [0; 8],
            output: 0,
            ram: vec![0; TAMA5_RAM_SIZE],
            pages,
            timer_enabled: true,
            alarm_enabled: false,
            sub_second_cycles: 0,
        }
    }

    pub fn cycle(&mut self) {
        if !self.timer_enabled {
            return;
        }
        self.sub_second_cycles += 1;
        if self.sub_second_cycles >= RTC_CYCLES_PER_SECOND {
            self.sub_second_cycles = 0;
            self.tick();
        }
    }

    pub fn advance_seconds(&mut self, seconds: u64) {
        if !self.timer_enabled {
            return;
        }
        for _ in 0..seconds.min(MAX_CATCH_UP_SECONDS) {
            self.tick();
        }
    }

    fn field(&self, index: usize) -> u8 {
        let page = &self.pages[TIMER];
        page[index] + 10 * page[index + 1]
    }

    fn set_field(&mut self, index: usize, value: u8) {
        self.pages[TIMER][index] = value % 10;
        self.pages[TIMER][index + 1] = value / 10;
    }

    fn days_in_month(&self, month: u8) -> u8 {
        match month {
            2 if self.pages[ALARM][0xB] & 0x03 == 0 => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }

    fn tick(&mut self) {
        let second = self.field(0) + 1;
        if second < 60 {
            return self.set_field(0, second);
        }
        self.set_field(0, 0);
        let minute = self.field(2) + 1;
        if minute < 60 {
            return self.set_field(2, minute);
        }
        self.set_field(2, 0);
        let hour = self.field(4) + 1;
        if hour < 24 {
            return self.set_field(4, hour);
        }
        self.set_field(4, 0);
        self.pages[TIMER][6] = (self.pages[TIMER][6] + 1) % 7;
        let month = self.field(9).clamp(1, 12);
        let day = self.field(7) + 1;
        if day <= self.days_in_month(month) {
            return self.set_field(7, day);
        }
        self.set_field(7, 1);
        if month < 12 {
            return self.set_field(9, month + 1);
        }
        self.set_field(9, 1);
        self.set_field(11, (self.field(11) + 1) % 100);
        let leap = &mut self.pages[ALARM][0xB];
        *leap = (*leap & !0x03) | ((*leap + 1) & 0x03);
    }

    fn run(&mut self) {
        let high = self.registers[ADDRESS_HIGH as usize];
        let low = self.registers[ADDRESS_LOW as usize];
        let address = (((high << 4) & 0x10) | low) as usize;
        let value = (self.registers[WRITE_HIGH as usize] << 4) | self.registers[WRITE_LOW as usize];
        match high >> 1 {
            0 => self.ram[address] = value,
            1 => self.output = self.ram[address],
            2 => self.misc(address as u8, value),
            4 => self.rtc(low, value),
            _ => {}
        }
    }

    fn misc(&mut self, command: u8, value: u8) {
        let bcd = |value: u8| (value >> 4) * 10 + (value & 0x0F);
        let to_bcd = |value: u8| ((value / 10) << 4) | (value % 10);
        match command {
            0x00 => self.timer_enabled = false,
            0x01 => {
                self.timer_enabled = true;
                self.sub_second_cycles = 0;
            }
            0x04 => self.set_field(2, bcd(value).min(59)),
            0x05 => self.set_field(4, bcd(value).min(23)),
            0x06 => self.output = to_bcd(self.field(2)),
            0x07 => self.output = to_bcd(self.field(4)),
            0x10 => self.alarm_enabled = false,
            0x11 => self.alarm_enabled = true,
            _ => {}
        }
    }

    fn rtc(&mut self, page_select: u8, value: u8) {
        let register = (value & 0x0F) as usize;
        if register >= 0x0D {
            return;
        }
        let page = ((page_select >> 1) & 0x03) as usize;
        if page_select & 0x01 == 0 {
            self.pages[page][register] = value >> 4;
        } else {
            self.output = 0xF0 | self.pages[page][register];
        }
    }

    pub fn save(&self, now: u64) -> Vec<u8> {
        let mut out = self.ram.clone();
        for page in &self.pages {
            out.extend(page.chunks(2).map(|pair| (pair[0] & 0x0F) | (pair[1] << 4)));
        }
        out.extend_from_slice(&now.to_le_bytes());
        out
    }

    pub fn load(&mut self, data: &[u8], now: u64) {
        let ram_len = data.len().min(TAMA5_RAM_SIZE);
        self.ram[..ram_len].copy_from_slice(&data[..ram_len]);
        if data.len() < TAMA5_SAVE_LEN {
            return;
        }
        for (page, bytes) in self
            .pages
            .iter_mut()
            .zip(data[TAMA5_RAM_SIZE..TAMA5_RAM_SIZE + 32].chunks(8))
        {
            for (pair, byte) in page.chunks_mut(2).zip(bytes) {
                pair[0] = byte & 0x0F;
                pair[1] = byte >> 4;
            }
        }
        let at = TAMA5_RAM_SIZE + 32;
        let saved = u64::from_le_bytes(data[at..at + 8].try_into().unwrap_or([0; 8]));
        if saved <= now {
            self.advance_seconds(now - saved);
        }
    }
}

impl MbcInterface for Tama5 {
    fn on_write(&mut self, addr: u16, value: u8) -> bool {
        if !(0xA000..=0xBFFF).contains(&addr) {
            return false;
        }
        if addr & 0x01 != 0 {
            self.selected = value & 0x0F;
            return false;
        }
        let value = value & 0x0F;
        if let Some(register) = self.registers.get_mut(self.selected as usize) {
            *register = value;
        }
        match self.selected {
            ROM_LOW | ROM_HIGH => false,
            ADDRESS_LOW => {
                self.run();
                true
            }
            _ => false,
        }
    }

    fn on_read(&self, addr: u16) -> Option<u8> {
        if !(0xA000..=0xBFFF).contains(&addr) {
            return None;
        }
        if addr & 0x01 != 0 {
            return Some(0xFF);
        }
        Some(match self.selected {
            READ_LOW => 0xF0 | (self.output & 0x0F),
            READ_HIGH => 0xF0 | (self.output >> 4),
            _ => 0xF1,
        })
    }

    fn rom_bank_low(&self) -> usize {
        0
    }

    fn rom_bank_high(&self) -> usize {
        let bank = self.registers[ROM_LOW as usize] | (self.registers[ROM_HIGH as usize] << 4);
        mask_bank_number(bank as usize, self.rom_bank_count)
    }

    fn ram_bank(&self) -> Option<usize> {
        None
    }

    fn soft_reset(&mut self) {
        let ram = std::mem::take(&mut self.ram);
        let pages = self.pages;
        let timer_enabled = self.timer_enabled;
        *self = Self::new(self.rom_bank_count);
        self.ram = ram;
        self.pages = pages;
        self.timer_enabled = timer_enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(mbc: &mut Tama5, register: u8, value: u8) {
        mbc.on_write(0xA001, register);
        mbc.on_write(0xA000, value);
    }

    fn get(mbc: &mut Tama5, register: u8) -> u8 {
        mbc.on_write(0xA001, register);
        mbc.on_read(0xA000).unwrap()
    }

    fn command(mbc: &mut Tama5, high: u8, low: u8, value: u8) {
        set(mbc, WRITE_LOW, value & 0x0F);
        set(mbc, WRITE_HIGH, value >> 4);
        set(mbc, ADDRESS_HIGH, high);
        set(mbc, ADDRESS_LOW, low);
    }

    fn output(mbc: &mut Tama5) -> u8 {
        (get(mbc, READ_LOW) & 0x0F) | (get(mbc, READ_HIGH) << 4)
    }

    #[test]
    fn handshake_reads_f1() {
        let mut mbc = Tama5::new(32);
        mbc.on_write(0xA001, 0x0A);
        assert_eq!(mbc.on_read(0xA000), Some(0xF1));
        assert_eq!(mbc.on_read(0xA001), Some(0xFF));
    }

    #[test]
    fn rom_bank_comes_from_two_nibbles() {
        let mut mbc = Tama5::new(32);
        set(&mut mbc, ROM_LOW, 0x5);
        set(&mut mbc, ROM_HIGH, 0x1);
        assert_eq!(mbc.rom_bank_high(), 0x15);
    }

    #[test]
    fn ram_writes_and_reads_through_the_command_registers() {
        let mut mbc = Tama5::new(32);
        command(&mut mbc, 0x01, 0x3, 0xA7);
        assert_eq!(mbc.ram[0x13], 0xA7);
        command(&mut mbc, 0x03, 0x3, 0x00);
        assert_eq!(output(&mut mbc), 0xA7);
        command(&mut mbc, 0x00, 0x2, 0x5C);
        command(&mut mbc, 0x02, 0x2, 0x00);
        assert_eq!(output(&mut mbc), 0x5C);
    }

    #[test]
    fn minutes_and_hours_write_and_read_as_bcd() {
        let mut mbc = Tama5::new(32);
        command(&mut mbc, 0x04, 0x4, 0x42);
        command(&mut mbc, 0x04, 0x5, 0x17);
        command(&mut mbc, 0x04, 0x6, 0x00);
        assert_eq!(output(&mut mbc), 0x42);
        command(&mut mbc, 0x04, 0x7, 0x00);
        assert_eq!(output(&mut mbc), 0x17);
    }

    #[test]
    fn rtc_page_access_writes_and_reads_nibbles() {
        let mut mbc = Tama5::new(32);
        command(&mut mbc, 0x08, 0x2, 0x73);
        assert_eq!(mbc.pages[ALARM][3], 7);
        command(&mut mbc, 0x08, 0x3, 0x03);
        assert_eq!(output(&mut mbc) & 0x0F, 7);
    }

    #[test]
    fn the_clock_ticks_and_carries_into_the_next_month() {
        let mut mbc = Tama5::new(32);
        mbc.set_field(0, 59);
        mbc.set_field(2, 59);
        mbc.set_field(4, 23);
        mbc.set_field(7, 30);
        mbc.set_field(9, 4);
        (0..RTC_CYCLES_PER_SECOND).for_each(|_| mbc.cycle());
        assert_eq!([mbc.field(0), mbc.field(2), mbc.field(4)], [0, 0, 0]);
        assert_eq!([mbc.field(7), mbc.field(9)], [1, 5]);
    }

    #[test]
    fn february_has_29_days_in_a_leap_year() {
        let mut mbc = Tama5::new(32);
        mbc.set_field(4, 23);
        mbc.set_field(2, 59);
        mbc.set_field(0, 59);
        mbc.set_field(7, 28);
        mbc.set_field(9, 2);
        mbc.advance_seconds(1);
        assert_eq!([mbc.field(7), mbc.field(9)], [29, 2]);
        mbc.pages[ALARM][0xB] = 1;
        mbc.set_field(7, 28);
        mbc.set_field(4, 23);
        mbc.set_field(2, 59);
        mbc.set_field(0, 59);
        mbc.advance_seconds(1);
        assert_eq!([mbc.field(7), mbc.field(9)], [1, 3]);
    }

    #[test]
    fn stopping_the_timer_holds_the_time() {
        let mut mbc = Tama5::new(32);
        command(&mut mbc, 0x04, 0x0, 0x00);
        (0..RTC_CYCLES_PER_SECOND).for_each(|_| mbc.cycle());
        assert_eq!(mbc.field(0), 0);
    }

    #[test]
    fn save_holds_ram_four_pages_and_a_timestamp() {
        let mut mbc = Tama5::new(32);
        mbc.ram[0] = 0x99;
        mbc.set_field(2, 10);
        let save = mbc.save(1_000);
        assert_eq!(save.len(), TAMA5_SAVE_LEN);
        assert_eq!(save[0], 0x99);
        assert_eq!(save[TAMA5_RAM_SIZE + 1], 0x10);
        assert_eq!(&save[TAMA5_RAM_SIZE + 32..], &1_000u64.to_le_bytes());
        let mut back = Tama5::new(32);
        back.load(&save, 1_000 + 120);
        assert_eq!(back.ram[0], 0x99);
        assert_eq!(back.field(2), 12);
    }
}
