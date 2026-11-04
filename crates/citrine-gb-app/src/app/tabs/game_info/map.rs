use super::journey::milestone_name;
use super::parts::{card, chip, paint_caught_mark, text_width};
use super::region_map::place_card;
use super::{HERE_TWO_COLUMNS, Pages, TILE};
use citrine_gb_gamedata::{
    Encounter, GameState, Generation, Location, Method, Position, Region, TimeOfDay, WildList,
};
use egui::{RichText, Sense, Ui, vec2};

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub(super) fn time_name(time: TimeOfDay) -> Option<&'static str> {
    match time {
        TimeOfDay::Morning => Some("Morning"),
        TimeOfDay::Day => Some("Day"),
        TimeOfDay::Night => Some("Night"),
        TimeOfDay::Unknown => None,
    }
}

impl Pages<'_> {
    pub(super) fn place_name(&self, state: &GameState) -> Option<String> {
        let position = state.location.as_ref()?;
        let Some(location) = position.gb() else {
            return Some(self.game.tables.marker(position)?.name.to_owned());
        };
        let spot = self.game.tables.gb()?.spot(location)?;
        let time = state.clock.and_then(|clock| time_name(clock.time));
        Some(match time {
            Some(time) => format!("{}, {time}", spot.name),
            None => spot.name.to_owned(),
        })
    }

    pub(super) fn goal_text(&self, state: &GameState) -> Option<String> {
        let (name, place) = milestone_name(self.game.tables.game(), state.next_goal()?);
        Some(match place {
            Some(place) => format!("{name}, {place}"),
            None => name,
        })
    }

    pub(super) fn map(&self, ui: &mut Ui, state: &GameState) {
        let Some(position) = &state.location else {
            return;
        };
        let place = |ui: &mut Ui| match position.gb() {
            Some(location) => self.place_card(ui, state, location),
            None => self.gba_place_card(ui, position),
        };
        if !self.mobile && ui.available_width() >= HERE_TWO_COLUMNS {
            ui.columns(2, |columns| {
                place(&mut columns[0]);
                self.wild_lists(&mut columns[1], state, position, false);
            });
            return;
        }
        place(ui);
        self.wild_lists(ui, state, position, true);
    }

    fn gba_place_card(&self, ui: &mut Ui, position: &Position) {
        let marker = self.game.tables.marker(position);
        let name = marker.map_or("Unknown place", |marker| marker.name);
        let key = marker.map(|marker| format!("{}_{:p}", self.key, marker.map));
        place_card(ui, name, "", key.as_deref().zip(marker.as_ref()));
    }

    fn place_card(&self, ui: &mut Ui, state: &GameState, location: &Location) {
        let Some(tables) = self.game.tables.gb() else {
            return;
        };
        let spot = tables.spot(location);
        let name = spot.map_or("Unknown place", |spot| spot.name);
        let marker = tables.marker(location).zip(spot.map(|spot| spot.region));
        let mut line = Vec::new();
        if let Some(spot) = spot
            .filter(|_| tables.game.generation() == Generation::Two)
            .filter(|_| marker.is_none())
        {
            line.push(match spot.region {
                Region::Kanto => "Kanto".to_owned(),
                Region::Johto => "Johto".to_owned(),
            });
        }
        if let Some(clock) = state.clock {
            let day = WEEKDAYS.get(usize::from(clock.weekday)).copied();
            match (time_name(clock.time), day) {
                (Some(time), Some(day)) => line.push(format!("{time}, {day}")),
                (Some(time), None) => line.push(time.to_owned()),
                (None, Some(day)) => line.push(day.to_owned()),
                (None, None) => {}
            }
        }
        let key = marker.map(|(_, region)| format!("{}_{region:?}", self.key));
        place_card(
            ui,
            name,
            &line.join(" · "),
            key.as_deref()
                .zip(marker.as_ref().map(|(marker, _)| marker)),
        );
    }

    fn wild_lists(&self, ui: &mut Ui, state: &GameState, position: &Position, below: bool) {
        let time = state.clock.map(|clock| clock.time);
        let lists = self.game.tables.wild(position, time);
        let mut first = true;
        let mut gap = |ui: &mut Ui| {
            if below || !first {
                ui.add_space(8.0);
            }
            first = false;
        };
        if lists.iter().all(|list| list.encounters.is_empty()) {
            gap(ui);
            ui.label(RichText::new("No wild Pokémon here").weak());
            return;
        }
        for list in lists.iter().filter(|list| !list.encounters.is_empty()) {
            gap(ui);
            self.wild_list(ui, state, list);
        }
    }

    fn wild_list(&self, ui: &mut Ui, state: &GameState, list: &WildList) {
        let heading = match list.method {
            Method::Grass => "Wild in the grass",
            Method::Water => "Wild on the water",
            Method::OldRod => "With the Old Rod",
            Method::GoodRod => "With the Good Rod",
            Method::SuperRod => "With the Super Rod",
            Method::RockSmash => "With Rock Smash",
        };
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(heading).weak());
            ui.spacing_mut().item_spacing.y = 4.0;
            for encounter in &list.encounters {
                self.encounter_row(ui, state, encounter);
            }
        });
    }

    fn encounter_row(&self, ui: &mut Ui, state: &GameState, encounter: &Encounter) {
        let tables = &self.game.tables;
        let dex = tables.dex_number(encounter.species);
        let caught = dex.is_some_and(|dex| state.pokedex.caught.contains(&dex));
        let seen = dex.is_some_and(|dex| state.pokedex.seen.contains(&dex));
        let name = tables.species_name(encounter.species).unwrap_or("?");
        let level = if encounter.min_level == encounter.max_level {
            format!("Lv {}", encounter.min_level)
        } else {
            format!("Lv {}-{}", encounter.min_level, encounter.max_level)
        };
        ui.horizontal(|ui| {
            self.tile(ui, Some((encounter.species, false, 'A')), TILE * 0.7);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(RichText::new(name).strong()).truncate());
                    if caught {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        paint_caught_mark(ui.painter(), rect.center());
                        response.on_hover_text("Caught");
                    } else if !seen {
                        ui.scope(|ui| {
                            let text = "Not seen";
                            ui.set_max_width(text_width(ui, RichText::new(text).small()) + 12.0);
                            chip(ui, text);
                        });
                    }
                });
                ui.label(RichText::new(level).weak());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!("{}%", encounter.percent));
            });
        });
    }
}
