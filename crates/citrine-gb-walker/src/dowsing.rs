use crate::eeprom::Eeprom;
use crate::rng::Rng;
use crate::route::{self, ROUTE_ITEM_COUNT};
use crate::rules::Rules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hint {
    Near,
    Far,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prize {
    Item(u16),
    EventItem(u16),
    Watts(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DowsingState {
    Choosing,
    Shaking,
    Hint(Hint),
    Found(Prize),
    Missed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dowsing {
    position: u8,
    cursor: u8,
    tried: u8,
    tries: u8,
    draws: u32,
    state: DowsingState,
}

pub(crate) fn route_item(eeprom: &Eeprom, rng: &mut Rng, today: u32) -> u16 {
    let slot = (0..ROUTE_ITEM_COUNT)
        .find(|slot| {
            let chance = route::route_item_chance(eeprom, *slot);
            today >= chance.steps && rng.low_percent() < chance.percent as u32
        })
        .unwrap_or(ROUTE_ITEM_COUNT - 1);
    route::route_item(eeprom, slot)
}

fn prize(
    eeprom: &Eeprom,
    rules: &Rules,
    rng: &mut Rng,
    today: u32,
    special: bool,
    tries: u8,
) -> Prize {
    if !special {
        return Prize::Item(route_item(eeprom, rng, today));
    }
    if let Some((item, chance)) = route::event_item(eeprom, special)
        && today >= chance.steps
        && rng.percent() < chance.percent as u32
    {
        return Prize::EventItem(item);
    }
    Prize::Watts(u16::from(tries) * rules.dowsing_watts_per_try + rules.dowsing_watts_base)
}

impl Dowsing {
    pub(crate) fn start(rules: &Rules, rng: &mut Rng) -> Self {
        let bushes = u32::from(rules.dowsing_bushes.max(1));
        Self {
            position: (((rng.next() >> 5) & 0xFF) % bushes) as u8,
            cursor: 0,
            tried: 0,
            tries: rules.dowsing_tries,
            draws: 0,
            state: DowsingState::Choosing,
        }
    }

    pub fn cursor(&self) -> u8 {
        self.cursor
    }

    pub fn tries_left(&self) -> u8 {
        self.tries
    }

    pub fn tried(&self, bush: u8) -> bool {
        self.tried & (1 << bush) != 0
    }

    pub fn state(&self) -> DowsingState {
        self.state
    }

    pub fn position(&self) -> Option<u8> {
        matches!(self.state, DowsingState::Found(_) | DowsingState::Missed).then_some(self.position)
    }

    pub(crate) fn left(&mut self, rules: &Rules) {
        if self.state == DowsingState::Choosing {
            self.cursor = (self.cursor + rules.dowsing_bushes - 1) % rules.dowsing_bushes;
        }
    }

    pub(crate) fn right(&mut self, rules: &Rules) {
        if self.state == DowsingState::Choosing {
            self.cursor = (self.cursor + 1) % rules.dowsing_bushes;
        }
    }

    pub(crate) fn pick(&mut self, rules: &Rules) {
        match self.state {
            DowsingState::Choosing if !self.tried(self.cursor) => {
                self.state = DowsingState::Shaking;
                self.draws = rules.dowsing_shake_draws;
            }
            DowsingState::Hint(_) => self.state = DowsingState::Choosing,
            _ => {}
        }
    }

    pub(crate) fn draw(
        &mut self,
        eeprom: &Eeprom,
        rules: &Rules,
        rng: &mut Rng,
        today: u32,
        special: bool,
    ) {
        if self.state != DowsingState::Shaking {
            return;
        }
        self.draws = self.draws.saturating_sub(1);
        if self.draws > 0 {
            return;
        }
        self.tried |= 1 << self.cursor;
        self.state = if self.cursor == self.position {
            DowsingState::Found(prize(eeprom, rules, rng, today, special, self.tries))
        } else if self.tries <= 1 {
            DowsingState::Missed
        } else if self.cursor.abs_diff(self.position) <= rules.dowsing_near {
            DowsingState::Hint(Hint::Near)
        } else {
            DowsingState::Hint(Hint::Far)
        };
        self.tries = self.tries.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(items: [(u16, u16, u8); 10]) -> Eeprom {
        let mut eeprom = Eeprom::blank();
        for (slot, (item, steps, percent)) in items.iter().enumerate() {
            eeprom.write(0x8F8C + 2 * slot, &item.to_le_bytes());
            eeprom.write(0x8FA0 + 2 * slot, &steps.to_le_bytes());
            eeprom.write(0x8FB4 + slot, &[*percent]);
        }
        eeprom
    }

    fn shake(dowsing: &mut Dowsing, eeprom: &Eeprom, rules: &Rules, special: bool) {
        let mut rng = Rng::new(5);
        dowsing.pick(rules);
        for _ in 0..rules.dowsing_shake_draws {
            dowsing.draw(eeprom, rules, &mut rng, 0, special);
        }
    }

    #[test]
    fn each_slot_that_qualifies_rolls_its_own_chance_in_order() {
        let mut items = [(0u16, 0u16, 0u8); 10];
        for (slot, item) in items.iter_mut().enumerate() {
            *item = (100 + slot as u16, 0, 0);
        }
        items[0] = (100, 5000, 100);
        items[1] = (101, 0, 30);
        items[2] = (102, 0, 60);
        let eeprom = route(items);
        let mut rng = Rng::new(21);
        let mut counts = std::collections::BTreeMap::new();
        for _ in 0..2000 {
            *counts
                .entry(route_item(&eeprom, &mut rng, 4999))
                .or_insert(0u32) += 1;
        }
        assert_eq!(counts.keys().copied().collect::<Vec<_>>(), [101, 102, 109]);
        assert!(counts[&101].abs_diff(2000 * 30 / 100) < 120, "{counts:?}");
        assert!(
            counts[&102].abs_diff(2000 * 70 * 60 / 10_000) < 120,
            "{counts:?}"
        );
        assert_eq!(route_item(&eeprom, &mut rng, 5000), 100);
    }

    #[test]
    fn a_special_route_gives_its_event_item_or_watts_by_the_tries_left() {
        let rules = Rules::ORIGINAL;
        let mut eeprom = route([(7, 0, 100); 10]);
        eeprom.write(0xBF48, &[0x2A, 0x00, 0x00, 0x00, 100]);
        let mut rng = Rng::new(1);
        assert_eq!(
            prize(&eeprom, &rules, &mut rng, 0, true, 2),
            Prize::EventItem(42)
        );
        assert_eq!(
            prize(&eeprom, &rules, &mut rng, 0, false, 2),
            Prize::Item(7)
        );
        eeprom.write(0xBF4C, &[0]);
        assert_eq!(
            prize(&eeprom, &rules, &mut rng, 0, true, 2),
            Prize::Watts(10)
        );
        assert_eq!(
            prize(&eeprom, &rules, &mut rng, 0, true, 1),
            Prize::Watts(6)
        );
    }

    #[test]
    fn a_miss_next_to_the_item_is_near_and_two_misses_end_it() {
        let rules = Rules::ORIGINAL;
        let eeprom = route([(9, 0, 100); 10]);
        let mut dowsing = Dowsing::start(&rules, &mut Rng::new(3));
        dowsing.position = 3;
        dowsing.cursor = 2;
        shake(&mut dowsing, &eeprom, &rules, false);
        assert_eq!(dowsing.state(), DowsingState::Hint(Hint::Near));
        assert_eq!(dowsing.position(), None);
        dowsing.pick(&rules);
        dowsing.cursor = 0;
        shake(&mut dowsing, &eeprom, &rules, false);
        assert_eq!(dowsing.state(), DowsingState::Missed);
        assert_eq!((dowsing.position(), dowsing.tries_left()), (Some(3), 0));
    }

    #[test]
    fn the_cursor_wraps_and_a_hit_finds_the_item() {
        let rules = Rules::ORIGINAL;
        let eeprom = route([(9, 0, 100); 10]);
        let mut dowsing = Dowsing::start(&rules, &mut Rng::new(3));
        dowsing.position = 5;
        dowsing.left(&rules);
        assert_eq!(dowsing.cursor(), 5);
        dowsing.right(&rules);
        assert_eq!(dowsing.cursor(), 0);
        dowsing.left(&rules);
        shake(&mut dowsing, &eeprom, &rules, false);
        assert_eq!(dowsing.state(), DowsingState::Found(Prize::Item(9)));
        assert!(dowsing.tried(5));
    }
}
