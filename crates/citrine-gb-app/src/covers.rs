mod art;
mod names;
pub mod net;
mod source;
mod store;

use citrine_gb::console::ConsoleKind;
use names::Names;
pub use names::System;
use net::{Answer, Net};
pub(crate) use source::encode;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
pub use store::{CoverStore, NAMESPACES};

const MISS_WAIT: u64 = 7 * 24 * 60 * 60;
const NAMES_FRESH: u64 = 30 * 24 * 60 * 60;
const MOST_LINKS: u8 = 2;

pub use art::decode;

#[derive(Clone, Debug, PartialEq)]
pub struct Game {
    rom_key: String,
    system: System,
    sha1: [u8; 20],
    serial: Option<[u8; 4]>,
    file_name: Option<String>,
}

impl Game {
    pub fn of(rom_key: &str, console: ConsoleKind, sha1: [u8; 20], rom: &[u8]) -> Self {
        let system = System::of(console, rom);
        let serial = (system == System::Nds)
            .then(|| rom.get(0x0C..0x10)?.try_into().ok())
            .flatten()
            .filter(|code: &[u8; 4]| code.iter().all(u8::is_ascii_alphanumeric));
        Self {
            rom_key: rom_key.to_owned(),
            system,
            sha1,
            serial,
            file_name: None,
        }
    }

    pub fn named(mut self, file_name: &str) -> Self {
        self.file_name = Some(file_name.to_owned());
        self
    }

    fn systems(&self) -> impl Iterator<Item = System> {
        std::iter::once(self.system).chain(self.system.sibling())
    }
}

enum List {
    Loading {
        reply: Receiver<Answer>,
        stale: Option<Names>,
    },
    Ready(Names),
    Failed,
}

enum Step {
    Name,
    Art {
        system: System,
        files: Vec<String>,
        at: usize,
        links: u8,
        reply: Receiver<Answer>,
    },
}

pub struct Finder {
    net: Box<dyn Net>,
    lists: HashMap<System, List>,
    games: Vec<(Game, Step)>,
}

impl Finder {
    pub fn new(net: Box<dyn Net>) -> Self {
        Self {
            net,
            lists: HashMap::new(),
            games: Vec::new(),
        }
    }

    pub fn want(&mut self, store: &CoverStore, game: Game, now: u64) {
        let key = &game.rom_key;
        if store.has_art(key)
            || self.games.iter().any(|(wanted, _)| wanted.rom_key == *key)
            || store
                .missed_at(key)
                .is_some_and(|at| now.saturating_sub(at) < MISS_WAIT)
        {
            return;
        }
        for system in game.systems() {
            self.open_list(store, system, now);
        }
        self.games.push((game, Step::Name));
    }

    pub fn forget_all(&mut self) {
        self.games.clear();
        self.lists.clear();
    }

    #[cfg(test)]
    pub fn is_busy(&self) -> bool {
        !self.games.is_empty()
    }

    pub fn poll(&mut self, store: &CoverStore, now: u64) -> Vec<String> {
        self.poll_lists(store, now);
        let mut found = Vec::new();
        let games = std::mem::take(&mut self.games);
        for (game, step) in games {
            match self.advance(store, &game, step, now) {
                Some(Ok(step)) => self.games.push((game, step)),
                Some(Err(())) => {}
                None => found.push(game.rom_key),
            }
        }
        found
    }

    fn open_list(&mut self, store: &CoverStore, system: System, now: u64) {
        if self.lists.contains_key(&system) {
            return;
        }
        let list = match store.names(system) {
            Some((fetched, names)) if now.saturating_sub(fetched) < NAMES_FRESH => {
                List::Ready(names)
            }
            stored => List::Loading {
                reply: self.net.get(source::names_url(system)),
                stale: stored.map(|(_, names)| names),
            },
        };
        self.lists.insert(system, list);
    }

    fn poll_lists(&mut self, store: &CoverStore, now: u64) {
        for (system, list) in &mut self.lists {
            let List::Loading { reply, stale } = list else {
                continue;
            };
            let answer = match reply.try_recv() {
                Ok(answer) => answer,
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => Err("no answer".to_owned()),
            };
            let fresh = answer
                .ok()
                .map(|dat| Names::parse(&String::from_utf8_lossy(&dat)))
                .filter(|names| !names.is_empty());
            *list = match (fresh, stale.take()) {
                (Some(names), _) => {
                    if let Err(err) = store.put_names(*system, now, &names) {
                        log::warn!("could not keep the cover names: {err}");
                    }
                    List::Ready(names)
                }
                (None, Some(stale)) => List::Ready(stale),
                (None, None) => List::Failed,
            };
        }
    }

    fn advance(
        &mut self,
        store: &CoverStore,
        game: &Game,
        step: Step,
        now: u64,
    ) -> Option<Result<Step, ()>> {
        match step {
            Step::Name => {
                let lists = game
                    .systems()
                    .map(|system| (system, self.lists.get(&system)))
                    .collect::<Vec<_>>();
                if lists
                    .iter()
                    .any(|(_, list)| matches!(list, Some(List::Loading { .. })))
                {
                    return Some(Ok(Step::Name));
                }
                let ready = lists
                    .iter()
                    .filter_map(|(system, list)| match list {
                        Some(List::Ready(names)) => Some((*system, names)),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let found = ready
                    .iter()
                    .find_map(|(system, names)| {
                        Some((*system, names.find(&game.sha1, game.serial)?))
                    })
                    .or_else(|| {
                        let file_name = game.file_name.as_deref()?;
                        ready.iter().find_map(|(system, names)| {
                            Some((*system, names.find_title(file_name)?))
                        })
                    });
                let Some((system, name)) = found else {
                    if ready.len() == lists.len() {
                        missed(store, game, now);
                    }
                    return Some(Err(()));
                };
                let files = source::art_files(name);
                let reply = self.net.get(source::art_url(system, &files[0]));
                Some(Ok(Step::Art {
                    system,
                    files,
                    at: 0,
                    links: 0,
                    reply,
                }))
            }
            Step::Art {
                system,
                files,
                at,
                links,
                reply,
            } => {
                let answer = match reply.try_recv() {
                    Ok(answer) => answer,
                    Err(TryRecvError::Empty) => {
                        return Some(Ok(Step::Art {
                            system,
                            files,
                            at,
                            links,
                            reply,
                        }));
                    }
                    Err(TryRecvError::Disconnected) => return Some(Err(())),
                };
                let body = match answer {
                    Ok(body) => body,
                    Err(err) if err.contains("404") => Vec::new(),
                    Err(_) => return Some(Err(())),
                };
                if let Some(target) = source::link_target(&body).filter(|_| links < MOST_LINKS) {
                    let reply = self.net.get(source::art_url(system, target));
                    return Some(Ok(Step::Art {
                        system,
                        files,
                        at,
                        links: links + 1,
                        reply,
                    }));
                }
                if let Some(png) = art::shrink(&body) {
                    if let Err(err) = store.put_art(&game.rom_key, &png) {
                        log::warn!("could not keep a cover: {err}");
                        return Some(Err(()));
                    }
                    return None;
                }
                let next = at + 1;
                let Some(file) = files.get(next) else {
                    missed(store, game, now);
                    return Some(Err(()));
                };
                let reply = self.net.get(source::art_url(system, file));
                Some(Ok(Step::Art {
                    system,
                    files,
                    at: next,
                    links: 0,
                    reply,
                }))
            }
        }
    }
}

impl Default for Finder {
    fn default() -> Self {
        Self::new(Box::new(net::Offline))
    }
}

fn missed(store: &CoverStore, game: &Game, now: u64) {
    if let Err(err) = store.put_miss(&game.rom_key, now) {
        log::warn!("could not note a missing cover: {err}");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use net::fake::Fake;
    use std::collections::BTreeMap;

    const DAY: u64 = 24 * 60 * 60;
    const NOW: u64 = 1_800_000_000;
    const RED: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PLATINUM: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const GB_URL: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy.dat";
    const GBC_URL: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Game%20Boy%20Color.dat";
    const DS_URL: &str = "https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/Nintendo%20-%20Nintendo%20DS.dat";
    const RED_ART: &str = "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy/master/Named_Boxarts/Pokemon%20-%20Red%20Version%20%28USA%2C%20Europe%29%20%28SGB%20Enhanced%29.png";
    const RED_SHORT: &str = "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy/master/Named_Boxarts/Pokemon%20-%20Red%20Version%20%28USA%2C%20Europe%29.png";
    const PLATINUM_ART: &str = "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Nintendo_DS/master/Named_Boxarts/Pokemon%20-%20Platinum%20Version%20%28USA%29.png";
    const PLATINUM_LINKED: &str = "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Nintendo_DS/master/Named_Boxarts/Pokemon%20-%20Platinum%20Version%20%28Europe%29.png";

    const YELLOW: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const YELLOW_ART: &str = "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy/master/Named_Boxarts/Pokemon%20-%20Yellow%20Version%20-%20Special%20Pikachu%20Edition%20%28USA%2C%20Europe%29%20%28CGB%2BSGB%20Enhanced%29.png";
    const YELLOW_DAT: &str = "clrmamepro (\n)\n\ngame (\n\tname \"Pokemon - Yellow Version - Special Pikachu Edition (USA, Europe) (CGB+SGB Enhanced)\"\n\tregion \"USA\"\n\tserial \"APSU\"\n\trom ( name \"y.gb\" size 1048576 crc 7D527D62 md5 D9290DB87B1F0A23B89F99EE4469E34B sha1 CC7D03262EBFAF2F06772C1A480C7D9D5F4A38E1 serial \"APSU\" )\n)\n";
    const ORACLE_ART: &str = "https://raw.githubusercontent.com/libretro-thumbnails/Nintendo_-_Game_Boy_Color/master/Named_Boxarts/Legend%20of%20Zelda%2C%20The%20-%20Oracle%20of%20Ages%20%28USA%2C%20Australia%29.png";
    const GBC_DAT: &str = "clrmamepro (\n)\n\ngame (\n\tname \"Legend of Zelda, The - Oracle of Ages (Europe) (En,Fr,De,Es,It)\"\n\tregion \"Europe\"\n\trom ( name \"e.gbc\" size 1 crc 0 md5 0 sha1 9F2DFF3C46406839F839A8D535353DFC1E3E8670 )\n)\ngame (\n\tname \"Legend of Zelda, The - Oracle of Ages (USA, Australia)\"\n\tregion \"USA\"\n\trom ( name \"u.gbc\" size 1 crc 0 md5 0 sha1 880374FB978B18AF4AA529E2E32F7FFB4D7DD2F4 )\n)\n";

    const GB_DAT: &str = "clrmamepro (\n)\n\ngame (\n\tname \"Pokemon - Red Version (USA, Europe) (SGB Enhanced)\"\n\tregion \"USA\"\n\trom ( name \"r.gb\" size 1 crc 0 md5 0 sha1 EA9BCAE617FDF159B045185467AE58B2E4A48B9A )\n)\n";
    const DS_DAT: &str = "clrmamepro (\n)\n\ngame (\n\tname \"Pokemon - Platinum Version (USA)\"\n\tregion \"USA\"\n\tserial \"CPUE\"\n\trom ( name \"p.nds\" size 1 crc 0 md5 0 sha1 CE81046EDA7D232513069519CB2085349896DEC7 serial \"CPUE\" )\n)\n";

    fn red() -> Game {
        let mut sha1 = [0; 20];
        sha1[..8].copy_from_slice(&[0xEA, 0x9B, 0xCA, 0xE6, 0x17, 0xFD, 0xF1, 0x59]);
        Game::of(RED, ConsoleKind::Gb, sha1, &[0; 0x150])
    }

    fn yellow() -> Game {
        let sha1 = [
            0xCC, 0x7D, 0x03, 0x26, 0x2E, 0xBF, 0xAF, 0x2F, 0x06, 0x77, 0x2C, 0x1A, 0x48, 0x0C,
            0x7D, 0x9D, 0x5F, 0x4A, 0x38, 0xE1,
        ];
        let mut rom = vec![0; 0x150];
        rom[0x134..0x142].copy_from_slice(b"POKEMON YELLOW");
        rom[0x143] = 0x80;
        Game::of(YELLOW, ConsoleKind::Gbc, sha1, &rom)
    }

    fn translated_oracle() -> Game {
        let mut rom = vec![0; 0x150];
        rom[0x143] = 0xC0;
        Game::of(YELLOW, ConsoleKind::Gbc, [0x55; 20], &rom)
            .named("The Legend of Zelda - Oracle of Ages (Translated De).gbc")
    }

    fn trimmed_platinum() -> Game {
        let mut rom = vec![0; 0x200];
        rom[0x0C..0x10].copy_from_slice(b"CPUE");
        Game::of(PLATINUM, ConsoleKind::Nds, [0x77; 20], &rom)
    }

    fn picture() -> Vec<u8> {
        crate::utils::screenshot::encode_png(480, 512, &vec![90; 480 * 512 * 4]).unwrap()
    }

    struct Shelf {
        finder: Finder,
        store: CoverStore,
    }

    impl Shelf {
        fn want(&mut self, game: Game, now: u64) {
            self.finder.want(&self.store, game, now);
        }

        fn poll(&mut self, now: u64) -> Vec<String> {
            self.finder.poll(&self.store, now)
        }

        fn is_busy(&self) -> bool {
            self.finder.is_busy()
        }

        fn store(&self) -> &CoverStore {
            &self.store
        }
    }

    fn finder(dir: &std::path::Path) -> (Shelf, Fake) {
        let net = Fake::default();
        let shelf = Shelf {
            finder: Finder::new(Box::new(net.clone())),
            store: CoverStore::at(dir.to_path_buf()),
        };
        (shelf, net)
    }

    fn files(entries: &[(&str, Vec<u8>)]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(url, body)| (url.to_string(), body.clone()))
            .collect()
    }

    fn settle(finder: &mut Shelf, net: &Fake, served: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
        let mut found = Vec::new();
        for _ in 0..8 {
            net.answer(served);
            found.extend(finder.poll(NOW));
        }
        found
    }

    #[test]
    fn a_cartridge_gets_its_box_by_sha1_and_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(red(), NOW);
        assert_eq!(net.urls(), [GB_URL, GBC_URL]);
        let served = files(&[(GB_URL, GB_DAT.into()), (RED_ART, picture())]);
        assert_eq!(settle(&mut finder, &net, &served), [RED]);
        let kept = decode(&finder.store().art(RED).unwrap()).unwrap();
        assert_eq!((kept.width, kept.height), (300, 320));

        let (mut again, net) = self::finder(dir.path());
        again.want(red(), NOW);
        assert!(net.urls().is_empty(), "a kept cover is never fetched again");
        assert!(!again.is_busy());
    }

    #[test]
    fn a_trimmed_ds_game_is_found_by_its_game_code_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(trimmed_platinum(), NOW);
        let served = files(&[
            (DS_URL, DS_DAT.into()),
            (
                PLATINUM_ART,
                b"Pokemon - Platinum Version (Europe).png".to_vec(),
            ),
            (PLATINUM_LINKED, picture()),
        ]);
        assert_eq!(settle(&mut finder, &net, &served), [PLATINUM]);
        assert!(finder.store().has_art(PLATINUM));
    }

    #[test]
    fn a_missing_box_tries_the_shorter_name_then_waits_a_week() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(red(), NOW);
        let mut asked = Vec::new();
        for _ in 0..8 {
            asked.extend(net.urls());
            net.answer(&files(&[(GB_URL, GB_DAT.into())]));
            assert!(finder.poll(NOW).is_empty());
        }
        assert_eq!(asked, [GB_URL, GBC_URL, RED_ART, RED_SHORT]);
        assert!(!finder.is_busy());

        finder.want(red(), NOW + 6 * DAY);
        assert!(net.urls().is_empty());
        finder.want(red(), NOW + 8 * DAY);
        assert!(finder.poll(NOW + 8 * DAY).is_empty());
        assert_eq!(net.urls(), [RED_ART]);
    }

    #[test]
    fn an_offline_try_leaves_no_mark_and_tries_again_next_time() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(red(), NOW);
        net.fail("dns failed");
        assert!(finder.poll(NOW).is_empty());
        assert!(!finder.is_busy());
        assert_eq!(finder.store().missed_at(RED), None);

        let (mut again, net) = self::finder(dir.path());
        again.want(red(), NOW);
        assert_eq!(net.urls(), [GB_URL, GBC_URL]);
    }

    #[test]
    fn the_name_list_is_kept_for_a_month() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(red(), NOW);
        settle(
            &mut finder,
            &net,
            &files(&[(GB_URL, GB_DAT.into()), (GBC_URL, GBC_DAT.into())]),
        );

        let (mut later, net) = self::finder(dir.path());
        later.want(Game::of(PLATINUM, ConsoleKind::Gb, [1; 20], &[]), NOW + DAY);
        assert!(net.urls().is_empty(), "{:?}", net.urls());
        assert!(later.poll(NOW + DAY).is_empty());

        let (mut month, net) = self::finder(dir.path());
        month.want(
            Game::of(PLATINUM, ConsoleKind::Gb, [1; 20], &[]),
            NOW + 31 * DAY,
        );
        assert_eq!(net.urls(), [GB_URL, GBC_URL]);
    }

    #[test]
    fn pokemon_yellow_is_found_in_the_game_boy_list_though_it_wants_colour() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(yellow(), NOW);
        assert_eq!(net.urls(), [GBC_URL, GB_URL]);
        let served = files(&[
            (GBC_URL, GBC_DAT.into()),
            (GB_URL, YELLOW_DAT.into()),
            (YELLOW_ART, picture()),
        ]);
        assert_eq!(settle(&mut finder, &net, &served), [YELLOW]);
        assert!(finder.store().has_art(YELLOW));
    }

    #[test]
    fn a_dump_off_the_list_is_found_by_its_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let (mut finder, net) = finder(dir.path());
        finder.want(translated_oracle(), NOW);
        let served = files(&[
            (GBC_URL, GBC_DAT.into()),
            (GB_URL, YELLOW_DAT.into()),
            (ORACLE_ART, picture()),
        ]);
        assert_eq!(settle(&mut finder, &net, &served), [YELLOW]);
    }
}
