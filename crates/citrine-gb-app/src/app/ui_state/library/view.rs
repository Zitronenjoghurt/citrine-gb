use super::{FileState, Filter, LibraryGame, Sort, rank};
use citrine_gb::console::ConsoleKind;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct View {
    revision: u64,
    sums: OnceCell<Sums>,
    shown: RefCell<Option<Picked>>,
    builds: Cell<usize>,
}

pub(super) struct Sums {
    pub entries: Vec<usize>,
    pub missing: usize,
    pub tags: Vec<(String, usize)>,
    pub genres: Vec<(String, usize)>,
    pub consoles: Vec<ConsoleKind>,
    pub folders: BTreeMap<String, usize>,
    lower: Vec<String>,
}

struct Picked {
    filter: Filter,
    search: String,
    games: Arc<[usize]>,
}

pub struct Shown<'a> {
    games: &'a [LibraryGame],
    picked: Arc<[usize]>,
}

impl<'a> Shown<'a> {
    pub fn len(&self) -> usize {
        self.picked.len()
    }

    pub fn is_empty(&self) -> bool {
        self.picked.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a LibraryGame> + '_ {
        let games = self.games;
        self.picked.iter().map(move |&index| &games[index])
    }
}

impl<'a> IntoIterator for Shown<'a> {
    type Item = &'a LibraryGame;
    type IntoIter = std::vec::IntoIter<&'a LibraryGame>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter().collect::<Vec<_>>().into_iter()
    }
}

impl View {
    pub fn forget(&mut self) {
        self.revision += 1;
        self.sums = OnceCell::new();
        self.shown.get_mut().take();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[cfg(test)]
    pub fn builds(&self) -> usize {
        self.builds.get()
    }

    pub fn sums(&self, games: &[LibraryGame]) -> &Sums {
        self.sums.get_or_init(|| Sums::of(games))
    }

    pub fn shown<'a>(&self, games: &'a [LibraryGame], filter: &Filter, search: &str) -> Shown<'a> {
        let search = search.trim().to_lowercase();
        let mut shown = self.shown.borrow_mut();
        if let Some(picked) = &*shown
            && picked.filter == *filter
            && picked.search == search
        {
            return Shown {
                games,
                picked: picked.games.clone(),
            };
        }
        let picked = pick(games, self.sums(games), filter, &search);
        self.builds.set(self.builds.get() + 1);
        *shown = Some(Picked {
            filter: filter.clone(),
            search,
            games: picked.clone(),
        });
        Shown { games, picked }
    }
}

impl Sums {
    fn of(games: &[LibraryGame]) -> Self {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut entries: Vec<usize> = Vec::new();
        for (index, game) in games.iter().enumerate() {
            let Some(sha1) = game.sha1.as_deref() else {
                entries.push(index);
                continue;
            };
            match seen.get(sha1) {
                Some(&at) if wins(game, &games[entries[at]]) => {
                    entries[at] = index;
                }
                Some(_) => {}
                None => {
                    seen.insert(sha1, entries.len());
                    entries.push(index);
                }
            }
        }
        let listed = || entries.iter().map(|&index| &games[index]);
        let mut folders: BTreeMap<String, usize> = BTreeMap::new();
        for tree in listed().flat_map(|game| game.folders.iter()) {
            *folders.entry(tree.clone()).or_default() += 1;
        }
        let mut consoles = Vec::new();
        for console in games.iter().filter_map(|game| game.console) {
            if !consoles.contains(&console) {
                consoles.push(console);
            }
        }
        Self {
            missing: listed()
                .filter(|game| game.file == FileState::Missing)
                .count(),
            tags: count(listed().flat_map(|game| game.tags.iter())),
            genres: count(listed().filter_map(|game| game.genre.as_ref())),
            lower: listed().map(|game| game.name.to_lowercase()).collect(),
            consoles,
            folders,
            entries,
        }
    }
}

fn wins(game: &LibraryGame, over: &LibraryGame) -> bool {
    (rank(game.file), game.archive.is_none()) > (rank(over.file), over.archive.is_none())
}

fn pick(games: &[LibraryGame], sums: &Sums, filter: &Filter, search: &str) -> Arc<[usize]> {
    let mut picked: Vec<(usize, usize)> = sums
        .entries
        .iter()
        .enumerate()
        .filter(|&(_, &index)| filter.keeps(&games[index]))
        .filter(|&(at, _)| search.is_empty() || sums.lower[at].contains(search))
        .map(|(at, &index)| (at, index))
        .collect();
    match filter.sort {
        Sort::Name => picked.sort_by(|a, b| sums.lower[a.0].cmp(&sums.lower[b.0])),
        Sort::Played => picked.sort_by_key(|&(_, index)| std::cmp::Reverse(games[index].played)),
        Sort::Added => picked.sort_by_key(|&(_, index)| std::cmp::Reverse(games[index].added)),
    }
    picked.into_iter().map(|(_, index)| index).collect()
}

fn count<'a>(names: impl Iterator<Item = &'a String>) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name).or_default() += 1;
    }
    let mut counted: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(name, count)| (name.to_owned(), count))
        .collect();
    counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counted
}
