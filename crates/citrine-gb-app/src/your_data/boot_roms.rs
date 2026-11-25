use super::kept_file::{FilePart, KeptFile};
use crate::app::boot_rom::BootRom;
use crate::storage::SettingsStore;

pub struct BootRoms<'a> {
    pub store: &'a SettingsStore,
    pub gb: &'a mut Option<Vec<u8>>,
    pub gbc: &'a mut Option<Vec<u8>>,
    pub names: &'a mut [String; 2],
}

impl BootRoms<'_> {
    pub fn part(&mut self, file: BootRom) -> FilePart<'_, BootRom> {
        let slot = match file {
            BootRom::Gb => &mut *self.gb,
            BootRom::Gbc => &mut *self.gbc,
        };
        FilePart {
            file,
            store: self.store,
            image: slot,
            name: &mut self.names[file.index()],
        }
    }
}

impl KeptFile for BootRom {
    fn dir(self) -> &'static str {
        match self {
            BootRom::Gb => "gb-boot-rom/",
            BootRom::Gbc => "gbc-boot-rom/",
        }
    }

    fn nouns(self) -> (&'static str, &'static str) {
        match self {
            BootRom::Gb => (self.noun(), "GB boot ROMs"),
            BootRom::Gbc => (self.noun(), "GBC boot ROMs"),
        }
    }

    fn fits(self, data: &[u8]) -> bool {
        self.accepts(data)
    }

    fn keys(self) -> (&'static str, &'static str) {
        self.store_keys()
    }

    fn system_file(self) -> crate::app::system_files::SystemFile {
        crate::app::system_files::SystemFile::BootRom(self)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::your_data::{Files, Part};
    use citrine_gb::machine::Firmware;
    use strum::IntoEnumIterator;

    #[test]
    fn each_boot_rom_travels_with_its_name_and_a_wrong_size_stays_out() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path().join("settings"));
        let mut slots = Firmware {
            gbc_boot_rom: Some(vec![3; 0x900]),
            ..Firmware::default()
        };
        let mut names = [String::new(), "cgb_boot.bin".to_owned()];
        let mut files = Files::new();
        for file in BootRom::iter() {
            let mut roms = BootRoms {
                store: &store,
                gb: &mut slots.gb_boot_rom,
                gbc: &mut slots.gbc_boot_rom,
                names: &mut names,
            };
            roms.part(file).export(&mut files);
        }
        files.insert("gb-boot-rom/long.bin".to_owned(), vec![1; 0x900]);
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["gb-boot-rom/long.bin", "gbc-boot-rom/cgb_boot.bin"]
        );

        let other_dir = tempfile::tempdir().unwrap();
        let other_store = SettingsStore::at(other_dir.path().join("settings"));
        let mut other_slots = Firmware::default();
        let mut other_names: [String; 2] = Default::default();
        for file in BootRom::iter() {
            let mut roms = BootRoms {
                store: &other_store,
                gb: &mut other_slots.gb_boot_rom,
                gbc: &mut other_slots.gbc_boot_rom,
                names: &mut other_names,
            };
            let added = roms.part(file).plan(&files).added;
            assert_eq!(added, usize::from(file == BootRom::Gbc), "{file:?}");
            roms.part(file).import(&files).unwrap();
        }
        assert_eq!(other_slots.gbc_boot_rom, Some(vec![3; 0x900]));
        assert_eq!(other_slots.gb_boot_rom, None);
        assert_eq!(
            other_store.kept_file(BootRom::Gbc.store_keys()),
            Some((vec![3; 0x900], "cgb_boot.bin".to_owned()))
        );
    }
}
