use super::Relay;
use super::relay::Look;
use citrine_gb_surface::proof::{ProofKey, TagKey};
use citrine_gb_surface::relay::{DoorKey, LOOK_KEYS};
#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;

const WANT_SECS: f64 = 1.0;
const LOOK_EVERY_SECS: f64 = 5.0;
#[cfg(not(target_arch = "wasm32"))]
const PROBE_EVERY_SECS: f64 = 1.0;
#[cfg(not(target_arch = "wasm32"))]
const FORGET_SECS: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceKeys {
    pub door: DoorKey,
    pub proof: ProofKey,
    pub tag: TagKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnDevice {
    pub name: String,
    pub keys: DeviceKeys,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub keys: DeviceKeys,
    pub devices: Vec<OwnDevice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    #[cfg(not(target_arch = "wasm32"))]
    Lan(SocketAddr),
    Relay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub own: bool,
    pub reach: Reach,
    pub keys: Option<DeviceKeys>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Scan {
    nearby: citrine_gb_surface::pairing::Nearby,
    probed_at: f64,
    heard: Vec<Heard>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Heard {
    host: citrine_gb_surface::pairing::NearbyHost,
    seen: f64,
    owner: Option<DoorKey>,
}

#[derive(Default)]
pub struct Suggest {
    wanted_until: f64,
    #[cfg(not(target_arch = "wasm32"))]
    scan: Option<Scan>,
    look: Option<Look>,
    look_at: f64,
    here: Option<Vec<DoorKey>>,
}

pub struct Around<'a> {
    pub now: f64,
    pub relay: Option<&'a Relay>,
    pub account: Option<&'a Account>,
    #[cfg(not(target_arch = "wasm32"))]
    pub skip: Option<u64>,
    #[cfg(not(target_arch = "wasm32"))]
    pub targets: Option<&'a [SocketAddr]>,
}

impl Suggest {
    pub fn want(&mut self, now: f64) {
        self.wanted_until = now + WANT_SECS;
    }

    pub fn forget_own(&mut self) {
        self.look = None;
        self.here = None;
        self.look_at = 0.0;
    }

    pub fn step(&mut self, around: Around<'_>) {
        if around.now > self.wanted_until {
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.scan = None;
            }
            self.look = None;
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.scan_lan(&around);
        self.look_own(&around);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scan_lan(&mut self, around: &Around<'_>) {
        use citrine_gb_surface::pairing::Nearby;
        let now = around.now;
        let scan = self.scan.get_or_insert_with(|| Scan {
            nearby: match around.targets {
                Some(targets) => Nearby::toward(targets.to_vec(), around.skip),
                None => Nearby::start(around.skip),
            },
            probed_at: now,
            heard: Vec::new(),
        });
        if now - scan.probed_at >= PROBE_EVERY_SECS {
            scan.nearby.probe();
            scan.probed_at = now;
        }
        let devices = around
            .account
            .map(|a| a.devices.as_slice())
            .unwrap_or_default();
        for host in scan.nearby.poll() {
            let owner = devices
                .iter()
                .find(|d| scan.nearby.tagged_by(&host, &d.keys.tag))
                .map(|d| d.keys.door);
            scan.heard.retain(|h| h.host.instance != host.instance);
            scan.heard.push(Heard {
                host,
                seen: now,
                owner,
            });
        }
        scan.heard.retain(|h| now - h.seen < FORGET_SECS);
    }

    fn look_own(&mut self, around: &Around<'_>) {
        let (Some(relay), Some(account)) = (around.relay, around.account) else {
            self.forget_own();
            return;
        };
        if let Some(look) = &mut self.look
            && let Some(answer) = look.step()
        {
            self.here = answer;
            self.look = None;
        }
        if self.look.is_none() && around.now >= self.look_at && !account.devices.is_empty() {
            let keys = account
                .devices
                .iter()
                .take(LOOK_KEYS)
                .map(|d| d.keys.door)
                .collect();
            self.look = Some(Look::start(&relay.url, keys));
            self.look_at = around.now + LOOK_EVERY_SECS;
        }
    }

    pub fn list(&self, account: Option<&Account>) -> Vec<Suggestion> {
        let devices = account.map(|a| a.devices.as_slice()).unwrap_or_default();
        let mut list: Vec<Suggestion> = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(scan) = &self.scan {
            for heard in &scan.heard {
                let host = &heard.host;
                if !host.offers(citrine_gb_surface::pairing::SCREEN_SERVICE) {
                    continue;
                }
                let own = devices.iter().find(|d| Some(d.keys.door) == heard.owner);
                list.push(Suggestion {
                    name: own.map_or_else(|| host.name.clone(), |d| d.name.clone()),
                    own: own.is_some(),
                    reach: Reach::Lan(host.addr),
                    keys: own.map(|d| d.keys),
                });
            }
        }
        for key in self.here.iter().flatten() {
            let Some(device) = devices.iter().find(|d| d.keys.door == *key) else {
                continue;
            };
            if list.iter().any(|s| s.keys == Some(device.keys)) {
                continue;
            }
            list.push(Suggestion {
                name: device.name.clone(),
                own: true,
                reach: Reach::Relay,
                keys: Some(device.keys),
            });
        }
        list.sort_by(|a, b| b.own.cmp(&a.own).then_with(|| a.name.cmp(&b.name)));
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(id: u8) -> DeviceKeys {
        DeviceKeys {
            door: DoorKey([id; 16]),
            proof: ProofKey([id; 32]),
            tag: TagKey([id; 32]),
        }
    }

    fn device(id: u8, name: &str) -> OwnDevice {
        OwnDevice {
            name: name.into(),
            keys: keys(id),
        }
    }

    #[test]
    fn own_devices_on_the_relay_show_once_and_first() {
        let account = Account {
            keys: keys(0),
            devices: vec![device(1, "MacBook"), device(2, "Old tablet")],
        };
        let suggest = Suggest {
            here: Some(vec![DoorKey([1; 16]), DoorKey([9; 16])]),
            ..Suggest::default()
        };
        let list = suggest.list(Some(&account));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "MacBook");
        assert!(list[0].own);
        assert_eq!(list[0].reach, Reach::Relay);
        assert_eq!(list[0].keys, Some(keys(1)));
        assert!(suggest.list(None).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_nearby_own_device_takes_the_network_and_drops_its_relay_row() {
        use citrine_gb_surface::pairing::{Nearby, NearbyHost};
        let account = Account {
            keys: keys(0),
            devices: vec![device(1, "MacBook")],
        };
        let at = |port| SocketAddr::from(([192, 168, 1, 20], port));
        let heard = |port, instance, owner, services: &str, name: &str| Heard {
            host: NearbyHost {
                addr: at(port),
                instance,
                tag: None,
                services: vec![services.into()],
                name: name.into(),
            },
            seen: 0.0,
            owner,
        };
        let suggest = Suggest {
            scan: Some(Scan {
                nearby: Nearby::toward(Vec::new(), None),
                probed_at: 0.0,
                heard: vec![
                    heard(
                        47813,
                        7,
                        Some(DoorKey([1; 16])),
                        "screen",
                        "Clara's MacBook",
                    ),
                    heard(47900, 8, None, "screen", "Living room PC"),
                    heard(47901, 9, None, "walker", "Den PC"),
                    heard(47902, 10, Some(DoorKey([5; 16])), "screen", "Gone tablet"),
                ],
            }),
            here: Some(vec![DoorKey([1; 16])]),
            ..Suggest::default()
        };
        let list = suggest.list(Some(&account));
        assert_eq!(list.len(), 3, "{list:?}");
        assert_eq!(list[0].name, "MacBook");
        assert_eq!(list[0].reach, Reach::Lan(at(47813)));
        assert_eq!(list[0].keys, Some(keys(1)));
        assert_eq!(list[1].name, "Gone tablet");
        assert!(!list[1].own);
        assert_eq!(list[2].name, "Living room PC");
        assert!(!list[2].own);
        assert_eq!(list[2].keys, None);
    }
}
