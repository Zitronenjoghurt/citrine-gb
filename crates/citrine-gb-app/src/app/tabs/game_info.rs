use parts::{badges, badges_width, card, fact, fact_width, money, text_width, tile_color};
use sprites::sprite_texture;
use status::{Clock, status_line};
mod bag;
mod battle;
mod boxes;
mod journey;
mod map;
mod parts;
mod party;
mod pokedex;
mod region_map;
mod sprites;
mod status;

use crate::app::tabs::TabViewer;
use crate::app::widgets::scroll;
use crate::app::widgets::tab_bar::{side_tabs, side_tabs_width, tab_bar};
use crate::game_info::{Game, Page, View};
use citrine_gb_gamedata::{GameState, Mon, Pokemon};
use egui::{Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Ui, vec2};

const SIDEBAR_MIN_WIDTH: f32 = 420.0;
const TILE: f32 = 56.0;
const LIST_WIDTH: f32 = 220.0;
const DETAILS_MIN: f32 = 345.0;
const DETAILS_TWO_COLUMNS: f32 = 520.0;
const HERE_TWO_COLUMNS: f32 = 560.0;
const GRID_CELL: f32 = 76.0;
const DEX_CELL: f32 = 96.0;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let mobile = viewer.ui.settings.layout.is_touch(ui.ctx());
    let key = viewer.game_info.rom_key().unwrap_or_default().to_owned();
    let name = viewer.emulator.game_name().to_owned();
    let (game, view) = viewer.game_info.parts();
    let Some(game) = game else {
        return;
    };
    let pages = Pages {
        key: &key,
        name: &name,
        touch: ui.spacing().interact_size.y,
        game,
        mobile,
        clock: Clock::new(),
    };
    let tabs = pages.tabs(view);
    let scroll = |ui: &mut Ui, view: &mut View, tab_row: bool| {
        let gutter = scroll::fixed_gutter(ui);
        egui::ScrollArea::vertical()
            .id_salt("game_info_scroll")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() - gutter);
                ui.style_mut().interaction.selectable_labels = false;
                ui.spacing_mut().interact_size.y = egui::style::Spacing::default().interact_size.y;
                pages.show(ui, view, tab_row.then_some(tabs.as_slice()));
            });
    };
    let details = mobile && view.details;
    if tabs.is_empty() || details || ui.available_width() < SIDEBAR_MIN_WIDTH {
        scroll(ui, view, true);
        return;
    }
    let height = ui.available_height();
    let tabs_width = side_tabs_width(ui, &tabs) + 8.0;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(tabs_width);
            ui.set_height(height);
            side_tabs(ui, &mut view.page, &tabs, 2.0);
        });
        ui.separator();
        ui.vertical(|ui| scroll(ui, view, false));
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
    fn tabs(&self, view: &mut View) -> Vec<(Page, String)> {
        let Some(state) = self.game.state() else {
            return Vec::new();
        };
        view.follow_battle(state.battle.is_some());
        let pages = Page::available(state);
        if !pages.contains(&view.page) {
            view.page = Page::default();
        }
        pages
            .into_iter()
            .map(|page| (page, format!("{} {}", page.icon(), page.label())))
            .collect()
    }

    fn show(&self, ui: &mut Ui, view: &mut View, tab_row: Option<&[(Page, String)]>) {
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
        let last = match view.page {
            Page::Map => self.goal_text(state).map(|goal| ("Next goal", goal)),
            _ => self.place_name(state).map(|place| ("Now at", place)),
        };
        if self.mobile && view.page == Page::Map {
            self.trainer_lines(ui, state, last);
        } else {
            self.trainer(ui, state, last);
        }
        ui.add_space(8.0);
        if let Some(tabs) = tab_row {
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = self.touch;
                tab_bar(ui, &mut view.page, tabs);
            });
            ui.add_space(8.0);
        }
        match view.page {
            Page::Map => self.map(ui, state),
            Page::Battle => self.battle(ui, state),
            Page::Journey => self.journey(ui, state),
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

    fn trainer_lines(&self, ui: &mut Ui, state: &GameState, last: Option<(&str, String)>) {
        let trainer = &state.trainer;
        card(ui, false).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(&trainer.name).size(19.0).strong());
                ui.label(RichText::new(format!("ID {:05}", trainer.id)).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(format!(
                        "{} of {} badges",
                        state.badge_count(),
                        trainer.badges.len()
                    ));
                });
            });
            let line = match last {
                Some((label, value)) => format!("{label}: {value}"),
                None => format!("Money {}", money(trainer.money)),
            };
            ui.add(egui::Label::new(RichText::new(line).weak()).truncate());
        });
    }

    fn trainer(&self, ui: &mut Ui, state: &GameState, last: Option<(&str, String)>) {
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
                    ui.add(egui::Label::new(RichText::new("Badges").weak()).extend());
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
            if let Some((label, value)) = &last {
                widths.push(fact_width(ui, label, value));
            }
            let dex = facts.len() - 1;
            let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
            let mut used = 0.0;
            for (item, &width) in widths.iter().enumerate() {
                let own_row = item > facts.len();
                let needed = if item == dex {
                    width + gap + widths[dex + 1]
                } else {
                    width
                };
                if used > 0.0 && (own_row || used + gap + needed > ui.available_width()) {
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
                            None if item == facts.len() => badge_fact(ui),
                            None => {
                                if let Some((label, value)) = &last {
                                    fact(ui, label, value);
                                }
                            }
                        }
                    }
                });
            }
        });
    }

    fn tile(&self, ui: &mut Ui, sprite: Option<(u16, bool, char)>, size: f32) {
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
        let [w, h] = texture.size();
        let scale = size / TILE.max(w as f32).max(h as f32);
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

#[cfg(test)]
mod tests;
