use super::Citrine;
use crate::utils::file_loader::{FileLoader, PickedFile};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

pub const NEEDS_BIOS: &str = "Encrypted game: needs your ARM7 BIOS";
pub const BIOS_SAVED: &str = "ARM7 BIOS saved for every game";

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum DsFile {
    Arm9Bios,
    Arm7Bios,
    Firmware,
}

impl DsFile {
    pub fn label(self) -> &'static str {
        match self {
            Self::Arm9Bios => "ARM9 BIOS",
            Self::Arm7Bios => "ARM7 BIOS",
            Self::Firmware => "Firmware",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Arm9Bios => "DS ARM9 BIOS",
            Self::Arm7Bios => "DS ARM7 BIOS",
            Self::Firmware => "DS firmware",
        }
    }

    fn sizes(self) -> &'static [usize] {
        match self {
            Self::Arm9Bios => &[0x1000],
            Self::Arm7Bios => &[0x4000],
            Self::Firmware => &[0x2_0000, 0x4_0000, 0x8_0000],
        }
    }

    fn size_text(self) -> &'static str {
        match self {
            Self::Arm9Bios => "4 KB",
            Self::Arm7Bios => "16 KB",
            Self::Firmware => "256 KB",
        }
    }

    pub fn accepts(self, data: &[u8]) -> bool {
        self.sizes().contains(&data.len())
    }

    pub fn store_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Arm9Bios => ("nds-arm9-bios.bin", "nds-arm9-bios-name.txt"),
            Self::Arm7Bios => ("nds-arm7-bios.bin", "nds-arm7-bios-name.txt"),
            Self::Firmware => ("nds-firmware.bin", "nds-firmware-name.txt"),
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

impl Citrine {
    pub(super) fn load_ds_files(&mut self) {
        #[cfg(ds_wifi)]
        self.load_ds_wifi();
        for file in DsFile::iter() {
            match self.settings_store.ds_file(file) {
                Some((image, name)) if file.accepts(&image) => {
                    self.emulator.set_ds_file(file, Some((image, name)));
                }
                Some(_) => log::warn!("ignoring the saved {}", file.noun()),
                None => {}
            }
        }
    }

    pub(super) fn pick_ds_file(&mut self, file: DsFile) {
        let tx = self.files.ds_file_tx.clone();
        FileLoader::new()
            .title(&format!("Pick a {}", file.noun()))
            .add_filter(file.noun(), &["bin", "rom"])
            .pick(&self.files.spawner, move |picked| {
                let _ = tx.send((file, picked));
            });
    }

    pub(super) fn handle_load_ds_file(&mut self, file: DsFile, picked: PickedFile) {
        if !file.accepts(&picked.data) {
            self.toasts.error(format!(
                "'{}' isn't a {}. A {} file is {}.",
                picked.name,
                file.noun(),
                file.noun(),
                file.size_text()
            ));
            return;
        }
        if let Err(err) = self
            .settings_store
            .store_ds_file(file, Some((&picked.data, &picked.name)))
        {
            self.toasts
                .error(format!("Could not keep the {}: {err}", file.noun()));
            return;
        }
        let waited = file == DsFile::Arm7Bios && self.waits_for_ds_bios();
        self.emulator
            .set_ds_file(file, Some((picked.data.to_vec(), picked.name)));
        let done = format!("Your {} is set", file.noun());
        if waited {
            self.toasts.close(NEEDS_BIOS);
            if let Err(err) = self.emulator.restart_ds() {
                self.toasts.error(format!("Couldn't start the game: {err}"));
                return;
            }
            self.toasts.success(BIOS_SAVED);
            return;
        }
        self.toasts.success(self.ds_toast(&done));
    }

    fn waits_for_ds_bios(&self) -> bool {
        self.emulator.kind() == citrine_gb::console::ConsoleKind::Nds
            && self.emulator.ds_file_name(DsFile::Arm7Bios).is_none()
            && citrine_gb::nds::card::secure_area_encrypted(&self.emulator.rom_bytes())
    }

    pub(super) fn remove_ds_file(&mut self, file: DsFile) {
        if let Err(err) = self.settings_store.store_ds_file(file, None) {
            self.toasts
                .error(format!("Could not remove the {}: {err}", file.noun()));
            return;
        }
        self.emulator.set_ds_file(file, None);
        self.toasts
            .success(self.ds_toast(&format!("Using the built-in {}", file.noun())));
    }

    pub(super) fn clear_jit_cache(&mut self) {
        use crate::app::ui_state::settings::{JIT_CLEAR_FAILED, JIT_CLEARED};
        if self.emulator.clear_jit_cache() {
            self.toasts.success(JIT_CLEARED);
        } else {
            self.toasts.error(JIT_CLEAR_FAILED);
        }
    }

    pub(super) fn ask_for_ds_bios(&mut self) {
        self.toasts.close(NEEDS_BIOS);
        if self.waits_for_ds_bios() {
            self.toasts
                .error(NEEDS_BIOS)
                .button("Add BIOS", super::events::AppEvent::FindDsBios)
                .needs_action();
        }
    }

    pub(super) fn find_ds_bios(&mut self, touch: bool) {
        self.ui.settings.find_ds_bios = true;
        self.open_settings(crate::app::ui_state::settings::SettingsTab::General, touch);
    }

    pub(super) fn find_ds_direct(&mut self, touch: bool) {
        self.ui.settings.find_ds_direct = true;
        self.open_settings(crate::app::ui_state::settings::SettingsTab::General, touch);
    }

    fn ds_toast(&self, done: &str) -> String {
        if self.emulator.kind() == citrine_gb::console::ConsoleKind::Nds {
            format!("{done}. It starts with the next DS game.")
        } else {
            done.to_owned()
        }
    }

    #[cfg(ds_wifi)]
    fn load_ds_wifi(&mut self) {
        let identity = self.settings_store.ds_wifi().unwrap_or_else(|| {
            let identity = crate::ds_wifi::Identity::random();
            if let Err(err) = self.settings_store.store_ds_wifi(&identity) {
                log::error!("could not keep the DS wifi identity: {err}");
            }
            identity
        });
        let wifi = &mut self.emulator.ds_wifi;
        wifi.identity = Some(identity);
        wifi.spawner = Some(self.files.spawner.clone());
    }

    #[cfg(ds_wifi)]
    fn sync_ds_wifi(&mut self) {
        let wifi = &mut self.emulator.ds_wifi;
        let Some(identity) = wifi.identity.as_ref() else {
            return;
        };
        if std::mem::take(&mut wifi.changed)
            && let Err(err) = self.settings_store.store_ds_wifi(identity)
        {
            log::error!("could not keep the DS wifi settings: {err}");
        }
        let dns = self.ui.settings.ds_online.dns();
        let relay = self.ui.settings.ds_online.relay();
        wifi.online = dns.is_some() || relay;
        wifi.direct = self.ui.settings.ds_online.direct;
        wifi.relay = relay
            .then(|| self.online.account().map(|a| a.online_id.value()))
            .flatten();
        let firmware = &mut self.emulator.firmware.nds;
        let online = identity.online(dns, relay, firmware.firmware.is_some());
        if firmware.online != online {
            firmware.online = online;
        }
    }

    pub(super) fn sync_ds_user(&mut self) {
        #[cfg(ds_wifi)]
        self.sync_ds_wifi();
        let account = self.online.account().map(|a| a.display_name.as_str());
        let user = &self.ui.settings.ds_user;
        let firmware = &mut self.emulator.firmware.nds;
        let keep = user.keep_firmware_settings && firmware.firmware.is_some();
        let wanted = (!keep).then(|| user.user_settings(account));
        if firmware.user_settings != wanted {
            firmware.user_settings = wanted;
        }
        if let Some(offset) = self.emulator.take_ds_clock_set() {
            self.ui.settings.ds_user.clock = Some(offset);
            self.ui.settings.dirty = true;
        }
        self.emulator
            .set_ds_clock(self.ui.settings.ds_user.clock.unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    use super::DsFile;
    use crate::app::Citrine;
    use crate::app::files::nds_tests::Opened;
    use crate::app::tabs::Tab;
    use crate::app::tests::{info_frame_out, inside_its_clip, play_frame, tap, text_rect};
    use crate::app::ui_state::ds_user::{DsLanguage, DsUser};
    use crate::app::ui_state::settings::{LayoutMode, SettingsTab};
    use crate::utils::file_loader::PickedFile;
    use citrine_gb::gba::wait::Width;
    use citrine_gb::nds::card::{KEY_TABLE_BYTES, KEY_TABLE_OFFSET, KeyTable, encrypt_secure_area};
    use citrine_gb::nds::spi::firmware::{Firmware, UserSettings};
    use egui::Context;

    const SUB_BG_VRAM: u32 = 0x0620_0000;
    const PREFIX: &str = "NICKNAME ";
    const SECURE_AREA_START: usize = 0x4000;
    const SECURE_AREA_END: usize = 0x8000;
    const SECURE_AREA_CRC: usize = 0x6C;
    const SECURE_AREA_CODE: usize = 0x800;
    const ARM9_RAM: u32 = 0x0200_0000;

    fn homebrew() -> PickedFile {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/nds/nickname.nds");
        PickedFile {
            name: "nickname.nds".to_owned(),
            data: std::fs::read(&path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
                .into(),
            path: None,
            seen: None,
        }
    }

    fn screen_text(app: &mut Citrine) -> String {
        app.handle_load_rom(homebrew());
        printed(app)
    }

    fn printed(app: &mut Citrine) -> String {
        let nds = app.emulator.as_nds_mut().expect("a DS");
        for _ in 0..60 {
            nds.run_frame();
        }
        let mut bus = nds.buses_mut().arm9();
        let tiles: Vec<i32> = (0..0x1_0000)
            .map(|index| (bus.read(SUB_BG_VRAM + 2 * index, Width::Half) & 0x3FF) as i32)
            .collect();
        let found = (0..tiles.len() - 32 * 6).find_map(|start| {
            let shift = tiles[start] - i32::from(b'N');
            PREFIX
                .bytes()
                .enumerate()
                .all(|(index, byte)| tiles[start + index] - i32::from(byte) == shift)
                .then_some((start, shift))
        });
        let (start, shift) = found.expect("the homebrew printed its lines");
        tiles[start..start + 32 * 6]
            .chunks(32)
            .map(|row| {
                row.iter()
                    .map(|&tile| match u8::try_from(tile - shift) {
                        Ok(byte) if byte.is_ascii_graphic() => char::from(byte),
                        _ => ' ',
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_owned()
    }

    #[test]
    fn a_homebrew_reads_the_settings_from_the_firmware() {
        let mut opened = Opened::new("ds-user");
        opened.app.ui.settings.ds_user = DsUser {
            nickname: Some("Zitrone".into()),
            birthday: (3, 14),
            language: Some(DsLanguage::French),
            favorite_color: 9,
            keep_firmware_settings: false,
            clock: None,
        };
        opened.app.sync_ds_user();
        assert_eq!(
            screen_text(&mut opened.app),
            "NICKNAME Zitrone\nCOLOR 9\nBIRTHDAY 3/14\nLANGUAGE 2\nFLASH Zitrone"
        );
    }

    #[test]
    fn a_clock_the_game_sets_shows_as_custom() {
        let mut opened = Opened::new("ds-clock");
        let file = opened.picked();
        opened.app.files.rom_tx.send(file).unwrap();
        opened.app.drain_file_channels();
        opened.app.sync_ds_user();
        assert_eq!(opened.app.ui.settings.ds_user.clock, None);
        let nds = opened.app.emulator.nds_mut().expect("a DS game");
        crate::emulator::gimmicks::tests::nds::game_sets_time(nds, [0x08, 0x30, 0x00]);
        let set = nds.buses().rtc().unix();
        opened.app.sync_ds_user();
        let offset = opened.app.ui.settings.ds_user.clock.expect("Custom");
        let device = crate::emulator::gimmicks::clock::local_unix(crate::storage::now_unix())
            .expect("local time");
        assert!((device + offset - set).abs() <= 5);
        assert!(opened.app.ui.settings.dirty);
    }

    fn own_firmware() -> PickedFile {
        let own = UserSettings {
            nickname: "Dump".into(),
            ..UserSettings::default()
        };
        PickedFile {
            name: "firmware.bin".to_owned(),
            data: Firmware::built_in(&own).bytes().into(),
            path: None,
            seen: None,
        }
    }

    #[test]
    fn a_user_firmware_gets_the_settings_unless_it_keeps_its_own() {
        let mut opened = Opened::new("ds-firmware");
        opened.app.ui.settings.ds_user.nickname = Some("Zitrone".into());
        opened
            .app
            .handle_load_ds_file(DsFile::Firmware, own_firmware());
        assert_eq!(opened.app.toasts.texts(), ["Your DS firmware is set"]);
        opened.app.sync_ds_user();
        assert!(screen_text(&mut opened.app).starts_with("NICKNAME Zitrone"));

        opened.app.ui.settings.ds_user.keep_firmware_settings = true;
        opened.app.sync_ds_user();
        assert!(screen_text(&mut opened.app).starts_with("NICKNAME Dump"));
    }

    #[test]
    fn ds_files_are_kept_and_removed() {
        let mut opened = Opened::new("ds-files");
        let arm7 = PickedFile {
            name: "biosnds7.rom".to_owned(),
            data: vec![0; 0x4000].into(),
            path: None,
            seen: None,
        };
        opened.app.handle_load_ds_file(DsFile::Arm7Bios, arm7);
        opened
            .app
            .handle_load_ds_file(DsFile::Firmware, own_firmware());

        let mut restarted = crate::app::tests::phone_app("ds-files-restart");
        restarted.settings_store =
            crate::storage::SettingsStore::at(opened.dir.path().join("settings"));
        restarted.load_ds_files();
        let names = |app: &Citrine| {
            <DsFile as strum::IntoEnumIterator>::iter()
                .map(|file| app.emulator.ds_file_name(file).map(str::to_owned))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&restarted),
            [
                None,
                Some("biosnds7.rom".to_owned()),
                Some("firmware.bin".to_owned())
            ]
        );

        restarted.remove_ds_file(DsFile::Firmware);
        assert_eq!(
            restarted.toasts.texts().last(),
            Some(&"Using the built-in DS firmware")
        );
        let mut again = crate::app::tests::phone_app("ds-files-again");
        again.settings_store =
            crate::storage::SettingsStore::at(opened.dir.path().join("settings"));
        again.load_ds_files();
        assert_eq!(names(&again), [None, Some("biosnds7.rom".to_owned()), None]);
    }

    #[test]
    fn a_file_of_the_wrong_size_is_refused() {
        let mut opened = Opened::new("ds-bad-file");
        let file = PickedFile {
            name: "bios9.bin".to_owned(),
            data: vec![0; 0x100].into(),
            path: None,
            seen: None,
        };
        opened.app.handle_load_ds_file(DsFile::Arm9Bios, file);
        assert_eq!(opened.app.emulator.ds_file_name(DsFile::Arm9Bios), None);
        assert_eq!(
            opened.app.toasts.texts(),
            ["'bios9.bin' isn't a DS ARM9 BIOS. A DS ARM9 BIOS file is 4 KB."]
        );
    }

    #[test]
    fn the_settings_fit_the_desktop_dock() {
        let mut opened = Opened::new("ds-settings-desktop");
        let app = &mut opened.app;
        app.ui.settings.layout = LayoutMode::Desktop;
        app.ui.settings.current_tab = SettingsTab::General;
        app.open_tab(Tab::Settings);
        let ctx = Context::default();
        for width in [560.0, 1280.0] {
            info_frame_out(app, &ctx, width, 1600.0);
            let output = info_frame_out(app, &ctx, width, 1600.0);
            for text in ["Nickname", "Colour", "Firmware"] {
                assert!(inside_its_clip(&output, text), "{text} at {width}");
            }
        }
    }

    #[test]
    fn the_phone_picks_a_birthday_month_then_day() {
        let mut opened = Opened::new("ds-settings-phone");
        let app = &mut opened.app;
        app.mobile.nav.open_settings(SettingsTab::General);
        let ctx = Context::default();
        let tap_text = |app: &mut Citrine, text: &str| {
            play_frame(app, &ctx, Vec::new());
            let output = play_frame(app, &ctx, Vec::new());
            let rect = text_rect(&output, text).unwrap_or_else(|| panic!("{text} on screen"));
            play_frame(app, &ctx, tap(rect.center()));
        };
        tap_text(app, "Birthday");
        tap_text(app, "July");
        tap_text(app, "July 5");
        assert_eq!(app.ui.settings.ds_user.birthday, (7, 5));
    }

    fn encrypted() -> PickedFile {
        let mut file = homebrew();
        file.name = "encrypted.nds".to_owned();
        let mut data = file.data.to_vec();
        data[SECURE_AREA_START..SECURE_AREA_START + 8]
            .copy_from_slice(b"\x91\x6e\x36\xb0\x0f\xe4\x56\x76");
        let crc =
            citrine_gb::utils::crc16::crc16(0xFFFF, &data[SECURE_AREA_START..SECURE_AREA_END]);
        data[SECURE_AREA_CRC..SECURE_AREA_CRC + 2].copy_from_slice(&crc.to_le_bytes());
        file.data = data.into();
        file
    }

    #[test]
    fn an_encrypted_game_asks_for_the_arm7_bios_and_shows_its_row() {
        let mut opened = Opened::new("ds-needs-bios");
        let app = &mut opened.app;
        app.handle_load_rom(encrypted());
        assert!(app.toasts.texts().contains(&super::NEEDS_BIOS));
        assert_eq!(app.toasts.button_labels(), ["Add BIOS"]);
        let ctx = Context::default();
        play_frame(app, &ctx, Vec::new());
        let output = play_frame(app, &ctx, Vec::new());
        let button = text_rect(&output, "Add BIOS").expect("the toast's button");
        play_frame(app, &ctx, tap(button.center()));
        for _ in 0..30 {
            play_frame(app, &ctx, Vec::new());
        }
        let output = play_frame(app, &ctx, Vec::new());
        let row = text_rect(&output, DsFile::Arm7Bios.label()).expect("the BIOS row");
        assert!(row.min.y >= 0.0 && row.max.y <= 915.0, "{row:?}");
        assert!(!app.ui.settings.find_ds_bios);
    }

    fn key_bios() -> PickedFile {
        let mut data = citrine_gb::nds::machine::FREE_ARM7_BIOS.to_vec();
        for (index, byte) in data[KEY_TABLE_OFFSET..KEY_TABLE_OFFSET + KEY_TABLE_BYTES]
            .iter_mut()
            .enumerate()
        {
            *byte = (index.wrapping_mul(167) ^ (index >> 5)) as u8;
        }
        PickedFile {
            name: "biosnds7.rom".to_owned(),
            data: data.into(),
            path: None,
            seen: None,
        }
    }

    fn plain_secure_code() -> Vec<u8> {
        (0..SECURE_AREA_CODE - 8).map(|index| index as u8).collect()
    }

    fn sealed(table: &KeyTable) -> PickedFile {
        let mut file = homebrew();
        file.name = "sealed.nds".to_owned();
        let mut data = file.data.to_vec();
        let gamecode = u32::from_le_bytes(data[0x0C..0x10].try_into().unwrap());
        let area = &mut data[SECURE_AREA_START..SECURE_AREA_END];
        area[..8].copy_from_slice(b"encryObj");
        area[8..SECURE_AREA_CODE].copy_from_slice(&plain_secure_code());
        encrypt_secure_area(area, table, gamecode);
        let crc =
            citrine_gb::utils::crc16::crc16(0xFFFF, &data[SECURE_AREA_START..SECURE_AREA_END]);
        data[SECURE_AREA_CRC..SECURE_AREA_CRC + 2].copy_from_slice(&crc.to_le_bytes());
        file.data = data.into();
        file
    }

    fn secure_code_in_ram(app: &mut Citrine) -> Vec<u8> {
        let nds = app.emulator.as_nds_mut().expect("a DS");
        let mut bus = nds.buses_mut().arm9();
        (8..SECURE_AREA_CODE as u32)
            .step_by(4)
            .flat_map(|offset| bus.read(ARM9_RAM + offset, Width::Word).to_le_bytes())
            .collect()
    }

    #[test]
    fn an_encrypted_game_starts_once_its_arm7_bios_is_added() {
        let mut opened = Opened::new("ds-sealed");
        let app = &mut opened.app;
        let bios = key_bios();
        let table = KeyTable::from_arm7_bios(&bios.data).expect("a key table");
        app.handle_load_rom(sealed(&table));
        assert!(app.toasts.open_texts().contains(&super::NEEDS_BIOS));
        assert_ne!(secure_code_in_ram(app), plain_secure_code());

        app.handle_load_ds_file(DsFile::Arm7Bios, bios);
        assert!(!app.toasts.open_texts().contains(&super::NEEDS_BIOS));
        assert_eq!(app.toasts.open_texts().last(), Some(&super::BIOS_SAVED));
        assert_eq!(
            app.emulator.ds_file_name(DsFile::Arm7Bios),
            Some("biosnds7.rom")
        );
        assert_eq!(secure_code_in_ram(app), plain_secure_code());
        assert!(printed(app).starts_with(PREFIX));
    }

    #[test]
    fn the_bios_notice_stays_until_it_is_pressed() {
        let mut opened = Opened::new("ds-bios-stays");
        let app = &mut opened.app;
        app.handle_load_rom(encrypted());
        let ctx = Context::default();
        let tenth_of_a_second = |app: &mut Citrine| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 915.0),
                )),
                predicted_dt: 0.1,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                app.render(ctx);
                app.show_toasts(ctx);
            })
        };
        for _ in 0..100 {
            tenth_of_a_second(app);
        }
        app.toasts.dismiss_all_toasts();
        app.toasts.success(super::BIOS_SAVED);
        tenth_of_a_second(app);
        let output = tenth_of_a_second(app);
        assert!(text_rect(&output, "Add BIOS").is_some());
        assert_eq!(
            app.toasts.open_texts(),
            [super::NEEDS_BIOS, super::BIOS_SAVED]
        );
        for wanted in [super::NEEDS_BIOS, super::BIOS_SAVED] {
            let elided = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.galley.elided)
                    }
                    _ => None,
                });
            assert_eq!(
                elided,
                Some(false),
                "{wanted}: None is not drawn, true is cut"
            );
        }
    }

    #[test]
    fn the_desktop_scrolls_to_the_arm7_bios_row() {
        let mut opened = Opened::new("ds-find-desktop");
        let app = &mut opened.app;
        app.ui.settings.layout = LayoutMode::Desktop;
        app.find_ds_bios(false);
        let ctx = Context::default();
        for _ in 0..4 {
            info_frame_out(app, &ctx, 1280.0, 600.0);
        }
        let output = info_frame_out(app, &ctx, 1280.0, 600.0);
        assert!(inside_its_clip(&output, DsFile::Arm7Bios.label()));
        assert!(!app.ui.settings.find_ds_bios);
    }

    #[test]
    fn a_set_bios_or_a_plain_game_asks_for_nothing() {
        let mut opened = Opened::new("ds-has-bios");
        let app = &mut opened.app;
        app.handle_load_rom(homebrew());
        assert!(!app.toasts.texts().contains(&super::NEEDS_BIOS));
        let arm7 = PickedFile {
            name: "biosnds7.rom".to_owned(),
            data: vec![0; 0x4000].into(),
            path: None,
            seen: None,
        };
        app.handle_load_ds_file(DsFile::Arm7Bios, arm7);
        app.handle_load_rom(encrypted());
        assert!(!app.toasts.texts().contains(&super::NEEDS_BIOS));
    }
}
