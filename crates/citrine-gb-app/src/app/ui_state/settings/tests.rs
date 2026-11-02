use super::*;
use crate::app::ui_state::palette::CustomPalette;

#[test]
fn the_friends_volume_is_kept_and_starts_at_half() {
    let fresh = Settings::default();
    assert_eq!(fresh.online.friend_volume, 0.5);
    assert!(!fresh.online.friend_muted);
    let mut settings = Settings::default();
    settings.online.friend_volume = 0.3;
    settings.online.friend_muted = true;
    let json = serde_json::to_string(&settings).unwrap();
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.online.friend_volume, 0.3);
    assert!(back.online.friend_muted);
    let older: Settings =
        serde_json::from_str(r#"{ "online": { "show_playing": false } }"#).unwrap();
    assert_eq!(older.online.friend_volume, 0.5);
}

#[test]
fn my_mute_and_the_friends_mute_are_separate() {
    let settings = Settings {
        volume: 0.8,
        muted: true,
        ..Default::default()
    };
    assert_eq!(settings.sound_levels().mine, 0.0);
    assert_eq!(settings.sound_levels().friend, 0.25);
    let mut settings = Settings {
        volume: 0.5,
        ..Default::default()
    };
    settings.online.friend_muted = true;
    assert_eq!(settings.sound_levels().mine, 0.25);
    assert_eq!(settings.sound_levels().friend, 0.0);
}

#[test]
fn a_custom_palette_wins_over_the_builtin_choice() {
    let mut settings = Settings::default();
    let mine = CustomPalette {
        name: "Mine".into(),
        colors: [[9, 9, 9], [8, 8, 8], [7, 7, 7], [6, 6, 6]],
    };
    settings.palettes.upsert(mine.clone());
    settings.custom_palette = Some("Mine".into());

    assert_eq!(settings.effective_theme(), mine.to_theme());
}

#[test]
fn a_deleted_palette_falls_back_to_the_builtin() {
    let mut settings = Settings {
        dmg_theme: DmgTheme::Pocket,
        custom_palette: Some("Gone".into()),
        ..Default::default()
    };

    assert_eq!(
        settings.effective_theme(),
        DmgTheme::Pocket,
        "a name with nothing behind it must not blank the screen"
    );

    settings.sanitize();
    assert_eq!(
        settings.custom_palette, None,
        "the dangling name is dropped"
    );
}

#[test]
fn the_desktop_ui_keeps_its_css_size_at_every_pixel_ratio_and_zoom() {
    let settings = Settings::default();
    let one_x = settings.desktop_pixels_per_point(1.0);
    assert_eq!(one_x, Settings::DEFAULT_UI_SCALE);
    for ratio in [1.0, 2.0, 1.25, 2.5, 3.0] {
        let ppp = settings.desktop_pixels_per_point(ratio);
        assert!(
            (ppp / ratio - one_x).abs() < 1e-5,
            "ratio {ratio}: {ppp} points per pixel"
        );
    }
}

#[test]
fn a_ui_scale_from_an_older_build_keeps_its_size_on_the_same_screen() {
    let load = |json: &str| -> Settings { serde_json::from_str(json).expect("loads") };

    let mut chosen = load(r#"{ "ui_scale": 3.0 }"#);
    assert!(!chosen.ui_scale_per_ratio);
    let old_ppp = chosen.ui_scale;
    chosen.adopt_pixel_ratio(2.0);
    assert_eq!(chosen.ui_scale, 1.5);
    assert_eq!(chosen.desktop_pixels_per_point(2.0), old_ppp);
    assert!(chosen.ui_scale_per_ratio);
    chosen.adopt_pixel_ratio(2.0);
    assert_eq!(chosen.ui_scale, 1.5, "adopted once");

    let mut untouched = load(&format!(
        r#"{{ "ui_scale": {} }}"#,
        Settings::DEFAULT_UI_SCALE
    ));
    untouched.adopt_pixel_ratio(2.0);
    assert_eq!(untouched.ui_scale, Settings::DEFAULT_UI_SCALE);

    let mut one_x = load(r#"{ "ui_scale": 3.0 }"#);
    one_x.adopt_pixel_ratio(1.0);
    assert_eq!(one_x.ui_scale, 3.0);

    let mut fresh = Settings {
        ui_scale: 3.0,
        ..Settings::default()
    };
    fresh.adopt_pixel_ratio(2.0);
    assert_eq!(fresh.ui_scale, 3.0);
}

#[test]
fn sanitize_clears_the_unusable_custom_theme_from_older_builds() {
    let mut settings = Settings {
        dmg_theme: DmgTheme::Custom(Default::default()),
        ..Default::default()
    };

    settings.sanitize();

    assert_eq!(settings.dmg_theme, DmgTheme::default());
    assert_eq!(settings.effective_theme(), DmgTheme::default());
}

#[test]
fn sanitize_pulls_out_of_range_values_back_in() {
    let mut settings = Settings {
        ui_scale: 99.0,
        volume: -3.0,
        audio_queue_ms: 5000,
        autosave_keep: 0,
        ..Default::default()
    };

    settings.sanitize();
    assert_eq!(settings.autosave_keep, 1);
    settings.autosave_keep = 900;
    settings.sanitize();
    assert_eq!(settings.autosave_keep, autosave::MAX_KEEP);

    assert_eq!(settings.ui_scale, 5.0);
    assert_eq!(settings.volume, 0.0);
    assert_eq!(settings.audio_queue_ms, crate::audio::MAX_LATENCY_MS);
}

#[test]
fn renaming_a_palette_keeps_everything_that_points_at_it() {
    let mine = CustomPalette {
        name: "Mine".into(),
        colors: [[9, 9, 9], [8, 8, 8], [7, 7, 7], [6, 6, 6]],
    };
    let mut settings = Settings {
        dmg_theme: DmgTheme::Pocket,
        custom_palette: Some("Mine".into()),
        ..Default::default()
    };
    settings.palettes.upsert(mine.clone());
    settings.set_game_palette("game", Some(PaletteChoice::Custom("Mine".into())));
    settings.set_game_palette("other", Some(PaletteChoice::Builtin(DmgTheme::Aqua)));

    assert!(settings.rename_palette("Mine", "  Ours  "));

    assert_eq!(settings.custom_palette.as_deref(), Some("Ours"));
    assert_eq!(
        settings.game_palette(Some("game")),
        Some(&PaletteChoice::Custom("Ours".into())),
        "the game follows the palette to its new name"
    );
    assert_eq!(settings.theme_for(Some("game")), mine.to_theme());
    assert_eq!(
        settings.game_palette(Some("other")),
        Some(&PaletteChoice::Builtin(DmgTheme::Aqua)),
        "games on other palettes are untouched"
    );
}

#[test]
fn a_refused_rename_leaves_the_games_alone() {
    let mut settings = Settings::default();
    for name in ["Mine", "Taken"] {
        settings.palettes.upsert(CustomPalette {
            name: name.into(),
            colors: [[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]],
        });
    }
    settings.set_game_palette("game", Some(PaletteChoice::Custom("Mine".into())));

    assert!(!settings.rename_palette("Mine", "Taken"));

    assert_eq!(
        settings.game_palette(Some("game")),
        Some(&PaletteChoice::Custom("Mine".into()))
    );
}

#[test]
fn settings_saved_by_an_older_build_still_load() {
    let stored = r#"{
        "ui_scale": 2.5,
        "ui_theme": "CatppuccinFrappe",
        "dmg_theme": "Pocket",
        "matrix": true,
        "ghosting": true,
        "matrix_edge_darkness": 0.2,
        "matrix_corner_darkness": 0.3,
        "ghosting_strength": 0.4,
        "volume": 0.8,
        "current_tab": "Sound",
        "quick_slot": 3,
        "randomized_ram": true,
        "dev_mode": true,
        "focus_mode": false,
        "track_pc": true
    }"#;

    let settings: Settings = serde_json::from_str(stored).expect("old settings still load");

    assert_eq!(settings.ui_scale, 2.5);
    assert_eq!(settings.dmg_theme, DmgTheme::Pocket);
    assert_eq!(settings.quick_slot, 3);
    assert!(settings.dev_mode);
    assert_eq!(settings.audio_queue_ms, DEFAULT_LATENCY_MS);
    assert_eq!(settings.channels, [true; 4]);
    assert_eq!(settings.layout, LayoutMode::Auto);
    assert!(settings.bindings.is_default());
    assert_eq!(settings.autosave_interval, AutosaveInterval::FiveMinutes);
    assert_eq!(settings.autosave_keep, autosave::DEFAULT_KEEP);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn autosave_choices_persist() {
    let root =
        std::env::temp_dir().join(format!("citrine-autosave-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = crate::storage::SettingsStore::at(root.clone());
    let settings = Settings {
        autosave_interval: AutosaveInterval::FifteenMinutes,
        autosave_keep: 25,
        ..Default::default()
    };
    store.store(&settings).expect("store");

    let back: Settings = store.load().expect("load");
    assert_eq!(back.autosave_interval, AutosaveInterval::FifteenMinutes);
    assert_eq!(back.autosave_keep, 25);
    let _ = std::fs::remove_dir_all(root);
}

const BOTH: &str = "BOTH";

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_game_console_persists() {
    let root = std::env::temp_dir().join(format!("citrine-game-console-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = crate::storage::SettingsStore::at(root.clone());
    let mut settings = Settings::default();
    settings.set_game_console(BOTH, GbModel::Dmg);
    store.store(&settings).expect("store");

    let back: Settings = store.load().expect("load");
    assert_eq!(back.console_for(BOTH, RomCgbMode::CgbAndGb), GbModel::Dmg);
    assert_eq!(
        back.console_for("OTHER", RomCgbMode::CgbAndGb),
        GbModel::Cgb,
        "another game keeps the default"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn settings_from_before_game_consoles_still_load() {
    let old = r#"{"console":"GameBoy","volume":0.5}"#;
    let settings: Settings = serde_json::from_str(old).expect("old settings load");
    assert_eq!(settings.console, ConsoleChoice::GameBoy);
    assert!(settings.game_consoles.is_empty());
}

#[test]
fn the_default_applies_without_a_game_choice() {
    let mut settings = Settings::default();
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbAndGb),
        GbModel::Cgb
    );
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbOnly),
        GbModel::Cgb
    );
    assert_eq!(settings.console_for(BOTH, RomCgbMode::None), GbModel::Dmg);

    settings.console = ConsoleChoice::GameBoy;
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbAndGb),
        GbModel::Dmg
    );
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbOnly),
        GbModel::Dmg,
        "only the default puts a Color-only game on a Game Boy"
    );
}

#[test]
fn a_game_choice_only_counts_for_games_that_run_on_both() {
    let mut settings = Settings::default();
    settings.game_consoles.insert(BOTH.into(), GbModel::Dmg);
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbAndGb),
        GbModel::Dmg
    );
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbOnly),
        GbModel::Cgb
    );
    settings.game_consoles.insert(BOTH.into(), GbModel::Cgb);
    assert_eq!(settings.console_for(BOTH, RomCgbMode::None), GbModel::Dmg);
}

#[test]
fn only_games_that_run_on_both_offer_the_choice() {
    assert!(plays_on_both(RomCgbMode::CgbAndGb));
    assert!(!plays_on_both(RomCgbMode::CgbOnly));
    assert!(!plays_on_both(RomCgbMode::None));
    let shelf: Vec<&str> = crate::homebrew::HOMEBREW_GAMES
        .iter()
        .filter(|game| plays_on_both(game.cgb_mode))
        .map(|game| game.id)
        .collect();
    assert_eq!(shelf, ["libbet", "tobu_deluxe"]);
}

#[test]
fn picking_what_the_default_gives_forgets_the_game_choice() {
    let mut settings = Settings::default();
    settings.set_game_console(BOTH, GbModel::Dmg);
    assert_eq!(settings.game_consoles.len(), 1);
    settings.set_game_console(BOTH, GbModel::Cgb);
    assert!(settings.game_consoles.is_empty());
    assert_eq!(
        settings.console_for(BOTH, RomCgbMode::CgbAndGb),
        GbModel::Cgb
    );
}

#[test]
fn a_rom_resolves_its_console_from_its_header() {
    let mut data = vec![0u8; 0x8000];
    data[0x143] = 0x80;
    let rom = Rom::new(&data);
    let key = rom.header().unwrap().sha256_hex_string();
    let mut settings = Settings::default();
    assert_eq!(settings.console_for_rom(&rom), GbModel::Cgb);
    settings.set_game_console(&key, GbModel::Dmg);
    assert_eq!(settings.console_for_rom(&rom), GbModel::Dmg);
}
