use super::Identity;
use crate::covers::Game;
#[cfg(not(target_arch = "wasm32"))]
use citrine_gb::rom::id::RomId;
use citrine_gb_threads::Spawner;
use std::collections::{HashSet, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::PoisonError;
use std::sync::mpsc::{Receiver, Sender, channel};
use web_time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
const IDENTIFY_MAX: usize = 64 << 20;
#[cfg(not(target_arch = "wasm32"))]
const DS_IDENTIFY_MAX: u64 = 512 << 20;
#[cfg(not(target_arch = "wasm32"))]
const HEAD_LEN: usize = 0x200;
#[cfg(not(target_arch = "wasm32"))]
#[expect(
    clippy::disallowed_types,
    reason = "identify workers read big DS files one at a time and share one hashing pool behind one global lock"
)]
static BIG_READS: std::sync::Mutex<Option<citrine_gb_threads::Threads>> =
    std::sync::Mutex::new(None);
const MOST_WORKERS: usize = 4;
#[cfg(not(target_arch = "wasm32"))]
const MOST_PER_JOB: usize = 16;
const BATCH_WAIT: Duration = Duration::from_millis(250);

pub(super) type Identified = (String, Option<(Identity, Game)>);

enum Answer {
    Found(Box<Identified>),
    #[cfg(not(target_arch = "wasm32"))]
    Free,
}

pub(super) struct Identifier {
    queue: VecDeque<String>,
    jobs: usize,
    peak: usize,
    answers: (Sender<Answer>, Receiver<Answer>),
    asked: HashSet<String>,
    pending: Vec<Identified>,
    batch_since: Option<Instant>,
    pub(super) workers: usize,
}

impl Default for Identifier {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            jobs: 0,
            peak: 0,
            answers: channel(),
            asked: HashSet::new(),
            pending: Vec::new(),
            batch_since: None,
            workers: std::thread::available_parallelism()
                .map_or(1, std::num::NonZero::get)
                .saturating_sub(1)
                .clamp(1, MOST_WORKERS),
        }
    }
}

impl Identifier {
    pub(super) fn busy(&self) -> bool {
        !self.asked.is_empty()
    }

    pub(super) fn want<'a>(&mut self, wanted: impl Iterator<Item = &'a str>) {
        let wanted: Vec<&str> = wanted.collect();
        let keep: HashSet<&str> = wanted.iter().copied().collect();
        let asked = &mut self.asked;
        self.queue.retain(|path| {
            let kept = keep.contains(path.as_str());
            if !kept {
                asked.remove(path);
            }
            kept
        });
        for path in wanted {
            if self.asked.insert(path.to_owned()) {
                if cfg!(target_arch = "wasm32") {
                    let _ = self
                        .answers
                        .0
                        .send(Answer::Found(Box::new((path.to_owned(), None))));
                } else {
                    self.queue.push_back(path.to_owned());
                }
            }
        }
    }

    pub(super) fn top_up(&mut self, spawner: &Spawner) {
        #[cfg(not(target_arch = "wasm32"))]
        while self.jobs < self.workers && !self.queue.is_empty() {
            let free = self.workers - self.jobs;
            let take = self.queue.len().div_ceil(free).clamp(1, MOST_PER_JOB);
            let paths: Vec<String> = self.queue.drain(..take).collect();
            let answers = self.answers.0.clone();
            self.jobs += 1;
            spawner.run("library identify", move || work(paths, answers));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (spawner, self.workers);
        self.peak = self.peak.max(self.jobs);
    }

    pub(super) fn take_batch(&mut self) -> Vec<Identified> {
        let before = self.pending.len();
        for answer in self.answers.1.try_iter() {
            match answer {
                Answer::Found(found) => self.pending.push(*found),
                #[cfg(not(target_arch = "wasm32"))]
                Answer::Free => self.jobs -= 1,
            }
        }
        if self.pending.len() > before && self.batch_since.is_none() {
            self.batch_since = Some(Instant::now());
        }
        let all_in = self.pending.len() >= self.asked.len();
        let waited = self
            .batch_since
            .is_some_and(|since| since.elapsed() >= BATCH_WAIT);
        if self.pending.is_empty() || !(all_in || waited) {
            return Vec::new();
        }
        self.batch_since = None;
        let batch = std::mem::take(&mut self.pending);
        for (path, _) in &batch {
            self.asked.remove(path);
        }
        batch
    }

    #[cfg(test)]
    pub(super) fn peak(&self) -> usize {
        self.peak
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct Shift {
    paths: std::vec::IntoIter<String>,
    current: Option<String>,
    answers: Sender<Answer>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Shift {
    fn drop(&mut self) {
        for path in self.current.take().into_iter().chain(self.paths.by_ref()) {
            let _ = self.answers.send(Answer::Found(Box::new((path, None))));
        }
        let _ = self.answers.send(Answer::Free);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn work(paths: Vec<String>, answers: Sender<Answer>) {
    let mut shift = Shift {
        paths: paths.into_iter(),
        current: None,
        answers,
    };
    for path in shift.paths.by_ref() {
        let found = read(shift.current.insert(path));
        let Some(path) = shift.current.take() else {
            break;
        };
        if shift
            .answers
            .send(Answer::Found(Box::new((path, found))))
            .is_err()
        {
            break;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read(path: &str) -> Option<(Identity, Game)> {
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        let (files, rx) = channel();
        crate::platform::android::files::read_rom(path, files);
        let file = rx.recv().ok()?;
        return identifiable(&file.data, file.data.len() as u64, Some(&file.name))
            .then(|| identity_of(&file.data, Some(&file.name)));
    }
    let file_path = std::path::Path::new(path);
    let file_name = file_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let Ok(meta) = std::fs::metadata(file_path) else {
        return read_zipped(file_path, file_name.as_deref());
    };
    let len = meta.len();
    let big = len > IDENTIFY_MAX as u64;
    if big {
        return read_big(file_path, len, file_name.as_deref());
    }
    let data = crate::rom_ids::read_shared(file_path)
        .ok()
        .filter(|data| identifiable(data, data.len() as u64, file_name.as_deref()))?;
    Some(identity_of(&data, file_name.as_deref()))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_zipped(path: &std::path::Path, file_name: Option<&str>) -> Option<(Identity, Game)> {
    use std::io::Read;
    crate::utils::zip_games::with_entry(path, |len, entry| {
        let mut head = Vec::with_capacity(HEAD_LEN);
        entry.take(HEAD_LEN as u64).read_to_end(&mut head)?;
        if !identifiable(&head, len, file_name) {
            return Ok(None);
        }
        let len = usize::try_from(len).map_err(std::io::Error::other)?;
        let mut rest = std::io::Cursor::new(head).chain(entry);
        let found = if len > IDENTIFY_MAX {
            let mut turn = BIG_READS.lock().unwrap_or_else(PoisonError::into_inner);
            let threads = turn.get_or_insert_with(|| {
                citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads)
            });
            let wanted = |head: &[u8]| identifiable(head, len as u64, file_name);
            let mut data = vec![0; len];
            RomId::filled(&mut data, threads, |chunk| rest.read_exact(chunk), wanted)?
                .map(|(id, _)| identity_with(&data, id, file_name))
        } else {
            let mut data = vec![0; len];
            rest.read_exact(&mut data)?;
            Some(identity_of(&data, file_name))
        };
        if rest.read(&mut [0])? != 0 {
            return Ok(None);
        }
        Ok(found)
    })
    .ok()
    .flatten()
}

#[cfg(not(target_arch = "wasm32"))]
fn read_big(path: &std::path::Path, len: u64, file_name: Option<&str>) -> Option<(Identity, Game)> {
    if !identifiable(&head(path)?, len, file_name) {
        return None;
    }
    let mut turn = BIG_READS.lock().unwrap_or_else(PoisonError::into_inner);
    let threads = turn
        .get_or_insert_with(|| citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads));
    let wanted = |head: &[u8]| identifiable(head, len, file_name);
    let (data, _, ids) =
        crate::rom_ids::read_stamped_hashing(path, Some((threads, &wanted))).ok()?;
    let (id, _) = ids?;
    Some(identity_with(&data, id, file_name))
}

#[cfg(not(target_arch = "wasm32"))]
fn identifiable(head: &[u8], len: u64, file_name: Option<&str>) -> bool {
    len <= IDENTIFY_MAX as u64
        || (len <= DS_IDENTIFY_MAX
            && citrine_gb::console::detect(head, file_name)
                == Some(citrine_gb::console::ConsoleKind::Nds))
}

#[cfg(not(target_arch = "wasm32"))]
fn head(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut head = Vec::with_capacity(HEAD_LEN);
    std::fs::File::open(path)
        .ok()?
        .take(HEAD_LEN as u64)
        .read_to_end(&mut head)
        .ok()?;
    Some(head)
}

#[cfg(not(target_arch = "wasm32"))]
fn identity_of(data: &[u8], file_name: Option<&str>) -> (Identity, Game) {
    identity_with(data, RomId::new(data), file_name)
}

#[cfg(not(target_arch = "wasm32"))]
fn identity_with(data: &[u8], id: RomId, file_name: Option<&str>) -> (Identity, Game) {
    let console = citrine_gb::console::detect(data, file_name);
    let rom_key = id.sha256_hex_string();
    let name = title(data, &id.sha1, file_name, console);
    let game = Game::of(
        &rom_key,
        console.unwrap_or(citrine_gb::console::ConsoleKind::Gb),
        id.sha1,
        data,
    );
    let game = match file_name {
        Some(file_name) => game.named(file_name),
        None => game,
    };
    let identity = Identity {
        rom_key,
        sha1: id.sha1_hex_string(),
        crc32: Some(id.crc32),
        ra_hash: crate::achievements::progress::hash(console, data),
        console,
        name,
    };
    (identity, game)
}

#[cfg(not(target_arch = "wasm32"))]
fn title(
    data: &[u8],
    sha1: &[u8; 20],
    file_name: Option<&str>,
    console: Option<citrine_gb::console::ConsoleKind>,
) -> Option<String> {
    #[cfg(feature = "nds")]
    if console == Some(citrine_gb::console::ConsoleKind::Nds) {
        return crate::ds_banner::game_name(data, sha1, file_name);
    }
    let _ = (sha1, console);
    let header = citrine_gb::rom::Rom::new(data).header().ok()?;
    Some(crate::game_name::game_name(
        &header.sha1,
        file_name,
        header.short_title(),
    ))
}

#[cfg(all(test, not(target_arch = "wasm32"), feature = "nds"))]
mod tests {
    use super::*;
    use crate::utils::zip_games::tests::zip_of;
    use citrine_gb::console::ConsoleKind;
    use zip::CompressionMethod;

    const MB: usize = 1 << 20;

    fn ds_image(len: usize) -> Vec<u8> {
        let mut rom = vec![0; len];
        rom[..12].copy_from_slice(b"BIGGAMETITLE");
        rom[0x0C..0x10].copy_from_slice(b"ABCE");
        rom[0x68..0x6C].copy_from_slice(&0x1000u32.to_le_bytes());
        rom[0x80..0x84].copy_from_slice(&(len as u32).to_le_bytes());
        rom[0x15C..0x15E].copy_from_slice(&0xCF56u16.to_le_bytes());
        let crc = citrine_gb::utils::crc16::crc16(0xFFFF, &rom[..0x15E]);
        rom[0x15E..0x160].copy_from_slice(&crc.to_le_bytes());
        rom[0x1000..0x1002].copy_from_slice(&1u16.to_le_bytes());
        for language in 0..6 {
            let at = 0x1000 + 0x240 + language * 0x100;
            for (i, unit) in "Big Banner Game".encode_utf16().enumerate() {
                rom[at + i * 2..at + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
        rom[len - 1] = 0x5A;
        rom
    }

    fn written(dir: &tempfile::TempDir, name: &str, data: &[u8]) -> String {
        let path = dir.path().join(name);
        std::fs::write(&path, data).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_128_mb_ds_game_gets_its_name_and_cover_like_a_small_one() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Big Game.nds", "Big Game.bin"] {
            let rom = ds_image(128 * MB);
            let (identity, game) = read(&written(&dir, name, &rom)).expect(name);
            let id = RomId::new(&rom);
            assert_eq!(identity.rom_key, id.sha256_hex_string(), "{name}");
            assert_eq!(identity.sha1, id.sha1_hex_string(), "{name}");
            assert_eq!(identity.crc32, Some(id.crc32), "{name}");
            assert_eq!(identity.console, Some(ConsoleKind::Nds), "{name}");
            assert_eq!(identity.name.as_deref(), Some("Big Banner Game"), "{name}");
            let small = Game::of(&identity.rom_key, ConsoleKind::Nds, id.sha1, &rom[..0x200]);
            assert_eq!(game, small.named(name), "{name}");
        }
    }

    fn gb_image(len: usize, cgb: u8) -> Vec<u8> {
        let mut rom = vec![0; len];
        rom[0x104..0x134].copy_from_slice(&citrine_gb::rom::header::NINTENDO_LOGO);
        rom[0x134..0x138].copy_from_slice(b"TOBU");
        rom[0x143] = cgb;
        rom[len - 1] = 0x5A;
        rom
    }

    fn gba_image(len: usize) -> Vec<u8> {
        let mut rom = vec![0; len];
        rom[3] = 0xEA;
        rom[0xA0..0xA8].copy_from_slice(b"KIRBYAMZ");
        rom[0xAC..0xB0].copy_from_slice(b"B8KE");
        rom[0xB2] = 0x96;
        rom[len - 1] = 0x5A;
        rom
    }

    #[test]
    fn game_boy_and_gba_games_keep_their_identity() {
        let dir = tempfile::tempdir().unwrap();
        let games = [
            ("tobu.gb", gb_image(0x8000, 0), ConsoleKind::Gb, "Tobu"),
            ("tobu.gbc", gb_image(0x8000, 0x80), ConsoleKind::Gbc, "Tobu"),
            ("dump.bin", gb_image(0x8000, 0x80), ConsoleKind::Gbc, "Dump"),
            ("kirby.gba", gba_image(MB), ConsoleKind::Gba, "Kirby"),
        ];
        for (name, rom, console, title) in games {
            let (identity, game) = read(&written(&dir, name, &rom)).expect(name);
            let id = RomId::new(&rom);
            assert_eq!(identity.rom_key, id.sha256_hex_string(), "{name}");
            assert_eq!(identity.sha1, id.sha1_hex_string(), "{name}");
            assert_eq!(identity.crc32, Some(id.crc32), "{name}");
            assert_eq!(identity.console, Some(console), "{name}");
            assert_eq!(identity.name.as_deref(), Some(title), "{name}");
            assert_eq!(
                game,
                Game::of(&identity.rom_key, console, id.sha1, &rom).named(name)
            );
        }
    }

    #[test]
    fn game_boy_and_gba_games_over_64_mb_stay_unread() {
        let dir = tempfile::tempdir().unwrap();
        let over = 64 * MB + 1;
        for (name, rom) in [("big.gb", gb_image(over, 0)), ("big.gba", gba_image(over))] {
            assert!(read(&written(&dir, name, &rom)).is_none(), "{name}");
        }
    }

    fn zipped(
        dir: &tempfile::TempDir,
        entry: &str,
        data: &[u8],
        method: CompressionMethod,
    ) -> String {
        let zip = dir.path().join("pack.zip");
        std::fs::write(
            &zip,
            zip_of(&[("readme.txt", b"hi"), (entry, data)], method),
        )
        .unwrap();
        zip.join(entry).to_string_lossy().into_owned()
    }

    #[test]
    fn a_zipped_game_boy_game_gets_its_name_and_cover() {
        let dir = tempfile::tempdir().unwrap();
        let rom = gb_image(0x8000, 0x80);
        let path = zipped(&dir, "roms/tobu.gbc", &rom, CompressionMethod::Deflated);
        let (identity, game) = read(&path).expect("identified");
        let id = RomId::new(&rom);
        assert_eq!(identity.rom_key, id.sha256_hex_string());
        assert_eq!(identity.console, Some(ConsoleKind::Gbc));
        assert_eq!(identity.name.as_deref(), Some("Tobu"));
        assert_eq!(
            game,
            Game::of(&identity.rom_key, ConsoleKind::Gbc, id.sha1, &rom).named("tobu.gbc")
        );
    }

    #[test]
    fn a_zipped_ds_game_over_64_mb_gets_its_name_and_cover() {
        let dir = tempfile::tempdir().unwrap();
        let rom = ds_image(72 * MB);
        let path = zipped(&dir, "Big Game.nds", &rom, CompressionMethod::Stored);
        let (identity, game) = read(&path).expect("identified");
        let id = RomId::new(&rom);
        assert_eq!(identity.rom_key, id.sha256_hex_string());
        assert_eq!(identity.sha1, id.sha1_hex_string());
        assert_eq!(identity.crc32, Some(id.crc32));
        assert_eq!(identity.console, Some(ConsoleKind::Nds));
        assert_eq!(identity.name.as_deref(), Some("Big Banner Game"));
        let small = Game::of(&identity.rom_key, ConsoleKind::Nds, id.sha1, &rom[..0x200]);
        assert_eq!(game, small.named("Big Game.nds"));
    }

    #[test]
    fn a_game_loose_and_zipped_gets_one_rom_key() {
        let dir = tempfile::tempdir().unwrap();
        let rom = gba_image(MB);
        let loose = read(&written(&dir, "kirby.gba", &rom)).expect("loose").0;
        let packed = zipped(&dir, "kirby.gba", &rom, CompressionMethod::Deflated);
        let packed = read(&packed).expect("zipped").0;
        assert_eq!(packed.rom_key, loose.rom_key);
        assert_eq!(packed.sha1, loose.sha1);
        assert_eq!(packed.ra_hash, loose.ra_hash);
    }

    #[test]
    fn zipped_games_over_their_limit_or_damaged_stay_unread() {
        let dir = tempfile::tempdir().unwrap();
        let over = gb_image(64 * MB + 1, 0);
        let path = zipped(&dir, "big.gb", &over, CompressionMethod::Stored);
        assert!(read(&path).is_none());
        let small = gb_image(0x8000, 0);
        let path = zipped(&dir, "small.gb", &small, CompressionMethod::Stored);
        assert!(read(&path).is_some());
        let zip = dir.path().join("pack.zip");
        let mut bytes = std::fs::read(&zip).unwrap();
        bytes[0x200] ^= 0xFF;
        std::fs::write(&zip, &bytes).unwrap();
        assert!(read(&path).is_none());
        assert!(read(&zip.join("missing.gb").to_string_lossy()).is_none());
    }

    #[test]
    #[ignore = "timing on the fleet"]
    fn identify_time_of_a_128_mb_ds_game() {
        let dir = tempfile::tempdir().unwrap();
        let path = written(&dir, "Big Game.nds", &ds_image(128 * MB));
        let rom = std::fs::read(&path).unwrap();
        let ms = |run: &dyn Fn()| {
            let mut times: Vec<f64> = (0..7)
                .map(|_| {
                    let start = std::time::Instant::now();
                    run();
                    start.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            times.sort_by(f64::total_cmp);
            format!(
                "min {:.1} median {:.1} max {:.1}",
                times[0], times[3], times[6]
            )
        };
        let pool = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        println!("TIME lanes {}", pool.pool_size());
        println!("TIME read {}", ms(&|| assert!(read(&path).is_some())));
        println!(
            "TIME fs::read {}",
            ms(&|| assert_eq!(std::fs::read(&path).unwrap().len(), rom.len()))
        );
        println!(
            "TIME rom_id {}",
            ms(&|| assert_ne!(RomId::new(&rom).crc32, 0))
        );
        println!(
            "TIME identity_of {}",
            ms(&|| assert!(identity_of(&rom, Some("Big Game.nds")).0.name.is_some()))
        );
    }

    #[test]
    #[ignore = "real 128 MB DS game in .roms"]
    fn platinum_at_128_mb_gets_its_name_and_serial() {
        let test = "platinum_at_128_mb_gets_its_name_and_serial";
        let path = citrine_gb_test_roms::roms_dir().join("pt.nds");
        let Some(rom) = citrine_gb_test_roms::read(test, &path) else {
            return;
        };
        assert_eq!(rom.len(), 128 * MB);
        let (identity, game) = read(&path.to_string_lossy()).expect("identified");
        let id = RomId::new(&rom);
        assert_eq!(identity.rom_key, id.sha256_hex_string());
        assert_eq!(identity.console, Some(ConsoleKind::Nds));
        assert_eq!(identity.name.as_deref(), Some("Pokémon Platinum"));
        assert!(identity.ra_hash.is_some());
        let small = Game::of(&identity.rom_key, ConsoleKind::Nds, id.sha1, &rom[..0x200]);
        assert_eq!(game, small.named("pt.nds"));
        assert!(
            format!("{game:?}").contains("serial: Some([67, 80, 85,"),
            "{game:?}"
        );
    }
}
