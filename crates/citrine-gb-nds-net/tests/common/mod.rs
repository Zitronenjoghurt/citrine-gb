use std::collections::VecDeque;
use std::net::Ipv4Addr;

use citrine_gb_nds_net::gateway::Gateway;
use smoltcp::iface::{Config as InterfaceConfig, Interface, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};

const CLIENT: [u8; 6] = [0x00, 0x09, 0xBF, 0x11, 0x22, 0x33];

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
        capabilities.max_transmission_unit = 1514;
        capabilities
    }
}

pub struct Client {
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
    wire: Wire,
    us: i64,
}

impl Client {
    pub fn new(gateway: Ipv4Addr) -> Self {
        let mut wire = Wire::default();
        let mut setup = InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(CLIENT)));
        setup.random_seed = 7;
        let mut iface = Interface::new(setup, &mut wire, Instant::from_micros(0));
        iface.update_ip_addrs(|addresses| {
            addresses
                .push(IpCidr::new(IpAddress::v4(10, 77, 0, 2), 24))
                .ok();
        });
        iface.routes_mut().add_default_ipv4_route(gateway).ok();
        Self {
            iface,
            sockets: SocketSet::new(Vec::new()),
            wire,
            us: 0,
        }
    }

    pub fn step(&mut self, gateway: &mut Gateway) {
        self.us += 1_000;
        let now = Instant::from_micros(self.us);
        self.iface.poll(now, &mut self.wire, &mut self.sockets);
        while let Some(frame) = self.wire.outbound.pop_front() {
            gateway.send(frame);
        }
        gateway.poll(self.us as u64);
        while let Some(frame) = gateway.take() {
            self.wire.inbound.push_back(frame);
        }
        self.iface.poll(now, &mut self.wire, &mut self.sockets);
    }

    #[allow(dead_code)]
    pub fn step_deaf(&mut self, gateway: &mut Gateway) {
        self.us += 1_000;
        let now = Instant::from_micros(self.us);
        self.iface.poll(now, &mut self.wire, &mut self.sockets);
        while let Some(frame) = self.wire.outbound.pop_front() {
            gateway.send(frame);
        }
        gateway.poll(self.us as u64);
        while gateway.take().is_some() {}
    }
}
