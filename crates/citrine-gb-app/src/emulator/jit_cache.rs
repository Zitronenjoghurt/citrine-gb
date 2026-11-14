use super::{Emulator, Machine};
use crate::app::ui_state::settings::GBA_USES_JIT;
use crate::storage::Write;
use citrine_gb::nds::machine::{JIT_AVAILABLE, JitCache};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const SAVE_EVERY: Duration = Duration::from_secs(5 * 60);
pub const FOLDER_LIMIT: u64 = 256 << 20;
#[cfg(not(target_arch = "wasm32"))]
const FOLDER: &str = "jit";
const EXTENSION: &str = "jit";

impl Emulator {
    pub(super) fn follow_jit_cache(&mut self) {
        let fresh = match &self.seat.core().machine {
            Machine::Nds(nds) if JIT_AVAILABLE => JitCache::for_rom(&nds.rom_bytes()),
            Machine::Gba(gba) if JIT_AVAILABLE && GBA_USES_JIT => {
                JitCache::for_rom(gba.bus().rom())
            }
            _ => {
                self.save_jit_cache();
                self.jit_cache = None;
                return;
            }
        };
        let cache = match self.jit_cache.take() {
            Some(cache) if cache.file_name() == fresh.file_name() => cache,
            old => {
                if let Some(old) = old {
                    self.writes.send(Write::JitCache(old));
                }
                let cache = Arc::new(fresh);
                load(&cache);
                cache
            }
        };
        match &mut self.seat.core_mut().machine {
            Machine::Nds(nds) => nds.set_jit_cache(Some(Arc::clone(&cache))),
            Machine::Gba(gba) => gba.set_jit_cache(Some(Arc::clone(&cache))),
            _ => {}
        }
        self.jit_cache = Some(cache);
        self.jit_saved = Some(web_time::Instant::now());
    }

    pub fn save_jit_cache(&mut self) {
        if let Some(cache) = &self.jit_cache
            && cache.take_changed()
        {
            self.writes.send(Write::JitCache(Arc::clone(cache)));
        }
        self.jit_saved = Some(web_time::Instant::now());
    }

    pub fn jit_cache_due(&mut self) {
        if self
            .jit_saved
            .is_some_and(|saved| saved.elapsed() >= SAVE_EVERY)
        {
            self.save_jit_cache();
        }
    }

    pub fn clear_jit_cache(&mut self) -> bool {
        if let Some(cache) = &self.jit_cache {
            cache.clear();
        }
        let Some(folder) = folder() else {
            return true;
        };
        let mut cleared = true;
        for (path, _, _) in files(&folder) {
            if let Err(err) = std::fs::remove_file(&path) {
                log::warn!("couldn't remove {}: {err}", path.display());
                cleared = false;
            }
        }
        cleared
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn folder() -> Option<PathBuf> {
    let folder = crate::storage::cache_dir().join(FOLDER);
    (!cfg!(test)).then_some(folder)
}

#[cfg(target_arch = "wasm32")]
fn folder() -> Option<PathBuf> {
    None
}

fn load(cache: &JitCache) {
    let Some(path) = folder().map(|folder| folder.join(cache.file_name())) else {
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let began = web_time::Instant::now();
    match cache.merge(&bytes) {
        Ok(blocks) => log::info!(
            "loaded {blocks} JIT blocks ({} KB) in {:.1} ms",
            bytes.len() >> 10,
            began.elapsed().as_secs_f64() * 1000.0
        ),
        Err(err) => {
            log::warn!("dropped the JIT cache {}: {err}", path.display());
            if let Err(err) = std::fs::remove_file(&path) {
                log::warn!("couldn't remove {}: {err}", path.display());
            }
        }
    }
}

pub fn store(cache: &JitCache) {
    let Some(folder) = folder() else {
        return;
    };
    if let Err(err) = write(&folder, cache) {
        log::warn!("couldn't save the JIT cache: {err}");
    }
    evict(&folder, &cache.file_name(), FOLDER_LIMIT);
}

fn write(folder: &std::path::Path, cache: &JitCache) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(cache.file_name());
    let part = path.with_extension("part");
    std::fs::write(&part, cache.encode())?;
    std::fs::rename(&part, &path)
}

fn evict(folder: &std::path::Path, keep: &str, limit: u64) {
    let mut games = files(folder);
    games.sort_by_key(|&(_, _, modified)| std::cmp::Reverse(modified));
    let mut total = 0;
    for (path, size, _) in games {
        total += size;
        let kept = path.file_name().is_some_and(|name| name == keep);
        if total > limit && !kept {
            if let Err(err) = std::fs::remove_file(&path) {
                log::warn!("couldn't remove {}: {err}", path.display());
            }
            total -= size;
        }
    }
}

fn files(folder: &std::path::Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == EXTENSION)
        })
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            Some((entry.path(), meta.len(), modified))
        })
        .collect()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn game(folder: &std::path::Path, name: &str, size: usize, age: u64) {
        let path = folder.join(name);
        std::fs::write(&path, vec![0; size]).expect("the file writes");
        let modified = std::time::SystemTime::now() - Duration::from_secs(age);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_modified(modified))
            .expect("the time sets");
    }

    #[test]
    fn the_oldest_games_leave_first_when_the_folder_is_full() {
        let dir = tempfile::tempdir().expect("a temp folder");
        game(dir.path(), "a.jit", 100, 30);
        game(dir.path(), "b.jit", 100, 20);
        game(dir.path(), "c.jit", 100, 10);
        game(dir.path(), "d.jit", 100, 0);
        game(dir.path(), "notes.txt", 10, 40);
        evict(dir.path(), "d.jit", 300);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .expect("the folder reads")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["b.jit", "c.jit", "d.jit", "notes.txt"]);
    }
}
