use super::{FileFound, FileState, Identity, Library};
use crate::covers::Game;
use crate::utils::file_loader::PickedFile;
use citrine_gb_threads::Spawner;
use std::sync::mpsc::{Receiver, Sender, channel};

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
const FOLDER_DEPTH: usize = 2;
const IDENTIFY_MAX: usize = 64 << 20;

pub struct Listing {
    pub tree: String,
    pub games: Vec<(String, String)>,
    pub readable: bool,
}

type Identified = (String, Option<(Identity, Game)>);
type Checked = Vec<(String, FileState)>;

pub(super) struct Scan {
    picked: (Sender<String>, Receiver<String>),
    lists: (Sender<Listing>, Receiver<Listing>),
    checks: (Sender<Checked>, Receiver<Checked>),
    ids: (Sender<Identified>, Receiver<Identified>),
    identifying: bool,
    started: bool,
    waiting: usize,
    skipped: std::collections::BTreeSet<String>,
}

impl Default for Scan {
    fn default() -> Self {
        Self {
            picked: channel(),
            lists: channel(),
            checks: channel(),
            ids: channel(),
            identifying: false,
            started: false,
            waiting: 0,
            skipped: std::collections::BTreeSet::new(),
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

    pub fn rescan(&mut self, spawner: &Spawner) {
        self.scan.started = true;
        for folder in &self.folders {
            self.scan.waiting += 1;
            list(spawner, &folder.tree, self.scan.lists.0.clone());
        }
        let loose = self.loose_files();
        if cfg!(not(target_arch = "wasm32")) && !loose.is_empty() {
            self.scan.waiting += 1;
            check(spawner, loose, self.scan.checks.0.clone());
        }
    }

    pub fn busy(&self) -> bool {
        self.scan.waiting > 0 || self.scan.identifying
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
            list(spawner, &tree, self.scan.lists.0.clone());
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
                list(spawner, &tree, self.scan.lists.0.clone());
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
        while let Ok((path, found)) = self.scan.ids.1.try_recv() {
            self.scan.identifying = false;
            match found {
                Some((identity, game)) => {
                    if identity.ra_hash.is_none() {
                        self.scan.skipped.insert(path.clone());
                    }
                    self.identify(&path, identity);
                    covers.push(game);
                }
                None => {
                    self.scan.skipped.insert(path);
                }
            }
        }
        let next = if self.scan.identifying {
            None
        } else {
            self.unidentified()
                .find(|game| !self.scan.skipped.contains(&game.path))
                .map(|game| game.path.clone())
        };
        if let Some(path) = next {
            self.scan.identifying = true;
            identify(spawner, path, self.scan.ids.0.clone());
        }
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

fn list(spawner: &Spawner, tree: &str, tx: Sender<Listing>) {
    #[cfg(target_os = "android")]
    {
        let _ = spawner;
        crate::platform::android::files::list_roms(tree, tx);
    }
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    {
        let tree = tree.to_owned();
        spawner.run("library folder scan", move || {
            let _ = tx.send(list_dir(&tree));
        });
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (spawner, tree, tx);
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub fn list_dir(tree: &str) -> Listing {
    let mut games = Vec::new();
    let readable = collect(std::path::Path::new(tree), FOLDER_DEPTH, &mut games).is_ok();
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
    } else if path.parent().is_some_and(std::path::Path::is_dir) {
        FileState::Missing
    } else {
        FileState::Offline
    }
}

fn identify(spawner: &Spawner, path: String, tx: Sender<Identified>) {
    let send = move |path: String, data: Option<&[u8]>, file_name: Option<&str>| {
        let found = data
            .filter(|data| data.len() <= IDENTIFY_MAX)
            .map(|data| identity_of(data, file_name));
        let _ = tx.send((path, found));
    };
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        let (files, rx) = channel();
        crate::platform::android::files::read_rom(&path, files);
        spawner.run("library identify", move || {
            let file = rx.recv().ok();
            send(
                path,
                file.as_ref().map(|file| &*file.data),
                file.as_ref().map(|file| file.name.as_str()),
            );
        });
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    spawner.run("library identify", move || {
        let file_path = std::path::Path::new(&path);
        let small =
            std::fs::metadata(file_path).is_ok_and(|meta| meta.len() <= IDENTIFY_MAX as u64);
        let data = small
            .then(|| crate::utils::zip_games::read_game(file_path).ok())
            .flatten();
        let file_name = file_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        send(path.clone(), data.as_deref(), file_name.as_deref());
    });
    #[cfg(target_arch = "wasm32")]
    {
        let _ = spawner;
        send(path, None, None);
    }
}

fn identity_of(data: &[u8], file_name: Option<&str>) -> (Identity, Game) {
    let id = citrine_gb::rom::id::RomId::new(data);
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
        let listing = list_dir(&dir.path().to_string_lossy());
        assert!(listing.readable);
        let names: Vec<&str> = listing
            .games
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["Adjustris.gb", "Petris.gbc"]);
        assert_eq!(folder_label("/home/clara/roms"), "roms");
        assert!(!list_dir("/no/such/folder").readable);
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
    fn a_tree_document_shows_as_a_phone_storage_path() {
        let uri = "content://com.android.externalstorage.documents/tree/primary%3AGames/document/primary%3AGames%2FGBA%2FAstro%20Boy.gba";
        assert_eq!(shown_path(uri), "Phone storage/Games/GBA/Astro Boy.gba");
        assert_eq!(shown_path("/roms/a.gb"), "/roms/a.gb");
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
