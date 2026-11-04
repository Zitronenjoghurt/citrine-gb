use super::Citrine;
use crate::utils::file_loader::{FileLoader, PickedFile};
use citrine_gb::gba::bios::Bios;

impl Citrine {
    pub(super) fn load_gba_bios(&mut self) {
        let Some((image, name)) = self.settings_store.gba_bios() else {
            return;
        };
        match Bios::user(&image) {
            Ok(bios) => {
                self.emulator.firmware.gba_bios = Some(bios);
                self.emulator.gba_bios_name = name;
            }
            Err(err) => log::warn!("ignoring the saved GBA BIOS: {err}"),
        }
    }

    pub(super) fn pick_gba_bios(&mut self) {
        FileLoader::new()
            .title("Pick a GBA BIOS")
            .add_filter("GBA BIOS", &["bin", "rom"])
            .dispatch(self.files.gba_bios_tx.clone());
    }

    pub(super) fn handle_load_gba_bios(&mut self, file: PickedFile) {
        let bios = match Bios::user(&file.data) {
            Ok(bios) => bios,
            Err(_) => {
                self.toasts.error(format!(
                    "'{}' isn't a GBA BIOS. A GBA BIOS file is 16 KB.",
                    file.name
                ));
                return;
            }
        };
        if let Err(err) = self
            .settings_store
            .store_gba_bios(Some((bios.image(), &file.name)))
        {
            self.toasts
                .error(format!("Could not keep the GBA BIOS: {err}"));
            return;
        }
        self.emulator.firmware.gba_bios = Some(bios);
        self.emulator.gba_bios_name = file.name;
        self.toasts.success(self.bios_toast("Your GBA BIOS is set"));
    }

    pub(super) fn forget_gba_bios(&mut self) -> std::io::Result<()> {
        self.emulator.firmware.gba_bios = None;
        self.emulator.gba_bios_name.clear();
        self.settings_store.store_gba_bios(None)
    }

    pub(super) fn use_built_in_bios(&mut self) {
        if let Err(err) = self.forget_gba_bios() {
            self.toasts
                .error(format!("Could not remove the GBA BIOS: {err}"));
            return;
        }
        self.toasts
            .success(self.bios_toast("Using the built-in GBA BIOS"));
    }

    fn bios_toast(&self, done: &str) -> String {
        if self.emulator.kind() == citrine_gb::console::ConsoleKind::Gba {
            format!("{done}. It starts with the next GBA game.")
        } else {
            done.to_owned()
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum BiosChoice<'a> {
    BuiltIn,
    File(&'a str),
    Pick { another: bool },
}

impl<'a> BiosChoice<'a> {
    pub fn current(name: Option<&'a str>) -> Self {
        name.map_or(Self::BuiltIn, Self::File)
    }

    pub fn all(name: Option<&'a str>) -> Vec<Self> {
        match name {
            Some(name) => vec![
                Self::BuiltIn,
                Self::File(name),
                Self::Pick { another: true },
            ],
            None => vec![Self::BuiltIn, Self::Pick { another: false }],
        }
    }

    pub fn apply(self, events: &mut super::events::AppEventQueue) {
        match self {
            Self::BuiltIn => events.use_built_in_bios(),
            Self::Pick { .. } => events.pick_gba_bios(),
            Self::File(_) => {}
        }
    }
}

impl std::fmt::Display for BiosChoice<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuiltIn => f.write_str("Built-in"),
            Self::File(name) => f.write_str(name),
            Self::Pick { another: true } => f.write_str("Pick another file"),
            Self::Pick { another: false } => f.write_str("Pick a file"),
        }
    }
}

pub const BIOS_HINT: &str = "Games run on Cult-of-GBA, a free BIOS built into Citrine. If a game misbehaves, try a BIOS file dumped from your own console.";

#[cfg(all(test, feature = "gba"))]
mod tests {
    use crate::app::files::tests::Opened;
    use crate::app::tabs::Tab;
    use crate::app::tests::{
        info_frame_out, inside_its_clip, phone_app, play_frame, tap, text_rect,
    };
    use crate::app::ui_state::settings::{LayoutMode, SettingsTab};
    use crate::utils::file_loader::PickedFile;
    use citrine_gb::gba::bios::BiosKind;
    use egui::Context;

    fn bios_file(size: usize) -> PickedFile {
        PickedFile {
            name: "gba_bios.bin".to_owned(),
            data: vec![0x5A; size],
            path: None,
        }
    }

    #[test]
    fn a_picked_bios_is_kept_and_runs_the_next_gba_game() {
        let mut opened = Opened::new("bios");
        opened
            .app
            .files
            .gba_bios_tx
            .send(bios_file(0x4000))
            .unwrap();
        opened.app.drain_file_channels();
        assert_eq!(opened.app.emulator.gba_bios_kind(), BiosKind::User);
        assert_eq!(opened.app.toasts.texts(), ["Your GBA BIOS is set"]);

        let file = opened.picked();
        opened.app.handle_load_rom(file);
        let gba = opened.app.emulator.as_gba().expect("a gba");
        assert_eq!(gba.bios(), BiosKind::User);

        let mut restarted = phone_app("gba-open-bios-restart");
        restarted.settings_store =
            crate::storage::SettingsStore::at(opened.dir.path().join("settings"));
        restarted.load_gba_bios();
        assert_eq!(restarted.emulator.gba_bios_kind(), BiosKind::User, "kept");
        assert_eq!(restarted.emulator.gba_bios_name(), Some("gba_bios.bin"));

        restarted.use_built_in_bios();
        assert_eq!(restarted.emulator.gba_bios_kind(), BiosKind::BuiltIn);
        let mut again = phone_app("gba-open-bios-again");
        again.settings_store =
            crate::storage::SettingsStore::at(opened.dir.path().join("settings"));
        again.load_gba_bios();
        assert_eq!(again.emulator.gba_bios_kind(), BiosKind::BuiltIn, "removed");
        assert_eq!(again.emulator.gba_bios_name(), None);
    }

    #[test]
    fn without_a_bios_a_gba_game_runs_on_the_built_in_one() {
        let mut opened = Opened::new("no-bios");
        opened.app.load_gba_bios();
        let file = opened.picked();
        opened.app.handle_load_rom(file);
        let gba = opened.app.emulator.as_gba().expect("a gba");
        assert_eq!(gba.bios(), BiosKind::BuiltIn);
    }

    #[test]
    fn a_file_of_the_wrong_size_is_not_a_bios() {
        let mut opened = Opened::new("bad-bios");
        opened.app.handle_load_gba_bios(bios_file(0x100));
        assert_eq!(opened.app.emulator.gba_bios_kind(), BiosKind::BuiltIn);
        assert_eq!(
            opened.app.toasts.texts(),
            ["'gba_bios.bin' isn't a GBA BIOS. A GBA BIOS file is 16 KB."]
        );
    }

    #[test]
    fn settings_show_the_bios_on_desktop_and_phone() {
        let mut desktop = Opened::new("bios-desktop");
        desktop.app.ui.settings.layout = LayoutMode::Desktop;
        desktop.app.ui.settings.current_tab = SettingsTab::General;
        desktop.app.open_tab(Tab::Settings);
        let ctx = Context::default();
        info_frame_out(&mut desktop.app, &ctx, 1280.0, 900.0);
        let output = info_frame_out(&mut desktop.app, &ctx, 1280.0, 900.0);
        assert!(text_rect(&output, "GBA BIOS").is_some());
        assert!(text_rect(&output, "Built-in").is_some());

        desktop.app.handle_load_gba_bios(bios_file(0x4000));
        for width in [560.0, 1280.0] {
            info_frame_out(&mut desktop.app, &ctx, width, 900.0);
            let output = info_frame_out(&mut desktop.app, &ctx, width, 900.0);
            for text in ["GBA BIOS", "gba_bios.bin"] {
                assert!(inside_its_clip(&output, text), "{text} at {width}");
            }
        }

        let mut phone = Opened::new("bios-phone");
        phone.app.mobile.nav.open_settings(SettingsTab::General);
        let ctx = Context::default();
        play_frame(&mut phone.app, &ctx, Vec::new());
        let output = play_frame(&mut phone.app, &ctx, Vec::new());
        assert!(text_rect(&output, "GBA BIOS").is_some());
        assert!(text_rect(&output, "Built-in").is_some());

        phone.app.handle_load_gba_bios(bios_file(0x4000));
        play_frame(&mut phone.app, &ctx, Vec::new());
        let output = play_frame(&mut phone.app, &ctx, Vec::new());
        let row = text_rect(&output, "gba_bios.bin").expect("the file name on the row");
        play_frame(&mut phone.app, &ctx, tap(row.center()));
        play_frame(&mut phone.app, &ctx, Vec::new());
        let output = play_frame(&mut phone.app, &ctx, Vec::new());
        for text in ["Built-in", "Pick another file"] {
            assert!(text_rect(&output, text).is_some(), "{text} in the sheet");
        }
    }

    #[test]
    fn delete_everything_forgets_the_bios() {
        let mut opened = Opened::new("bios-delete");
        opened.app.handle_load_gba_bios(bios_file(0x4000));
        let store = crate::storage::SettingsStore::at(opened.dir.path().join("settings"));
        assert!(store.gba_bios().is_some());
        assert!(opened.app.forget_gba_bios().is_ok());
        assert!(store.gba_bios().is_none());
        assert_eq!(opened.app.emulator.gba_bios_kind(), BiosKind::BuiltIn);
    }
}
