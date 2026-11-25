use crate::progress::Progress;
use flate2::{Crc, Decompress, FlushDecompress, Status};
use std::io::Cursor;
use std::sync::Arc;
use web_time::{Duration, Instant};
use zip::{CompressionMethod, ZipArchive};

const MAX_GAME: u64 = 1 << 30;
const STEP: usize = 1 << 20;

#[cfg(not(feature = "nds"))]
const GAMES: &[&str] = &["gb", "gbc", "gba"];
#[cfg(feature = "nds")]
const GAMES: &[&str] = &["gb", "gbc", "gba", "nds"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ZipError {
    #[error("it is damaged")]
    Damaged,
    #[error("it needs a password")]
    Locked,
    #[error("it is packed in a way Citrine can't unpack")]
    Unsupported,
    #[error("the game in it is too big")]
    TooBig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipGame {
    index: usize,
    pub entry: String,
    pub size: u64,
}

impl ZipGame {
    pub fn file_name(&self) -> &str {
        file_name(&self.entry)
    }
}

fn file_name(entry: &str) -> &str {
    entry.rsplit(['/', '\\']).next().unwrap_or(entry)
}

fn extension(name: &str) -> Option<String> {
    std::path::Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
}

pub fn is_zip(name: &str, data: &[u8]) -> bool {
    match extension(name).as_deref() {
        Some("zip") => true,
        Some(extension) if GAMES.contains(&extension) => false,
        _ => data.starts_with(b"PK\x03\x04"),
    }
}

pub fn games(zip: &[u8]) -> Result<Vec<ZipGame>, ZipError> {
    let mut archive = ZipArchive::new(Cursor::new(zip)).map_err(|_| ZipError::Damaged)?;
    listed(&mut archive)
}

fn listed<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<ZipGame>, ZipError> {
    let mut games = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index).map_err(|_| ZipError::Damaged)?;
        let Some(entry) = file.enclosed_name().as_deref().and_then(entry_name) else {
            continue;
        };
        let name = file_name(&entry);
        let hidden = entry.starts_with("__MACOSX/") || name.starts_with("._");
        let game = extension(name).is_some_and(|extension| GAMES.contains(&extension.as_str()));
        if file.is_dir() || hidden || !game {
            continue;
        }
        games.push(ZipGame {
            index,
            entry,
            size: file.size(),
        });
    }
    games.sort_by_key(|game| game.entry.to_lowercase());
    Ok(games)
}

fn entry_name(path: &std::path::Path) -> Option<String> {
    let parts = path
        .components()
        .map(|part| match part {
            std::path::Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub fn find<'a>(games: &'a [ZipGame], entry: &str) -> Option<&'a ZipGame> {
    games.iter().find(|game| game.entry == entry)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn split_entry(path: &std::path::Path) -> Option<(std::path::PathBuf, String)> {
    let zip = path.ancestors().skip(1).find(|dir| dir.is_file())?;
    let zipped = zip
        .file_name()
        .and_then(|name| extension(&name.to_string_lossy()))
        .is_some_and(|extension| extension == "zip");
    let entry = path
        .strip_prefix(zip)
        .ok()?
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    zipped.then(|| (zip.to_path_buf(), entry))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn outside_zip(path: &std::path::Path) -> &std::path::Path {
    path.ancestors()
        .skip(1)
        .find(|dir| {
            dir.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
                && !dir.is_dir()
        })
        .unwrap_or(path)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn on_disk(path: &std::path::Path) -> bool {
    path.is_file()
        || split_entry(path).is_some_and(|(zip, entry)| {
            entries(&zip).is_ok_and(|games| find(&games, &entry).is_some())
        })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn entries(zip: &std::path::Path) -> std::io::Result<Arc<[ZipGame]>> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    type Listed = HashMap<std::path::PathBuf, (crate::rom_ids::Stamp, Arc<[ZipGame]>)>;
    thread_local! {
        static LISTED: RefCell<Listed> = RefCell::new(HashMap::new());
    }
    const KEPT: usize = 256;
    let stamp = crate::rom_ids::Stamp::of(zip)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "the zip is gone"))?;
    let cached = LISTED.with_borrow(|listed| {
        listed
            .get(zip)
            .filter(|(seen, _)| *seen == stamp)
            .map(|(_, games)| games.clone())
    });
    if let Some(games) = cached {
        return Ok(games);
    }
    let file = std::io::BufReader::new(std::fs::File::open(zip)?);
    let mut archive =
        ZipArchive::new(file).map_err(|_| std::io::Error::other(ZipError::Damaged))?;
    let games: Arc<[ZipGame]> = listed(&mut archive).map_err(std::io::Error::other)?.into();
    if !stamp.settled() {
        return Ok(games);
    }
    LISTED.with_borrow_mut(|listed| {
        if listed.len() >= KEPT {
            listed.clear();
        }
        listed.insert(zip.to_path_buf(), (stamp, games.clone()));
    });
    Ok(games)
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn file_on_disk(path: &std::path::Path) -> std::path::PathBuf {
    match split_entry(path).filter(|_| !path.is_file()) {
        Some((zip, _)) => zip,
        None => path.to_path_buf(),
    }
}

#[cfg(target_arch = "wasm32")]
pub fn on_disk(path: &std::path::Path) -> bool {
    path.is_file()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_game(path: &std::path::Path) -> std::io::Result<Arc<[u8]>> {
    let Some((zip, entry)) = split_entry(path).filter(|_| !path.is_file()) else {
        return crate::rom_ids::read_shared(path);
    };
    let data = crate::rom_ids::read_shared(&zip)?;
    let listed = games(&data).map_err(std::io::Error::other)?;
    let game = find(&listed, &entry).cloned().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the game is no longer in the zip",
        )
    })?;
    let mut unzip = Unzip::new(data, &game).map_err(std::io::Error::other)?;
    loop {
        match unzip.step(STEP) {
            Step::Working(next) => unzip = next,
            Step::Done(game) => return Ok(game),
            Step::Failed(err) => return Err(std::io::Error::other(err)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn with_entry<T>(
    path: &std::path::Path,
    read: impl FnOnce(u64, &mut dyn std::io::Read) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let gone = || std::io::Error::new(std::io::ErrorKind::NotFound, "the game is not in a zip");
    let (zip, entry) = split_entry(path).ok_or_else(gone)?;
    let file = std::io::BufReader::new(std::fs::File::open(zip)?);
    let mut archive =
        ZipArchive::new(file).map_err(|_| std::io::Error::other(ZipError::Damaged))?;
    let listed = listed(&mut archive).map_err(std::io::Error::other)?;
    let game = find(&listed, &entry).ok_or_else(gone)?;
    let mut file = archive
        .by_index(game.index)
        .map_err(|_| std::io::Error::other(ZipError::Damaged))?;
    read(game.size, &mut file)
}

#[cfg(target_arch = "wasm32")]
pub fn read_game(path: &std::path::Path) -> std::io::Result<Arc<[u8]>> {
    std::fs::read(path).map(Arc::from)
}

pub struct Unzip {
    zip: Arc<[u8]>,
    start: usize,
    end: usize,
    size: usize,
    crc: u32,
    sum: Crc,
    checked: usize,
    inflate: Option<(Decompress, Vec<u8>)>,
}

pub enum Step {
    Working(Unzip),
    Done(Arc<[u8]>),
    Failed(ZipError),
}

impl Unzip {
    pub fn new(zip: Arc<[u8]>, game: &ZipGame) -> Result<Self, ZipError> {
        let (start, end, size, crc, stored) = {
            let mut archive =
                ZipArchive::new(Cursor::new(&zip[..])).map_err(|_| ZipError::Damaged)?;
            let file = archive
                .by_index_raw(game.index)
                .map_err(|_| ZipError::Damaged)?;
            if file.encrypted() {
                return Err(ZipError::Locked);
            }
            if file.size() > MAX_GAME {
                return Err(ZipError::TooBig);
            }
            let stored = match file.compression() {
                CompressionMethod::Stored => true,
                CompressionMethod::Deflated => false,
                _ => return Err(ZipError::Unsupported),
            };
            let start = usize::try_from(file.data_start().ok_or(ZipError::Damaged)?)
                .map_err(|_| ZipError::Damaged)?;
            let end = usize::try_from(file.compressed_size())
                .ok()
                .and_then(|length| start.checked_add(length))
                .filter(|end| *end <= zip.len())
                .ok_or(ZipError::Damaged)?;
            (start, end, file.size() as usize, file.crc32(), stored)
        };
        if stored && end - start != size {
            return Err(ZipError::Damaged);
        }
        Ok(Self {
            zip,
            start,
            end,
            size,
            crc,
            sum: Crc::new(),
            checked: 0,
            inflate: (!stored).then(|| (Decompress::new(false), vec![0; size + 1])),
        })
    }

    pub fn progress(&self) -> Progress {
        Progress::bytes(self.done() as u64, self.size as u64)
    }

    fn done(&self) -> usize {
        self.inflate
            .as_ref()
            .map_or(self.checked, |(inflate, _)| inflate.total_out() as usize)
    }

    pub fn run_for(self, limit: Duration) -> Step {
        let started = Instant::now();
        let mut unzip = self;
        loop {
            match unzip.step(STEP) {
                Step::Working(next) if started.elapsed() < limit => unzip = next,
                step => return step,
            }
        }
    }

    pub fn step(mut self, budget: usize) -> Step {
        let budget = budget.max(1);
        let Some((inflate, out)) = &mut self.inflate else {
            let to = (self.checked + budget).min(self.size);
            self.sum
                .update(&self.zip[self.start + self.checked..self.start + to]);
            self.checked = to;
            if to < self.size {
                return Step::Working(self);
            }
            if self.sum.sum() != self.crc {
                return Step::Failed(ZipError::Damaged);
            }
            return Step::Done(Arc::from(&self.zip[self.start..self.end]));
        };
        let done = inflate.total_out() as usize;
        let read = (self.start + inflate.total_in() as usize).min(self.end);
        let room = (done + budget).min(out.len());
        let status = inflate.decompress(
            &self.zip[read..self.end],
            &mut out[done..room],
            FlushDecompress::None,
        );
        let now = inflate.total_out() as usize;
        self.sum.update(&out[done..now]);
        match status {
            Ok(Status::StreamEnd) if now == self.size && self.sum.sum() == self.crc => {
                Step::Done(Arc::from(&out[..self.size]))
            }
            Ok(Status::Ok) if now <= self.size => Step::Working(self),
            _ => Step::Failed(ZipError::Damaged),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    pub(crate) fn zip_of(files: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, data) in files {
            if name.ends_with('/') {
                zip.add_directory(*name, options).unwrap();
                continue;
            }
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn game(seed: u8, size: usize) -> Vec<u8> {
        (0..size)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) ^ (i >> 9) as u8)
            .collect()
    }

    fn unzip_all(zip: Vec<u8>, game: &ZipGame, budget: usize) -> (Arc<[u8]>, Vec<Progress>) {
        let mut seen = Vec::new();
        let mut unzip = Unzip::new(zip.into(), game).unwrap();
        loop {
            seen.push(unzip.progress());
            match unzip.step(budget) {
                Step::Working(next) => unzip = next,
                Step::Done(data) => return (data, seen),
                Step::Failed(err) => panic!("{err}"),
            }
        }
    }

    #[test]
    fn a_zip_is_known_by_its_name_or_its_first_bytes() {
        let zip = zip_of(&[("a.gb", b"x")], CompressionMethod::Stored);
        assert!(is_zip("pack.zip", b""));
        assert!(is_zip("PACK.ZIP", b""));
        assert!(is_zip("download", &zip));
        assert!(!is_zip("tetris.gb", &zip));
        assert!(!is_zip("download", b"not a zip"));
    }

    #[test]
    fn lists_only_games_sorted_by_name() {
        let zip = zip_of(
            &[
                ("readme.txt", b"hi"),
                ("roms/", b""),
                ("roms/Zelda.gbc", b"z"),
                ("Advance.GBA", b"a"),
                ("__MACOSX/roms/._Zelda.gbc", b"m"),
                ("._Tetris.gb", b"m"),
                ("Tetris.gb", b"tt"),
                ("Tetris.sav", b"s"),
            ],
            CompressionMethod::Deflated,
        );
        let games = games(&zip).unwrap();
        let entries: Vec<_> = games.iter().map(|game| game.entry.as_str()).collect();
        assert_eq!(entries, ["Advance.GBA", "roms/Zelda.gbc", "Tetris.gb"]);
        assert_eq!(games[1].file_name(), "Zelda.gbc");
        assert_eq!(games[2].size, 2);
    }

    #[test]
    fn an_entry_name_never_leaves_the_zip() {
        let zip = zip_of(
            &[
                ("../Evil.gb", b"e"),
                ("roms/../../Up.gb", b"u"),
                ("/abs/Root.gb", b"r"),
                ("\\\\attacker.example\\share\\Pokemon.gb", b"p"),
                ("C:\\Drive.gb", b"d"),
                ("roms/../Fine.gb", b"f"),
            ],
            CompressionMethod::Stored,
        );
        let games = games(&zip).unwrap();
        let entries: Vec<_> = games.iter().map(|game| game.entry.as_str()).collect();
        assert_eq!(
            entries,
            ["abs/Root.gb", "Drive.gb", "Fine.gb", "Pokemon.gb"]
        );
        for entry in entries {
            let mut parts = std::path::Path::new(entry).components();
            let normal = parts.all(|part| matches!(part, std::path::Component::Normal(_)));
            assert!(normal, "{entry}");
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn lists_ds_games() {
        let zip = zip_of(&[("Game.nds", b"d")], CompressionMethod::Stored);
        assert_eq!(games(&zip).unwrap()[0].entry, "Game.nds");
    }

    #[test]
    fn a_damaged_zip_lists_nothing() {
        assert_eq!(games(b"PK\x03\x04 cut short"), Err(ZipError::Damaged));
    }

    #[test]
    fn unpacks_a_deflated_game_in_steps_with_byte_progress() {
        let rom = game(7, 300_000);
        let zip = zip_of(&[("Big.gba", &rom)], CompressionMethod::Deflated);
        let games = games(&zip).unwrap();
        let (data, seen) = unzip_all(zip, &games[0], 64 * 1024);
        assert_eq!(&data[..], rom);
        assert!(seen.len() >= 5);
        assert_eq!(seen[0], Progress::bytes(0, 300_000));
        assert!(seen.windows(2).all(|pair| pair[0].done <= pair[1].done));
    }

    #[test]
    fn unpacks_a_stored_game() {
        let rom = game(3, 100_000);
        let zip = zip_of(
            &[("a.txt", b"x"), ("Game.gb", &rom)],
            CompressionMethod::Stored,
        );
        let games = games(&zip).unwrap();
        let (data, _) = unzip_all(zip, &games[0], 16 * 1024);
        assert_eq!(&data[..], rom);
    }

    #[test]
    fn run_for_finishes_a_small_game_at_once() {
        let rom = game(1, 50_000);
        let zip = zip_of(&[("Game.gbc", &rom)], CompressionMethod::Deflated);
        let games = games(&zip).unwrap();
        let unzip = Unzip::new(zip.into(), &games[0]).unwrap();
        match unzip.run_for(Duration::from_secs(5)) {
            Step::Done(data) => assert_eq!(&data[..], rom),
            _ => panic!("not done"),
        }
    }

    #[test]
    fn a_changed_byte_fails_the_check() {
        let rom = game(5, 20_000);
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let mut zip = zip_of(&[("Game.gb", &rom)], method);
            let games = games(&zip).unwrap();
            let unzip = Unzip::new(zip.clone().into(), &games[0]).unwrap();
            let middle = unzip.start + (unzip.end - unzip.start) / 2;
            zip[middle] ^= 0x55;
            let failed = match Unzip::new(zip.into(), &games[0])
                .unwrap()
                .run_for(Duration::from_secs(5))
            {
                Step::Failed(err) => err,
                _ => panic!("{method:?} passed"),
            };
            assert_eq!(failed, ZipError::Damaged);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn splits_a_path_into_the_zip_and_the_game_inside() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pack.zip");
        std::fs::write(&zip, b"PK").unwrap();
        assert_eq!(
            split_entry(&zip.join("roms").join("Game.gb")),
            Some((zip.clone(), "roms/Game.gb".to_owned()))
        );
        let bare = dir.path().join("Game.gb");
        std::fs::write(&bare, b"g").unwrap();
        assert_eq!(split_entry(&bare), None);
        assert_eq!(split_entry(&bare.join("x.gb")), None);
    }

    #[test]
    fn a_game_reads_from_its_zip_or_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("Game.gb");
        std::fs::write(&bare, b"bare").unwrap();
        let zip = dir.path().join("games.zip");
        let packed = zip_of(&[("roms/Game.gb", b"packed")], CompressionMethod::Deflated);
        std::fs::write(&zip, packed).unwrap();

        assert_eq!(&read_game(&bare).unwrap()[..], b"bare");
        assert_eq!(
            &read_game(&zip.join("roms").join("Game.gb")).unwrap()[..],
            b"packed"
        );
        assert!(on_disk(&zip.join("roms").join("Game.gb")));
        assert_eq!(file_on_disk(&zip.join("roms").join("Game.gb")), zip);
        assert_eq!(file_on_disk(&bare), bare);
        assert!(read_game(&zip.join("Other.gb")).is_err());
        std::fs::remove_file(&zip).unwrap();
        assert!(!on_disk(&zip.join("roms").join("Game.gb")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_game_taken_out_of_its_zip_is_gone_while_the_rest_stays() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pack.zip");
        let both = zip_of(
            &[("roms/Game.gb", b"g"), ("Other.gbc", b"o")],
            CompressionMethod::Deflated,
        );
        std::fs::write(&zip, both).unwrap();
        let game = zip.join("roms").join("Game.gb");
        let other = zip.join("Other.gbc");
        assert!(on_disk(&game));
        assert!(on_disk(&other));
        assert!(!on_disk(&zip.join("readme.txt")));

        let left = zip_of(&[("Other.gbc", b"o")], CompressionMethod::Deflated);
        std::fs::write(&zip, left).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&zip)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(!on_disk(&game));
        assert!(on_disk(&other));

        std::fs::write(&zip, b"PK\x03\x04 cut short").unwrap();
        assert!(!on_disk(&other));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_zipped_game_lives_in_the_folder_of_its_zip() {
        let zipped = std::path::Path::new("/games/gba/pack.zip/roms/Game.gba");
        assert_eq!(
            outside_zip(zipped),
            std::path::Path::new("/games/gba/pack.zip")
        );
        let upper = std::path::Path::new("/games/gb/PACK.ZIP/Tetris.gb");
        assert_eq!(
            outside_zip(upper),
            std::path::Path::new("/games/gb/PACK.ZIP")
        );
        let loose = std::path::Path::new("/games/gb/Tetris.gb");
        assert_eq!(outside_zip(loose), loose);
        let zip = std::path::Path::new("/games/gb/pack.zip");
        assert_eq!(outside_zip(zip), zip);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_folder_named_like_a_zip_is_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("games.zip");
        std::fs::create_dir(&folder).unwrap();
        let game = folder.join("Tetris.gb");
        std::fs::write(&game, b"g").unwrap();
        assert_eq!(outside_zip(&game), game);
        let zip = dir.path().join("pack.zip");
        std::fs::write(
            &zip,
            zip_of(&[("Game.gb", b"g")], CompressionMethod::Stored),
        )
        .unwrap();
        assert_eq!(outside_zip(&zip.join("Game.gb")), zip);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_zip_rewritten_at_the_same_size_and_time_lists_its_new_games() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pack.zip");
        let first = zip_of(&[("Game.gb", b"g")], CompressionMethod::Stored);
        let second = zip_of(&[("Gold.gb", b"g")], CompressionMethod::Stored);
        assert_eq!(first.len(), second.len());
        std::fs::write(&zip, first).unwrap();
        let written = std::fs::metadata(&zip).unwrap().modified().unwrap();
        let names =
            |games: Arc<[ZipGame]>| games.iter().map(|g| g.entry.clone()).collect::<Vec<_>>();
        assert_eq!(names(entries(&zip).unwrap()), ["Game.gb"]);

        std::fs::write(&zip, second).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&zip)
            .unwrap()
            .set_modified(written)
            .unwrap();
        assert_eq!(names(entries(&zip).unwrap()), ["Gold.gb"]);
    }
}
