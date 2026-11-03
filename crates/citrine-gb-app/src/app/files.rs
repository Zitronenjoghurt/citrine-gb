use super::{Citrine, PendingImport, QUICK_LOAD_HOLD};
use super::{tabs, widgets};
use crate::app::ui_state::settings::plays_on_both;
use crate::emulator::SaveImport;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use crate::utils::save_files::{FileKind, SaveFiles};
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

    pub(super) fn handle_load_rom(&mut self, file: PickedFile) {
        self.try_start_audio();
        let rom = Rom::new(&file.data);
        let model = self.ui.settings.console_for_rom(&rom);
        if let Err(err) = self.emulator.load_rom(
            &rom,
            model,
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
        let header = &self.emulator.gb.cartridge.header;
        let sha = header.sha256_hex_string();
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
        let key = plays_on_both(header.cgb_mode).then_some(sha);
        let dropped = self.ui.recent.record(&path, &title, key);
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
        self.emulator.gb.load_boot_rom(&file.data);
        self.ui.settings.dirty = true;
        self.toasts.success("Boot ROM loaded");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn handle_export_e2e(&mut self, dir: &std::path::Path) {
        if self.ui.e2e.title.is_empty() {
            self.toasts.error("Please enter a title for the E2E test");
            return;
        }

        let e2e = self
            .emulator
            .gb
            .create_e2e_test(&self.ui.e2e.title, &self.ui.e2e.description);

        if let Err(err) = e2e.export(dir) {
            self.toasts.error(format!("Failed to export E2E: {}", err));
        } else {
            self.toasts
                .success(format!("Exported E2E to '{}'", dir.display()));
        }
    }

    pub(super) fn handle_load_rom_data(&mut self, data: Vec<u8>) {
        self.try_start_audio();
        let rom = Rom::new(&data);
        let model = self.ui.settings.console_for_rom(&rom);
        #[cfg(not(target_arch = "wasm32"))]
        let result = self.emulator.load_rom(&rom, model, None);
        #[cfg(target_arch = "wasm32")]
        let result = self.emulator.load_rom(&rom, model);

        match result {
            Ok(()) => self.close_mobile_sheet(),
            Err(err) => {
                self.toasts.error(format!("Failed to load ROM: {err}"));
            }
        }
        self.ui.settings.dirty = true;
    }
}
