use super::Citrine;
use super::tabs::Tab;
use super::ui_state::UiState;
use super::ui_state::settings_tree::SettingsTab;
use super::widgets::confirm::{self, Answer, Ask};
use super::widgets::settings::your_data::{
    self as your_data_ui, Exporting, PendingImport, Request,
};
use crate::progress::Counter;
use crate::utils::file_loader::FileLoader;
use crate::utils::file_saver::{FileSaver, SaveOutcome};
use crate::your_data::{self, Change, Files, Kind, Packer, Parts, Tally};
use egui::Context;
use egui_dock::DockState;

const COUNT_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
const FILTER: &str = "Citrine data";

impl Citrine {
    pub(super) fn data_parts(&mut self) -> Parts<'_> {
        Parts {
            saves: &self.emulator.store,
            media: &mut self.media,
            rom_copies: &self.files.rom_copies,
            online: &mut self.online,
            settings: &mut self.ui.settings,
            stats: &mut self.stats,
            #[cfg(not(target_arch = "wasm32"))]
            recent: &mut self.ui.recent,
            boot_roms: your_data::boot_roms::BootRoms {
                store: &self.settings_store,
                gb: &mut self.emulator.firmware.gb_boot_rom,
                gbc: &mut self.emulator.firmware.gbc_boot_rom,
                names: &mut self.emulator.boot_rom_names,
            },
            #[cfg(feature = "gba")]
            gba_bios: your_data::gba_bios::GbaBios {
                store: &self.settings_store,
                bios: &mut self.emulator.firmware.gba_bios,
                name: &mut self.emulator.gba_bios_name,
            },
            #[cfg(feature = "nds")]
            ds_files: your_data::ds_files::DsFiles {
                store: &self.settings_store,
                slots: &mut self.emulator.firmware.nds,
                names: &mut self.emulator.ds_file_names,
            },
            #[cfg(feature = "nds")]
            walker: &mut self.emulator.pokewalker,
            #[cfg(ds_wifi)]
            ds_wifi: your_data::ds_wifi::DsWifiPart {
                store: &self.settings_store,
                identity: &mut self.emulator.ds_wifi.identity,
            },
        }
    }

    pub(super) fn count_your_data(&mut self) {
        if !self.settings_page_open(SettingsTab::Storage) {
            self.ui.your_data.counted_at = None;
            return;
        }
        let now = web_time::Instant::now();
        if self
            .ui
            .your_data
            .counted_at
            .is_some_and(|at| now.duration_since(at) < COUNT_EVERY)
        {
            return;
        }
        let usage = self.storage_usage();
        self.ui.your_data.rows = self.data_parts().tallies(&usage);
        self.ui.your_data.counted_at = Some(now);
    }

    fn recount_your_data(&mut self) {
        self.ui.your_data.counted_at = None;
        self.ui.snapshots.invalidate();
    }

    pub(super) fn handle_your_data(&mut self, ctx: &Context) {
        let request = self.ui.your_data.request.take();
        let loading = your_data_ui::loading(&self.ui.your_data);
        let empty = your_data_ui::nothing_stored(&self.ui.your_data);
        match request {
            Some(Request::Export | Request::Import) if loading => {}
            Some(Request::Export | Request::Delete) if empty => {}
            Some(Request::Export) => self.start_data_export(),
            Some(Request::Import) => {
                FileLoader::new()
                    .title("Import your data")
                    .add_filter(FILTER, &[your_data::FILE_EXTENSION])
                    .dispatch(&self.files.spawner, self.ui.your_data.picked_tx.clone());
            }
            Some(Request::Delete) => {
                self.ui.your_data.deleting = true;
                if self.ui.your_data.exporting == Exporting::Done {
                    self.ui.your_data.exporting = Exporting::Idle;
                }
            }
            None => {}
        }
        self.poll_data_export();
        while let Ok(file) = self.ui.your_data.picked_rx.try_recv() {
            self.unpack_data_file(file.name, file.data);
        }
        while let Ok((name, files)) = self.ui.your_data.unpacked_rx.try_recv() {
            self.ui.your_data.importing = None;
            match files {
                Ok(files) => self.read_data_file(name, files),
                Err(err) => {
                    self.toasts.error(err.text());
                }
            }
        }
        self.import_dialog(ctx);
        self.delete_dialog(ctx);
    }

    fn start_data_export(&mut self) {
        if your_data_ui::loading(&self.ui.your_data) {
            return;
        }
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save before the export: {err:?}");
        }
        let files = self.data_parts().export();
        let counter = Counter::default();
        counter.set_total(your_data::export_bytes(&files, self.media.items()));
        self.ui.your_data.progress = counter.clone();
        self.ui.your_data.exporting = Exporting::Busy;
        let tx = self.ui.your_data.packed_tx.clone();
        let mut files = Some(files);
        let mut packer = Some(Packer::new(counter));
        self.media.read_files(&self.files.spawner, move |media| {
            let Some(packing) = packer.as_mut() else {
                return;
            };
            if let Some(files) = files.take() {
                packing.add_all(&files);
            }
            match media {
                Some((id, bytes)) => packing.add_media(&id, &bytes),
                None => {
                    if let Some(packing) = packer.take() {
                        let _ = tx.send(packing.finish());
                    }
                }
            }
        });
    }

    fn unpack_data_file(&mut self, name: String, bytes: std::sync::Arc<[u8]>) {
        let counter = Counter::default();
        counter.set_total(bytes.len() as u64);
        self.ui.your_data.progress = counter.clone();
        self.ui.your_data.importing = Some(name.clone());
        let tx = self.ui.your_data.unpacked_tx.clone();
        #[cfg(not(target_arch = "wasm32"))]
        self.files.spawner.run("import", move || {
            let files = your_data::unpack_counted(bytes, &counter);
            let _ = tx.send((name, files));
        });
        #[cfg(target_arch = "wasm32")]
        self.files.spawner.run_future("import", async move {
            let files = async {
                let mut unpacker = your_data::Unpacker::new(bytes, counter)?;
                while unpacker.step_for(your_data::STEP_BUDGET)? {
                    crate::utils::next_tick().await;
                }
                Ok(unpacker.finish())
            }
            .await;
            let _ = tx.send((name, files));
        });
    }

    fn poll_data_export(&mut self) {
        while let Ok(packed) = self.ui.your_data.packed_rx.try_recv() {
            match packed {
                Ok(bytes) => {
                    FileSaver::new(&your_data::file_name(crate::storage::now_unix()))
                        .add_filter(FILTER, &[your_data::FILE_EXTENSION])
                        .dispatch(
                            &self.files.spawner,
                            bytes,
                            self.ui.your_data.saved_tx.clone(),
                        );
                }
                Err(err) => {
                    self.ui.your_data.exporting = Exporting::Idle;
                    self.toasts.error(format!("Could not export: {err}"));
                }
            }
        }
        while let Ok(outcome) = self.ui.your_data.saved_rx.try_recv() {
            self.ui.your_data.exporting = match outcome {
                SaveOutcome::Saved(name) | SaveOutcome::SavedPair(name, _) => {
                    self.toasts.success(format!("Exported to '{name}'"));
                    Exporting::Done
                }
                SaveOutcome::Failed(err) => {
                    self.toasts.error(format!("Could not export: {err}"));
                    Exporting::Idle
                }
                SaveOutcome::Cancelled => Exporting::Idle,
            };
        }
    }

    fn read_data_file(&mut self, name: String, files: Files) {
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save before the import: {err:?}");
        }
        let plan = self.data_parts().plan(&files);
        if plan.iter().all(|(_, change)| *change == Change::default()) {
            self.toasts
                .info(format!("This device already has everything in '{name}'"));
            return;
        }
        self.ui.your_data.pending = Some(PendingImport { name, files, plan });
    }

    fn import_dialog(&mut self, ctx: &Context) {
        let Some(pending) = &self.ui.your_data.pending else {
            return;
        };
        let message = format!("Import '{}'?", pending.name);
        let notes = import_notes(&pending.plan);
        let answer = confirm::ask(
            ctx,
            "your_data_import",
            Ask {
                message: &message,
                list: &[],
                notes: &notes,
                action: "Import",
                action_enabled: true,
                extra: None,
                load: None,
                code: None,
            },
        );
        match answer {
            Some(Answer::Action) => {
                if let Some(pending) = self.ui.your_data.pending.take() {
                    self.import_data(pending.name, &pending.files);
                }
            }
            Some(_) => self.ui.your_data.pending = None,
            None => {}
        }
    }

    fn import_data(&mut self, name: String, files: &Files) {
        let rom = self.emulator.rom_key().map(str::to_owned);
        let battery = rom
            .as_deref()
            .and_then(|rom| self.emulator.store.battery_saved_at(rom));
        let result = self.data_parts().import(files);
        self.media.name_games(&self.ui.settings.game_names);
        if let Some(rom) = rom
            && self.emulator.store.battery_saved_at(&rom) != battery
            && let Err(err) = self.emulator.reload_battery()
        {
            log::error!("failed to load the imported save: {err:?}");
        }
        self.recount_your_data();
        match result {
            Ok(()) => self.toasts.success(format!("Imported '{name}'")),
            Err(err) => self
                .toasts
                .error(format!("Could not import everything: {err}")),
        };
    }

    fn delete_dialog(&mut self, ctx: &Context) {
        if !self.ui.your_data.deleting {
            return;
        }
        let (list, notes) = delete_ask(&self.ui.your_data.rows);
        let stored = stores_anything(&self.ui.your_data.rows);
        let exporting = self.ui.your_data.exporting;
        let extra = match exporting {
            Exporting::Idle | Exporting::Busy => "Export first",
            Exporting::Done => "Exported",
        };
        let load = your_data_ui::load(&self.ui.your_data);
        let answer = confirm::ask(
            ctx,
            "your_data_delete",
            Ask {
                message: "Delete everything on this device?",
                list: &list,
                notes: &notes,
                action: "Delete everything",
                action_enabled: stored && exporting != Exporting::Busy,
                extra: Some((extra, stored && exporting == Exporting::Idle)),
                load: load
                    .as_ref()
                    .map(|(label, progress)| (label.as_str(), *progress)),
                code: None,
            },
        );
        match answer {
            Some(Answer::Extra) => self.start_data_export(),
            Some(Answer::Action) => {
                self.ui.your_data.deleting = false;
                self.delete_everything();
            }
            Some(Answer::Cancel) => self.ui.your_data.deleting = false,
            None => {}
        }
    }

    pub(super) fn delete_everything(&mut self) {
        if self.play.session().is_some() {
            self.play.leave(&mut self.online);
        }
        self.emulator.unload();
        self.emulator.writes.settle();
        let deleted = self.data_parts().delete_all();
        let result = deleted.and(self.clear_cache());
        self.ui.recent.clear();
        self.online.cloud_forget_local();
        self.first_launch();
        match result {
            Ok(()) => self.toasts.success("Deleted everything on this device"),
            Err(err) => self
                .toasts
                .error(format!("Could not delete everything: {err}")),
        };
    }

    fn clear_cache(&self) -> std::io::Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        crate::media::recording::codec::shared().remove()?;
        self.emulator.store.clear_cache()
    }

    fn first_launch(&mut self) {
        let settings = std::mem::take(&mut self.ui.settings);
        let recent = std::mem::take(&mut self.ui.recent);
        let mut your_data = std::mem::take(&mut self.ui.your_data);
        your_data.counted_at = None;
        your_data.exporting = Exporting::Idle;
        *self.ui = UiState {
            settings,
            recent,
            your_data,
            ..UiState::default()
        };
        self.ui.settings.current_tab = SettingsTab::default();
        self.dock = DockState::new(vec![Tab::GameBoy]);
        self.open_tab(Tab::Info);
        self.mobile.nav = Default::default();
        self.game_info = Default::default();
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        {
            self.shots = Default::default();
        }
        self.cloud_rom = None;
        self.cloud_hold = None;
        self.pending_import = None;
        self.pending_reset = None;
        self.pending_console = None;
        self.reset_without_asking = false;
        self.quick_load_hold = None;
    }
}

fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn import_notes(plan: &[(Kind, Change)]) -> Vec<String> {
    let settings = |kind: &Kind| kind.name == "Settings";
    let added: Vec<String> = plan
        .iter()
        .filter(|(_, change)| change.added > 0)
        .map(|(kind, change)| kind.noun(change.added))
        .collect();
    let replaced: Vec<String> = plan
        .iter()
        .filter(|(kind, change)| change.replaced > 0 && !settings(kind))
        .map(|(kind, change)| kind.noun(change.replaced))
        .collect();
    let mut notes = Vec::new();
    if !added.is_empty() {
        notes.push(format!("Adds {}.", listed(&added)));
    }
    if !replaced.is_empty() {
        let copies = match plan
            .iter()
            .filter(|(kind, _)| !settings(kind))
            .map(|(_, change)| change.replaced)
            .sum::<usize>()
        {
            1 => "a newer copy",
            _ => "newer copies",
        };
        notes.push(format!(
            "Replaces {} with {copies} from the file.",
            listed(&replaced)
        ));
    }
    if let Some((kind, change)) = plan
        .iter()
        .find(|(kind, change)| settings(kind) && change.replaced > 0)
    {
        notes.push(format!(
            "Changes {} to match the file.",
            kind.noun(change.replaced)
        ));
    }
    if !notes.is_empty() {
        notes.push("Everything else on this device stays.".to_owned());
    }
    notes
}

fn stores_anything(rows: &[(Kind, Tally)]) -> bool {
    rows.iter().any(|(_, tally)| tally.count > 0)
}

pub const CLOUD_STAYS: &str = "Your online account and its cloud saves stay.";

fn delete_ask(rows: &[(Kind, Tally)]) -> (Vec<String>, Vec<String>) {
    let settings = |kind: &Kind| kind.name == "Settings";
    let mut list: Vec<String> = rows
        .iter()
        .filter(|(kind, tally)| tally.count > 0 && !settings(kind) && kind.system_file.is_none())
        .map(|(kind, tally)| kind.noun(tally.count))
        .collect();
    let system_files: usize = rows
        .iter()
        .filter(|(kind, _)| kind.system_file.is_some())
        .map(|(_, tally)| tally.count)
        .sum();
    if system_files > 0 {
        list.push(Kind::of(crate::storage::kinds::Kind::SystemFiles).noun(system_files));
    }
    list.extend(
        rows.iter()
            .find(|(kind, tally)| settings(kind) && tally.count > 0)
            .map(|(kind, tally)| format!("{}, back to default", kind.noun(tally.count))),
    );
    if list.is_empty() {
        list.push("Nothing you made is stored here yet.".to_owned());
    }
    let mut notes = Vec::new();
    let copies = Kind::of(crate::storage::kinds::Kind::GameCopies).name;
    if rows
        .iter()
        .any(|(kind, tally)| kind.name == copies && tally.count > 0)
    {
        notes.push(crate::storage::kinds::EXPORT_LEAVES_COPIES.to_owned());
    }
    notes.push(CLOUD_STAYS.to_owned());
    (list, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_everything_is_off_while_nothing_is_stored() {
        assert!(!stores_anything(&[]));
        assert_eq!(delete_ask(&[]).0, ["Nothing you made is stored here yet."]);
        let saves = Kind::of(crate::storage::kinds::Kind::Saves);
        let empty = Tally { count: 0 };
        assert!(!stores_anything(&[(saves, empty)]));
        let one = Tally { count: 1 };
        assert!(stores_anything(&[(saves, one)]));
    }

    #[test]
    fn the_import_confirm_says_what_it_adds_and_replaces() {
        let kind = |name, nouns| Kind {
            name,
            nouns,
            store: crate::storage::kinds::Kind::Saves,
            system_file: None,
        };
        let saves = kind("Saves", ("save", "saves"));
        let chats = kind("Chats", ("chat message", "chat messages"));
        let settings = kind("Settings", ("setting", "settings"));
        let notes = import_notes(&[
            (
                saves,
                Change {
                    added: 2,
                    replaced: 1,
                },
            ),
            (
                chats,
                Change {
                    added: 1,
                    replaced: 0,
                },
            ),
            (
                settings,
                Change {
                    added: 0,
                    replaced: 4,
                },
            ),
        ]);
        assert_eq!(
            notes,
            [
                "Adds 2 saves and 1 chat message.",
                "Replaces 1 save with a newer copy from the file.",
                "Changes 4 settings to match the file.",
                "Everything else on this device stays.",
            ]
        );
    }

    #[test]
    fn the_delete_confirm_counts_what_goes_and_says_what_stays() {
        let kind = |name, nouns| Kind {
            name,
            nouns,
            store: crate::storage::kinds::Kind::Saves,
            system_file: None,
        };
        let tally = |count| Tally { count };
        let rows = [
            (kind("Saves", ("save", "saves")), tally(4)),
            (kind("Media", ("file", "files")), tally(0)),
            (kind("Settings", ("setting", "settings")), tally(2)),
            (kind("Game names", ("game name", "game names")), tally(1)),
        ];
        let (list, notes) = delete_ask(&rows);
        assert_eq!(
            list,
            ["4 saves", "1 game name", "2 settings, back to default"]
        );
        assert_eq!(notes, [CLOUD_STAYS]);
        assert_eq!(
            delete_ask(&rows[1..2]).0,
            ["Nothing you made is stored here yet."]
        );
        let copies = [
            rows[0],
            (Kind::of(crate::storage::kinds::Kind::GameCopies), tally(2)),
        ];
        let (list, notes) = delete_ask(&copies);
        assert_eq!(list, ["4 saves", "2 game copies"]);
        assert_eq!(
            notes,
            [crate::storage::kinds::EXPORT_LEAVES_COPIES, CLOUD_STAYS]
        );
        let wifi = [(Kind::of(crate::storage::kinds::Kind::DsWifi), tally(2))];
        assert_eq!(delete_ask(&wifi).0, ["2 DS wifi connections"]);
    }

    #[test]
    fn an_empty_app_offers_no_export_and_no_delete() {
        let _dirs = crate::storage::TestDirs::new("empty-delete");
        let mut app = crate::app::tests::test_app();
        let ctx = egui::Context::default();
        let usage = app.storage_usage();
        let mut rows = app.data_parts().tallies(&usage);
        for (_, tally) in &mut rows {
            *tally = Tally::default();
        }
        app.ui.your_data.rows = rows;
        assert!(
            !your_data_ui::nothing_stored(&app.ui.your_data),
            "nothing is known before the count"
        );
        app.ui.your_data.counted_at = Some(web_time::Instant::now());
        assert!(your_data_ui::nothing_stored(&app.ui.your_data));
        for request in [Request::Delete, Request::Export] {
            app.ui.your_data.request = Some(request);
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.handle_your_data(ctx));
            assert!(!app.ui.your_data.deleting, "{request:?}");
            assert_eq!(app.ui.your_data.exporting, Exporting::Idle, "{request:?}");
        }
        app.ui.your_data.rows[0].1 = Tally { count: 1 };
        assert!(!your_data_ui::nothing_stored(&app.ui.your_data));
        app.ui.your_data.request = Some(Request::Delete);
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.handle_your_data(ctx));
        assert!(app.ui.your_data.deleting);
    }

    #[cfg(feature = "nds")]
    #[test]
    fn delete_everything_lists_and_removes_the_ds_files() {
        use crate::app::ds_firmware::DsFile;
        let mut opened = crate::app::files::nds_tests::Opened::new("ds-delete");
        let app = &mut opened.app;
        for (file, size) in [
            (DsFile::Arm9Bios, 0x1000),
            (DsFile::Arm7Bios, 0x4000),
            (DsFile::Firmware, 0x4_0000),
        ] {
            let picked = crate::utils::file_loader::PickedFile {
                name: format!("{}.bin", file.label()),
                data: vec![1; size].into(),
                path: None,
                seen: None,
            };
            app.handle_load_ds_file(file, picked);
        }
        let usage = app.storage_usage();
        let rows = app.data_parts().tallies(&usage);
        let (list, _) = delete_ask(&rows);
        assert!(
            list.contains(&"3 BIOS and boot ROM files".to_owned()),
            "{list:?}"
        );

        app.delete_everything();
        for file in <DsFile as strum::IntoEnumIterator>::iter() {
            assert_eq!(app.emulator.ds_file_name(file), None, "{file:?}");
            assert_eq!(
                app.settings_store.kept_file(file.store_keys()),
                None,
                "{file:?}"
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn files_in(dir: &std::path::Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut open = vec![dir.to_path_buf()];
        while let Some(next) = open.pop() {
            for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    open.push(path);
                } else if let Ok(relative) = path.strip_prefix(dir) {
                    found.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        found.sort();
        found
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn folders(dir: &std::path::Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn every_folder_the_app_writes_is_your_data_or_cache_and_delete_everything_empties_both() {
        use crate::media::{Frame, MediaKind, NewMedia};
        use crate::storage::kinds::{Folder, Kind};
        const GAME: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let dirs = crate::storage::TestDirs::new("sweep");
        let mut app = crate::app::tests::test_app();
        app.online.persist_chats();
        app.stats = crate::stats::Stats::open(crate::stats::StatsStore::default());

        let store = app.emulator.store.clone();
        store.put_file(GAME, "battery.sav", b"save").unwrap();
        store
            .put_file(
                "borrowed-00112233445566778899aabbccddeeff00112233",
                "battery.sav",
                b"save",
            )
            .unwrap();
        store.write_ledger("1234", b"cloud list").unwrap();
        store.covers().put_art(GAME, b"art").unwrap();
        let frame = Frame {
            width: 160,
            height: 144,
            rgba: vec![9; 160 * 144 * 4],
        };
        let png = crate::utils::screenshot::encode_png(160, 144, &frame.rgba).unwrap();
        let shot = NewMedia {
            kind: MediaKind::Screenshot,
            game: "Tobu Tobu Girl".into(),
            rom_key: Some(GAME.into()),
            model: None,
            created_at: 100,
            width: 160,
            height: 144,
            length_ms: None,
            frame_times: false,
            video: false,
            replay: None,
            frame,
        };
        app.media.add_screenshot(shot, &png).unwrap();
        std::fs::write(app.media.temp_path().unwrap(), b"half a recording").unwrap();
        app.online
            .chat_restore("1234_relay_example.bin", b"chat")
            .unwrap();
        app.stats.recorder().play(
            GAME,
            "Tobu Tobu Girl",
            std::time::Duration::from_secs(60),
            crate::stats::Activity::default(),
            1_000,
            "2026-11-07",
        );
        app.stats.flush();
        app.files.rom_copies.keep(GAME, "tobu.gb", b"rom").unwrap();
        app.ui.settings.volume = 0.25;
        app.settings_store.store(&app.ui.settings).unwrap();
        #[cfg(feature = "gba")]
        app.settings_store
            .store_gba_bios(Some((&[0; 0x4000], "gba_bios.bin")))
            .unwrap();
        #[cfg(feature = "nds")]
        app.emulator
            .pokewalker
            .press(citrine_gb_walker::Button::Middle);
        #[cfg(ds_wifi)]
        {
            let mut identity = crate::ds_wifi::Identity::random();
            identity.connections = Some(vec![
                1;
                citrine_gb::nds::spi::firmware::online::CONNECTIONS_BYTES
            ]);
            app.settings_store.store_ds_wifi(&identity).unwrap();
            app.emulator.ds_wifi.identity = Some(identity);
        }
        for folder in [
            Folder::Codecs,
            Folder::Export,
            Folder::Updates,
            Folder::Reports,
        ] {
            let dir = crate::storage::folder(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("file"), b"cache").unwrap();
        }
        crate::game_facts::InfoStore::on(crate::storage::Folders::default())
            .put_found(GAME, &crate::game_facts::Found::default())
            .unwrap();
        store
            .put_species_names(1, &["Bulbasaur".to_owned()])
            .unwrap();
        let cache = crate::storage::Folders::default();
        cache.write(Folder::Badges, "1.png", b"badge").unwrap();
        cache.write(Folder::AchievementLists, "gb", b"[]").unwrap();
        let threads = citrine_gb_threads::Threads::new(citrine_gb_threads::Mode::Inline);
        let rom = crate::storage::scratch_dir().join(format!("sweep-{}.gb", std::process::id()));
        std::fs::write(&rom, b"rom").unwrap();
        let stamp = crate::rom_ids::Stamp::new(
            3,
            std::time::SystemTime::now() - std::time::Duration::from_secs(3600),
        )
        .unwrap();
        crate::rom_ids::RomIds::default().learn(
            &crate::rom_ids::Seen::at(&rom, stamp),
            citrine_gb::rom::id::RomId::with_game_sha1(b"rom"),
            &threads.spawner(),
        );
        threads.run_side_jobs();
        let _ = std::fs::remove_file(&rom);
        #[cfg(feature = "nds")]
        {
            let jit = crate::storage::folder(Folder::Jit);
            std::fs::create_dir_all(&jit).unwrap();
            std::fs::write(jit.join("game.jit"), b"code").unwrap();
            std::fs::write(
                crate::storage::root_file(crate::storage::kinds::RootFile::JitKey),
                [7; 32],
            )
            .unwrap();
        }

        let usage = crate::storage::kinds::usage();
        assert_eq!(usage.get(&Kind::OtherData), None, "{usage:?}");
        assert_eq!(usage.get(&Kind::OtherCache), None, "{usage:?}");
        let data = [
            "chats",
            #[cfg(feature = "nds")]
            "jit.key",
            "media",
            "roms",
            "saves",
            "settings",
            "stats",
            #[cfg(feature = "nds")]
            "walker",
        ];
        assert_eq!(folders(&dirs.data), data);
        assert_eq!(
            folders(&dirs.cache),
            [
                "achievements",
                "badges",
                "codecs",
                "covers",
                "export",
                "game-pages",
                #[cfg(feature = "nds")]
                "jit",
                "media-temp",
                "reports",
                "rom-ids",
                "species-names",
                "updates",
            ]
        );
        let usage = app.storage_usage();
        app.ui.your_data.rows = app.data_parts().tallies(&usage);
        let (list, notes) = delete_ask(&app.ui.your_data.rows);
        assert!(
            list.starts_with(&["2 saves".to_owned(), "1 screenshot".to_owned()]),
            "{list:?}"
        );
        assert!(list.contains(&"1 game copy".to_owned()), "{list:?}");
        assert_eq!(notes[0], crate::storage::kinds::EXPORT_LEAVES_COPIES);

        app.delete_everything();
        let mut kept = vec![
            "saves/cloud/1234".to_owned(),
            "settings/settings.json".to_owned(),
        ];
        #[cfg(ds_wifi)]
        kept.push("settings/nds-wifi.bin".to_owned());
        #[cfg(feature = "nds")]
        kept.push("jit.key".to_owned());
        kept.sort();
        assert_eq!(files_in(&dirs.data), kept);
        assert_eq!(files_in(&dirs.cache), Vec::<String>::new());
        #[cfg(ds_wifi)]
        assert_eq!(
            app.emulator
                .ds_wifi
                .identity
                .as_ref()
                .map(|identity| identity.connections.is_some()),
            Some(false)
        );
    }

    #[test]
    fn delete_everything_counts_the_kinds_the_your_data_section_lists() {
        const GAME: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut app = crate::app::tests::test_app();
        let store = app.emulator.store.clone();
        store.put_file(GAME, "battery.sav", b"save").unwrap();
        store.put_file(GAME, "slot0.state", b"a state").unwrap();
        app.files.rom_copies.keep(GAME, "tobu.gb", b"rom").unwrap();
        let mystery = crate::storage::data_dir().join("mystery.bin");
        std::fs::write(&mystery, b"left over").unwrap();
        let listed = |app: &mut Citrine| {
            app.ui.storage.usage = app.storage_usage();
            let storage: Vec<(&str, u64)> = app
                .ui
                .storage
                .rows(crate::storage::kinds::Section::Data)
                .map(|kind| (kind.name(), app.ui.storage.bytes(kind)))
                .collect();
            let usage = app.storage_usage();
            let mut counted: Vec<&str> = Vec::new();
            let mut files = 0;
            for (kind, _) in app.data_parts().tallies(&usage) {
                if kind.system_file.is_some() {
                    files += 1;
                    if files == 1 {
                        counted.push(crate::storage::kinds::Kind::SystemFiles.name());
                    }
                } else {
                    counted.push(kind.name);
                }
            }
            assert!(files >= 2, "each system file is counted on its own");
            (storage, counted)
        };
        let (storage, counted) = listed(&mut app);
        let names: Vec<&str> = storage.iter().map(|(name, _)| *name).collect();
        assert_eq!(counted, names);
        for name in ["Saves", "Save states", "Game copies", "Other data files"] {
            assert!(
                storage
                    .iter()
                    .any(|(row, bytes)| *row == name && *bytes > 0),
                "{name}: {storage:?}"
            );
        }
        std::fs::remove_file(&mystery).unwrap();
        let (storage, counted) = listed(&mut app);
        let names: Vec<&str> = storage.iter().map(|(name, _)| *name).collect();
        assert_eq!(counted, names);
        assert!(
            storage.iter().all(|(row, _)| *row != "Other data files"),
            "{storage:?}"
        );
    }
}
