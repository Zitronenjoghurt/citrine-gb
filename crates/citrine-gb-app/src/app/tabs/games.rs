use crate::app::big_screen::Cover;
use crate::app::events::GamesAction;
use crate::app::mobile::games::{RecentGame, recent_games};
use crate::app::tabs::TabViewer;
use crate::app::widgets::field::row_field;
use crate::app::widgets::game_tile::{Archive, GameTile, Tag, name_lines};
use crate::app::widgets::settings::page::{Heading, PAGE_EDGE, Page};
use crate::icons;
use egui::{Align, Layout, RichText, ScrollArea, Ui};

pub mod library;

const MAX_COLUMN: f32 = 880.0;
const BAR_GAP: f32 = 8.0;
const TILE_MIN: f32 = 100.0;
const TILE_MAX: f32 = 124.0;
const TILE_GAP: f32 = 10.0;
const GAP_MAX: f32 = 20.0;
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
    let span = span(ui);
    column(ui, span, |ui| {
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
    let marks = library::recent_marks(&viewer.ui.library, viewer.online, &recent);
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
        let download = crate::app::archive::download_progress(viewer.online, game);
        Some(library::info::card(
            game,
            viewer.game_facts,
            cover,
            progress,
            download,
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
    let online = &*viewer.online;
    let downloads = |game: &crate::app::ui_state::library::LibraryGame| {
        crate::app::archive::download_progress(online, game)
    };
    let pane = &mut viewer.ui.games;
    let shelf = library::shelf(
        &viewer.ui.library,
        pane,
        &search,
        library::desk_page(pane.grid_columns),
        viewer.covers,
        &source,
        &library::TileFacts {
            progress: &progress,
            downloads: &downloads,
        },
    );
    let shelf_view = library::Section {
        library: &viewer.ui.library,
        pane,
        shelf: &shelf,
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        column(ui, span, |ui| {
            ui.add_space(PAGE_EDGE);
            let recent = RecentShelf {
                games: &recent,
                covers: &covers,
                marks: &marks,
            };
            let picked = Page::show(ui, |page| sections(page, &search, recent, shelf_view));
            if let Some(picked) = picked {
                action = Some(picked);
            }
        })
    });
    if let Some(action) = action {
        viewer.events.games(action);
    }
}

#[derive(Clone, Copy)]
struct Span {
    left: f32,
    width: f32,
}

fn span(ui: &Ui) -> Span {
    let room = ui.available_rect_before_wrap();
    let width = (room.width() - 2.0 * PAGE_EDGE).clamp(0.0, MAX_COLUMN);
    Span {
        left: room.left() + ((room.width() - width) / 2.0).max(0.0),
        width,
    }
}

fn column(ui: &mut Ui, span: Span, body: impl FnOnce(&mut Ui)) {
    let room = ui.available_rect_before_wrap();
    let left = (span.left - room.left()).max(0.0);
    let width = span.width.min(room.width() - left - PAGE_EDGE).max(0.0);
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
            .on_hover_text(crate::app::mobile::OPEN_ROM)
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

#[derive(Clone, Copy)]
struct RecentShelf<'a> {
    games: &'a [RecentGame],
    covers: &'a [Option<Cover>],
    marks: &'a [(Option<Tag>, Option<Archive>)],
}

fn sections(
    page: &mut Page,
    search: &str,
    recent: RecentShelf<'_>,
    shelf: library::Section<'_>,
) -> Option<GamesAction> {
    let mut action = None;
    let mut shown = false;
    if !recent.games.is_empty() {
        shown = true;
        action = page.section(
            Heading::new(icons::CLOCK_COUNTER_CLOCKWISE, "Recent"),
            |ui| recent_tiles(ui, recent),
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

pub(super) struct TileGrid {
    pub columns: usize,
    pub width: f32,
    pub gap: f32,
}

pub(super) fn tile_layout(width: f32) -> TileGrid {
    let columns = ((width + TILE_GAP) / (TILE_MIN + TILE_GAP))
        .floor()
        .max(1.0) as usize;
    let gaps = (columns - 1) as f32;
    let even = (width - TILE_GAP * gaps) / columns as f32;
    let gap = if even <= TILE_MAX || columns == 1 {
        TILE_GAP
    } else {
        ((width - TILE_MAX * columns as f32) / gaps).min(GAP_MAX)
    };
    TileGrid {
        columns,
        width: (width - gap * gaps) / columns as f32,
        gap,
    }
}

fn check_gone(viewer: &mut TabViewer, now: f64) {
    let ui = &mut *viewer.ui;
    if ui.games.checked.is_some_and(|at| now - at < GONE_CHECK) {
        return;
    }
    ui.games.checked = Some(now);
    let copies = &viewer.files.rom_copies;
    let library = &ui.library;
    ui.games.gone = ui
        .recent
        .entries()
        .iter()
        .map(|entry| &entry.path)
        .filter(|path| library.recent_gone(copies, path))
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
        |path| viewer.ui.library.find(path.to_str()?)?.console,
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

fn recent_tiles(ui: &mut Ui, recent: RecentShelf<'_>) -> Option<GamesAction> {
    let grid = tile_layout(ui.available_width());
    let (columns, tile_width) = (grid.columns, grid.width);
    let mut picked = None;
    let names: Vec<&str> = recent.games.iter().map(|game| game.name.as_str()).collect();
    ui.spacing_mut().item_spacing.y = ROW_GAP;
    for (row, games) in recent.games.chunks(columns).enumerate() {
        let row_names: Vec<&str> = games.iter().map(|game| game.name.as_str()).collect();
        let lines = name_lines(ui, &row_names, tile_width, false);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = grid.gap;
            for (column, game) in games.iter().enumerate() {
                let index = row * columns + column;
                let path = game.path.to_string_lossy();
                let (tag, archive) = recent.marks[index];
                let response = ui.add(
                    GameTile::new(&game.name, tile_width)
                        .neighbours(&names)
                        .name_lines(lines)
                        .cover(recent.covers[index].as_ref())
                        .console(game.console)
                        .tag(tag, &path)
                        .archive(archive),
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
    fn the_archive_line_says_in_words_whether_the_game_is_here() {
        use library::info::ArchiveCard;
        let downloaded = ArchiveCard::Downloaded { bytes: 12 << 20 }.line();
        assert!(downloaded.ends_with("Downloaded, 12\u{a0}MB"));
        let remote = ArchiveCard::Remote { bytes: 3 << 20 }.line();
        assert!(remote.ends_with("In the Game Archive, 3.0\u{a0}MB download"));
        let downloading = ArchiveCard::Downloading {
            done: 1 << 20,
            total: 4 << 20,
        }
        .line();
        assert!(downloading.ends_with(" Downloading"));
        assert!(!downloading.contains("MB"));
        assert!(
            ArchiveCard::Offline
                .line()
                .ends_with("In the Game Archive, offline")
        );
        assert!(!ArchiveCard::Offline.hint().contains("Game Archive"));
    }

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
            archive: None,
            from_archive: false,
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

    fn laid_library(width: f32) -> (egui::Rect, f32, Vec<egui::Rect>, Vec<egui::Rect>) {
        use crate::app::widgets::settings::page::{Mark, Page, tests::after_pass, tests::quiet};
        let ctx = egui::Context::default();
        quiet(&ctx);
        let library = crate::app::ui_state::library::Library::default();
        let mut pane = crate::app::ui_state::games::GamesPane::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 1600.0),
            )),
            ..Default::default()
        };
        let mut laid = Default::default();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let shelf = library::Shelf {
                tiles: (0..12)
                    .map(|n| tile(&format!("game {n}.gb"), None))
                    .collect(),
                shown: 12,
                pages: 1,
            };
            shapes = ctx
                .run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        Page::show(ui, |page| {
                            let view = library::Section {
                                library: &library,
                                pane: &mut pane,
                                shelf: &shelf,
                            };
                            library::section(page, view, "", &mut false)
                        })
                    });
                })
                .shapes;
            laid = after_pass(&ctx);
        }
        let (marks, controls) = laid;
        let heading = marks
            .iter()
            .find(|(kind, _)| *kind == Mark::Heading)
            .map(|(_, rect)| *rect)
            .expect("the library draws its heading");
        let title = shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "Library" => {
                    Some(text.pos.x + text.galley.rect.left())
                }
                _ => None,
            })
            .expect("the library draws its title");
        let (tiles, buttons): (Vec<_>, Vec<_>) = controls
            .iter()
            .copied()
            .filter(|rect| rect.top() >= heading.bottom() - 0.5)
            .partition(|rect| rect.height() >= super::TILE_MIN);
        assert!(
            !tiles.is_empty() && !buttons.is_empty(),
            "at {width}: under the heading {heading:?} no tiles or no filter row in {controls:?}"
        );
        (heading, title, tiles, buttons)
    }

    #[test]
    fn the_tiles_sit_further_from_the_filter_row_than_the_heading_does() {
        for width in [320.0, 480.0, 720.0, 1000.0] {
            let (heading, _, tiles, buttons) = laid_library(width);
            let row_top = buttons.iter().map(|b| b.top()).fold(f32::MAX, f32::min);
            let row_bottom = buttons.iter().map(|b| b.bottom()).fold(f32::MIN, f32::max);
            let first = tiles.iter().map(|t| t.top()).fold(f32::MAX, f32::min);
            let above = row_top - heading.bottom();
            let below = first - row_bottom;
            assert!(
                below >= above && below >= super::ROW_GAP - 0.5,
                "at {width}: {below} under the filter row, {above} above it"
            );
        }
    }

    #[test]
    fn the_tile_columns_fill_the_pane_without_running_past_the_filter_row() {
        for width in [320.0, 400.0, 480.0, 560.0, 720.0, 1000.0] {
            let (_, title, tiles, buttons) = laid_library(width);
            let right = tiles.iter().map(|t| t.right()).fold(f32::MIN, f32::max);
            let left = tiles.iter().map(|t| t.left()).fold(f32::MAX, f32::min);
            let edge = buttons.iter().map(|b| b.right()).fold(f32::MIN, f32::max);
            assert!(
                (right - edge).abs() < 0.5,
                "at {width}: tiles end at {right}, the filter row at {edge}"
            );
            assert!(
                (left - title).abs() < 0.5,
                "at {width}: tiles start at {left}, the heading text at {title}"
            );
        }
    }

    #[test]
    fn tiles_grow_to_their_largest_before_the_gaps_widen() {
        for step in 0..800 {
            let width = 100.0 + step as f32;
            let grid = super::tile_layout(width);
            let gaps = (grid.columns - 1) as f32;
            let filled = grid.width * grid.columns as f32 + grid.gap * gaps;
            assert!((filled - width).abs() < 0.01, "at {width}: {filled}");
            assert!(grid.width >= super::TILE_MIN, "at {width}: {}", grid.width);
            assert!(
                (super::TILE_GAP..=super::GAP_MAX).contains(&grid.gap),
                "at {width}: gap {}",
                grid.gap
            );
            if grid.gap > super::TILE_GAP {
                assert!(grid.width >= super::TILE_MAX - 0.01, "at {width}");
            }
            if grid.width > super::TILE_MAX + 0.01 && grid.columns > 1 {
                assert_eq!(grid.gap, super::GAP_MAX, "at {width}");
            }
        }
    }

    #[test]
    fn a_desktop_page_fills_five_rows_of_the_grid() {
        assert_eq!(library::desk_page(8), 40);
        assert_eq!(library::desk_page(5), 25);
        assert_eq!(library::desk_page(0), 40);
    }

    #[test]
    fn all_consoles_starts_where_the_console_names_start() {
        use crate::app::ui_state::library::Filter;
        let ctx = egui::Context::default();
        crate::app::Citrine::setup_fonts(&ctx);
        let mut filter = Filter::default();
        let mut output = None;
        for _ in 0..2 {
            output = Some(ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        library::console_ticks(ui, &mut filter)
                    });
                });
            }));
        }
        let start_of = |wanted: &str, first: char| {
            output
                .as_ref()
                .unwrap()
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        text.galley.rows.iter().find_map(|row| {
                            row.glyphs
                                .iter()
                                .find(|glyph| glyph.chr == first)
                                .map(|glyph| text.pos.x + row.pos.x + glyph.pos.x)
                        })
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no {wanted}"))
        };
        let all = start_of("All consoles", 'A');
        for kind in library::consoles() {
            let label = crate::app::ui_state::settings::kind_label(kind);
            let name = crate::app::ui_state::settings::kind_name(kind);
            let start = start_of(&label, name.chars().next().unwrap());
            assert!((all - start).abs() < 0.5, "{label}: {start} vs {all}");
        }
    }

    #[test]
    fn all_consoles_is_a_tick_row_like_the_consoles() {
        use crate::app::ui_state::library::Filter;
        use citrine_gb::console::ConsoleKind;
        let ctx = egui::Context::default();
        crate::app::Citrine::setup_fonts(&ctx);
        let mut filter = Filter::default();
        filter.toggle_console(ConsoleKind::Gba);
        let mut frame = |events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            library::console_ticks(ui, &mut filter)
                        });
                    });
                },
            )
        };
        let shown = frame(Vec::new());
        let all = shown
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "All consoles" => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()))
                }
                _ => None,
            })
            .expect("an All consoles row");
        let at = all.center();
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(at)]);
        frame(vec![press(true)]);
        let clicked = frame(vec![press(false)]);
        let info = clicked
            .platform_output
            .events
            .iter()
            .find_map(|event| match event {
                egui::output::OutputEvent::Clicked(info) => Some(info.clone()),
                _ => None,
            })
            .expect("clicking All consoles reports the click");
        assert_eq!(info.typ, egui::WidgetType::Checkbox);
        assert_eq!(info.label.as_deref(), Some("All consoles"));
        assert_eq!(info.selected, Some(true));
        assert!(filter.consoles.is_empty());
    }

    #[test]
    fn the_console_filter_names_its_picks() {
        use citrine_gb::console::ConsoleKind;
        assert_eq!(library::console_label(&[]), "All consoles");
        assert_eq!(
            library::console_label(&[ConsoleKind::Gba]),
            crate::app::ui_state::settings::kind_label(ConsoleKind::Gba)
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
