use super::{CloudState, account, rom_bytes, walker};
use crate::online::{KeepSaves, Online};
use crate::storage::SaveStore;
use crate::storage::cloud::is_game_key;
use citrine_gb_net::GameKey;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use web_time::Instant;

const GAMES: &str = "games";

#[derive(Serialize, Deserialize)]
struct GamesIndex {
    roms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elsewhere {
    pub game: GameKey,
    pub rom: Option<String>,
    pub title: String,
    pub devices: Vec<String>,
    pub saves: usize,
    pub bytes: u64,
    pub fetched: Option<(u64, u64)>,
}

impl CloudState {
    fn saves_here(&self) -> BTreeSet<String> {
        let mut roms: BTreeSet<String> = self
            .store
            .as_ref()
            .map(SaveStore::games)
            .unwrap_or_default()
            .into_iter()
            .collect();
        if let Some(ledger) = &self.ledger {
            roms.extend(ledger.games.keys().cloned());
        }
        roms
    }

    pub(super) fn forget_keep(&mut self) {
        self.wanted.clear();
        self.index.clear();
        self.here.clear();
    }

    fn fetched(&self, game: GameKey, rom: &str) -> Option<(u64, u64)> {
        if !self.wanted.contains(rom) {
            return None;
        }
        let (mut done, mut total) = (0, 0);
        for ((_, slot), remote) in self
            .listing
            .iter()
            .flatten()
            .filter(|((g, _), _)| *g == game)
        {
            let version = remote.info.version;
            total += remote.info.size;
            if self.entry(rom, slot).is_some_and(|e| e.version >= version)
                || self.unreadable.contains(&(game, slot.clone(), version))
            {
                done += remote.info.size;
            } else if matches!(&self.op, Some(super::Op::Get { game: g, slot: s }) if *g == game && s == slot)
            {
                done += (self.parts.len() as u64).min(remote.info.size);
            }
        }
        (done < total).then_some((done, total))
    }
}

impl Online {
    pub fn cloud_keep(&mut self, keep: KeepSaves, library: impl IntoIterator<Item = String>) {
        let library: BTreeSet<String> =
            library.into_iter().filter(|rom| is_game_key(rom)).collect();
        if keep == self.cloud.keep && library == self.cloud.library {
            return;
        }
        self.cloud.keep = keep;
        self.cloud.library = library;
        if self.cloud.enabled && self.cloud.listing.is_some() {
            self.cloud_listed_now();
        }
    }

    pub fn cloud_fetch_game(&mut self, game: GameKey) -> bool {
        let Some(rom) = self
            .cloud
            .index
            .get(&game)
            .or_else(|| self.cloud.rom_of.get(&game))
            .cloned()
        else {
            return false;
        };
        self.cloud.wanted.insert(rom.clone());
        self.cloud.here.insert(game);
        self.cloud.rom_of.insert(game, rom.clone());
        if self.cloud.enabled {
            self.cloud.due.insert(rom, Instant::now());
        }
        self.cloud.changed = true;
        true
    }

    pub fn cloud_elsewhere(&self) -> Vec<Elsewhere> {
        let Some(keys) = self.keys.as_ref().filter(|_| self.cloud.enabled) else {
            return Vec::new();
        };
        let skip = [account::account_key(keys), walker::walker_key(keys)];
        let mut games: BTreeMap<GameKey, Elsewhere> = BTreeMap::new();
        for ((game, _), remote) in self.cloud.listing.iter().flatten() {
            if skip.contains(game) {
                continue;
            }
            let rom = self
                .cloud
                .index
                .get(game)
                .or_else(|| self.cloud.rom_of.get(game));
            let fetched = rom.and_then(|rom| self.cloud.fetched(*game, rom));
            if self.cloud.here.contains(game) && fetched.is_none() {
                continue;
            }
            let entry = games.entry(*game).or_insert_with(|| Elsewhere {
                game: *game,
                rom: rom.cloned(),
                title: String::new(),
                devices: Vec::new(),
                saves: 0,
                bytes: 0,
                fetched,
            });
            entry.saves += 1;
            entry.bytes += remote.info.size;
            if let Some(label) = &remote.label {
                if entry.title.is_empty() {
                    entry.title = label.game.clone();
                }
                if !label.device.is_empty() && !entry.devices.contains(&label.device) {
                    entry.devices.push(label.device.clone());
                }
            }
        }
        let mut games: Vec<Elsewhere> = games.into_values().collect();
        games.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| b.bytes.cmp(&a.bytes)));
        games
    }

    pub(super) fn cloud_keep_listed(&mut self) {
        let Some(keys) = self.keys.as_ref() else {
            return;
        };
        let cloud = &mut self.cloud;
        let mut roms = cloud.saves_here();
        if let Some((rom, _)) = &cloud.open {
            roms.insert(rom.clone());
        }
        roms.extend(cloud.wanted.iter().cloned());
        let listed: BTreeSet<GameKey> = cloud
            .listing
            .iter()
            .flatten()
            .map(|((game, _), _)| *game)
            .collect();
        let now = Instant::now();
        cloud.here.clear();
        for rom in roms {
            let game = keys.game_key(&rom_bytes(&rom));
            cloud.here.insert(game);
            cloud.rom_of.insert(game, rom.clone());
            cloud.due.entry(rom).or_insert(now);
        }
        for rom in &cloud.library {
            let game = keys.game_key(&rom_bytes(rom));
            if cloud.here.insert(game) && listed.contains(&game) {
                cloud.rom_of.insert(game, rom.clone());
                cloud.due.entry(rom.clone()).or_insert(now);
            }
        }
        if cloud.keep == KeepSaves::All {
            for (game, rom) in &cloud.index {
                if listed.contains(game) && cloud.here.insert(*game) {
                    cloud.rom_of.insert(*game, rom.clone());
                    cloud.due.entry(rom.clone()).or_insert(now);
                }
            }
        }
        cloud.changed = true;
        self.cloud_index_put();
    }

    pub(super) fn cloud_index_put(&mut self) {
        if !self.cloud.enabled {
            return;
        }
        let roms: Vec<String> = self
            .cloud
            .saves_here()
            .into_iter()
            .filter(|rom| is_game_key(rom))
            .collect();
        if roms.is_empty() {
            self.account_follow(GAMES);
            return;
        }
        match postcard::to_allocvec(&GamesIndex { roms }) {
            Ok(bytes) => self.account_put(GAMES, bytes),
            Err(err) => log::warn!("could not encode the games list: {err}"),
        }
    }

    pub(super) fn cloud_index_stop(&mut self) {
        self.account_stop(GAMES);
    }

    pub(super) fn cloud_index_follow(&mut self) {
        let copies = self.account_take(GAMES);
        let Some(keys) = self.keys.as_ref().filter(|_| !copies.is_empty()) else {
            return;
        };
        let mut grew = false;
        for copy in copies {
            let Ok(index) = postcard::from_bytes::<GamesIndex>(&copy.data) else {
                log::warn!("the games list from {} could not be read", copy.device);
                continue;
            };
            for rom in index.roms.into_iter().filter(|rom| is_game_key(rom)) {
                let game = keys.game_key(&rom_bytes(&rom));
                grew |= self.cloud.index.insert(game, rom).is_none();
            }
        }
        if grew && self.cloud.enabled && self.cloud.listing.is_some() {
            self.cloud_listed_now();
        }
    }
}
