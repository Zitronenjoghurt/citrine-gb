use super::{ItemStack, Mon, Pokemon, RomTables, SaveState};
use crate::state::{
    self, Bag, Caught, GameState, Gen3Genes, Genes, Milestone, MoveSlot, PcBox, Position, Step,
    TimeOfDay, Trainer,
};

impl SaveState {
    pub fn to_game_state(&self, tables: &RomTables) -> GameState {
        let trainer = &self.trainer;
        let mut journey = vec![Step {
            milestone: Milestone::Pokedex,
            done: self.pokedex_received,
        }];
        journey.extend(self.badges.iter().enumerate().map(|(i, &done)| Step {
            milestone: Milestone::Badge(i),
            done,
        }));
        journey.push(Step {
            milestone: Milestone::EliteFour,
            done: self.champion,
        });
        GameState {
            game: tables.game,
            trainer: Trainer {
                name: trainer.name.clone(),
                id: trainer.id,
                gender: Some(trainer.gender),
                money: trainer.money,
                mom_money: None,
                coins: trainer.coins,
                play_time: trainer.play_time,
                badges: self.badges.clone(),
            },
            party: self.party.iter().map(|p| pokemon(tables, p)).collect(),
            boxes: self
                .boxes
                .iter()
                .map(|pc| PcBox {
                    number: pc.number,
                    name: Some(pc.name.clone()),
                    pokemon: pc
                        .slots
                        .iter()
                        .flatten()
                        .map(|p| pokemon(tables, p))
                        .collect(),
                })
                .collect(),
            current_box: self.current_box,
            bag: Bag {
                items: stacks(&self.bag.items),
                key_items: stacks(&self.bag.key_items),
                balls: stacks(&self.bag.balls),
                tms_hms: stacks(&self.bag.tms_hms),
                berries: stacks(&self.bag.berries),
            },
            pc_items: stacks(&self.pc_items),
            pokedex: self.pokedex.clone(),
            copy: self.copy,
            location: Some(Position::Gba(self.whereabouts)),
            journey,
            hall_of_fame: None,
            battle: None,
            clock: None,
        }
    }
}

fn stacks(list: &[ItemStack]) -> Vec<state::ItemStack> {
    list.iter()
        .map(|stack| state::ItemStack {
            item: stack.item,
            count: stack.count,
        })
        .collect()
}

fn pokemon(tables: &RomTables, pokemon: &Pokemon) -> state::Pokemon {
    match pokemon {
        Pokemon::BadEgg | Pokemon::Egg(_) => state::Pokemon::Egg,
        Pokemon::Hatched(mon) => state::Pokemon::Hatched(Box::new(fold_mon(tables, mon))),
    }
}

fn fold_mon(tables: &RomTables, mon: &Mon) -> state::Mon {
    state::Mon {
        species: mon.species,
        nickname: mon.nickname.clone(),
        ot_name: mon.ot_name.clone(),
        ot_id: mon.ot_id,
        level: tables.mon_level(mon).or(mon.level).unwrap_or(1),
        exp: mon.exp,
        moves: mon
            .moves
            .iter()
            .map(|slot| MoveSlot {
                id: slot.id,
                pp: slot.pp,
                pp_ups: slot.pp_ups,
            })
            .collect(),
        held_item: mon.held_item,
        genes: Genes::Gba(Gen3Genes {
            personality: mon.personality,
            ot_secret_id: mon.ot_secret_id,
            nature: mon.nature(),
            ivs: mon.ivs,
            evs: mon.evs,
            ability_slot: mon.ability_slot,
            ball: mon.met.ball,
        }),
        hp: mon.party.map(|party| party.hp),
        stats: tables.mon_stats(mon),
        status: mon.party.map(|party| party.status as u8),
        friendship: Some(mon.friendship),
        pokerus: Some(mon.pokerus),
        caught: Some(Caught {
            time_of_day: TimeOfDay::Unknown,
            level: mon.met.level,
            ot_gender: mon.met.ot_gender,
            location: mon.met.location,
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::gen3_game::{gen3_sample_save, gen3_state, hatched};
    use crate::gen3::{Layout, Nature};
    use crate::{Generation, ItemStack, Milestone, Pokemon, Position};

    #[test]
    fn gen3_saves_fold_into_game_state() {
        for layout in [
            Layout::RubySapphire,
            Layout::Emerald,
            Layout::FireRedLeafGreen,
        ] {
            let state = gen3_state(layout, &gen3_sample_save(layout));
            assert_eq!(state.game.generation(), Generation::Three, "{layout:?}");
            assert_eq!(state.trainer.name, "BRENDAN");
            assert_eq!(state.trainer.badges.iter().filter(|&&b| b).count(), 3);
            assert_eq!(state.party.len(), 2);
            let lead = hatched(&state.party[0]);
            assert_eq!(lead.species, 277);
            assert_eq!(lead.hp, Some(30));
            assert!(lead.stats.is_some());
            assert_eq!(lead.dvs(), None);
            let genes = lead.gen3().expect("gen 3 genes");
            assert_eq!(genes.personality, 0x1234_5678);
            assert_eq!(genes.nature, Nature::Gentle);
            assert_eq!(genes.ivs.hp, 31);
            assert_eq!(genes.evs.attack, 252);
            assert_eq!(state.boxes[0].pokemon.len(), 2);
            assert!(hatched(&state.boxes[0].pokemon[0]).stats.is_some());
            assert_eq!(state.boxes[0].pokemon[1], Pokemon::Egg);
            assert_eq!(state.boxes[0].name.as_deref(), Some("BOX0"));
            assert_eq!(
                state.bag.berries,
                vec![ItemStack {
                    item: 133,
                    count: 3,
                }]
            );
            assert!(matches!(state.location, Some(Position::Gba(_))));
            let done: Vec<(Milestone, bool)> = state
                .journey
                .iter()
                .map(|step| (step.milestone, step.done))
                .collect();
            assert_eq!(done.len(), 10);
            assert_eq!(done[0], (Milestone::Pokedex, true));
            assert_eq!(done[3], (Milestone::Badge(2), true));
            assert_eq!(done[4], (Milestone::Badge(3), false));
            assert_eq!(done[9], (Milestone::EliteFour, false));
        }
    }
}
