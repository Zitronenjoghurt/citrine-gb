use super::{kit, sheet};
use crate::app::Citrine;
use crate::app::archive::download_progress;
use crate::app::big_screen::ring;
use crate::app::events::GamesAction;
use crate::app::tabs::games::library::info::{self, CREDIT, GameCard, tag_row_label, tag_rows};
use crate::app::tabs::games::library::{
    self as shelf, CoverSource, LibraryTile, Press, SORT_WORDS, consoles, count_line, count_words,
    folder_rows, missing_words, place_choice, place_label, places, tag_of,
};
use crate::app::ui_state::games::Undo;
use crate::app::ui_state::library::{
    Access, Filter, Library, LibraryGame, Place, TagPick, scan::shown_path,
};
use crate::app::ui_state::settings::kind_label;
use crate::app::widgets::game_tile::{GameTile, Tag, name_lines};
use crate::app::widgets::pager::Pager;
use crate::app::widgets::tag_chip::{Look, TagChip, Tap};
use crate::icons;
use egui::{Align, Color32, Context, Id, Layout, RichText, Stroke, Ui, vec2};

pub const PHONE_PAGE: usize = 24;
const FILTER_PAD: f32 = 8.0;
const COLUMNS: usize = 3;
const BIG_SCREEN_COLUMNS: usize = 4;
const TILE_GAP: f32 = 10.0;
const COUCH_ROW_GAP: f32 = 24.0;
const TRASH_SIDE: f32 = 48.0;
const THUMB_SIDE: f32 = 64.0;
const CHIP_GAP: f32 = 8.0;
const TAG_CHIPS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilterView {
    Choices,
    Tags,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilterStep {
    Open(FilterView),
    Done,
}

#[derive(Default)]
pub struct LibrarySheets {
    filter: Option<FilterView>,
    game: Option<String>,
}

impl LibrarySheets {
    pub fn game(&self) -> Option<&str> {
        self.game.as_deref()
    }

    #[cfg(test)]
    pub fn open(&self) -> bool {
        self.filter.is_some() || self.game.is_some()
    }
}

impl Citrine {
    pub(super) fn library_section(&mut self, ui: &mut Ui, search: &str) -> Option<GamesAction> {
        let couch = self.big_screen_on();
        let columns = if couch { BIG_SCREEN_COLUMNS } else { COLUMNS };
        let library = &self.ui.library;
        if library.is_empty() && library.folders().is_empty() {
            return None;
        }
        let source = CoverSource {
            ctx: ui.ctx(),
            store: &self.emulator.store,
            playing: self.emulator.rom_key(),
            boxes: self.ui.settings.covers,
        };
        let achievements = &*self.achievements;
        let ra = &self.ui.settings.achievements;
        let progress = |hash: Option<&str>| achievements.progress(ra, hash);
        let online = &self.online;
        let downloads = |game: &LibraryGame| download_progress(online, game);
        let pane = &mut self.ui.games;
        pane.library_shown = true;
        let found = shelf::shelf(
            library,
            pane,
            search,
            PHONE_PAGE,
            &mut self.big.covers,
            &source,
            &shelf::TileFacts {
                progress: &progress,
                downloads: &downloads,
            },
        );
        if found.shown == 0 && !search.is_empty() {
            return None;
        }
        let mut action = None;
        let count = count_line(found.shown, library.len());
        let place = (pane.filter.place != Place::All).then(|| place_label(&pane.filter.place));
        if header(ui, &count, pane.filter.narrows(), place.as_deref(), couch) {
            action = Some(GamesAction::LibraryFilter);
        }
        for row in folder_rows(library) {
            if row.access == Access::Unreadable && row.tree.starts_with("content://") {
                let words = format!("Lost access to {}", row.name);
                if warning(ui, &words, &["Allow again"]) == Some(0) {
                    action = Some(GamesAction::AddFolder);
                }
            }
        }
        if let Some(undo) = &pane.undo {
            if undo_strip(ui, undo) {
                action = Some(GamesAction::Undo);
            }
        } else if library.missing_count() > 0 {
            let missing = library.missing_count();
            let show = if pane.filter.missing {
                "Show all"
            } else {
                "Show"
            };
            let remove = format!("Remove {missing}");
            match warning(ui, &missing_words(missing), &[show, &remove]) {
                Some(0) => {
                    pane.filter.missing = !pane.filter.missing;
                    pane.page = 0;
                }
                Some(_) => action = Some(GamesAction::RemoveMissing),
                None => {}
            }
        }
        if found.tiles.is_empty() {
            let words = if library.is_empty() {
                "No games yet"
            } else {
                "No games match these filters"
            };
            kit::card(ui, |ui| {
                kit::padded(ui, |ui| ui.label(RichText::new(words).weak()))
            });
        }
        if let Some(picked) = grid(ui, &found.tiles, columns, couch) {
            action = Some(picked);
        }
        if found.pages > 1 {
            ui.add(Pager::new(&mut pane.page, found.pages).touch());
        }
        action
    }

    pub(super) fn library_sheets(&mut self, ctx: &Context) {
        if let Some(view) = self.mobile.library.filter {
            self.library_filter_sheet(ctx, view);
        }
        if let Some(path) = self.mobile.library.game.clone() {
            self.library_game_sheet(ctx, &path);
        }
    }

    pub(super) fn open_library_filter(&mut self) {
        self.mobile.library.filter = Some(FilterView::Choices);
        self.release_touch();
    }

    pub(super) fn open_library_game(&mut self, path: String) {
        self.mobile.library.game = Some(path);
        self.release_touch();
    }

    fn library_filter_sheet(&mut self, ctx: &Context, view: FilterView) {
        let library = &self.ui.library;
        let places = places(library);
        let tags = library.tags();
        let genres = library.genres();
        let search = self.games_search_text();
        let pane = &mut self.ui.games;
        let before = pane.filter.clone();
        let filter = &mut pane.filter;
        let title = match view {
            FilterView::Choices => "Filter",
            FilterView::Tags => "Tags",
        };
        let shown = sheet::show(ctx, Id::new("library_filter"), title, |ui| match view {
            FilterView::Choices => {
                let showing = library.shown(filter, &search).len();
                filter_choices(ui, filter, &places, tags, genres, showing)
            }
            FilterView::Tags => {
                let back = kit::Row::new("Back to filters")
                    .icon(icons::CARET_LEFT)
                    .no_chevron();
                if kit::rows(ui, vec![back]).is_some() {
                    return Some(FilterStep::Open(FilterView::Choices));
                }
                let picked = tag_sections(ui, tags, genres, filter.tag.as_ref())?;
                filter.tag = picked;
                Some(FilterStep::Open(FilterView::Choices))
            }
        });
        if before != pane.filter {
            pane.page = 0;
        }
        match shown.inner {
            Some(FilterStep::Open(next)) => self.mobile.library.filter = Some(next),
            Some(FilterStep::Done) => self.mobile.library.filter = None,
            None if shown.closed => self.mobile.library.filter = None,
            None => {}
        }
    }

    fn library_game_sheet(&mut self, ctx: &Context, path: &str) {
        let Some(game) = self.ui.library.find(path) else {
            self.mobile.library.game = None;
            self.ui.games.tag_draft = None;
            return;
        };
        let cover = game.rom_key.as_deref().and_then(|key| {
            self.big.covers.get(
                ctx,
                &self.emulator.store,
                key,
                self.emulator.rom_key(),
                self.ui.settings.covers,
            )
        });
        let note = tag_of(game)
            .filter(|tag| *tag != Tag::ArchiveOffline)
            .map(|tag| tag.hint(&shown_path(&game.path)));
        let progress = self
            .achievements
            .progress(&self.ui.settings.achievements, game.ra_hash.as_deref());
        let download = download_progress(&self.online, game);
        let card = info::card(game, &self.game_facts, cover, progress, download);
        let library = &self.ui.library;
        let draft = &mut self.ui.games.tag_draft;
        let shown = sheet::show_headed(
            ctx,
            Id::new("library_game").with(&card.path),
            |ui, _| Some(sheet::inset(ui, |ui| title(ui, &card))),
            |ui| match draft {
                Some(typed) => {
                    let (picked, done) = add_tag(ui, &card, library, typed);
                    if done {
                        *draft = None;
                    }
                    picked
                }
                None => game_sheet(ui, &card, note.as_deref(), draft),
            },
        );
        let leaves = matches!(
            shown.inner,
            Some(GamesAction::Play(_) | GamesAction::RemoveGame(_))
        );
        if leaves || shown.closed {
            self.mobile.library.game = None;
            self.ui.games.tag_draft = None;
        }
        self.apply_games(shown.inner);
    }

    pub(super) fn rom_folders_page(&mut self, ui: &mut Ui) {
        let mut action = None;
        kit::place(ui, "rom_folders_page", |ui| {
            let rows = folder_rows(&self.ui.library);
            if !rows.is_empty() {
                ui.add_space(8.0);
                kit::list(ui, |list| {
                    for row in &rows {
                        let count = count_words(row.count);
                        let unreadable = row.access == Access::Unreadable;
                        let allow = unreadable && row.tree.starts_with("content://");
                        let icon = if unreadable {
                            icons::WARNING
                        } else {
                            icons::FOLDER
                        };
                        let value = if allow {
                            "Allow again".to_owned()
                        } else {
                            count
                        };
                        let response = list.row(
                            kit::Row::new(&row.name)
                                .icon(icon)
                                .detail(&row.detail)
                                .value(value)
                                .trailing(icons::TRASH),
                        );
                        if response.clicked() {
                            let on_trash = response
                                .interact_pointer_pos()
                                .is_some_and(|at| at.x > response.rect.right() - TRASH_SIDE);
                            if on_trash {
                                action = Some(GamesAction::RemoveFolder(row.tree.clone()));
                            } else if allow {
                                action = Some(GamesAction::AddFolder);
                            }
                        }
                    }
                });
            }
            if let Some(undo) = &self.ui.games.undo
                && undo.folder.is_some()
                && undo_strip(ui, undo)
            {
                action = Some(GamesAction::Undo);
            }
            ui.add_space(12.0);
            let add = kit::Row::new("Add folder")
                .icon(icons::FOLDER_PLUS)
                .no_chevron();
            if kit::rows(ui, vec![add]).is_some() {
                action = Some(GamesAction::AddFolder);
            }
        });
        self.apply_games(action);
    }
}

fn header(ui: &mut Ui, count: &str, narrowed: bool, place: Option<&str>, couch: bool) -> bool {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let secondary = kit::section_color(ui.visuals());
        ui.label(RichText::new("Library").size(13.0).color(secondary));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut text =
                RichText::new(format!("{} {}", icons::FUNNEL, place.unwrap_or("Filter")));
            if narrowed {
                text = text.color(crate::app::ui_state::ui_theme::brand_mark(ui.visuals()));
            }
            let room = if couch { ring::MARGIN } else { 0.0 };
            ui.add_space(room);
            if couch {
                let tall = ui.spacing().button_padding.y;
                ui.spacing_mut().button_padding = vec2(FILTER_PAD, tall);
                let widgets = &mut ui.visuals_mut().widgets;
                for state in [&mut widgets.hovered, &mut widgets.active] {
                    state.weak_bg_fill = Color32::TRANSPARENT;
                    state.bg_stroke = Stroke::NONE;
                }
            }
            let button = egui::Button::new(text)
                .frame(couch)
                .frame_when_inactive(false);
            let clicked = ui.add(button).on_hover_text("Filter").clicked();
            ui.add_space(room);
            ui.label(RichText::new(count).size(13.0).color(secondary));
            clicked
        })
        .inner
    })
    .inner
}

fn warning(ui: &mut Ui, words: &str, buttons: &[&str]) -> Option<usize> {
    strip_with(ui, Some(icons::WARNING), buttons, |ui| {
        ui.add(egui::Label::new(RichText::new(words).size(15.0)).truncate());
    })
}

fn undo_strip(ui: &mut Ui, undo: &Undo) -> bool {
    strip_with(ui, None, &["Undo"], |ui| {
        crate::app::widgets::text::name_then(ui, &undo.name, &undo.tail, Some(15.0), 0.0);
    }) == Some(0)
}

fn strip_with(
    ui: &mut Ui,
    icon: Option<&str>,
    buttons: &[&str],
    words: impl FnOnce(&mut Ui),
) -> Option<usize> {
    ui.add_space(8.0);
    kit::card(ui, |ui| {
        kit::padded(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(icon) = icon {
                    let color = ui.visuals().warn_fg_color;
                    ui.label(RichText::new(icon).size(18.0).color(color));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut picked = None;
                    for (index, button) in buttons.iter().enumerate().rev() {
                        if kit::pill(ui, button, false).clicked() {
                            picked = Some(index);
                        }
                    }
                    words(ui);
                    picked
                })
                .inner
            })
            .inner
        })
    })
}

fn grid(ui: &mut Ui, tiles: &[LibraryTile], columns: usize, couch: bool) -> Option<GamesAction> {
    ui.add_space(8.0);
    let width = (ui.available_width() - TILE_GAP * (columns - 1) as f32) / columns as f32;
    let mut action = None;
    let names: Vec<&str> = tiles.iter().map(|tile| tile.name.as_str()).collect();
    ui.spacing_mut().item_spacing.y = if couch { COUCH_ROW_GAP } else { TILE_GAP };
    for row in tiles.chunks(columns) {
        let row_names: Vec<&str> = row.iter().map(|tile| tile.name.as_str()).collect();
        let lines = name_lines(ui, &row_names, width, couch);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = TILE_GAP;
            for tile in row {
                let response = ui.add(
                    GameTile::new(&tile.name, width)
                        .neighbours(&names)
                        .name_lines(lines)
                        .cover(tile.cover.as_ref())
                        .console(tile.console)
                        .couch(couch)
                        .tag(tile.tag, &shown_path(&tile.path))
                        .progress(tile.progress)
                        .archive(tile.archive),
                );
                if let Some(press) = Press::of(&response) {
                    action = Some(phone_press(press, tile));
                }
                crate::app::big_screen::offer_details(&response, "Details");
                if crate::app::big_screen::take_details(&response) {
                    action = Some(GamesAction::GameMenu(tile.path.clone()));
                }
            }
        });
    }
    action
}

fn game_sheet(
    ui: &mut Ui,
    card: &GameCard,
    note: Option<&str>,
    draft: &mut Option<String>,
) -> Option<GamesAction> {
    let mut action = None;
    if let Some(note) = note {
        sheet::note(ui, note);
    }
    if let Some(info::ArchiveCard::Downloading { done, total }) = card.archive {
        sheet::inset(ui, |ui| {
            ui.add(crate::app::widgets::progress::ProgressBar::new(
                "",
                crate::progress::Progress::bytes(done, total),
            ))
        });
        ui.add_space(8.0);
        if kit::wide_button(ui, &format!("{} Cancel", icons::X), false).clicked() {
            action = Some(GamesAction::CancelDownload(card.path.clone()));
        }
    } else if card.playable && kit::primary_button(ui, &format!("{} Play", icons::PLAY)) {
        action = Some(GamesAction::Play(card.path.clone()));
    }
    kit::section_label(ui, "Tags");
    chips(ui, |ui| {
        for tag in &card.tags {
            if TagChip::new(tag, Look::Own).large().removable().show(ui) == Some(Tap::Remove) {
                action = Some(GamesAction::RemoveTag(card.path.clone(), tag.clone()));
            }
        }
        if TagChip::new("Add tag", Look::Add)
            .large()
            .show(ui)
            .is_some()
        {
            *draft = Some(String::new());
        }
    });
    if !card.genres.is_empty() {
        kit::section_label(ui, "Genres");
        chips(ui, |ui| {
            for genre in &card.genres {
                TagChip::new(genre, Look::Genre).large().show(ui);
            }
        });
    }
    kit::section_label(ui, "About");
    let about = card.about_rows();
    let facts = about.len();
    let mut rows: Vec<kit::Row<'_>> = about
        .into_iter()
        .map(|(name, value)| {
            kit::Row::new(name)
                .value(value.to_owned())
                .whole_value()
                .no_chevron()
        })
        .collect();
    rows.extend(
        card.links
            .iter()
            .map(|(name, _)| kit::Row::new(name).trailing(icons::ARROW_SQUARE_OUT)),
    );
    if let Some(index) = kit::rows(ui, rows)
        && let Some((_, url)) = index
            .checked_sub(facts)
            .and_then(|link| card.links.get(link))
    {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
    if card.has_facts() {
        ui.add_space(6.0);
        sheet::note(ui, CREDIT);
    } else {
        ui.add_space(16.0);
    }
    match card.archive {
        None => {
            if kit::danger_button(ui, &format!("{} Remove from library", icons::TRASH)).clicked() {
                action = Some(GamesAction::RemoveGame(card.path.clone()));
            }
        }
        Some(info::ArchiveCard::Downloaded { .. }) => {
            if kit::wide_button(ui, &format!("{} Delete download", icons::TRASH), false).clicked() {
                action = Some(GamesAction::DeleteDownload(card.path.clone()));
            }
        }
        Some(_) => {}
    }
    action
}

fn title(ui: &mut Ui, card: &GameCard) -> f32 {
    let mut bottom = f32::NEG_INFINITY;
    ui.horizontal(|ui| {
        if let Some(cover) = &card.cover {
            let (rect, _) =
                ui.allocate_exact_size(egui::Vec2::splat(THUMB_SIDE), egui::Sense::hover());
            bottom = rect.bottom();
            let at = kit::fit_inside(cover.size, rect);
            egui::Image::new((cover.id(), cover.size))
                .uv(cover.uv)
                .corner_radius(6)
                .paint_at(ui, at);
            ui.add_space(8.0);
        }
        ui.vertical(|ui| {
            let mut last = ui
                .add(egui::Label::new(RichText::new(&card.name).size(20.0)).wrap())
                .rect;
            if let Some(console) = &card.console {
                last = ui
                    .label(
                        RichText::new(console)
                            .size(14.0)
                            .color(kit::section_color(ui.visuals())),
                    )
                    .rect;
            }
            if let Some(archive) = card.archive {
                ui.horizontal(|ui| {
                    last = ui
                        .label(
                            RichText::new(archive.line())
                                .size(14.0)
                                .color(kit::section_color(ui.visuals())),
                        )
                        .rect;
                    crate::app::widgets::info_mark::info_mark_below(ui, archive.hint());
                });
            }
            bottom = bottom.max(last.bottom());
        });
    });
    bottom
}

fn chips(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    sheet::inset(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, CHIP_GAP);
        ui.horizontal_wrapped(body);
    });
}

fn add_tag(
    ui: &mut Ui,
    card: &GameCard,
    library: &Library,
    typed: &mut String,
) -> (Option<GamesAction>, bool) {
    kit::section_label(ui, "Add a tag");
    let (response, done) = ui
        .horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let done = kit::pill(ui, "Done", false).clicked();
                let response =
                    kit::field(ui, egui::TextEdit::singleline(typed).hint_text("Tag name"));
                (response, done)
            })
            .inner
        })
        .inner;
    if typed.is_empty() && !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    let entered = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if done || entered {
        let tag = typed.trim().to_owned();
        let picked = (!tag.is_empty()).then(|| GamesAction::AddTag(card.path.clone(), tag));
        return (picked, true);
    }
    let found = tag_rows(library, &card.path, typed);
    let labels: Vec<String> = found.iter().map(tag_row_label).collect();
    let rows = found
        .iter()
        .zip(&labels)
        .map(|(row, label)| {
            let icon = if row.new { icons::PLUS } else { icons::TAG };
            let line = kit::Row::new(label).icon(icon).no_chevron();
            if row.new {
                line
            } else {
                line.value(count_words(row.count))
            }
        })
        .collect();
    ui.add_space(8.0);
    let mut action = None;
    if let Some(index) = kit::rows(ui, rows) {
        action = Some(GamesAction::AddTag(
            card.path.clone(),
            found[index].name.clone(),
        ));
    }
    if !card.tags.is_empty() {
        kit::section_label(ui, "Tags");
        chips(ui, |ui| {
            for tag in &card.tags {
                if TagChip::new(tag, Look::Own).large().removable().show(ui) == Some(Tap::Remove) {
                    action = Some(GamesAction::RemoveTag(card.path.clone(), tag.clone()));
                }
            }
        });
    }
    let done = matches!(action, Some(GamesAction::AddTag(..)));
    (action, done)
}

fn tag_sections(
    ui: &mut Ui,
    tags: &[(String, usize)],
    genres: &[(String, usize)],
    current: Option<&TagPick>,
) -> Option<Option<TagPick>> {
    if tags.is_empty() && genres.is_empty() && current.is_none() {
        return None;
    }
    let mut picked = None;
    kit::section_label(ui, "Your tags");
    let mut rows = vec![kit::Row::new("All tags").checked(current.is_none())];
    rows.extend(tags.iter().map(|(name, count)| {
        let pick = TagPick::Own(name.clone());
        kit::Row::new(name)
            .value(count.to_string())
            .checked(current == Some(&pick))
    }));
    if let Some(index) = kit::rows(ui, rows) {
        picked = Some(
            index
                .checked_sub(1)
                .map(|tag| TagPick::Own(tags[tag].0.clone())),
        );
    }
    if !genres.is_empty() {
        kit::section_label(ui, "Genres");
        let rows = genres
            .iter()
            .map(|(name, count)| {
                let pick = TagPick::Genre(name.clone());
                kit::Row::new(name)
                    .value(count.to_string())
                    .checked(current == Some(&pick))
            })
            .collect();
        if let Some(index) = kit::rows(ui, rows) {
            picked = Some(Some(TagPick::Genre(genres[index].0.clone())));
        }
    }
    picked
}

pub fn phone_press(press: Press, tile: &LibraryTile) -> GamesAction {
    match press {
        Press::Tap if tile.tag.is_none() => GamesAction::Play(tile.path.clone()),
        Press::Tap | Press::Double | Press::Hold => GamesAction::GameMenu(tile.path.clone()),
    }
}

fn filter_choices(
    ui: &mut Ui,
    filter: &mut Filter,
    places: &[Place],
    tags: &[(String, usize)],
    genres: &[(String, usize)],
    showing: usize,
) -> Option<FilterStep> {
    let mut next = None;
    kit::section_label(ui, "Console");
    chips(ui, |ui| {
        if choice(ui, "All", filter.consoles.is_empty()) {
            filter.consoles.clear();
        }
        for console in consoles() {
            if choice(ui, &kind_label(console), filter.consoles.contains(&console)) {
                filter.toggle_console(console);
            }
        }
    });
    if places.len() > 1 {
        kit::section_label(ui, "From");
        chips(ui, |ui| {
            for place in places {
                if choice(ui, &place_choice(place), filter.place == *place) {
                    filter.place = place.clone();
                }
            }
        });
    }
    kit::section_label(ui, "Sort by");
    sheet::inset(ui, |ui| {
        if let Some(sort) = kit::segmented(ui, &SORT_WORDS, filter.sort) {
            filter.sort = sort;
        }
    });
    if !tags.is_empty() || !genres.is_empty() || filter.tag.is_some() {
        kit::section_label(ui, "Tags");
        chips(ui, |ui| {
            for pick in tag_chips(tags, genres, filter.tag.as_ref()) {
                let on = filter.tag.as_ref() == Some(&pick);
                let label = match &pick {
                    TagPick::Own(name) | TagPick::Genre(name) => name.clone(),
                };
                if choice(ui, &label, on) {
                    filter.tag = (!on).then_some(pick);
                }
            }
            let all = format!("All tags {}", icons::CARET_RIGHT);
            if TagChip::new(&all, Look::Choice).large().show(ui).is_some() {
                next = Some(FilterStep::Open(FilterView::Tags));
            }
        });
    }
    ui.add_space(18.0);
    let words = format!("Show {}", count_words(showing));
    if kit::primary_button(ui, &words) {
        next = Some(FilterStep::Done);
    }
    next
}

fn choice(ui: &mut Ui, label: &str, on: bool) -> bool {
    TagChip::new(label, Look::Choice)
        .large()
        .picked(on)
        .show(ui)
        .is_some()
}

fn tag_chips(
    tags: &[(String, usize)],
    genres: &[(String, usize)],
    current: Option<&TagPick>,
) -> Vec<TagPick> {
    let mut picks: Vec<TagPick> = tags
        .iter()
        .map(|(name, _)| TagPick::Own(name.clone()))
        .chain(genres.iter().map(|(name, _)| TagPick::Genre(name.clone())))
        .take(TAG_CHIPS)
        .collect();
    if let Some(current) = current
        && !picks.contains(current)
    {
        picks.insert(0, current.clone());
    }
    picks
}

#[cfg(test)]
mod tests {
    use super::{COLUMNS, grid, phone_press};
    use crate::app::events::GamesAction;
    use crate::app::tabs::games::library::{LibraryTile, Press};
    use crate::app::widgets::game_tile::Tag;
    use egui::{
        Event, Modifiers, PointerButton, RawInput, TouchDeviceId, TouchId, TouchPhase, pos2,
    };

    fn tile(path: &str, tag: Option<Tag>) -> LibraryTile {
        LibraryTile {
            path: path.to_owned(),
            name: path.to_owned(),
            console: None,
            tag,
            cover: None,
            progress: None,
            archive: None,
            from_archive: false,
        }
    }

    fn touch(phase: TouchPhase, time: f64) -> RawInput {
        let at = pos2(100.0, 100.0);
        let mut events = vec![Event::Touch {
            device_id: TouchDeviceId(1),
            id: TouchId(1),
            phase,
            pos: at,
            force: None,
        }];
        let pressed = match phase {
            TouchPhase::Start => Some(true),
            TouchPhase::End => Some(false),
            TouchPhase::Move | TouchPhase::Cancel => None,
        };
        if let Some(pressed) = pressed {
            events.push(Event::PointerMoved(at));
            events.push(Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        if phase == TouchPhase::End {
            events.push(Event::PointerGone);
        }
        RawInput {
            time: Some(time),
            events,
            ..Default::default()
        }
    }

    fn idle(time: f64) -> RawInput {
        RawInput {
            time: Some(time),
            ..Default::default()
        }
    }

    fn presses(tiles: &[LibraryTile], frames: Vec<RawInput>) -> Vec<GamesAction> {
        let ctx = egui::Context::default();
        let mut picked = Vec::new();
        for input in std::iter::once(idle(0.0)).chain(frames) {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    picked.extend(grid(ui, tiles, COLUMNS, false));
                });
            });
        }
        picked
    }

    #[test]
    fn a_tap_on_a_tile_plays_the_game() {
        let tiles = [tile("red.gb", None)];
        let taps = vec![touch(TouchPhase::Start, 0.1), touch(TouchPhase::End, 0.2)];
        assert_eq!(
            presses(&tiles, taps),
            [GamesAction::Play("red.gb".to_owned())]
        );
    }

    #[test]
    fn a_long_press_on_a_tile_opens_its_sheet_and_never_plays() {
        let tiles = [tile("red.gb", None)];
        let hold = vec![
            touch(TouchPhase::Start, 0.1),
            idle(0.5),
            idle(1.0),
            idle(1.2),
            touch(TouchPhase::End, 1.3),
        ];
        assert_eq!(
            presses(&tiles, hold),
            [GamesAction::GameMenu("red.gb".to_owned())]
        );
    }

    #[test]
    fn a_tile_that_cannot_play_opens_its_sheet_on_a_tap() {
        let missing = tile("gone.gb", Some(Tag::Missing));
        assert_eq!(
            phone_press(Press::Tap, &missing),
            GamesAction::GameMenu("gone.gb".to_owned())
        );
        let ready = tile("red.gb", None);
        assert_eq!(
            phone_press(Press::Hold, &ready),
            GamesAction::GameMenu("red.gb".to_owned())
        );
    }

    #[test]
    fn a_tile_shows_its_achievement_count_under_the_name() {
        let mut red = tile("red.gb", None);
        red.progress = Some(crate::app::widgets::game_tile::Progress {
            earned: 12,
            total: 52,
        });
        let tiles = [red, tile("blue.gb", None)];
        let ctx = egui::Context::default();
        let output = ctx.run(idle(0.0), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = grid(ui, &tiles, COLUMNS, false);
            });
        });
        let counts = output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text.galley.text().ends_with(" 12 of 52"),
                _ => false,
            })
            .count();
        assert_eq!(counts, 1);
        let name_top = |name: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == name => Some(text.pos.y),
                    _ => None,
                })
                .unwrap()
        };
        assert_eq!(name_top("red.gb"), name_top("blue.gb"));
    }
}
