mod accel;
mod eeprom;
mod lcd;
mod rtc;
mod sci3;
mod ssu;
mod timers;
mod wdt;

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use crate::h8::{Bus, Cpu, FLAG_I, Fault};
use accel::Accel;
pub use eeprom::EEPROM_SIZE;
use eeprom::Eeprom;
use lcd::Lcd;
pub use lcd::{HEIGHT, WIDTH};
use rtc::Rtc;
use sci3::Sci3;
use ssu::{Spi, Ssu};
use timers::{TimerB1, TimerW};
use wdt::{Overflow, Wdt};

pub const CLOCK_HZ: u64 = 3_686_400;
const ACCEL_INT_PERIOD: u64 = CLOCK_HZ / 16;
pub const ROM_SIZE: usize = 0xC000;
const RAM_START: u16 = 0xF780;
const RAM_END: u16 = 0xFF7F;

const PDR1: u16 = 0xFFD4;
const PDR9: u16 = 0xFFDC;
const PDRB: u16 = 0xFFDE;
const IEGR: u16 = 0xFFF2;
const IENR1: u16 = 0xFFF3;
const IENR2: u16 = 0xFFF4;
const IRR1: u16 = 0xFFF6;
const IRR2: u16 = 0xFFF7;
const ADRR: u16 = 0xFFBC;
const ADSR: u16 = 0xFFBF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Enter,
    Right,
}

impl Button {
    fn mask(self) -> u8 {
        match self {
            Button::Enter => 0x01,
            Button::Left => 0x04,
            Button::Right => 0x10,
        }
    }
}

#[derive(Clone, Default)]
struct Motion {
    from: u64,
    until: u64,
    hz: f64,
}

impl Motion {
    fn sample(&self, now: u64) -> [i16; 3] {
        if now < self.from || now >= self.until {
            return [0, 0, 256];
        }
        let t = (now - self.from) as f64 / CLOCK_HZ as f64;
        let phase = TAU * self.hz * t;
        [
            (40.0 * (phase / 2.0).sin()) as i16,
            (60.0 * phase.cos()) as i16,
            (256.0 + 150.0 * phase.sin()) as i16,
        ]
    }
}

#[derive(Clone, Default, Debug)]
pub struct IoCount {
    pub reads: u64,
    pub writes: u64,
}

pub struct Board {
    rom: Vec<u8>,
    ram: Vec<u8>,
    io: Vec<u8>,
    ssu: Ssu,
    eeprom: Eeprom,
    lcd: Lcd,
    accel: Accel,
    sci: Sci3,
    tb1: TimerB1,
    tw: TimerW,
    rtc: Rtc,
    wdt: Wdt,
    wdt_reset: bool,
    motion: Motion,
    buttons: u8,
    pub battery: u16,
    pub now: u64,
    pub unknown_io: BTreeMap<u16, IoCount>,
    pub pc: u16,
    pub io_trace: Option<(u16, u16, Vec<IoAccess>)>,
}

#[derive(Clone, Copy, Debug)]
pub struct IoAccess {
    pub at: u64,
    pub pc: u16,
    pub addr: u16,
    pub value: u8,
    pub write: bool,
}

struct SpiBus<'a> {
    eeprom: &'a mut Eeprom,
    lcd: &'a mut Lcd,
    accel: &'a mut Accel,
    pdr1: u8,
    pdr9: u8,
}

impl Spi for SpiBus<'_> {
    fn exchange(&mut self, mosi: u8) -> u8 {
        if self.pdr1 & 0x04 == 0 {
            self.eeprom.transfer(mosi)
        } else if self.pdr1 & 0x01 == 0 {
            if self.pdr1 & 0x02 != 0 {
                self.lcd.data(mosi);
            } else {
                self.lcd.command(mosi);
            }
            0x00
        } else if self.pdr9 & 0x01 == 0 {
            self.accel.transfer(mosi)
        } else {
            0xFF
        }
    }
}

fn fresh_io() -> Vec<u8> {
    let mut io = vec![0; 0x1000];
    io[(PDR1 - 0xF000) as usize] = 0xFF;
    io[(PDR9 - 0xF000) as usize] = 0xFF;
    io
}

impl Board {
    fn new(rom: Vec<u8>, eeprom: Vec<u8>) -> Self {
        assert_eq!(rom.len(), ROM_SIZE);
        Self {
            rom,
            ram: vec![0; (RAM_END - RAM_START + 1) as usize],
            io: fresh_io(),
            ssu: Ssu::default(),
            eeprom: Eeprom::new(eeprom),
            lcd: Lcd::default(),
            accel: Accel::default(),
            sci: Sci3::default(),
            tb1: TimerB1::default(),
            tw: TimerW::default(),
            rtc: Rtc::default(),
            wdt: Wdt::default(),
            wdt_reset: false,
            motion: Motion::default(),
            buttons: 0,
            battery: 0x2E0,
            now: 0,
            unknown_io: BTreeMap::new(),
            pc: 0,
            io_trace: None,
        }
    }

    pub fn sci_reads(&self) -> u64 {
        self.sci.reads
    }

    fn reg(&self, addr: u16) -> u8 {
        self.io[(addr - 0xF000) as usize]
    }

    fn set_reg(&mut self, addr: u16, v: u8) {
        self.io[(addr - 0xF000) as usize] = v;
    }

    fn spi(&mut self) -> (&mut Ssu, SpiBus<'_>) {
        let pdr1 = self.io[(PDR1 - 0xF000) as usize];
        let pdr9 = self.io[(PDR9 - 0xF000) as usize];
        (
            &mut self.ssu,
            SpiBus {
                eeprom: &mut self.eeprom,
                lcd: &mut self.lcd,
                accel: &mut self.accel,
                pdr1,
                pdr9,
            },
        )
    }

    fn port_write(&mut self, addr: u16, v: u8) {
        let old = self.reg(addr);
        self.set_reg(addr, v);
        let rose = !old & v;
        let fell = old & !v;
        if addr == PDR1 && rose & 0x04 != 0 {
            self.eeprom.deselect();
        }
        if addr == PDR9 {
            if rose & 0x01 != 0 {
                self.accel.deselect();
            }
            if fell & 0x01 != 0 {
                let s = self.motion.sample(self.now);
                self.accel.set_sample(s);
            }
        }
    }

    fn tick(&mut self, cycles: u64) {
        let before = self.now;
        self.now += cycles;
        if !self.accel.asleep() && self.now / ACCEL_INT_PERIOD != before / ACCEL_INT_PERIOD {
            let v = self.reg(IRR1) | 0x02;
            self.set_reg(IRR1, v);
        }
        self.tb1.tick(cycles);
        if self.tb1.overflowed {
            self.tb1.overflowed = false;
            let v = self.reg(IRR2) | 0x04;
            self.set_reg(IRR2, v);
        }
        self.tw.tick(cycles);
        self.rtc.tick(cycles);
        self.sci.tick(self.now);
        if self.wdt.tick(cycles) == Some(Overflow::Reset) {
            self.wdt_reset = true;
        }
    }

    fn reset_on_chip(&mut self) {
        self.io = fresh_io();
        self.ssu = Ssu::default();
        self.sci = self.sci.after_reset();
        self.tb1 = TimerB1::default();
        self.tw = TimerW::default();
        self.wdt = self.wdt.after_reset();
        self.wdt_reset = false;
    }

    fn next_event(&self) -> Option<u64> {
        [
            self.tb1.cycles_to_overflow(),
            self.rtc.cycles_to_tick(),
            self.sci
                .next_event()
                .map(|t| t.saturating_sub(self.now).max(1)),
            self.tw.running().then_some(256),
            self.wdt.cycles_to_overflow(),
            (!self.accel.asleep()).then(|| ACCEL_INT_PERIOD - self.now % ACCEL_INT_PERIOD),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn pending_vector(&self) -> Option<u8> {
        let irr1 = self.reg(IRR1) & self.reg(IENR1);
        let irr2 = self.reg(IRR2) & self.reg(IENR2);
        if irr1 & 0x01 != 0 {
            return Some(16);
        }
        if irr1 & 0x02 != 0 {
            return Some(17);
        }
        if self.reg(IENR1) & 0x80 != 0
            && let Some(bit) = self.rtc.pending()
        {
            return Some(23 + bit);
        }
        if irr2 & 0x04 != 0 {
            return Some(33);
        }
        if self.tw.interrupt() {
            return Some(35);
        }
        if self.wdt.interrupt() {
            return Some(31);
        }
        if self.sci.interrupt() {
            return Some(37);
        }
        if irr2 & 0x40 != 0 {
            return Some(38);
        }
        None
    }

    fn note_unknown(&mut self, addr: u16, write: bool) {
        let c = self.unknown_io.entry(addr).or_default();
        if write {
            c.writes += 1;
        } else {
            c.reads += 1;
        }
    }
}

impl Board {
    fn traced(&mut self, addr: u16, value: u8, write: bool) {
        let (at, pc) = (self.now, self.pc);
        if let Some((lo, hi, log)) = &mut self.io_trace
            && (*lo..=*hi).contains(&addr)
            && (!(RAM_START..=RAM_END).contains(&addr)
                || (RAM_START..=RAM_END).contains(lo) && (RAM_START..=RAM_END).contains(hi))
            && log.len() < 200_000
        {
            log.push(IoAccess {
                at,
                pc,
                addr,
                value,
                write,
            });
        }
    }
}

impl Bus for Board {
    fn read8(&mut self, addr: u16) -> u8 {
        let v = self.read_raw(addr);
        if self.io_trace.is_some() {
            self.traced(addr, v, false);
        }
        v
    }

    fn write8(&mut self, addr: u16, value: u8) {
        if self.io_trace.is_some() {
            self.traced(addr, value, true);
        }
        self.write_raw(addr, value);
    }
}

impl Board {
    fn read_raw(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0xBFFF => self.rom[addr as usize],
            RAM_START..=RAM_END => self.ram[(addr - RAM_START) as usize],
            0xF0E0..=0xF0EF => {
                let (ssu, mut spi) = self.spi();
                ssu.read(addr, &mut spi)
            }
            timers::TMB1 | timers::TCB1 => self.tb1.read(addr),
            timers::TMRW..=timers::GRD => self.tw.read(addr),
            rtc::RTCFLG..=rtc::RTCCSR => self.rtc.read(addr),
            sci3::SMR3..=sci3::RDR3 => self.sci.read(addr),
            wdt::TMWD..=wdt::TCWD => self.wdt.read(addr),
            ADRR => (self.battery >> 2) as u8,
            0xFFBD => (self.battery << 6) as u8,
            PDRB => self.buttons,
            0xF000..=0xF0FF | 0xFF80..=0xFFFF => {
                if !matches!(
                    addr,
                    PDR1 | PDR9 | IEGR | IENR1 | IENR2 | IRR1 | IRR2 | 0xFFBE | ADSR
                ) {
                    self.note_unknown(addr, false);
                }
                self.reg(addr)
            }
            _ => {
                self.note_unknown(addr, false);
                0xFF
            }
        }
    }

    fn write_raw(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0xBFFF => self.note_unknown(addr, true),
            RAM_START..=RAM_END => self.ram[(addr - RAM_START) as usize] = value,
            0xF0E0..=0xF0EF => {
                let (ssu, mut spi) = self.spi();
                ssu.write(addr, value, &mut spi);
            }
            timers::TMB1 | timers::TCB1 => self.tb1.write(addr, value),
            timers::TMRW..=timers::GRD => self.tw.write(addr, value),
            rtc::RTCFLG..=rtc::RTCCSR => self.rtc.write(addr, value),
            sci3::SMR3..=sci3::RDR3 => self.sci.write(addr, value, self.now),
            wdt::TMWD..=wdt::TCWD => self.wdt.write(addr, value),
            PDR1 | PDR9 => self.port_write(addr, value),
            IRR1 | IRR2 => {
                let v = self.reg(addr) & value;
                self.set_reg(addr, v);
            }
            ADSR => {
                if value & 0x80 != 0 {
                    let v = self.reg(IRR2) | 0x40;
                    self.set_reg(IRR2, v);
                }
                self.set_reg(addr, value & 0x7F);
            }
            0xF000..=0xF0FF | 0xFF80..=0xFFFF => {
                if !matches!(addr, IEGR | IENR1 | IENR2 | 0xFFBE) {
                    self.note_unknown(addr, true);
                }
                self.set_reg(addr, value);
            }
            _ => self.note_unknown(addr, true),
        }
    }
}

pub struct Walker {
    pub cpu: Cpu,
    pub board: Board,
    trace: Vec<u16>,
    pub watchdog_resets: Vec<u64>,
    pub hits: Vec<(u16, u64)>,
}

impl Walker {
    pub fn new(rom: Vec<u8>, eeprom: Vec<u8>) -> Self {
        let mut board = Board::new(rom, eeprom);
        let mut cpu = Cpu::default();
        cpu.reset(&mut board);
        Self {
            cpu,
            board,
            trace: vec![0; 64],
            watchdog_resets: Vec::new(),
            hits: Vec::new(),
        }
    }

    pub fn now(&self) -> u64 {
        self.board.now
    }

    pub fn recent_pcs(&self) -> Vec<u16> {
        let i = (self.cpu.cycles as usize) % self.trace.len();
        self.trace[i..]
            .iter()
            .chain(&self.trace[..i])
            .copied()
            .collect()
    }

    pub fn run_cycles(&mut self, cycles: u64) -> Result<(), Fault> {
        self.run(cycles, None).map(|_| ())
    }

    fn run(&mut self, cycles: u64, stop: Option<u16>) -> Result<bool, Fault> {
        let end = self.board.now + cycles;
        let mut slot = 0usize;
        while self.board.now < end {
            if self.cpu.ccr & FLAG_I == 0
                && let Some(v) = self.board.pending_vector()
            {
                let before = self.cpu.cycles;
                self.cpu.interrupt(&mut self.board, v);
                let spent = self.cpu.cycles - before;
                self.board.tick(spent);
                self.take_watchdog_reset();
                continue;
            }
            if self.cpu.sleeping {
                let wait = self
                    .board
                    .next_event()
                    .unwrap_or(u64::MAX)
                    .min(end - self.board.now)
                    .max(1);
                self.board.tick(wait);
                self.take_watchdog_reset();
                continue;
            }
            self.trace[slot] = self.cpu.pc;
            for (at, count) in &mut self.hits {
                if *at == self.cpu.pc {
                    *count += 1;
                }
            }
            self.board.pc = self.cpu.pc;
            slot = (slot + 1) % self.trace.len();
            let spent = self.cpu.step(&mut self.board)?;
            self.board.tick(spent as u64);
            self.take_watchdog_reset();
            if stop == Some(self.cpu.pc) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn take_watchdog_reset(&mut self) {
        if self.board.wdt_reset {
            self.board.reset_on_chip();
            self.cpu.reset(&mut self.board);
            self.watchdog_resets.push(self.board.now);
        }
    }

    pub fn run_seconds(&mut self, seconds: f64) -> Result<(), Fault> {
        self.run_cycles((seconds * CLOCK_HZ as f64) as u64)
    }

    pub fn run_until(&mut self, pc: u16, seconds: f64) -> Result<bool, Fault> {
        self.run((seconds * CLOCK_HZ as f64) as u64, Some(pc))
    }

    pub fn set_button(&mut self, button: Button, down: bool) {
        let was = self.board.buttons & button.mask() != 0;
        if down {
            self.board.buttons |= button.mask();
        } else {
            self.board.buttons &= !button.mask();
        }
        if button == Button::Enter && down && !was {
            let v = self.board.reg(IRR1) | 0x01;
            self.board.set_reg(IRR1, v);
        }
    }

    pub fn press(&mut self, button: Button) -> Result<(), Fault> {
        self.set_button(button, true);
        self.run_seconds(0.2)?;
        self.set_button(button, false);
        self.run_seconds(0.3)
    }

    pub fn walk(&mut self, steps: u32, steps_per_second: f64) {
        let now = self.board.now;
        let from = self.board.motion.until.max(now);
        self.board.motion = Motion {
            from,
            until: from + (steps as f64 / steps_per_second * CLOCK_HZ as f64) as u64,
            hz: steps_per_second,
        };
    }

    pub fn walking_until(&self) -> u64 {
        self.board.motion.until
    }

    pub fn ir_receive(&mut self, bytes: &[u8]) {
        let now = self.board.now;
        self.board.sci.receive(bytes, now);
    }

    pub fn ir_pending(&self) -> usize {
        self.board.sci.rx_pending()
    }

    pub fn ir_overruns(&self) -> u64 {
        self.board.sci.overruns
    }

    pub fn ir_dropped(&self) -> u64 {
        self.board.sci.dropped
    }

    pub fn ir_take(&mut self) -> Vec<(u64, u8)> {
        std::mem::take(&mut self.board.sci.tx)
    }

    pub fn frame(&self) -> Vec<u8> {
        self.board.lcd.frame()
    }

    pub fn lcd_commands(&self) -> &[u8] {
        &self.board.lcd.commands
    }

    pub fn eeprom(&self) -> &[u8] {
        &self.board.eeprom.data
    }

    pub fn take_eeprom_reads(&mut self) -> Vec<(u16, u16)> {
        std::mem::take(&mut self.board.eeprom.reads)
    }

    pub fn accel_regs(&self) -> &[u8] {
        self.board.accel.regs()
    }

    pub fn eeprom_writes(&self) -> u64 {
        self.board.eeprom.writes
    }

    pub fn ram(&self) -> &[u8] {
        &self.board.ram
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        self.board.read8(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with(code: &[u8]) -> Vec<u8> {
        let mut rom = vec![0; ROM_SIZE];
        rom[0..2].copy_from_slice(&[0x01, 0x00]);
        rom[0x100..0x100 + code.len()].copy_from_slice(code);
        rom
    }

    #[test]
    fn eeprom_read_through_ssu_and_chip_select() {
        let code = [
            0x79, 0x07, 0xFF, 0x80, 0xF8, 0xC0, 0x6A, 0x88, 0xF0, 0xE3, 0xF8, 0xFB, 0x38, 0xD4,
            0xF8, 0x03, 0x6A, 0x88, 0xF0, 0xEB, 0xF8, 0x12, 0x6A, 0x88, 0xF0, 0xEB, 0xF8, 0x34,
            0x6A, 0x88, 0xF0, 0xEB, 0x6A, 0x88, 0xF0, 0xEB, 0x6A, 0x09, 0xF0, 0xE9, 0xF8, 0xFF,
            0x38, 0xD4, 0x40, 0xFE,
        ];
        let mut eeprom = vec![0; EEPROM_SIZE];
        eeprom[0x1234] = 0x5A;
        let mut w = Walker::new(rom_with(&code), eeprom);
        w.run_cycles(400).unwrap();
        assert_eq!(w.cpu.r8(0x9), 0x5A);
    }

    #[test]
    fn timer_b1_interrupt_wakes_sleep() {
        let code = [
            0xF8, 0x9E, 0x38, 0xB1, 0xF8, 0xA2, 0x38, 0xB1, 0x79, 0x07, 0xFF, 0x80, 0xF8, 0xF8,
            0x6A, 0x88, 0xF0, 0xD1, 0xF8, 0xC7, 0x6A, 0x88, 0xF0, 0xD0, 0xF8, 0x04, 0x38, 0xF4,
            0x06, 0x7F, 0x01, 0x80, 0x0A, 0x09, 0x40, 0xFA,
        ];
        let mut rom = rom_with(&code);
        rom[0x42..0x44].copy_from_slice(&[0x02, 0x00]);
        rom[0x200..0x206].copy_from_slice(&[0x7F, 0xF7, 0x72, 0x20, 0x56, 0x70]);
        let mut w = Walker::new(rom, vec![0xFF; EEPROM_SIZE]);
        w.run_seconds(1.01).unwrap();
        assert_eq!(w.cpu.r8(0x9), 16);
    }

    #[test]
    fn an_awake_accelerometer_pulses_irq1_at_sixteen_hertz() {
        let code = [0x40, 0xFE];
        let mut w = Walker::new(rom_with(&code), vec![0xFF; EEPROM_SIZE]);
        w.run_cycles(ACCEL_INT_PERIOD + 10).unwrap();
        assert_eq!(w.board.reg(IRR1) & 0x02, 0x02);
        w.board.set_reg(IRR1, 0);
        w.board.accel.transfer(0x0A);
        w.board.accel.transfer(0x01);
        w.board.accel.deselect();
        w.run_seconds(1.0).unwrap();
        assert_eq!(w.board.reg(IRR1) & 0x02, 0);
    }

    #[test]
    fn watchdog_overflow_restarts_the_program_and_keeps_ram() {
        let code = [
            0x6A, 0x08, 0xF7, 0x80, 0x0A, 0x08, 0x6A, 0x88, 0xF7, 0x80, 0x01, 0x80, 0x40, 0xFC,
        ];
        let mut w = Walker::new(rom_with(&code), vec![0xFF; EEPROM_SIZE]);
        w.run_seconds(1.2).unwrap();
        assert_eq!(w.watchdog_resets.len(), 2);
        assert_eq!(w.read(0xF780), 3);
        assert_eq!(w.read(wdt::TCSRWD1) & 0x01, 0x01);
    }

    #[test]
    fn sci3_bytes_go_out_and_come_in() {
        let code = [
            0x79, 0x07, 0xFF, 0x80, 0xF8, 0x00, 0x38, 0x99, 0xF8, 0x30, 0x38, 0x9A, 0xF8, 0x56,
            0x38, 0x9B, 0x7E, 0x9C, 0x73, 0x60, 0x47, 0xFA, 0x28, 0x9D, 0x40, 0xFE,
        ];
        let mut w = Walker::new(rom_with(&code), vec![0xFF; EEPROM_SIZE]);
        w.run_cycles(1000).unwrap();
        assert_eq!(
            w.ir_take().iter().map(|&(_, b)| b).collect::<Vec<_>>(),
            [0x56]
        );
        w.ir_receive(&[0xA5]);
        w.run_cycles(1000).unwrap();
        assert_eq!(w.cpu.r8(0x8), 0xA5);
    }

    #[test]
    fn walking_moves_the_accelerometer() {
        let m = Motion {
            from: 0,
            until: CLOCK_HZ,
            hz: 2.0,
        };
        assert_eq!(m.sample(CLOCK_HZ * 2), [0, 0, 256]);
        assert!(m.sample(CLOCK_HZ / 8)[2] > 350);
    }
}
