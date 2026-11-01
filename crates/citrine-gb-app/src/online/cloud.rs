mod account;

use super::{Online, Phase};
pub use crate::storage::SaveStore;
pub use crate::storage::cloud::{LocalSave, OtherBattery, thumb_of};
use crate::storage::cloud::{SlotKind, battery_slot, is_game_key, random_token, slot_kind};
use crate::storage::{MAX_SAVE_NAME, default_slot_name};
pub use account::AccountCopy;
use citrine_gb_net::{
    ClientMsg, ErrorCode, GameKey, MAX_SAVE_BYTES, SaveInfo, SaveLabel, SavePayload, ServerMsg, Tag,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use web_time::Instant;

const SETTLE: Duration = Duration::from_secs(3);
const UNKNOWN: [u8; 32] = [0; 32];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Mark {
    Synced,
    CloudGone,
    DeletedHere { cloud_too: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    version: u64,
    hash: [u8; 32],
    mark: Mark,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GameLedger {
    title: String,
    dirty: bool,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Ledger {
    device: String,
    enabled: bool,
    games: BTreeMap<String, GameLedger>,
}

impl Ledger {
    fn new() -> Self {
        Self {
            device: random_token(),
            enabled: false,
            games: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Synced,
    Waiting,
    Fetching,
    Conflict,
    OnlyHere,
    Full,
    TooLarge,
    NoStorage,
    Unreadable,
}

impl SyncState {
    pub fn text(self) -> &'static str {
        match self {
            Self::Synced => "Saved in the cloud",
            Self::Waiting => "Waiting to upload",
            Self::Fetching => "Getting the newer copy from the cloud",
            Self::Conflict => "Changed here and in the cloud",
            Self::OnlyHere => "Only on this device, the cloud copy was deleted",
            Self::Full => "Not uploaded, your cloud storage is full",
            Self::TooLarge => "Not uploaded, too large for the relay",
            Self::NoStorage => "Not uploaded, this relay has no storage",
            Self::Unreadable => "Not downloaded, the cloud copy could not be opened",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Keep,
    Adopt,
    Upload(u64),
    Fetch,
    Conflict,
    CloudGone,
    DeleteCloud(u64),
    Forget,
}

fn decide(
    local: Option<[u8; 32]>,
    cloud: Option<(u64, [u8; 32])>,
    entry: Option<&Entry>,
    autosave: bool,
    locked: bool,
) -> Step {
    let fetch = if locked { Step::Conflict } else { Step::Fetch };
    match (local, cloud, entry) {
        (Some(here), Some((_, there)), _) if here == there => Step::Adopt,
        (Some(_), Some((version, _)), None) => {
            if autosave {
                Step::Upload(version)
            } else {
                Step::Conflict
            }
        }
        (Some(here), Some((version, _)), Some(entry)) => {
            match (here != entry.hash, version != entry.version) {
                (true, false) => Step::Upload(version),
                (false, true) => fetch,
                (true, true) if autosave => Step::Upload(version),
                (true, true) => Step::Conflict,
                (false, false) => Step::Keep,
            }
        }
        (Some(_), None, None) => Step::Upload(0),
        (Some(here), None, Some(entry)) => match entry.mark {
            Mark::Synced if here == entry.hash => Step::CloudGone,
            Mark::CloudGone if here == entry.hash => Step::Keep,
            _ => Step::Upload(0),
        },
        (None, Some((version, _)), Some(entry)) if version == entry.version => match entry.mark {
            Mark::DeletedHere { cloud_too: true } => Step::DeleteCloud(version),
            Mark::DeletedHere { cloud_too: false } => Step::Keep,
            Mark::Synced if autosave => Step::DeleteCloud(version),
            _ => fetch,
        },
        (None, Some(_), _) => fetch,
        (None, None, Some(_)) => Step::Forget,
        (None, None, None) => Step::Keep,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    KeepHere,
    KeepCloud,
    KeepBoth,
}

#[derive(Debug, Clone)]
pub struct ConflictSide {
    pub device: String,
    pub saved_at: u64,
    pub thumb: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub rom: String,
    pub slot: String,
    pub save: Option<LocalSave>,
    pub name: String,
    pub here: Option<ConflictSide>,
    pub cloud: ConflictSide,
    version: u64,
    payload: Option<SavePayload>,
    dismissed: bool,
}

impl Conflict {
    pub fn ready(&self) -> bool {
        self.payload.is_some()
    }

    pub fn dismissed(&self) -> bool {
        self.dismissed
    }
}

#[derive(Debug, Clone)]
pub struct CloudGame {
    pub game: GameKey,
    pub title: String,
    pub bytes: u64,
    pub saves: usize,
}

#[derive(Debug, Clone)]
struct Remote {
    info: SaveInfo,
    label: Option<SaveLabel>,
}

impl Remote {
    fn hash(&self) -> [u8; 32] {
        self.label.as_ref().map_or(UNKNOWN, |l| l.hash)
    }
}

enum Op {
    Put {
        tag: Tag,
        rom: String,
        slot: String,
        hash: [u8; 32],
        label: SaveLabel,
    },
    Get {
        game: GameKey,
        slot: String,
    },
    Delete {
        tag: Tag,
        game: GameKey,
        slot: String,
    },
    AccountPut {
        tag: Tag,
        name: String,
        slot: String,
        hash: [u8; 32],
        label: SaveLabel,
    },
}

enum Action {
    Upload {
        rom: String,
        slot: String,
        save: LocalSave,
        base: u64,
        payload: SavePayload,
    },
    Get {
        slot: String,
    },
    Delete {
        slot: String,
        base: u64,
    },
}

#[derive(Default)]
pub struct CloudState {
    store: Option<SaveStore>,
    owner: Option<String>,
    enabled: bool,
    ledger: Option<Ledger>,
    ledger_dirty: bool,
    listing: Option<BTreeMap<(GameKey, String), Remote>>,
    asked: bool,
    rom_of: BTreeMap<GameKey, String>,
    names: BTreeMap<String, String>,
    due: BTreeMap<String, Instant>,
    op: Option<Op>,
    deletes: Vec<(GameKey, String, u64)>,
    next_tag: Tag,
    states: BTreeMap<(String, LocalSave), SyncState>,
    conflicts: BTreeMap<(String, String), Conflict>,
    refusals: BTreeMap<(String, String), ([u8; 32], SyncState)>,
    unreadable: BTreeSet<(GameKey, String, u64)>,
    fetching: BTreeSet<(String, String)>,
    open: Option<(String, String)>,
    holding: Option<String>,
    held_checked: bool,
    reload: bool,
    changed: bool,
    storage: Option<(u64, u64)>,
    account: account::AccountSlots,
}

fn ledger_name(owner: &str) -> String {
    let safe: String = owner
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{safe}.bin")
}

fn rom_bytes(rom: &str) -> Vec<u8> {
    (0..rom.len() / 2)
        .filter_map(|i| u8::from_str_radix(&rom[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

fn name_of(payload: &SavePayload) -> String {
    payload
        .file("name")
        .and_then(|n| std::str::from_utf8(n).ok())
        .unwrap_or_default()
        .to_string()
}

fn default_name(slot: &str) -> String {
    match slot_kind(slot) {
        Some(SlotKind::Snapshot(n)) => default_slot_name(n),
        Some(SlotKind::Autosave) => "Autosave".to_string(),
        _ => "Battery save".to_string(),
    }
}

fn copy_name(name: &str, slot: &str, device: &str) -> String {
    let name = if name.is_empty() {
        default_name(slot)
    } else {
        name.to_string()
    };
    let full = format!("{name} from {device}");
    full.chars().take(MAX_SAVE_NAME).collect()
}

fn slot_order(slot: &str) -> u8 {
    match slot_kind(slot) {
        Some(SlotKind::Battery) => 0,
        Some(SlotKind::OtherBattery) => 1,
        _ => 2,
    }
}

impl CloudState {
    fn tag(&mut self) -> Tag {
        self.next_tag = self.next_tag.wrapping_add(1);
        self.next_tag
    }

    fn ledger(&mut self) -> &mut Ledger {
        self.ledger.get_or_insert_with(Ledger::new)
    }

    fn game(&mut self, rom: &str) -> &mut GameLedger {
        self.ledger_dirty = true;
        self.ledger().games.entry(rom.to_string()).or_default()
    }

    fn entry(&self, rom: &str, slot: &str) -> Option<&Entry> {
        self.ledger.as_ref()?.games.get(rom)?.entries.get(slot)
    }

    fn set_entry(&mut self, rom: &str, slot: &str, entry: Option<Entry>) {
        let game = self.game(rom);
        match entry {
            Some(entry) => game.entries.insert(slot.to_string(), entry),
            None => game.entries.remove(slot),
        };
    }

    fn device(&mut self) -> String {
        self.ledger().device.clone()
    }

    fn load(&mut self, owner: &str) {
        let Some(store) = &self.store else {
            return;
        };
        self.ledger = store
            .read_ledger(&ledger_name(owner))
            .and_then(|bytes| postcard::from_bytes(&bytes).ok());
        self.ledger_dirty = false;
    }

    fn save_ledger(&mut self) {
        if !self.ledger_dirty {
            return;
        }
        self.ledger_dirty = false;
        let (Some(store), Some(owner), Some(ledger)) = (&self.store, &self.owner, &self.ledger)
        else {
            return;
        };
        match postcard::to_allocvec(ledger) {
            Ok(bytes) => {
                if let Err(err) = store.write_ledger(&ledger_name(owner), &bytes) {
                    log::warn!("could not keep the cloud save list: {err}");
                }
            }
            Err(err) => log::warn!("could not encode the cloud save list: {err}"),
        }
    }

    fn disconnect(&mut self) {
        self.listing = None;
        self.asked = false;
        self.op = None;
        self.deletes.clear();
        self.refusals.clear();
        self.conflicts.clear();
        self.fetching.clear();
        self.unreadable.clear();
        let roms: Vec<String> = self.states.keys().map(|(rom, _)| rom.clone()).collect();
        self.states.clear();
        let now = Instant::now();
        for rom in roms {
            self.due.entry(rom).or_insert(now);
        }
        self.changed = true;
    }

    fn mark_all_dirty(&mut self) {
        let games = self
            .store
            .as_ref()
            .map(SaveStore::games)
            .unwrap_or_default();
        let now = Instant::now();
        for rom in games {
            self.game(&rom).dirty = true;
            self.due.insert(rom, now);
        }
    }

    fn local_save(&self, rom: &str, slot: &str) -> Option<LocalSave> {
        match slot_kind(slot)? {
            SlotKind::Battery => Some(LocalSave::Battery),
            SlotKind::OtherBattery => slot
                .strip_prefix(&battery_slot(Some("")))
                .filter(|token| !token.is_empty() && !token.contains('.'))
                .map(|token| LocalSave::OtherBattery(token.to_string())),
            SlotKind::Snapshot(n) => Some(LocalSave::Snapshot(n)),
            SlotKind::Autosave => {
                let store = self.store.as_ref()?;
                store
                    .autosave_ids(rom)
                    .into_iter()
                    .find(|id| store.autosave_slot(rom, *id).as_deref() == Some(slot))
                    .map(LocalSave::Autosave)
            }
        }
    }

    fn slot_of(&self, rom: &str, save: &LocalSave) -> Option<String> {
        match save {
            LocalSave::Battery => Some(battery_slot(None)),
            LocalSave::OtherBattery(token) => Some(battery_slot(Some(token))),
            LocalSave::Snapshot(n) => Some(crate::storage::cloud::snapshot_slot(*n)),
            LocalSave::Autosave(id) => self.store.as_ref()?.autosave_slot(rom, *id),
        }
    }

    fn offline_states(&mut self, rom: &str) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let device = self.device();
        self.states.retain(|(r, _), _| r != rom);
        for item in store.local_items(rom, &device) {
            let state = match self.entry(rom, &item.slot) {
                Some(entry) if entry.hash == item.payload.hash() => match entry.mark {
                    Mark::Synced => SyncState::Synced,
                    Mark::CloudGone => SyncState::OnlyHere,
                    Mark::DeletedHere { .. } => SyncState::Waiting,
                },
                _ => SyncState::Waiting,
            };
            self.states.insert((rom.to_string(), item.save), state);
        }
        self.changed = true;
    }

    fn reconcile(&mut self, rom: &str, game: GameKey) -> Option<Action> {
        let store = self.store.clone()?;
        let device = self.device();
        let items = store.local_items(rom, &device);
        let remote: BTreeMap<String, Remote> = self
            .listing
            .as_ref()?
            .range((game, String::new())..)
            .take_while(|((g, _), _)| *g == game)
            .map(|((_, slot), remote)| (slot.clone(), remote.clone()))
            .collect();
        let entries = self
            .ledger
            .as_ref()
            .and_then(|l| l.games.get(rom))
            .map(|g| g.entries.clone())
            .unwrap_or_default();
        let mut slots: Vec<(u8, std::cmp::Reverse<u64>, String)> = Vec::new();
        let mut seen = BTreeSet::new();
        for item in &items {
            seen.insert(item.slot.clone());
            slots.push((
                slot_order(&item.slot),
                std::cmp::Reverse(item.saved_at),
                item.slot.clone(),
            ));
        }
        for (slot, remote) in &remote {
            if seen.insert(slot.clone()) {
                let saved_at = remote
                    .label
                    .as_ref()
                    .map_or(remote.info.updated, |l| l.saved_at);
                slots.push((slot_order(slot), std::cmp::Reverse(saved_at), slot.clone()));
            }
        }
        for slot in entries.keys() {
            if seen.insert(slot.clone()) {
                slots.push((slot_order(slot), std::cmp::Reverse(0), slot.clone()));
            }
        }
        slots.sort();
        let locked_rom = self.open.as_ref().is_some_and(|(r, _)| r == rom)
            && self.holding.as_deref() != Some(rom);
        self.states.retain(|(r, _), _| r != rom);
        self.fetching.retain(|(r, _)| r != rom);
        if self.holding.as_deref() == Some(rom) {
            self.held_checked = true;
        }
        let mut keep_conflicts = BTreeSet::new();
        let mut action = None;
        for (_, _, slot) in slots {
            let item = items.iter().find(|i| i.slot == slot);
            let local = item.map(|i| i.payload.hash());
            let cloud = remote.get(&slot);
            let entry = entries.get(&slot);
            let autosave = slot_kind(&slot) == Some(SlotKind::Autosave);
            let locked = locked_rom && slot == battery_slot(None);
            let step = decide(
                local,
                cloud.map(|c| (c.info.version, c.hash())),
                entry,
                autosave,
                locked,
            );
            let key = (rom.to_string(), slot.clone());
            let unreadable = cloud.is_some_and(|c| {
                self.unreadable
                    .contains(&(game, slot.clone(), c.info.version))
            });
            let mut state = SyncState::Synced;
            match step {
                Step::Keep => {
                    if entry.is_some_and(|e| e.mark == Mark::CloudGone) {
                        state = SyncState::OnlyHere;
                    }
                }
                Step::Adopt => {
                    let (version, hash) = (cloud?.info.version, local?);
                    if entry.is_none_or(|e| {
                        e.version != version || e.hash != hash || e.mark != Mark::Synced
                    }) {
                        self.set_entry(
                            rom,
                            &slot,
                            Some(Entry {
                                version,
                                hash,
                                mark: Mark::Synced,
                            }),
                        );
                    }
                    let unnamed = !locked
                        && cloud?.label.as_ref().is_some_and(|l| l.game.is_empty())
                        && !self.title_of(rom).is_empty();
                    if !unnamed {
                        self.refusals.remove(&key);
                    } else if let Some((refused_hash, refused)) = self.refusals.get(&key)
                        && *refused_hash == hash
                    {
                        state = *refused;
                    } else {
                        state = SyncState::Waiting;
                        if action.is_none() {
                            action = Some(Action::Upload {
                                rom: rom.to_string(),
                                slot: slot.clone(),
                                save: item?.save.clone(),
                                base: version,
                                payload: item?.payload.clone(),
                            });
                        }
                    }
                }
                Step::CloudGone => {
                    let mut entry = entry.cloned()?;
                    entry.mark = Mark::CloudGone;
                    self.set_entry(rom, &slot, Some(entry));
                    state = SyncState::OnlyHere;
                }
                Step::Forget => self.set_entry(rom, &slot, None),
                Step::Upload(base) => {
                    let item = item?;
                    match self.refusals.get(&key) {
                        Some((hash, refused)) if Some(*hash) == local => state = *refused,
                        _ => {
                            state = SyncState::Waiting;
                            if action.is_none() {
                                action = Some(Action::Upload {
                                    rom: rom.to_string(),
                                    slot: slot.clone(),
                                    save: item.save.clone(),
                                    base,
                                    payload: item.payload.clone(),
                                });
                            }
                        }
                    }
                }
                Step::Fetch => {
                    if unreadable {
                        state = SyncState::Unreadable;
                    } else {
                        state = SyncState::Fetching;
                        self.fetching.insert(key.clone());
                        if action.is_none() {
                            action = Some(Action::Get { slot: slot.clone() });
                        }
                    }
                }
                Step::DeleteCloud(base) => {
                    if action.is_none() {
                        action = Some(Action::Delete {
                            slot: slot.clone(),
                            base,
                        });
                    }
                }
                Step::Conflict => {
                    let remote = cloud?;
                    state = SyncState::Conflict;
                    keep_conflicts.insert(key.clone());
                    let fresh = self.conflicts.get(&key).is_none_or(|c| {
                        c.version != remote.info.version || c.here.is_some() != item.is_some()
                    });
                    if fresh {
                        let label = remote.label.clone();
                        let name = label
                            .as_ref()
                            .map(|l| l.name.clone())
                            .filter(|n| !n.is_empty())
                            .or_else(|| item.map(|i| name_of(&i.payload)).filter(|n| !n.is_empty()))
                            .unwrap_or_else(|| default_name(&slot));
                        let conflict = Conflict {
                            rom: rom.to_string(),
                            slot: slot.clone(),
                            save: item.map(|i| i.save.clone()),
                            name,
                            here: item.map(|i| ConflictSide {
                                device: device_name(),
                                saved_at: i.saved_at,
                                thumb: thumb_of(&i.payload),
                            }),
                            cloud: ConflictSide {
                                device: label
                                    .as_ref()
                                    .map(|l| l.device.clone())
                                    .unwrap_or_default(),
                                saved_at: label
                                    .as_ref()
                                    .map_or(remote.info.updated, |l| l.saved_at),
                                thumb: None,
                            },
                            version: remote.info.version,
                            payload: None,
                            dismissed: false,
                        };
                        self.conflicts.insert(key.clone(), conflict);
                        self.changed = true;
                    }
                    if !unreadable
                        && self
                            .conflicts
                            .get(&key)
                            .is_some_and(|c| c.payload.is_none())
                        && action.is_none()
                    {
                        action = Some(Action::Get { slot: slot.clone() });
                    }
                }
            }
            if let Some(item) = item {
                self.states
                    .insert((rom.to_string(), item.save.clone()), state);
            }
        }
        self.conflicts
            .retain(|(r, s), _| r != rom || keep_conflicts.contains(&(r.clone(), s.clone())));
        self.changed = true;
        action
    }

    fn write_fetched(
        &mut self,
        rom: &str,
        slot: &str,
        payload: &SavePayload,
        version: u64,
    ) -> bool {
        let Some(store) = self.store.clone() else {
            return false;
        };
        let written = match self.local_save(rom, slot) {
            Some(save) => store.write_payload(rom, &save, payload).map(|()| save),
            None if slot_kind(slot) == Some(SlotKind::Autosave) => store
                .add_autosave(rom, slot, payload)
                .map(LocalSave::Autosave),
            None => return false,
        };
        let save = match written {
            Ok(save) => save,
            Err(err) => {
                log::warn!("could not keep a save from the cloud: {err}");
                return false;
            }
        };
        if save == LocalSave::Battery && self.open.as_ref().is_some_and(|(r, _)| r == rom) {
            self.reload = true;
        }
        let hash = store
            .read_payload(rom, &save)
            .map_or(UNKNOWN, |kept| kept.hash());
        self.set_entry(
            rom,
            slot,
            Some(Entry {
                version,
                hash,
                mark: Mark::Synced,
            }),
        );
        true
    }

    fn keep_both(&mut self, conflict: &Conflict, payload: &SavePayload) -> std::io::Result<()> {
        let Some(store) = self.store.clone() else {
            return Ok(());
        };
        let rom = &conflict.rom;
        let name = copy_name(&conflict.name, &conflict.slot, &conflict.cloud.device);
        match slot_kind(&conflict.slot) {
            Some(SlotKind::Battery | SlotKind::OtherBattery) => {
                let token = store.add_other_battery(rom, payload)?;
                store.set_other_battery_name(rom, &token, &name)
            }
            Some(SlotKind::Snapshot(_)) => {
                let slot = store.next_free_slot(rom);
                store.write_payload(rom, &LocalSave::Snapshot(slot), payload)?;
                store.set_snapshot_name(rom, slot, &name)
            }
            Some(SlotKind::Autosave) | None => Ok(()),
        }
    }
}

impl Online {
    pub fn cloud_attach(&mut self, store: SaveStore) {
        self.cloud.store = Some(store);
    }

    pub fn set_cloud(&mut self, owner: Option<String>, enabled: bool) {
        let enabled = enabled && owner.is_some() && self.cloud.store.is_some();
        if owner == self.cloud.owner && enabled == self.cloud.enabled {
            return;
        }
        self.cloud.save_ledger();
        if owner != self.cloud.owner {
            self.cloud.ledger = None;
            self.cloud.account.reset();
            if let Some(owner) = &owner {
                self.cloud.load(owner);
            }
        }
        self.cloud.owner = owner;
        self.cloud.enabled = enabled;
        self.cloud.disconnect();
        self.cloud.due.clear();
        self.cloud.states.clear();
        if enabled {
            if self.cloud.ledger.as_ref().is_none_or(|l| !l.enabled) {
                self.cloud.ledger().enabled = true;
                self.cloud.mark_all_dirty();
            }
            if let Some((rom, _)) = self.cloud.open.clone() {
                self.cloud.due.insert(rom, Instant::now());
            }
        } else if self.cloud.owner.is_some() && self.cloud.ledger.is_some() {
            self.cloud.ledger().enabled = false;
            self.cloud.ledger_dirty = true;
        }
        self.cloud.save_ledger();
    }

    pub fn cloud_forget_local(&mut self) {
        if let Some(store) = &self.cloud.store {
            store.take_touched();
            for name in store.ledger_names() {
                let Some(mut ledger) = store
                    .read_ledger(&name)
                    .and_then(|bytes| postcard::from_bytes::<Ledger>(&bytes).ok())
                else {
                    continue;
                };
                ledger.games.clear();
                if let Ok(bytes) = postcard::to_allocvec(&ledger)
                    && let Err(err) = store.write_ledger(&name, &bytes)
                {
                    log::warn!("could not keep the cloud save list: {err}");
                }
            }
        }
        if let Some(ledger) = &mut self.cloud.ledger {
            ledger.games.clear();
        }
        self.cloud.ledger_dirty = true;
        self.cloud.disconnect();
        self.cloud.due.clear();
        self.cloud.save_ledger();
    }

    pub fn cloud_enabled(&self) -> bool {
        self.cloud.enabled
    }

    pub fn cloud_game(&mut self, game: Option<(String, String)>) {
        let game = game.filter(|(rom, _)| is_game_key(rom));
        if game == self.cloud.open {
            return;
        }
        if let Some((rom, _)) = self.cloud.open.take() {
            self.cloud.due.insert(rom, Instant::now());
        }
        self.cloud.reload = false;
        if let Some((rom, title)) = &game
            && self.cloud.enabled
        {
            if self.cloud.entry_title(rom) != Some(title.as_str()) {
                self.cloud.game(rom).title = title.clone();
            }
            if let Some(keys) = &self.keys {
                self.cloud
                    .rom_of
                    .insert(keys.game_key(&rom_bytes(rom)), rom.clone());
            }
            self.cloud.due.insert(rom.clone(), Instant::now());
        }
        self.cloud.open = game;
    }

    pub fn cloud_hold(&mut self, rom: &str) {
        self.cloud.holding = Some(rom.to_string());
        self.cloud.held_checked = false;
        self.cloud.due.insert(rom.to_string(), Instant::now());
    }

    pub fn cloud_release(&mut self) {
        if let Some(rom) = self.cloud.holding.take() {
            self.cloud.due.insert(rom, Instant::now());
        }
    }

    pub fn cloud_settled(&self, rom: &str) -> bool {
        if !self.cloud.enabled || !is_game_key(rom) {
            return true;
        }
        match self.phase {
            Phase::Connecting | Phase::Probed | Phase::SigningIn => false,
            Phase::Connected => {
                let battery = (rom.to_string(), battery_slot(None));
                self.cloud.listing.is_some()
                    && self.cloud.held_checked
                    && self.cloud.holding.as_deref() == Some(rom)
                    && !self.cloud.fetching.contains(&battery)
            }
            _ => true,
        }
    }

    pub fn cloud_take_reload(&mut self) -> bool {
        std::mem::take(&mut self.cloud.reload)
    }

    pub fn cloud_take_changed(&mut self) -> bool {
        std::mem::take(&mut self.cloud.changed)
    }

    pub fn cloud_state(&self, rom: &str, save: &LocalSave) -> Option<SyncState> {
        if !self.cloud.enabled || !is_game_key(rom) {
            return None;
        }
        Some(
            self.cloud
                .states
                .get(&(rom.to_string(), save.clone()))
                .copied()
                .unwrap_or(SyncState::Waiting),
        )
    }

    pub fn cloud_conflict(&self, rom: &str, save: &LocalSave) -> Option<&Conflict> {
        self.cloud
            .conflicts
            .values()
            .find(|c| c.rom == rom && c.save.as_ref() == Some(save))
    }

    pub fn cloud_ask(&self) -> Option<&Conflict> {
        let (rom, _) = self.cloud.open.as_ref()?;
        self.cloud
            .conflicts
            .values()
            .find(|c| &c.rom == rom && !c.dismissed)
    }

    pub fn cloud_dismiss(&mut self, rom: &str, slot: &str) {
        if let Some(c) = self
            .cloud
            .conflicts
            .get_mut(&(rom.to_string(), slot.to_string()))
        {
            c.dismissed = true;
            self.cloud.changed = true;
        }
    }

    pub fn cloud_ask_again(&mut self, rom: &str, slot: &str) {
        if let Some(c) = self
            .cloud
            .conflicts
            .get_mut(&(rom.to_string(), slot.to_string()))
        {
            c.dismissed = false;
            self.cloud.changed = true;
        }
    }

    pub fn cloud_resolve(&mut self, rom: &str, slot: &str, choice: Choice) -> std::io::Result<()> {
        let key = (rom.to_string(), slot.to_string());
        let Some(conflict) = self.cloud.conflicts.get(&key).cloned() else {
            return Ok(());
        };
        let pending = Entry {
            version: conflict.version,
            hash: UNKNOWN,
            mark: Mark::Synced,
        };
        match choice {
            Choice::KeepHere => {
                if conflict.here.is_none() {
                    return Ok(());
                }
                self.cloud.set_entry(rom, slot, Some(pending));
            }
            Choice::KeepCloud => {
                let Some(payload) = &conflict.payload else {
                    return Ok(());
                };
                if !self
                    .cloud
                    .write_fetched(rom, slot, payload, conflict.version)
                {
                    return Err(std::io::Error::other("the cloud save could not be kept"));
                }
            }
            Choice::KeepBoth => {
                let Some(payload) = &conflict.payload else {
                    return Ok(());
                };
                if conflict.here.is_none() {
                    return Ok(());
                }
                self.cloud.keep_both(&conflict, payload)?;
                self.cloud.set_entry(rom, slot, Some(pending));
            }
        }
        self.cloud.conflicts.remove(&key);
        self.cloud.due.insert(rom.to_string(), Instant::now());
        self.cloud.changed = true;
        self.cloud.save_ledger();
        Ok(())
    }

    pub fn cloud_has_copy(&self, rom: &str, save: &LocalSave) -> bool {
        if !self.cloud.enabled || !is_game_key(rom) {
            return false;
        }
        let Some(slot) = self.cloud.slot_of(rom, save) else {
            return false;
        };
        match (&self.cloud.listing, &self.keys) {
            (Some(listing), Some(keys)) => {
                listing.contains_key(&(keys.game_key(&rom_bytes(rom)), slot))
            }
            _ => self
                .cloud
                .entry(rom, &slot)
                .is_some_and(|e| e.mark == Mark::Synced),
        }
    }

    pub fn cloud_forget(&mut self, rom: &str, save: &LocalSave, cloud_too: bool) {
        if !self.cloud.enabled || !is_game_key(rom) {
            return;
        }
        let Some(slot) = self.cloud.slot_of(rom, save) else {
            return;
        };
        let version = match (&self.cloud.listing, &self.keys) {
            (Some(listing), Some(keys)) => listing
                .get(&(keys.game_key(&rom_bytes(rom)), slot.clone()))
                .map(|r| r.info.version),
            _ => None,
        }
        .or_else(|| self.cloud.entry(rom, &slot).map(|e| e.version));
        let Some(version) = version else {
            self.cloud.set_entry(rom, &slot, None);
            return;
        };
        let hash = self.cloud.entry(rom, &slot).map_or(UNKNOWN, |e| e.hash);
        self.cloud.set_entry(
            rom,
            &slot,
            Some(Entry {
                version,
                hash,
                mark: Mark::DeletedHere { cloud_too },
            }),
        );
        self.cloud.save_ledger();
    }

    pub fn cloud_games(&self) -> Vec<CloudGame> {
        let mut games: BTreeMap<GameKey, CloudGame> = BTreeMap::new();
        let account = self.keys.as_ref().map(account::account_key);
        for ((game, _), remote) in self.cloud.listing.iter().flatten() {
            if Some(*game) == account {
                continue;
            }
            let entry = games.entry(*game).or_insert_with(|| CloudGame {
                game: *game,
                title: String::new(),
                bytes: 0,
                saves: 0,
            });
            entry.bytes += remote.info.size;
            entry.saves += 1;
            if entry.title.is_empty()
                && let Some(label) = &remote.label
            {
                entry.title = label.game.clone();
            }
        }
        for (game, entry) in games.iter_mut() {
            if let Some(title) = self
                .cloud
                .rom_of
                .get(game)
                .map(|rom| self.cloud.title_of(rom))
                .filter(|title| !title.is_empty())
            {
                entry.title = title.to_string();
            }
        }
        let mut games: Vec<CloudGame> = games.into_values().collect();
        games.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.title.cmp(&b.title)));
        games
    }

    pub fn cloud_name(&mut self, rom: &str, name: &str) {
        if name.is_empty() || self.cloud.names.get(rom).is_some_and(|known| known == name) {
            return;
        }
        self.cloud.names.insert(rom.to_string(), name.to_string());
        if self.cloud.enabled && is_game_key(rom) {
            self.cloud
                .due
                .entry(rom.to_string())
                .or_insert(Instant::now());
        }
        self.cloud.changed = true;
    }

    pub fn account_bytes(&self) -> Option<u64> {
        let account = account::account_key(self.keys.as_ref()?);
        let listing = self.cloud.listing.as_ref()?;
        Some(
            listing
                .iter()
                .filter(|((game, _), _)| *game == account)
                .map(|(_, remote)| remote.info.size)
                .sum(),
        )
    }

    pub fn cloud_listed(&self) -> bool {
        self.cloud.listing.is_some()
    }

    pub fn cloud_delete_game(&mut self, game: GameKey) {
        let Some(listing) = &self.cloud.listing else {
            return;
        };
        let doomed: Vec<(GameKey, String, u64)> = listing
            .iter()
            .filter(|((g, _), _)| *g == game)
            .map(|((g, slot), remote)| (*g, slot.clone(), remote.info.version))
            .filter(|d| !self.cloud.deletes.contains(d))
            .collect();
        self.cloud.deletes.extend(doomed);
    }

    pub fn cloud_flush(&mut self) {
        let now = Instant::now();
        for at in self.cloud.due.values_mut() {
            *at = (*at).min(now);
        }
        self.cloud.account.flush(now);
        self.cloud.save_ledger();
    }

    pub(super) fn cloud_reset(&mut self) {
        self.cloud.save_ledger();
        self.cloud.disconnect();
    }

    pub(super) fn cloud_tick(&mut self, now: Instant) {
        let Some(store) = self.cloud.store.clone() else {
            return;
        };
        let touched = store.take_touched();
        let account = self.cloud.account.is_active() && self.cloud.owner.is_some();
        if !self.cloud.enabled && !account {
            if self.cloud.asked || self.cloud.listing.is_some() {
                self.cloud.disconnect();
            }
            return;
        }
        let touched = if self.cloud.enabled {
            touched
        } else {
            Default::default()
        };
        for rom in touched {
            if self.cloud.entry_title(&rom).is_none() || !self.cloud.is_dirty(&rom) {
                self.cloud.game(&rom).dirty = true;
            }
            let at = if self.cloud.holding.as_deref() == Some(rom.as_str()) {
                now
            } else {
                now + SETTLE
            };
            let due = self.cloud.due.entry(rom).or_insert(at);
            if at < *due {
                *due = at;
            }
        }
        if self.phase != Phase::Connected || self.keys.is_none() {
            if self.cloud.asked || self.cloud.listing.is_some() {
                self.cloud.disconnect();
            }
            self.cloud_offline(now);
        } else {
            if !self.cloud.asked {
                self.cloud.asked = true;
                self.send(ClientMsg::ListSaves);
            }
            self.cloud_storage_changed();
            if self.cloud.listing.is_some()
                && self.cloud.op.is_none()
                && !self.account_pump(now)
                && self.cloud.enabled
            {
                self.cloud_pump(now);
            }
        }
        self.cloud.save_ledger();
    }

    fn cloud_offline(&mut self, now: Instant) {
        let ready: Vec<String> = self
            .cloud
            .due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(rom, _)| rom.clone())
            .collect();
        for rom in ready {
            self.cloud.due.remove(&rom);
            let shown = self.cloud.open.as_ref().is_some_and(|(r, _)| *r == rom)
                || self.cloud.states.keys().any(|(r, _)| *r == rom);
            if shown {
                self.cloud.offline_states(&rom);
            }
        }
    }

    fn cloud_storage_changed(&mut self) {
        let Some(account) = &self.account else {
            return;
        };
        let now = (account.storage_used, account.quota_bytes);
        let before = self.cloud.storage.replace(now);
        if before.is_none_or(|b| b == now) {
            return;
        }
        self.cloud.account.storage_changed();
        let full: Vec<(String, String)> = self
            .cloud
            .refusals
            .iter()
            .filter(|(_, (_, state))| *state == SyncState::Full)
            .map(|(key, _)| key.clone())
            .collect();
        let at = Instant::now();
        for key in full {
            self.cloud.refusals.remove(&key);
            self.cloud.due.insert(key.0, at);
        }
    }

    fn cloud_pump(&mut self, now: Instant) {
        if let Some((game, slot, base)) = self.cloud.deletes.pop() {
            let tag = self.cloud.tag();
            self.send(ClientMsg::DeleteSave {
                tag,
                game,
                slot: slot.clone(),
                base,
            });
            self.cloud.op = Some(Op::Delete { tag, game, slot });
            return;
        }
        let open = self.cloud.open.as_ref().map(|(r, _)| r.clone());
        let next = open
            .filter(|rom| self.cloud.due.get(rom).is_some_and(|at| *at <= now))
            .or_else(|| {
                self.cloud
                    .due
                    .iter()
                    .filter(|(_, at)| **at <= now)
                    .min_by_key(|(_, at)| **at)
                    .map(|(rom, _)| rom.clone())
            });
        let Some(rom) = next else {
            return;
        };
        let Some(keys) = self.keys.as_ref() else {
            return;
        };
        let game = keys.game_key(&rom_bytes(&rom));
        self.cloud.rom_of.insert(game, rom.clone());
        match self.cloud.reconcile(&rom, game) {
            None => {
                self.cloud.due.remove(&rom);
                if self.cloud.is_dirty(&rom) {
                    self.cloud.game(&rom).dirty = false;
                }
            }
            Some(Action::Upload {
                rom,
                slot,
                save,
                base,
                payload,
            }) => {
                let hash = payload.hash();
                let title = self.cloud.title_of(&rom).to_string();
                let saved_at = crate::storage::cloud::saved_at_of(&payload);
                let label = SaveLabel {
                    game: title,
                    name: name_of(&payload),
                    device: device_name(),
                    saved_at,
                    hash,
                };
                let sealed_label = keys.seal_label(&label);
                let data = keys.seal_save(&payload);
                let tag = self.cloud.tag();
                let op = Op::Put {
                    tag,
                    rom: rom.clone(),
                    slot: slot.clone(),
                    hash,
                    label,
                };
                if data.len() + sealed_label.len() > MAX_SAVE_BYTES {
                    self.cloud.op = Some(op);
                    self.cloud_refused(tag, ErrorCode::TooLarge, None);
                    return;
                }
                log::debug!(
                    "cloud upload {save:?} of {rom}: {} bytes, {} sealed",
                    payload.size(),
                    data.len()
                );
                self.send(ClientMsg::PutSave {
                    tag,
                    game,
                    slot,
                    base,
                    label: sealed_label,
                    data,
                });
                self.cloud.op = Some(op);
            }
            Some(Action::Get { slot }) => {
                self.send(ClientMsg::GetSave {
                    game,
                    slot: slot.clone(),
                });
                self.cloud.op = Some(Op::Get { game, slot });
            }
            Some(Action::Delete { slot, base }) => {
                let tag = self.cloud.tag();
                self.send(ClientMsg::DeleteSave {
                    tag,
                    game,
                    slot: slot.clone(),
                    base,
                });
                self.cloud.op = Some(Op::Delete { tag, game, slot });
            }
        }
    }

    fn cloud_remote(&self, info: SaveInfo) -> Remote {
        let label = self
            .keys
            .as_ref()
            .and_then(|keys| keys.open_label(&info.label).ok());
        Remote { info, label }
    }

    fn cloud_list_insert(&mut self, remote: Remote) {
        let key = (remote.info.game, remote.info.slot.clone());
        if let Some(listing) = self.cloud.listing.as_mut() {
            listing.insert(key, remote);
        }
    }

    fn cloud_redo(&mut self, game: GameKey) {
        if let Some(rom) = self.cloud.rom_of.get(&game).cloned() {
            self.cloud.due.insert(rom, Instant::now());
        }
    }

    fn cloud_refused(&mut self, tag: Tag, error: ErrorCode, current: Option<SaveInfo>) {
        let game = match &self.cloud.op {
            Some(Op::Put { tag: t, rom, .. }) if *t == tag => {
                let rom = rom.clone();
                self.keys.as_ref().map(|k| k.game_key(&rom_bytes(&rom)))
            }
            Some(Op::Delete { tag: t, game, .. }) if *t == tag => Some(*game),
            Some(Op::AccountPut { tag: t, .. }) if *t == tag => {
                self.keys.as_ref().map(account::account_key)
            }
            _ => return,
        };
        let op = self.cloud.op.take();
        if error == ErrorCode::SaveChanged {
            if let (Some(game), Some(op)) = (game, &op) {
                let slot = match op {
                    Op::Put { slot, .. }
                    | Op::Delete { slot, .. }
                    | Op::Get { slot, .. }
                    | Op::AccountPut { slot, .. } => slot.clone(),
                };
                match current {
                    Some(info) => {
                        let remote = self.cloud_remote(info);
                        self.cloud_list_insert(remote);
                    }
                    None => {
                        if let Some(listing) = self.cloud.listing.as_mut() {
                            listing.remove(&(game, slot));
                        }
                    }
                }
                self.cloud_redo(game);
            }
            return;
        }
        if let Some(Op::AccountPut { name, hash, .. }) = op {
            self.account_refused(name, hash, error);
            return;
        }
        if let Some(Op::Put {
            rom, slot, hash, ..
        }) = op
        {
            let state = match error {
                ErrorCode::QuotaFull => SyncState::Full,
                ErrorCode::TooLarge => SyncState::TooLarge,
                _ => SyncState::NoStorage,
            };
            self.cloud
                .refusals
                .insert((rom.clone(), slot), (hash, state));
            self.cloud.due.insert(rom, Instant::now());
        }
        self.cloud.changed = true;
    }

    pub(super) fn cloud_receive(&mut self, msg: ServerMsg) -> Option<ServerMsg> {
        match msg {
            ServerMsg::Saves(list) => {
                if !self.cloud.enabled && !self.cloud.account.is_active() {
                    return None;
                }
                let listing: BTreeMap<(GameKey, String), Remote> = list
                    .into_iter()
                    .map(|info| {
                        let remote = self.cloud_remote(info);
                        ((remote.info.game, remote.info.slot.clone()), remote)
                    })
                    .collect();
                self.cloud.listing = Some(listing);
                if self.cloud.enabled {
                    self.cloud_listed_now();
                }
            }
            ServerMsg::SaveStored { tag, info } if matches!(&self.cloud.op, Some(Op::AccountPut { tag: t, .. }) if *t == tag) => {
                if let Some(Op::AccountPut { name, label, .. }) = self.cloud.op.take() {
                    self.account_stored(name, info, label);
                }
            }
            ServerMsg::SaveStored { tag, info } => {
                let Some(Op::Put {
                    tag: t,
                    rom,
                    slot,
                    hash,
                    label,
                    ..
                }) = &self.cloud.op
                else {
                    return None;
                };
                if *t != tag {
                    return None;
                }
                let (rom, slot, hash, label) = (rom.clone(), slot.clone(), *hash, label.clone());
                self.cloud.op = None;
                self.cloud.set_entry(
                    &rom,
                    &slot,
                    Some(Entry {
                        version: info.version,
                        hash,
                        mark: Mark::Synced,
                    }),
                );
                self.cloud.refusals.remove(&(rom.clone(), slot));
                self.cloud_list_insert(Remote {
                    info,
                    label: Some(label),
                });
                self.cloud.due.insert(rom, Instant::now());
                self.cloud.changed = true;
            }
            ServerMsg::SaveRefused {
                tag,
                error,
                current,
            } => self.cloud_refused(tag, error, current),
            ServerMsg::Save { info, data } => {
                let Some(Op::Get { game, slot }) = &self.cloud.op else {
                    return None;
                };
                if *game != info.game || *slot != info.slot {
                    return None;
                }
                self.cloud.op = None;
                if self.keys.as_ref().map(account::account_key) == Some(info.game) {
                    let remote = self.cloud_remote(info);
                    self.account_fetched(remote, &data);
                    return None;
                }
                let game = info.game;
                let slot = info.slot.clone();
                let version = info.version;
                let remote = self.cloud_remote(info);
                self.cloud_list_insert(remote);
                let opened = self.keys.as_ref().map(|k| k.open_save(&data));
                let rom = self.cloud.rom_of.get(&game).cloned()?;
                match opened {
                    Some(Ok(payload)) => {
                        let key = (rom.clone(), slot.clone());
                        match self.cloud.conflicts.get_mut(&key) {
                            Some(conflict) if conflict.version == version => {
                                conflict.cloud.thumb = thumb_of(&payload);
                                conflict.payload = Some(payload);
                            }
                            Some(_) => {}
                            None => {
                                if !self.cloud.write_fetched(&rom, &slot, &payload, version) {
                                    self.cloud.unreadable.insert((game, slot, version));
                                }
                            }
                        }
                    }
                    _ => {
                        log::warn!("a cloud save of {rom} could not be opened");
                        self.cloud.unreadable.insert((game, slot, version));
                    }
                }
                self.cloud.due.insert(rom, Instant::now());
                self.cloud.changed = true;
            }
            ServerMsg::SaveMissing { game, slot } => {
                if !matches!(&self.cloud.op, Some(Op::Get { game: g, slot: s }) if *g == game && *s == slot)
                {
                    return None;
                }
                self.cloud.op = None;
                if let Some(listing) = self.cloud.listing.as_mut() {
                    listing.remove(&(game, slot));
                }
                self.cloud_redo(game);
            }
            ServerMsg::SaveDeleted { tag } => {
                let Some(Op::Delete { tag: t, game, slot }) = &self.cloud.op else {
                    return None;
                };
                if *t != tag {
                    return None;
                }
                let (game, slot) = (*game, slot.clone());
                self.cloud.op = None;
                if let Some(listing) = self.cloud.listing.as_mut() {
                    listing.remove(&(game, slot));
                }
                self.cloud_redo(game);
                self.cloud.changed = true;
            }
            other => return Some(other),
        }
        None
    }

    fn cloud_listed_now(&mut self) {
        let Some(keys) = self.keys.as_ref() else {
            return;
        };
        let mut roms: BTreeSet<String> = self
            .cloud
            .store
            .as_ref()
            .map(SaveStore::games)
            .unwrap_or_default()
            .into_iter()
            .collect();
        if let Some(ledger) = &self.cloud.ledger {
            roms.extend(ledger.games.keys().cloned());
        }
        if let Some((rom, _)) = &self.cloud.open {
            roms.insert(rom.clone());
        }
        let now = Instant::now();
        for rom in roms {
            let game = keys.game_key(&rom_bytes(&rom));
            self.cloud.rom_of.insert(game, rom.clone());
            self.cloud.due.entry(rom).or_insert(now);
        }
        self.cloud.changed = true;
    }
}

impl CloudState {
    fn is_dirty(&self, rom: &str) -> bool {
        self.ledger
            .as_ref()
            .and_then(|l| l.games.get(rom))
            .is_some_and(|g| g.dirty)
    }

    fn entry_title(&self, rom: &str) -> Option<&str> {
        self.ledger
            .as_ref()?
            .games
            .get(rom)
            .map(|g| g.title.as_str())
    }

    fn title_of(&self, rom: &str) -> &str {
        self.names
            .get(rom)
            .map(String::as_str)
            .or_else(|| self.entry_title(rom))
            .unwrap_or_default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn device_name() -> String {
    match std::env::consts::OS {
        "macos" => "Mac",
        "windows" => "Windows PC",
        "linux" => "Linux PC",
        "android" => "Android phone",
        "ios" => "iPhone",
        _ => "Computer",
    }
    .to_string()
}

#[cfg(target_arch = "wasm32")]
pub fn device_name() -> String {
    let agent = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default();
    browser_on(&agent)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn browser_on(agent: &str) -> String {
    let browser = if agent.contains("Firefox/") {
        "Firefox"
    } else if agent.contains("Edg/") {
        "Edge"
    } else if agent.contains("OPR/") {
        "Opera"
    } else if agent.contains("Chrome/") {
        "Chrome"
    } else if agent.contains("Safari/") {
        "Safari"
    } else {
        "Browser"
    };
    let system = if agent.contains("Android") {
        "Android"
    } else if agent.contains("iPhone") {
        "iPhone"
    } else if agent.contains("iPad") {
        "iPad"
    } else if agent.contains("Mac OS X") {
        "Mac"
    } else if agent.contains("Windows") {
        "Windows"
    } else if agent.contains("Linux") || agent.contains("CrOS") {
        "Linux"
    } else {
        return browser.to_string();
    };
    format!("{browser} on {system}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(version: u64, hash: u8, mark: Mark) -> Entry {
        Entry {
            version,
            hash: [hash; 32],
            mark,
        }
    }

    #[test]
    fn equal_copies_are_adopted() {
        assert_eq!(
            decide(Some([1; 32]), Some((3, [1; 32])), None, false, false),
            Step::Adopt
        );
    }

    #[test]
    fn first_meeting_of_two_different_copies_is_a_conflict() {
        assert_eq!(
            decide(Some([1; 32]), Some((3, [2; 32])), None, false, false),
            Step::Conflict
        );
        assert_eq!(
            decide(Some([1; 32]), Some((3, [2; 32])), None, true, false),
            Step::Upload(3)
        );
    }

    #[test]
    fn one_side_changed_moves_that_way() {
        let synced = entry(3, 1, Mark::Synced);
        assert_eq!(
            decide(
                Some([2; 32]),
                Some((3, [1; 32])),
                Some(&synced),
                false,
                false
            ),
            Step::Upload(3)
        );
        assert_eq!(
            decide(
                Some([1; 32]),
                Some((4, [5; 32])),
                Some(&synced),
                false,
                false
            ),
            Step::Fetch
        );
        assert_eq!(
            decide(
                Some([1; 32]),
                Some((4, [5; 32])),
                Some(&synced),
                false,
                true
            ),
            Step::Conflict
        );
        assert_eq!(
            decide(
                Some([2; 32]),
                Some((4, [5; 32])),
                Some(&synced),
                false,
                false
            ),
            Step::Conflict
        );
    }

    #[test]
    fn new_saves_upload_and_missing_ones_come_down() {
        assert_eq!(
            decide(Some([1; 32]), None, None, false, false),
            Step::Upload(0)
        );
        assert_eq!(
            decide(None, Some((2, [1; 32])), None, false, false),
            Step::Fetch
        );
        assert_eq!(
            decide(None, Some((2, [1; 32])), None, false, true),
            Step::Conflict
        );
    }

    #[test]
    fn deletes_follow_the_ledger() {
        let synced = entry(3, 1, Mark::Synced);
        assert_eq!(
            decide(Some([1; 32]), None, Some(&synced), false, false),
            Step::CloudGone
        );
        let gone = entry(3, 1, Mark::CloudGone);
        assert_eq!(
            decide(Some([1; 32]), None, Some(&gone), false, false),
            Step::Keep
        );
        assert_eq!(
            decide(Some([2; 32]), None, Some(&gone), false, false),
            Step::Upload(0)
        );
        let both = entry(3, 1, Mark::DeletedHere { cloud_too: true });
        assert_eq!(
            decide(None, Some((3, [1; 32])), Some(&both), false, false),
            Step::DeleteCloud(3)
        );
        assert_eq!(
            decide(None, Some((4, [1; 32])), Some(&both), false, false),
            Step::Fetch
        );
        let here = entry(3, 1, Mark::DeletedHere { cloud_too: false });
        assert_eq!(
            decide(None, Some((3, [1; 32])), Some(&here), false, false),
            Step::Keep
        );
        assert_eq!(
            decide(None, Some((3, [1; 32])), Some(&synced), true, false),
            Step::DeleteCloud(3)
        );
        assert_eq!(
            decide(None, Some((3, [1; 32])), Some(&synced), false, false),
            Step::Fetch
        );
        assert_eq!(
            decide(None, None, Some(&synced), false, false),
            Step::Forget
        );
    }

    #[test]
    fn devices_read_as_browser_and_system() {
        let firefox = "Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0";
        assert_eq!(browser_on(firefox), "Firefox on Android");
        let chrome = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";
        assert_eq!(browser_on(chrome), "Chrome on Mac");
    }

    #[test]
    fn copies_are_named_after_their_device() {
        assert_eq!(copy_name("", "slot-2", "Mac"), "Slot 3 from Mac");
        assert_eq!(copy_name("Boss", "battery", "Mac"), "Boss from Mac");
    }
}
