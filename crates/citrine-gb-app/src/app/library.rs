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
        let depth = self.ui.settings.library_depth;
        self.ui.library.follow_depth(&self.files.spawner, depth);
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
            self.settle_undo(undo);
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
            for &console in self.ui.library.consoles() {
                self.game_facts.want_facts(console);
            }
        }
        if let Some(ask) = open {
            self.game_facts.want_page(&store, ask, now);
        }
        self.game_facts.poll(&store, &self.files.spawner, now);
        let facts = &self.game_facts;
        self.ui.library.follow_genres(facts.revision(), |game| {
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
        if self.play_archive(&path) {
            return;
        }
        self.try_start_audio();
        #[cfg(target_arch = "wasm32")]
        {
            let game = self.ui.library.find(&path);
            let name = game.map(|game| game.name.clone()).unwrap_or_default();
            let copies = match game.is_some_and(|game| game.archive.is_some()) {
                true => self.archive.store(),
                false => &self.files.rom_copies,
            };
            copies.read(std::path::Path::new(&path), name, self.files.rom_tx.clone());
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
            if let Some(path) = &undo.download {
                self.keep_download(path);
            }
            self.ui
                .library
                .bring_back(&self.files.spawner, undo.folder, undo.games);
        }
    }

    pub(super) fn offer_undo(&mut self, undo: Undo) {
        if undo.folder.is_none() && undo.games.is_empty() && undo.download.is_none() {
            return;
        }
        self.ui.games.page = 0;
        if let Some(old) = self.ui.games.offer_undo(undo) {
            self.settle_undo(old);
        }
    }

    pub(super) fn settle_undo(&mut self, undo: Undo) {
        if let Some(path) = &undo.download {
            self.drop_download(path);
        }
        self.forget(undo.games);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::Citrine;
    use crate::app::tests::phone_app;
    use crate::app::ui_state::library::{FileFound, Identity};
    use crate::app::ui_state::settings::LayoutMode;
    use citrine_gb::console::ConsoleKind;
    use egui::Context;
    use std::time::Duration;

    const GAMES: usize = 300;
    const NOW: u64 = 1_800_000_000;

    struct Shelf {
        app: Citrine,
        ctx: Context,
        frame: eframe::Frame,
        time: f64,
        causes: Vec<String>,
        _roms: tempfile::TempDir,
    }

    impl Shelf {
        fn new(name: &str, layout: LayoutMode) -> Self {
            Self::with_games(name, layout, GAMES)
        }

        fn with_games(name: &str, layout: LayoutMode, games: usize) -> Self {
            let roms = tempfile::tempdir().unwrap();
            let mut app = phone_app(name);
            app.ui.settings.layout = layout;
            app.ui.settings.check_updates = false;
            app.emulator.running = false;
            let tree = roms.path().to_string_lossy().into_owned();
            let files = (0..games)
                .map(|n| {
                    let path = roms.path().join(format!("Game {n}.gb"));
                    std::fs::write(&path, [n as u8; 0x200]).unwrap();
                    FileFound {
                        name: format!("Game {n}"),
                        path: path.to_string_lossy().into_owned(),
                    }
                })
                .collect::<Vec<_>>();
            let library = &mut app.ui.library;
            library.add_folder(&tree);
            library.apply_listing(&tree, Some(files.clone()), NOW);
            for (n, file) in files.iter().enumerate() {
                library.identify(
                    &file.path,
                    Identity {
                        rom_key: format!("{n:064x}"),
                        sha1: format!("{n:040x}"),
                        crc32: Some(n as u32),
                        ra_hash: Some(format!("{n:032x}")),
                        console: Some(ConsoleKind::Gb),
                        name: None,
                    },
                );
            }
            Self {
                app,
                ctx: Context::default(),
                frame: eframe::Frame::_new_kittest(),
                time: 0.0,
                causes: Vec::new(),
                _roms: roms,
            }
        }

        fn step(&mut self) -> Duration {
            self.step_after(Duration::from_secs_f64(1.0 / 60.0))
        }

        fn step_after(&mut self, dt: Duration) -> Duration {
            self.time += dt.as_secs_f64();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                predicted_dt: 1.0 / 60.0,
                time: Some(self.time),
                focused: true,
                ..Default::default()
            };
            let Self {
                app,
                ctx,
                frame,
                causes,
                ..
            } = self;
            let output = ctx.run(input, |ctx| {
                *causes = ctx
                    .repaint_causes()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                eframe::App::update(app, ctx, frame)
            });
            output
                .viewport_output
                .values()
                .map(|viewport| viewport.repaint_delay)
                .min()
                .unwrap_or(Duration::MAX)
        }

        fn builds(&self) -> usize {
            self.app.ui.library.shown_builds()
        }
    }

    #[test]
    fn an_idle_games_tab_never_rebuilds_its_list() {
        for layout in [LayoutMode::Desktop, LayoutMode::Mobile] {
            let mut shelf = Shelf::new("idle-shelf", layout);
            assert_eq!(shelf.app.ui.library.len(), GAMES);
            for _ in 0..5 {
                shelf.step();
            }
            let built = shelf.builds();
            assert!(built > 0, "{layout:?} never showed the games");
            for _ in 0..5 {
                shelf.step();
            }
            assert_eq!(
                shelf.builds(),
                built,
                "{layout:?} rebuilt an unchanged list"
            );
        }
    }

    #[test]
    fn a_new_search_rebuilds_the_list_once() {
        let mut shelf = Shelf::new("search-shelf", LayoutMode::Desktop);
        for _ in 0..5 {
            shelf.step();
        }
        let built = shelf.builds();
        shelf.app.ui.games.search = "Game 12".to_owned();
        for _ in 0..5 {
            shelf.step();
        }
        assert_eq!(shelf.builds(), built + 1);
    }

    fn idle_updates_in_a_second(name: &str, games: usize) -> (usize, Vec<(Duration, Vec<String>)>) {
        let mut shelf = Shelf::with_games(name, LayoutMode::Desktop, games);
        for _ in 0..30 {
            shelf.step();
        }
        for _ in 0..600 {
            if shelf.app.toasts.texts().is_empty() {
                break;
            }
            shelf.step();
        }
        assert_eq!(shelf.app.toasts.texts(), Vec::<&str>::new());
        let refresh = Duration::from_secs_f64(1.0 / 180.0);
        let mut at = Duration::ZERO;
        let mut wait = refresh;
        let mut early = Vec::new();
        let mut updates = 0;
        while at < Duration::from_secs(1) {
            let next = shelf.step_after(wait).max(refresh);
            if wait < Duration::from_millis(20) {
                early.push((wait, shelf.causes.clone()));
            }
            wait = next;
            at += wait;
            updates += 1;
        }
        (updates, early)
    }

    #[test]
    fn an_idle_games_tab_with_many_games_redraws_no_more_than_an_empty_one() {
        let (empty, _) = idle_updates_in_a_second("idle-shelf-empty", 0);
        let (full, early) = idle_updates_in_a_second("idle-shelf-repaint", GAMES);
        assert!(
            full <= empty + 2 && full <= 40,
            "{full} updates a second with {GAMES} games, {empty} with none, early wakes {early:?}"
        );
    }
}
