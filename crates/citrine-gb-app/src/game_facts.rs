mod facts;
mod store;
mod wiki;

use crate::covers::System;
use crate::covers::net::{Answer, Net};
use citrine_gb::console::ConsoleKind;
pub use facts::Facts;
use facts::{Field, Sheet};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, TryRecvError};
pub use store::{Found, InfoStore};

const SHEET_FRESH: u64 = 30 * 24 * 60 * 60;
const MISS_WAIT: u64 = 7 * 24 * 60 * 60;
const METADAT: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat";

#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub rom_key: String,
    pub name: String,
    pub console: Option<ConsoleKind>,
    pub crc32: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub facts: Facts,
    pub page: Option<String>,
}

enum Load {
    Loading {
        replies: Vec<(Field, Receiver<Answer>)>,
        sheet: Sheet,
        failed: bool,
        stale: Option<Sheet>,
    },
    Ready(Sheet),
    Failed,
}

enum Step {
    Search(Receiver<Answer>),
    Claims(Receiver<Answer>, wiki::Page),
    Labels(Receiver<Answer>, wiki::Page, wiki::Claims),
}

enum Next {
    Wait(Step),
    Done(Found),
    Drop,
}

pub struct Finder {
    net: Box<dyn Net>,
    sheets: HashMap<System, Load>,
    asks: Vec<(Ask, Step)>,
    found: HashMap<String, Found>,
}

impl Finder {
    pub fn new(net: Box<dyn Net>) -> Self {
        Self {
            net,
            sheets: HashMap::new(),
            asks: Vec::new(),
            found: HashMap::new(),
        }
    }

    pub fn want_facts(&mut self, store: &InfoStore, console: ConsoleKind, now: u64) {
        for &system in systems(console) {
            if self.sheets.contains_key(&system) {
                continue;
            }
            let load = match store.sheet(system) {
                Some((fetched, sheet)) if now.saturating_sub(fetched) < SHEET_FRESH => {
                    Load::Ready(sheet)
                }
                stored => Load::Loading {
                    replies: Field::ALL
                        .iter()
                        .map(|&field| (field, self.net.get(metadat_url(field, system))))
                        .collect(),
                    sheet: Sheet::default(),
                    failed: false,
                    stale: stored.map(|(_, sheet)| sheet),
                },
            };
            self.sheets.insert(system, load);
        }
    }

    pub fn want_page(&mut self, store: &InfoStore, ask: Ask, now: u64) {
        if self
            .asks
            .iter()
            .any(|(asked, _)| asked.rom_key == ask.rom_key)
        {
            return;
        }
        if let Some(console) = ask.console {
            self.want_facts(store, console, now);
        }
        let found = match self.found.get(&ask.rom_key) {
            Some(found) => Some(found.clone()),
            None => store.found(&ask.rom_key),
        };
        if let Some(found) = found {
            let fresh = found.page.is_some() || now.saturating_sub(found.fetched) < MISS_WAIT;
            self.found.insert(ask.rom_key.clone(), found);
            if fresh {
                return;
            }
        }
        let reply = self.net.get(wiki::search_url(&ask.name));
        self.asks.push((ask, Step::Search(reply)));
    }

    pub fn is_busy(&self) -> bool {
        !self.asks.is_empty()
            || self
                .sheets
                .values()
                .any(|load| matches!(load, Load::Loading { .. }))
    }

    pub fn poll(&mut self, store: &InfoStore, now: u64) -> bool {
        let mut changed = self.poll_sheets(store, now);
        for (ask, step) in std::mem::take(&mut self.asks) {
            match self.advance(&ask, step) {
                Next::Wait(step) => self.asks.push((ask, step)),
                Next::Drop => {
                    let missed = Found {
                        fetched: now,
                        ..Found::default()
                    };
                    self.found.insert(ask.rom_key, missed);
                }
                Next::Done(found) => {
                    let found = Found {
                        fetched: now,
                        ..found
                    };
                    if let Err(err) = store.put_found(&ask.rom_key, &found) {
                        log::warn!("could not keep game info: {err}");
                    }
                    self.found.insert(ask.rom_key, found);
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn facts(&self, console: Option<ConsoleKind>, crc32: Option<u32>) -> Option<&Facts> {
        let crc32 = crc32?;
        systems(console?)
            .iter()
            .find_map(|system| match self.sheets.get(system) {
                Some(Load::Ready(sheet)) => sheet.get(crc32),
                _ => None,
            })
    }

    pub fn info(&self, ask: &Ask) -> Info {
        let mut facts = self
            .facts(ask.console, ask.crc32)
            .cloned()
            .unwrap_or_default();
        let found = self.found.get(&ask.rom_key);
        if let Some(found) = found {
            facts.fill(&found.facts);
        }
        Info {
            facts,
            page: found
                .and_then(|found| found.page.as_ref())
                .map(|page| page.url.clone()),
        }
    }

    fn poll_sheets(&mut self, store: &InfoStore, now: u64) -> bool {
        let mut changed = false;
        for (system, load) in &mut self.sheets {
            let Load::Loading {
                replies,
                sheet,
                failed,
                stale,
            } = load
            else {
                continue;
            };
            replies.retain(|(field, reply)| match reply.try_recv() {
                Ok(Ok(dat)) => {
                    sheet.read(*field, &String::from_utf8_lossy(&dat));
                    false
                }
                Ok(Err(err)) if err.contains("404") => false,
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    *failed = true;
                    false
                }
                Err(TryRecvError::Empty) => true,
            });
            if !replies.is_empty() {
                continue;
            }
            *load = if !*failed && !sheet.is_empty() {
                if let Err(err) = store.put_sheet(*system, now, sheet) {
                    log::warn!("could not keep game facts: {err}");
                }
                Load::Ready(std::mem::take(sheet))
            } else if let Some(stale) = stale.take() {
                Load::Ready(stale)
            } else {
                Load::Failed
            };
            changed = true;
        }
        changed
    }

    fn advance(&mut self, ask: &Ask, step: Step) -> Next {
        match step {
            Step::Search(reply) => match answer(reply) {
                Reply::Waiting(reply) => Next::Wait(Step::Search(reply)),
                Reply::Failed => Next::Drop,
                Reply::Got(body) => match wiki::pick(&body) {
                    None => Next::Done(Found::default()),
                    Some((key, page)) => {
                        let have = self.facts(ask.console, ask.crc32);
                        if have.is_some_and(Facts::is_whole) {
                            return Next::Done(Found {
                                page: Some(page),
                                ..Found::default()
                            });
                        }
                        Next::Wait(Step::Claims(self.net.get(wiki::claims_url(&key)), page))
                    }
                },
            },
            Step::Claims(reply, page) => match answer(reply) {
                Reply::Waiting(reply) => Next::Wait(Step::Claims(reply, page)),
                Reply::Failed => Next::Drop,
                Reply::Got(body) => {
                    let claims = wiki::claims(&body).unwrap_or_default();
                    let ids = claims.ids();
                    if ids.is_empty() {
                        return Next::Done(Found {
                            page: Some(page),
                            facts: claims.facts(&BTreeMap::new()),
                            ..Found::default()
                        });
                    }
                    let reply = self.net.get(wiki::labels_url(&ids));
                    Next::Wait(Step::Labels(reply, page, claims))
                }
            },
            Step::Labels(reply, page, claims) => match answer(reply) {
                Reply::Waiting(reply) => Next::Wait(Step::Labels(reply, page, claims)),
                Reply::Failed => Next::Drop,
                Reply::Got(body) => Next::Done(Found {
                    page: Some(page),
                    facts: claims.facts(&wiki::labels(&body)),
                    ..Found::default()
                }),
            },
        }
    }
}

enum Reply {
    Waiting(Receiver<Answer>),
    Failed,
    Got(Vec<u8>),
}

fn answer(reply: Receiver<Answer>) -> Reply {
    match reply.try_recv() {
        Ok(Ok(body)) => Reply::Got(body),
        Ok(Err(_)) | Err(TryRecvError::Disconnected) => Reply::Failed,
        Err(TryRecvError::Empty) => Reply::Waiting(reply),
    }
}

impl Default for Finder {
    fn default() -> Self {
        Self::new(Box::new(crate::covers::net::Offline))
    }
}

pub fn systems(console: ConsoleKind) -> &'static [System] {
    match console {
        ConsoleKind::Gba => &[System::Gba],
        ConsoleKind::Nds => &[System::Nds],
        ConsoleKind::Gb | ConsoleKind::Gbc => &[System::Gb, System::Gbc],
    }
}

fn metadat_url(field: Field, system: System) -> String {
    format!(
        "{METADAT}/{}/{}.dat",
        field.key(),
        crate::covers::encode(system.title())
    )
}

pub fn metacritic_url(name: &str) -> String {
    format!(
        "https://www.metacritic.com/search/{}/",
        crate::covers::encode(name)
    )
}

pub fn how_long_to_beat_url(name: &str) -> String {
    format!(
        "https://howlongtobeat.com/?q={}",
        crate::covers::encode(name)
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::covers::net::fake::Fake;

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 24 * 60 * 60;
    const KEY: &str = "4444444444444444444444444444444444444444444444444444444444444444";
    const GBA: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/developer/Nintendo%20-%20Game%20Boy%20Advance.dat";
    const GBC: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/genre/Nintendo%20-%20Game%20Boy%20Color.dat";
    const SEARCH: &str = "https://en.wikipedia.org/w/rest.php/v1/search/title?q=Castlevania%3A%20Aria%20of%20Sorrow&limit=5";
    const CLAIMS: &str = "https://www.wikidata.org/w/api.php?action=wbgetentities&sites=enwiki&titles=Castlevania%3A_Aria_of_Sorrow&props=claims&format=json&origin=*";
    const LABELS: &str = "https://www.wikidata.org/w/api.php?action=wbgetentities&ids=Q2&props=labels&languages=en&format=json&origin=*";

    fn served(entries: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(url, body)| (url.to_string(), body.as_bytes().to_vec()))
            .collect()
    }

    fn settle(
        finder: &mut Finder,
        store: &InfoStore,
        net: &Fake,
        files: &BTreeMap<String, Vec<u8>>,
    ) {
        for _ in 0..8 {
            net.answer(files);
            finder.poll(store, NOW);
        }
    }

    fn castlevania() -> Ask {
        Ask {
            rom_key: KEY.to_owned(),
            name: "Castlevania: Aria of Sorrow".to_owned(),
            console: Some(ConsoleKind::Gba),
            crc32: Some(0x35_53_61_83),
        }
    }

    const DEVELOPER: &str = "game (\n\tcomment \"Castlevania - Aria of Sorrow (USA)\"\n\tdeveloper \"Konami\"\n\trom ( crc 35536183 )\n)\n";
    const PAGES: &str = r#"{"pages":[{"key":"Castlevania:_Aria_of_Sorrow","title":"Castlevania: Aria of Sorrow","description":"2003 video game"}]}"#;
    const ENTITY: &str = r#"{"entities":{"Q1":{"claims":{"P123":[{"mainsnak":{"datavalue":{"value":{"id":"Q2"}}}}],"P577":[{"mainsnak":{"datavalue":{"value":{"time":"+2003-05-06T00:00:00Z","precision":11}}}}]}}}}"#;
    const NAMES: &str = r#"{"entities":{"Q2":{"labels":{"en":{"value":"Konami"}}}}}"#;

    #[test]
    fn libretro_facts_come_first_and_wikidata_fills_the_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let store = InfoStore::at(dir.path().to_path_buf());
        let net = Fake::default();
        let mut finder = Finder::new(Box::new(net.clone()));
        finder.want_page(&store, castlevania(), NOW);
        let files = served(&[
            (GBA, DEVELOPER),
            (SEARCH, PAGES),
            (CLAIMS, ENTITY),
            (LABELS, NAMES),
        ]);
        settle(&mut finder, &store, &net, &files);
        assert!(!finder.is_busy());
        let info = finder.info(&castlevania());
        assert_eq!(info.facts.developer.as_deref(), Some("Konami"));
        assert_eq!(info.facts.publisher.as_deref(), Some("Konami"));
        assert_eq!((info.facts.year, info.facts.day), (Some(2003), Some(6)));
        assert_eq!(
            info.page.as_deref(),
            Some("https://en.wikipedia.org/wiki/Castlevania%3A_Aria_of_Sorrow")
        );

        let net = Fake::default();
        let mut again = Finder::new(Box::new(net.clone()));
        again.want_page(&store, castlevania(), NOW + DAY);
        assert!(
            net.urls().iter().all(|url| !url.contains("wiki")),
            "a kept page is never asked for again"
        );
        assert_eq!(again.info(&castlevania()).page, info.page);
    }

    #[test]
    fn a_game_boy_game_finds_its_facts_in_the_color_list_too() {
        let dir = tempfile::tempdir().unwrap();
        let store = InfoStore::at(dir.path().to_path_buf());
        let net = Fake::default();
        let mut finder = Finder::new(Box::new(net.clone()));
        finder.want_facts(&store, ConsoleKind::Gb, NOW);
        assert_eq!(net.urls().len(), 10);
        let genre = "game (\n\tcomment \"Tetris DX (World)\"\n\tgenre \"Puzzle\"\n\trom ( crc 6E30394C )\n)\n";
        settle(&mut finder, &store, &net, &served(&[(GBC, genre)]));
        let facts = finder
            .facts(Some(ConsoleKind::Gb), Some(0x6E30_394C))
            .unwrap();
        assert_eq!(facts.genre.as_deref(), Some("Puzzle"));
    }

    #[test]
    fn a_game_wikipedia_lacks_is_tried_again_after_a_week() {
        let dir = tempfile::tempdir().unwrap();
        let store = InfoStore::at(dir.path().to_path_buf());
        let net = Fake::default();
        let mut finder = Finder::new(Box::new(net.clone()));
        finder.want_page(&store, castlevania(), NOW);
        settle(
            &mut finder,
            &store,
            &net,
            &served(&[(SEARCH, r#"{"pages":[]}"#)]),
        );
        assert_eq!(finder.info(&castlevania()).page, None);

        let net = Fake::default();
        let mut later = Finder::new(Box::new(net.clone()));
        later.want_page(&store, castlevania(), NOW + DAY);
        assert!(!net.urls().contains(&SEARCH.to_owned()));
        later.want_page(&store, castlevania(), NOW + 8 * DAY);
        assert!(net.urls().contains(&SEARCH.to_owned()));
    }

    #[test]
    fn a_failed_download_keeps_nothing_and_asks_again_on_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let store = InfoStore::at(dir.path().to_path_buf());
        let net = Fake::default();
        let mut finder = Finder::new(Box::new(net.clone()));
        finder.want_page(&store, castlevania(), NOW);
        net.fail("offline");
        finder.poll(&store, NOW);
        assert_eq!(store.found(KEY), None);
        assert_eq!(store.sheet(System::Gba), None);
        let asked = net.urls().len();
        finder.want_facts(&store, ConsoleKind::Gba, NOW);
        finder.want_page(&store, castlevania(), NOW);
        assert_eq!(net.urls().len(), asked);

        let net = Fake::default();
        let mut finder = Finder::new(Box::new(net.clone()));
        finder.want_page(&store, castlevania(), NOW);
        assert!(net.urls().contains(&SEARCH.to_owned()));
    }

    #[test]
    fn search_links_carry_the_name() {
        assert_eq!(
            metacritic_url("Dr. Mario"),
            "https://www.metacritic.com/search/Dr.%20Mario/"
        );
        assert_eq!(
            how_long_to_beat_url("Pokémon Red"),
            "https://howlongtobeat.com/?q=Pok%C3%A9mon%20Red"
        );
    }
}
