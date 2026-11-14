use super::device::device_name;
use super::work::Upload;
use super::{Action, Entry, Mark, OP_TIMEOUT, Op, Remote, SETTLE, SyncState, name_of, rom_bytes};
use crate::online::{Online, Phase};
use crate::storage::SaveStore;
use crate::storage::cloud::thumb_of;
use citrine_gb_net::{
    ClientMsg, ErrorCode, GameKey, MAX_CLOUD_SAVE_BYTES, MAX_SAVE_BYTES, SAVE_PART_BYTES,
    SAVE_PARTS_PROTOCOL, SaveInfo, SaveLabel, ServerMsg, Tag, last_part_at,
};
use std::collections::{BTreeMap, BTreeSet};
use web_time::Instant;

impl Online {
    pub(super) fn fits_cloud(&self, bytes: usize) -> bool {
        bytes <= MAX_SAVE_BYTES || (self.speaks_save_parts() && bytes <= MAX_CLOUD_SAVE_BYTES)
    }

    fn speaks_save_parts(&self) -> bool {
        self.protocol >= SAVE_PARTS_PROTOCOL
            && self
                .info
                .as_ref()
                .is_some_and(|info| info.protocol >= SAVE_PARTS_PROTOCOL)
    }

    pub(super) fn put_save(
        &mut self,
        tag: Tag,
        game: GameKey,
        slot: String,
        base: u64,
        label: Vec<u8>,
        data: Vec<u8>,
    ) {
        let upload = Upload {
            game,
            slot,
            base,
            label,
        };
        for frame in save_frames(tag, upload, data) {
            self.send_bytes(frame);
        }
    }

    fn cloud_send_parts(&mut self) {
        let Some(tag) = self.cloud.sending.as_ref().map(|(tag, _)| *tag) else {
            return;
        };
        if !matches!(&self.cloud.op, Some(Op::Put { tag: t, .. }) if *t == tag) {
            self.cloud.sending = None;
            return;
        }
        let Some((_, frames)) = self.cloud.sending.as_mut() else {
            return;
        };
        let frame = frames.pop_front();
        if frames.is_empty() {
            self.cloud.sending = None;
        }
        if let Some(frame) = frame {
            self.send_bytes(frame);
        }
    }
}

pub(super) fn save_frames(tag: Tag, upload: Upload, mut data: Vec<u8>) -> Vec<Vec<u8>> {
    let mut msgs = Vec::new();
    if data.len() + upload.label.len() > MAX_SAVE_BYTES {
        let last = data.split_off(last_part_at(data.len()));
        for part in data.chunks(SAVE_PART_BYTES) {
            msgs.push(ClientMsg::SavePart {
                tag,
                data: part.to_vec(),
            });
        }
        data = last;
    }
    msgs.push(ClientMsg::PutSave {
        tag,
        game: upload.game,
        slot: upload.slot,
        base: upload.base,
        label: upload.label,
        data,
    });
    msgs.iter().filter_map(|msg| msg.encode().ok()).collect()
}

impl Online {
    pub(crate) fn cloud_reset(&mut self) {
        self.cloud.save_ledger();
        self.cloud.disconnect();
    }

    pub(crate) fn cloud_tick(&mut self, now: Instant) {
        let Some(store) = self.cloud.store.clone() else {
            return;
        };
        let touched = store.take_touched();
        let account = (self.cloud.account.is_active() || self.cloud.walker.is_active())
            && self.cloud.owner.is_some();
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
            self.cloud.touched(&rom);
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
            self.cloud_expire(now);
            self.cloud_opened();
            self.cloud_send_parts();
            self.cloud_sealed(now);
            if self.cloud.listing.is_some()
                && self.cloud.op.is_none()
                && !self.walker_pump()
                && !self.account_pump(now)
                && self.cloud.enabled
            {
                self.cloud_pump(now);
            }
        }
        self.cloud.save_ledger();
    }

    pub(super) fn cloud_offline(&mut self, now: Instant) {
        let ready: Vec<String> = self
            .cloud
            .due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(rom, _)| rom.clone())
            .collect();
        for rom in ready {
            let shown = self.cloud.open.as_ref().is_some_and(|(r, _)| *r == rom)
                || self.cloud.states.keys().any(|(r, _)| *r == rom);
            if shown {
                let Some(items) = self.cloud.scanned(&rom) else {
                    continue;
                };
                self.cloud.offline_states(&rom, &items);
            }
            self.cloud.due.remove(&rom);
            self.cloud.rescan_if_stale(&rom, now);
        }
    }

    pub(super) fn cloud_storage_changed(&mut self) {
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
            .filter(|(_, (_, state))| state.waits_for_storage())
            .map(|(key, _)| key.clone())
            .collect();
        let at = Instant::now();
        for key in full {
            self.cloud.refusals.remove(&key);
            self.cloud.due.insert(key.0, at);
        }
    }

    pub(super) fn refusal_state(&self, error: ErrorCode) -> SyncState {
        match error {
            ErrorCode::QuotaFull => SyncState::Full,
            ErrorCode::TooLarge => SyncState::TooLarge,
            _ if self.info().is_some_and(|i| i.quota_bytes > 0) => SyncState::NoAccountStorage,
            _ => SyncState::NoStorage,
        }
    }

    fn cloud_expire(&mut self, now: Instant) {
        let late = self
            .cloud
            .op_at
            .is_some_and(|at| now.duration_since(at) >= OP_TIMEOUT);
        if !late {
            return;
        }
        let Some(op) = self.cloud.op.take() else {
            return;
        };
        self.cloud.sealing = None;
        self.cloud.sending = None;
        self.cloud.opening = None;
        log::warn!(
            "a cloud request got no answer in {} s",
            OP_TIMEOUT.as_secs()
        );
        match op {
            Op::Put { rom, .. } => {
                self.cloud.due.insert(rom, now);
            }
            Op::Get { game, .. } if self.is_walker(game) => self.walker_lost(),
            Op::Get { game, .. } | Op::Delete { game, .. } => self.cloud_redo(game),
            Op::AccountPut { .. } => {}
            Op::WalkerPut { .. } => self.walker_lost(),
        }
    }

    pub(super) fn cloud_pump(&mut self, now: Instant) {
        if let Some((game, slot, base)) = self.cloud.deletes.pop() {
            let tag = self.cloud.tag();
            self.send(ClientMsg::DeleteSave {
                tag,
                game,
                slot: slot.clone(),
                base,
            });
            self.cloud.start(Op::Delete { tag, game, slot });
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
        let Some(items) = self.cloud.scanned(&rom) else {
            return;
        };
        match self.cloud.reconcile(&rom, game, &items) {
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
                hash,
            }) => {
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
                let tag = self.cloud.tag();
                log::debug!("cloud upload {save:?} of {rom}: {} bytes", payload.size());
                let upload = Upload {
                    game,
                    slot: slot.clone(),
                    base,
                    label: sealed_label,
                };
                self.cloud.seal(tag, upload, keys, payload);
                self.cloud.start(Op::Put {
                    tag,
                    rom,
                    slot,
                    hash,
                    label,
                });
                self.cloud_sealed(now);
            }
            Some(Action::Get { slot }) => {
                self.cloud.parts.clear();
                self.send(ClientMsg::GetSave {
                    game,
                    slot: slot.clone(),
                });
                self.cloud.start(Op::Get { game, slot });
            }
            Some(Action::Delete { slot, base }) => {
                let tag = self.cloud.tag();
                self.send(ClientMsg::DeleteSave {
                    tag,
                    game,
                    slot: slot.clone(),
                    base,
                });
                self.cloud.start(Op::Delete { tag, game, slot });
            }
        }
        self.cloud.rescan_if_stale(&rom, now);
    }

    pub(super) fn cloud_remote(&self, info: SaveInfo) -> Remote {
        let label = self
            .keys
            .as_ref()
            .and_then(|keys| keys.open_label(&info.label).ok());
        Remote { info, label }
    }

    pub(super) fn cloud_list_insert(&mut self, remote: Remote) {
        let key = (remote.info.game, remote.info.slot.clone());
        if let Some(listing) = self.cloud.listing.as_mut() {
            listing.insert(key, remote);
        }
    }

    pub(super) fn cloud_redo(&mut self, game: GameKey) {
        if let Some(rom) = self.cloud.rom_of.get(&game).cloned() {
            self.cloud.due.insert(rom, Instant::now());
        }
    }

    fn is_walker(&self, game: GameKey) -> bool {
        self.keys.as_ref().map(super::walker::walker_key) == Some(game)
    }

    pub(super) fn cloud_refused(&mut self, tag: Tag, error: ErrorCode, current: Option<SaveInfo>) {
        if matches!(&self.cloud.op, Some(Op::WalkerPut { tag: t, .. }) if *t == tag) {
            self.cloud.op = None;
            let changed = error == ErrorCode::SaveChanged;
            if changed {
                match current {
                    Some(info) => {
                        let remote = self.cloud_remote(info);
                        self.cloud_list_insert(remote);
                    }
                    None => {
                        let key = self.keys.as_ref().map(super::walker::walker_key);
                        if let (Some(game), Some(listing)) = (key, self.cloud.listing.as_mut()) {
                            listing.retain(|(g, _), _| *g != game);
                        }
                    }
                }
            }
            self.walker_refused(changed);
            return;
        }
        let game = match &self.cloud.op {
            Some(Op::Put { tag: t, rom, .. }) if *t == tag => {
                let rom = rom.clone();
                self.keys.as_ref().map(|k| k.game_key(&rom_bytes(&rom)))
            }
            Some(Op::Delete { tag: t, game, .. }) if *t == tag => Some(*game),
            Some(Op::AccountPut { tag: t, .. }) if *t == tag => {
                self.keys.as_ref().map(super::account::account_key)
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
                    Op::WalkerPut { .. } => return,
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
            let state = self.refusal_state(error);
            self.cloud
                .refusals
                .insert((rom.clone(), slot), (hash, state));
            self.cloud.due.insert(rom, Instant::now());
        }
        self.cloud.changed = true;
    }

    pub(crate) fn cloud_receive(&mut self, msg: ServerMsg) -> Option<ServerMsg> {
        match msg {
            ServerMsg::Saves(list) => {
                if !self.cloud.enabled
                    && !self.cloud.account.is_active()
                    && !self.cloud.walker.is_active()
                {
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
            ServerMsg::SaveStored { tag, info } if matches!(&self.cloud.op, Some(Op::WalkerPut { tag: t, .. }) if *t == tag) => {
                if let Some(Op::WalkerPut { label, .. }) = self.cloud.op.take() {
                    self.walker_stored(info, label);
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
            ServerMsg::SavePart { game, slot, data } => {
                if matches!(&self.cloud.op, Some(Op::Get { game: g, slot: s }) if *g == game && *s == slot)
                    && self.cloud.parts.len() + data.len() <= MAX_CLOUD_SAVE_BYTES
                {
                    self.cloud.parts.extend_from_slice(&data);
                }
            }
            ServerMsg::Save { info, data } => {
                let Some(Op::Get { game, slot }) = &self.cloud.op else {
                    return None;
                };
                if *game != info.game || *slot != info.slot {
                    return None;
                }
                let mut whole = std::mem::take(&mut self.cloud.parts);
                whole.extend_from_slice(&data);
                let data = whole;
                if self.keys.as_ref().map(super::account::account_key) == Some(info.game) {
                    self.cloud.op = None;
                    let remote = self.cloud_remote(info);
                    self.account_fetched(remote, &data);
                    return None;
                }
                if self.is_walker(info.game) {
                    self.cloud.op = None;
                    let remote = self.cloud_remote(info);
                    self.walker_fetched(remote, &data);
                    return None;
                }
                let remote = self.cloud_remote(info.clone());
                self.cloud_list_insert(remote);
                self.cloud.open(info, self.keys.as_ref(), data);
                self.cloud_opened();
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
                if self.is_walker(game) {
                    self.walker_lost();
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
            ServerMsg::SavesChanged => self.cloud.asked = false,
            other => return Some(other),
        }
        None
    }

    fn cloud_sealed(&mut self, now: Instant) {
        let Some((tag, upload, sealed)) = self.cloud.sealed() else {
            return;
        };
        if !matches!(&self.cloud.op, Some(Op::Put { tag: t, .. }) if *t == tag) {
            return;
        }
        let Some(sealed) = sealed else {
            if let Some(Op::Put { rom, .. }) = self.cloud.op.take() {
                log::warn!("a cloud save of {rom} could not be sealed");
                self.cloud.due.insert(rom, now);
            }
            return;
        };
        if !self.fits_cloud(sealed.len + upload.label.len()) {
            self.cloud_refused(tag, ErrorCode::TooLarge, None);
            return;
        }
        let remote = self
            .cloud
            .listing
            .as_ref()
            .and_then(|l| l.get(&(upload.game, upload.slot.clone())));
        if let Some(error) = self.cloud.doomed(sealed.len, upload.label.len(), remote) {
            self.cloud_refused(tag, error, None);
            return;
        }
        self.cloud.sending = Some((tag, sealed.frames.into()));
        self.cloud_send_parts();
    }

    fn cloud_opened(&mut self) {
        let Some((info, opened)) = self.cloud.opened() else {
            return;
        };
        if !matches!(&self.cloud.op, Some(Op::Get { game, slot }) if *game == info.game && *slot == info.slot)
        {
            return;
        }
        self.cloud.op = None;
        let SaveInfo {
            game,
            slot,
            version,
            ..
        } = info;
        let Some(rom) = self.cloud.rom_of.get(&game).cloned() else {
            return;
        };
        match opened {
            Some((payload, hash)) => {
                let key = (rom.clone(), slot.clone());
                let foreign = self
                    .cloud
                    .store
                    .as_ref()
                    .is_some_and(|store| !store.takes(&rom, &payload));
                match self.cloud.conflicts.get_mut(&key) {
                    Some(_) if foreign => {
                        self.cloud.unreadable.insert((game, slot, version));
                    }
                    Some(conflict) if conflict.version == version => {
                        conflict.cloud.thumb = thumb_of(&payload);
                        conflict.payload = Some(payload);
                    }
                    Some(_) => {}
                    None => {
                        if !self
                            .cloud
                            .write_fetched(&rom, &slot, &payload, hash, version)
                        {
                            self.cloud.unreadable.insert((game, slot, version));
                        }
                    }
                }
            }
            None => {
                log::warn!("a cloud save of {rom} could not be opened");
                self.cloud.unreadable.insert((game, slot, version));
            }
        }
        self.cloud.due.insert(rom, Instant::now());
        self.cloud.changed = true;
    }

    pub(super) fn cloud_listed_now(&mut self) {
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
