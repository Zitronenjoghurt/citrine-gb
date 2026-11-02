use super::device::device_name;
use super::{Op, Remote, SETTLE, SyncState};
use crate::online::Online;
use citrine_gb_net::{
    ClientMsg, ErrorCode, GameKey, Keys, MAX_SAVE_BYTES, SaveInfo, SaveLabel, SavePayload,
};
use std::collections::{BTreeMap, BTreeSet};
use web_time::Instant;

const ACCOUNT: &[u8] = b"account";
const DATA: &str = "data";

pub fn account_key(keys: &Keys) -> GameKey {
    keys.game_key(ACCOUNT)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountCopy {
    pub device: String,
    pub saved_at: u64,
    pub data: Vec<u8>,
}

struct Wanted {
    payload: SavePayload,
    hash: [u8; 32],
    due: Instant,
}

#[derive(Default)]
pub(super) struct AccountSlots {
    wanted: BTreeMap<String, Wanted>,
    active: BTreeSet<String>,
    fetched: BTreeMap<String, u64>,
    unreadable: BTreeSet<(String, u64)>,
    refused: BTreeMap<String, ([u8; 32], SyncState)>,
    arrived: BTreeMap<String, Vec<AccountCopy>>,
}

enum Next {
    Put {
        name: String,
        slot: String,
        base: u64,
    },
    Get {
        slot: String,
    },
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 16 && name.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn name_of(slot: &str) -> Option<&str> {
    slot.split_once('-').map(|(name, _)| name)
}

impl AccountSlots {
    pub(super) fn is_active(&self) -> bool {
        !self.active.is_empty()
    }

    pub(super) fn reset(&mut self) {
        self.fetched.clear();
        self.unreadable.clear();
        self.refused.clear();
        self.arrived.clear();
    }

    pub(super) fn flush(&mut self, now: Instant) {
        for wanted in self.wanted.values_mut() {
            wanted.due = wanted.due.min(now);
        }
    }

    pub(super) fn storage_changed(&mut self) {
        self.refused
            .retain(|_, (_, state)| !state.waits_for_storage());
    }

    fn own_slot(name: &str, device: &str) -> String {
        format!("{name}-{device}")
    }

    fn next(
        &self,
        listing: &BTreeMap<(GameKey, String), Remote>,
        game: GameKey,
        device: &str,
        now: Instant,
    ) -> Option<Next> {
        for (name, wanted) in &self.wanted {
            if !self.active.contains(name) || wanted.due > now {
                continue;
            }
            if self
                .refused
                .get(name)
                .is_some_and(|(hash, _)| *hash == wanted.hash)
            {
                continue;
            }
            let slot = Self::own_slot(name, device);
            let remote = listing.get(&(game, slot.clone()));
            if remote.is_some_and(|r| r.hash() == wanted.hash) {
                continue;
            }
            let base = remote.map_or(0, |r| r.info.version);
            return Some(Next::Put {
                name: name.clone(),
                slot,
                base,
            });
        }
        for ((g, slot), remote) in listing.range((game, String::new())..) {
            if *g != game {
                break;
            }
            let Some(name) = name_of(slot) else {
                continue;
            };
            if !self.active.contains(name) || *slot == Self::own_slot(name, device) {
                continue;
            }
            let version = remote.info.version;
            if self.fetched.get(slot).is_some_and(|v| *v >= version)
                || self.unreadable.contains(&(slot.clone(), version))
            {
                continue;
            }
            return Some(Next::Get { slot: slot.clone() });
        }
        None
    }
}

impl Online {
    pub fn account_put(&mut self, name: &str, data: Vec<u8>) {
        if !valid_name(name) {
            log::warn!("account slot name {name:?} is not allowed");
            return;
        }
        let payload = SavePayload {
            files: vec![(DATA.to_string(), data)],
        };
        let hash = payload.hash();
        let slots = &mut self.cloud.account;
        slots.active.insert(name.to_string());
        if slots.wanted.get(name).is_some_and(|w| w.hash == hash) {
            return;
        }
        slots.wanted.insert(
            name.to_string(),
            Wanted {
                payload,
                hash,
                due: Instant::now() + SETTLE,
            },
        );
    }

    pub fn account_stop(&mut self, name: &str) {
        let slots = &mut self.cloud.account;
        slots.active.remove(name);
        slots.wanted.remove(name);
        slots.arrived.remove(name);
        slots.refused.remove(name);
    }

    pub fn account_take(&mut self, name: &str) -> Vec<AccountCopy> {
        self.cloud.account.arrived.remove(name).unwrap_or_default()
    }

    pub fn account_problem(&self, name: &str) -> Option<SyncState> {
        self.account_state(name).filter(|state| {
            matches!(
                state,
                SyncState::Full
                    | SyncState::TooLarge
                    | SyncState::NoStorage
                    | SyncState::NoAccountStorage
            )
        })
    }

    pub fn account_state(&self, name: &str) -> Option<SyncState> {
        let slots = &self.cloud.account;
        let wanted = slots.wanted.get(name)?;
        if let Some((hash, state)) = slots.refused.get(name)
            && *hash == wanted.hash
        {
            return Some(*state);
        }
        let keys = self.keys.as_ref()?;
        let device = self.cloud.ledger.as_ref()?.device.clone();
        let slot = AccountSlots::own_slot(name, &device);
        let synced = self
            .cloud
            .listing
            .as_ref()?
            .get(&(account_key(keys), slot))
            .is_some_and(|r| r.hash() == wanted.hash);
        Some(if synced {
            SyncState::Synced
        } else {
            SyncState::Waiting
        })
    }

    pub(super) fn account_pump(&mut self, now: Instant) -> bool {
        if !self.cloud.account.is_active() {
            return false;
        }
        if self.cloud.ledger.is_none() {
            self.cloud.ledger_dirty = true;
        }
        let device = self.cloud.device();
        let (Some(keys), Some(listing)) = (self.keys.as_ref(), self.cloud.listing.as_ref()) else {
            return false;
        };
        let game = account_key(keys);
        let Some(next) = self.cloud.account.next(listing, game, &device, now) else {
            return false;
        };
        match next {
            Next::Put { name, slot, base } => {
                let wanted = &self.cloud.account.wanted[&name];
                let label = SaveLabel {
                    game: String::new(),
                    name: name.clone(),
                    device: device_name(),
                    saved_at: crate::storage::now_unix(),
                    hash: wanted.hash,
                };
                let sealed_label = keys.seal_label(&label);
                let data = keys.seal_save(&wanted.payload);
                let hash = wanted.hash;
                if data.len() + sealed_label.len() > MAX_SAVE_BYTES {
                    self.cloud
                        .account
                        .refused
                        .insert(name, (hash, SyncState::TooLarge));
                    self.cloud.changed = true;
                    return true;
                }
                let remote = listing.get(&(game, slot.clone()));
                if let Some(error) = self.cloud.doomed(data.len(), sealed_label.len(), remote) {
                    self.account_refused(name, hash, error);
                    return true;
                }
                let tag = self.cloud.tag();
                self.send(ClientMsg::PutSave {
                    tag,
                    game,
                    slot: slot.clone(),
                    base,
                    label: sealed_label,
                    data,
                });
                self.cloud.start(Op::AccountPut {
                    tag,
                    name,
                    slot,
                    hash,
                    label,
                });
            }
            Next::Get { slot } => {
                self.send(ClientMsg::GetSave {
                    game,
                    slot: slot.clone(),
                });
                self.cloud.start(Op::Get { game, slot });
            }
        }
        true
    }

    pub(super) fn account_stored(&mut self, name: String, info: SaveInfo, label: SaveLabel) {
        self.cloud.account.refused.remove(&name);
        self.cloud_list_insert(Remote {
            info,
            label: Some(label),
        });
        self.cloud.changed = true;
    }

    pub(super) fn account_refused(&mut self, name: String, hash: [u8; 32], error: ErrorCode) {
        let state = self.refusal_state(error);
        self.cloud.account.refused.insert(name, (hash, state));
        self.cloud.changed = true;
    }

    pub(super) fn account_fetched(&mut self, remote: Remote, data: &[u8]) {
        let slot = remote.info.slot.clone();
        let version = remote.info.version;
        let label = remote.label.clone();
        self.cloud_list_insert(remote);
        let Some(name) = name_of(&slot).map(str::to_string) else {
            return;
        };
        let payload = self.keys.as_ref().map(|k| k.open_save(data));
        let data = match payload {
            Some(Ok(payload)) => payload.file(DATA).map(<[u8]>::to_vec),
            _ => None,
        };
        let slots = &mut self.cloud.account;
        let Some(data) = data else {
            log::warn!("an account copy of {name} could not be opened");
            slots.unreadable.insert((slot, version));
            return;
        };
        slots.fetched.insert(slot, version);
        if !slots.active.contains(&name) {
            return;
        }
        let copy = AccountCopy {
            device: label.as_ref().map(|l| l.device.clone()).unwrap_or_default(),
            saved_at: label.as_ref().map_or(0, |l| l.saved_at),
            data,
        };
        slots.arrived.entry(name).or_default().push(copy);
        self.cloud.changed = true;
    }
}
