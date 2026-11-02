use parts::{badges, badges_width, card, fact, fact_width, money, text_width, tile_color};
use sprites::sprite_texture;
use status::{Clock, status_line};
mod bag;
mod boxes;
mod parts;
mod party;
mod pokedex;
mod sprites;
mod status;

use crate::app::tabs::TabViewer;
use crate::app::widgets::text::faint;
use crate::game_info::{Game, Page, View};
use crate::icons;
use citrine_gb_gamedata::{GameState, Mon, Pokemon};
use egui::{Align, Color32, CornerRadius, Layout, Rect, RichText, Sense, Ui, vec2};

const TILE: f32 = 56.0;
const LIST_WIDTH: f32 = 220.0;
const DETAILS_MIN: f32 = 360.0;
const GRID_CELL: f32 = 76.0;
const DEX_CELL: f32 = 96.0;
const BOX_SIZE: usize = 20;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let mobile = viewer.ui.settings.layout.is_touch(ui.ctx());
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
                    ui.add(egui::Label::new(faint("Badges")).extend());
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

#[cfg(test)]
mod tests;
