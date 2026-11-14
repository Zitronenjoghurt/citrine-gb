use crate::homebrew::HOMEBREW_GAMES;
use citrine_gb_net::GameHash;
use citrine_gb_threads::{Claim, Pending, Threads};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

const KEPT_LOOKS: usize = 8;

pub trait Shelf: Send + Sync {
    fn find(&self, hash: &GameHash) -> Option<Arc<[u8]>>;

    fn holds_ds_game(&self, _code: [u8; 4]) -> bool {
        false
    }
}

pub struct Homebrew;

impl Shelf for Homebrew {
    fn find(&self, hash: &GameHash) -> Option<Arc<[u8]>> {
        HOMEBREW_GAMES
            .iter()
            .find(|game| game.sha1 == *hash)
            .and_then(|game| game.embedded())
            .map(Arc::from)
    }
}

pub const MAX_OWN_GAME_BYTES: u64 = 512 << 20;

type Indexed = HashMap<PathBuf, (u64, SystemTime, GameHash)>;

pub struct ShelfIndex(#[cfg_attr(target_arch = "wasm32", allow(dead_code))] Claim<Indexed>);

impl Default for ShelfIndex {
    fn default() -> Self {
        Self(Claim::new(HashMap::new()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ShelfIndex {
    fn known(&self, path: &std::path::Path, size: u64, changed: SystemTime) -> Option<GameHash> {
        self.0
            .try_with(|map| {
                map.get(path)
                    .filter(|(s, c, _)| *s == size && *c == changed)
                    .map(|(_, _, hash)| *hash)
            })
            .flatten()
    }

    fn keep(&self, path: &std::path::Path, size: u64, changed: SystemTime, hash: GameHash) {
        self.0
            .try_with(|map| map.insert(path.to_path_buf(), (size, changed, hash)));
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Files {
    pub paths: Vec<PathBuf>,
    pub index: Arc<ShelfIndex>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Files {
    fn hash_of(&self, path: &std::path::Path) -> Option<GameHash> {
        let meta = std::fs::metadata(path).ok()?;
        let (size, changed) = (meta.len(), meta.modified().ok()?);
        if size == 0 || size > MAX_OWN_GAME_BYTES {
            return None;
        }
        if let Some(hash) = self.index.known(path, size, changed) {
            return Some(hash);
        }
        let file = std::fs::File::open(path).ok()?;
        let hash = citrine_gb::rom::id::game_sha1_from(file, size).ok()?;
        self.index.keep(path, size, changed, hash);
        Some(hash)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Shelf for Files {
    fn find(&self, hash: &GameHash) -> Option<Arc<[u8]>> {
        Homebrew.find(hash).or_else(|| {
            self.paths.iter().find_map(|path| {
                (self.hash_of(path)? == *hash)
                    .then(|| std::fs::read(path).ok().map(Arc::from))
                    .flatten()
            })
        })
    }

    fn holds_ds_game(&self, code: [u8; 4]) -> bool {
        self.paths.iter().any(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nds"))
                && ds_code(path).is_some_and(|found| found[..3] == code[..3])
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ds_code(path: &std::path::Path) -> Option<[u8; 4]> {
    use std::io::Read;
    let mut head = [0; 0x10];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    head[0x0C..].try_into().ok()
}

#[derive(Debug, Clone)]
pub enum Look {
    Found(Arc<[u8]>),
    Missing,
    Looking,
}

type Found = Option<Arc<[u8]>>;

pub(crate) struct Finder {
    shelf: Arc<dyn Shelf>,
    wanted: Vec<GameHash>,
    looking: Vec<(GameHash, Pending<Found>)>,
    seen: VecDeque<(GameHash, Found)>,
    unchecked: Vec<(GameHash, Vec<u8>)>,
    checking: Vec<(GameHash, Pending<Found>)>,
    checked: VecDeque<(GameHash, Found)>,
}

impl Finder {
    pub(crate) fn new(shelf: Box<dyn Shelf>) -> Self {
        Self {
            shelf: shelf.into(),
            wanted: Vec::new(),
            looking: Vec::new(),
            seen: VecDeque::new(),
            unchecked: Vec::new(),
            checking: Vec::new(),
            checked: VecDeque::new(),
        }
    }

    pub(crate) fn holds_ds_game(&self, code: [u8; 4]) -> bool {
        self.shelf.holds_ds_game(code)
    }

    pub(crate) fn known(&self, hash: &GameHash) -> Look {
        match self.seen.iter().find(|(h, _)| h == hash) {
            Some((_, found)) => found.clone().map_or(Look::Missing, Look::Found),
            None => Look::Looking,
        }
    }

    pub(crate) fn look(&mut self, hash: &GameHash) -> Look {
        let known = self.known(hash);
        let asked = self.wanted.contains(hash) || self.looking.iter().any(|(h, _)| h == hash);
        if matches!(known, Look::Looking) && !asked {
            self.wanted.push(*hash);
        }
        known
    }

    pub(crate) fn check(&mut self, hash: GameHash, data: Vec<u8>) {
        self.unchecked.push((hash, data));
    }

    pub(crate) fn checked(&mut self, hash: &GameHash) -> Option<Found> {
        let index = self.checked.iter().position(|(h, _)| h == hash)?;
        self.checked.remove(index).map(|(_, found)| found)
    }

    pub(crate) fn run(&mut self, threads: &Threads) {
        for hash in self.wanted.drain(..) {
            let shelf = self.shelf.clone();
            let pending = threads.background("shelf lookup", move || {
                shelf
                    .find(&hash)
                    .filter(|rom| super::sha1_of(rom) == Some(hash))
            });
            self.looking.push((hash, pending));
        }
        for (hash, data) in self.unchecked.drain(..) {
            let pending = threads.background("game check", move || {
                (super::sha1_of(&data) == Some(hash)).then(|| Arc::from(data))
            });
            self.checking.push((hash, pending));
        }
        collect(&mut self.looking, &mut self.seen);
        collect(&mut self.checking, &mut self.checked);
    }
}

fn collect(running: &mut Vec<(GameHash, Pending<Found>)>, done: &mut VecDeque<(GameHash, Found)>) {
    let mut i = 0;
    while i < running.len() {
        match running[i].1.try_take() {
            Some(found) => {
                let (hash, _) = running.swap_remove(i);
                done.push_back((hash, found.ok().flatten()));
                while done.len() > KEPT_LOOKS {
                    done.pop_front();
                }
            }
            None => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_threads::Mode;

    struct Counted(std::sync::mpsc::Sender<()>);

    impl Shelf for Counted {
        fn find(&self, _hash: &GameHash) -> Option<Arc<[u8]>> {
            let _ = self.0.send(());
            None
        }
    }

    fn settled(finder: &mut Finder, threads: &Threads, hash: &GameHash) -> Found {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            finder.run(threads);
            if let Some(found) = finder.checked(hash) {
                return found;
            }
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn knowing_never_looks_and_looking_asks_once() {
        let (sent, looks) = std::sync::mpsc::channel();
        let mut finder = Finder::new(Box::new(Counted(sent)));
        let threads = Threads::new(Mode::Inline);
        assert!(matches!(finder.known(&[1; 20]), Look::Looking));
        finder.run(&threads);
        assert_eq!(looks.try_iter().count(), 0);
        for _ in 0..3 {
            finder.look(&[1; 20]);
        }
        while matches!(finder.known(&[1; 20]), Look::Looking) {
            finder.run(&threads);
        }
        assert!(matches!(finder.look(&[1; 20]), Look::Missing));
        finder.run(&threads);
        assert_eq!(looks.try_iter().count(), 1);
    }

    #[test]
    fn a_checked_game_comes_back_once_and_a_wrong_one_comes_back_empty() {
        let threads = Threads::new(Mode::Threads);
        let mut finder = Finder::new(Box::new(Homebrew));
        let data = vec![7; 1000];
        let hash = super::super::sha1_of(&data).unwrap();
        finder.check(hash, data.clone());
        assert!(finder.checked(&hash).is_none());
        assert_eq!(
            settled(&mut finder, &threads, &hash).as_deref(),
            Some(&data[..])
        );
        assert!(finder.checked(&hash).is_none());
        finder.check([3; 20], data);
        assert!(settled(&mut finder, &threads, &[3; 20]).is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_trimmed_ds_copy_matches_the_full_one_and_its_hash_is_kept() {
        let full = include_bytes!("../../fixtures/nds/blocksds-console-both-screens.nds");
        let used = 89_536;
        let dir = tempfile::tempdir().unwrap();
        let trimmed = dir.path().join("trimmed.nds");
        std::fs::write(&trimmed, &full[..used]).unwrap();
        let hash = super::super::sha1_of(full).unwrap();
        let files = |index: &Arc<ShelfIndex>| Files {
            paths: vec![dir.path().join("gone.nds"), trimmed.clone()],
            index: index.clone(),
        };
        let index = Arc::new(ShelfIndex::default());
        assert_eq!(files(&index).find(&hash).as_deref(), Some(&full[..used]));
        let changed = std::fs::metadata(&trimmed).unwrap().modified().unwrap();
        std::fs::write(&trimmed, vec![0; used]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&trimmed)
            .unwrap()
            .set_modified(changed)
            .unwrap();
        assert_eq!(
            files(&index).find(&hash).as_deref(),
            Some(&vec![0; used][..])
        );
        assert!(files(&Arc::default()).find(&hash).is_none());
    }

    #[test]
    #[ignore = "frame times, run in release"]
    fn checking_a_32_mb_game_keeps_frames_short() {
        let threads = Threads::new(Mode::Threads);
        let mut finder = Finder::new(Box::new(Homebrew));
        let data: Vec<u8> = (0..32u32 << 20).map(|i| ((i * 31) >> 7) as u8).collect();
        let started = std::time::Instant::now();
        let hash = super::super::sha1_of(&data).unwrap();
        let inline = started.elapsed().as_secs_f64() * 1000.0;
        finder.check(hash, data);
        let mut frames = Vec::new();
        let found = loop {
            let started = std::time::Instant::now();
            finder.run(&threads);
            let found = finder.checked(&hash);
            frames.push(started.elapsed().as_secs_f64() * 1000.0);
            if let Some(found) = found {
                break found;
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        };
        assert!(found.is_some_and(|rom| rom.len() == 32 << 20));
        let longest = frames.iter().copied().fold(0.0, f64::max);
        println!(
            "hash on the ui thread {inline:.2} ms, frames {} longest {longest:.3} ms",
            frames.len()
        );
        assert!(longest < 16.7, "longest frame {longest:.2} ms");
    }
}
