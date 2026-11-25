use citrine_gb::rom::id::RomId;
use citrine_gb_threads::Spawner;
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb_threads::Threads;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const FILE: &str = "rom-ids.json";
#[cfg(target_arch = "wasm32")]
const NAMESPACE: crate::storage::kinds::Folder = crate::storage::kinds::Folder::RomIds;
const KEPT: usize = 64;
const SETTLED: Duration = Duration::from_secs(10);
#[cfg(not(target_arch = "wasm32"))]
const MAX_READ_BYTES: u64 = 1 << 30;
#[cfg(target_arch = "wasm32")]
const WARM_LEN: usize = 64 * 1024;

pub type Ids = (RomId, [u8; 20]);

#[cfg(not(target_arch = "wasm32"))]
pub type Hashing<'a> = Option<(&'a Threads, &'a dyn Fn(&[u8]) -> bool)>;

#[cfg(not(target_arch = "wasm32"))]
pub type Stamped = (Arc<[u8]>, Option<Stamp>, Option<Ids>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    len: u64,
    modified_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    key: PathBuf,
    stamp: Stamp,
}

impl Stamp {
    pub fn new(len: u64, modified: SystemTime) -> Option<Self> {
        let modified = modified.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            len,
            modified_ns: modified.as_nanos().try_into().ok()?,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Self::new(meta.len(), meta.modified().ok()?)
    }

    pub fn settled(&self) -> bool {
        self.settled_at(now())
    }

    fn settled_at(&self, now: Duration) -> bool {
        now.checked_sub(Duration::from_nanos(self.modified_ns))
            .is_some_and(|age| age >= SETTLED)
    }
}

impl Seen {
    pub fn at(key: impl Into<PathBuf>, stamp: Stamp) -> Self {
        Self {
            key: key.into(),
            stamp,
        }
    }

    pub fn inside(&self, entry: &str) -> Self {
        Self::at(self.key.join(entry), self.stamp)
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn file(name: &str, len: usize, modified: Option<SystemTime>) -> Option<Self> {
        Some(Self::at(name, Stamp::new(len as u64, modified?)?))
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn kept(copy: &Path, len: usize) -> Option<Self> {
        Some(Self::at(
            Path::new("kept").join(copy),
            Stamp::new(len as u64, UNIX_EPOCH)?,
        ))
    }
}

#[cfg(target_arch = "wasm32")]
pub fn warm_hashes() {
    RomId::with_game_sha1_on(&vec![0; WARM_LEN], None);
}

fn now() -> Duration {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_stamped(path: &Path) -> std::io::Result<(Arc<[u8]>, Option<Stamp>)> {
    let (data, stamp, _) = read_stamped_hashing(path, None)?;
    Ok((data, stamp))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_stamped_hashing(path: &Path, hashing: Hashing) -> std::io::Result<Stamped> {
    let before = Stamp::of(path);
    let (data, ids) = read_hashing(path, hashing)?;
    let stamp = before
        .filter(|stamp| stamp.len == data.len() as u64)
        .filter(|stamp| Stamp::of(path) == Some(*stamp));
    Ok((data, stamp, ids))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_shared(path: &Path) -> std::io::Result<Arc<[u8]>> {
    read_hashing(path, None).map(|(data, _)| data)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_hashing(path: &Path, hashing: Hashing) -> std::io::Result<(Arc<[u8]>, Option<Ids>)> {
    use std::io::Read;
    let (mut file, len) = open_regular(path)?;
    let len = usize::try_from(len).map_err(std::io::Error::other)?;
    let mut data = unsafe { Arc::<[u8]>::new_zeroed_slice(len).assume_init() };
    let whole = Arc::get_mut(&mut data).expect("a new slice has one owner");
    let ids = match hashing {
        Some((threads, wanted)) => {
            RomId::filled(whole, threads, |chunk| file.read_exact(chunk), wanted)?
        }
        None => {
            file.read_exact(whole)?;
            None
        }
    };
    if file.read(&mut [0])? == 0 {
        return Ok((data, ids));
    }
    let mut data = Vec::new();
    open_regular(path)?
        .0
        .take(MAX_READ_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_READ_BYTES {
        return Err(not_a_game_file());
    }
    Ok((Arc::from(data), None))
}

#[cfg(not(target_arch = "wasm32"))]
fn open_regular(path: &Path) -> std::io::Result<(std::fs::File, u64)> {
    let fits = |meta: &std::fs::Metadata| meta.is_file() && meta.len() <= MAX_READ_BYTES;
    if !fits(&std::fs::metadata(path)?) {
        return Err(not_a_game_file());
    }
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    if !fits(&meta) {
        return Err(not_a_game_file());
    }
    Ok((file, meta.len()))
}

#[cfg(not(target_arch = "wasm32"))]
fn not_a_game_file() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a game file")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: PathBuf,
    stamp: Stamp,
    id: RomId,
    game_sha1: [u8; 20],
}

#[derive(Default)]
pub struct RomIds {
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<PathBuf>,
    entries: Option<Vec<Entry>>,
}

impl RomIds {
    #[cfg(test)]
    pub fn at(file: PathBuf) -> Self {
        Self {
            file: Some(file),
            entries: None,
        }
    }

    pub fn known(&mut self, seen: &Seen) -> Option<Ids> {
        self.entries()
            .iter()
            .find(|entry| entry.path == seen.key && entry.stamp == seen.stamp)
            .map(|entry| (entry.id, entry.game_sha1))
    }

    pub fn learn(&mut self, seen: &Seen, ids: Ids, spawner: &Spawner) {
        if !seen.stamp.settled() {
            return;
        }
        let entry = Entry {
            path: seen.key.clone(),
            stamp: seen.stamp,
            id: ids.0,
            game_sha1: ids.1,
        };
        let entries = self.entries();
        if entries.first() == Some(&entry) {
            return;
        }
        entries.retain(|kept| kept.path != seen.key);
        entries.insert(0, entry);
        entries.truncate(KEPT);
        let Ok(text) = serde_json::to_vec(entries) else {
            return;
        };
        self.keep(text, spawner);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn keep(&mut self, text: Vec<u8>, spawner: &Spawner) {
        let file = self.file();
        spawner.run("rom ids", move || {
            if let Err(err) = write(&file, &text) {
                log::warn!("couldn't keep the ROM ids in {}: {err}", file.display());
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn keep(&mut self, text: Vec<u8>, _spawner: &Spawner) {
        if let Err(err) = crate::storage::Folders::default().write(NAMESPACE, FILE, &text) {
            log::warn!("couldn't keep the ROM ids: {err}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn file(&mut self) -> PathBuf {
        self.file
            .get_or_insert_with(|| {
                crate::storage::folder(crate::storage::kinds::Folder::RomIds).join(FILE)
            })
            .clone()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn stored(&mut self) -> Option<Vec<u8>> {
        std::fs::read(self.file()).ok()
    }

    #[cfg(target_arch = "wasm32")]
    fn stored(&mut self) -> Option<Vec<u8>> {
        crate::storage::Folders::default().read(NAMESPACE, FILE)
    }

    fn entries(&mut self) -> &mut Vec<Entry> {
        if self.entries.is_none() || cfg!(target_arch = "wasm32") {
            let stored = self
                .stored()
                .and_then(|text| serde_json::from_slice(&text).ok());
            self.entries = Some(stored.unwrap_or_default());
        }
        self.entries.get_or_insert_with(Vec::new)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write(file: &Path, text: &[u8]) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let part = file.with_extension(format!("{}.part", std::process::id()));
    std::fs::write(&part, text)?;
    std::fs::rename(&part, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_threads::{Mode, Threads};
    use std::fs::File;

    fn folder(name: &str) -> PathBuf {
        let folder = crate::storage::scratch_dir()
            .join(format!("citrine-rom-ids-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn put(path: &Path, data: &[u8], age: Duration) {
        std::fs::write(path, data).unwrap();
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    fn opened(cache: &mut RomIds, path: &Path, threads: &Threads) -> (Ids, bool) {
        let (data, stamp) = read_stamped(path).unwrap();
        let seen = Seen::at(path, stamp.expect("a still file has a stamp"));
        if let Some(ids) = cache.known(&seen) {
            return (ids, true);
        }
        let ids = RomId::with_game_sha1(&data);
        cache.learn(&seen, ids, &threads.spawner());
        threads.run_side_jobs();
        (ids, false)
    }

    #[test]
    fn a_reopened_file_reuses_its_ids_and_a_changed_one_never_does() {
        let folder = folder("changed");
        let threads = Threads::new(Mode::Inline);
        let store = folder.join("cache").join(FILE);
        let rom = folder.join("game.nds");
        let old = Duration::from_secs(3600);
        put(&rom, b"first", old);
        let mut cache = RomIds::at(store.clone());
        let first = RomId::with_game_sha1(b"first");
        assert_eq!(opened(&mut cache, &rom, &threads), (first, false));
        assert_eq!(opened(&mut cache, &rom, &threads), (first, true));
        assert_eq!(
            opened(&mut RomIds::at(store.clone()), &rom, &threads),
            (first, true),
            "a new run reads the stored ids"
        );

        put(&rom, b"other", old - Duration::from_secs(1));
        let other = RomId::with_game_sha1(b"other");
        assert_eq!(
            opened(&mut cache, &rom, &threads),
            (other, false),
            "same size, new time"
        );

        let mut stale = RomIds::at(store.clone());
        put(&rom, b"longer", old - Duration::from_secs(1));
        let longer = RomId::with_game_sha1(b"longer");
        assert_eq!(
            opened(&mut stale, &rom, &threads),
            (longer, false),
            "same time, new size"
        );

        put(&rom, b"fresh", Duration::ZERO);
        let fresh = RomId::with_game_sha1(b"fresh");
        assert_eq!(opened(&mut cache, &rom, &threads), (fresh, false));
        put(&rom, b"fresk", Duration::ZERO);
        assert_eq!(
            opened(&mut cache, &rom, &threads),
            (RomId::with_game_sha1(b"fresk"), false),
            "a file written moments ago is never kept"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_browser_file_is_known_by_name_size_and_time() {
        let folder = folder("browser");
        let threads = Threads::new(Mode::Inline);
        let mut cache = RomIds::at(folder.join(FILE));
        let at = UNIX_EPOCH + Duration::from_millis(1_762_650_000_123);
        let ids = RomId::with_game_sha1(b"game");
        let seen = Seen::file("game.nds", 4, Some(at)).unwrap();
        assert_eq!(cache.known(&seen), None);
        cache.learn(&seen, ids, &threads.spawner());
        threads.run_side_jobs();
        let reopened = Seen::file("game.nds", 4, Some(at)).unwrap();
        assert_eq!(RomIds::at(folder.join(FILE)).known(&reopened), Some(ids));
        let later = at + Duration::from_millis(1);
        for changed in [
            Seen::file("game.nds", 4, Some(later)),
            Seen::file("game.nds", 5, Some(at)),
            Seen::file("other.nds", 4, Some(at)),
            Seen::kept(Path::new("game.nds"), 4),
        ] {
            assert_eq!(cache.known(&changed.unwrap()), None);
        }
        assert_eq!(Seen::file("game.nds", 4, None), None);

        let zip = Seen::file("pack.zip", 4, Some(at)).unwrap();
        let inside = zip.inside("roms/game.nds");
        assert_eq!(
            inside,
            Seen::at(Path::new("pack.zip/roms/game.nds"), zip.stamp)
        );
        assert_ne!(inside, zip.inside("roms/other.nds"));
        assert_ne!(
            inside,
            Seen::file("pack.zip", 5, Some(at))
                .unwrap()
                .inside("roms/game.nds")
        );

        let kept = Seen::kept(Path::new("ab12.nds"), 4).unwrap();
        cache.learn(&kept, ids, &threads.spawner());
        assert_eq!(cache.known(&kept), Some(ids));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_store_keeps_the_newest_files_and_drops_a_broken_one() {
        let folder = folder("kept");
        let threads = Threads::new(Mode::Inline);
        let store = folder.join(FILE);
        let mut cache = RomIds::at(store.clone());
        let old = Duration::from_secs(3600);
        for at in 0..KEPT + 1 {
            let rom = folder.join(format!("{at}.gba"));
            put(&rom, at.to_string().as_bytes(), old);
            opened(&mut cache, &rom, &threads);
        }
        let mut again = RomIds::at(store.clone());
        assert_eq!(again.entries().len(), KEPT);
        assert!(opened(&mut again, &folder.join(format!("{KEPT}.gba")), &threads).1);
        assert!(!opened(&mut again, &folder.join("0.gba"), &threads).1);

        std::fs::write(&store, b"not json").unwrap();
        assert!(RomIds::at(store).entries().is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_read_file_keeps_its_exact_bytes() {
        let folder = folder("bytes");
        for data in [&b""[..], b"citrine", &[7; 70_000]] {
            let rom = folder.join("game.gb");
            std::fs::write(&rom, data).unwrap();
            assert_eq!(&read_stamped(&rom).unwrap().0[..], data);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_read_while_it_changes_has_no_stamp() {
        let folder = folder("moving");
        let rom = folder.join("game.gb");
        put(&rom, b"data", Duration::from_secs(3600));
        assert!(read_stamped(&rom).unwrap().1.is_some());
        assert!(read_stamped(&folder.join("missing.gb")).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn only_a_regular_file_within_the_cap_is_read() {
        let folder = folder("regular");
        let big = folder.join("big.nds");
        File::create(&big)
            .unwrap()
            .set_len(MAX_READ_BYTES + 1)
            .unwrap();
        assert!(read_stamped(&big).is_err());
        assert!(read_stamped(&folder).is_err());
        #[cfg(unix)]
        assert!(read_stamped(Path::new("/dev/null")).is_err());
        let fine = folder.join("fine.gb");
        std::fs::write(&fine, b"rom").unwrap();
        assert_eq!(&read_stamped(&fine).unwrap().0[..], b"rom");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
