use crate::Walker;
use crate::info::PokemonSummary;
use crate::route::{self, LogEntry, LogKind};

impl Walker {
    pub fn steps(&mut self, count: u32) {
        let rules = &self.rules;
        let ram = &mut self.ram;
        ram.hour_steps =
            (u32::from(ram.hour_steps) + count).min(u32::from(rules.max_hour_steps)) as u16;
        ram.today_steps = ram
            .today_steps
            .saturating_add(count)
            .min(rules.max_today_steps);
        let mut health = self.health();
        health.set_total_steps(
            health
                .total_steps()
                .saturating_add(count)
                .min(rules.max_total_steps),
        );
        let toward = health.steps_toward_watt() as u32 + count;
        let watts = (health.watts() as u32 + toward / rules.steps_per_watt.max(1))
            .min(rules.max_watts as u32);
        health.set_steps_toward_watt((toward % rules.steps_per_watt.max(1)) as u8);
        health.set_watts(watts as u16);
        self.set_health(&health);
        if self.ram.walking {
            self.walk_steps = self.walk_steps.saturating_add(count);
        }
    }

    pub fn home_event(&self) -> Option<crate::HomeEvent> {
        self.home
    }

    pub fn walk_steps(&self) -> u32 {
        self.walk_steps
    }

    pub fn caught(&self) -> Vec<PokemonSummary> {
        route::caught(&self.eeprom, self.rules.max_caught)
    }

    pub fn items(&self) -> Vec<u16> {
        route::items(&self.eeprom, self.rules.max_items)
    }

    pub fn history(&self) -> Vec<u32> {
        route::history(&self.eeprom, self.rules.history_days)
    }

    pub(crate) fn add_watts(&mut self, watts: u16) {
        let mut health = self.health();
        health.set_watts(
            health
                .watts()
                .saturating_add(watts)
                .min(self.rules.max_watts),
        );
        self.set_health(&health);
        self.save_health();
    }

    pub(crate) fn log(
        &mut self,
        kind: LogKind,
        other_species: u16,
        extra: u16,
        other: Option<PokemonSummary>,
    ) {
        let entries = self.rules.event_log_entries.max(1);
        let mut health = self.health();
        let mut index = health.event_log_index() % entries;
        let held = route::log_kind_at(&self.eeprom, index);
        if kind == LogKind::Hour && held != 0 {
            return;
        }
        if held == LogKind::WalkStart as u8 {
            index = (index + 1) % entries;
        }
        let entry = LogEntry {
            kind,
            time: health.clock(),
            other_species,
            extra,
            other,
            hour_steps: self.ram.hour_steps,
            today_steps: self.ram.today_steps,
            special: health.special_route(),
        };
        route::write_log(&mut self.eeprom, index, entry);
        health.set_event_log_index((index + 1) % entries);
        self.set_health(&health);
        self.save_health();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eeprom::{self, Eeprom};

    fn walking() -> Walker {
        let mut eeprom = Eeprom::blank();
        eeprom.write(eeprom::ROUTE, &[25, 0]);
        let mut walker = Walker::from_eeprom(eeprom, 1);
        walker.ram.walking = true;
        walker
    }

    #[test]
    fn twenty_steps_make_a_watt_and_the_rest_carries() {
        let mut walker = Walker::new(1);
        walker.steps(45);
        walker.steps(15);
        let health = walker.health();
        assert_eq!((walker.today_steps(), health.total_steps()), (60, 60));
        assert_eq!((health.watts(), health.steps_toward_watt()), (3, 0));
    }

    #[test]
    fn each_count_stops_at_its_own_cap() {
        let mut walker = Walker::new(1);
        let mut health = walker.health();
        health.set_total_steps(9_999_990);
        health.set_watts(9_998);
        walker.set_health(&health);
        walker.steps(99_990);
        walker.steps(50);
        let health = walker.health();
        assert_eq!(walker.today_steps(), 99_999);
        assert_eq!(walker.ram.hour_steps, 9_999);
        assert_eq!(health.total_steps(), 9_999_999);
        assert_eq!(health.watts(), 9_999);
        assert_eq!(health.steps_toward_watt(), 0);
    }

    #[test]
    fn steps_stay_in_ram_until_the_health_is_saved() {
        let mut walker = Walker::new(1);
        walker.steps(40);
        assert_eq!(walker.eeprom.read(0x0156, 4), vec![0; 4]);
        walker.save_health();
        assert_eq!(walker.eeprom.read(0x0156, 4), 40u32.to_be_bytes());
    }

    #[test]
    fn log_entries_carry_the_walk_and_wrap_after_the_last_slot() {
        let mut walker = walking();
        walker.steps(40);
        walker.log(LogKind::ItemDowsed, 0, 0x44, None);
        let entry = walker.eeprom.read(0xCF0C, 0x88);
        assert_eq!(entry[0x0A..0x0C], [25, 0]);
        assert_eq!(entry[0x0E..0x10], [0x44, 0]);
        assert_eq!(entry[0x78..0x7A], 40u16.to_be_bytes());
        assert_eq!(entry[0x7C..0x80], 40u32.to_be_bytes());
        assert_eq!(entry[0x84], 0x0B);
        for _ in 1..23 {
            walker.log(LogKind::PokemonRan, 0, 0, None);
        }
        assert_eq!(walker.health().event_log_index(), 0);
        assert_eq!(walker.eeprom.read(0xCF0C + 22 * 0x88 + 0x84, 1), vec![0x0F]);
        walker.log(LogKind::Hour, 0, 0, None);
        assert_eq!(walker.health().event_log_index(), 0);
    }
}
