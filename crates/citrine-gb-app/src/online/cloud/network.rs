use super::reconcile::keep_fetched;
use super::work::{Ready, ready, run};
use super::{
    Choice, CloudState, Conflict, ConflictSide, Elsewhere, Entry, Mark, Step, UNKNOWN, decide,
    default_name, name_of, slot_order,
};
use crate::online::{KeepSaves, Online};
use crate::storage::SaveStore;
use crate::storage::cloud::{LocalSave, SlotKind, battery_slot, is_game_key, slot_kind, thumb_of};
use citrine_gb_net::{PeerKey, SavePayload};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use web_time::Instant;

const PART: usize = 512 * 1024;
pub(super) const LEDGER: &str = "lan-peers";
const LEDGER_FORM: u8 = 1;
const GET_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub key: PeerKey,
    pub name: String,
    pub account: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerSync {
    Syncing,
    TurnOnThere,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkDevice {
    pub key: PeerKey,
    pub name: String,
    pub account: bool,
    pub allowed: bool,
    pub sync: PeerSync,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Listed {
    rom: String,
    title: String,
    slot: String,
    name: String,
    hash: [u8; 32],
    size: u64,
    saved_at: u64,
    over: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum Wire {
    Hello {
        syncs: bool,
        settings: bool,
    },
    List,
    Listing(Vec<Listed>),
    Changed,
    Get {
        rom: String,
        slot: String,
    },
    Part {
        rom: String,
        slot: String,
        hash: [u8; 32],
        total: u64,
        bytes: Vec<u8>,
    },
    Missing {
        rom: String,
        slot: String,
    },
    Settings(Vec<u8>),
}

#[derive(Default, Serialize, Deserialize)]
struct PeerLedger {
    peers: BTreeMap<[u8; 32], BTreeMap<String, BTreeMap<String, Entry>>>,
}

#[derive(Clone)]
struct Mine {
    save: LocalSave,
    listed: Listed,
    thumb: Option<crate::media::Frame>,
}

struct PeerState {
    peer: Peer,
    syncs: Option<bool>,
    settings: bool,
    settings_at: u64,
    told: Option<(bool, bool)>,
    asked: bool,
    owed_list: bool,
    listing: Option<Vec<Listed>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Want {
    peer: [u8; 32],
    rom: String,
    slot: String,
    hash: [u8; 32],
    conflict: bool,
}

struct Getting {
    want: Want,
    bytes: Vec<u8>,
    total: u64,
    since: Instant,
}

#[derive(Default)]
pub(super) struct Network {
    on: bool,
    allowed: BTreeSet<[u8; 32]>,
    peers: BTreeMap<[u8; 32], PeerState>,
    out: Vec<(PeerKey, Vec<u8>)>,
    ledger: Option<PeerLedger>,
    ledger_dirty: bool,
    mine: BTreeMap<String, Vec<Mine>>,
    stale: BTreeSet<String>,
    listed_all: bool,
    scan: Option<(String, Receiver<Vec<Mine>>)>,
    roms: BTreeSet<String>,
    gets: VecDeque<Want>,
    getting: Option<Getting>,
    conflicts: BTreeMap<(String, String), ([u8; 32], Conflict)>,
    settings: Option<(u64, Vec<u8>)>,
    settings_at: u64,
    settings_in: Vec<(String, Vec<u8>)>,
}

fn version_of(hash: &[u8; 32]) -> u64 {
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&hash[..8]);
    u64::from_le_bytes(bytes) | 1
}

fn encode(wire: &Wire) -> Option<Vec<u8>> {
    postcard::to_allocvec(wire).ok()
}

fn scan(store: &SaveStore, rom: &str, device: &str, title: &str) -> Vec<Mine> {
    store
        .local_items(rom, device)
        .into_iter()
        .map(|item| Mine {
            thumb: thumb_of(&item.payload),
            listed: Listed {
                rom: rom.to_string(),
                title: title.to_string(),
                slot: item.slot.clone(),
                name: name_of(&item.payload),
                hash: item.payload.hash(),
                size: item.payload.size() as u64,
                saved_at: item.saved_at,
                over: None,
            },
            save: item.save,
        })
        .collect()
}

impl PeerState {
    fn allowed(&self, on: bool, allowed: &BTreeSet<[u8; 32]>) -> bool {
        on && (self.peer.account || allowed.contains(&self.peer.key.0))
    }
}

impl CloudState {
    fn network_allows(&self, peer: &PeerState) -> bool {
        peer.allowed(self.network.on, &self.network.allowed)
    }

    fn network_syncs(&self, peer: &PeerState) -> bool {
        self.network_allows(peer) && peer.syncs == Some(true)
    }

    fn network_send(&mut self, to: PeerKey, wire: &Wire) {
        if let Some(body) = encode(wire) {
            self.network.out.push((to, body));
        }
    }

    fn network_ledger(&mut self) -> &mut PeerLedger {
        if self.network.ledger.is_none() {
            let read = self
                .store
                .as_ref()
                .and_then(|store| store.read_ledger(LEDGER))
                .filter(|bytes| bytes.first() == Some(&LEDGER_FORM))
                .and_then(|bytes| postcard::from_bytes(&bytes[1..]).ok());
            self.network.ledger = Some(read.unwrap_or_default());
        }
        self.network.ledger.get_or_insert_with(PeerLedger::default)
    }

    fn network_entries(&mut self, peer: [u8; 32], rom: &str) -> BTreeMap<String, Entry> {
        self.network_ledger()
            .peers
            .get(&peer)
            .and_then(|games| games.get(rom))
            .cloned()
            .unwrap_or_default()
    }

    fn network_set_entry(&mut self, peer: [u8; 32], rom: &str, slot: &str, entry: Option<Entry>) {
        let games = self.network_ledger().peers.entry(peer).or_default();
        let slots = games.entry(rom.to_string()).or_default();
        let changed = match entry {
            Some(entry) => slots.insert(slot.to_string(), entry.clone()) != Some(entry),
            None => slots.remove(slot).is_some(),
        };
        if slots.is_empty() {
            games.remove(rom);
        }
        self.network.ledger_dirty |= changed;
    }

    fn network_save_ledger(&mut self) {
        if !std::mem::take(&mut self.network.ledger_dirty) {
            return;
        }
        let (Some(store), Some(ledger)) = (&self.store, &self.network.ledger) else {
            return;
        };
        let Ok(body) = postcard::to_allocvec(ledger) else {
            return;
        };
        if let Err(err) = store.write_ledger(LEDGER, &[&[LEDGER_FORM][..], &body].concat()) {
            log::warn!("could not keep the list of saves on this network: {err}");
        }
    }

    fn network_kept(&self, rom: &str) -> bool {
        self.keep == KeepSaves::All
            || self.library.contains(rom)
            || self.wanted.contains(rom)
            || self.network.mine.contains_key(rom)
            || self.open.as_ref().is_some_and(|(open, _)| open == rom)
    }

    fn network_title(&self, rom: &str) -> String {
        self.title_of(rom).to_string()
    }

    fn network_listing_for(&mut self, peer: [u8; 32]) -> Vec<Listed> {
        let mine: Vec<Listed> = self
            .network
            .mine
            .values()
            .flatten()
            .map(|mine| mine.listed.clone())
            .collect();
        let entries = self
            .network_ledger()
            .peers
            .get(&peer)
            .cloned()
            .unwrap_or_default();
        mine.into_iter()
            .map(|mut listed| {
                listed.over = entries
                    .get(&listed.rom)
                    .and_then(|slots| slots.get(&listed.slot))
                    .filter(|entry| entry.hash == UNKNOWN && entry.mark == Mark::Synced)
                    .map(|entry| entry.version);
                listed
            })
            .collect()
    }

    fn network_scan(&mut self) {
        if let Some((rom, done)) = self.network.scan.take() {
            match ready(&done) {
                Ready::Waiting => {
                    self.network.scan = Some((rom, done));
                    return;
                }
                Ready::Done(found) => self.network_scanned(rom, found),
                Ready::Lost => {}
            }
        }
        let Some(store) = self.store.clone() else {
            return;
        };
        if !self.network.listed_all {
            self.network.listed_all = true;
            self.network.stale.extend(store.games());
        }
        let Some(rom) = self.network.stale.pop_first() else {
            return;
        };
        let device = self.device_name();
        let title = self.network_title(&rom);
        let key = rom.clone();
        let done = run(self.spawner.as_ref(), "network scan", move || {
            scan(&store, &key, &device, &title)
        });
        self.network.scan = Some((rom, done));
        self.network_scan();
    }

    fn network_scanned(&mut self, rom: String, found: Vec<Mine>) {
        let before: Vec<&Listed> = self
            .network
            .mine
            .get(&rom)
            .map(|mine| mine.iter().map(|m| &m.listed).collect())
            .unwrap_or_default();
        let after: Vec<&Listed> = found.iter().map(|m| &m.listed).collect();
        let changed = before != after;
        if found.is_empty() {
            self.network.mine.remove(&rom);
        } else {
            self.network.mine.insert(rom.clone(), found);
        }
        self.network.roms.insert(rom);
        if changed {
            let syncing: Vec<PeerKey> = self
                .network
                .peers
                .values()
                .filter(|peer| self.network_allows(peer))
                .map(|peer| peer.peer.key)
                .collect();
            for key in syncing {
                self.network_send(key, &Wire::Changed);
            }
        }
    }

    fn network_scanning(&self) -> bool {
        !self.network.listed_all || self.network.scan.is_some() || !self.network.stale.is_empty()
    }

    fn network_reconcile(&mut self, key: [u8; 32], rom: &str) {
        let Some(peer) = self.network.peers.get(&key) else {
            return;
        };
        if !self.network_syncs(peer) || !self.network_kept(rom) {
            return;
        }
        let Some(listing) = &peer.listing else {
            return;
        };
        let device = peer.peer.name.clone();
        let there: BTreeMap<String, Listed> = listing
            .iter()
            .filter(|listed| listed.rom == rom)
            .map(|listed| (listed.slot.clone(), listed.clone()))
            .collect();
        let here: BTreeMap<String, Mine> = self
            .network
            .mine
            .get(rom)
            .into_iter()
            .flatten()
            .map(|mine| (mine.listed.slot.clone(), mine.clone()))
            .collect();
        let entries = self.network_entries(key, rom);
        let mut slots: Vec<(u8, String)> = there
            .keys()
            .chain(here.keys())
            .chain(entries.keys())
            .map(|slot| (slot_order(slot), slot.clone()))
            .collect();
        slots.sort();
        slots.dedup();
        let locked_rom = self.open.as_ref().is_some_and(|(open, _)| open == rom)
            && self.holding.as_deref() != Some(rom);
        let mut keep_conflicts = BTreeSet::new();
        for (_, slot) in slots {
            let mine = here.get(&slot);
            let theirs = there.get(&slot);
            let local = mine.map(|m| m.listed.hash);
            let entry = entries.get(&slot);
            let autosave = slot_kind(&slot) == Some(SlotKind::Autosave);
            let locked = locked_rom && slot == battery_slot(None);
            let mut step = decide(
                local,
                theirs.map(|t| (version_of(&t.hash), t.hash)),
                entry,
                autosave,
                locked,
            );
            if step == Step::Conflict
                && !locked
                && theirs.and_then(|t| t.over).is_some()
                && theirs.and_then(|t| t.over) == local.as_ref().map(version_of)
            {
                step = Step::Fetch;
            }
            match step {
                Step::Adopt => {
                    let (Some(theirs), Some(local)) = (theirs, local) else {
                        continue;
                    };
                    self.network_set_entry(
                        key,
                        rom,
                        &slot,
                        Some(Entry {
                            version: version_of(&theirs.hash),
                            hash: local,
                            mark: Mark::Synced,
                        }),
                    );
                }
                Step::Fetch => {
                    let Some(theirs) = theirs else {
                        continue;
                    };
                    self.network_want(Want {
                        peer: key,
                        rom: rom.to_string(),
                        slot: slot.clone(),
                        hash: theirs.hash,
                        conflict: false,
                    });
                }
                Step::Conflict => {
                    let Some(theirs) = theirs else {
                        continue;
                    };
                    let at = (rom.to_string(), slot.clone());
                    keep_conflicts.insert(at.clone());
                    let version = version_of(&theirs.hash);
                    let fresh = self.network.conflicts.get(&at).is_none_or(|(from, c)| {
                        *from != key || c.version != version || c.here.is_some() != mine.is_some()
                    });
                    if fresh {
                        let name = Some(theirs.name.clone())
                            .filter(|n| !n.is_empty())
                            .or_else(|| mine.map(|m| m.listed.name.clone()))
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| default_name(&slot));
                        let conflict = Conflict {
                            rom: rom.to_string(),
                            slot: slot.clone(),
                            save: mine.map(|m| m.save.clone()),
                            name,
                            here: mine.map(|m| ConflictSide {
                                device: self.device_name(),
                                saved_at: m.listed.saved_at,
                                thumb: m.thumb.clone(),
                            }),
                            cloud: ConflictSide {
                                device: device.clone(),
                                saved_at: theirs.saved_at,
                                thumb: None,
                            },
                            version,
                            payload: None,
                            dismissed: false,
                        };
                        self.network.conflicts.insert(at, (key, conflict));
                        self.changed = true;
                        self.network_want(Want {
                            peer: key,
                            rom: rom.to_string(),
                            slot: slot.clone(),
                            hash: theirs.hash,
                            conflict: true,
                        });
                    }
                }
                Step::CloudGone => {
                    if let Some(mut entry) = entry.cloned() {
                        entry.mark = Mark::CloudGone;
                        self.network_set_entry(key, rom, &slot, Some(entry));
                    }
                }
                Step::Forget => self.network_set_entry(key, rom, &slot, None),
                Step::Keep | Step::Upload(_) | Step::DeleteCloud(_) => {}
            }
        }
        let before = self.network.conflicts.len();
        self.network.conflicts.retain(|(r, s), (from, _)| {
            *from != key || r != rom || keep_conflicts.contains(&(r.clone(), s.clone()))
        });
        self.changed |= before != self.network.conflicts.len();
    }

    fn network_want(&mut self, want: Want) {
        let same = |other: &Want| {
            other.peer == want.peer
                && other.rom == want.rom
                && other.slot == want.slot
                && other.hash == want.hash
        };
        if self.network.gets.iter().any(same)
            || self.network.getting.as_ref().is_some_and(|g| same(&g.want))
        {
            return;
        }
        self.network.gets.retain(|other| {
            other.peer != want.peer || other.rom != want.rom || other.slot != want.slot
        });
        self.network.gets.push_back(want);
    }

    fn network_get_next(&mut self, now: Instant) {
        if self
            .network
            .getting
            .as_ref()
            .is_some_and(|g| now.duration_since(g.since) > GET_TIMEOUT)
        {
            self.network.getting = None;
        }
        if self.network.getting.is_some() {
            return;
        }
        while let Some(want) = self.network.gets.pop_front() {
            let Some(peer) = self.network.peers.get(&want.peer) else {
                continue;
            };
            if !self.network_syncs(peer) {
                continue;
            }
            let to = peer.peer.key;
            self.network_send(
                to,
                &Wire::Get {
                    rom: want.rom.clone(),
                    slot: want.slot.clone(),
                },
            );
            self.network.getting = Some(Getting {
                want,
                bytes: Vec::new(),
                total: 0,
                since: now,
            });
            return;
        }
    }

    fn network_serve(&mut self, to: PeerKey, rom: String, slot: String) {
        let found = self
            .network
            .mine
            .get(&rom)
            .and_then(|mine| mine.iter().find(|m| m.listed.slot == slot))
            .map(|m| m.save.clone());
        let payload = found.and_then(|save| self.store.as_ref()?.read_payload(&rom, &save));
        let Some(body) = payload.and_then(|payload| {
            let hash = payload.hash();
            postcard::to_allocvec(&payload)
                .ok()
                .map(|body| (hash, body))
        }) else {
            self.network_send(to, &Wire::Missing { rom, slot });
            return;
        };
        let (hash, body) = body;
        let total = body.len() as u64;
        for bytes in body.chunks(PART) {
            self.network_send(
                to,
                &Wire::Part {
                    rom: rom.clone(),
                    slot: slot.clone(),
                    hash,
                    total,
                    bytes: bytes.to_vec(),
                },
            );
        }
    }

    fn network_part(&mut self, from: [u8; 32], rom: &str, slot: &str, total: u64, bytes: &[u8]) {
        let Some(getting) = self.network.getting.as_mut() else {
            return;
        };
        if getting.want.peer != from || getting.want.rom != rom || getting.want.slot != slot {
            return;
        }
        getting.total = total;
        getting.bytes.extend_from_slice(bytes);
        getting.since = Instant::now();
        if (getting.bytes.len() as u64) < total {
            return;
        }
        let Some(getting) = self.network.getting.take() else {
            return;
        };
        let Ok(payload) = postcard::from_bytes::<SavePayload>(&getting.bytes) else {
            return;
        };
        let hash = payload.hash();
        let want = getting.want;
        if want.conflict {
            if let Some((peer, conflict)) = self
                .network
                .conflicts
                .get_mut(&(want.rom.clone(), want.slot.clone()))
                && *peer == want.peer
                && conflict.version == version_of(&want.hash)
            {
                conflict.cloud.thumb = thumb_of(&payload);
                conflict.payload = Some(payload);
                self.changed = true;
            }
            return;
        }
        let kept = self
            .fetched_store(&want.rom, &payload)
            .and_then(|store| keep_fetched(&store, &want.rom, &want.slot, &payload, hash));
        let Some((save, kept)) = kept else {
            return;
        };
        if save == LocalSave::Battery && self.open.as_ref().is_some_and(|(r, _)| *r == want.rom) {
            self.reload = true;
        }
        self.network_set_entry(
            want.peer,
            &want.rom,
            &want.slot,
            Some(Entry {
                version: version_of(&want.hash),
                hash: kept,
                mark: Mark::Synced,
            }),
        );
        self.network.stale.insert(want.rom);
        self.changed = true;
    }

    fn network_fetching(&self, rom: &str) -> bool {
        let battery = battery_slot(None);
        let wanted = |want: &Want| want.rom == rom && want.slot == battery;
        self.network.gets.iter().any(wanted)
            || self
                .network
                .getting
                .as_ref()
                .is_some_and(|g| wanted(&g.want))
    }

    fn network_peer_settled(&self, peer: &PeerState) -> bool {
        !self.network_allows(peer) || peer.syncs == Some(false) || peer.listing.is_some()
    }

    fn network_fetched(&self, rom: &str, from: &[[u8; 32]]) -> Option<(u64, u64)> {
        if !self.wanted.contains(rom) {
            return None;
        }
        let (mut done, mut total) = (0, 0);
        for key in from {
            let Some(listing) = self.network.peers.get(key).and_then(|p| p.listing.as_ref()) else {
                continue;
            };
            for listed in listing.iter().filter(|l| l.rom == rom) {
                total += listed.size;
                let here = self
                    .network
                    .mine
                    .get(rom)
                    .is_some_and(|mine| mine.iter().any(|m| m.listed.hash == listed.hash));
                if here {
                    done += listed.size;
                } else if let Some(getting) = &self.network.getting
                    && getting.want.rom == rom
                    && getting.want.slot == listed.slot
                {
                    done += (getting.bytes.len() as u64).min(listed.size);
                }
            }
        }
        (done < total).then_some((done, total))
    }
}

impl Online {
    pub fn network_settings(&mut self, on: bool, allowed: impl IntoIterator<Item = [u8; 32]>) {
        let allowed: BTreeSet<[u8; 32]> = allowed.into_iter().collect();
        let network = &mut self.cloud.network;
        if network.on == on && network.allowed == allowed {
            return;
        }
        network.on = on;
        network.allowed = allowed;
        self.cloud.changed = true;
    }

    pub fn network_peers(&mut self, peers: Vec<Peer>) {
        let network = &mut self.cloud.network;
        let keys: BTreeSet<[u8; 32]> = peers.iter().map(|p| p.key.0).collect();
        let before = network.peers.len();
        network.peers.retain(|key, _| keys.contains(key));
        let mut changed = before != network.peers.len();
        for peer in peers {
            match network.peers.get_mut(&peer.key.0) {
                Some(state) if state.peer == peer => {}
                Some(state) => {
                    state.peer = peer;
                    changed = true;
                }
                None => {
                    network.peers.insert(
                        peer.key.0,
                        PeerState {
                            peer,
                            syncs: None,
                            settings: false,
                            settings_at: 0,
                            told: None,
                            asked: false,
                            owed_list: false,
                            listing: None,
                        },
                    );
                    changed = true;
                }
            }
        }
        network
            .gets
            .retain(|want| network.peers.contains_key(&want.peer));
        if network
            .getting
            .as_ref()
            .is_some_and(|g| !network.peers.contains_key(&g.want.peer))
        {
            network.getting = None;
        }
        self.cloud.changed |= changed;
    }

    pub fn network_receive(&mut self, from: PeerKey, body: &[u8]) {
        let Ok(wire) = postcard::from_bytes::<Wire>(body) else {
            return;
        };
        let cloud = &mut self.cloud;
        let Some(peer) = cloud.network.peers.get(&from.0) else {
            return;
        };
        let allows = cloud.network_allows(peer);
        match wire {
            Wire::Hello { syncs, settings } => {
                let Some(peer) = cloud.network.peers.get_mut(&from.0) else {
                    return;
                };
                peer.syncs = Some(syncs);
                peer.settings = settings;
                if !settings {
                    peer.settings_at = 0;
                }
                if !syncs {
                    peer.asked = false;
                    peer.listing = None;
                }
                cloud.changed = true;
            }
            Wire::List if allows => {
                if let Some(peer) = cloud.network.peers.get_mut(&from.0) {
                    peer.owed_list = true;
                }
            }
            Wire::Listing(listing) => {
                if !cloud.network_syncs(peer) {
                    return;
                }
                let mut roms: BTreeSet<String> =
                    listing.iter().map(|listed| listed.rom.clone()).collect();
                roms.extend(
                    cloud
                        .network_ledger()
                        .peers
                        .get(&from.0)
                        .into_iter()
                        .flat_map(|games| games.keys().cloned()),
                );
                if let Some(peer) = cloud.network.peers.get_mut(&from.0) {
                    peer.listing = Some(listing);
                }
                cloud.network.roms.extend(roms);
                cloud.changed = true;
            }
            Wire::Changed => {
                if cloud.network_syncs(peer) {
                    cloud.network_send(from, &Wire::List);
                }
            }
            Wire::Get { rom, slot } if allows => cloud.network_serve(from, rom, slot),
            Wire::Part {
                rom,
                slot,
                total,
                bytes,
                ..
            } => cloud.network_part(from.0, &rom, &slot, total, &bytes),
            Wire::Missing { rom, slot } => {
                if cloud.network.getting.as_ref().is_some_and(|g| {
                    g.want.peer == from.0 && g.want.rom == rom && g.want.slot == slot
                }) {
                    cloud.network.getting = None;
                }
            }
            Wire::Settings(bytes) => {
                if cloud.network_syncs(peer) && cloud.network.settings.is_some() {
                    let name = peer.peer.name.clone();
                    cloud.network.settings_in.push((name, bytes));
                }
            }
            Wire::List | Wire::Get { .. } => {}
        }
    }

    pub fn network_take_out(&mut self) -> Vec<(PeerKey, Vec<u8>)> {
        std::mem::take(&mut self.cloud.network.out)
    }

    pub(crate) fn network_tick(&mut self, now: Instant) {
        let cloud = &mut self.cloud;
        if cloud.store.is_none() {
            return;
        }
        let keys: Vec<[u8; 32]> = cloud.network.peers.keys().copied().collect();
        for key in &keys {
            let Some(peer) = cloud.network.peers.get(key) else {
                continue;
            };
            let allows = cloud.network_allows(peer);
            let syncs = cloud.network_syncs(peer);
            let to = peer.peer.key;
            let told = peer.told;
            let asked = peer.asked;
            let wants = peer.settings && peer.settings_at != cloud.network.settings_at;
            let settings = cloud.network.settings.is_some();
            if told != Some((allows, settings)) {
                cloud.network_send(
                    to,
                    &Wire::Hello {
                        syncs: allows,
                        settings,
                    },
                );
            }
            if syncs && !asked {
                cloud.network_send(to, &Wire::List);
            }
            let wire = cloud
                .network
                .settings
                .as_ref()
                .filter(|_| syncs && wants)
                .map(|(_, bytes)| Wire::Settings(bytes.clone()));
            let sent = wire.is_some();
            if let Some(wire) = wire {
                cloud.network_send(to, &wire);
            }
            if let Some(peer) = cloud.network.peers.get_mut(key) {
                peer.told = Some((allows, settings));
                peer.asked = syncs;
                if sent {
                    peer.settings_at = cloud.network.settings_at;
                } else if !syncs {
                    peer.settings_at = 0;
                }
                if !syncs {
                    peer.listing = None;
                }
            }
        }
        if !cloud.network.on {
            cloud.network.gets.clear();
            cloud.network.getting = None;
            cloud.network.conflicts.clear();
            return;
        }
        cloud.network_scan();
        if !cloud.network_scanning() {
            for key in &keys {
                let owed = cloud
                    .network
                    .peers
                    .get_mut(key)
                    .is_some_and(|peer| std::mem::take(&mut peer.owed_list));
                if owed && let Some(to) = cloud.network.peers.get(key).map(|p| p.peer.key) {
                    let listing = cloud.network_listing_for(*key);
                    cloud.network_send(to, &Wire::Listing(listing));
                }
            }
            let roms = std::mem::take(&mut cloud.network.roms);
            for rom in roms {
                for key in &keys {
                    cloud.network_reconcile(*key, &rom);
                }
            }
        }
        cloud.network_get_next(now);
        cloud.network_save_ledger();
    }

    pub(crate) fn network_settings_put(&mut self, doc: Option<(u64, Vec<u8>)>) {
        let network = &mut self.cloud.network;
        match doc {
            Some(doc) => {
                network.settings_at += 1;
                network.settings = Some(doc);
            }
            None => {
                network.settings = None;
                network.settings_in.clear();
            }
        }
    }

    pub(crate) fn network_settings_edits(&self) -> Option<u64> {
        self.cloud
            .network
            .settings
            .as_ref()
            .map(|(edits, _)| *edits)
    }

    pub(crate) fn network_settings_take(&mut self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.cloud.network.settings_in)
    }

    pub(crate) fn network_touched(&mut self, roms: &BTreeSet<String>) {
        self.cloud.network.stale.extend(roms.iter().cloned());
    }

    pub(crate) fn network_settled(&self, rom: &str) -> bool {
        let cloud = &self.cloud;
        if !cloud.network.on || !is_game_key(rom) {
            return true;
        }
        let peers_listed = cloud
            .network
            .peers
            .values()
            .all(|peer| cloud.network_peer_settled(peer));
        peers_listed
            && !cloud.network.stale.contains(rom)
            && cloud.network.scan.as_ref().is_none_or(|(r, _)| r != rom)
            && !cloud.network.roms.contains(rom)
            && !cloud.network_fetching(rom)
    }

    pub fn network_getting_from(&self, rom: &str) -> Option<String> {
        let battery = battery_slot(None);
        let want = self
            .cloud
            .network
            .getting
            .as_ref()
            .map(|g| &g.want)
            .filter(|w| w.rom == rom && w.slot == battery)
            .or_else(|| {
                self.cloud
                    .network
                    .gets
                    .iter()
                    .find(|w| w.rom == rom && w.slot == battery)
            })?;
        Some(self.cloud.network.peers.get(&want.peer)?.peer.name.clone())
    }

    pub fn network_devices(&self) -> Vec<NetworkDevice> {
        let cloud = &self.cloud;
        cloud
            .network
            .peers
            .values()
            .map(|state| NetworkDevice {
                key: state.peer.key,
                name: state.peer.name.clone(),
                account: state.peer.account,
                allowed: state.peer.account || cloud.network.allowed.contains(&state.peer.key.0),
                sync: if cloud.network_syncs(state) {
                    PeerSync::Syncing
                } else if cloud.network_allows(state) && state.syncs == Some(false) {
                    PeerSync::TurnOnThere
                } else {
                    PeerSync::Off
                },
            })
            .collect()
    }

    pub fn network_elsewhere(&self) -> Vec<Elsewhere> {
        let cloud = &self.cloud;
        let mut games: BTreeMap<String, Elsewhere> = BTreeMap::new();
        for state in cloud.network.peers.values() {
            if !cloud.network_syncs(state) {
                continue;
            }
            for listed in state.listing.iter().flatten() {
                let wanted = cloud.wanted.contains(&listed.rom);
                if cloud.network_kept(&listed.rom) && !wanted {
                    continue;
                }
                let entry = games
                    .entry(listed.rom.clone())
                    .or_insert_with(|| Elsewhere {
                        game: citrine_gb_net::GameKey([0; 32]),
                        rom: Some(listed.rom.clone()),
                        title: listed.title.clone(),
                        devices: Vec::new(),
                        saves: 0,
                        bytes: 0,
                        fetched: None,
                    });
                if entry.title.is_empty() {
                    entry.title = listed.title.clone();
                }
                if !entry.devices.contains(&state.peer.name) {
                    entry.devices.push(state.peer.name.clone());
                }
                entry.saves += 1;
                entry.bytes += listed.size;
            }
        }
        let peers: Vec<[u8; 32]> = cloud.network.peers.keys().copied().collect();
        let mut games: Vec<Elsewhere> = games
            .into_values()
            .map(|mut game| {
                if let Some(rom) = &game.rom {
                    game.fetched = cloud.network_fetched(rom, &peers);
                }
                game
            })
            .filter(|game| {
                !cloud
                    .wanted
                    .contains(game.rom.as_deref().unwrap_or_default())
                    || game.fetched.is_some()
            })
            .collect();
        games.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| b.bytes.cmp(&a.bytes)));
        games
    }

    pub(crate) fn network_syncs_any(&self) -> bool {
        let cloud = &self.cloud;
        cloud
            .network
            .peers
            .values()
            .any(|peer| cloud.network_allows(peer))
    }

    pub fn saves_elsewhere(&self) -> Vec<Elsewhere> {
        let mut games = self.cloud_elsewhere();
        for game in self.network_elsewhere() {
            match games
                .iter_mut()
                .find(|other| other.rom.is_some() && other.rom == game.rom)
            {
                Some(other) => {
                    for device in game.devices {
                        if !other.devices.contains(&device) {
                            other.devices.push(device);
                        }
                    }
                    if other.fetched.is_none() {
                        other.fetched = game.fetched;
                    }
                }
                None => games.push(game),
            }
        }
        games.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| b.bytes.cmp(&a.bytes)));
        games
    }

    pub fn fetch_elsewhere(&mut self, game: &Elsewhere) -> bool {
        let mut fetched = false;
        if let Some(rom) = &game.rom {
            fetched = self.network_fetch_game(rom);
        }
        if game.game.0 != [0; 32] {
            fetched |= self.cloud_fetch_game(game.game);
        }
        fetched
    }

    pub fn network_fetch_game(&mut self, rom: &str) -> bool {
        if !is_game_key(rom) {
            return false;
        }
        self.cloud.wanted.insert(rom.to_string());
        self.cloud.network.roms.insert(rom.to_string());
        self.cloud.changed = true;
        true
    }

    pub(crate) fn network_conflict(&self, rom: &str, save: &LocalSave) -> Option<&Conflict> {
        self.cloud
            .network
            .conflicts
            .values()
            .map(|(_, c)| c)
            .find(|c| c.rom == rom && c.save.as_ref() == Some(save))
    }

    pub(crate) fn network_ask(&self) -> Option<&Conflict> {
        let (rom, _) = self.cloud.open.as_ref()?;
        self.cloud
            .network
            .conflicts
            .values()
            .map(|(_, c)| c)
            .find(|c| &c.rom == rom && !c.dismissed)
    }

    pub(crate) fn network_has_conflict(&self, rom: &str, slot: &str) -> bool {
        self.cloud
            .network
            .conflicts
            .contains_key(&(rom.to_string(), slot.to_string()))
    }

    pub(crate) fn network_dismiss(&mut self, rom: &str, slot: &str, dismissed: bool) {
        if let Some((_, c)) = self
            .cloud
            .network
            .conflicts
            .get_mut(&(rom.to_string(), slot.to_string()))
        {
            c.dismissed = dismissed;
            self.cloud.changed = true;
        }
    }

    pub(crate) fn network_resolve(
        &mut self,
        rom: &str,
        slot: &str,
        choice: Choice,
    ) -> std::io::Result<()> {
        let at = (rom.to_string(), slot.to_string());
        let Some((peer, conflict)) = self.cloud.network.conflicts.get(&at).cloned() else {
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
                self.cloud.network_set_entry(peer, rom, slot, Some(pending));
            }
            Choice::KeepCloud => {
                let Some(payload) = &conflict.payload else {
                    return Ok(());
                };
                let kept = self
                    .cloud
                    .fetched_store(rom, payload)
                    .and_then(|store| keep_fetched(&store, rom, slot, payload, payload.hash()));
                let Some((save, hash)) = kept else {
                    return Err(std::io::Error::other(
                        "the other device's save could not be kept",
                    ));
                };
                if save == LocalSave::Battery
                    && self.cloud.open.as_ref().is_some_and(|(r, _)| r == rom)
                {
                    self.cloud.reload = true;
                }
                self.cloud.network_set_entry(
                    peer,
                    rom,
                    slot,
                    Some(Entry {
                        version: conflict.version,
                        hash,
                        mark: Mark::Synced,
                    }),
                );
            }
            Choice::KeepBoth => {
                let Some(payload) = &conflict.payload else {
                    return Ok(());
                };
                if conflict.here.is_none() {
                    return Ok(());
                }
                self.cloud.keep_both(&conflict, payload)?;
                self.cloud.network_set_entry(peer, rom, slot, Some(pending));
            }
        }
        self.cloud.network.conflicts.remove(&at);
        self.cloud.network.stale.insert(rom.to_string());
        self.cloud.network.roms.insert(rom.to_string());
        let to: Vec<PeerKey> = self
            .cloud
            .network
            .peers
            .values()
            .filter(|p| p.peer.key.0 == peer)
            .map(|p| p.peer.key)
            .collect();
        for key in to {
            self.cloud.network_send(key, &Wire::Changed);
        }
        self.cloud.changed = true;
        self.cloud.network_save_ledger();
        Ok(())
    }

    pub(crate) fn network_forget(&mut self, rom: &str, slot: &str) {
        let peers: Vec<[u8; 32]> = self
            .cloud
            .network_ledger()
            .peers
            .iter()
            .filter(|(_, games)| games.get(rom).is_some_and(|slots| slots.contains_key(slot)))
            .map(|(peer, _)| *peer)
            .collect();
        for peer in peers {
            let Some(mut entry) = self.cloud.network_entries(peer, rom).get(slot).cloned() else {
                continue;
            };
            entry.mark = Mark::DeletedHere { cloud_too: false };
            self.cloud.network_set_entry(peer, rom, slot, Some(entry));
        }
        self.cloud.network_save_ledger();
    }

    pub(crate) fn network_forget_local(&mut self) {
        self.cloud.network.ledger = Some(PeerLedger::default());
        self.cloud.network.ledger_dirty = true;
        self.cloud.network.mine.clear();
        self.cloud.network.listed_all = false;
        self.cloud.network.conflicts.clear();
        self.cloud.network_save_ledger();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::synced::{SyncDoc, Values};
    use citrine_gb::console::ConsoleKind;

    const ROM: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    static FRAME: [u8; 160 * 144 * 4] = [0; 160 * 144 * 4];

    struct Device {
        online: Online,
        store: SaveStore,
        key: PeerKey,
        name: &'static str,
        _dir: tempfile::TempDir,
    }

    fn device(byte: u8, name: &'static str, keep: KeepSaves) -> Device {
        let dir = tempfile::tempdir().unwrap();
        let store = SaveStore::at(dir.path().to_path_buf());
        let mut online = Online::default();
        online.cloud_attach(store.clone());
        online.choose_device_name(name);
        online.network_settings(true, []);
        online.cloud_keep(keep, Vec::<String>::new());
        Device {
            online,
            store,
            key: citrine_gb_net::RelayKey([byte; 32]),
            name,
            _dir: dir,
        }
    }

    fn meet(a: &mut Device, b: &mut Device, account: bool) {
        a.online.network_peers(vec![Peer {
            key: b.key,
            name: b.name.to_owned(),
            account,
        }]);
        b.online.network_peers(vec![Peer {
            key: a.key,
            name: a.name.to_owned(),
            account,
        }]);
    }

    fn run(a: &mut Device, b: &mut Device) {
        let now = Instant::now();
        for _ in 0..40 {
            a.online.update(now);
            b.online.update(now);
            for (to, body) in a.online.network_take_out() {
                assert_eq!(to, b.key);
                b.online.network_receive(a.key, &body);
            }
            for (to, body) in b.online.network_take_out() {
                assert_eq!(to, a.key);
                a.online.network_receive(b.key, &body);
            }
        }
    }

    #[test]
    fn a_battery_a_state_slot_and_an_autosave_copy_to_the_other_device() {
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::All);
        deck.store
            .store_battery(ROM, b"battery from the deck")
            .unwrap();
        deck.store
            .store_snapshot(ROM, 1, b"state one", &FRAME, ConsoleKind::Gbc)
            .unwrap();
        deck.store
            .store_autosave(ROM, b"autosave", &FRAME, ConsoleKind::Gbc, 3)
            .unwrap();
        meet(&mut deck, &mut phone, true);
        run(&mut deck, &mut phone);
        assert_eq!(
            phone.store.load_battery(ROM).as_deref(),
            Some(&b"battery from the deck"[..])
        );
        assert_eq!(
            phone.store.load_snapshot(ROM, 1).as_deref(),
            Some(&b"state one"[..])
        );
        assert!(phone.store.newest_autosave_info(ROM).is_some());
        assert!(phone.online.network_settled(ROM));
    }

    #[test]
    fn a_change_on_one_device_follows_to_the_other() {
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::All);
        deck.store.store_battery(ROM, b"first").unwrap();
        meet(&mut deck, &mut phone, true);
        run(&mut deck, &mut phone);
        phone.store.store_battery(ROM, b"second").unwrap();
        run(&mut deck, &mut phone);
        assert_eq!(
            deck.store.load_battery(ROM).as_deref(),
            Some(&b"second"[..])
        );
        assert!(deck.online.network_ask().is_none());
    }

    #[test]
    fn two_different_saves_ask_and_name_the_other_device() {
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::All);
        deck.store.store_battery(ROM, b"deck").unwrap();
        phone.store.store_battery(ROM, b"phone").unwrap();
        phone
            .online
            .cloud_game(Some((ROM.to_owned(), "Tetris".to_owned())));
        meet(&mut deck, &mut phone, true);
        run(&mut deck, &mut phone);
        let conflict = phone.online.cloud_ask().expect("a conflict");
        assert_eq!(conflict.cloud.device, "Steam Deck");
        assert!(conflict.ready());
        assert_eq!(
            phone.store.load_battery(ROM).as_deref(),
            Some(&b"phone"[..])
        );
        phone
            .online
            .cloud_resolve(ROM, &battery_slot(None), Choice::KeepCloud)
            .unwrap();
        assert_eq!(phone.store.load_battery(ROM).as_deref(), Some(&b"deck"[..]));
        run(&mut deck, &mut phone);
        assert!(phone.online.cloud_ask().is_none());
        assert_eq!(deck.store.load_battery(ROM).as_deref(), Some(&b"deck"[..]));
    }

    #[test]
    fn keeping_only_this_devices_games_waits_until_a_game_is_asked_for() {
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::Library);
        deck.store.store_battery(ROM, b"deck").unwrap();
        meet(&mut deck, &mut phone, true);
        run(&mut deck, &mut phone);
        assert_eq!(phone.store.load_battery(ROM), None);
        let games = phone.online.saves_elsewhere();
        let [game] = &games[..] else {
            panic!("{games:?}")
        };
        assert_eq!(game.devices, ["Steam Deck"]);
        assert_eq!(game.rom.as_deref(), Some(ROM));
        assert!(phone.online.fetch_elsewhere(game));
        run(&mut deck, &mut phone);
        assert_eq!(phone.store.load_battery(ROM).as_deref(), Some(&b"deck"[..]));
        assert!(phone.online.saves_elsewhere().is_empty());
    }

    #[test]
    fn a_paired_device_syncs_only_once_both_sides_turn_it_on() {
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::All);
        deck.store.store_battery(ROM, b"deck").unwrap();
        meet(&mut deck, &mut phone, false);
        phone.online.network_settings(true, [deck.key.0]);
        run(&mut deck, &mut phone);
        assert_eq!(phone.store.load_battery(ROM), None);
        let devices = phone.online.network_devices();
        let [there] = &devices[..] else {
            panic!("{devices:?}")
        };
        assert_eq!(there.sync, PeerSync::TurnOnThere);
        assert!(there.allowed);
        deck.online.network_settings(true, [phone.key.0]);
        run(&mut deck, &mut phone);
        assert_eq!(phone.store.load_battery(ROM).as_deref(), Some(&b"deck"[..]));
        assert_eq!(phone.online.network_devices()[0].sync, PeerSync::Syncing);
    }

    #[test]
    fn saves_on_this_network_off_copies_nothing() {
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::All);
        deck.store.store_battery(ROM, b"deck").unwrap();
        phone.online.network_settings(false, []);
        meet(&mut deck, &mut phone, true);
        run(&mut deck, &mut phone);
        assert_eq!(phone.store.load_battery(ROM), None);
        assert_eq!(deck.online.network_devices()[0].sync, PeerSync::TurnOnThere);
    }

    fn doc(values: &[(&str, serde_json::Value)], now: u64) -> SyncDoc {
        let mut doc = SyncDoc::default();
        let values: Values = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect();
        doc.note(&values, &Values::new(), |_| true, now);
        doc
    }

    fn run_settings(a: &mut Device, b: &mut Device, docs: [&mut SyncDoc; 2], on: [bool; 2]) {
        let now = Instant::now();
        let [a_doc, b_doc] = docs;
        for _ in 0..10 {
            a.online.sync_settings(a_doc, false, on[0]);
            b.online.sync_settings(b_doc, false, on[1]);
            a.online.update(now);
            b.online.update(now);
            for (_, body) in a.online.network_take_out() {
                b.online.network_receive(a.key, &body);
            }
            for (_, body) in b.online.network_take_out() {
                a.online.network_receive(b.key, &body);
            }
        }
    }

    #[test]
    fn settings_cross_once_both_devices_sync_them_and_changes_follow() {
        let theme = |doc: &SyncDoc| doc.values().get("ui_theme").cloned();
        let mut deck = device(1, "Steam Deck", KeepSaves::All);
        let mut phone = device(2, "Pixel", KeepSaves::All);
        meet(&mut deck, &mut phone, true);
        let mut deck_doc = doc(&[("ui_theme", "dark".into())], 5);
        let mut phone_doc = doc(&[], 5);
        run_settings(
            &mut deck,
            &mut phone,
            [&mut deck_doc, &mut phone_doc],
            [true, false],
        );
        assert_eq!(theme(&phone_doc), None);
        run_settings(
            &mut deck,
            &mut phone,
            [&mut deck_doc, &mut phone_doc],
            [true, true],
        );
        assert_eq!(theme(&phone_doc), Some("dark".into()));
        let light: Values = [("ui_theme".to_owned(), "light".into())].into();
        phone_doc.note(&light, &Values::new(), |_| true, 9);
        run_settings(
            &mut deck,
            &mut phone,
            [&mut deck_doc, &mut phone_doc],
            [true, true],
        );
        assert_eq!(theme(&deck_doc), Some("light".into()));
    }

    #[test]
    fn the_wire_reads_back_what_it_wrote() {
        let wire = Wire::Part {
            rom: ROM.to_owned(),
            slot: battery_slot(None),
            hash: [7; 32],
            total: 3,
            bytes: vec![1, 2, 3],
        };
        let body = encode(&wire).unwrap();
        assert_eq!(postcard::from_bytes::<Wire>(&body).unwrap(), wire);
    }
}
