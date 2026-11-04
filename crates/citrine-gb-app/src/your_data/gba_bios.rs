use super::{Change, Files, Kind, Part, Tally};
use crate::icons;
use crate::storage::SettingsStore;
use citrine_gb::gba::bios::Bios;
use std::io;

const BIOS_DIR: &str = "gba-bios/";

pub struct GbaBios<'a> {
    pub store: &'a SettingsStore,
    pub bios: &'a mut Option<Bios>,
    pub name: &'a mut String,
}

impl GbaBios<'_> {
    fn theirs(files: &Files) -> Option<(&str, Bios)> {
        files.iter().find_map(|(path, bytes)| {
            let name = path.strip_prefix(BIOS_DIR)?;
            (!name.is_empty() && !name.contains('/'))
                .then(|| Bios::user(bytes).ok())
                .flatten()
                .map(|bios| (name, bios))
        })
    }
}

impl Part for GbaBios<'_> {
    fn kind(&self) -> Kind {
        Kind {
            name: "GBA BIOS",
            icon: icons::CPU,
            units: ("", ""),
            nouns: ("GBA BIOS", "GBA BIOS files"),
        }
    }

    fn tally(&mut self) -> Tally {
        self.bios
            .as_ref()
            .map_or_else(Tally::default, |bios| Tally {
                count: 1,
                bytes: bios.image().len() as u64,
            })
    }

    fn export(&mut self, out: &mut Files) {
        if let Some(bios) = self.bios.as_ref() {
            out.insert(format!("{BIOS_DIR}{}", self.name), bios.image().to_vec());
        }
    }

    fn plan(&mut self, files: &Files) -> Change {
        match (Self::theirs(files), self.bios.as_ref()) {
            (None, _) => Change::default(),
            (Some(_), None) => Change {
                added: 1,
                replaced: 0,
            },
            (Some((name, theirs)), Some(mine)) => Change {
                added: 0,
                replaced: usize::from(theirs.image() != mine.image() || name != self.name),
            },
        }
    }

    fn import(&mut self, files: &Files) -> io::Result<()> {
        let Some((name, bios)) = Self::theirs(files) else {
            return Ok(());
        };
        self.store.store_gba_bios(Some((bios.image(), name)))?;
        *self.name = name.to_owned();
        *self.bios = Some(bios);
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        *self.bios = None;
        self.name.clear();
        self.store.store_gba_bios(None)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use citrine_gb::gba::bios::CULT_OF_GBA;

    struct Home {
        _dir: tempfile::TempDir,
        store: SettingsStore,
        bios: Option<Bios>,
        name: String,
    }

    impl Home {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            Self {
                store: SettingsStore::at(dir.path().join("settings")),
                _dir: dir,
                bios: None,
                name: String::new(),
            }
        }

        fn part(&mut self) -> GbaBios<'_> {
            GbaBios {
                store: &self.store,
                bios: &mut self.bios,
                name: &mut self.name,
            }
        }
    }

    #[test]
    fn the_bios_travels_with_its_file_name() {
        let mut home = Home::new();
        home.bios = Some(Bios::user(CULT_OF_GBA).unwrap());
        home.name = "gba_bios.bin".to_owned();
        let mut files = Files::new();
        home.part().export(&mut files);
        assert_eq!(home.part().tally().count, 1);
        assert_eq!(files["gba-bios/gba_bios.bin"].len(), 0x4000);

        let mut other = Home::new();
        assert_eq!(other.part().plan(&files).added, 1);
        other.part().import(&files).unwrap();
        assert_eq!(other.name, "gba_bios.bin");
        assert!(other.bios.is_some());
        assert_eq!(
            other.store.gba_bios(),
            Some((CULT_OF_GBA.to_vec(), "gba_bios.bin".to_owned()))
        );
        assert_eq!(other.part().plan(&files), Change::default());

        other.part().delete_all().unwrap();
        assert!(other.bios.is_none());
        assert_eq!(other.store.gba_bios(), None);
    }

    #[test]
    fn a_file_that_is_no_bios_imports_nothing() {
        let mut home = Home::new();
        let files = Files::from([("gba-bios/notes.txt".to_owned(), b"text".to_vec())]);
        assert_eq!(home.part().plan(&files), Change::default());
        home.part().import(&files).unwrap();
        assert!(home.bios.is_none());
    }
}
