use crate::app::Citrine;
use crate::app::tabs::games::library::info;
use crate::app::ui_state::games::Undo;
use crate::app::ui_state::library::LibraryGame;
use std::path::PathBuf;

const SCAN_REPAINT: std::time::Duration = std::time::Duration::from_millis(250);
const UNDO_REPAINT: std::time::Duration = std::time::Duration::from_secs(1);

impl Citrine {
    pub(super) fn follow_library(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "android")]
        let visible = crate::platform::android::lifecycle::visible();
        #[cfg(not(target_os = "android"))]
        let visible = true;
        let focused = visible && ctx.input(|input| input.focused);
        let was = std::mem::replace(&mut self.ui.games.focused, focused);
        if focused && !was {
            self.ui.library.rescan(&self.files.spawner);
        }
        let now = crate::storage::now_unix();
        let found = self.ui.library.poll(&self.files.spawner, now);
        if self.ui.settings.covers {
            for game in found {
                self.covers.want(&self.emulator.store.covers(), game, now);
            }
        }
        #[cfg(target_arch = "wasm32")]
        self.check_library_copies();
        self.find_game_facts(ctx, now);
        if self.ui.library.busy() {
            ctx.request_repaint_after(SCAN_REPAINT);
        }
        if self.ui.games.undo.is_some() {
            ctx.request_repaint_after(UNDO_REPAINT);
        }
        if let Some(undo) = self.ui.games.expired_undo(ctx.input(|input| input.time)) {
            self.forget(undo.games);
        }
    }

    fn find_game_facts(&mut self, ctx: &egui::Context, now: u64) {
        let shown = std::mem::take(&mut self.ui.games.library_shown);
        let open = self
            .mobile
            .library
            .game()
            .or(self.ui.games.info.as_deref())
            .and_then(|path| self.ui.library.find(path))
            .and_then(info::ask);
        if !shown && open.is_none() && !self.game_facts.is_busy() {
            return;
        }
        let store = self.emulator.store.game_facts();
        if shown {
            for console in self.ui.library.consoles() {
                self.game_facts.want_facts(&store, console, now);
            }
        }
        if let Some(ask) = open {
            self.game_facts.want_page(&store, ask, now);
        }
        self.game_facts.poll(&store, now);
        let facts = &self.game_facts;
        self.ui.library.set_genres(|game| {
            facts
                .facts(game.console, game.crc32)
                .and_then(|facts| facts.genre.as_deref())
        });
        if self.game_facts.is_busy() {
            ctx.request_repaint_after(SCAN_REPAINT);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn check_library_copies(&mut self) {
        use crate::app::ui_state::library::FileState;
        let copies = &self.files.rom_copies;
        let checks: Vec<(String, FileState)> = self
            .ui
            .library
            .loose_files()
            .into_iter()
            .map(|path| {
                let file = if copies.is_gone(std::path::Path::new(&path)) {
                    FileState::Missing
                } else {
                    FileState::Present
                };
                (path, file)
            })
            .collect();
        self.ui.library.apply_checks(checks);
    }

    pub(super) fn play_library(&mut self, path: String) {
        self.try_start_audio();
        #[cfg(target_arch = "wasm32")]
        {
            let name = self
                .ui
                .library
                .find(&path)
                .map(|game| game.name.clone())
                .unwrap_or_default();
            self.files.rom_copies.read(
                std::path::Path::new(&path),
                name,
                self.files.rom_tx.clone(),
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        if path.starts_with("content://") {
            self.ui
                .library
                .read(&self.files.spawner, &path, self.files.rom_tx.clone());
        } else {
            self.load_recent(PathBuf::from(path));
        }
    }

    pub(super) fn remove_from_library(&mut self, path: &str) {
        let games = self.ui.library.remove_game(path);
        self.offer_undo(Undo::new(None, games));
    }

    pub(super) fn remove_missing(&mut self) {
        let games = self.ui.library.remove_missing();
        self.ui.games.filter.missing = false;
        self.offer_undo(Undo::new(None, games));
    }

    pub(super) fn remove_folder(&mut self, tree: String) {
        let games = self.ui.library.remove_folder(&tree);
        let filter = &mut self.ui.games.filter;
        if filter.place == crate::app::ui_state::library::Place::Folder(tree.clone()) {
            filter.place = crate::app::ui_state::library::Place::All;
        }
        self.offer_undo(Undo::new(Some(tree), games));
    }

    pub(super) fn undo_library(&mut self) {
        if let Some(undo) = self.ui.games.undo.take() {
            self.ui
                .library
                .bring_back(&self.files.spawner, undo.folder, undo.games);
        }
    }

    fn offer_undo(&mut self, undo: Undo) {
        if undo.folder.is_none() && undo.games.is_empty() {
            return;
        }
        self.ui.games.page = 0;
        if let Some(old) = self.ui.games.offer_undo(undo) {
            self.forget(old.games);
        }
    }

    fn forget(&mut self, games: Vec<LibraryGame>) {
        let recent = self.ui.recent.entries();
        let library = &self.ui.library;
        let paths: Vec<PathBuf> = games
            .iter()
            .filter(|game| library.find(&game.path).is_none())
            .map(|game| PathBuf::from(&game.path))
            .filter(|path| !recent.iter().any(|entry| &entry.path == path))
            .collect();
        self.files
            .rom_copies
            .drop_copies(paths.iter().map(PathBuf::as_path));
    }
}
