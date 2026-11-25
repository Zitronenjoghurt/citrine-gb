#![no_main]

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use citrine_gb_nds_net::gateway::{Config, Gateway, MAC, Resolver};
use citrine_gb_nds_net::server_list::Host;
use citrine_gb_nds_net::services::{
    GPCM_PORT, GPSP_PORT, GtsLink, HTTP_PORT, QR_PORT, Services, validation,
};
use citrine_gb_nds_net::{dwc, patch};
use citrine_gb_threads::Threads;
use libfuzzer_sys::fuzz_target;

const MAX_PAYLOAD: usize = 1400;
const UDP: u8 = 17;

#[derive(Default)]
struct Echo {
    calls: Vec<u64>,
    claims: Vec<u64>,
    lists: Vec<u64>,
}

impl GtsLink for Echo {
    fn call(&mut self, id: u64, _: &str) {
        self.calls.push(id);
    }

    fn answer(&mut self) -> Option<(u64, Option<Vec<u8>>)> {
        self.calls.pop().map(|id| (id, Some(b"ok".to_vec())))
    }

    fn claim(&mut self, id: u64, _: u32) {
        self.claims.push(id);
    }

    fn claimed(&mut self) -> Option<(u64, Option<bool>)> {
        self.claims.pop().map(|id| (id, Some(true)))
    }

    fn host(&mut self, _: &str, _: u16, _: &[(String, String)]) {}

    fn unhost(&mut self) {}

    fn list(&mut self, id: u64, _: &str) {
        self.lists.push(id);
    }

    fn listed(&mut self) -> Option<(u64, Option<Vec<Host>>)> {
        self.lists.pop().map(|id| (id, Some(Vec::new())))
    }

    fn message(&mut self, _: SocketAddrV4, _: &[u8]) {}

    fn messages(&mut self) -> Option<(Ipv4Addr, u16, Vec<u8>)> {
        None
    }

    fn natneg(&mut self, _: u32, _: u8, _: u16) {}

    fn natnegged(&mut self) -> Option<(u32, SocketAddrV4)> {
        None
    }
}

fn clock() -> u64 {
    1_700_000_000
}

fn fresh_services() -> Services {
    Services::new(7, Box::new(Echo::default())).with_clock(clock)
}

fn flow(port: u16, cut: usize, data: &[u8]) {
    let mut services = fresh_services();
    let Some(id) = services.open(port) else {
        return;
    };
    let (first, second) = data.split_at(cut.min(data.len()));
    for part in [first, second] {
        services.input(id, part);
        services.poll(0);
        let _ = services.take_output(id, usize::MAX);
    }
    if services.profile_wanted().is_some() {
        services.saved_profile(Some(1));
        services.poll(0);
        let _ = services.take_output(id, usize::MAX);
    }
    let _ = services.finished(id);
    let _ = services.take_log();
}

fn ones(bytes: &[u8], mut total: u32) -> u32 {
    for pair in bytes.chunks(2) {
        let low = pair.get(1).copied().unwrap_or(0);
        total += u32::from(u16::from_be_bytes([pair[0], low]));
    }
    total
}

fn fold(mut total: u32) -> u16 {
    while total >> 16 != 0 {
        total = (total & 0xFFFF) + (total >> 16);
    }
    !(total as u16)
}

fn frame(
    to_mac: [u8; 6],
    from: Ipv4Addr,
    to: Ipv4Addr,
    ports: (u16, u16),
    payload: &[u8],
) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let total = 20 + udp_len;
    let mut ip = vec![0x45, 0, 0, 0, 0, 1, 0, 0, 64, UDP, 0, 0];
    ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    ip.extend_from_slice(&from.octets());
    ip.extend_from_slice(&to.octets());
    let check = fold(ones(&ip, 0));
    ip[10..12].copy_from_slice(&check.to_be_bytes());
    let mut udp = Vec::with_capacity(udp_len);
    udp.extend_from_slice(&ports.0.to_be_bytes());
    udp.extend_from_slice(&ports.1.to_be_bytes());
    udp.extend_from_slice(&(udp_len as u16).to_be_bytes());
    udp.extend_from_slice(&[0, 0]);
    udp.extend_from_slice(payload);
    let mut pseudo = Vec::with_capacity(12);
    pseudo.extend_from_slice(&from.octets());
    pseudo.extend_from_slice(&to.octets());
    pseudo.extend_from_slice(&[0, UDP]);
    pseudo.extend_from_slice(&(udp_len as u16).to_be_bytes());
    let check = match fold(ones(&udp, ones(&pseudo, 0))) {
        0 => 0xFFFF,
        check => check,
    };
    udp[6..8].copy_from_slice(&check.to_be_bytes());
    let mut out = to_mac.to_vec();
    out.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x10, 0x08, 0x00]);
    out.extend_from_slice(&ip);
    out.extend_from_slice(&udp);
    out
}

fn through_gateway(dhcp: bool, data: &[u8]) {
    let threads = Threads::default();
    let resolver: Resolver = Arc::new(|_: &str| Vec::<Ipv4Addr>::new());
    let config = Config::default();
    let mut gateway = Gateway::new(config, threads.spawner())
        .with_services(fresh_services())
        .with_resolver(resolver);
    let payload = &data[..data.len().min(MAX_PAYLOAD)];
    let packet = if dhcp {
        frame(
            [0xFF; 6],
            Ipv4Addr::UNSPECIFIED,
            Ipv4Addr::BROADCAST,
            (68, 67),
            payload,
        )
    } else {
        frame(
            MAC,
            config.first_lease,
            config.address,
            (40000, 53),
            payload,
        )
    };
    gateway.send(packet);
    gateway.poll(1_000);
    gateway.poll(2_000);
    while gateway.take().is_some() {}
    if let Some(services) = gateway.services_mut() {
        let _ = services.take_log();
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&head, rest)) = data.split_first() else {
        return;
    };
    let cut = rest.len() * usize::from(head >> 3) / 32;
    match head & 7 {
        0 => flow(HTTP_PORT, cut, rest),
        1 => flow(GPCM_PORT, cut, rest),
        2 => flow(GPSP_PORT, cut, rest),
        3 => {
            let from = SocketAddrV4::new(Ipv4Addr::new(10, 77, 0, 2), 2000);
            let _ = fresh_services().datagram(QR_PORT, from, rest);
        }
        4 => through_gateway(false, rest),
        5 => through_gateway(true, rest),
        6 => {
            let _ = dwc::saved_user(rest);
            let _ = dwc::saved_profile(rest, 1);
            let _ = patch::plain_http_spots(rest);
        }
        _ => {
            let _ = validation(rest);
        }
    }
});
