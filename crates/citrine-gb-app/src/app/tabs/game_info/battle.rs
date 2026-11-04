use super::parts::{card, hp_color, labelled_meter, paint_caught_mark};
use super::party::{MoveRow, pp_text};
use super::{Pages, TILE};
use citrine_gb_gamedata::{Battle, BattleKind, Effect, Fighter, GameState};
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, vec2};

pub(super) fn effect_text(effect: Effect) -> &'static str {
    match effect {
        Effect::NoEffect => "No effect",
        Effect::Quarter | Effect::Half => "Not very effective",
        Effect::Normal => "Effective",
        Effect::Double | Effect::Quadruple => "Super effective",
    }
}

fn effect_color(ui: &Ui, effect: Effect) -> Color32 {
    match effect {
        Effect::NoEffect | Effect::Quarter | Effect::Half => ui.visuals().weak_text_color(),
        Effect::Normal => ui.visuals().text_color(),
        Effect::Double | Effect::Quadruple => hp_color(1.0),
    }
}

impl Pages<'_> {
    pub(super) fn battle(&self, ui: &mut Ui, state: &GameState) {
        let Some(battle) = &state.battle else {
            return;
        };
        let tables = &self.game.tables;
        let caught = tables
            .dex_number(battle.enemy.species)
            .is_some_and(|dex| state.pokedex.caught.contains(&dex));
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let heading = match battle.kind {
                BattleKind::Wild => "Wild Pokémon",
                BattleKind::Trainer => "Trainer's Pokémon",
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(heading).weak());
                if battle.kind == BattleKind::Trainer {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        party_balls(ui, battle);
                    });
                }
            });
            self.fighter(ui, &battle.enemy, caught);
        });
        ui.add_space(8.0);
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Your Pokémon").weak());
            self.fighter(ui, &battle.player, false);
            ui.add_space(6.0);
            self.battle_moves(ui, battle);
        });
    }

    fn fighter(&self, ui: &mut Ui, fighter: &Fighter, caught: bool) {
        let tables = &self.game.tables;
        let name = tables.species_name(fighter.species).unwrap_or("?");
        ui.horizontal_top(|ui| {
            self.tile(ui, Some((fighter.species, false, 'A')), TILE);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(name).size(16.0).strong()).truncate());
                    if caught {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        paint_caught_mark(ui.painter(), rect.center());
                        response.on_hover_text("Caught");
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("Lv {}", fighter.level));
                    });
                });
                let mut types = fighter.types.to_vec();
                types.dedup();
                self.type_tags(ui, &types);
                let fraction = f32::from(fighter.hp) / f32::from(fighter.max_hp.max(1));
                labelled_meter(
                    ui,
                    "HP",
                    &format!("{} / {}", fighter.hp, fighter.max_hp),
                    fraction,
                    hp_color(fraction),
                );
            });
        });
    }

    fn battle_moves(&self, ui: &mut Ui, battle: &Battle) {
        let tables = &self.game.tables;
        let rows: Vec<MoveRow> = battle
            .player
            .moves
            .iter()
            .filter_map(|slot| {
                Some(MoveRow {
                    name: tables.move_name(slot.id)?.to_owned(),
                    type_id: tables.move_data(slot.id).map(|data| data.type_id),
                    effect: tables
                        .effect(slot.id, battle.enemy.types)
                        .map(|effect| (effect_text(effect), effect_color(ui, effect))),
                    pp: pp_text(slot.pp, Some(tables.max_pp(*slot).unwrap_or(slot.pp))),
                })
            })
            .collect();
        self.move_rows(ui, &rows);
    }
}

fn party_balls(ui: &mut Ui, battle: &Battle) {
    let side = 10.0;
    let gap = 4.0;
    let count = battle.trainer_party.len();
    let width = count as f32 * side + count.saturating_sub(1) as f32 * gap;
    let (rect, response) = ui.allocate_exact_size(vec2(width, side), Sense::hover());
    let left = battle.trainer_party.iter().filter(|alive| **alive).count();
    for (i, alive) in battle.trainer_party.iter().enumerate() {
        let center = egui::pos2(
            rect.left() + side / 2.0 + i as f32 * (side + gap),
            rect.center().y,
        );
        if *alive {
            paint_caught_mark(ui.painter(), center);
        } else {
            ui.painter().circle_stroke(
                center,
                side / 2.0,
                Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
            );
        }
    }
    response.on_hover_text(format!("{left} of {count} Pokémon left"));
}
