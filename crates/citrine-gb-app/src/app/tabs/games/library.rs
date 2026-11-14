pub mod info;
pub mod panel;

use crate::app::big_screen::{Cover, Covers};
use crate::app::events::GamesAction;
use crate::app::ui_state::games::GamesPane;
use crate::app::ui_state::library::{
    Access, FileState, Filter, Library, LibraryGame, Place, Sort, TagPick, scan::folder_label,
    scan::shown_path,
};
use crate::app::ui_state::settings::kind_name;
use crate::app::widgets::game_tile::{GameTile, Progress, Tag};
use crate::app::widgets::pager::Pager;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::icons;
use citrine_gb::console::ConsoleKind;
use egui::{Align, Layout, RichText, Ui};

pub const DESK_ROWS: usize = 5;
const DESK_COLUMNS: usize = 8;
const FILTER_WIDTH: f32 = 150.0;
const FOLDERS_WIDTH: f32 = 340.0;
const FOLDERS_ROOM: f32 = 90.0;
const CONSOLES: [ConsoleKind; 4] = [
    ConsoleKind::Gb,
    ConsoleKind::Gbc,
    ConsoleKind::Gba,
    ConsoleKind::Nds,
];

pub struct LibraryTile {
    pub path: String,
    pub name: String,
    pub console: Option<&'static str>,
    pub tag: Option<Tag>,
    pub cover: Option<Cover>,
    pub progress: Option<Progress>,
}

pub struct FolderRow {
    pub tree: String,
    pub name: String,
    pub detail: String,
    pub count: usize,
    pub access: Access,
}

pub struct CoverSource<'a> {
    pub ctx: &'a egui::Context,
    pub store: &'a crate::storage::SaveStore,
    pub playing: Option<&'a str>,
    pub boxes: bool,
}

pub struct Shelf {
    pub tiles: Vec<LibraryTile>,
    pub shown: usize,
    pub pages: usize,
}

pub fn shelf(
    library: &Library,
    pane: &mut GamesPane,
    search: &str,
    per_page: usize,
    covers: &mut Covers,
    source: &CoverSource<'_>,
    progress: &dyn Fn(Option<&str>) -> Option<Progress>,
) -> Shelf {
    if pane.filter.missing && library.missing_count() == 0 {
        pane.filter.missing = false;
    }
    let shown = library.shown(&pane.filter, search);
    let pages = shown.len().div_ceil(per_page).max(1);
    pane.page = pane.page.min(pages - 1);
    let tiles = shown
        .iter()
        .skip(pane.page * per_page)
        .take(per_page)
        .map(|game| LibraryTile {
            path: game.path.clone(),
            name: game.name.clone(),
            console: game.console.map(kind_name),
            tag: tag_of(game),
            cover: game.rom_key.as_deref().and_then(|key| {
                covers.get(source.ctx, source.store, key, source.playing, source.boxes)
            }),
            progress: progress(game.ra_hash.as_deref()),
        })
        .collect();
    Shelf {
        tiles,
        shown: shown.len(),
        pages,
    }
}

pub fn tag_of(game: &LibraryGame) -> Option<Tag> {
    match game.file {
        FileState::Missing => Some(Tag::Missing),
        FileState::Offline if game.path.starts_with("content://") => Some(Tag::NoAccess),
        FileState::Offline => Some(Tag::NotConnected),
        FileState::Present | FileState::Unchecked => None,
    }
}

pub fn folder_rows(library: &Library) -> Vec<FolderRow> {
    library
        .folders()
        .iter()
        .map(|folder| FolderRow {
            tree: folder.tree.clone(),
            name: folder_label(&folder.tree),
            detail: match folder.access {
                Access::Unreadable if folder.tree.starts_with("content://") => {
                    "Access lost".to_owned()
                }
                Access::Unreadable => "Drive not connected".to_owned(),
                Access::Readable | Access::Unchecked => shown_path(&folder.tree),
            },
            count: library.folder_games(&folder.tree),
            access: folder.access,
        })
        .collect()
}

pub fn desk_page(columns: usize) -> usize {
    if columns == 0 {
        DESK_COLUMNS * DESK_ROWS
    } else {
        columns * DESK_ROWS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Tap,
    Double,
    Hold,
}

impl Press {
    pub fn of(response: &egui::Response) -> Option<Self> {
        if response.double_clicked() {
            Some(Self::Double)
        } else if response.long_touched() || response.secondary_clicked() {
            Some(Self::Hold)
        } else if response.clicked() {
            Some(Self::Tap)
        } else {
            None
        }
    }
}

pub fn desk_press(press: Press, tile: &LibraryTile) -> Option<GamesAction> {
    match press {
        Press::Double if tile.tag.is_none() => Some(GamesAction::Play(tile.path.clone())),
        Press::Tap | Press::Double => Some(GamesAction::GameInfo(tile.path.clone())),
        Press::Hold => None,
    }
}

pub fn count_line(shown: usize, total: usize) -> String {
    if shown < total {
        format!("{shown} of {}", count_words(total))
    } else {
        count_words(total)
    }
}

pub fn count_words(count: usize) -> String {
    match count {
        1 => "1 game".to_owned(),
        count => format!("{count} games"),
    }
}

pub fn console_label(consoles: &[ConsoleKind]) -> String {
    match consoles {
        [] => "All consoles".to_owned(),
        [console] => kind_name(*console).to_owned(),
        many => format!("{} consoles", many.len()),
    }
}

pub fn place_label(place: &Place) -> String {
    match place {
        Place::All => "All folders".to_owned(),
        Place::Folder(tree) => folder_label(tree),
        Place::Opened => "Opened files".to_owned(),
    }
}

pub fn sort_label(sort: Sort) -> &'static str {
    match sort {
        Sort::Name => "Sort by name",
        Sort::Played => "Sort by last played",
        Sort::Added => "Sort by date added",
    }
}

pub const SORT_WORDS: [(Sort, &str); 3] = [
    (Sort::Name, "Name"),
    (Sort::Played, "Last played"),
    (Sort::Added, "Added"),
];

pub fn sort_word(sort: Sort) -> &'static str {
    SORT_WORDS
        .iter()
        .find(|(pick, _)| *pick == sort)
        .map_or("Name", |(_, word)| *word)
}

pub fn consoles() -> [ConsoleKind; 4] {
    CONSOLES
}

pub fn places(library: &Library) -> Vec<Place> {
    let mut places = vec![Place::All];
    places.extend(
        library
            .folders()
            .iter()
            .map(|folder| Place::Folder(folder.tree.clone())),
    );
    places.push(Place::Opened);
    places
}

pub fn missing_words(count: usize) -> String {
    match count {
        1 => "1 game missing".to_owned(),
        count => format!("{count} games missing"),
    }
}

pub struct Section<'a> {
    pub library: &'a Library,
    pub pane: &'a mut GamesPane,
    pub shelf: &'a Shelf,
}

pub fn section(
    page: &mut Page,
    view: Section<'_>,
    search: &str,
    shown: &mut bool,
) -> Option<GamesAction> {
    let total = count_line(view.shelf.shown, view.library.len());
    let folders = cfg!(not(target_arch = "wasm32"));
    if view.library.is_empty() && !folders {
        return None;
    }
    if view.shelf.shown == 0 && !search.is_empty() {
        return None;
    }
    *shown = true;
    view.pane.library_shown = true;
    let selected = view.pane.info.clone();
    page.section(Heading::new(icons::BOOKS, "Library").aside(&total), |ui| {
        let mut action = controls(ui, view.library, view.pane, folders);
        action = action.or(strip(ui, view.library, view.pane));
        if view.shelf.tiles.is_empty() {
            let words = if view.library.is_empty() {
                "No games yet"
            } else {
                "No games match these filters"
            };
            ui.label(RichText::new(words).weak());
        }
        let (picked, columns) = grid(ui, &view.shelf.tiles, selected.as_deref());
        view.pane.grid_columns = columns;
        action = action.or(picked);
        if view.shelf.pages > 1 {
            ui.add(Pager::new(&mut view.pane.page, view.shelf.pages));
        }
        action
    })
}

fn controls(
    ui: &mut Ui,
    library: &Library,
    pane: &mut GamesPane,
    folders: bool,
) -> Option<GamesAction> {
    let before = pane.filter.clone();
    let filter = &mut pane.filter;
    let search = &mut pane.tag_search;
    let mut action = None;
    let gap = ui.spacing().item_spacing.x;
    let combos = if folders { 4.0 } else { 3.0 };
    let fits = ui.available_width() >= combos * (FILTER_WIDTH + gap) + FOLDERS_ROOM;
    ui.horizontal(|ui| {
        if fits {
            console_filter(ui, filter, FILTER_WIDTH);
            if folders {
                place_filter(ui, library, &mut filter.place, FILTER_WIDTH);
            }
            sort_filter(ui, &mut filter.sort, FILTER_WIDTH);
            tag_filter(ui, library, &mut filter.tag, search, FILTER_WIDTH);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if folders {
                action = folders_button(ui, library);
            }
            if !fits {
                filter_button(ui, library, filter, search, folders);
            }
        });
    });
    if before != pane.filter {
        pane.page = 0;
    }
    action
}

fn filter_button(
    ui: &mut Ui,
    library: &Library,
    filter: &mut Filter,
    search: &mut String,
    folders: bool,
) {
    let mut text = RichText::new(format!("{} Filter", icons::FUNNEL));
    if filter.narrows() {
        text = text.color(crate::app::ui_state::ui_theme::brand_mark(ui.visuals()));
    }
    let button = ui.button(text);
    egui::Popup::menu(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(FILTER_WIDTH + 60.0);
            ui.menu_button(
                menu_label(ui, "Console", &console_label(&filter.consoles)),
                |ui| console_ticks(ui, filter),
            );
            if folders {
                ui.menu_button(
                    menu_label(ui, "Folder", &place_label(&filter.place)),
                    |ui| {
                        for place in places(library) {
                            let label = place_label(&place);
                            ui.selectable_value(&mut filter.place, place, label);
                        }
                    },
                );
            }
            ui.menu_button(menu_label(ui, "Sort", sort_word(filter.sort)), |ui| {
                for (pick, word) in SORT_WORDS {
                    ui.selectable_value(&mut filter.sort, pick, word);
                }
            });
            let tags = library.tags();
            let genres = library.genres();
            if !tags.is_empty() || !genres.is_empty() || filter.tag.is_some() {
                let shown = ui.menu_button(
                    menu_label(ui, "Tags", tag_label(filter.tag.as_ref())),
                    |ui| tag_choices(ui, &tags, &genres, &mut filter.tag, search),
                );
                if shown.inner.is_none() {
                    search.clear();
                }
            }
        });
}

fn menu_label(ui: &Ui, name: &str, value: &str) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{name}  "),
        0.0,
        egui::TextFormat::simple(font.clone(), egui::Color32::PLACEHOLDER),
    );
    job.append(
        value,
        0.0,
        egui::TextFormat::simple(font, ui.visuals().weak_text_color()),
    );
    job
}

fn console_filter(ui: &mut Ui, filter: &mut Filter, width: f32) {
    egui::ComboBox::from_id_salt("library_console")
        .selected_text(console_label(&filter.consoles))
        .truncate()
        .width(width)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| console_ticks(ui, filter));
}

fn console_ticks(ui: &mut Ui, filter: &mut Filter) {
    if ui
        .selectable_label(filter.consoles.is_empty(), console_label(&[]))
        .clicked()
    {
        filter.consoles.clear();
    }
    for kind in consoles() {
        let mut on = filter.consoles.contains(&kind);
        if ui.checkbox(&mut on, kind_name(kind)).changed() {
            filter.toggle_console(kind);
        }
    }
}

fn place_filter(ui: &mut Ui, library: &Library, picked: &mut Place, width: f32) {
    egui::ComboBox::from_id_salt("library_folder")
        .selected_text(place_label(picked))
        .truncate()
        .width(width)
        .show_ui(ui, |ui| {
            for place in places(library) {
                let label = place_label(&place);
                ui.selectable_value(picked, place, label);
            }
        });
}

fn sort_filter(ui: &mut Ui, sort: &mut Sort, width: f32) {
    egui::ComboBox::from_id_salt("library_sort")
        .selected_text(sort_label(*sort))
        .truncate()
        .width(width)
        .show_ui(ui, |ui| {
            for pick in [Sort::Name, Sort::Played, Sort::Added] {
                ui.selectable_value(sort, pick, sort_label(pick));
            }
        });
}

pub fn tag_label(tag: Option<&TagPick>) -> &str {
    match tag {
        None => "All tags",
        Some(TagPick::Own(name) | TagPick::Genre(name)) => name,
    }
}

fn tag_filter(
    ui: &mut Ui,
    library: &Library,
    tag: &mut Option<TagPick>,
    search: &mut String,
    width: f32,
) {
    let tags = library.tags();
    let genres = library.genres();
    if tags.is_empty() && genres.is_empty() && tag.is_none() {
        return;
    }
    let shown = egui::ComboBox::from_id_salt("library_tag")
        .selected_text(tag_label(tag.as_ref()))
        .truncate()
        .width(width)
        .show_ui(ui, |ui| tag_choices(ui, &tags, &genres, tag, search));
    if shown.inner.is_none() {
        search.clear();
    }
}

fn tag_choices(
    ui: &mut Ui,
    tags: &[(String, usize)],
    genres: &[(String, usize)],
    tag: &mut Option<TagPick>,
    search: &mut String,
) {
    ui.add(
        egui::TextEdit::singleline(search)
            .hint_text(format!("{} Find a tag", icons::MAGNIFYING_GLASS)),
    );
    ui.selectable_value(tag, None, "All tags");
    let find = search.trim().to_lowercase();
    tag_group(ui, "Your tags", tags, &find, tag, TagPick::Own);
    tag_group(ui, "Genres", genres, &find, tag, TagPick::Genre);
}

fn tag_group(
    ui: &mut Ui,
    heading: &str,
    names: &[(String, usize)],
    find: &str,
    tag: &mut Option<TagPick>,
    pick: fn(String) -> TagPick,
) {
    let found: Vec<&(String, usize)> = names
        .iter()
        .filter(|(name, _)| name.to_lowercase().contains(find))
        .collect();
    if found.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(RichText::new(heading).weak().small());
    for (name, count) in found {
        let picked = Some(pick(name.clone()));
        let button = egui::Button::selectable(*tag == picked, format!("{} {name}", icons::TAG))
            .shortcut_text(count.to_string());
        if ui.add(button).clicked() {
            *tag = picked;
        }
    }
}

fn folders_button(ui: &mut Ui, library: &Library) -> Option<GamesAction> {
    let mut label = format!("{} Folders", icons::FOLDERS);
    if library.needs_access() {
        label = format!("{} Folders", icons::WARNING);
    }
    let button = ui.button(label);
    let mut action = None;
    egui::Popup::menu(&button)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(FOLDERS_WIDTH);
            for row in folder_rows(library) {
                if let Some(picked) = folder_row(ui, &row) {
                    action = Some(picked);
                }
            }
            if !library.folders().is_empty() {
                ui.separator();
            }
            if ui
                .button(format!("{} Add folder", icons::FOLDER_PLUS))
                .clicked()
            {
                action = Some(GamesAction::AddFolder);
            }
        });
    action
}

fn folder_row(ui: &mut Ui, row: &FolderRow) -> Option<GamesAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let icon = if row.access == Access::Unreadable {
            RichText::new(icons::WARNING).color(ui.visuals().warn_fg_color)
        } else {
            RichText::new(icons::FOLDER)
        };
        ui.label(icon.size(18.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button(icons::TRASH)
                .on_hover_text("Remove folder from library")
                .clicked()
            {
                action = Some(GamesAction::RemoveFolder(row.tree.clone()));
            }
            ui.label(RichText::new(count_words(row.count)).weak());
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.add(egui::Label::new(&row.name).truncate());
                ui.add(egui::Label::new(RichText::new(&row.detail).weak().small()).truncate())
                    .on_hover_text(&row.tree);
            });
        });
    });
    action
}

fn strip(ui: &mut Ui, library: &Library, pane: &mut GamesPane) -> Option<GamesAction> {
    if let Some(undo) = &pane.undo {
        return ui
            .horizontal(|ui| {
                ui.label(&undo.words);
                ui.button("Undo").clicked().then_some(GamesAction::Undo)
            })
            .inner;
    }
    let missing = library.missing_count();
    if missing == 0 {
        return None;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::WARNING).color(ui.visuals().warn_fg_color));
        ui.label(missing_words(missing));
        let show = if pane.filter.missing {
            "Show all"
        } else {
            "Show"
        };
        if ui.button(show).clicked() {
            pane.filter.missing = !pane.filter.missing;
            pane.page = 0;
        }
        ui.button(format!("Remove {missing} missing"))
            .clicked()
            .then_some(GamesAction::RemoveMissing)
    })
    .inner
}

fn grid(
    ui: &mut Ui,
    tiles: &[LibraryTile],
    selected: Option<&str>,
) -> (Option<GamesAction>, usize) {
    let (columns, width) = super::tile_layout(ui.available_width());
    let mut action = None;
    ui.spacing_mut().item_spacing.y = super::ROW_GAP;
    for row in tiles.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = super::TILE_GAP;
            for tile in row {
                let response = ui.add(
                    GameTile::new(&tile.name, width)
                        .cover(tile.cover.as_ref())
                        .console(tile.console)
                        .tag(tile.tag, &shown_path(&tile.path))
                        .progress(tile.progress)
                        .selected(selected == Some(tile.path.as_str())),
                );
                if let Some(press) = Press::of(&response)
                    && let Some(picked) = desk_press(press, tile)
                {
                    action = Some(picked);
                }
                response.context_menu(|ui| {
                    if ui
                        .add_enabled(
                            tile.tag.is_none(),
                            egui::Button::new(format!("{} Play", icons::PLAY)),
                        )
                        .clicked()
                    {
                        action = Some(GamesAction::Play(tile.path.clone()));
                        ui.close();
                    }
                    if ui.button(format!("{} Game info", icons::INFO)).clicked() {
                        action = Some(GamesAction::GameInfo(tile.path.clone()));
                        ui.close();
                    }
                    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
                    if ui
                        .add_enabled(
                            tile.tag.is_none(),
                            egui::Button::new(format!(
                                "{} {}",
                                icons::FOLDER_OPEN,
                                crate::media::reveal::LABEL
                            )),
                        )
                        .clicked()
                    {
                        action = Some(GamesAction::ShowInFolder(tile.path.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(format!("{} Remove from library", icons::TRASH))
                        .clicked()
                    {
                        action = Some(GamesAction::RemoveGame(tile.path.clone()));
                        ui.close();
                    }
                });
            }
        });
    }
    (action, columns)
}
