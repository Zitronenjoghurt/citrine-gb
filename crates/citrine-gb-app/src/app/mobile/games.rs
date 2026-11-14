use super::kit;
use super::nav::Place;
use crate::app::Citrine;
use crate::app::events::GamesAction;
use crate::app::tabs::{Tab, playing, stats};
use crate::app::ui_state::recent::RecentRom;
use crate::app::ui_state::settings;
use crate::app::widgets::action::main_button;
use crate::icons;
use egui::{Align, CornerRadius, Layout, RichText, Ui, vec2};

const THUMB_WIDTH: f32 = 128.0;

const SEARCH_FROM: usize = 8;

#[derive(Default)]
pub struct GamesSearch {
    open: bool,
    text: String,
    focus: bool,
}

pub struct RecentGame {
    pub name: String,
    pub detail: String,
    pub gone: bool,
    pub path: std::path::PathBuf,
    pub console: Option<&'static str>,
}

pub fn recent_games(
    entries: &[RecentRom],
    playing: (Option<&std::path::Path>, Option<&str>),
    today: jiff::civil::Date,
    gone: impl Fn(&std::path::Path) -> bool,
) -> Vec<RecentGame> {
    entries
        .iter()
        .filter(|entry| {
            Some(entry.path.as_path()) != playing.0
                && (playing.1.is_none()
                    || entry.path.file_stem().and_then(|stem| stem.to_str()) != playing.1)
        })
        .map(|entry| {
            let gone = gone(&entry.path);
            RecentGame {
                name: entry.display_name(),
                detail: if gone {
                    "File missing".to_owned()
                } else {
                    let played = stats::last_played(entry.last_played, today);
                    match entry.console {
                        Some(kind) => format!("{}, {played}", settings::kind_name(kind)),
                        None => stats::capitalized(&played),
                    }
                },
                gone,
                path: entry.path.clone(),
                console: entry.console.map(settings::kind_name),
            }
        })
        .collect()
}

impl Citrine {
    pub(super) fn games_place(&mut self, ui: &mut Ui) {
        let mut action = None;
        kit::place(ui, "games_place", |ui| {
            let searchable = self.games_searchable();
            let open_row = self.ui.recent.entries().is_empty() && !self.game_open();
            kit::place_title(ui, Place::Games.label(), |ui| {
                let side = ui.spacing().interact_size.y;
                if !open_row
                    && ui
                        .add(kit::header_button(icons::FOLDER_OPEN, side))
                        .on_hover_text("Open a ROM")
                        .clicked()
                {
                    action = Some(GamesAction::Open);
                }
                if searchable
                    && ui
                        .add(kit::header_button(icons::MAGNIFYING_GLASS, side))
                        .on_hover_text("Search")
                        .clicked()
                {
                    action = Some(GamesAction::Search);
                }
            });
            let search = self.games_search(ui, searchable);
            if self.game_open() && search.is_empty() {
                kit::section_label(ui, "Now playing");
                if self.now_playing(ui) {
                    action = Some(GamesAction::Resume);
                }
            }
            #[cfg(feature = "nds")]
            if search.is_empty() && self.walker_card(ui) {
                action = Some(GamesAction::Walker);
            }
            #[cfg(feature = "nds")]
            if search.is_empty() && self.gts_card(ui) {
                action = Some(GamesAction::Gts);
            }
            let today = crate::stats::today();
            #[cfg(not(target_arch = "wasm32"))]
            let playing = self.emulator.rom_path();
            #[cfg(target_arch = "wasm32")]
            let playing = None;
            let playing = (playing, self.emulator.rom_key());
            let copies = &self.files.rom_copies;
            let recent = recent_games(self.ui.recent.entries(), playing, today, |path| {
                copies.is_gone(path)
            });
            let lowered = search.to_lowercase();
            let recent: Vec<RecentGame> = recent
                .into_iter()
                .filter(|game| game.name.to_lowercase().contains(&lowered))
                .collect();
            let first_time = recent.is_empty() && !self.game_open();
            if !recent.is_empty() {
                kit::section_label(ui, "Recent");
                let picked = if self.big_screen_on() && !self.pad_keys_shown() {
                    self.recent_shelf(ui, &recent)
                } else {
                    let covers = self.box_covers(ui.ctx(), &recent);
                    let rows = recent
                        .iter()
                        .zip(covers)
                        .map(|(game, cover)| {
                            let row = kit::Row::new(&game.name)
                                .icon(icons::GAME_CONTROLLER)
                                .cover(cover)
                                .picture(self.recent_picture(ui.ctx(), &game.path))
                                .detail(&game.detail)
                                .no_chevron();
                            if game.gone { row.value("Remove") } else { row }
                        })
                        .collect();
                    kit::rows(ui, rows)
                };
                if let Some(index) = picked {
                    let game = &recent[index];
                    action = Some(if game.gone {
                        GamesAction::Remove(game.path.clone())
                    } else {
                        GamesAction::Recent(game.path.clone())
                    });
                }
            }
            let library = &self.ui.library;
            let games_above = !first_time || !library.is_empty() || !library.folders().is_empty();
            if let Some(picked) = self.library_section(ui, &search) {
                action = Some(picked);
            }
            if search.is_empty() {
                if games_above {
                    kit::section_label(ui, "More games");
                } else {
                    ui.add_space(12.0);
                }
                let mut rows = Vec::new();
                let mut actions = Vec::new();
                if first_time {
                    rows.push(kit::Row::new("Open a ROM").icon(icons::FOLDER_OPEN));
                    actions.push(GamesAction::Open);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let library = &self.ui.library;
                    let icon = if library.needs_access() {
                        icons::WARNING
                    } else {
                        icons::FOLDER_SIMPLE
                    };
                    let folders = library.folders().len();
                    let value = if folders == 0 {
                        "Add".to_owned()
                    } else {
                        folders.to_string()
                    };
                    rows.push(
                        kit::Row::new(Tab::RomFolders.title())
                            .icon(icon)
                            .value(value),
                    );
                    actions.push(GamesAction::Folders);
                }
                rows.push(kit::Row::new(Tab::Homebrew.label()).icon(icons::JOYSTICK));
                actions.push(GamesAction::Homebrew);
                rows.push(kit::Row::new(Tab::SecondScreens.title()).icon(icons::DEVICES));
                actions.push(GamesAction::SecondScreens);
                if let Some(index) = kit::rows(ui, rows) {
                    action = actions.into_iter().nth(index);
                }
            }
        });
        self.library_sheets(ui.ctx());
        self.apply_games(action);
    }

    pub(in crate::app) fn apply_games(&mut self, action: Option<GamesAction>) {
        match action {
            Some(GamesAction::Open) => self.pick_rom(),
            Some(GamesAction::Resume) => self.resume_game(),
            Some(GamesAction::Search) => {
                let search = &mut self.mobile.games_search;
                search.open = !search.open;
                search.focus = search.open;
                search.text.clear();
            }
            Some(GamesAction::Homebrew) => {
                self.mobile.open(Tab::Homebrew);
                self.release_touch();
            }
            Some(GamesAction::SecondScreens) => {
                self.mobile.open(Tab::SecondScreens);
                self.release_touch();
            }
            #[cfg(feature = "nds")]
            Some(GamesAction::Walker) => {
                self.mobile.open(Tab::Walker);
                self.release_touch();
            }
            #[cfg(feature = "nds")]
            Some(GamesAction::Gts) => {
                self.mobile.open(Tab::Gts);
                self.release_touch();
            }
            Some(GamesAction::Remove(path)) => self.ui.recent.remove(&path),
            #[cfg(not(target_arch = "wasm32"))]
            Some(GamesAction::Recent(path)) => {
                self.try_start_audio();
                self.load_recent(path);
            }
            #[cfg(target_arch = "wasm32")]
            Some(GamesAction::Recent(path)) => {
                self.try_start_audio();
                let name = self
                    .ui
                    .recent
                    .entries()
                    .iter()
                    .find(|entry| entry.path == path)
                    .map(|entry| entry.display_name())
                    .unwrap_or_default();
                self.files
                    .rom_copies
                    .read(&path, name, self.files.rom_tx.clone());
            }
            Some(GamesAction::AddFolder) => self.ui.library.choose_folder(&self.files.spawner),
            Some(GamesAction::RemoveFolder(tree)) => self.remove_folder(tree),
            Some(GamesAction::Play(path)) => self.play_library(path),
            Some(GamesAction::RemoveGame(path)) => self.remove_from_library(&path),
            Some(GamesAction::RemoveMissing) => self.remove_missing(),
            Some(GamesAction::Undo) => self.undo_library(),
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            Some(GamesAction::ShowInFolder(path)) => {
                let path = std::path::Path::new(&path);
                if let Err(err) = crate::media::reveal::reveal(&self.files.spawner, path) {
                    log::warn!("couldn't show {}: {err}", path.display());
                }
            }
            Some(GamesAction::LibraryFilter) => self.open_library_filter(),
            Some(GamesAction::Folders) => {
                self.mobile.open(Tab::RomFolders);
                self.release_touch();
            }
            Some(GamesAction::GameMenu(path)) => self.open_library_game(path),
            Some(GamesAction::GameInfo(path)) => self.ui.games.open_info(path),
            Some(GamesAction::CloseInfo) => self.ui.games.close_info(),
            Some(GamesAction::AddTag(path, tag)) => {
                self.ui.library.add_tag(&path, &tag);
                self.ui.games.tag_draft = None;
            }
            Some(GamesAction::RemoveTag(path, tag)) => self.ui.library.remove_tag(&path, &tag),
            None => {}
        }
    }

    fn games_search(&mut self, ui: &mut Ui, searchable: bool) -> String {
        let search = &mut self.mobile.games_search;
        if !searchable || !search.open {
            return String::new();
        }
        let response = kit::field(
            ui,
            egui::TextEdit::singleline(&mut search.text).hint_text("Search"),
        );
        if std::mem::take(&mut search.focus) {
            response.request_focus();
        }
        search.text.clone()
    }

    pub(in crate::app) fn games_searchable(&self) -> bool {
        self.ui.library.len() > SEARCH_FROM
    }

    pub(in crate::app) fn open_games_search(&mut self) {
        if self.games_searchable() && !self.mobile.games_search.open {
            self.apply_games(Some(GamesAction::Search));
        }
    }

    pub(super) fn games_search_text(&self) -> String {
        if self.mobile.games_search.open {
            self.mobile.games_search.text.clone()
        } else {
            String::new()
        }
    }

    pub(super) fn games_search_open(&self) -> bool {
        let nav = &self.mobile.nav;
        self.mobile.games_search.open
            && nav.place() == Place::Games
            && !nav.on_game()
            && nav.page().is_none()
    }

    pub(super) fn close_games_search(&mut self) {
        self.mobile.games_search = GamesSearch::default();
    }

    fn now_playing(&self, ui: &mut Ui) -> bool {
        let title = self.now_playing_title();
        let detail = self.now_playing_detail();
        kit::card(ui, |ui| {
            kit::padded(ui, |ui| {
                ui.horizontal(|ui| {
                    let height = THUMB_WIDTH * self.emulator.video().aspect();
                    let rect = match self.emulator.screen_texture() {
                        Some(texture) => {
                            ui.add(
                                egui::Image::new((texture, vec2(THUMB_WIDTH, height)))
                                    .corner_radius(CornerRadius::same(10)),
                            )
                            .rect
                        }
                        None => {
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(THUMB_WIDTH, height),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(10),
                                ui.visuals().extreme_bg_color,
                            );
                            rect
                        }
                    };
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(10),
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    ui.add_space(6.0);
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(RichText::new(title).size(17.0).strong()).truncate(),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(detail)
                                    .size(13.0)
                                    .color(kit::value_color(ui.visuals())),
                            )
                            .truncate(),
                        );
                        ui.add_space(10.0);
                        let text = format!("{} Resume", icons::PLAY);
                        crate::app::widgets::action::add_centered(
                            ui,
                            main_button(ui, RichText::new(text).size(16.0))
                                .corner_radius(CornerRadius::same(22))
                                .min_size(vec2(120.0, 44.0)),
                        )
                        .clicked()
                    })
                    .inner
                })
                .inner
            })
        })
    }

    fn box_covers(
        &mut self,
        ctx: &egui::Context,
        recent: &[RecentGame],
    ) -> Vec<Option<(egui::TextureId, egui::Vec2)>> {
        if !self.ui.settings.covers {
            return vec![None; recent.len()];
        }
        let playing = self.emulator.rom_key().map(str::to_owned);
        recent
            .iter()
            .map(|game| {
                let key = self.ui.recent.game_key(&game.path)?;
                let cover = self.big.covers.get(
                    ctx,
                    &self.emulator.store,
                    key,
                    playing.as_deref(),
                    true,
                )?;
                cover.boxed.then(|| (cover.id(), cover.size))
            })
            .collect()
    }

    #[cfg(feature = "nds")]
    fn recent_picture(
        &self,
        ctx: &egui::Context,
        path: &std::path::Path,
    ) -> Option<egui::TextureId> {
        let icon = crate::ds_banner::icon_of_text(self.ui.recent.icon(path)?)?;
        crate::ds_banner::texture(ctx, &icon).map(|texture| texture.id())
    }

    #[cfg(not(feature = "nds"))]
    fn recent_picture(
        &self,
        _ctx: &egui::Context,
        _path: &std::path::Path,
    ) -> Option<egui::TextureId> {
        None
    }

    #[cfg(feature = "nds")]
    fn walker_card(&mut self, ui: &mut Ui) -> bool {
        let Some(walker) = self.emulator.shown_walker() else {
            return false;
        };
        let pokemon = walker.walking_pokemon().map(|_| {
            citrine_gb_pkm::pkm::text::decode(citrine_gb_pkm::Gen::Four, &walker.walking_nickname())
        });
        let steps = crate::app::tabs::walker::format_steps(walker.today_steps());
        let watts = crate::app::tabs::walker::watts(walker.health().watts());
        let counts = format!("{steps} steps today, {watts}");
        let detail = match pokemon {
            Some(name) => format!("{name}, {counts}"),
            None => counts,
        };
        ui.add_space(8.0);
        let row = kit::Row::new(Tab::Walker.title())
            .icon(icons::PERSON_SIMPLE_WALK)
            .detail(&detail);
        kit::rows(ui, vec![row]).is_some()
    }

    #[cfg(feature = "nds")]
    fn gts_card(&mut self, ui: &mut Ui) -> bool {
        if !self.ui.settings.online.is_joined() || !self.ui.settings.ds_online.gts_here() {
            return false;
        }
        self.online.gts_want_mine();
        let mine = self.online.gts_mine().unwrap_or_default();
        let traded = mine
            .iter()
            .filter(|d| matches!(d.state, citrine_gb_net::GtsState::Traded { .. }))
            .count();
        let waiting = mine.len() - traded;
        let detail = match (traded, waiting) {
            (0, 0) if self.online.gts_simulated() => {
                "Search results include simulated trainers".to_owned()
            }
            (0, 0) => "Trade Pokémon with players on your relay".to_owned(),
            (0, waiting) => format!("{waiting} waiting for a trade"),
            (traded, 0) => format!("{traded} traded, pick up in the game"),
            (traded, waiting) => format!("{traded} traded, {waiting} waiting"),
        };
        ui.add_space(8.0);
        let row = kit::Row::new(Tab::Gts.label())
            .icon(icons::GLOBE_HEMISPHERE_EAST)
            .detail(&detail);
        kit::rows(ui, vec![row]).is_some()
    }

    fn now_playing_title(&self) -> String {
        if self.emulator.has_game() {
            return self.emulator.game_name().to_owned();
        }
        match self.play.session() {
            Some(session) => {
                let name = playing::member_name(&self.play, &self.online, session.friend());
                format!("Playing with {name}")
            }
            None => "Linked game".to_owned(),
        }
    }

    fn now_playing_detail(&self) -> String {
        let state = if self.play.session().is_some() {
            "With a friend"
        } else if self.emulator.running && !self.mobile.held_pause {
            "Running"
        } else {
            "Paused"
        };
        let today = self
            .emulator
            .rom_key()
            .and_then(|key| {
                self.stats
                    .book()
                    .overview(crate::stats::today())
                    .game(key)
                    .map(|game| game.today_ms)
            })
            .filter(|ms| *ms >= 60_000);
        match today {
            Some(ms) => format!("{state}, played {} today", stats::play_time(ms)),
            None => state.to_owned(),
        }
    }
}

mod shelf;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(path: PathBuf, title: &str) -> RecentRom {
        RecentRom {
            path,
            title: title.to_owned(),
            last_played: 0,
            rom_key: None,
            console: None,
        }
    }

    #[test]
    fn the_game_now_playing_is_not_listed_again() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.gb");
        let second = dir.path().join("second.gb");
        std::fs::write(&first, b"rom").unwrap();
        std::fs::write(&second, b"rom").unwrap();
        let entries = [entry(first.clone(), "First"), entry(second, "Second")];
        let today = jiff::civil::date(2026, 11, 2);
        let games = recent_games(&entries, (Some(&first), None), today, |path| {
            !path.is_file()
        });
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].name, "Second");
        assert!(!games[0].gone);
    }

    #[test]
    fn a_missing_file_shows_as_gone_with_remove() {
        let dir = tempfile::tempdir().unwrap();
        let entries = [entry(dir.path().join("moved.gb"), "Moved")];
        let games = recent_games(
            &entries,
            (None, None),
            jiff::civil::date(2026, 11, 2),
            |path| !path.is_file(),
        );
        assert!(games[0].gone);
        assert_eq!(games[0].detail, "File missing");
    }

    #[test]
    fn a_kept_copy_of_the_game_now_playing_is_not_listed_again() {
        let entries = [entry(PathBuf::from("abc123.gbc"), "Copy")];
        let today = jiff::civil::date(2026, 11, 2);
        let games = recent_games(&entries, (None, Some("abc123")), today, |_| false);
        assert!(games.is_empty());
    }

    const KEY: &str = "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789";

    fn app_with_a_boxed_game(name: &str) -> (Citrine, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("red.gb");
        std::fs::write(&rom, b"rom").unwrap();
        let mut app = crate::app::tests::phone_app(name);
        app.ui.recent.record(&rom, "Pokémon Red", None);
        app.ui.recent.set_game_key(&rom, KEY.to_owned());
        let art = crate::utils::screenshot::encode_png(30, 28, &[200; 30 * 28 * 4]).unwrap();
        app.emulator.store.covers().put_art(KEY, &art).unwrap();
        (app, dir)
    }

    fn painted(app: &mut Citrine, ctx: &egui::Context) -> Vec<egui::TextureId> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 900.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(input.clone(), |ctx| {
                crate::app::mobile::touch_spacing(ctx, app.is_mobile(ctx));
                app.render(ctx);
            }));
        }
        fn textures(shape: &egui::Shape, out: &mut Vec<egui::TextureId>) {
            match shape {
                egui::Shape::Mesh(mesh) => out.push(mesh.texture_id),
                egui::Shape::Rect(rect) => out.push(rect.fill_texture_id()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| textures(shape, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.unwrap().shapes {
            textures(&clipped.shape, &mut out);
        }
        out
    }

    fn boxes(ctx: &egui::Context, shown: &[egui::TextureId]) -> usize {
        let textures = ctx.tex_manager();
        let textures = textures.read();
        shown
            .iter()
            .filter(|id| {
                textures
                    .meta(**id)
                    .is_some_and(|meta| meta.name == format!("box_{KEY}"))
            })
            .count()
    }

    #[test]
    fn a_kept_cover_shows_in_the_library_list() {
        let (mut app, _dir) = app_with_a_boxed_game("cover-list");
        let ctx = egui::Context::default();
        let shown = painted(&mut app, &ctx);
        assert_eq!(boxes(&ctx, &shown), 1);
    }

    #[test]
    fn a_kept_cover_shows_on_the_big_screen_shelf() {
        let (mut app, _dir) = app_with_a_boxed_game("cover-shelf");
        app.ui.settings.big_screen.on = true;
        let ctx = egui::Context::default();
        let shown = painted(&mut app, &ctx);
        assert_eq!(boxes(&ctx, &shown), 1);
    }

    #[test]
    fn covers_turned_off_show_no_box() {
        for big in [false, true] {
            let (mut app, _dir) = app_with_a_boxed_game("cover-off");
            app.ui.settings.covers = false;
            app.ui.settings.big_screen.on = big;
            let ctx = egui::Context::default();
            let shown = painted(&mut app, &ctx);
            assert_eq!(boxes(&ctx, &shown), 0, "big screen {big}");
        }
    }

    #[test]
    fn deleting_everything_drops_the_covers_too() {
        let (mut app, _dir) = app_with_a_boxed_game("cover-delete");
        app.delete_everything();
        assert!(!app.emulator.store.covers().has_art(KEY));
    }
}
