use super::identify::Identifier;
use super::{FileFound, FileState, Library};
use crate::covers::Game;
use crate::utils::file_loader::PickedFile;
use citrine_gb_threads::Spawner;
use std::sync::mpsc::{Receiver, Sender, channel};

const ALL_LEVELS: usize = 32;

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum_macros::EnumIter,
)]
pub enum LibraryDepth {
    Off,
    One,
    #[default]
    Two,
    All,
}

impl LibraryDepth {
    pub fn levels(self) -> usize {
        match self {
            Self::Off => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::All => ALL_LEVELS,
        }
    }
}

impl std::fmt::Display for LibraryDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Off => "Off",
            Self::One => "1 level",
            Self::Two => "2 levels",
            Self::All => "All levels",
        })
    }
}

pub struct Listing {
    pub tree: String,
    pub games: Vec<(String, String)>,
    pub readable: bool,
}

type Checked = Vec<(String, FileState)>;

pub(super) struct Scan {
    picked: (Sender<String>, Receiver<String>),
    lists: (Sender<Listing>, Receiver<Listing>),
    checks: (Sender<Checked>, Receiver<Checked>),
    pub(super) ids: Identifier,
    started: bool,
    waiting: usize,
    skipped: std::collections::HashSet<String>,
    wanted_at: Option<u64>,
    depth: LibraryDepth,
}

impl Default for Scan {
    fn default() -> Self {
        Self {
            picked: channel(),
            lists: channel(),
            checks: channel(),
            ids: Identifier::default(),
            started: false,
            waiting: 0,
            skipped: std::collections::HashSet::new(),
            wanted_at: None,
            depth: LibraryDepth::default(),
        }
    }
}

impl Library {
    pub fn choose_folder(&self, spawner: &Spawner) {
        let trees = self.scan.picked.0.clone();
        #[cfg(target_os = "android")]
        {
            let _ = spawner;
            crate::platform::android::files::pick_rom_folder(trees);
        }
        #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
        crate::utils::file_loader::FolderPicker::new()
            .title("Add a ROM folder")
            .pick(spawner, move |path| {
                let _ = trees.send(path.to_string_lossy().into_owned());
            });
        #[cfg(target_arch = "wasm32")]
        let _ = (spawner, trees);
    }

    pub fn follow_depth(&mut self, spawner: &Spawner, depth: LibraryDepth) {
        if std::mem::replace(&mut self.scan.depth, depth) != depth && self.scan.started {
            self.rescan(spawner);
        }
    }

    pub fn rescan(&mut self, spawner: &Spawner) {
        self.scan.started = true;
        for folder in &self.folders {
            self.scan.waiting += 1;
            list(
                spawner,
                &folder.tree,
                self.scan.depth,
                self.scan.lists.0.clone(),
            );
        }
        let loose = self.loose_files();
        if cfg!(not(target_arch = "wasm32")) && !loose.is_empty() {
            self.scan.waiting += 1;
            check(spawner, loose, self.scan.checks.0.clone());
        }
    }

    pub fn busy(&self) -> bool {
        self.scan.waiting > 0 || self.scan.ids.busy()
    }

    pub fn bring_back(
        &mut self,
        spawner: &Spawner,
        folder: Option<String>,
        games: Vec<super::LibraryGame>,
    ) {
        if let Some(tree) = folder
            && self.add_folder(&tree)
        {
            self.scan.waiting += 1;
            list(spawner, &tree, self.scan.depth, self.scan.lists.0.clone());
        }
        self.restore(games);
    }

    pub fn poll(&mut self, spawner: &Spawner, now: u64) -> Vec<Game> {
        if !self.scan.started {
            self.rescan(spawner);
        }
        while let Ok(tree) = self.scan.picked.1.try_recv() {
            if self.add_folder(&tree) {
                self.scan.waiting += 1;
                list(spawner, &tree, self.scan.depth, self.scan.lists.0.clone());
            }
        }
        while let Ok(listing) = self.scan.lists.1.try_recv() {
            self.scan.waiting = self.scan.waiting.saturating_sub(1);
            let files = listing.readable.then(|| {
                listing
                    .games
                    .into_iter()
                    .filter_map(|(file, path)| {
                        let name = game_name(&file);
                        (name != file).then_some(FileFound { name, path })
                    })
                    .collect()
            });
            self.apply_listing(&listing.tree, files, now);
        }
        while let Ok(checks) = self.scan.checks.1.try_recv() {
            self.scan.waiting = self.scan.waiting.saturating_sub(1);
            self.apply_checks(checks);
        }
        let mut covers = Vec::new();
        let mut known = Vec::new();
        for (path, found) in self.scan.ids.take_batch() {
            match found {
                Some((identity, game)) => {
                    if identity.ra_hash.is_none() {
                        self.scan.skipped.insert(path.clone());
                    }
                    known.push((path, identity));
                    covers.push(game);
                }
                None => {
                    self.scan.skipped.insert(path);
                }
            }
        }
        if !known.is_empty() {
            self.identify_all(known);
        }
        if self.scan.wanted_at != Some(self.revision()) {
            self.scan.wanted_at = Some(self.revision());
            let Library { games, scan, .. } = self;
            let skipped = &scan.skipped;
            scan.ids.want(
                games
                    .iter()
                    .filter(|game| super::waits(game) && !skipped.contains(&game.path))
                    .map(|game| game.path.as_str()),
            );
        }
        self.scan.ids.top_up(spawner);
        covers
    }

    pub fn read(&self, spawner: &Spawner, path: &str, tx: Sender<PickedFile>) {
        #[cfg(target_os = "android")]
        if path.starts_with("content://") {
            let _ = spawner;
            crate::platform::android::files::read_rom(path, tx);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = std::path::PathBuf::from(path);
            spawner.run("library read", move || {
                if let Ok(file) = PickedFile::open(path) {
                    let _ = tx.send(file);
                }
            });
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (spawner, path, tx);
    }
}

pub fn game_name(file: &str) -> String {
    let lower = file.to_lowercase();
    let cut = crate::utils::save_files::ROM_FILTER
        .1
        .iter()
        .find(|extension| {
            lower
                .strip_suffix(*extension)
                .is_some_and(|stem| stem.ends_with('.'))
        })
        .map_or(file.len(), |extension| file.len() - extension.len() - 1);
    file[..cut].trim().to_owned()
}

pub fn console_of(file: &str) -> Option<citrine_gb::console::ConsoleKind> {
    use citrine_gb::console::ConsoleKind;
    let extension = file.rsplit_once('.')?.1.to_lowercase();
    match extension.as_str() {
        "gb" => Some(ConsoleKind::Gb),
        "gbc" => Some(ConsoleKind::Gbc),
        "gba" => Some(ConsoleKind::Gba),
        "nds" => Some(ConsoleKind::Nds),
        _ => None,
    }
}

pub fn folder_label(tree: &str) -> String {
    if !tree.starts_with("content://") {
        return std::path::Path::new(tree).file_name().map_or_else(
            || tree.to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
    }
    let last = tree.rsplit('/').next().unwrap_or(tree);
    let decoded = percent_decode(last);
    match decoded.split_once(':') {
        Some((_, "")) => "Phone storage".to_owned(),
        Some((_, path)) => path.to_owned(),
        None => decoded,
    }
}

pub fn shown_path(path: &str) -> String {
    if !path.starts_with("content://") {
        return path.to_owned();
    }
    let decoded = percent_decode(path.rsplit('/').next().unwrap_or(path));
    match decoded.split_once(':') {
        Some((_, rest)) => format!("Phone storage/{rest}"),
        None => decoded,
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn list(spawner: &Spawner, tree: &str, depth: LibraryDepth, tx: Sender<Listing>) {
    #[cfg(target_os = "android")]
    {
        let _ = spawner;
        crate::platform::android::files::list_roms(tree, depth.levels(), tx);
    }
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    {
        let tree = tree.to_owned();
        spawner.run("library folder scan", move || {
            let _ = tx.send(list_dir(&tree, depth.levels()));
        });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (spawner, tree, depth.levels(), tx);
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub fn list_dir(tree: &str, levels: usize) -> Listing {
    let mut games = Vec::new();
    let readable = collect(std::path::Path::new(tree), levels, &mut games).is_ok();
    games.sort_by_key(|(name, _)| name.to_lowercase());
    Listing {
        tree: tree.to_owned(),
        games,
        readable,
    }
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn collect(
    dir: &std::path::Path,
    depth: usize,
    games: &mut Vec<(String, String)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth > 0 {
                let _ = collect(&path, depth - 1, games);
            }
        } else if game_name(&name) != name {
            games.push((name, path.to_string_lossy().into_owned()));
        }
    }
    Ok(())
}

fn check(spawner: &Spawner, paths: Vec<String>, tx: Sender<Checked>) {
    if paths.is_empty() {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    spawner.run("library file check", move || {
        let checks = paths
            .into_iter()
            .map(|path| {
                let file = file_state(std::path::Path::new(&path));
                (path, file)
            })
            .collect();
        let _ = tx.send(checks);
    });
    #[cfg(target_arch = "wasm32")]
    let _ = (spawner, paths, tx);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn file_state(path: &std::path::Path) -> FileState {
    if crate::utils::zip_games::on_disk(path) {
        FileState::Present
    } else if crate::utils::zip_games::outside_zip(path)
        .parent()
        .is_some_and(std::path::Path::is_dir)
    {
        FileState::Missing
    } else {
        FileState::Offline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_uri_shows_as_its_folder_path() {
        let tree =
            "content://com.android.externalstorage.documents/tree/primary%3ADownload%2FGame%20Boy";
        assert_eq!(folder_label(tree), "Download/Game Boy");
        let root = "content://com.android.externalstorage.documents/tree/primary%3A";
        assert_eq!(folder_label(root), "Phone storage");
    }

    #[test]
    fn game_names_drop_the_extension_in_any_case() {
        assert_eq!(game_name("Tobu Tobu Girl.gb"), "Tobu Tobu Girl");
        assert_eq!(game_name("Petris.GBC"), "Petris");
        assert_eq!(game_name("readme.txt"), "readme.txt");
        assert_eq!(game_name("Tobu.gbcx"), "Tobu.gbcx");
    }

    #[test]
    fn gba_games_drop_their_extension_too() {
        assert_eq!(game_name("Pokemon Emerald.GBA"), "Pokemon Emerald");
    }

    #[test]
    fn a_plain_folder_lists_its_roms_two_levels_deep() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        let too_deep = nested.join("c");
        std::fs::create_dir_all(&too_deep).unwrap();
        std::fs::write(dir.path().join("Petris.gbc"), b"rom").unwrap();
        std::fs::write(nested.join("Adjustris.gb"), b"rom").unwrap();
        std::fs::write(too_deep.join("Hidden.gb"), b"rom").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"text").unwrap();
        let listing = list_dir(
            &dir.path().to_string_lossy(),
            LibraryDepth::default().levels(),
        );
        assert!(listing.readable);
        let names: Vec<&str> = listing
            .games
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["Adjustris.gb", "Petris.gbc"]);
        assert_eq!(folder_label("/home/clara/roms"), "roms");
        assert!(!list_dir("/no/such/folder", 2).readable);
    }

    #[test]
    fn the_subfolder_depth_decides_how_deep_a_folder_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut folder = dir.path().to_path_buf();
        for (deeper, name) in [
            (0, "Top.gb"),
            (1, "One.gb"),
            (1, "Two.gb"),
            (1, "Three.gb"),
            (3, "Six.gb"),
        ] {
            for step in 0..deeper {
                folder = folder.join(format!("{name}-{step}"));
            }
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(name), b"rom").unwrap();
        }
        let tree = dir.path().to_string_lossy().into_owned();
        let found = |depth: LibraryDepth| {
            let mut names: Vec<String> = list_dir(&tree, depth.levels())
                .games
                .into_iter()
                .map(|(name, _)| name)
                .collect();
            names.sort();
            names
        };
        assert_eq!(found(LibraryDepth::Off), ["Top.gb"]);
        assert_eq!(found(LibraryDepth::One), ["One.gb", "Top.gb"]);
        assert_eq!(found(LibraryDepth::Two), ["One.gb", "Top.gb", "Two.gb"]);
        assert_eq!(
            found(LibraryDepth::All),
            ["One.gb", "Six.gb", "Three.gb", "Top.gb", "Two.gb"]
        );
        assert_eq!(LibraryDepth::default(), LibraryDepth::Two);
        assert_eq!(LibraryDepth::All.to_string(), "All levels");
    }

    #[test]
    fn a_new_subfolder_depth_reads_the_folders_again() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(dir.path().join("Top.gb"), b"top rom").unwrap();
        std::fs::write(deep.join("Deep.gb"), b"deep rom").unwrap();
        let spawner = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads).spawner();
        let mut library = Library::default();
        library.add_folder(&dir.path().to_string_lossy());
        let settle = |library: &mut Library| {
            library.poll(&spawner, 1);
            let start = Instant::now();
            while library.busy() && start.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(5));
                library.poll(&spawner, 1);
            }
            library.entries().len()
        };
        library.follow_depth(&spawner, LibraryDepth::Two);
        assert_eq!(settle(&mut library), 1);
        library.follow_depth(&spawner, LibraryDepth::All);
        assert!(library.busy(), "a new depth starts a scan");
        assert_eq!(settle(&mut library), 2);
    }

    #[test]
    fn a_gone_file_is_missing_and_a_gone_folder_is_not_connected() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("Petris.gbc");
        std::fs::write(&rom, b"rom").unwrap();
        assert_eq!(file_state(&rom), FileState::Present);
        std::fs::remove_file(&rom).unwrap();
        assert_eq!(file_state(&rom), FileState::Missing);
        assert_eq!(
            file_state(std::path::Path::new("/Volumes/Gone/GBA/Petris.gbc")),
            FileState::Offline
        );
    }

    #[test]
    fn a_game_gone_from_its_zip_or_with_its_zip_is_missing() {
        use crate::utils::zip_games::tests::zip_of;
        use zip::CompressionMethod;
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pack.zip");
        let both = zip_of(
            &[("roms/Petris.gbc", b"p"), ("Snake.gb", b"s")],
            CompressionMethod::Stored,
        );
        std::fs::write(&zip, both).unwrap();
        let petris = zip.join("roms").join("Petris.gbc");
        assert_eq!(file_state(&petris), FileState::Present);
        std::fs::write(
            &zip,
            zip_of(&[("Snake.gb", b"s")], CompressionMethod::Stored),
        )
        .unwrap();
        assert_eq!(file_state(&petris), FileState::Missing);
        assert_eq!(file_state(&zip.join("Snake.gb")), FileState::Present);
        std::fs::remove_file(&zip).unwrap();
        assert_eq!(file_state(&zip.join("Snake.gb")), FileState::Missing);
    }

    #[test]
    fn a_tree_document_shows_as_a_phone_storage_path() {
        let uri = "content://com.android.externalstorage.documents/tree/primary%3AGames/document/primary%3AGames%2FGBA%2FAstro%20Boy.gba";
        assert_eq!(shown_path(uri), "Phone storage/Games/GBA/Astro Boy.gba");
        assert_eq!(shown_path("/roms/a.gb"), "/roms/a.gb");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn shelf_of(dir: &std::path::Path, count: usize) -> Library {
        let tree = dir.to_string_lossy().into_owned();
        let files = (0..count)
            .map(|n| {
                let path = dir.join(format!("Game {n}.gb"));
                let mut rom = vec![0u8; 0x200];
                rom[0x150..0x158].copy_from_slice(&(n as u64).to_le_bytes());
                std::fs::write(&path, rom).unwrap();
                FileFound {
                    name: format!("Game {n}"),
                    path: path.to_string_lossy().into_owned(),
                }
            })
            .collect();
        let mut library = Library::default();
        library.add_folder(&tree);
        library.apply_listing(&tree, Some(files), 1);
        library
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn identify_all(library: &mut Library, workers: usize) -> usize {
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Threads);
        let spawner = threads.spawner();
        library.scan.ids.workers = workers;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let unknown = |library: &Library| {
            library
                .entries()
                .iter()
                .filter(|game| game.rom_key.is_none())
                .count()
        };
        let mut left = unknown(library);
        let mut batches = 0;
        while (library.busy() || left > 0) && std::time::Instant::now() < deadline {
            library.poll(&spawner, 1);
            let now_left = unknown(library);
            if now_left < left {
                batches += 1;
            }
            left = now_left;
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        batches
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_folder_is_identified_by_no_more_than_the_given_workers() {
        for workers in [1, 3] {
            let dir = tempfile::tempdir().unwrap();
            let mut library = shelf_of(dir.path(), 60);
            identify_all(&mut library, workers);
            assert!(!library.busy());
            assert_eq!(library.unidentified().count(), 0);
            assert!(library.entries().iter().all(|game| game.rom_key.is_some()));
            assert_eq!(library.scan.ids.peak(), workers);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn identified_games_reach_the_library_in_batches() {
        let dir = tempfile::tempdir().unwrap();
        let mut library = shelf_of(dir.path(), 400);
        let batches = identify_all(&mut library, 3);
        assert_eq!(library.unidentified().count(), 0);
        assert!(batches * 20 <= 400, "{batches} batches for 400 games");
    }

    #[test]
    fn a_folder_game_names_its_console_from_its_file() {
        use citrine_gb::console::ConsoleKind;
        assert_eq!(console_of("roms/Tetris.gb"), Some(ConsoleKind::Gb));
        assert_eq!(
            console_of("roms/Pokemon Crystal.GBC"),
            Some(ConsoleKind::Gbc)
        );
        assert_eq!(console_of("roms/Emerald.gba"), Some(ConsoleKind::Gba));
        assert_eq!(console_of("roms/Games.zip"), None);
    }
}
