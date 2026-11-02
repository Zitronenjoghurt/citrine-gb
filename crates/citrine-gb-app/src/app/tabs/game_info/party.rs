use super::parts::{
    badge_color, beside, card, chip, egg_details, exp_color, grouped, hp_color, labelled_meter,
    meter, right_cell, type_color, type_text_color,
};
use super::sprites::{pokemon_sprite, sprite_key};
use super::{LIST_WIDTH, Pages, TILE};
use crate::game_info::View;
use crate::icons;
use citrine_gb_gamedata::{GameState, Gender, Generation, Mon, Pokemon, Pokerus, TimeOfDay};
use egui::{Align, Color32, Layout, RichText, Sense, Ui, vec2};

impl Pages<'_> {
    pub(super) fn party(&self, ui: &mut Ui, state: &GameState, view: &mut View) {
        if state.party.is_empty() {
            ui.label(RichText::new("No Pokémon in the party").weak());
            return;
        }
        view.party_pick = view.party_pick.min(state.party.len() - 1);
        if self.mobile {
            for (i, pokemon) in state.party.iter().enumerate() {
                if self.party_card(ui, pokemon).clicked() && matches!(pokemon, Pokemon::Hatched(_))
                {
                    view.party_pick = i;
                    view.details = true;
                }
                ui.add_space(6.0);
            }
            return;
        }
        let list = |ui: &mut Ui, view: &mut View| {
            for (i, pokemon) in state.party.iter().enumerate() {
                if self.party_row(ui, pokemon, i == view.party_pick).clicked() {
                    view.party_pick = i;
                }
                ui.add_space(4.0);
            }
        };
        let picked = &state.party[view.party_pick];
        if !beside(ui, LIST_WIDTH) {
            list(ui, view);
            ui.add_space(4.0);
            self.pokemon_details(ui, picked);
            return;
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(LIST_WIDTH);
                list(ui, view);
            });
            ui.vertical(|ui| self.pokemon_details(ui, picked));
        });
    }

    pub(super) fn party_row(
        &self,
        ui: &mut Ui,
        pokemon: &Pokemon,
        selected: bool,
    ) -> egui::Response {
        card(ui, selected)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    self.tile(ui, pokemon_sprite(pokemon), TILE);
                    ui.vertical(|ui| {
                        self.name_row(ui, pokemon, false);
                        if let Pokemon::Hatched(mon) = pokemon {
                            self.hp_bar(ui, mon);
                        }
                    });
                });
            })
            .response
            .interact(Sense::click())
    }

    pub(super) fn party_card(&self, ui: &mut Ui, pokemon: &Pokemon) -> egui::Response {
        card(ui, false)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    self.tile(ui, pokemon_sprite(pokemon), TILE);
                    ui.vertical(|ui| {
                        self.name_row(ui, pokemon, true);
                        if let Pokemon::Hatched(mon) = pokemon {
                            self.hp_bar(ui, mon);
                        }
                    });
                });
                if let Pokemon::Hatched(mon) = pokemon {
                    let names: Vec<&str> = mon
                        .moves
                        .iter()
                        .filter_map(|slot| self.game.tables.move_name(slot.id))
                        .collect();
                    egui::Grid::new(("party_moves", mon.species, mon.exp))
                        .num_columns(2)
                        .min_col_width((ui.available_width() - 8.0) / 2.0)
                        .show(ui, |ui| {
                            for (i, name) in names.iter().enumerate() {
                                chip(ui, name);
                                if i % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                }
            })
            .response
            .interact(Sense::click())
    }

    pub(super) fn name_row(&self, ui: &mut Ui, pokemon: &Pokemon, chevron: bool) {
        ui.horizontal(|ui| {
            match pokemon {
                Pokemon::Hatched(mon) => {
                    ui.add(
                        egui::Label::new(RichText::new(&mon.nickname).size(16.0).strong())
                            .truncate(),
                    );
                    self.gender_mark(ui, mon);
                }
                Pokemon::Egg => {
                    ui.label(RichText::new("Egg").size(16.0).strong());
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if chevron {
                    ui.label(RichText::new(icons::CARET_RIGHT).weak());
                }
                if let Pokemon::Hatched(mon) = pokemon {
                    ui.label(format!("Lv {}", mon.level));
                }
            });
        });
    }

    pub(super) fn gender_mark(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        let shiny = tables.game.generation() == Generation::Two && mon.dvs.shiny();
        match tables.mon_gender(mon) {
            Some(Gender::Male) => {
                ui.label(RichText::new("♂").color(Color32::from_rgb(0x4a, 0x8f, 0xe0)));
            }
            Some(Gender::Female) => {
                ui.label(RichText::new("♀").color(Color32::from_rgb(0xe0, 0x60, 0x8a)));
            }
            None => {}
        }
        if shiny {
            ui.label(RichText::new(icons::SPARKLE).color(badge_color(ui)))
                .on_hover_text("Shiny");
        }
    }

    pub(super) fn hp_bar(&self, ui: &mut Ui, mon: &Mon) {
        let Some(max) = self.stats(mon).map(|s| s.max_hp) else {
            return;
        };
        let hp = mon.hp.unwrap_or(max).min(max);
        let fraction = f32::from(hp) / f32::from(max.max(1));
        meter(ui, fraction, hp_color(fraction));
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(RichText::new(format!("{hp} / {max} HP")).small());
        });
    }

    pub(super) fn stats(&self, mon: &Mon) -> Option<citrine_gb_gamedata::Stats> {
        mon.stats.or_else(|| self.game.tables.mon_stats(mon))
    }

    pub(super) fn pokemon_details(&self, ui: &mut Ui, pokemon: &Pokemon) {
        match pokemon {
            Pokemon::Hatched(mon) => self.details(ui, mon),
            Pokemon::Egg => egg_details(ui),
        }
    }

    pub(super) fn details(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        let wide = !self.mobile && ui.available_width() >= 400.0;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                self.tile(
                    ui,
                    Some(sprite_key(tables.game.generation(), mon)),
                    TILE * 2.0,
                );
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&mon.nickname).size(20.0).strong())
                                .truncate(),
                        );
                        self.gender_mark(ui, mon);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("Lv {}", mon.level)).size(15.0));
                        });
                    });
                    let species = tables.species_name(mon.species).unwrap_or_default();
                    let dex = tables.dex_number(mon.species).unwrap_or_default();
                    let number = if species == mon.nickname {
                        format!("No. {dex}")
                    } else {
                        format!("No. {dex} {species}")
                    };
                    ui.label(RichText::new(number).weak());
                    self.types(ui, mon);
                    if let (Some(hp), Some(stats)) = (mon.hp, self.stats(mon)) {
                        let max = stats.max_hp;
                        labelled_meter(
                            ui,
                            "HP",
                            &format!("{} / {max}", hp.min(max)),
                            f32::from(hp) / f32::from(max.max(1)),
                            hp_color(f32::from(hp) / f32::from(max.max(1))),
                        );
                    }
                    if let Some((text, fraction)) = self.exp_line(mon) {
                        labelled_meter(ui, "EXP", &text, fraction, exp_color(ui));
                    }
                });
            });
            ui.add_space(8.0);
            if !wide {
                self.stat_table(ui, mon);
                ui.add_space(8.0);
                self.moves(ui, mon);
                ui.add_space(8.0);
                self.facts(ui, mon);
            } else {
                ui.columns(2, |columns| {
                    self.stat_table(&mut columns[0], mon);
                    self.moves(&mut columns[1], mon);
                    columns[1].add_space(8.0);
                    self.facts(&mut columns[1], mon);
                });
            }
        });
    }

    pub(super) fn exp_line(&self, mon: &Mon) -> Option<(String, f32)> {
        let tables = &self.game.tables;
        let base = tables.base_stats(mon.species)?;
        if mon.level >= 100 {
            return None;
        }
        let start = base.growth_rate.exp_at(mon.level);
        let next = base.growth_rate.exp_at(mon.level + 1);
        let left = tables.exp_to_next_level(mon)?;
        let fraction =
            mon.exp.saturating_sub(start) as f32 / next.saturating_sub(start).max(1) as f32;
        Some((
            format!("{} EXP to Lv {}", grouped(left), mon.level + 1),
            fraction,
        ))
    }

    pub(super) fn types(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        let Some(base) = tables.base_stats(mon.species) else {
            return;
        };
        let mut ids = base.types.to_vec();
        ids.dedup();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for id in ids {
                let Some(name) = tables.type_name(id).filter(|name| !name.is_empty()) else {
                    continue;
                };
                egui::Frame::new()
                    .fill(type_color(id))
                    .corner_radius(3.0)
                    .inner_margin(egui::Margin::symmetric(6, 1))
                    .show(ui, |ui| {
                        ui.label(RichText::new(name).small().color(type_text_color(id)));
                    });
            }
        });
    }

    pub(super) fn stat_table(&self, ui: &mut Ui, mon: &Mon) {
        let Some(stats) = self.stats(mon) else {
            return;
        };
        let dvs = mon.dvs;
        let gen2 = self.game.tables.game.generation() == Generation::Two;
        let mut rows = vec![
            ("HP", stats.max_hp, dvs.hp()),
            ("Attack", stats.attack, dvs.attack),
            ("Defense", stats.defense, dvs.defense),
        ];
        if gen2 {
            rows.push(("Sp. Atk", stats.special_attack, dvs.special));
            rows.push(("Sp. Def", stats.special_defense, dvs.special));
            rows.push(("Speed", stats.speed, dvs.speed));
        } else {
            rows.push(("Speed", stats.speed, dvs.speed));
            rows.push(("Special", stats.special_attack, dvs.special));
        }
        let hint = if gen2 {
            "Hidden values from 0 to 15 that raise each stat. Sp. Atk and Sp. Def share one."
        } else {
            "Hidden values from 0 to 15 that raise each stat."
        };
        ui.scope(|ui| {
            ui.set_max_width(280.0);
            egui::Grid::new(("stats", mon.species, mon.exp))
                .num_columns(3)
                .spacing(vec2(18.0, 2.0))
                .min_col_width(0.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("Stats").weak());
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("DV").weak());
                        crate::app::widgets::info_mark::info_mark(ui, hint);
                    });
                    ui.end_row();
                    for (name, value, dv) in rows {
                        ui.label(name);
                        right_cell(ui, 32.0, RichText::new(value.to_string()).strong());
                        right_cell(ui, 32.0, RichText::new(dv.to_string()));
                        ui.end_row();
                    }
                });
        });
    }

    pub(super) fn moves(&self, ui: &mut Ui, mon: &Mon) {
        ui.label(RichText::new("Moves").weak());
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for slot in &mon.moves {
                let Some(name) = self.game.tables.move_name(slot.id) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("{} PP", slot.pp));
                    });
                });
            }
        });
    }

    pub(super) fn facts(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        let mut facts: Vec<(&str, String)> = Vec::new();
        if tables.game.generation() == Generation::Two {
            let item = mon
                .held_item
                .filter(|item| *item != 0)
                .and_then(|item| tables.item_name(item))
                .unwrap_or_else(|| "None".to_owned());
            facts.push(("Held item", item));
        }
        if let Some(friendship) = mon.friendship {
            facts.push(("Friendship", format!("{friendship} of 255")));
        }
        match mon.pokerus_status() {
            Some(Pokerus::Infected { .. }) => facts.push(("Pokérus", "Infected".to_owned())),
            Some(Pokerus::Cured { .. }) => facts.push(("Pokérus", "Cured".to_owned())),
            None => {}
        }
        if let Some(caught) = &mon.caught
            && let Some(place) = tables.caught_place(caught)
        {
            let place = match place {
                citrine_gb_gamedata::Place::Landmark(name) => name.to_owned(),
                citrine_gb_gamedata::Place::Gift => "A gift".to_owned(),
                citrine_gb_gamedata::Place::Event => "An event".to_owned(),
            };
            let time = match caught.time_of_day {
                TimeOfDay::Morning => " in the morning",
                TimeOfDay::Day => " by day",
                TimeOfDay::Night => " at night",
                TimeOfDay::Unknown => "",
            };
            facts.push(("Caught", format!("{place}{time}, Lv {}", caught.level)));
        }
        if let Some(state) = self.game.state()
            && (mon.ot_id != state.trainer.id || mon.ot_name != state.trainer.name)
        {
            facts.push((
                "Original trainer",
                format!("{}, ID {:05}", mon.ot_name, mon.ot_id),
            ));
        }
        egui::Grid::new(("facts", mon.species, mon.exp))
            .num_columns(2)
            .spacing(vec2(16.0, 2.0))
            .show(ui, |ui| {
                for (label, value) in facts {
                    ui.label(RichText::new(label).weak());
                    ui.label(value);
                    ui.end_row();
                }
            });
    }
}
