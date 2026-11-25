use super::PlayMsg;
use super::pieces::Pieces;
use super::wire::Wire;
use crate::crypto::{OpenError, PeerSealer, Sealed};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

pub const ACK_DELAY: Duration = Duration::from_millis(100);
pub const MIN_RESEND: Duration = Duration::from_secs(1);
pub const PROBE_EVERY: Duration = Duration::from_millis(250);
pub const DIRECT_SILENT: Duration = Duration::from_secs(1);
pub const DIRECT_UNACKED: Duration = Duration::from_millis(500);
pub const DIRECT_ECHOES: u32 = 3;
pub const DIRECT_SLOWER: Duration = Duration::from_millis(20);
pub const DIRECT_MAX_BYTES: usize = 16 * 1024;
pub const DIRECT_RETRIES: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
];
const DIRECT_STABLE: Duration = Duration::from_secs(60);
const MAX_RESEND: Duration = Duration::from_secs(8);
const RESEND_BYTES: usize = 256 * 1024;
const MAX_AHEAD: u64 = 1 << 14;
const MAX_HELD_BYTES: usize = 64 << 20;
const PROBES_OUT: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPath {
    Relay,
    Direct,
}

struct Sent {
    seq: u64,
    sealed: Sealed,
    sent_at: Instant,
    stamp: u64,
    tries: u32,
    path: LinkPath,
}

#[derive(Default)]
struct Probes {
    next: u32,
    out: VecDeque<(u32, Instant)>,
    due: Option<Instant>,
    rtt: Option<Duration>,
    echoes: u32,
}

impl Probes {
    fn due(&mut self, now: Instant) -> Option<u32> {
        if self.due.is_some_and(|at| now < at) {
            return None;
        }
        self.due = Some(now + PROBE_EVERY);
        self.next = self.next.wrapping_add(1);
        self.out.push_back((self.next, now));
        if self.out.len() > PROBES_OUT {
            self.out.pop_front();
        }
        Some(self.next)
    }

    fn echoed(&mut self, n: u32, now: Instant) {
        let Some(at) = self.out.iter().position(|(sent, _)| *sent == n) else {
            return;
        };
        let sample = now.duration_since(self.out[at].1);
        self.out.drain(..=at);
        self.rtt = Some(self.rtt.map_or(sample, |rtt| (rtt * 3 + sample) / 4));
        self.echoes += 1;
    }
}

struct Direct {
    probes: Probes,
    heard_at: Instant,
}

impl Direct {
    fn silent(&self, now: Instant) -> bool {
        now.duration_since(self.heard_at) >= DIRECT_SILENT
    }
}

pub struct PeerLink {
    sealer: Arc<PeerSealer>,
    id: u32,
    peer: u32,
    retired: Vec<u32>,
    next: u64,
    unacked: VecDeque<Sent>,
    upto: u64,
    highest: u64,
    held: BTreeMap<u64, Vec<u8>>,
    held_bytes: usize,
    pieces: Pieces,
    ready: VecDeque<PlayMsg>,
    ack_at: Option<Instant>,
    round_trip: Option<Duration>,
    acked_at: Option<Instant>,
    out: Out,
    last_piece: u32,
    path: LinkPath,
    direct: Option<Direct>,
    relay_probes: Probes,
    direct_since: Option<Instant>,
    retry_at: Option<Instant>,
    retries: usize,
    duplicates: u64,
}

impl PeerLink {
    pub fn new(sealer: Arc<PeerSealer>) -> Self {
        let mut id = [0; 4];
        getrandom::getrandom(&mut id).expect("no system random source");
        Self {
            sealer,
            id: u32::from_le_bytes(id) | 1,
            peer: 0,
            retired: Vec::new(),
            next: 0,
            unacked: VecDeque::new(),
            upto: 0,
            highest: 0,
            held: BTreeMap::new(),
            held_bytes: 0,
            pieces: Pieces::default(),
            ready: VecDeque::new(),
            ack_at: None,
            round_trip: None,
            acked_at: None,
            out: Out::default(),
            last_piece: 0,
            path: LinkPath::Relay,
            direct: None,
            relay_probes: Probes::default(),
            direct_since: None,
            retry_at: None,
            retries: 0,
            duplicates: 0,
        }
    }

    pub fn send(&mut self, msg: &PlayMsg, now: Instant) {
        for body in msg.bodies(&mut self.last_piece) {
            self.next += 1;
            let head = Wire::Seq {
                link: self.id,
                base: self.unacked.front().map_or(self.next, |sent| sent.seq),
                seq: self.next,
                peer: self.peer,
                upto: self.upto,
                highest: self.highest,
            };
            let sealed = self.seal(&head, &body);
            let (stamp, path) = self.out.send(self.path, sealed.clone());
            self.unacked.push_back(Sent {
                seq: self.next,
                sealed,
                sent_at: now,
                stamp,
                tries: 0,
                path,
            });
            self.ack_at = None;
        }
    }

    pub fn hear(&mut self, path: LinkPath, sealed: &Sealed, now: Instant) -> Result<(), OpenError> {
        let bytes = self.sealer.open(sealed)?;
        let (head, body) = postcard::take_from_bytes::<Wire>(&bytes).map_err(|_| OpenError)?;
        if let (LinkPath::Direct, Some(direct)) = (path, &mut self.direct) {
            direct.heard_at = now;
        }
        match head {
            Wire::Seq {
                link,
                base,
                seq,
                peer,
                upto,
                highest,
            } => {
                if !self.meet(link, base) {
                    return Ok(());
                }
                if peer == self.id {
                    self.acked(upto, highest, now);
                }
                self.arrive(seq, body, now);
            }
            Wire::Ack {
                peer,
                upto,
                highest,
            } if body.is_empty() => {
                if peer == self.id {
                    self.acked(upto, highest, now);
                }
            }
            Wire::Probe { link, n } if body.is_empty() => {
                let echo = self.seal(&Wire::Echo { peer: link, n }, &[]);
                self.out.frames.push((path, echo));
            }
            Wire::Echo { peer, n } if body.is_empty() => {
                if peer == self.id {
                    match (path, &mut self.direct) {
                        (LinkPath::Relay, _) => self.relay_probes.echoed(n, now),
                        (LinkPath::Direct, Some(direct)) => direct.probes.echoed(n, now),
                        (LinkPath::Direct, None) => {}
                    }
                }
            }
            join @ Wire::Join { .. } if body.is_empty() => self.ready.extend(join.into_msg()),
            _ => return Err(OpenError),
        }
        Ok(())
    }

    pub fn take(&mut self) -> Option<PlayMsg> {
        self.ready.pop_front()
    }

    pub fn poll(&mut self, now: Instant) -> Vec<(LinkPath, Sealed)> {
        self.choose_path(now);
        let wait = self.resend_after();
        for sent in &mut self.unacked {
            let after = (wait * 2u32.pow(sent.tries.min(3))).min(MAX_RESEND);
            let since = self
                .acked_at
                .map_or(sent.sent_at, |at| at.max(sent.sent_at));
            if now.duration_since(since) >= after && !self.out.resend(sent, self.path, now) {
                break;
            }
        }
        if self.ack_at.is_some_and(|at| now >= at) {
            let head = Wire::Ack {
                peer: self.peer,
                upto: self.upto,
                highest: self.highest,
            };
            let sealed = self.seal(&head, &[]);
            self.out.frames.push((self.path, sealed));
            self.ack_at = None;
        }
        std::mem::take(&mut self.out.frames)
    }

    pub fn receiving(&self) -> Option<(u32, u32)> {
        self.pieces.receiving()
    }

    pub fn path(&self) -> LinkPath {
        self.path
    }

    pub fn duplicates(&self) -> u64 {
        self.duplicates
    }

    pub fn direct_up(&mut self, now: Instant) {
        self.direct = Some(Direct {
            probes: Probes::default(),
            heard_at: now,
        });
    }

    pub fn direct_down(&mut self, now: Instant) {
        self.direct = None;
        match self.path {
            LinkPath::Direct => self.fall_back(now),
            LinkPath::Relay => {
                self.retry_at.get_or_insert(now + self.retry_wait());
            }
        }
    }

    pub fn retry_due(&mut self, now: Instant) -> bool {
        let alive = self.direct.as_ref().is_some_and(|d| !d.silent(now));
        if self.path == LinkPath::Direct || alive || self.retry_at.is_none_or(|at| now < at) {
            return false;
        }
        self.retries += 1;
        self.retry_at = Some(now + self.retry_wait());
        true
    }

    fn retry_wait(&self) -> Duration {
        DIRECT_RETRIES[self.retries.min(DIRECT_RETRIES.len() - 1)]
    }

    fn choose_path(&mut self, now: Instant) {
        let Some(direct) = &mut self.direct else {
            return;
        };
        if let Some(n) = direct.probes.due(now) {
            let probe = self.seal(&Wire::Probe { link: self.id, n }, &[]);
            self.out.frames.push((LinkPath::Direct, probe));
        }
        let Some(direct) = &self.direct else {
            return;
        };
        match self.path {
            LinkPath::Direct => {
                let stuck = self.unacked.iter().any(|sent| {
                    sent.path == LinkPath::Direct
                        && now.duration_since(sent.sent_at) >= DIRECT_UNACKED
                });
                if direct.silent(now) || stuck {
                    self.fall_back(now);
                } else if self
                    .direct_since
                    .is_some_and(|at| now.duration_since(at) >= DIRECT_STABLE)
                {
                    self.retries = 0;
                }
            }
            LinkPath::Relay => {
                let ready = direct.probes.echoes >= DIRECT_ECHOES
                    && !direct.silent(now)
                    && self.retry_at.is_none_or(|at| now >= at);
                let fast = match (direct.probes.rtt, self.relay_probes.rtt) {
                    (Some(direct), Some(relay)) => direct <= relay + DIRECT_SLOWER,
                    _ => true,
                };
                if ready && fast {
                    self.path = LinkPath::Direct;
                    self.direct_since = Some(now);
                    self.retry_at = None;
                } else if let Some(n) = self.relay_probes.due(now) {
                    let probe = self.seal(&Wire::Probe { link: self.id, n }, &[]);
                    self.out.frames.push((LinkPath::Relay, probe));
                }
            }
        }
    }

    fn fall_back(&mut self, now: Instant) {
        self.path = LinkPath::Relay;
        self.direct_since = None;
        self.retry_at = Some(now + self.retry_wait());
        if let Some(direct) = &mut self.direct {
            direct.probes.echoes = 0;
        }
        for sent in &mut self.unacked {
            if !self.out.resend(sent, LinkPath::Relay, now) {
                break;
            }
        }
    }

    fn seal(&self, head: &Wire, body: &[u8]) -> Sealed {
        let mut bytes = postcard::to_allocvec(head).expect("link heads always encode");
        bytes.extend_from_slice(body);
        self.sealer.seal(&bytes)
    }

    fn resend_after(&self) -> Duration {
        self.round_trip
            .map_or(MIN_RESEND, |rtt| (rtt * 4).max(MIN_RESEND))
    }

    fn meet(&mut self, link: u32, base: u64) -> bool {
        if link == self.peer {
            return true;
        }
        if self.retired.contains(&link) {
            return false;
        }
        if self.peer != 0 {
            self.retired.push(self.peer);
        }
        self.peer = link;
        self.upto = base.saturating_sub(1);
        self.highest = self.upto;
        self.held.clear();
        self.held_bytes = 0;
        self.pieces = Pieces::default();
        true
    }

    fn acked(&mut self, upto: u64, highest: u64, now: Instant) {
        while self.unacked.front().is_some_and(|sent| sent.seq <= upto) {
            let Some(sent) = self.unacked.pop_front() else {
                break;
            };
            self.acked_at = Some(now);
            if sent.tries == 0 {
                let sample = now.duration_since(sent.sent_at);
                self.round_trip = Some(match self.round_trip {
                    Some(rtt) => (rtt * 7 + sample) / 8,
                    None => sample,
                });
            }
        }
        let Some((mark, path)) = self
            .unacked
            .iter()
            .find(|sent| sent.seq == highest)
            .map(|sent| (sent.stamp, sent.path))
        else {
            return;
        };
        for sent in self
            .unacked
            .iter_mut()
            .take_while(|sent| sent.seq < highest)
        {
            if sent.stamp < mark && sent.path == path && !self.out.resend(sent, self.path, now) {
                break;
            }
        }
    }

    fn arrive(&mut self, seq: u64, body: &[u8], now: Instant) {
        if seq <= self.upto || self.held.contains_key(&seq) {
            self.duplicates += 1;
            self.ack_at = Some(now);
            return;
        }
        if seq > self.upto + MAX_AHEAD {
            self.ack_at = Some(now);
            return;
        }
        let in_order = seq == self.upto + 1;
        if !in_order && self.held_bytes + body.len() > MAX_HELD_BYTES {
            return;
        }
        self.highest = self.highest.max(seq);
        self.held_bytes += body.len();
        self.held.insert(seq, body.to_vec());
        while let Some(body) = self.held.remove(&(self.upto + 1)) {
            self.held_bytes -= body.len();
            self.upto += 1;
            if let Ok((wire, [])) = postcard::take_from_bytes::<Wire>(&body) {
                self.ready.extend(self.pieces.read(wire));
            }
        }
        let due = match in_order {
            true => now + ACK_DELAY,
            false => now,
        };
        self.ack_at = Some(self.ack_at.map_or(due, |at| at.min(due)));
    }
}

#[derive(Default)]
struct Out {
    frames: Vec<(LinkPath, Sealed)>,
    stamps: u64,
    budget: usize,
    budget_at: Option<Instant>,
}

impl Out {
    fn send(&mut self, path: LinkPath, sealed: Sealed) -> (u64, LinkPath) {
        let path = match sealed.ciphertext.len() > DIRECT_MAX_BYTES {
            true => LinkPath::Relay,
            false => path,
        };
        self.frames.push((path, sealed));
        self.stamps += 1;
        (self.stamps, path)
    }

    fn resend(&mut self, sent: &mut Sent, path: LinkPath, now: Instant) -> bool {
        if self
            .budget_at
            .is_none_or(|at| now.duration_since(at) >= MIN_RESEND)
        {
            self.budget = RESEND_BYTES;
            self.budget_at = Some(now);
        }
        let bytes = sent.sealed.ciphertext.len();
        if bytes > self.budget && self.budget < RESEND_BYTES {
            return false;
        }
        self.budget = self.budget.saturating_sub(bytes);
        (sent.stamp, sent.path) = self.send(path, sent.sealed.clone());
        sent.sent_at = now;
        sent.tries += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::pack;
    use crate::{ClientMsg, Keys, MAX_FRAME_BYTES, Origin, RejoinPoint, RelayId};

    fn keys() -> (Keys, Keys) {
        (RelayId::random().keys(), RelayId::random().keys())
    }

    fn link(from: &Keys, to: &Keys) -> PeerLink {
        PeerLink::new(Arc::new(from.sealer(&to.public_key())))
    }

    fn pair() -> (PeerLink, PeerLink) {
        let (a, b) = keys();
        (link(&a, &b), link(&b, &a))
    }

    fn rejoin(frame: u32) -> PlayMsg {
        PlayMsg::Rejoin {
            round: 1,
            from: RejoinPoint {
                session: 9,
                round: 1,
                frame,
                oldest: 0,
            },
        }
    }

    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    fn heartgold_resync() -> PlayMsg {
        let mut data = noise(2 << 20, 7);
        data.resize(7 << 20, 0);
        PlayMsg::Resync {
            round: 3,
            frame: 4_000,
            unit: 1,
            data,
            cable: None,
        }
    }

    fn heartgold_resume() -> PlayMsg {
        PlayMsg::Setup {
            round: 3,
            from: Origin::Resume(pack(&noise(2 << 20, 11), true)),
            budget: 0,
        }
    }

    fn hear_all(link: &mut PeerLink, frames: &[(LinkPath, Sealed)], now: Instant) {
        for (path, sealed) in frames {
            link.hear(*path, sealed, now).unwrap();
        }
    }

    fn taken(link: &mut PeerLink) -> Vec<PlayMsg> {
        std::iter::from_fn(|| link.take()).collect()
    }

    #[test]
    fn a_peer_that_skips_one_message_cant_park_more_than_the_byte_cap() {
        let (_, mut b) = pair();
        let now = Instant::now();
        let body = vec![0; 1 << 20];
        for seq in 2..200 {
            b.arrive(seq, &body, now);
        }
        assert!(b.held.values().map(Vec::len).sum::<usize>() <= MAX_HELD_BYTES);
        assert_eq!(b.held_bytes, b.held.values().map(Vec::len).sum::<usize>());
        b.arrive(1, &body, now);
        assert_eq!(b.held_bytes, b.held.values().map(Vec::len).sum::<usize>());
        assert!(b.held.is_empty());
    }

    #[test]
    fn messages_arrive_in_order_and_acks_clear_the_sender() {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        for frame in 0..3 {
            a.send(&rejoin(frame), now);
        }
        let frames = a.poll(now);
        hear_all(&mut b, &frames, now);
        assert_eq!(taken(&mut b), (0..3).map(rejoin).collect::<Vec<_>>());
        assert!(b.poll(now).is_empty());
        let acks = b.poll(now + ACK_DELAY);
        assert_eq!(acks.len(), 1);
        hear_all(&mut a, &acks, now + ACK_DELAY);
        assert!(a.unacked.is_empty());
        assert!(a.poll(now + MIN_RESEND * 10).is_empty());
    }

    #[test]
    fn a_lost_frame_is_sent_again_when_a_later_one_arrives() {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        for frame in 0..3 {
            a.send(&rejoin(frame), now);
        }
        let frames = a.poll(now);
        hear_all(&mut b, &[frames[0].clone(), frames[2].clone()], now);
        assert_eq!(taken(&mut b), vec![rejoin(0)]);
        let acks = b.poll(now);
        hear_all(&mut a, &acks, now);
        let again = a.poll(now);
        assert_eq!(again.len(), 1);
        hear_all(&mut b, &again, now);
        assert_eq!(taken(&mut b), vec![rejoin(1), rejoin(2)]);
    }

    #[test]
    fn a_frame_heard_twice_is_taken_once() {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        a.send(&rejoin(5), now);
        let frames = a.poll(now);
        hear_all(&mut b, &frames, now);
        hear_all(&mut b, &frames, now);
        assert_eq!(taken(&mut b), vec![rejoin(5)]);
    }

    #[test]
    fn a_lost_last_frame_goes_again_after_the_resend_wait() {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        a.send(&rejoin(1), now);
        let _lost = a.poll(now);
        assert!(a.poll(now + MIN_RESEND / 2).is_empty());
        let again = a.poll(now + MIN_RESEND);
        hear_all(&mut b, &again, now + MIN_RESEND);
        assert_eq!(taken(&mut b), vec![rejoin(1)]);
    }

    #[test]
    fn heartgold_sized_states_cross_in_pieces_that_fit_a_frame() {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        for msg in [heartgold_resync(), heartgold_resume()] {
            a.send(&msg, now);
            let frames = a.poll(now);
            assert!(frames.len() > 1);
            let mut seen = Vec::new();
            for sealed in &frames {
                let frame = ClientMsg::Play {
                    session: u64::MAX,
                    sealed: sealed.1.clone(),
                };
                assert!(frame.encode().unwrap().len() <= MAX_FRAME_BYTES);
                b.hear(sealed.0, &sealed.1, now).unwrap();
                seen.extend(b.receiving());
            }
            assert_eq!(taken(&mut b), vec![msg]);
            assert_eq!(b.receiving(), None);
            assert!(seen.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 == w[1].1));
        }
    }

    #[test]
    fn a_join_from_an_older_version_still_arrives() {
        let (a, b): (Keys, Keys) = (RelayId::random().keys(), RelayId::random().keys());
        let mut link = PeerLink::new(Arc::new(b.sealer(&a.public_key())));
        let join = PlayMsg::Join {
            version: 12,
            round: 0,
            game: None,
        };
        let sealed = join.seal(&a, &b.public_key());
        link.hear(LinkPath::Relay, &sealed, Instant::now()).unwrap();
        assert_eq!(taken(&mut link), vec![join]);
        let pong = PlayMsg::Pong(3).seal(&a, &b.public_key());
        assert_eq!(
            link.hear(LinkPath::Relay, &pong, Instant::now()),
            Err(OpenError)
        );
    }

    #[test]
    fn a_receiver_made_again_gets_what_the_sender_still_holds() {
        let (ka, kb) = keys();
        let (mut a, mut b) = (link(&ka, &kb), link(&kb, &ka));
        let now = Instant::now();
        a.send(&rejoin(0), now);
        hear_all(&mut b, &a.poll(now), now);
        let later = now + ACK_DELAY;
        hear_all(&mut a, &b.poll(later), later);
        assert!(a.unacked.is_empty());
        a.send(&rejoin(1), later);
        let _lost = a.poll(later);
        let mut b = link(&kb, &ka);
        a.send(&rejoin(2), later);
        hear_all(&mut b, &a.poll(later), later);
        hear_all(&mut a, &b.poll(later), later);
        hear_all(&mut b, &a.poll(later), later);
        assert_eq!(taken(&mut b), vec![rejoin(1), rejoin(2)]);
    }

    #[test]
    fn a_sender_made_again_starts_over_and_its_old_frames_are_ignored() {
        let (ka, kb) = keys();
        let (mut a, mut b) = (link(&ka, &kb), link(&kb, &ka));
        let now = Instant::now();
        for frame in 0..3 {
            a.send(&rejoin(frame), now);
        }
        let old = a.poll(now);
        hear_all(&mut b, &old[..2], now);
        assert_eq!(taken(&mut b), vec![rejoin(0), rejoin(1)]);
        let mut a = link(&ka, &kb);
        a.send(&rejoin(9), now);
        hear_all(&mut b, &a.poll(now), now);
        hear_all(&mut b, &old[2..], now);
        assert_eq!(taken(&mut b), vec![rejoin(9)]);
    }

    const TICK: Duration = Duration::from_millis(10);

    struct Sim {
        links: [PeerLink; 2],
        now: Instant,
        relay: Option<Duration>,
        direct: Option<Duration>,
        flying: Vec<(Instant, usize, LinkPath, Sealed)>,
        got: [Vec<PlayMsg>; 2],
    }

    impl Sim {
        fn new(relay_ms: u64, direct_ms: u64) -> Self {
            let (mut a, mut b) = pair();
            let now = Instant::now();
            a.direct_up(now);
            b.direct_up(now);
            Self {
                links: [a, b],
                now,
                relay: Some(Duration::from_millis(relay_ms)),
                direct: Some(Duration::from_millis(direct_ms)),
                flying: Vec::new(),
                got: [Vec::new(), Vec::new()],
            }
        }

        fn run(&mut self, time: Duration) {
            let end = self.now + time;
            while self.now < end {
                self.now += TICK;
                let now = self.now;
                self.flying.sort_by_key(|(at, ..)| *at);
                let due = self.flying.iter().take_while(|(at, ..)| *at <= now).count();
                for (_, to, path, sealed) in self.flying.drain(..due).collect::<Vec<_>>() {
                    self.links[to].hear(path, &sealed, now).unwrap();
                }
                for from in 0..2 {
                    for (path, sealed) in self.links[from].poll(now) {
                        let delay = match path {
                            LinkPath::Relay => self.relay,
                            LinkPath::Direct => self.direct,
                        };
                        if let Some(delay) = delay {
                            self.flying.push((now + delay, 1 - from, path, sealed));
                        }
                    }
                    self.got[from].extend(std::iter::from_fn(|| self.links[from].take()));
                }
            }
        }

        fn send(&mut self, from: usize, msg: PlayMsg) {
            self.links[from].send(&msg, self.now);
        }

        fn paths(&self) -> [LinkPath; 2] {
            [self.links[0].path(), self.links[1].path()]
        }
    }

    #[test]
    fn both_sides_move_to_a_working_direct_path() {
        let mut sim = Sim::new(40, 10);
        sim.run(PROBE_EVERY * (DIRECT_ECHOES + 1));
        assert_eq!(sim.paths(), [LinkPath::Direct; 2]);
        sim.relay = None;
        for frame in 0..5 {
            sim.send(0, rejoin(frame));
        }
        sim.run(Duration::from_millis(100));
        assert_eq!(sim.got[1], (0..5).map(rejoin).collect::<Vec<_>>());
    }

    #[test]
    fn a_direct_path_much_slower_than_the_relay_is_not_used() {
        let mut slow = Sim::new(20, 35);
        slow.run(Duration::from_secs(3));
        assert_eq!(slow.paths(), [LinkPath::Relay; 2]);
        let mut close = Sim::new(20, 25);
        close.run(Duration::from_secs(3));
        assert_eq!(close.paths(), [LinkPath::Direct; 2]);
    }

    #[test]
    fn a_direct_path_that_goes_quiet_falls_back_to_the_relay() {
        let mut sim = Sim::new(30, 10);
        sim.run(Duration::from_secs(2));
        assert_eq!(sim.paths(), [LinkPath::Direct; 2]);
        sim.direct = None;
        sim.run(DIRECT_SILENT - PROBE_EVERY - TICK * 3);
        assert_eq!(sim.paths(), [LinkPath::Direct; 2]);
        sim.run(PROBE_EVERY + TICK * 5);
        assert_eq!(sim.paths(), [LinkPath::Relay; 2]);
    }

    #[test]
    fn messages_in_flight_when_the_direct_path_breaks_arrive_once_in_order() {
        let mut sim = Sim::new(30, 10);
        sim.run(Duration::from_secs(2));
        let mut sent = Vec::new();
        for frame in 0..120 {
            if frame == 30 {
                sim.direct = None;
            }
            sim.send(0, rejoin(frame));
            sent.push(rejoin(frame));
            sim.run(Duration::from_millis(16));
            if frame == 30 + DIRECT_UNACKED.as_millis() as u32 / 16 + 2 {
                assert_eq!(sim.links[0].path(), LinkPath::Relay);
            }
        }
        sim.run(Duration::from_secs(2));
        assert_eq!(sim.got[1], sent);
    }

    #[test]
    fn a_big_message_takes_the_relay_while_small_ones_go_direct() {
        let mut sim = Sim::new(30, 10);
        sim.run(Duration::from_secs(2));
        assert_eq!(sim.paths(), [LinkPath::Direct; 2]);
        let now = sim.now;
        let link = &mut sim.links[0];
        link.send(&rejoin(1), now);
        link.send(&PlayMsg::Bytes(vec![]), now);
        let resume = heartgold_resume();
        link.send(&resume, now);
        let frames = link.poll(now);
        assert_eq!([frames[0].0, frames[1].0], [LinkPath::Direct; 2]);
        let big: Vec<LinkPath> = frames
            .iter()
            .filter(|(_, sealed)| sealed.ciphertext.len() > DIRECT_MAX_BYTES)
            .map(|(path, _)| *path)
            .collect();
        assert!(big.len() > 1);
        assert!(big.iter().all(|path| *path == LinkPath::Relay));
    }

    #[test]
    fn a_message_heard_on_both_paths_is_taken_once() {
        let (mut a, mut b) = pair();
        let now = Instant::now();
        a.send(&rejoin(4), now);
        let (_, sealed) = a.poll(now).remove(0);
        b.hear(LinkPath::Direct, &sealed, now).unwrap();
        b.hear(LinkPath::Relay, &sealed, now + TICK).unwrap();
        assert_eq!(taken(&mut b), vec![rejoin(4)]);
        assert_eq!(b.duplicates(), 1);
    }

    #[test]
    fn a_broken_direct_path_is_tried_again_after_5_then_15_then_every_60_seconds() {
        let (mut a, _) = pair();
        let start = Instant::now();
        a.direct_down(start);
        let mut tries = Vec::new();
        let mut now = start;
        while now < start + Duration::from_secs(200) {
            if a.retry_due(now) {
                tries.push(now.duration_since(start).as_secs());
            }
            now += Duration::from_millis(500);
        }
        assert_eq!(tries, vec![5, 20, 80, 140]);
    }

    #[test]
    fn the_direct_path_comes_back_only_after_the_retry_wait() {
        let mut sim = Sim::new(30, 10);
        sim.run(Duration::from_secs(2));
        let delay = sim.direct.take();
        sim.run(DIRECT_SILENT + TICK * 5);
        assert_eq!(sim.paths(), [LinkPath::Relay; 2]);
        sim.direct = delay;
        sim.run(DIRECT_RETRIES[0] - DIRECT_SILENT);
        assert_eq!(sim.paths(), [LinkPath::Relay; 2]);
        sim.run(DIRECT_SILENT * 2);
        assert_eq!(sim.paths(), [LinkPath::Direct; 2]);
    }

    #[test]
    fn a_closed_direct_path_moves_to_the_relay_at_once() {
        let mut sim = Sim::new(30, 10);
        sim.run(Duration::from_secs(2));
        let now = sim.now;
        sim.links[0].direct_down(now);
        assert_eq!(sim.links[0].path(), LinkPath::Relay);
        assert!(!sim.links[0].retry_due(now + DIRECT_RETRIES[0] - TICK));
        assert!(sim.links[0].retry_due(now + DIRECT_RETRIES[0]));
    }
}
