use crate::online::address::ServerAddress;
use citrine_gb_net::{Account, RelayId};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OnlineSettings {
    pub address: String,
    pub joined: Option<String>,
    pub relays: BTreeMap<String, RelayEntry>,
    pub show_playing: bool,
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
}

impl fmt::Debug for RelayEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RelayEntry")
            .field("online_id", &self.online_id)
            .field("display_name", &self.display_name)
            .finish_non_exhaustive()
    }
}

impl RelayEntry {
    pub fn relay_id(&self) -> Option<RelayId> {
        self.relay_id.parse().ok()
    }
}

pub struct JoinedRelay {
    pub address: ServerAddress,
    pub relay_id: RelayId,
    pub password: String,
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
    ) {
        let key = address.key();
        self.relays.insert(
            key.clone(),
            RelayEntry {
                relay_id: relay_id.to_string(),
                password: password.to_owned(),
                online_id: Some(account.online_id.to_string()),
                display_name: Some(account.display_name.clone()),
            },
        );
        self.address = key.clone();
        self.joined = Some(key);
    }

    pub fn note_account(&mut self, account: &Account) {
        if let Some(key) = &self.joined
            && let Some(entry) = self.relays.get_mut(key)
        {
            entry.online_id = Some(account.online_id.to_string());
            entry.display_name = Some(account.display_name.clone());
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

    pub fn forget(&mut self, key: &str) {
        self.relays.remove(key);
        if self.joined.as_deref() == Some(key) {
            self.joined = None;
        }
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
        );
        settings.remember(
            &address("relay.example.org:8443"),
            &other,
            "",
            &account("Zitrone"),
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
    fn a_forgotten_server_keeps_nothing() {
        let mut settings = OnlineSettings::default();
        settings.remember(
            &address("gb.lemon.industries"),
            &RelayId::random(),
            "pw",
            &account("Clara"),
        );
        settings.leave();
        settings.forget("gb.lemon.industries");
        assert!(settings.entry(&address("gb.lemon.industries")).is_none());
        assert!(!settings.is_joined());
    }

    #[test]
    fn online_settings_survive_the_settings_file() {
        let mut settings = crate::app::ui_state::settings::Settings::default();
        let id = RelayId::random();
        settings.online.remember(
            &address("gb.lemon.industries"),
            &id,
            "lemon tree",
            &account("Clara"),
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
        assert!(!loaded.online.show_playing);
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
        settings.remember(&address("x.org"), &id, "hunter2", &account("Clara"));
        let text = format!("{settings:?}");
        assert!(!text.contains(&id.to_string()));
        assert!(!text.contains("hunter2"));
    }
}
