use super::*;

#[test]
fn about_follows_your_data_and_developer_shows_only_in_dev_mode() {
    let plain = entries(false);
    assert_eq!(plain.last(), Some(&Entry::About));
    assert!(!plain.contains(&Entry::Page(SettingsTab::Developer)));
    assert_eq!(
        entries(true).last(),
        Some(&Entry::Page(SettingsTab::Developer))
    );
}

#[test]
fn rows_show_the_value_each_page_holds() {
    let settings = Settings {
        volume: 0.8,
        ..Settings::default()
    };
    assert_eq!(value(&settings, SettingsTab::Sound, ConsoleKind::Gb), "80%");
    let muted = Settings {
        muted: true,
        ..Settings::default()
    };
    assert_eq!(value(&muted, SettingsTab::Sound, ConsoleKind::Gb), "Muted");
    assert_eq!(
        value(&settings, SettingsTab::Online, ConsoleKind::Gb),
        "Signed out"
    );
    assert_eq!(value(&settings, SettingsTab::YourData, ConsoleKind::Gb), "");
}

#[test]
fn a_gba_game_names_the_style_page_after_its_console_and_shows_the_screen() {
    let settings = Settings::default();
    assert_eq!(SettingsTab::Style.title_for(ConsoleKind::Gb), "GB style");
    assert_eq!(SettingsTab::Style.title_for(ConsoleKind::Gba), "GBA style");
    assert_eq!(SettingsTab::Sound.title_for(ConsoleKind::Gba), "Sound");
    assert_eq!(
        value(&settings, SettingsTab::Style, ConsoleKind::Gba),
        GbaScreen(settings.color_correction).to_string()
    );
    assert_eq!(
        value(&settings, SettingsTab::Style, ConsoleKind::Gb),
        settings.dmg_theme.to_string()
    );
}
