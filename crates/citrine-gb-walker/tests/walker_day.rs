use citrine_gb_walker::{
    App, BattleState, Block, Button, DowsingState, Eeprom, FLAG_HAS_POKEMON, FLAG_INIT, Memory,
    Screen, Walker,
};

const FRAME_MS: u64 = 250;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;
const START: u64 = 1_000;
const END_HOUR: u8 = 2;

fn route_eeprom() -> Eeprom {
    let mut eeprom = Eeprom::blank();
    let mut info = eeprom.read(Block::Info.address(), Block::Info.size());
    info[0x5B] = FLAG_INIT | FLAG_HAS_POKEMON | (END_HOUR << 3);
    eeprom.write_reliable(Block::Info, &info);
    eeprom.write(
        0x8F00,
        &[0x19, 0x00, 0, 0, 0x54, 0, 0, 0, 0, 0, 0, 0, 12, 0, 0, 0],
    );
    eeprom.write(0x8F26, &[100]);
    for (slot, species) in [0x10u16, 0x13, 0x29].iter().enumerate() {
        let mut pokemon = [0u8; 16];
        pokemon[..2].copy_from_slice(&species.to_le_bytes());
        pokemon[12] = 5 + slot as u8;
        eeprom.write(0x8F52 + 16 * slot, &pokemon);
    }
    eeprom.write(0x8F82, &[0, 0, 0xF4, 0x01, 0xD0, 0x07]);
    eeprom.write(0x8F88, &[40, 50, 100]);
    for slot in 0..10u16 {
        eeprom.write(0x8F8C + 2 * slot as usize, &(0x11 + slot).to_le_bytes());
        eeprom.write(0x8FA0 + 2 * slot as usize, &(slot * 200).to_le_bytes());
        eeprom.write(0x8FB4 + slot as usize, &[10 + 9 * slot as u8]);
    }
    eeprom
}

struct Day {
    walker: Walker,
    now: u64,
    radar_battles: u32,
    catches: u32,
    dowsings: u32,
}

impl Day {
    fn new(seed: u64) -> Self {
        let mut walker = Walker::from_eeprom(route_eeprom(), seed);
        walker.tick(START);
        Self {
            walker,
            now: START,
            radar_battles: 0,
            catches: 0,
            dowsings: 0,
        }
    }

    fn wait(&mut self, frames: u64) {
        self.now += frames * FRAME_MS;
        self.walker.tick(self.now);
    }

    fn press(&mut self, button: Button) {
        if self.walker.asleep() {
            self.walker.press(Button::Middle);
            self.wait(1);
        }
        self.walker.press(button);
        self.wait(1);
    }

    fn walk(&mut self, steps: u32, minutes: u64) {
        for _ in 0..minutes {
            self.walker.steps(steps / minutes as u32);
            self.now += 60_000;
            self.walker.tick(self.now);
        }
        self.press(Button::Middle);
        self.wait(1);
        if self.walker.home_event().is_some() {
            self.press(Button::Middle);
        }
        self.leave_to_home();
    }

    fn leave_to_home(&mut self) {
        for _ in 0..8 {
            match self.walker.screen() {
                Screen::Home => return,
                Screen::Switch { .. } => self.press(Button::Middle),
                Screen::Menu { .. } => self.press(Button::Left),
                _ => self.press(Button::Middle),
            }
        }
        for _ in 0..100 {
            if self.walker.screen() == Screen::Home {
                return;
            }
            self.press(Button::Left);
        }
        panic!("stuck on {:?}", self.walker.screen());
    }

    fn open(&mut self, app: App) {
        self.leave_to_home();
        self.press(Button::Right);
        for _ in 0..app.index() {
            self.press(Button::Right);
        }
        self.press(Button::Middle);
    }

    fn radar(&mut self) {
        self.open(App::Radar);
        for _ in 0..2000 {
            match self.walker.screen() {
                Screen::Radar(radar) => match radar.shown() {
                    Some(bush) if bush == radar.cursor() => self.press(Button::Middle),
                    Some(_) => self.press(Button::Right),
                    None => self.wait(1),
                },
                Screen::Battle(battle) => match battle.state() {
                    BattleState::Appearing => self.press(Button::Middle),
                    BattleState::Choosing if battle.their_hp() <= 2 => self.press(Button::Middle),
                    BattleState::Choosing => self.press(Button::Left),
                    BattleState::Caught => {
                        self.catches += 1;
                        self.radar_battles += 1;
                        self.press(Button::Middle);
                    }
                    BattleState::Fled | BattleState::Lost => {
                        self.radar_battles += 1;
                        self.press(Button::Middle);
                    }
                    _ => self.wait(1),
                },
                _ => break,
            }
        }
        self.leave_to_home();
    }

    fn dowse(&mut self) {
        self.open(App::Dowsing);
        for _ in 0..200 {
            match self.walker.screen() {
                Screen::Dowsing(dowsing) => match dowsing.state() {
                    DowsingState::Choosing if dowsing.tried(dowsing.cursor()) => {
                        self.press(Button::Right)
                    }
                    DowsingState::Choosing | DowsingState::Hint(_) => self.press(Button::Middle),
                    DowsingState::Found(_) | DowsingState::Missed => {
                        self.dowsings += 1;
                        self.press(Button::Middle);
                    }
                    DowsingState::Shaking => self.wait(1),
                },
                _ => break,
            }
        }
        self.leave_to_home();
    }

    fn play(seed: u64) -> (Self, Vec<u8>, Memory) {
        let mut day = Self::new(seed);
        day.walk(3_000, 60);
        for _ in 0..50 {
            if day.catches > 0 || day.walker.health().watts() < 10 {
                break;
            }
            day.radar();
        }
        assert_eq!(day.catches, 1, "watts {}", day.walker.health().watts());
        day.dowse();
        day.walk(1_500, 30);
        day.dowse();
        day.now = START + DAY_MS;
        day.walker.tick(day.now);
        let eeprom = day.walker.eeprom().bytes().to_vec();
        let memory = day.walker.memory();
        (day, eeprom, memory)
    }
}

#[test]
fn a_seeded_day_replays_to_the_same_eeprom() {
    let (day, eeprom, memory) = Day::play(0x5EED);
    let (_, again, again_memory) = Day::play(0x5EED);
    assert!(eeprom == again, "the replay wrote a different eeprom");
    assert_eq!(memory, again_memory);

    assert!(day.radar_battles >= 1 && day.catches == 1);
    assert_eq!(day.dowsings, 2);
    let caught = day.walker.eeprom().read(0xCE8C, 16);
    assert!([0x10, 0x13, 0x29].contains(&caught[0]));
    let health = day.walker.health();
    assert_eq!((day.walker.today_steps(), health.total_steps()), (0, 4_500));
    assert_eq!(health.total_days(), 1);
    assert_eq!(day.walker.history()[0], 4_500);
    assert_ne!(day.walker.eeprom().read(0xCF0C + 0x84, 1), vec![0]);

    let (_, other, _) = Day::play(0xD1FF);
    assert!(eeprom != other, "another seed should play another day");
}
