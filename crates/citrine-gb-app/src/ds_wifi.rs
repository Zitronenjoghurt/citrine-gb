mod gts;
mod net;

use citrine_gb::machine::Machine;
use citrine_gb::nds::spi::firmware::online::{
    AccessPointSlot, CONNECTIONS_BYTES, MAC_PREFIX, Online,
};
use citrine_gb::nds::wifi::Wifi;
use citrine_gb::nds::wifi::air::{Station, WifiPeer};
use citrine_gb_nds_net::access_point::{AccessPoint, SSID};
use citrine_gb_nds_net::gateway::{Config, Gateway};
use citrine_gb_nds_net::patch;
use citrine_gb_nds_net::relay::{self, Relay};
use citrine_gb_nds_net::services::Services;
use citrine_gb_net::ClientMsg;
use citrine_gb_threads::Spawner;
use std::net::Ipv4Addr;

pub use gts::GtsPort;
pub use net::{NetPort, Refusal};

const MAC_BYTES: usize = 6;
const QUIET_EXCHANGE_US: u64 = 32;
const MAX_CATCH_UP_US: u64 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub mac: [u8; MAC_BYTES],
    pub connections: Option<Vec<u8>>,
}

impl Identity {
    pub fn random() -> Self {
        let mut tail = [0; MAC_BYTES - MAC_PREFIX.len()];
        if getrandom::getrandom(&mut tail).is_err() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos());
            let len = tail.len();
            tail.copy_from_slice(&nanos.to_le_bytes()[..len]);
        }
        let mut mac = [0; MAC_BYTES];
        mac[..MAC_PREFIX.len()].copy_from_slice(&MAC_PREFIX);
        mac[MAC_PREFIX.len()..].copy_from_slice(&tail);
        Self {
            mac,
            connections: None,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mac: [u8; MAC_BYTES] = bytes.get(..MAC_BYTES)?.try_into().ok()?;
        if mac[..MAC_PREFIX.len()] != MAC_PREFIX {
            return None;
        }
        let connections = bytes
            .get(MAC_BYTES..)
            .filter(|rest| rest.len() == CONNECTIONS_BYTES)
            .map(<[u8]>::to_vec);
        Some(Self { mac, connections })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.mac.to_vec();
        if let Some(connections) = &self.connections {
            bytes.extend_from_slice(connections);
        }
        bytes
    }

    pub fn online(&self, dns: Option<Ipv4Addr>, relay: bool, own_firmware: bool) -> Online {
        Online {
            mac: (!own_firmware).then_some(self.mac),
            connections: self.connections.clone(),
            access_point: (dns.is_some() || relay).then(|| AccessPointSlot {
                ssid: SSID.to_vec(),
                dns: dns.map(|address| address.octets()),
            }),
        }
    }
}

#[derive(Default)]
pub struct DsWifi {
    pub identity: Option<Identity>,
    pub spawner: Option<Spawner>,
    pub online: bool,
    pub relay: Option<u64>,
    pub direct: bool,
    pub changed: bool,
    gts: Option<GtsPort>,
    net: Option<NetPort>,
    next_stream: u32,
    closing: Vec<u32>,
    patched_at: u64,
}

impl DsWifi {
    pub fn follow(&mut self, machine: &mut Machine, network: &mut Option<Network>) {
        let Machine::Nds(nds) = machine else {
            return;
        };
        if !self.online
            || network
                .as_ref()
                .is_some_and(|n| n.relay != self.relay || n.direct != self.direct)
        {
            *network = None;
            self.gts = None;
            if let Some(port) = self.net.take() {
                let (next, open) = port.retire();
                self.next_stream = next;
                self.closing.extend(open);
            }
            self.patched_at = 0;
        }
        if self.online
            && network.is_none()
            && let Some(spawner) = &self.spawner
        {
            let egress = (!self.direct).then(|| {
                let (relay, end) = relay::relay();
                self.net = Some(NetPort::new(end, self.next_stream));
                relay
            });
            *network = Some(match self.relay {
                Some(seed) => {
                    let (port, link) = gts::pair();
                    self.gts = Some(port);
                    Network::with_relay(spawner.clone(), seed, Box::new(link), egress)
                }
                None => Network::new(spawner.clone(), egress),
            });
        }
        if let Some(network) = network.as_mut()
            && network.relay.is_some()
        {
            let gateway = network.access_point.gateway_mut();
            let moments = gateway.dhcp_replies() + gateway.nintendo_lookups();
            if moments > self.patched_at {
                self.patched_at = moments;
                let spots = patch::plain_http(nds);
                if let Some(services) = network.access_point.gateway_mut().services_mut() {
                    services.note(format!("plain http in {spots} places"));
                }
            }
            for line in network.take_log() {
                log::info!("ds wifi: {line}");
            }
        }
        let firmware = &mut nds.buses_mut().spi_mut().devices_mut().firmware;
        if !firmware.dirty() {
            return;
        }
        firmware.clear_dirty();
        let connections = firmware.connections();
        if let Some(identity) = self.identity.as_mut()
            && identity.connections.as_deref() != Some(connections)
        {
            identity.connections = Some(connections.to_vec());
            self.changed = true;
        }
    }
}

impl DsWifi {
    pub fn pump(&mut self, online: &mut crate::online::Online) {
        for stream in self.closing.drain(..) {
            online.send(ClientMsg::NetClose { stream });
        }
        if let Some(port) = self.gts.as_mut() {
            port.pump(online);
        }
        if let Some(port) = self.net.as_mut() {
            port.pump(online);
        }
    }

    pub fn take_refusal(&mut self) -> Option<Refusal> {
        self.net.as_mut().and_then(NetPort::take_refusal)
    }
}

pub struct Network {
    access_point: AccessPoint,
    relay: Option<u64>,
    direct: bool,
    seen: Option<u64>,
}

impl Network {
    pub fn new(spawner: Spawner, egress: Option<Relay>) -> Self {
        Self::with_gateway(Gateway::new(Config::default(), spawner), None, egress)
    }

    pub fn with_relay(
        spawner: Spawner,
        seed: u64,
        link: Box<dyn citrine_gb_nds_net::services::GtsLink>,
        egress: Option<Relay>,
    ) -> Self {
        let gateway =
            Gateway::new(Config::default(), spawner).with_services(Services::new(seed, link));
        Self::with_gateway(gateway, Some(seed), egress)
    }

    fn with_gateway(gateway: Gateway, relay: Option<u64>, egress: Option<Relay>) -> Self {
        let direct = egress.is_none();
        let gateway = match egress {
            Some(egress) => gateway.with_relay(egress),
            None => gateway,
        };
        Self {
            access_point: AccessPoint::new(gateway),
            relay,
            direct,
            seen: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn gateway(&self) -> &Gateway {
        self.access_point.gateway()
    }

    #[cfg(test)]
    pub(crate) fn gateway_mut(&mut self) -> &mut Gateway {
        self.access_point.gateway_mut()
    }

    fn take_log(&mut self) -> Vec<String> {
        self.access_point
            .gateway_mut()
            .services_mut()
            .map(Services::take_log)
            .unwrap_or_default()
    }
}

impl WifiPeer for Network {
    fn exchange(&mut self, chip: &mut Wifi) {
        let now = chip.now_us();
        let seen = *self.seen.get_or_insert(now);
        let gap = now.saturating_sub(seen);
        if gap < QUIET_EXCHANGE_US && !chip.on_air() && !self.access_point.on_air() {
            return;
        }
        self.seen = Some(now);
        let target = self.access_point.now_us() + gap.min(MAX_CATCH_UP_US);
        for frame in chip.take_frames_at(target) {
            self.access_point.receive(frame);
        }
        self.access_point.run_until(target);
        for frame in self.access_point.take_frames() {
            chip.receive_at(target, frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_installs_get_different_macs_with_nintendos_prefix() {
        let first = Identity::random();
        let second = Identity::random();
        assert_ne!(first.mac, second.mac);
        assert_eq!(first.mac[..3], MAC_PREFIX);
    }

    #[test]
    fn an_identity_comes_back_from_its_bytes() {
        let mut identity = Identity::random();
        assert_eq!(
            Identity::from_bytes(&identity.to_bytes()),
            Some(identity.clone())
        );
        identity.connections = Some(vec![7; CONNECTIONS_BYTES]);
        assert_eq!(Identity::from_bytes(&identity.to_bytes()), Some(identity));
        assert_eq!(Identity::from_bytes(&[1, 2, 3, 4, 5, 6]), None);
        assert_eq!(Identity::from_bytes(&[0, 9]), None);
    }

    #[test]
    fn a_dumped_firmware_keeps_its_own_mac() {
        let identity = Identity::random();
        assert_eq!(identity.online(None, false, true).mac, None);
        assert_eq!(identity.online(None, false, false).mac, Some(identity.mac));
    }

    #[test]
    fn the_access_point_is_offered_only_with_a_service() {
        let identity = Identity::random();
        assert_eq!(identity.online(None, false, false).access_point, None);
        let slot = identity
            .online(Some(Ipv4Addr::new(1, 2, 3, 4)), false, false)
            .access_point;
        assert_eq!(slot.unwrap().dns, Some([1, 2, 3, 4]));
    }

    #[test]
    fn the_gateway_goes_out_through_the_relay_unless_direct() {
        let threads = citrine_gb_threads::Threads::default();
        let (egress, _end) = relay::relay();
        let mut relayed = Network::new(threads.spawner(), Some(egress));
        assert!(relayed.access_point.gateway_mut().relayed());
        assert!(!relayed.direct);
        let mut direct = Network::new(threads.spawner(), None);
        assert!(!direct.access_point.gateway_mut().relayed());
        assert!(direct.direct);
    }

    #[test]
    fn the_relay_access_point_takes_the_gateways_dns() {
        let identity = Identity::random();
        let slot = identity.online(None, true, false).access_point.unwrap();
        assert_eq!(slot.ssid, SSID.to_vec());
        assert_eq!(slot.dns, None);
    }
}
