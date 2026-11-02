use super::record::{Book, Record};
use super::recorder::Recorder;
use crate::online::Online;
use crate::online::cloud::SyncState;
use crate::storage::{Backend, compress, decompress};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use web_time::Instant;

const NAMESPACE: &str = "stats";
const FILE: &str = "stats.json.br";
const SLOT: &str = "stats";
const KEEP_EVERY: Duration = Duration::from_secs(30);
const PUSH_EVERY: Duration = Duration::from_secs(120);
const MAX_IMPORT_BYTES: usize = 32 << 20;

impl Online {
    pub fn stats_problem(&self) -> Option<SyncState> {
        self.account_problem(SLOT)
    }
}

#[derive(Clone)]
pub struct StatsStore {
    backend: Backend,
}

impl Default for StatsStore {
    fn default() -> Self {
        Self {
            backend: Backend::settings(),
        }
    }
}

impl StatsStore {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn in_dir(root: std::path::PathBuf) -> Self {
        Self {
            backend: Backend::at(root),
        }
    }

    pub fn load(&self) -> Book {
        let book = self
            .backend
            .read(NAMESPACE, FILE)
            .and_then(|raw| decompress(&raw))
            .and_then(|json| match serde_json::from_slice::<Book>(&json) {
                Ok(book) => Some(book),
                Err(err) => {
                    log::error!("Ignoring unreadable stats: {err}");
                    None
                }
            });
        match book {
            Some(book) if !book.device.is_empty() => book,
            Some(mut book) => {
                book.device = Book::new().device;
                book
            }
            None => Book::new(),
        }
    }

    pub fn store(&self, book: &Book) -> std::io::Result<()> {
        let json = serde_json::to_vec(book).map_err(std::io::Error::other)?;
        self.backend.write(NAMESPACE, FILE, &compress(&json))
    }

    pub fn delete(&self) -> std::io::Result<()> {
        self.backend.delete(NAMESPACE, FILE)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("This file is too large for a stats file")]
    TooLarge,
    #[error("This file isn't a Citrine stats file")]
    NotStats,
}

#[derive(Serialize, Deserialize)]
struct DeviceCopy {
    device: String,
    record: Record,
}

pub struct Stats {
    recorder: Recorder,
    store: Option<StatsStore>,
    kept_at: Option<Instant>,
    pushed_at: Option<Instant>,
    unpushed: bool,
    syncing: bool,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            recorder: Recorder::new(Book::new()),
            store: None,
            kept_at: None,
            pushed_at: None,
            unpushed: true,
            syncing: false,
        }
    }
}

impl Stats {
    pub fn open(store: StatsStore) -> Self {
        let book = store.load();
        Self {
            recorder: Recorder::new(book),
            store: Some(store),
            ..Default::default()
        }
    }

    pub fn recorder(&mut self) -> &mut Recorder {
        &mut self.recorder
    }

    pub fn book(&self) -> &Book {
        self.recorder.book()
    }

    pub fn started_at(&self, rom: &str) -> Option<u64> {
        self.recorder.started_at(rom)
    }

    pub fn session_ms(&self, rom: &str) -> Option<u64> {
        self.recorder.session_ms(rom)
    }

    fn keep(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        if let Err(err) = store.store(self.recorder.book()) {
            log::warn!("could not keep the stats: {err}");
        }
    }

    pub fn tick(&mut self, now: Instant) {
        if self.recorder.take_dirty() {
            self.unpushed = true;
            if self.kept_at.is_none() {
                self.kept_at = Some(now);
            }
        }
        if self
            .kept_at
            .is_some_and(|at| now.duration_since(at) >= KEEP_EVERY)
        {
            self.kept_at = None;
            self.keep();
        }
    }

    pub fn flush(&mut self) {
        if self.recorder.take_dirty() {
            self.unpushed = true;
            self.kept_at = Some(Instant::now());
        }
        if self.kept_at.take().is_some() {
            self.keep();
        }
        self.pushed_at = None;
    }

    pub fn sync(&mut self, online: &mut Online, enabled: bool, now: Instant) {
        if !enabled {
            if self.syncing {
                online.account_stop(SLOT);
                self.syncing = false;
            }
            return;
        }
        if !self.syncing {
            self.syncing = true;
            self.unpushed = true;
            self.pushed_at = None;
        }
        for copy in online.account_take(SLOT) {
            match serde_json::from_slice::<DeviceCopy>(&copy.data) {
                Ok(theirs) => self.recorder.merge_device(&theirs.device, &theirs.record),
                Err(err) => log::warn!("ignoring stats from {}: {err}", copy.device),
            }
        }
        let due = self
            .pushed_at
            .is_none_or(|at| now.duration_since(at) >= PUSH_EVERY);
        if self.unpushed && due {
            self.unpushed = false;
            self.pushed_at = Some(now);
            let book = self.recorder.book();
            let copy = DeviceCopy {
                device: book.device.clone(),
                record: book.own_record().cloned().unwrap_or_default(),
            };
            match serde_json::to_vec(&copy) {
                Ok(data) => online.account_put(SLOT, data),
                Err(err) => log::warn!("could not encode the stats: {err}"),
            }
        }
    }

    pub fn export(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self.recorder.book()).unwrap_or_default()
    }

    pub fn import(&mut self, bytes: &[u8]) -> Result<(), ImportError> {
        if bytes.len() > MAX_IMPORT_BYTES {
            return Err(ImportError::TooLarge);
        }
        let book: Book = serde_json::from_slice(bytes).map_err(|_| ImportError::NotStats)?;
        if book.device.is_empty() && book.devices.is_empty() {
            return Err(ImportError::NotStats);
        }
        self.recorder.merge(&book);
        self.flush();
        Ok(())
    }

    pub fn delete_all(&mut self) -> std::io::Result<()> {
        self.recorder.clear();
        self.recorder.take_dirty();
        self.kept_at = None;
        self.unpushed = true;
        match &self.store {
            Some(store) => store.delete(),
            None => Ok(()),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::stats::Activity;

    const ROM: &str = "aa";
    const DAY: &str = "2026-10-31";

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("citrine-stats-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn played(stats: &mut Stats, rom: &str, secs: u64, now: u64) {
        stats.recorder().play(
            rom,
            "Pocket Quest",
            Duration::from_secs(secs),
            Activity::default(),
            now,
            DAY,
        );
    }

    #[test]
    fn stats_survive_a_restart() {
        let root = dir("restart");
        let mut stats = Stats::open(StatsStore::in_dir(root.clone()));
        played(&mut stats, ROM, 42, 100);
        stats.flush();
        let again = Stats::open(StatsStore::in_dir(root.clone()));
        assert_eq!(again.book(), stats.book());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn export_then_import_adds_nothing_twice() {
        let root = dir("export");
        let mut here = Stats::open(StatsStore::in_dir(root.join("here")));
        played(&mut here, ROM, 60, 100);
        let file = here.export();
        let mut there = Stats::open(StatsStore::in_dir(root.join("there")));
        played(&mut there, ROM, 30, 200);
        there.import(&file).unwrap();
        there.import(&file).unwrap();
        let overview = there.book().overview("2026-10-31".parse().unwrap());
        assert_eq!(overview.play_ms, 90_000);
        assert_eq!(overview.games.len(), 1);
        here.import(&file).unwrap();
        let overview = here.book().overview("2026-10-31".parse().unwrap());
        assert_eq!(overview.play_ms, 60_000);
        assert!(matches!(
            there.import(b"{\"x\":1}"),
            Err(ImportError::NotStats)
        ));
        assert!(matches!(there.import(b"nope"), Err(ImportError::NotStats)));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delete_all_leaves_nothing_behind() {
        let root = dir("delete");
        let mut stats = Stats::open(StatsStore::in_dir(root.clone()));
        let device = stats.book().device.clone();
        played(&mut stats, ROM, 60, 100);
        stats.flush();
        stats.delete_all().unwrap();
        assert!(stats.book().is_empty());
        assert_ne!(stats.book().device, device);
        let again = Stats::open(StatsStore::in_dir(root.clone()));
        assert!(again.book().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn two_devices_add_up_and_resyncing_changes_nothing() {
        let mut a = Stats::open(StatsStore::in_dir(dir("a")));
        let mut b = Stats::open(StatsStore::in_dir(dir("b")));
        played(&mut a, ROM, 60, 100);
        played(&mut b, ROM, 45, 300);
        b.recorder().snapshot(ROM, DAY);
        let copy = |stats: &Stats| DeviceCopy {
            device: stats.book().device.clone(),
            record: stats.book().own_record().cloned().unwrap(),
        };
        let from_a = copy(&a);
        let from_b = copy(&b);
        for _ in 0..2 {
            a.recorder().merge_device(&from_b.device, &from_b.record);
            b.recorder().merge_device(&from_a.device, &from_a.record);
        }
        let today = "2026-10-31".parse().unwrap();
        for stats in [&a, &b] {
            let overview = stats.book().overview(today);
            assert_eq!(overview.play_ms, 105_000);
            assert_eq!(overview.today_ms, 105_000);
            assert_eq!(overview.sessions, 2);
            assert_eq!(overview.snapshots, 1);
            let game = overview.game(ROM).unwrap();
            assert_eq!(game.longest_ms, 60_000);
            assert_eq!((game.first_played, game.last_played), (100, 300));
        }
        played(&mut a, ROM, 15, 400);
        let from_a = copy(&a);
        b.recorder().merge_device(&from_a.device, &from_a.record);
        assert_eq!(b.book().overview(today).play_ms, 120_000);
        let _ = std::fs::remove_dir_all(dir("a"));
        let _ = std::fs::remove_dir_all(dir("b"));
    }
}
