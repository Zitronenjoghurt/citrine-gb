use super::parts::{beside, box_name, card_with_margin};
use super::sprites::pokemon_sprite;
use super::{BOX_SIZE, GRID_CELL, Pages, TILE};
use crate::game_info::View;
use crate::icons;
use citrine_gb_gamedata::{GameState, Pokemon};
use egui::{RichText, Sense, Ui, vec2};

impl Pages<'_> {
    pub(super) fn boxes(&self, ui: &mut Ui, state: &GameState, view: &mut View) {
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

    pub(super) fn box_cell(
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
}
