use super::names::{Names, System};
use crate::storage::Folders;
use crate::storage::kinds::Folder;

const ART: Folder = Folder::Covers;
const NAMES: Folder = Folder::CoverNames;
const MISS_MARK: u8 = 2;
pub const FOLDERS: [Folder; 2] = [ART, NAMES];

#[derive(Clone)]
pub struct CoverStore {
    backend: Folders,
}

impl CoverStore {
    pub(crate) fn on(backend: Folders) -> Self {
        Self { backend }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn at(root: std::path::PathBuf) -> Self {
        Self::on(Folders::at(root))
    }

    pub fn art(&self, rom_key: &str) -> Option<Vec<u8>> {
        self.backend.read(ART, &format!("{rom_key}.png"))
    }

    pub fn has_art(&self, rom_key: &str) -> bool {
        self.backend.exists(ART, &format!("{rom_key}.png"))
    }

    pub fn put_art(&self, rom_key: &str, png: &[u8]) -> std::io::Result<()> {
        self.backend.delete(ART, &format!("{rom_key}.miss"))?;
        self.backend.write(ART, &format!("{rom_key}.png"), png)
    }

    pub fn missed_at(&self, rom_key: &str) -> Option<u64> {
        let raw = self.backend.read(ART, &format!("{rom_key}.miss"))?;
        if raw.get(8) != Some(&MISS_MARK) {
            return None;
        }
        Some(u64::from_le_bytes(raw.get(..8)?.try_into().ok()?))
    }

    pub fn put_miss(&self, rom_key: &str, now: u64) -> std::io::Result<()> {
        let mut raw = now.to_le_bytes().to_vec();
        raw.push(MISS_MARK);
        self.backend.write(ART, &format!("{rom_key}.miss"), &raw)
    }

    pub fn names(&self, system: System) -> Option<(u64, Names)> {
        let raw = self.backend.read(NAMES, system.short())?;
        let fetched = u64::from_le_bytes(raw.get(..8)?.try_into().ok()?);
        Some((fetched, Names::from_bytes(&raw[8..])?))
    }

    pub fn put_names(&self, system: System, now: u64, names: &Names) -> std::io::Result<()> {
        let mut raw = now.to_le_bytes().to_vec();
        raw.extend(names.to_bytes());
        self.backend.write(NAMES, system.short(), &raw)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const KEY: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const NOW: u64 = 1_800_000_000;

    #[test]
    fn a_miss_from_before_the_wider_match_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        Folders::at(dir.path().to_path_buf())
            .write(ART, &format!("{KEY}.miss"), &NOW.to_le_bytes())
            .unwrap();
        let store = CoverStore::at(dir.path().to_path_buf());
        assert_eq!(store.missed_at(KEY), None);
        store.put_miss(KEY, NOW).unwrap();
        assert_eq!(store.missed_at(KEY), Some(NOW));
    }
}
