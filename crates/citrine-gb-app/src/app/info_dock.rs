use super::{Citrine, GAME_INFO_DOCK_SHARE};
use crate::app::tabs::Tab;
use citrine_gb_gamedata::Ram;
use egui_dock::SurfaceIndex;

impl Citrine {
    pub(super) fn watch_game_info(&mut self) {
        let key = self.emulator.rom_key().map(str::to_owned);
        if key.as_deref() != self.game_info.rom_key() {
            let rom = self.game_info_rom();
            let saved_at = key
                .as_deref()
                .and_then(|key| self.emulator.store.battery_saved_at(key));
            self.game_info.set_rom(key.as_deref(), &rom, saved_at);
            if !self.game_info.recognised() {
                self.close_game_info();
            }
        }
        let open =
            self.dock.find_tab(&Tab::GameInfo).is_some() || self.mobile.showing(Tab::GameInfo);
        let now = web_time::Instant::now();
        let read = open && self.game_info.due(now);
        let progress = self.game_info.progress_due(now);
        if !read && !progress {
            return;
        }
        let unix = crate::storage::now_unix();
        if let Some(gb) = self.emulator.as_gb() {
            let battery = gb.cartridge.battery_ram();
            let wram: Vec<u8> = (0..2)
                .filter_map(|bank| gb.wram_bank(bank))
                .flatten()
                .copied()
                .collect();
            self.game_info.read(&battery, Ram::Gb(&wram), unix);
        } else {
            #[cfg(feature = "gba")]
            if let Some(gba) = self.emulator.as_gba() {
                let memory = gba_memory::GbaMemory::read(gba);
                self.game_info.read(&memory.battery, memory.ram(), unix);
            }
        }
        if progress
            && let (Some(rom), Some(state)) = (self.game_info.rom_key(), self.game_info.progress())
        {
            let day = crate::stats::day_key(crate::stats::local_date(unix));
            self.stats.recorder().progress(rom, &day, state);
        }
    }

    fn game_info_rom(&self) -> Vec<u8> {
        if let Some(gb) = self.emulator.as_gb() {
            return gb.cartridge.rom_bytes();
        }
        #[cfg(feature = "gba")]
        if let Some(gba) = self.emulator.as_gba() {
            return gba.bus().rom().to_vec();
        }
        Vec::new()
    }

    pub(super) fn widen_for_game_info(&mut self) {
        let Some((surface, node, _)) = self.dock.find_tab(&Tab::GameInfo) else {
            return;
        };
        let Some(parent) = node.parent() else {
            return;
        };
        if surface == SurfaceIndex::main()
            && node.is_right()
            && let egui_dock::Node::Horizontal(split) = &mut self.dock.main_surface_mut()[parent]
            && split.fraction > GAME_INFO_DOCK_SHARE
        {
            split.fraction = GAME_INFO_DOCK_SHARE;
        }
    }

    pub(super) fn close_game_info(&mut self) {
        if let Some(location) = self.dock.find_tab(&Tab::GameInfo) {
            self.dock.remove_tab(location);
        }
        if self.mobile.showing(Tab::GameInfo) {
            self.mobile.close();
        }
    }
}

#[cfg(feature = "gba")]
mod gba_memory {
    use citrine_gb::gba::machine::Gba;
    use citrine_gb::gba::region::{EWRAM_SIZE, IWRAM_SIZE};
    use citrine_gb_gamedata::Ram;
    use citrine_gb_gamedata::gen3::Memory;

    const EWRAM: u32 = 0x0200_0000;
    const IWRAM: u32 = 0x0300_0000;

    pub(super) struct GbaMemory {
        pub battery: Vec<u8>,
        ewram: Vec<u8>,
        iwram: Vec<u8>,
    }

    impl GbaMemory {
        pub fn read(gba: &Gba) -> Self {
            let bus = gba.bus();
            let copy = |start: u32, len: usize| -> Vec<u8> {
                (0..len as u32)
                    .step_by(4)
                    .flat_map(|at| bus.read32(start + at).to_le_bytes())
                    .collect()
            };
            Self {
                battery: bus.backup().bytes().to_vec(),
                ewram: copy(EWRAM, EWRAM_SIZE),
                iwram: copy(IWRAM, IWRAM_SIZE),
            }
        }

        pub fn ram(&self) -> Ram<'_> {
            Ram::Gba(Memory {
                ewram: &self.ewram,
                iwram: &self.iwram,
            })
        }
    }
}
