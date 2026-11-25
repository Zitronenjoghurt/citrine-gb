mod common;

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant as Clock};

use citrine_gb_nds_net::gateway::{Config, Gateway};
use citrine_gb_threads::Threads;
use common::Client;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp;
use smoltcp::wire::{IpAddress, IpEndpoint};

const DS_PORT: u16 = 40_000;
const WAIT: Duration = Duration::from_secs(5);

struct Rig {
    gateway: Gateway,
    client: Client,
    ds: SocketHandle,
    address: IpAddress,
    _threads: Threads,
}

impl Rig {
    fn new() -> Self {
        let threads = Threads::default();
        let config = Config {
            host_loopback: true,
            ..Config::default()
        };
        let gateway = Gateway::new(config, threads.spawner());
        let mut client = Client::new(config.address);
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
        );
        socket.bind(DS_PORT).unwrap();
        let ds = client.sockets.add(socket);
        Self {
            gateway,
            client,
            ds,
            address: IpAddress::Ipv4(config.address),
            _threads: threads,
        }
    }

    fn send(&mut self, to: &UdpSocket, bytes: &[u8]) {
        let port = to.local_addr().unwrap().port();
        self.client
            .sockets
            .get_mut::<udp::Socket>(self.ds)
            .send_slice(bytes, IpEndpoint::new(self.address, port))
            .unwrap();
    }

    fn until<T>(&mut self, mut found: impl FnMut(&mut Self) -> Option<T>) -> Option<T> {
        let started = Clock::now();
        while started.elapsed() < WAIT {
            self.client.step(&mut self.gateway);
            if let Some(value) = found(self) {
                return Some(value);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    fn heard(&mut self) -> Option<(Vec<u8>, IpEndpoint)> {
        self.client
            .sockets
            .get_mut::<udp::Socket>(self.ds)
            .recv()
            .ok()
            .map(|(data, meta)| (data.to_vec(), meta.endpoint))
    }
}

fn remote(at: Ipv4Addr) -> Option<UdpSocket> {
    let socket = UdpSocket::bind((at, 0)).ok()?;
    socket.set_nonblocking(true).unwrap();
    Some(socket)
}

fn arrival(socket: &UdpSocket) -> Option<(Vec<u8>, SocketAddr)> {
    let mut buffer = [0; 512];
    socket
        .recv_from(&mut buffer)
        .ok()
        .map(|(length, from)| (buffer[..length].to_vec(), from))
}

fn mapped(rig: &mut Rig, server: &UdpSocket) -> SocketAddr {
    rig.send(server, b"hello");
    let (bytes, from) = rig
        .until(|_| arrival(server))
        .expect("the server hears the ds");
    assert_eq!(bytes, b"hello");
    from
}

#[test]
fn one_ds_port_reaches_every_remote_from_one_host_port() {
    let mut rig = Rig::new();
    let first = remote(Ipv4Addr::LOCALHOST).unwrap();
    let second = remote(Ipv4Addr::LOCALHOST).unwrap();
    let seen_by_first = mapped(&mut rig, &first);
    let seen_by_second = mapped(&mut rig, &second);
    assert_eq!(seen_by_first, seen_by_second);
}

#[test]
fn a_contacted_address_reaches_the_ds_from_a_port_it_never_sent_to() {
    let mut rig = Rig::new();
    let server = remote(Ipv4Addr::LOCALHOST).unwrap();
    let outside = mapped(&mut rig, &server);
    let peer = remote(Ipv4Addr::LOCALHOST).unwrap();
    peer.send_to(b"punch", outside).unwrap();
    let (bytes, from) = rig.until(Rig::heard).expect("the ds hears the peer");
    assert_eq!(bytes, b"punch");
    let peer_port = peer.local_addr().unwrap().port();
    assert_eq!(from, IpEndpoint::new(rig.address, peer_port));
}

#[test]
fn an_address_the_ds_never_sent_to_reaches_a_port_that_sent_out() {
    let mut rig = Rig::new();
    let server = remote(Ipv4Addr::LOCALHOST).unwrap();
    let Some(stranger) = remote(Ipv4Addr::new(127, 0, 0, 2)) else {
        return;
    };
    let outside = mapped(&mut rig, &server);
    stranger.send_to(b"stranger", outside).unwrap();
    server.send_to(b"known", outside).unwrap();
    let mut heard = Vec::new();
    rig.until(|rig| {
        let (bytes, _) = rig.heard()?;
        heard.push(bytes);
        (heard.len() == 2).then_some(())
    })
    .expect("the ds hears both");
    heard.sort();
    assert_eq!(heard, vec![b"known".to_vec(), b"stranger".to_vec()]);
}

#[test]
fn a_flood_of_sources_holds_a_bounded_number_of_sockets_and_the_server_gets_through() {
    let mut rig = Rig::new();
    let server = remote(Ipv4Addr::LOCALHOST).unwrap();
    let outside = mapped(&mut rig, &server);
    let flood: Vec<UdpSocket> = (0..300)
        .filter_map(|_| remote(Ipv4Addr::LOCALHOST))
        .collect();
    for socket in &flood {
        socket.send_to(b"flood", outside).unwrap();
    }
    for _ in 0..200 {
        rig.client.step(&mut rig.gateway);
        while rig.heard().is_some() {}
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        rig.gateway.udp_sockets() <= 64,
        "{} sockets",
        rig.gateway.udp_sockets()
    );
    server.send_to(b"known", outside).unwrap();
    let heard = rig.until(|rig| rig.heard().filter(|(bytes, _)| bytes == b"known"));
    assert!(heard.is_some(), "the server still reaches the ds");
}
