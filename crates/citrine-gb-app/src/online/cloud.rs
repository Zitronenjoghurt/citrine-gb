mod account;
mod device;
mod ledger;
mod reconcile;
mod server;

use super::{Online, Phase};
pub use crate::storage::SaveStore;
pub use crate::storage::cloud::{LocalSave, OtherBattery, thumb_of};
use crate::storage::cloud::{SlotKind, battery_slot, is_game_key, random_token, slot_kind};
use crate::storage::{MAX_SAVE_NAME, default_slot_name};
pub use account::AccountCopy;
use citrine_gb_net::{ErrorCode, GameKey, SaveInfo, SaveLabel, SavePayload, Tag, storage_cost};
pub(crate) use device::device_name;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use web_time::Instant;

const SETTLE: Duration = Duration::from_secs(3);
const OP_TIMEOUT: Duration = Duration::from_secs(30);
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
    NoAccountStorage,
    Unreadable,
}

impl SyncState {
    fn waits_for_storage(self) -> bool {
        matches!(self, Self::Full | Self::NoStorage | Self::NoAccountStorage)
    }

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
            Self::NoAccountStorage => "Not uploaded, your account on this relay has no storage",
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
    op_at: Option<Instant>,
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
    fn start(&mut self, op: Op) {
        self.op = Some(op);
        self.op_at = Some(Instant::now());
    }

    fn doomed(&self, data: usize, label: usize, replacing: Option<&Remote>) -> Option<ErrorCode> {
        let (used, quota) = self.storage?;
        if quota == 0 {
            return Some(ErrorCode::SharingOff);
        }
        let freed = replacing.map_or(0, |r| storage_cost(r.info.size) + r.info.label.len() as u64);
        let cost = storage_cost(data as u64) + label as u64;
        (used.saturating_sub(freed).saturating_add(cost) > quota).then_some(ErrorCode::QuotaFull)
    }

    fn tag(&mut self) -> Tag {
        self.next_tag = self.next_tag.wrapping_add(1);
        self.next_tag
    }

    fn disconnect(&mut self) {
        self.listing = None;
        self.asked = false;
        self.op = None;
        self.op_at = None;
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
}

#[cfg(test)]
mod tests;
