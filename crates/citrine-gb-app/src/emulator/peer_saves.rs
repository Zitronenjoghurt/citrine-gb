use super::Emulator;
use crate::storage::cloud::LocalSave;
use crate::storage::{BatteryFiles, Write, now_unix};
use citrine_gb::console::Console;
use citrine_gb::error::GbResult;
use citrine_gb_net::OnlineId;
use serde::{Deserialize, Serialize};

const PENDING: &str = "peer.pending";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pending {
    own: String,
    friend: OnlineId,
    name: Option<String>,
    after_session: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divert {
    pub own: String,
    pub borrowed: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerNotice {
    pub friend: OnlineId,
    pub divert: Divert,
    pub name: Option<String>,
}

impl PeerNotice {
    fn from_pending(pending: Pending, borrowed: String) -> Self {
        Self {
            friend: pending.friend,
            divert: Divert {
                own: pending.own,
                borrowed,
            },
            name: pending.name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptSaves {
    divert: Divert,
    before: Option<BatteryFiles>,
    battery_name: Option<String>,
    aside: Option<String>,
    filed: FiledSaves,
    pending: Option<Pending>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FiledSaves {
    snapshots: Vec<(usize, Option<String>)>,
    autosaves: Vec<usize>,
}

impl KeptSaves {
    #[cfg(test)]
    pub fn aside(&self) -> Option<&str> {
        self.aside.as_deref()
    }

    pub fn set_mine_aside(&self) -> bool {
        self.aside.is_some()
    }
}

impl Emulator {
    pub fn saves_key(&self) -> Option<String> {
        match &self.divert {
            Some(divert) if self.rom_key.as_deref() == Some(divert.own.as_str()) => {
                Some(divert.borrowed.clone())
            }
            _ => self.rom_key.clone(),
        }
    }

    pub(super) fn start_divert(&mut self, friend: OnlineId, divert: Divert) {
        let name = self
            .read_pending(&divert.borrowed)
            .filter(|old| old.own == divert.own && old.friend == friend)
            .and_then(|old| old.name);
        let pending = Pending {
            own: divert.own.clone(),
            friend,
            name,
            after_session: false,
        };
        self.write_pending(&divert.borrowed, &pending);
        self.peer_notice
            .push(PeerNotice::from_pending(pending, divert.borrowed.clone()));
        self.divert = Some(divert);
    }

    pub(super) fn stop_divert(&mut self) {
        if let Some(divert) = self.divert.take() {
            self.ask_again_after_session(divert);
        }
        #[cfg(ds_wifi)]
        {
            self.ds_wifi.peer_console = false;
        }
    }

    fn ask_again_after_session(&mut self, divert: Divert) {
        let Some(mut pending) = self
            .read_pending(&divert.borrowed)
            .filter(|pending| pending.after_session)
        else {
            return;
        };
        self.writes.settle();
        if self.has_peer_files(&divert.borrowed) {
            pending.after_session = false;
            self.write_pending(&divert.borrowed, &pending);
            self.peer_notice
                .push(PeerNotice::from_pending(pending, divert.borrowed));
        } else {
            self.drop_pending(&divert.borrowed);
        }
    }

    pub fn reopen_peer_notices(&mut self) {
        for key in self.store.save_keys() {
            if !key.starts_with(crate::storage::BORROWED) {
                continue;
            }
            let Some(mut pending) = self.read_pending(&key) else {
                continue;
            };
            if self.has_peer_files(&key) {
                pending.after_session = false;
                self.write_pending(&key, &pending);
                self.peer_notice
                    .push(PeerNotice::from_pending(pending, key));
            } else {
                self.drop_pending(&key);
            }
        }
    }

    pub fn name_peer_saves(&mut self, notice: &PeerNotice) {
        if let Some(mut pending) = self.read_pending(&notice.divert.borrowed) {
            pending.name.clone_from(&notice.name);
            self.write_pending(&notice.divert.borrowed, &pending);
        }
    }

    fn read_pending(&self, borrowed: &str) -> Option<Pending> {
        let bytes = self.store.read_file(borrowed, PENDING)?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_pending(&self, borrowed: &str, pending: &Pending) {
        let written = serde_json::to_vec(pending)
            .map_err(std::io::Error::other)
            .and_then(|bytes| self.store.put_file(borrowed, PENDING, &bytes));
        if let Err(err) = written {
            log::warn!(
                "the question about a friend's saves won't come back after a restart: {err}"
            );
        }
    }

    fn drop_pending(&self, borrowed: &str) {
        if self.store.read_file(borrowed, PENDING).is_none() {
            return;
        }
        if let Err(err) = self.store.remove_file(borrowed, PENDING) {
            log::warn!("couldn't clear the question about a friend's saves: {err}");
        }
    }

    fn has_peer_files(&self, borrowed: &str) -> bool {
        self.store.battery_files(borrowed).is_some()
            || !self.store.used_slots(borrowed).is_empty()
            || !self.store.autosave_ids(borrowed).is_empty()
    }

    pub fn diverting(&self, divert: &Divert) -> bool {
        self.divert.as_ref() == Some(divert)
    }

    #[cfg(test)]
    pub fn divert(&self) -> Option<&Divert> {
        self.divert.as_ref()
    }

    pub fn take_peer_notice(&mut self) -> Option<PeerNotice> {
        (!self.peer_notice.is_empty()).then(|| self.peer_notice.remove(0))
    }

    pub fn keep_peer_saves(&mut self, divert: &Divert, friend: &str) -> GbResult<KeptSaves> {
        let running = self.divert.as_ref() == Some(divert);
        let loaded = self.rom_key.as_deref() == Some(divert.own.as_str());
        if loaded {
            self.flush_save()?;
        }
        self.writes.settle();
        let before = self.store.battery_files(&divert.own);
        let battery_name = self.store.battery_name(&divert.own);
        let mut aside = None;
        if let Some(theirs) = self.store.battery_files(&divert.borrowed) {
            if let Some(mine) = self.store.read_payload(&divert.own, &LocalSave::Battery) {
                let token = self.store.add_other_battery(&divert.own, &mine)?;
                let name = battery_name
                    .clone()
                    .unwrap_or_else(|| format!("Your save before {friend}'s"));
                self.store
                    .set_other_battery_name(&divert.own, &token, &name)?;
                aside = Some(token);
            }
            self.store.put_battery_files(&divert.own, &theirs)?;
            self.store
                .set_battery_name(&divert.own, &format!("{friend}'s save"), "")?;
            if loaded && !running {
                let machine = &mut self.seat.core_mut().machine;
                let save = machine.battery_from_file(&theirs.sav);
                machine.put_battery(save, now_unix());
            }
        }
        let filed = self.file_friend_saves(divert, friend)?;
        if running {
            self.divert = None;
        }
        let pending = self.read_pending(&divert.borrowed);
        self.drop_pending(&divert.borrowed);
        Ok(KeptSaves {
            divert: divert.clone(),
            before,
            battery_name,
            aside,
            filed,
            pending,
        })
    }

    pub fn discard_peer_saves(&mut self, divert: &Divert) -> GbResult<()> {
        self.writes.settle();
        let borrowed = divert.borrowed.as_str();
        self.store.remove_battery(borrowed)?;
        for slot in self.store.used_slots(borrowed) {
            self.store.delete_snapshot(borrowed, slot)?;
        }
        for id in self.store.autosave_ids(borrowed) {
            self.store.delete_autosave(borrowed, id)?;
        }
        self.answered(divert);
        Ok(())
    }

    pub fn set_peer_saves_aside(&mut self, divert: &Divert, friend: &str) -> GbResult<()> {
        self.writes.settle();
        let (from, to) = (divert.borrowed.as_str(), divert.own.as_str());
        if let Some(theirs) = self.store.read_payload(from, &LocalSave::Battery) {
            let token = self.store.add_other_battery(to, &theirs)?;
            self.store
                .set_other_battery_name(to, &token, &format!("{friend}'s save"))?;
            self.store.remove_battery(from)?;
        }
        self.file_friend_saves(divert, friend)?;
        self.answered(divert);
        Ok(())
    }

    fn file_friend_saves(&self, divert: &Divert, friend: &str) -> std::io::Result<FiledSaves> {
        let (from, to) = (divert.borrowed.as_str(), divert.own.as_str());
        let mut filed = FiledSaves::default();
        for slot in self.store.used_slots(from) {
            let given = self.store.snapshot_name(from, slot);
            let name = match &given {
                Some(name) => format!("{friend}'s {name}"),
                None => format!("{friend}'s snapshot"),
            };
            let moved = self.store.move_snapshot(from, slot, to)?;
            self.store.set_snapshot_name(to, moved, &name)?;
            filed.snapshots.push((moved, given));
        }
        for id in self.store.autosave_ids(from) {
            let moved = self.store.autosave_to_snapshot(from, id, to)?;
            self.store
                .set_snapshot_name(to, moved, &format!("{friend}'s autosave"))?;
            self.store.file_as_autosave(to, moved)?;
            filed.autosaves.push(moved);
        }
        Ok(filed)
    }

    fn answered(&self, divert: &Divert) {
        let borrowed = divert.borrowed.as_str();
        if self.divert.as_ref() == Some(divert) {
            if let Some(mut pending) = self.read_pending(borrowed) {
                pending.after_session = true;
                self.write_pending(borrowed, &pending);
            }
        } else {
            self.drop_pending(borrowed);
        }
    }

    pub fn undo_kept_saves(&mut self, kept: &KeptSaves) -> GbResult<()> {
        self.writes.settle();
        let own = kept.divert.own.as_str();
        match &kept.before {
            Some(files) => self.store.put_battery_files(own, files)?,
            None => self.store.remove_battery(own)?,
        }
        self.store
            .set_battery_name(own, kept.battery_name.as_deref().unwrap_or(""), "")?;
        if let Some(token) = &kept.aside {
            self.store
                .delete_save(own, &LocalSave::OtherBattery(token.clone()))?;
        }
        let borrowed = kept.divert.borrowed.as_str();
        for (slot, given) in &kept.filed.snapshots {
            let moved = self.store.move_snapshot(own, *slot, borrowed)?;
            self.store
                .set_snapshot_name(borrowed, moved, given.as_deref().unwrap_or(""))?;
        }
        for &slot in &kept.filed.autosaves {
            self.store.snapshot_to_autosave(own, slot, borrowed)?;
        }
        if self.rom_key.as_deref() == Some(own) {
            self.divert = Some(kept.divert.clone());
            if let Some(save) = self.seat.core_mut().machine.battery(true, now_unix()) {
                self.writes.send(Write::Battery {
                    store: self.store.clone(),
                    key: kept.divert.borrowed.clone(),
                    save,
                });
            }
        }
        if let Some(pending) = &kept.pending {
            let mut pending = pending.clone();
            pending.after_session = false;
            self.write_pending(borrowed, &pending);
            self.peer_notice
                .push(PeerNotice::from_pending(pending, borrowed.to_owned()));
        }
        Ok(())
    }
}
