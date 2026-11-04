use super::{Citrine, PendingImport, QUICK_LOAD_HOLD};
use super::{tabs, widgets};
use crate::app::ui_state::settings::plays_on_both;
use crate::emulator::SaveImport;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::utils::save_files::{FileKind, SaveFiles};
use citrine_gb::console::{ConsoleKind, detect};
use citrine_gb::rom::Rom;
use egui::Context;

impl Citrine {
    pub(super) fn drain_file_channels(&mut self) {
        while let Ok(file) = self.files.rom_rx.try_recv() {
            self.handle_load_rom(file);
        }
        while let Ok(file) = self.files.play_rom_rx.try_recv() {
            if std::mem::take(&mut self.ui.playing.copy_for) {
                let Some(session) = self.play.session_mut() else {
                    continue;
                };
                let title = tabs::playing::game_title(session);
                if !session.hand_rom(&file.data) {
                    self.toasts.error(format!("That file isn't {title}"));
                }
                continue;
            }
            match self.ui.playing.file_for.take() {
                Some(target) => self.picked_for(target, |app| app.handle_load_rom(file)),
                None => self.handle_load_rom(file),
            }
        }
        while let Ok(file) = self.files.second_rom_rx.try_recv() {
            self.link_picked(file);
        }
        while let Ok(file) = self.files.boot_rom_rx.try_recv() {
            self.handle_load_boot_rom(file);
        }
        #[cfg(feature = "gba")]
        while let Ok(file) = self.files.gba_bios_rx.try_recv() {
            self.handle_load_gba_bios(file);
        }
        while let Ok(file) = self.files.picture_rx.try_recv() {
            self.picture_picked(file);
        }
        while let Ok(files) = self.files.sav_rx.try_recv() {
            self.handle_import_saves(files);
        }
        #[cfg(not(target_arch = "wasm32"))]
        while let Ok(dir) = self.files.folder_rx.try_recv() {
            self.handle_export_e2e(&dir);
        }
        while let Ok(outcome) = self.files.save_rx.try_recv() {
            match outcome {
                SaveOutcome::Saved(name) => self.toasts.success(format!("Saved to '{name}'")),
                SaveOutcome::SavedPair(first, second) => self
                    .toasts
                    .success(format!("Saved to '{first}' and '{second}'")),
                SaveOutcome::Failed(err) => self.toasts.error(format!("Failed to save: {err}")),
                SaveOutcome::Cancelled => continue,
            };
        }
        while let Ok(outcome) = self.files.copy_rx.try_recv() {
            match outcome {
                Ok(done) if !done.is_empty() => self.toasts.success(done),
                Err(err) if !err.is_empty() => self.toasts.error(format!("Could not copy: {err}")),
                _ => continue,
            };
        }
    }

    pub(super) fn name_games(&mut self) {
        if let Some(key) = self.emulator.rom_key()
            && self
                .ui
                .settings
                .game_names
                .learn(key, self.emulator.game_name())
        {
            self.ui.settings.dirty = true;
        }
        self.media.name_games(&self.ui.settings.game_names);
        if self.online.cloud_enabled() {
            let names = &self.ui.settings.game_names;
            for recent in self.ui.recent.entries() {
                if let Some(key) = &recent.rom_key {
                    let name = recent.display_name();
                    self.online.cloud_name(key, names.name_or(Some(key), &name));
                }
            }
            for (key, name) in names.iter() {
                self.online.cloud_name(key, name);
            }
        }
    }

    pub(super) fn quick_load_overlay(&self, ctx: &Context) {
        let Some(started) = self.quick_load_hold else {
            return;
        };
        let progress =
            (started.elapsed().as_secs_f32() / QUICK_LOAD_HOLD.as_secs_f32()).clamp(0.0, 1.0);
        let name = self.emulator.snapshot_name(self.ui.settings.quick_slot);

        egui::Area::new("quick_load_hold".into())
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(220.0);
                    ui.label(format!("Hold to load '{name}'"));
                    ui.add(egui::ProgressBar::new(progress).desired_height(6.0));
                });
            });
    }

    pub(super) fn opened_as(&self, rom: &Rom, name: Option<&str>) -> ConsoleKind {
        match detect(&rom.data, name) {
            Some(ConsoleKind::Gba) => ConsoleKind::Gba,
            _ => self.ui.settings.console_for_rom(rom).into(),
        }
    }

    pub(super) fn handle_load_rom(&mut self, file: PickedFile) {
        let rom = Rom::new(&file.data);
        let kind = self.opened_as(&rom, Some(&file.name));
        self.try_start_audio();
        if let Err(err) = self.emulator.load_game(
            &rom,
            kind,
            #[cfg(not(target_arch = "wasm32"))]
            file.path.as_deref(),
        ) {
            self.toasts.error(format!("Failed to load ROM: {}", err));
        } else {
            self.emulator.name_game(Some(&file.name));
            self.toasts.success(format!("Loaded ROM '{}'", file.name));
            self.ui.settings.dirty = true;
            self.close_mobile_sheet();

            self.remember_rom(&file);

            if self.emulator.imported_legacy_save {
                self.toasts
                    .success("Imported the save file found next to the ROM");
            } else if self.emulator.save_loaded {
                self.toasts.success("Loaded save data");
            }
        }
    }

    fn remember_rom(&mut self, file: &PickedFile) {
        let copies = &self.files.rom_copies;
        let Some(sha) = self.emulator.rom_key().map(str::to_owned) else {
            return;
        };
        let both = self
            .emulator
            .as_gb()
            .is_some_and(|gb| plays_on_both(gb.cartridge.header.cgb_mode));
        #[cfg(not(target_arch = "wasm32"))]
        let picked = file.path.clone();
        #[cfg(target_arch = "wasm32")]
        let picked: Option<std::path::PathBuf> = None;
        let path = match picked {
            Some(path) => path,
            None => match copies.keep(&sha, &file.name, &file.data) {
                Ok(path) => path,
                Err(err) => {
                    log::warn!("couldn't keep a copy of {}: {err}", file.name);
                    return;
                }
            },
        };
        let title = self.emulator.game_name().to_string();
        let console = self.emulator.kind();
        let key = both.then_some(sha);
        let dropped = self.ui.recent.record_on(&path, &title, key, Some(console));
        copies.drop_copies(dropped.iter().map(|entry| entry.path.as_path()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_recent(&mut self, path: std::path::PathBuf) {
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(err) => {
                self.toasts
                    .error(format!("Could not open {}: {err}", path.display()));
                self.ui.recent.remove(&path);
                return;
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.handle_load_rom(PickedFile {
            name,
            data,
            path: Some(path),
        });
    }

    pub(super) fn handle_import_saves(&mut self, files: Vec<PickedFile>) {
        let sorted = match crate::utils::save_files::sort(files) {
            Ok(sorted) => sorted,
            Err(err) => {
                self.toasts.error(err);
                return;
            }
        };
        let blocked = self.emulator.import_blocker(sorted.clock.is_some());
        if blocked.is_none() && self.emulator.has_been_playing() {
            self.pending_import = Some(PendingImport {
                files: sorted,
                rom_key: self.emulator.rom_key().map(str::to_owned),
                was_running: self
                    .pending_import
                    .as_ref()
                    .map_or(self.emulator.running, |pending| pending.was_running),
            });
            self.emulator.running = false;
            self.emulator.release_all();
            return;
        }
        self.import_saves(sorted);
    }

    pub(super) fn import_confirm(&mut self, ctx: &Context) {
        let Some(pending) = &self.pending_import else {
            return;
        };
        if pending.rom_key.as_deref() != self.emulator.rom_key() {
            self.pending_import = None;
            return;
        }
        let choice = widgets::confirm::confirm(
            ctx,
            "import_confirm",
            "Restart the game with this save? Anything not saved in the game is lost.",
            "Import",
        );
        let Some(confirmed) = choice else {
            return;
        };
        let Some(pending) = self.pending_import.take() else {
            return;
        };
        if confirmed {
            self.import_saves(pending.files);
        } else {
            self.emulator.running = pending.was_running;
        }
    }

    pub(super) fn import_saves(&mut self, sorted: SaveFiles) {
        let save_name = sorted.save.as_ref().map(|file| file.name.clone());
        let (clock_name, clock) = sorted.clock.unzip();
        let result = self
            .emulator
            .import_save(sorted.save.as_ref().map(|file| file.data.as_slice()), clock);
        if let Ok(SaveImport::Imported) = result {
            self.emulator.restart();
        }
        match (result, save_name, clock_name) {
            (Ok(SaveImport::Imported), Some(save), Some(clock)) => {
                self.toasts
                    .success(format!("Restarted with '{save}' and '{clock}'"));
            }
            (Ok(SaveImport::Imported), Some(save), None) => {
                self.toasts.success(format!("Restarted with '{save}'"));
            }
            (Ok(SaveImport::Imported), None, Some(clock)) => {
                self.toasts
                    .success(format!("Restarted with the clock from '{clock}'"));
            }
            (Ok(SaveImport::NoClock), _, Some(clock)) => {
                self.toasts.error(format!(
                    "This game has no clock, so '{clock}' can't be used"
                ));
            }
            (Ok(SaveImport::NoRom), _, _) => {
                self.toasts.error("Load a ROM before importing a save");
            }
            (Ok(_), _, _) => {}
            (Err(err), _, _) => {
                self.toasts.error(format!("Failed to import save: {err:?}"));
            }
        }
    }

    pub(super) fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        let mut saves = Vec::new();
        for file in dropped {
            let Some(file) = PickedFile::from_drop(file) else {
                continue;
            };
            match crate::utils::save_files::kind_of(&file.name) {
                FileKind::Rom if self.ui.linked.picking => self.link_picked(file),
                FileKind::Rom => self.handle_load_rom(file),
                FileKind::Save | FileKind::Clock => saves.push(file),
                FileKind::Other => {
                    self.toasts
                        .error(format!("'{}' is not a ROM or a save file", file.name));
                }
            }
        }
        if !saves.is_empty() {
            self.handle_import_saves(saves);
        }
    }

    pub(super) fn handle_load_boot_rom(&mut self, file: PickedFile) {
        self.try_start_audio();
        if let Some(gb) = self.emulator.as_gb_mut() {
            gb.load_boot_rom(&file.data);
        }
        self.ui.settings.dirty = true;
        self.toasts.success("Boot ROM loaded");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn handle_export_e2e(&mut self, dir: &std::path::Path) {
        if self.ui.e2e.title.is_empty() {
            self.toasts.error("Please enter a title for the E2E test");
            return;
        }

        let Some(gb) = self.emulator.as_gb() else {
            return;
        };
        let e2e = gb.create_e2e_test(&self.ui.e2e.title, &self.ui.e2e.description);

        if let Err(err) = e2e.export(dir) {
            self.toasts.error(format!("Failed to export E2E: {}", err));
        } else {
            self.toasts
                .success(format!("Exported E2E to '{}'", dir.display()));
        }
    }

    pub(super) fn handle_load_rom_data(&mut self, data: Vec<u8>) {
        let rom = Rom::new(&data);
        let kind = self.opened_as(&rom, None);
        self.try_start_audio();
        #[cfg(not(target_arch = "wasm32"))]
        let result = self.emulator.load_game(&rom, kind, None);
        #[cfg(target_arch = "wasm32")]
        let result = self.emulator.load_game(&rom, kind);

        match result {
            Ok(()) => self.close_mobile_sheet(),
            Err(err) => {
                self.toasts.error(format!("Failed to load ROM: {err}"));
            }
        }
        self.ui.settings.dirty = true;
    }
}

#[cfg(all(test, feature = "gba"))]
pub(crate) mod tests {
    use crate::app::Citrine;
    use crate::app::mobile::nav::Place;
    use crate::app::tests::{phone_app, play_frame, text_rect};
    use crate::utils::file_loader::PickedFile;
    use citrine_gb::console::ConsoleKind;
    use egui::Context;

    pub(crate) const GAME: &str = "save-sram.gba";

    pub(crate) fn fixture() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/gba")
            .join(GAME);
        std::fs::read(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    pub(crate) struct Opened {
        pub(crate) app: Citrine,
        pub(crate) dir: tempfile::TempDir,
    }

    impl Opened {
        pub(crate) fn new(name: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mut app = phone_app(&format!("gba-open-{name}"));
            app.settings_store = crate::storage::SettingsStore::at(dir.path().join("settings"));
            Self { app, dir }
        }

        pub(crate) fn rom_file(&self) -> std::path::PathBuf {
            let path = self.dir.path().join(GAME);
            std::fs::write(&path, fixture()).unwrap();
            path
        }

        pub(crate) fn picked(&self) -> PickedFile {
            let path = self.rom_file();
            PickedFile {
                name: GAME.to_owned(),
                data: fixture(),
                path: Some(path),
            }
        }
    }

    fn runs_the_gba(app: &Citrine) {
        assert_eq!(app.emulator.kind(), ConsoleKind::Gba);
        assert!(app.emulator.rom_key().is_some(), "the game is open");
        assert!(app.emulator.running, "the game runs");
        assert_eq!(app.toasts.texts(), [format!("Loaded ROM '{GAME}'")]);
    }

    fn dropped(app: &mut Citrine, path: &std::path::Path) {
        let ctx = Context::default();
        let input = egui::RawInput {
            dropped_files: vec![egui::DroppedFile {
                path: Some(path.to_path_buf()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| app.handle_dropped_files(ctx));
    }

    #[test]
    fn a_gba_game_opens_from_the_file_picker() {
        let mut opened = Opened::new("picker");
        let file = opened.picked();
        opened.app.files.rom_tx.send(file).unwrap();
        opened.app.drain_file_channels();
        runs_the_gba(&opened.app);
    }

    #[test]
    fn a_gba_game_opens_from_the_play_picker() {
        let mut opened = Opened::new("play-picker");
        let file = opened.picked();
        opened.app.files.play_rom_tx.send(file).unwrap();
        opened.app.drain_file_channels();
        runs_the_gba(&opened.app);
    }

    #[test]
    fn a_gba_game_opens_when_dropped() {
        let mut opened = Opened::new("drop");
        let path = opened.rom_file();
        dropped(&mut opened.app, &path);
        runs_the_gba(&opened.app);
    }

    #[test]
    fn a_gba_game_opens_from_its_bytes_alone() {
        let mut opened = Opened::new("bytes");
        opened.app.handle_load_rom_data(fixture());
        assert_eq!(opened.app.emulator.kind(), ConsoleKind::Gba);
        assert!(opened.app.emulator.rom_key().is_some());
    }

    #[test]
    fn a_gba_game_joins_the_recent_list_with_its_console_and_opens_from_it() {
        let mut opened = Opened::new("recent");
        let path = opened.rom_file();
        dropped(&mut opened.app, &path);
        let entry = opened.app.ui.recent.entries()[0].clone();
        assert_eq!(entry.path, path);
        assert_eq!(entry.console, Some(ConsoleKind::Gba));
        assert_eq!(entry.rom_key, None, "a GBA game has no second console");

        opened.app.emulator.unload();
        opened.app.toasts = Default::default();
        opened.app.load_recent(path);
        runs_the_gba(&opened.app);
    }

    #[test]
    fn the_phone_recent_list_names_the_console() {
        let mut opened = Opened::new("recent-phone");
        let path = opened.rom_file();
        dropped(&mut opened.app, &path);
        opened.app.emulator.unload();
        opened.app.mobile.nav.go(Place::Games);
        let ctx = Context::default();
        let mut output = play_frame(&mut opened.app, &ctx, Vec::new());
        for _ in 0..2 {
            output = play_frame(&mut opened.app, &ctx, Vec::new());
        }
        assert!(
            text_rect(&output, "Game Boy Advance, today").is_some(),
            "the recent row's detail"
        );
    }
}
