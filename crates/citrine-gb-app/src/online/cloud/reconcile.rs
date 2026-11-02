use super::device::device_name;
use super::{
    Action, CloudState, Conflict, ConflictSide, Entry, Mark, Remote, Step, SyncState, UNKNOWN,
    copy_name, decide, default_name, name_of, slot_order,
};
use crate::storage::cloud::{LocalSave, thumb_of};
use crate::storage::cloud::{SlotKind, battery_slot, slot_kind};
use citrine_gb_net::{GameKey, SavePayload};
use std::collections::{BTreeMap, BTreeSet};

impl CloudState {
    pub(super) fn reconcile(&mut self, rom: &str, game: GameKey) -> Option<Action> {
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

    pub(super) fn write_fetched(
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

    pub(super) fn keep_both(
        &mut self,
        conflict: &Conflict,
        payload: &SavePayload,
    ) -> std::io::Result<()> {
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
