mod art;
pub mod net;
mod store;

use citrine_gb::console::ConsoleKind;
use citrine_gb_gameinfo::covers::{names, source};
use citrine_gb_threads::Spawner;
use names::Names;
pub use names::System;
use net::{Answer, Net};
pub(crate) use source::encode;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
pub use store::{CoverStore, FOLDERS};

const MISS_WAIT: u64 = 7 * 24 * 60 * 60;
const NAMES_FRESH: u64 = 30 * 24 * 60 * 60;
const MOST_LINKS: u8 = 2;

pub use art::decode;

pub trait SystemOf {
    fn of(console: ConsoleKind, rom: &[u8]) -> Self;
}

impl SystemOf for System {
    fn of(console: ConsoleKind, rom: &[u8]) -> Self {
        match console {
            ConsoleKind::Gba => Self::Gba,
            ConsoleKind::Nds => Self::Nds,
            _ => Self::game_boy(rom),
        }
    }
}

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
    Reading(Receiver<Option<(u64, Names)>>),
    Loading {
        reply: Receiver<Answer>,
        stale: Option<Names>,
    },
    Parsing {
        reply: Receiver<Option<Names>>,
        stale: Option<Names>,
    },
    Ready(Names),
    Failed,
}

impl List {
    fn is_pending(&self) -> bool {
        matches!(
            self,
            List::Reading(_) | List::Loading { .. } | List::Parsing { .. }
        )
    }

    fn next(
        self,
        system: System,
        net: &mut dyn Net,
        store: &CoverStore,
        spawner: &Spawner,
        now: u64,
    ) -> Self {
        match self {
            List::Reading(reply) => match reply.try_recv() {
                Err(TryRecvError::Empty) => List::Reading(reply),
                Ok(Some((fetched, names))) if now.saturating_sub(fetched) < NAMES_FRESH => {
                    List::Ready(names)
                }
                stored => List::Loading {
                    reply: net.get(source::names_url(system)),
                    stale: stored.ok().flatten().map(|(_, names)| names),
                },
            },
            List::Loading { reply, stale } => {
                let answer = match reply.try_recv() {
                    Ok(answer) => answer,
                    Err(TryRecvError::Empty) => return List::Loading { reply, stale },
                    Err(TryRecvError::Disconnected) => Err("no answer".to_owned()),
                };
                let Ok(dat) = answer else {
                    return stale.map_or(List::Failed, List::Ready);
                };
                let (done, reply) = channel();
                let store = store.clone();
                spawner.run("cover names", move || {
                    let names = Some(Names::parse(&String::from_utf8_lossy(&dat)))
                        .filter(|names| !names.is_empty());
                    if let Some(names) = &names
                        && let Err(err) = store.put_names(system, now, names)
                    {
                        log::warn!("could not keep the cover names: {err}");
                    }
                    let _ = done.send(names);
                });
                List::Parsing { reply, stale }
            }
            List::Parsing { reply, stale } => match reply.try_recv() {
                Err(TryRecvError::Empty) => List::Parsing { reply, stale },
                Ok(Some(names)) => List::Ready(names),
                _ => stale.map_or(List::Failed, List::Ready),
            },
            done => done,
        }
    }
}

enum Kept {
    Stored,
    NotArt,
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
    Keep {
        system: System,
        files: Vec<String>,
        at: usize,
        reply: Receiver<Kept>,
    },
}

pub struct Finder {
    net: Box<dyn Net>,
    lists: Vec<(System, List)>,
    asked: Vec<System>,
    games: Vec<(Game, Step)>,
}

impl Finder {
    pub fn new(net: Box<dyn Net>) -> Self {
        Self {
            net,
            lists: Vec::new(),
            asked: Vec::new(),
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
        self.asked.extend(game.systems());
        self.games.push((game, Step::Name));
    }

    pub fn forget_all(&mut self) {
        self.games.clear();
        self.lists.clear();
        self.asked.clear();
    }

    #[cfg(test)]
    pub fn is_busy(&self) -> bool {
        !self.games.is_empty()
    }

    pub fn poll(&mut self, store: &CoverStore, spawner: &Spawner, now: u64) -> Vec<String> {
        for system in std::mem::take(&mut self.asked) {
            self.open_list(store, spawner, system);
        }
        self.poll_lists(store, spawner, now);
        let mut found = Vec::new();
        let games = std::mem::take(&mut self.games);
        for (game, step) in games {
            match self.advance(store, spawner, &game, step, now) {
                Some(Ok(step)) => self.games.push((game, step)),
                Some(Err(())) => {}
                None => found.push(game.rom_key),
            }
        }
        found
    }

    fn open_list(&mut self, store: &CoverStore, spawner: &Spawner, system: System) {
        if list_of(&self.lists, system).is_some() {
            return;
        }
        let (done, reply) = channel();
        let store = store.clone();
        spawner.run("cover names read", move || {
            let _ = done.send(store.names(system));
        });
        self.lists.push((system, List::Reading(reply)));
    }

    fn poll_lists(&mut self, store: &CoverStore, spawner: &Spawner, now: u64) {
        for (system, list) in &mut self.lists {
            if list.is_pending() {
                let taken = std::mem::replace(list, List::Failed);
                *list = taken.next(*system, self.net.as_mut(), store, spawner, now);
            }
        }
    }

    fn advance(
        &mut self,
        store: &CoverStore,
        spawner: &Spawner,
        game: &Game,
        step: Step,
        now: u64,
    ) -> Option<Result<Step, ()>> {
        match step {
            Step::Name => {
                let lists = game
                    .systems()
                    .map(|system| (system, list_of(&self.lists, system)))
                    .collect::<Vec<_>>();
                if lists
                    .iter()
                    .any(|(_, list)| list.is_none_or(List::is_pending))
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
                if body.is_empty() {
                    return self.next_file(store, game, system, files, at, now);
                }
                let (done, reply) = channel();
                let store = store.clone();
                let rom_key = game.rom_key.clone();
                spawner.run("cover art", move || {
                    let kept = match art::shrink(&body) {
                        None => Kept::NotArt,
                        Some(png) => match store.put_art(&rom_key, &png) {
                            Ok(()) => Kept::Stored,
                            Err(err) => {
                                log::warn!("could not keep a cover: {err}");
                                Kept::Failed
                            }
                        },
                    };
                    let _ = done.send(kept);
                });
                Some(Ok(Step::Keep {
                    system,
                    files,
                    at,
                    reply,
                }))
            }
            Step::Keep {
                system,
                files,
                at,
                reply,
            } => match reply.try_recv() {
                Err(TryRecvError::Empty) => Some(Ok(Step::Keep {
                    system,
                    files,
                    at,
                    reply,
                })),
                Ok(Kept::Stored) => None,
                Ok(Kept::NotArt) => self.next_file(store, game, system, files, at, now),
                Ok(Kept::Failed) | Err(TryRecvError::Disconnected) => Some(Err(())),
            },
        }
    }

    fn next_file(
        &mut self,
        store: &CoverStore,
        game: &Game,
        system: System,
        files: Vec<String>,
        at: usize,
        now: u64,
    ) -> Option<Result<Step, ()>> {
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

impl Default for Finder {
    fn default() -> Self {
        Self::new(Box::new(net::Offline))
    }
}

fn list_of(lists: &[(System, List)], system: System) -> Option<&List> {
    lists
        .iter()
        .find(|(listed, _)| *listed == system)
        .map(|(_, list)| list)
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
    #[test]
    fn a_colour_cartridge_looks_in_the_colour_list() {
        let mut rom = vec![0; 0x150];
        assert_eq!(System::of(ConsoleKind::Gb, &rom), System::Gb);
        rom[0x143] = 0x80;
        assert_eq!(System::of(ConsoleKind::Gbc, &rom), System::Gbc);
        rom[0x143] = 0xC0;
        assert_eq!(System::of(ConsoleKind::Gb, &rom), System::Gbc);
        assert_eq!(System::of(ConsoleKind::Gba, &rom), System::Gba);
        assert_eq!(System::of(ConsoleKind::Nds, &rom), System::Nds);
    }

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
        threads: citrine_gb_threads::Threads,
    }

    impl Shelf {
        fn want(&mut self, game: Game, now: u64) {
            self.finder.want(&self.store, game, now);
            for _ in 0..3 {
                assert!(self.poll(now).is_empty());
            }
        }

        fn poll(&mut self, now: u64) -> Vec<String> {
            assert!(self.threads.run_side_jobs().is_empty());
            self.finder.poll(&self.store, &self.threads.spawner(), now)
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
            threads: citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Inline),
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
        for _ in 0..16 {
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
