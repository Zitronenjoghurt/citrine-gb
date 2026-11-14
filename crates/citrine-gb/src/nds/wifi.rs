pub mod air;
mod baseband;
pub mod calibration;
mod clock;
pub mod frame;
mod link;
mod mp;
mod power;
mod radio;
pub mod registers;
mod rx;
mod timers;
mod tx;

use self::baseband::Baseband;
use self::calibration::Calibration;
use self::clock::MicroClock;
use self::frame::Frame;
use self::link::{Exchange, Link};
use self::power::Power;
use self::radio::Radio;
use self::registers::*;
use super::interrupt::Irq;
use super::rtc::CYCLES_PER_SECOND;
use super::spi::firmware::Firmware;
use crate::traffic::{Entry, Kind, Traffic};

pub const BASE: u32 = 0x0480_0000;
pub const SIZE: u32 = 0x1_0000;
pub const RAM_BYTES: usize = 0x2000;

const WINDOW: usize = 0x7FFE;
const REGISTER_BYTES: usize = 0x1000;
const PASSIVE_END: usize = 0x2000;
const RAM_START: usize = 0x4000;
const RAM_END: usize = 0x6000;
const RAM_MASK: usize = RAM_BYTES - 2;
const ID_DS: u16 = 0x1440;
const ID_LITE: u16 = 0xC340;
const RANDOM_PERIOD: usize = 0x5FD;
const RANDOM_BITS: u16 = 0x7FF;
const IF_UNUSED: u16 = 1 << 10;
const MAX_SETTLE: usize = 64;

pub const IRQ_RX_DONE: u16 = 1 << 0;
pub const IRQ_TX_DONE: u16 = 1 << 1;
pub const IRQ_RX_START: u16 = 1 << 6;
pub const IRQ_TX_START: u16 = 1 << 7;
pub const IRQ_TX_COUNT: u16 = 1 << 8;
pub const IRQ_RX_COUNT: u16 = 1 << 9;
pub const IRQ_WAKE: u16 = 1 << 11;
pub const IRQ_MP_END: u16 = 1 << 12;
pub const IRQ_POST_BEACON: u16 = 1 << 13;
pub const IRQ_BEACON: u16 = 1 << 14;
pub const IRQ_PRE_BEACON: u16 = 1 << 15;

static RANDOM: [u16; RANDOM_PERIOD] = random_table();

const fn random_table() -> [u16; RANDOM_PERIOD] {
    let mut table = [0; RANDOM_PERIOD];
    let mut value: u16 = 1;
    let mut index = 0;
    while index < RANDOM_PERIOD {
        table[index] = value;
        let rotated = ((value << 1) | (value >> 10)) & RANDOM_BITS;
        value = (value & 1) ^ rotated;
        index += 1;
    }
    table
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Wifi {
    regs: Vec<u16>,
    ram: Vec<u8>,
    calibration: Calibration,
    baseband: Baseband,
    radio: Radio,
    channel: u8,
    clock: MicroClock,
    powered: bool,
    now: u64,
    us_count: u64,
    cmd_count: u32,
    block_beacon: bool,
    ms_due: bool,
    pre_fired: Option<u64>,
    random: u16,
    power: Power,
    link: Link,
    exchange: Option<Exchange>,
    #[cfg_attr(feature = "serde", serde(with = "crate::utils::retired"))]
    retired_last_cmd: (),
    line: bool,
    raised: bool,
    outbox: Vec<Frame>,
    inbox: Vec<Frame>,
    #[cfg_attr(feature = "serde", serde(skip))]
    traffic: Traffic,
}

impl Wifi {
    pub fn new(firmware: &Firmware) -> Self {
        let calibration = Calibration::read(firmware.bytes());
        let mut regs = vec![0; REGISTER_BYTES / 2];
        for (offset, value) in RESET {
            regs[offset >> 1] = value;
        }
        regs[W_ID >> 1] = if calibration.lite { ID_LITE } else { ID_DS };
        let bits = u16::from(calibration.radio_bits);
        regs[W_RF_CNT >> 1] = (bits & 0x3F) | ((bits & 0x80) << 1);
        let mut wifi = Self {
            regs,
            ram: vec![0; RAM_BYTES],
            baseband: Baseband::new(&calibration),
            radio: Radio::new(&calibration),
            calibration,
            channel: 0,
            clock: MicroClock::default(),
            powered: false,
            now: 0,
            us_count: 0,
            cmd_count: 0,
            block_beacon: false,
            ms_due: false,
            pre_fired: None,
            random: RANDOM[0],
            power: Power::default(),
            link: Link::Idle,
            exchange: None,
            retired_last_cmd: (),
            line: false,
            raised: false,
            outbox: Vec::new(),
            inbox: Vec::new(),
            traffic: Traffic::default(),
        };
        wifi.channel = wifi.calibration.channel_of(wifi.radio.registers());
        wifi
    }

    pub fn set_powered(&mut self, on: bool) {
        self.powered = on;
    }

    pub fn powered(&self) -> bool {
        self.powered
    }

    pub fn channel(&self) -> u8 {
        self.channel
    }

    pub fn now_us(&self) -> u64 {
        self.now
    }

    pub fn mac(&self) -> [u8; 6] {
        self.address(W_MACADDR)
    }

    pub fn read16(&mut self, offset: usize) -> u16 {
        if !self.powered {
            return 0;
        }
        let offset = offset & WINDOW;
        match offset {
            0..REGISTER_BYTES => self.read_active(offset),
            REGISTER_BYTES..PASSIVE_END => self.reg(offset & 0xFFE),
            PASSIVE_END..RAM_START => 0xFFFF,
            RAM_START..RAM_END => self.ram16(offset - RAM_START),
            _ => self.reg(offset & 0xFFE),
        }
    }

    pub fn write16(&mut self, offset: usize, value: u16) {
        if !self.powered {
            return;
        }
        let offset = offset & WINDOW;
        match offset {
            0..REGISTER_BYTES => self.write_active(offset, value),
            REGISTER_BYTES..PASSIVE_END => self.set(offset & 0xFFE, value),
            PASSIVE_END..RAM_START => {}
            RAM_START..RAM_END => self.set_ram16(offset - RAM_START, value),
            _ => self.set(offset & 0xFFE, value),
        }
    }

    pub fn read8(&mut self, offset: usize) -> u8 {
        (self.read16(offset) >> (8 * (offset & 1))) as u8
    }

    pub fn read32(&mut self, offset: usize) -> u32 {
        let low = self.read16(offset);
        let high = self.read16(offset + 2);
        u32::from(low) | (u32::from(high) << 16)
    }

    pub fn write32(&mut self, offset: usize, value: u32) {
        self.write16(offset, value as u16);
        self.write16(offset + 2, (value >> 16) as u16);
    }

    #[inline]
    pub fn step(&mut self, cycles: u32) -> Irq {
        if !self.running() {
            return self.take_requests();
        }
        let us = self.clock.advance(cycles);
        self.step_us(us)
    }

    #[inline]
    pub fn until_event(&self) -> Option<u32> {
        if !self.running() {
            return None;
        }
        self.until_event_us()
            .map(|us| self.clock.cycles_until(us.max(1)))
    }

    #[inline(never)]
    pub fn step_us(&mut self, us: u64) -> Irq {
        if self.running() {
            self.settle();
            let mut left = us;
            while left > 0 {
                let wait = self.until_event_us().unwrap_or(left).clamp(1, left);
                self.advance(wait);
                left -= wait;
                self.settle();
            }
        }
        self.take_requests()
    }

    pub fn until_event_us(&self) -> Option<u64> {
        if !self.running() {
            return None;
        }
        [
            self.timer_event(),
            self.pre_beacon_event(),
            self.power.waking_until.map(|at| at - self.now),
            self.link_deadline().map(|at| at.saturating_sub(self.now)),
            self.inbox
                .iter()
                .map(|frame| frame.start.saturating_sub(self.now))
                .min(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn take_requests(&mut self) -> Irq {
        if std::mem::take(&mut self.raised) {
            Irq::WIFI
        } else {
            Irq::empty()
        }
    }

    pub fn take_frames(&mut self) -> Vec<Frame> {
        let frames = std::mem::take(&mut self.outbox);
        for frame in &frames {
            self.log_frame(Kind::WifiSent, frame);
        }
        frames
    }

    pub fn receive(&mut self, frame: Frame) {
        if self.running() && frame.start + frame.preamble_us() > self.now {
            self.log_frame(Kind::WifiReceived, &frame);
            self.inbox.push(frame);
        }
    }

    pub fn traffic(&self) -> &Traffic {
        &self.traffic
    }

    pub fn traffic_mut(&mut self) -> &mut Traffic {
        &mut self.traffic
    }

    fn log_frame(&mut self, kind: Kind, frame: &Frame) {
        let at = self.traffic.clock();
        self.traffic.push(
            Entry::new(kind)
                .tag(frame.channel)
                .at(at)
                .sent(&frame.bytes),
        );
    }

    pub fn take_frames_at(&mut self, now: u64) -> Vec<Frame> {
        let mut frames = self.take_frames();
        for frame in &mut frames {
            frame.start = now.saturating_sub(self.now - frame.start);
        }
        frames
    }

    pub fn receive_at(&mut self, now: u64, mut frame: Frame) {
        if let Some(start) = (frame.start + self.now).checked_sub(now) {
            frame.start = start;
            self.receive(frame);
        }
    }

    pub fn forget_the_air(&mut self) {
        self.inbox.clear();
    }

    #[cfg(test)]
    pub(crate) fn frames_due(&self) -> usize {
        self.inbox.len()
    }

    pub fn sending(&self) -> bool {
        !self.outbox.is_empty()
    }

    pub fn on_air(&self) -> bool {
        !matches!(self.link, Link::Idle) || !self.inbox.is_empty() || !self.outbox.is_empty()
    }

    #[inline]
    fn running(&self) -> bool {
        self.powered && self.reg(W_POWER_US) & 1 == 0
    }

    fn advance(&mut self, us: u64) {
        self.now += us;
        if self.reg(W_US_COUNTCNT) & 1 != 0 {
            let old = self.us_count;
            self.us_count = old.wrapping_add(us);
            if (old ^ self.us_count) >> 10 != 0 {
                self.ms_due = true;
            }
        }
        if self.reg(W_CMD_COUNTCNT) & 1 != 0 {
            self.cmd_count = self.cmd_count.saturating_sub(us as u32);
        }
        let free = self.reg(W_CONTENTFREE);
        self.set(W_CONTENTFREE, free.saturating_sub(us.min(0xFFFF) as u16));
    }

    fn settle(&mut self) {
        for _ in 0..MAX_SETTLE {
            let mut acted = false;
            if std::mem::take(&mut self.ms_due) {
                self.millisecond();
                acted = true;
            }
            acted |= self.check_pre_beacon();
            acted |= self.check_wake();
            acted |= self.check_link();
            acted |= self.check_inbox();
            if !acted {
                return;
            }
        }
    }

    fn link_deadline(&self) -> Option<u64> {
        match &self.link {
            Link::Listening => self.exchange.as_ref().map(|exchange| exchange.window_end),
            Link::Failing => {
                (self.reg(W_CMD_COUNTCNT) & 1 != 0).then(|| self.now + u64::from(self.cmd_count))
            }
            other => other.deadline(),
        }
    }

    fn check_link(&mut self) -> bool {
        match self.link_deadline() {
            Some(at) if at <= self.now => {}
            _ => return false,
        }
        match std::mem::take(&mut self.link) {
            Link::Sending(mut sending) if !sending.started => {
                sending.started = true;
                self.preamble_sent(&sending);
                self.link = Link::Sending(sending);
            }
            Link::Sending(sending) => self.sent(sending),
            Link::Receiving(mut receiving) if !receiving.started => {
                receiving.started = true;
                self.preamble_received();
                self.link = Link::Receiving(receiving);
            }
            Link::Receiving(receiving) => self.received(receiving),
            Link::Listening => self.window_closed(),
            Link::Waiting(_, reply) => self.send_reply(reply),
            Link::Failing => self.exchange_failed(),
            Link::Idle => {}
        }
        true
    }

    fn raise(&mut self, bits: u16) {
        let flags = self.reg(W_IF) | (bits & !IF_UNUSED);
        self.set(W_IF, flags);
        self.update_line();
    }

    fn update_line(&mut self) {
        let on = self.reg(W_IF) & self.reg(W_IE) != 0;
        if on && !self.line {
            self.raised = true;
        }
        self.line = on;
    }

    fn reg(&self, offset: usize) -> u16 {
        self.regs[offset >> 1]
    }

    fn set(&mut self, offset: usize, value: u16) {
        self.regs[offset >> 1] = value;
    }

    fn address(&self, offset: usize) -> [u8; 6] {
        let mut bytes = [0; 6];
        for half in 0..3 {
            let value = self.reg(offset + 2 * half).to_le_bytes();
            bytes[2 * half..2 * half + 2].copy_from_slice(&value);
        }
        bytes
    }

    fn ram16(&self, at: usize) -> u16 {
        let at = at & RAM_MASK;
        u16::from_le_bytes([self.ram[at], self.ram[at + 1]])
    }

    fn ram8(&self, at: usize) -> u8 {
        self.ram[at & (RAM_BYTES - 1)]
    }

    fn set_ram8(&mut self, at: usize, value: u8) {
        self.ram[at & (RAM_BYTES - 1)] = value;
    }

    fn set_ram16(&mut self, at: usize, value: u16) {
        let at = at & RAM_MASK;
        self.ram[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn read_active(&mut self, offset: usize) -> u16 {
        match offset {
            W_RANDOM => {
                let value = self.random;
                let cycles = u128::from(self.now) * u128::from(CYCLES_PER_SECOND) / 1_000_000;
                self.random = RANDOM[(cycles % RANDOM_PERIOD as u128) as usize];
                value
            }
            W_US_COUNT0..=W_US_COUNT3 => (self.us_count >> (8 * (offset - W_US_COUNT0))) as u16,
            W_CMD_COUNT => self.cmd_count.div_ceil(10) as u16,
            W_RXBUF_RD_DATA => self.read_rx_data(),
            W_TXBUF_WR_ADDR_MIRROR => self.reg(W_TXBUF_WR_ADDR),
            W_RF_DATA2_MIRROR => self.reg(W_RF_DATA2),
            W_PREAMBLE => self.reg(W_PREAMBLE) & 3,
            W_TXBUSY => self.reg(W_TXBUSY) & 0x1F,
            W_RXSTAT..=W_RXSTAT_LAST | W_RX_COUNT | W_CMD_STAT..=W_CMD_STAT_LAST => {
                let value = self.reg(offset);
                self.set(offset, 0);
                value
            }
            _ => self.reg(offset),
        }
    }

    fn write_active(&mut self, offset: usize, value: u16) {
        if READ_ONLY.contains(&offset) {
            return;
        }
        let masked = value & write_mask(offset);
        match offset {
            W_MODE_RST => self.write_mode_reset(value),
            W_IF => {
                let flags = self.reg(W_IF) & !value;
                self.set(W_IF, flags);
                self.update_line();
            }
            W_IE => {
                self.set(W_IE, value);
                self.update_line();
            }
            W_IF_SET => self.raise(value),
            W_RXCNT => self.write_rx_control(value),
            W_POWERFORCE => self.write_power_force(masked),
            W_POWERSTATE => self.write_power_state(value),
            W_RXBUF_RD_DATA => self.count_rx_read(),
            W_TXBUF_WR_DATA => self.write_tx_data(value),
            W_TXBUF_CMD => {
                let value = if self.cmd_count == 0 {
                    (value & 0x7FFF) | (self.reg(W_TXBUF_CMD) & 0x8000)
                } else {
                    value
                };
                self.set(W_TXBUF_CMD, value);
                self.try_send();
            }
            W_TXBUF_LOC1 | W_TXBUF_LOC2 | W_TXBUF_LOC3 => {
                self.set(offset, value);
                self.try_send();
            }
            W_TXREQ_RESET => {
                let requests = self.reg(W_TXREQ_READ) & !(value & 0xF);
                self.set(W_TXREQ_READ, requests);
            }
            W_TXREQ_SET => {
                let requests = self.reg(W_TXREQ_READ) | (value & 0xF);
                self.set(W_TXREQ_READ, requests);
                self.try_send();
            }
            W_TXBUF_RESET => self.reset_slots(value),
            W_US_COMPARECNT => {
                self.set(W_US_COMPARECNT, masked);
                if value & 2 != 0 {
                    self.beacon(timers::Source::Forced);
                }
            }
            W_US_COMPARE0 => {
                self.set(W_US_COMPARE0, masked);
                self.block_beacon = value & 1 != 0;
            }
            W_US_COUNT0..=W_US_COUNT3 => {
                let shift = 8 * (offset - W_US_COUNT0);
                self.us_count = (self.us_count & !(0xFFFF << shift)) | (u64::from(value) << shift);
                self.pre_fired = None;
            }
            W_CMD_COUNT => {
                self.set(W_CMD_COUNT, value);
                self.cmd_count = u32::from(value) * 10;
            }
            W_BB_CNT => self.write_baseband(value),
            W_RF_DATA2 => self.write_radio(value),
            _ => self.set(offset, masked),
        }
    }

    fn write_mode_reset(&mut self, value: u16) {
        let old = self.reg(W_MODE_RST);
        self.set(W_MODE_RST, value & write_mask(W_MODE_RST));
        if old & 1 == 0 && value & 1 != 0 {
            self.set(W_INTERNAL_27C, 0x0005);
            self.set(W_RF_PINS, 0x0046);
            self.set(W_RF_STATUS, 0x0009);
        }
        if old & 1 != 0 && value & 1 == 0 {
            self.set(W_INTERNAL_27C, 0x000A);
        }
        if value & 0x2000 != 0 {
            for offset in [W_RXBUF_WR_ADDR, W_CMD_TOTALTIME, W_CMD_REPLYTIME, W_X_1A4] {
                self.set(offset, 0);
            }
            self.set(W_INTERNAL_278, 0x000F);
        }
        if value & 0x4000 != 0 {
            for offset in [
                W_MODE_WEP,
                W_TXSTATCNT,
                W_X_00A,
                W_MACADDR,
                W_MACADDR + 2,
                W_MACADDR + 4,
                W_BSSID,
                W_BSSID + 2,
                W_BSSID + 4,
                W_AID_LOW,
                W_AID_FULL,
                W_INTERNAL_02E,
                W_TXBUF_TIM,
                W_TX_HDR_CNT,
                W_INTERNAL_198,
            ] {
                self.set(offset, 0);
            }
            for (offset, value) in [
                (W_TX_RETRYLIMIT, 0x0707),
                (W_RXBUF_BEGIN, 0x4000),
                (W_RXBUF_END, 0x4800),
                (W_PREAMBLE, 0x0001),
                (W_RXFILTER, 0x0401),
                (W_CONFIG_0D4, 0x0001),
                (W_RXFILTER2, 0x0008),
                (W_CONFIG_0EC, 0x3F03),
                (W_X_1A2, 0x0001),
                (W_INTERNAL_224, 0x0003),
                (W_INTERNAL_230, 0x0047),
            ] {
                self.set(offset, value);
            }
        }
        self.try_send();
    }

    fn write_tx_data(&mut self, value: u16) {
        let at = usize::from(self.reg(W_TXBUF_WR_ADDR) & 0x1FFE);
        self.set_ram16(at, value);
        let mut next = (at + 2) & RAM_MASK;
        if next == usize::from(self.reg(W_TXBUF_GAP)) {
            next = (next + 2 * usize::from(self.reg(W_TXBUF_GAPDISP))) & RAM_MASK;
        }
        self.set(W_TXBUF_WR_ADDR, next as u16);
        let count = self.reg(W_TXBUF_COUNT);
        if count != 0 {
            self.set(W_TXBUF_COUNT, count - 1);
            if count == 1 {
                self.raise(IRQ_TX_COUNT);
            }
        }
    }

    fn write_baseband(&mut self, value: u16) {
        self.set(W_BB_CNT, value);
        if self.reg(W_BB_POWER) & 0x8000 != 0 {
            return;
        }
        let index = value as u8;
        match value >> 12 {
            5 => self.baseband.write(index, self.reg(W_BB_WRITE) as u8),
            6 => self.set(W_BB_READ, u16::from(self.baseband.read(index))),
            _ => {}
        }
    }

    fn write_radio(&mut self, value: u16) {
        let (data1, data2) = self.radio.transfer(self.reg(W_RF_DATA1), value);
        self.set(W_RF_DATA1, data1);
        self.set(W_RF_DATA2, data2);
        self.channel = self.calibration.channel_of(self.radio.registers());
    }

    fn set_rf_status(&mut self, status: u16) {
        let pins = match status {
            1 | 5 => 0x0084,
            6 => 0x0087,
            3 | 8 | 9 => 0x0046,
            _ => 0x0004,
        };
        self.set(W_RF_STATUS, status);
        self.set(W_RF_PINS, pins);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn wifi() -> Wifi {
        let mut wifi = Wifi::new(&Firmware::default());
        wifi.set_powered(true);
        wifi
    }

    pub fn reg(wifi: &mut Wifi, offset: usize) -> u16 {
        wifi.read16(offset)
    }

    #[test]
    fn reset_values_follow_gbatek() {
        let mut wifi = wifi();
        for (offset, value) in [
            (W_ID, 0x1440),
            (W_TX_RETRYLIMIT, 0x0707),
            (W_POWER_US, 0x0001),
            (W_POWER_TX, 0x0003),
            (W_POWERSTATE, 0x0200),
            (W_RXBUF_BEGIN, 0x4000),
            (W_RXBUF_END, 0x4800),
            (W_BEACONINT, 0x0064),
            (W_TXREQ_READ, 0x0010),
            (W_PREAMBLE, 0x0001),
            (W_RXFILTER, 0x0401),
            (W_RX_LEN_CROP, 0x0602),
            (W_RXFILTER2, 0x0008),
            (W_US_COMPARE0, 0xFC00),
            (W_US_COMPARE3, 0xFFFF),
            (W_POST_BEACON, 0xFFFF),
            (W_BB_POWER, 0x800D),
            (W_RF_DATA1, 0xC008),
            (W_RF_CNT, 0x0114),
            (W_RF_STATUS, 0x0009),
            (W_RXTX_ADDR, 0x0005),
            (W_MACADDR, 0x0000),
        ] {
            assert_eq!(reg(&mut wifi, offset), value, "{offset:#x}");
        }
    }

    #[test]
    fn a_lite_firmware_gives_the_lite_chip_id() {
        let mut image = Firmware::default().bytes().to_vec();
        image[0x1D] = 0x20;
        let mut wifi = Wifi::new(&Firmware::new(&image));
        wifi.set_powered(true);
        assert_eq!(reg(&mut wifi, W_ID), 0xC340);
    }

    #[test]
    fn without_power_reads_are_zero_and_writes_vanish() {
        let mut wifi = Wifi::new(&Firmware::default());
        assert_eq!(wifi.read16(W_ID), 0);
        wifi.write16(W_IE, 0xFFFF);
        wifi.set_powered(true);
        assert_eq!(reg(&mut wifi, W_IE), 0);
    }

    #[test]
    fn read_only_registers_ignore_writes() {
        let mut wifi = wifi();
        for offset in [W_ID, W_RANDOM, W_TXBUSY, W_TXSTAT, W_TX_SEQNO, W_RF_STATUS] {
            let before = reg(&mut wifi, offset);
            wifi.write16(offset, 0x1234);
            if offset != W_RANDOM {
                assert_eq!(reg(&mut wifi, offset), before, "{offset:#x}");
            }
        }
    }

    #[test]
    fn write_masks_keep_only_the_defined_bits() {
        let mut wifi = wifi();
        for (offset, mask) in [
            (W_AID_LOW, 0x000F),
            (W_AID_FULL, 0x07FF),
            (W_BEACONINT, 0x03FF),
            (W_MODE_WEP, 0x007F),
            (W_RXBUF_WR_ADDR, 0x0FFF),
            (W_TXBUF_GAP, 0x1FFE),
            (W_US_COMPARE0, 0xFC00),
        ] {
            wifi.write16(offset, 0xFFFF);
            assert_eq!(reg(&mut wifi, offset), mask, "{offset:#x}");
        }
    }

    #[test]
    fn the_whole_block_mirrors_every_32_kilobytes() {
        let mut wifi = wifi();
        wifi.write16(0x8000 + W_IE, 0x00C0);
        assert_eq!(reg(&mut wifi, W_IE), 0x00C0);
        wifi.write16(0x4100, 0xBEEF);
        assert_eq!(reg(&mut wifi, 0xC100), 0xBEEF);
    }

    #[test]
    fn passive_mirrors_read_storage_without_side_effects() {
        let mut wifi = wifi();
        let first = reg(&mut wifi, 0x1000 + W_RANDOM);
        assert_eq!(reg(&mut wifi, 0x1000 + W_RANDOM), first);
        assert_eq!(reg(&mut wifi, 0x6000 + W_ID), 0x1440);
        assert_eq!(reg(&mut wifi, 0x2000), 0xFFFF);
        wifi.write16(0x3000, 0x1234);
        assert_eq!(reg(&mut wifi, 0x3000), 0xFFFF);
    }

    #[test]
    fn byte_reads_pick_from_the_halfword_and_byte_writes_drop() {
        let mut wifi = wifi();
        assert_eq!(wifi.read8(W_ID), 0x40);
        assert_eq!(wifi.read8(W_ID + 1), 0x14);
        wifi.write32(W_MACADDR, 0x2211_0900);
        assert_eq!(wifi.read32(W_MACADDR), 0x2211_0900);
    }

    #[test]
    fn wifi_ram_holds_eight_kilobytes() {
        let mut wifi = wifi();
        wifi.write16(0x4000, 0x1111);
        wifi.write16(0x5FFE, 0x2222);
        assert_eq!(reg(&mut wifi, 0x4000), 0x1111);
        assert_eq!(reg(&mut wifi, 0x5FFE), 0x2222);
    }

    #[test]
    fn random_walks_the_gbatek_sequence_and_returns_the_latched_value() {
        assert_eq!(RANDOM[..5], [1, 3, 7, 15, 31]);
        let last = RANDOM[RANDOM_PERIOD - 1];
        assert_eq!((last & 1) ^ (((last << 1) | (last >> 10)) & RANDOM_BITS), 1);
        let distinct: std::collections::HashSet<u16> = RANDOM.iter().copied().collect();
        assert_eq!(distinct.len(), RANDOM_PERIOD);
        assert!(RANDOM.iter().all(|&value| value <= RANDOM_BITS));
        let mut wifi = wifi();
        assert_eq!(reg(&mut wifi, W_RANDOM), 1);
    }

    #[test]
    fn interrupts_raise_the_arm7_line_on_the_rising_edge_only() {
        let mut wifi = wifi();
        wifi.write16(W_IF_SET, IRQ_RX_DONE);
        assert_eq!(wifi.take_requests(), Irq::empty());
        wifi.write16(W_IE, IRQ_RX_DONE | IRQ_TX_DONE);
        assert_eq!(wifi.take_requests(), Irq::WIFI);
        wifi.write16(W_IF_SET, IRQ_TX_DONE);
        assert_eq!(wifi.take_requests(), Irq::empty());
        wifi.write16(W_IF, IRQ_RX_DONE | IRQ_TX_DONE);
        assert_eq!(reg(&mut wifi, W_IF), 0);
        wifi.write16(W_IF_SET, IRQ_TX_DONE);
        assert_eq!(wifi.take_requests(), Irq::WIFI);
    }

    #[test]
    fn if_set_cannot_raise_bit_ten() {
        let mut wifi = wifi();
        wifi.write16(W_IF_SET, 0xFFFF);
        assert_eq!(reg(&mut wifi, W_IF), 0xFBFF);
    }

    #[test]
    fn mode_reset_bit_fourteen_restores_the_mac_side() {
        let mut wifi = wifi();
        wifi.write16(W_MACADDR, 0x1234);
        wifi.write16(W_RXFILTER, 0);
        wifi.write16(W_RXBUF_END, 0x5000);
        wifi.write16(W_AID_LOW, 3);
        wifi.write16(W_MODE_RST, 0x4000);
        assert_eq!(reg(&mut wifi, W_MACADDR), 0);
        assert_eq!(reg(&mut wifi, W_RXFILTER), 0x0401);
        assert_eq!(reg(&mut wifi, W_RXBUF_END), 0x4800);
        assert_eq!(reg(&mut wifi, W_AID_LOW), 0);
        assert_eq!(reg(&mut wifi, W_MODE_RST) & 0x6000, 0);
    }

    #[test]
    fn mode_reset_bit_thirteen_clears_the_rx_write_address_and_cmd_times() {
        let mut wifi = wifi();
        wifi.write16(W_RXBUF_WR_ADDR, 0x123);
        wifi.write16(W_CMD_REPLYTIME, 300);
        wifi.write16(W_MODE_RST, 0x2000);
        assert_eq!(reg(&mut wifi, W_RXBUF_WR_ADDR), 0);
        assert_eq!(reg(&mut wifi, W_CMD_REPLYTIME), 0);
        assert_eq!(reg(&mut wifi, W_INTERNAL_278), 0x000F);
    }

    #[test]
    fn mode_reset_bit_zero_edges_set_the_internal_ports() {
        let mut wifi = wifi();
        wifi.write16(W_MODE_RST, 1);
        assert_eq!(reg(&mut wifi, W_INTERNAL_27C), 0x0005);
        assert_eq!(reg(&mut wifi, W_RF_PINS), 0x0046);
        wifi.write16(W_MODE_RST, 0);
        assert_eq!(reg(&mut wifi, W_INTERNAL_27C), 0x000A);
    }

    #[test]
    fn tx_data_writes_advance_skip_the_gap_and_count_down() {
        let mut wifi = wifi();
        wifi.write16(W_IE, IRQ_TX_COUNT);
        wifi.write16(W_TXBUF_WR_ADDR, 0x0100);
        wifi.write16(W_TXBUF_GAP, 0x0104);
        wifi.write16(W_TXBUF_GAPDISP, 0x0010);
        wifi.write16(W_TXBUF_COUNT, 3);
        for value in [0xAAAA, 0xBBBB, 0xCCCC] {
            wifi.write16(W_TXBUF_WR_DATA, value);
        }
        assert_eq!(reg(&mut wifi, 0x4100), 0xAAAA);
        assert_eq!(reg(&mut wifi, 0x4102), 0xBBBB);
        assert_eq!(reg(&mut wifi, 0x4124), 0xCCCC);
        assert_eq!(reg(&mut wifi, W_TXBUF_WR_ADDR), 0x0126);
        assert_eq!(reg(&mut wifi, W_TXBUF_WR_ADDR_MIRROR), 0x0126);
        assert_eq!(reg(&mut wifi, W_TXBUF_COUNT), 0);
        assert_eq!(reg(&mut wifi, W_IF), IRQ_TX_COUNT);
        assert_eq!(wifi.take_requests(), Irq::WIFI);
    }

    #[test]
    fn the_baseband_answers_through_bb_cnt_once_powered() {
        let mut wifi = wifi();
        wifi.write16(W_BB_CNT, 0x6000);
        assert_eq!(reg(&mut wifi, W_BB_READ), 0x00B5);
        wifi.write16(W_BB_POWER, 0);
        wifi.write16(W_BB_CNT, 0x6000);
        assert_eq!(reg(&mut wifi, W_BB_READ), 0x6D);
        wifi.write16(W_BB_CNT, 0x601E);
        assert_eq!(reg(&mut wifi, W_BB_READ), 0x24);
        wifi.write16(W_BB_WRITE, 0x42);
        wifi.write16(W_BB_CNT, 0x5013);
        wifi.write16(W_BB_CNT, 0x6013);
        assert_eq!(reg(&mut wifi, W_BB_READ), 0x42);
        assert_eq!(reg(&mut wifi, W_BB_BUSY), 0);
    }

    pub fn tune(wifi: &mut Wifi, channel: u8) {
        let writes = wifi.calibration.channels[usize::from(channel) - 1];
        for write in writes {
            let (data1, data2) = radio::words(wifi.calibration.radio, write);
            wifi.write16(W_RF_DATA1, data1);
            wifi.write16(W_RF_DATA2, data2);
        }
    }

    #[test]
    fn rf_writes_from_the_firmware_channel_table_tune_the_channel() {
        let mut wifi = wifi();
        assert_eq!(wifi.channel(), 0);
        tune(&mut wifi, 7);
        assert_eq!(wifi.channel(), 7);
        tune(&mut wifi, 13);
        assert_eq!(wifi.channel(), 13);
        let write = wifi.calibration.channels[12][0];
        wifi.write16(W_RF_DATA1, u16::from(write.index) << 8);
        wifi.write16(W_RF_DATA2, 6);
        assert_eq!(reg(&mut wifi, W_RF_DATA1) & 0xFF, write.value as u16);
    }

    #[test]
    fn sent_frames_land_in_the_traffic_ring_at_the_stamped_clock() {
        let mut wifi = wifi();
        wifi.traffic_mut().set_clock(1234);
        let frame = Frame {
            start: 0,
            channel: 7,
            rate: super::frame::Rate::Mbit2,
            short_preamble: false,
            bytes: vec![0x08, 0x01, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        };
        wifi.outbox.push(frame.clone());
        wifi.outbox.push(frame);
        assert_eq!(wifi.take_frames().len(), 2);
        let entries: Vec<_> = wifi.traffic().entries().copied().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].at, 1234);
        assert_eq!(entries[0].repeats, 2);
        assert_eq!(
            entries[0].describe(),
            "wifi sent 14 bytes on channel 7: 08 01 00 00 01 02 03 04 05 06 07 08 .. x2 until frame 0"
        );
    }
}
