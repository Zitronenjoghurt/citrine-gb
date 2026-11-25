use super::Citrine;
use crate::lan::{Event, Finder, Lan, LanAccount, SERVICE, Service};
use crate::online::cloud::Peer;
use citrine_gb_surface::proof::TagKey;

#[derive(Default)]
pub(super) struct LanPeers {
    pub(super) lan: Option<Lan>,
    #[cfg_attr(not(feature = "nds"), allow(dead_code))]
    pub(super) events: Vec<Event>,
    finder: Finder,
}

impl Citrine {
    pub(super) fn follow_lan(&mut self) {
        let name = self.online.device_name();
        let account = self.lan_account();
        let instance = self.second.instance();
        let now = self.second.now();
        let spawner = self.threads.spawner();
        let store = &self.settings_store;
        let lan = self
            .lan
            .lan
            .get_or_insert_with(|| Lan::new(store.lan_keyring().as_deref()));
        lan.set_name(&name, instance);
        if !self.ui.settings.online.is_joined() {
            lan.end_account();
        }
        self.second.offer(SERVICE, true);
        for arrival in self.second.take_peers(SERVICE) {
            let psk = match (arrival.hello.account, &account) {
                (false, _) => None,
                (true, Some(account)) => Some(account.own_psk),
                (true, None) => continue,
            };
            lan.arrived(&arrival.first, Box::new(arrival.link), psk, now);
        }
        self.lan.finder.step(lan, account.as_ref(), &spawner, now);
        self.lan.events = lan.step(now);
        self.online.network_peers(
            lan.trusted_peers()
                .map(|(key, name, account)| Peer {
                    key,
                    name: name.to_owned(),
                    account,
                })
                .collect(),
        );
        for event in &self.lan.events {
            if let Event::Part {
                from,
                service: Service::Saves,
                body,
            } = event
            {
                self.online.network_receive(*from, body);
            }
        }
        for (to, body) in self.online.network_take_out() {
            lan.send(to, Service::Saves, body);
        }
        if let Some(bytes) = lan.take_keyring()
            && let Err(err) = store.store_lan_keyring(&bytes)
        {
            log::error!("failed to keep the nearby devices: {err}");
        }
    }

    fn lan_account(&self) -> Option<LanAccount> {
        self.ui.settings.online.joined_relay()?;
        let keys = self.online.keys()?;
        let (devices, own) = self.online.devices();
        let own = own?;
        let devices = devices
            .iter()
            .filter(|device| device.id != own)
            .map(|device| {
                (
                    TagKey(keys.screen_tag_key(device.id)),
                    keys.walker_psk(device.id),
                )
            })
            .collect();
        Some(LanAccount {
            own_psk: keys.walker_psk(own),
            devices,
        })
    }
}
