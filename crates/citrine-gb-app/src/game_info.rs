use citrine_gb_gamedata::{GameState, RomTables, SaveCopy, identify, read_live, read_save};
use web_time::{Duration, Instant};

use crate::stats::Progress;

pub const READ_EVERY: Duration = Duration::from_millis(250);
pub const PROGRESS_EVERY: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct GameInfo {
    rom_key: Option<String>,
    game: Option<Game>,
    next_read: Option<Instant>,
    next_progress: Option<Instant>,
    pub view: View,
}

#[derive(Default)]
pub struct View {
    pub page: Page,
    pub party_pick: usize,
    pub pc_box: Option<usize>,
    pub box_pick: usize,
    pub dex_search: String,
    pub dex_caught: bool,
    pub details: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    #[default]
    Party,
    Boxes,
    Bag,
    Pokedex,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Page::Party => "Party",
            Page::Boxes => "Boxes",
            Page::Bag => "Bag",
            Page::Pokedex => "Pokédex",
        }
    }
}

pub struct Game {
    pub tables: RomTables,
    pub rom: Vec<u8>,
    battery: Vec<u8>,
    pub saved: Option<GameState>,
    pub saved_at: Option<u64>,
    pub live: Option<GameState>,
    pub behind: bool,
}

impl GameInfo {
    pub fn rom_key(&self) -> Option<&str> {
        self.rom_key.as_deref()
    }

    pub fn game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    pub fn recognised(&self) -> bool {
        self.game.is_some()
    }

    pub fn parts(&mut self) -> (Option<&Game>, &mut View) {
        (self.game.as_ref(), &mut self.view)
    }

    pub fn details_open(&self) -> bool {
        self.game.is_some() && self.view.details
    }

    pub fn set_rom(&mut self, key: Option<&str>, rom: &[u8], saved_at: Option<u64>) {
        self.rom_key = key.map(str::to_owned);
        self.next_read = None;
        self.next_progress = None;
        self.view = View::default();
        self.game = key.and(identify(rom)).and_then(|game| {
            let tables = RomTables::load(game, rom).ok()?;
            Some(Game {
                tables,
                rom: rom.to_vec(),
                battery: Vec::new(),
                saved: None,
                saved_at,
                live: None,
                behind: false,
            })
        });
    }

    pub fn due(&mut self, now: Instant) -> bool {
        self.game.is_some() && tick(&mut self.next_read, now, READ_EVERY)
    }

    pub fn progress_due(&mut self, now: Instant) -> bool {
        self.game.is_some() && tick(&mut self.next_progress, now, PROGRESS_EVERY)
    }

    pub fn progress(&self) -> Option<Progress> {
        let state = self.game.as_ref()?.state()?;
        let time = &state.trainer.play_time;
        Some(Progress {
            in_game_secs: u64::from(time.hours) * 3600
                + u64::from(time.minutes) * 60
                + u64::from(time.seconds),
            dex_owned: count(&state.pokedex.caught),
            dex_seen: count(&state.pokedex.seen),
            badges: state.trainer.badges.iter().filter(|b| **b).count() as u8,
        })
    }

    pub fn summary(&self) -> Option<String> {
        let progress = self.progress()?;
        let time = self.game.as_ref()?.state()?.trainer.play_time;
        Some(format!(
            "🏅\u{a0}{} · Dex\u{a0}{}/{} · {}:{:02}",
            progress.badges, progress.dex_owned, progress.dex_seen, time.hours, time.minutes
        ))
    }

    pub fn lead_and_money(&self) -> Option<String> {
        let game = self.game.as_ref()?;
        let state = game.state()?;
        let money = game.tables.game.money(state.trainer.money);
        let Some(citrine_gb_gamedata::Pokemon::Hatched(lead)) = state.party.first() else {
            return Some(money);
        };
        let name = game
            .tables
            .species_name(lead.species)
            .unwrap_or(&lead.nickname);
        Some(format!("{name} Lv\u{a0}{} · {money}", lead.level))
    }

    #[cfg(test)]
    pub fn next_read_now(&mut self) {
        self.next_read = None;
    }

    pub fn read(&mut self, battery: &[u8], wram: &[u8], now: u64) {
        let Some(game) = self.game.as_mut() else {
            return;
        };
        if battery != game.battery {
            if !game.battery.is_empty() {
                game.saved_at = Some(now);
            }
            game.saved = read_save(game.tables.game, battery).ok();
            game.battery = battery.to_vec();
        }
        game.live = read_live(&game.tables, wram, battery).ok();
        game.behind = match (&game.live, &game.saved) {
            (Some(live), Some(saved)) => !same_progress(live, saved),
            _ => false,
        };
    }
}

fn tick(next: &mut Option<Instant>, now: Instant, every: Duration) -> bool {
    if next.is_some_and(|next| now < next) {
        return false;
    }
    *next = Some(now + every);
    true
}

fn count(list: &[u16]) -> u16 {
    list.len().try_into().unwrap_or(u16::MAX)
}

impl Game {
    pub fn state(&self) -> Option<&GameState> {
        self.live.as_ref().or(self.saved.as_ref())
    }
}

fn same_progress(live: &GameState, saved: &GameState) -> bool {
    let mut live = live.clone();
    live.trainer.play_time = saved.trainer.play_time;
    live.copy = SaveCopy::Main;
    let mut saved = saved.clone();
    saved.copy = SaveCopy::Main;
    live == saved
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_gamedata::fixtures::{
        crystal_cartridge, crystal_live_wram, crystal_save, japanese_cartridge_in_latin,
        set_crystal_live_money,
    };

    fn crystal() -> GameInfo {
        let mut info = GameInfo::default();
        info.set_rom(Some("crystal"), &crystal_cartridge(), Some(100));
        info
    }

    #[test]
    fn a_pokemon_rom_is_recognised_and_others_are_not() {
        assert!(crystal().recognised());
        let mut info = GameInfo::default();
        info.set_rom(Some("zero"), &vec![0u8; 0x8000], None);
        assert!(!info.recognised());
        info.set_rom(Some("midori"), &japanese_cartridge_in_latin(), None);
        assert!(!info.recognised());
        info.set_rom(None, &crystal_cartridge(), None);
        assert!(!info.recognised());
    }

    #[test]
    fn the_first_read_keeps_the_stored_save_time() {
        let mut info = crystal();
        let save = crystal_save();
        info.read(&save, &crystal_live_wram(&save), 500);
        let game = info.game().unwrap();
        assert_eq!(game.saved_at, Some(100));
        assert!(game.saved.is_some());
        assert!(game.live.is_some());
        assert!(!game.behind);
    }

    #[test]
    fn live_progress_past_the_save_is_behind_until_the_game_saves() {
        let mut info = crystal();
        let save = crystal_save();
        let mut wram = crystal_live_wram(&save);
        info.read(&save, &wram, 500);
        set_crystal_live_money(&mut wram, 4815);
        info.read(&save, &wram, 600);
        let game = info.game().unwrap();
        assert!(game.behind);
        assert_eq!(game.state().unwrap().trainer.money, 4815);
        assert_eq!(game.saved_at, Some(100));

        let mut saved = save.clone();
        saved[0x4000] ^= 1;
        info.read(&saved, &wram, 700);
        assert_eq!(info.game().unwrap().saved_at, Some(700));
    }

    #[test]
    fn a_game_on_its_title_screen_shows_the_save() {
        let mut info = crystal();
        info.read(&crystal_save(), &vec![0u8; 0x2000], 500);
        let game = info.game().unwrap();
        assert!(game.live.is_none());
        assert!(game.state().is_some());
        assert!(!game.behind);
    }

    #[test]
    fn progress_comes_once_a_minute() {
        let mut info = crystal();
        let start = Instant::now();
        assert!(info.progress_due(start));
        assert!(!info.progress_due(start + READ_EVERY));
        assert!(info.progress_due(start + PROGRESS_EVERY));
        assert!(!GameInfo::default().progress_due(start));
    }

    #[test]
    fn progress_and_summary_come_from_the_save() {
        let mut info = crystal();
        assert_eq!(info.progress(), None);
        assert_eq!(info.summary(), None);
        info.read(&crystal_save(), &vec![0u8; 0x2000], 500);
        let game = info.game().unwrap();
        let state = game.state().unwrap();
        let progress = info.progress().unwrap();
        assert_eq!(progress.in_game_secs, 300 * 3600 + 30 * 60 + 15);
        assert_eq!(progress.badges, 4);
        assert_eq!(usize::from(progress.dex_owned), state.pokedex.caught.len());
        assert_eq!(usize::from(progress.dex_seen), state.pokedex.seen.len());
        let citrine_gb_gamedata::Pokemon::Hatched(lead) = &state.party[0] else {
            panic!("the lead is an egg");
        };
        let name = game.tables.species_name(lead.species).unwrap();
        let time = state.trainer.play_time;
        assert_eq!((time.hours, time.minutes), (300, 30));
        assert_eq!(
            info.summary().unwrap(),
            format!(
                "🏅\u{a0}4 · Dex\u{a0}{}/{} · 300:30",
                progress.dex_owned, progress.dex_seen
            )
        );
        assert_eq!(state.trainer.money, 100_000);
        assert_eq!(
            info.lead_and_money().unwrap(),
            format!("{name} Lv\u{a0}{} · ¥100,000", lead.level)
        );
    }

    #[test]
    fn reads_come_every_quarter_second() {
        let mut info = crystal();
        let start = Instant::now();
        assert!(info.due(start));
        assert!(!info.due(start + Duration::from_millis(100)));
        assert!(info.due(start + READ_EVERY));
        let mut other = GameInfo::default();
        assert!(!other.due(start));
    }
}
