use crate::app::big_screen::Cover;
use crate::app::ui_state::library::{FileState, Library, LibraryGame};
use crate::app::ui_state::settings::kind_label;
use crate::app::widgets::game_tile::Progress;
use crate::game_facts::{Ask, Facts, Finder, how_long_to_beat_url, metacritic_url};
use crate::icons;

pub const CREDIT: &str = "Facts from libretro and Wikidata";
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub struct GameCard {
    pub path: String,
    pub name: String,
    pub console: Option<String>,
    pub cover: Option<Cover>,
    pub playable: bool,
    pub archive: Option<ArchiveCard>,
    pub tags: Vec<String>,
    pub genres: Vec<String>,
    pub achievements: Option<String>,
    pub about: Vec<(&'static str, String)>,
    pub links: Vec<(&'static str, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveCard {
    Remote { bytes: u64 },
    Downloading { done: u64, total: u64 },
    Downloaded { bytes: u64 },
    Offline,
}

impl GameCard {
    pub fn about_rows(&self) -> Vec<(&'static str, &str)> {
        self.achievements
            .iter()
            .map(|count| ("Achievements", count.as_str()))
            .chain(
                self.about
                    .iter()
                    .map(|(name, value)| (*name, value.as_str())),
            )
            .collect()
    }

    pub fn has_facts(&self) -> bool {
        !self.about.is_empty() || !self.genres.is_empty()
    }
}

pub fn ask(game: &LibraryGame) -> Option<Ask> {
    Some(Ask {
        rom_key: game.rom_key.clone()?,
        name: game.name.clone(),
        console: game.console,
        crc32: game.crc32,
    })
}

pub fn card(
    game: &LibraryGame,
    finder: &Finder,
    cover: Option<Cover>,
    progress: Option<Progress>,
    download: Option<(u64, u64)>,
) -> GameCard {
    let info = ask(game).map(|ask| finder.info(&ask)).unwrap_or_default();
    let facts = info.facts;
    let mut about = Vec::new();
    if let Some(date) = released(&facts) {
        about.push(("Released", date));
    }
    if let Some(developer) = facts.developer.clone() {
        about.push(("Developer", developer));
    }
    if let Some(publisher) = facts.publisher.clone() {
        about.push(("Publisher", publisher));
    }
    let mut links = Vec::new();
    if let Some(page) = info.page {
        links.push(("Wikipedia", page));
    }
    links.push(("Reviews on Metacritic", metacritic_url(&game.name)));
    links.push(("Length on HowLongToBeat", how_long_to_beat_url(&game.name)));
    GameCard {
        path: game.path.clone(),
        name: game.name.clone(),
        console: game.console.map(kind_label),
        cover,
        playable: !matches!(game.file, FileState::Missing | FileState::Offline),
        archive: game.archive.as_ref().map(|copy| match download {
            Some((done, total)) => ArchiveCard::Downloading { done, total },
            None if copy.downloaded => ArchiveCard::Downloaded { bytes: copy.size },
            None if game.file == FileState::Offline => ArchiveCard::Offline,
            None => ArchiveCard::Remote {
                bytes: copy.download,
            },
        }),
        tags: game.tags.clone(),
        genres: facts.genre.into_iter().collect(),
        achievements: progress.map(|progress| format!("{} of {}", progress.earned, progress.total)),
        about,
        links,
    }
}

pub fn released(facts: &Facts) -> Option<String> {
    let year = facts.year?;
    let month = facts
        .month
        .and_then(|month| MONTHS.get(usize::from(month).checked_sub(1)?));
    Some(match (facts.day, month) {
        (Some(day), Some(month)) => format!("{day} {month} {year}"),
        (None, Some(month)) => format!("{month} {year}"),
        _ => year.to_string(),
    })
}

pub struct TagRow {
    pub name: String,
    pub count: usize,
    pub new: bool,
}

pub fn tag_rows(library: &Library, path: &str, typed: &str) -> Vec<TagRow> {
    let typed = typed.trim();
    let mut rows: Vec<TagRow> = library
        .suggest_tags(path, typed)
        .into_iter()
        .map(|(name, count)| TagRow {
            name,
            count,
            new: false,
        })
        .collect();
    let known = library
        .tags()
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(typed));
    if !typed.is_empty() && !known {
        rows.push(TagRow {
            name: typed.to_owned(),
            count: 0,
            new: true,
        });
    }
    rows
}

pub fn tag_row_label(row: &TagRow) -> String {
    if row.new {
        format!("New tag \"{}\"", row.name)
    } else {
        row.name.clone()
    }
}

impl ArchiveCard {
    pub fn line(self) -> String {
        let size = crate::progress::size_words;
        match self {
            ArchiveCard::Remote { bytes } => {
                format!(
                    "{} In the Game Archive, {} download",
                    icons::CLOUD_ARROW_DOWN,
                    size(bytes)
                )
            }
            ArchiveCard::Downloading { .. } => format!("{} Downloading", icons::CLOUD_ARROW_DOWN),
            ArchiveCard::Downloaded { bytes } => {
                format!("{} Downloaded, {}", icons::CLOUD_CHECK, size(bytes))
            }
            ArchiveCard::Offline => format!("{} In the Game Archive, offline", icons::CLOUD_SLASH),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ArchiveCard::Remote { .. } => "Downloads when you play it",
            ArchiveCard::Downloading { .. } => "Stays on this device once downloaded",
            ArchiveCard::Downloaded { .. } => "On this device, so it plays offline",
            ArchiveCard::Offline => "Downloads once you're online",
        }
    }
}
