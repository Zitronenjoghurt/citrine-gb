use super::{Citrine, GAME_INFO_DOCK_SHARE};
use crate::app::tabs::Tab;
use citrine_gb_gamedata::Ram;
use egui_dock::SurfaceIndex;
use std::sync::Arc;

#[cfg(feature = "nds")]
const WALKER_REDRAW: std::time::Duration = std::time::Duration::from_millis(250);

impl Citrine {
    pub(super) fn watch_game_info(&mut self) {
        let key = self.emulator.rom_key().map(str::to_owned);
        if key.as_deref() != self.game_info.rom_key() {
            let rom = self.game_info_rom();
            let saved_at = key
                .as_deref()
                .and_then(|key| self.emulator.store.battery_saved_at(key));
            self.game_info
                .set_rom(key.as_deref(), rom, saved_at, &self.threads);
            if !self.game_info.loading() {
                self.close_game_info();
            }
        }
        if self.game_info.settle() == Some(false) {
            self.close_game_info();
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
            #[cfg(feature = "nds")]
            if let Some(nds) = self.emulator.as_nds() {
                self.game_info
                    .read(&[], Ram::Ds(nds.buses().main_ram()), unix);
            }
        }
        if progress
            && let (Some(rom), Some(state)) = (self.game_info.rom_key(), self.game_info.progress())
        {
            let day = crate::stats::day_key(crate::stats::local_date(unix));
            self.stats.recorder().progress(rom, &day, state);
        }
    }

    fn game_info_rom(&self) -> Arc<[u8]> {
        if let Some(gb) = self.emulator.as_gb() {
            return gb.cartridge.rom_bytes();
        }
        #[cfg(feature = "gba")]
        if let Some(gba) = self.emulator.as_gba() {
            return gba.bus().shared_rom();
        }
        #[cfg(feature = "nds")]
        if let Some(nds) = self.emulator.as_nds() {
            return nds.rom_bytes();
        }
        Arc::from([])
    }

    pub(super) fn widen_for_game_info(&mut self) {
        self.widen_for(Tab::GameInfo, GAME_INFO_DOCK_SHARE);
    }

    #[cfg(feature = "nds")]
    pub(super) fn open_walker(&mut self) {
        self.open_tab(Tab::Walker);
    }

    #[cfg(feature = "nds")]
    pub(super) fn follow_walker(&mut self, ctx: &egui::Context) {
        let now_ms = (ctx.input(|i| i.time) * 1000.0) as u64;
        let online = &self.ui.settings.online;
        let synced = online.cloud_saves && online.cloud_owner().is_some();
        self.emulator.sync_walker(&mut self.online, synced);
        let showing =
            self.dock.find_tab(&Tab::Walker).is_some() || self.mobile.showing(Tab::Walker);
        self.follow_phone_steps(showing);
        self.emulator
            .set_steps_in_game(self.ui.settings.walker.in_game);
        self.emulator.tick_walker(now_ms);
        if self.emulator.take_walker_linked() && !self.is_mobile(ctx) {
            self.open_walker();
        }
        if showing {
            ctx.request_repaint_after(WALKER_REDRAW);
        }
    }

    #[cfg(all(feature = "nds", target_os = "android"))]
    fn follow_phone_steps(&mut self, showing: bool) {
        use crate::platform::android::steps;
        let on = self.ui.settings.walker.phone;
        steps::follow(on, on && (showing || self.ui.settings.walker_asked));
        let pokewalker = &mut self.emulator.pokewalker;
        if on != pokewalker.has_step_source() {
            pokewalker.set_step_source(on.then(|| {
                Box::new(crate::walker::PhoneSteps::new(
                    Box::new(steps::reading),
                    crate::storage::Backend::settings(),
                )) as Box<dyn crate::walker::StepSource>
            }));
        }
    }

    #[cfg(all(feature = "nds", not(target_os = "android")))]
    fn follow_phone_steps(&mut self, _showing: bool) {}

    fn widen_for(&mut self, tab: Tab, share: f32) {
        let Some((surface, node, _)) = self.dock.find_tab(&tab) else {
            return;
        };
        let Some(parent) = node.parent() else {
            return;
        };
        if surface == SurfaceIndex::main()
            && node.is_right()
            && let egui_dock::Node::Horizontal(split) = &mut self.dock.main_surface_mut()[parent]
            && split.fraction > share
        {
            split.fraction = share;
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
