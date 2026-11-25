use super::Citrine;
use super::boot_rom::BootRom;
#[cfg(feature = "nds")]
use super::ds_firmware::DsFile;
use super::events::{AppEvent, AppEventQueue};
use super::ui_state::settings_tree::rows::RowId;
use crate::utils::file_loader::PickedFile;

#[cfg(feature = "gba")]
pub const GBA_BIOS_ICON: &str = crate::icons::GBA_BIOS;

pub const PICK_ANOTHER: &str = "Pick another";

pub const BOOT_ROM_HINT: &str = "A boot ROM dumped from your own console shows the logo and plays the chime before each game. Without one, games start at once.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFile {
    BootRom(BootRom),
    #[cfg(feature = "gba")]
    GbaBios,
    #[cfg(feature = "nds")]
    Ds(DsFile),
}

impl SystemFile {
    pub fn all() -> Vec<Self> {
        #[cfg_attr(not(feature = "gba"), expect(unused_mut))]
        let mut files = vec![Self::BootRom(BootRom::Gb), Self::BootRom(BootRom::Gbc)];
        #[cfg(feature = "gba")]
        files.push(Self::GbaBios);
        #[cfg(feature = "nds")]
        files.extend([
            Self::Ds(DsFile::Arm9Bios),
            Self::Ds(DsFile::Arm7Bios),
            Self::Ds(DsFile::Firmware),
        ]);
        files
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::BootRom(file) => file.noun(),
            #[cfg(feature = "gba")]
            Self::GbaBios => "GBA BIOS",
            #[cfg(feature = "nds")]
            Self::Ds(file) => file.noun(),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::BootRom(file) => file.icon(),
            #[cfg(feature = "gba")]
            Self::GbaBios => GBA_BIOS_ICON,
            #[cfg(feature = "nds")]
            Self::Ds(file) => file.icon(),
        }
    }

    pub fn row(self) -> RowId {
        match self {
            Self::BootRom(BootRom::Gb) => RowId::GbBootRom,
            Self::BootRom(BootRom::Gbc) => RowId::GbcBootRom,
            #[cfg(feature = "gba")]
            Self::GbaBios => RowId::GbaBios,
            #[cfg(feature = "nds")]
            Self::Ds(DsFile::Arm9Bios) => RowId::Arm9Bios,
            #[cfg(feature = "nds")]
            Self::Ds(DsFile::Arm7Bios) => RowId::Arm7Bios,
            #[cfg(feature = "nds")]
            Self::Ds(DsFile::Firmware) => RowId::Firmware,
        }
    }

    pub fn of_row(row: RowId) -> Option<Self> {
        Self::all().into_iter().find(|file| file.row() == row)
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::BootRom(_) => BOOT_ROM_HINT,
            #[cfg(feature = "gba")]
            Self::GbaBios => super::gba_bios::BIOS_HINT,
            #[cfg(feature = "nds")]
            Self::Ds(_) => super::widgets::settings::ds::FILES_HINT,
        }
    }

    pub fn accepts(self, data: &[u8]) -> bool {
        match self {
            Self::BootRom(file) => file.accepts(data),
            #[cfg(feature = "gba")]
            Self::GbaBios => data.len() == citrine_gb::gba::region::BIOS_SIZE,
            #[cfg(feature = "nds")]
            Self::Ds(file) => file.accepts(data),
        }
    }

    fn size_text(self) -> &'static str {
        match self {
            Self::BootRom(file) => file.size_text(),
            #[cfg(feature = "gba")]
            Self::GbaBios => "16 KB",
            #[cfg(feature = "nds")]
            Self::Ds(file) => file.size_text(),
        }
    }

    pub fn home(self, data: &[u8]) -> Option<Self> {
        if self.accepts(data) {
            return Some(self);
        }
        let mut homes = Self::all().into_iter().filter(|file| file.accepts(data));
        match (homes.next(), homes.next()) {
            (Some(home), None) => Some(home),
            _ => None,
        }
    }

    pub fn empty(self) -> FileChoice<'static> {
        match self {
            Self::BootRom(_) => FileChoice::None,
            #[cfg(feature = "gba")]
            Self::GbaBios => FileChoice::BuiltIn,
            #[cfg(feature = "nds")]
            Self::Ds(_) => FileChoice::BuiltIn,
        }
    }
}

impl Citrine {
    pub(super) fn pick_system_file(&mut self, file: SystemFile) {
        match file {
            SystemFile::BootRom(file) => self.pick_boot_rom(file),
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => self.pick_gba_bios(),
            #[cfg(feature = "nds")]
            SystemFile::Ds(file) => self.pick_ds_file(file),
        }
    }

    pub(super) fn handle_system_file(&mut self, asked: SystemFile, picked: PickedFile) {
        let Some(file) = asked.home(&picked.data) else {
            self.wrong_size(asked, &picked);
            return;
        };
        let done = match file {
            SystemFile::BootRom(file) => self.keep_boot_rom(file, picked),
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => self.keep_gba_bios(picked),
            #[cfg(feature = "nds")]
            SystemFile::Ds(file) => self.keep_ds_file(file, picked),
        };
        let Some(done) = done else {
            return;
        };
        if file == asked {
            self.toasts.success(done);
        } else {
            self.toasts
                .success(format!("Saved as your {}.", file.noun()));
        }
    }

    pub(super) fn wrong_size(&mut self, asked: SystemFile, picked: &PickedFile) {
        self.toasts
            .error(format!(
                "'{}' is {}. A {} is {}.",
                picked.name,
                size_words(picked.data.len()),
                asked.noun(),
                asked.size_text()
            ))
            .button(PICK_ANOTHER, AppEvent::PickFile(asked))
            .needs_action();
    }

    pub(super) fn remove_system_file(&mut self, file: SystemFile) {
        match file {
            SystemFile::BootRom(file) => self.remove_boot_rom(file),
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => self.use_built_in_bios(),
            #[cfg(feature = "nds")]
            SystemFile::Ds(file) => self.remove_ds_file(file),
        }
    }
}

impl crate::emulator::Emulator {
    pub fn system_file_name(&self, file: SystemFile) -> Option<&str> {
        match file {
            SystemFile::BootRom(file) => self.boot_rom_name(file),
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => self.gba_bios_name(),
            #[cfg(feature = "nds")]
            SystemFile::Ds(file) => self.ds_file_name(file),
        }
    }
}

fn size_words(len: usize) -> String {
    if len >= 1024 && len.is_multiple_of(1024) {
        format!("{} KB", len / 1024)
    } else {
        format!("{len} bytes")
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum FileChoice<'a> {
    #[cfg(feature = "gba")]
    BuiltIn,
    None,
    File(&'a str),
    Pick {
        another: bool,
    },
}

impl<'a> FileChoice<'a> {
    pub fn current(file: SystemFile, name: Option<&'a str>) -> Self {
        name.map_or(file.empty(), Self::File)
    }

    pub fn all(file: SystemFile, name: Option<&'a str>) -> Vec<Self> {
        match name {
            Some(name) => vec![file.empty(), Self::File(name), Self::Pick { another: true }],
            None => vec![file.empty(), Self::Pick { another: false }],
        }
    }

    pub fn apply(self, file: SystemFile, events: &mut AppEventQueue) {
        match self {
            #[cfg(feature = "gba")]
            Self::BuiltIn => events.remove_system_file(file),
            Self::None => events.remove_system_file(file),
            Self::Pick { .. } => events.pick_system_file(file),
            Self::File(_) => {}
        }
    }
}

impl std::fmt::Display for FileChoice<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "gba")]
            Self::BuiltIn => f.write_str("Built-in"),
            Self::None => f.write_str("None"),
            Self::File(name) => f.write_str(name),
            Self::Pick { another: true } => f.write_str("Pick another file"),
            Self::Pick { another: false } => f.write_str("Pick a file"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SystemFile;
    use crate::app::Citrine;
    use crate::app::boot_rom::BootRom;
    use crate::app::tabs::Tab;
    use crate::app::tests::{info_frame_out, phone_app, phone_settings_output, text_rect};
    use crate::app::ui_state::settings::LayoutMode;
    use crate::app::ui_state::settings_tree::SettingsTab;
    use crate::utils::file_loader::PickedFile;
    use egui::Context;

    fn picked(name: &str, size: usize) -> PickedFile {
        PickedFile {
            name: name.to_owned(),
            data: vec![0x5A; size].into(),
            path: None,
            seen: None,
        }
    }

    fn size(file: SystemFile) -> usize {
        match file {
            SystemFile::BootRom(BootRom::Gb) => 0x100,
            SystemFile::BootRom(BootRom::Gbc) => 0x900,
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => 0x4000,
            #[cfg(feature = "nds")]
            SystemFile::Ds(file) => match file {
                crate::app::ds_firmware::DsFile::Arm9Bios => 0x1000,
                crate::app::ds_firmware::DsFile::Arm7Bios => 0x4000,
                crate::app::ds_firmware::DsFile::Firmware => 0x4_0000,
            },
        }
    }

    fn tab(file: SystemFile) -> SettingsTab {
        match file {
            SystemFile::BootRom(_) => SettingsTab::GameBoy,
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => SettingsTab::Gba,
            #[cfg(feature = "nds")]
            SystemFile::Ds(_) => SettingsTab::Ds,
        }
    }

    fn deliver(app: &mut Citrine, file: SystemFile, picked: PickedFile) {
        let files = &app.files;
        match file {
            SystemFile::BootRom(file) => files.boot_rom_tx.send((file, picked)).unwrap(),
            #[cfg(feature = "gba")]
            SystemFile::GbaBios => files.gba_bios_tx.send(picked).unwrap(),
            #[cfg(feature = "nds")]
            SystemFile::Ds(file) => files.ds_file_tx.send((file, picked)).unwrap(),
        }
        app.drain_file_channels();
    }

    fn app_at(name: &str, dir: &tempfile::TempDir) -> Citrine {
        let mut app = phone_app(name);
        app.settings_store = crate::storage::SettingsStore::at(dir.path().join("settings"));
        app
    }

    #[test]
    fn every_file_row_has_its_own_icon_from_the_set() {
        let icons: Vec<&str> = SystemFile::all()
            .into_iter()
            .map(SystemFile::icon)
            .collect();
        for (index, icon) in icons.iter().enumerate() {
            assert!(
                crate::icons::OWN.iter().any(|(_, glyph)| glyph == icon),
                "{icon:?} is not one of our own icons"
            );
            assert!(!icons[..index].contains(icon), "{icon:?} shows on two rows");
        }
        #[cfg(all(feature = "gba", feature = "nds"))]
        assert_eq!(icons.len(), 6);
    }

    #[test]
    fn each_pick_path_refuses_a_file_of_the_wrong_size() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-wrong-size", &dir);
        for file in SystemFile::all() {
            deliver(&mut app, file, picked("tiny.bin", 3));
            let texts = app.toasts.texts();
            let notice = texts.last().expect("a notice");
            let noun = file.noun();
            assert!(
                notice.starts_with(&format!("'tiny.bin' is 3 bytes. A {noun} is ")),
                "{notice}"
            );
            assert_eq!(app.emulator.system_file_name(file), None, "{file:?}");
            assert_eq!(
                app.toasts.button_labels().last(),
                Some(&super::PICK_ANOTHER)
            );
        }
        assert!(
            SystemFile::all()
                .into_iter()
                .all(|file| app.emulator.system_file_name(file).is_none())
        );
        let shown = app.toasts.open_texts().len();
        app.toasts.dismiss_all_toasts();
        assert_eq!(
            app.toasts.open_texts().len(),
            shown,
            "they stay until closed"
        );
    }

    #[test]
    fn each_file_has_its_own_icon() {
        let mut icons: Vec<&str> = SystemFile::all()
            .into_iter()
            .map(SystemFile::icon)
            .collect();
        let count = icons.len();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), count);
        for taken in [crate::icons::GAME_CONTROLLER, crate::icons::CPU] {
            assert!(!icons.contains(&taken), "{taken} belongs to another action");
        }
        for console in [SettingsTab::GameBoy, SettingsTab::Gba, SettingsTab::Ds] {
            assert!(
                !icons.contains(&console.icon()),
                "{console:?} owns its icon"
            );
        }
    }

    #[test]
    fn a_file_picked_in_the_wrong_slot_goes_to_the_one_its_size_fits() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-moved", &dir);
        let gb = SystemFile::BootRom(BootRom::Gb);
        let gbc = SystemFile::BootRom(BootRom::Gbc);
        deliver(&mut app, gb, picked("cgb_boot.bin", size(gbc)));
        deliver(&mut app, gbc, picked("dmg_boot.bin", size(gb)));
        assert_eq!(app.emulator.system_file_name(gbc), Some("cgb_boot.bin"));
        assert_eq!(app.emulator.system_file_name(gb), Some("dmg_boot.bin"));
        assert_eq!(
            app.toasts.texts(),
            ["Saved as your GBC boot ROM.", "Saved as your GB boot ROM."]
        );
    }

    #[test]
    fn a_picked_file_keeps_its_name_after_the_game_closes() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-after-close", &dir);
        for file in SystemFile::all() {
            deliver(&mut app, file, picked("mine.bin", size(file)));
        }
        app.emulator.unload();
        for file in SystemFile::all() {
            assert_eq!(
                app.emulator.system_file_name(file),
                Some("mine.bin"),
                "{file:?}"
            );
        }
    }

    #[test]
    fn a_picked_file_shows_in_its_consoles_settings_row() {
        for file in SystemFile::all() {
            let dir = tempfile::tempdir().unwrap();
            let mut desktop = app_at("files-row-desktop", &dir);
            desktop.ui.settings.layout = LayoutMode::Desktop;
            desktop.ui.settings.current_tab = tab(file);
            desktop.open_tab(Tab::Settings);
            deliver(&mut desktop, file, picked("mine.bin", size(file)));
            assert_eq!(desktop.emulator.system_file_name(file), Some("mine.bin"));
            let ctx = Context::default();
            info_frame_out(&mut desktop, &ctx, 1280.0, 5000.0);
            let output = info_frame_out(&mut desktop, &ctx, 1280.0, 5000.0);
            assert!(
                text_rect(&output, "mine.bin").is_some(),
                "{file:?} on desktop"
            );

            let mut phone = app_at("files-row-phone", &dir);
            deliver(&mut phone, file, picked("mine.bin", size(file)));
            let output = phone_settings_output(&mut phone, &Context::default(), tab(file));
            assert!(
                text_rect(&output, "mine.bin").is_some(),
                "{file:?} on the phone"
            );
        }
    }

    #[test]
    fn the_phone_games_place_lists_every_file_on_the_shared_pick_path() {
        use crate::app::mobile::games::FILES_TITLE;
        use crate::app::mobile::nav::Place;
        use crate::app::tests::{play_frame, tap};
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-phone-place", &dir);
        app.mobile.nav.go(Place::Games);
        let ctx = Context::default();
        let frames = |app: &mut Citrine, events| {
            play_frame(app, &ctx, events);
            play_frame(app, &ctx, Vec::new());
            play_frame(app, &ctx, Vec::new())
        };
        let output = frames(&mut app, Vec::new());
        let row = text_rect(&output, FILES_TITLE).expect("the row in More games");
        let output = frames(&mut app, tap(row.center()));
        for file in SystemFile::all() {
            assert!(
                text_rect(&output, file.noun()).is_some(),
                "{file:?} in the sheet"
            );
        }
        let gbc = text_rect(&output, "GBC boot ROM").expect("the GBC row");
        let output = frames(&mut app, tap(gbc.center()));
        for text in ["None", "Pick a file"] {
            assert!(
                text_rect(&output, text).is_some(),
                "{text} in the file sheet"
            );
        }
        assert!(
            text_rect(&output, super::BOOT_ROM_HINT).is_none(),
            "the explanation sits behind the info mark"
        );
        let mark = text_rect(&output, crate::icons::INFO).expect("the info mark");
        let touch = |app: &mut Citrine, at: egui::Pos2| {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            play_frame(app, &ctx, vec![egui::Event::PointerMoved(at), press(true)]);
            frames(app, vec![press(false), egui::Event::PointerGone])
        };
        let opened = touch(&mut app, mark.center());
        let hint = text_rect(&opened, super::BOOT_ROM_HINT)
            .expect("a tap on the info mark opens the explanation");
        let sheet_top = crate::app::mobile::sheet::above(&ctx)
            .expect("the sheet is open")
            .bottom();
        assert!(
            hint.bottom() < sheet_top - 4.0,
            "the hint at {hint:?} ends above the sheet's top edge {sheet_top}, so the grab handle shows"
        );
        assert!(
            hint.x_range().contains(mark.center().x),
            "the hint at {hint:?} sits over its mark at {mark:?}"
        );
        let title = opened
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "GBC boot ROM" => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .find(|rect| rect.y_range().contains(mark.center().y) && rect.right() < mark.left())
            .expect("the sheet's title beside its mark");
        let lit = opened
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle)
                    if circle.center.distance(mark.center()) < mark.width() =>
                {
                    Some(circle.center.x - circle.radius)
                }
                _ => None,
            })
            .expect("the open hint lights its mark");
        assert!(
            lit >= title.right() + 3.0,
            "the lit mark at {lit} keeps a gap after the title's end {}",
            title.right()
        );
        let screen = ctx.content_rect();
        assert!(
            hint.left() >= screen.left() + 16.0 && hint.right() <= screen.right() - 16.0,
            "the hint at {hint:?} keeps clear of the screen's edges {screen:?}"
        );
        for text in ["None", "Pick a file"] {
            let row = text_rect(&opened, text).expect("the row under the open hint");
            assert!(
                hint.bottom() < mark.top() && hint.bottom() < row.top(),
                "the hint at {hint:?} opens above the title row, clear of {text} at {row:?}"
            );
        }
        let output = touch(&mut app, mark.center());
        assert!(text_rect(&output, super::BOOT_ROM_HINT).is_none());
        let none = text_rect(&output, "None").expect("none");
        frames(&mut app, tap(none.center()));
        let gbc = SystemFile::BootRom(BootRom::Gbc);
        deliver(&mut app, gbc, picked("cgb_boot.bin", size(gbc)));
        let output = frames(&mut app, Vec::new());
        assert_eq!(app.emulator.system_file_name(gbc), Some("cgb_boot.bin"));
        assert!(
            text_rect(&output, "cgb_boot.bin").is_some(),
            "the list shows the file"
        );
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_file_sheet_opened_from_the_games_place_names_its_console() {
        use crate::app::mobile::games::FILES_TITLE;
        use crate::app::mobile::nav::Place;
        use crate::app::tests::{play_frame, tap};
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-sheet-title", &dir);
        app.mobile.nav.go(Place::Games);
        let ctx = Context::default();
        let frames = |app: &mut Citrine, events| {
            play_frame(app, &ctx, events);
            play_frame(app, &ctx, Vec::new());
            play_frame(app, &ctx, Vec::new())
        };
        let output = frames(&mut app, Vec::new());
        let row = text_rect(&output, FILES_TITLE).expect("the row in More games");
        let output = frames(&mut app, tap(row.center()));
        let gba = text_rect(&output, "GBA BIOS").expect("the GBA row");
        let output = frames(&mut app, tap(gba.center()));
        assert!(
            text_rect(&output, "Pick a file").is_some(),
            "the file sheet"
        );
        assert!(text_rect(&output, "GBA BIOS").is_some(), "its title");
    }

    fn frame(app: &mut Citrine, ctx: &Context, size: egui::Vec2) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            crate::app::mobile::touch_spacing(ctx, touch);
            app.render(ctx);
            app.show_toasts(ctx);
        })
    }

    fn covered_texts(output: &egui::FullOutput, notice: egui::Rect) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text(), text.visual_bounding_rect())),
                _ => None,
            })
            .filter(|(_, rect)| rect.intersects(notice) && !notice.contains_rect(*rect))
            .map(|(text, _)| text.to_owned())
            .collect()
    }

    #[test]
    fn the_wrong_size_notice_covers_no_text_on_desktop() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-notice-desktop", &dir);
        app.ui.settings.layout = LayoutMode::Desktop;
        let ctx = Context::default();
        let size = egui::vec2(1280.0, 800.0);
        frame(&mut app, &ctx, size);
        deliver(
            &mut app,
            SystemFile::BootRom(BootRom::Gb),
            picked("odd.bin", 0x180),
        );
        let mut output = frame(&mut app, &ctx, size);
        for _ in 0..4 {
            output = frame(&mut app, &ctx, size);
        }
        let notices = app.toasts.rects(&ctx);
        assert_eq!(notices.len(), 1);
        assert_eq!(covered_texts(&output, notices[0]), Vec::<String>::new());
    }

    #[test]
    fn a_notice_from_a_phone_sheet_shows_above_the_sheet() {
        use crate::app::mobile::games::FilesView;
        use crate::app::mobile::nav::Place;
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_at("files-notice-phone", &dir);
        app.mobile.nav.go(Place::Games);
        let gbc = SystemFile::BootRom(BootRom::Gbc);
        app.mobile.files_sheet = Some(FilesView::File(gbc));
        let ctx = Context::default();
        let size = egui::vec2(412.0, 915.0);
        frame(&mut app, &ctx, size);
        deliver(&mut app, gbc, picked("odd.bin", 0x180));
        let mut output = frame(&mut app, &ctx, size);
        for _ in 0..4 {
            output = frame(&mut app, &ctx, size);
        }
        let above = crate::app::mobile::sheet::above(&ctx).expect("the sheet is open");
        let notices = app.toasts.rects(&ctx);
        assert_eq!(notices.len(), 1);
        assert!(
            above.contains_rect(notices[0]),
            "{above:?} {:?}",
            notices[0]
        );
        assert!(text_rect(&output, super::PICK_ANOTHER).is_some());
    }
}
