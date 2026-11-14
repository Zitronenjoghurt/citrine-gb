use citrine_gb::console::ConsoleKind;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod copies;

const MAX_ENTRIES: usize = 8;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "StoredRom", into = "StoredRom")]
pub struct RecentRom {
    pub path: PathBuf,
    pub title: String,
    pub last_played: u64,
    pub rom_key: Option<String>,
    pub console: Option<ConsoleKind>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct StoredRom {
    path: PathBuf,
    title: String,
    last_played: u64,
    #[serde(default)]
    rom_key: Option<String>,
    #[serde(default)]
    console: Option<ConsoleKind>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    nds: bool,
}

impl From<StoredRom> for RecentRom {
    fn from(stored: StoredRom) -> Self {
        Self {
            path: stored.path,
            title: stored.title,
            last_played: stored.last_played,
            rom_key: stored.rom_key,
            console: if stored.nds {
                Some(ConsoleKind::Nds)
            } else {
                stored.console
            },
        }
    }
}

impl From<RecentRom> for StoredRom {
    fn from(rom: RecentRom) -> Self {
        let nds = rom.console == Some(ConsoleKind::Nds);
        Self {
            path: rom.path,
            title: rom.title,
            last_played: rom.last_played,
            rom_key: rom.rom_key,
            console: rom.console.filter(|_| !nds),
            nds,
        }
    }
}

impl RecentRom {
    pub fn is_gba(&self) -> bool {
        self.console.map_or_else(
            || {
                self.path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("gba"))
            },
            |console| console == ConsoleKind::Gba,
        )
    }

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    icons: BTreeMap<PathBuf, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    games: BTreeMap<PathBuf, String>,
    #[serde(skip)]
    named: bool,
}

impl RecentRoms {
    pub fn record(&mut self, path: &Path, title: &str, rom_key: Option<String>) -> Vec<RecentRom> {
        self.record_on(path, title, rom_key, None)
    }

    pub fn record_on(
        &mut self,
        path: &Path,
        title: &str,
        rom_key: Option<String>,
        console: Option<ConsoleKind>,
    ) -> Vec<RecentRom> {
        self.entries.retain(|e| e.path != path);
        self.entries.insert(
            0,
            RecentRom {
                path: path.to_path_buf(),
                title: title.trim().to_string(),
                last_played: now_unix(),
                rom_key,
                console,
            },
        );
        let dropped = self.entries.split_off(MAX_ENTRIES.min(self.entries.len()));
        self.keep_icons();
        dropped
    }

    pub fn set_icon(&mut self, path: &Path, icon: Option<String>) {
        match icon {
            Some(icon) => self.icons.insert(path.to_path_buf(), icon),
            None => self.icons.remove(path),
        };
        self.keep_icons();
    }

    pub fn icon(&self, path: &Path) -> Option<&str> {
        self.icons.get(path).map(String::as_str)
    }

    pub fn set_game_key(&mut self, path: &Path, rom_key: String) {
        self.games.insert(path.to_path_buf(), rom_key);
        self.keep_icons();
    }

    pub fn game_key(&self, path: &Path) -> Option<&str> {
        self.games.get(path).map(String::as_str).or_else(|| {
            self.entries
                .iter()
                .find(|entry| entry.path == path)
                .and_then(|entry| entry.rom_key.as_deref())
        })
    }

    fn keep_icons(&mut self) {
        let entries = &self.entries;
        let listed = |path: &PathBuf| entries.iter().any(|entry| entry.path == *path);
        self.icons.retain(|path, _| listed(path));
        self.games.retain(|path, _| listed(path));
    }

    pub fn prune(&mut self) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|e| crate::utils::zip_games::on_disk(&e.path));
        self.keep_icons();
        before - self.entries.len()
    }

    pub fn name_from_files(&mut self) {
        if std::mem::replace(&mut self.named, true) {
            return;
        }
        for entry in &mut self.entries {
            let Ok(data) = std::fs::read(&entry.path) else {
                continue;
            };
            #[cfg(feature = "nds")]
            if entry.console == Some(ConsoleKind::Nds)
                && let Some(title) = crate::ds_banner::game_name(
                    &data,
                    &citrine_gb::rom::id::rom_sha1(&data),
                    entry.path.file_name().and_then(|name| name.to_str()),
                )
            {
                entry.title = title;
                continue;
            }
            let Ok(header) = citrine_gb::rom::Rom::new(&data).header() else {
                continue;
            };
            let file_name = entry.path.file_name().map(|n| n.to_string_lossy());
            entry.title = crate::game_name::game_name(
                &header.sha1,
                file_name.as_deref(),
                header.short_title(),
            );
        }
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
        self.keep_icons();
    }

    pub fn entries(&self) -> &[RecentRom] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn merged(&self, other: &[RecentRom]) -> (Vec<RecentRom>, usize, usize) {
        let mut entries = self.entries.clone();
        let mut added = 0;
        let mut replaced = 0;
        for theirs in other {
            match entries.iter_mut().find(|e| e.path == theirs.path) {
                Some(mine) if theirs.last_played > mine.last_played => {
                    *mine = theirs.clone();
                    replaced += 1;
                }
                Some(_) => {}
                None => {
                    entries.push(theirs.clone());
                    added += 1;
                }
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.last_played));
        entries.truncate(MAX_ENTRIES);
        (entries, added, replaced)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn replace(&mut self, entries: Vec<RecentRom>) {
        self.entries = entries;
        self.named = false;
        self.keep_icons();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.icons.clear();
        self.named = false;
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
    fn a_ds_entry_keeps_its_console_and_older_builds_still_read_it() {
        #[derive(serde::Deserialize)]
        enum OlderConsole {
            Gb,
            Gbc,
            Gba,
        }
        #[derive(serde::Deserialize)]
        struct OlderRom {
            #[serde(default)]
            console: Option<OlderConsole>,
        }
        let entry = RecentRom {
            path: PathBuf::from("/roms/Both Screens.nds"),
            title: "Both Screens Console".to_owned(),
            last_played: 7,
            rom_key: Some("KEY".to_owned()),
            console: Some(ConsoleKind::Nds),
        };
        let text = serde_json::to_string(&entry).expect("write");
        assert_eq!(
            serde_json::from_str::<RecentRom>(&text).expect("read"),
            entry
        );
        let older: OlderRom = serde_json::from_str(&text).expect("older build");
        assert!(older.console.is_none());
        let gba = serde_json::to_string(&RecentRom {
            console: Some(ConsoleKind::Gba),
            ..entry
        })
        .expect("write");
        let older: OlderRom = serde_json::from_str(&gba).expect("older build");
        assert!(matches!(older.console, Some(OlderConsole::Gba)));
    }

    #[test]
    fn an_entry_is_a_gba_game_by_its_console_or_else_its_extension() {
        let entry = |path: &str, console| RecentRom {
            path: PathBuf::from(path),
            title: String::new(),
            last_played: 0,
            rom_key: None,
            console,
        };
        assert!(entry("/roms/Emerald.GBA", None).is_gba());
        assert!(!entry("/roms/Silver.gbc", None).is_gba());
        assert!(entry("/roms/game.bin", Some(ConsoleKind::Gba)).is_gba());
        assert!(!entry("/roms/game.gba", Some(ConsoleKind::Gb)).is_gba());
    }

    #[test]
    fn most_recent_comes_first_without_duplicates() {
        let mut recent = RecentRoms::default();
        recent.record(Path::new("/a.gb"), "A", None);
        recent.record(Path::new("/b.gb"), "B", None);
        recent.record(Path::new("/a.gb"), "A", None);

        assert_eq!(recent.entries().len(), 2, "re-playing does not duplicate");
        assert_eq!(recent.entries()[0].path, PathBuf::from("/a.gb"));
    }

    #[test]
    fn the_list_is_capped() {
        let mut recent = RecentRoms::default();
        for i in 0..MAX_ENTRIES + 5 {
            recent.record(Path::new(&format!("/rom{i}.gb")), "R", None);
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
        recent.record(&kept, "Kept", None);
        recent.record(&removed, "Removed", None);
        std::fs::remove_file(&removed).expect("remove");

        assert_eq!(recent.prune(), 1);
        assert_eq!(recent.entries().len(), 1);
        assert_eq!(recent.entries()[0].path, kept);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recording_past_the_cap_hands_back_the_oldest() {
        let mut recent = RecentRoms::default();
        for i in 0..MAX_ENTRIES {
            assert!(
                recent
                    .record(Path::new(&format!("/rom{i}.gb")), "R", None)
                    .is_empty()
            );
        }
        let dropped = recent.record(Path::new("/new.gb"), "N", None);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].path, PathBuf::from("/rom0.gb"));
    }

    #[test]
    fn copies_keep_one_file_per_game_and_drop_only_their_own() {
        let dir = temp_dir("copies");
        let copies = copies::RomCopies::at(dir.join("roms"));
        let first = copies.keep("abc", "Game.GBC", b"rom").expect("keep");
        let again = copies.keep("abc", "Game.GBC", b"rom").expect("keep");
        assert_eq!(first, again);
        assert_eq!(first.extension().and_then(|e| e.to_str()), Some("gbc"));
        let outside = touch(&dir, "outside.gb");
        copies.drop_copies([first.as_path(), outside.as_path()]);
        assert!(!first.exists());
        assert!(outside.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn display_name_falls_back_to_the_file_name() {
        let mut recent = RecentRoms::default();
        recent.record(Path::new("/roms/untitled.gb"), "   ", None);
        assert_eq!(recent.entries()[0].display_name(), "untitled.gb");

        recent.record(Path::new("/roms/tetris.gb"), "TETRIS", None);
        assert_eq!(recent.entries()[0].display_name(), "TETRIS");
    }
}
