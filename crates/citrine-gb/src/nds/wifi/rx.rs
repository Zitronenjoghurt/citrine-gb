use super::frame::{Frame, HEADER_BYTES, Kind, MP_ACK, MP_CMD};
use super::link::{Link, Purpose, Receiving};
use super::registers::*;
use super::{IRQ_RX_COUNT, IRQ_RX_DONE, IRQ_RX_START, RAM_MASK, Wifi};

pub const RX_HEADER_BYTES: usize = 12;
pub const MP_LAST_SEQCTRL: usize = 0x1F7E;
const RSSI: u16 = 0x40A1;
const UNKNOWN_HEADER: u16 = 0x0040;
const BSSID_MATCH: u16 = 1 << 15;
const BASE_FLAGS: u16 = 0x0010;
const MORE_FRAGMENTS: u16 = 1 << 10;
const WEP_IV_BYTES: usize = 4;
const STAT_FULL: usize = 4;
const STAT_BAD: usize = 6;
const STAT_OK: usize = 7;
const STAT_FLAG_FULL: u16 = 1 << 3;
const STAT_FLAG_BAD: u16 = 1 << 5;
const STAT_FLAG_OK: u16 = 1 << 6;

impl Wifi {
    pub(super) fn check_inbox(&mut self) -> bool {
        let Some(index) = self
            .inbox
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.start <= self.now)
            .min_by_key(|(_, frame)| frame.start)
            .map(|(index, _)| index)
        else {
            return false;
        };
        let frame = self.inbox.remove(index);
        self.arrive(frame);
        true
    }

    pub(super) fn preamble_received(&mut self) {
        self.raise(IRQ_RX_START);
        self.set_rf_status(6);
        let at = self.ring_skip(usize::from(self.reg(W_RXBUF_WRCSR)) * 2, RX_HEADER_BYTES);
        self.set(W_RXTX_ADDR, (at / 2) as u16);
    }

    pub(super) fn received(&mut self, receiving: Receiving) {
        let frame = receiving.frame;
        let flags = receiving.flags;
        if let Purpose::HostReply(client) = receiving.purpose {
            if flags != 0 && self.store(&frame, flags) {
                self.raise(IRQ_RX_DONE);
            }
            if let Some(exchange) = self.exchange.as_mut() {
                exchange.pending &= !(1 << client);
            }
            self.set_rf_status(5);
            self.link = Link::Listening;
            return;
        }
        self.link = Link::Idle;
        self.set_rf_status(1);
        let matched = flags & BSSID_MATCH != 0;
        let cmd = matched && flags & 0xF == 0xC;
        let duplicate = cmd && self.ram16(MP_LAST_SEQCTRL) == frame.sequence();
        if cmd {
            self.set_ram16(MP_LAST_SEQCTRL, frame.sequence());
        }
        if !duplicate && self.store(&frame, flags) {
            self.raise(IRQ_RX_DONE);
        }
        if matched && flags & 0xF == 0x1 && frame.bytes.len() >= HEADER_BYTES + 8 {
            let stamp = &frame.bytes[HEADER_BYTES..HEADER_BYTES + 8];
            let stamp = u64::from_le_bytes(stamp.try_into().unwrap_or_default());
            self.us_count = stamp + frame.data_us();
            self.pre_fired = None;
        }
        self.frame_done();
        if cmd && self.power.sleep_pending.is_none() && self.awake() {
            self.cmd_received(&frame, duplicate);
        }
        self.try_send();
    }

    pub(super) fn filter(&self, frame: &Frame) -> Option<u16> {
        let mac = self.mac();
        let bssid = self.address(W_BSSID);
        let filter = self.reg(W_RXFILTER);
        let first = frame.address(1);
        if first[0] & 1 == 0 && first != mac {
            return None;
        }
        if frame.wep() && self.reg(W_WEP_CNT) & 0x8000 == 0 {
            return None;
        }
        let control = frame.control();
        let mut flags = BASE_FLAGS;
        if control & MORE_FRAGMENTS != 0 {
            flags |= 0x0100;
        }
        if frame.sequence() & 0xF != 0 || control & MORE_FRAGMENTS != 0 {
            flags |= 0x0200;
        }
        let (kind, matched) = match frame.kind() {
            Kind::Management(subtype) => {
                let matched = frame.address(3) == bssid;
                match subtype {
                    8 if !matched && filter & 1 == 0 => return None,
                    8 => (0x1, matched),
                    0..=5 | 10..=12 if !matched && filter & 0x0600 == 0 => return None,
                    _ => (0x0, matched),
                }
            }
            Kind::Control(10) => {
                let matched = first == bssid;
                if !matched && filter & 0x0800 == 0 {
                    return None;
                }
                (0x5, matched)
            }
            Kind::Control(_) => return None,
            Kind::Data(subtype) => {
                let ds = (control >> 8) & 3;
                let matched = match ds {
                    0 => frame.address(3) == bssid,
                    1 => first == bssid,
                    2 => frame.address(2) == bssid,
                    _ => false,
                };
                if !matched && self.reg(W_RXFILTER2) & (1 << ds) != 0 {
                    return None;
                }
                let kind = if frame.is_mp_reply() {
                    if subtype == 5 { 0xF } else { 0xE }
                } else if first == MP_CMD {
                    0xC
                } else if first == MP_ACK {
                    0xD
                } else {
                    0x8
                };
                if kind == 0xF && filter & 0x0100 == 0 {
                    return None;
                }
                (kind, matched)
            }
            Kind::Reserved => (0x0, false),
        };
        Some(flags | kind | if matched { BSSID_MATCH } else { 0 })
    }

    pub(super) fn write_rx_control(&mut self, value: u16) {
        if value & 1 != 0 {
            let start = self.reg(W_RXBUF_WR_ADDR);
            self.set(W_RXBUF_WRCSR, start);
        }
        if value & 0x80 != 0 {
            let next = self.reg(W_TXBUF_REPLY1);
            self.set(W_TXBUF_REPLY2, next);
            self.set(W_TXBUF_REPLY1, 0);
        }
        self.set(W_RXCNT, value & 0xFF0E);
    }

    pub(super) fn read_rx_data(&mut self) -> u16 {
        let at = usize::from(self.reg(W_RXBUF_RD_ADDR)) & RAM_MASK;
        let value = self.ram16(at);
        let (begin, end) = self.ring();
        let mut next = (at + 2) & RAM_MASK;
        if next == end {
            next = begin;
        }
        let gap = usize::from(self.reg(W_RXBUF_GAP));
        if gap != 0 && next == gap {
            next += 2 * usize::from(self.reg(W_RXBUF_GAPDISP));
            if next >= end && end > begin {
                next = next + begin - end;
            }
            next &= RAM_MASK;
            if self.calibration.lite {
                self.set(W_RXBUF_GAPDISP, 0);
            }
        }
        self.set(W_RXBUF_RD_ADDR, next as u16);
        self.set(W_RXBUF_RD_DATA, value);
        self.count_rx_read();
        value
    }

    pub(super) fn count_rx_read(&mut self) {
        let count = self.reg(W_RXBUF_COUNT);
        if count != 0 {
            self.set(W_RXBUF_COUNT, count - 1);
            if count == 1 {
                self.raise(IRQ_RX_COUNT);
            }
        }
    }

    fn arrive(&mut self, frame: Frame) {
        if frame.start + frame.preamble_us() <= self.now
            || self.channel == 0
            || frame.channel != self.channel
            || !self.awake()
            || self.power.sleep_pending.is_some()
        {
            return;
        }
        match self.link {
            Link::Listening => return self.reply_arrived(frame),
            Link::Idle => {}
            _ => return,
        }
        if self.reg(W_RXCNT) & 0x8000 == 0 {
            return;
        }
        if !frame.fcs_ok() {
            self.count_stat(STAT_BAD, STAT_FLAG_BAD, 8);
            return;
        }
        let Some(flags) = self.filter(&frame) else {
            return;
        };
        self.link = Link::Receiving(Receiving {
            frame,
            flags,
            purpose: Purpose::Normal,
            started: false,
        });
    }

    fn ring(&self) -> (usize, usize) {
        (
            usize::from(self.reg(W_RXBUF_BEGIN)) & RAM_MASK,
            usize::from(self.reg(W_RXBUF_END)) & RAM_MASK,
        )
    }

    fn ring_next(&self, at: usize) -> usize {
        let (begin, end) = self.ring();
        let next = (at + 2) & RAM_MASK;
        if next == end { begin } else { next }
    }

    fn ring_skip(&self, at: usize, bytes: usize) -> usize {
        (0..bytes / 2).fold(at, |at, _| self.ring_next(at))
    }

    fn store(&mut self, frame: &Frame, flags: u16) -> bool {
        let (begin, end) = self.ring();
        if begin >= end {
            return false;
        }
        let crop = self.reg(W_RX_LEN_CROP);
        let mut body = frame.bytes.clone();
        let crop = if frame.wep() {
            if body.len() >= HEADER_BYTES + WEP_IV_BYTES {
                body.drain(HEADER_BYTES..HEADER_BYTES + WEP_IV_BYTES);
            }
            crop >> 8
        } else {
            crop & 0xFF
        };
        body.truncate(body.len().saturating_sub(2 * usize::from(crop)));
        let start = usize::from(self.reg(W_RXBUF_WRCSR)) * 2;
        let read = usize::from(self.reg(W_RXBUF_READCSR)) * 2;
        let size = end - begin;
        let free = match read.cmp(&start) {
            std::cmp::Ordering::Equal => size,
            std::cmp::Ordering::Greater => read - start,
            std::cmp::Ordering::Less => size - (start - read),
        };
        let need = RX_HEADER_BYTES + body.len().next_multiple_of(4);
        if need >= free {
            self.count_stat(STAT_FULL, STAT_FLAG_FULL, 8);
            return false;
        }
        let length = body.len() as u16;
        let header = [
            flags,
            UNKNOWN_HEADER,
            0,
            u16::from(frame.rate.code()),
            length,
            RSSI,
        ];
        let mut at = start;
        for (index, value) in header.into_iter().enumerate() {
            if index != 2 {
                self.set_ram16(at, value);
            }
            at = self.ring_next(at);
        }
        for pair in body.chunks(2) {
            let value = u16::from_le_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]);
            self.set_ram16(at, value);
            at = self.ring_next(at);
        }
        if (at - begin) % 4 != 0 {
            at = self.ring_next(at);
        }
        self.set(W_RXTX_ADDR, (at / 2) as u16);
        self.set(W_RXBUF_WRCSR, (at / 2) as u16);
        self.count_stat(STAT_OK, STAT_FLAG_OK, 0);
        true
    }

    fn count_stat(&mut self, counter: usize, flag: u16, count_shift: u16) {
        let offset = W_RXSTAT + (counter & !1);
        let shift = 8 * (counter & 1);
        let value = self.reg(offset);
        let byte = ((value >> shift) as u8).wrapping_add(1);
        self.set(
            offset,
            (value & !(0xFF << shift)) | (u16::from(byte) << shift),
        );
        let flags = self.reg(W_RXSTAT_INC_IF) | flag;
        self.set(W_RXSTAT_INC_IF, flags);
        if byte & 0x80 != 0 {
            let overflow = self.reg(W_RXSTAT_OVF_IF) | flag;
            self.set(W_RXSTAT_OVF_IF, overflow);
        }
        let total = self.reg(W_RX_COUNT);
        let part = ((total >> count_shift) as u8).wrapping_add(1);
        self.set(
            W_RX_COUNT,
            (total & !(0xFF << count_shift)) | (u16::from(part) << count_shift),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::frame::{Rate, seal};
    use super::super::tests::reg;
    use super::super::tx::tests::{awake_on, data_frame};
    use super::*;

    const ME: [u8; 6] = [0x00, 0x09, 0xBF, 0x00, 0x00, 0x01];
    const OTHER: [u8; 6] = [0x00, 0x09, 0xBF, 0x00, 0x00, 0x02];
    const BROADCAST: [u8; 6] = [0xFF; 6];

    fn listening() -> Wifi {
        let mut wifi = awake_on(7);
        for (half, value) in [0x0900, 0x00BF, 0x0100].into_iter().enumerate() {
            wifi.write16(W_MACADDR + 2 * half, value);
        }
        wifi.write16(W_RXCNT, 0x8001);
        wifi
    }

    fn on_air(wifi: &Wifi, body: Vec<u8>) -> Frame {
        let mut bytes = body;
        bytes.extend_from_slice(&[0; 4]);
        seal(&mut bytes);
        Frame {
            start: wifi.now_us(),
            channel: 7,
            rate: Rate::Mbit2,
            short_preamble: true,
            bytes,
        }
    }

    fn take(wifi: &mut Wifi, frame: Frame, limit: u64) -> u16 {
        wifi.receive(frame);
        let start = wifi.now_us();
        let mut seen = 0;
        while wifi.now_us() - start < limit {
            let wait = wifi.until_event_us().unwrap_or(limit).max(1);
            wifi.step_us(wait);
            seen |= wifi.reg(W_IF);
            wifi.write16(W_IF, 0xFFFF);
        }
        seen
    }

    #[test]
    fn a_frame_for_us_lands_in_the_ring_with_its_header() {
        let mut wifi = listening();
        let body = data_frame(ME, OTHER, &[0xA1, 0xA2, 0xA3]);
        let frame = on_air(&wifi, body.clone());
        let seen = take(&mut wifi, frame, 2000);
        assert_eq!(
            seen & (IRQ_RX_START | IRQ_RX_DONE),
            IRQ_RX_START | IRQ_RX_DONE
        );
        assert_eq!(reg(&mut wifi, 0x4000), 0x0018);
        assert_eq!(reg(&mut wifi, 0x4002), 0x0040);
        assert_eq!(reg(&mut wifi, 0x4006), 0x0014);
        assert_eq!(reg(&mut wifi, 0x4008), body.len() as u16);
        assert_eq!(reg(&mut wifi, 0x400C), 0x0008);
        assert_eq!(reg(&mut wifi, 0x400C + 24), 0xA2A1);
        let stored = RX_HEADER_BYTES + body.len().next_multiple_of(4);
        assert_eq!(usize::from(reg(&mut wifi, W_RXBUF_WRCSR)) * 2, stored);
    }

    #[test]
    fn rx_start_comes_after_the_preamble_and_rx_done_at_the_end() {
        let mut wifi = listening();
        let frame = on_air(&wifi, data_frame(ME, OTHER, &[0; 8]));
        let airtime = frame.airtime_us();
        wifi.receive(frame);
        let start = wifi.now_us();
        let mut times = Vec::new();
        while times.len() < 2 {
            let wait = wifi.until_event_us().unwrap().max(1);
            wifi.step_us(wait);
            let flags = wifi.reg(W_IF);
            if flags != 0 {
                times.push((wifi.now_us() - start, flags));
                wifi.write16(W_IF, flags);
            }
        }
        assert_eq!(times, [(96, IRQ_RX_START), (airtime, IRQ_RX_DONE)]);
    }

    #[test]
    fn frames_for_another_mac_are_dropped() {
        let mut wifi = listening();
        let frame = on_air(&wifi, data_frame(OTHER, ME, &[0; 8]));
        assert_eq!(take(&mut wifi, frame, 2000), 0);
        assert_eq!(reg(&mut wifi, W_RXBUF_WRCSR), 0);
    }

    #[test]
    fn beacons_from_other_networks_need_rxfilter_bit_zero() {
        let mut wifi = listening();
        let mut beacon = vec![0x80, 0x00, 0, 0];
        beacon.extend_from_slice(&BROADCAST);
        beacon.extend_from_slice(&OTHER);
        beacon.extend_from_slice(&OTHER);
        beacon.extend_from_slice(&[0; 2 + 12]);
        wifi.write16(W_RXFILTER, 0);
        let frame = on_air(&wifi, beacon.clone());
        assert_eq!(take(&mut wifi, frame, 2000), 0);
        wifi.write16(W_RXFILTER, 1);
        let frame = on_air(&wifi, beacon);
        assert_ne!(take(&mut wifi, frame, 2000) & IRQ_RX_DONE, 0);
        assert_eq!(reg(&mut wifi, 0x4000), 0x0011);
    }

    #[test]
    fn a_beacon_from_our_bssid_sets_the_counter_to_its_stamp() {
        let mut wifi = listening();
        for half in 0..3 {
            wifi.write16(W_BSSID + 2 * half, 0x5555);
        }
        let mut beacon = vec![0x80, 0x00, 0, 0];
        beacon.extend_from_slice(&BROADCAST);
        beacon.extend_from_slice(&[0x55; 6]);
        beacon.extend_from_slice(&[0x55; 6]);
        beacon.extend_from_slice(&[0, 0]);
        beacon.extend_from_slice(&50_000u64.to_le_bytes());
        beacon.extend_from_slice(&[0; 4]);
        let frame = on_air(&wifi, beacon);
        let data = frame.data_us();
        take(&mut wifi, frame, 1);
        while wifi.reg(W_RXBUF_WRCSR) == 0 {
            wifi.step_us(1);
        }
        assert_eq!(wifi.us_count, 50_000 + data);
        assert_eq!(reg(&mut wifi, 0x4000), 0x8011);
    }

    #[test]
    fn a_bad_fcs_is_counted_and_dropped() {
        let mut wifi = listening();
        let mut frame = on_air(&wifi, data_frame(ME, OTHER, &[0; 8]));
        frame.bytes[30] ^= 0xFF;
        assert_eq!(take(&mut wifi, frame, 2000), 0);
        assert_eq!(reg(&mut wifi, W_RXSTAT + 6), 0x0001);
        assert_eq!(reg(&mut wifi, W_RXSTAT + 6), 0);
    }

    #[test]
    fn the_ring_never_passes_readcsr() {
        let mut wifi = listening();
        wifi.write16(W_RXBUF_END, 0x4040);
        let body = data_frame(ME, OTHER, &[0; 8]);
        let first = on_air(&wifi, body.clone());
        assert_ne!(take(&mut wifi, first, 2000) & IRQ_RX_DONE, 0);
        let second = on_air(&wifi, body);
        assert_eq!(take(&mut wifi, second, 2000) & IRQ_RX_DONE, 0);
        assert_eq!(reg(&mut wifi, W_RXSTAT + 4), 0x0001);
    }

    #[test]
    fn the_ring_wraps_from_end_to_begin() {
        let mut wifi = listening();
        wifi.write16(W_RXBUF_BEGIN, 0x4100);
        wifi.write16(W_RXBUF_END, 0x4140);
        wifi.write16(W_RXBUF_WR_ADDR, 0x0098);
        wifi.write16(W_RXBUF_READCSR, 0x0090);
        wifi.write16(W_RXCNT, 0x8001);
        let frame = on_air(&wifi, data_frame(ME, OTHER, &[7; 4]));
        assert_ne!(take(&mut wifi, frame, 2000) & IRQ_RX_DONE, 0);
        assert_eq!(reg(&mut wifi, 0x4130), 0x0018);
        assert_eq!(reg(&mut wifi, 0x413C), 0x0008);
        assert_eq!(reg(&mut wifi, 0x4100), 0x0900);
    }

    #[test]
    fn rd_data_reads_through_the_ring_and_counts_down() {
        let mut wifi = listening();
        wifi.write16(0x47FE, 0x1111);
        wifi.write16(0x4000, 0x2222);
        wifi.write16(W_RXBUF_RD_ADDR, 0x07FE);
        wifi.write16(W_RXBUF_COUNT, 2);
        assert_eq!(reg(&mut wifi, W_RXBUF_RD_DATA), 0x1111);
        assert_eq!(reg(&mut wifi, W_RXBUF_RD_ADDR), 0x0000);
        assert_eq!(reg(&mut wifi, 0x1000 + W_RXBUF_RD_DATA), 0x1111);
        assert_eq!(reg(&mut wifi, W_RXBUF_RD_DATA), 0x2222);
        assert_eq!(reg(&mut wifi, W_RXBUF_COUNT), 0);
        assert_eq!(reg(&mut wifi, W_IF) & IRQ_RX_COUNT, IRQ_RX_COUNT);
    }

    #[test]
    fn rxcnt_copies_write_address_and_moves_reply1() {
        let mut wifi = listening();
        wifi.write16(W_RXBUF_WR_ADDR, 0x0123);
        wifi.write16(W_TXBUF_REPLY1, 0x8456);
        wifi.write16(W_RXCNT, 0x8081);
        assert_eq!(reg(&mut wifi, W_RXBUF_WRCSR), 0x0123);
        assert_eq!(reg(&mut wifi, W_TXBUF_REPLY2), 0x8456);
        assert_eq!(reg(&mut wifi, W_TXBUF_REPLY1), 0);
        assert_eq!(reg(&mut wifi, W_RXCNT), 0x8000);
    }

    fn cmd_from(host: [u8; 6], sequence: u16) -> Vec<u8> {
        let mut body = vec![0x28, 0x02, 0, 0];
        body.extend_from_slice(&MP_CMD);
        body.extend_from_slice(&host);
        body.extend_from_slice(&host);
        body.extend_from_slice(&(sequence << 4).to_le_bytes());
        body.extend_from_slice(&382u16.to_le_bytes());
        body.extend_from_slice(&0x0002u16.to_le_bytes());
        body.extend_from_slice(&0x8000u16.to_le_bytes());
        body
    }

    fn until_cmd_stored(wifi: &mut Wifi, frame: Frame) {
        wifi.receive(frame);
        while wifi.reg(W_IF) & IRQ_RX_DONE == 0 {
            let wait = wifi.until_event_us().unwrap().max(1);
            wifi.step_us(wait);
        }
        wifi.write16(W_IF, 0xFFFF);
    }

    #[test]
    fn a_cmd_counts_the_new_reply_and_marks_the_old_one_done_before_any_reply_is_sent() {
        let mut wifi = listening();
        for (half, value) in [0x0900, 0x00BF, 0x0200].into_iter().enumerate() {
            wifi.write16(W_BSSID + 2 * half, value);
        }
        wifi.write16(W_AID_LOW, 1);
        for at in [0x0200, 0x0300] {
            wifi.write16(0x4000 + at + 8, 0x0014);
            wifi.write16(0x4000 + at + 0xA, 30);
        }
        wifi.write16(W_TXBUF_REPLY1, 0x8000 | 0x0100);
        let cmd = on_air(&wifi, cmd_from(OTHER, 1));
        until_cmd_stored(&mut wifi, cmd);
        assert_eq!(reg(&mut wifi, W_TXBUF_REPLY2), 0x8100);
        assert_eq!(reg(&mut wifi, 0x4204), 0x0001);
        assert_eq!(reg(&mut wifi, 0x4200), 0x0000);
        let quiet = wifi.until_event_us().unwrap_or(4000);
        wifi.step_us(quiet + 4000);
        wifi.write16(W_IF, 0xFFFF);
        wifi.write16(W_TXBUF_REPLY1, 0x8000 | 0x0180);
        let cmd = on_air(&wifi, cmd_from(OTHER, 2));
        until_cmd_stored(&mut wifi, cmd);
        assert_eq!(reg(&mut wifi, 0x4200), 0x0001);
        assert_eq!(reg(&mut wifi, W_TXBUF_REPLY2), 0x8180);
        assert_eq!(reg(&mut wifi, 0x4304), 0x0001);
    }

    #[test]
    fn nothing_is_received_with_rxcnt_off_or_on_another_channel() {
        let mut wifi = listening();
        wifi.write16(W_RXCNT, 0);
        let frame = on_air(&wifi, data_frame(ME, OTHER, &[0; 8]));
        assert_eq!(take(&mut wifi, frame, 2000), 0);
        wifi.write16(W_RXCNT, 0x8000);
        let mut frame = on_air(&wifi, data_frame(ME, OTHER, &[0; 8]));
        frame.channel = 1;
        assert_eq!(take(&mut wifi, frame, 2000), 0);
    }
}
