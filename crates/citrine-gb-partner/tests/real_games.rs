use citrine_gb::gb::joypad::JoypadState;
use citrine_gb::gb::{GameBoy, GbModel};
use citrine_gb::netplay::PeerCable;
use citrine_gb::persistence::sram_dump::SramDump;
use citrine_gb::rom::Rom;
use citrine_gb_gamedata::{
    Game, GameState, Mon, Pokemon, RomTables, Version, identify, read_live, read_save,
};
use citrine_gb_partner::{Happening, Partner, Stage, event};
use std::path::PathBuf;
use std::time::Instant;

const MAP_GROUP: u16 = 0xDA00;
const MAP_NUMBER: u16 = 0xDA01;
const Y: u16 = 0xDA02;
const X: u16 = 0xDA03;
const VIOLET_CITY: (u8, u8) = (10, 5);
const VIOLET_POKECENTER: (u8, u8) = (10, 10);
const TRADE_CENTER: (u8, u8) = (20, 2);
const NOW: u64 = 1_790_000_000;

fn rom_dir() -> PathBuf {
    std::env::var_os("CITRINE_ROMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").expect("home")).join("roms/gb"))
}

fn silver_rom() -> Option<(Vec<u8>, Game)> {
    std::fs::read_dir(rom_dir()).ok()?.find_map(|entry| {
        let data = std::fs::read(entry.ok()?.path()).ok()?;
        let game = identify(&data)?;
        (game.version == Version::Silver).then_some((data, game))
    })
}

struct Play {
    gb: GameBoy,
    cable: PeerCable,
    partner: Partner,
    happened: Vec<Happening>,
    frames: u64,
    shots: Option<PathBuf>,
}

impl Play {
    fn frame(&mut self) {
        self.cable.run_frame(&mut self.gb, &mut self.partner);
        self.happened.extend(self.partner.take_happenings());
        self.frames += 1;
    }

    fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.frame();
        }
    }

    fn read(&self, addr: u16) -> u8 {
        self.gb.wram_bank(1).expect("wram bank 1")[usize::from(addr - 0xD000)]
    }

    fn map(&self) -> (u8, u8) {
        (self.read(MAP_GROUP), self.read(MAP_NUMBER))
    }

    fn at(&self) -> (u8, u8) {
        (self.read(X), self.read(Y))
    }

    fn tap(&mut self, button: JoypadState) {
        self.gb.press_button(button);
        self.frames(6);
        self.gb.release_button(button);
        self.frames(20);
    }

    fn shot(&self, name: &str) {
        if let Some(dir) = &self.shots {
            std::fs::create_dir_all(dir).expect("shots dir");
            std::fs::write(
                dir.join(format!("{name}.png")),
                self.gb.frame().render_png(),
            )
            .expect("shot");
        }
        println!(
            "{name}: frame {} map {:?} at {:?} stage {:?} transfers {}",
            self.frames,
            self.map(),
            self.at(),
            self.partner.stage(),
            self.cable.transfers()
        );
    }

    fn step(&mut self, button: JoypadState) -> bool {
        let (map, at) = (self.map(), self.at());
        self.gb.press_button(button);
        let mut moved = false;
        for _ in 0..40 {
            self.frame();
            if self.map() != map || self.at() != at {
                moved = true;
                break;
            }
        }
        self.gb.release_button(button);
        self.frames(16);
        moved || self.map() != map || self.at() != at
    }

    fn walk(&mut self, button: JoypadState, steps: u32) {
        for i in 0..steps {
            if !self.step(button) {
                self.shot(&format!("blocked-{:?}-{i}", button.bits()));
                panic!(
                    "blocked walking {:?} at {:?} on {:?}",
                    button,
                    self.at(),
                    self.map()
                );
            }
        }
    }

    fn mash_until(&mut self, button: JoypadState, limit: u32, done: impl Fn(&Self) -> bool) {
        for _ in 0..limit {
            if done(self) {
                return;
            }
            self.tap(button);
        }
        self.shot("mash-timeout");
        panic!("waited too long");
    }

    fn wait_until(&mut self, limit: u32, done: impl Fn(&Self) -> bool) {
        for _ in 0..limit {
            if done(self) {
                return;
            }
            self.frame();
        }
        self.shot("wait-timeout");
        panic!("waited too long");
    }

    fn live(&self, tables: &RomTables) -> Option<GameState> {
        let wram = [
            &self.gb.wram_bank(0).expect("bank 0")[..],
            &self.gb.wram_bank(1).expect("bank 1")[..],
        ]
        .concat();
        read_live(tables, &wram, &self.gb.cartridge.battery_ram()).ok()
    }
}

#[test]
#[ignore]
fn silver_receives_celebi_from_the_event_partner() {
    let Some(save_path) = std::env::var_os("CITRINE_SILVER_SAVE") else {
        println!("CITRINE_SILVER_SAVE not set, skipped");
        return;
    };
    let save = std::fs::read(save_path).expect("silver save");
    let (data, game) = silver_rom().expect("a silver rom");
    let started = Instant::now();
    let tables = RomTables::load(game, &data).expect("tables");
    let before = read_save(game, &save).expect("save");
    let celebi = event("celebi-pcny").expect("event");
    let partner = Partner::new(game, &data, celebi, 2026).expect("partner");

    let rom = Rom::new(&data);
    let mut gb = GameBoy::new_empty(GbModel::for_rom(&rom).expect("model"));
    gb.load_rom(&rom).expect("load");
    gb.put_sram_dump(SramDump::from_slice(&save), NOW);
    let mut play = Play {
        gb,
        cable: {
            let mut cable = PeerCable::new();
            cable.set_plugged(true);
            cable
        },
        partner,
        happened: Vec::new(),
        frames: 0,
        shots: std::env::var_os("CITRINE_SHOTS").map(PathBuf::from),
    };
    play.frames(60);
    for step in 0..120 {
        let button = if step % 2 == 0 {
            JoypadState::START
        } else {
            JoypadState::A
        };
        play.tap(button);
        play.frames(34);
        if play.live(&tables).is_some_and(|s| s.party == before.party) {
            break;
        }
    }
    play.tap(JoypadState::A);
    for i in 0..6 {
        play.frames(90);
        play.shot(&format!("boot-{i}"));
        play.tap(JoypadState::B);
    }
    play.frames(60);
    play.shot("overworld");
    assert_eq!(play.map(), VIOLET_CITY);

    play.walk(JoypadState::DOWN, 4);
    play.walk(JoypadState::RIGHT, 2);
    play.walk(JoypadState::DOWN, 1);
    play.walk(JoypadState::RIGHT, 2);
    play.walk(JoypadState::UP, 1);
    play.frames(60);
    play.shot("pokecenter");
    assert_eq!(play.map(), VIOLET_POKECENTER);
    play.walk(JoypadState::LEFT, 3);
    play.frames(60);
    play.shot("upstairs");
    play.walk(JoypadState::RIGHT, 5);
    play.walk(JoypadState::UP, 4);
    play.shot("desk");
    play.mash_until(JoypadState::A, 60, |p| {
        p.happened.contains(&Happening::Linked)
    });
    play.shot("linked");
    play.mash_until(JoypadState::A, 60, |p| p.map() == TRADE_CENTER);
    play.frames(90);
    play.shot("trade-center");
    play.walk(JoypadState::LEFT, 1);
    play.walk(JoypadState::UP, 3);
    play.tap(JoypadState::RIGHT);
    play.shot("seat");
    play.mash_until(JoypadState::A, 60, |p| p.partner.stage() == Stage::Choosing);
    play.frames(150);
    play.shot("trade-menu");
    play.tap(JoypadState::A);
    play.shot("submenu");
    play.tap(JoypadState::RIGHT);
    play.tap(JoypadState::A);
    play.wait_until(1200, |p| {
        p.happened
            .iter()
            .any(|h| matches!(h, Happening::Chosen { .. }))
    });
    play.frames(120);
    play.shot("confirm");
    play.mash_until(JoypadState::A, 40, |p| {
        p.happened
            .iter()
            .any(|h| matches!(h, Happening::Traded { .. }))
    });
    play.shot("traded");
    let celebi = tables.species_for_dex(251).expect("celebi");
    play.mash_until(JoypadState::B, 600, |p| {
        read_save(game, &p.gb.cartridge.battery_ram())
            .ok()
            .and_then(|s| received(&s, celebi))
            .is_some()
    });
    play.shot("saved");
    play.frames(300);
    play.shot("after");
    let saved = read_save(game, &play.gb.cartridge.battery_ram()).expect("saved game");
    let mon = received(&saved, celebi).expect("celebi in the save");
    let live = play.live(&tables).expect("live game");
    assert_eq!(received(&live, celebi).as_ref(), Some(&mon));
    assert!(mon.ot_name.starts_with("PCNY"), "{}", mon.ot_name);
    assert!((32..=1053).contains(&mon.ot_id), "{}", mon.ot_id);
    assert_eq!(mon.level, 5);
    assert_eq!(
        mon.moves.iter().map(|m| m.id).collect::<Vec<_>>(),
        [73, 93, 215, 105]
    );
    assert_eq!(
        saved.party.len() + saved.boxes.iter().map(|b| b.pokemon.len()).sum::<usize>(),
        before.party.len() + before.boxes.iter().map(|b| b.pokemon.len()).sum::<usize>()
    );
    println!("took {:?}", started.elapsed());
}

fn received(state: &GameState, species: u16) -> Option<Mon> {
    state
        .party
        .iter()
        .chain(state.boxes.iter().flat_map(|b| b.pokemon.iter()))
        .find_map(|p| match p {
            Pokemon::Hatched(mon) if mon.species == species => Some((**mon).clone()),
            _ => None,
        })
}
