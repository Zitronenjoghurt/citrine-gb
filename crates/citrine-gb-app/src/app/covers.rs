use super::Citrine;
use crate::covers::Game;
use crate::storage::now_unix;
use citrine_gb::console::Console;

impl Citrine {
    pub(super) fn find_covers(&mut self) {
        self.big.covers.follow_spawner(&self.files.spawner);
        if !self.ui.settings.covers {
            self.covers.forget_all();
            return;
        }
        let store = self.emulator.store.covers();
        for key in self.covers.poll(&store, &self.files.spawner, now_unix()) {
            self.big.covers.forget(&key);
        }
    }

    pub(super) fn want_cover(&mut self, file_name: &str, rom: &[u8]) {
        if !self.ui.settings.covers {
            return;
        }
        let Some(key) = self.emulator.rom_key() else {
            return;
        };
        let sha1 = self.emulator.debug().rom_id().sha1;
        let game = Game::of(key, self.emulator.kind(), sha1, rom).named(file_name);
        self.covers
            .want(&self.emulator.store.covers(), game, now_unix());
    }
}
