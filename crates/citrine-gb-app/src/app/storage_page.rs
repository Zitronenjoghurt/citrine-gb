use super::Citrine;
use super::tabs::Tab;
use super::ui_state::settings_tree::SettingsTab;
use super::widgets::confirm;
use super::widgets::settings::storage::{CLEAR, CLEAR_ALL, Request, ask_message, clear_ask, rows};
use crate::storage::kinds::{self, Folder, Kind, Section};
use egui::Context;

const COUNT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

impl Citrine {
    pub(super) fn settings_page_open(&self, page: SettingsTab) -> bool {
        (self.ui.settings.current_tab == page && self.dock.find_tab(&Tab::Settings).is_some())
            || self.mobile.nav.settings_page() == Some(page)
    }

    pub(super) fn count_storage(&mut self) {
        let whole = self.settings_page_open(SettingsTab::Storage);
        let compiled = (self.settings_page_open(SettingsTab::Ds)
            || self.settings_page_open(SettingsTab::Gba))
            && Kind::CompiledCode.shown();
        if !whole && !compiled {
            self.ui.storage.counted_at = None;
            return;
        }
        let now = web_time::Instant::now();
        if self
            .ui
            .storage
            .counted_at
            .is_some_and(|at| now.duration_since(at) < COUNT_EVERY)
        {
            return;
        }
        if whole {
            self.ui.storage.usage = self.storage_usage();
            self.ui.storage.counting = self.storage_counting();
        } else {
            let bytes = kinds::size(Kind::CompiledCode);
            self.ui.storage.usage.insert(Kind::CompiledCode, bytes);
        }
        self.ui.storage.counted_at = Some(now);
    }

    pub(super) fn storage_usage(&self) -> kinds::Usage {
        let mut usage = kinds::usage();
        let names: std::collections::BTreeMap<&str, &str> =
            self.ui.settings.game_names.iter().collect();
        let names = serde_json::to_vec(&names).map_or(0, |raw| raw.len() as u64);
        kinds::carve_from_settings(&mut usage, Kind::GameNames, names);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let recent = serde_json::to_vec(self.ui.recent.entries()).map_or(0, |raw| raw.len());
            kinds::carve_from_settings(&mut usage, Kind::RecentGames, recent as u64);
        }
        #[cfg(not(target_arch = "wasm32"))]
        for (media, dir) in self.media.chosen_folders() {
            kinds::count_folder(&mut usage, Folder::Media.kind_of(media.folder()), &dir);
        }
        #[cfg(target_arch = "wasm32")]
        for item in self.media.items() {
            *usage
                .entry(Folder::Media.kind_of(item.info.kind.folder()))
                .or_default() += item.info.bytes;
        }
        #[cfg(target_arch = "wasm32")]
        for (kind, store) in self.copy_stores() {
            if let Some(bytes) = store.bytes() {
                *usage.entry(kind).or_default() += bytes;
            }
        }
        usage
    }

    #[cfg(target_arch = "wasm32")]
    fn copy_stores(&self) -> [(Kind, &super::ui_state::recent::copies::RomCopies); 2] {
        [
            (Kind::GameCopies, &self.files.rom_copies),
            (Kind::GameArchive, self.archive.store()),
        ]
    }

    #[cfg(target_arch = "wasm32")]
    fn storage_counting(&self) -> std::collections::BTreeSet<Kind> {
        self.copy_stores()
            .into_iter()
            .filter(|(_, store)| store.bytes().is_none())
            .map(|(kind, _)| kind)
            .collect()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn storage_counting(&self) -> std::collections::BTreeSet<Kind> {
        std::collections::BTreeSet::new()
    }

    pub(super) fn handle_storage(&mut self, ctx: &Context) {
        match self.ui.storage.request.take() {
            Some(Request::Clear(kind)) => self.ui.storage.asking = Some(vec![kind]),
            Some(Request::ClearAll) => {
                self.ui.storage.asking = Some(rows(Section::Cache).collect());
            }
            None => {}
        }
        let Some(asked) = self.ui.storage.asking.clone() else {
            return;
        };
        let message = ask_message(&asked);
        let shown = clear_ask(&self.ui.storage, &asked);
        let answer = confirm::ask(
            ctx,
            "storage_clear",
            confirm::Ask {
                message: &message,
                list: &shown.list,
                notes: &shown.notes,
                action: if asked.len() == 1 { CLEAR } else { CLEAR_ALL },
                action_enabled: true,
                extra: None,
                load: None,
                code: None,
            },
        );
        match answer {
            Some(confirm::Answer::Action) => {
                self.ui.storage.asking = None;
                self.clear_storage(&asked);
            }
            Some(_) => self.ui.storage.asking = None,
            None => {}
        }
    }

    fn clear_storage(&mut self, kinds_to_clear: &[Kind]) {
        self.ui.storage.kept = None;
        let mut failed = None;
        for kind in kinds_to_clear {
            let result = match kind {
                #[cfg(feature = "nds")]
                Kind::CompiledCode => {
                    if self.emulator.clear_jit_cache() {
                        Ok(())
                    } else {
                        Err(std::io::Error::other("a file stayed"))
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                Kind::VideoCodec => crate::media::recording::codec::shared().remove(),
                Kind::GameArchive => {
                    let result = kinds::clear(Kind::GameArchive);
                    self.clear_archive_downloads();
                    result
                }
                kind => kinds::clear(*kind),
            };
            if let Err(err) = result {
                log::warn!("couldn't clear {}: {err}", kind.name());
                failed = Some((*kind, err));
            }
        }
        self.ui.storage.counted_at = None;
        if let Some((kind, err)) = failed {
            self.toasts
                .error(format!("Couldn't clear {}: {err}", kind.in_sentence()));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::mobile::nav::Place;
    use crate::app::tests::{info_frame_out, phone_app, phone_settings_output, tap, text_rect};
    use crate::app::ui_state::settings::{LayoutMode, Settings};
    use crate::app::ui_state::settings_tree::GroupId;
    use crate::app::ui_state::settings_tree::rows::RowId;
    use crate::app::widgets::settings::your_data::{DELETE, EXPORT, IMPORT, nothing_stored};
    use strum::IntoEnumIterator;

    const GAME: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn ending_with(output: &egui::FullOutput, wanted: &str) -> Option<egui::Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text().trim().ends_with(wanted) => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
    }

    fn assert_fused(output: &egui::FullOutput, place: &str) {
        let at = |wanted: &str| {
            ending_with(output, wanted).unwrap_or_else(|| panic!("{place}: {wanted} missing"))
        };
        let heading = at(Section::Data.title());
        let saves = at(Kind::Saves.name());
        let export = at(EXPORT);
        let import = at(IMPORT);
        let delete = at(DELETE);
        let cache = at(Section::Cache.title());
        let clear_all = at(CLEAR_ALL);
        assert!(
            heading.bottom() <= saves.top(),
            "{place}: {heading:?} {saves:?}"
        );
        for (name, button) in [("Export", export), ("Import", import), ("Delete", delete)] {
            assert!(saves.bottom() <= button.top(), "{place}: {name} {button:?}");
            assert!(button.bottom() <= cache.top(), "{place}: {name} {button:?}");
        }
        assert!(
            export.top() <= delete.top(),
            "{place}: {export:?} {delete:?}"
        );
        assert!(
            cache.bottom() <= clear_all.top(),
            "{place}: {cache:?} {clear_all:?}"
        );
    }

    fn desktop_storage(app: &mut Citrine, ctx: &Context) -> egui::FullOutput {
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.current_tab = SettingsTab::Storage;
        app.open_tab(Tab::Settings);
        app.count_storage();
        app.count_your_data();
        let mut output = info_frame_out(app, ctx, 1280.0, 3000.0);
        for _ in 0..2 {
            output = info_frame_out(app, ctx, 1280.0, 3000.0);
        }
        output
    }

    #[test]
    fn storage_holds_your_data_with_its_three_buttons_and_the_cache_on_desktop_and_phone() {
        let _dirs = crate::storage::TestDirs::new("storage-fused");
        let mut app = phone_app("storage-fused");
        let output = desktop_storage(&mut app, &Context::default());
        assert_fused(&output, "desktop");

        app.ui.settings.layout = LayoutMode::Mobile;
        let output = phone_settings_output(&mut app, &Context::default(), SettingsTab::Storage);
        assert_fused(&output, "phone");
        assert!(text_rect(&output, "Saves and app data").is_none());
    }

    fn shown_texts(output: &egui::FullOutput) -> Vec<(String, bool)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().trim().to_owned(), text.galley.elided))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn your_data_rows_count_what_they_hold_before_the_size_on_one_line() {
        let _dirs = crate::storage::TestDirs::new("storage-counts");
        let mut app = phone_app("storage-counts");
        let other = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        for game in [GAME, other] {
            app.emulator.store.store_battery(game, b"battery").unwrap();
        }
        let desktop = shown_texts(&desktop_storage(&mut app, &Context::default()));
        app.ui.settings.layout = LayoutMode::Mobile;
        app.count_your_data();
        let phone = shown_texts(&phone_settings_output(
            &mut app,
            &Context::default(),
            SettingsTab::Storage,
        ));
        for (place, texts) in [("desktop", desktop), ("phone", phone)] {
            assert!(
                texts.iter().any(|(text, _)| text.starts_with("2 saves · ")),
                "{place}: {texts:?}"
            );
            assert!(
                !texts
                    .iter()
                    .any(|(text, _)| text.starts_with("0 ") && text.contains(" · ")),
                "{place}: an empty row shows its size alone: {texts:?}"
            );
            let cut: Vec<&String> = texts
                .iter()
                .filter(|(text, elided)| {
                    *elided
                        && (text.contains(" · ")
                            || Kind::iter().any(|kind| kind.name() == text.as_str()))
                })
                .map(|(text, _)| text)
                .collect();
            assert!(cut.is_empty(), "{place}: cut short {cut:?}");
        }
    }

    #[test]
    fn a_saved_your_data_page_opens_storage() {
        let _dirs = crate::storage::TestDirs::new("storage-saved-tab");
        let stored = r#"{ "current_tab": "YourData", "volume": 0.3 }"#;
        let settings: Settings = serde_json::from_str(stored).expect("the old page still loads");
        assert_eq!(settings.current_tab, SettingsTab::Storage);
        let mut app = phone_app("storage-saved-tab");
        app.ui.settings = settings;
        app.ui.settings.layout = LayoutMode::Desktop;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        let mut output = info_frame_out(&mut app, &ctx, 1280.0, 3000.0);
        for _ in 0..2 {
            output = info_frame_out(&mut app, &ctx, 1280.0, 3000.0);
        }
        assert_eq!(app.ui.settings.current_tab, SettingsTab::Storage);
        assert_fused(&output, "saved desktop");
    }

    #[test]
    fn every_way_that_opened_your_data_opens_storage() {
        let _dirs = crate::storage::TestDirs::new("storage-ways-in");
        let mut app = phone_app("storage-ways-in");
        for touch in [false, true] {
            for row in [
                RowId::SpaceUsed,
                RowId::Export,
                RowId::Import,
                RowId::DeleteEverything,
            ] {
                app.ui.settings.current_tab = SettingsTab::Screen;
                app.mobile.nav.open_settings(SettingsTab::Screen);
                app.find_setting(row, touch);
                assert_eq!(app.ui.settings.current_tab, SettingsTab::Storage, "{row:?}");
                if touch {
                    assert_eq!(
                        app.mobile.nav.settings_page(),
                        Some(SettingsTab::Storage),
                        "{row:?}"
                    );
                }
            }
            app.ui.settings.current_tab = SettingsTab::Screen;
            app.find_group(GroupId::YourData, touch);
            assert_eq!(app.ui.settings.current_tab, SettingsTab::Storage);
            assert_eq!(app.ui.settings.find_group, Some(GroupId::YourData));
        }

        let ctx = Context::default();
        let mut app = phone_app("storage-ways-in-phone");
        let tall = |app: &mut Citrine, ctx: &Context, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 3000.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                let touch = app.is_mobile(ctx);
                crate::app::mobile::touch_spacing(ctx, touch);
                app.render(ctx);
                app.handle_event_queue(touch);
            })
        };
        app.mobile.nav.go(Place::Settings);
        let mut output = tall(&mut app, &ctx, Vec::new());
        for _ in 0..2 {
            output = tall(&mut app, &ctx, Vec::new());
        }
        assert!(text_rect(&output, SettingsTab::Storage.title()).is_some());
        assert!(
            ending_with(&output, "Your data").is_none(),
            "no Your data page"
        );

        app.ui.settings.search = "export".to_owned();
        tall(&mut app, &ctx, Vec::new());
        let output = tall(&mut app, &ctx, Vec::new());
        let hit = text_rect(&output, EXPORT).expect("the hit for Export");
        tall(&mut app, &ctx, tap(hit.center()));
        assert_eq!(app.mobile.nav.settings_page(), Some(SettingsTab::Storage));
        let mut output = tall(&mut app, &ctx, Vec::new());
        for _ in 0..30 {
            output = tall(&mut app, &ctx, Vec::new());
        }
        assert_fused(&output, "phone search");

        let ctx = Context::default();
        let mut app = phone_app("storage-ways-in-desktop");
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.current_tab = SettingsTab::Screen;
        app.open_tab(Tab::Settings);
        let mut output = info_frame_out(&mut app, &ctx, 1280.0, 3000.0);
        for _ in 0..2 {
            output = info_frame_out(&mut app, &ctx, 1280.0, 3000.0);
        }
        let page_list = crate::app::widgets::settings::page_list::text(SettingsTab::Storage);
        assert!(
            text_rect(&output, &page_list).is_some(),
            "{page_list} missing"
        );
        assert!(
            ending_with(&output, "Your data").is_none(),
            "no Your data page"
        );
    }

    #[test]
    fn delete_everything_on_storage_asks_first_says_what_goes_and_offers_export() {
        let _dirs = crate::storage::TestDirs::new("storage-delete-asks");
        let ctx = Context::default();
        let mut app = phone_app("storage-delete-asks");
        app.emulator.store.store_battery(GAME, b"battery").unwrap();
        let saves = crate::your_data::Kind::of(crate::storage::kinds::Kind::Saves);
        app.ui.your_data.rows = vec![(saves, crate::your_data::Tally { count: 2 })];
        app.ui.your_data.counted_at =
            Some(web_time::Instant::now() + std::time::Duration::from_secs(3600));
        assert!(!nothing_stored(&app.ui.your_data));
        app.mobile.nav.go(Place::Settings);
        app.mobile.nav.open_settings(SettingsTab::Storage);
        let frame = |app: &mut Citrine, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 3000.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                let touch = app.is_mobile(ctx);
                crate::app::mobile::touch_spacing(ctx, touch);
                app.render(ctx);
                app.handle_your_data(ctx);
            })
        };
        frame(&mut app, Vec::new());
        let output = frame(&mut app, Vec::new());
        let delete = text_rect(&output, DELETE).expect("Delete everything on Storage");
        frame(&mut app, tap(delete.center()));
        let mut output = frame(&mut app, Vec::new());
        for _ in 0..3 {
            output = frame(&mut app, Vec::new());
        }
        assert!(app.ui.your_data.deleting, "a tap asks first");
        assert!(
            !app.emulator.store.files().is_empty(),
            "a tap alone deletes nothing"
        );
        for wanted in [
            "Delete everything on this device?",
            "2 saves",
            crate::app::your_data::CLOUD_STAYS,
            "Export first",
        ] {
            assert!(
                text_rect(&output, wanted).is_some(),
                "the ask misses {wanted:?}"
            );
        }
    }

    #[test]
    fn a_chosen_capture_folder_counts_its_media_and_nothing_else_in_it() {
        let _dirs = crate::storage::TestDirs::new("storage-capture");
        let mut app = crate::app::tests::test_app();
        let chosen =
            crate::storage::scratch_dir().join(format!("citrine-capture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&chosen);
        std::fs::create_dir_all(chosen.join("screenshots")).unwrap();
        std::fs::create_dir_all(chosen.join("recordings")).unwrap();
        std::fs::write(chosen.join("screenshots").join("a.png"), [0; 40]).unwrap();
        std::fs::write(chosen.join("recordings").join("a.mp4"), [0; 300]).unwrap();
        std::fs::write(chosen.join("holiday.jpg"), [0; 5000]).unwrap();
        let before = app.storage_usage();
        app.media.follow_folder(chosen.to_str());
        let after = app.storage_usage();
        let grew = |kind| after.get(&kind).unwrap_or(&0) - before.get(&kind).unwrap_or(&0);
        assert!(grew(Kind::Screenshots) >= 40, "{before:?} {after:?}");
        assert!(grew(Kind::Recordings) >= 300, "{before:?} {after:?}");
        let total = |usage: &kinds::Usage| usage.values().sum::<u64>();
        assert!(
            total(&after) - total(&before) < 5000,
            "{before:?} {after:?}"
        );
        let _ = std::fs::remove_dir_all(&chosen);
    }

    fn contents(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut found = std::collections::BTreeMap::new();
        let mut open = vec![dir.to_path_buf()];
        while let Some(next) = open.pop() {
            for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    open.push(path);
                } else if let Ok(relative) = path.strip_prefix(dir) {
                    let bytes = std::fs::read(&path).unwrap();
                    found.insert(relative.to_string_lossy().into_owned(), bytes);
                }
            }
        }
        found
    }

    #[test]
    fn clearing_each_cache_kind_and_then_all_keeps_every_data_file() {
        use crate::storage::kinds::RootFile;
        const GAME: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let dirs = crate::storage::TestDirs::new("clear-keeps-data");
        let mut app = crate::app::tests::test_app();
        let store = app.emulator.store.clone();
        store.put_file(GAME, "battery.sav", b"save").unwrap();
        store.put_file(GAME, "slot0.snap", b"state").unwrap();
        store.covers().put_art(GAME, b"art").unwrap();
        store
            .put_species_names(1, &["Bulbasaur".to_owned()])
            .unwrap();
        app.settings_store.store(&app.ui.settings).unwrap();
        app.files.rom_copies.keep(GAME, "tobu.gb", b"rom").unwrap();
        crate::storage::Folders::default()
            .write(Folder::Chats, "1234_relay_example.bin", b"chat")
            .unwrap();
        std::fs::write(crate::storage::root_file(RootFile::AppState), b"layout").unwrap();
        std::fs::write(crate::storage::root_file(RootFile::JitKey), [7; 32]).unwrap();
        std::fs::write(app.media.temp_path().unwrap(), b"half a recording").unwrap();
        let cache = crate::storage::Folders::default();
        for folder in Folder::iter().filter(|folder| folder.root() == kinds::Root::Cache) {
            cache.write(folder, "file", b"cache").unwrap();
        }
        std::fs::write(dirs.cache.join("loose.bin"), b"cache").unwrap();
        let data = contents(&dirs.data);
        assert!(data.len() >= 7, "{:?}", data.keys());

        for kind in Section::Cache.kinds() {
            app.clear_storage(&[kind]);
            assert_eq!(contents(&dirs.data), data, "{kind:?}");
        }
        app.clear_storage(&rows(Section::Cache).collect::<Vec<_>>());
        assert_eq!(contents(&dirs.data), data);
        store.clear_cache().unwrap();
        assert_eq!(contents(&dirs.data), data);
        assert_eq!(contents(&dirs.cache).len(), 0);
    }
}
