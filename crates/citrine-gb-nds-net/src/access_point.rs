use std::collections::VecDeque;

use citrine_gb::nds::wifi::air::Station;
use citrine_gb::nds::wifi::frame::{self, FCS_BYTES, Frame, HEADER_BYTES, Kind, Rate};

use crate::gateway::Gateway;

pub const SSID: &[u8] = b"citrine";
pub const CHANNEL: u8 = 6;
pub const BSSID: [u8; 6] = [0x02, 0x09, 0xBF, 0xC1, 0x7B, 0x01];

const BROADCAST: [u8; 6] = [0xFF; 6];
const BEACON_INTERVAL_TU: u16 = 100;
const TU_US: u64 = 1024;
const ANSWER_GAP_US: u64 = 50;
const POLL_US: u64 = 1_000;
const CAPABILITY: u16 = 0x0021;
const RATES: [u8; 2] = [0x82, 0x84];
const SNAP: [u8; 6] = [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00];
const TO_DS: u16 = 0x0100;
const FROM_DS: u16 = 0x0200;

const ASSOC_REQUEST: u8 = 0;
const ASSOC_RESPONSE: u8 = 1;
const PROBE_REQUEST: u8 = 4;
const PROBE_RESPONSE: u8 = 5;
const BEACON: u8 = 8;
const DISASSOC: u8 = 10;
const AUTH: u8 = 11;
const DEAUTH: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
    Authenticated,
    Associated(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Member {
    mac: [u8; 6],
    membership: Membership,
}

pub struct AccessPoint {
    now: u64,
    next_beacon: u64,
    next_poll: u64,
    busy_until: u64,
    sequence: u16,
    members: Vec<Member>,
    heard: Vec<Frame>,
    queue: VecDeque<(u64, Vec<u8>)>,
    outbox: Vec<Frame>,
    gateway: Gateway,
}

impl AccessPoint {
    pub fn new(gateway: Gateway) -> Self {
        Self {
            now: 0,
            next_beacon: 0,
            next_poll: 0,
            busy_until: 0,
            sequence: 0,
            members: Vec::new(),
            heard: Vec::new(),
            queue: VecDeque::new(),
            outbox: Vec::new(),
            gateway,
        }
    }

    pub fn membership(&self, mac: [u8; 6]) -> Option<Membership> {
        self.member(mac).map(|member| member.membership)
    }

    pub fn gateway(&self) -> &Gateway {
        &self.gateway
    }

    pub fn gateway_mut(&mut self) -> &mut Gateway {
        &mut self.gateway
    }

    fn member(&self, mac: [u8; 6]) -> Option<&Member> {
        self.members.iter().find(|member| member.mac == mac)
    }

    fn next_event(&self) -> Option<u64> {
        let heard = self.heard.iter().map(Frame::end).min();
        let queued = self
            .queue
            .front()
            .map(|&(at, _)| self.clear_from(at.max(self.busy_until)));
        [Some(self.next_beacon), heard, queued]
            .into_iter()
            .flatten()
            .min()
    }

    fn handle_heard(&mut self) {
        let now = self.now;
        let (done, waiting): (Vec<Frame>, Vec<Frame>) =
            self.heard.drain(..).partition(|frame| frame.end() <= now);
        self.heard = waiting;
        for frame in done {
            self.hear(&frame);
        }
    }

    fn hear(&mut self, frame: &Frame) {
        if frame.channel != CHANNEL || !frame.fcs_ok() || frame.bytes.len() < HEADER_BYTES {
            return;
        }
        let to = frame.address(1);
        if to != BSSID && to != BROADCAST {
            return;
        }
        let from = frame.address(2);
        let body = &frame.bytes[HEADER_BYTES..frame.bytes.len() - FCS_BYTES];
        let answer_at = frame.end() + ANSWER_GAP_US;
        match frame.kind() {
            Kind::Management(PROBE_REQUEST) if asks_for_us(body) => {
                let answer = self.probe_response(from);
                self.queue.push_back((answer_at, answer));
            }
            Kind::Management(AUTH) if frame.address(3) == BSSID => {
                if frame::half(body, 0) == 0 && frame::half(body, 2) == 1 {
                    self.forget(from);
                    self.members.push(Member {
                        mac: from,
                        membership: Membership::Authenticated,
                    });
                    let mut answer = self.header(AUTH, 0, from, BSSID);
                    answer.extend_from_slice(&[0, 0, 2, 0, 0, 0]);
                    self.queue.push_back((answer_at, answer));
                }
            }
            Kind::Management(ASSOC_REQUEST) if frame.address(3) == BSSID => {
                if self.member(from).is_none() {
                    return;
                }
                let aid = self.free_aid();
                if let Some(member) = self.members.iter_mut().find(|member| member.mac == from) {
                    member.membership = Membership::Associated(aid);
                }
                let mut answer = self.header(ASSOC_RESPONSE, 0, from, BSSID);
                answer.extend_from_slice(&CAPABILITY.to_le_bytes());
                answer.extend_from_slice(&[0, 0]);
                answer.extend_from_slice(&(0xC000 | aid).to_le_bytes());
                answer.extend_from_slice(&rates());
                self.queue.push_back((answer_at, answer));
            }
            Kind::Management(DISASSOC) | Kind::Management(DEAUTH) => self.forget(from),
            Kind::Data(0) if frame.control() & (TO_DS | FROM_DS) == TO_DS => {
                let associated = matches!(self.membership(from), Some(Membership::Associated(_)));
                if associated && body.len() >= SNAP.len() + 2 && body[..SNAP.len()] == SNAP {
                    let to = frame.address(3);
                    let mut ethernet = to.to_vec();
                    ethernet.extend_from_slice(&from);
                    ethernet.extend_from_slice(&body[SNAP.len()..]);
                    let group = to[0] & 1 != 0;
                    if (group && self.associated_besides(from)) || self.is_associated(to) {
                        self.deliver(&ethernet);
                    }
                    if group || !self.is_associated(to) {
                        self.gateway.send(ethernet);
                    }
                    self.next_poll = self.now;
                }
            }
            _ => {}
        }
    }

    fn clear_from(&self, mut at: u64) -> u64 {
        while let Some(end) = self
            .heard
            .iter()
            .filter(|frame| frame.start <= at && frame.end() > at)
            .map(Frame::end)
            .max()
        {
            at = end;
        }
        at
    }

    fn hearing(&self) -> bool {
        self.heard.iter().any(|frame| frame.start <= self.now)
    }

    fn is_associated(&self, mac: [u8; 6]) -> bool {
        matches!(self.membership(mac), Some(Membership::Associated(_)))
    }

    fn associated_besides(&self, mac: [u8; 6]) -> bool {
        self.members.iter().any(|member| {
            member.mac != mac && matches!(member.membership, Membership::Associated(_))
        })
    }

    fn forget(&mut self, mac: [u8; 6]) {
        self.members.retain(|member| member.mac != mac);
    }

    fn free_aid(&self) -> u16 {
        (1..2008)
            .find(|aid| {
                !self
                    .members
                    .iter()
                    .any(|member| member.membership == Membership::Associated(*aid))
            })
            .unwrap_or(1)
    }

    fn header(&mut self, subtype: u8, flags: u16, to: [u8; 6], third: [u8; 6]) -> Vec<u8> {
        let control = (u16::from(subtype) << 4) | flags;
        let mut bytes = control.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&to);
        bytes.extend_from_slice(&BSSID);
        bytes.extend_from_slice(&third);
        bytes.extend_from_slice(&(self.sequence << 4).to_le_bytes());
        self.sequence = (self.sequence + 1) & 0x0FFF;
        bytes
    }

    fn announcement(&mut self, subtype: u8, to: [u8; 6]) -> Vec<u8> {
        let mut bytes = self.header(subtype, 0, to, BSSID);
        bytes.extend_from_slice(&self.now.to_le_bytes());
        bytes.extend_from_slice(&BEACON_INTERVAL_TU.to_le_bytes());
        bytes.extend_from_slice(&CAPABILITY.to_le_bytes());
        bytes.extend_from_slice(&[0, SSID.len() as u8]);
        bytes.extend_from_slice(SSID);
        bytes.extend_from_slice(&rates());
        bytes.extend_from_slice(&[3, 1, CHANNEL]);
        bytes
    }

    fn probe_response(&mut self, to: [u8; 6]) -> Vec<u8> {
        self.announcement(PROBE_RESPONSE, to)
    }

    fn beacon(&mut self) -> Vec<u8> {
        let mut bytes = self.announcement(BEACON, BROADCAST);
        bytes.extend_from_slice(&[5, 4, 0, 1, 0, 0]);
        bytes
    }

    fn deliver(&mut self, ethernet: &[u8]) {
        let Some(payload) = ethernet.get(12..) else {
            return;
        };
        let to: [u8; 6] = ethernet[..6].try_into().unwrap_or(BROADCAST);
        let from: [u8; 6] = ethernet[6..12].try_into().unwrap_or(BSSID);
        if to[0] & 1 == 0 && !self.is_associated(to) {
            return;
        }
        let mut bytes = self.header(0, FROM_DS | 0x0008, to, from);
        bytes.extend_from_slice(&SNAP);
        bytes.extend_from_slice(payload);
        self.queue.push_back((self.now, bytes));
    }

    fn transmit(&mut self, mut bytes: Vec<u8>) {
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        frame::seal(&mut bytes);
        let frame = Frame {
            start: self.now,
            channel: CHANNEL,
            rate: Rate::Mbit2,
            short_preamble: false,
            bytes,
        };
        self.busy_until = frame.end() + ANSWER_GAP_US;
        self.outbox.push(frame);
    }

    fn poll_gateway(&mut self) {
        if self.now < self.next_poll {
            return;
        }
        self.next_poll = self.now + POLL_US;
        self.gateway.poll(self.now);
        while let Some(ethernet) = self.gateway.take() {
            self.deliver(&ethernet);
        }
    }
}

fn rates() -> Vec<u8> {
    let mut bytes = vec![1, RATES.len() as u8];
    bytes.extend_from_slice(&RATES);
    bytes
}

fn asks_for_us(body: &[u8]) -> bool {
    match body {
        [0, 0, ..] => true,
        [0, length, rest @ ..] => rest.get(..usize::from(*length)) == Some(SSID),
        _ => false,
    }
}

impl Station for AccessPoint {
    fn now_us(&self) -> u64 {
        self.now
    }

    fn run_until(&mut self, us: u64) {
        while let Some(at) = self.next_event().filter(|&at| at <= us) {
            self.now = self.now.max(at);
            self.handle_heard();
            if self.now >= self.next_beacon {
                self.next_beacon = self.now + u64::from(BEACON_INTERVAL_TU) * TU_US;
                let beacon = self.beacon();
                self.queue.push_front((self.now, beacon));
            }
            if self.now >= self.busy_until
                && !self.hearing()
                && let Some(&(ready, _)) = self.queue.front()
                && ready <= self.now
                && let Some((_, bytes)) = self.queue.pop_front()
            {
                self.transmit(bytes);
            }
        }
        self.now = self.now.max(us);
        self.poll_gateway();
    }

    fn take_frames(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.outbox)
    }

    fn receive(&mut self, frame: Frame) {
        self.heard.push(frame);
    }

    fn on_air(&self) -> bool {
        !self.heard.is_empty() || !self.queue.is_empty() || self.now < self.busy_until
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::Config;

    fn station() -> AccessPoint {
        AccessPoint::new(Gateway::new(
            Config::default(),
            citrine_gb_threads::Threads::default().spawner(),
        ))
    }

    fn management(subtype: u8, from: [u8; 6], third: [u8; 6], body: &[u8]) -> Frame {
        let mut bytes = (u16::from(subtype) << 4).to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&BSSID);
        bytes.extend_from_slice(&from);
        bytes.extend_from_slice(&third);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        frame::seal(&mut bytes);
        Frame {
            start: 0,
            channel: CHANNEL,
            rate: Rate::Mbit2,
            short_preamble: false,
            bytes,
        }
    }

    fn exchange(ap: &mut AccessPoint, mut frame: Frame) -> Vec<Frame> {
        ap.run_until(ap.now_us() + 10);
        ap.take_frames();
        frame.start = ap.now_us();
        let end = frame.end();
        ap.receive(frame);
        ap.run_until(end + 5_000);
        ap.take_frames()
            .into_iter()
            .filter(|frame| frame.kind() != Kind::Management(BEACON))
            .collect()
    }

    const DS: [u8; 6] = [0x00, 0x09, 0xBF, 0x11, 0x22, 0x33];
    const OTHER_DS: [u8; 6] = [0x00, 0x09, 0xBF, 0x44, 0x55, 0x66];

    fn associate(ap: &mut AccessPoint, mac: [u8; 6]) {
        exchange(ap, management(AUTH, mac, BSSID, &[0, 0, 1, 0, 0, 0]));
        exchange(
            ap,
            management(ASSOC_REQUEST, mac, BSSID, &[0x21, 0, 0x0A, 0]),
        );
    }

    fn data(from: [u8; 6], to: [u8; 6], payload: &[u8]) -> Frame {
        let mut bytes = (TO_DS | 0x0008).to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&BSSID);
        bytes.extend_from_slice(&from);
        bytes.extend_from_slice(&to);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&SNAP);
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        frame::seal(&mut bytes);
        Frame {
            start: 0,
            channel: CHANNEL,
            rate: Rate::Mbit2,
            short_preamble: false,
            bytes,
        }
    }

    #[test]
    fn data_between_two_stations_goes_straight_from_one_to_the_other() {
        let mut ap = station();
        associate(&mut ap, DS);
        associate(&mut ap, OTHER_DS);
        let payload = [0x08, 0x00, 0x45, 0x00];
        let relayed = exchange(&mut ap, data(DS, OTHER_DS, &payload));
        assert_eq!(relayed.len(), 1);
        let frame = &relayed[0];
        assert_eq!(frame.control() & (TO_DS | FROM_DS), FROM_DS);
        assert_eq!(frame.address(1), OTHER_DS);
        assert_eq!(frame.address(3), DS);
        let body = &frame.bytes[HEADER_BYTES..frame.bytes.len() - FCS_BYTES];
        assert_eq!(body, [&SNAP[..], &payload].concat());
    }

    #[test]
    fn a_broadcast_reaches_the_other_stations_and_a_lone_station_hears_no_echo() {
        let arp = [0x08, 0x06, 0x00, 0x01];
        let mut ap = station();
        associate(&mut ap, DS);
        assert!(exchange(&mut ap, data(DS, BROADCAST, &arp)).is_empty());
        associate(&mut ap, OTHER_DS);
        let relayed = exchange(&mut ap, data(DS, BROADCAST, &arp));
        assert_eq!(relayed.len(), 1);
        assert_eq!(relayed[0].address(1), BROADCAST);
        assert_eq!(relayed[0].address(3), DS);
    }

    #[test]
    fn beacons_go_out_every_hundred_time_units_with_the_ssid_and_channel() {
        let mut ap = station();
        ap.run_until(350_000);
        let beacons = ap.take_frames();
        let starts: Vec<u64> = beacons.iter().map(|frame| frame.start).collect();
        assert_eq!(starts, [0, 102_400, 204_800, 307_200]);
        let beacon = &beacons[1];
        assert_eq!(beacon.kind(), Kind::Management(BEACON));
        assert!(beacon.fcs_ok());
        assert_eq!(beacon.address(1), BROADCAST);
        assert_eq!(beacon.address(3), BSSID);
        let body = &beacon.bytes[HEADER_BYTES..];
        assert_eq!(u64::from_le_bytes(body[..8].try_into().unwrap()), 102_400);
        assert_eq!(&body[12..14 + SSID.len()], b"\x00\x07citrine");
        assert!(body.windows(3).any(|window| window == [3, 1, CHANNEL]));
    }

    #[test]
    fn a_probe_for_any_network_or_ours_gets_an_answer_and_others_do_not() {
        let mut ap = station();
        let answers = exchange(&mut ap, management(PROBE_REQUEST, DS, BROADCAST, &[0, 0]));
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].kind(), Kind::Management(PROBE_RESPONSE));
        assert_eq!(answers[0].address(1), DS);
        let ours = [&[0, 7][..], SSID].concat();
        assert_eq!(
            exchange(&mut ap, management(PROBE_REQUEST, DS, BROADCAST, &ours)).len(),
            1
        );
        let other = management(PROBE_REQUEST, DS, BROADCAST, b"\x00\x05other");
        assert!(exchange(&mut ap, other).is_empty());
    }

    #[test]
    fn open_authentication_then_association_gives_aid_one() {
        let mut ap = station();
        let assoc = management(ASSOC_REQUEST, DS, BSSID, &[0x21, 0, 0x0A, 0]);
        assert!(exchange(&mut ap, assoc.clone()).is_empty());
        let answers = exchange(&mut ap, management(AUTH, DS, BSSID, &[0, 0, 1, 0, 0, 0]));
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].kind(), Kind::Management(AUTH));
        assert_eq!(
            &answers[0].bytes[HEADER_BYTES..HEADER_BYTES + 6],
            &[0, 0, 2, 0, 0, 0]
        );
        assert_eq!(ap.membership(DS), Some(Membership::Authenticated));
        let answers = exchange(&mut ap, assoc);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].kind(), Kind::Management(ASSOC_RESPONSE));
        assert_eq!(answers[0].body_half(2), 0);
        assert_eq!(answers[0].body_half(4), 0xC001);
        assert_eq!(ap.membership(DS), Some(Membership::Associated(1)));
        exchange(&mut ap, management(DEAUTH, DS, BSSID, &[3, 0]));
        assert_eq!(ap.membership(DS), None);
    }

    #[test]
    fn an_answer_waits_while_another_frame_is_heard_and_after_its_own_last_one() {
        let mut ap = station();
        ap.run_until(10);
        ap.take_frames();
        let mut probe = management(PROBE_REQUEST, DS, BROADCAST, &[0, 0]);
        probe.start = 20;
        let end = probe.end();
        ap.receive(probe.clone());
        probe.start = end;
        let second_end = probe.end();
        ap.receive(probe);
        ap.run_until(second_end + 10_000);
        let answers = ap.take_frames();
        assert_eq!(answers.len(), 2);
        assert_eq!(answers[0].start, second_end);
        assert_eq!(answers[1].start, answers[0].end() + ANSWER_GAP_US);
    }
}
