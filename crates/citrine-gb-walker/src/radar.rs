use crate::eeprom::Eeprom;
use crate::rng::Rng;
use crate::route::{self, Found};
use crate::rules::Rules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RadarState {
    Looking,
    Hit,
    Failed,
    Starting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Radar {
    found: Found,
    target: u8,
    level: u8,
    bush: u8,
    cursor: u8,
    hidden: u32,
    active: u32,
    hold: u32,
    state: RadarState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadarStep {
    Stay,
    Battle(Found),
    Home,
}

pub(crate) fn choose(
    eeprom: &Eeprom,
    rules: &Rules,
    rng: &mut Rng,
    today: u32,
    special: bool,
) -> (Found, u8) {
    let found = pick(eeprom, rng, today, special);
    let hits = match found {
        Found::Event => rules.radar_event_hits,
        Found::Route(slot) => rules.radar_slot_hits[slot as usize],
        Found::GroupC => rules.radar_fallback_hits,
    };
    let bonus = ((rng.next() >> 3) & 1) as u8;
    (found, (hits + bonus).saturating_sub(1))
}

fn pick(eeprom: &Eeprom, rng: &mut Rng, today: u32, special: bool) -> Found {
    if let Some((_, chance)) = route::event_pokemon(eeprom, special)
        && today >= chance.steps
        && rng.whole_percent() < chance.percent as u32
    {
        return Found::Event;
    }
    let roll = rng.whole_percent();
    (0..3)
        .find(|slot| {
            let chance = route::route_pokemon_chance(eeprom, *slot);
            today >= chance.steps && roll < chance.percent as u32
        })
        .map_or(Found::GroupC, |slot| Found::Route(slot as u8))
}

impl Radar {
    pub(crate) fn start(
        eeprom: &Eeprom,
        rules: &Rules,
        rng: &mut Rng,
        today: u32,
        special: bool,
    ) -> Self {
        let (found, target) = choose(eeprom, rules, rng, today, special);
        let target = target.min(rules.radar_shown_passes.len() as u8 - 1);
        let mut radar = Self {
            found,
            target,
            level: 0,
            bush: 0,
            cursor: 0,
            hidden: 0,
            active: 0,
            hold: 0,
            state: RadarState::Looking,
        };
        radar.arm(rules, rng);
        radar
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn target(&self) -> u8 {
        self.target
    }

    pub fn cursor(&self) -> u8 {
        self.cursor
    }

    pub fn state(&self) -> RadarState {
        self.state
    }

    pub fn shown(&self) -> Option<u8> {
        (self.state == RadarState::Looking && self.hidden == 0 && self.active > 0)
            .then_some(self.bush)
    }

    fn arm(&mut self, rules: &Rules, rng: &mut Rng) {
        let level = self.level as usize;
        self.bush = (rng.low(5) % u32::from(rules.radar_bushes.max(1))) as u8;
        self.hidden = match level {
            0 => rules.radar_first_hidden_passes,
            _ => {
                rules.radar_hidden_base
                    + rng.low(2) % rules.radar_hidden_spread[(level - 1).min(2)].max(1)
            }
        };
        self.active = rules.radar_shown_passes[level];
        self.state = RadarState::Looking;
    }

    pub(crate) fn left(&mut self, rules: &Rules) {
        if self.state == RadarState::Looking {
            self.cursor = (self.cursor + rules.radar_bushes - 1) % rules.radar_bushes;
        }
    }

    pub(crate) fn right(&mut self, rules: &Rules) {
        if self.state == RadarState::Looking {
            self.cursor = (self.cursor + 1) % rules.radar_bushes;
        }
    }

    pub(crate) fn pick(&mut self, rules: &Rules) -> RadarStep {
        match self.state {
            RadarState::Looking => {
                if self.cursor == self.bush {
                    self.state = RadarState::Hit;
                    self.hold = rules.radar_hit_passes;
                } else if self.level > 0 {
                    self.state = RadarState::Failed;
                }
                RadarStep::Stay
            }
            RadarState::Failed => RadarStep::Home,
            RadarState::Hit | RadarState::Starting => RadarStep::Stay,
        }
    }

    pub(crate) fn pass(&mut self, rules: &Rules, rng: &mut Rng) {
        match self.state {
            RadarState::Looking => {
                if self.hidden > 0 {
                    self.hidden -= 1;
                } else if self.active > 0 {
                    self.active -= 1;
                }
                if self.hidden == 0 && self.active == 0 {
                    self.state = RadarState::Failed;
                }
            }
            RadarState::Hit => {
                self.hold = self.hold.saturating_sub(1);
                if self.hold == 0 {
                    if self.level >= self.target {
                        self.state = RadarState::Starting;
                        self.hold = rules.radar_start_draws;
                    } else {
                        self.level += 1;
                        self.arm(rules, rng);
                    }
                }
            }
            RadarState::Starting | RadarState::Failed => {}
        }
    }

    pub(crate) fn draw(&mut self) -> RadarStep {
        if self.state != RadarState::Starting {
            return RadarStep::Stay;
        }
        self.hold = self.hold.saturating_sub(1);
        if self.hold == 0 {
            RadarStep::Battle(self.found)
        } else {
            RadarStep::Stay
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(steps: [u16; 3], percent: [u8; 3]) -> Eeprom {
        let mut eeprom = Eeprom::blank();
        for slot in 0..3 {
            eeprom.write(0x8F52 + 16 * slot, &[10 + slot as u8, 0]);
            eeprom.write(0x8F82 + 2 * slot, &steps[slot].to_le_bytes());
            eeprom.write(0x8F88 + slot, &[percent[slot]]);
        }
        eeprom
    }

    fn found_counts(eeprom: &Eeprom, today: u32) -> [u32; 4] {
        let mut rng = Rng::new(3);
        let mut counts = [0; 4];
        for _ in 0..1000 {
            match choose(eeprom, &Rules::ORIGINAL, &mut rng, today, false).0 {
                Found::Route(slot) => counts[slot as usize] += 1,
                Found::GroupC => counts[3] += 1,
                Found::Event => panic!("no event pokemon on this route"),
            }
        }
        counts
    }

    #[test]
    fn one_roll_takes_the_first_slot_whose_steps_and_percent_allow_it() {
        let eeprom = route([0, 0, 0], [10, 40, 100]);
        let counts = found_counts(&eeprom, 0);
        assert_eq!(counts[3], 0);
        assert!((70..=150).contains(&counts[0]), "{counts:?}");
        assert!((250..=380).contains(&counts[1]), "{counts:?}");
    }

    #[test]
    fn slots_need_todays_steps_and_slot_three_takes_the_rest() {
        let eeprom = route([500, 0, 3000], [99, 0, 99]);
        let counts = found_counts(&eeprom, 400);
        assert_eq!(counts[0] + counts[1] + counts[2], 0);
        let eeprom = route([400, 0, 3000], [100, 0, 99]);
        assert_eq!(found_counts(&eeprom, 400)[0], 1000);
    }

    #[test]
    fn hits_follow_the_slot_with_one_random_hit_on_top() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(9);
        for (slot, base) in [(0usize, 2u8), (1, 1), (2, 0)] {
            let mut percent = [0; 3];
            percent[slot] = 100;
            let eeprom = route([0; 3], percent);
            let levels: Vec<u8> = (0..50)
                .map(|_| choose(&eeprom, &rules, &mut rng, 0, false).1)
                .collect();
            assert!(levels.iter().all(|l| (base..=base + 1).contains(l)));
            assert!(levels.contains(&base) && levels.contains(&(base + 1)));
        }
    }

    #[test]
    fn an_event_pokemon_comes_first_once_todays_steps_reach_its_own() {
        let mut eeprom = route([0; 3], [100; 3]);
        eeprom.write(0xBF08, &[0xEC, 0x01]);
        eeprom.write(0xBF44, &[100, 0, 100]);
        eeprom.write(0xBF7A, &[1]);
        let mut rng = Rng::new(1);
        let rules = Rules::ORIGINAL;
        assert_eq!(
            choose(&eeprom, &rules, &mut rng, 99, true).0,
            Found::Route(0)
        );
        let (found, level) = choose(&eeprom, &rules, &mut rng, 100, true);
        assert_eq!(found, Found::Event);
        assert!((2..=3).contains(&level));
    }

    #[test]
    fn the_first_bush_hides_five_passes_then_shows_for_sixty_four() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(5);
        let mut radar = Radar::start(&route([0; 3], [100; 3]), &rules, &mut rng, 0, false);
        for _ in 0..5 {
            assert_eq!(radar.shown(), None);
            radar.pass(&rules, &mut rng);
        }
        for _ in 0..64 {
            assert_eq!(radar.shown(), Some(radar.bush));
            radar.pass(&rules, &mut rng);
        }
        assert_eq!(radar.state(), RadarState::Failed);
        assert_eq!(radar.pick(&rules), RadarStep::Home);
    }

    #[test]
    fn a_wrong_bush_only_fails_after_the_first_hit() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(5);
        let mut radar = Radar::start(&route([0; 3], [100, 0, 0]), &rules, &mut rng, 0, false);
        radar.cursor = (radar.bush + 1) % 4;
        radar.pick(&rules);
        assert_eq!(radar.state(), RadarState::Looking);
        radar.cursor = radar.bush;
        radar.pick(&rules);
        assert_eq!(radar.state(), RadarState::Hit);
        for _ in 0..rules.radar_hit_passes {
            radar.pass(&rules, &mut rng);
        }
        assert_eq!((radar.state(), radar.level()), (RadarState::Looking, 1));
        radar.cursor = (radar.bush + 1) % 4;
        radar.pick(&rules);
        assert_eq!(radar.state(), RadarState::Failed);
    }

    #[test]
    fn each_hit_climbs_a_level_with_shorter_windows_until_the_battle() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(8);
        let mut radar = Radar::start(&route([0; 3], [100, 0, 0]), &rules, &mut rng, 0, false);
        let target = radar.target();
        assert!((2..=3).contains(&target));
        let mut windows = Vec::new();
        let mut delays = Vec::new();
        loop {
            delays.push(radar.hidden);
            windows.push(radar.active);
            radar.cursor = radar.bush;
            radar.pick(&rules);
            while radar.state() == RadarState::Hit {
                radar.pass(&rules, &mut rng);
            }
            if radar.state() == RadarState::Starting {
                let step = (0..rules.radar_start_draws).map(|_| radar.draw()).last();
                assert_eq!(step, Some(RadarStep::Battle(Found::Route(0))));
                break;
            }
        }
        assert_eq!(windows, rules.radar_shown_passes[..=target as usize]);
        assert_eq!(delays[0], 5);
        for (level, delay) in delays.iter().enumerate().skip(1) {
            assert!((16..16 + 16 * level as u32).contains(delay), "{delays:?}");
        }
    }
}
