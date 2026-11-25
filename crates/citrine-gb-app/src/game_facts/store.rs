use super::facts::{Facts, Sheet};
use super::wiki::Page;
use crate::covers::System;
use crate::storage::Folders;
use crate::storage::kinds::Folder;

const FACTS: Folder = Folder::GameFacts;
const PAGES: Folder = Folder::GamePages;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Found {
    pub fetched: u64,
    pub page: Option<Page>,
    pub facts: Facts,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Kept {
    fetched: u64,
    sheet: Sheet,
}

#[derive(serde::Serialize)]
struct Keep<'a> {
    fetched: u64,
    sheet: &'a Sheet,
}

#[derive(Clone)]
pub struct InfoStore {
    backend: Folders,
}

impl InfoStore {
    pub(crate) fn on(backend: Folders) -> Self {
        Self { backend }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self::on(Folders::at(root))
    }

    pub fn sheet(&self, system: System) -> Option<(u64, Sheet)> {
        #[cfg(test)]
        super::facts::count_parse();
        let kept: Kept = serde_json::from_slice(&self.backend.read(FACTS, system.short())?).ok()?;
        Some((kept.fetched, kept.sheet))
    }

    pub fn put_sheet(&self, system: System, now: u64, sheet: &Sheet) -> std::io::Result<()> {
        let keep = Keep {
            fetched: now,
            sheet,
        };
        self.backend
            .write(FACTS, system.short(), &serde_json::to_vec(&keep)?)
    }

    pub fn found(&self, rom_key: &str) -> Option<Found> {
        serde_json::from_slice(&self.backend.read(PAGES, rom_key)?).ok()
    }

    pub fn put_found(&self, rom_key: &str, found: &Found) -> std::io::Result<()> {
        self.backend
            .write(PAGES, rom_key, &serde_json::to_vec(found)?)
    }
}
