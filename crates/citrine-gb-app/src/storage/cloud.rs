use super::{SaveStore, autosave_cloud, decode_meta, decompress};
use citrine_gb_net::SavePayload;
use std::collections::BTreeSet;

const LEDGER_NAMESPACE: &str = "cloud";
const BATTERY_STEM: &str = "battery";
const OTHER_BATTERY: &str = "battery-";
const SNAPSHOT_SLOT: &str = "slot-";
const AUTOSAVE_SLOT: &str = "auto-";
const BATTERY_ROLES: &[&str] = &["sav", "meta", "name"];
const STATE_ROLES: &[&str] = &["snap", "meta", "thumb", "name"];
const AUTOSAVE_ROLES: &[&str] = &["snap", "meta", "thumb"];

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LocalSave {
    Battery,
    OtherBattery(String),
    Snapshot(usize),
    Autosave(usize),
}

impl LocalSave {
    fn stem(&self) -> String {
        match self {
            Self::Battery => BATTERY_STEM.to_string(),
            Self::OtherBattery(token) => format!("{OTHER_BATTERY}{token}"),
            Self::Snapshot(slot) => format!("slot{slot}"),
            Self::Autosave(id) => format!("auto{id}"),
        }
    }

    fn roles(&self) -> &'static [&'static str] {
        match self {
            Self::Battery | Self::OtherBattery(_) => BATTERY_ROLES,
            Self::Snapshot(_) => STATE_ROLES,
            Self::Autosave(_) => AUTOSAVE_ROLES,
        }
    }

    fn file(&self, role: &str) -> String {
        format!("{}.{role}", self.stem())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Battery,
    OtherBattery,
    Snapshot(usize),
    Autosave,
}

pub fn slot_kind(slot: &str) -> Option<SlotKind> {
    if slot == BATTERY_STEM {
        return Some(SlotKind::Battery);
    }
    if slot.starts_with(OTHER_BATTERY) {
        return Some(SlotKind::OtherBattery);
    }
    if let Some(n) = slot.strip_prefix(SNAPSHOT_SLOT) {
        return n.parse().ok().map(SlotKind::Snapshot);
    }
    slot.starts_with(AUTOSAVE_SLOT)
        .then_some(SlotKind::Autosave)
}

pub fn battery_slot(token: Option<&str>) -> String {
    match token {
        None => BATTERY_STEM.to_string(),
        Some(token) => format!("{OTHER_BATTERY}{token}"),
    }
}

pub fn snapshot_slot(slot: usize) -> String {
    format!("{SNAPSHOT_SLOT}{slot}")
}

pub fn new_autosave_slot(saved_at: u64, device: &str) -> String {
    format!("{AUTOSAVE_SLOT}{saved_at}-{device}")
}

pub fn is_game_key(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn random_token() -> String {
    let mut bytes = [0u8; 4];
    getrandom::getrandom(&mut bytes).expect("no system random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct LocalItem {
    pub save: LocalSave,
    pub slot: String,
    pub payload: SavePayload,
    pub saved_at: u64,
}

pub struct OtherBattery {
    pub token: String,
    pub name: Option<String>,
    pub saved_at: Option<u64>,
}

impl SaveStore {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_dir(root: std::path::PathBuf) -> Self {
        Self {
            backend: super::Backend::at(root),
            touched: Default::default(),
        }
    }

    pub fn take_touched(&self) -> BTreeSet<String> {
        std::mem::take(
            &mut *self
                .touched
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    pub fn games(&self) -> Vec<String> {
        self.backend
            .namespaces()
            .into_iter()
            .filter(|ns| is_game_key(ns))
            .collect()
    }

    pub fn read_ledger(&self, name: &str) -> Option<Vec<u8>> {
        self.backend.read(LEDGER_NAMESPACE, name)
    }

    pub fn ledger_names(&self) -> Vec<String> {
        self.backend.list(LEDGER_NAMESPACE)
    }

    pub fn write_ledger(&self, name: &str, data: &[u8]) -> std::io::Result<()> {
        self.backend.write(LEDGER_NAMESPACE, name, data)
    }

    pub fn read_payload(&self, rom_key: &str, save: &LocalSave) -> Option<SavePayload> {
        let primary = save.roles()[0];
        if !self.backend.exists(rom_key, &save.file(primary)) {
            return None;
        }
        let files = save
            .roles()
            .iter()
            .filter_map(|role| {
                self.backend
                    .read(rom_key, &save.file(role))
                    .map(|data| (role.to_string(), data))
            })
            .collect();
        Some(SavePayload { files })
    }

    pub fn write_payload(
        &self,
        rom_key: &str,
        save: &LocalSave,
        payload: &SavePayload,
    ) -> std::io::Result<()> {
        let primary = save.roles()[0];
        if payload.file(primary).is_none() {
            return Err(std::io::Error::other("the save has no data"));
        }
        for role in save.roles().iter().rev() {
            match payload.file(role) {
                Some(data) => self.write(rom_key, &save.file(role), data)?,
                None => self.delete(rom_key, &save.file(role))?,
            }
        }
        Ok(())
    }

    pub fn delete_save(&self, rom_key: &str, save: &LocalSave) -> std::io::Result<()> {
        if let LocalSave::Autosave(id) = save {
            self.delete(rom_key, &autosave_cloud(*id))?;
        }
        for role in save.roles().iter().rev() {
            self.delete(rom_key, &save.file(role))?;
        }
        Ok(())
    }

    pub fn autosave_slot(&self, rom_key: &str, id: usize) -> Option<String> {
        let raw = self.backend.read(rom_key, &autosave_cloud(id))?;
        String::from_utf8(raw)
            .ok()
            .filter(|slot| citrine_gb_net::valid_slot(slot))
    }

    pub fn add_autosave(
        &self,
        rom_key: &str,
        slot: &str,
        payload: &SavePayload,
    ) -> std::io::Result<usize> {
        let id = self
            .numbered(rom_key, "auto")
            .into_iter()
            .max()
            .map_or(0, |last| last + 1);
        let save = LocalSave::Autosave(id);
        self.write_payload(rom_key, &save, payload)?;
        self.write(rom_key, &autosave_cloud(id), slot.as_bytes())?;
        Ok(id)
    }

    pub fn local_items(&self, rom_key: &str, device: &str) -> Vec<LocalItem> {
        let mut saves = vec![LocalSave::Battery];
        saves.extend(
            self.other_battery_tokens(rom_key)
                .into_iter()
                .map(LocalSave::OtherBattery),
        );
        saves.extend(
            self.numbered(rom_key, "slot")
                .into_iter()
                .map(LocalSave::Snapshot),
        );
        saves.extend(
            self.numbered(rom_key, "auto")
                .into_iter()
                .map(LocalSave::Autosave),
        );
        saves
            .into_iter()
            .filter_map(|save| {
                let payload = self.read_payload(rom_key, &save)?;
                let saved_at = saved_at_of(&payload);
                let slot = match &save {
                    LocalSave::Battery => battery_slot(None),
                    LocalSave::OtherBattery(token) => battery_slot(Some(token)),
                    LocalSave::Snapshot(slot) => snapshot_slot(*slot),
                    LocalSave::Autosave(id) => match self.autosave_slot(rom_key, *id) {
                        Some(slot) => slot,
                        None => {
                            let slot = new_autosave_slot(saved_at, device);
                            if let Err(err) =
                                self.write(rom_key, &autosave_cloud(*id), slot.as_bytes())
                            {
                                log::warn!("could not name autosave {id} for the cloud: {err}");
                            }
                            slot
                        }
                    },
                };
                Some(LocalItem {
                    save,
                    slot,
                    payload,
                    saved_at,
                })
            })
            .collect()
    }

    fn other_battery_tokens(&self, rom_key: &str) -> Vec<String> {
        let mut tokens: Vec<String> = self
            .backend
            .list(rom_key)
            .iter()
            .filter_map(|name| {
                name.strip_prefix(OTHER_BATTERY)
                    .and_then(|rest| rest.strip_suffix(".sav"))
                    .filter(|token| !token.is_empty() && !token.contains('.'))
                    .map(str::to_owned)
            })
            .collect();
        tokens.sort();
        tokens
    }

    pub fn other_batteries(&self, rom_key: &str) -> Vec<OtherBattery> {
        let mut found: Vec<OtherBattery> = self
            .other_battery_tokens(rom_key)
            .into_iter()
            .map(|token| {
                let save = LocalSave::OtherBattery(token.clone());
                let name = self.read_name(rom_key, &save.file("name"));
                let saved_at = self
                    .backend
                    .read(rom_key, &save.file("meta"))
                    .and_then(|b| b.get(..8).and_then(|b| b.try_into().ok()))
                    .map(u64::from_le_bytes);
                OtherBattery {
                    token,
                    name,
                    saved_at,
                }
            })
            .collect();
        found.sort_by_key(|b| std::cmp::Reverse(b.saved_at));
        found
    }

    pub fn add_other_battery(
        &self,
        rom_key: &str,
        payload: &SavePayload,
    ) -> std::io::Result<String> {
        let token = random_token();
        self.write_payload(rom_key, &LocalSave::OtherBattery(token.clone()), payload)?;
        Ok(token)
    }

    pub fn swap_battery(&self, rom_key: &str, token: &str) -> std::io::Result<()> {
        let other = LocalSave::OtherBattery(token.to_string());
        let Some(chosen) = self.read_payload(rom_key, &other) else {
            return Err(std::io::Error::other("that battery save is gone"));
        };
        match self.read_payload(rom_key, &LocalSave::Battery) {
            Some(current) => self.write_payload(rom_key, &other, &current)?,
            None => self.delete_save(rom_key, &other)?,
        }
        self.write_payload(rom_key, &LocalSave::Battery, &chosen)
    }

    pub fn set_other_battery_name(
        &self,
        rom_key: &str,
        token: &str,
        name: &str,
    ) -> std::io::Result<()> {
        let save = LocalSave::OtherBattery(token.to_string());
        self.write_name(rom_key, &save.file("name"), name, "")
    }
}

pub fn thumb_of(payload: &SavePayload) -> Option<Vec<u8>> {
    payload.file("thumb").and_then(decompress)
}

pub fn saved_at_of(payload: &SavePayload) -> u64 {
    payload
        .file("meta")
        .and_then(|meta| decode_meta(meta).0)
        .unwrap_or(0)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use citrine_gb::gb::GbModel;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn new_store(name: &str) -> (SaveStore, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("citrine-cloud-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        (SaveStore::in_dir(root.clone()), root)
    }

    #[test]
    fn every_kind_of_save_is_one_item_that_round_trips() {
        let (store, root) = new_store("items");
        store.store_battery(KEY, &[1; 64]).unwrap();
        store
            .store_snapshot(KEY, 2, &[2; 50], &[0; 160 * 144 * 4], GbModel::Cgb)
            .unwrap();
        store
            .store_autosave(KEY, &[3; 40], &[0; 160 * 144 * 4], GbModel::Dmg, 5)
            .unwrap();
        let items = store.local_items(KEY, "dev1");
        let slots: Vec<&str> = items.iter().map(|i| i.slot.as_str()).collect();
        assert_eq!(slots[..2], ["battery", "slot-2"]);
        assert!(slots[2].starts_with("auto-") && slots[2].ends_with("-dev1"));
        assert_eq!(store.local_items(KEY, "other")[2].slot, slots[2]);

        let (copy, copy_root) = new_store("items-copy");
        for item in &items {
            copy.write_payload(KEY, &item.save, &item.payload).unwrap();
        }
        assert_eq!(copy.load_battery(KEY), Some(vec![1; 64]));
        assert_eq!(copy.load_snapshot(KEY, 2), Some(vec![2; 50]));
        let again = copy.local_items(KEY, "dev2");
        assert_eq!(again[0].payload.hash(), items[0].payload.hash());
        assert_eq!(again[1].payload.hash(), items[1].payload.hash());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(copy_root);
    }

    #[test]
    fn writes_note_the_game_once() {
        let (store, root) = new_store("touched");
        store.store_battery(KEY, &[1; 8]).unwrap();
        store.store_battery(KEY, &[2; 8]).unwrap();
        store.store_battery("borrowed-abc", &[2; 8]).unwrap();
        assert_eq!(store.take_touched().into_iter().collect::<Vec<_>>(), [KEY]);
        assert!(store.take_touched().is_empty());
        assert_eq!(store.games(), [KEY]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn another_battery_save_can_be_swapped_in() {
        let (store, root) = new_store("other-battery");
        store.store_battery(KEY, &[1; 8]).unwrap();
        let mut payload = store.read_payload(KEY, &LocalSave::Battery).unwrap();
        payload.files[0].1 = vec![9; 8];
        let token = store.add_other_battery(KEY, &payload).unwrap();
        assert_eq!(store.other_batteries(KEY).len(), 1);
        store.swap_battery(KEY, &token).unwrap();
        assert_eq!(store.load_battery(KEY), Some(vec![9; 8]));
        let other = LocalSave::OtherBattery(token);
        let swapped = store.read_payload(KEY, &other).unwrap();
        assert_eq!(swapped.file("sav"), Some(&[1u8; 8][..]));
        store.delete_save(KEY, &other).unwrap();
        assert!(store.other_batteries(KEY).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn autosaves_order_by_when_they_were_made() {
        let (store, root) = new_store("autosave-order");
        let frame = vec![0; 160 * 144 * 4];
        store
            .store_autosave(KEY, &[1; 8], &frame, GbModel::Dmg, 5)
            .unwrap();
        let mut older = store.read_payload(KEY, &LocalSave::Autosave(0)).unwrap();
        for (role, data) in &mut older.files {
            if role == "meta" {
                data[..8].copy_from_slice(&1_000u64.to_le_bytes());
            }
        }
        let id = store.add_autosave(KEY, "auto-1000-dev2", &older).unwrap();
        assert_eq!(id, 1);
        assert_eq!(store.autosave_ids(KEY), [1, 0]);
        assert_eq!(
            store.autosave_slot(KEY, 1).as_deref(),
            Some("auto-1000-dev2")
        );
        store.prune_autosaves(KEY, 1).unwrap();
        assert_eq!(store.autosave_ids(KEY), [0]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn slot_names_say_what_they_hold() {
        assert_eq!(slot_kind("battery"), Some(SlotKind::Battery));
        assert_eq!(slot_kind("battery-ab12cd34"), Some(SlotKind::OtherBattery));
        assert_eq!(slot_kind("slot-4"), Some(SlotKind::Snapshot(4)));
        assert_eq!(slot_kind("auto-5-ab"), Some(SlotKind::Autosave));
        assert_eq!(slot_kind("thing"), None);
        assert!(is_game_key(KEY));
        assert!(!is_game_key("cloud"));
    }
}
