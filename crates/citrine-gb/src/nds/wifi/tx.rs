use super::frame::{self, Frame, Rate};
use super::link::{Link, Sending, Slot};
use super::registers::*;
use super::{IRQ_TX_DONE, IRQ_TX_START, RAM_BYTES, Wifi};

pub const TX_HEADER_BYTES: usize = 12;
const LENGTH_MASK: u16 = 0x3FFF;
const ENABLE: u16 = 1 << 15;
const KEEP_SEQUENCE: u16 = 1 << 13;
const ADDRESS_MASK: u16 = 0x0FFF;
const SEQUENCE_MASK: u16 = 0x0FFF;
const TIMESTAMP: usize = 24;
const SHORT_PREAMBLE: u16 = 1 << 2;
const STATUS_OK: u16 = 0x0001;

impl Slot {
    pub fn register(self) -> usize {
        match self {
            Slot::Loc1 => W_TXBUF_LOC1,
            Slot::Cmd | Slot::Ack => W_TXBUF_CMD,
            Slot::Loc2 => W_TXBUF_LOC2,
            Slot::Loc3 => W_TXBUF_LOC3,
            Slot::Beacon => W_TXBUF_BEACON,
            Slot::Reply | Slot::EmptyReply => W_TXBUF_REPLY2,
        }
    }

    fn status(self) -> u16 {
        match self {
            Slot::Loc2 => 0x1001,
            Slot::Loc3 => 0x2001,
            _ => 0x0001,
        }
    }
}

impl Wifi {
    pub(super) fn try_send(&mut self) {
        let mut busy = self.reg(W_TXBUSY);
        for slot in [Slot::Loc1, Slot::Cmd, Slot::Loc2, Slot::Loc3] {
            if self.reg(slot.register()) & ENABLE != 0
                && self.reg(W_TXREQ_READ) & slot.request_bit() != 0
            {
                busy |= slot.busy_bit();
            }
        }
        self.set(W_TXBUSY, busy);
        if self.link != Link::Idle
            || self.reg(W_MODE_RST) & 1 == 0
            || self.power.sleep_pending.is_some()
        {
            return;
        }
        if !self.awake() {
            if busy & Slot::Cmd.busy_bit() != 0 {
                self.request_wake();
            }
            return;
        }
        for slot in Slot::QUEUED {
            if self.reg(W_TXBUSY) & slot.busy_bit() == 0 {
                continue;
            }
            if self.reg(slot.register()) & ENABLE == 0 {
                let busy = self.reg(W_TXBUSY) & !slot.busy_bit();
                self.set(W_TXBUSY, busy);
                continue;
            }
            if slot == Slot::Cmd {
                self.start_exchange();
            } else {
                self.start_slot(slot);
            }
            return;
        }
    }

    pub(super) fn frame_from_ram(&mut self, header: usize, sequence: bool) -> Frame {
        let length = usize::from(self.ram16(header + 0xA) & LENGTH_MASK);
        let body = (header + TX_HEADER_BYTES).min(RAM_BYTES);
        let length = length.min(RAM_BYTES - body);
        let mut bytes = self.ram[body..body + length].to_vec();
        if let Some(first) = bytes.first_mut() {
            *first &= !3;
        }
        if self.ram8(header + 4) != 0 {
            frame::mark_retry(&mut bytes);
        } else if sequence && self.reg(W_TX_HDR_CNT) & 4 == 0 {
            let number = self.next_sequence();
            frame::set_sequence(&mut bytes, number);
            let at = body + 22;
            if at + 2 <= RAM_BYTES {
                self.set_ram16(at, number << 4);
            }
        }
        Frame {
            start: self.now,
            channel: self.channel,
            rate: Rate::from_code(self.ram8(header + 8)),
            short_preamble: self.reg(W_PREAMBLE) & SHORT_PREAMBLE != 0,
            bytes,
        }
    }

    pub(super) fn next_sequence(&mut self) -> u16 {
        let number = self.reg(W_TX_SEQNO);
        self.set(W_TX_SEQNO, (number + 1) & SEQUENCE_MASK);
        number
    }

    pub(super) fn seal(&self, bytes: &mut [u8]) {
        if self.reg(W_TX_HDR_CNT) & 2 == 0 {
            frame::seal(bytes);
        }
    }

    pub(super) fn transmit(&mut self, slot: Slot, header: Option<usize>, mut frame: Frame) {
        frame.start = self.now;
        frame.channel = self.channel;
        if self.channel != 0 {
            self.outbox.push(frame.clone());
        }
        self.link = Link::Sending(Sending {
            slot,
            header,
            frame,
            started: false,
        });
    }

    pub(super) fn preamble_sent(&mut self, sending: &Sending) {
        self.raise(IRQ_TX_START);
        let status = match sending.slot {
            Slot::Reply | Slot::EmptyReply | Slot::Ack => 8,
            _ => 3,
        };
        self.set_rf_status(status);
        if let Some(header) = sending.header {
            self.set(W_RXTX_ADDR, (header / 2) as u16);
        }
    }

    pub(super) fn sent(&mut self, sending: Sending) {
        self.link = Link::Idle;
        let header = sending.header.unwrap_or(0);
        match sending.slot {
            Slot::Loc1 | Slot::Loc2 | Slot::Loc3 => {
                self.set_ram16(header, STATUS_OK);
                self.clear_busy(sending.slot);
                self.set(W_TXSTAT, sending.slot.status());
                self.raise(IRQ_TX_DONE);
                let register = self.reg(sending.slot.register()) & !ENABLE;
                self.set(sending.slot.register(), register);
            }
            Slot::Beacon => {
                self.set_ram16(header, STATUS_OK);
                self.clear_busy(Slot::Beacon);
                if self.reg(W_TXSTATCNT) & (1 << 15) != 0 {
                    self.set(W_TXSTAT, 0x0301);
                    self.raise(IRQ_TX_DONE);
                }
            }
            Slot::Cmd => return self.cmd_sent(),
            Slot::Ack => return self.ack_sent(),
            Slot::Reply => {
                if self.reg(W_TXSTATCNT) & (1 << 12) != 0 {
                    self.set(W_TXSTAT, 0x0401);
                    self.raise(IRQ_TX_DONE);
                }
                self.set_ram8(header + 5, 0);
            }
            Slot::EmptyReply => {}
        }
        self.set_rf_status(1);
        self.frame_done();
        self.try_send();
    }

    pub(super) fn reset_slots(&mut self, value: u16) {
        for (bit, slot) in [
            (0, Some(Slot::Loc1)),
            (1, Some(Slot::Cmd)),
            (2, Some(Slot::Loc2)),
            (3, Some(Slot::Loc3)),
            (6, None),
            (7, None),
        ] {
            if value & (1 << bit) == 0 {
                continue;
            }
            let register = match (bit, slot) {
                (_, Some(slot)) => slot.register(),
                (6, None) => W_TXBUF_REPLY2,
                _ => W_TXBUF_REPLY1,
            };
            let cleared = self.reg(register) & !ENABLE;
            self.set(register, cleared);
            if let Some(slot) = slot {
                let on_air = matches!(&self.link, Link::Sending(sending) if sending.slot == slot);
                if !on_air {
                    self.clear_busy(slot);
                }
            }
        }
    }

    fn start_slot(&mut self, slot: Slot) {
        let value = self.reg(slot.register());
        let header = usize::from(value & ADDRESS_MASK) * 2;
        let sequence = slot == Slot::Beacon || value & KEEP_SEQUENCE == 0;
        let mut frame = self.frame_from_ram(header, sequence);
        if slot == Slot::Beacon && self.reg(W_US_COUNTCNT) & 1 != 0 {
            let stamp = self.us_count + frame.preamble_us();
            if let Some(field) = frame.bytes.get_mut(TIMESTAMP..TIMESTAMP + 8) {
                field.copy_from_slice(&stamp.to_le_bytes());
            }
        }
        self.seal(&mut frame.bytes);
        self.transmit(slot, Some(header), frame);
    }

    fn clear_busy(&mut self, slot: Slot) {
        let busy = self.reg(W_TXBUSY) & !slot.busy_bit();
        self.set(W_TXBUSY, busy);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::power::WAKE_US;
    use super::super::tests::{reg, tune, wifi};
    use super::*;

    pub fn awake_on(channel: u8) -> Wifi {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_MODE_RST, 1);
        wifi.write16(W_IE, 0xFFFF);
        wifi.write16(W_POWERFORCE, 0x8000);
        wifi.write16(W_POWERFORCE, 0);
        wifi.step_us(WAKE_US);
        wifi.write16(W_IF, 0xFFFF);
        tune(&mut wifi, channel);
        wifi
    }

    pub fn put_frame(wifi: &mut Wifi, header: usize, rate: u8, bytes: &[u8]) {
        let total = bytes.len() + 4;
        let mut ram = vec![0u8; TX_HEADER_BYTES];
        ram[8] = rate;
        ram[0xA..0xC].copy_from_slice(&(total as u16).to_le_bytes());
        ram.extend_from_slice(bytes);
        ram.extend_from_slice(&[0; 4]);
        if ram.len() % 2 == 1 {
            ram.push(0);
        }
        for (index, pair) in ram.chunks(2).enumerate() {
            wifi.write16(
                0x4000 + header + 2 * index,
                u16::from_le_bytes([pair[0], pair[1]]),
            );
        }
    }

    pub fn data_frame(to: [u8; 6], from: [u8; 6], body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x08, 0x00, 0, 0];
        bytes.extend_from_slice(&to);
        bytes.extend_from_slice(&from);
        bytes.extend_from_slice(&from);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(body);
        bytes
    }

    fn run_until(wifi: &mut Wifi, bit: u16, limit: u64) -> Option<u64> {
        let start = wifi.now_us();
        while wifi.now_us() - start < limit {
            let wait = wifi.until_event_us().unwrap_or(limit).max(1);
            wifi.step_us(wait);
            if wifi.reg(W_IF) & bit != 0 {
                wifi.write16(W_IF, bit);
                return Some(wifi.now_us() - start);
            }
        }
        None
    }

    const OTHER: [u8; 6] = [0x00, 0x09, 0xBF, 0x00, 0x00, 0x02];
    const ME: [u8; 6] = [0x00, 0x09, 0xBF, 0x00, 0x00, 0x01];

    #[test]
    fn a_loc1_frame_goes_out_with_irq7_after_the_preamble_and_irq1_at_the_end() {
        let mut wifi = awake_on(7);
        let body = data_frame(OTHER, ME, &[1, 2, 3, 4]);
        put_frame(&mut wifi, 0x100, 0x14, &body);
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8000 | 0x80);
        let frames = wifi.take_frames();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];
        assert_eq!(frame.channel, 7);
        assert_eq!(frame.rate, Rate::Mbit2);
        assert_eq!(frame.bytes.len(), body.len() + 4);
        assert!(frame.fcs_ok());
        assert_eq!(reg(&mut wifi, W_TXBUSY), 1);
        assert_eq!(run_until(&mut wifi, IRQ_TX_START, 10_000), Some(192));
        assert_eq!(reg(&mut wifi, W_RF_STATUS), 3);
        let data = (body.len() as u64 + 4) * 4;
        assert_eq!(run_until(&mut wifi, IRQ_TX_DONE, 10_000), Some(data));
        assert_eq!(reg(&mut wifi, W_TXSTAT), 0x0001);
        assert_eq!(reg(&mut wifi, W_TXBUSY), 0);
        assert_eq!(reg(&mut wifi, W_TXBUF_LOC1), 0x0080);
        assert_eq!(reg(&mut wifi, 0x4100), 0x0001);
        assert_eq!(reg(&mut wifi, W_RF_STATUS), 1);
    }

    #[test]
    fn each_frame_takes_the_next_sequence_number_unless_kept() {
        let mut wifi = awake_on(1);
        put_frame(&mut wifi, 0x100, 0x14, &data_frame(OTHER, ME, &[0; 4]));
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        let first = wifi.take_frames().remove(0);
        run_until(&mut wifi, IRQ_TX_DONE, 10_000);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        let second = wifi.take_frames().remove(0);
        run_until(&mut wifi, IRQ_TX_DONE, 10_000);
        assert_eq!(first.sequence() >> 4, 0);
        assert_eq!(second.sequence() >> 4, 1);
        assert_eq!(reg(&mut wifi, W_TX_SEQNO), 2);
        wifi.write16(W_TXBUF_LOC1, 0x8000 | KEEP_SEQUENCE | 0x80);
        let kept = wifi.take_frames().remove(0);
        assert_eq!(kept.sequence() >> 4, 1);
        assert_eq!(reg(&mut wifi, W_TX_SEQNO), 2);
    }

    #[test]
    fn loc3_goes_before_loc2_before_loc1() {
        let mut wifi = awake_on(1);
        for (header, tag) in [(0x100, 1), (0x200, 2), (0x300, 3)] {
            put_frame(&mut wifi, header, 0x14, &data_frame(OTHER, ME, &[tag; 4]));
        }
        wifi.write16(W_MODE_RST, 0);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        wifi.write16(W_TXBUF_LOC2, 0x8100);
        wifi.write16(W_TXBUF_LOC3, 0x8180);
        wifi.write16(W_TXREQ_SET, 0xD);
        assert_eq!(reg(&mut wifi, W_TXBUSY), 0x0D);
        wifi.write16(W_MODE_RST, 1);
        let mut order = Vec::new();
        let mut busy = Vec::new();
        for _ in 0..3 {
            order.extend(wifi.take_frames().iter().map(|frame| frame.bytes[24]));
            run_until(&mut wifi, IRQ_TX_DONE, 10_000);
            busy.push(reg(&mut wifi, W_TXBUSY));
        }
        assert_eq!(order, [3, 2, 1]);
        assert_eq!(busy, [0x05, 0x01, 0x00]);
    }

    #[test]
    fn a_loc_slot_waits_for_its_txreq_bit() {
        let mut wifi = awake_on(1);
        put_frame(&mut wifi, 0x100, 0x14, &data_frame(OTHER, ME, &[0; 4]));
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        assert!(wifi.take_frames().is_empty());
        wifi.write16(W_TXREQ_SET, 1);
        assert_eq!(wifi.take_frames().len(), 1);
    }

    #[test]
    fn txbuf_reset_cancels_a_queued_slot() {
        let mut wifi = awake_on(1);
        wifi.write16(W_MODE_RST, 0);
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        assert_eq!(reg(&mut wifi, W_TXBUSY), 1);
        wifi.write16(W_TXBUF_RESET, 1);
        assert_eq!(reg(&mut wifi, W_TXBUSY), 0);
        assert_eq!(reg(&mut wifi, W_TXBUF_LOC1), 0x0080);
        wifi.write16(W_MODE_RST, 1);
        assert!(wifi.take_frames().is_empty());
    }

    #[test]
    fn one_megabit_frames_take_eight_microseconds_a_byte_and_a_long_preamble() {
        let mut wifi = awake_on(1);
        wifi.write16(W_PREAMBLE, 4);
        put_frame(&mut wifi, 0x100, 0x0A, &data_frame(OTHER, ME, &[0; 6]));
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        assert_eq!(run_until(&mut wifi, IRQ_TX_START, 10_000), Some(192));
        assert_eq!(run_until(&mut wifi, IRQ_TX_DONE, 10_000), Some(34 * 8));
    }

    #[test]
    fn short_preamble_applies_at_two_megabits() {
        let mut wifi = awake_on(1);
        wifi.write16(W_PREAMBLE, 4);
        assert_eq!(reg(&mut wifi, W_PREAMBLE), 0);
        put_frame(&mut wifi, 0x100, 0x14, &data_frame(OTHER, ME, &[0; 6]));
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        assert_eq!(run_until(&mut wifi, IRQ_TX_START, 10_000), Some(96));
    }

    #[test]
    fn the_beacon_slot_sends_at_each_beacon_with_the_counter_stamped_in() {
        let mut wifi = awake_on(7);
        let mut beacon = vec![0x80, 0x00, 0, 0];
        beacon.extend_from_slice(&[0xFF; 6]);
        beacon.extend_from_slice(&ME);
        beacon.extend_from_slice(&ME);
        beacon.extend_from_slice(&[0, 0]);
        beacon.extend_from_slice(&[0; 12]);
        put_frame(&mut wifi, 0x200, 0x14, &beacon);
        wifi.write16(W_TXSTATCNT, 0x8000);
        wifi.write16(W_TXBUF_BEACON, 0x8100);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi.write16(W_US_COMPARECNT, 1);
        wifi.write16(W_BEACONINT, 2);
        wifi.write16(W_BEACON_COUNT, 2);
        let mut stamps = Vec::new();
        for _ in 0..3 {
            run_until(&mut wifi, IRQ_TX_DONE, 10_000);
            assert_eq!(reg(&mut wifi, W_TXSTAT), 0x0301);
            let frame = wifi.take_frames().remove(0);
            assert!(frame.fcs_ok());
            stamps.push(u64::from_le_bytes(frame.bytes[24..32].try_into().unwrap()));
        }
        assert_eq!(stamps, [2048 + 192, 4096 + 192, 6144 + 192]);
        assert_eq!(reg(&mut wifi, W_TXBUF_BEACON), 0x8100);
    }

    #[test]
    fn nothing_goes_out_asleep_or_with_mode_rst_clear() {
        let mut wifi = wifi();
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        wifi.write16(W_MODE_RST, 1);
        wifi.step_us(10_000);
        assert!(wifi.take_frames().is_empty());
        assert_eq!(reg(&mut wifi, W_TXBUSY), 1);
    }

    #[test]
    fn powerforce_during_a_frame_waits_for_the_frame_to_end() {
        let mut wifi = awake_on(1);
        put_frame(&mut wifi, 0x100, 0x14, &data_frame(OTHER, ME, &[0; 40]));
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        wifi.step_us(100);
        wifi.write16(W_POWERFORCE, 0x8001);
        assert!(wifi.awake());
        assert_eq!(reg(&mut wifi, W_TRX_POWER), 2);
        let airtime = 192 + (24 + 40 + 4) * 4;
        assert_eq!(
            run_until(&mut wifi, IRQ_TX_DONE, 10_000),
            Some(airtime - 100)
        );
        assert!(!wifi.awake());
        assert_eq!(reg(&mut wifi, W_RF_STATUS), 9);
    }

    #[test]
    fn a_frame_on_no_channel_runs_but_reaches_no_one() {
        let mut wifi = awake_on(1);
        let index = wifi.calibration.channels[0][0].index;
        let (data1, data2) = super::super::radio::words(
            wifi.calibration.radio,
            super::super::calibration::RadioWrite { index, value: 0 },
        );
        wifi.write16(W_RF_DATA1, data1);
        wifi.write16(W_RF_DATA2, data2);
        assert_eq!(wifi.channel(), 0);
        put_frame(&mut wifi, 0x100, 0x14, &data_frame(OTHER, ME, &[0; 4]));
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8080);
        assert!(wifi.take_frames().is_empty());
        assert!(run_until(&mut wifi, IRQ_TX_DONE, 10_000).is_some());
    }
}
