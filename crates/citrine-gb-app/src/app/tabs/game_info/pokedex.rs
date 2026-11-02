use super::parts::{paint_caught_mark, tile_color};
use super::{DEX_CELL, Pages, TILE};
use crate::game_info::View;
use crate::icons;
use citrine_gb_gamedata::GameState;
use egui::{Align, CornerRadius, Rect, RichText, Sense, Ui, vec2};

impl Pages<'_> {
    pub(super) fn pokedex(&self, ui: &mut Ui, state: &GameState, view: &mut View) {
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

    pub(super) fn dex_cell(
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
}
