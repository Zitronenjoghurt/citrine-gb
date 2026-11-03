use crate::online::address::ServerAddress;
use citrine_gb_net::{Account, DeviceKey, Fingerprint, RelayId, RelayKey};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OnlineSettings {
    pub address: String,
    pub joined: Option<String>,
    pub relays: BTreeMap<String, RelayEntry>,
    pub show_playing: bool,
    pub typing_indicators: bool,
    pub cloud_saves: bool,
    pub sync_settings: bool,
    pub friend_volume: f32,
    pub friend_muted: bool,
}

impl OnlineSettings {
    pub const DEFAULT_FRIEND_VOLUME: f32 = 0.5;
}

impl Default for OnlineSettings {
    fn default() -> Self {
        Self {
            address: String::new(),
            joined: None,
            relays: BTreeMap::new(),
            show_playing: true,
            typing_indicators: true,
            cloud_saves: true,
            sync_settings: true,
            friend_volume: Self::DEFAULT_FRIEND_VOLUME,
            friend_muted: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RelayEntry {
    pub relay_id: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub online_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub relay_key: Option<RelayKey>,
    #[serde(default)]
    pub device: Option<DeviceKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<SavedAccount>,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SavedAccount {
    pub relay_id: String,
    #[serde(default)]
    pub online_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub device: Option<DeviceKey>,
}

impl fmt::Debug for SavedAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SavedAccount")
            .field("online_id", &self.online_id)
            .field("display_name", &self.display_name)
            .finish_non_exhaustive()
    }
}

impl SavedAccount {
    pub fn name(&self) -> &str {
        self.display_name.as_deref().unwrap_or_default()
    }

    pub fn seed(&self) -> u64 {
        self.online_id
            .as_deref()
            .and_then(|id| id.parse::<citrine_gb_net::OnlineId>().ok())
            .map(|id| id.value())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trust {
    pub relay_key: Option<RelayKey>,
    pub expected: Option<Fingerprint>,
    pub device: Option<DeviceKey>,
}

impl fmt::Debug for RelayEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayEntry")
            .field("online_id", &self.online_id)
            .field("display_name", &self.display_name)
            .field("others", &self.others)
            .finish_non_exhaustive()
    }
}

impl RelayEntry {
    pub fn relay_id(&self) -> Option<RelayId> {
        self.relay_id.parse().ok()
    }

    fn current(&self) -> Option<SavedAccount> {
        self.relay_id().map(|_| SavedAccount {
            relay_id: self.relay_id.clone(),
            online_id: self.online_id.clone(),
            display_name: self.display_name.clone(),
            device: self.device.clone(),
        })
    }

    fn set_current(&mut self, account: Option<SavedAccount>) {
        let account = account.unwrap_or(SavedAccount {
            relay_id: String::new(),
            online_id: None,
            display_name: None,
            device: None,
        });
        self.relay_id = account.relay_id;
        self.online_id = account.online_id;
        self.display_name = account.display_name;
        self.device = account.device;
    }

    pub fn accounts(&self) -> Vec<SavedAccount> {
        self.current()
            .into_iter()
            .chain(self.others.iter().cloned())
            .collect()
    }

    fn take(&mut self, relay_id: &str) -> Option<SavedAccount> {
        if relay_id.is_empty() {
            return None;
        }
        if self.relay_id == relay_id {
            let current = self.current();
            let next = (!self.others.is_empty()).then(|| self.others.remove(0));
            self.set_current(next);
            return current;
        }
        let at = self.others.iter().position(|a| a.relay_id == relay_id)?;
        Some(self.others.remove(at))
    }

    fn put_first(&mut self, account: SavedAccount) {
        self.take(&account.relay_id);
        if let Some(current) = self.current() {
            self.others.insert(0, current);
        }
        self.set_current(Some(account));
    }

    pub fn trust(&self) -> Trust {
        Trust {
            relay_key: self.relay_key,
            expected: None,
            device: self.device.clone(),
        }
    }
}

pub struct JoinedRelay {
    pub address: ServerAddress,
    pub relay_id: RelayId,
    pub password: String,
    pub trust: Trust,
}

impl OnlineSettings {
    pub fn entry(&self, address: &ServerAddress) -> Option<&RelayEntry> {
        self.relays.get(&address.key())
    }

    pub fn joined_entry(&self) -> Option<&RelayEntry> {
        self.relays.get(self.joined.as_ref()?)
    }

    pub fn joined_relay(&self) -> Option<JoinedRelay> {
        let key = self.joined.as_ref()?;
        let entry = self.relays.get(key)?;
        Some(JoinedRelay {
            address: ServerAddress::parse(key).ok()?,
            relay_id: entry.relay_id()?,
            password: entry.password.clone(),
            trust: entry.trust(),
        })
    }

    pub fn cloud_owner(&self) -> Option<String> {
        let key = self.joined.as_ref()?;
        let online_id = self.relays.get(key)?.online_id.as_ref()?;
        Some(format!("{online_id}@{key}"))
    }

    pub fn is_joined(&self) -> bool {
        self.joined_relay().is_some()
    }

    pub fn remember(
        &mut self,
        address: &ServerAddress,
        relay_id: &RelayId,
        password: &str,
        account: &Account,
        trust: &Trust,
    ) {
        let key = address.key();
        let entry = self
            .relays
            .entry(key.clone())
            .or_insert_with(|| RelayEntry {
                relay_id: String::new(),
                password: String::new(),
                online_id: None,
                display_name: None,
                relay_key: None,
                device: None,
                others: Vec::new(),
            });
        entry.password = password.to_owned();
        entry.relay_key = trust.relay_key;
        entry.put_first(SavedAccount {
            relay_id: relay_id.to_string(),
            online_id: Some(account.online_id.to_string()),
            display_name: Some(account.display_name.clone()),
            device: trust.device.clone(),
        });
        self.address = key.clone();
        self.joined = Some(key);
    }

    pub fn accounts_on(&self, key: &str) -> Vec<SavedAccount> {
        self.relays
            .get(key)
            .map(RelayEntry::accounts)
            .unwrap_or_default()
    }

    pub fn other_accounts(&self) -> Vec<SavedAccount> {
        self.joined_entry()
            .map(|entry| entry.others.clone())
            .unwrap_or_default()
    }

    pub fn switch_to(&mut self, key: &str, relay_id: &str) -> Option<JoinedRelay> {
        let entry = self.relays.get_mut(key)?;
        let account = entry.take(relay_id)?;
        entry.put_first(account);
        self.address = key.to_owned();
        self.joined = Some(key.to_owned());
        self.joined_relay()
    }

    pub fn forget(&mut self, key: &str) {
        self.relays.remove(key);
        if self.joined.as_deref() == Some(key) {
            self.joined = None;
        }
    }

    pub fn forget_account(&mut self, key: &str, relay_id: &str) {
        let Some(entry) = self.relays.get_mut(key) else {
            return;
        };
        let current = entry.relay_id == relay_id;
        entry.take(relay_id);
        if current && self.joined.as_deref() == Some(key) {
            self.joined = None;
        }
    }

    pub fn forget_current(&mut self, key: &str) {
        let relay_id = self
            .relays
            .get(key)
            .map(|entry| entry.relay_id.clone())
            .unwrap_or_default();
        self.forget_account(key, &relay_id);
        if self.joined.as_deref() == Some(key) {
            self.joined = None;
        }
    }

    pub fn note_account(&mut self, account: &Account) {
        if let Some(key) = &self.joined
            && let Some(entry) = self.relays.get_mut(key)
        {
            entry.online_id = Some(account.online_id.to_string());
            entry.display_name = Some(account.display_name.clone());
        }
    }

    pub fn set_trust(&mut self, trust: &Trust) {
        if let Some(key) = &self.joined
            && let Some(entry) = self.relays.get_mut(key)
        {
            entry.relay_key = trust.relay_key;
            entry.device = trust.device.clone();
        }
    }

    pub fn set_password(&mut self, password: &str) {
        if let Some(key) = &self.joined
            && let Some(entry) = self.relays.get_mut(key)
        {
            entry.password = password.to_owned();
        }
    }

    pub fn leave(&mut self) {
        self.joined = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::OnlineId;

    fn account(name: &str) -> Account {
        Account {
            online_id: OnlineId::new(0x1234_5678).unwrap(),
            display_name: name.into(),
            storage_used: 0,
            quota_bytes: 0,
            two_factor: false,
            codes_left: 0,
        }
    }

    fn address(text: &str) -> ServerAddress {
        ServerAddress::parse(text).unwrap()
    }

    #[test]
    fn each_server_keeps_its_own_relay_id() {
        let mut settings = OnlineSettings::default();
        let lemon = RelayId::random();
        let other = RelayId::random();
        settings.remember(
            &address("gb.lemon.industries"),
            &lemon,
            "pw",
            &account("Clara"),
            &Trust::default(),
        );
        settings.remember(
            &address("relay.example.org:8443"),
            &other,
            "",
            &account("Zitrone"),
            &Trust::default(),
        );
        assert_eq!(settings.joined.as_deref(), Some("relay.example.org:8443"));

        let back = settings
            .entry(&address("https://GB.lemon.industries/"))
            .unwrap();
        assert_eq!(back.relay_id().unwrap().as_bytes(), lemon.as_bytes());
        assert_eq!(back.password, "pw");
        let joined = settings.joined_relay().unwrap();
        assert_eq!(joined.relay_id.as_bytes(), other.as_bytes());
        assert_eq!(joined.address, address("relay.example.org:8443"));

        settings.leave();
        assert!(!settings.is_joined());
        assert!(
            settings.entry(&address("relay.example.org:8443")).is_some(),
            "leaving keeps the relay ID for this server"
        );
    }

    #[test]
    fn a_second_account_on_one_server_keeps_the_first() {
        let mut settings = OnlineSettings::default();
        let lemon = address("gb.lemon.industries");
        let (ana, ben) = (RelayId::random(), RelayId::random());
        let trust = |byte| Trust {
            relay_key: Some(RelayKey([4; 32])),
            expected: None,
            device: Some(DeviceKey([byte; 32])),
        };
        settings.remember(&lemon, &ana, "pw", &account("Ana"), &trust(1));
        settings.leave();
        settings.remember(&lemon, &ben, "pw", &account("Ben"), &trust(2));
        let names = |s: &OnlineSettings| {
            s.accounts_on("gb.lemon.industries")
                .iter()
                .map(|a| a.name().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&settings), ["Ben", "Ana"]);
        assert_eq!(settings.other_accounts().len(), 1);

        let joined = settings
            .switch_to("gb.lemon.industries", &ana.to_string())
            .unwrap();
        assert_eq!(joined.relay_id.as_bytes(), ana.as_bytes());
        assert_eq!(joined.password, "pw");
        assert_eq!(joined.trust.device, Some(DeviceKey([1; 32])));
        assert_eq!(names(&settings), ["Ana", "Ben"]);

        settings.remember(&lemon, &ana, "pw", &account("Ana"), &trust(1));
        assert_eq!(names(&settings), ["Ana", "Ben"]);
    }

    #[test]
    fn losing_an_account_keeps_the_server_and_the_other_accounts() {
        let mut settings = OnlineSettings::default();
        let lemon = address("gb.lemon.industries");
        let ben = RelayId::random();
        settings.remember(
            &lemon,
            &RelayId::random(),
            "pw",
            &account("Ana"),
            &Trust::default(),
        );
        settings.remember(&lemon, &ben, "pw", &account("Ben"), &Trust::default());
        settings.forget_current("gb.lemon.industries");
        assert!(!settings.is_joined());
        let entry = settings.entry(&lemon).unwrap();
        assert_eq!(entry.password, "pw");
        assert_eq!(entry.display_name.as_deref(), Some("Ana"));
        assert!(entry.others.is_empty());

        settings.forget_current("gb.lemon.industries");
        let entry = settings.entry(&lemon).unwrap();
        assert_eq!(entry.password, "pw");
        assert!(entry.relay_id().is_none());
        assert!(settings.accounts_on("gb.lemon.industries").is_empty());
        assert_eq!(settings.address, "gb.lemon.industries");
        settings.forget_current("gb.lemon.industries");
        assert_eq!(settings.entry(&lemon).unwrap().password, "pw");
    }

    #[test]
    fn online_settings_survive_the_settings_file() {
        let mut settings = crate::app::ui_state::settings::Settings::default();
        let id = RelayId::random();
        let trust = Trust {
            relay_key: Some(RelayKey([4; 32])),
            expected: None,
            device: Some(DeviceKey([5; 32])),
        };
        settings.online.remember(
            &address("gb.lemon.industries"),
            &id,
            "lemon tree",
            &account("Clara"),
            &trust,
        );
        settings.online.show_playing = false;

        let root =
            std::env::temp_dir().join(format!("citrine-online-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = crate::storage::SettingsStore::at(root.clone());
        store.store(&settings).unwrap();
        let loaded: crate::app::ui_state::settings::Settings = store.load().unwrap();
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(loaded.online, settings.online);
        let joined = loaded.online.joined_relay().unwrap();
        assert_eq!(joined.relay_id.as_bytes(), id.as_bytes());
        assert_eq!(joined.relay_id.keys().public_key(), id.keys().public_key());
        assert_eq!(joined.password, "lemon tree");
        assert_eq!(joined.trust, trust);
        assert!(!loaded.online.show_playing);
    }

    #[test]
    fn an_older_entry_loads_without_a_key_or_device() {
        let settings: OnlineSettings = serde_json::from_str(
            r#"{"joined": "x.org", "relays": {"x.org": {"relay_id": "cgb1-x", "password": "pw"}}}"#,
        )
        .unwrap();
        let entry = settings.joined_entry().unwrap();
        assert_eq!(entry.trust(), Trust::default());
    }

    #[test]
    fn older_settings_without_online_still_load() {
        let settings: crate::app::ui_state::settings::Settings =
            serde_json::from_str(r#"{"ui_scale": 1.5}"#).unwrap();
        assert!(!settings.online.is_joined());
        assert!(settings.online.show_playing);
    }

    #[test]
    fn debug_never_shows_the_relay_id_or_password() {
        let mut settings = OnlineSettings::default();
        let id = RelayId::random();
        let trust = Trust {
            relay_key: None,
            expected: None,
            device: Some(DeviceKey([0xEE; 32])),
        };
        settings.remember(&address("x.org"), &id, "hunter2", &account("Clara"), &trust);
        let text = format!("{settings:?}");
        assert!(!text.contains("238"));
        assert!(!text.contains(&id.to_string()));
        assert!(!text.contains("hunter2"));
    }
}
