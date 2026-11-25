use super::Citrine;
use super::system_files::SystemFile;
use crate::utils::file_loader::{FileLoader, PickedFile};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum BootRom {
    Gb,
    Gbc,
}

impl BootRom {
    pub fn noun(self) -> &'static str {
        SystemFile::BootRom(self)
            .row()
            .label(super::ui_state::settings_tree::Surface::Desktop)
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Gb => crate::icons::GB_BOOT_ROM,
            Self::Gbc => crate::icons::GBC_BOOT_ROM,
        }
    }

    fn size(self) -> usize {
        match self {
            Self::Gb => 0x100,
            Self::Gbc => 0x900,
        }
    }

    pub(super) fn size_text(self) -> &'static str {
        match self {
            Self::Gb => "256 bytes",
            Self::Gbc => "2304 bytes",
        }
    }

    pub fn accepts(self, data: &[u8]) -> bool {
        data.len() == self.size()
    }

    pub fn store_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Gb => ("gb-boot-rom.bin", "gb-boot-rom-name.txt"),
            Self::Gbc => ("gbc-boot-rom.bin", "gbc-boot-rom-name.txt"),
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

impl Citrine {
    pub(super) fn load_boot_roms(&mut self) {
        for file in BootRom::iter() {
            match self.settings_store.kept_file(file.store_keys()) {
                Some((image, name)) if file.accepts(&image) => {
                    self.emulator.set_boot_rom(file, Some((image, name)));
                }
                Some(_) => log::warn!("ignoring the saved {}", file.noun()),
                None => {}
            }
        }
    }

    pub(super) fn pick_boot_rom(&mut self, file: BootRom) {
        let tx = self.files.boot_rom_tx.clone();
        FileLoader::new()
            .title(&format!("Pick a {}", file.noun()))
            .add_filter(file.noun(), &["bin", "rom"])
            .pick(&self.files.spawner, move |picked| {
                let _ = tx.send((file, picked));
            });
    }

    pub(super) fn handle_load_boot_rom(&mut self, file: BootRom, picked: PickedFile) {
        self.handle_system_file(SystemFile::BootRom(file), picked);
    }

    pub(super) fn keep_boot_rom(&mut self, file: BootRom, picked: PickedFile) -> Option<String> {
        if let Err(err) = self
            .settings_store
            .store_kept_file(file.store_keys(), Some((&picked.data, &picked.name)))
        {
            self.toasts
                .error(format!("Could not keep the {}: {err}", file.noun()));
            return None;
        }
        self.emulator
            .set_boot_rom(file, Some((picked.data.to_vec(), picked.name)));
        let mut done = format!("Your {} is set", file.noun());
        if self.emulator.has_game() && self.emulator.as_gb().is_some() {
            done.push_str(". It runs when the next Game Boy game starts.");
        }
        Some(done)
    }

    pub(super) fn remove_boot_rom(&mut self, file: BootRom) {
        if let Err(err) = self.settings_store.store_kept_file(file.store_keys(), None) {
            self.toasts
                .error(format!("Could not remove the {}: {err}", file.noun()));
            return;
        }
        self.emulator.set_boot_rom(file, None);
        self.toasts.success(format!("{} removed", file.noun()));
    }
}

#[cfg(test)]
mod tests {
    use super::BootRom;
    use crate::app::Citrine;
    use crate::app::tests::{load_game, phone_app};
    use crate::storage::SettingsStore;
    use crate::utils::file_loader::PickedFile;

    const TOBU: &[u8] = include_bytes!("../../homebrew/tobu.gb");

    fn boot_file(name: &str, size: usize) -> PickedFile {
        let mut data = vec![0; size];
        data[..2].copy_from_slice(&[0x18, 0xFE]);
        PickedFile {
            name: name.to_owned(),
            data: data.into(),
            path: None,
            seen: None,
        }
    }

    fn app_at(name: &str, dir: &std::path::Path) -> Citrine {
        let mut app = phone_app(name);
        app.settings_store = SettingsStore::at(dir.join("settings"));
        app.load_boot_roms();
        app
    }

    #[test]
    fn a_picked_boot_rom_is_kept_and_runs_the_next_game_of_its_console() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("boot-rom-pick", dir.path());
        assert!(!app.emulator.has_game());
        for (file, name, size) in [
            (BootRom::Gb, "dmg_boot.bin", 0x100),
            (BootRom::Gbc, "cgb_boot.bin", 0x900),
        ] {
            app.files
                .boot_rom_tx
                .send((file, boot_file(name, size)))
                .unwrap();
        }
        app.drain_file_channels();
        assert_eq!(
            app.toasts.texts(),
            ["Your GB boot ROM is set", "Your GBC boot ROM is set"]
        );

        let mut restarted = app_at("boot-rom-restart", dir.path());
        assert_eq!(
            restarted.emulator.boot_rom_name(BootRom::Gb),
            Some("dmg_boot.bin")
        );
        assert_eq!(
            restarted.emulator.boot_rom_name(BootRom::Gbc),
            Some("cgb_boot.bin")
        );
        restarted
            .emulator
            .load_rom(
                &citrine_gb::rom::Rom::new(TOBU),
                citrine_gb::gb::GbModel::Dmg,
                None,
            )
            .unwrap();
        let gb = restarted.emulator.as_gb().expect("a game boy");
        assert!(gb.boot_rom.mounted, "the GB boot ROM runs first");
        assert_eq!(gb.boot_rom.rom.len(), 0x100);
        assert_eq!(gb.cpu.pc, 0);

        load_game(&mut restarted, TOBU, None);
        let gb = restarted.emulator.as_gb().expect("a game boy color");
        assert!(
            gb.boot_rom.mounted,
            "the Game Boy Color boot ROM runs first"
        );
        assert_eq!(gb.boot_rom.rom.len(), 0x900);

        restarted.remove_boot_rom(BootRom::Gbc);
        load_game(&mut restarted, TOBU, None);
        assert!(
            !restarted.emulator.as_gb().unwrap().boot_rom.mounted,
            "removed"
        );
        let again = app_at("boot-rom-again", dir.path());
        assert_eq!(again.emulator.boot_rom_name(BootRom::Gbc), None);
        assert_eq!(
            again.emulator.boot_rom_name(BootRom::Gb),
            Some("dmg_boot.bin")
        );
    }

    #[test]
    fn a_boot_rom_picked_in_the_other_slot_is_kept_in_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("boot-rom-wrong", dir.path());
        app.handle_load_boot_rom(BootRom::Gb, boot_file("cgb_boot.bin", 0x900));
        app.handle_load_boot_rom(BootRom::Gbc, boot_file("odd.bin", 0x180));
        assert_eq!(
            app.toasts.texts(),
            [
                "Saved as your GBC boot ROM.",
                "'odd.bin' is 384 bytes. A GBC boot ROM is 2304 bytes."
            ]
        );
        assert_eq!(app.emulator.boot_rom_name(BootRom::Gb), None);
        assert_eq!(
            app.emulator.boot_rom_name(BootRom::Gbc),
            Some("cgb_boot.bin")
        );
        assert_eq!(app.settings_store.kept_file(BootRom::Gb.store_keys()), None);
    }
}
