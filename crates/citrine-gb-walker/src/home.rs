use crate::Walker;
use crate::eeprom::Block;
use crate::route::{self, LogKind, ROUTE_ITEM_COUNT};
use crate::screen::{HomeEvent, Screen};

impl Walker {
    pub(crate) fn wake(&mut self) {
        self.ram.awake_seconds = self.rules.wake_seconds;
        self.woke = true;
    }

    pub(crate) fn home_pass(&mut self) {
        if self.offer > 0 {
            self.offer -= 1;
            if self.offer == 0 {
                self.home = None;
            }
        }
        if self.woke && self.screen == Screen::Home {
            self.woke = false;
            self.home_check();
        }
    }

    fn home_check(&mut self) {
        let rules = &self.rules;
        if self.ram.event_seconds < rules.home_event_seconds
            || self.rng.whole_percent() >= u32::from(rules.home_chance)
        {
            return;
        }
        let Some(event) = self.choose_home_event() else {
            return;
        };
        if !matches!(event, HomeEvent::Item(_) | HomeEvent::Joined) {
            self.rng.next();
        }
        self.home = Some(event);
        self.offer = self.rules.home_offer_passes;
    }

    fn choose_home_event(&self) -> Option<HomeEvent> {
        let rules = &self.rules;
        let steps = self.ram.hour_steps;
        if !route::has_walking_pokemon(&self.eeprom) {
            return (steps >= rules.home_joined_steps).then_some(HomeEvent::Joined);
        }
        let happiness = route::happiness(&self.eeprom);
        let free = route::free_item_slot(&self.eeprom, rules.max_items).is_some();
        let [big, mid, small] = rules.home_watts;
        if free && happiness >= rules.home_item_happiness && steps >= rules.home_item_steps {
            let back = usize::from(steps / rules.home_item_steps).min(ROUTE_ITEM_COUNT - 1);
            return Some(HomeEvent::Item(route::route_item(
                &self.eeprom,
                ROUTE_ITEM_COUNT - 1 - back,
            )));
        }
        if happiness >= rules.home_big_happiness && steps >= rules.home_big_steps {
            return Some(HomeEvent::Watts(big));
        }
        if steps >= rules.home_mid_steps {
            return Some(HomeEvent::Watts(mid));
        }
        if steps >= rules.home_small_steps {
            return Some(HomeEvent::Watts(small));
        }
        let minutes = self.health().walk_minutes();
        (minutes >= rules.home_nothing_minutes && steps <= rules.home_nothing_steps)
            .then_some(HomeEvent::Nothing)
    }

    pub(crate) fn open_home_event(&mut self, event: HomeEvent) -> Screen {
        self.home = None;
        self.offer = 0;
        self.ram.event_seconds = 0;
        let [big, mid, _] = self.rules.home_watts;
        match event {
            HomeEvent::Item(item) => {
                if let Some(slot) = route::free_item_slot(&self.eeprom, self.rules.max_items) {
                    route::put_item(&mut self.eeprom, slot, item);
                }
                self.log(LogKind::FoundItem, 0, item, None);
            }
            HomeEvent::Watts(watts) => {
                self.add_watts(watts);
                let kind = match watts {
                    w if w == big => LogKind::FoundBigWatts,
                    w if w == mid => LogKind::FoundMidWatts,
                    _ => LogKind::FoundSmallWatts,
                };
                self.log(kind, 0, 0, None);
            }
            HomeEvent::Nothing => self.log(LogKind::FoundNothing, 0, 0, None),
            HomeEvent::Joined => self.pokemon_joined(),
        }
        Screen::Collected { event, page: 0 }
    }

    fn pokemon_joined(&mut self) {
        self.ram.hour_steps = 0;
        self.ram.walking = true;
        let mut info = self.stored_info();
        if !info.has_pokemon() {
            info.join();
            self.eeprom.write_reliable(Block::Info, info.bytes());
            route::join(&mut self.eeprom, self.rules.joined_happiness);
            let mut health = self.health();
            health.set_walk_minutes(0);
            self.set_health(&health);
            self.save_health();
            route::clear_log_kinds(&mut self.eeprom);
        }
        self.log(LogKind::PokemonJoined, 0, 0, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eeprom::{self, Eeprom};

    fn walking(happiness: u8, hour_steps: u16) -> Walker {
        let mut eeprom = Eeprom::blank();
        eeprom.write(eeprom::ROUTE, &[25, 0]);
        eeprom.write(route::HAPPINESS, &[happiness]);
        for slot in 0..10 {
            eeprom.write(0x8F8C + 2 * slot, &(100 + slot as u16).to_le_bytes());
        }
        let mut walker = Walker::from_eeprom(eeprom, 1);
        walker.ram.walking = true;
        walker.ram.hour_steps = hour_steps;
        walker
    }

    #[test]
    fn events_follow_this_hours_steps_and_happiness() {
        assert_eq!(
            walking(90, 500).choose_home_event(),
            Some(HomeEvent::Item(108))
        );
        assert_eq!(
            walking(90, 4600).choose_home_event(),
            Some(HomeEvent::Item(100))
        );
        assert_eq!(
            walking(89, 500).choose_home_event(),
            Some(HomeEvent::Watts(50))
        );
        assert_eq!(
            walking(79, 500).choose_home_event(),
            Some(HomeEvent::Watts(20))
        );
        assert_eq!(
            walking(0, 199).choose_home_event(),
            Some(HomeEvent::Watts(10))
        );
        assert_eq!(walking(0, 99).choose_home_event(), None);
        let mut idle = walking(0, 50);
        let mut health = idle.health();
        health.set_walk_minutes(60);
        idle.set_health(&health);
        assert_eq!(idle.choose_home_event(), Some(HomeEvent::Nothing));
    }

    #[test]
    fn an_event_comes_on_a_wake_at_most_once_an_hour_and_waits_for_a_press() {
        let mut offered = 0u32;
        for seed in 0..200 {
            let mut walker = walking(0, 150);
            walker.set_rng_state(seed * 0x9E37_79B1 % (1 << 32));
            walker.wake();
            walker.home_pass();
            if walker.home_event().is_some() {
                offered += 1;
                for _ in 1..48 {
                    walker.home_pass();
                }
                assert_eq!(walker.home_event(), Some(HomeEvent::Watts(10)));
                walker.home_pass();
                assert_eq!(walker.home_event(), None);
            }
        }
        assert!(offered.abs_diff(80) < 25, "{offered}");
        let mut walker = walking(0, 150);
        walker.ram.event_seconds = 3_599;
        for seed in 0..50 {
            walker.set_rng_state(seed);
            walker.wake();
            walker.home_pass();
            assert_eq!(walker.home_event(), None);
        }
    }

    #[test]
    fn opening_an_event_pays_it_and_logs_it() {
        let mut walker = walking(0, 150);
        let screen = walker.open_home_event(HomeEvent::Watts(20));
        assert_eq!(
            screen,
            Screen::Collected {
                event: HomeEvent::Watts(20),
                page: 0
            }
        );
        assert_eq!(walker.health().watts(), 20);
        assert_eq!(walker.eeprom.read(0xCF0C + 0x84, 1), vec![0x13]);
        walker.open_home_event(HomeEvent::Item(0x44));
        assert_eq!(walker.items(), vec![0x44]);
        assert_eq!(walker.eeprom.read(0xCF0C + 0x88 + 0x0E, 2), vec![0x44, 0]);
    }
}
