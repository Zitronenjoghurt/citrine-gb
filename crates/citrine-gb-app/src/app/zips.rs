use super::Citrine;
use super::{tabs, widgets};
use crate::media::format_size;
use crate::utils::file_loader::PickedFile;
use crate::utils::zip_games::{self, Step, Unzip, ZipError, ZipGame};
use egui::Context;
use std::sync::Arc;
use web_time::Duration;

const UNZIP_LOAD: &str = "unzip";
const FRAME_BUDGET: Duration = Duration::from_millis(8);

#[cfg(test)]
thread_local! {
    static FRAME_BYTES: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

fn unpack_frame(unzip: Unzip) -> Step {
    #[cfg(test)]
    if let Some(bytes) = FRAME_BYTES.get() {
        return unzip.step(bytes);
    }
    unzip.run_for(FRAME_BUDGET)
}

pub(super) enum RomFor {
    Load,
    Link,
    Play(tabs::playing::PickFor),
    Hand,
}

pub(super) struct ZipOpen {
    name: String,
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
    seen: Option<crate::rom_ids::Seen>,
    to: RomFor,
    stage: Stage,
}

enum Stage {
    Choose(Arc<[u8]>, Vec<ZipGame>),
    Unpack(Unzip, ZipGame),
}

impl Citrine {
    pub(super) fn take_rom(&mut self, file: PickedFile, to: RomFor) {
        if zip_games::is_zip(&file.name, &file.data) {
            self.open_zip(file, to, None);
            return;
        }
        match to {
            RomFor::Load => self.handle_load_rom(file),
            RomFor::Link => self.link_picked(file),
            RomFor::Play(target) => self.picked_for(target, |app| app.handle_load_rom(file)),
            RomFor::Hand => self.hand_picked(file),
        }
    }

    pub(super) fn open_zip(&mut self, file: PickedFile, to: RomFor, entry: Option<&str>) {
        let games = match zip_games::games(&file.data) {
            Ok(games) => games,
            Err(err) => return self.zip_failed(&file.name, err),
        };
        let chosen = match (
            entry.and_then(|entry| zip_games::find(&games, entry)),
            &games[..],
        ) {
            (Some(game), _) | (None, [game]) => Some(game.clone()),
            (None, []) => {
                self.toasts.error(format!("No game in '{}'", file.name));
                return;
            }
            (None, _) => None,
        };
        let open = ZipOpen {
            name: file.name,
            #[cfg(not(target_arch = "wasm32"))]
            path: file.path,
            seen: file.seen,
            to,
            stage: Stage::Choose(Arc::default(), Vec::new()),
        };
        self.toasts.load_done(UNZIP_LOAD);
        match chosen {
            Some(game) => self.unpack(open, file.data, game),
            None => {
                self.zip = Some(ZipOpen {
                    stage: Stage::Choose(file.data, games),
                    ..open
                })
            }
        }
    }

    fn unpack(&mut self, open: ZipOpen, data: Arc<[u8]>, game: ZipGame) {
        match Unzip::new(data, &game) {
            Ok(unzip) => {
                self.zip = Some(ZipOpen {
                    stage: Stage::Unpack(unzip, game),
                    ..open
                })
            }
            Err(err) => self.zip_failed(&open.name, err),
        }
    }

    fn zip_failed(&mut self, name: &str, err: ZipError) {
        self.toasts.load_done(UNZIP_LOAD);
        self.toasts.error(format!("Could not open '{name}': {err}"));
    }

    pub(super) fn zip_frame(&mut self, ctx: &Context) {
        let Some(open) = self.zip.take() else {
            return;
        };
        match open.stage {
            Stage::Choose(data, games) => {
                let options: Vec<(String, String)> = games
                    .iter()
                    .map(|game| (game.file_name().to_owned(), format_size(game.size)))
                    .collect();
                let text = format!("Which game in '{}'?", open.name);
                match widgets::confirm::pick(ctx, "zip_pick", &text, &options) {
                    None => {
                        self.zip = Some(ZipOpen {
                            stage: Stage::Choose(data, games),
                            ..open
                        })
                    }
                    Some(None) => {}
                    Some(Some(index)) => {
                        let game = games[index].clone();
                        let open = ZipOpen {
                            stage: Stage::Choose(Arc::default(), Vec::new()),
                            ..open
                        };
                        self.unpack(open, data, game);
                        ctx.request_repaint();
                    }
                }
            }
            Stage::Unpack(..) if self.toasts.dismissed(UNZIP_LOAD) => {
                self.toasts.load_done(UNZIP_LOAD);
            }
            Stage::Unpack(unzip, game) => match unpack_frame(unzip) {
                Step::Working(unzip) => {
                    let now = web_time::Instant::now();
                    let text = format!("Unpacking '{}'", game.file_name());
                    self.toasts.load(UNZIP_LOAD, text, unzip.progress(), now);
                    self.zip = Some(ZipOpen {
                        stage: Stage::Unpack(unzip, game),
                        ..open
                    });
                    ctx.request_repaint();
                }
                Step::Failed(err) => self.zip_failed(&open.name, err),
                Step::Done(data) => {
                    self.toasts.load_done(UNZIP_LOAD);
                    let file = PickedFile {
                        name: game.file_name().to_owned(),
                        data,
                        #[cfg(not(target_arch = "wasm32"))]
                        path: open.path.map(|zip| zip.join(&game.entry)),
                        seen: open.seen.map(|seen| seen.inside(&game.entry)),
                    };
                    self.take_rom(file, open.to);
                }
            },
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_recent_zip(&mut self, path: &std::path::Path) -> bool {
        let Some((zip, entry)) = zip_games::split_entry(path) else {
            return false;
        };
        match crate::rom_ids::read_stamped(&zip) {
            Ok((data, stamp)) => {
                let name = zip
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let file = PickedFile {
                    name,
                    data,
                    seen: stamp.map(|stamp| crate::rom_ids::Seen::at(&zip, stamp)),
                    path: Some(zip),
                };
                self.open_zip(file, RomFor::Load, Some(&entry));
            }
            Err(err) => {
                self.toasts
                    .error(format!("Could not open {}: {err}", zip.display()));
                self.ui.recent.remove(path);
            }
        }
        true
    }

    pub(super) fn hand_picked(&mut self, file: PickedFile) {
        let Some(session) = self.play.session_mut() else {
            return;
        };
        let title = tabs::playing::game_title(session);
        if !session.hand_rom(file.data) {
            self.toasts.error(format!("That file isn't {title}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Citrine;
    use crate::app::tests::{phone_app, text_rect};
    use crate::utils::file_loader::PickedFile;
    use crate::utils::zip_games::tests::zip_of;
    use citrine_gb::console::ConsoleKind;
    use egui::Context;
    use std::path::{Path, PathBuf};
    use zip::CompressionMethod;

    fn homebrew(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("homebrew")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    struct Zipped {
        app: Citrine,
        ctx: Context,
        dir: tempfile::TempDir,
        size: egui::Vec2,
    }

    impl Zipped {
        fn new(name: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mut app = phone_app(&format!("zip-{name}"));
            app.settings_store = crate::storage::SettingsStore::at(dir.path().join("settings"));
            app.files.rom_copies =
                crate::app::ui_state::recent::copies::RomCopies::at(dir.path().join("copies"));
            Self {
                app,
                ctx: Context::default(),
                dir,
                size: egui::vec2(412.0, 915.0),
            }
        }

        fn desktop(name: &str) -> Self {
            let mut zipped = Self::new(name);
            zipped.app.ui.settings.layout = crate::app::ui_state::settings::LayoutMode::Desktop;
            zipped.size = egui::vec2(1280.0, 800.0);
            zipped
        }

        fn write(&self, name: &str, data: &[u8]) -> PathBuf {
            let path = self.dir.path().join(name);
            std::fs::write(&path, data).unwrap();
            path
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            self.frame_dt(events, 1.0 / 60.0)
        }

        fn frame_dt(&mut self, events: Vec<egui::Event>, dt: f32) -> egui::FullOutput {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                predicted_dt: dt,
                ..Default::default()
            };
            let app = &mut self.app;
            self.ctx.run(input, |ctx| {
                app.render(ctx);
                app.zip_frame(ctx);
                app.show_toasts(ctx);
            })
        }

        fn pick(&mut self, path: &Path) {
            self.start_pick(path);
            self.settle();
        }

        fn start_pick(&mut self, path: &Path) {
            let file = PickedFile {
                name: path.file_name().unwrap().to_string_lossy().into_owned(),
                data: std::fs::read(path).unwrap().into(),
                path: Some(path.to_path_buf()),
                seen: None,
            };
            self.app.files.rom_tx.send(file).unwrap();
            self.app.drain_file_channels();
        }

        fn drop_file(&mut self, path: &Path) {
            let input = egui::RawInput {
                dropped_files: vec![egui::DroppedFile {
                    path: Some(path.to_path_buf()),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let app = &mut self.app;
            let _ = self.ctx.run(input, |ctx| app.handle_dropped_files(ctx));
            self.settle();
        }

        fn settle(&mut self) {
            for _ in 0..10 {
                self.frame(Vec::new());
            }
        }

        fn opened(&self) -> (ConsoleKind, String) {
            assert!(self.app.emulator.running, "the game runs");
            (
                self.app.emulator.kind(),
                self.app
                    .emulator
                    .rom_key()
                    .expect("a game is open")
                    .to_owned(),
            )
        }

        fn reset(&mut self) {
            self.app.emulator.unload();
            self.app.toasts = Default::default();
        }
    }

    fn tap(at: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    fn opens_like_the_bare_file(name: &str, data: Vec<u8>) -> (ConsoleKind, Zipped) {
        let mut zipped = Zipped::new(name);
        let bare = zipped.write(name, &data);
        zipped.pick(&bare);
        let bare = zipped.opened();
        zipped.reset();

        let zip = zip_of(&[(name, &data)], CompressionMethod::Deflated);
        let zip = zipped.write("game.zip", &zip);
        zipped.pick(&zip);
        assert_eq!(zipped.opened(), bare);
        assert!(
            zipped
                .app
                .toasts
                .texts()
                .contains(&format!("Loaded ROM '{name}'").as_str()),
            "{:?}",
            zipped.app.toasts.texts()
        );
        let recent = &zipped.app.ui.recent.entries()[0];
        assert_eq!(recent.path, zip.join(name));
        assert_eq!(recent.console, Some(bare.0));
        (bare.0, zipped)
    }

    #[test]
    fn a_gb_game_opens_from_a_zip() {
        let (kind, zipped) = opens_like_the_bare_file("snake.gb", homebrew("snake.gb"));
        assert!(matches!(kind, ConsoleKind::Gb | ConsoleKind::Gbc));
        assert!(zipped.app.emulator.as_gb().is_some());
    }

    #[test]
    fn a_gbc_game_opens_from_a_zip() {
        let (kind, _) = opens_like_the_bare_file("petris.gbc", homebrew("petris.gbc"));
        assert_eq!(kind, ConsoleKind::Gbc);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_game_opens_from_a_zip() {
        let data = crate::app::files::tests::fixture();
        let (kind, _) = opens_like_the_bare_file("save-sram.gba", data);
        assert_eq!(kind, ConsoleKind::Gba);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn a_ds_game_opens_from_a_zip() {
        let data = crate::app::files::nds_tests::fixture();
        let (kind, _) = opens_like_the_bare_file("blocksds-console-both-screens.nds", data);
        assert_eq!(kind, ConsoleKind::Nds);
    }

    #[test]
    fn a_dropped_gb_homebrew_zip_opens() {
        let mut zipped = Zipped::new("drop");
        let zip = zip_of(
            &[("snake.gb", &homebrew("snake.gb"))],
            CompressionMethod::Deflated,
        );
        let zip = zipped.write("Snake.zip", &zip);
        zipped.drop_file(&zip);
        assert!(zipped.app.emulator.as_gb().is_some());
        assert!(zipped.app.emulator.running);
        assert_eq!(zipped.app.ui.recent.entries()[0].path, zip.join("snake.gb"));
    }

    fn several(zipped: &mut Zipped) -> PathBuf {
        let zip = zip_of(
            &[
                ("snake.gb", &homebrew("snake.gb")),
                ("readme.txt", b"two games"),
                ("color/petris.gbc", &homebrew("petris.gbc")),
            ],
            CompressionMethod::Deflated,
        );
        zipped.write("pack.zip", &zip)
    }

    #[test]
    fn a_zip_with_several_games_asks_which_one() {
        let mut zipped = Zipped::new("several");
        let zip = several(&mut zipped);
        zipped.pick(&zip);
        assert!(zipped.app.emulator.rom_key().is_none(), "nothing opens yet");
        let output = zipped.frame(Vec::new());
        assert!(text_rect(&output, "Which game in 'pack.zip'?").is_some());
        assert!(text_rect(&output, "snake.gb").is_some());
        assert!(text_rect(&output, "readme.txt").is_none());
        let petris = text_rect(&output, "petris.gbc").expect("petris in the list");

        zipped.frame(tap(petris.center()));
        zipped.settle();
        assert_eq!(zipped.opened().0, ConsoleKind::Gbc);
        assert_eq!(
            zipped.app.ui.recent.entries()[0].path,
            zip.join("color/petris.gbc")
        );
    }

    #[test]
    fn cancelling_the_list_opens_nothing() {
        let mut zipped = Zipped::new("cancel");
        let zip = several(&mut zipped);
        zipped.pick(&zip);
        let output = zipped.frame(Vec::new());
        let cancel = text_rect(&output, "Cancel").expect("a cancel button");
        zipped.frame(tap(cancel.center()));
        zipped.settle();
        assert!(zipped.app.zip.is_none());
        assert!(zipped.app.emulator.rom_key().is_none());
    }

    #[test]
    fn a_tap_outside_the_phone_list_closes_it() {
        let mut zipped = Zipped::new("outside");
        let zip = several(&mut zipped);
        zipped.pick(&zip);
        let output = zipped.frame(Vec::new());
        let question = text_rect(&output, "Which game in 'pack.zip'?").unwrap();
        assert!(question.top() > 200.0, "the sheet leaves room above it");
        zipped.frame(tap(egui::pos2(206.0, 100.0)));
        zipped.settle();
        assert!(zipped.app.zip.is_none());
        assert!(zipped.app.emulator.rom_key().is_none());
    }

    #[test]
    fn escape_closes_the_desktop_list() {
        let mut zipped = Zipped::desktop("escape");
        let zip = several(&mut zipped);
        zipped.pick(&zip);
        let output = zipped.frame(Vec::new());
        assert!(text_rect(&output, "Which game in 'pack.zip'?").is_some());
        zipped.frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]);
        zipped.settle();
        assert!(zipped.app.zip.is_none());
        assert!(zipped.app.emulator.rom_key().is_none());
    }

    #[test]
    fn closing_the_unpacking_toast_stops_the_unpack() {
        let mut zipped = Zipped::new("stop");
        super::FRAME_BYTES.set(Some(1 << 20));
        let pattern: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let big = pattern.repeat(32 * 1024);
        let zip = zip_of(&[("big.gba", &big)], CompressionMethod::Deflated);
        let zip = zipped.write("big.zip", &zip);
        zipped.start_pick(&zip);
        zipped.frame(Vec::new());
        assert!(zipped.app.zip.is_some(), "still unpacking");
        std::thread::sleep(std::time::Duration::from_millis(450));
        let mut close = None;
        for _ in 0..10 {
            let output = zipped.frame_dt(Vec::new(), 0.1);
            close = text_rect(&output, crate::icons::X);
            if close.is_some() {
                break;
            }
        }
        let close = close.expect("the unpacking toast has a close button");
        assert!(
            zipped.app.zip.is_some(),
            "still unpacking when the toast shows"
        );
        zipped.frame(tap(close.center()));
        zipped.settle();
        assert!(zipped.app.zip.is_none());
        assert!(zipped.app.toasts.loads().is_empty());
        for _ in 0..200 {
            zipped.frame(Vec::new());
        }
        assert!(zipped.app.emulator.rom_key().is_none(), "nothing loads");
    }

    #[test]
    fn a_zip_game_reopens_from_the_recent_list_without_asking() {
        let mut zipped = Zipped::new("recent");
        let zip = several(&mut zipped);
        zipped.pick(&zip);
        let output = zipped.frame(Vec::new());
        let petris = text_rect(&output, "petris.gbc").unwrap();
        zipped.frame(tap(petris.center()));
        zipped.settle();
        let opened = zipped.opened();
        zipped.reset();

        let path = zipped.app.ui.recent.entries()[0].path.clone();
        zipped.app.load_recent(path);
        zipped.settle();
        assert_eq!(zipped.opened(), opened);
        assert_eq!(zipped.app.ui.recent.entries().len(), 1);
    }

    fn zipped_petris(name: &str) -> (Zipped, PathBuf, PathBuf) {
        let mut zipped = Zipped::new(name);
        let data = homebrew("petris.gbc");
        let zip = zip_of(&[("petris.gbc", &data)], CompressionMethod::Deflated);
        let zip = zipped.write("petris.zip", &zip);
        zipped.pick(&zip);
        let path = zipped.app.ui.recent.entries()[0].path.clone();
        assert_eq!(path, zip.join("petris.gbc"));
        (zipped, zip, path)
    }

    #[test]
    fn a_zip_game_stays_in_the_recent_list_and_opens_from_it() {
        let (mut zipped, _, path) = zipped_petris("stays");
        let opened = zipped.opened();
        zipped.reset();

        assert_eq!(zipped.app.ui.recent.prune(), 0);
        assert_eq!(zipped.app.ui.recent.entries().len(), 1);
        assert!(!zipped.app.files.rom_copies.is_gone(&path));
        zipped.app.load_recent(path.clone());
        zipped.settle();
        assert_eq!(zipped.opened(), opened);
        assert_eq!(zipped.app.ui.recent.entries()[0].path, path);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_zipped_gba_game_keeps_its_ids_for_the_next_load() {
        use crate::rom_ids::{RomIds, Seen, Stamp};
        use std::time::{Duration, SystemTime};
        let mut zipped = Zipped::new("ids");
        let store = zipped.dir.path().join("rom-ids.json");
        zipped.app.rom_ids = RomIds::at(store.clone());
        let data = crate::app::files::tests::fixture();
        let zip = zip_of(
            &[("save-sram.gba", &data), ("other.gba", b"other")],
            CompressionMethod::Deflated,
        );
        let zip = zipped.write("pack.zip", &zip);
        let age = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&zip)
            .unwrap()
            .set_modified(age)
            .unwrap();
        let game = zip.join("save-sram.gba");
        zipped.app.load_recent(game.clone());
        zipped.settle();
        zipped.app.threads.run_side_jobs();
        let ids = citrine_gb::rom::id::RomId::with_game_sha1(&data);
        assert_eq!(zipped.opened().1, ids.0.sha256_hex_string());

        let stamp = Stamp::of(&zip).unwrap();
        let mut kept = RomIds::at(store);
        assert_eq!(kept.known(&Seen::at(&game, stamp)), Some(ids));
        assert_eq!(kept.known(&Seen::at(&zip, stamp)), None);
        assert_eq!(kept.known(&Seen::at(zip.join("other.gba"), stamp)), None);
    }

    #[test]
    fn a_deleted_zip_leaves_the_recent_list() {
        let (mut zipped, zip, path) = zipped_petris("deleted");
        zipped.reset();
        std::fs::remove_file(&zip).unwrap();

        assert!(zipped.app.files.rom_copies.is_gone(&path));
        assert_eq!(zipped.app.ui.recent.prune(), 1);
        assert!(zipped.app.ui.recent.is_empty());
    }

    #[test]
    fn a_zip_without_a_path_keeps_the_game_inside_for_the_recent_list() {
        let mut zipped = Zipped::new("copy");
        let data = homebrew("petris.gbc");
        let zip = zip_of(&[("petris.gbc", &data)], CompressionMethod::Stored);
        let file = PickedFile {
            name: "petris.zip".to_owned(),
            data: zip.into(),
            path: None,
            seen: None,
        };
        zipped.app.files.rom_tx.send(file).unwrap();
        zipped.app.drain_file_channels();
        zipped.settle();
        let opened = zipped.opened();
        let path = zipped.app.ui.recent.entries()[0].path.clone();
        assert_eq!(path.extension().unwrap(), "gbc");
        assert_eq!(std::fs::read(&path).unwrap(), data);
        zipped.reset();

        zipped.app.load_recent(path);
        zipped.settle();
        assert_eq!(zipped.opened(), opened);
    }

    #[test]
    fn a_zip_without_a_game_says_so_in_one_toast() {
        let mut zipped = Zipped::new("none");
        let zip = zip_of(
            &[("notes.txt", b"no games here")],
            CompressionMethod::Deflated,
        );
        let zip = zipped.write("notes.zip", &zip);
        zipped.drop_file(&zip);
        assert_eq!(zipped.app.toasts.texts(), ["No game in 'notes.zip'"]);
        assert!(zipped.app.zip.is_none());
        assert!(zipped.app.emulator.rom_key().is_none());
    }

    #[test]
    fn a_damaged_zip_says_so() {
        let mut zipped = Zipped::new("damaged");
        let zip = zipped.write("broken.zip", b"PK\x03\x04 cut short");
        zipped.drop_file(&zip);
        assert_eq!(
            zipped.app.toasts.texts(),
            ["Could not open 'broken.zip': it is damaged"]
        );
    }

    #[cfg(feature = "gba")]
    #[test]
    fn saves_are_shared_between_the_zip_and_the_bare_file() {
        use citrine_gb::console::Console;
        let data = crate::app::files::tests::fixture();
        let mut zipped = Zipped::new("saves");
        let zip = zip_of(&[("save-sram.gba", &data)], CompressionMethod::Deflated);
        let zip = zipped.write("save-sram.zip", &zip);
        zipped.pick(&zip);
        for _ in 0..60 {
            zipped.app.emulator.debug_mut().run_frame();
        }
        zipped.app.emulator.flush_save().unwrap();
        let written = zipped.app.emulator.export_save_bytes().expect("a save");
        zipped.reset();

        let bare = zipped.write("save-sram.gba", &data);
        zipped.pick(&bare);
        assert!(
            zipped.app.emulator.save_loaded,
            "the bare file finds the save"
        );
        assert_eq!(
            zipped.app.emulator.export_save_bytes(),
            Some(written.clone())
        );
        zipped.reset();

        zipped.pick(&zip);
        assert!(
            zipped.app.emulator.save_loaded,
            "and the zip finds it again"
        );
        assert_eq!(zipped.app.emulator.export_save_bytes(), Some(written));
    }

    #[cfg(feature = "nds")]
    #[test]
    #[ignore = "needs a zipped DS game named in CITRINE_DS_ZIP"]
    fn a_real_zipped_ds_game_draws_both_screens_past_its_intro() {
        let path = PathBuf::from(std::env::var("CITRINE_DS_ZIP").expect("CITRINE_DS_ZIP"));
        let shots = std::env::var("CITRINE_DS_SHOTS").ok();
        let mut zipped = Zipped::desktop("real-ds");
        zipped.pick(&path);
        let mut frames = 0;
        while zipped.app.zip.is_some() && frames < 20_000 {
            zipped.frame(Vec::new());
            frames += 1;
        }
        println!("unzipped in {frames} frames");
        assert_eq!(zipped.app.emulator.kind(), ConsoleKind::Nds);
        assert!(
            zipped
                .app
                .toasts
                .texts()
                .iter()
                .all(|text| text.starts_with("Unpacking") || text.starts_with("Loaded ROM")),
            "{:?}",
            zipped.app.toasts.texts()
        );
        let start = citrine_gb::console::Keys::START;
        let keyboard = crate::held::InputSource::Keyboard;
        let mut screens = [0, 0];
        for frame in 1..=2700 {
            if frame == 2100 {
                zipped.app.emulator.press(keyboard, start);
            }
            if frame == 2106 {
                zipped.app.emulator.release(keyboard, start);
            }
            assert_eq!(zipped.app.emulator.lend_one_frame(), 1, "frame {frame}");
            if frame % 300 != 0 {
                continue;
            }
            let (painted, [width, height]) = zipped.app.emulator.painted();
            let half = painted.len() / 2;
            for (screen, pixels) in [&painted[..half], &painted[half..]].iter().enumerate() {
                let colours: std::collections::HashSet<&[u8]> = pixels.chunks(4).collect();
                screens[screen] = colours.len();
            }
            println!("frame {frame} {width}x{height} colours {screens:?}");
            if let Some(dir) = &shots {
                image::save_buffer(
                    Path::new(dir).join(format!("ds-zip-{frame:05}.png")),
                    painted,
                    width as u32,
                    height as u32,
                    image::ColorType::Rgba8,
                )
                .expect("picture");
            }
        }
        assert!(screens.iter().all(|&colours| colours > 16), "{screens:?}");
    }
}
