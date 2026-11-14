use crate::Walker;
use crate::battle::{Action, Battle, BattleEnd};
use crate::dowsing::{Dowsing, DowsingState, Prize};
use crate::info::PokemonSummary;
use crate::radar::{Radar, RadarStep};
use crate::route::{self, Found, LogKind};
use crate::session::Link;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum App {
    Radar,
    Dowsing,
    Connect,
    Card,
    Inventory,
    Settings,
}

impl App {
    pub const ALL: [App; 6] = [
        App::Radar,
        App::Dowsing,
        App::Connect,
        App::Card,
        App::Inventory,
        App::Settings,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Message {
    NothingHeld,
    NoPokemon,
    NeedWatts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HomeEvent {
    Nothing,
    Watts(u16),
    Item(u16),
    Joined,
}

impl HomeEvent {
    pub fn pages(self) -> u8 {
        match self {
            HomeEvent::Nothing => 1,
            HomeEvent::Joined => 2,
            HomeEvent::Watts(_) | HomeEvent::Item(_) => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    Pokemon(PokemonSummary),
    Item(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Setting {
    Sound,
    Shade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shelf {
    Walking,
    Caught(u8),
    EventPokemon,
    Item(u8),
    EventItem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Home,
    Menu {
        app: App,
        message: Option<Message>,
    },
    Radar(Radar),
    Battle(Battle),
    Dowsing(Dowsing),
    Switch {
        new: Keep,
        cursor: u8,
    },
    Collected {
        event: HomeEvent,
        page: u8,
    },
    Connect {
        started: bool,
    },
    Card {
        page: u8,
    },
    Inventory {
        shelf: Shelf,
    },
    Settings {
        setting: Setting,
        editing: Option<u8>,
    },
}

fn menu(app: App) -> Screen {
    Screen::Menu { app, message: None }
}

impl Walker {
    pub fn screen(&self) -> Screen {
        self.screen.clone()
    }

    pub fn press(&mut self, button: Button) {
        if self.asleep() {
            if button == Button::Middle {
                self.wake();
            }
            return;
        }
        self.ram.awake_seconds = self.rules.awake_seconds;
        let screen = std::mem::replace(&mut self.screen, Screen::Home);
        self.screen = match screen {
            Screen::Home => self.press_home(button),
            Screen::Menu { app, message } => self.press_menu(app, message, button),
            Screen::Radar(radar) => self.press_radar(radar, button),
            Screen::Battle(battle) => self.press_battle(battle, button),
            Screen::Dowsing(dowsing) => self.press_dowsing(dowsing, button),
            Screen::Switch { new, cursor } => self.press_switch(new, cursor, button),
            Screen::Collected { event, page } => match button {
                Button::Middle if page + 1 < event.pages() => Screen::Collected {
                    event,
                    page: page + 1,
                },
                Button::Middle => Screen::Home,
                _ => Screen::Collected { event, page },
            },
            Screen::Connect { .. } => menu(App::Connect),
            Screen::Card { page } => match button {
                Button::Left if page == 0 => menu(App::Card),
                Button::Left => Screen::Card { page: page - 1 },
                Button::Right => Screen::Card {
                    page: (page + 1).min(self.rules.history_days as u8),
                },
                Button::Middle => Screen::Home,
            },
            Screen::Inventory { shelf } => self.press_inventory(shelf, button),
            Screen::Settings { setting, editing } => self.press_settings(setting, editing, button),
        };
    }

    pub(crate) fn pass(&mut self) {
        self.home_pass();
        self.rng.next();
        let rules = &self.rules;
        match &mut self.screen {
            Screen::Radar(radar) => radar.pass(rules, &mut self.rng),
            Screen::Battle(battle) => battle.pass(rules, &mut self.rng, false),
            _ => {}
        }
    }

    pub(crate) fn draw(&mut self) {
        let today = self.ram.today_steps;
        let special = self.health().special_route();
        let screen = std::mem::replace(&mut self.screen, Screen::Home);
        self.screen = match screen {
            Screen::Radar(mut radar) => match radar.draw() {
                RadarStep::Battle(found) => Screen::Battle(Battle::new(found, &self.rules)),
                RadarStep::Home => Screen::Home,
                RadarStep::Stay => Screen::Radar(radar),
            },
            Screen::Battle(mut battle) => {
                battle.draw(&self.rules, &mut self.rng);
                Screen::Battle(battle)
            }
            Screen::Dowsing(mut dowsing) => {
                dowsing.draw(&self.eeprom, &self.rules, &mut self.rng, today, special);
                Screen::Dowsing(dowsing)
            }
            Screen::Connect { started: true } if self.link() == Link::Idle => Screen::Home,
            screen => screen,
        };
    }

    pub(crate) fn connect_screen_started(&mut self) {
        if let Screen::Connect { started } = &mut self.screen {
            *started = true;
        }
    }

    fn press_home(&mut self, button: Button) -> Screen {
        if let Some(event) = self.home {
            return self.open_home_event(event);
        }
        if self.stored_info().flags() & crate::info::FLAG_INIT == 0 {
            return match button {
                Button::Middle => Screen::Connect { started: false },
                _ => Screen::Home,
            };
        }
        menu(match button {
            Button::Left => App::Settings,
            Button::Middle => App::Connect,
            Button::Right => App::Radar,
        })
    }

    fn press_menu(&mut self, app: App, message: Option<Message>, button: Button) -> Screen {
        if message.is_some() {
            return menu(app);
        }
        let index = app.index();
        match button {
            Button::Left if index == 0 => Screen::Home,
            Button::Left => menu(App::ALL[index - 1]),
            Button::Right if index == App::ALL.len() - 1 => Screen::Home,
            Button::Right => menu(App::ALL[index + 1]),
            Button::Middle => match self.open(app) {
                Ok(screen) => screen,
                Err(message) => Screen::Menu {
                    app,
                    message: Some(message),
                },
            },
        }
    }

    fn open(&mut self, app: App) -> Result<Screen, Message> {
        let cost = self.rules.app_costs[app.index()];
        let mut health = self.health();
        if health.watts() < cost {
            return Err(Message::NeedWatts);
        }
        if app == App::Radar && !self.ram.walking {
            return Err(Message::NoPokemon);
        }
        if app == App::Inventory && self.shelves().is_empty() {
            return Err(Message::NothingHeld);
        }
        if cost > 0 {
            health.set_watts(health.watts() - cost);
            self.set_health(&health);
            self.save_health();
        }
        let today = self.ram.today_steps;
        Ok(match app {
            App::Radar => Screen::Radar(Radar::start(
                &self.eeprom,
                &self.rules,
                &mut self.rng,
                today,
                health.special_route(),
            )),
            App::Dowsing => Screen::Dowsing(Dowsing::start(&self.rules, &mut self.rng)),
            App::Connect => Screen::Connect { started: false },
            App::Card => Screen::Card { page: 0 },
            App::Inventory => Screen::Inventory {
                shelf: self.shelves()[0],
            },
            App::Settings => Screen::Settings {
                setting: Setting::Sound,
                editing: None,
            },
        })
    }

    fn press_radar(&mut self, mut radar: Radar, button: Button) -> Screen {
        if radar.state() == crate::radar::RadarState::Failed {
            return Screen::Home;
        }
        match button {
            Button::Left => radar.left(&self.rules),
            Button::Right => radar.right(&self.rules),
            Button::Middle => {
                if radar.pick(&self.rules) == RadarStep::Home {
                    return Screen::Home;
                }
            }
        }
        Screen::Radar(radar)
    }

    fn press_battle(&mut self, mut battle: Battle, button: Button) -> Screen {
        match battle.end() {
            Some(end) => self.finish_battle(battle.found(), end),
            None if battle.proceed() => Screen::Battle(battle),
            None => {
                match button {
                    Button::Left => battle.fight(Action::Attack, &self.rules),
                    Button::Right => battle.fight(Action::Evade, &self.rules),
                    Button::Middle => battle.throw(&self.rules),
                }
                Screen::Battle(battle)
            }
        }
    }

    fn finish_battle(&mut self, found: Found, end: BattleEnd) -> Screen {
        let pokemon = route::found_pokemon(&self.eeprom, found);
        let species = pokemon.species;
        match end {
            BattleEnd::Fled => {
                self.log(LogKind::PokemonRan, species, 0, Some(pokemon));
                Screen::Home
            }
            BattleEnd::Lost => {
                let mut health = self.health();
                let lost = self.rules.lost_battle_watts.min(health.watts());
                health.set_watts(health.watts() - lost);
                self.set_health(&health);
                self.save_health();
                self.log(LogKind::PokemonLost, species, 0, Some(pokemon));
                Screen::Home
            }
            BattleEnd::Caught if found == Found::Event => {
                if route::event_pokemon(&self.eeprom, true).is_some() {
                    route::put_event_pokemon(&mut self.eeprom);
                    self.log(LogKind::SpecialPokemonCaught, species, 0, Some(pokemon));
                }
                Screen::Home
            }
            BattleEnd::Caught => match route::free_caught_slot(&self.eeprom, self.rules.max_caught)
            {
                Some(slot) => {
                    route::put_caught(&mut self.eeprom, slot, pokemon);
                    self.log(LogKind::PokemonCaught, species, 0, Some(pokemon));
                    Screen::Home
                }
                None => Screen::Switch {
                    new: Keep::Pokemon(pokemon),
                    cursor: 1,
                },
            },
        }
    }

    fn press_dowsing(&mut self, mut dowsing: Dowsing, button: Button) -> Screen {
        match (dowsing.state(), button) {
            (DowsingState::Found(prize), _) => match prize {
                Prize::EventItem(item) => {
                    route::put_event_item(&mut self.eeprom, item);
                    self.log(LogKind::SpecialItemDowsed, 0, item, None);
                    Screen::Home
                }
                Prize::Watts(watts) => {
                    self.add_watts(watts);
                    Screen::Home
                }
                Prize::Item(item) => match self.store_item(item) {
                    Some(_) => {
                        if self.ram.walking {
                            self.log(LogKind::ItemDowsed, 0, item, None);
                        }
                        Screen::Home
                    }
                    None => Screen::Switch {
                        new: Keep::Item(item),
                        cursor: 1,
                    },
                },
            },
            (DowsingState::Missed, _) => Screen::Home,
            (_, Button::Left) => {
                dowsing.left(&self.rules);
                Screen::Dowsing(dowsing)
            }
            (_, Button::Right) => {
                dowsing.right(&self.rules);
                Screen::Dowsing(dowsing)
            }
            (_, Button::Middle) => {
                dowsing.pick(&self.rules);
                Screen::Dowsing(dowsing)
            }
        }
    }

    fn press_switch(&mut self, new: Keep, cursor: u8, button: Button) -> Screen {
        let last = match new {
            Keep::Pokemon(_) => self.rules.max_caught,
            Keep::Item(_) => self.rules.max_items,
        } as u8
            - 1;
        match button {
            Button::Left if cursor == 0 => Screen::Home,
            Button::Left => Screen::Switch {
                new,
                cursor: cursor - 1,
            },
            Button::Right => Screen::Switch {
                new,
                cursor: (cursor + 1).min(last),
            },
            Button::Middle => {
                match new {
                    Keep::Pokemon(pokemon) => {
                        route::put_caught(&mut self.eeprom, cursor as usize, pokemon);
                        self.log(LogKind::PokemonCaught, pokemon.species, 0, Some(pokemon));
                    }
                    Keep::Item(item) => {
                        route::put_item(&mut self.eeprom, cursor as usize, item);
                        if self.ram.walking {
                            self.log(LogKind::ItemDowsed, 0, item, None);
                        }
                    }
                }
                Screen::Home
            }
        }
    }

    fn press_inventory(&mut self, shelf: Shelf, button: Button) -> Screen {
        let shelves = self.shelves();
        let at = shelves.iter().position(|s| *s == shelf).unwrap_or(0);
        match button {
            Button::Left if at == 0 => menu(App::Inventory),
            Button::Left => Screen::Inventory {
                shelf: shelves[at - 1],
            },
            Button::Right => Screen::Inventory {
                shelf: shelves[(at + 1).min(shelves.len() - 1)],
            },
            Button::Middle => Screen::Home,
        }
    }

    pub fn shelves(&self) -> Vec<Shelf> {
        let eeprom = &self.eeprom;
        let mut shelves = Vec::new();
        if self.ram.walking {
            shelves.push(Shelf::Walking);
        }
        for slot in 0..self.rules.max_caught {
            if route::caught_slot(eeprom, slot).species != 0 {
                shelves.push(Shelf::Caught(slot as u8));
            }
        }
        if route::has_caught_event_pokemon(eeprom) {
            shelves.push(Shelf::EventPokemon);
        }
        for slot in 0..self.rules.max_items {
            if route::item_slot(eeprom, slot) != 0 {
                shelves.push(Shelf::Item(slot as u8));
            }
        }
        if route::has_event_item(eeprom) {
            shelves.push(Shelf::EventItem);
        }
        shelves
    }

    fn press_settings(&mut self, setting: Setting, editing: Option<u8>, button: Button) -> Screen {
        let Some(value) = editing else {
            return match (setting, button) {
                (Setting::Sound, Button::Left) => menu(App::Settings),
                (Setting::Shade, Button::Left) => Screen::Settings {
                    setting: Setting::Sound,
                    editing: None,
                },
                (_, Button::Right) => Screen::Settings {
                    setting: Setting::Shade,
                    editing: None,
                },
                (_, Button::Middle) => {
                    let health = self.health();
                    let value = match setting {
                        Setting::Sound => health.sound(),
                        Setting::Shade => health.shade(),
                    };
                    Screen::Settings {
                        setting,
                        editing: Some(value),
                    }
                }
            };
        };
        if button == Button::Middle {
            self.save_health();
            return Screen::Home;
        }
        let value = match (setting, button) {
            (Setting::Sound, Button::Left) => {
                (value + self.rules.sound_levels - 1) % self.rules.sound_levels
            }
            (Setting::Sound, _) => (value + 1) % self.rules.sound_levels,
            (Setting::Shade, Button::Left) => value.saturating_sub(1),
            (Setting::Shade, _) => (value + 1).min(self.rules.shade_levels - 1),
        };
        let mut health = self.health();
        match setting {
            Setting::Sound => health.set_sound(value),
            Setting::Shade => health.set_shade(value),
        }
        self.set_health(&health);
        Screen::Settings {
            setting,
            editing: Some(value),
        }
    }

    fn store_item(&mut self, item: u16) -> Option<usize> {
        let slot = route::free_item_slot(&self.eeprom, self.rules.max_items)?;
        route::put_item(&mut self.eeprom, slot, item);
        Some(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::BattleState;
    use crate::eeprom::{self, Block, Eeprom};
    use crate::info::FLAG_INIT;
    use crate::session::CYCLES_PER_SECOND;

    fn walker(watts: u16) -> Walker {
        let mut eeprom = Eeprom::blank();
        eeprom.write(eeprom::ROUTE, &[25, 0]);
        for slot in 0..3 {
            eeprom.write(0x8F52 + 16 * slot, &[0x10 + slot as u8, 0]);
            eeprom.write(0x8F88 + slot, &[100]);
        }
        let mut info = vec![0; 0x68];
        info[0x5B] = FLAG_INIT | 0x02;
        eeprom.write_reliable(Block::Info, &info);
        let mut walker = Walker::from_eeprom(eeprom, 7);
        walker.ram.walking = true;
        walker.add_watts(watts);
        walker
    }

    fn presses(walker: &mut Walker, buttons: &[Button]) {
        for button in buttons {
            walker.press(*button);
        }
    }

    fn draws(walker: &mut Walker, count: u32) {
        for _ in 0..count {
            for _ in 0..walker.rules.passes_per_draw {
                walker.pass();
            }
            walker.draw();
        }
    }

    #[test]
    fn the_home_buttons_open_the_menu_at_settings_connect_and_radar() {
        let mut walker = walker(0);
        for (button, app) in [
            (Button::Left, App::Settings),
            (Button::Middle, App::Connect),
            (Button::Right, App::Radar),
        ] {
            walker.screen = Screen::Home;
            walker.press(button);
            assert_eq!(walker.screen(), menu(app));
        }
    }

    #[test]
    fn an_unpaired_walker_only_connects_from_home() {
        let mut walker = Walker::new(1);
        walker.press(Button::Left);
        assert_eq!(walker.screen(), Screen::Home);
        walker.press(Button::Middle);
        assert_eq!(walker.screen(), Screen::Connect { started: false });
    }

    #[test]
    fn the_menu_goes_home_past_either_end() {
        let mut walker = walker(0);
        presses(&mut walker, &[Button::Right, Button::Left]);
        assert_eq!(walker.screen(), Screen::Home);
        presses(&mut walker, &[Button::Left, Button::Right]);
        assert_eq!(walker.screen(), Screen::Home);
        presses(&mut walker, &[Button::Right, Button::Right]);
        assert_eq!(walker.screen(), menu(App::Dowsing));
    }

    #[test]
    fn the_radar_costs_ten_watts_and_dowsing_three() {
        let mut walker = walker(13);
        presses(&mut walker, &[Button::Right, Button::Middle]);
        assert!(matches!(walker.screen(), Screen::Radar(_)));
        assert_eq!(walker.health().watts(), 3);
        walker.screen = menu(App::Dowsing);
        walker.press(Button::Middle);
        assert!(matches!(walker.screen(), Screen::Dowsing(_)));
        assert_eq!(walker.health().watts(), 0);
        walker.screen = menu(App::Dowsing);
        walker.press(Button::Middle);
        assert_eq!(
            walker.screen(),
            Screen::Menu {
                app: App::Dowsing,
                message: Some(Message::NeedWatts)
            }
        );
        walker.press(Button::Right);
        assert_eq!(walker.screen(), menu(App::Dowsing));
    }

    #[test]
    fn watts_are_checked_before_the_walking_pokemon() {
        let mut walker = Walker::new(1);
        walker.screen = menu(App::Radar);
        walker.press(Button::Middle);
        assert_eq!(
            walker.screen(),
            Screen::Menu {
                app: App::Radar,
                message: Some(Message::NeedWatts)
            }
        );
        walker.add_watts(50);
        for (app, message) in [
            (App::Radar, Message::NoPokemon),
            (App::Inventory, Message::NothingHeld),
        ] {
            walker.screen = menu(app);
            walker.press(Button::Middle);
            assert_eq!(
                walker.screen(),
                Screen::Menu {
                    app,
                    message: Some(message)
                }
            );
        }
        walker.screen = menu(App::Dowsing);
        walker.press(Button::Middle);
        assert!(matches!(walker.screen(), Screen::Dowsing(_)));
        assert_eq!(walker.health().watts(), 47);
    }

    fn catch(walker: &mut Walker) -> Screen {
        loop {
            walker.screen = Screen::Battle(Battle::new(Found::Route(1), &walker.rules));
            draws(walker, 16);
            walker.press(Button::Middle);
            walker.press(Button::Middle);
            draws(walker, 64);
            let Screen::Battle(battle) = walker.screen() else {
                unreachable!()
            };
            if battle.state() == BattleState::Caught {
                break;
            }
        }
        walker.press(Button::Middle);
        walker.screen()
    }

    #[test]
    fn a_catch_fills_the_first_free_slot_and_a_fourth_offers_the_switch() {
        let mut walker = walker(0);
        for slot in 0..3 {
            assert_eq!(catch(&mut walker), Screen::Home);
            assert_eq!(walker.eeprom.read(0xCE8C + 16 * slot, 2), vec![0x11, 0]);
        }
        assert_eq!(walker.caught().len(), 3);
        let Screen::Switch { new, cursor: 1 } = catch(&mut walker) else {
            panic!("no switch screen")
        };
        assert_eq!(walker.health().event_log_index(), 3);
        presses(&mut walker, &[Button::Right, Button::Right, Button::Left]);
        assert_eq!(walker.screen(), Screen::Switch { new, cursor: 1 });
        presses(&mut walker, &[Button::Left, Button::Left]);
        assert_eq!(walker.screen(), Screen::Home);
        assert_eq!(walker.health().event_log_index(), 3);

        walker.eeprom.write(0xCE8C + 16, &[0x77, 0]);
        let Screen::Switch { .. } = catch(&mut walker) else {
            panic!("no switch screen")
        };
        walker.press(Button::Middle);
        assert_eq!(walker.eeprom.read(0xCE9C, 2), vec![0x11, 0]);
        assert_eq!(walker.screen(), Screen::Home);
        let entry = 0xCF0C + 3 * 0x88;
        assert_eq!(walker.eeprom.read(entry + 0x84, 1), vec![0x0D]);
        assert_eq!(walker.eeprom.read(entry + 0x0C, 2), vec![0x11, 0]);
    }

    #[test]
    fn a_lost_battle_costs_up_to_ten_watts() {
        for (watts, left) in [(25, 15), (4, 0)] {
            let mut walker = walker(watts);
            let rules = walker.rules.clone();
            let mut battle = Battle::new(Found::Route(0), &rules);
            while battle.end() != Some(BattleEnd::Lost) {
                battle = Battle::new(Found::Route(0), &rules);
                while battle.end().is_none() {
                    for _ in 0..16 {
                        battle.draw(&rules, &mut walker.rng);
                    }
                    battle.proceed();
                    battle.pass(&rules, &mut walker.rng, false);
                    battle.fight(Action::Attack, &rules);
                }
            }
            walker.screen = Screen::Battle(battle);
            walker.press(Button::Left);
            assert_eq!(walker.screen(), Screen::Home);
            assert_eq!(walker.health().watts(), left);
            assert_eq!(walker.eeprom.read(0xCF0C + 0x84, 1), vec![0x10]);
        }
    }

    #[test]
    fn dowsing_ends_at_home_with_the_item_found_or_missed() {
        let mut walker = walker(3);
        for slot in 0..10 {
            walker.eeprom.write(0x8F8C + 2 * slot, &[0x2A, 0]);
        }
        walker.screen = menu(App::Dowsing);
        walker.press(Button::Middle);
        loop {
            let Screen::Dowsing(dowsing) = walker.screen() else {
                unreachable!()
            };
            match dowsing.state() {
                DowsingState::Found(_) | DowsingState::Missed => break,
                DowsingState::Shaking => draws(&mut walker, 1),
                DowsingState::Choosing if dowsing.tried(dowsing.cursor()) => {
                    walker.press(Button::Right)
                }
                _ => walker.press(Button::Middle),
            }
        }
        let Screen::Dowsing(dowsing) = walker.screen() else {
            unreachable!()
        };
        walker.press(Button::Middle);
        assert_eq!(walker.screen(), Screen::Home);
        let found = matches!(dowsing.state(), DowsingState::Found(_));
        assert_eq!(walker.items(), if found { vec![0x2A] } else { vec![] });
    }

    #[test]
    fn any_button_opens_an_offered_event_and_middle_pages_through_it() {
        let mut walker = walker(0);
        walker.home = Some(HomeEvent::Watts(50));
        walker.offer = 10;
        walker.press(Button::Left);
        assert_eq!(
            walker.screen(),
            Screen::Collected {
                event: HomeEvent::Watts(50),
                page: 0
            }
        );
        assert_eq!((walker.health().watts(), walker.home_event()), (50, None));
        walker.press(Button::Left);
        assert_eq!(
            walker.screen(),
            Screen::Collected {
                event: HomeEvent::Watts(50),
                page: 0
            }
        );
        presses(&mut walker, &[Button::Middle, Button::Middle]);
        assert_eq!(
            walker.screen(),
            Screen::Collected {
                event: HomeEvent::Watts(50),
                page: 2
            }
        );
        walker.press(Button::Middle);
        assert_eq!(walker.screen(), Screen::Home);
        assert_eq!(walker.eeprom.read(0xCF0C + 0x84, 1), vec![0x12]);
    }

    #[test]
    fn settings_apply_at_once_and_save_on_middle() {
        let mut walker = walker(0);
        walker.screen = menu(App::Settings);
        presses(&mut walker, &[Button::Middle, Button::Middle, Button::Left]);
        assert_eq!(walker.health().sound(), 1);
        assert_eq!(walker.eeprom.read(0x016D, 1), vec![0x24]);
        walker.press(Button::Middle);
        assert_eq!(walker.screen(), Screen::Home);
        walker.screen = menu(App::Settings);
        presses(
            &mut walker,
            &[Button::Middle, Button::Right, Button::Right, Button::Middle],
        );
        for _ in 0..12 {
            walker.press(Button::Right);
        }
        walker.press(Button::Middle);
        let health = walker.health();
        assert_eq!((health.sound(), health.shade()), (1, 9));
        assert_eq!(walker.eeprom.read(0x016D, 1), vec![(1 << 1) | (9 << 3)]);
    }

    #[test]
    fn the_card_pages_through_seven_days() {
        let mut walker = walker(0);
        walker.screen = menu(App::Card);
        walker.press(Button::Middle);
        for _ in 0..10 {
            walker.press(Button::Right);
        }
        assert_eq!(walker.screen(), Screen::Card { page: 7 });
        walker.screen = Screen::Card { page: 0 };
        walker.press(Button::Left);
        assert_eq!(walker.screen(), menu(App::Card));
    }

    #[test]
    fn the_inventory_skips_empty_shelves() {
        let mut walker = walker(0);
        route::put_item(&mut walker.eeprom, 1, 5);
        walker.eeprom.write(0xCE8C + 32, &[0x10, 0]);
        assert_eq!(
            walker.shelves(),
            vec![Shelf::Walking, Shelf::Caught(2), Shelf::Item(1)]
        );
        walker.screen = menu(App::Inventory);
        presses(&mut walker, &[Button::Middle, Button::Right, Button::Right]);
        assert_eq!(
            walker.screen(),
            Screen::Inventory {
                shelf: Shelf::Item(1)
            }
        );
        presses(
            &mut walker,
            &[Button::Right, Button::Left, Button::Left, Button::Left],
        );
        assert_eq!(walker.screen(), menu(App::Inventory));
    }

    #[test]
    fn a_sleeping_walker_wakes_on_the_middle_button_only() {
        let mut walker = walker(0);
        walker.screen = menu(App::Card);
        walker.ram.awake_seconds = 0;
        walker.press(Button::Right);
        assert!(walker.asleep());
        walker.press(Button::Middle);
        assert!(!walker.asleep());
        assert_eq!(walker.screen(), menu(App::Card));
        assert_eq!(walker.ram.awake_seconds, 60);
        walker.press(Button::Right);
        assert_eq!(walker.ram.awake_seconds, 90);
    }

    #[test]
    fn connect_goes_home_when_its_session_ends() {
        let mut walker = walker(0);
        walker.screen = menu(App::Connect);
        walker.press(Button::Middle);
        draws(&mut walker, 3);
        assert_eq!(walker.screen(), Screen::Connect { started: false });
        walker.connect(0);
        assert_eq!(walker.screen(), Screen::Connect { started: true });
        let mut now = 0;
        while walker.link() != Link::Idle {
            now += CYCLES_PER_SECOND / 1000;
            walker.take(now);
        }
        draws(&mut walker, 1);
        assert_eq!(walker.screen(), Screen::Home);
    }
}
