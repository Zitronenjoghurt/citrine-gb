use super::Wifi;
use super::frame::Frame;
use crate::nds::interrupt::Irq;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sent {
    pub from: usize,
    pub frame: Frame,
}

pub const SCANLINE_LEAD_US: u64 = 63;
pub const BURST_LEAD_US: u64 = 1;

pub trait Station {
    fn now_us(&self) -> u64;
    fn run_until(&mut self, us: u64);
    fn take_frames(&mut self) -> Vec<Frame>;
    fn receive(&mut self, frame: Frame);
    fn on_air(&self) -> bool;
}

pub trait WifiPeer {
    fn exchange(&mut self, chip: &mut Wifi);

    fn delivers(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NoPeer;

impl WifiPeer for NoPeer {
    fn exchange(&mut self, chip: &mut Wifi) {
        chip.take_frames();
    }

    fn delivers(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Air {
    now: u64,
    delays: Vec<Vec<i64>>,
    log: Vec<Sent>,
    logging: bool,
}

impl Air {
    pub fn new(chips: usize) -> Self {
        Self {
            now: 0,
            delays: vec![vec![0; chips]; chips],
            log: Vec::new(),
            logging: false,
        }
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn set_delay(&mut self, from: usize, to: usize, us: i64) {
        self.delays[from][to] = us;
    }

    pub fn set_logging(&mut self, on: bool) {
        self.logging = on;
    }

    pub fn log(&self) -> &[Sent] {
        &self.log
    }

    pub fn take_log(&mut self) -> Vec<Sent> {
        std::mem::take(&mut self.log)
    }

    pub fn until_event(&self, chips: &[&mut Wifi]) -> Option<u64> {
        chips.iter().filter_map(|chip| chip.until_event_us()).min()
    }

    pub fn run(
        &mut self,
        chips: &mut [&mut Wifi],
        us: u64,
        mut software: impl FnMut(usize, &mut Wifi, Irq),
    ) {
        let end = self.now + us;
        loop {
            let left = end - self.now;
            let wait = self.until_event(chips).unwrap_or(left).min(left);
            for (index, chip) in chips.iter_mut().enumerate() {
                let irq = chip.step_us(wait);
                if !irq.is_empty() {
                    software(index, chip, irq);
                }
            }
            self.now += wait;
            self.route(chips);
            if self.now >= end {
                return;
            }
        }
    }

    pub fn lead_us(stations: &[&mut dyn Station]) -> u64 {
        if stations.iter().any(|station| station.on_air()) {
            BURST_LEAD_US
        } else {
            SCANLINE_LEAD_US
        }
    }

    pub fn run_stations(&mut self, stations: &mut [&mut dyn Station], until: u64) {
        loop {
            let Some(slowest) = stations.iter().map(|station| station.now_us()).min() else {
                return;
            };
            self.now = slowest;
            if slowest >= until {
                return;
            }
            let horizon = (slowest + Self::lead_us(stations)).min(until);
            for from in 0..stations.len() {
                if stations[from].now_us() < horizon {
                    stations[from].run_until(horizon);
                }
                self.broadcast(stations, from);
            }
        }
    }

    fn broadcast(&mut self, stations: &mut [&mut dyn Station], from: usize) {
        for frame in stations[from].take_frames() {
            for (to, station) in stations.iter_mut().enumerate() {
                if to != from {
                    let mut copy = frame.clone();
                    copy.start = copy.start.saturating_add_signed(self.delays[from][to]);
                    station.receive(copy);
                }
            }
            if self.logging {
                self.log.push(Sent { from, frame });
            }
        }
    }

    pub fn route(&mut self, chips: &mut [&mut Wifi]) {
        for from in 0..chips.len() {
            for frame in chips[from].take_frames() {
                for (to, chip) in chips.iter_mut().enumerate() {
                    if to != from {
                        let mut copy = frame.clone();
                        copy.start = chip.now_us().saturating_add_signed(self.delays[from][to]);
                        chip.receive(copy);
                    }
                }
                if self.logging {
                    self.log.push(Sent { from, frame });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::calibration::Calibration;
    use super::super::frame::{self, Kind, MP_CMD, Rate};
    use super::super::mp::{ACK_BYTES, AIR_ROUNDING_US, FIRST_REPLY_GAP_US, SLOT_GAP_US};
    use super::super::power::WAKE_US;
    use super::super::registers::*;
    use super::super::rx::MP_LAST_SEQCTRL;
    use super::super::{IRQ_BEACON, IRQ_MP_END, IRQ_RX_DONE, IRQ_TX_DONE};
    use super::*;
    use crate::nds::spi::firmware::Firmware;

    const CHANNEL: u8 = 7;
    const BEACON_AT: usize = 0x1000;
    const LOC1_AT: usize = 0x1200;
    const CMD_AT: usize = 0x1400;
    const REPLY_AT: [usize; 2] = [0x1600, 0x1700];
    const REPLY_TIME: u16 = 36 * 4 + 0xD0;
    const PAYLOAD: usize = 8;
    const CMD_BYTES: usize = 24 + 4 + PAYLOAD + 4;
    const REPLY_BYTES: usize = 24 + PAYLOAD + 4;

    fn mac(last: u8) -> [u8; 6] {
        [0x00, 0x09, 0xBF, 0x12, 0x34, last]
    }

    fn write_address(wifi: &mut Wifi, offset: usize, address: [u8; 6]) {
        for half in 0..3 {
            let value = u16::from_le_bytes([address[2 * half], address[2 * half + 1]]);
            wifi.write16(offset + 2 * half, value);
        }
    }

    fn boot(last: u8) -> Wifi {
        let firmware = Firmware::default();
        let calibration = Calibration::read(firmware.bytes());
        let mut wifi = Wifi::new(&firmware);
        wifi.set_powered(true);
        wifi.write16(W_POWER_US, 0);
        wifi.write16(W_MODE_RST, 0);
        wifi.write16(W_MODE_RST, 0x6000);
        wifi.write16(W_BB_POWER, 0);
        for (index, &value) in calibration.baseband.iter().enumerate() {
            wifi.write16(W_BB_WRITE, u16::from(value));
            wifi.write16(W_BB_CNT, 0x5000 | index as u16);
        }
        for &write in &calibration.radio_init {
            let (data1, data2) = super::super::radio::words(calibration.radio, write);
            wifi.write16(W_RF_DATA1, data1);
            wifi.write16(W_RF_DATA2, data2);
        }
        for (offset, value) in super::super::calibration::REGISTERS
            .into_iter()
            .zip(calibration.registers)
        {
            wifi.write16(offset, value);
        }
        write_address(&mut wifi, W_MACADDR, mac(last));
        wifi.write16(W_IE, IRQ_RX_DONE | IRQ_TX_DONE | IRQ_MP_END | IRQ_BEACON);
        wifi.write16(W_MODE_RST, 1);
        wifi.write16(W_POWERFORCE, 0x8000);
        wifi.write16(W_POWERFORCE, 0);
        wifi.write16(W_US_COUNTCNT, 1);
        wifi.write16(W_RXCNT, 0x8001);
        tune(&mut wifi, CHANNEL);
        wifi
    }

    fn tune(wifi: &mut Wifi, channel: u8) {
        let calibration = Calibration::read(Firmware::default().bytes());
        for write in calibration.channels[usize::from(channel) - 1] {
            let (data1, data2) = super::super::radio::words(calibration.radio, write);
            wifi.write16(W_RF_DATA1, data1);
            wifi.write16(W_RF_DATA2, data2);
        }
    }

    fn put(wifi: &mut Wifi, at: usize, rate: u8, frame: &[u8]) {
        let mut bytes = vec![0u8; 12];
        bytes[8] = rate;
        bytes[0xA..0xC].copy_from_slice(&((frame.len() + 4) as u16).to_le_bytes());
        bytes.extend_from_slice(frame);
        bytes.extend_from_slice(&[0; 4]);
        if bytes.len() % 2 == 1 {
            bytes.push(0);
        }
        for (index, pair) in bytes.chunks(2).enumerate() {
            wifi.write16(
                0x4000 + at + 2 * index,
                u16::from_le_bytes([pair[0], pair[1]]),
            );
        }
    }

    fn header(control: u16, to: [u8; 6], from: [u8; 6], bss: [u8; 6]) -> Vec<u8> {
        let mut bytes = control.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&to);
        bytes.extend_from_slice(&from);
        bytes.extend_from_slice(&bss);
        bytes.extend_from_slice(&[0, 0]);
        bytes
    }

    fn send_loc1(wifi: &mut Wifi, frame: &[u8]) {
        put(wifi, LOC1_AT, 0x14, frame);
        wifi.write16(W_TXREQ_SET, 1);
        wifi.write16(W_TXBUF_LOC1, 0x8000 | (LOC1_AT / 2) as u16);
    }

    fn drain(wifi: &mut Wifi) -> Vec<(u16, Vec<u8>)> {
        let mut frames = Vec::new();
        let (begin, end) = (0x0000, 0x0800);
        while wifi.read16(W_RXBUF_READCSR) != wifi.read16(W_RXBUF_WRCSR) {
            let start = usize::from(wifi.read16(W_RXBUF_READCSR)) * 2;
            wifi.write16(W_RXBUF_RD_ADDR, start as u16);
            let mut halves = Vec::new();
            for _ in 0..6 {
                halves.push(wifi.read16(W_RXBUF_RD_DATA));
            }
            let length = usize::from(halves[4]);
            let mut bytes = Vec::new();
            for _ in 0..length.div_ceil(2) {
                bytes.extend_from_slice(&wifi.read16(W_RXBUF_RD_DATA).to_le_bytes());
            }
            bytes.truncate(length);
            let used = 12 + length.next_multiple_of(4);
            let mut next = start + used;
            if next >= end {
                next = next - end + begin;
            }
            wifi.write16(W_RXBUF_READCSR, (next / 2) as u16);
            frames.push((halves[0], bytes));
        }
        frames
    }

    #[derive(Default)]
    struct Host {
        exchanges: u32,
        target: u32,
        echoes: Vec<u64>,
        statuses: Vec<(u16, u16)>,
        replies_seen: u32,
        cmd_count: u16,
        period: Option<u64>,
        due: Option<u64>,
    }

    impl Host {
        fn pace(&mut self, wifi: &mut Wifi) {
            if self.due.is_some_and(|due| wifi.now_us() >= due) {
                self.due = None;
                self.send_cmd(wifi);
            }
        }

        fn start(&self, wifi: &mut Wifi) {
            let me = wifi.mac();
            write_address(wifi, W_BSSID, me);
            let mut beacon = header(0x0080, [0xFF; 6], me, me);
            beacon.extend_from_slice(&[0; 8]);
            beacon.extend_from_slice(&0x00D0u16.to_le_bytes());
            beacon.extend_from_slice(&0x0021u16.to_le_bytes());
            beacon.extend_from_slice(&[0xDD, 0x08, 0x00, 0x09, 0xBF, 0x00, b'C', b'G', b'B', b'1']);
            put(wifi, BEACON_AT, 0x14, &beacon);
            wifi.write16(W_TXBUF_BEACON, 0x8000 | (BEACON_AT / 2) as u16);
            wifi.write16(W_BEACONINT, 0x00D0);
            wifi.write16(W_BEACON_COUNT, 0x0010);
            wifi.write16(W_US_COMPARECNT, 1);
            wifi.write16(W_RXFILTER, 0x0581);
            wifi.write16(W_PREAMBLE, 6);
        }

        fn send_cmd(&self, wifi: &mut Wifi) {
            let me = wifi.mac();
            let mut cmd = header(0x0228, MP_CMD, me, me);
            frame::set_half(&mut cmd, 2, 0);
            cmd.extend_from_slice(&REPLY_TIME.to_le_bytes());
            cmd.extend_from_slice(&0x0002u16.to_le_bytes());
            cmd.extend_from_slice(&u64::from(self.exchanges).to_le_bytes());
            assert_eq!(cmd.len() + 4, CMD_BYTES);
            put(wifi, CMD_AT, 0x14, &cmd);
            wifi.write16(W_CMD_REPLYTIME, REPLY_TIME);
            wifi.write16(W_CMD_COUNT, self.cmd_count);
            wifi.write16(W_TXREQ_SET, 2);
            wifi.write16(W_TXBUF_CMD, 0x8000 | (CMD_AT / 2) as u16);
        }

        fn on_irq(&mut self, wifi: &mut Wifi) {
            let flags = wifi.read16(W_IF);
            wifi.write16(W_IF, flags);
            if flags & IRQ_RX_DONE != 0 {
                for (rx, bytes) in drain(wifi) {
                    let from: [u8; 6] = bytes[10..16].try_into().unwrap();
                    match (rx & 0xF, frame::half(&bytes, 0)) {
                        (0x0, 0x00B0) => {
                            let mut auth = header(0x00B0, from, wifi.mac(), wifi.mac());
                            auth.extend_from_slice(&[0, 0, 2, 0, 0, 0]);
                            send_loc1(wifi, &auth);
                        }
                        (0x0, 0x0000) => {
                            let mut answer = header(0x0010, from, wifi.mac(), wifi.mac());
                            answer.extend_from_slice(&[0x21, 0, 0, 0, 0x01, 0xC0]);
                            send_loc1(wifi, &answer);
                        }
                        (0xE, _) => {
                            self.replies_seen += 1;
                            self.echoes
                                .push(u64::from_le_bytes(bytes[24..32].try_into().unwrap()));
                        }
                        _ => {}
                    }
                }
            }
            if flags & IRQ_TX_DONE != 0 && wifi.read16(W_TXSTAT) == 0x0001 {
                let answer = frame::half(&wifi.ram, LOC1_AT + 12);
                if answer == 0x0010 && self.exchanges == 0 && self.target > 0 {
                    self.send_cmd(wifi);
                }
            }
            if flags & IRQ_MP_END != 0 {
                let status = frame::half(&wifi.ram, CMD_AT);
                let failed = frame::half(&wifi.ram, CMD_AT + 2);
                self.statuses.push((status, failed));
                self.exchanges += 1;
                if self.exchanges < self.target {
                    if let Some(period) = self.period {
                        self.due = Some((wifi.now_us() / period + 1) * period);
                    } else {
                        self.send_cmd(wifi);
                    }
                }
            }
        }
    }

    #[derive(Default)]
    struct Client {
        host: Option<[u8; 6]>,
        aid: u16,
        cmds: Vec<u64>,
        acks: Vec<u16>,
        next: usize,
        wake_on_cmd: bool,
    }

    impl Client {
        fn prepare_reply(&mut self, wifi: &mut Wifi, echo: u64) {
            let host = self.host.unwrap();
            let mut reply = header(0x0118, host, wifi.mac(), frame::MP_REPLY);
            reply.extend_from_slice(&echo.to_le_bytes());
            assert_eq!(reply.len() + 4, REPLY_BYTES);
            let at = REPLY_AT[self.next];
            put(wifi, at, 0x14, &reply);
            wifi.write16(W_TXBUF_REPLY1, 0x8000 | (at / 2) as u16);
            self.next ^= 1;
        }

        fn on_irq(&mut self, wifi: &mut Wifi) {
            let flags = wifi.read16(W_IF);
            wifi.write16(W_IF, flags);
            if flags & IRQ_RX_DONE == 0 {
                return;
            }
            for (rx, bytes) in drain(wifi) {
                match rx & 0xF {
                    0x1 if self.host.is_none() => {
                        let host: [u8; 6] = bytes[16..22].try_into().unwrap();
                        assert_eq!(&bytes[36..40], &[0xDD, 0x08, 0x00, 0x09]);
                        self.host = Some(host);
                        write_address(wifi, W_BSSID, host);
                        let mut auth = header(0x00B0, host, wifi.mac(), host);
                        auth.extend_from_slice(&[0, 0, 1, 0, 0, 0]);
                        send_loc1(wifi, &auth);
                    }
                    0x0 if frame::half(&bytes, 0) == 0x00B0 => {
                        let host = self.host.unwrap();
                        let mut assoc = header(0x0000, host, wifi.mac(), host);
                        assoc.extend_from_slice(&[0x21, 0, 0x0A, 0]);
                        send_loc1(wifi, &assoc);
                    }
                    0x0 if frame::half(&bytes, 0) == 0x0010 => {
                        self.aid = frame::half(&bytes, 28) & 0x07FF;
                        wifi.write16(W_AID_FULL, self.aid);
                        wifi.write16(W_AID_LOW, self.aid & 0xF);
                        self.prepare_reply(wifi, u64::MAX);
                    }
                    0xC => {
                        let counter = u64::from_le_bytes(bytes[28..36].try_into().unwrap());
                        self.cmds.push(counter);
                        if self.wake_on_cmd {
                            self.wake_on_cmd = false;
                            wifi.write16(0x4000 + MP_LAST_SEQCTRL, 0xFFFF);
                            wifi.write16(W_AID_LOW, self.aid & 0xF);
                        }
                        self.prepare_reply(wifi, counter);
                    }
                    0xD => self.acks.push(frame::half(&bytes, 26)),
                    _ => {}
                }
            }
        }
    }

    fn session(target: u32, cmd_count: u16) -> (Air, Wifi, Wifi, Host, Client) {
        let mut air = Air::new(2);
        let mut host_chip = boot(1);
        let mut client_chip = boot(2);
        let host = Host {
            target,
            cmd_count,
            ..Host::default()
        };
        let client = Client::default();
        air.run(
            &mut [&mut host_chip, &mut client_chip],
            WAKE_US,
            |_, _, _| {},
        );
        host.start(&mut host_chip);
        client_chip.write16(W_PREAMBLE, 6);
        (air, host_chip, client_chip, host, client)
    }

    fn gbatek_cmd_count() -> u16 {
        let master = CMD_BYTES as u16 * 4 + 96;
        (904 + REPLY_TIME + master + 50) / 10
    }

    fn run(
        air: &mut Air,
        host_chip: &mut Wifi,
        client_chip: &mut Wifi,
        host: &mut Host,
        client: &mut Client,
        us: u64,
    ) {
        air.run(&mut [host_chip, client_chip], us, |index, chip, _| {
            if index == 0 {
                host.on_irq(chip);
            } else {
                client.on_irq(chip);
            }
        });
    }

    fn cmds(log: &[Sent]) -> Vec<&Sent> {
        log.iter().filter(|sent| sent.frame.is_mp_cmd()).collect()
    }

    #[test]
    fn a_host_beacons_a_client_associates_and_100_mp_exchanges_meet_their_windows() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(100, gbatek_cmd_count());
        air.set_logging(true);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            2_000_000,
        );
        assert_eq!(client.host, Some(host_chip.mac()));
        assert_eq!(client.aid, 1);
        assert_eq!(host.exchanges, 100);
        assert!(host.statuses.iter().all(|&status| status == (0x0001, 0)));
        assert_eq!(client.cmds, (0..100).collect::<Vec<u64>>());
        assert_eq!(client.acks.len(), 100);
        assert!(client.acks.iter().all(|&failed| failed == 0));
        assert_eq!(host.replies_seen, 100);
        let mut expected = vec![u64::MAX];
        expected.extend(0..99);
        assert_eq!(host.echoes, expected);

        let log = air.log();
        let beacons = log
            .iter()
            .filter(|sent| sent.from == 0 && sent.frame.kind() == Kind::Management(8))
            .count();
        assert!(beacons >= 1);
        let commands = cmds(log);
        assert_eq!(commands.len(), 100);
        let slot = SLOT_GAP_US + u64::from(REPLY_TIME);
        for command in commands {
            let end = command.frame.end();
            let after: Vec<&Sent> = log
                .iter()
                .filter(|sent| sent.frame.start >= end && sent.frame.start < end + 2 * slot)
                .collect();
            let reply = after.iter().find(|sent| sent.frame.is_mp_reply()).unwrap();
            assert_eq!(reply.from, 1);
            assert_eq!(reply.frame.start - end, FIRST_REPLY_GAP_US);
            assert!(reply.frame.end() <= end + FIRST_REPLY_GAP_US + slot);
            let ack = after.iter().find(|sent| sent.frame.is_mp_ack()).unwrap();
            assert_eq!(ack.frame.start, end + FIRST_REPLY_GAP_US + slot);
            assert_eq!(ack.frame.bytes.len(), ACK_BYTES);
            assert_eq!(command.frame.rate, Rate::Mbit2);
            assert_eq!(command.frame.preamble_us(), 96);
        }
    }

    #[test]
    fn each_cmd_takes_a_new_sequence_number_and_the_ack_the_next() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(5, gbatek_cmd_count());
        air.set_logging(true);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            1_000_000,
        );
        let numbers: Vec<u16> = air
            .log()
            .iter()
            .filter(|sent| sent.from == 0 && (sent.frame.is_mp_cmd() || sent.frame.is_mp_ack()))
            .map(|sent| sent.frame.sequence() >> 4)
            .collect();
        assert_eq!(numbers.len(), 10);
        assert!(numbers.windows(2).all(|pair| pair[1] == pair[0] + 1));
    }

    #[test]
    fn a_reply_after_its_window_fails_the_client_after_one_resend_on_gbatek_timing() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        assert_eq!(client.aid, 1);
        air.set_logging(true);
        air.set_delay(1, 0, 200);
        host.target = 3;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            100_000,
        );
        assert_eq!(host.statuses, [(0x0005, 0x0002); 3]);
        assert_eq!(host.replies_seen, 0);
        assert_eq!(client.acks, [0x0002; 3]);
        assert_eq!(client.cmds, [0, 1, 2]);
        let commands = cmds(air.log());
        let retries: Vec<bool> = commands.iter().map(|sent| sent.frame.retry()).collect();
        assert_eq!(retries, [false, true, false, true, false, true]);
        for pair in commands.chunks(2) {
            assert_eq!(pair[0].frame.sequence(), pair[1].frame.sequence());
            assert_eq!(pair[1].frame.body_half(2), 0x0002);
        }
        let replies = air
            .log()
            .iter()
            .filter(|sent| sent.frame.is_mp_reply())
            .count();
        assert_eq!(replies, 6);
        assert_eq!(host_chip.read16(W_CMD_STAT), 0x0300);
        assert_eq!(host_chip.read16(W_CMD_STAT), 0);
    }

    #[test]
    fn without_time_for_a_second_round_the_host_acks_at_once() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) = session(0, 96);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        air.set_logging(true);
        air.set_delay(1, 0, 200);
        host.target = 1;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            100_000,
        );
        assert_eq!(cmds(air.log()).len(), 1);
        assert_eq!(host.statuses, [(0x0005, 0x0002)]);
        assert_eq!(client.acks, [0x0002]);
    }

    #[test]
    fn the_exchange_after_a_failed_one_completes_once_replies_are_on_time() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        air.set_delay(1, 0, 200);
        host.target = 1;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            50_000,
        );
        assert_eq!(host.statuses, [(0x0005, 0x0002)]);
        air.set_delay(1, 0, 0);
        host.target = 2;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            50_000,
        );
        assert_eq!(host.statuses[1], (0x0001, 0));
        assert_eq!(host.replies_seen, 1);
    }

    #[test]
    fn a_reply_late_but_inside_its_window_still_counts() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        let airtime = 96 + REPLY_BYTES as u64 * 4;
        air.set_delay(1, 0, (SLOT_GAP_US + u64::from(REPLY_TIME) - airtime) as i64);
        host.target = 2;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            100_000,
        );
        assert_eq!(host.statuses, [(0x0001, 0); 2]);
        assert_eq!(host.replies_seen, 2);
    }

    #[test]
    fn a_reply_that_rounding_puts_a_few_microseconds_early_still_counts() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        air.set_delay(1, 0, -(AIR_ROUNDING_US as i64));
        host.target = 2;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            100_000,
        );
        assert_eq!(host.statuses, [(0x0001, 0); 2]);
        assert_eq!(host.replies_seen, 2);
    }

    #[test]
    fn a_cmd_without_time_for_one_round_fails_when_cmd_count_runs_out() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) = session(0, 50);
        air.set_logging(true);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        air.take_log();
        host.target = 1;
        host.send_cmd(&mut host_chip);
        let sent_at = air.now();
        let mut ended = None;
        air.run(
            &mut [&mut host_chip, &mut client_chip],
            10_000,
            |index, chip, _| {
                if index == 0 && chip.read16(W_IF) & IRQ_MP_END != 0 && ended.is_none() {
                    ended = Some(chip.now_us());
                }
                if index == 0 {
                    host.on_irq(chip);
                }
            },
        );
        assert_eq!(ended.map(|at| at - sent_at), Some(500));
        assert_eq!(host.statuses, [(0x0005, 0x0002)]);
        assert!(cmds(air.log()).is_empty());
    }

    #[test]
    fn a_client_without_an_aid_bit_stays_silent() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        client_chip.write16(W_AID_LOW, 3);
        air.set_logging(true);
        host.target = 1;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            50_000,
        );
        assert!(air.log().iter().all(|sent| !sent.frame.is_mp_reply()));
        assert_eq!(host.statuses, [(0x0005, 0x0002)]);
    }

    #[test]
    fn a_resent_cmd_reaches_a_client_that_dropped_the_first_copy() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        client_chip.write16(W_AID_LOW, 0);
        client.wake_on_cmd = true;
        air.set_logging(true);
        host.target = 1;
        host.send_cmd(&mut host_chip);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            50_000,
        );
        let retries: Vec<bool> = cmds(air.log())
            .iter()
            .map(|sent| sent.frame.retry())
            .collect();
        assert_eq!(retries, [false, true]);
        assert_eq!(client.cmds, [0, 0]);
        assert_eq!(host.statuses, [(0x0001, 0)]);
    }

    #[test]
    fn a_client_with_no_reply_queued_sends_an_empty_one() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(0, gbatek_cmd_count());
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            400_000,
        );
        client_chip.write16(W_TXBUF_RESET, 0x00C0);
        air.set_logging(true);
        host.target = 1;
        host.send_cmd(&mut host_chip);
        air.run(
            &mut [&mut host_chip, &mut client_chip],
            50_000,
            |index, chip, _| {
                if index == 0 {
                    host.on_irq(chip);
                } else {
                    let flags = chip.read16(W_IF);
                    chip.write16(W_IF, flags);
                    drain(chip);
                }
            },
        );
        let reply = air
            .log()
            .iter()
            .find(|sent| sent.frame.is_mp_reply())
            .unwrap();
        assert_eq!(reply.frame.control(), 0x0158);
        assert_eq!(reply.frame.bytes.len(), 28);
        assert_eq!(host.statuses, [(0x0001, 0)]);
        assert_eq!(host.replies_seen, 0);
    }

    #[test]
    fn a_scripted_scan_with_no_one_around_finds_nothing_and_ends() {
        let mut air = Air::new(1);
        let mut chip = boot(9);
        air.set_logging(true);
        air.run(&mut [&mut chip], WAKE_US, |_, _, _| {});
        chip.write16(W_US_COMPARECNT, 1);
        chip.write16(W_BEACONINT, 50);
        chip.write16(W_BEACON_COUNT, 50);
        let mut channel = 1;
        let mut found = 0;
        let mut probes_done = 0;
        let probe = |chip: &mut Wifi| {
            let mut request = header(0x0040, [0xFF; 6], chip.mac(), [0xFF; 6]);
            request.extend_from_slice(&[0x00, 0x00, 0x01, 0x02, 0x82, 0x84]);
            send_loc1(chip, &request);
        };
        tune(&mut chip, channel);
        probe(&mut chip);
        air.run(&mut [&mut chip], 13 * 50 * 1024 + 5_000, |_, chip, _| {
            let flags = chip.read16(W_IF);
            chip.write16(W_IF, flags);
            if flags & IRQ_RX_DONE != 0 {
                found += drain(chip).len();
            }
            if flags & IRQ_TX_DONE != 0 && chip.read16(W_TXSTAT) == 0x0001 {
                probes_done += 1;
            }
            if flags & IRQ_BEACON != 0 && channel < 13 {
                channel += 1;
                tune(chip, channel);
                probe(chip);
            }
        });
        assert_eq!(found, 0);
        assert_eq!(probes_done, 13);
        assert_eq!(chip.read16(W_RXBUF_WRCSR), chip.read16(W_RXBUF_READCSR));
        let channels: Vec<u8> = air.log().iter().map(|sent| sent.frame.channel).collect();
        assert_eq!(channels, (1..=13).collect::<Vec<u8>>());
        assert!(air.log().iter().all(|sent| sent.frame.fcs_ok()));
        assert_eq!(chip.read16(W_RF_STATUS), 1);
    }

    enum Software {
        Idle,
        Host(Host),
        Client(Client),
    }

    struct Unit {
        chip: Wifi,
        software: Software,
        us: u64,
        cycles: u64,
        batch: Option<u32>,
        load: u32,
        spent: u64,
    }

    impl Unit {
        fn new(chip: Wifi, software: Software) -> Self {
            Self {
                chip,
                software,
                us: 0,
                cycles: 0,
                batch: None,
                load: 0,
                spent: 0,
            }
        }

        fn batched(mut self, batch: u32, load: u32) -> Self {
            self.batch = Some(batch);
            self.load = load;
            self
        }

        fn host(&mut self) -> &mut Host {
            match &mut self.software {
                Software::Host(host) => host,
                _ => panic!("not a host"),
            }
        }

        fn client(&mut self) -> &mut Client {
            match &mut self.software {
                Software::Client(client) => client,
                _ => panic!("not a client"),
            }
        }

        fn react(&mut self, irq: Irq) {
            if !irq.is_empty() {
                match &mut self.software {
                    Software::Host(host) => host.on_irq(&mut self.chip),
                    Software::Client(client) => client.on_irq(&mut self.chip),
                    Software::Idle => {
                        let flags = self.chip.read16(W_IF);
                        self.chip.write16(W_IF, flags);
                    }
                }
            }
            if let Software::Host(host) = &mut self.software {
                host.pace(&mut self.chip);
            }
        }
    }

    fn burn(seed: u64, load: u32) -> u64 {
        let mut value = seed;
        for _ in 0..load {
            value = std::hint::black_box(
                value
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407),
            );
        }
        value
    }

    const HZ: u64 = crate::nds::rtc::CYCLES_PER_SECOND as u64;

    impl Station for Unit {
        fn now_us(&self) -> u64 {
            self.us
        }

        fn run_until(&mut self, us: u64) {
            let Some(batch) = self.batch else {
                while self.us < us {
                    let left = us - self.us;
                    let wait = self.chip.until_event_us().unwrap_or(left).clamp(1, left);
                    let irq = self.chip.step_us(wait);
                    self.us += wait;
                    self.react(irq);
                }
                return;
            };
            let target = (us * HZ).div_ceil(1_000_000);
            while self.cycles < target {
                self.spent = burn(self.spent, self.load);
                let irq = self.chip.step(batch);
                self.cycles += u64::from(batch);
                self.react(irq);
            }
            self.us = self.cycles * 1_000_000 / HZ;
        }

        fn take_frames(&mut self) -> Vec<Frame> {
            self.chip.take_frames_at(self.us)
        }

        fn receive(&mut self, frame: Frame) {
            self.chip.receive_at(self.us, frame);
        }

        fn on_air(&self) -> bool {
            self.chip.on_air()
        }
    }

    fn units(target: u32, period: Option<u64>, batch: Option<(u32, u32)>) -> (Air, Unit, Unit) {
        let host = Host {
            target,
            cmd_count: gbatek_cmd_count(),
            period,
            ..Host::default()
        };
        let mut host = Unit::new(boot(1), Software::Host(host));
        let mut client = Unit::new(boot(2), Software::Client(Client::default()));
        if let Some((batch, load)) = batch {
            host = host.batched(batch, load);
            client = client.batched(batch, load);
        }
        let mut air = Air::new(2);
        air.run_stations(&mut [&mut host, &mut client], WAKE_US);
        let Unit { chip, software, .. } = &mut host;
        if let Software::Host(software) = software {
            software.start(chip);
        }
        client.chip.write16(W_PREAMBLE, 6);
        (air, host, client)
    }

    #[test]
    fn stations_a_scanline_apart_put_the_same_frames_on_the_air_as_lockstep() {
        let (mut air, mut host_chip, mut client_chip, mut host, mut client) =
            session(100, gbatek_cmd_count());
        air.set_logging(true);
        run(
            &mut air,
            &mut host_chip,
            &mut client_chip,
            &mut host,
            &mut client,
            2_000_000,
        );

        let (mut stations, mut host_unit, mut client_unit) = units(100, None, None);
        stations.set_logging(true);
        stations.run_stations(&mut [&mut host_unit, &mut client_unit], WAKE_US + 2_000_000);

        assert_eq!(host_unit.host().exchanges, 100);
        assert_eq!(host_unit.host().statuses, host.statuses);
        assert_eq!(client_unit.client().cmds, client.cmds);
        assert_eq!(stations.log().len(), air.log().len());
        assert_eq!(stations.log(), air.log());
    }

    #[test]
    fn stations_stepping_in_cycle_batches_meet_every_reply_window() {
        let (mut air, mut host, mut client) = units(40, Some(16_715), Some((16, 0)));
        air.set_logging(true);
        air.run_stations(&mut [&mut host, &mut client], 1_000_000);
        assert_eq!(host.host().exchanges, 40);
        assert!(
            host.host()
                .statuses
                .iter()
                .all(|&status| status == (0x0001, 0))
        );
        assert_eq!(client.client().cmds, (0..40).collect::<Vec<u64>>());
        let starts: Vec<u64> = cmds(air.log())
            .iter()
            .map(|sent| sent.frame.start)
            .collect();
        assert!(
            starts[1..]
                .windows(2)
                .all(|pair| pair[1] - pair[0] >= 16_000)
        );
    }

    #[test]
    fn the_lead_is_a_scanline_while_quiet_and_a_microsecond_while_frames_fly() {
        let (_, mut host, mut client) = units(1, None, None);
        assert_eq!(Air::lead_us(&[&mut host, &mut client]), SCANLINE_LEAD_US);
        let probe = header(0x0040, [0xFF; 6], host.chip.mac(), [0xFF; 6]);
        send_loc1(&mut host.chip, &probe);
        host.run_until(host.us + 1);
        assert_eq!(Air::lead_us(&[&mut host, &mut client]), BURST_LEAD_US);
        let frames = host.take_frames();
        assert_eq!(frames.len(), 1);
        client.receive(frames[0].clone());
        host.run_until(host.us + 1_000);
        assert_eq!(Air::lead_us(&[&mut host]), SCANLINE_LEAD_US);
        assert_eq!(Air::lead_us(&[&mut client]), BURST_LEAD_US);
    }

    #[test]
    fn a_frame_that_reaches_a_chip_inside_its_preamble_still_arrives() {
        for (late, arrives) in [(0, true), (SCANLINE_LEAD_US, true), (95, true), (96, false)] {
            let mut sender = boot(1);
            let mut receiver = boot(2);
            receiver.write16(W_RXFILTER, 0x0FFF);
            let mut beacon = header(0x0080, [0xFF; 6], sender.mac(), sender.mac());
            beacon.extend_from_slice(&[0; 12]);
            sender.write16(W_PREAMBLE, 6);
            sender.step_us(WAKE_US);
            receiver.step_us(WAKE_US);
            send_loc1(&mut sender, &beacon);
            sender.step_us(1);
            let frames = sender.take_frames_at(10_000 + sender.now_us());
            assert_eq!(frames[0].preamble_us(), 96);
            receiver.step_us(late);
            receiver.receive_at(10_000 + receiver.now_us(), frames[0].clone());
            receiver.step_us(2_000);
            assert_eq!(
                drain(&mut receiver).len(),
                usize::from(arrives),
                "late by {late}"
            );
        }
    }

    #[test]
    #[ignore = "bench, run with --release --ignored --nocapture"]
    fn bench_two_stations_with_and_without_traffic() {
        const SECONDS: u64 = 10;
        const BATCH: u32 = 16;
        const FRAME_US: u64 = 16_715;
        let end = SECONDS * 1_000_000;
        let timed = |load: u32, traffic: Option<Option<u64>>| {
            let started = std::time::Instant::now();
            match traffic {
                None => {
                    let mut air = Air::new(2);
                    let mut a = Unit::new(boot(1), Software::Idle).batched(BATCH, load);
                    let mut b = Unit::new(boot(2), Software::Idle).batched(BATCH, load);
                    air.run_stations(&mut [&mut a, &mut b], end);
                }
                Some(period) => {
                    let (mut air, mut a, mut b) = units(u32::MAX, period, Some((BATCH, load)));
                    air.run_stations(&mut [&mut a, &mut b], end);
                    assert!(a.host().exchanges > 0);
                    assert!(
                        a.host()
                            .statuses
                            .iter()
                            .all(|&status| status == (0x0001, 0))
                    );
                }
            }
            started.elapsed().as_secs_f64() * 1_000.0 / SECONDS as f64
        };
        let alone = |load: u32| {
            let started = std::time::Instant::now();
            for last in [1, 2] {
                Unit::new(boot(last), Software::Idle)
                    .batched(BATCH, load)
                    .run_until(end);
            }
            started.elapsed().as_secs_f64() * 1_000.0 / SECONDS as f64
        };
        let probe = 64;
        let per_load = alone(probe) / f64::from(probe);
        let load = (720.0 / per_load) as u32;
        for load in [0, load] {
            let base = alone(load);
            println!("load {load}: alone {base:.1} ms per emulated second for two");
            for (name, traffic) in [
                ("quiet", None),
                ("one exchange a frame", Some(Some(FRAME_US))),
                ("exchanges back to back", Some(None)),
            ] {
                let took = timed(load, traffic);
                println!(
                    "load {load}: {name} {took:.1} ms, overhead {:.1} ms ({:+.1}%)",
                    took - base,
                    (took - base) / base * 100.0
                );
            }
        }
    }
}
