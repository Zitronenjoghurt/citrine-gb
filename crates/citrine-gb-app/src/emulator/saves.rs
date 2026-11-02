use super::{Emulator, PLAYING_AFTER, SAVE_COOLDOWN, SaveImport, SaveStatus};
#[cfg(not(target_arch = "wasm32"))]
use crate::storage::SaveStore;
use crate::storage::now_unix;
use citrine_gb::error::GbResult;
use citrine_gb::persistence::rtc_footer::RtcFooter;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;

impl Emulator {
    pub fn handle_save(&mut self) -> GbResult<()> {
        if let Some(link) = &mut self.link {
            link.handle_save(&self.store)?;
        }
        if let Some(last_save) = self.last_save
            && last_save.elapsed() < SAVE_COOLDOWN
        {
            return Ok(());
        }
        self.flush_save()
    }

    pub fn flush_save(&mut self) -> GbResult<()> {
        if let Some(link) = &mut self.link {
            link.flush_save(&self.store)?;
        }
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        let Some(sdump) = self.gb.poll_sram_dump(false, now_unix()) else {
            return Ok(());
        };
        self.store.store_battery(&key, sdump.as_slice())?;
        self.last_save = Some(web_time::Instant::now());
        Ok(())
    }

    pub fn name_game(&mut self, file_name: Option<&str>) {
        let header = &self.gb.cartridge.header;
        self.game_name = crate::game_name::game_name(
            &header.sha1,
            file_name,
            &header.title,
            header.cgb_mode.is_cgb(),
        );
    }

    pub fn game_name(&self) -> &str {
        &self.game_name
    }

    pub fn default_battery_name(&self) -> String {
        if self.game_name.is_empty() {
            "Battery save".to_string()
        } else {
            self.game_name.clone()
        }
    }

    pub fn rename_battery(&mut self, name: &str) -> GbResult<()> {
        if let Some(key) = self.rom_key.clone() {
            self.store
                .set_battery_name(&key, name, &self.default_battery_name())?;
        }
        Ok(())
    }

    pub fn reload_battery(&mut self) -> GbResult<()> {
        if self.rom_key.is_none() {
            return Ok(());
        }
        let running = self.running;
        let rom = Rom::new(&self.gb.cartridge.rom_bytes());
        let name = self.game_name.clone();
        let launched = self.stat_events.launched;
        #[cfg(not(target_arch = "wasm32"))]
        let path = self.rom_path.clone();
        self.load_rom(
            &rom,
            self.gb.model,
            #[cfg(not(target_arch = "wasm32"))]
            path.as_deref(),
        )?;
        self.game_name = name;
        self.stat_events.launched = launched;
        self.running = running;
        Ok(())
    }

    pub fn use_battery(&mut self, token: &str) -> GbResult<()> {
        let Some(key) = self.rom_key.clone() else {
            return Ok(());
        };
        self.flush_save()?;
        self.store.swap_battery(&key, token)?;
        self.reload_battery()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn rom_path(&self) -> Option<&std::path::Path> {
        self.rom_path.as_deref()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn reimport_legacy_save(&mut self) -> bool {
        let (Some(key), Some(path)) = (self.rom_key.clone(), self.rom_path.clone()) else {
            return false;
        };
        if !self.store.import_legacy_save(&key, &path) {
            return false;
        }
        if let Some(data) = self.store.load_battery(&key) {
            self.gb
                .put_sram_dump(SramDump::from_slice(&data), now_unix());
            self.save_loaded = true;
            return true;
        }
        false
    }

    pub fn import_save(
        &mut self,
        ram: Option<&[u8]>,
        clock: Option<RtcFooter>,
    ) -> GbResult<SaveImport> {
        if let Some(blocked) = self.import_blocker(clock.is_some()) {
            return Ok(blocked);
        }
        let Some(key) = self.rom_key.clone() else {
            return Ok(SaveImport::NoRom);
        };
        let now = now_unix();
        match (ram, clock) {
            (Some(ram), None) => {
                self.store.store_battery(&key, ram)?;
                self.gb.put_sram_dump(SramDump::from_slice(ram), now);
            }
            (ram, Some(footer)) => {
                if let Some(ram) = ram {
                    self.gb.put_sram_dump(SramDump::from_slice(ram), now);
                }
                self.gb.cartridge.put_rtc_footer(footer, now);
                if let Some(dump) = self.gb.poll_sram_dump(true, now) {
                    self.store.store_battery(&key, dump.as_slice())?;
                }
            }
            (None, None) => return Ok(SaveImport::Nothing),
        }
        self.save_loaded = true;
        self.last_save = Some(web_time::Instant::now());
        Ok(SaveImport::Imported)
    }

    pub fn import_blocker(&self, with_clock: bool) -> Option<SaveImport> {
        if self.rom_key.is_none() {
            Some(SaveImport::NoRom)
        } else if with_clock && self.gb.cartridge.rtc().is_none() {
            Some(SaveImport::NoClock)
        } else {
            None
        }
    }

    pub fn has_been_playing(&self) -> bool {
        self.started_at
            .is_some_and(|started| started.elapsed() >= PLAYING_AFTER)
    }

    pub fn restart(&mut self) {
        self.gb.soft_reset();
        self.running = true;
        self.started_at = Some(web_time::Instant::now());
    }

    pub fn export_rtc_files(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        let footer = self.gb.cartridge.rtc_footer(now_unix())?;
        Some((
            self.gb.cartridge.battery_ram(),
            footer.encode_rtc_file().to_vec(),
        ))
    }

    pub fn export_save_bytes(&mut self) -> Option<Vec<u8>> {
        self.gb
            .poll_sram_dump(true, now_unix())
            .map(|dump| dump.as_slice().to_vec())
    }

    pub fn save_status(&self) -> SaveStatus {
        let key = self.rom_key.as_deref();
        SaveStatus {
            name: key
                .and_then(|k| self.store.battery_name(k))
                .unwrap_or_else(|| self.default_battery_name()),
            battery_backed: self.gb.cartridge.has_battery(),
            stored: key.is_some_and(|k| self.store.has_battery(k)),
            saved_at: key.and_then(|k| self.store.battery_saved_at(k)),
            location: self.store.location(),
            has_clock: self.gb.cartridge.rtc().is_some(),
            #[cfg(not(target_arch = "wasm32"))]
            legacy_path: self
                .rom_path
                .as_deref()
                .map(SaveStore::legacy_save_path)
                .filter(|p| p.is_file()),
        }
    }
}
