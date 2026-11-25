use crate::{CONNECT_WAIT, Event, Note, Stun};
use citrine_gb_net::{STUN_ID_BYTES, StunId, stun_mapped, stun_request};
use citrine_gb_threads::Spawner;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelConfig, ChannelId, Reliability};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event as RtcEvent, IceConnectionState, Input, Output, Rtc, RtcConfig};

const LABEL: &str = "citrine";
const IDLE: Duration = Duration::from_millis(100);
const READ_WAIT: Duration = Duration::from_millis(250);
const STUN_TRIES: u32 = 5;
const STUN_EVERY: Duration = Duration::from_millis(400);
const ROUTE_V4: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 9);
const ROUTE_V6: SocketAddr = SocketAddr::new(
    IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
    9,
);

enum Cmd {
    Stun(Option<Stun>),
    Offer(u64),
    Hear(u64, Note),
    Send(u64, Vec<u8>),
    Close(u64),
}

enum In {
    Cmd(Cmd),
    Packet {
        socket: usize,
        from: SocketAddr,
        data: Vec<u8>,
    },
}

#[derive(Default)]
pub struct Counts {
    sent: AtomicU64,
    dropped: AtomicU64,
}

impl Counts {
    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Acquire)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Acquire)
    }
}

pub struct Direct {
    tx: Option<Sender<In>>,
    events: Option<Receiver<Event>>,
    stun: Option<Stun>,
    spawner: Option<Spawner>,
    deaf: Arc<AtomicBool>,
    mute: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    counts: Arc<Counts>,
}

impl Default for Direct {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Direct {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Direct {
    pub fn new() -> Self {
        Self {
            tx: None,
            events: None,
            stun: None,
            spawner: None,
            deaf: Arc::new(AtomicBool::new(false)),
            mute: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            counts: Arc::default(),
        }
    }

    pub fn counts(&self) -> Arc<Counts> {
        self.counts.clone()
    }

    pub fn give_spawner(&mut self, spawner: impl FnOnce() -> Spawner) {
        if self.spawner.is_none() {
            self.spawner = Some(spawner());
        }
    }

    pub fn set_stun(&mut self, stun: Option<Stun>) {
        if self.stun == stun {
            return;
        }
        self.stun = stun.clone();
        if let Some(tx) = &self.tx {
            let _ = tx.send(In::Cmd(Cmd::Stun(stun)));
        }
    }

    pub fn offer(&mut self, peer: u64) {
        self.command(Cmd::Offer(peer));
    }

    pub fn hear(&mut self, peer: u64, note: Note) {
        self.command(Cmd::Hear(peer, note));
    }

    pub fn send(&mut self, peer: u64, frame: Vec<u8>) {
        if let Some(tx) = &self.tx {
            self.counts.sent.fetch_add(1, Ordering::AcqRel);
            if tx.send(In::Cmd(Cmd::Send(peer, frame))).is_err() {
                self.counts.dropped.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    pub fn close(&mut self, peer: u64) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(In::Cmd(Cmd::Close(peer)));
        }
    }

    pub fn cut(&self, cut: bool) {
        self.deaf.store(cut, Ordering::Relaxed);
        self.mute.store(cut, Ordering::Relaxed);
    }

    pub fn deafen(&self, deaf: bool) {
        self.deaf.store(deaf, Ordering::Relaxed);
    }

    pub fn poll(&mut self) -> Vec<Event> {
        self.events
            .as_ref()
            .map(|events| events.try_iter().collect())
            .unwrap_or_default()
    }

    fn command(&mut self, cmd: Cmd) {
        if self.tx.is_none() {
            self.start();
        }
        if let Some(tx) = &self.tx
            && tx.send(In::Cmd(cmd)).is_err()
        {
            self.tx = None;
            self.events = None;
        }
    }

    fn start(&mut self) {
        let Some(spawner) = self.spawner.clone() else {
            return;
        };
        let sockets: Vec<UdpSocket> = [
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
        ]
        .into_iter()
        .filter_map(|addr| UdpSocket::bind(addr).ok())
        .collect();
        if sockets.is_empty() {
            log::warn!("direct connections: no UDP socket could open");
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (events_tx, events) = mpsc::channel();
        for (index, socket) in sockets.iter().enumerate() {
            let Ok(socket) = socket.try_clone() else {
                continue;
            };
            let (tx, stop, deaf) = (tx.clone(), self.stop.clone(), self.deaf.clone());
            spawner.thread("citrine-direct-read", move || {
                read(index, socket, tx, stop, deaf)
            });
        }
        if let Some(stun) = self.stun.clone() {
            let _ = tx.send(In::Cmd(Cmd::Stun(Some(stun))));
        }
        let worker = Worker::new(
            sockets,
            rx,
            events_tx,
            [self.stop.clone(), self.mute.clone()],
            self.counts.clone(),
        );
        spawner.thread("citrine-direct", move || worker.run());
        self.tx = Some(tx);
        self.events = Some(events);
    }
}

fn read(
    index: usize,
    socket: UdpSocket,
    tx: Sender<In>,
    stop: Arc<AtomicBool>,
    deaf: Arc<AtomicBool>,
) {
    let _ = socket.set_read_timeout(Some(READ_WAIT));
    let mut buf = vec![0; 2048];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if deaf.load(Ordering::Relaxed) {
                    continue;
                }
                let data = buf[..n].to_vec();
                if tx
                    .send(In::Packet {
                        socket: index,
                        from,
                        data,
                    })
                    .is_err()
                {
                    return;
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

struct Ask {
    server: SocketAddr,
    id: StunId,
    tries: u32,
    at: Instant,
}

struct Sock {
    socket: UdpSocket,
    port: u16,
    v6: bool,
    host: Option<SocketAddr>,
    mapped: Option<SocketAddr>,
    ask: Option<Ask>,
}

impl Sock {
    fn new(socket: UdpSocket) -> Option<Self> {
        let local = socket.local_addr().ok()?;
        let route = match local.is_ipv6() {
            true => ROUTE_V6,
            false => ROUTE_V4,
        };
        let mut sock = Self {
            socket,
            port: local.port(),
            v6: local.is_ipv6(),
            host: None,
            mapped: None,
            ask: None,
        };
        sock.host = sock.host_toward(route);
        Some(sock)
    }

    fn host_toward(&self, target: SocketAddr) -> Option<SocketAddr> {
        let unspecified = match self.v6 {
            true => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
            false => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        };
        let probe = UdpSocket::bind(unspecified).ok()?;
        probe.connect(target).ok()?;
        let ip = probe.local_addr().ok()?.ip();
        (!ip.is_unspecified()).then_some(SocketAddr::new(ip, self.port))
    }

    fn sends(&self, source: SocketAddr) -> bool {
        source.port() == self.port && source.is_ipv6() == self.v6
    }
}

struct Peer {
    rtc: Rtc,
    channel: Option<ChannelId>,
    pending: Option<SdpPendingOffer>,
    wake: Instant,
    made: Instant,
    up: bool,
}

struct Worker {
    rx: Receiver<In>,
    events: Sender<Event>,
    socks: Vec<Sock>,
    peers: HashMap<u64, Peer>,
    stop: Arc<AtomicBool>,
    mute: Arc<AtomicBool>,
    counts: Arc<Counts>,
}

impl Worker {
    fn new(
        sockets: Vec<UdpSocket>,
        rx: Receiver<In>,
        events: Sender<Event>,
        [stop, mute]: [Arc<AtomicBool>; 2],
        counts: Arc<Counts>,
    ) -> Self {
        Self {
            rx,
            events,
            socks: sockets.into_iter().filter_map(Sock::new).collect(),
            peers: HashMap::new(),
            stop,
            mute,
            counts,
        }
    }

    fn run(mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            self.ask_stun(now);
            self.wake(now);
            let next = self
                .peers
                .values()
                .map(|peer| peer.wake)
                .chain(
                    self.socks
                        .iter()
                        .filter_map(|sock| sock.ask.as_ref().map(|ask| ask.at)),
                )
                .fold(now + IDLE, Instant::min);
            match self
                .rx
                .recv_timeout(next.saturating_duration_since(Instant::now()))
            {
                Ok(In::Cmd(cmd)) => self.command(cmd),
                Ok(In::Packet { socket, from, data }) => self.packet(socket, from, &data),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn command(&mut self, cmd: Cmd) {
        let now = Instant::now();
        match cmd {
            Cmd::Stun(stun) => self.set_stun(stun, now),
            Cmd::Offer(id) => self.offer(id, now),
            Cmd::Hear(id, Note::Offer(sdp)) => self.answer(id, &sdp, now),
            Cmd::Hear(id, Note::Answer(sdp)) => {
                let Ok(answer) = SdpAnswer::from_sdp_string(&sdp) else {
                    return log::warn!("direct connections: an answer didn't parse");
                };
                let Some(peer) = self.peers.get_mut(&id) else {
                    return;
                };
                let Some(pending) = peer.pending.take() else {
                    return;
                };
                if let Err(e) = peer.rtc.sdp_api().accept_answer(pending, answer) {
                    log::warn!("direct connections: an answer was refused: {e}");
                }
                self.drain(id);
            }
            Cmd::Hear(id, Note::Candidate(line)) => {
                let Ok(candidate) = Candidate::from_sdp_string(&line) else {
                    return log::debug!("direct connections: skipped candidate {line}");
                };
                let Some(peer) = self.peers.get_mut(&id) else {
                    return;
                };
                peer.rtc.add_remote_candidate(candidate);
                self.drain(id);
            }
            Cmd::Send(id, frame) => {
                let mute = self.mute.load(Ordering::Relaxed);
                let wrote = self.peers.get_mut(&id).and_then(|peer| {
                    let mut channel = peer.channel.and_then(|channel| peer.rtc.channel(channel))?;
                    Some(matches!(channel.write(true, &frame), Ok(true)))
                });
                if wrote != Some(true) || mute {
                    self.counts.dropped.fetch_add(1, Ordering::AcqRel);
                }
                if wrote.is_some() {
                    self.drain(id);
                }
            }
            Cmd::Close(id) => {
                if let Some(mut peer) = self.peers.remove(&id) {
                    peer.rtc.disconnect();
                }
            }
        }
    }

    fn set_stun(&mut self, stun: Option<Stun>, now: Instant) {
        let servers: Vec<SocketAddr> = stun
            .and_then(|stun| {
                (stun.host.trim_matches(['[', ']']), stun.port)
                    .to_socket_addrs()
                    .ok()
            })
            .map(|addrs| addrs.collect())
            .unwrap_or_default();
        for sock in &mut self.socks {
            let server = servers
                .iter()
                .find(|addr| addr.is_ipv6() == sock.v6)
                .copied();
            sock.ask = server.map(|server| Ask {
                server,
                id: random_id(),
                tries: 0,
                at: now,
            });
            if let Some(host) = server.and_then(|server| sock.host_toward(server)) {
                sock.host = Some(host);
            }
        }
    }

    fn ask_stun(&mut self, now: Instant) {
        for sock in &mut self.socks {
            let Some(ask) = &mut sock.ask else {
                continue;
            };
            if ask.tries >= STUN_TRIES || now < ask.at {
                continue;
            }
            ask.tries += 1;
            ask.at = now + STUN_EVERY;
            if !self.mute.load(Ordering::Relaxed) {
                let _ = sock.socket.send_to(&stun_request(&ask.id), ask.server);
            }
            if ask.tries >= STUN_TRIES {
                ask.at = now + Duration::from_secs(3600);
            }
        }
    }

    fn rtc(&self, now: Instant) -> Rtc {
        let mut rtc = RtcConfig::new().build(now);
        for candidate in self.local_candidates() {
            rtc.add_local_candidate(candidate);
            while let Ok(output) = rtc.poll_output() {
                if matches!(output, Output::Timeout(_)) {
                    break;
                }
            }
        }
        rtc
    }

    fn local_candidates(&self) -> Vec<Candidate> {
        let mut out = Vec::new();
        for sock in &self.socks {
            let Some(host) = sock.host else {
                continue;
            };
            out.extend(Candidate::host(host, "udp").ok());
            if let Some(mapped) = sock.mapped.filter(|mapped| *mapped != host) {
                out.extend(Candidate::server_reflexive(mapped, host, "udp").ok());
            }
        }
        out
    }

    fn offer(&mut self, id: u64, now: Instant) {
        self.drop_peer(id);
        let mut rtc = self.rtc(now);
        let mut change = rtc.sdp_api();
        let channel = change.add_channel_with_config(ChannelConfig {
            label: LABEL.into(),
            ordered: false,
            reliability: Reliability::MaxRetransmits { retransmits: 0 },
            ..ChannelConfig::default()
        });
        let Some((offer, pending)) = change.apply() else {
            return;
        };
        self.peers.insert(
            id,
            Peer {
                rtc,
                channel: Some(channel),
                pending: Some(pending),
                wake: now,
                made: now,
                up: false,
            },
        );
        self.drain(id);
        let _ = self
            .events
            .send(Event::Note(id, Note::Offer(offer.to_sdp_string())));
    }

    fn answer(&mut self, id: u64, sdp: &str, now: Instant) {
        let Ok(offer) = SdpOffer::from_sdp_string(sdp) else {
            return log::warn!("direct connections: an offer didn't parse");
        };
        self.drop_peer(id);
        let mut rtc = self.rtc(now);
        let answer = match rtc.sdp_api().accept_offer(offer) {
            Ok(answer) => answer,
            Err(e) => return log::warn!("direct connections: an offer was refused: {e}"),
        };
        self.peers.insert(
            id,
            Peer {
                rtc,
                channel: None,
                pending: None,
                wake: now,
                made: now,
                up: false,
            },
        );
        self.drain(id);
        let _ = self
            .events
            .send(Event::Note(id, Note::Answer(answer.to_sdp_string())));
    }

    fn drop_peer(&mut self, id: u64) {
        if let Some(mut peer) = self.peers.remove(&id) {
            peer.rtc.disconnect();
            if peer.up {
                let _ = self.events.send(Event::Down(id));
            }
        }
    }

    fn packet(&mut self, socket: usize, from: SocketAddr, data: &[u8]) {
        let Some(sock) = self.socks.get_mut(socket) else {
            return;
        };
        if let Some(ask) = &sock.ask
            && ask.server == from
            && let Some(mapped) = stun_mapped(data, &ask.id)
        {
            sock.ask = None;
            sock.mapped = Some(mapped);
            return self.found(socket);
        }
        let Some(destination) = sock.host else {
            return;
        };
        let Ok(receive) = Receive::new(Protocol::Udp, from, destination, data) else {
            return;
        };
        let input = Input::Receive(Instant::now(), receive);
        let Some(id) = self
            .peers
            .iter()
            .find(|(_, peer)| peer.rtc.accepts(&input))
            .map(|(id, _)| *id)
        else {
            return;
        };
        if let Some(peer) = self.peers.get_mut(&id)
            && let Err(e) = peer.rtc.handle_input(input)
        {
            log::debug!("direct connections: {e}");
        }
        self.drain(id);
    }

    fn found(&mut self, socket: usize) {
        let sock = &self.socks[socket];
        let (Some(host), Some(mapped)) = (sock.host, sock.mapped) else {
            return;
        };
        if host == mapped {
            return;
        }
        let Ok(candidate) = Candidate::server_reflexive(mapped, host, "udp") else {
            return;
        };
        let ids: Vec<u64> = self.peers.keys().copied().collect();
        for id in ids {
            let Some(peer) = self.peers.get_mut(&id) else {
                continue;
            };
            let line = peer
                .rtc
                .add_local_candidate(candidate.clone())
                .map(Candidate::to_sdp_string);
            self.drain(id);
            if let Some(line) = line {
                let _ = self.events.send(Event::Note(id, Note::Candidate(line)));
            }
        }
    }

    fn wake(&mut self, now: Instant) {
        let ids: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, peer)| peer.wake <= now || !peer.up && now >= peer.made + CONNECT_WAIT)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            let Some(peer) = self.peers.get_mut(&id) else {
                continue;
            };
            if !peer.up && now >= peer.made + CONNECT_WAIT {
                self.peers.remove(&id);
                let _ = self.events.send(Event::Down(id));
                continue;
            }
            if let Err(e) = peer.rtc.handle_input(Input::Timeout(now)) {
                log::debug!("direct connections: {e}");
            }
            self.drain(id);
        }
    }

    fn drain(&mut self, id: u64) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };
        let mute = self.mute.load(Ordering::Relaxed);
        let mut alive = true;
        loop {
            match peer.rtc.poll_output() {
                Ok(Output::Timeout(at)) => {
                    peer.wake = at;
                    break;
                }
                Ok(Output::Transmit(transmit)) => {
                    if !mute
                        && let Some(sock) =
                            self.socks.iter().find(|sock| sock.sends(transmit.source))
                    {
                        let _ = sock
                            .socket
                            .send_to(&transmit.contents, transmit.destination);
                    }
                }
                Ok(Output::Event(event)) => match event {
                    RtcEvent::ChannelOpen(channel, _) => {
                        peer.channel = Some(channel);
                        if !peer.up {
                            peer.up = true;
                            let _ = self.events.send(Event::Up(id));
                        }
                    }
                    RtcEvent::ChannelData(data) => {
                        let _ = self.events.send(Event::Frame(id, data.data));
                    }
                    RtcEvent::ChannelClose(_)
                    | RtcEvent::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                        alive = false;
                    }
                    _ => {}
                },
                Err(e) => {
                    log::debug!("direct connections: {e}");
                    alive = false;
                    break;
                }
            }
        }
        if !alive || !peer.rtc.is_alive() {
            self.peers.remove(&id);
            let _ = self.events.send(Event::Down(id));
        }
    }
}

fn random_id() -> StunId {
    let mut id = [0; STUN_ID_BYTES];
    getrandom::getrandom(&mut id).expect("no system random source");
    id
}
