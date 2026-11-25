use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use citrine_gb_threads::Spawner;
use smoltcp::iface::{Config as InterfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    ArpOperation, ArpPacket, DhcpMessageType, DhcpOption, DhcpPacket, DhcpRepr, EthernetAddress,
    EthernetFrame, EthernetProtocol, HardwareAddress, IpAddress, IpCidr, IpEndpoint,
    IpListenEndpoint, IpProtocol, Ipv4Packet, TcpPacket, UdpPacket,
};

use crate::dns::Query;
use crate::relay::{NetIn, NetOut, Relay, WINDOW_BYTES};
use crate::services::{self, Services};

pub const MAC: [u8; 6] = [0x02, 0x09, 0xBF, 0xC1, 0x7B, 0x02];

const MTU: usize = 1514;
const TCP_BUFFER: usize = 64 * 1024;
const UDP_PACKETS: usize = 32;
const MAX_UDP_PEERS: usize = 32;
const MAX_UDP_PORTS: usize = 64;
const MAX_UDP_TARGETS: usize = 64;
const UDP_BUFFER: usize = 64 * 1024;
const DHCP_SERVER: u16 = 67;
const DHCP_CLIENT: u16 = 68;
const DNS: u16 = 53;
const LEASE_SECONDS: u32 = 86_400;
const OPTION_DNS: u8 = 6;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_IDLE_US: u64 = 60_000_000;
const LEASE_WAIT_US: u64 = 3_000_000;
const RELAY_CHUNK: usize = 16 * 1024;
const TAKEN_STEP: u64 = 4 * 1024;
const TRACE_LIMIT: usize = 4096;
const ALIASES: &[(&str, &str)] = &[("gamestats2.gs.nintendowifi.net", "pkmnclassic.net")];

pub type Resolver = Arc<dyn Fn(&str) -> Vec<Ipv4Addr> + Send + Sync>;

fn system_resolver() -> Resolver {
    Arc::new(|name| {
        (name, 0)
            .to_socket_addrs()
            .map(|addresses| {
                addresses
                    .filter_map(|address| match address.ip() {
                        std::net::IpAddr::V4(v4) => Some(v4),
                        std::net::IpAddr::V6(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn alias_of(name: &str) -> Option<&'static str> {
    ALIASES
        .iter()
        .find(|(from, _)| from.eq_ignore_ascii_case(name))
        .map(|&(_, to)| to)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub first_lease: Ipv4Addr,
    pub host_loopback: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: Ipv4Addr::new(10, 77, 0, 1),
            prefix: 16,
            first_lease: Ipv4Addr::new(10, 77, 0, 2),
            host_loopback: false,
        }
    }
}

impl Config {
    fn subnet_mask(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::MAX << (32 - u32::from(self.prefix)))
    }

    fn on_subnet(&self, address: Ipv4Addr) -> bool {
        let mask = u32::from(self.subnet_mask());
        u32::from(address) & mask == u32::from(self.address) & mask
    }

    fn host_on_subnet(&self, address: Ipv4Addr) -> bool {
        let host = u32::from(address) & !u32::from(self.subnet_mask());
        self.on_subnet(address) && host != 0 && host != !u32::from(self.subnet_mask())
    }

    fn upstream(&self, address: Ipv4Addr) -> Option<Ipv4Addr> {
        if address == self.address {
            self.host_loopback.then_some(Ipv4Addr::LOCALHOST)
        } else if self.on_subnet(address)
            || address.is_broadcast()
            || address.is_multicast()
            || address.is_unspecified()
        {
            None
        } else {
            Some(address)
        }
    }

    fn seen_from_ds(&self, address: Ipv4Addr) -> Option<Ipv4Addr> {
        if address.is_loopback() {
            self.host_loopback.then_some(self.address)
        } else {
            Some(address)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lan {
    Unasked,
    Asked(Instant),
    Leased(Ipv4Addr),
    Refused,
}

fn lan_peer(config: &Config, lan: Lan, address: Ipv4Addr) -> bool {
    let [_, _, c, d] = address.octets();
    matches!(lan, Lan::Leased(own) if own != address)
        && config.host_on_subnet(address)
        && (1..=254).contains(&c)
        && (1..=254).contains(&d)
}

#[derive(Default)]
struct Wire {
    inbound: VecDeque<Vec<u8>>,
    outbound: VecDeque<Vec<u8>>,
}

struct Incoming(Vec<u8>);

struct Outgoing<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::RxToken for Incoming {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

impl phy::TxToken for Outgoing<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0; len];
        let result = f(&mut buffer);
        self.0.push_back(buffer);
        result
    }
}

impl Device for Wire {
    type RxToken<'a> = Incoming;
    type TxToken<'a> = Outgoing<'a>;

    fn receive(&mut self, _: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.inbound.pop_front()?;
        Some((Incoming(frame), Outgoing(&mut self.outbound)))
    }

    fn transmit(&mut self, _: Instant) -> Option<Self::TxToken<'_>> {
        Some(Outgoing(&mut self.outbound))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = MTU;
        capabilities
    }
}

enum Upstream {
    Connecting(Receiver<std::io::Result<TcpStream>>),
    Open(TcpStream),
    Local(u64),
    Relay { stream: u32, open: bool },
    Gone,
}

struct TcpFlow {
    handle: SocketHandle,
    from: IpEndpoint,
    to: IpEndpoint,
    host: SocketAddrV4,
    told: bool,
    connected: bool,
    accepted: bool,
    state: tcp::State,
    segments: Segments,
    up: u64,
    relayed: u64,
    down: u64,
    upstream: Upstream,
    to_host: Vec<u8>,
    from_host: Vec<u8>,
    written: u64,
    taken_told: u64,
    host_done: bool,
    ds_done: bool,
    relay_done: bool,
}

enum PortSocket {
    Direct(UdpSocket),
    Relay(VecDeque<(SocketAddrV4, Vec<u8>)>),
}

struct UdpPort {
    from: IpEndpoint,
    socket: PortSocket,
    peers: Vec<UdpPeer>,
    used: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UdpPeer {
    pub ds_port: u16,
    pub peer: SocketAddrV4,
    pub up: u32,
    pub down: u32,
}

impl UdpPort {
    fn peer(&mut self, peer: SocketAddrV4) -> Option<&mut UdpPeer> {
        let at = match self.peers.iter().position(|known| known.peer == peer) {
            Some(at) => at,
            None if self.peers.len() < MAX_UDP_PEERS => {
                self.peers.push(UdpPeer {
                    ds_port: self.from.port,
                    peer,
                    up: 0,
                    down: 0,
                });
                self.peers.len() - 1
            }
            None => return None,
        };
        self.peers.get_mut(at)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trace {
    Datagram {
        ds_port: u16,
        peer: SocketAddrV4,
        up: bool,
        data: Vec<u8>,
    },
    TcpOpen {
        ds_port: u16,
        to: SocketAddrV4,
    },
    TcpOpened {
        ds_port: u16,
        to: SocketAddrV4,
        ok: bool,
    },
    TcpState {
        ds_port: u16,
        to: SocketAddrV4,
        state: tcp::State,
        segments: Segments,
    },
    TcpClosed {
        ds_port: u16,
        to: SocketAddrV4,
        up: u64,
        relayed: u64,
        down: u64,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segments {
    pub syn: u32,
    pub ack: u32,
    pub rst: u32,
    pub fin: u32,
    pub data: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TcpPeer {
    pub ds_port: u16,
    pub to: SocketAddrV4,
    pub stream: Option<u32>,
    pub state: tcp::State,
    pub segments: Segments,
    pub up: u64,
    pub relayed: u64,
    pub down: u64,
}

fn record(trace: &mut Option<Vec<Trace>>, item: impl FnOnce() -> Trace) {
    if let Some(trace) = trace
        && trace.len() < TRACE_LIMIT
    {
        trace.push(item());
    }
}

struct UdpTarget {
    handle: SocketHandle,
    to: IpEndpoint,
    used: Instant,
}

struct Lookup {
    query: Query,
    from: IpEndpoint,
    server: Option<IpEndpoint>,
    answer: Receiver<Vec<Ipv4Addr>>,
}

pub struct Gateway {
    config: Config,
    iface: Interface,
    wire: Wire,
    sockets: SocketSet<'static>,
    dhcp: SocketHandle,
    dns: SocketHandle,
    leases: Vec<(EthernetAddress, Ipv4Addr)>,
    lan: Lan,
    held_dhcp: Option<Vec<u8>>,
    tcp: Vec<TcpFlow>,
    udp: Vec<UdpTarget>,
    ports: Vec<UdpPort>,
    lookups: Vec<Lookup>,
    resolver: Resolver,
    spawner: Spawner,
    services: Option<Services>,
    relay: Option<Relay>,
    next_stream: u32,
    dhcp_replies: u64,
    nintendo_lookups: u64,
    now: Instant,
    trace: Option<Vec<Trace>>,
}

impl Gateway {
    pub fn new(config: Config, spawner: Spawner) -> Self {
        let mut wire = Wire::default();
        let mut setup = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(MAC)));
        setup.random_seed = 0x4E44_5357;
        let now = Instant::from_micros(0);
        let mut iface = Interface::new(setup, &mut wire, now);
        iface.update_ip_addrs(|addresses| {
            addresses
                .push(IpCidr::new(IpAddress::Ipv4(config.address), config.prefix))
                .ok();
        });
        iface.set_any_ip(true);
        let mut sockets = SocketSet::new(Vec::new());
        let mut dhcp = udp_socket();
        dhcp.bind(DHCP_SERVER).ok();
        let dhcp = sockets.add(dhcp);
        let mut dns = udp_socket();
        dns.bind((IpAddress::Ipv4(config.address), DNS)).ok();
        let dns = sockets.add(dns);
        Self {
            config,
            iface,
            wire,
            sockets,
            dhcp,
            dns,
            leases: Vec::new(),
            lan: Lan::Unasked,
            held_dhcp: None,
            tcp: Vec::new(),
            udp: Vec::new(),
            ports: Vec::new(),
            lookups: Vec::new(),
            resolver: system_resolver(),
            spawner,
            services: None,
            relay: None,
            next_stream: 0,
            dhcp_replies: 0,
            nintendo_lookups: 0,
            now,
            trace: None,
        }
    }

    pub fn with_services(mut self, services: Services) -> Self {
        self.services = Some(services);
        self
    }

    pub fn services_mut(&mut self) -> Option<&mut Services> {
        self.services.as_mut()
    }

    pub fn with_relay(mut self, relay: Relay) -> Self {
        self.relay = Some(relay);
        self
    }

    pub fn relayed(&self) -> bool {
        self.relay.is_some()
    }

    pub fn with_resolver(mut self, resolver: Resolver) -> Self {
        self.resolver = resolver;
        self
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn lease(&self, mac: [u8; 6]) -> Option<Ipv4Addr> {
        self.leases
            .iter()
            .find(|(owner, _)| owner.0 == mac)
            .map(|&(_, address)| address)
    }

    pub fn dhcp_replies(&self) -> u64 {
        self.dhcp_replies
    }

    pub fn nintendo_lookups(&self) -> u64 {
        self.nintendo_lookups
    }

    pub fn flows(&self) -> usize {
        self.tcp.len() + self.ports.len()
    }

    pub fn udp_sockets(&self) -> usize {
        self.udp.len()
    }

    pub fn udp_peers(&self) -> Vec<UdpPeer> {
        self.ports
            .iter()
            .flat_map(|port| port.peers.iter().copied())
            .collect()
    }

    pub fn tcp_peers(&self) -> Vec<TcpPeer> {
        self.tcp
            .iter()
            .map(|flow| TcpPeer {
                ds_port: flow.from.port,
                to: flow.host,
                stream: match flow.upstream {
                    Upstream::Relay { stream, .. } => Some(stream),
                    _ => None,
                },
                state: flow.state,
                segments: flow.segments,
                up: flow.up,
                relayed: flow.relayed,
                down: flow.down,
            })
            .collect()
    }

    pub fn trace(&mut self) {
        self.trace.get_or_insert_with(Vec::new);
    }

    pub fn take_trace(&mut self) -> Vec<Trace> {
        self.trace.as_mut().map(std::mem::take).unwrap_or_default()
    }

    pub fn send(&mut self, ethernet: Vec<u8>) {
        if self.admit(&ethernet) {
            self.wire.inbound.push_back(ethernet);
        }
    }

    pub fn take(&mut self) -> Option<Vec<u8>> {
        self.wire.outbound.pop_front()
    }

    pub fn poll(&mut self, us: u64) {
        self.now = Instant::from_micros(i64::try_from(us).unwrap_or(i64::MAX));
        self.iface.poll(self.now, &mut self.wire, &mut self.sockets);
        if let Some(services) = self.services.as_mut() {
            services.poll(us);
        }
        self.serve_dhcp();
        self.serve_dns();
        self.hear_relay();
        self.pump_tcp();
        self.pump_udp();
        self.answer_lookups();
        self.iface.poll(self.now, &mut self.wire, &mut self.sockets);
    }

    fn admit(&mut self, ethernet: &[u8]) -> bool {
        let Ok(frame) = EthernetFrame::new_checked(ethernet) else {
            return false;
        };
        match frame.ethertype() {
            EthernetProtocol::Arp => ArpPacket::new_checked(frame.payload()).is_ok_and(|arp| {
                let target = <[u8; 4]>::try_from(arp.target_protocol_addr())
                    .map(Ipv4Addr::from)
                    .ok();
                arp.operation() != ArpOperation::Request
                    || target == Some(self.config.address)
                    || target.is_some_and(|target| lan_peer(&self.config, self.lan, target))
            }),
            EthernetProtocol::Ipv4 => {
                let Ok(packet) = Ipv4Packet::new_checked(frame.payload()) else {
                    return false;
                };
                let from = packet.src_addr();
                let to = packet.dst_addr();
                match packet.next_header() {
                    IpProtocol::Tcp => {
                        let Ok(segment) = TcpPacket::new_checked(packet.payload()) else {
                            return false;
                        };
                        let from = IpEndpoint::new(from.into(), segment.src_port());
                        let to = IpEndpoint::new(to.into(), segment.dst_port());
                        if segment.syn() && !segment.ack() {
                            self.open_tcp(from, to);
                        }
                        if let Some(flow) = self
                            .tcp
                            .iter_mut()
                            .find(|flow| flow.from == from && flow.to == to)
                        {
                            let seen = &mut flow.segments;
                            seen.syn += u32::from(segment.syn());
                            seen.ack += u32::from(segment.ack());
                            seen.rst += u32::from(segment.rst());
                            seen.fin += u32::from(segment.fin());
                            seen.data += u32::from(!segment.payload().is_empty());
                        }
                        true
                    }
                    IpProtocol::Udp => {
                        let Ok(datagram) = UdpPacket::new_checked(packet.payload()) else {
                            return false;
                        };
                        let port = datagram.dst_port();
                        let local =
                            port == DHCP_SERVER || (to == self.config.address && port == DNS);
                        if !local {
                            self.open_udp(
                                IpEndpoint::new(from.into(), datagram.src_port()),
                                IpEndpoint::new(to.into(), port),
                            );
                        }
                        true
                    }
                    IpProtocol::Icmp => true,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn open_tcp(&mut self, from: IpEndpoint, to: IpEndpoint) {
        if self
            .tcp
            .iter()
            .any(|flow| flow.from == from && flow.to == to)
        {
            return;
        }
        let IpAddress::Ipv4(address) = to.addr;
        let mut far = SocketAddrV4::new(address, to.port);
        let upstream = if address == services::ADDRESS {
            let Some(services) = self.services.as_mut() else {
                return;
            };
            let Some(id) = services.open(to.port) else {
                return;
            };
            let IpAddress::Ipv4(peer) = from.addr;
            services.peer(id, SocketAddrV4::new(peer, from.port));
            Upstream::Local(id)
        } else {
            let Some(host) = self.config.upstream(address) else {
                return;
            };
            let target = SocketAddrV4::new(host, to.port);
            far = target;
            if let Some(relay) = &self.relay {
                let stream = self.next_stream;
                self.next_stream = self.next_stream.wrapping_add(1);
                if relay.out.send(NetOut::Open { stream, to: target }).is_err() {
                    return;
                }
                Upstream::Relay {
                    stream,
                    open: false,
                }
            } else {
                Self::connect(&self.spawner, SocketAddr::V4(target))
            }
        };
        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; TCP_BUFFER]),
            tcp::SocketBuffer::new(vec![0; TCP_BUFFER]),
        );
        if socket
            .listen(IpListenEndpoint {
                addr: Some(to.addr),
                port: to.port,
            })
            .is_err()
        {
            return;
        }
        record(&mut self.trace, || Trace::TcpOpen {
            ds_port: from.port,
            to: far,
        });
        self.tcp.push(TcpFlow {
            handle: self.sockets.add(socket),
            from,
            to,
            host: far,
            told: false,
            connected: false,
            accepted: false,
            state: tcp::State::Listen,
            segments: Segments::default(),
            up: 0,
            relayed: 0,
            down: 0,
            upstream,
            to_host: Vec::new(),
            from_host: Vec::new(),
            written: 0,
            taken_told: 0,
            host_done: false,
            ds_done: false,
            relay_done: false,
        });
    }

    fn connect(spawner: &Spawner, target: SocketAddr) -> Upstream {
        let (sender, receiver) = mpsc::channel();
        spawner.thread("wifi connect", move || {
            sender
                .send(TcpStream::connect_timeout(&target, CONNECT_TIMEOUT))
                .ok();
        });
        Upstream::Connecting(receiver)
    }

    fn open_udp(&mut self, from: IpEndpoint, to: IpEndpoint) {
        let IpAddress::Ipv4(address) = to.addr;
        if address == services::ADDRESS {
            if self.services.is_some() {
                udp_target(
                    &mut self.udp,
                    &mut self.sockets,
                    &self.lookups,
                    to,
                    self.now,
                );
            }
            return;
        }
        let lan = lan_peer(&self.config, self.lan, address);
        if (self.config.upstream(address).is_none() && !lan)
            || udp_target(
                &mut self.udp,
                &mut self.sockets,
                &self.lookups,
                to,
                self.now,
            )
            .is_none()
            || self.ports.iter().any(|port| port.from == from)
        {
            return;
        }
        let socket = if self.relay.is_some() {
            PortSocket::Relay(VecDeque::new())
        } else {
            let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
                return;
            };
            if socket.set_nonblocking(true).is_err() {
                return;
            }
            PortSocket::Direct(socket)
        };
        self.ports.push(UdpPort {
            from,
            socket,
            peers: Vec::new(),
            used: self.now,
        });
    }

    fn serve_dhcp(&mut self) {
        let mut requests: Vec<Vec<u8>> = self.held_dhcp.take().into_iter().collect();
        let socket = self.sockets.get_mut::<udp::Socket>(self.dhcp);
        while let Ok((data, _)) = socket.recv() {
            requests.push(data.to_vec());
        }
        if let Some(relay) = &self.relay
            && self.lan == Lan::Unasked
            && !requests.is_empty()
        {
            self.lan = match relay.out.send(NetOut::Lease) {
                Ok(()) => Lan::Asked(self.now),
                Err(_) => Lan::Refused,
            };
        }
        if let Lan::Asked(at) = self.lan {
            if (self.now - at).total_micros() < LEASE_WAIT_US {
                self.held_dhcp = requests.pop();
                return;
            }
            self.lan = Lan::Refused;
        }
        for request in requests {
            if let Some(reply) = self.dhcp_reply(&request) {
                let mut meta = udp::UdpMetadata::from(IpEndpoint::new(
                    IpAddress::Ipv4(Ipv4Addr::BROADCAST),
                    DHCP_CLIENT,
                ));
                meta.local_address = Some(IpAddress::Ipv4(self.config.address));
                let socket = self.sockets.get_mut::<udp::Socket>(self.dhcp);
                socket.send_slice(&reply, meta).ok();
                self.dhcp_replies += 1;
            }
        }
    }

    fn dhcp_reply(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        let packet = DhcpPacket::new_checked(request).ok()?;
        let asked = DhcpRepr::parse(&packet).ok()?;
        let mac = asked.client_hardware_address;
        let lease = self.lease_for(mac);
        let wanted = asked.requested_ip.unwrap_or(asked.client_ip);
        let message_type = match asked.message_type {
            DhcpMessageType::Discover => DhcpMessageType::Offer,
            DhcpMessageType::Request if wanted == lease => DhcpMessageType::Ack,
            DhcpMessageType::Request => DhcpMessageType::Nak,
            _ => return None,
        };
        let dns = self.config.address.octets();
        let options = [DhcpOption {
            kind: OPTION_DNS,
            data: &dns,
        }];
        let granted = message_type != DhcpMessageType::Nak;
        let reply = DhcpRepr {
            message_type,
            transaction_id: asked.transaction_id,
            secs: 0,
            client_hardware_address: mac,
            client_ip: Ipv4Addr::UNSPECIFIED,
            your_ip: if granted {
                lease
            } else {
                Ipv4Addr::UNSPECIFIED
            },
            server_ip: self.config.address,
            router: granted.then_some(self.config.address),
            subnet_mask: granted.then_some(self.config.subnet_mask()),
            relay_agent_ip: Ipv4Addr::UNSPECIFIED,
            broadcast: true,
            requested_ip: None,
            client_identifier: None,
            server_identifier: Some(self.config.address),
            parameter_request_list: None,
            dns_servers: None,
            max_size: None,
            lease_duration: granted.then_some(LEASE_SECONDS),
            renew_duration: None,
            rebind_duration: None,
            additional_options: if granted { &options } else { &[] },
        };
        let mut bytes = vec![0; reply.buffer_len()];
        reply
            .emit(&mut DhcpPacket::new_unchecked(&mut bytes))
            .ok()?;
        Some(bytes)
    }

    fn lease_for(&mut self, mac: EthernetAddress) -> Ipv4Addr {
        if let Some(&(_, address)) = self.leases.iter().find(|(owner, _)| *owner == mac) {
            return address;
        }
        let address = match self.lan {
            Lan::Leased(address) if self.leases.is_empty() => address,
            _ => {
                let first = u32::from(self.config.first_lease);
                Ipv4Addr::from(first + self.leases.len() as u32)
            }
        };
        self.leases.push((mac, address));
        address
    }

    fn serve_dns(&mut self) {
        let socket = self.sockets.get_mut::<udp::Socket>(self.dns);
        let mut asked = Vec::new();
        while let Ok((data, meta)) = socket.recv() {
            if let Some(query) = Query::parse(data) {
                asked.push((query, meta.endpoint));
            }
        }
        for (query, from) in asked {
            self.look_up(query, from, None);
        }
    }

    fn look_up(&mut self, query: Query, from: IpEndpoint, server: Option<IpEndpoint>) {
        let (sender, answer) = mpsc::channel();
        if let Some(services) = self.services.as_mut()
            && services::is_nintendo(&query.name)
        {
            self.nintendo_lookups += 1;
            let served = services::is_served(&query.name);
            services.note(format!(
                "dns {} {}",
                query.name,
                if served { "served" } else { "unknown" }
            ));
            let addresses = if served && query.wants_address {
                vec![services::ADDRESS]
            } else {
                Vec::new()
            };
            sender.send(addresses).ok();
            self.lookups.push(Lookup {
                query,
                from,
                server,
                answer,
            });
            return;
        }
        let name = alias_of(&query.name).map_or_else(|| query.name.clone(), str::to_owned);
        let wanted = query.wants_address;
        let config = self.config;
        let resolver = Arc::clone(&self.resolver);
        self.spawner.thread("wifi lookup", move || {
            let mut unique = Vec::new();
            if wanted {
                for address in resolver(&name)
                    .into_iter()
                    .filter_map(|address| config.seen_from_ds(address))
                {
                    if !unique.contains(&address) {
                        unique.push(address);
                    }
                }
            }
            sender.send(unique).ok();
        });
        self.lookups.push(Lookup {
            query,
            from,
            server,
            answer,
        });
    }

    fn answer_lookups(&mut self) {
        let mut answered = Vec::new();
        self.lookups
            .retain(|lookup| match lookup.answer.try_recv() {
                Ok(addresses) => {
                    answered.push((lookup.from, lookup.server, lookup.query.answer(&addresses)));
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => false,
            });
        for (to, server, bytes) in answered {
            let handle = match server {
                None => Some(self.dns),
                Some(server) => self
                    .udp
                    .iter()
                    .find(|target| target.to == server)
                    .map(|target| target.handle),
            };
            let Some(handle) = handle else {
                continue;
            };
            let mut meta = udp::UdpMetadata::from(to);
            meta.local_address = server.map(|server| server.addr);
            self.sockets
                .get_mut::<udp::Socket>(handle)
                .send_slice(&bytes, meta)
                .ok();
        }
    }

    fn hear_relay(&mut self) {
        let Some(relay) = &self.relay else {
            return;
        };
        while let Ok(heard) = relay.back.try_recv() {
            match heard {
                NetIn::Heard { port, from, data } => {
                    let found = self.ports.iter().position(|udp| udp.from.port == port);
                    let at = match (found, self.lan) {
                        (Some(at), _) => at,
                        (None, Lan::Leased(own))
                            if self.ports.len() < MAX_UDP_PORTS
                                && self.leases.iter().any(|(_, held)| *held == own) =>
                        {
                            self.ports.push(UdpPort {
                                from: IpEndpoint::new(IpAddress::Ipv4(own), port),
                                socket: PortSocket::Relay(VecDeque::new()),
                                peers: Vec::new(),
                                used: self.now,
                            });
                            self.ports.len() - 1
                        }
                        (None, _) => continue,
                    };
                    if let PortSocket::Relay(queue) = &mut self.ports[at].socket
                        && queue.len() < UDP_PACKETS
                    {
                        queue.push_back((from, data));
                    }
                }
                NetIn::Opened { stream, ok } => {
                    let Some(flow) = relay_flow(&mut self.tcp, stream) else {
                        continue;
                    };
                    if ok {
                        flow.upstream = Upstream::Relay { stream, open: true };
                    } else {
                        self.sockets.get_mut::<tcp::Socket>(flow.handle).abort();
                        flow.upstream = Upstream::Gone;
                    }
                }
                NetIn::Read { stream, data } => {
                    let Some(flow) = relay_flow(&mut self.tcp, stream) else {
                        continue;
                    };
                    if flow.from_host.len() + data.len() > WINDOW_BYTES {
                        self.sockets.get_mut::<tcp::Socket>(flow.handle).abort();
                        flow.upstream = Upstream::Gone;
                        relay.out.send(NetOut::Close { stream }).ok();
                        continue;
                    }
                    flow.relayed += data.len() as u64;
                    flow.from_host.extend_from_slice(&data);
                }
                NetIn::Written { stream, total } => {
                    if let Some(flow) = relay_flow(&mut self.tcp, stream) {
                        flow.written = flow.written.max(total.min(flow.up));
                    }
                }
                NetIn::Closed { stream } => {
                    if let Some(flow) = relay_flow(&mut self.tcp, stream) {
                        flow.relay_done = true;
                    }
                }
                NetIn::Leased { address } if matches!(self.lan, Lan::Asked(_)) => {
                    self.lan = address.map_or(Lan::Refused, Lan::Leased);
                }
                NetIn::Leased { .. } => {}
            }
        }
    }

    fn pump_tcp(&mut self) {
        for flow in &mut self.tcp {
            let socket = self.sockets.get_mut::<tcp::Socket>(flow.handle);
            match (&flow.upstream, self.services.as_mut(), &self.relay) {
                (Upstream::Local(id), Some(services), _) => pump_local(*id, flow, socket, services),
                (Upstream::Local(_), None, _) => {
                    socket.abort();
                    flow.upstream = Upstream::Gone;
                }
                (Upstream::Relay { .. }, _, Some(relay)) => pump_relay(flow, socket, relay),
                (Upstream::Relay { .. }, _, None) => {
                    socket.abort();
                    flow.upstream = Upstream::Gone;
                }
                _ => pump_flow(flow, socket),
            }
            let ok = match flow.upstream {
                Upstream::Open(_) | Upstream::Local(_) | Upstream::Relay { open: true, .. } => {
                    Some(true)
                }
                Upstream::Gone => Some(flow.connected),
                Upstream::Connecting(_) | Upstream::Relay { open: false, .. } => None,
            };
            let state = socket.state();
            if state != tcp::State::Listen {
                flow.accepted = true;
            }
            if state != flow.state {
                flow.state = state;
                record(&mut self.trace, || Trace::TcpState {
                    ds_port: flow.from.port,
                    to: flow.host,
                    state,
                    segments: flow.segments,
                });
            }
            if !flow.told
                && let Some(ok) = ok
            {
                flow.told = true;
                record(&mut self.trace, || Trace::TcpOpened {
                    ds_port: flow.from.port,
                    to: flow.host,
                    ok,
                });
            }
        }
        let sockets = &mut self.sockets;
        let services = &mut self.services;
        let relay = &self.relay;
        let trace = &mut self.trace;
        self.tcp.retain(|flow| {
            let socket = sockets.get::<tcp::Socket>(flow.handle);
            let state = socket.state();
            let over = (state == tcp::State::Closed && socket.remote_endpoint().is_none())
                || state == tcp::State::TimeWait
                || (state == tcp::State::Listen
                    && (flow.accepted || matches!(flow.upstream, Upstream::Gone)));
            if over {
                record(trace, || Trace::TcpClosed {
                    ds_port: flow.from.port,
                    to: flow.host,
                    up: flow.up,
                    relayed: flow.relayed,
                    down: flow.down,
                });
                sockets.remove(flow.handle);
                match (&flow.upstream, services.as_mut(), relay) {
                    (Upstream::Local(id), Some(services), _) => services.close(*id),
                    (Upstream::Relay { stream, .. }, _, Some(relay))
                        if !flow.relay_done && !flow.ds_done =>
                    {
                        relay.out.send(NetOut::Close { stream: *stream }).ok();
                    }
                    _ => {}
                }
            }
            !over
        });
    }

    fn pump_udp(&mut self) {
        let now = self.now;
        let mut aliased = Vec::new();
        for target in &mut self.udp {
            let socket = self.sockets.get_mut::<udp::Socket>(target.handle);
            let IpAddress::Ipv4(address) = target.to.addr;
            let host = if lan_peer(&self.config, self.lan, address) {
                Some(address)
            } else {
                self.config.upstream(address)
            };
            if address == services::ADDRESS {
                let mut replies = Vec::new();
                while let Ok((data, meta)) = socket.recv() {
                    if let Some(reply) = self.services.as_mut().and_then(|services| {
                        let IpAddress::Ipv4(ip) = meta.endpoint.addr;
                        let from = SocketAddrV4::new(ip, meta.endpoint.port);
                        services.datagram(target.to.port, from, data)
                    }) {
                        replies.push((reply, meta.endpoint));
                    }
                    target.used = now;
                }
                if let Some(services) = self.services.as_mut() {
                    for (to, data) in services.take_datagrams(target.to.port) {
                        let to = IpEndpoint::new(IpAddress::Ipv4(*to.ip()), to.port());
                        replies.push((data, to));
                    }
                }
                for (reply, to) in replies {
                    let mut meta = udp::UdpMetadata::from(to);
                    meta.local_address = Some(target.to.addr);
                    socket.send_slice(&reply, meta).ok();
                }
                continue;
            }
            while let Ok((data, meta)) = socket.recv() {
                if target.to.port == DNS
                    && let Some(query) = Query::parse(data)
                    && (alias_of(&query.name).is_some()
                        || (self.services.is_some() && services::is_nintendo(&query.name)))
                {
                    aliased.push((query, meta.endpoint, target.to));
                    continue;
                }
                let Some(host) = host else {
                    continue;
                };
                let Some(port) = self
                    .ports
                    .iter_mut()
                    .find(|port| port.from == meta.endpoint)
                else {
                    continue;
                };
                let to = SocketAddrV4::new(host, target.to.port);
                if to.port() != DNS {
                    record(&mut self.trace, || Trace::Datagram {
                        ds_port: port.from.port,
                        peer: to,
                        up: true,
                        data: data.to_vec(),
                    });
                }
                match (&port.socket, &self.relay) {
                    (PortSocket::Direct(socket), _) => {
                        socket.send_to(data, to).ok();
                    }
                    (PortSocket::Relay(_), Some(relay)) => {
                        let data = data.to_vec();
                        let port = port.from.port;
                        relay.out.send(NetOut::Send { port, to, data }).ok();
                    }
                    (PortSocket::Relay(_), None) => continue,
                }
                if let Some(peer) = port.peer(to) {
                    peer.up = peer.up.saturating_add(1);
                }
                port.used = now;
                target.used = now;
            }
        }
        let mut buffer = vec![0; UDP_BUFFER];
        for port in &mut self.ports {
            let mut arrived = Vec::new();
            match &mut port.socket {
                PortSocket::Direct(socket) => {
                    while let Ok((length, source)) = socket.recv_from(&mut buffer) {
                        if let SocketAddr::V4(source) = source {
                            arrived.push((source, buffer[..length].to_vec()));
                        }
                    }
                }
                PortSocket::Relay(queue) => arrived.extend(queue.drain(..)),
            }
            for (source, data) in arrived {
                if let Some(peer) = port.peer(source) {
                    peer.down = peer.down.saturating_add(1);
                }
                if source.port() != DNS {
                    record(&mut self.trace, || Trace::Datagram {
                        ds_port: port.from.port,
                        peer: source,
                        up: false,
                        data: data.clone(),
                    });
                }
                let Some(seen) = self.config.seen_from_ds(*source.ip()) else {
                    continue;
                };
                let to = IpEndpoint::new(IpAddress::Ipv4(seen), source.port());
                let Some(handle) =
                    udp_target(&mut self.udp, &mut self.sockets, &self.lookups, to, now)
                else {
                    continue;
                };
                let mut meta = udp::UdpMetadata::from(port.from);
                meta.local_address = Some(to.addr);
                self.sockets
                    .get_mut::<udp::Socket>(handle)
                    .send_slice(&data, meta)
                    .ok();
                port.used = now;
            }
        }
        self.ports
            .retain(|port| (now - port.used).total_micros() < UDP_IDLE_US);
        for (query, from, server) in aliased {
            self.look_up(query, from, Some(server));
        }
        let sockets = &mut self.sockets;
        let lookups = &self.lookups;
        self.udp.retain(|target| {
            let keep = (now - target.used).total_micros() < UDP_IDLE_US
                || lookups
                    .iter()
                    .any(|lookup| lookup.server == Some(target.to));
            if !keep {
                sockets.remove(target.handle);
            }
            keep
        });
    }
}

fn udp_target(
    targets: &mut Vec<UdpTarget>,
    sockets: &mut SocketSet<'static>,
    lookups: &[Lookup],
    to: IpEndpoint,
    now: Instant,
) -> Option<SocketHandle> {
    if let Some(target) = targets.iter_mut().find(|target| target.to == to) {
        target.used = now;
        return Some(target.handle);
    }
    if targets.len() >= MAX_UDP_TARGETS {
        let oldest = targets
            .iter()
            .enumerate()
            .filter(|(_, target)| {
                target.to.addr != IpAddress::Ipv4(services::ADDRESS)
                    && !lookups
                        .iter()
                        .any(|lookup| lookup.server == Some(target.to))
            })
            .min_by_key(|(_, target)| target.used)
            .map(|(at, _)| at)?;
        sockets.remove(targets.swap_remove(oldest).handle);
    }
    let mut socket = udp_socket();
    socket.bind(to).ok()?;
    let handle = sockets.add(socket);
    targets.push(UdpTarget {
        handle,
        to,
        used: now,
    });
    Some(handle)
}

fn pump_flow(flow: &mut TcpFlow, socket: &mut tcp::Socket) {
    if let Upstream::Connecting(receiver) = &flow.upstream {
        match receiver.try_recv() {
            Ok(Ok(stream)) if stream.set_nonblocking(true).is_ok() => {
                flow.connected = true;
                flow.upstream = Upstream::Open(stream);
            }
            Err(TryRecvError::Empty) => return,
            _ => {
                socket.abort();
                flow.upstream = Upstream::Gone;
                return;
            }
        }
    }
    let Upstream::Open(stream) = &mut flow.upstream else {
        return;
    };
    while socket.can_recv() {
        let taken = socket.recv(|data| {
            flow.to_host.extend_from_slice(data);
            flow.up += data.len() as u64;
            (data.len(), data.len())
        });
        if !matches!(taken, Ok(length) if length > 0) {
            break;
        }
    }
    if !flow.to_host.is_empty() {
        match stream.write(&flow.to_host) {
            Ok(written) => {
                flow.to_host.drain(..written);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => {
                socket.abort();
                flow.upstream = Upstream::Gone;
                return;
            }
        }
    }
    let mut buffer = [0; 4096];
    while !flow.host_done && socket.can_send() {
        let room = (socket.send_capacity() - socket.send_queue()).min(buffer.len());
        if room == 0 {
            break;
        }
        match stream.read(&mut buffer[..room]) {
            Ok(0) => {
                flow.host_done = true;
                socket.close();
            }
            Ok(length) => {
                let sent = socket.send_slice(&buffer[..length]).unwrap_or(0);
                flow.down += sent as u64;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(_) => {
                socket.abort();
                flow.upstream = Upstream::Gone;
                return;
            }
        }
    }
    let ds_closed = matches!(
        socket.state(),
        tcp::State::CloseWait | tcp::State::LastAck | tcp::State::Closing | tcp::State::TimeWait
    );
    if ds_closed && !flow.ds_done && flow.to_host.is_empty() {
        flow.ds_done = true;
        stream.shutdown(Shutdown::Write).ok();
    }
}

fn relay_flow(flows: &mut [TcpFlow], stream: u32) -> Option<&mut TcpFlow> {
    flows
        .iter_mut()
        .find(|flow| matches!(flow.upstream, Upstream::Relay { stream: id, .. } if id == stream))
}

fn pump_relay(flow: &mut TcpFlow, socket: &mut tcp::Socket, relay: &Relay) {
    let Upstream::Relay { stream, open } = flow.upstream else {
        return;
    };
    if !open {
        return;
    }
    while socket.can_recv() && flow.up - flow.written < WINDOW_BYTES as u64 {
        let room = WINDOW_BYTES - (flow.up - flow.written) as usize;
        let read = socket.recv(|data| {
            let length = data.len().min(RELAY_CHUNK).min(room);
            let data = data[..length].to_vec();
            relay.out.send(NetOut::Write { stream, data }).ok();
            flow.up += length as u64;
            (length, length)
        });
        if !matches!(read, Ok(length) if length > 0) {
            break;
        }
    }
    if !flow.from_host.is_empty() && socket.can_send() {
        let sent = socket.send_slice(&flow.from_host).unwrap_or(0);
        flow.from_host.drain(..sent);
        flow.down += sent as u64;
        if flow.down - flow.taken_told >= TAKEN_STEP
            || (flow.from_host.is_empty() && flow.down > flow.taken_told)
        {
            flow.taken_told = flow.down;
            let total = flow.down;
            relay.out.send(NetOut::Taken { stream, total }).ok();
        }
    }
    if flow.relay_done && flow.from_host.is_empty() && !flow.host_done {
        flow.host_done = true;
        socket.close();
    }
    if socket.state() == tcp::State::CloseWait && !flow.relay_done && !flow.ds_done {
        flow.ds_done = true;
        flow.host_done = true;
        relay.out.send(NetOut::Close { stream }).ok();
        socket.close();
    }
}

fn pump_local(id: u64, flow: &mut TcpFlow, socket: &mut tcp::Socket, services: &mut Services) {
    while socket.can_recv() {
        let mut taken = Vec::new();
        let read = socket.recv(|data| {
            taken.extend_from_slice(data);
            (data.len(), data.len())
        });
        if !matches!(read, Ok(length) if length > 0) {
            break;
        }
        flow.up += taken.len() as u64;
        services.input(id, &taken);
    }
    if socket.can_send() {
        let room = socket.send_capacity() - socket.send_queue();
        let out = services.take_output(id, room);
        if !out.is_empty() {
            let sent = socket.send_slice(&out).unwrap_or(0);
            flow.down += sent as u64;
        }
    }
    let ds_closed = matches!(
        socket.state(),
        tcp::State::CloseWait | tcp::State::LastAck | tcp::State::Closing | tcp::State::TimeWait
    );
    if !flow.host_done && (services.finished(id) || ds_closed) && socket.send_queue() == 0 {
        flow.host_done = true;
        socket.close();
    }
}

fn udp_socket() -> udp::Socket<'static> {
    udp::Socket::new(
        udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; UDP_PACKETS],
            vec![0; UDP_BUFFER],
        ),
        udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; UDP_PACKETS],
            vec![0; UDP_BUFFER],
        ),
    )
}
