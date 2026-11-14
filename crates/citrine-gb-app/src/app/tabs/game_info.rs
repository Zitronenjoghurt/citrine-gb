use crate::app::big_screen::scroll::Following;
use parts::{badges, badges_width, card, fact, fact_width, money, text_width, tile_color};
use sprites::sprite_texture;
use status::{Clock, status_line};
mod bag;
mod battle;
mod boxes;
mod journey;
mod legality;
mod map;
mod parts;
mod party;
mod pokedex;
mod region_map;
pub(crate) mod sprites;
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
const DEX_ROW: f32 = 170.0;
const DEX_ROW_HEIGHT: f32 = 30.0;

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
            .show_following(ui, |ui| {
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
                    let shown = self.shown_badges(state);
                    ui.label(format!(
                        "{} of {} badges",
                        shown.iter().filter(|b| **b).count(),
                        shown.len()
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
            let shown = self.shown_badges(state);
            let badge_fact = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new("Badges").weak()).extend());
                    badges(ui, shown);
                })
                .response
                .on_hover_text(format!(
                    "{} of {} badges",
                    shown.iter().filter(|b| **b).count(),
                    shown.len()
                ));
            };
            let gap = 22.0;
            let mut widths: Vec<f32> = facts
                .iter()
                .map(|(label, value)| fact_width(ui, label, value))
                .collect();
            widths.push(badges_width(shown.len()));
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
            paint_ball(&painter, rect.center(), size * 0.28, tile_color(ui));
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

fn paint_ball(painter: &egui::Painter, center: egui::Pos2, radius: f32, fill: Color32) {
    let stroke = egui::Stroke::new((radius / 8.0).max(1.0), Color32::from_gray(0xb4));
    painter.circle_stroke(center, radius, stroke);
    painter.line_segment(
        [center - vec2(radius, 0.0), center + vec2(radius, 0.0)],
        stroke,
    );
    painter.circle(center, radius * 0.3, fill, stroke);
}

#[cfg(test)]
mod tests {
    use super::pokedex;
    use super::status::Clock;
    use super::*;
    use crate::game_info::GameInfo;
    use citrine_gb_gamedata::Ram;
    use citrine_gb_gamedata::fixtures::{crystal_cartridge, crystal_live_wram, crystal_save};

    pub(super) const NOON: i64 = 1_761_912_000;

    pub(super) fn clock() -> Clock {
        Clock::in_zone(
            jiff::tz::TimeZone::UTC,
            jiff::Timestamp::from_second(NOON).unwrap(),
        )
    }

    pub(super) fn crystal(saved_at: Option<u64>) -> GameInfo {
        let mut info = GameInfo::default();
        info.set_rom_now(Some("crystal"), crystal_cartridge().into(), saved_at);
        info
    }

    #[test]
    fn every_page_fits_a_narrow_desktop_panel() {
        let save = crystal_save();
        let mut info = crystal(Some(NOON as u64));
        info.read(&save, Ram::Gb(&crystal_live_wram(&save)), NOON as u64);
        let game = info.game().unwrap();
        let ctx = egui::Context::default();
        let mut wide = Vec::new();
        for width in [
            280.0, 300.0, 320.0, 340.0, 360.0, 380.0, 400.0, 419.0, 460.0, 500.0, 540.0, 580.0,
            640.0,
        ] {
            for page in [
                Page::Map,
                Page::Party,
                Page::Journey,
                Page::Bag,
                Page::Pokedex,
            ] {
                let pages = Pages {
                    key: "crystal",
                    name: "POKEMON_SLV",
                    touch: 24.0,
                    game,
                    mobile: false,
                    clock: clock(),
                };
                let mut view = View::default();
                view.page = page;
                for frame in 0..2 {
                    let _ = ctx.run(Default::default(), |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                            let used = ui
                                .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                    pages.show(ui, &mut view, None);
                                })
                                .response
                                .rect
                                .width();
                            if frame == 1 && used > width + 0.5 {
                                wide.push(format!("{page:?} takes {used} in a {width} panel"));
                            }
                        });
                    });
                }
            }
        }
        assert!(wide.is_empty(), "{wide:#?}");
    }

    #[test]
    fn details_with_long_facts_fit_a_narrow_desktop_panel() {
        use citrine_gb_gamedata::{Caught, Gender, Place, TimeOfDay};
        let save = crystal_save();
        let mut info = crystal(Some(NOON as u64));
        info.read(&save, Ram::Gb(&crystal_live_wram(&save)), NOON as u64);
        let game = info.game().unwrap();
        let tables = &game.tables;
        let state = game.state().unwrap();
        let mut mon = state
            .party
            .iter()
            .find_map(|pokemon| match pokemon {
                Pokemon::Hatched(mon) => Some(mon.clone()),
                Pokemon::Egg => None,
            })
            .unwrap();
        let place_len = |location: u16| {
            let caught = Caught {
                time_of_day: TimeOfDay::Morning,
                level: 100,
                ot_gender: Gender::Male,
                location,
            };
            match tables.caught_place(&caught) {
                Some(Place::Landmark(name)) => name.len(),
                _ => 0,
            }
        };
        let location = (0..=0x7D)
            .max_by_key(|location| place_len(*location))
            .unwrap();
        mon.caught = Some(Caught {
            time_of_day: TimeOfDay::Morning,
            level: 100,
            ot_gender: Gender::Male,
            location,
        });
        mon.held_item = (1..=u16::from(u8::MAX))
            .max_by_key(|item| tables.item_name(*item).map_or(0, |name| name.len()));
        mon.ot_name = "WWWWWWW".to_owned();
        mon.ot_id = state.trainer.id.wrapping_add(1);
        let ctx = egui::Context::default();
        let mut wide = Vec::new();
        for width in [
            260.0, 280.0, 300.0, 320.0, 360.0, 400.0, 460.0, 520.0, 600.0,
        ] {
            let pages = Pages {
                key: "crystal",
                name: "POKEMON_SLV",
                touch: 24.0,
                game,
                mobile: false,
                clock: clock(),
            };
            for frame in 0..2 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                        let used = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                pages.details(ui, &mon);
                            })
                            .response
                            .rect
                            .width();
                        if frame == 1 && used > width + 0.5 {
                            wide.push(format!("details take {used} in a {width} panel"));
                        }
                    });
                });
            }
        }
        assert!(wide.is_empty(), "{wide:#?}");
    }

    #[test]
    fn long_facts_wrap_inside_a_narrow_card() {
        let facts = [
            ("Held item", "CHARCOAL".to_owned()),
            ("Hidden Power", "FLYING, power 70".to_owned()),
            ("Original trainer", "WWWWWWW, ID 24602".to_owned()),
            ("Caught", "NEW BARK TOWN in the morning, Lv 5".to_owned()),
        ];
        let ctx = egui::Context::default();
        let mut wide = Vec::new();
        for width in [200.0, 240.0, 280.0, 320.0, 360.0] {
            for frame in 0..2 {
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                        let used = ui
                            .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                party::fact_rows(ui, &facts, |ui, value: &String| {
                                    ui.add(egui::Label::new(value.as_str()).wrap());
                                });
                            })
                            .response
                            .rect
                            .width();
                        if frame == 1 && used > width + 0.5 {
                            wide.push(format!("facts take {used} in {width}"));
                        }
                    });
                });
            }
        }
        assert!(wide.is_empty(), "{wide:#?}");
    }

    #[test]
    fn the_trainer_card_keeps_its_height_on_every_page() {
        let save = crystal_save();
        let mut info = crystal(Some(NOON as u64));
        info.read(&save, Ram::Gb(&crystal_live_wram(&save)), NOON as u64);
        let game = info.game().unwrap();
        let state = game.state().unwrap();
        let pages = Pages {
            key: "crystal",
            name: "POKEMON_SLV",
            touch: 24.0,
            game,
            mobile: false,
            clock: clock(),
        };
        let goal = pages.goal_text(state).expect("next goal");
        let place = "ROUTE 38, Night".to_owned();
        let ctx = egui::Context::default();
        let mut moved = Vec::new();
        for width in [280.0, 320.0, 360.0, 400.0, 460.0, 540.0, 640.0, 800.0] {
            let mut heights = Vec::new();
            for last in [("Next goal", goal.clone()), ("Now at", place.clone())] {
                let mut height = 0.0;
                for _ in 0..2 {
                    let _ = ctx.run(Default::default(), |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let rect = Rect::from_min_size(ui.min_rect().min, vec2(width, 4000.0));
                            height = ui
                                .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                    pages.trainer(ui, state, Some(last.clone()));
                                })
                                .response
                                .rect
                                .height();
                        });
                    });
                }
                heights.push(height);
            }
            if (heights[0] - heights[1]).abs() > 0.5 {
                moved.push(format!("{heights:?} at {width}"));
            }
        }
        assert!(moved.is_empty(), "{moved:#?}");
    }

    #[test]
    fn a_dex_row_keeps_number_name_and_mark_on_one_line() {
        let ctx = egui::Context::default();
        for name in ["Floink", "Fletchinder of a very long name"] {
            let mut row = Rect::NOTHING;
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    row = ui
                        .scope(|ui| pokedex::dex_row(ui, DEX_ROW, 498, Some(name), true))
                        .response
                        .rect;
                });
            });
            assert_eq!(row.size(), vec2(DEX_ROW, DEX_ROW_HEIGHT), "{name}");
            let texts: Vec<Rect> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.visual_bounding_rect()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts.len(), 2, "{name}");
            for text in texts {
                assert!(row.contains_rect(text), "{name}: {text:?} outside {row:?}");
            }
        }
    }
}
