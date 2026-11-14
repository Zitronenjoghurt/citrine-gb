use super::TabViewer;
use crate::app::mobile::kit;
use crate::app::tabs::friends::{DESK_SECOND_LINE, ago};
use crate::app::tabs::game_info::sprites::sprite_texture;
use crate::app::widgets::action::{icon_button, quiet_button};
use crate::app::widgets::avatar;
use crate::app::widgets::field::row_field;
use crate::app::widgets::row::row_highlight;
use crate::icons;
use crate::online::Online;
use crate::online::gts::{GTS_GENERATION, GtsQuery, GtsSimNote};
use citrine_gb_net::{GTS_PAGE, GtsDeposit, GtsListing, GtsPokemon, GtsState, GtsWanted, OnlineId};
use egui::{Align, Color32, CursorIcon, Layout, RichText, Sense, TextStyle, Ui, vec2};

const SEARCH_HITS: usize = 6;
const DESK_SPRITE: f32 = 40.0;
const PHONE_SPRITE: f32 = 48.0;
const FRIEND_FACE: f32 = 18.0;
const MALE: u8 = 1;
const FEMALE: u8 = 2;
const SIM_HOVER: &str = "A simulated trainer";

#[derive(Default)]
pub struct GtsPane {
    names: Vec<String>,
    names_from: Option<String>,
    search: String,
    query: GtsQuery,
    #[cfg(ds_wifi)]
    pub switch: SwitchGuard,
}

#[cfg(ds_wifi)]
#[derive(Default)]
pub struct SwitchGuard {
    seen: Option<crate::app::ui_state::ds_online::OnlineService>,
    asking: Option<crate::app::ui_state::ds_online::OnlineService>,
}

#[cfg(ds_wifi)]
impl SwitchGuard {
    pub fn guard(
        &mut self,
        ctx: &egui::Context,
        online: &mut crate::app::ui_state::ds_online::DsOnline,
        deposits: usize,
    ) -> bool {
        use crate::app::ui_state::ds_online::OnlineService;
        if let Some(target) = self.asking {
            let message = switch_message(deposits, target);
            match crate::app::widgets::confirm::confirm(ctx, "gts_switch", &message, "Switch") {
                Some(true) => {
                    online.set_service(target);
                    self.seen = Some(target);
                    self.asking = None;
                    return true;
                }
                Some(false) => self.asking = None,
                None => {}
            }
            return false;
        }
        let now = online.service;
        if self.seen == Some(OnlineService::Citrine)
            && now != OnlineService::Citrine
            && deposits > 0
        {
            online.set_service(OnlineService::Citrine);
            self.asking = Some(now);
            return true;
        }
        self.seen = Some(now);
        false
    }
}

#[cfg(ds_wifi)]
fn switch_message(
    deposits: usize,
    target: crate::app::ui_state::ds_online::OnlineService,
) -> String {
    let what = if deposits == 1 {
        "1 of your Pokémon".to_owned()
    } else {
        format!("{deposits} of your Pokémon")
    };
    format!(
        "Your relay's GTS holds {what}. {target} can't see the relay's GTS, and the game may lose track of a deposit there. Switch anyway?"
    )
}

impl GtsPane {
    pub fn learn_names(
        &mut self,
        game_info: &crate::game_info::GameInfo,
        store: &crate::storage::SaveStore,
    ) {
        let tables = game_info
            .game()
            .and_then(|game| game.tables.ds())
            .zip(game_info.rom_key());
        let Some((tables, key)) = tables else {
            if self.names.is_empty() && self.names_from.is_none() {
                self.names = store.species_names(GTS_GENERATION).unwrap_or_default();
                self.names_from = Some(String::new());
            }
            return;
        };
        if self.names_from.as_deref() == Some(key) {
            return;
        }
        let names: Vec<String> = (0..=tables.pokedex_size())
            .map(|species| tables.species_name(species).unwrap_or_default().to_owned())
            .collect();
        if names != self.names
            && let Err(err) = store.put_species_names(GTS_GENERATION, &names)
        {
            log::warn!("couldn't keep the species names: {err}");
        }
        self.names = names;
        self.names_from = Some(key.to_owned());
    }

    fn known(&self, species: u16) -> Option<&str> {
        self.names
            .get(usize::from(species))
            .map(String::as_str)
            .filter(|name| !name.is_empty())
    }

    pub fn species(&self, species: u16) -> String {
        self.known(species)
            .map_or_else(|| format!("No. {species}"), str::to_owned)
    }

    pub fn pokemon(&self, pokemon: &GtsPokemon) -> String {
        match self.known(pokemon.species) {
            Some(name)
                if pokemon.nickname.is_empty()
                    || pokemon.nickname.to_lowercase() == name.to_lowercase() =>
            {
                name.to_owned()
            }
            Some(name) => format!("{} ({name})", pokemon.nickname),
            None if !pokemon.nickname.is_empty() => pokemon.nickname.clone(),
            None => self.species(pokemon.species),
        }
    }

    pub fn traded_note(&self, deposit: &GtsDeposit) -> Option<String> {
        let GtsState::Traded {
            received,
            trainer,
            simulated,
            ..
        } = &deposit.state
        else {
            return None;
        };
        let traded = format!(
            "{} was traded for {}",
            self.pokemon(&deposit.listing.offered),
            self.pokemon(received)
        );
        Some(if *simulated {
            format!("{traded} with a simulated trainer, {trainer}")
        } else {
            traded
        })
    }

    fn hits(&self) -> Vec<u16> {
        let typed = self.search.trim().to_lowercase();
        if typed.is_empty() {
            return Vec::new();
        }
        let mut starts = Vec::new();
        let mut inside = Vec::new();
        for (species, name) in self.names.iter().enumerate().skip(1) {
            let lower = name.to_lowercase();
            if lower.starts_with(&typed) {
                starts.push(species as u16);
            } else if lower.contains(&typed) {
                inside.push(species as u16);
            }
        }
        starts.extend(inside);
        starts.truncate(SEARCH_HITS);
        starts
    }
}

pub fn sim_note(note: &GtsSimNote) -> String {
    match note {
        GtsSimNote::Searched => "Search results include simulated trainers".to_owned(),
        GtsSimNote::TradedWith(trainer) => {
            format!("You traded with a simulated trainer, {trainer}")
        }
    }
}

pub fn wanted_line(pane: &GtsPane, wanted: &GtsWanted) -> String {
    let gender = match wanted.gender {
        MALE => format!(" {}", icons::GENDER_MALE),
        FEMALE => format!(" {}", icons::GENDER_FEMALE),
        _ => String::new(),
    };
    let levels = match (wanted.min_level, wanted.max_level) {
        (0, 0) => "any level".to_owned(),
        (min, 0) => format!("Lv {min} and up"),
        (0, max) => format!("up to Lv {max}"),
        (min, max) if min == max => format!("Lv {min}"),
        (min, max) => format!("Lv {min} to {max}"),
    };
    format!("Wants {}{gender}, {levels}", pane.species(wanted.species))
}

enum Act {
    Browse(GtsQuery),
    Reload,
    Search(Option<u16>),
}

pub fn show(viewer: &mut TabViewer<'_>, ui: &mut Ui) {
    let phone = viewer.ui.settings.layout.is_touch(ui.ctx());
    let unix_now = jiff::Timestamp::now().as_second().max(0) as u64;
    viewer
        .ui
        .gts
        .learn_names(viewer.game_info, &viewer.emulator.store);
    let relay_service = viewer.ui.settings.ds_online.gts_here();
    let online = &mut *viewer.online;
    let pane = &mut viewer.ui.gts;
    if online.gts_open() {
        online.gts_want_mine();
        online.gts_browse(pane.query);
    }
    let game = viewer
        .game_info
        .game()
        .filter(|game| game.tables.ds().is_some());
    let key = viewer.game_info.rom_key().unwrap_or_default().to_owned();
    let sprites = |ui: &Ui, species: u16, shiny: bool| {
        game.and_then(|game| sprite_texture(ui.ctx(), &key, game, species, shiny, 'A'))
    };
    let mut act = None;
    egui::ScrollArea::vertical()
        .id_salt("gts_pane")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let margin = if phone {
                egui::Margin::ZERO
            } else {
                egui::Margin {
                    left: 12,
                    right: 12,
                    top: 10,
                    bottom: 16,
                }
            };
            egui::Frame::new().inner_margin(margin).show(ui, |ui| {
                if let Some(line) = closed_line(online, relay_service) {
                    if phone {
                        kit::card(ui, |ui| {
                            kit::padded(ui, |ui| ui.label(RichText::new(line).weak()));
                        });
                    } else {
                        ui.label(RichText::new(line).weak());
                    }
                    return;
                }
                let face = if phone { PHONE_SPRITE } else { DESK_SPRITE };
                let reload = heading(ui, phone, "Yours", Some(online.gts_loading()));
                if reload {
                    act = Some(Act::Reload);
                }
                let mine = online.gts_mine().map(<[_]>::to_vec);
                match mine.as_deref() {
                    None => placeholder(ui, phone, "Loading"),
                    Some([]) => placeholder(ui, phone, "Nothing deposited"),
                    Some(mine) => rows(ui, phone, mine.len(), |ui, at| {
                        let deposit = &mine[at];
                        let offered = &deposit.listing.offered;
                        let texture = sprites(ui, offered.species, offered.shiny);
                        let (line, state) = match &deposit.state {
                            GtsState::Waiting => (
                                wanted_line(pane, &deposit.listing.wanted),
                                State::Waiting(ago(deposit.listing.deposited, unix_now)),
                            ),
                            GtsState::Traded {
                                received,
                                trainer,
                                simulated,
                                ..
                            } => (
                                format!(
                                    "For {} with {trainer}{}",
                                    pane.pokemon(received),
                                    if *simulated {
                                        format!(" {}", icons::ROBOT)
                                    } else {
                                        String::new()
                                    }
                                ),
                                State::Traded,
                            ),
                        };
                        pokemon_row(
                            ui,
                            face,
                            texture,
                            &pane.pokemon(offered),
                            offered,
                            &line,
                            |ui| state.show(ui),
                        );
                    }),
                }

                let total = online.gts_page().map_or(0, |page| page.total);
                heading(ui, phone, &format!("On offer ({total})"), None);
                if online.gts_simulated() {
                    sims_line(ui, phone);
                }
                if let Some(species) = search(ui, phone, pane) {
                    act = Some(Act::Search(species));
                }
                let page = online.gts_page().cloned();
                match page {
                    None => placeholder(ui, phone, "Loading"),
                    Some(page) if page.listings.is_empty() => {
                        let empty = match page.query.species {
                            Some(species) => format!("No {} on offer", pane.species(species)),
                            None => "Nothing on offer yet".to_owned(),
                        };
                        placeholder(ui, phone, &empty);
                    }
                    Some(page) => {
                        rows(ui, phone, page.listings.len(), |ui, at| {
                            let listing = &page.listings[at];
                            let offered = &listing.offered;
                            let texture = sprites(ui, offered.species, offered.shiny);
                            pokemon_row(
                                ui,
                                face,
                                texture,
                                &pane.pokemon(offered),
                                offered,
                                &wanted_line(pane, &listing.wanted),
                                |ui| trainer(ui, online, listing, unix_now),
                            );
                        });
                        if let Some(next) = pager(ui, page.query, page.total) {
                            act = Some(Act::Browse(next));
                        }
                    }
                }
            });
        });
    match act {
        Some(Act::Browse(query)) => pane.query = query,
        Some(Act::Search(species)) => {
            pane.query = GtsQuery { species, page: 0 };
            pane.search = species.map(|s| pane.species(s)).unwrap_or_default();
        }
        Some(Act::Reload) => online.gts_reload(pane.query),
        None => {}
    }
}

fn closed_line(online: &Online, relay_service: bool) -> Option<&'static str> {
    if online.account().is_none() {
        Some("Sign in to a relay to trade on its GTS.")
    } else if online.gts_off() {
        Some("This relay has no GTS.")
    } else if !online.is_connected() {
        Some("Offline. The GTS shows again once the relay is back.")
    } else if !relay_service {
        Some(
            "Your DS games use another online service. Pick Citrine relay in DS settings to trade here.",
        )
    } else {
        None
    }
}

fn sims_line(ui: &mut Ui, phone: bool) {
    let text = RichText::new(format!("{} Includes simulated trainers", icons::ROBOT))
        .small()
        .weak();
    if phone {
        ui.add_space(-ui.spacing().item_spacing.y);
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 6,
                bottom: 6,
                ..Default::default()
            })
            .show(ui, |ui| ui.label(text));
    } else {
        ui.label(text);
    }
}

fn heading(ui: &mut Ui, phone: bool, text: &str, reload: Option<bool>) -> bool {
    let mut clicked = false;
    if phone {
        match reload {
            Some(_) => {
                clicked = kit::section_label_button(ui, text, icons::ARROWS_CLOCKWISE, "Reload")
            }
            None => kit::section_label(ui, text),
        }
        return clicked;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).strong());
        if let Some(loading) = reload {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let side = ui.spacing().interact_size.y;
                clicked = ui
                    .add_enabled(!loading, icon_button(icons::ARROWS_CLOCKWISE, side))
                    .on_hover_text("Reload")
                    .clicked();
            });
        }
    });
    ui.add_space(2.0);
    clicked
}

fn placeholder(ui: &mut Ui, phone: bool, text: &str) {
    if phone {
        kit::card(ui, |ui| {
            kit::padded(ui, |ui| ui.label(RichText::new(text).weak()));
        });
    } else {
        ui.label(RichText::new(text).weak());
    }
}

fn rows(ui: &mut Ui, phone: bool, count: usize, mut row: impl FnMut(&mut Ui, usize)) {
    if phone {
        kit::list(ui, |list| {
            for at in 0..count {
                list.custom(|ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.style_mut()
                                .text_styles
                                .insert(TextStyle::Body, egui::FontId::proportional(16.0));
                            ui.style_mut()
                                .text_styles
                                .insert(TextStyle::Small, egui::FontId::proportional(13.0));
                            row(ui, at);
                        });
                });
            }
        });
    } else {
        ui.scope(|ui| {
            ui.style_mut().text_styles.insert(
                TextStyle::Small,
                egui::FontId::proportional(DESK_SECOND_LINE),
            );
            for at in 0..count {
                row(ui, at);
                ui.add_space(4.0);
            }
        });
    }
}

enum State {
    Waiting(String),
    Traded,
}

impl State {
    fn show(&self, ui: &mut Ui) {
        match self {
            State::Waiting(since) => {
                ui.label(RichText::new(since).small().weak())
                    .on_hover_text("Deposited, waiting for a trade");
            }
            State::Traded => {
                ui.label(
                    RichText::new(format!("{} Traded", icons::CHECK_CIRCLE))
                        .small()
                        .color(traded_color(ui)),
                )
                .on_hover_text("Pick it up at the GTS in the game");
            }
        }
    }
}

fn traded_color(ui: &Ui) -> Color32 {
    crate::app::ui_state::ui_theme::brand_mark(ui.visuals())
}

fn pokemon_row(
    ui: &mut Ui,
    face: f32,
    texture: Option<egui::TextureHandle>,
    name: &str,
    pokemon: &GtsPokemon,
    line: &str,
    right: impl FnOnce(&mut Ui),
) {
    let small = ui.text_style_height(&TextStyle::Small);
    let body = ui.text_style_height(&TextStyle::Body);
    let height = face.max(body + 2.0 + small);
    let area = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height));
    if ui.rect_contains_pointer(area) {
        ui.painter().add(row_highlight(ui, area));
    }
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            match texture {
                Some(texture) => {
                    let (rect, _) = ui.allocate_exact_size(vec2(face, face), Sense::hover());
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
                }
                None => {
                    let (rect, _) = ui.allocate_exact_size(vec2(face, face), Sense::hover());
                    let ball = rect.shrink(face * 0.22);
                    ui.painter().circle_stroke(
                        ball.center(),
                        ball.width() / 2.0,
                        (1.5, ui.visuals().weak_text_color()),
                    );
                    ui.painter().hline(
                        ball.x_range(),
                        ball.center().y,
                        (1.5, ui.visuals().weak_text_color()),
                    );
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                right(ui);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.add_space(((height - body - 2.0 - small) / 2.0).max(0.0));
                    let gender = match pokemon.gender {
                        MALE => format!(" {}", icons::GENDER_MALE),
                        FEMALE => format!(" {}", icons::GENDER_FEMALE),
                        _ => String::new(),
                    };
                    let shiny = if pokemon.shiny {
                        format!(" {}", icons::SPARKLE)
                    } else {
                        String::new()
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "{name}{gender}{shiny}  Lv\u{a0}{}",
                                pokemon.level
                            ))
                            .strong(),
                        )
                        .truncate(),
                    );
                    ui.add(egui::Label::new(RichText::new(line).small()).truncate())
                        .on_hover_text(line);
                });
            });
        },
    );
}

fn trainer(ui: &mut Ui, online: &Online, listing: &GtsListing, unix_now: u64) {
    ui.with_layout(Layout::top_down(Align::Max), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        match listing.friend.and_then(|id| friend(online, id)) {
            Some((name, id)) => {
                let line = ui.text_style_height(&TextStyle::Small).max(FRIEND_FACE);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), line),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(RichText::new(&listing.trainer).small());
                        avatar::avatar(ui, &name, id.value(), FRIEND_FACE, None)
                            .on_hover_cursor(CursorIcon::Default)
                            .on_hover_text(format!("Your friend {name}"));
                    },
                );
            }
            None if listing.simulated => {
                let line = ui.text_style_height(&TextStyle::Small);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), line),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(RichText::new(&listing.trainer).small());
                        ui.label(RichText::new(icons::ROBOT).small().weak())
                            .on_hover_cursor(CursorIcon::Default)
                            .on_hover_text(SIM_HOVER);
                    },
                );
            }
            None => {
                ui.label(RichText::new(&listing.trainer).small());
            }
        }
        ui.label(
            RichText::new(ago(listing.deposited, unix_now))
                .small()
                .weak(),
        );
    });
}

fn friend(online: &Online, id: OnlineId) -> Option<(String, OnlineId)> {
    online
        .friends()
        .into_iter()
        .find(|friend| friend.online_id == id)
        .map(|friend| (friend.display_name.clone(), id))
}

fn search(ui: &mut Ui, phone: bool, pane: &mut GtsPane) -> Option<Option<u16>> {
    if pane.names.is_empty() {
        return None;
    }
    let mut picked = None;
    let filtered = pane.query.species.is_some();
    ui.horizontal(|ui| {
        let clear_side = ui.spacing().interact_size.y;
        let room = if filtered || !pane.search.is_empty() {
            ui.available_width() - clear_side - ui.spacing().item_spacing.x
        } else {
            ui.available_width()
        };
        let edit = row_field(ui, &mut pane.search)
            .hint_text(format!("{} Search a Pokémon", icons::MAGNIFYING_GLASS))
            .desired_width(room);
        let response = if phone {
            ui.scope(|ui| {
                ui.set_width(room);
                kit::field(ui, edit)
            })
            .inner
        } else {
            ui.add(edit)
        };
        if response.changed() && pane.search.trim().is_empty() && filtered {
            picked = Some(None);
        }
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(first) = pane.hits().first()
        {
            picked = Some(Some(*first));
        }
        if (filtered || !pane.search.is_empty())
            && ui
                .add(icon_button(icons::X, clear_side))
                .on_hover_text("Clear")
                .clicked()
        {
            pane.search.clear();
            picked = Some(None);
        }
    });
    let chosen = pane.query.species.map(|s| pane.species(s));
    if chosen.as_deref() != Some(pane.search.trim()) {
        let hits = pane.hits();
        if !hits.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for species in hits {
                    if ui.add(quiet_button(ui, pane.species(species))).clicked() {
                        picked = Some(Some(species));
                    }
                }
            });
        }
    }
    ui.add_space(4.0);
    picked
}

fn pager(ui: &mut Ui, query: GtsQuery, total: u32) -> Option<GtsQuery> {
    let per = GTS_PAGE as u32;
    if total <= per {
        return None;
    }
    let pages = total.div_ceil(per);
    let mut next = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let side = ui.spacing().interact_size.y;
        if ui
            .add_enabled(query.page > 0, icon_button(icons::CARET_LEFT, side))
            .on_hover_text("Previous page")
            .clicked()
        {
            next = Some(GtsQuery {
                page: query.page - 1,
                ..query
            });
        }
        let first = query.page * per + 1;
        let last = ((query.page + 1) * per).min(total);
        ui.label(format!("{first} to {last} of {total}"));
        if ui
            .add_enabled(
                query.page + 1 < pages,
                icon_button(icons::CARET_RIGHT, side),
            )
            .on_hover_text("Next page")
            .clicked()
        {
            next = Some(GtsQuery {
                page: query.page + 1,
                ..query
            });
        }
    });
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> GtsPane {
        GtsPane {
            names: vec![
                String::new(),
                "BULBASAUR".into(),
                "IVYSAUR".into(),
                "CYNDAQUIL".into(),
                "QUILAVA".into(),
            ],
            ..GtsPane::default()
        }
    }

    fn wanted(species: u16, gender: u8, min_level: u8, max_level: u8) -> GtsWanted {
        GtsWanted {
            species,
            gender,
            min_level,
            max_level,
        }
    }

    #[test]
    fn a_wanted_line_names_the_species_gender_and_levels() {
        let pane = pane();
        assert_eq!(
            wanted_line(&pane, &wanted(4, 3, 0, 0)),
            "Wants QUILAVA, any level"
        );
        assert_eq!(
            wanted_line(&pane, &wanted(4, MALE, 10, 0)),
            format!("Wants QUILAVA {}, Lv 10 and up", icons::GENDER_MALE)
        );
        assert_eq!(
            wanted_line(&pane, &wanted(1, FEMALE, 0, 30)),
            format!("Wants BULBASAUR {}, up to Lv 30", icons::GENDER_FEMALE)
        );
        assert_eq!(
            wanted_line(&pane, &wanted(2, 3, 11, 20)),
            "Wants IVYSAUR, Lv 11 to 20"
        );
    }

    #[test]
    fn a_trade_with_a_simulated_trainer_says_so() {
        let pane = pane();
        let pokemon = |species| GtsPokemon {
            species,
            level: 5,
            gender: 1,
            shiny: false,
            nickname: String::new(),
            language: 2,
        };
        let deposit = |simulated| GtsDeposit {
            generation: 4,
            listing: GtsListing {
                offered: pokemon(3),
                wanted: wanted(1, 3, 0, 0),
                trainer: "Clara".into(),
                game: 12,
                country: 0,
                deposited: 0,
                friend: None,
                simulated: false,
            },
            state: GtsState::Traded {
                at: 1,
                received: pokemon(1),
                trainer: "Ren".into(),
                simulated,
            },
        };
        assert_eq!(
            pane.traded_note(&deposit(false)).unwrap(),
            "CYNDAQUIL was traded for BULBASAUR"
        );
        assert_eq!(
            pane.traded_note(&deposit(true)).unwrap(),
            "CYNDAQUIL was traded for BULBASAUR with a simulated trainer, Ren"
        );
        assert_eq!(
            sim_note(&GtsSimNote::TradedWith("Ren".into())),
            "You traded with a simulated trainer, Ren"
        );
        assert_eq!(
            sim_note(&GtsSimNote::Searched),
            "Search results include simulated trainers"
        );
    }

    #[test]
    fn a_nickname_shows_with_its_species() {
        let pane = pane();
        let mut pokemon = GtsPokemon {
            species: 3,
            level: 5,
            gender: MALE,
            shiny: false,
            nickname: "Cyndaquil".into(),
            language: 2,
        };
        assert_eq!(pane.pokemon(&pokemon), "CYNDAQUIL");
        pokemon.nickname = "CYNDAQUIL".into();
        assert_eq!(pane.pokemon(&pokemon), "CYNDAQUIL");
        pokemon.nickname = "Ember".into();
        assert_eq!(pane.pokemon(&pokemon), "Ember (CYNDAQUIL)");
        assert_eq!(GtsPane::default().pokemon(&pokemon), "Ember");
    }

    #[test]
    fn with_no_rom_the_names_come_from_the_cache() {
        let root = std::env::temp_dir().join(format!("citrine-gts-names-{}", std::process::id()));
        let cache = root.with_file_name(format!("citrine-gts-names-{}-cache", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&cache);
        let store = crate::storage::SaveStore::at(root.clone());
        store
            .put_species_names(GTS_GENERATION, &pane().names)
            .unwrap();
        let mut fresh = GtsPane::default();
        fresh.learn_names(&crate::game_info::GameInfo::default(), &store);
        assert_eq!(fresh.species(3), "CYNDAQUIL");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn search_puts_names_that_start_with_the_text_first() {
        let mut pane = pane();
        pane.search = "saur".into();
        assert_eq!(pane.hits(), vec![1, 2]);
        pane.search = "i".into();
        assert_eq!(pane.hits(), vec![2, 3, 4]);
        pane.search = "  ".into();
        assert!(pane.hits().is_empty());
    }
}
