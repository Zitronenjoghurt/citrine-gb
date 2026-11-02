use super::Emulator;
use crate::autosave::{PlayClock, StateCopy};
use crate::storage::now_unix;
use citrine_gb::error::GbResult;
use citrine_gb::persistence::full_dump::FullDumpRef;
use citrine_gb::rom::Rom;

impl Emulator {
    pub fn save_snapshot(&mut self, slot: usize) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let state = self.gb.dump_full(now_unix())?;
        let frame = self.gb.frame().as_slice().to_vec();
        self.store
            .store_snapshot(&key, slot, &state, &frame, self.gb.model)?;
        self.started_at = Some(web_time::Instant::now());
        self.stat_events.snapshots += 1;
        Ok(())
    }

    pub fn load_snapshot(&mut self, slot: usize) -> GbResult<bool> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(false);
        };
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
        let Some(state) = self.store.load_autosave(&key, id) else {
            return Ok(false);
        };
        self.restore(&state)?;
        self.stat_events.loads += 1;
        Ok(true)
    }

    pub(super) fn restore(&mut self, state: &[u8]) -> GbResult<()> {
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let restored = citrine_gb::gb::GameBoy::from_dump(state, &rom, now_unix())?;
        let sample_rate = self.gb.apu.output_sample_rate;
        if restored.model != self.gb.model {
            self.console_switched = Some(restored.model);
        }
        self.gb = restored;
        self.gb.apu.set_sample_rate(sample_rate);
        self.sync_joypad();
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
        self.play = PlayClock::default();
        self.copy_last_good();
        Ok(())
    }

    pub fn delete_autosave(&mut self, id: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store.delete_autosave(&key, id)?;
        }
        Ok(())
    }

    pub fn autosave(&mut self) -> GbResult<Option<usize>> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(None);
        };
        self.play.attempted();
        self.flush_save()?;
        let state = self.gb.dump_full(now_unix())?;
        let frame = self.gb.frame().as_slice().to_vec();
        let id =
            self.store
                .store_autosave(&key, &state, &frame, self.gb.model, self.autosave_keep)?;
        self.play.saved();
        self.last_good.mark_saved();
        self.started_at = Some(web_time::Instant::now());
        Ok(Some(id))
    }

    pub fn autosave_if_played(&mut self) -> GbResult<Option<usize>> {
        if !self.play.has_unsaved_play() {
            return Ok(None);
        }
        self.autosave()
    }

    pub fn autosave_if_due(&mut self) -> GbResult<Option<usize>> {
        if !self.play.autosave_due(self.autosave_interval) {
            return Ok(None);
        }
        self.autosave()
    }

    pub(super) fn copy_last_good(&mut self) {
        self.play.copied();
        let Some(key) = self.rom_key.clone() else {
            return;
        };
        let sram = self.gb.poll_sram_dump(true, now_unix());
        let battery = sram.as_ref().map(|dump| dump.as_slice().to_vec());
        let state = match (FullDumpRef { gb: &self.gb, sram }).to_rmp() {
            Ok(state) => state,
            Err(err) => {
                log::error!("could not copy the state for autosave: {err:?}");
                return;
            }
        };
        self.last_good.keep(StateCopy {
            store: self.store.clone(),
            rom_key: key,
            state,
            frame: self.gb.frame().as_slice().to_vec(),
            model: self.gb.model,
            battery,
            keep: self.autosave_keep,
            unsaved: self.play.has_unsaved_play(),
        });
    }

    pub fn delete_snapshot(&mut self, slot: usize) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
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
            self.store.set_snapshot_name(&key, slot, name)?;
        }
        Ok(())
    }
}
