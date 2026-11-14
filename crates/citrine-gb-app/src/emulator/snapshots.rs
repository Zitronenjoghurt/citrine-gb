use super::Emulator;
use super::runner::Taken;
use crate::autosave::{PlayClock, StateCopy};
use crate::storage::{Write, now_unix};
use citrine_gb::console::Console;
use citrine_gb::error::GbResult;
use citrine_gb::gb::GbModel;
use citrine_gb::persistence::state::StateBody;

impl Emulator {
    pub fn save_snapshot(&mut self, slot: usize) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let core = self.seat.core_mut();
        let state = core.machine.state_body(now_unix())?;
        let frame = core.machine.frame().to_vec();
        self.writes.send(Write::Snapshot {
            store: self.store.clone(),
            key,
            slot,
            state,
            frame,
        });
        self.started_at = Some(web_time::Instant::now());
        self.stat_events.snapshots += 1;
        Ok(())
    }

    pub fn load_snapshot(&mut self, slot: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        self.writes.settle();
        let Some(state) = self.store.load_snapshot(&key, slot) else {
            return Ok(false);
        };
        self.restore(&state)?;
        self.stat_events.loads += 1;
        Ok(true)
    }

    pub fn load_autosave(&mut self, id: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        self.writes.settle();
        let Some(state) = self.store.load_autosave(&key, id) else {
            return Ok(false);
        };
        self.restore(&state)?;
        self.stat_events.loads += 1;
        Ok(true)
    }

    pub(super) fn restore(&mut self, state: &[u8]) -> GbResult<()> {
        let restored = self
            .seat
            .core_mut()
            .machine
            .restore(state, now_unix(), &self.firmware)?;
        let sample_rate = self.seat.core_mut().machine.sample_rate();
        if restored.kind() != self.seat.core_mut().machine.kind() {
            self.console_switched = GbModel::try_from(restored.kind()).ok();
        }
        self.replace_machine(restored);
        self.seat.core_mut().machine.set_sample_rate(sample_rate);
        self.sync_joypad();
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.copy_last_good();
        Ok(())
    }

    pub fn delete_autosave(&mut self, id: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.writes.settle();
            self.store.delete_autosave(&key, id)?;
        }
        Ok(())
    }

    pub fn autosave(&mut self) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        let core = self.seat.core_mut();
        let state = core.machine.state_body(now_unix())?;
        let frame = core.machine.frame().to_vec();
        self.send_autosave(key, state, frame)
    }

    fn send_autosave(&mut self, key: String, state: StateBody, frame: Vec<u8>) -> GbResult<bool> {
        self.play.attempted();
        self.flush_save()?;
        self.writes.send(Write::Autosave {
            store: self.store.clone(),
            key,
            state,
            frame,
            keep: self.autosave_keep,
        });
        self.play.saved();
        self.last_good.mark_saved();
        self.started_at = Some(web_time::Instant::now());
        Ok(true)
    }

    pub fn autosave_if_played(&mut self) -> GbResult<bool> {
        if !self.play.has_unsaved_play() {
            return Ok(false);
        }
        self.autosave()
    }

    pub fn autosave_if_due(&mut self) -> GbResult<bool> {
        if !self.play.autosave_due(self.autosave_interval) {
            return Ok(false);
        }
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
        match self.play_copy() {
            None => Ok(false),
            Some(Ok(taken)) => self.send_autosave(key, taken.state, taken.frame),
            Some(Err(err)) => {
                self.play.attempted();
                Err(err)
            }
        }
    }

    pub(super) fn copy_last_good(&mut self) {
        if self.rom_key.is_none() {
            self.play.copied();
            return;
        }
        let taken = Taken::of(&mut self.seat.core_mut().machine, now_unix());
        self.keep_copy(taken);
    }

    pub(super) fn copy_in_play(&mut self) {
        if self.rom_key.is_none() {
            self.play.copied();
            return;
        }
        if let Some(taken) = self.play_copy() {
            self.keep_copy(taken);
        }
    }

    fn play_copy(&mut self) -> Option<GbResult<Taken>> {
        if let Some(ready) = self.ready.take() {
            return Some(Ok(ready));
        }
        if self.lending || self.lent.is_some() {
            self.want_copy = true;
            return None;
        }
        Some(Taken::of(&mut self.seat.core_mut().machine, now_unix()))
    }

    fn keep_copy(&mut self, taken: GbResult<Taken>) {
        self.play.copied();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let taken = match taken {
            Ok(taken) => taken,
            Err(err) => {
                log::error!("could not copy the state for autosave: {err:?}");
                return;
            }
        };
        self.last_good.keep(StateCopy {
            store: self.store.clone(),
            rom_key: key,
            state: taken.state,
            frame: taken.frame,
            battery: taken.battery,
            keep: self.autosave_keep,
            unsaved: self.play.has_unsaved_play(),
        });
    }

    pub fn delete_snapshot(&mut self, slot: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.writes.settle();
            self.store.delete_snapshot(&key, slot)?;
        }
        Ok(())
    }

    pub fn snapshot_name(&self, slot: usize) -> String {
        self.rom_key
            .as_deref()
            .and_then(|key| self.store.snapshot_name(key, slot))
            .unwrap_or_else(|| crate::storage::default_slot_name(slot))
    }

    pub fn rename_snapshot(&mut self, slot: usize, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.writes.settle();
            self.store.set_snapshot_name(&key, slot, name)?;
        }
        Ok(())
    }
}
