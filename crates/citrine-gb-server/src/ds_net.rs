use crate::hub::Outbox;
use crate::limit::{Bucket, Buckets};
use citrine_gb_net::{
    MAX_DATAGRAM_BYTES, MAX_NET_READ_BYTES, NET_WINDOW_BYTES, NetAddr, OnlineId, ServerMsg,
};
use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

pub const TCP_PORTS: &[u16] = &[80, 443, 28910, 29900, 29901, 29920];
pub const DATAGRAM_OVERHEAD: usize = 28;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_IDLE: Duration = Duration::from_secs(120);
const DNS_PORT: u16 = 53;
const STUN_MAGIC: u32 = 0x2112_a442;
const STUN_WAIT: Duration = Duration::from_millis(250);
const STUN_TRIES: u32 = 2;
const PACE_CHUNK: usize = 4 * 1024;
const PACE_WAIT: Duration = Duration::from_millis(20);
const PEERS_KEPT: usize = 64;
const LAN_NET: [u8; 2] = [10, 77];
const LAN_HOSTS: u32 = 254 * 254;

#[derive(Default)]
struct Peers(Mutex<VecDeque<SocketAddrV4>>);

impl Peers {
    fn note(&self, to: SocketAddrV4) {
        let mut sent = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = sent.iter().position(|peer| *peer == to) {
            sent.remove(at);
        } else if sent.len() >= PEERS_KEPT {
            sent.pop_front();
        }
        sent.push_back(to);
    }

    fn knows(&self, from: SocketAddrV4) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&from)
    }

    fn knows_ip(&self, ip: Ipv4Addr) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|peer| *peer.ip() == ip)
    }
}

#[derive(Clone)]
struct Owner {
    net: u64,
    outbox: Outbox,
    local: u16,
    peers: Arc<Peers>,
}

#[derive(Clone)]
struct Lessee {
    net: u64,
    who: OnlineId,
    outbox: Outbox,
}

#[derive(Default)]
struct Leases {
    held: HashMap<Ipv4Addr, Lessee>,
    next: u32,
}

pub struct Ports {
    owners: Mutex<HashMap<u16, Owner>>,
    leases: Mutex<Leases>,
    nets: AtomicU64,
    own: Vec<Ipv4Addr>,
    account_bytes: Buckets<i64>,
    account_packets: Buckets<i64>,
}

pub fn on_lan(ip: Ipv4Addr) -> bool {
    ip.octets()[..2] == LAN_NET
}

fn lan_host(index: u32) -> Ipv4Addr {
    let [a, b] = LAN_NET;
    Ipv4Addr::new(a, b, (index / 254 + 1) as u8, (index % 254 + 1) as u8)
}

fn lan_lease(ip: Ipv4Addr) -> bool {
    let [_, _, c, d] = ip.octets();
    on_lan(ip) && (1..=254).contains(&c) && (1..=254).contains(&d)
}

pub fn route_ip() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(addr) if !addr.ip().is_unspecified() && !addr.ip().is_loopback() => {
            Some(*addr.ip())
        }
        _ => None,
    }
}

impl Ports {
    pub fn new(account_bytes: u64, account_packets: u64, own: Vec<Ipv4Addr>) -> Self {
        let (bytes, packets) = (account_bytes as f64, account_packets as f64);
        Self {
            owners: Mutex::default(),
            leases: Mutex::default(),
            nets: AtomicU64::new(0),
            own,
            account_bytes: Buckets::new(bytes, bytes * 2.0),
            account_packets: Buckets::new(packets, packets * 2.0),
        }
    }

    fn may_reach(&self, ip: Ipv4Addr, rules: &Rules) -> bool {
        reachable(ip, rules.private) && rules.public_ip != Some(ip) && !self.own.contains(&ip)
    }

    fn claim(&self, port: u16, owner: Owner) {
        self.owners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(port, owner);
    }

    fn release(&self, port: u16, net: u64) {
        let mut owners = self.owners.lock().unwrap_or_else(|e| e.into_inner());
        if owners.get(&port).is_some_and(|owner| owner.net == net) {
            owners.remove(&port);
        }
    }

    fn owner(&self, port: u16) -> Option<Owner> {
        self.owners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&port)
            .cloned()
    }

    fn lease(&self, want: Option<Ipv4Addr>, lessee: Lessee) -> Option<Ipv4Addr> {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        let wanted = want.filter(|ip| {
            lan_lease(*ip)
                && leases
                    .held
                    .get(ip)
                    .is_none_or(|held| held.who == lessee.who)
        });
        let ip = match wanted {
            Some(ip) => ip,
            None => {
                let index = (0..LAN_HOSTS)
                    .map(|step| (leases.next + step) % LAN_HOSTS)
                    .find(|index| !leases.held.contains_key(&lan_host(*index)))?;
                leases.next = (index + 1) % LAN_HOSTS;
                lan_host(index)
            }
        };
        leases.held.insert(ip, lessee);
        Some(ip)
    }

    fn release_lease(&self, ip: Ipv4Addr, net: u64) {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        if leases.held.get(&ip).is_some_and(|lessee| lessee.net == net) {
            leases.held.remove(&ip);
        }
    }

    fn lessee(&self, ip: Ipv4Addr) -> Option<Lessee> {
        self.leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .held
            .get(&ip)
            .cloned()
    }
}

pub fn reachable(ip: Ipv4Addr, private: bool) -> bool {
    if ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
        return false;
    }
    if private {
        return true;
    }
    let [a, b, c, _] = ip.octets();
    let reserved = a == 0
        || a >= 240
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && (c == 0 || c == 2))
        || (a == 198 && (18..20).contains(&b))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113);
    !(reserved || ip.is_private() || ip.is_loopback() || ip.is_link_local())
}

pub struct Rules {
    pub public_ip: Option<Ipv4Addr>,
    pub private: bool,
    pub ports: usize,
    pub streams: usize,
    pub stun: Option<String>,
    pub account: i64,
}

struct Budget {
    bytes: Bucket,
    packets: Bucket,
}

#[derive(Clone)]
struct Egress {
    budget: Arc<Mutex<Budget>>,
    ports: Arc<Ports>,
    account: i64,
}

impl Egress {
    fn take(&self, bytes: usize, packets: u32) -> bool {
        let (bytes, packets) = (bytes as f64, packets as f64);
        let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        let ports = &self.ports;
        let fits = budget.bytes.has(bytes)
            && budget.packets.has(packets)
            && ports.account_bytes.has(self.account, bytes)
            && ports.account_packets.has(self.account, packets);
        if fits {
            budget.bytes.take(bytes);
            budget.packets.take(packets);
            ports.account_bytes.take(self.account, bytes);
            ports.account_packets.take(self.account, packets);
        }
        fits
    }

    async fn pace(&self, bytes: usize) {
        while !self.take(bytes, 0) {
            tokio::time::sleep(PACE_WAIT).await;
        }
    }
}

struct Activity {
    start: Instant,
    last_ms: AtomicU64,
    heard: AtomicBool,
}

impl Activity {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
            heard: AtomicBool::new(false),
        }
    }

    fn touch(&self) {
        let now = self.start.elapsed().as_millis() as u64;
        self.last_ms.fetch_max(now, Ordering::Relaxed);
    }

    fn last_ms(&self) -> u64 {
        self.last_ms.load(Ordering::Relaxed)
    }

    fn idle(&self) -> Duration {
        self.start
            .elapsed()
            .saturating_sub(Duration::from_millis(self.last_ms()))
    }
}

struct Udp {
    socket: Arc<UdpSocket>,
    reader: JoinHandle<()>,
    public: u16,
    claimed: bool,
    activity: Arc<Activity>,
    only_dns: bool,
}

impl Udp {
    fn answered_dns(&self) -> bool {
        self.only_dns && self.activity.heard.load(Ordering::Relaxed)
    }
}

struct Tcp {
    writes: mpsc::Sender<Vec<u8>>,
    queued: Arc<AtomicUsize>,
    read: Arc<AtomicU64>,
    taken: watch::Sender<u64>,
    task: JoinHandle<()>,
}

pub struct Net {
    id: u64,
    ports: Arc<Ports>,
    udp: HashMap<u16, Udp>,
    tcp: HashMap<u32, Tcp>,
    budget: Arc<Mutex<Budget>>,
    peers: Arc<Peers>,
    idle: Duration,
    dns_port: u16,
    stun: Option<(String, SocketAddr)>,
    lease: Option<Ipv4Addr>,
}

impl Net {
    pub fn new(ports: Arc<Ports>, bytes_per_second: u64, packets_per_second: u64) -> Self {
        let (bytes, packets) = (bytes_per_second as f64, packets_per_second as f64);
        Self {
            id: ports.nets.fetch_add(1, Ordering::Relaxed),
            ports,
            udp: HashMap::new(),
            tcp: HashMap::new(),
            budget: Arc::new(Mutex::new(Budget {
                bytes: Bucket::new(bytes, bytes * 2.0),
                packets: Bucket::new(packets, packets * 2.0),
            })),
            peers: Arc::default(),
            idle: UDP_IDLE,
            dns_port: DNS_PORT,
            stun: None,
            lease: None,
        }
    }

    fn egress(&self, rules: &Rules) -> Egress {
        Egress {
            budget: self.budget.clone(),
            ports: self.ports.clone(),
            account: rules.account,
        }
    }

    pub fn lease(
        &mut self,
        outbox: &Outbox,
        who: OnlineId,
        want: Option<Ipv4Addr>,
    ) -> Option<Ipv4Addr> {
        if self.lease.is_none() {
            let lessee = Lessee {
                net: self.id,
                who,
                outbox: outbox.clone(),
            };
            self.lease = self.ports.lease(want, lessee);
        }
        self.lease
    }

    pub async fn send(
        &mut self,
        rules: &Rules,
        outbox: &Outbox,
        port: u16,
        to: NetAddr,
        data: Vec<u8>,
    ) -> bool {
        if data.len() > MAX_DATAGRAM_BYTES
            || !self.egress(rules).take(data.len() + DATAGRAM_OVERHEAD, 1)
        {
            return false;
        }
        let to = to.socket();
        if on_lan(*to.ip()) {
            let (Some(lease), Some(lessee)) = (self.lease, self.ports.lessee(*to.ip())) else {
                return false;
            };
            if lessee.net == self.id {
                return false;
            }
            return lessee.outbox.try_push(ServerMsg::NetHeard {
                port: to.port(),
                from: NetAddr {
                    ip: lease.octets(),
                    port,
                },
                data,
            });
        }
        let hairpin = rules.public_ip == Some(*to.ip());
        if !hairpin && !self.ports.may_reach(*to.ip(), rules) {
            return false;
        }
        let Some((socket, public)) = self.socket(rules, outbox, port, to.port()).await else {
            return false;
        };
        self.peers.note(to);
        if hairpin {
            let Some(owner) = self.ports.owner(to.port()) else {
                return false;
            };
            let from = SocketAddrV4::new(*to.ip(), public);
            if !owner.peers.knows(from) {
                return false;
            }
            return owner.outbox.try_push(ServerMsg::NetHeard {
                port: owner.local,
                from: from.into(),
                data,
            });
        }
        socket.send_to(&data, to).await.is_ok()
    }

    async fn socket(
        &mut self,
        rules: &Rules,
        outbox: &Outbox,
        port: u16,
        to_port: u16,
    ) -> Option<(Arc<UdpSocket>, u16)> {
        if let Some(udp) = self.udp.get_mut(&port) {
            if !udp.reader.is_finished() {
                udp.activity.touch();
                udp.only_dns &= to_port == self.dns_port;
                return Some((udp.socket.clone(), udp.public));
            }
            self.drop_udp(port);
        }
        self.udp.retain(|_, udp| !udp.reader.is_finished());
        if rules.ports == 0 {
            return None;
        }
        while self.udp.len() >= rules.ports {
            let first = self
                .udp
                .iter()
                .max_by_key(|(_, udp)| (udp.answered_dns(), udp.activity.idle()))
                .map(|(&port, _)| port)?;
            self.drop_udp(first);
        }
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await.ok()?);
        let bound = socket.local_addr().ok()?.port();
        let stun = match &rules.stun {
            Some(server) => self.stun_addr(server).await,
            None => None,
        };
        let mapped = match stun {
            Some(to) => stun_port(&socket, to).await,
            None => None,
        };
        let claimed = rules.stun.is_none() || mapped.is_some();
        let public = mapped.unwrap_or(bound);
        if claimed {
            self.ports.claim(
                public,
                Owner {
                    net: self.id,
                    outbox: outbox.clone(),
                    local: port,
                    peers: self.peers.clone(),
                },
            );
        }
        let activity = Arc::new(Activity::new());
        let reader = tokio::spawn(
            Reader {
                socket: socket.clone(),
                ports: self.ports.clone(),
                outbox: outbox.clone(),
                port,
                claim: claimed.then_some(public),
                net: self.id,
                activity: activity.clone(),
                peers: self.peers.clone(),
                idle: self.idle,
                stun,
            }
            .run(),
        );
        self.udp.insert(
            port,
            Udp {
                socket: socket.clone(),
                reader,
                public,
                claimed,
                activity,
                only_dns: to_port == self.dns_port,
            },
        );
        Some((socket, public))
    }

    async fn stun_addr(&mut self, server: &str) -> Option<SocketAddr> {
        if let Some((name, addr)) = &self.stun
            && name == server
        {
            return Some(*addr);
        }
        let addr = tokio::net::lookup_host(server)
            .await
            .ok()?
            .find(SocketAddr::is_ipv4)?;
        self.stun = Some((server.to_string(), addr));
        Some(addr)
    }

    fn drop_udp(&mut self, port: u16) {
        if let Some(udp) = self.udp.remove(&port) {
            udp.reader.abort();
            if udp.claimed {
                self.ports.release(udp.public, self.id);
            }
        }
    }

    pub fn open(
        &mut self,
        rules: &Rules,
        outbox: &Outbox,
        stream: u32,
        to: NetAddr,
        who: OnlineId,
    ) -> bool {
        self.tcp.retain(|_, tcp| !tcp.task.is_finished());
        let to = to.socket();
        let allowed = TCP_PORTS.contains(&to.port()) || rules.private;
        if self.tcp.len() >= rules.streams
            || self.tcp.contains_key(&stream)
            || !allowed
            || !self.ports.may_reach(*to.ip(), rules)
        {
            return false;
        }
        let (writes, rx) = mpsc::channel(NET_WINDOW_BYTES);
        let queued = Arc::new(AtomicUsize::new(0));
        let read = Arc::new(AtomicU64::new(0));
        let (taken, taken_rx) = watch::channel(0);
        let flow = Flow {
            to: to.into(),
            outbox: outbox.clone(),
            stream,
            who,
            egress: self.egress(rules),
            queued: queued.clone(),
            read: read.clone(),
        };
        let task = tokio::spawn(flow.run(rx, taken_rx));
        self.tcp.insert(
            stream,
            Tcp {
                writes,
                queued,
                read,
                taken,
                task,
            },
        );
        true
    }

    pub fn write(&mut self, stream: u32, data: Vec<u8>) -> bool {
        let sent = self.tcp.get(&stream).is_some_and(|tcp| {
            if data.is_empty() {
                return true;
            }
            let queued = tcp.queued.fetch_add(data.len(), Ordering::Relaxed) + data.len();
            queued <= NET_WINDOW_BYTES && tcp.writes.try_send(data).is_ok()
        });
        if !sent {
            self.close(stream);
        }
        sent
    }

    pub fn taken(&mut self, stream: u32, total: u64) {
        if let Some(tcp) = self.tcp.get(&stream) {
            let total = total.min(tcp.read.load(Ordering::Relaxed));
            tcp.taken.send_if_modified(|taken| {
                let more = total > *taken;
                *taken = (*taken).max(total);
                more
            });
        }
    }

    pub fn close(&mut self, stream: u32) {
        if let Some(tcp) = self.tcp.remove(&stream) {
            tcp.task.abort();
        }
    }

    pub fn finish(&mut self) {
        if let Some(lease) = self.lease.take() {
            self.ports.release_lease(lease, self.id);
        }
        let ports: Vec<u16> = self.udp.keys().copied().collect();
        for port in ports {
            self.drop_udp(port);
        }
        for (_, tcp) in self.tcp.drain() {
            tcp.task.abort();
        }
    }
}

struct Reader {
    socket: Arc<UdpSocket>,
    ports: Arc<Ports>,
    outbox: Outbox,
    port: u16,
    claim: Option<u16>,
    net: u64,
    activity: Arc<Activity>,
    peers: Arc<Peers>,
    idle: Duration,
    stun: Option<SocketAddr>,
}

impl Reader {
    async fn run(self) {
        let mut buf = vec![0; MAX_DATAGRAM_BYTES];
        loop {
            let left = self.idle.saturating_sub(self.activity.idle());
            if left.is_zero() {
                break;
            }
            match tokio::time::timeout(left, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((_, from))) if Some(from) == self.stun => {}
                Ok(Ok((n, SocketAddr::V4(from)))) if self.peers.knows_ip(*from.ip()) => {
                    self.activity.heard.store(true, Ordering::Relaxed);
                    self.activity.touch();
                    self.outbox.try_push(ServerMsg::NetHeard {
                        port: self.port,
                        from: from.into(),
                        data: buf[..n].to_vec(),
                    });
                }
                Ok(Ok(_)) | Err(_) => {}
                Ok(Err(_)) => break,
            }
        }
        if let Some(public) = self.claim {
            self.ports.release(public, self.net);
        }
    }
}

async fn stun_port(socket: &UdpSocket, to: SocketAddr) -> Option<u16> {
    let mut request = [0u8; 20];
    request[..2].copy_from_slice(&1u16.to_be_bytes());
    request[4..8].copy_from_slice(&STUN_MAGIC.to_be_bytes());
    getrandom::getrandom(&mut request[8..]).ok()?;
    let mut buf = [0u8; 512];
    for _ in 0..STUN_TRIES {
        socket.send_to(&request, to).await.ok()?;
        let Ok(Ok((n, from))) = tokio::time::timeout(STUN_WAIT, socket.recv_from(&mut buf)).await
        else {
            continue;
        };
        if from == to
            && let Some(port) = stun_mapped_port(&buf[..n], &request[8..])
        {
            return Some(port);
        }
    }
    None
}

fn stun_mapped_port(reply: &[u8], id: &[u8]) -> Option<u16> {
    if reply.len() < 20 || reply[..2] != [0x01, 0x01] || reply[8..20] != *id {
        return None;
    }
    let mut at = 20;
    while at + 4 <= reply.len() {
        let kind = u16::from_be_bytes([reply[at], reply[at + 1]]);
        let len = u16::from_be_bytes([reply[at + 2], reply[at + 3]]) as usize;
        let value = reply.get(at + 4..at + 4 + len)?;
        if value.len() >= 8 && value[1] == 1 {
            let port = u16::from_be_bytes([value[2], value[3]]);
            match kind {
                0x0020 => return Some(port ^ (STUN_MAGIC >> 16) as u16),
                0x0001 => return Some(port),
                _ => {}
            }
        }
        at += 4 + len.div_ceil(4) * 4;
    }
    None
}

enum End {
    Far,
    Ds,
    Relay,
}

struct Flow {
    to: SocketAddr,
    outbox: Outbox,
    stream: u32,
    who: OnlineId,
    egress: Egress,
    queued: Arc<AtomicUsize>,
    read: Arc<AtomicU64>,
}

impl Flow {
    async fn run(self, writes: mpsc::Receiver<Vec<u8>>, taken: watch::Receiver<u64>) {
        let (to, stream, who) = (self.to, self.stream, self.who);
        let connected = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(to)).await;
        let Ok(Ok(conn)) = connected else {
            log::debug!("ds net {who} stream {stream} to {to}: connect failed");
            self.outbox
                .push(ServerMsg::NetOpened { stream, ok: false })
                .await;
            return;
        };
        log::debug!("ds net {who} stream {stream} to {to}: connected");
        let _ = conn.set_nodelay(true);
        if !self
            .outbox
            .push(ServerMsg::NetOpened { stream, ok: true })
            .await
        {
            return;
        }
        let (read, write) = conn.into_split();
        let end = tokio::select! {
            end = self.upload(write, writes) => end,
            end = self.download(read, taken) => end,
        };
        match end {
            End::Far => {
                log::debug!("ds net {who} stream {stream} to {to}: far end closed");
                self.outbox.push(ServerMsg::NetClosed { stream }).await;
            }
            End::Ds => log::debug!("ds net {who} stream {stream} to {to}: closed by the ds"),
            End::Relay => log::debug!("ds net {who} stream {stream} to {to}: relay gone"),
        }
    }

    async fn upload(&self, mut write: OwnedWriteHalf, mut writes: mpsc::Receiver<Vec<u8>>) -> End {
        let mut total = 0_u64;
        while let Some(data) = writes.recv().await {
            for piece in data.chunks(PACE_CHUNK) {
                self.egress.pace(piece.len()).await;
                if write.write_all(piece).await.is_err() {
                    return End::Far;
                }
            }
            self.queued.fetch_sub(data.len(), Ordering::Relaxed);
            total += data.len() as u64;
            let written = ServerMsg::NetWritten {
                stream: self.stream,
                total,
            };
            if !self.outbox.push(written).await {
                return End::Relay;
            }
        }
        End::Ds
    }

    async fn download(&self, mut read: OwnedReadHalf, mut taken: watch::Receiver<u64>) -> End {
        let window = NET_WINDOW_BYTES as u64;
        let mut buf = vec![0; MAX_NET_READ_BYTES];
        let mut sent = 0_u64;
        loop {
            let open = taken
                .wait_for(|taken| sent.saturating_sub(*taken) < window)
                .await
                .map(|taken| *taken);
            let Ok(acked) = open else {
                return End::Ds;
            };
            let room = (window - sent.saturating_sub(acked)) as usize;
            let want = room.min(buf.len());
            match read.read(&mut buf[..want]).await {
                Ok(0) | Err(_) => return End::Far,
                Ok(n) => {
                    if sent == 0 {
                        log::debug!(
                            "ds net {} stream {} to {}: first read {n} B",
                            self.who,
                            self.stream,
                            self.to
                        );
                    }
                    sent += n as u64;
                    self.read.store(sent, Ordering::Relaxed);
                    let data = buf[..n].to_vec();
                    let read = ServerMsg::NetRead {
                        stream: self.stream,
                        data,
                    };
                    if !self.outbox.push(read).await {
                        return End::Relay;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::Queued;

    fn rules(ports: usize) -> Rules {
        Rules {
            public_ip: None,
            private: true,
            ports,
            streams: 4,
            stun: None,
            account: 1,
        }
    }

    fn ports() -> Arc<Ports> {
        Arc::new(Ports::new(1 << 30, 1 << 20, Vec::new()))
    }

    fn net(ports: &Arc<Ports>) -> (Net, Outbox, mpsc::Receiver<Queued>) {
        let (outbox, rx) = Outbox::new(1 << 20);
        (Net::new(ports.clone(), 1 << 20, 1 << 16), outbox, rx)
    }

    fn addr(addr: SocketAddr) -> NetAddr {
        match addr {
            SocketAddr::V4(addr) => addr.into(),
            SocketAddr::V6(_) => panic!("expected IPv4"),
        }
    }

    async fn heard(rx: &mut mpsc::Receiver<Queued>) -> ServerMsg {
        let queued = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("a datagram back in time")
            .expect("an open outbox");
        ServerMsg::decode(&queued.bytes).unwrap()
    }

    async fn far_end(answers: bool) -> (SocketAddr, mpsc::UnboundedReceiver<u16>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = socket.local_addr().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut buf = [0; 2048];
            while let Ok((n, from)) = socket.recv_from(&mut buf).await {
                let _ = tx.send(from.port());
                if answers {
                    let _ = socket.send_to(&buf[..n], from).await;
                }
            }
        });
        (at, rx)
    }

    async fn stun_server() -> SocketAddr {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = socket.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0; 512];
            while let Ok((n, from)) = socket.recv_from(&mut buf).await {
                if n < 20 {
                    continue;
                }
                let mapped = from.port().wrapping_add(1) ^ (STUN_MAGIC >> 16) as u16;
                let mut reply = vec![0x01, 0x01, 0, 12];
                reply.extend_from_slice(&buf[4..20]);
                reply.extend_from_slice(&[0x00, 0x20, 0, 8, 0, 1]);
                reply.extend_from_slice(&mapped.to_be_bytes());
                reply.extend_from_slice(&[0x5e, 0x12, 0xa4, 0x43]);
                let _ = socket.send_to(&reply, from).await;
            }
        });
        at
    }

    #[tokio::test]
    async fn answered_dns_sockets_close_before_a_long_lived_one() {
        let ports = ports();
        let (mut net, outbox, mut rx) = net(&ports);
        let (master, mut master_saw) = far_end(true).await;
        let (dns, _dns_saw) = far_end(true).await;
        net.dns_port = dns.port();
        let rules = rules(8);

        assert!(
            net.send(&rules, &outbox, 6000, addr(master), b"hi".to_vec())
                .await
        );
        heard(&mut rx).await;
        let public = master_saw.recv().await.unwrap();
        for port in 7000..7020u16 {
            assert!(
                net.send(&rules, &outbox, port, addr(dns), b"q".to_vec())
                    .await
            );
            assert!(
                matches!(heard(&mut rx).await, ServerMsg::NetHeard { port: p, .. } if p == port)
            );
        }
        assert!(
            net.send(&rules, &outbox, 6000, addr(master), b"hi".to_vec())
                .await
        );
        heard(&mut rx).await;

        assert_eq!(master_saw.recv().await, Some(public));
        assert!(net.udp.len() <= 8);
    }

    #[tokio::test]
    async fn a_socket_that_keeps_sending_keeps_its_port_and_a_quiet_one_closes() {
        let ports = ports();
        let (mut net, outbox, _rx) = net(&ports);
        net.idle = Duration::from_secs(1);
        let (far, mut saw) = far_end(false).await;
        let rules = rules(8);

        let mut seen = Vec::new();
        for _ in 0..4 {
            assert!(
                net.send(&rules, &outbox, 5000, addr(far), b"up".to_vec())
                    .await
            );
            seen.push(saw.recv().await.unwrap());
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        assert!(seen.iter().all(|&port| port == seen[0]), "{seen:?}");
        assert!(!net.udp[&5000].reader.is_finished());

        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(net.udp[&5000].reader.is_finished());
        assert!(ports.owner(net.udp[&5000].public).is_none());
    }

    #[tokio::test]
    async fn a_hairpin_finds_the_owner_by_the_port_stun_saw() {
        let ports = ports();
        let (mut a, a_out, mut a_rx) = net(&ports);
        let (mut b, b_out, mut b_rx) = net(&ports);
        let public = Ipv4Addr::new(203, 0, 113, 7);
        let rules = Rules {
            public_ip: Some(public),
            stun: Some(stun_server().await.to_string()),
            ..rules(8)
        };
        let (far, mut saw) = far_end(false).await;

        assert!(
            b.send(&rules, &b_out, 5000, addr(far), b"out".to_vec())
                .await
        );
        let b_public = saw.recv().await.unwrap().wrapping_add(1);
        assert_eq!(b.udp[&5000].public, b_public);
        let to_b = NetAddr {
            ip: public.octets(),
            port: b_public,
        };
        assert!(
            !a.send(&rules, &a_out, 5001, to_b, b"unasked".to_vec())
                .await,
            "B never sent to A"
        );

        let a_udp = &a.udp[&5001];
        assert_ne!(
            a_udp.public,
            a_udp.socket.local_addr().unwrap().port(),
            "A's port as STUN saw it"
        );
        let to_a = NetAddr {
            ip: public.octets(),
            port: a_udp.public,
        };
        assert!(b.send(&rules, &b_out, 5000, to_a, b"hi a".to_vec()).await);
        assert_eq!(
            heard(&mut a_rx).await,
            ServerMsg::NetHeard {
                port: 5001,
                from: to_b,
                data: b"hi a".to_vec(),
            }
        );
        assert!(a.send(&rules, &a_out, 5001, to_b, b"hi b".to_vec()).await);
        assert_eq!(
            heard(&mut b_rx).await,
            ServerMsg::NetHeard {
                port: 5000,
                from: to_a,
                data: b"hi b".to_vec(),
            }
        );
    }

    #[test]
    fn a_stun_reply_gives_the_xor_mapped_port() {
        let id = [7u8; 12];
        let mut reply = vec![0x01, 0x01, 0, 12];
        reply.extend_from_slice(&STUN_MAGIC.to_be_bytes());
        reply.extend_from_slice(&id);
        reply.extend_from_slice(&[0x00, 0x20, 0, 8, 0, 1]);
        reply.extend_from_slice(&(40_000u16 ^ 0x2112).to_be_bytes());
        reply.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(stun_mapped_port(&reply, &id), Some(40_000));
        assert_eq!(stun_mapped_port(&reply, &[8u8; 12]), None);
        assert_eq!(stun_mapped_port(&reply[..19], &id), None);
    }

    #[tokio::test]
    async fn a_lease_is_unique_among_live_players_and_a_free_wanted_one_comes_back() {
        let ports = ports();
        let (mut a, a_box, _a_rx) = net(&ports);
        let (mut b, b_box, _b_rx) = net(&ports);
        let who = OnlineId::random();
        let first = a.lease(&a_box, who, None).unwrap();
        let second = b.lease(&b_box, OnlineId::random(), None).unwrap();
        assert!(on_lan(first) && on_lan(second));
        assert_ne!(first, second);
        assert_eq!(a.lease(&a_box, who, Some(second)), Some(first));
        b.finish();
        let (mut c, c_box, _c_rx) = net(&ports);
        assert_eq!(
            c.lease(&c_box, OnlineId::random(), Some(second)),
            Some(second)
        );
        let (mut d, d_box, _d_rx) = net(&ports);
        let fourth = d.lease(&d_box, OnlineId::random(), Some(first)).unwrap();
        assert!(fourth != first && fourth != second);
        let (mut e, e_box, _e_rx) = net(&ports);
        let gateway = Ipv4Addr::new(10, 77, 0, 1);
        let fifth = e.lease(&e_box, OnlineId::random(), Some(gateway)).unwrap();
        assert!(fifth != gateway && fifth != fourth);
    }

    #[tokio::test]
    async fn a_player_back_on_a_new_connection_takes_over_their_lease() {
        let ports = ports();
        let who = OnlineId::random();
        let (mut old, old_box, _old_rx) = net(&ports);
        let held = old.lease(&old_box, who, None).unwrap();
        let (mut new, new_box, _new_rx) = net(&ports);
        assert_eq!(new.lease(&new_box, who, Some(held)), Some(held));
        old.finish();
        let (mut other, other_box, _other_rx) = net(&ports);
        assert_ne!(
            other.lease(&other_box, OnlineId::random(), Some(held)),
            Some(held)
        );
        assert_eq!(ports.lessee(held).map(|lessee| lessee.net), Some(new.id));
    }

    #[tokio::test]
    async fn a_datagram_to_a_lease_reaches_its_player_from_the_sender_lease() {
        let ports = ports();
        let rules = rules(4);
        let (mut a, a_box, _a_rx) = net(&ports);
        let (mut b, b_box, mut b_rx) = net(&ports);
        let from = a.lease(&a_box, OnlineId::random(), None).unwrap();
        let to = b.lease(&b_box, OnlineId::random(), None).unwrap();
        let at = NetAddr {
            ip: to.octets(),
            port: 2000,
        };
        assert!(a.send(&rules, &a_box, 1000, at, b"hi".to_vec()).await);
        assert_eq!(
            heard(&mut b_rx).await,
            ServerMsg::NetHeard {
                port: 2000,
                from: NetAddr {
                    ip: from.octets(),
                    port: 1000,
                },
                data: b"hi".to_vec(),
            }
        );
        let gateway = NetAddr {
            ip: [10, 77, 0, 1],
            port: 2000,
        };
        assert!(!a.send(&rules, &a_box, 1000, gateway, b"gw".to_vec()).await);
        let broadcast = NetAddr {
            ip: [10, 77, 255, 255],
            port: 2000,
        };
        assert!(
            !a.send(&rules, &a_box, 1000, broadcast, b"all".to_vec())
                .await
        );
        b.finish();
        assert!(!a.send(&rules, &a_box, 1000, at, b"stale".to_vec()).await);
        let (mut c, c_box, _c_rx) = net(&ports);
        assert!(!c.send(&rules, &c_box, 3000, at, b"unleased".to_vec()).await);
    }

    #[tokio::test]
    async fn empty_datagrams_count_against_the_packet_and_byte_limits() {
        let ports = ports();
        let (outbox, _rx) = Outbox::new(1 << 20);
        let (far, _saw) = far_end(false).await;
        let rules = rules(8);

        let mut packets = Net::new(ports.clone(), 1 << 20, 10);
        let mut sent = 0;
        for _ in 0..100 {
            sent += usize::from(
                packets
                    .send(&rules, &outbox, 5000, addr(far), Vec::new())
                    .await,
            );
        }
        assert!((20..=22).contains(&sent), "{sent} sent at 10 per second");

        let overhead = DATAGRAM_OVERHEAD as u64;
        let mut bytes = Net::new(ports.clone(), 16 * overhead, 1 << 20);
        let mut sent = 0;
        for _ in 0..100 {
            sent += usize::from(
                bytes
                    .send(&rules, &outbox, 5000, addr(far), Vec::new())
                    .await,
            );
        }
        assert!(
            (32..=35).contains(&sent),
            "{sent} sent at 16 overheads per second"
        );
    }

    #[tokio::test]
    async fn one_account_shares_its_limit_across_its_connections() {
        let ports = Arc::new(Ports::new(1 << 30, 10, Vec::new()));
        let (far, _saw) = far_end(false).await;
        let (mut first, first_box, _first_rx) = net(&ports);
        let (mut second, second_box, _second_rx) = net(&ports);
        let rules = rules(8);
        let mut sent = 0;
        for _ in 0..50 {
            sent += usize::from(
                first
                    .send(&rules, &first_box, 5000, addr(far), b"a".to_vec())
                    .await,
            );
            sent += usize::from(
                second
                    .send(&rules, &second_box, 5000, addr(far), b"b".to_vec())
                    .await,
            );
        }
        assert!((20..=22).contains(&sent), "{sent} sent by one account");

        let (mut other, other_box, _other_rx) = net(&ports);
        let rules = Rules {
            account: 2,
            ..rules
        };
        assert!(
            other
                .send(&rules, &other_box, 5000, addr(far), b"c".to_vec())
                .await
        );
    }

    #[tokio::test]
    async fn an_unreachable_address_binds_no_socket() {
        let ports = ports();
        let (mut net, outbox, _rx) = net(&ports);
        let rules = Rules {
            private: false,
            ..rules(8)
        };
        let to = NetAddr {
            ip: [10, 0, 0, 1],
            port: 9,
        };
        assert!(!net.send(&rules, &outbox, 5000, to, b"x".to_vec()).await);
        assert!(net.udp.is_empty());
    }

    #[tokio::test]
    async fn the_servers_own_addresses_are_out_of_reach() {
        let own = Ipv4Addr::LOCALHOST;
        let (far, _saw) = far_end(true).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp = addr(listener.local_addr().unwrap());

        let blocked = Arc::new(Ports::new(1 << 30, 1 << 20, vec![own]));
        let (mut guarded, outbox, _rx) = net(&blocked);
        let to = addr(far);
        assert!(
            !guarded
                .send(&rules(8), &outbox, 5000, to, b"x".to_vec())
                .await
        );
        assert!(guarded.udp.is_empty());
        assert!(!guarded.open(&rules(8), &outbox, 1, tcp, OnlineId::random()));

        let open = ports();
        let (mut plain, outbox, _rx) = net(&open);
        let with_public = Rules {
            public_ip: Some(own),
            ..rules(8)
        };
        assert!(!plain.open(&with_public, &outbox, 1, tcp, OnlineId::random()));
        assert!(plain.open(&rules(8), &outbox, 2, tcp, OnlineId::random()));
    }

    #[tokio::test]
    async fn a_ds_port_hears_only_addresses_it_sent_to() {
        let ports = ports();
        let (mut net, outbox, mut rx) = net(&ports);
        let rules = rules(8);
        let elsewhere = NetAddr {
            ip: [10, 255, 255, 1],
            port: 9,
        };
        net.send(&rules, &outbox, 5000, elsewhere, b"out".to_vec())
            .await;
        let port = net.udp[&5000].socket.local_addr().unwrap().port();
        let stranger = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let at = addr(stranger.local_addr().unwrap());
        stranger
            .send_to(b"unasked", ("127.0.0.1", port))
            .await
            .unwrap();
        let quiet = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await;
        assert!(quiet.is_err(), "a stranger's datagram got through");

        assert!(net.send(&rules, &outbox, 5000, at, b"hi".to_vec()).await);
        stranger
            .send_to(b"answer", ("127.0.0.1", port))
            .await
            .unwrap();
        assert_eq!(
            heard(&mut rx).await,
            ServerMsg::NetHeard {
                port: 5000,
                from: at,
                data: b"answer".to_vec(),
            }
        );
    }

    #[tokio::test]
    async fn small_writes_to_a_stalled_far_end_queue_up_to_the_window() {
        let ports = ports();
        let (mut net, outbox, _rx) = net(&ports);
        let stalled = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let to = addr(stalled.local_addr().unwrap());
        assert!(net.open(&rules(8), &outbox, 1, to, OnlineId::random()));
        assert!(net.write(1, Vec::new()));
        let segment = 16;
        for n in 0..NET_WINDOW_BYTES / segment {
            assert!(
                net.write(1, vec![0x5C; segment]),
                "write {n} closed the stream"
            );
        }
        assert!(!net.write(1, vec![0x5C]), "a write past the window");
        assert!(net.tcp.is_empty());
    }

    #[tokio::test]
    async fn a_client_takes_no_more_than_the_stream_read() {
        let ports = ports();
        let (mut net, outbox, mut rx) = net(&ports);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let to = addr(listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut far, _) = listener.accept().await.unwrap();
            far.write_all(&vec![0xA5; 3 * NET_WINDOW_BYTES]).await.ok();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        assert!(net.open(&rules(8), &outbox, 1, to, OnlineId::random()));
        assert!(matches!(
            heard(&mut rx).await,
            ServerMsg::NetOpened { ok: true, .. }
        ));
        net.taken(1, u64::MAX);
        async fn read_until_quiet(rx: &mut mpsc::Receiver<Queued>) -> usize {
            let mut got = 0;
            let quiet = Duration::from_millis(300);
            while let Ok(Some(queued)) = tokio::time::timeout(quiet, rx.recv()).await {
                if let ServerMsg::NetRead { data, .. } = ServerMsg::decode(&queued.bytes).unwrap() {
                    got += data.len();
                }
            }
            got
        }
        assert_eq!(read_until_quiet(&mut rx).await, NET_WINDOW_BYTES);
        net.taken(1, u64::MAX);
        assert_eq!(read_until_quiet(&mut rx).await, NET_WINDOW_BYTES);
    }
}
