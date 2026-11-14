use crate::rng::Rng;
use crate::route::Found;
use crate::rules::Rules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Attack,
    Evade,
    Special,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BattleState {
    Appearing,
    Choosing,
    Round { ours: Action, theirs: Action },
    Throw { checks: u8, caught: bool },
    Fled,
    Lost,
    Caught,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battle {
    found: Found,
    ours: u8,
    theirs: u8,
    row: usize,
    fleeing: bool,
    next: Action,
    draws: u32,
    state: BattleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BattleEnd {
    Fled,
    Lost,
    Caught,
}

impl Battle {
    pub(crate) fn new(found: Found, rules: &Rules) -> Self {
        Self {
            found,
            ours: rules.battle_hp,
            theirs: rules.battle_hp,
            row: 0,
            fleeing: false,
            next: Action::Attack,
            draws: rules.battle_intro_draws,
            state: BattleState::Appearing,
        }
    }

    pub fn found(&self) -> Found {
        self.found
    }

    pub fn our_hp(&self) -> u8 {
        self.ours
    }

    pub fn their_hp(&self) -> u8 {
        self.theirs
    }

    pub fn state(&self) -> BattleState {
        self.state
    }

    fn enemy(&self, rules: &Rules, rng: &mut Rng) -> Action {
        let [_, evade, special] = rules.battle_action_odds[self.row].map(u32::from);
        let roll = rng.percent();
        if roll < special {
            Action::Special
        } else if roll < special + evade {
            Action::Evade
        } else {
            Action::Attack
        }
    }

    pub(crate) fn pass(&mut self, rules: &Rules, rng: &mut Rng, held: bool) {
        if self.state == BattleState::Choosing && !held {
            self.next = self.enemy(rules, rng);
        }
    }

    pub(crate) fn fight(&mut self, ours: Action, rules: &Rules) {
        if self.state != BattleState::Choosing {
            return;
        }
        let theirs = self.next;
        let (hurt_us, hurt_them) = match (ours, theirs) {
            (Action::Attack, Action::Attack) => (1, 1),
            (Action::Attack, Action::Evade) => (1, 0),
            (Action::Attack, Action::Special) => (1, rules.battle_special_damage),
            (Action::Evade, Action::Attack) => (0, rules.battle_counter_damage),
            (Action::Evade, Action::Special) => {
                self.fleeing = true;
                (0, 0)
            }
            _ => (0, 0),
        };
        match (ours, theirs) {
            (Action::Attack, theirs) => self.row = rules.battle_row_after[theirs as usize],
            (Action::Evade, Action::Attack) => self.row = rules.battle_evade_row_after[0],
            (Action::Evade, Action::Evade) => self.row = rules.battle_evade_row_after[1],
            _ => {}
        }
        self.ours = self.ours.saturating_sub(hurt_us);
        self.theirs = self.theirs.saturating_sub(hurt_them);
        self.draws = rules.battle_round_draws;
        self.state = BattleState::Round { ours, theirs };
    }

    pub(crate) fn proceed(&mut self) -> bool {
        if self.state != BattleState::Appearing {
            return false;
        }
        if self.draws == 0 {
            self.state = BattleState::Choosing;
        }
        true
    }

    pub(crate) fn throw(&mut self, rules: &Rules) {
        if self.state != BattleState::Choosing {
            return;
        }
        self.draws = rules.catch_check_draws;
        self.state = BattleState::Throw {
            checks: 0,
            caught: false,
        };
    }

    pub(crate) fn draw(&mut self, rules: &Rules, rng: &mut Rng) {
        if self.draws == 0 {
            return;
        }
        self.draws -= 1;
        if self.draws > 0 {
            return;
        }
        self.state = match self.state {
            BattleState::Appearing => BattleState::Appearing,
            BattleState::Round { .. } if self.theirs == 0 || self.fleeing => BattleState::Fled,
            BattleState::Round { .. } if self.ours == 0 => BattleState::Lost,
            BattleState::Round { .. } => BattleState::Choosing,
            BattleState::Throw {
                checks,
                caught: false,
            } => {
                let chance = u32::from(rules.catch_odds[(self.theirs.max(1) - 1) as usize]);
                if rng.percent() >= chance {
                    BattleState::Fled
                } else if checks + 1 >= rules.catch_checks {
                    BattleState::Caught
                } else {
                    self.draws = rules.catch_check_draws;
                    BattleState::Throw {
                        checks: checks + 1,
                        caught: false,
                    }
                }
            }
            state => state,
        };
    }

    pub(crate) fn end(&self) -> Option<BattleEnd> {
        match self.state {
            BattleState::Fled => Some(BattleEnd::Fled),
            BattleState::Lost => Some(BattleEnd::Lost),
            BattleState::Caught => Some(BattleEnd::Caught),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(battle: &mut Battle, rng: &mut Rng) {
        let rules = Rules::ORIGINAL;
        for _ in 0..64 {
            battle.draw(&rules, rng);
        }
    }

    fn ready(rules: &Rules, rng: &mut Rng) -> Battle {
        let mut battle = Battle::new(Found::Route(0), rules);
        settle(&mut battle, rng);
        assert!(battle.proceed());
        battle
    }

    #[test]
    fn the_intro_waits_for_a_press_once_it_has_played() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(3);
        let mut battle = Battle::new(Found::Route(0), &rules);
        battle.draw(&rules, &mut rng);
        assert!(battle.proceed());
        assert_eq!(battle.state(), BattleState::Appearing);
        settle(&mut battle, &mut rng);
        assert_eq!(battle.state(), BattleState::Appearing);
        assert!(battle.proceed());
        assert_eq!(battle.state(), BattleState::Choosing);
        assert!(!battle.proceed());
    }

    fn fight(battle: &mut Battle, ours: Action, rules: &Rules, rng: &mut Rng) -> Action {
        battle.pass(rules, rng, false);
        battle.fight(ours, rules);
        let BattleState::Round { theirs, .. } = battle.state else {
            unreachable!()
        };
        theirs
    }

    fn enemy_counts(row: usize) -> [u32; 3] {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(17);
        let mut battle = Battle::new(Found::Route(0), &rules);
        battle.row = row;
        let mut counts = [0; 3];
        for _ in 0..10_000 {
            counts[battle.enemy(&rules, &mut rng) as usize] += 1;
        }
        counts
    }

    #[test]
    fn the_odds_tables_match_the_rom_byte_for_byte() {
        let rules = Rules::ORIGINAL;
        let bytes: Vec<u8> = rules.battle_action_odds.iter().flatten().copied().collect();
        assert_eq!(
            bytes,
            [45, 35, 20, 40, 30, 30, 50, 40, 10, 60, 30, 10, 20, 30, 50]
        );
        assert_eq!(rules.catch_odds, [97, 79, 66, 56]);
    }

    #[test]
    fn the_enemy_follows_its_rows_odds() {
        for (row, odds) in Rules::ORIGINAL.battle_action_odds.iter().enumerate() {
            let counts = enemy_counts(row);
            for (count, percent) in counts.iter().zip(odds) {
                let expected = *percent as u32 * 100;
                assert!(count.abs_diff(expected) < 250, "row {row}: {counts:?}");
            }
        }
    }

    #[test]
    fn the_enemy_keeps_the_action_rolled_before_the_press() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(3);
        let mut battle = ready(&rules, &mut rng);
        battle.pass(&rules, &mut rng, false);
        let rolled = battle.next;
        let state = rng.state();
        battle.pass(&rules, &mut rng, true);
        assert_eq!(rng.state(), state);
        battle.fight(Action::Evade, &rules);
        assert!(matches!(battle.state, BattleState::Round { theirs, .. } if theirs == rolled));
    }

    #[test]
    fn the_row_follows_what_the_enemy_did() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(2);
        let mut seen = [false; 3];
        for _ in 0..200 {
            let mut battle = ready(&rules, &mut rng);
            battle.row = 4;
            let theirs = fight(&mut battle, Action::Attack, &rules, &mut rng);
            let expected = match theirs {
                Action::Attack => 1,
                Action::Evade => 3,
                Action::Special => 2,
            };
            assert_eq!(battle.row, expected);
            seen[theirs as usize] = true;

            let mut battle = ready(&rules, &mut rng);
            battle.row = 0;
            let theirs = fight(&mut battle, Action::Evade, &rules, &mut rng);
            let expected = match theirs {
                Action::Attack => 1,
                Action::Evade => 4,
                Action::Special => 0,
            };
            assert_eq!(battle.row, expected);
        }
        assert_eq!(seen, [true; 3]);
    }

    #[test]
    fn each_pair_of_actions_does_its_damage() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(4);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..400 {
            for ours in [Action::Attack, Action::Evade] {
                let mut battle = ready(&rules, &mut rng);
                let theirs = fight(&mut battle, ours, &rules, &mut rng);
                let expected = match (ours, theirs) {
                    (Action::Attack, Action::Attack) => (3, 3, false),
                    (Action::Attack, Action::Evade) => (3, 4, false),
                    (Action::Attack, Action::Special) => (3, 2, false),
                    (Action::Evade, Action::Attack) => (4, 3, false),
                    (Action::Evade, Action::Evade) => (4, 4, false),
                    (Action::Evade, Action::Special) => (4, 4, true),
                    _ => unreachable!(),
                };
                assert_eq!((battle.ours, battle.theirs, battle.fleeing), expected);
                settle(&mut battle, &mut rng);
                let after = if expected.2 {
                    BattleState::Fled
                } else {
                    BattleState::Choosing
                };
                assert_eq!(battle.state, after);
                seen.insert((ours, theirs));
            }
        }
        assert_eq!(seen.len(), 6);
    }

    #[test]
    fn zero_hp_ends_the_battle_with_them_fleeing_first() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(6);
        let mut ends = std::collections::HashSet::new();
        for _ in 0..100 {
            let mut battle = ready(&rules, &mut rng);
            while battle.end().is_none() {
                fight(&mut battle, Action::Attack, &rules, &mut rng);
                settle(&mut battle, &mut rng);
            }
            match battle.end().unwrap() {
                BattleEnd::Fled => assert_eq!(battle.theirs, 0),
                BattleEnd::Lost => assert!(battle.ours == 0 && battle.theirs > 0),
                BattleEnd::Caught => unreachable!(),
            }
            ends.insert(battle.end());
        }
        assert_eq!(ends.len(), 2);
    }

    #[test]
    fn three_catch_checks_by_their_hp_give_the_documented_odds() {
        let rules = Rules::ORIGINAL;
        let mut rng = Rng::new(12);
        for (hp, overall) in [(1u8, 91u32), (2, 49), (3, 29), (4, 18)] {
            let mut caught = 0u32;
            for _ in 0..10_000 {
                let mut battle = ready(&rules, &mut rng);
                battle.theirs = hp;
                battle.throw(&rules);
                settle(&mut battle, &mut rng);
                if battle.end() == Some(BattleEnd::Caught) {
                    caught += 1;
                }
            }
            assert!(caught.abs_diff(overall * 100) < 200, "hp {hp}: {caught}");
        }
    }
}
