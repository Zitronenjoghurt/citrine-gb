use super::ROW_HEIGHT;
use crate::app::tabs::TabViewer;
use crate::app::widgets::info_mark::info_mark;
use crate::emulator::Emulator;
use crate::icons;
use crate::partner_link::{PartnerLink, Status};
use citrine_gb_gamedata::{Game, Generation, Palette, RomTables, Side, Sprite, identify};
use citrine_gb_net::GameHash;
use citrine_gb_partner::{Event, Genes, events_for};
use egui::{Label, RichText, Ui};

const TILE: f32 = 48.0;
const SMALL_TILE: f32 = 36.0;
const CARD_TILE: f32 = 96.0;
const NARROW_CARD: f32 = 340.0;
const CAPSULE_HINT: &str = "These arrive through the Time Capsule, the right-hand desk upstairs. It opens after you meet Bill in Ecruteak City, and your party may hold only Pokémon and moves from Red and Blue.";

struct Row {
    event: &'static Event,
    name: String,
    sprite: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub struct EventCache {
    hash: Option<GameHash>,
    game: Option<Game>,
    rows: Vec<Row>,
    search: String,
}

impl EventCache {
    fn refresh(&mut self, ctx: &egui::Context, emulator: &Emulator) {
        let hash = emulator.open_game();
        if hash == self.hash {
            return;
        }
        *self = Self {
            hash,
            ..Self::default()
        };
        if hash.is_some() {
            self.load(ctx, &emulator.gb.cartridge.rom_bytes());
        }
    }

    fn load(&mut self, ctx: &egui::Context, rom: &[u8]) {
        let Some(game) = identify(rom) else {
            return;
        };
        let Ok(tables) = RomTables::load(game, rom) else {
            return;
        };
        let mut rows: Vec<Row> = events_for(game)
            .filter_map(|event| {
                let species = tables.species_for_dex(event.dex)?;
                let name = tables.species_name(species)?.to_owned();
                let shiny = event.genes == Genes::Shiny;
                let sprite = tables
                    .sprite(rom, species, None, Side::Front)
                    .ok()
                    .map(|sprite| {
                        let palette = tables.palette(species, shiny).unwrap_or(Palette::GRAY);
                        ctx.load_texture(
                            format!("event_{}", event.key),
                            image(&sprite, &palette),
                            egui::TextureOptions::NEAREST,
                        )
                    });
                Some(Row {
                    event,
                    name,
                    sprite,
                })
            })
            .collect();
        rows.sort_by_key(|row| row.event.generation != game.generation());
        self.game = Some(game);
        self.rows = rows;
    }

    fn sprite(&self, event: &Event) -> Option<&egui::TextureHandle> {
        self.rows
            .iter()
            .find(|row| row.event.key == event.key)?
            .sprite
            .as_ref()
    }
}

fn image(sprite: &Sprite, palette: &Palette) -> egui::ColorImage {
    let (w, h) = (sprite.width_px(), sprite.height_px());
    let mut outside = vec![false; w * h];
    let mut stack: Vec<(usize, usize)> = (0..w)
        .flat_map(|x| [(x, 0), (x, h - 1)])
        .chain((0..h).flat_map(|y| [(0, y), (w - 1, y)]))
        .collect();
    while let Some((x, y)) = stack.pop() {
        if outside[y * w + x] || sprite.pixel(x, y) != 0 {
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
    let pixels = (0..w * h)
        .map(|i| {
            let [r, g, b] = palette.rgb(sprite.pixel(i % w, i / w));
            let alpha = if outside[i] { 0 } else { 255 };
            egui::Color32::from_rgba_unmultiplied(r, g, b, alpha)
        })
        .collect();
    egui::ColorImage::new([w, h], pixels)
}

pub fn has_events(viewer: &mut TabViewer, ctx: &egui::Context) -> bool {
    let cache = &mut viewer.ui.linked.events;
    cache.refresh(ctx, viewer.emulator);
    !cache.rows.is_empty()
}

fn tile(ui: &mut Ui, sprite: Option<&egui::TextureHandle>, side: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, side / 8.0, ui.visuals().faint_bg_color);
    if let Some(sprite) = sprite {
        egui::Image::new(egui::load::SizedTexture::new(sprite.id(), rect.size()))
            .paint_at(ui, rect);
    }
}

fn title(event: &Event, name: &str) -> String {
    if event.egg {
        format!("{}  {name} egg", icons::EGG)
    } else {
        name.to_owned()
    }
}

fn when(event: &Event) -> String {
    if event.egg {
        event.year.to_string()
    } else {
        format!("Level {}, {}", event.level, event.year)
    }
}

fn sparkle(ui: &mut Ui, event: &Event) {
    if event.genes == Genes::Shiny {
        ui.label(RichText::new(icons::SPARKLE).color(ui.visuals().warn_fg_color))
            .on_hover_text("Shiny");
    }
}

fn card_row(
    ui: &mut Ui,
    sprite: Option<&egui::TextureHandle>,
    content: impl FnOnce(&mut Ui),
    button: &str,
) -> bool {
    let mut picked = false;
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let side = if ui.available_width() < NARROW_CARD {
                SMALL_TILE
            } else {
                TILE
            };
            tile(ui, sprite, side);
            let side = crate::app::mobile::touch_square(ui).max(ROW_HEIGHT * 0.8);
            let button_width = side * 1.5;
            let text_width = ui.available_width() - button_width - ui.spacing().item_spacing.x;
            ui.vertical(|ui| {
                ui.set_max_width(text_width);
                ui.style_mut().interaction.selectable_labels = false;
                content(ui);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                picked = ui
                    .add(egui::Button::new(button).min_size(egui::vec2(button_width, 0.0)))
                    .clicked();
            });
        });
    });
    picked
}

pub fn shelf_row(viewer: &mut TabViewer, ui: &mut Ui, button_width: f32) {
    let count = viewer.ui.linked.events.rows.len();
    let game = viewer.emulator.game_name().to_owned();
    if shelf_card(ui, count, &game, button_width) {
        viewer.ui.linked.listing = true;
    }
}

fn shelf_card(ui: &mut Ui, count: usize, game: &str, button_width: f32) -> bool {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        let mut picked = false;
        ui.horizontal(|ui| {
            let side = ui.spacing().interact_size.y;
            ui.add_sized(
                egui::vec2(side * 0.6, side),
                Label::new(RichText::new(icons::HANDSHAKE)).selectable(false),
            );
            let text_width = ui.available_width() - button_width - ui.spacing().item_spacing.x;
            ui.vertical(|ui| {
                ui.set_max_width(text_width);
                ui.style_mut().interaction.selectable_labels = false;
                ui.add(Label::new(RichText::new("Event partner").strong().size(16.0)).truncate());
                ui.add(
                    Label::new(RichText::new(format!("{count} events for {game}")).small())
                        .truncate(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                picked = ui
                    .add(egui::Button::new("Open").min_size(egui::vec2(button_width, 0.0)))
                    .clicked();
            });
        });
        picked
    })
    .inner
}

pub fn list(viewer: &mut TabViewer, ui: &mut Ui) {
    let touch = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
    viewer.ui.linked.events.refresh(ui.ctx(), viewer.emulator);
    if !touch
        && ui.ctx().input(|i| i.key_pressed(egui::Key::Escape))
        && !crate::app::mobile::layer_open(ui.ctx())
    {
        viewer.ui.linked.listing = false;
    }
    let (back, chosen) = list_body(ui, &mut viewer.ui.linked.events, touch);
    if back {
        viewer.ui.linked.listing = false;
    } else if let Some(event) = chosen {
        viewer.events.start_partner(event);
    }
}

fn list_body(ui: &mut Ui, cache: &mut EventCache, touch: bool) -> (bool, Option<&'static Event>) {
    let count = cache.rows.len();
    let mut back = false;
    ui.horizontal(|ui| {
        if !touch {
            back = ui
                .add_sized(
                    egui::vec2(0.0, ui.spacing().interact_size.y),
                    egui::Button::new(format!("{}  Back", icons::ARROW_LEFT)),
                )
                .clicked();
        }
        ui.add(
            egui::TextEdit::singleline(&mut cache.search)
                .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                .vertical_align(egui::Align::Center)
                .hint_text(format!(
                    "{}  Search {count} events",
                    icons::MAGNIFYING_GLASS
                ))
                .desired_width(f32::INFINITY),
        );
    });
    if back {
        return (true, None);
    }
    ui.add_space(4.0);
    let needle = cache.search.trim().to_lowercase();
    let own = cache.game.map(Game::generation);
    let shown: Vec<usize> = (0..cache.rows.len())
        .filter(|&i| {
            let row = &cache.rows[i];
            needle.is_empty()
                || row.name.to_lowercase().contains(&needle)
                || row.event.place.to_lowercase().contains(&needle)
                || row.event.year.to_string().contains(&needle)
        })
        .collect();
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if shown.is_empty() {
                ui.label(RichText::new("No events match").weak());
            }
            let mut heading_for = None;
            for i in shown {
                let row = &cache.rows[i];
                let generation = row.event.generation;
                if own == Some(Generation::Two) && heading_for != Some(generation) {
                    heading_for = Some(generation);
                    heading(ui, generation);
                }
                let picked = card_row(
                    ui,
                    row.sprite.as_ref(),
                    |ui| {
                        ui.spacing_mut().interact_size.y = 0.0;
                        ui.horizontal(|ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(title(row.event, &row.name))
                                        .strong()
                                        .size(16.0),
                                )
                                .truncate(),
                            );
                            sparkle(ui, row.event);
                        });
                        ui.add(Label::new(RichText::new(row.event.place).small()).truncate());
                        ui.add(Label::new(RichText::new(when(row.event)).small()).truncate());
                    },
                    "Trade",
                );
                if picked {
                    chosen = Some(row.event);
                }
            }
        });
    (false, chosen)
}

fn heading(ui: &mut Ui, generation: Generation) {
    ui.horizontal(|ui| match generation {
        Generation::Two => {
            ui.label(RichText::new("Gold and Silver events").strong());
        }
        Generation::One => {
            ui.label(RichText::new("Red, Blue and Yellow events").strong());
            info_mark(ui, CAPSULE_HINT);
        }
    });
}

struct Card<'a> {
    event: &'static Event,
    name: &'a str,
    trainer: &'a str,
    ot_id: u16,
    held: Option<&'a str>,
    moves: &'a [String],
}

impl<'a> Card<'a> {
    fn of(partner: &'a PartnerLink) -> Self {
        let offered = partner.partner().offered();
        Self {
            event: partner.event(),
            name: partner.name(),
            trainer: offered.trainer,
            ot_id: offered.ot_id,
            held: partner.held(),
            moves: partner.moves(),
        }
    }
}

#[derive(Default)]
struct PanelPicks {
    plugged: Option<bool>,
    another: bool,
    close: bool,
}

pub fn panel(viewer: &mut TabViewer, ui: &mut Ui) {
    let Some(partner) = viewer.emulator.partner.as_ref() else {
        return;
    };
    viewer.ui.linked.events.refresh(ui.ctx(), viewer.emulator);
    let sprite = viewer.ui.linked.events.sprite(partner.event()).cloned();
    let picks = egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            panel_body(
                ui,
                &Card::of(partner),
                &partner.status(),
                sprite.as_ref(),
                partner.plugged(),
            )
        })
        .inner;
    if let Some(plugged) = picks.plugged
        && let Some(partner) = viewer.emulator.partner.as_mut()
    {
        partner.set_plugged(plugged);
    }
    if picks.another {
        viewer.ui.linked.listing = true;
    }
    if picks.close {
        viewer.events.end_link();
    }
}

fn panel_body(
    ui: &mut Ui,
    card_data: &Card,
    status: &Status,
    sprite: Option<&egui::TextureHandle>,
    plugged: bool,
) -> PanelPicks {
    let mut picks = PanelPicks::default();
    let row_padding =
        ((ui.spacing().interact_size.y - ui.text_style_height(&egui::TextStyle::Body)) / 2.0)
            .max(0.0);
    card(ui, card_data, sprite);
    ui.add_space(6.0 + row_padding);
    status_card(ui, status);
    ui.add_space(6.0);
    let mut plugged = plugged;
    if ui.checkbox(&mut plugged, "Link cable plugged in").changed() {
        picks.plugged = Some(plugged);
    }
    ui.add_space(6.0);
    let height = ui.spacing().interact_size.y.max(32.0);
    let another = format!("{}  Another event", icons::LIST_BULLETS);
    let close = format!("{}  Close event partner", icons::X);
    let padding = ui.spacing().button_padding.x;
    let needed = [&another, &close]
        .iter()
        .map(|label| {
            egui::WidgetText::from(label.as_str())
                .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Button)
                .size()
                .x
                + 2.0 * padding
        })
        .fold(0.0, f32::max);
    if needed * 2.0 + ui.spacing().item_spacing.x <= ui.available_width() {
        ui.columns(2, |columns| {
            let width = columns[0].available_width();
            picks.another = columns[0]
                .add_sized(egui::vec2(width, height), egui::Button::new(&another))
                .clicked();
            picks.close = columns[1]
                .add_sized(egui::vec2(width, height), egui::Button::new(&close))
                .clicked();
        });
    } else {
        let size = egui::vec2(ui.available_width(), height);
        picks.another = ui.add_sized(size, egui::Button::new(&another)).clicked();
        picks.close = ui.add_sized(size, egui::Button::new(&close)).clicked();
    }
    picks
}

fn card(ui: &mut Ui, card: &Card, sprite: Option<&egui::TextureHandle>) {
    let narrow = ui.available_width() < NARROW_CARD;
    let facts = facts(card);
    let label_width = facts
        .iter()
        .map(|(label, _)| {
            egui::WidgetText::from(*label)
                .into_galley(ui, None, f32::INFINITY, egui::TextStyle::Body)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            tile(ui, sprite, if narrow { TILE } else { CARD_TILE });
            ui.vertical(|ui| {
                card_title(ui, card);
                fact_grid(ui, "partner_facts", &facts, label_width);
            });
        });
        ui.add_space(4.0);
        ui.columns(2, |columns| {
            for (i, name) in card.moves.iter().enumerate() {
                let column = &mut columns[i % 2];
                egui::Frame::new()
                    .fill(column.visuals().faint_bg_color)
                    .corner_radius(3.0)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(column, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(Label::new(name.as_str()).truncate());
                    });
            }
        });
    });
}

fn card_title(ui: &mut Ui, card: &Card) {
    let event = card.event;
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.add(
                Label::new(RichText::new(title(event, card.name)).size(20.0).strong()).truncate(),
            );
            sparkle(ui, event);
        },
        |ui| {
            if !event.egg {
                ui.label(RichText::new(format!("Level {}", event.level)).weak());
            }
        },
    );
}

fn facts(card: &Card) -> Vec<(&'static str, String)> {
    let event = card.event;
    let mut facts = vec![
        ("OT", card.trainer.to_owned()),
        ("ID", format!("{:05}", card.ot_id)),
    ];
    if let Some(held) = card.held {
        facts.push(("Holds", held.to_owned()));
    }
    facts.push(("From", format!("{}, {}", event.place, event.year)));
    facts
}

fn fact_grid(ui: &mut Ui, id: &str, facts: &[(&str, String)], label_width: f32) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing(egui::vec2(14.0, 2.0))
        .min_col_width(label_width)
        .min_row_height(ui.text_style_height(&egui::TextStyle::Body))
        .show(ui, |ui| {
            for (label, value) in facts {
                ui.label(RichText::new(*label).weak());
                ui.add(Label::new(value.as_str()).wrap());
                ui.end_row();
            }
        });
}

pub fn status(ui: &mut Ui, partner: &PartnerLink, sprite: Option<&egui::TextureHandle>) {
    let status = partner.status();
    ui.horizontal(|ui| {
        if sprite.is_some() {
            tile(ui, sprite, TILE);
        }
        ui.label(RichText::new(status.icon).weak());
        status_text(ui, &status);
    });
}

fn status_card(ui: &mut Ui, status: &Status) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().interact_size.y = 0.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new(status.icon).size(20.0));
            status_text(ui, status);
        });
    });
}

fn status_text(ui: &mut Ui, status: &Status) {
    ui.vertical(|ui| {
        ui.add(Label::new(RichText::new(status.headline).strong()).truncate());
        if let Some(detail) = &status.detail {
            ui.add(Label::new(RichText::new(detail).weak()).wrap());
        }
    });
}

pub fn sprite(cache: &EventCache, partner: &PartnerLink) -> Option<egui::TextureHandle> {
    cache.sprite(partner.event()).cloned()
}
