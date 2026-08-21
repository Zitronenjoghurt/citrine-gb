use std::path::{Path, PathBuf};
const MAX_ENTRIES: usize = 8;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RecentRom {
    pub path: PathBuf,
    pub title: String,
    pub last_played: u64,
}

impl RecentRom {
    pub fn display_name(&self) -> String {
        if self.title.is_empty() {
            self.path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string())
        } else {
            self.title.clone()
        }
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct RecentRoms {
    entries: Vec<RecentRom>,
}

impl RecentRoms {
    pub fn record(&mut self, path: &Path, title: &str) {
        self.entries.retain(|e| e.path != path);
        self.entries.insert(
            0,
            RecentRom {
                path: path.to_path_buf(),
                title: title.trim().to_string(),
                last_played: now_unix(),
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn prune(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.path.is_file());
        before - self.entries.len()
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    pub fn entries(&self) -> &[RecentRom] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn now_unix() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"rom").expect("write");
        path
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("citrine-recent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn most_recent_comes_first_without_duplicates() {
        let mut recent = RecentRoms::default();
        recent.record(Path::new("/a.gb"), "A");
        recent.record(Path::new("/b.gb"), "B");
        recent.record(Path::new("/a.gb"), "A");

        assert_eq!(recent.entries().len(), 2, "re-playing does not duplicate");
        assert_eq!(recent.entries()[0].path, PathBuf::from("/a.gb"));
    }

    #[test]
    fn the_list_is_capped() {
        let mut recent = RecentRoms::default();
        for i in 0..MAX_ENTRIES + 5 {
            recent.record(Path::new(&format!("/rom{i}.gb")), "R");
        }
        assert_eq!(recent.entries().len(), MAX_ENTRIES);
        assert_eq!(
            recent.entries()[0].path,
            PathBuf::from(&format!("/rom{}.gb", MAX_ENTRIES + 4)),
            "newest first"
        );
    }

    #[test]
    fn pruning_drops_files_that_no_longer_exist() {
        let dir = temp_dir("prune");
        let kept = touch(&dir, "kept.gb");
        let removed = touch(&dir, "removed.gb");

        let mut recent = RecentRoms::default();
        recent.record(&kept, "Kept");
        recent.record(&removed, "Removed");
        std::fs::remove_file(&removed).expect("remove");

        assert_eq!(recent.prune(), 1);
        assert_eq!(recent.entries().len(), 1);
        assert_eq!(recent.entries()[0].path, kept);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn display_name_falls_back_to_the_file_name() {
        let mut recent = RecentRoms::default();
        recent.record(Path::new("/roms/untitled.gb"), "   ");
        assert_eq!(recent.entries()[0].display_name(), "untitled.gb");

        recent.record(Path::new("/roms/tetris.gb"), "TETRIS");
        assert_eq!(recent.entries()[0].display_name(), "TETRIS");
    }
}
