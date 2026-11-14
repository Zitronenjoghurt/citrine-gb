use crate::Walker;
use crate::eeprom::{self, Block};
use crate::info::{IDENTITY_SIZE, WalkerInfo};
use crate::lz;
use crate::packet::{self, Packet};
use crate::rng::Rng;
use crate::route::{self, LogKind};

pub const CYCLES_PER_SECOND: u64 = 33_513_982;
const BAUD: u64 = 115_200;
const BITS_PER_BYTE: u64 = 10;
const TICKS_PER_SECOND: u64 = 32_768;
pub const REPLY_DELAY: u64 = CYCLES_PER_SECOND / 1000;
pub const ADVERTISE_GAP: u64 = ticks(0xC80);
pub const ADVERTISEMENTS: u64 = 20;
pub const SESSION_TIMEOUT: u64 = ticks(0xC80);
const BACKOFF_UNIT: u64 = 0x60;
const BAD_PACKETS: u8 = 20;

const FROM_WALKER: u8 = 0x02;
const BLOCK_WRITE_SIZE: usize = 128;

const RAM_WRITE: u8 = 0x06;
const WRITE_ACK: u8 = 0x04;
const WRITE_ANY: u8 = 0x0A;
const READ: u8 = 0x0C;
const READ_REPLY: u8 = 0x0E;
const IDENTITY_REQUEST: u8 = 0x20;
const IDENTITY_REPLY: u8 = 0x22;
const PING: u8 = 0x24;
const PONG: u8 = 0x26;
const RESET_STEPS: u8 = 0x2A;
const RESET_KEEP: u8 = 0x2C;
const IDENTITY_SEND: [u8; 4] = [0x32, 0x40, 0x52, 0x60];
const NO_COMPLETE: [u8; 4] = [0x36, 0x44, 0x56, 0x64];
const WALK_START_INIT: u8 = 0x38;
const WALK_END: u8 = 0x4E;
const WALK_END_ACK: u8 = 0x50;
const WALK_START: u8 = 0x5A;
const CONNECT_COMPLETE: u8 = 0x66;
const CONNECT_COMPLETE_ACK: u8 = 0x68;
const NO_RX: [u8; 2] = [0x9C, 0x9E];
const DISCONNECT: u8 = 0xF4;
const SLAVE_ACK: u8 = 0xF8;
const ASSERT_MASTER: u8 = 0xFA;

const fn ticks(count: u64) -> u64 {
    count * CYCLES_PER_SECOND / TICKS_PER_SECOND
}

pub fn airtime(bytes: usize) -> u64 {
    (bytes as u64 * BITS_PER_BYTE * CYCLES_PER_SECOND).div_ceil(BAUD)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Link {
    Idle,
    Advertising,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gift {
    Map,
    Pokemon,
    Item,
    Route,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reset {
    Pairing,
    Steps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum End {
    Completed,
    Cancelled,
    NoPeer,
    TimedOut,
    Garbled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    Connected,
    WalkStarted,
    WalkEnded,
    Gift { gift: Gift, stamp: bool },
    Reset(Reset),
    Ended(End),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Advertising { due: Option<u64>, quiet: u64 },
    Connected { session: [u8; 4], last: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum After {
    StartFresh,
    Start,
    End,
    Complete,
    Reset(Reset),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Session {
    state: State,
    own: [u8; 4],
    peer: WalkerInfo,
    sent: u64,
    bad: u8,
    after: Option<After>,
    outbox: Option<(Vec<u8>, u64)>,
    events: Vec<Event>,
}

enum Outcome {
    Reply(Packet),
    ReplyAndClose(Packet, End),
    Close(End),
    Ignore,
}

impl Session {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Idle,
            own: [0; 4],
            peer: WalkerInfo::from_slice(&[]),
            sent: 0,
            bad: 0,
            after: None,
            outbox: None,
            events: Vec::new(),
        }
    }

    pub(crate) fn link(&self) -> Link {
        match self.state {
            State::Idle => Link::Idle,
            State::Advertising { .. } => Link::Advertising,
            State::Connected { .. } => Link::Connected,
        }
    }

    pub(crate) fn events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    fn advertise(&mut self, now: u64) {
        self.state = State::Advertising {
            due: Some(now),
            quiet: now,
        };
    }

    pub(crate) fn take(&mut self, now: u64, rng: &mut Rng) -> Option<Vec<u8>> {
        if let Some((_, ready)) = &self.outbox
            && *ready <= now
        {
            return self.outbox.take().map(|(wire, _)| wire);
        }
        let State::Advertising { due, quiet } = self.state else {
            return None;
        };
        match due {
            Some(at) if at <= now => {
                self.sent += 1;
                let quiet = at + airtime(1);
                self.state = State::Advertising { due: None, quiet };
                Some(packet::advertisement())
            }
            None if now >= quiet + ADVERTISE_GAP && self.sent < ADVERTISEMENTS => {
                let backoff = u64::from((rng.next() >> 5) & 0x0F) * ticks(BACKOFF_UNIT);
                self.state = State::Advertising {
                    due: Some(quiet + ADVERTISE_GAP + backoff),
                    quiet,
                };
                self.take(now, rng)
            }
            _ => None,
        }
    }

    fn post(&mut self, packet: Packet, arrived: u64) {
        let wire = packet.encode();
        let ready = arrived + REPLY_DELAY + airtime(wire.len());
        self.outbox = Some((wire, ready));
    }
}

impl Walker {
    pub fn connect(&mut self, now: u64) {
        let session = &mut self.session;
        session.own = self.rng.state().to_be_bytes();
        session.outbox = None;
        session.after = None;
        session.sent = 0;
        session.bad = 0;
        session.advertise(now);
        self.connect_screen_started();
    }

    pub fn send(&mut self, wire: &[u8], at: u64) {
        let arrived = at + airtime(wire.len());
        self.expire_session(arrived);
        let Ok(request) = Packet::decode(wire) else {
            if let State::Connected { .. } = self.session.state {
                self.session.bad += 1;
                if self.session.bad >= BAD_PACKETS {
                    self.close(End::Garbled);
                }
            }
            return;
        };
        match self.session.state {
            State::Idle => {}
            State::Advertising { .. } => {
                if request.cmd != ASSERT_MASTER {
                    return;
                }
                if !matches!(request.extra, 1 | 2) {
                    self.close(End::Cancelled);
                    return;
                }
                let own = self.session.own;
                let reply = Packet::new(SLAVE_ACK, FROM_WALKER, own, Vec::new());
                let session = std::array::from_fn(|i| own[i] ^ request.session[i]);
                self.session.state = State::Connected {
                    session,
                    last: arrived,
                };
                self.session.events.push(Event::Connected);
                self.session.post(reply, arrived);
            }
            State::Connected { session, .. } => {
                if request.cmd == ASSERT_MASTER {
                    self.session.outbox = None;
                    self.session.advertise(arrived);
                    return;
                }
                if request.cmd >= SLAVE_ACK || request.session != session {
                    return;
                }
                self.session.state = State::Connected {
                    session,
                    last: arrived,
                };
                match self.handle(&request, session) {
                    Outcome::Reply(packet) => self.session.post(packet, arrived),
                    Outcome::ReplyAndClose(packet, end) => {
                        self.session.post(packet, arrived);
                        self.close(end);
                    }
                    Outcome::Close(end) => self.close(end),
                    Outcome::Ignore => {}
                }
            }
        }
    }

    pub(crate) fn expire_session(&mut self, now: u64) {
        match self.session.state {
            State::Advertising { due: None, quiet }
                if self.session.sent >= ADVERTISEMENTS && now >= quiet + ADVERTISE_GAP =>
            {
                self.close(End::NoPeer)
            }
            State::Connected { last, .. } if now >= last + SESSION_TIMEOUT => {
                self.close(End::TimedOut)
            }
            _ => {}
        }
    }

    fn handle(&mut self, request: &Packet, session: [u8; 4]) -> Outcome {
        let cmd = request.cmd;
        let payload = &request.payload;
        let reply = |cmd: u8, payload: Vec<u8>| Packet::new(cmd, FROM_WALKER, session, payload);
        let echo = |cmd: u8| Packet::new(cmd, request.extra, session, Vec::new());
        match cmd {
            0x00 | 0x80 => {
                let address = ((request.extra as usize) << 8) | (cmd & 0x80) as usize;
                let block = if payload.len() == BLOCK_WRITE_SIZE {
                    Some(payload.to_vec())
                } else {
                    payload
                        .get(4..)
                        .and_then(|stream| lz::unpack(stream, BLOCK_WRITE_SIZE))
                };
                if let Some(block) = block {
                    self.eeprom.write(address, &block);
                }
                Outcome::Reply(echo(WRITE_ACK))
            }
            0x02 | 0x82 => {
                let address = ((request.extra as usize) << 8) | (cmd & 0x80) as usize;
                self.eeprom.write(address, payload);
                Outcome::Reply(echo(WRITE_ACK))
            }
            WRITE_ANY => {
                if let Some((low, data)) = payload.split_first() {
                    self.eeprom
                        .write(((request.extra as usize) << 8) + *low as usize, data);
                }
                Outcome::Reply(echo(WRITE_ACK))
            }
            RAM_WRITE => Outcome::Reply(echo(RAM_WRITE)),
            READ => {
                let [high, low, len, ..] = payload[..] else {
                    return Outcome::Ignore;
                };
                let address = u16::from_be_bytes([high, low]) as usize;
                Outcome::Reply(reply(READ_REPLY, self.eeprom.read(address, len as usize)))
            }
            IDENTITY_REQUEST => Outcome::Reply(reply(IDENTITY_REPLY, self.identity_reply())),
            PING => Outcome::Reply(reply(PONG, Vec::new())),
            _ if IDENTITY_SEND.contains(&cmd) => {
                self.take_peer(payload, cmd != 0x60);
                Outcome::Reply(reply(cmd + 2, Vec::new()))
            }
            _ if NO_COMPLETE.contains(&cmd) => Outcome::Close(End::Cancelled),
            WALK_START_INIT => {
                self.session.after = Some(After::StartFresh);
                Outcome::Reply(reply(cmd, Vec::new()))
            }
            WALK_START => {
                self.session.after = Some(After::Start);
                Outcome::ReplyAndClose(reply(cmd, Vec::new()), End::Completed)
            }
            WALK_END => {
                self.session.after = Some(After::End);
                Outcome::ReplyAndClose(reply(WALK_END_ACK, Vec::new()), End::Completed)
            }
            CONNECT_COMPLETE => {
                self.session.after = Some(After::Complete);
                Outcome::Reply(reply(CONNECT_COMPLETE_ACK, Vec::new()))
            }
            _ if NO_RX.contains(&cmd) => {
                Outcome::ReplyAndClose(reply(cmd, Vec::new()), End::Cancelled)
            }
            DISCONNECT => Outcome::Close(End::Completed),
            RESET_KEEP | RESET_STEPS => {
                self.session.after = Some(After::Reset(match cmd {
                    RESET_KEEP => Reset::Pairing,
                    _ => Reset::Steps,
                }));
                let identity = self
                    .eeprom
                    .read_reliable(Block::UniqueIdentity)
                    .unwrap_or_else(|| vec![0; IDENTITY_SIZE]);
                Outcome::Reply(reply(RESET_STEPS, identity))
            }
            0xC0 | 0xC2 | 0xC4 | 0xC6 | 0xD0 | 0xD2 | 0xD4 | 0xD6 => {
                self.gift(cmd);
                Outcome::ReplyAndClose(reply(cmd & !0x10, Vec::new()), End::Completed)
            }
            _ => Outcome::Ignore,
        }
    }

    fn close(&mut self, end: End) {
        self.session.state = State::Idle;
        match self.session.after.take() {
            Some(After::StartFresh) => {
                self.ram.today_steps = 0;
                self.start_walk();
            }
            Some(After::Start) => {
                self.start_walk();
                self.eeprom.fill(eeprom::RECEIVED, eeprom::RECEIVED_SIZE, 0);
            }
            Some(After::End) => self.end_walk(),
            Some(After::Complete) => {
                let mut health = self.health();
                health.set_watts(0);
                self.set_health(&health);
                self.save_health();
                self.eeprom.fill(eeprom::CAUGHT, eeprom::CAUGHT_SIZE, 0);
            }
            Some(After::Reset(kind)) => self.reset(kind),
            None => {}
        }
        self.release_clock();
        self.session.events.push(Event::Ended(end));
    }

    fn identity_reply(&mut self) -> Vec<u8> {
        let mut info = self.stored_info();
        let health = self.health();
        info.set_step_count(health.total_steps());
        self.save_health();
        self.eeprom
            .write(eeprom::WATTS, &health.watts().to_be_bytes());
        info.bytes().to_vec()
    }

    fn take_peer(&mut self, payload: &[u8], save: bool) {
        let peer = WalkerInfo::from_slice(payload);
        let hour = peer.day_end_hour();
        if hour < 24 {
            self.ram.rollover_hour = hour;
        }
        if peer.last_sync() != 0 {
            let mut health = self.health();
            health.set_clock(peer.last_sync());
            self.set_health(&health);
            self.clock.restart();
        }
        self.session.peer = peer;
        if save {
            self.save_health();
        }
    }

    fn gift(&mut self, cmd: u8) {
        let kind = (cmd & 0x06) >> 1;
        let mut received = self.eeprom.read(eeprom::RECEIVED, 1)[0];
        received |= if cmd & 0x10 != 0 {
            [0x1F, 0x2F, 0x4F, 0x8F][kind as usize]
        } else {
            0x10 << kind
        };
        self.eeprom.write(eeprom::RECEIVED, &[received]);
        let event = self.session.peer.event_index();
        if event != 0 {
            let mut info = self.stored_info();
            info.mark_event(event);
            self.eeprom.write_reliable(Block::Info, info.bytes());
        }
        if kind == 3 {
            let mut health = self.health();
            health.set_special_route(true);
            self.set_health(&health);
            self.save_health();
        }
        self.session.events.push(Event::Gift {
            gift: [Gift::Map, Gift::Pokemon, Gift::Item, Gift::Route][kind as usize],
            stamp: cmd & 0x10 != 0,
        });
    }

    fn start_walk(&mut self) {
        let eeprom = &mut self.eeprom;
        eeprom.write_reliable(Block::CopyMarker, &[0xA5]);
        eeprom.copy_within(
            eeprom::ROUTE_STAGING,
            eeprom::ROUTE,
            eeprom::ROUTE_UPLOAD_SIZE,
        );
        eeprom.copy_within(eeprom::TEAM_STAGING, eeprom::TEAM, eeprom::TEAM_UPLOAD_SIZE);
        eeprom.write_reliable(Block::CopyMarker, &[0x00]);
        route::clear_log_kinds(eeprom);
        eeprom.fill(eeprom::PEERS, eeprom::PEERS_SIZE, 0);
        self.ram.hour_steps = 0;
        let mut health = self.health();
        health.set_walk_minutes(0);
        health.set_event_log_index(0);
        health.set_watts(0);
        self.set_health(&health);
        self.save_health();
        self.ram.walking = true;
        let mut info = self.stored_info();
        info.start_walk_from(&self.session.peer.clone());
        self.eeprom.write_reliable(Block::Info, info.bytes());
        self.log(LogKind::WalkStart, 0, 0, None);
        self.eeprom.fill(eeprom::CAUGHT, eeprom::CAUGHT_SIZE, 0);
        self.walk_steps = 0;
        self.home = None;
        self.offer = 0;
        self.session.events.push(Event::WalkStarted);
    }

    fn end_walk(&mut self) {
        self.ram.walking = false;
        let mut health = self.health();
        health.set_special_route(false);
        health.set_event_log_index(0);
        health.set_watts(0);
        self.set_health(&health);
        self.save_health();
        let mut info = self.stored_info();
        self.session.peer = info.clone();
        info.end_walk();
        self.eeprom.write_reliable(Block::Info, info.bytes());
        let eeprom = &mut self.eeprom;
        eeprom.fill(eeprom::CAUGHT, eeprom::CAUGHT_SIZE, 0);
        route::clear_log_kinds(eeprom);
        eeprom.fill(eeprom::RECEIVED, eeprom::RECEIVED_SIZE, 0);
        eeprom.fill(eeprom::PEERS, eeprom::PEERS_SIZE, 0);
        eeprom.fill(eeprom::ROUTE, 0x10, 0);
        self.walk_steps = 0;
        self.home = None;
        self.offer = 0;
        self.session.events.push(Event::WalkEnded);
    }

    fn reset(&mut self, kind: Reset) {
        self.ram.today_steps = 0;
        self.ram.hour_steps = 0;
        self.ram.event_seconds = self.rules.home_event_seconds;
        self.ram.walking = false;
        let identity = self
            .eeprom
            .read(Block::UniqueIdentity.address(), IDENTITY_SIZE);
        let mut info = self.stored_info();
        info.reset(&identity, kind == Reset::Steps);
        self.eeprom.write_reliable(Block::Info, info.bytes());
        let mut health = self.health();
        health.reset(kind == Reset::Steps);
        self.set_health(&health);
        self.save_health();
        let eeprom = &mut self.eeprom;
        match kind {
            Reset::Steps => eeprom.fill(eeprom::RESULTS, eeprom::RESULTS_SIZE, 0),
            Reset::Pairing => {
                eeprom.fill(eeprom::CAUGHT, eeprom::CAUGHT_SIZE, 0);
                route::clear_log_kinds(eeprom);
                eeprom.fill(route::HISTORY, 0x1C, 0);
            }
        }
        eeprom.fill(eeprom::PEERS, eeprom::PEERS_SIZE, 0);
        self.session.events.push(Event::Reset(kind));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(cmd: u8, extra: u8, session: [u8; 4], payload: &[u8]) -> Vec<u8> {
        Packet::new(cmd, extra, session, payload.to_vec()).encode()
    }

    const MASTER: [u8; 4] = [0x11, 0x22, 0x33, 0x44];
    const ASSERT_MASTER_WIRE: [u8; 8] = [0x50, 0xAB, 0xC0, 0x94, 0xBB, 0x88, 0x99, 0xEE];
    const PING_WIRE: [u8; 8] = [0x8E, 0xAB, 0x94, 0x2B, 0x65, 0x25, 0x27, 0x01];
    const PONG_WIRE: [u8; 8] = [0x8C, 0xA8, 0x95, 0x29, 0x65, 0x25, 0x27, 0x01];
    const OWN: u32 = 0xDEAD_BEEF;
    const JOINED: [u8; 4] = [0xCF, 0x8F, 0x8D, 0xAB];

    fn exchange(walker: &mut Walker, wire: &[u8], at: u64) -> Option<Vec<u8>> {
        walker.send(wire, at);
        walker.take(at + airtime(wire.len()) + REPLY_DELAY + airtime(136))
    }

    fn connected() -> (Walker, u64) {
        let mut walker = Walker::new(1);
        walker.set_rng_state(OWN as u64);
        walker.connect(0);
        assert_eq!(walker.take(0), Some(vec![0x56]));
        let at = CYCLES_PER_SECOND / 100;
        let ack = exchange(&mut walker, &ASSERT_MASTER_WIRE, at);
        assert_eq!(ack, Some(reference(0xF8, 0x02, OWN.to_be_bytes(), &[])));
        walker.events();
        (walker, at + CYCLES_PER_SECOND / 100)
    }

    #[test]
    fn the_documented_handshake_packets_match_the_reference_encoder() {
        assert_eq!(reference(0xFA, 0x01, MASTER, &[]), ASSERT_MASTER_WIRE);
        assert_eq!(reference(0x24, 0x01, JOINED, &[]), PING_WIRE);
        assert_eq!(reference(0x26, 0x02, JOINED, &[]), PONG_WIRE);
    }

    #[test]
    fn the_walker_advertises_at_once_then_after_each_quiet_gap_twenty_times() {
        let mut walker = Walker::new(1);
        walker.connect(1_000);
        assert_eq!(walker.take(1_000), Some(vec![0x56]));
        let mut at = 1_000;
        let mut gaps = Vec::new();
        let mut last = 1_000;
        while at < 4 * CYCLES_PER_SECOND {
            at += CYCLES_PER_SECOND / 10_000;
            if walker.take(at).is_some() {
                gaps.push(at - last);
                last = at;
            }
        }
        assert_eq!(gaps.len() as u64, ADVERTISEMENTS - 1);
        for gap in gaps {
            assert!(
                (ADVERTISE_GAP..ADVERTISE_GAP + ticks(16 * BACKOFF_UNIT) + airtime(1) + 4_000)
                    .contains(&gap)
            );
        }
        assert_eq!(walker.link(), Link::Idle);
        assert_eq!(walker.events(), vec![Event::Ended(End::NoPeer)]);
    }

    #[test]
    fn the_handshake_answers_with_the_generator_state_as_its_id() {
        let (mut walker, at) = connected();
        assert_eq!(walker.link(), Link::Connected);
        assert_eq!(
            exchange(&mut walker, &PING_WIRE, at),
            Some(PONG_WIRE.to_vec())
        );
    }

    #[test]
    fn a_master_with_a_bad_extra_is_dropped() {
        let mut walker = Walker::new(1);
        walker.connect(0);
        walker.take(0);
        let wire = reference(0xFA, 0x03, MASTER, &[]);
        assert_eq!(exchange(&mut walker, &wire, 1_000), None);
        assert_eq!(walker.link(), Link::Idle);
    }

    #[test]
    fn packets_with_a_bad_sum_or_another_session_go_unanswered() {
        let (mut walker, at) = connected();
        let mut bad = PING_WIRE;
        bad[0] ^= 0x01;
        assert_eq!(exchange(&mut walker, &bad, at), None);
        let stranger = reference(0x24, 0x01, MASTER, &[]);
        assert_eq!(exchange(&mut walker, &stranger, at), None);
        assert_eq!(walker.link(), Link::Connected);
    }

    #[test]
    fn writes_echo_the_requests_extra_and_land_where_it_says() {
        let (mut walker, at) = connected();
        let block: Vec<u8> = (0..128).collect();
        let request = reference(0x82, 0xD7, JOINED, &block);
        assert_eq!(
            exchange(&mut walker, &request, at),
            Some(reference(0x04, 0xD7, JOINED, &[]))
        );
        assert_eq!(walker.eeprom.read(0xD780, 128), block);
        let short = reference(0x02, 0xD8, JOINED, &[9, 9]);
        walker.eeprom.fill(0xD800, 4, 7);
        exchange(&mut walker, &short, at + CYCLES_PER_SECOND / 100);
        assert_eq!(walker.eeprom.read(0xD800, 4), vec![9, 9, 7, 7]);
    }

    #[test]
    fn compressed_writes_unpack_to_one_block() {
        let (mut walker, at) = connected();
        let block: Vec<u8> = (0..128u32).map(|i| (i / 16) as u8).collect();
        let mut payload = vec![0x10, 0x80, 0x00, 0x00];
        payload.extend(lz::pack(&block));
        let request = reference(0x00, 0xD7, JOINED, &payload);
        assert_eq!(
            exchange(&mut walker, &request, at),
            Some(reference(0x04, 0xD7, JOINED, &[]))
        );
        assert_eq!(walker.eeprom.read(0xD700, 128), block);
    }

    #[test]
    fn identity_sends_are_acked_two_higher_and_set_the_clock() {
        for cmd in IDENTITY_SEND {
            let (mut walker, at) = connected();
            let mut info = [0; 0x68];
            info[0x5B] = 6 << 3;
            info[0x60..0x64].copy_from_slice(&0x2000_0000u32.to_be_bytes());
            let request = reference(cmd, 0x01, JOINED, &info);
            assert_eq!(
                exchange(&mut walker, &request, at),
                Some(reference(cmd + 2, 0x02, JOINED, &[]))
            );
            assert_eq!(walker.clock(), 0x2000_0000);
            assert_eq!(walker.ram.rollover_hour, 6);
        }
    }

    #[test]
    fn connect_complete_is_acked_and_clears_the_watts_once_the_session_ends() {
        let (mut walker, at) = connected();
        walker.steps(400);
        let request = reference(0x66, 0x01, JOINED, &[]);
        assert_eq!(
            exchange(&mut walker, &request, at),
            Some(reference(0x68, 0x02, JOINED, &[]))
        );
        assert_eq!(walker.link(), Link::Connected);
        assert_eq!(walker.health().watts(), 20);
        walker.take(at + CYCLES_PER_SECOND);
        assert_eq!(walker.link(), Link::Idle);
        assert_eq!(walker.events(), vec![Event::Ended(End::TimedOut)]);
        assert_eq!(walker.health().watts(), 0);
    }

    #[test]
    fn disconnect_and_no_complete_end_without_a_reply() {
        for (cmd, end) in [
            (0xF4, End::Completed),
            (0x56, End::Cancelled),
            (0x36, End::Cancelled),
        ] {
            let (mut walker, at) = connected();
            assert_eq!(
                exchange(&mut walker, &reference(cmd, 0x01, JOINED, &[]), at),
                None
            );
            assert_eq!(walker.events(), vec![Event::Ended(end)]);
        }
    }

    #[test]
    fn an_unknown_command_goes_unanswered_and_the_session_stays() {
        let (mut walker, at) = connected();
        assert_eq!(
            exchange(&mut walker, &reference(0x7E, 0x01, JOINED, &[]), at),
            None
        );
        assert_eq!(walker.link(), Link::Connected);
    }

    #[test]
    fn gifts_mark_what_was_received_and_end_the_session() {
        let (mut walker, at) = connected();
        let request = reference(0xC2, 0x01, JOINED, &[]);
        assert_eq!(
            exchange(&mut walker, &request, at),
            Some(reference(0xC2, 0x02, JOINED, &[]))
        );
        assert_eq!(walker.eeprom.read(0xB800, 1), vec![0x20]);
        let (mut walker, at) = connected();
        let stamped = reference(0xD6, 0x01, JOINED, &[]);
        assert_eq!(
            exchange(&mut walker, &stamped, at),
            Some(reference(0xC6, 0x02, JOINED, &[]))
        );
        assert_eq!(walker.eeprom.read(0xB800, 1), vec![0x8F]);
        assert!(walker.health().special_route());
        assert_eq!(
            walker.events(),
            vec![
                Event::Gift {
                    gift: Gift::Route,
                    stamp: true
                },
                Event::Ended(End::Completed)
            ]
        );
    }

    #[test]
    fn a_reset_answers_with_the_unique_identity_and_unpairs_after_the_session() {
        let (mut walker, at) = connected();
        let identity: Vec<u8> = (1..=0x28).collect();
        walker
            .eeprom
            .write_reliable(Block::UniqueIdentity, &identity);
        let mut info = vec![0; 0x68];
        info[0x5B] = 0x03;
        walker.eeprom.write_reliable(Block::Info, &info);
        let request = reference(0x2C, 0x01, JOINED, &[]);
        assert_eq!(
            exchange(&mut walker, &request, at),
            Some(reference(0x2A, 0x02, JOINED, &identity))
        );
        assert_eq!(walker.stored_info().flags(), 0x03);
        exchange(
            &mut walker,
            &reference(0xF4, 0x01, JOINED, &[]),
            at + CYCLES_PER_SECOND / 100,
        );
        let info = walker.stored_info();
        assert_eq!(info.flags(), 0);
        assert_eq!(&info.bytes()[0x10..0x38], &identity[..]);
        assert_eq!(info.bytes()[0x5F], 2);
    }
}
