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
    assert_eq!(value(&settings, SettingsTab::Sound), "80%");
    let muted = Settings {
        muted: true,
        ..Settings::default()
    };
    assert_eq!(value(&muted, SettingsTab::Sound), "Muted");
    assert_eq!(value(&settings, SettingsTab::Online), "Signed out");
    assert_eq!(value(&settings, SettingsTab::YourData), "");
}
