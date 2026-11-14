use citrine_gb::console::ConsoleKind;
use std::collections::{BTreeMap, BTreeSet};

pub mod scan;

const SUGGESTIONS: usize = 6;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LibraryGame {
    pub path: String,
    pub name: String,
    pub console: Option<ConsoleKind>,
    pub rom_key: Option<String>,
    pub sha1: Option<String>,
    pub crc32: Option<u32>,
    pub ra_hash: Option<String>,
    pub opened: bool,
    pub folders: BTreeSet<String>,
    pub added: u64,
    pub played: u64,
    pub tags: Vec<String>,
    #[serde(skip)]
    pub genre: Option<String>,
    #[serde(skip)]
    pub file: FileState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileState {
    #[default]
    Unchecked,
    Present,
    Missing,
    Offline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    #[default]
    Unchecked,
    Readable,
    Unreadable,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LibraryFolder {
    pub tree: String,
    #[serde(skip)]
    pub access: Access,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct OldFolder {
    pub tree: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileFound {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opened {
    pub path: String,
    pub name: String,
    pub console: Option<ConsoleKind>,
    pub rom_key: Option<String>,
    pub sha1: Option<String>,
    pub crc32: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub rom_key: String,
    pub sha1: String,
    pub crc32: Option<u32>,
    pub ra_hash: Option<String>,
    pub console: Option<ConsoleKind>,
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Sort {
    #[default]
    Name,
    Played,
    Added,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Place {
    #[default]
    All,
    Folder(String),
    Opened,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TagPick {
    Own(String),
    Genre(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Filter {
    pub consoles: Vec<ConsoleKind>,
    pub place: Place,
    pub tag: Option<TagPick>,
    pub sort: Sort,
    #[serde(skip)]
    pub missing: bool,
}

impl Filter {
    pub fn narrows(&self) -> bool {
        !self.consoles.is_empty() || self.place != Place::All || self.tag.is_some()
    }

    pub fn toggle_console(&mut self, console: ConsoleKind) {
        match self.consoles.iter().position(|kept| *kept == console) {
            Some(at) => {
                self.consoles.remove(at);
            }
            None => self.consoles.push(console),
        }
    }

    fn keeps(&self, game: &LibraryGame) -> bool {
        let console = self.consoles.is_empty()
            || game
                .console
                .is_some_and(|console| self.consoles.contains(&console));
        let place = match &self.place {
            Place::All => true,
            Place::Folder(tree) => game.folders.contains(tree),
            Place::Opened => game.opened,
        };
        let tag = match &self.tag {
            None => true,
            Some(TagPick::Own(tag)) => game.tags.iter().any(|kept| kept.eq_ignore_ascii_case(tag)),
            Some(TagPick::Genre(genre)) => game.genre.as_ref() == Some(genre),
        };
        let missing = !self.missing || game.file == FileState::Missing;
        console && place && tag && missing
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Library {
    folders: Vec<LibraryFolder>,
    games: Vec<LibraryGame>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    removed: BTreeSet<String>,
    seeded: bool,
    #[serde(skip)]
    listed: BTreeMap<String, Option<BTreeSet<String>>>,
    #[serde(skip)]
    checked: BTreeMap<String, FileState>,
    #[serde(skip)]
    scan: scan::Scan,
}

impl Library {
    pub fn folders(&self) -> &[LibraryFolder] {
        &self.folders
    }

    pub fn folder_games(&self, tree: &str) -> usize {
        self.entries()
            .iter()
            .filter(|game| game.folders.contains(tree))
            .count()
    }

    pub fn needs_access(&self) -> bool {
        self.folders
            .iter()
            .any(|folder| folder.access == Access::Unreadable)
    }

    pub fn add_folder(&mut self, tree: &str) -> bool {
        if self.folders.iter().any(|folder| folder.tree == tree) {
            return false;
        }
        self.folders.push(LibraryFolder {
            tree: tree.to_owned(),
            access: Access::Unchecked,
        });
        true
    }

    pub fn remove_folder(&mut self, tree: &str) -> Vec<LibraryGame> {
        self.folders.retain(|folder| folder.tree != tree);
        self.listed.remove(tree);
        let (taken, mut kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.games)
            .into_iter()
            .partition(|game| !game.opened && game.folders.iter().all(|folder| folder == tree));
        for game in &mut kept {
            game.folders.remove(tree);
        }
        self.games = kept;
        self.settle();
        taken
    }

    pub fn seed(&mut self, tree: Option<&str>, opened: impl IntoIterator<Item = (Opened, u64)>) {
        if std::mem::replace(&mut self.seeded, true) {
            return;
        }
        if let Some(tree) = tree {
            self.add_folder(tree);
        }
        for (game, played) in opened {
            self.record_opened(game, played);
        }
    }

    pub fn record_opened(&mut self, opened: Opened, now: u64) {
        self.removed.remove(&opened.path);
        let game = self.game_mut(&opened.path, &opened.name, now);
        game.opened = true;
        game.played = game.played.max(now);
        if !opened.name.trim().is_empty() {
            game.name = opened.name.trim().to_owned();
        }
        if opened.console.is_some() {
            game.console = opened.console;
        }
        if opened.rom_key.is_some() {
            game.rom_key = opened.rom_key;
        }
        if opened.sha1.is_some() {
            game.sha1 = opened.sha1;
        }
        if opened.crc32.is_some() {
            game.crc32 = opened.crc32;
        }
        self.share_tags(&opened.path);
        self.checked.insert(opened.path, FileState::Present);
        self.settle();
    }

    pub fn identify(&mut self, path: &str, identity: Identity) {
        if let Some(game) = self.games.iter_mut().find(|game| game.path == path) {
            game.rom_key = Some(identity.rom_key);
            game.sha1 = Some(identity.sha1);
            game.crc32 = identity.crc32.or(game.crc32);
            game.ra_hash = identity.ra_hash.or(game.ra_hash.take());
            if identity.console.is_some() {
                game.console = identity.console;
            }
            if let Some(name) = identity.name.filter(|name| !name.trim().is_empty()) {
                game.name = name;
            }
        }
        self.share_tags(path);
    }

    pub fn unidentified(&self) -> impl Iterator<Item = &LibraryGame> {
        self.games
            .iter()
            .filter(|game| game.file == FileState::Present)
            .filter(|game| {
                game.rom_key.is_none()
                    || (game.ra_hash.is_none()
                        && crate::achievements::progress::wants_hash(game.console))
            })
    }

    pub fn apply_listing(&mut self, tree: &str, files: Option<Vec<FileFound>>, now: u64) {
        let Some(folder) = self.folders.iter_mut().find(|folder| folder.tree == tree) else {
            return;
        };
        folder.access = if files.is_some() {
            Access::Readable
        } else {
            Access::Unreadable
        };
        let paths = files.map(|files| {
            let mut paths = BTreeSet::new();
            for file in files {
                paths.insert(file.path.clone());
                if self.removed.contains(&file.path) {
                    continue;
                }
                let game = self.game_mut(&file.path, &file.name, now);
                game.folders.insert(tree.to_owned());
                if game.console.is_none() {
                    game.console = scan::console_of(&file.path);
                }
            }
            paths
        });
        self.listed.insert(tree.to_owned(), paths);
        self.settle();
    }

    pub fn loose_files(&self) -> Vec<String> {
        self.games
            .iter()
            .filter(|game| game.folders.is_empty())
            .map(|game| game.path.clone())
            .collect()
    }

    pub fn apply_check(&mut self, path: &str, file: FileState) {
        self.apply_checks([(path.to_owned(), file)]);
    }

    pub fn apply_checks(&mut self, checks: impl IntoIterator<Item = (String, FileState)>) {
        let mut changed = false;
        for (path, file) in checks {
            changed |= self.checked.insert(path, file) != Some(file);
        }
        if changed {
            self.settle();
        }
    }

    pub fn find(&self, path: &str) -> Option<&LibraryGame> {
        self.games.iter().find(|game| game.path == path)
    }

    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    pub fn entries(&self) -> Vec<&LibraryGame> {
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        let mut entries: Vec<&LibraryGame> = Vec::new();
        for game in &self.games {
            let Some(sha1) = game.sha1.as_deref() else {
                entries.push(game);
                continue;
            };
            match seen.get(sha1) {
                Some(&index) if rank(game.file) > rank(entries[index].file) => {
                    entries[index] = game;
                }
                Some(_) => {}
                None => {
                    seen.insert(sha1, entries.len());
                    entries.push(game);
                }
            }
        }
        entries
    }

    pub fn shown(&self, filter: &Filter, search: &str) -> Vec<&LibraryGame> {
        let search = search.trim().to_lowercase();
        let mut shown: Vec<&LibraryGame> = self
            .entries()
            .into_iter()
            .filter(|game| filter.keeps(game))
            .filter(|game| search.is_empty() || game.name.to_lowercase().contains(&search))
            .collect();
        match filter.sort {
            Sort::Name => shown.sort_by_cached_key(|game| game.name.to_lowercase()),
            Sort::Played => shown.sort_by_key(|game| std::cmp::Reverse(game.played)),
            Sort::Added => shown.sort_by_key(|game| std::cmp::Reverse(game.added)),
        }
        shown
    }

    pub fn missing_count(&self) -> usize {
        self.entries()
            .iter()
            .filter(|game| game.file == FileState::Missing)
            .count()
    }

    pub fn remove_game(&mut self, path: &str) -> Vec<LibraryGame> {
        let sha1 = self
            .games
            .iter()
            .find(|game| game.path == path)
            .and_then(|game| game.sha1.clone());
        self.take(|game| game.path == path || sha1.is_some() && game.sha1 == sha1)
    }

    pub fn remove_missing(&mut self) -> Vec<LibraryGame> {
        let gone: BTreeSet<String> = self
            .entries()
            .iter()
            .filter(|game| game.file == FileState::Missing)
            .map(|game| game.sha1.clone().unwrap_or_else(|| game.path.clone()))
            .collect();
        self.take(|game| {
            gone.contains(game.sha1.as_ref().unwrap_or(&game.path))
                && game.file == FileState::Missing
        })
    }

    pub fn restore(&mut self, games: Vec<LibraryGame>) {
        for game in games {
            self.removed.remove(&game.path);
            if !self.games.iter().any(|kept| kept.path == game.path) {
                self.games.push(game);
            }
        }
        self.settle();
    }

    fn take(&mut self, gone: impl Fn(&LibraryGame) -> bool) -> Vec<LibraryGame> {
        let (taken, kept) = std::mem::take(&mut self.games).into_iter().partition(gone);
        self.games = kept;
        for game in &taken {
            self.removed.insert(game.path.clone());
        }
        taken
    }

    pub fn add_tag(&mut self, path: &str, tag: &str) -> bool {
        let tag = tag.trim();
        if tag.is_empty() {
            return false;
        }
        let tag = self
            .tags()
            .into_iter()
            .map(|(known, _)| known)
            .find(|known| known.eq_ignore_ascii_case(tag))
            .unwrap_or_else(|| tag.to_owned());
        let mut added = false;
        for game in self.copies_mut(path) {
            if !game.tags.iter().any(|kept| kept.eq_ignore_ascii_case(&tag)) {
                game.tags.push(tag.clone());
                added = true;
            }
        }
        added
    }

    pub fn remove_tag(&mut self, path: &str, tag: &str) {
        for game in self.copies_mut(path) {
            game.tags.retain(|kept| !kept.eq_ignore_ascii_case(tag));
        }
    }

    pub fn tags(&self) -> Vec<(String, usize)> {
        count(self.entries().into_iter().flat_map(|game| game.tags.iter()))
    }

    pub fn genres(&self) -> Vec<(String, usize)> {
        count(
            self.entries()
                .into_iter()
                .filter_map(|game| game.genre.as_ref()),
        )
    }

    pub fn suggest_tags(&self, path: &str, typed: &str) -> Vec<(String, usize)> {
        let typed = typed.trim().to_lowercase();
        let has = self
            .find(path)
            .map(|game| game.tags.clone())
            .unwrap_or_default();
        self.tags()
            .into_iter()
            .filter(|(tag, _)| tag.to_lowercase().contains(&typed))
            .filter(|(tag, _)| !has.iter().any(|kept| kept.eq_ignore_ascii_case(tag)))
            .take(SUGGESTIONS)
            .collect()
    }

    pub fn set_genres<'a>(&mut self, genre_of: impl Fn(&LibraryGame) -> Option<&'a str>) -> bool {
        let mut changed = false;
        for game in &mut self.games {
            let genre = genre_of(game);
            if game.genre.as_deref() != genre {
                game.genre = genre.map(str::to_owned);
                changed = true;
            }
        }
        changed
    }

    pub fn consoles(&self) -> Vec<ConsoleKind> {
        let mut consoles = Vec::new();
        for console in self.games.iter().filter_map(|game| game.console) {
            if !consoles.contains(&console) {
                consoles.push(console);
            }
        }
        consoles
    }

    fn copies_mut(&mut self, path: &str) -> impl Iterator<Item = &mut LibraryGame> {
        let sha1 = self
            .games
            .iter()
            .find(|game| game.path == path)
            .and_then(|game| game.sha1.clone());
        self.games
            .iter_mut()
            .filter(move |game| game.path == path || sha1.is_some() && game.sha1 == sha1)
    }

    fn share_tags(&mut self, path: &str) {
        let mut tags: Vec<String> = Vec::new();
        for game in self.copies_mut(path) {
            for tag in &game.tags {
                if !tags.iter().any(|kept| kept.eq_ignore_ascii_case(tag)) {
                    tags.push(tag.clone());
                }
            }
        }
        for game in self.copies_mut(path) {
            game.tags.clone_from(&tags);
        }
    }

    fn game_mut(&mut self, path: &str, name: &str, now: u64) -> &mut LibraryGame {
        let index = match self.games.iter().position(|game| game.path == path) {
            Some(index) => index,
            None => {
                self.games.push(LibraryGame {
                    path: path.to_owned(),
                    name: name.trim().to_owned(),
                    added: now,
                    ..LibraryGame::default()
                });
                self.games.len() - 1
            }
        };
        &mut self.games[index]
    }

    fn settle(&mut self) {
        for game in &mut self.games {
            game.file = file_state(game, &self.listed, &self.checked);
        }
    }
}

fn file_state(
    game: &LibraryGame,
    listed: &BTreeMap<String, Option<BTreeSet<String>>>,
    checked: &BTreeMap<String, FileState>,
) -> FileState {
    let listings: Vec<Option<&BTreeSet<String>>> = game
        .folders
        .iter()
        .filter_map(|tree| listed.get(tree).map(Option::as_ref))
        .collect();
    if listings
        .iter()
        .flatten()
        .any(|paths| paths.contains(&game.path))
    {
        return FileState::Present;
    }
    if listings.iter().any(Option::is_some) {
        return FileState::Missing;
    }
    if let Some(&file) = checked.get(&game.path) {
        return file;
    }
    if listings.is_empty() {
        FileState::Unchecked
    } else {
        FileState::Offline
    }
}

fn count<'a>(names: impl Iterator<Item = &'a String>) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name.clone()).or_default() += 1;
    }
    let mut counted: Vec<(String, usize)> = counts.into_iter().collect();
    counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counted
}

fn rank(file: FileState) -> u8 {
    match file {
        FileState::Present => 3,
        FileState::Unchecked => 2,
        FileState::Offline => 1,
        FileState::Missing => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, path: &str) -> FileFound {
        FileFound {
            name: name.to_owned(),
            path: path.to_owned(),
        }
    }

    fn opened(path: &str, name: &str, console: Option<ConsoleKind>) -> Opened {
        Opened {
            path: path.to_owned(),
            name: name.to_owned(),
            console,
            ..Opened::default()
        }
    }

    fn identity(sha1: &str) -> Identity {
        Identity {
            rom_key: format!("key-{sha1}"),
            sha1: sha1.to_owned(),
            crc32: None,
            ra_hash: None,
            console: None,
            name: None,
        }
    }

    trait At {
        fn apply_listing_at(&mut self, tree: &str, files: Option<Vec<FileFound>>);
    }

    impl At for Library {
        fn apply_listing_at(&mut self, tree: &str, files: Option<Vec<FileFound>>) {
            self.apply_listing(tree, files, 1);
        }
    }

    fn names(library: &Library) -> Vec<&str> {
        library
            .entries()
            .into_iter()
            .map(|game| game.name.as_str())
            .collect()
    }

    fn state_of(library: &Library, path: &str) -> FileState {
        library
            .entries()
            .into_iter()
            .find(|game| game.path == path)
            .map(|game| game.file)
            .unwrap()
    }

    #[test]
    fn a_game_opened_once_survives_a_restart() {
        let mut library = Library::default();
        library.record_opened(
            opened("/roms/tetris.gb", "Tetris", Some(ConsoleKind::Gb)),
            1,
        );
        library.identify("/roms/tetris.gb", identity("ab12"));
        let saved = serde_json::to_string(&library).unwrap();
        let back: Library = serde_json::from_str(&saved).unwrap();
        let game = back.entries()[0];
        assert_eq!(game.name, "Tetris");
        assert_eq!(game.sha1.as_deref(), Some("ab12"));
        assert!(game.opened);
    }

    #[test]
    fn removing_a_folder_keeps_games_opened_from_it() {
        let mut library = Library::default();
        library.add_folder("/roms");
        library.apply_listing_at(
            "/roms",
            Some(vec![
                found("Tetris", "/roms/tetris.gb"),
                found("Pong", "/roms/pong.gb"),
            ]),
        );
        library.record_opened(opened("/roms/pong.gb", "Pong", None), 1);
        library.record_opened(opened("/elsewhere/zelda.gb", "Zelda", None), 1);
        library.remove_folder("/roms");
        assert_eq!(names(&library), ["Pong", "Zelda"]);
        assert!(library.folders().is_empty());
    }

    #[test]
    fn a_file_gone_from_a_readable_folder_is_tagged_missing() {
        let mut library = Library::default();
        library.add_folder("/roms");
        library.apply_listing_at("/roms", Some(vec![found("Tetris", "/roms/tetris.gb")]));
        assert_eq!(state_of(&library, "/roms/tetris.gb"), FileState::Present);
        library.apply_listing_at("/roms", Some(vec![]));
        assert_eq!(state_of(&library, "/roms/tetris.gb"), FileState::Missing);
        assert_eq!(library.missing_count(), 1);
        library.apply_listing_at("/roms", Some(vec![found("Tetris", "/roms/tetris.gb")]));
        assert_eq!(state_of(&library, "/roms/tetris.gb"), FileState::Present);
    }

    #[test]
    fn an_opened_file_deleted_later_is_missing_on_the_next_scan() {
        let mut library = Library::default();
        library.add_folder("/roms");
        library.apply_listing_at("/roms", Some(vec![found("Tetris", "/roms/tetris.gb")]));
        library.record_opened(opened("/roms/tetris.gb", "Tetris", None), 1);
        library.apply_listing_at("/roms", Some(vec![]));
        assert_eq!(state_of(&library, "/roms/tetris.gb"), FileState::Missing);
    }

    #[test]
    fn an_unreadable_folder_tags_its_games_offline_and_not_missing() {
        let mut library = Library::default();
        library.add_folder("/mnt/ssd");
        library.apply_listing_at(
            "/mnt/ssd",
            Some(vec![found("Tetris", "/mnt/ssd/tetris.gb")]),
        );
        library.apply_listing_at("/mnt/ssd", None);
        assert_eq!(state_of(&library, "/mnt/ssd/tetris.gb"), FileState::Offline);
        assert_eq!(library.folders()[0].access, Access::Unreadable);
        assert_eq!(library.missing_count(), 0);
        assert!(library.remove_missing().is_empty());
    }

    #[test]
    fn a_loose_file_takes_its_state_from_the_check() {
        let mut library = Library::default();
        library.record_opened(opened("/usb/tetris.gb", "Tetris", None), 1);
        library.apply_check("/usb/tetris.gb", FileState::Offline);
        assert_eq!(state_of(&library, "/usb/tetris.gb"), FileState::Offline);
        library.apply_check("/usb/tetris.gb", FileState::Missing);
        assert_eq!(library.missing_count(), 1);
    }

    #[test]
    fn the_same_file_through_two_folders_is_listed_once() {
        let mut library = Library::default();
        library.add_folder("/roms");
        library.add_folder("/roms/gb");
        library.apply_listing_at("/roms", Some(vec![found("Tetris", "/roms/gb/tetris.gb")]));
        library.apply_listing_at(
            "/roms/gb",
            Some(vec![found("Tetris", "/roms/gb/tetris.gb")]),
        );
        assert_eq!(names(&library), ["Tetris"]);
        library.remove_folder("/roms");
        assert_eq!(names(&library), ["Tetris"]);
    }

    #[test]
    fn copies_of_one_game_in_two_folders_are_listed_once() {
        let mut library = Library::default();
        library.add_folder("/a");
        library.add_folder("/b");
        library.apply_listing_at("/a", Some(vec![found("Tetris", "/a/tetris.gb")]));
        library.apply_listing_at("/b", Some(vec![found("Tetris (copy)", "/b/tetris.gb")]));
        library.identify("/a/tetris.gb", identity("ab12"));
        library.identify("/b/tetris.gb", identity("ab12"));
        assert_eq!(library.entries().len(), 1);
        library.apply_listing_at("/a", Some(vec![]));
        assert_eq!(library.entries()[0].path, "/b/tetris.gb");
        assert_eq!(library.missing_count(), 0);
    }

    #[test]
    fn remove_missing_can_be_undone() {
        let mut library = Library::default();
        library.add_folder("/roms");
        library.apply_listing_at(
            "/roms",
            Some(vec![
                found("Tetris", "/roms/tetris.gb"),
                found("Pong", "/roms/pong.gb"),
            ]),
        );
        library.apply_listing_at("/roms", Some(vec![found("Pong", "/roms/pong.gb")]));
        let removed = library.remove_missing();
        assert_eq!(removed.len(), 1);
        assert_eq!(names(&library), ["Pong"]);
        library.restore(removed);
        assert_eq!(names(&library), ["Pong", "Tetris"]);
        assert_eq!(state_of(&library, "/roms/tetris.gb"), FileState::Missing);
    }

    #[test]
    fn a_removed_game_stays_out_until_opened_again() {
        let mut library = Library::default();
        library.add_folder("/roms");
        let listing = || Some(vec![found("Tetris", "/roms/tetris.gb")]);
        library.apply_listing_at("/roms", listing());
        library.remove_game("/roms/tetris.gb");
        library.apply_listing_at("/roms", listing());
        assert!(library.entries().is_empty());
        library.record_opened(opened("/roms/tetris.gb", "Tetris", None), 1);
        library.apply_listing_at("/roms", listing());
        assert_eq!(names(&library), ["Tetris"]);
    }

    #[test]
    fn a_game_opened_once_survives_recent_pruning() {
        let mut ui = crate::app::ui_state::UiState::default();
        let path = std::path::Path::new("/nowhere/tetris.gb");
        ui.recent.record(path, "Tetris", None);
        ui.library
            .record_opened(opened("/nowhere/tetris.gb", "Tetris", None), 1);
        assert_eq!(ui.recent.prune(), 1);
        assert!(ui.recent.is_empty());
        assert_eq!(names(&ui.library), ["Tetris"]);
    }

    #[test]
    fn the_first_start_takes_recent_games_and_the_old_folder_once() {
        let mut ui = crate::app::ui_state::UiState::default();
        ui.recent
            .record(std::path::Path::new("/roms/tetris.gb"), "Tetris", None);
        ui.rom_folder.tree = Some("/roms".to_owned());
        ui.seed_library();
        assert_eq!(names(&ui.library), ["Tetris"]);
        assert_eq!(ui.library.folders()[0].tree, "/roms");
        assert!(ui.rom_folder.tree.is_none());
        ui.recent
            .record(std::path::Path::new("/roms/zelda.gb"), "Zelda", None);
        ui.seed_library();
        assert_eq!(names(&ui.library), ["Tetris"]);
    }

    fn shelf() -> Library {
        let mut library = Library::default();
        library.add_folder("/roms");
        library.apply_listing(
            "/roms",
            Some(vec![
                found("Tetris", "/roms/tetris.gb"),
                found("Metroid", "/roms/metroid.gba"),
            ]),
            5,
        );
        library.record_opened(
            opened("/loose/zelda.gbc", "Zelda", Some(ConsoleKind::Gbc)),
            9,
        );
        library
    }

    fn shown_names(library: &Library, filter: &Filter, search: &str) -> Vec<String> {
        library
            .shown(filter, search)
            .into_iter()
            .map(|game| game.name.clone())
            .collect()
    }

    #[test]
    fn the_filter_narrows_by_console_and_place() {
        let library = shelf();
        let all = Filter::default();
        assert_eq!(
            shown_names(&library, &all, ""),
            ["Metroid", "Tetris", "Zelda"]
        );
        let gba = Filter {
            consoles: vec![ConsoleKind::Gba],
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &gba, ""), ["Metroid"]);
        let folder = Filter {
            place: Place::Folder("/roms".to_owned()),
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &folder, ""), ["Metroid", "Tetris"]);
        let opened_only = Filter {
            place: Place::Opened,
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &opened_only, ""), ["Zelda"]);
        assert!(gba.narrows() && folder.narrows() && !all.narrows());
    }

    #[test]
    fn console_picks_add_up_and_toggle_off() {
        let library = shelf();
        let mut filter = Filter::default();
        filter.toggle_console(ConsoleKind::Gba);
        filter.toggle_console(ConsoleKind::Gbc);
        assert_eq!(shown_names(&library, &filter, ""), ["Metroid", "Zelda"]);
        assert!(filter.narrows());
        filter.toggle_console(ConsoleKind::Gba);
        assert_eq!(filter.consoles, [ConsoleKind::Gbc]);
        assert_eq!(shown_names(&library, &filter, ""), ["Zelda"]);
        filter.toggle_console(ConsoleKind::Gbc);
        assert!(!filter.narrows());
        assert_eq!(
            shown_names(&library, &filter, ""),
            ["Metroid", "Tetris", "Zelda"]
        );
    }

    #[test]
    fn search_and_sort_order_the_shelf() {
        let library = shelf();
        assert_eq!(
            shown_names(&library, &Filter::default(), " TET "),
            ["Tetris"]
        );
        let played = Filter {
            sort: Sort::Played,
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &played, "")[0], "Zelda");
    }

    #[test]
    fn show_only_missing_keeps_the_missing_games() {
        let mut library = shelf();
        library.apply_listing_at("/roms", Some(vec![found("Tetris", "/roms/tetris.gb")]));
        let missing = Filter {
            missing: true,
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &missing, ""), ["Metroid"]);
    }

    #[test]
    fn a_tag_goes_on_every_copy_and_keeps_its_first_spelling() {
        let mut library = Library::default();
        library.add_folder("/a");
        library.add_folder("/b");
        library.apply_listing_at("/a", Some(vec![found("Tetris", "/a/tetris.gb")]));
        library.apply_listing_at("/b", Some(vec![found("Tetris", "/b/tetris.gb")]));
        library.identify("/a/tetris.gb", identity("same"));
        library.identify("/b/tetris.gb", identity("same"));
        assert!(library.add_tag("/a/tetris.gb", " Puzzle "));
        assert_eq!(library.find("/b/tetris.gb").unwrap().tags, ["Puzzle"]);
        assert!(!library.add_tag("/b/tetris.gb", "puzzle"));
        library.record_opened(opened("/c/dr.gb", "Dr. Mario", None), 3);
        assert!(library.add_tag("/c/dr.gb", "PUZZLE"));
        assert_eq!(library.find("/c/dr.gb").unwrap().tags, ["Puzzle"]);
        assert!(!library.add_tag("/c/dr.gb", "  "));
    }

    #[test]
    fn a_new_copy_of_a_tagged_game_takes_its_tags() {
        let mut library = Library::default();
        library.add_folder("/a");
        library.apply_listing_at("/a", Some(vec![found("Tetris", "/a/tetris.gb")]));
        library.identify("/a/tetris.gb", identity("same"));
        library.add_tag("/a/tetris.gb", "Puzzle");
        library.add_folder("/b");
        library.apply_listing_at("/b", Some(vec![found("Tetris", "/b/tetris.gb")]));
        library.identify("/b/tetris.gb", identity("same"));
        assert_eq!(library.find("/b/tetris.gb").unwrap().tags, ["Puzzle"]);
    }

    #[test]
    fn removing_a_tag_takes_it_off_every_copy() {
        let mut library = shelf();
        library.add_tag("/roms/tetris.gb", "Puzzle");
        library.remove_tag("/roms/tetris.gb", "puzzle");
        assert!(library.find("/roms/tetris.gb").unwrap().tags.is_empty());
        assert!(library.tags().is_empty());
    }

    #[test]
    fn tags_and_genres_count_each_game_once() {
        let mut library = shelf();
        library.add_tag("/roms/tetris.gb", "Puzzle");
        library.add_tag("/roms/metroid.gba", "Done");
        library.add_tag("/loose/zelda.gbc", "Done");
        assert_eq!(
            library.tags(),
            [("Done".to_owned(), 2), ("Puzzle".to_owned(), 1)]
        );
        let changed = library.set_genres(|game| (game.name != "Zelda").then_some("Action"));
        assert!(changed);
        assert!(!library.set_genres(|game| (game.name != "Zelda").then_some("Action")));
        assert_eq!(library.genres(), [("Action".to_owned(), 2)]);
    }

    #[test]
    fn suggestions_match_what_was_typed_and_skip_tags_the_game_has() {
        let mut library = shelf();
        library.add_tag("/roms/tetris.gb", "Puzzle");
        library.add_tag("/roms/tetris.gb", "Party");
        library.add_tag("/roms/metroid.gba", "Done");
        let names = |typed: &str| -> Vec<String> {
            library
                .suggest_tags("/roms/tetris.gb", typed)
                .into_iter()
                .map(|(tag, _)| tag)
                .collect()
        };
        assert_eq!(names(""), ["Done"]);
        assert!(names("p").is_empty());
        let zelda: Vec<String> = library
            .suggest_tags("/loose/zelda.gbc", "P")
            .into_iter()
            .map(|(tag, _)| tag)
            .collect();
        assert_eq!(zelda, ["Party", "Puzzle"]);
    }

    #[test]
    fn the_filter_narrows_by_own_tag_and_by_genre() {
        let mut library = shelf();
        library.add_tag("/loose/zelda.gbc", "Done");
        library.set_genres(|game| (game.name == "Metroid").then_some("Action"));
        let done = Filter {
            tag: Some(TagPick::Own("Done".to_owned())),
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &done, ""), ["Zelda"]);
        let action = Filter {
            tag: Some(TagPick::Genre("Action".to_owned())),
            ..Filter::default()
        };
        assert_eq!(shown_names(&library, &action, ""), ["Metroid"]);
        assert!(done.narrows() && action.narrows());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_game_boy_game_is_read_again_once_for_its_achievements_hash() {
        let mut library = Library::default();
        for (path, console) in [
            ("/roms/red.gb", ConsoleKind::Gb),
            ("/roms/zelda.gba", ConsoleKind::Gba),
        ] {
            library.record_opened(
                Opened {
                    rom_key: Some(format!("key-{path}")),
                    ..opened(path, path, Some(console))
                },
                1,
            );
        }
        let waiting = |library: &Library| -> Vec<String> {
            library
                .unidentified()
                .map(|game| game.path.clone())
                .collect()
        };
        assert_eq!(waiting(&library), ["/roms/red.gb"]);
        library.identify(
            "/roms/red.gb",
            Identity {
                ra_hash: Some("3d45c1ee9abd5738df46d2bdda8b57dc".to_owned()),
                console: Some(ConsoleKind::Gb),
                ..identity("ab12")
            },
        );
        assert!(waiting(&library).is_empty());
        library.identify("/roms/red.gb", identity("ab12"));
        assert_eq!(
            library.find("/roms/red.gb").unwrap().ra_hash.as_deref(),
            Some("3d45c1ee9abd5738df46d2bdda8b57dc")
        );
    }
}
