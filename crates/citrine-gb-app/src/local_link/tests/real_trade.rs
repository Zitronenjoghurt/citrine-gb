use super::*;
use crate::held::InputSource;
use crate::real_roms::{rom_of, save_from};
use citrine_gb_gamedata::{GameState, Pokemon, RomTables, Version, read_live, read_save};
use std::path::PathBuf;

const CUR_MAP: u16 = 0xD35E;
const X: u16 = 0xD362;
const Y: u16 = 0xD361;
const PARTY: u16 = 0xD163;
const ENEMY_PARTY: u16 = 0xD89C;
const BLOCK: u16 = 0x194;
const TRADE_CENTER: u8 = 0xEF;
const FRAME: Duration = Duration::from_nanos(16_742_706);

struct Walk {
    emulator: Emulator,
    now: Instant,
    frames: u64,
    shots: Option<PathBuf>,
}

impl Walk {
    fn frame(&mut self) {
        self.now += FRAME;
        self.emulator.advance(self.now, None);
        self.frames += 1;
    }

    fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn gb(&self, game: Game) -> &GameBoy {
        match game {
            Game::First => &self.emulator.gb,
            Game::Second => &self.emulator.link.as_ref().expect("a linked game").gb,
        }
    }

    fn wram(&self, game: Game, addr: u16) -> u8 {
        let gb = self.gb(game);
        match addr {
            0xC000..=0xCFFF => gb.wram_bank(0).expect("bank 0")[usize::from(addr - 0xC000)],
            _ => gb.wram_bank(1).expect("bank 1")[usize::from(addr - 0xD000)],
        }
    }

    fn pad(game: Game) -> InputSource {
        InputSource::PadButtons(match game {
            Game::First => 0,
            Game::Second => 1,
        })
    }

    fn tap(&mut self, game: Game, button: JoypadState) {
        self.emulator.press(Self::pad(game), button);
        self.frames(6);
        self.emulator.release(Self::pad(game), button);
        self.frames(20);
    }

    fn map(&self, game: Game) -> u8 {
        self.wram(game, CUR_MAP)
    }

    fn at(&self, game: Game) -> (u8, u8) {
        (self.wram(game, X), self.wram(game, Y))
    }

    fn taps(&mut self, game: Game, button: JoypadState, n: u32) {
        for _ in 0..n {
            self.tap(game, button);
        }
    }

    fn step(&mut self, game: Game, button: JoypadState) -> bool {
        let (map, at) = (self.map(game), self.at(game));
        self.emulator.press(Self::pad(game), button);
        for _ in 0..40 {
            self.frame();
            if self.map(game) != map || self.at(game) != at {
                break;
            }
        }
        self.emulator.release(Self::pad(game), button);
        self.frames(16);
        self.map(game) != map || self.at(game) != at
    }

    fn walk(&mut self, game: Game, button: JoypadState, steps: u32) {
        for i in 0..steps {
            if !self.step(game, button) {
                self.shot(game, &format!("blocked-{game:?}-{i}"));
                panic!(
                    "{game:?} blocked walking {button:?} at {:?} on {:?}",
                    self.at(game),
                    self.map(game)
                );
            }
        }
    }

    fn report(&self, game: Game, name: &str) {
        self.shot(game, name);
        println!(
            "{name}: frame {} map {:?} at {:?}",
            self.frames,
            self.map(game),
            self.at(game)
        );
    }

    fn live(&self, game: Game, tables: &RomTables) -> Option<GameState> {
        let gb = self.gb(game);
        let wram = [
            &gb.wram_bank(0).expect("bank 0")[..],
            &gb.wram_bank(1).expect("bank 1")[..],
        ]
        .concat();
        read_live(tables, &wram, &gb.cartridge.battery_ram()).ok()
    }

    fn shot(&self, game: Game, name: &str) {
        if let Some(dir) = &self.shots {
            std::fs::create_dir_all(dir).expect("shots dir");
            std::fs::write(
                dir.join(format!("{name}.png")),
                self.gb(game).frame().render_png(),
            )
            .expect("shot");
        }
    }
}

fn species(mon: &Pokemon) -> u8 {
    match mon {
        Pokemon::Hatched(mon) => mon.species,
        Pokemon::Egg => 0,
    }
}

fn nickname(mon: &Pokemon) -> &str {
    match mon {
        Pokemon::Hatched(mon) => &mon.nickname,
        Pokemon::Egg => "egg",
    }
}

fn owned(state: &GameState, kind: u8) -> usize {
    state
        .party
        .iter()
        .chain(state.boxes.iter().flat_map(|b| b.pokemon.iter()))
        .filter(|mon| species(mon) == kind)
        .count()
}

fn counts(state: &GameState) -> (usize, usize) {
    (
        state.party.len(),
        state.boxes.iter().map(|b| b.pokemon.len()).sum(),
    )
}

#[test]
#[ignore]
fn blue_trades_with_red_over_local_link() {
    let Some(save) = save_from("CITRINE_BLUE_SAVE") else {
        println!("CITRINE_BLUE_SAVE not set, skipped");
        return;
    };
    let (blue_rom, blue) = rom_of(Version::Blue).expect("a blue rom");
    let (red_rom, red) = rom_of(Version::Red).expect("a red rom");
    let tables = [
        RomTables::load(blue, &blue_rom).expect("blue tables"),
        RomTables::load(red, &red_rom).expect("red tables"),
    ];
    let before = read_save(blue, &save).expect("blue save");
    println!("before {:?}", counts(&before));

    let mut emulator = emulator("real-trade");
    let blue_rom = Rom::new(&blue_rom);
    let red_rom = Rom::new(&red_rom);
    let blue_key = blue_rom.header().expect("blue header").sha256_hex_string();
    let red_key = red_rom.header().expect("red header").sha256_hex_string();
    assert_ne!(blue_key, red_key);
    emulator
        .store
        .store_battery(&blue_key, &save)
        .expect("blue battery");
    emulator
        .store
        .store_battery(&red_key, &save)
        .expect("red battery");
    emulator
        .load_rom(&blue_rom, GbModel::for_rom(&blue_rom).expect("model"), None)
        .expect("load blue");
    emulator
        .start_link(
            &red_rom,
            GbModel::for_rom(&red_rom).expect("model"),
            "Red".into(),
        )
        .expect("link red");
    link(&mut emulator).set_settings(&LinkSettings {
        controls: Controls::TwoControllers,
        ..LinkSettings::default()
    });
    let mut walk = Walk {
        emulator,
        now: Instant::now(),
        frames: 0,
        shots: std::env::var_os("CITRINE_SHOTS").map(PathBuf::from),
    };
    walk.frames(60);
    for (game, tables) in [(Game::First, &tables[0]), (Game::Second, &tables[1])] {
        for step in 0..120 {
            let button = if step % 2 == 0 {
                JoypadState::START
            } else {
                JoypadState::A
            };
            walk.tap(game, button);
            walk.frames(34);
            if walk
                .live(game, tables)
                .is_some_and(|s| s.party == before.party)
            {
                break;
            }
        }
        for _ in 0..3 {
            walk.frames(120);
            walk.tap(game, JoypadState::A);
        }
        walk.frames(240);
        walk.report(game, &format!("{game:?}-0-start"));

        walk.tap(game, JoypadState::START);
        walk.tap(game, JoypadState::DOWN);
        walk.tap(game, JoypadState::A);
        walk.frames(30);
        walk.taps(game, JoypadState::DOWN, 4);
        walk.tap(game, JoypadState::A);
        walk.tap(game, JoypadState::A);
        walk.frames(400);
        walk.report(game, &format!("{game:?}-1-dug"));
        walk.walk(game, JoypadState::UP, 1);
        walk.frames(60);
        while walk.step(game, JoypadState::UP) {}
        walk.walk(game, JoypadState::RIGHT, 8);
        walk.tap(game, JoypadState::UP);
        walk.report(game, &format!("{game:?}-2-desk"));
    }
    for _ in 0..160 {
        let maps = [walk.map(Game::First), walk.map(Game::Second)];
        if maps.iter().all(|&m| m == TRADE_CENTER) {
            break;
        }
        for (game, map) in [Game::First, Game::Second].into_iter().zip(maps) {
            if map != TRADE_CENTER {
                walk.tap(game, JoypadState::A);
            }
        }
    }
    assert_eq!(
        [walk.map(Game::First), walk.map(Game::Second)],
        [TRADE_CENTER; 2]
    );
    walk.frames(60);
    walk.report(Game::First, "First-4-trade-center");
    walk.report(Game::Second, "Second-4-trade-center");
    walk.frames(200);
    walk.tap(Game::First, JoypadState::RIGHT);
    walk.tap(Game::First, JoypadState::A);
    walk.tap(Game::Second, JoypadState::LEFT);
    walk.tap(Game::Second, JoypadState::A);
    walk.frames(400);
    walk.report(Game::First, "First-5-menu");
    walk.report(Game::Second, "Second-5-menu");
    walk.tap(Game::First, JoypadState::A);
    walk.frames(30);
    walk.report(Game::First, "First-6-sub");
    walk.tap(Game::First, JoypadState::RIGHT);
    walk.tap(Game::First, JoypadState::A);
    walk.taps(Game::Second, JoypadState::DOWN, 2);
    walk.tap(Game::Second, JoypadState::A);
    walk.frames(30);
    walk.report(Game::Second, "Second-6-sub");
    walk.tap(Game::Second, JoypadState::RIGHT);
    walk.tap(Game::Second, JoypadState::A);
    walk.frames(300);
    walk.report(Game::First, "First-7-confirm");
    walk.report(Game::Second, "Second-7-confirm");
    for (from, to) in [(Game::First, Game::Second), (Game::Second, Game::First)] {
        let sent: Vec<u8> = (PARTY..PARTY + BLOCK).map(|a| walk.wram(from, a)).collect();
        let got: Vec<u8> = (ENEMY_PARTY..ENEMY_PARTY + BLOCK)
            .map(|a| walk.wram(to, a))
            .collect();
        let changed = sent.iter().zip(&got).position(|(a, b)| a != b);
        assert_eq!(changed, None, "party block from {from:?} arrived changed");
    }
    let flareon = species(&before.party[0]);
    let blastoise = species(&before.party[2]);
    for _ in 0..2 {
        walk.tap(Game::First, JoypadState::A);
        walk.tap(Game::Second, JoypadState::A);
        walk.frames(60);
    }
    walk.report(Game::First, "First-8-chosen");
    walk.report(Game::Second, "Second-8-chosen");
    let traded = |walk: &Walk, game: Game, gone: u8, got: u8| {
        let version = match game {
            Game::First => blue,
            Game::Second => red,
        };
        read_save(version, &walk.gb(game).cartridge.battery_ram()).is_ok_and(|s| {
            owned(&s, gone) == owned(&before, gone) - 1 && owned(&s, got) == owned(&before, got) + 1
        })
    };
    for _ in 0..200 {
        if traded(&walk, Game::First, flareon, blastoise)
            && traded(&walk, Game::Second, blastoise, flareon)
        {
            break;
        }
        walk.frames(60);
        for game in [Game::First, Game::Second] {
            walk.tap(game, JoypadState::B);
        }
    }
    walk.frames(120);
    walk.report(Game::First, "First-9-done");
    walk.report(Game::Second, "Second-9-done");
    walk.emulator.flush_save().expect("flush");
    walk.emulator.end_link().expect("end link");
    walk.emulator.flush_save().expect("flush");
    let blue_after = read_save(
        blue,
        &walk
            .emulator
            .store
            .load_battery(&blue_key)
            .expect("blue battery"),
    )
    .expect("blue after");
    let red_after = read_save(
        red,
        &walk
            .emulator
            .store
            .load_battery(&red_key)
            .expect("red battery"),
    )
    .expect("red after");
    for (name, state) in [("blue", &blue_after), ("red", &red_after)] {
        let party: Vec<String> = state
            .party
            .iter()
            .map(|p| format!("{} {}", species(p), nickname(p)))
            .collect();
        println!("{name} after {:?} party {party:?}", counts(state));
    }
    assert_eq!(counts(&blue_after), counts(&before));
    assert_eq!(counts(&red_after), counts(&before));
    assert_eq!(owned(&blue_after, flareon), owned(&before, flareon) - 1);
    assert_eq!(owned(&blue_after, blastoise), owned(&before, blastoise) + 1);
    assert_eq!(owned(&red_after, blastoise), owned(&before, blastoise) - 1);
    assert_eq!(owned(&red_after, flareon), owned(&before, flareon) + 1);
    assert_eq!(blue_after.party.last(), Some(&before.party[2]));
    assert_eq!(red_after.party.last(), Some(&before.party[0]));
}
