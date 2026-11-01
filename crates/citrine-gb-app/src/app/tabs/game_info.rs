use crate::app::tabs::TabViewer;
use crate::game_info::{Game, Page, View};
use crate::icons;
use citrine_gb_gamedata::{
    GameState, Gender, Generation, ItemStack, Mon, Pokemon, Pokerus, Side, TimeOfDay,
};
use egui::{
    Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Stroke, TextureHandle, Ui, vec2,
};
use std::collections::HashMap;

const TILE: f32 = 56.0;
const LIST_WIDTH: f32 = 220.0;
const DETAILS_MIN: f32 = 360.0;
const GRID_CELL: f32 = 76.0;
const DEX_CELL: f32 = 96.0;
const BOX_SIZE: usize = 20;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let mobile = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
    let key = viewer.game_info.rom_key().unwrap_or_default().to_owned();
    let name = viewer.emulator.game_name().to_owned();
    let (game, view) = viewer.game_info.parts();
    let Some(game) = game else {
        return;
    };
    let page = Pages {
        key: &key,
        name: &name,
        touch: ui.spacing().interact_size.y,
        game,
        mobile,
        clock: Clock::new(),
    };
    egui::ScrollArea::vertical()
        .id_salt("game_info_scroll")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            ui.spacing_mut().interact_size.y = egui::style::Spacing::default().interact_size.y;
            page.show(ui, view);
        });
}

struct Pages<'a> {
    key: &'a str,
    name: &'a str,
    touch: f32,
    game: &'a Game,
    mobile: bool,
    clock: Clock,
}

impl Pages<'_> {
    fn show(&self, ui: &mut Ui, view: &mut View) {
        self.source_line(ui);
        let Some(state) = self.game.state() else {
            return;
        };
        if self.mobile && view.details {
            if let Some(mon) = self.picked(state, view) {
                ui.add_space(6.0);
                self.details(ui, mon);
                return;
            }
            view.details = false;
        }
        ui.add_space(6.0);
        self.trainer(ui, state);
        ui.add_space(8.0);
        self.page_tabs(ui, view);
        ui.add_space(8.0);
        match view.page {
            Page::Party => self.party(ui, state, view),
            Page::Boxes => self.boxes(ui, state, view),
            Page::Bag => self.bag(ui, state),
            Page::Pokedex => self.pokedex(ui, state, view),
        }
        ui.add_space(8.0);
    }

    fn picked<'s>(&self, state: &'s GameState, view: &View) -> Option<&'s Mon> {
        let list = match view.page {
            Page::Party => &state.party,
            Page::Boxes => &state.boxes.get(view.pc_box?)?.pokemon,
            _ => return None,
        };
        let pick = match view.page {
            Page::Party => view.party_pick,
            _ => view.box_pick,
        };
        match list.get(pick)? {
            Pokemon::Hatched(mon) => Some(mon),
            Pokemon::Egg => None,
        }
    }

    fn source_line(&self, ui: &mut Ui) {
        let (status, text) = status_line(self.game, &self.clock);
        let name = RichText::new(self.name).strong();
        let dot = |ui: &mut Ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 4.0, status.color(ui.visuals().dark_mode));
        };
        let needed = text_width(ui, name.clone()) + text_width(ui, RichText::new(&text)) + 40.0;
        if needed <= ui.available_width() {
            ui.horizontal(|ui| {
                ui.label(name);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(text);
                    dot(ui);
                });
            });
        } else {
            ui.add(egui::Label::new(name).truncate());
            ui.horizontal(|ui| {
                dot(ui);
                ui.add(egui::Label::new(text).truncate());
            });
        }
    }

    fn trainer(&self, ui: &mut Ui, state: &GameState) {
        let trainer = &state.trainer;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&trainer.name).size(19.0).strong());
                ui.label(RichText::new(format!("ID {:05}", trainer.id)).weak());
            });
            let mut facts: Vec<(&str, String)> = vec![("Money", money(trainer.money))];
            if let Some(mom) = trainer.mom_money {
                facts.push(("Mom's savings", money(mom)));
            }
            facts.push((
                "Play time",
                format!(
                    "{} h {} min",
                    trainer.play_time.hours, trainer.play_time.minutes
                ),
            ));
            facts.push((
                "Pokédex",
                format!(
                    "{} caught, {} seen",
                    state.pokedex.caught.len(),
                    state.pokedex.seen.len()
                ),
            ));
            let badge_fact = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new("Badges").small().weak()).extend());
                    badges(ui, &trainer.badges);
                })
                .response
                .on_hover_text(format!(
                    "{} of {} badges",
                    state.badge_count(),
                    trainer.badges.len()
                ));
            };
            let gap = 22.0;
            let mut widths: Vec<f32> = facts
                .iter()
                .map(|(label, value)| fact_width(ui, label, value))
                .collect();
            widths.push(badges_width(trainer.badges.len()));
            let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
            let mut used = 0.0;
            for (item, width) in widths.into_iter().enumerate() {
                if used > 0.0 && used + gap + width > ui.available_width() {
                    rows.push(Vec::new());
                    used = 0.0;
                }
                used += if used > 0.0 { gap + width } else { width };
                rows.last_mut().unwrap().push(item);
            }
            ui.spacing_mut().item_spacing.y = 10.0;
            for row in rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for item in row {
                        match facts.get(item) {
                            Some((label, value)) => fact(ui, label, value),
                            None => badge_fact(ui),
                        }
                    }
                });
            }
        });
    }

    fn page_tabs(&self, ui: &mut Ui, view: &mut View) {
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = self.touch;
            let pages = [
                (Page::Party, icons::USERS_THREE),
                (Page::Boxes, icons::PACKAGE),
                (Page::Bag, icons::BACKPACK),
                (Page::Pokedex, icons::BOOK_OPEN),
            ];
            if self.mobile {
                ui.columns(pages.len(), |columns| {
                    for (column, (page, icon)) in columns.iter_mut().zip(pages) {
                        column.vertical_centered_justified(|ui| {
                            let text = format!("{icon}\n{}", page.label());
                            if ui
                                .add(egui::Button::selectable(view.page == page, text))
                                .clicked()
                            {
                                view.page = page;
                            }
                        });
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    for (page, icon) in pages {
                        let text = format!("{icon}  {}", page.label());
                        if ui.selectable_label(view.page == page, text).clicked() {
                            view.page = page;
                        }
                    }
                });
            }
        });
    }

    fn party(&self, ui: &mut Ui, state: &GameState, view: &mut View) {
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

    fn party_row(&self, ui: &mut Ui, pokemon: &Pokemon, selected: bool) -> egui::Response {
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

    fn party_card(&self, ui: &mut Ui, pokemon: &Pokemon) -> egui::Response {
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

    fn name_row(&self, ui: &mut Ui, pokemon: &Pokemon, chevron: bool) {
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

    fn gender_mark(&self, ui: &mut Ui, mon: &Mon) {
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

    fn hp_bar(&self, ui: &mut Ui, mon: &Mon) {
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

    fn stats(&self, mon: &Mon) -> Option<citrine_gb_gamedata::Stats> {
        mon.stats.or_else(|| self.game.tables.mon_stats(mon))
    }

    fn pokemon_details(&self, ui: &mut Ui, pokemon: &Pokemon) {
        match pokemon {
            Pokemon::Hatched(mon) => self.details(ui, mon),
            Pokemon::Egg => egg_details(ui),
        }
    }

    fn details(&self, ui: &mut Ui, mon: &Mon) {
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

    fn exp_line(&self, mon: &Mon) -> Option<(String, f32)> {
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

    fn types(&self, ui: &mut Ui, mon: &Mon) {
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

    fn stat_table(&self, ui: &mut Ui, mon: &Mon) {
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

    fn moves(&self, ui: &mut Ui, mon: &Mon) {
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

    fn facts(&self, ui: &mut Ui, mon: &Mon) {
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

    fn boxes(&self, ui: &mut Ui, state: &GameState, view: &mut View) {
        if state.boxes.is_empty() {
            return;
        }
        let count = state.boxes.len();
        let index = view.pc_box.unwrap_or(state.current_box).min(count - 1);
        view.pc_box = Some(index);
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = self.touch;
            if ui
                .add(egui::Button::new(icons::CARET_LEFT).min_size(vec2(self.touch, 0.0)))
                .on_hover_text("Previous box")
                .clicked()
            {
                view.pc_box = Some((index + count - 1) % count);
                view.box_pick = 0;
            }
            let name = |i: usize| box_name(state, i);
            egui::ComboBox::from_id_salt("pc_box")
                .selected_text(name(index))
                .show_ui(ui, |ui| {
                    for i in 0..count {
                        if ui.selectable_label(i == index, name(i)).clicked() {
                            view.pc_box = Some(i);
                            view.box_pick = 0;
                        }
                    }
                });
            if ui
                .add(egui::Button::new(icons::CARET_RIGHT).min_size(vec2(self.touch, 0.0)))
                .on_hover_text("Next box")
                .clicked()
            {
                view.pc_box = Some((index + 1) % count);
                view.box_pick = 0;
            }
            let held = state.boxes[index].pokemon.len();
            ui.label(RichText::new(format!("{held} / {BOX_SIZE} Pokémon")).weak());
        });
        ui.add_space(6.0);
        let index = view.pc_box.unwrap_or(index);
        let pokemon = &state.boxes[index].pokemon;
        if pokemon.is_empty() {
            ui.label(RichText::new("This box is empty").weak());
            return;
        }
        view.box_pick = view.box_pick.min(pokemon.len() - 1);
        let grid = |ui: &mut Ui, view: &mut View, columns: usize| {
            let cell = (ui.available_width() + 6.0) / columns as f32 - 6.0;
            egui::Grid::new(("box_grid", index))
                .num_columns(columns)
                .spacing(vec2(6.0, 6.0))
                .show(ui, |ui| {
                    for (i, entry) in pokemon.iter().enumerate() {
                        if self
                            .box_cell(ui, cell, entry, !self.mobile && i == view.box_pick)
                            .clicked()
                        {
                            view.box_pick = i;
                            view.details = self.mobile && matches!(entry, Pokemon::Hatched(_));
                        }
                        if i % columns == columns - 1 {
                            ui.end_row();
                        }
                    }
                });
        };
        if self.mobile {
            let columns = ((ui.available_width() + 6.0) / (GRID_CELL + 6.0))
                .floor()
                .max(1.0) as usize;
            grid(ui, view, columns);
            return;
        }
        let grid_width = 4.0 * (GRID_CELL + 6.0);
        if !beside(ui, grid_width) {
            let columns = ((ui.available_width() + 6.0) / (GRID_CELL + 6.0))
                .floor()
                .max(1.0) as usize;
            grid(ui, view, columns);
            ui.add_space(6.0);
            self.pokemon_details(ui, &pokemon[view.box_pick]);
            return;
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(grid_width);
                grid(ui, view, 4);
            });
            ui.vertical(|ui| self.pokemon_details(ui, &pokemon[view.box_pick]));
        });
    }

    fn box_cell(
        &self,
        ui: &mut Ui,
        width: f32,
        pokemon: &Pokemon,
        selected: bool,
    ) -> egui::Response {
        let frame = card_with_margin(ui, selected, 2, 4);
        let inner = width - frame.total_margin().sum().x;
        frame
            .show(ui, |ui| {
                ui.set_width(inner);
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    self.tile(ui, pokemon_sprite(pokemon), TILE);
                    let (name, level) = match pokemon {
                        Pokemon::Hatched(mon) => {
                            (mon.nickname.as_str(), format!("Lv {}", mon.level))
                        }
                        Pokemon::Egg => ("Egg", String::new()),
                    };
                    ui.add(egui::Label::new(RichText::new(name).small()).truncate());
                    ui.label(RichText::new(level).small());
                });
            })
            .response
            .interact(Sense::click())
    }

    fn bag(&self, ui: &mut Ui, state: &GameState) {
        let bag = &state.bag;
        let sections: [(&str, &[ItemStack], bool); 4] = [
            ("Items", &bag.items, false),
            ("Key items", &bag.key_items, false),
            ("Balls", &bag.balls, false),
            ("TMs and HMs", &bag.tms_hms, true),
        ];
        let sections: Vec<_> = sections
            .into_iter()
            .filter(|(_, items, _)| !items.is_empty())
            .collect();
        if sections.is_empty() {
            ui.label(RichText::new("The bag is empty").weak());
            return;
        }
        if self.mobile || sections.len() == 1 {
            for (title, items, machines) in sections {
                self.bag_section(ui, title, items, machines);
                ui.add_space(8.0);
            }
            return;
        }
        let (left, right) = sections.split_at(1.max(sections.len() / 2));
        ui.columns(2, |columns| {
            for (column, half) in columns.iter_mut().zip([left, right]) {
                for (title, items, machines) in half {
                    self.bag_section(column, title, items, *machines);
                    column.add_space(8.0);
                }
            }
        });
    }

    fn bag_section(&self, ui: &mut Ui, title: &str, items: &[ItemStack], machines: bool) {
        let tables = &self.game.tables;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).weak());
            let machine_width = text_width(ui, RichText::new("HM00"));
            for stack in items {
                ui.horizontal(|ui| {
                    let name = tables.item_name(stack.item).unwrap_or_default();
                    if machines {
                        ui.allocate_ui_with_layout(
                            vec2(machine_width, ui.spacing().interact_size.y),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_min_width(machine_width);
                                ui.label(name);
                            },
                        );
                        if let Some(name) = tables.machine_move(stack.item) {
                            ui.label(name);
                        }
                    } else {
                        ui.label(name);
                    }
                    if !tables.is_hm(stack.item) {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(format!("× {}", stack.count)).weak());
                        });
                    }
                });
            }
        });
    }

    fn pokedex(&self, ui: &mut Ui, state: &GameState, view: &mut View) {
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = self.touch;
            let width = (ui.available_width() - 150.0).clamp(120.0, 260.0);
            ui.add(
                egui::TextEdit::singleline(&mut view.dex_search)
                    .hint_text(format!("{}  Search", icons::MAGNIFYING_GLASS))
                    .desired_width(width)
                    .min_size(vec2(0.0, self.touch))
                    .vertical_align(Align::Center),
            );
            for (caught, label) in [(false, "All"), (true, "Caught")] {
                let chip = egui::Button::selectable(view.dex_caught == caught, label)
                    .min_size(vec2(self.touch, 0.0));
                if ui.add(chip).clicked() {
                    view.dex_caught = caught;
                }
            }
        });
        ui.add_space(6.0);
        let tables = &self.game.tables;
        let search = view.dex_search.trim().to_lowercase();
        let numbers: Vec<u16> = if view.dex_caught {
            state.pokedex.caught.clone()
        } else {
            let last = state.pokedex.seen.iter().copied().max().unwrap_or(0);
            (1..=last).collect()
        };
        let entries: Vec<(u16, Option<(u8, &str)>)> = numbers
            .into_iter()
            .map(|number| {
                let named = if state.pokedex.seen.contains(&number) {
                    tables
                        .species_for_dex(number)
                        .and_then(|species| Some((species, tables.species_name(species)?)))
                } else {
                    None
                };
                (number, named)
            })
            .filter(|(number, named)| {
                search.is_empty()
                    || named.is_some_and(|(_, name)| name.to_lowercase().contains(&search))
                    || number.to_string() == search
            })
            .collect();
        if entries.is_empty() {
            ui.label(RichText::new("No Pokémon match").weak());
            return;
        }
        let columns = ((ui.available_width() + 6.0) / (DEX_CELL + 6.0))
            .floor()
            .max(1.0) as usize;
        let cell = (ui.available_width() + 6.0) / columns as f32 - 6.0;
        egui::Grid::new("dex_grid")
            .num_columns(columns)
            .spacing(vec2(6.0, 6.0))
            .show(ui, |ui| {
                for (i, (number, named)) in entries.iter().enumerate() {
                    let caught = state.pokedex.caught.contains(number);
                    ui.allocate_ui(vec2(cell, TILE + 34.0), |ui| {
                        self.dex_cell(ui, cell, *number, *named, caught);
                    });
                    if i % columns == columns - 1 {
                        ui.end_row();
                    }
                }
            });
    }

    fn dex_cell(
        &self,
        ui: &mut Ui,
        width: f32,
        number: u16,
        named: Option<(u8, &str)>,
        caught: bool,
    ) {
        let (rect, _) = ui.allocate_exact_size(vec2(width, TILE + 34.0), Sense::hover());
        let visuals = ui.visuals().clone();
        let small = egui::TextStyle::Small.resolve(ui.style());
        let opacity = if named.is_some() { 1.0 } else { 0.55 };
        let mut painter = ui.painter_at(rect);
        painter.multiply_opacity(opacity);
        painter.rect(
            rect,
            6.0,
            visuals.faint_bg_color,
            visuals.widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.left_top() + vec2(7.0, 4.0),
            egui::Align2::LEFT_TOP,
            number.to_string(),
            small.clone(),
            visuals.weak_text_color(),
        );
        let tile = Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 14.0 + TILE / 2.0),
            vec2(TILE, TILE),
        );
        let Some((species, name)) = named else {
            let fill = if visuals.dark_mode {
                visuals.widgets.inactive.weak_bg_fill
            } else {
                tile_color(ui)
            };
            painter.rect_filled(tile, CornerRadius::same((TILE / 9.0) as u8), fill);
            return;
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(tile), |ui| {
            self.tile(ui, Some((species, false, 'A')), TILE);
        });
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 4.0),
            egui::Align2::CENTER_BOTTOM,
            name,
            small,
            visuals.text_color(),
        );
        if caught {
            let mark =
                Rect::from_center_size(rect.right_top() + vec2(-11.0, 10.0), vec2(10.0, 10.0));
            paint_caught_mark(&painter, mark.center());
            ui.interact(mark, ui.id().with(("caught", number)), Sense::hover())
                .on_hover_text("Caught");
        }
    }

    fn tile(&self, ui: &mut Ui, sprite: Option<(u8, bool, char)>, size: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same((size / 9.0) as u8), tile_color(ui));
        let Some((species, shiny, letter)) = sprite else {
            return;
        };
        let Some(texture) = sprite_texture(ui.ctx(), self.key, self.game, species, shiny, letter)
        else {
            return;
        };
        let scale = size / TILE;
        let [w, h] = texture.size();
        let image = vec2(w as f32, h as f32) * scale;
        let at = Rect::from_min_size(rect.center_bottom() - vec2(image.x / 2.0, image.y), image);
        painter.image(
            texture.id(),
            at,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

fn sprite_key(generation: Generation, mon: &Mon) -> (u8, bool, char) {
    let shiny = generation == Generation::Two && mon.dvs.shiny();
    (mon.species, shiny, mon.dvs.unown_letter())
}

fn pokemon_sprite(pokemon: &Pokemon) -> Option<(u8, bool, char)> {
    match pokemon {
        Pokemon::Hatched(mon) => Some((mon.species, mon.dvs.shiny(), mon.dvs.unown_letter())),
        Pokemon::Egg => None,
    }
}

type SpriteCache = HashMap<(String, u8, bool, char), Option<TextureHandle>>;

fn sprite_texture(
    ctx: &egui::Context,
    key: &str,
    game: &Game,
    species: u8,
    shiny: bool,
    letter: char,
) -> Option<TextureHandle> {
    let shiny = shiny && game.tables.game.generation() == Generation::Two;
    let id = egui::Id::new("game_info_sprites");
    let cache_key = (key.to_owned(), species, shiny, letter);
    if let Some(found) = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<SpriteCache>(id)
            .get(&cache_key)
            .cloned()
    }) {
        return found;
    }
    let texture = sprite_image(game, species, shiny, letter).map(|image| {
        ctx.load_texture(
            format!("game_info_{key}_{species}_{shiny}_{letter}"),
            image,
            egui::TextureOptions::NEAREST,
        )
    });
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<SpriteCache>(id)
            .insert(cache_key, texture.clone())
    });
    texture
}

pub fn sprite_image(
    game: &Game,
    species: u8,
    shiny: bool,
    letter: char,
) -> Option<egui::ColorImage> {
    let tables = &game.tables;
    let sprite = tables
        .sprite(&game.rom, species, Some(letter), Side::Front)
        .ok()?;
    let palette = tables.palette(species, shiny);
    let rgb = |value: u8| match &palette {
        Some(palette) => palette.rgb(value),
        None => {
            let grey = [0xFF, 0xAA, 0x55, 0x00][usize::from(value & 3)];
            [grey, grey, grey]
        }
    };
    Some(cut_out(
        sprite.width_px(),
        sprite.height_px(),
        |x, y| sprite.pixel(x, y),
        rgb,
    ))
}

fn cut_out(
    w: usize,
    h: usize,
    shade: impl Fn(usize, usize) -> u8,
    rgb: impl Fn(u8) -> [u8; 3],
) -> egui::ColorImage {
    let mut outside = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        if outside[y * w + x] || shade(x, y) != 0 {
            continue;
        }
        outside[y * w + x] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = rgb(shade(x, y));
            let alpha = if outside[y * w + x] { 0 } else { 0xFF };
            rgba.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

fn beside(ui: &Ui, list_width: f32) -> bool {
    ui.available_width() >= list_width + 6.0 + DETAILS_MIN
}

fn egg_details(ui: &mut Ui) {
    card(ui, false).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Egg").size(20.0).strong());
    });
}

fn card(ui: &Ui, selected: bool) -> egui::Frame {
    card_with_margin(ui, selected, 10, 8)
}

fn card_with_margin(ui: &Ui, selected: bool, x: i8, y: i8) -> egui::Frame {
    let visuals = ui.visuals();
    let normal = visuals.widgets.noninteractive.bg_stroke;
    let stroke = if selected {
        Stroke::new(2.0_f32, visuals.selection.stroke.color)
    } else {
        normal
    };
    let extra = (stroke.width - normal.width).round() as i8;
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(x - extra, y - extra))
}

fn fact(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(egui::Label::new(RichText::new(label).small().weak()).extend());
        ui.add(egui::Label::new(value).extend());
    });
}

fn text_width(ui: &Ui, text: RichText) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

fn fact_width(ui: &Ui, label: &str, value: &str) -> f32 {
    text_width(ui, RichText::new(label).small()).max(text_width(ui, RichText::new(value)))
}

fn right_cell(ui: &mut Ui, width: f32, text: RichText) {
    ui.allocate_ui_with_layout(
        vec2(width, ui.spacing().interact_size.y),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.set_width(width);
            ui.label(text);
        },
    );
}

fn chip(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
        .corner_radius(3.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).small()).truncate());
        });
}

fn meter(ui: &mut Ui, fraction: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
    let painter = ui.painter();
    let track = if ui.visuals().dark_mode {
        ui.visuals().extreme_bg_color
    } else {
        ui.visuals().widgets.inactive.weak_bg_fill
    };
    painter.rect_filled(rect, 3.0, track);
    let mut fill = rect;
    fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    painter.rect_filled(fill, 3.0, color);
}

fn labelled_meter(ui: &mut Ui, label: &str, value: &str, fraction: f32, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).small());
        });
    });
    meter(ui, fraction, color);
}

const BADGE_SIDE: f32 = 12.0;
const BADGE_GAP: f32 = 6.0;

fn badges_width(count: usize) -> f32 {
    count as f32 * (BADGE_SIDE + BADGE_GAP)
}

fn badges(ui: &mut Ui, earned: &[bool]) {
    let side = BADGE_SIDE;
    let gap = BADGE_GAP;
    let width = badges_width(earned.len());
    let height = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let color = badge_color(ui);
    let off = ui.visuals().weak_text_color();
    for (i, on) in earned.iter().enumerate() {
        let center = egui::pos2(
            rect.left() + side / 2.0 + 2.0 + i as f32 * (side + gap),
            rect.center().y,
        );
        let r = side * 0.62;
        let points = vec![
            center + vec2(0.0, -r),
            center + vec2(r, 0.0),
            center + vec2(0.0, r),
            center + vec2(-r, 0.0),
        ];
        let shape = if *on {
            egui::Shape::convex_polygon(points, color, Stroke::NONE)
        } else {
            egui::Shape::convex_polygon(points, Color32::TRANSPARENT, Stroke::new(1.5_f32, off))
        };
        ui.painter().add(shape);
    }
}

fn paint_caught_mark(painter: &egui::Painter, c: egui::Pos2) {
    painter.circle_filled(c, 5.0, Color32::WHITE);
    painter.add(egui::Shape::convex_polygon(
        (0..=16)
            .map(|i| {
                let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                c + vec2(a.cos(), a.sin()) * 5.0
            })
            .collect(),
        Color32::from_rgb(0xe0, 0x40, 0x40),
        Stroke::NONE,
    ));
    painter.circle_stroke(c, 5.0, Stroke::new(1.0_f32, Color32::from_gray(60)));
}

fn tile_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xe0, 0xe0, 0xd8)
    } else {
        Color32::from_rgb(0xef, 0xef, 0xef)
    }
}

fn badge_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xe3, 0xa5, 0x1a)
    } else {
        Color32::from_rgb(0xd9, 0x9a, 0x00)
    }
}

fn exp_color(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0x4a, 0x9f, 0xd8)
    } else {
        Color32::from_rgb(0x2a, 0x7f, 0xc0)
    }
}

fn hp_color(fraction: f32) -> Color32 {
    if fraction > 0.5 {
        Color32::from_rgb(0x3c, 0xa8, 0x46)
    } else if fraction > 0.2 {
        Color32::from_rgb(0xd8, 0x9a, 0x10)
    } else {
        Color32::from_rgb(0xd8, 0x40, 0x40)
    }
}

fn type_color(id: u8) -> Color32 {
    let rgb = match id {
        0 => 0x9a9a6a,
        1 => 0xb83a30,
        2 => 0x8e7ad8,
        3 => 0x9a4a9a,
        4 => 0xc8a850,
        5 => 0xa89238,
        7 => 0x98a820,
        8 => 0x6a5890,
        9 => 0x9898b8,
        20 => 0xe07a30,
        21 => 0x5a88e0,
        22 => 0x68b048,
        23 => 0xd8b020,
        24 => 0xe0507a,
        25 => 0x78c8c8,
        26 => 0x6a40e0,
        27 => 0x6a5848,
        _ => 0x7a7a7a,
    };
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn type_text_color(id: u8) -> Color32 {
    let fill = type_color(id);
    let linear = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance =
        0.2126 * linear(fill.r()) + 0.7152 * linear(fill.g()) + 0.0722 * linear(fill.b());
    let dark = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    if 1.05 / (luminance + 0.05) >= (luminance + 0.05) / 0.06 {
        Color32::WHITE
    } else {
        dark
    }
}

fn box_name(state: &GameState, index: usize) -> String {
    state
        .boxes
        .get(index)
        .and_then(|b| b.name.clone())
        .unwrap_or_else(|| format!("Box {}", index + 1))
}

fn money(amount: u32) -> String {
    format!("¥{}", grouped(amount))
}

fn grouped(value: u32) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Saved,
    Behind,
    File,
}

impl Status {
    fn color(self, dark: bool) -> Color32 {
        match (self, dark) {
            (Status::Saved, true) => Color32::from_rgb(0x4c, 0xaf, 0x50),
            (Status::Saved, false) => Color32::from_rgb(0x2e, 0x9a, 0x3a),
            (Status::Behind, true) => Color32::from_rgb(0xe3, 0xa5, 0x1a),
            (Status::Behind, false) => Color32::from_rgb(0xc9, 0x8a, 0x00),
            (Status::File, _) => Color32::from_gray(0x8c),
        }
    }
}

pub struct Clock {
    zone: jiff::tz::TimeZone,
    today: jiff::civil::Date,
}

impl Clock {
    pub fn new() -> Self {
        Self::in_zone(jiff::tz::TimeZone::system(), jiff::Timestamp::now())
    }

    pub fn in_zone(zone: jiff::tz::TimeZone, now: jiff::Timestamp) -> Self {
        let today = now.to_zoned(zone.clone()).date();
        Self { zone, today }
    }

    fn when(&self, at: u64) -> String {
        let Ok(stamp) = jiff::Timestamp::from_second(at as i64) else {
            return String::new();
        };
        let zoned = stamp.to_zoned(self.zone.clone());
        if zoned.date() == self.today {
            zoned.strftime("%H:%M").to_string()
        } else {
            zoned.strftime("%-d %b, %H:%M").to_string()
        }
    }
}

pub fn status_line(game: &Game, clock: &Clock) -> (Status, String) {
    let when = game.saved_at.map(|at| clock.when(at));
    match (game.live.is_some(), game.behind, game.saved.is_some(), when) {
        (true, false, true, Some(when)) => {
            (Status::Saved, format!("Playing, saved in game at {when}"))
        }
        (true, false, true, None) => (Status::Saved, "Playing, saved in game".to_owned()),
        (true, _, true, Some(when)) => (
            Status::Behind,
            format!("Playing, not saved in game since {when}"),
        ),
        (true, _, _, _) => (Status::Behind, "Playing, not saved in game yet".to_owned()),
        (false, _, true, Some(when)) => (Status::File, format!("Save file from {when}")),
        (false, _, true, None) => (Status::File, "Save file".to_owned()),
        (false, _, false, _) => (Status::File, "No saved game yet".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_info::GameInfo;
    use citrine_gb_gamedata::fixtures::{
        crystal_cartridge, crystal_live_wram, crystal_save, set_crystal_live_money,
    };

    const NOON: i64 = 1_761_912_000;

    #[test]
    fn type_tags_pick_the_text_colour_with_more_contrast() {
        assert_eq!(type_text_color(0), Color32::from_rgb(0x1a, 0x1a, 0x1a));
        assert_eq!(type_text_color(8), Color32::WHITE);
        assert_eq!(type_text_color(1), Color32::WHITE);
    }

    #[test]
    fn selecting_a_card_keeps_its_content_in_place() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert_eq!(
                    card(ui, true).total_margin(),
                    card(ui, false).total_margin()
                );
                assert_eq!(
                    card_with_margin(ui, true, 2, 4).total_margin(),
                    card_with_margin(ui, false, 2, 4).total_margin()
                );
            });
        });
    }

    fn clock() -> Clock {
        Clock::in_zone(
            jiff::tz::TimeZone::UTC,
            jiff::Timestamp::from_second(NOON).unwrap(),
        )
    }

    fn crystal(saved_at: Option<u64>) -> GameInfo {
        let mut info = GameInfo::default();
        info.set_rom(Some("crystal"), &crystal_cartridge(), saved_at);
        info
    }

    #[test]
    fn the_line_says_whether_the_live_game_is_saved() {
        let save = crystal_save();
        let mut wram = crystal_live_wram(&save);
        let mut info = crystal(Some(NOON as u64 - 3600));
        info.read(&save, &wram, NOON as u64);
        assert_eq!(
            status_line(info.game().unwrap(), &clock()),
            (Status::Saved, "Playing, saved in game at 11:00".to_owned())
        );

        set_crystal_live_money(&mut wram, 4815);
        info.read(&save, &wram, NOON as u64);
        assert_eq!(
            status_line(info.game().unwrap(), &clock()),
            (
                Status::Behind,
                "Playing, not saved in game since 11:00".to_owned()
            )
        );
    }

    #[test]
    fn the_line_shows_the_save_file_and_its_day_when_the_game_is_not_running() {
        let mut info = crystal(Some(NOON as u64 - 3 * 86_400));
        info.read(&crystal_save(), &vec![0u8; 0x2000], NOON as u64);
        assert_eq!(
            status_line(info.game().unwrap(), &clock()),
            (Status::File, "Save file from 28 Oct, 12:00".to_owned())
        );

        let mut empty = crystal(None);
        empty.read(&vec![0u8; 0x8000], &vec![0u8; 0x2000], NOON as u64);
        assert_eq!(
            status_line(empty.game().unwrap(), &clock()),
            (Status::File, "No saved game yet".to_owned())
        );
    }

    #[test]
    fn a_sprite_is_clear_outside_its_outline_and_solid_inside() {
        let ring = |x: usize, y: usize| -> u8 {
            let edge = (1..=4).contains(&x) && (1..=4).contains(&y);
            let inner = (2..=3).contains(&x) && (2..=3).contains(&y);
            if edge && !inner { 3 } else { 0 }
        };
        let image = cut_out(6, 6, ring, |value| [value, value, value]);
        let alpha = |x: usize, y: usize| image.pixels[y * 6 + x].a();
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(5, 3), 0);
        assert_eq!(alpha(1, 1), 0xFF);
        assert_eq!(alpha(2, 2), 0xFF);
        assert_eq!(alpha(3, 3), 0xFF);
    }
}
