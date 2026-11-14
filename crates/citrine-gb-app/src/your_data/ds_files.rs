use super::{Change, Files, Kind, Part, Tally};
use crate::app::ds_firmware::DsFile;
use crate::icons;
use crate::storage::SettingsStore;
use citrine_gb::nds::machine::NdsFirmware;
use std::io;

pub struct DsFiles<'a> {
    pub store: &'a SettingsStore,
    pub slots: &'a mut NdsFirmware,
    pub names: &'a mut [String; 3],
}

impl DsFiles<'_> {
    pub fn part(&mut self, file: DsFile) -> DsFilePart<'_> {
        let slot = match file {
            DsFile::Arm9Bios => &mut self.slots.arm9_bios,
            DsFile::Arm7Bios => &mut self.slots.arm7_bios,
            DsFile::Firmware => &mut self.slots.firmware,
        };
        DsFilePart {
            file,
            store: self.store,
            image: slot,
            name: &mut self.names[file.index()],
        }
    }
}

pub struct DsFilePart<'a> {
    file: DsFile,
    store: &'a SettingsStore,
    image: &'a mut Option<Vec<u8>>,
    name: &'a mut String,
}

impl DsFilePart<'_> {
    fn dir(&self) -> &'static str {
        match self.file {
            DsFile::Arm9Bios => "ds-arm9-bios/",
            DsFile::Arm7Bios => "ds-arm7-bios/",
            DsFile::Firmware => "ds-firmware/",
        }
    }

    fn theirs<'f>(&self, files: &'f Files) -> Option<(&'f str, &'f [u8])> {
        files.iter().find_map(|(path, bytes)| {
            let name = path.strip_prefix(self.dir())?;
            (!name.is_empty() && !name.contains('/') && self.file.accepts(bytes))
                .then_some((name, bytes.as_slice()))
        })
    }
}

impl Part for DsFilePart<'_> {
    fn kind(&self) -> Kind {
        let nouns = match self.file {
            DsFile::Arm9Bios => ("DS ARM9 BIOS", "DS ARM9 BIOS files"),
            DsFile::Arm7Bios => ("DS ARM7 BIOS", "DS ARM7 BIOS files"),
            DsFile::Firmware => ("DS firmware", "DS firmware files"),
        };
        Kind {
            name: nouns.0,
            icon: icons::CPU,
            units: ("", ""),
            nouns,
        }
    }

    fn tally(&mut self) -> Tally {
        self.image
            .as_ref()
            .map_or_else(Tally::default, |image| Tally {
                count: 1,
                bytes: image.len() as u64,
            })
    }

    fn export(&mut self, out: &mut Files) {
        if let Some(image) = self.image.as_ref() {
            out.insert(format!("{}{}", self.dir(), self.name), image.clone());
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        match (self.theirs(files), self.image.as_ref()) {
            (None, _) => Change::default(),
            (Some(_), None) => Change {
                added: 1,
                replaced: 0,
            },
            (Some((name, theirs)), Some(mine)) => Change {
                added: 0,
                replaced: usize::from(theirs != mine.as_slice() || name != self.name.as_str()),
            },
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let Some((name, image)) = self.theirs(files) else {
            return Ok(());
        };
        self.store.store_ds_file(self.file, Some((image, name)))?;
        *self.name = name.to_owned();
        *self.image = Some(image.to_vec());
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        *self.image = None;
        self.name.clear();
        self.store.store_ds_file(self.file, None)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
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
            other.store.ds_file(DsFile::Arm7Bios),
            Some((vec![7; 0x4000], "biosnds7.rom".to_owned()))
        );
    }
}
