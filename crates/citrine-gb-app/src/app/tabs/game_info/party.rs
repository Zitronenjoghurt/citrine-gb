use super::parts::{
    badge_color, beside, card, chip, egg_details, exp_color, grouped, hp_color, labelled_meter,
    meter, right_cell, text_width, type_color, type_text_color,
};
use super::sprites::{pokemon_sprite, sprite_key};
use super::{DETAILS_TWO_COLUMNS, LIST_WIDTH, Pages, TILE};
use crate::game_info::View;
use crate::icons;
use citrine_gb_gamedata::{
    GameState, Gen3Genes, Gender, Generation, Mon, Pokemon, Pokerus, Stats, TimeOfDay,
};
use egui::{Align, Color32, Layout, RichText, Sense, Ui, vec2};

const NATURE_ROWS: [usize; 5] = [1, 2, 5, 3, 4];

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
                    ui.columns(2, |columns| {
                        for (i, name) in names.iter().enumerate() {
                            chip(&mut columns[i % 2], name);
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
        let shiny = tables.generation() != Generation::One && mon.shiny();
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

    pub(super) fn stats(&self, mon: &Mon) -> Option<Stats> {
        self.game.tables.mon_stats(mon)
    }

    pub(super) fn pokemon_details(&self, ui: &mut Ui, pokemon: &Pokemon) {
        match pokemon {
            Pokemon::Hatched(mon) => self.details(ui, mon),
            Pokemon::Egg => egg_details(ui),
        }
    }

    pub(super) fn details(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        let wide = !self.mobile && ui.available_width() >= DETAILS_TWO_COLUMNS;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                self.tile(ui, Some(sprite_key(tables.generation(), mon)), TILE * 2.0);
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
        let (start, next) = self.game.tables.exp_span(mon)?;
        let left = next.saturating_sub(mon.exp);
        let fraction =
            mon.exp.saturating_sub(start) as f32 / next.saturating_sub(start).max(1) as f32;
        Some((
            format!("{} EXP to Lv {}", grouped(left), mon.level + 1),
            fraction,
        ))
    }

    pub(super) fn types(&self, ui: &mut Ui, mon: &Mon) {
        let ids = self.game.tables.species_type_ids(mon.species);
        self.type_tags(ui, &ids);
    }

    pub(super) fn type_tags(&self, ui: &mut Ui, ids: &[u8]) {
        let tables = &self.game.tables;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for &id in ids {
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
        if let Some(genes) = mon.gen3() {
            self.gen3_stat_table(ui, mon, stats, genes);
            return;
        }
        let Some(dvs) = mon.dvs() else {
            return;
        };
        let gen2 = self.game.tables.generation() == Generation::Two;
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

    fn gen3_stat_table(&self, ui: &mut Ui, mon: &Mon, stats: Stats, genes: &Gen3Genes) {
        let (iv, ev) = (genes.ivs, genes.evs);
        let rows = [
            ("HP", stats.max_hp, iv.hp, ev.hp),
            ("Attack", stats.attack, iv.attack, ev.attack),
            ("Defense", stats.defense, iv.defense, ev.defense),
            (
                "Sp. Atk",
                stats.special_attack,
                iv.special_attack,
                ev.special_attack,
            ),
            (
                "Sp. Def",
                stats.special_defense,
                iv.special_defense,
                ev.special_defense,
            ),
            ("Speed", stats.speed, iv.speed, ev.speed),
        ];
        let nature = genes.nature.raised_and_lowered();
        ui.scope(|ui| {
            ui.set_max_width(320.0);
            egui::Grid::new(("stats", mon.species, mon.exp))
                .num_columns(4)
                .spacing(vec2(18.0, 2.0))
                .min_col_width(0.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("Stats").weak());
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("IV").weak());
                        crate::app::widgets::info_mark::info_mark(
                            ui,
                            "Hidden values from 0 to 31 that raise each stat.",
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("EV").weak());
                        crate::app::widgets::info_mark::info_mark(
                            ui,
                            "Points from battles, up to 255 per stat and 510 in all. Every 4 add 1 at level 100.",
                        );
                    });
                    ui.end_row();
                    for (i, (name, value, iv, ev)) in rows.into_iter().enumerate() {
                        let mark = match nature {
                            Some((up, _)) if i == NATURE_ROWS[up] => Some((icons::CARET_UP, "Raised by its nature")),
                            Some((_, down)) if i == NATURE_ROWS[down] => {
                                Some((icons::CARET_DOWN, "Lowered by its nature"))
                            }
                            _ => None,
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.label(name);
                            if let Some((icon, hint)) = mark {
                                ui.label(RichText::new(icon).small().weak()).on_hover_text(hint);
                            }
                        });
                        right_cell(ui, 32.0, RichText::new(value.to_string()).strong());
                        right_cell(ui, 32.0, RichText::new(iv.to_string()));
                        right_cell(ui, 32.0, RichText::new(ev.to_string()));
                        ui.end_row();
                    }
                });
        });
    }

    pub(super) fn moves(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        ui.label(RichText::new("Moves").weak());
        let rows: Vec<MoveRow> = mon
            .moves
            .iter()
            .filter_map(|slot| {
                Some(MoveRow {
                    name: tables.move_name(slot.id)?.to_owned(),
                    type_id: tables.move_data(slot.id).map(|data| data.type_id),
                    effect: None,
                    pp: pp_text(slot.pp, tables.max_pp(*slot)),
                })
            })
            .collect();
        self.move_rows(ui, &rows);
    }

    pub(super) fn move_rows(&self, ui: &mut Ui, rows: &[MoveRow]) {
        let tables = &self.game.tables;
        let name_width = rows
            .iter()
            .map(|row| text_width(ui, RichText::new(&row.name).strong()))
            .fold(0.0, f32::max);
        let tag_width = rows
            .iter()
            .filter_map(|row| tables.type_name(row.type_id?))
            .map(|name| text_width(ui, RichText::new(name).small()) + 12.0)
            .fold(0.0, f32::max);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for row in rows {
                ui.horizontal(|ui| {
                    fixed_cell(ui, name_width, |ui| {
                        ui.label(RichText::new(&row.name).strong());
                    });
                    fixed_cell(ui, tag_width, |ui| {
                        if let Some(id) = row.type_id {
                            self.type_tags(ui, &[id]);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&row.pp).weak());
                        if let Some((text, color)) = row.effect {
                            ui.label(RichText::new(text).color(color));
                        }
                    });
                });
            }
        });
    }

    pub(super) fn facts(&self, ui: &mut Ui, mon: &Mon) {
        let tables = &self.game.tables;
        let mut facts: Vec<(&str, Fact)> = Vec::new();
        if tables.generation() != Generation::One {
            let item = mon
                .held_item
                .filter(|item| *item != 0)
                .and_then(|item| tables.item_name(item))
                .unwrap_or_else(|| "None".to_owned());
            facts.push(("Held item", Fact::Text(item)));
            if let Some((kind, power)) = tables.hidden_power(mon)
                && tables.type_name(kind).is_some_and(|name| !name.is_empty())
            {
                facts.push(("Hidden Power", Fact::HiddenPower(kind, power)));
            }
        }
        if let Some(genes) = mon.gen3() {
            facts.push(("Nature", Fact::Text(genes.nature.name().to_owned())));
        }
        if let Some(ability) = tables.mon_ability(mon) {
            facts.push(("Ability", Fact::Text(ability.to_owned())));
        }
        if let Some(friendship) = mon.friendship {
            facts.push(("Friendship", Fact::Text(format!("{friendship} of 255"))));
        }
        match mon.pokerus_status() {
            Some(Pokerus::Infected { .. }) => {
                facts.push(("Pokérus", Fact::Text("Infected".to_owned())))
            }
            Some(Pokerus::Cured { .. }) => facts.push(("Pokérus", Fact::Text("Cured".to_owned()))),
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
            facts.push((
                "Caught",
                Fact::Text(format!("{place}{time}, Lv {}", caught.level)),
            ));
        }
        if let Some(state) = self.game.state()
            && (mon.ot_id != state.trainer.id || mon.ot_name != state.trainer.name)
        {
            facts.push((
                "Original trainer",
                Fact::Text(format!("{}, ID {:05}", mon.ot_name, mon.ot_id)),
            ));
        }
        fact_rows(ui, &facts, |ui, fact| match fact {
            Fact::Text(text) => {
                ui.add(egui::Label::new(text.as_str()).wrap());
            }
            Fact::HiddenPower(kind, power) => {
                ui.horizontal(|ui| {
                    self.type_tags(ui, &[*kind]);
                    ui.label(format!("{power} power"));
                });
            }
        });
    }
}

enum Fact {
    Text(String),
    HiddenPower(u8, u8),
}

pub(super) struct MoveRow {
    pub(super) name: String,
    pub(super) type_id: Option<u8>,
    pub(super) effect: Option<(&'static str, Color32)>,
    pub(super) pp: String,
}

pub(super) fn pp_text(pp: u8, max: Option<u8>) -> String {
    match max {
        Some(max) => format!("{pp} / {max} PP"),
        None => format!("{pp} PP"),
    }
}

fn fixed_cell(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        vec2(width, ui.spacing().interact_size.y),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width);
            add(ui);
        },
    );
}

pub(super) fn fact_rows<V>(ui: &mut Ui, facts: &[(&str, V)], mut value: impl FnMut(&mut Ui, &V)) {
    let label_width = facts
        .iter()
        .map(|(label, _)| text_width(ui, RichText::new(*label).weak()))
        .fold(0.0, f32::max);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (label, fact) in facts {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                ui.allocate_ui_with_layout(
                    vec2(label_width, 0.0),
                    Layout::left_to_right(Align::Min),
                    |ui| {
                        ui.set_width(label_width);
                        ui.label(RichText::new(*label).weak());
                    },
                );
                value(ui, fact);
            });
        }
    });
}
