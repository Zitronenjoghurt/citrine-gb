mod common;

use std::net::{Ipv4Addr, SocketAddrV4};

use citrine_gb_nds_net::gateway::{Config, Gateway};
use citrine_gb_nds_net::relay::{self, NetIn, NetOut, RelayEnd};
use citrine_gb_threads::Threads;
use common::Client;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::{dhcpv4, udp};
use smoltcp::wire::{IpAddress, IpCidr, IpEndpoint};

const STEPS: usize = 5_000;
const FAR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 27_900);
const A: Ipv4Addr = Ipv4Addr::new(10, 77, 1, 1);
const B: Ipv4Addr = Ipv4Addr::new(10, 77, 1, 2);

struct Ds {
    gateway: Gateway,
    client: Client,
    end: RelayEnd,
    dhcp: SocketHandle,
    udp: SocketHandle,
    address: Option<Ipv4Addr>,
}

impl Ds {
    fn new(threads: &Threads, port: u16) -> Self {
        let (relay, end) = relay::relay();
        let config = Config::default();
        let gateway = Gateway::new(config, threads.spawner()).with_relay(relay);
        let mut client = Client::new(config.address);
        client.iface.update_ip_addrs(|addresses| addresses.clear());
        client.iface.routes_mut().remove_default_ipv4_route();
        let dhcp = client.sockets.add(dhcpv4::Socket::new());
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
        );
        socket.bind(port).unwrap();
        let udp = client.sockets.add(socket);
        Self {
            gateway,
            client,
            end,
            dhcp,
            udp,
            address: None,
        }
    }

    fn step(&mut self) {
        self.client.step(&mut self.gateway);
        let event = self
            .client
            .sockets
            .get_mut::<dhcpv4::Socket>(self.dhcp)
            .poll();
        if let Some(dhcpv4::Event::Configured(config)) = event {
            self.client.iface.update_ip_addrs(|addresses| {
                addresses.clear();
                addresses.push(IpCidr::Ipv4(config.address)).unwrap();
            });
            self.client
                .iface
                .routes_mut()
                .add_default_ipv4_route(config.router.unwrap())
                .unwrap();
            self.address = Some(config.address.address());
        }
    }

    fn send(&mut self, to: SocketAddrV4, data: &[u8]) {
        let to = IpEndpoint::new(IpAddress::Ipv4(*to.ip()), to.port());
        self.client
            .sockets
            .get_mut::<udp::Socket>(self.udp)
            .send_slice(data, to)
            .unwrap();
    }

    fn heard(&mut self) -> Option<(Vec<u8>, SocketAddrV4)> {
        self.client
            .sockets
            .get_mut::<udp::Socket>(self.udp)
            .recv()
            .ok()
            .map(|(data, meta)| {
                let IpAddress::Ipv4(ip) = meta.endpoint.addr;
                (data.to_vec(), SocketAddrV4::new(ip, meta.endpoint.port))
            })
    }
}

struct Lan {
    dses: Vec<Ds>,
    leases: Vec<Option<Ipv4Addr>>,
    _threads: Threads,
}

impl Lan {
    fn new(leases: &[Option<Ipv4Addr>]) -> Self {
        let threads = Threads::default();
        let dses = (0..leases.len())
            .map(|index| Ds::new(&threads, 1000 * (index as u16 + 1)))
            .collect();
        Self {
            dses,
            leases: leases.to_vec(),
            _threads: threads,
        }
    }

    fn serve(&mut self) {
        for index in 0..self.dses.len() {
            while let Ok(out) = self.dses[index].end.out.try_recv() {
                match out {
                    NetOut::Lease => {
                        let address = self.leases[index];
                        let back = &self.dses[index].end.back;
                        back.send(NetIn::Leased { address }).unwrap();
                    }
                    NetOut::Send { port, to, data } => {
                        let Some(from) = self.leases[index] else {
                            continue;
                        };
                        let Some(owner) = self.leases.iter().position(|l| *l == Some(*to.ip()))
                        else {
                            continue;
                        };
                        let heard = NetIn::Heard {
                            port: to.port(),
                            from: SocketAddrV4::new(from, port),
                            data,
                        };
                        self.dses[owner].end.back.send(heard).unwrap();
                    }
                    _ => {}
                }
            }
        }
    }

    fn until<T>(&mut self, mut found: impl FnMut(&mut Self) -> Option<T>) -> Option<T> {
        for _ in 0..STEPS {
            for ds in &mut self.dses {
                ds.step();
            }
            self.serve();
            if let Some(value) = found(self) {
                return Some(value);
            }
        }
        None
    }

    fn addresses(&mut self) -> Option<Vec<Ipv4Addr>> {
        self.until(|lan| lan.dses.iter().map(|ds| ds.address).collect())
    }
}

#[test]
fn two_relayed_dses_hear_each_other_on_ports_they_sent_from() {
    let mut lan = Lan::new(&[Some(A), Some(B)]);
    assert_eq!(lan.addresses(), Some(vec![A, B]));
    lan.dses[1].send(FAR, b"beat");
    lan.until(|lan| (!lan.dses[1].gateway.udp_peers().is_empty()).then_some(()))
        .expect("b's port goes out");
    lan.dses[0].send(SocketAddrV4::new(B, 2000), b"hi b");
    assert_eq!(
        lan.until(|lan| lan.dses[1].heard()),
        Some((b"hi b".to_vec(), SocketAddrV4::new(A, 1000)))
    );
    lan.dses[1].send(SocketAddrV4::new(A, 1000), b"hi a");
    assert_eq!(
        lan.until(|lan| lan.dses[0].heard()),
        Some((b"hi a".to_vec(), SocketAddrV4::new(B, 2000)))
    );
}

#[test]
fn a_relay_with_no_lease_leaves_the_ds_on_its_own_address_with_no_lan() {
    let mut lan = Lan::new(&[None, Some(B)]);
    let fallback = Config::default().first_lease;
    assert_eq!(lan.addresses(), Some(vec![fallback, B]));
    lan.dses[1].send(FAR, b"beat");
    lan.until(|lan| (!lan.dses[1].gateway.udp_peers().is_empty()).then_some(()))
        .expect("b's port goes out");
    lan.dses[0].send(SocketAddrV4::new(B, 2000), b"hi b");
    assert_eq!(lan.until(|lan| lan.dses[1].heard()), None);
}

#[test]
fn a_lan_datagram_reaches_a_port_the_ds_never_sent_from() {
    let mut lan = Lan::new(&[Some(A), Some(B)]);
    assert_eq!(lan.addresses(), Some(vec![A, B]));
    lan.dses[0].send(SocketAddrV4::new(B, 2000), b"first");
    assert_eq!(
        lan.until(|lan| lan.dses[1].heard()),
        Some((b"first".to_vec(), SocketAddrV4::new(A, 1000)))
    );
    lan.dses[1].send(SocketAddrV4::new(A, 1000), b"back");
    assert_eq!(
        lan.until(|lan| lan.dses[0].heard()),
        Some((b"back".to_vec(), SocketAddrV4::new(B, 2000)))
    );
}

#[test]
fn lan_datagrams_to_many_ports_open_a_bounded_number() {
    let mut lan = Lan::new(&[Some(A), Some(B)]);
    assert_eq!(lan.addresses(), Some(vec![A, B]));
    for port in 3000..3200 {
        let heard = NetIn::Heard {
            port,
            from: SocketAddrV4::new(A, 1000),
            data: b"knock".to_vec(),
        };
        lan.dses[1].end.back.send(heard).unwrap();
    }
    for _ in 0..50 {
        lan.dses[1].step();
    }
    let mut ports: Vec<u16> = lan.dses[1]
        .gateway
        .udp_peers()
        .iter()
        .map(|peer| peer.ds_port)
        .collect();
    ports.dedup();
    assert!(
        !ports.is_empty() && ports.len() <= 64,
        "{} ports",
        ports.len()
    );
}
