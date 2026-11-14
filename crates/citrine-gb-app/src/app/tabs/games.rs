use crate::app::big_screen::Cover;
use crate::app::events::GamesAction;
use crate::app::mobile::games::{RecentGame, recent_games};
use crate::app::tabs::TabViewer;
use crate::app::widgets::field::row_field;
use crate::app::widgets::game_tile::{GameTile, Tag};
use crate::app::widgets::settings::page::{Heading, PAGE_EDGE, Page};
use crate::icons;
use egui::{Align, Layout, RichText, ScrollArea, Ui};

pub mod library;

const MAX_COLUMN: f32 = 880.0;
const BAR_GAP: f32 = 8.0;
const TILE_MIN: f32 = 96.0;
const TILE_GAP: f32 = 10.0;
const ROW_GAP: f32 = 12.0;
const GONE_CHECK: f64 = 2.0;

pub fn idle(emulator: &crate::emulator::Emulator, play: &crate::play::Play) -> bool {
    !emulator.has_game() && !play.session().is_some_and(|session| session.runs_engine())
}

pub fn fills(viewer: &TabViewer) -> bool {
    idle(viewer.emulator, viewer.play)
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let mut action = None;
    ui.add_space(BAR_GAP);
    column(ui, |ui| {
        if bar(ui, &mut viewer.ui.games.search) {
            action = Some(GamesAction::Open);
        }
    });
    ui.add_space(BAR_GAP);
    ui.separator();
    check_gone(viewer, ui.input(|input| input.time));
    let search = viewer.ui.games.search.trim().to_lowercase();
    let recent: Vec<RecentGame> = recent_list(viewer)
        .into_iter()
        .filter(|game| game.name.to_lowercase().contains(&search))
        .collect();
    let covers = recent_covers(viewer, ui.ctx(), &recent);
    let ctx = ui.ctx().clone();
    let source = library::CoverSource {
        ctx: &ctx,
        store: &viewer.emulator.store,
        playing: viewer.emulator.rom_key(),
        boxes: viewer.ui.settings.covers,
    };
    let card = viewer.ui.games.info.as_deref().and_then(|path| {
        let game = viewer.ui.library.find(path)?;
        let cover = game.rom_key.as_deref().and_then(|key| {
            viewer
                .covers
                .get(source.ctx, source.store, key, source.playing, source.boxes)
        });
        let progress = viewer
            .achievements
            .progress(&viewer.ui.settings.achievements, game.ra_hash.as_deref());
        Some(library::info::card(
            game,
            viewer.game_facts,
            cover,
            progress,
        ))
    });
    if card.is_none() && viewer.ui.games.info.is_some() {
        viewer.ui.games.close_info();
    }
    if let Some(card) = &card {
        egui::SidePanel::right("library_info")
            .resizable(false)
            .exact_width(library::panel::PANEL_WIDTH)
            .show_inside(ui, |ui| {
                let draft = &mut viewer.ui.games.tag_draft;
                if let Some(picked) = library::panel::panel(ui, card, &viewer.ui.library, draft) {
                    action = Some(picked);
                }
            });
    }
    let achievements = &*viewer.achievements;
    let ra = &viewer.ui.settings.achievements;
    let progress = |hash: Option<&str>| achievements.progress(ra, hash);
    let pane = &mut viewer.ui.games;
    let shelf = library::shelf(
        &viewer.ui.library,
        pane,
        &search,
        library::desk_page(pane.grid_columns),
        viewer.covers,
        &source,
        &progress,
    );
    let shelf_view = library::Section {
        library: &viewer.ui.library,
        pane,
        shelf: &shelf,
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        column(ui, |ui| {
            ui.add_space(PAGE_EDGE);
            let picked = Page::show(ui, |page| {
                sections(page, &search, &recent, &covers, shelf_view)
            });
            if let Some(picked) = picked {
                action = Some(picked);
            }
        })
    });
    if let Some(action) = action {
        viewer.events.games(action);
    }
}

fn column(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    let room = ui.available_width();
    let width = (room - 2.0 * PAGE_EDGE).clamp(0.0, MAX_COLUMN);
    let left = ((room - width) / 2.0).max(0.0);
    let spacing = ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(left);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            ui.set_width(width);
            body(ui);
        });
    });
}

fn bar(ui: &mut Ui, search: &mut String) -> bool {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let open = ui
            .button(format!("{} Open", icons::FOLDER_OPEN))
            .on_hover_text("Open a ROM file")
            .clicked();
        ui.add(
            row_field(ui, search)
                .hint_text(format!("{} Search games", icons::MAGNIFYING_GLASS))
                .desired_width(ui.available_width()),
        );
        open
    })
    .inner
}

fn sections(
    page: &mut Page,
    search: &str,
    recent: &[RecentGame],
    covers: &[Option<Cover>],
    shelf: library::Section<'_>,
) -> Option<GamesAction> {
    let mut action = None;
    let mut shown = false;
    if !recent.is_empty() {
        shown = true;
        action = page.section(
            Heading::new(icons::CLOCK_COUNTER_CLOCKWISE, "Recent"),
            |ui| recent_tiles(ui, recent, covers),
        );
    }
    if let Some(picked) = library::section(page, shelf, search, &mut shown) {
        action = action.or(Some(picked));
    }
    if !shown {
        let empty = if search.is_empty() {
            "No games yet"
        } else {
            "No games found"
        };
        page.plain(|ui| ui.label(RichText::new(empty).weak()));
    }
    action
}

pub(super) fn tile_layout(width: f32) -> (usize, f32) {
    let columns = ((width + TILE_GAP) / (TILE_MIN + TILE_GAP))
        .floor()
        .max(1.0) as usize;
    let tile_width = (width - TILE_GAP * (columns - 1) as f32) / columns as f32;
    (columns, tile_width)
}

fn check_gone(viewer: &mut TabViewer, now: f64) {
    let ui = &mut *viewer.ui;
    if ui.games.checked.is_some_and(|at| now - at < GONE_CHECK) {
        return;
    }
    ui.games.checked = Some(now);
    let copies = &viewer.files.rom_copies;
    ui.games.gone = ui
        .recent
        .entries()
        .iter()
        .map(|entry| &entry.path)
        .filter(|path| copies.is_gone(path))
        .cloned()
        .collect();
}

fn recent_list(viewer: &TabViewer) -> Vec<RecentGame> {
    #[cfg(not(target_arch = "wasm32"))]
    let playing = viewer.emulator.rom_path();
    #[cfg(target_arch = "wasm32")]
    let playing = None;
    let gone = &viewer.ui.games.gone;
    recent_games(
        viewer.ui.recent.entries(),
        (playing, viewer.emulator.rom_key()),
        crate::stats::today(),
        |path| gone.iter().any(|known| known == path),
    )
}

fn recent_covers(
    viewer: &mut TabViewer,
    ctx: &egui::Context,
    recent: &[RecentGame],
) -> Vec<Option<Cover>> {
    let playing = viewer.emulator.rom_key().map(str::to_owned);
    let boxes = viewer.ui.settings.covers;
    recent
        .iter()
        .map(|game| {
            let key = viewer.ui.recent.game_key(&game.path)?;
            viewer
                .covers
                .get(ctx, &viewer.emulator.store, key, playing.as_deref(), boxes)
        })
        .collect()
}

fn recent_tiles(
    ui: &mut Ui,
    recent: &[RecentGame],
    covers: &[Option<Cover>],
) -> Option<GamesAction> {
    let (columns, tile_width) = tile_layout(ui.available_width());
    let mut picked = None;
    ui.spacing_mut().item_spacing.y = ROW_GAP;
    for (row, games) in recent.chunks(columns).enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = TILE_GAP;
            for (column, game) in games.iter().enumerate() {
                let index = row * columns + column;
                let path = game.path.to_string_lossy();
                let response = ui.add(
                    GameTile::new(&game.name, tile_width)
                        .cover(covers[index].as_ref())
                        .console(game.console)
                        .tag(game.gone.then_some(Tag::Missing), &path),
                );
                if response.clicked() {
                    picked = Some(GamesAction::Recent(game.path.clone()));
                }
                response.context_menu(|ui| {
                    if ui
                        .button(format!("{} Remove from recent", icons::TRASH))
                        .clicked()
                    {
                        picked = Some(GamesAction::Remove(game.path.clone()));
                        ui.close();
                    }
                });
            }
        });
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::library;

    #[test]
    fn the_desktop_games_pane_never_draws_phone_widgets() {
        for source in [include_str!("games.rs"), include_str!("games/library.rs")] {
            assert!(!source.contains(concat!("games", "_place")));
        }
    }

    fn tile(path: &str, tag: Option<crate::app::widgets::game_tile::Tag>) -> library::LibraryTile {
        library::LibraryTile {
            path: path.to_owned(),
            name: path.to_owned(),
            console: None,
            tag,
            cover: None,
            progress: None,
        }
    }

    #[test]
    fn the_count_says_how_many_of_the_library_a_filter_shows() {
        assert_eq!(library::count_line(3, 34), "3 of 34 games");
        assert_eq!(library::count_line(0, 1), "0 of 1 game");
        assert_eq!(library::count_line(34, 34), "34 games");
        assert_eq!(library::count_line(1, 1), "1 game");
    }

    #[test]
    fn a_click_opens_the_info_panel_and_a_double_click_plays() {
        use crate::app::events::GamesAction;
        use library::Press;
        let ready = tile("red.gb", None);
        assert_eq!(
            library::desk_press(Press::Tap, &ready),
            Some(GamesAction::GameInfo("red.gb".to_owned()))
        );
        assert_eq!(
            library::desk_press(Press::Double, &ready),
            Some(GamesAction::Play("red.gb".to_owned()))
        );
        assert_eq!(library::desk_press(Press::Hold, &ready), None);
        let missing = tile(
            "gone.gb",
            Some(crate::app::widgets::game_tile::Tag::Missing),
        );
        assert_eq!(
            library::desk_press(Press::Double, &missing),
            Some(GamesAction::GameInfo("gone.gb".to_owned()))
        );
    }

    #[test]
    fn a_desktop_page_fills_five_rows_of_the_grid() {
        assert_eq!(library::desk_page(8), 40);
        assert_eq!(library::desk_page(5), 25);
        assert_eq!(library::desk_page(0), 40);
    }

    #[test]
    fn the_console_filter_names_its_picks() {
        use citrine_gb::console::ConsoleKind;
        assert_eq!(library::console_label(&[]), "All consoles");
        assert_eq!(
            library::console_label(&[ConsoleKind::Gba]),
            "Game Boy Advance"
        );
        assert_eq!(
            library::console_label(&[ConsoleKind::Gb, ConsoleKind::Gbc]),
            "2 consoles"
        );
    }

    #[test]
    fn the_sort_value_says_it_once() {
        use crate::app::ui_state::library::Sort;
        assert_eq!(library::sort_word(Sort::Name), "Name");
        assert_eq!(library::sort_word(Sort::Played), "Last played");
        assert_eq!(library::sort_word(Sort::Added), "Added");
    }
}
