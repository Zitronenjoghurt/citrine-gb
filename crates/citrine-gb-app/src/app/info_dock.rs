use super::{Citrine, GAME_INFO_DOCK_SHARE};
use crate::app::tabs::Tab;
use egui_dock::SurfaceIndex;

impl Citrine {
    pub(super) fn watch_game_info(&mut self) {
        let key = self.emulator.rom_key().map(str::to_owned);
        if key.as_deref() != self.game_info.rom_key() {
            let rom = self.emulator.gb.cartridge.rom_bytes();
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
        let gb = &self.emulator.gb;
        let battery = gb.cartridge.battery_ram();
        let wram: Vec<u8> = (0..2)
            .filter_map(|bank| gb.wram_bank(bank))
            .flatten()
            .copied()
            .collect();
        let unix = crate::storage::now_unix();
        self.game_info.read(&battery, &wram, unix);
        if progress
            && let (Some(rom), Some(state)) = (self.game_info.rom_key(), self.game_info.progress())
        {
            let day = crate::stats::day_key(crate::stats::local_date(unix));
            self.stats.recorder().progress(rom, &day, state);
        }
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
