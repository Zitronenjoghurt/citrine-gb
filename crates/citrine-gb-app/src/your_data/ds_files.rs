use super::kept_file::{FilePart, KeptFile};
use crate::app::ds_firmware::DsFile;
use crate::storage::SettingsStore;
use citrine_gb::nds::machine::NdsFirmware;

pub struct DsFiles<'a> {
    pub store: &'a SettingsStore,
    pub slots: &'a mut NdsFirmware,
    pub names: &'a mut [String; 3],
}

impl DsFiles<'_> {
    pub fn part(&mut self, file: DsFile) -> FilePart<'_, DsFile> {
        let slot = match file {
            DsFile::Arm9Bios => &mut self.slots.arm9_bios,
            DsFile::Arm7Bios => &mut self.slots.arm7_bios,
            DsFile::Firmware => &mut self.slots.firmware,
        };
        FilePart {
            file,
            store: self.store,
            image: slot,
            name: &mut self.names[file.index()],
        }
    }
}

impl KeptFile for DsFile {
    fn dir(self) -> &'static str {
        match self {
            DsFile::Arm9Bios => "ds-arm9-bios/",
            DsFile::Arm7Bios => "ds-arm7-bios/",
            DsFile::Firmware => "ds-firmware/",
        }
    }

    fn nouns(self) -> (&'static str, &'static str) {
        match self {
            DsFile::Arm9Bios => ("DS ARM9 BIOS", "DS ARM9 BIOS files"),
            DsFile::Arm7Bios => ("DS ARM7 BIOS", "DS ARM7 BIOS files"),
            DsFile::Firmware => ("DS firmware", "DS firmware files"),
        }
    }

    fn fits(self, data: &[u8]) -> bool {
        self.accepts(data)
    }

    fn keys(self) -> (&'static str, &'static str) {
        self.store_keys()
    }

    fn system_file(self) -> crate::app::system_files::SystemFile {
        crate::app::system_files::SystemFile::Ds(self)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::your_data::{Files, Part};
    use strum::IntoEnumIterator;

    struct Home {
        _dir: tempfile::TempDir,
        store: SettingsStore,
        slots: NdsFirmware,
        names: [String; 3],
    }

    impl Home {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            Self {
                store: SettingsStore::at(dir.path().join("settings")),
                _dir: dir,
                slots: NdsFirmware::default(),
                names: Default::default(),
            }
        }

        fn files(&mut self) -> DsFiles<'_> {
            DsFiles {
                store: &self.store,
                slots: &mut self.slots,
                names: &mut self.names,
            }
        }
    }

    #[test]
    fn each_file_travels_with_its_name_and_a_wrong_size_stays_out() {
        let mut home = Home::new();
        home.slots.arm7_bios = Some(vec![7; 0x4000]);
        home.names[DsFile::Arm7Bios.index()] = "biosnds7.rom".to_owned();
        let mut files = Files::new();
        for file in DsFile::iter() {
            home.files().part(file).export(&mut files);
        }
        files.insert("ds-arm9-bios/short.bin".to_owned(), vec![9; 0x100]);
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["ds-arm7-bios/biosnds7.rom", "ds-arm9-bios/short.bin"]
        );

        let mut other = Home::new();
        for file in DsFile::iter() {
            let added = other.files().part(file).plan(&files).added;
            assert_eq!(added, usize::from(file == DsFile::Arm7Bios), "{file:?}");
            other.files().part(file).import(&files).unwrap();
        }
        assert_eq!(other.slots.arm7_bios, Some(vec![7; 0x4000]));
        assert_eq!(other.slots.arm9_bios, None);
        assert_eq!(
            other.store.kept_file(DsFile::Arm7Bios.store_keys()),
            Some((vec![7; 0x4000], "biosnds7.rom".to_owned()))
        );
    }
}
