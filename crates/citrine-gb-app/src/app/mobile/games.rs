use super::kit;
use super::nav::Place;
use crate::app::Citrine;
use crate::app::tabs::{Tab, playing, stats};
use crate::app::ui_state::recent::RecentRom;
use crate::app::widgets::action::main_button;
use crate::icons;
use egui::{Align, CornerRadius, Layout, RichText, Ui, vec2};

const THUMB_WIDTH: f32 = 128.0;

const SEARCH_FROM: usize = 8;

enum GamesAction {
    Open,
    Resume,
    Search,
    Homebrew,
    Recent(std::path::PathBuf),
    Remove(std::path::PathBuf),
    #[cfg(not(target_arch = "wasm32"))]
    ChooseFolder,
    #[cfg(not(target_arch = "wasm32"))]
    Folder(crate::app::ui_state::rom_folder::FolderGame),
}

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
                    "File not found".to_owned()
                } else {
                    stats::capitalized(&stats::last_played(entry.last_played, today))
                },
                gone,
                path: entry.path.clone(),
            }
        })
        .collect()
}

impl Citrine {
    pub(super) fn games_place(&mut self, ui: &mut Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        self.ui.rom_folder.poll();
        let mut action = None;
        kit::place(ui, "games_place", |ui| {
            let searchable = self.folder_count() > SEARCH_FROM;
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
                let rows = recent
                    .iter()
                    .map(|game| {
                        let row = kit::Row::new(&game.name)
                            .icon(icons::GAME_CONTROLLER)
                            .detail(&game.detail)
                            .no_chevron();
                        if game.gone { row.value("Remove") } else { row }
                    })
                    .collect();
                if let Some(index) = kit::rows(ui, rows) {
                    let game = &recent[index];
                    action = Some(if game.gone {
                        GamesAction::Remove(game.path.clone())
                    } else {
                        GamesAction::Recent(game.path.clone())
                    });
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            let games_above = !first_time || self.ui.rom_folder.label().is_some();
            #[cfg(target_arch = "wasm32")]
            let games_above = !first_time;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(picked) = self.folder_section(ui, &search) {
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
                    let label = self.ui.rom_folder.label();
                    rows.push(
                        kit::Row::new("ROM folder")
                            .icon(icons::FOLDER_SIMPLE)
                            .value(label.unwrap_or_else(|| "Choose".to_owned())),
                    );
                    actions.push(GamesAction::ChooseFolder);
                }
                rows.push(kit::Row::new(Tab::Homebrew.label()).icon(icons::JOYSTICK));
                actions.push(GamesAction::Homebrew);
                if let Some(index) = kit::rows(ui, rows) {
                    action = actions.into_iter().nth(index);
                }
            }
        });
        self.apply_games(action);
    }

    fn apply_games(&mut self, action: Option<GamesAction>) {
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
            #[cfg(not(target_arch = "wasm32"))]
            Some(GamesAction::ChooseFolder) => self.ui.rom_folder.choose(),
            #[cfg(not(target_arch = "wasm32"))]
            Some(GamesAction::Folder(game)) => {
                self.try_start_audio();
                self.ui.rom_folder.read(&game, self.files.rom_tx.clone());
            }
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

    fn folder_count(&self) -> usize {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.ui.rom_folder.games.as_ref().map_or(0, Vec::len)
        }
        #[cfg(target_arch = "wasm32")]
        {
            0
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn folder_section(&mut self, ui: &mut Ui, search: &str) -> Option<GamesAction> {
        use crate::app::ui_state::rom_folder::matching;
        let folder = &self.ui.rom_folder;
        let label = folder.label()?;
        let Some(games) = folder.games.as_deref() else {
            kit::section_label(ui, &label);
            kit::card(ui, |ui| {
                kit::padded(ui, |ui| {
                    ui.label(RichText::new("Reading the folder").weak())
                })
            });
            return None;
        };
        if !folder.readable {
            kit::section_label(ui, &label);
            let rows = vec![
                kit::Row::new("Can't open this folder")
                    .icon(icons::WARNING)
                    .detail("Choose it again"),
            ];
            return kit::rows(ui, rows).map(|_| GamesAction::ChooseFolder);
        }
        let shown = matching(games, search);
        if shown.is_empty() && !search.is_empty() {
            return None;
        }
        let count = match games.len() {
            1 => "1 game".to_owned(),
            n => format!("{n} games"),
        };
        kit::section_label_with(ui, &label, &count);
        if shown.is_empty() {
            kit::card(ui, |ui| {
                kit::padded(ui, |ui| {
                    ui.label(RichText::new("No Game Boy games in this folder").weak())
                })
            });
            return None;
        }
        let rows = shown
            .iter()
            .map(|game| {
                kit::Row::new(&game.name)
                    .icon(icons::GAME_CONTROLLER)
                    .no_chevron()
            })
            .collect();
        kit::rows(ui, rows).map(|index| GamesAction::Folder(shown[index].clone()))
    }

    fn now_playing(&self, ui: &mut Ui) -> bool {
        let title = self.now_playing_title();
        let detail = self.now_playing_detail();
        kit::card(ui, |ui| {
            kit::padded(ui, |ui| {
                ui.horizontal(|ui| {
                    let height = THUMB_WIDTH * 144.0 / 160.0;
                    match self.emulator.screen_texture() {
                        Some(texture) => {
                            ui.add(
                                egui::Image::new((texture, vec2(THUMB_WIDTH, height)))
                                    .corner_radius(CornerRadius::same(10)),
                            );
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
                        }
                    }
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
                        ui.add(
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

    fn now_playing_title(&self) -> String {
        if self.emulator.gb.cartridge.has_rom_loaded {
            return self.emulator.game_name().to_owned();
        }
        match self.play.session() {
            Some(session) => {
                let name = playing::friend_name(&self.online, session.friend());
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

#[cfg(test)]
mod tests;
