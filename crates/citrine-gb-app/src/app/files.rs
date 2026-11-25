use super::widgets;
use super::zips::RomFor;
use super::{Citrine, PendingImport, QUICK_LOAD_HOLD};
use crate::app::ui_state::settings::plays_on_both;
use crate::app::widgets::snapshots::{FailedStep, failed_message, named};
use crate::emulator::SaveImport;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::utils::save_files::{FileKind, SaveFiles};
use citrine_gb::console::{Console, ConsoleKind, detect};
use citrine_gb::rom::Rom;
use citrine_gb::rom::id::RomId;
use egui::Context;

impl Citrine {
    pub(super) fn drain_file_channels(&mut self) {
        while let Ok(file) = self.files.rom_rx.try_recv() {
            self.take_rom(file, RomFor::Load);
        }
        while let Ok(file) = self.files.play_rom_rx.try_recv() {
            let to = if std::mem::take(&mut self.ui.playing.copy_for) {
                RomFor::Hand
            } else {
                self.ui
                    .playing
                    .file_for
                    .take()
                    .map_or(RomFor::Load, RomFor::Play)
            };
            self.take_rom(file, to);
        }
        while let Ok(file) = self.files.second_rom_rx.try_recv() {
            self.take_rom(file, RomFor::Link);
        }
        while let Ok((file, picked)) = self.files.boot_rom_rx.try_recv() {
            self.handle_load_boot_rom(file, picked);
        }
        #[cfg(feature = "gba")]
        while let Ok(file) = self.files.gba_bios_rx.try_recv() {
            self.handle_load_gba_bios(file);
        }
        #[cfg(feature = "nds")]
        while let Ok((file, picked)) = self.files.ds_file_rx.try_recv() {
            self.handle_load_ds_file(file, picked);
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
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        while let Ok(dir) = self.files.capture_folder_rx.try_recv() {
            self.ui.settings.capture_folder = Some(dir.to_string_lossy().into_owned());
            self.ui.settings.dirty = true;
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
        while let Ok(message) = self.files.error_rx.try_recv() {
            self.toasts.error(message);
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
            Some(kind @ (ConsoleKind::Gba | ConsoleKind::Nds)) => kind,
            _ => self.ui.settings.console_for_rom(rom).into(),
        }
    }

    fn with_rom_ids(&mut self, rom: Rom, kind: ConsoleKind, file: Option<&PickedFile>) -> Rom {
        let digested = self
            .digested
            .take()
            .filter(|(data, _)| std::sync::Arc::ptr_eq(data, &rom.data))
            .map(|(_, ids)| ids);
        if !matches!(kind, ConsoleKind::Gba | ConsoleKind::Nds) {
            return rom;
        }
        let seen = file.and_then(|file| file.seen.as_ref());
        if let Some(seen) = seen
            && let Some(ids) = self.rom_ids.known(seen)
        {
            return rom.with_ids(ids);
        }
        let ids =
            digested.unwrap_or_else(|| RomId::with_game_sha1_on(&rom.data, Some(&self.threads)));
        if let Some(seen) = seen {
            self.rom_ids.learn(seen, ids, &self.threads.spawner());
        }
        rom.with_ids(ids)
    }

    pub(super) fn handle_load_rom(&mut self, file: PickedFile) {
        self.close_replay_show();
        let rom = Rom::from(file.data.clone());
        let kind = self.opened_as(&rom, Some(&file.name));
        let rom = self.with_rom_ids(rom, kind, Some(&file));
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
            #[cfg(feature = "nds")]
            self.ask_for_ds_bios();
            self.ui.settings.dirty = true;
            self.close_mobile_sheet();

            #[cfg(not(target_arch = "wasm32"))]
            self.remember_rom(&file);
            #[cfg(target_arch = "wasm32")]
            if let Some(copy) = self.remember_rom(&file) {
                self.learn_kept(&copy, &rom, kind, file.data.len());
            }

            if self.emulator.imported_legacy_save {
                self.toasts
                    .success("Imported the save file found next to the ROM");
            } else if self.emulator.save_loaded {
                self.toasts.success("Loaded save data");
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn learn_kept(&mut self, copy: &std::path::Path, rom: &Rom, kind: ConsoleKind, len: usize) {
        if !matches!(kind, ConsoleKind::Gba | ConsoleKind::Nds) {
            return;
        }
        if let Some(seen) = crate::rom_ids::Seen::kept(copy, len) {
            self.rom_ids
                .learn(&seen, rom.ids(), &self.threads.spawner());
        }
    }

    fn remember_rom(&mut self, file: &PickedFile) -> Option<std::path::PathBuf> {
        let copies = &self.files.rom_copies;
        let sha = self.emulator.rom_key().map(str::to_owned)?;
        let both = self
            .emulator
            .as_gb()
            .is_some_and(|gb| plays_on_both(gb.cartridge.header.cgb_mode));
        #[cfg(not(target_arch = "wasm32"))]
        let picked = file.path.clone();
        #[cfg(target_arch = "wasm32")]
        let picked: Option<std::path::PathBuf> = None;
        let archived = self
            .ui
            .library
            .archive_path(&sha)
            .map(std::path::PathBuf::from);
        let path = match picked.or(archived) {
            Some(path) => path,
            None => match copies.keep(&sha, &file.name, &file.data) {
                Ok(path) => path,
                Err(err) => {
                    log::warn!("couldn't keep a copy of {}: {err}", file.name);
                    return None;
                }
            },
        };
        let title = self.emulator.game_name().to_string();
        let console = self.emulator.kind();
        let key = both.then_some(sha.clone());
        let dropped = self.ui.recent.record_on(&path, &title, key, Some(console));
        self.ui.recent.set_game_key(&path, sha.clone());
        let rom_id = self.emulator.debug().rom_id();
        let (sha1, crc32) = (rom_id.sha1_hex_string(), rom_id.crc32);
        self.ui.library.record_opened(
            crate::app::ui_state::library::Opened {
                path: path.to_string_lossy().into_owned(),
                name: title.clone(),
                console: Some(console),
                rom_key: Some(sha),
                sha1: Some(sha1),
                crc32: Some(crc32),
            },
            crate::storage::now_unix(),
        );
        let library = &self.ui.library;
        #[cfg(feature = "nds")]
        self.ui.recent.set_icon(
            &path,
            crate::ds_banner::banner(&file.data)
                .map(|banner| crate::ds_banner::icon_text(&banner.icon)),
        );
        copies.drop_copies(
            dropped
                .iter()
                .map(|entry| entry.path.as_path())
                .filter(|path| library.find(&path.to_string_lossy()).is_none()),
        );
        self.want_cover(&file.name, &file.data);
        Some(path)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn load_recent(&mut self, path: std::path::PathBuf) {
        if self.play_archive(&path.to_string_lossy()) {
            return;
        }
        if !path.is_file() && self.load_recent_zip(&path) {
            return;
        }
        match self.open_hashing(path.clone()) {
            Ok(file) => {
                self.take_rom(file, RomFor::Load);
                self.digested = None;
            }
            Err(err) => {
                self.toasts
                    .error(format!("Could not open {}: {err}", path.display()));
                self.ui.recent.remove(&path);
            }
        }
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
            .import_save(sorted.save.as_ref().map(|file| &file.data[..]), clock);
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
            (Ok(SaveImport::ClockInSave), _, Some(_)) => {
                self.toasts
                    .error("GBA clocks are in the .sav. Import the .sav alone");
            }
            (Ok(SaveImport::NoRom), _, _) => {
                self.toasts.error("Load a ROM before importing a save");
            }
            (Ok(_), _, _) => {}
            (Err(err), save, clock) => {
                let what = named(&save.or(clock).unwrap_or_default(), "the save");
                self.toasts
                    .error(failed_message(FailedStep::Import, &what, &err));
            }
        }
    }

    pub(super) fn handle_dropped_files(&mut self, ctx: &Context) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        let mut saves = Vec::new();
        for file in dropped {
            let Some(file) = self.open_dropped(file) else {
                continue;
            };
            match crate::utils::save_files::kind_of(&file.name) {
                FileKind::Rom if self.ui.linked.picking => self.take_rom(file, RomFor::Link),
                FileKind::Rom => self.take_rom(file, RomFor::Load),
                FileKind::Save | FileKind::Clock => saves.push(file),
                FileKind::Other => {
                    self.toasts
                        .error(format!("'{}' is not a ROM or a save file", file.name));
                }
            }
            self.digested = None;
        }
        if !saves.is_empty() {
            self.handle_import_saves(saves);
        }
    }

    fn open_dropped(&mut self, file: egui::DroppedFile) -> Option<PickedFile> {
        self.digested = None;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = file.path.clone()
            && crate::utils::save_files::kind_of(&path.to_string_lossy()) == FileKind::Rom
        {
            return self.open_hashing(path).ok();
        }
        PickedFile::from_drop(file)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn open_hashing(&mut self, path: std::path::PathBuf) -> std::io::Result<PickedFile> {
        let rom_ids = &mut self.rom_ids;
        let (file, ids) =
            PickedFile::open_hashing(path, &self.threads, |seen| rom_ids.known(seen).is_some())?;
        self.digested = ids.map(|ids| (file.data.clone(), ids));
        Ok(file)
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
        self.close_replay_show();
        let rom = Rom::new(&data);
        let kind = self.opened_as(&rom, None);
        let rom = self.with_rom_ids(rom, kind, None);
        self.try_start_audio();
        #[cfg(not(target_arch = "wasm32"))]
        let result = self.emulator.load_game(&rom, kind, None);
        #[cfg(target_arch = "wasm32")]
        let result = self.emulator.load_game(&rom, kind);

        match result {
            Ok(()) => {
                self.close_mobile_sheet();
                #[cfg(feature = "nds")]
                self.ask_for_ds_bios();
            }
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
                data: fixture().into(),
                path: Some(path),
                seen: None,
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
    fn a_dropped_gba_game_runs_on_the_dropped_bytes() {
        use citrine_gb::console::Console;
        let mut opened = Opened::new("drop-bytes");
        let bytes: std::sync::Arc<[u8]> = fixture().into();
        let ctx = Context::default();
        let input = egui::RawInput {
            dropped_files: vec![egui::DroppedFile {
                name: GAME.to_owned(),
                bytes: Some(bytes.clone()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| opened.app.handle_dropped_files(ctx));
        runs_the_gba(&opened.app);
        let running = opened.app.emulator.debug().rom_bytes();
        assert!(std::sync::Arc::ptr_eq(&running, &bytes));
        assert_eq!(&running[..], &fixture()[..]);
        let key = citrine_gb::rom::id::RomId::new(&fixture()).sha256_hex_string();
        assert_eq!(opened.app.emulator.rom_key(), Some(key.as_str()));
    }

    #[test]
    fn a_picked_gba_game_runs_on_the_bytes_read_from_disk() {
        use citrine_gb::console::Console;
        let mut opened = Opened::new("picked-bytes");
        let file = PickedFile::open(opened.rom_file()).unwrap();
        let bytes = file.data.clone();
        assert_eq!(&bytes[..], &fixture()[..]);
        opened.app.handle_load_rom(file);
        runs_the_gba(&opened.app);
        let running = opened.app.emulator.debug().rom_bytes();
        assert!(std::sync::Arc::ptr_eq(&running, &bytes));
        let key = citrine_gb::rom::id::RomId::new(&fixture()).sha256_hex_string();
        assert_eq!(opened.app.emulator.rom_key(), Some(key.as_str()));
    }

    #[test]
    fn a_reopened_gba_game_reuses_its_ids_and_a_changed_one_hashes_again() {
        use std::time::{Duration, SystemTime};
        let mut opened = Opened::new("ids");
        let store = opened.dir.path().join("rom-ids.json");
        opened.app.rom_ids = crate::rom_ids::RomIds::at(store.clone());
        let path = opened.rom_file();
        let age = |path: &std::path::Path, secs: u64| {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(secs))
                .unwrap();
        };
        age(&path, 3600);
        let file = PickedFile::open(path.clone()).unwrap();
        let seen = file.seen.clone().expect("a still file is seen");
        let ids = citrine_gb::rom::id::RomId::with_game_sha1(&fixture());
        opened.app.handle_load_rom(file);
        opened.app.threads.run_side_jobs();
        let key = ids.0.sha256_hex_string();
        assert_eq!(opened.app.emulator.rom_key(), Some(key.as_str()));
        assert_eq!(
            crate::rom_ids::RomIds::at(store.clone()).known(&seen),
            Some(ids)
        );
        opened
            .app
            .handle_load_rom(PickedFile::open(path.clone()).unwrap());
        assert_eq!(opened.app.emulator.rom_key(), Some(key.as_str()));

        let mut changed = fixture();
        *changed.last_mut().unwrap() ^= 0xFF;
        std::fs::write(&path, &changed).unwrap();
        age(&path, 1800);
        opened
            .app
            .handle_load_rom(PickedFile::open(path.clone()).unwrap());
        let changed_key = citrine_gb::rom::id::RomId::new(&changed).sha256_hex_string();
        assert_ne!(changed_key, key);
        assert_eq!(opened.app.emulator.rom_key(), Some(changed_key.as_str()));
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

#[cfg(all(test, feature = "nds"))]
pub(crate) mod nds_tests {
    use crate::app::Citrine;
    use crate::app::mobile::nav::Place;
    use crate::app::tests::{phone_app, play_frame, text_rect};
    use crate::utils::file_loader::PickedFile;
    use citrine_gb::console::ConsoleKind;
    use egui::Context;

    pub(crate) const GAME: &str = "blocksds-console-both-screens.nds";
    pub(crate) const TITLE: &str = "Both Screens Console";

    pub(crate) fn fixture() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/nds")
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
            let mut app = phone_app(&format!("nds-open-{name}"));
            app.settings_store = crate::storage::SettingsStore::at(dir.path().join("settings"));
            Self { app, dir }
        }

        pub(crate) fn rom_file(&self) -> std::path::PathBuf {
            let path = self.dir.path().join(GAME);
            std::fs::write(&path, fixture()).unwrap();
            path
        }

        pub(crate) fn picked(&self) -> PickedFile {
            PickedFile {
                name: GAME.to_owned(),
                data: fixture().into(),
                path: Some(self.rom_file()),
                seen: None,
            }
        }
    }

    fn runs_the_ds(app: &Citrine) {
        assert_eq!(app.emulator.kind(), ConsoleKind::Nds);
        assert!(app.emulator.rom_key().is_some(), "the game is open");
        assert!(app.emulator.running, "the game runs");
        assert_eq!(app.emulator.game_name(), TITLE);
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

    pub(crate) fn shows_texture(output: &egui::FullOutput, texture: egui::TextureId) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) => mesh.texture_id == texture,
            egui::Shape::Rect(rect) => rect
                .brush
                .as_ref()
                .is_some_and(|brush| brush.fill_texture_id == texture),
            _ => false,
        })
    }

    pub(crate) fn icon_texture(ctx: &Context) -> egui::TextureId {
        let banner = crate::ds_banner::banner(&fixture()).expect("a banner");
        crate::ds_banner::texture(ctx, &banner.icon)
            .expect("an icon")
            .id()
    }

    #[test]
    fn a_ds_game_opens_from_the_file_picker() {
        let mut opened = Opened::new("picker");
        let file = opened.picked();
        opened.app.files.rom_tx.send(file).unwrap();
        opened.app.drain_file_channels();
        runs_the_ds(&opened.app);
    }

    #[test]
    fn a_ds_game_opens_when_dropped() {
        let mut opened = Opened::new("drop");
        let path = opened.rom_file();
        dropped(&mut opened.app, &path);
        runs_the_ds(&opened.app);
    }

    #[test]
    fn a_ds_game_opens_from_its_bytes_alone() {
        let mut opened = Opened::new("bytes");
        opened.app.handle_load_rom_data(fixture());
        assert_eq!(opened.app.emulator.kind(), ConsoleKind::Nds);
        assert!(opened.app.emulator.rom_key().is_some());
    }

    #[test]
    fn a_ds_game_under_another_name_opens_by_its_header() {
        let mut opened = Opened::new("renamed");
        let mut file = opened.picked();
        file.name = "game.bin".to_owned();
        opened.app.handle_load_rom(file);
        assert_eq!(opened.app.emulator.kind(), ConsoleKind::Nds);
    }

    #[test]
    fn a_ds_game_joins_the_recent_list_with_its_console_and_icon_and_opens_from_it() {
        let mut opened = Opened::new("recent");
        let path = opened.rom_file();
        dropped(&mut opened.app, &path);
        let entry = opened.app.ui.recent.entries()[0].clone();
        assert_eq!(entry.path, path);
        assert_eq!(entry.console, Some(ConsoleKind::Nds));
        assert_eq!(entry.title, TITLE);
        assert!(
            opened.app.ui.recent.icon(&path).is_some(),
            "the banner icon"
        );

        opened.app.emulator.unload();
        opened.app.toasts = Default::default();
        opened.app.load_recent(path);
        runs_the_ds(&opened.app);
    }

    #[test]
    fn the_phone_recent_list_names_the_console_and_shows_the_icon() {
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
        assert!(text_rect(&output, TITLE).is_some(), "the recent row's name");
        assert!(
            text_rect(&output, "Nintendo DS, today").is_some(),
            "the recent row's detail"
        );
        assert!(
            shows_texture(&output, icon_texture(&ctx)),
            "the banner icon"
        );
    }

    #[test]
    fn a_ds_shelf_game_opens_with_its_name_and_shows_its_icon_on_the_shelf() {
        let game = crate::homebrew::HOMEBREW_GAMES
            .iter()
            .find(|game| game.console == crate::homebrew::ShelfConsole::NintendoDs)
            .expect("a DS shelf game");
        let mut opened = Opened::new("shelf");
        opened
            .app
            .handle_load_rom_data(game.embedded().expect("embedded"));
        assert_eq!(opened.app.emulator.kind(), ConsoleKind::Nds);
        assert_eq!(opened.app.emulator.game_name(), game.title);

        opened.app.emulator.unload();
        opened.app.mobile.nav.go(Place::Games);
        opened.app.mobile.open(crate::app::tabs::Tab::Homebrew);
        let ctx = Context::default();
        let texture = game.icon_texture(&ctx).expect("an icon");
        play_frame(
            &mut opened.app,
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(206.0, 500.0))],
        );
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -4000.0),
            modifiers: egui::Modifiers::NONE,
        };
        let mut output = play_frame(&mut opened.app, &ctx, vec![wheel]);
        for _ in 0..40 {
            output = play_frame(&mut opened.app, &ctx, Vec::new());
        }
        assert!(text_rect(&output, game.title).is_some(), "the shelf row");
        assert!(shows_texture(&output, texture), "the shelf icon");
    }

    #[test]
    fn a_ds_game_in_a_rom_folder_drops_its_extension() {
        assert_eq!(
            crate::app::ui_state::library::scan::game_name("Both Screens.NDS"),
            "Both Screens"
        );
    }

    #[test]
    fn a_ds_game_never_links() {
        let mut opened = Opened::new("link");
        let file = opened.picked();
        opened.app.handle_load_rom(file);
        opened.app.toasts = Default::default();
        assert!(
            !opened
                .app
                .link_game(fixture().into(), Some(GAME.to_owned()))
        );
        assert_eq!(
            opened.app.toasts.texts(),
            ["Nintendo DS games can't link or play online yet"]
        );
    }

    #[test]
    #[ignore = "needs DS games named in CITRINE_DS"]
    fn real_ds_games_paint_many_colours_in_the_app() {
        let shots = std::env::var("CITRINE_DS_SHOTS").ok();
        for path in std::env::var("CITRINE_DS").expect("CITRINE_DS").split('|') {
            let mut app = phone_app("real-ds");
            app.handle_load_rom_data(std::fs::read(path).expect("rom"));
            assert_eq!(app.emulator.kind(), ConsoleKind::Nds, "{path}");
            assert!(app.emulator.running, "{path}");
            let mut most = 0;
            for frame in 1..=1800 {
                assert_eq!(app.emulator.lend_one_frame(), 1, "{path} frame {frame}");
                if frame % 300 == 0 {
                    let (painted, [width, height]) = app.emulator.painted();
                    let colours: std::collections::HashSet<&[u8]> = painted.chunks(4).collect();
                    println!(
                        "{path}: frame {frame} {width}x{height} colours {}",
                        colours.len()
                    );
                    most = most.max(colours.len());
                    if let Some(dir) = &shots {
                        let name = std::path::Path::new(path)
                            .file_stem()
                            .unwrap()
                            .to_string_lossy();
                        let file = std::path::Path::new(dir).join(format!("{name}-{frame}.png"));
                        image::save_buffer(
                            &file,
                            painted,
                            width as u32,
                            height as u32,
                            image::ColorType::Rgba8,
                        )
                        .expect("picture");
                    }
                }
            }
            assert!(most > 16, "{path} paints {most} colours at most");
        }
    }

    #[test]
    #[ignore = "needs HeartGold named in CITRINE_HG"]
    fn real_heartgold_reaches_its_tutorial_and_keeps_a_flash_save() {
        let path = std::env::var("CITRINE_HG").expect("CITRINE_HG");
        let shots = std::env::var("CITRINE_DS_SHOTS").ok();
        let mut app = phone_app("real-hg");
        app.handle_load_rom_data(std::fs::read(&path).expect("rom"));
        app.toasts = Default::default();
        let start = citrine_gb::console::Keys::START;
        let keyboard = crate::held::InputSource::Keyboard;
        for frame in 1..=3000 {
            if [1900, 2150, 2600].contains(&frame) {
                app.emulator.press(keyboard, start);
            }
            if [1906, 2156, 2606].contains(&frame) {
                app.emulator.release(keyboard, start);
            }
            assert_eq!(app.emulator.lend_one_frame(), 1, "frame {frame}");
            app.emulator.handle_save().expect("save");
            if let (Some(dir), 2160 | 2280 | 2520 | 3000) = (&shots, frame) {
                let (painted, [width, height]) = app.emulator.painted();
                image::save_buffer(
                    std::path::Path::new(dir).join(format!("hg-app-{frame}.png")),
                    painted,
                    width as u32,
                    height as u32,
                    image::ColorType::Rgba8,
                )
                .expect("picture");
            }
        }
        let backup = app.emulator.game_file().and_then(|file| file.save);
        println!(
            "backup in the machine: {:?} bytes",
            backup.as_ref().map(Vec::len)
        );
        app.emulator.flush_save().expect("flush");
        app.emulator.writes.settle();
        let news = app.emulator.take_save_news();
        println!("save news: {news:?}");
        let key = app.emulator.rom_key().expect("key").to_owned();
        let stored = app.emulator.store.load_battery(&key);
        println!("stored save: {:?} bytes", stored.as_ref().map(Vec::len));
        println!("toasts: {:?}", app.toasts.texts());
        assert!(news.iter().all(Result::is_ok), "{news:?}");
        assert!(app.toasts.texts().is_empty());
        assert_eq!(backup.map(|save| save.len()), Some(512 * 1024));
    }
}
