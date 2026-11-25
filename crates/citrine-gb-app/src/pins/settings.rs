use crate::app::ui_state::ds_3d::{DistantTextures, Resolution, TextureFilter};
use crate::app::ui_state::settings::Settings;
use crate::storage::SettingsStore;
use serde_json::Value;

const RELEASES: [(&str, &str); 2] = [
    (
        "v0.12.0",
        include_str!("../../fixtures/settings-v0.12.0.json"),
    ),
    (
        "v0.13.0-c12",
        include_str!("../../fixtures/settings-v0.13.0-c12.json"),
    ),
];

fn stored(raw: &str) -> (tempfile::TempDir, SettingsStore) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("settings")).unwrap();
    std::fs::write(dir.path().join("settings/settings.json"), raw).unwrap();
    let store = SettingsStore::at(dir.path().to_path_buf());
    (dir, store)
}

fn strict(raw: &str) -> Value {
    serde_json::to_value(serde_json::from_str::<Settings>(raw).unwrap()).unwrap()
}

fn signed_in(settings: &Settings) -> bool {
    settings.online.joined.as_deref() == Some("relay.example")
        && settings.online.relays["relay.example"].relay_id == "fixture relay id"
        && settings
            .game_names
            .name_or(Some("POKEMON SILVER|0123ABCD"), "")
            == "Pokémon Silver"
}

#[test]
fn settings_from_each_release_load_whole() {
    for (tag, raw) in RELEASES {
        if let Err(err) = serde_json::from_str::<Settings>(raw) {
            panic!("{tag} settings no longer load: {err}");
        }
        let (_dir, store) = stored(raw);
        let settings = store.load::<Settings>().unwrap();
        assert!(signed_in(&settings), "{tag} keeps the sign-in");
        assert!(store.unreadable().is_none(), "{tag} keeps no copy");
    }
}

#[test]
fn ds_3d_textures_from_0_12_stay_nearest_until_picked() {
    let (_dir, store) = stored(RELEASES[0].1);
    let mut settings = store.load::<Settings>().unwrap();
    assert_eq!(settings.ds_3d.resolution, Resolution::Double);
    assert_eq!(settings.ds_3d.texture_filter, TextureFilter::Nearest);
    assert_eq!(settings.ds_3d.distant_textures, DistantTextures::Off);

    settings.ds_3d.texture_filter = TextureFilter::Bicubic;
    settings.ds_3d.distant_textures = DistantTextures::Anisotropic4x;
    store.store(&settings).unwrap();

    let back = store.load::<Settings>().unwrap();
    assert_eq!(back.ds_3d, settings.ds_3d);
    let fresh = Settings::default();
    store.store(&fresh).unwrap();
    assert_eq!(store.load::<Settings>().unwrap().ds_3d, fresh.ds_3d);
    assert_eq!(fresh.ds_3d.texture_filter, TextureFilter::Bicubic);
}

#[test]
fn a_value_that_cannot_be_read_falls_back_alone() {
    let raw = RELEASES[0].1;
    let field = "\n  \"ghosting_strength\": ";
    let start = raw.find(field).unwrap() + field.len();
    let end = start + raw[start..].find(',').unwrap();
    let broken = format!("{}null{}", &raw[..start], &raw[end..]);
    let (_dir, store) = stored(&broken);

    let settings = store.load::<Settings>().unwrap();

    let mut expected = strict(raw);
    expected["ghosting_strength"] = Settings::default().ghosting_strength.into();
    let loaded = serde_json::to_value(&settings).unwrap();
    for (key, value) in expected.as_object().unwrap() {
        assert_eq!(loaded[key], *value, "{key} loads as a whole file loads it");
    }
    assert!(signed_in(&settings));
    assert_eq!(store.unreadable().as_deref(), Some(broken.as_bytes()));
}

#[test]
fn a_cut_off_file_keeps_the_values_before_the_cut() {
    let raw = RELEASES[0].1;
    let cut = (0..=raw.len() / 2)
        .rev()
        .find(|at| raw.is_char_boundary(*at))
        .unwrap();
    let (_dir, store) = stored(&raw[..cut]);

    let settings = serde_json::to_value(store.load::<Settings>().unwrap()).unwrap();

    let Value::Object(whole) = serde_json::from_str::<Value>(raw).unwrap() else {
        panic!("settings are an object");
    };
    let expected = strict(raw);
    let mut ends = whole
        .keys()
        .map(|key| (raw.find(&format!("\n  \"{key}\":")).unwrap(), key))
        .collect::<Vec<_>>();
    ends.sort();
    let kept = ends
        .windows(2)
        .take_while(|pair| pair[1].0 < cut)
        .map(|pair| pair[0].1)
        .collect::<Vec<_>>();
    assert!(!kept.is_empty());
    for key in kept {
        assert_eq!(settings[key], expected[key], "{key} survives the cut");
    }
    assert_eq!(store.unreadable().as_deref(), Some(&raw.as_bytes()[..cut]));
}
