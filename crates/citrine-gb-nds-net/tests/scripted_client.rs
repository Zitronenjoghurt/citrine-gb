use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, UdpSocket};

use citrine_gb::nds::interrupt::Irq;
use citrine_gb::nds::spi::firmware::Firmware;
use citrine_gb::nds::wifi::air::{Air, Station};
use citrine_gb::nds::wifi::calibration::{self, Calibration, RadioType};
use citrine_gb::nds::wifi::frame::{self, Frame, HEADER_BYTES};
use citrine_gb::nds::wifi::registers::*;
use citrine_gb::nds::wifi::{IRQ_RX_DONE, IRQ_TX_DONE, Wifi};
use citrine_gb_nds_net::access_point::{self, AccessPoint, Membership};
use citrine_gb_nds_net::gateway::{self, Config, Gateway};
use citrine_gb_threads::Threads;
use smoltcp::iface::{Config as InterfaceConfig, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::{dhcpv4, dns, tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{
    DnsQueryType, EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Cidr,
};

const MAC: [u8; 6] = [0x00, 0x09, 0xBF, 0x12, 0x34, 0x56];
const LOC1_AT: usize = 0x1200;
const TICK_US: u64 = 1_000;
const DWELL_US: u64 = 20_000;
const CLIENT_MTU: usize = 620;
const FETCH_BYTES: usize = 20_000;
const LOOPBACK_NAME: &str = "127.0.0.1";
const SNAP: [u8; 6] = [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00];

fn write_address(wifi: &mut Wifi, offset: usize, address: [u8; 6]) {
    for half in 0..3 {
        let value = u16::from_le_bytes([address[2 * half], address[2 * half + 1]]);
        wifi.write16(offset + 2 * half, value);
    }
}

fn radio_write(wifi: &mut Wifi, kind: RadioType, write: &calibration::RadioWrite) {
    let (low, high) = match kind {
        RadioType::Serial24 => (
            write.value as u16,
            (u16::from(write.index) << 2) | ((write.value >> 16) as u16 & 3),
        ),
        RadioType::Serial20 => (
            (u16::from(write.index) << 8) | (write.value as u16 & 0xFF),
            5,
        ),
    };
    wifi.write16(W_RF_DATA1, low);
    wifi.write16(W_RF_DATA2, high);
}

fn boot() -> (Wifi, Calibration) {
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
    for write in &calibration.radio_init {
        radio_write(&mut wifi, calibration.radio, write);
    }
    for (offset, value) in calibration::REGISTERS
        .into_iter()
        .zip(calibration.registers)
    {
        wifi.write16(offset, value);
    }
    write_address(&mut wifi, W_MACADDR, MAC);
    wifi.write16(W_IE, IRQ_RX_DONE | IRQ_TX_DONE);
    wifi.write16(W_MODE_RST, 1);
    wifi.write16(W_POWERFORCE, 0x8000);
    wifi.write16(W_POWERFORCE, 0);
    wifi.write16(W_US_COUNTCNT, 1);
    wifi.write16(W_RXFILTER, 0x0FFF);
    wifi.write16(W_RXCNT, 0x8001);
    (wifi, calibration)
}

fn put(wifi: &mut Wifi, at: usize, frame: &[u8]) {
    let mut bytes = vec![0u8; 12];
    bytes[8] = 0x14;
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

fn drain(wifi: &mut Wifi) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let end = 0x0800;
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
        let mut next = start + 12 + length.next_multiple_of(4);
        if next >= end {
            next -= end;
        }
        wifi.write16(W_RXBUF_READCSR, (next / 2) as u16);
        frames.push(bytes);
    }
    frames
}

fn header(control: u16, first: [u8; 6], third: [u8; 6]) -> Vec<u8> {
    let mut bytes = control.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&first);
    bytes.extend_from_slice(&MAC);
    bytes.extend_from_slice(&third);
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn ssid_of(body: &[u8]) -> Option<(&[u8], u8)> {
    let mut at = 12;
    let mut ssid = None;
    let mut channel = None;
    while at + 2 <= body.len() {
        let (id, length) = (body[at], usize::from(body[at + 1]));
        let value = body.get(at + 2..at + 2 + length)?;
        match id {
            0 => ssid = Some(value),
            3 => channel = value.first().copied(),
            _ => {}
        }
        at += 2 + length;
    }
    Some((ssid?, channel?))
}

#[derive(Default)]
struct Wire {
    inbound: VecDeque<Vec<u8>>,
    outbound: VecDeque<Vec<u8>>,
}

struct Incoming(Vec<u8>);

struct Outgoing<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::RxToken for Incoming {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl phy::TxToken for Outgoing<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
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
        capabilities.max_transmission_unit = CLIENT_MTU;
        capabilities
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Scanning(u8),
    Authenticating,
    Associating,
    Online,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Results {
    found: Vec<u8>,
    aid: Option<u16>,
    address: Option<Ipv4Cidr>,
    resolved: Option<IpAddress>,
    dns_error: Option<String>,
    echo: Option<Vec<u8>>,
    fetched: Vec<u8>,
    fetch_closed: bool,
}

struct Ds {
    chip: Wifi,
    calibration: Calibration,
    us: u64,
    next_tick: u64,
    phase: Phase,
    scan_until: u64,
    network: Option<([u8; 6], u8)>,
    queue: VecDeque<Vec<u8>>,
    sending: bool,
    wire: Wire,
    iface: Interface,
    sockets: SocketSet<'static>,
    dhcp: SocketHandle,
    dns: Option<(SocketHandle, dns::QueryHandle)>,
    udp: SocketHandle,
    tcp: SocketHandle,
    udp_port: u16,
    tcp_port: u16,
    udp_sent: bool,
    tcp_started: bool,
    requested: bool,
    results: Results,
}

impl Ds {
    fn new(udp_port: u16, tcp_port: u16) -> Self {
        let (chip, calibration) = boot();
        let mut wire = Wire::default();
        let mut setup = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(MAC)));
        setup.random_seed = 7;
        let iface = Interface::new(setup, &mut wire, Instant::from_micros(0));
        let mut sockets = SocketSet::new(Vec::new());
        let dhcp = sockets.add(dhcpv4::Socket::new());
        let mut udp = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]),
        );
        udp.bind(40_000).unwrap();
        let udp = sockets.add(udp);
        let tcp = sockets.add(tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; 8192]),
            tcp::SocketBuffer::new(vec![0; 2048]),
        ));
        Self {
            chip,
            calibration,
            us: 0,
            next_tick: 0,
            phase: Phase::Scanning(0),
            scan_until: 0,
            network: None,
            queue: VecDeque::new(),
            sending: false,
            wire,
            iface,
            sockets,
            dhcp,
            dns: None,
            udp,
            tcp,
            udp_port,
            tcp_port,
            udp_sent: false,
            tcp_started: false,
            requested: false,
            results: Results::default(),
        }
    }

    fn done(&self) -> bool {
        (self.results.fetch_closed && self.results.echo.is_some())
            || self.results.dns_error.is_some()
    }

    fn tune(&mut self, channel: u8) {
        for write in self.calibration.channels[usize::from(channel) - 1] {
            radio_write(&mut self.chip, self.calibration.radio, &write);
        }
    }

    fn send(&mut self, bytes: Vec<u8>) {
        self.queue.push_back(bytes);
        self.next_send();
    }

    fn next_send(&mut self) {
        if self.sending {
            return;
        }
        if let Some(bytes) = self.queue.pop_front() {
            self.sending = true;
            put(&mut self.chip, LOC1_AT, &bytes);
            self.chip.write16(W_TXREQ_SET, 1);
            self.chip
                .write16(W_TXBUF_LOC1, 0x8000 | (LOC1_AT / 2) as u16);
        }
    }

    fn react(&mut self, irq: Irq) {
        if irq.is_empty() {
            return;
        }
        let flags = self.chip.read16(W_IF);
        self.chip.write16(W_IF, flags);
        if flags & IRQ_TX_DONE != 0 {
            self.sending = false;
            self.next_send();
        }
        if flags & IRQ_RX_DONE != 0 {
            for bytes in drain(&mut self.chip) {
                self.heard(&bytes);
            }
        }
    }

    fn heard(&mut self, bytes: &[u8]) {
        if bytes.len() < HEADER_BYTES {
            return;
        }
        let control = frame::half(bytes, 0);
        let body = &bytes[HEADER_BYTES..];
        let third: [u8; 6] = bytes[16..22].try_into().unwrap();
        match (self.phase, control) {
            (Phase::Scanning(channel), 0x0050 | 0x0080) => {
                if let Some((ssid, on)) = ssid_of(body)
                    && ssid == access_point::SSID
                    && on == channel
                {
                    if !self.results.found.contains(&channel) {
                        self.results.found.push(channel);
                    }
                    self.network = Some((third, channel));
                }
            }
            (Phase::Authenticating, 0x00B0) if body[..6] == [0, 0, 2, 0, 0, 0] => {
                let (bssid, _) = self.network.unwrap();
                let mut request = header(0x0000, bssid, bssid);
                request.extend_from_slice(&[0x21, 0, 0x0A, 0]);
                request.extend_from_slice(&[0, access_point::SSID.len() as u8]);
                request.extend_from_slice(access_point::SSID);
                request.extend_from_slice(&[1, 2, 0x82, 0x84]);
                self.phase = Phase::Associating;
                self.send(request);
            }
            (Phase::Associating, 0x0010) if frame::half(body, 2) == 0 => {
                let aid = frame::half(body, 4) & 0x07FF;
                self.results.aid = Some(aid);
                self.chip.write16(W_AID_FULL, aid);
                self.phase = Phase::Online;
            }
            (Phase::Online, 0x0208) if body.len() >= 8 && body[..6] == SNAP => {
                let mut ethernet = bytes[4..10].to_vec();
                ethernet.extend_from_slice(&third);
                ethernet.extend_from_slice(&body[6..]);
                self.wire.inbound.push_back(ethernet);
            }
            _ => {}
        }
    }

    fn probe(&mut self) {
        let mut request = header(0x0040, [0xFF; 6], [0xFF; 6]);
        request.extend_from_slice(&[0, 0, 1, 2, 0x82, 0x84]);
        self.send(request);
    }

    fn tick(&mut self) {
        match self.phase {
            Phase::Scanning(channel) if self.us >= self.scan_until => {
                if channel == 13 {
                    let (bssid, on) = self.network.expect("no network found");
                    self.tune(on);
                    write_address(&mut self.chip, W_BSSID, bssid);
                    let mut request = header(0x00B0, bssid, bssid);
                    request.extend_from_slice(&[0, 0, 1, 0, 0, 0]);
                    self.phase = Phase::Authenticating;
                    self.send(request);
                } else {
                    self.phase = Phase::Scanning(channel + 1);
                    self.tune(channel + 1);
                    self.scan_until = self.us + DWELL_US;
                    self.probe();
                }
            }
            Phase::Online => self.online(),
            _ => {}
        }
    }

    fn online(&mut self) {
        let now = Instant::from_micros(self.us as i64);
        self.iface.poll(now, &mut self.wire, &mut self.sockets);
        if let Some(dhcpv4::Event::Configured(config)) =
            self.sockets.get_mut::<dhcpv4::Socket>(self.dhcp).poll()
        {
            let router = config.router.unwrap();
            let server = config.dns_servers[0];
            self.results.address = Some(config.address);
            self.iface.update_ip_addrs(|addresses| {
                addresses.clear();
                addresses.push(IpCidr::Ipv4(config.address)).unwrap();
            });
            self.iface
                .routes_mut()
                .add_default_ipv4_route(router)
                .unwrap();
            let mut resolver = dns::Socket::new(&[IpAddress::Ipv4(server)], vec![]);
            let query = resolver
                .start_query(self.iface.context(), LOOPBACK_NAME, DnsQueryType::A)
                .unwrap();
            self.dns = Some((self.sockets.add(resolver), query));
        }
        if self.results.address.is_none() {
            self.flush();
            return;
        }
        let gateway = IpAddress::Ipv4(gateway::Config::default().address);
        if !self.udp_sent {
            self.udp_sent = true;
            let socket = self.sockets.get_mut::<udp::Socket>(self.udp);
            socket
                .send_slice(b"ping", IpEndpoint::new(gateway, self.udp_port))
                .unwrap();
        }
        let socket = self.sockets.get_mut::<udp::Socket>(self.udp);
        if let Ok((data, _)) = socket.recv() {
            self.results.echo = Some(data.to_vec());
        }
        if let Some((handle, query)) = self.dns {
            match self
                .sockets
                .get_mut::<dns::Socket>(handle)
                .get_query_result(query)
            {
                Ok(found) => {
                    self.dns = None;
                    self.results.resolved = found.first().copied();
                }
                Err(dns::GetQueryResultError::Pending) => {}
                Err(error) => {
                    self.dns = None;
                    self.results.dns_error = Some(format!("{error:?}"));
                }
            }
        }
        if let Some(resolved) = self.results.resolved
            && !self.tcp_started
        {
            self.tcp_started = true;
            let context = self.iface.context();
            let socket = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            socket
                .connect(context, IpEndpoint::new(resolved, self.tcp_port), 41_000)
                .unwrap();
        }
        let socket = self.sockets.get_mut::<tcp::Socket>(self.tcp);
        if socket.may_send() && !self.requested {
            self.requested = true;
            socket.send_slice(b"GET / HTTP/1.0\r\n\r\n").unwrap();
        }
        while socket.can_recv() {
            socket
                .recv(|data| {
                    self.results.fetched.extend_from_slice(data);
                    (data.len(), ())
                })
                .unwrap();
        }
        if self.tcp_started && !socket.may_recv() && socket.state() != tcp::State::SynSent {
            if !self.results.fetched.is_empty() {
                self.results.fetch_closed = true;
            }
            socket.close();
        }
        self.flush();
    }

    fn flush(&mut self) {
        let now = Instant::from_micros(self.us as i64);
        self.iface.poll(now, &mut self.wire, &mut self.sockets);
        let (bssid, _) = self.network.unwrap();
        while let Some(ethernet) = self.wire.outbound.pop_front() {
            let destination: [u8; 6] = ethernet[..6].try_into().unwrap();
            let mut data = header(0x0108, bssid, destination);
            data.extend_from_slice(&SNAP);
            data.extend_from_slice(&ethernet[12..]);
            self.send(data);
        }
    }
}

impl Station for Ds {
    fn now_us(&self) -> u64 {
        self.us
    }

    fn run_until(&mut self, us: u64) {
        while self.us < us {
            let left = us - self.us;
            let wait = [
                self.chip.until_event_us(),
                Some(self.next_tick.saturating_sub(self.us)),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(left)
            .clamp(1, left);
            let irq = self.chip.step_us(wait);
            self.us += wait;
            self.react(irq);
            if self.us >= self.next_tick {
                self.next_tick = self.us + TICK_US;
                self.tick();
            }
        }
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

fn servers(threads: &Threads) -> (u16, u16, Vec<u8>) {
    let spawner = threads.spawner();
    let echo = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let udp_port = echo.local_addr().unwrap().port();
    spawner.thread("udp echo", move || {
        let mut buffer = [0; 1500];
        while let Ok((length, from)) = echo.recv_from(&mut buffer) {
            let mut answer = b"pong:".to_vec();
            answer.extend_from_slice(&buffer[..length]);
            echo.send_to(&answer, from).ok();
        }
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let tcp_port = listener.local_addr().unwrap().port();
    let page: Vec<u8> = (0..FETCH_BYTES).map(|index| (index % 251) as u8).collect();
    let served = page.clone();
    spawner.thread("tcp page", move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0; 64];
            let mut read = 0;
            while !request[..read].ends_with(b"\r\n\r\n") {
                match stream.read(&mut request[read..]) {
                    Ok(0) | Err(_) => return,
                    Ok(length) => read += length,
                }
            }
            stream.write_all(&served).ok();
        }
    });
    (udp_port, tcp_port, page)
}

#[test]
fn a_scripted_ds_scans_joins_leases_an_address_and_reaches_local_servers() {
    let threads = Threads::default();
    let (udp_port, tcp_port, page) = servers(&threads);
    let config = Config {
        host_loopback: true,
        ..Config::default()
    };
    let mut ap = AccessPoint::new(Gateway::new(config, threads.spawner()));
    let mut ds = Ds::new(udp_port, tcp_port);
    let mut air = Air::new(2);
    let mut until = 0;
    while !ds.done() && until < 60_000_000 {
        until += 5_000;
        air.run_stations(&mut [&mut ds, &mut ap], until);
        std::thread::sleep(std::time::Duration::from_micros(500));
    }
    assert_eq!(ds.results.found, [access_point::CHANNEL]);
    assert_eq!(ds.results.aid, Some(1));
    assert_eq!(ap.membership(MAC), Some(Membership::Associated(1)));
    let lease = Ipv4Addr::new(10, 77, 0, 2);
    assert_eq!(ds.results.address, Some(Ipv4Cidr::new(lease, 16)));
    assert_eq!(ap.gateway().lease(MAC), Some(lease));
    let host = std::net::ToSocketAddrs::to_socket_addrs(&(LOOPBACK_NAME, 0)).map(Vec::from_iter);
    assert_eq!(
        ds.results.dns_error, None,
        "the host resolves {LOOPBACK_NAME} to {host:?}"
    );
    assert_eq!(ds.results.resolved, Some(IpAddress::Ipv4(config.address)));
    assert_eq!(ds.results.echo.as_deref(), Some(&b"pong:ping"[..]));
    assert_eq!(ds.results.fetched.len(), page.len());
    assert!(ds.results.fetched == page);
    assert!(ds.results.fetch_closed);
}
