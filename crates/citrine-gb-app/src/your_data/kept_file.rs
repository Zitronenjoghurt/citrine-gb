use super::{Change, Files, Kind, Part, Store, Tally};
use crate::app::system_files::SystemFile;
use crate::storage::SettingsStore;
use std::io;

pub trait KeptFile: Copy {
    fn dir(self) -> &'static str;
    fn nouns(self) -> (&'static str, &'static str);
    fn fits(self, data: &[u8]) -> bool;
    fn keys(self) -> (&'static str, &'static str);
    fn system_file(self) -> SystemFile;
}

pub struct FilePart<'a, F> {
    pub file: F,
    pub store: &'a SettingsStore,
    pub image: &'a mut Option<Vec<u8>>,
    pub name: &'a mut String,
}

impl<F: KeptFile> FilePart<'_, F> {
    fn theirs<'f>(&self, files: &'f Files) -> Option<(&'f str, &'f [u8])> {
        files.iter().find_map(|(path, bytes)| {
            let name = path.strip_prefix(self.file.dir())?;
            (!name.is_empty() && !name.contains('/') && self.file.fits(bytes))
                .then_some((name, bytes.as_slice()))
        })
    }
}

impl<F: KeptFile> Part for FilePart<'_, F> {
    fn kind(&self) -> Store {
        Store::SystemFiles
    }

    fn file_row(&self) -> Option<Kind> {
        let nouns = self.file.nouns();
        Some(Kind {
            name: nouns.0,
            nouns,
            store: crate::storage::kinds::Kind::SystemFiles,
            system_file: Some(self.file.system_file()),
        })
    }

    fn tally(&mut self) -> Tally {
        Tally {
            count: usize::from(self.image.is_some()),
        }
    }

    fn export(&mut self, out: &mut Files) {
        if let Some(image) = self.image.as_ref() {
            out.insert(format!("{}{}", self.file.dir(), self.name), image.clone());
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
        self.store
            .store_kept_file(self.file.keys(), Some((image, name)))?;
        *self.name = name.to_owned();
        *self.image = Some(image.to_vec());
        Ok(())
    }

    fn delete_all(&mut self) -> io::Result<()> {
        *self.image = None;
        self.name.clear();
        self.store.store_kept_file(self.file.keys(), None)
    }
}
