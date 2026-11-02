use crate::app::ui_state::settings::Settings;
use crate::online::synced::Values;
use serde_json::{Map, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Whole,
    Map,
    Named,
}

const SYNCED: &[(&str, Kind)] = &[
    ("ui_theme", Kind::Whole),
    ("console", Kind::Whole),
    ("game_consoles", Kind::Map),
    ("game_names", Kind::Map),
    ("dmg_theme", Kind::Whole),
    ("custom_palette", Kind::Whole),
    ("gbc_palette", Kind::Whole),
    ("palettes.entries", Kind::Named),
    ("game_palettes", Kind::Map),
    ("color_correction", Kind::Whole),
    ("scaling", Kind::Whole),
    ("matrix", Kind::Whole),
    ("ghosting", Kind::Whole),
    ("matrix_edge_darkness", Kind::Whole),
    ("matrix_corner_darkness", Kind::Whole),
    ("ghosting_strength", Kind::Whole),
    ("bindings.keyboard", Kind::Whole),
    ("hotkeys", Kind::Whole),
    ("online.show_playing", Kind::Whole),
    ("local_link.swapped", Kind::Whole),
    ("local_link.second_sound", Kind::Whole),
    ("local_link.left_keys", Kind::Whole),
    ("local_link.right_keys", Kind::Whole),
    ("autosave_interval", Kind::Whole),
    ("autosave_keep", Kind::Whole),
    ("screenshot_scale", Kind::Whole),
    ("recording_scale", Kind::Whole),
    ("randomized_ram", Kind::Whole),
    ("dev_mode", Kind::Whole),
    ("track_pc", Kind::Whole),
    ("stats_range", Kind::Whole),
];

#[cfg_attr(not(test), allow(dead_code))]
const LOCAL: &[&str] = &[
    "ui_scale",
    "ui_scale_per_ratio",
    "layout",
    "volume",
    "muted",
    "audio_queue_ms",
    "audio_device",
    "channels",
    "bindings.gamepad",
    "bindings.stick_as_dpad",
    "bindings.stick_deadzone",
    "touch",
    "online.address",
    "online.joined",
    "online.relays",
    "online.cloud_saves",
    "online.sync_settings",
    "online.friend_volume",
    "online.friend_muted",
    "local_link.controls",
    "current_tab",
    "quick_slot",
    "focus_mode",
    "focus_place",
    "discord",
    "synced",
];

fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(root, |value, part| value.get(part))
}

fn lookup_mut<'a>(root: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    path.split('.')
        .try_fold(root, |value, part| value.get_mut(part))
}

fn prefix(path: &str) -> String {
    format!("{path}/")
}

pub fn is_synced_key(key: &str) -> bool {
    SYNCED.iter().any(|(path, kind)| match kind {
        Kind::Whole => key == *path,
        Kind::Map | Kind::Named => key.starts_with(&prefix(path)),
    })
}

pub fn synced_values(settings: &Settings) -> Values {
    let root = serde_json::to_value(settings).unwrap_or_default();
    let mut out = Values::new();
    for (path, kind) in SYNCED {
        let Some(value) = lookup(&root, path) else {
            continue;
        };
        match kind {
            Kind::Whole => {
                out.insert((*path).to_owned(), value.clone());
            }
            Kind::Map => {
                for (key, item) in value.as_object().into_iter().flatten() {
                    out.insert(format!("{path}/{key}"), item.clone());
                }
            }
            Kind::Named => {
                for item in value.as_array().into_iter().flatten() {
                    if let Some(name) = item.get("name").and_then(Value::as_str) {
                        out.insert(format!("{path}/{name}"), item.clone());
                    }
                }
            }
        }
    }
    out
}

fn children<'a>(values: &'a Values, path: &str) -> impl Iterator<Item = (&'a str, &'a Value)> {
    let prefix = prefix(path);
    values
        .range(prefix.clone()..)
        .take_while(move |(key, _)| key.starts_with(&prefix))
        .map(move |(key, value)| (&key[path.len() + 1..], value))
}

pub fn adopt(settings: &mut Settings, values: &Values) -> bool {
    let Ok(before) = serde_json::to_value(&*settings) else {
        return false;
    };
    let mut root = before.clone();
    for (path, kind) in SYNCED {
        let Some(slot) = lookup_mut(&mut root, path) else {
            continue;
        };
        match kind {
            Kind::Whole => {
                if let Some(value) = values.get(*path) {
                    *slot = value.clone();
                }
            }
            Kind::Map => {
                let map: Map<String, Value> = children(values, path)
                    .map(|(key, value)| (key.to_owned(), value.clone()))
                    .collect();
                *slot = Value::Object(map);
            }
            Kind::Named => {
                let mut wanted: Vec<(&str, &Value)> = children(values, path).collect();
                let mut items = Vec::new();
                for item in slot.as_array().into_iter().flatten() {
                    let name = item.get("name").and_then(Value::as_str);
                    if let Some(at) = wanted.iter().position(|(n, _)| Some(*n) == name) {
                        items.push(wanted.remove(at).1.clone());
                    }
                }
                items.extend(wanted.into_iter().map(|(_, value)| value.clone()));
                *slot = Value::Array(items);
            }
        }
    }
    if root == before {
        return false;
    }
    let Ok(mut next) = serde_json::from_value::<Settings>(root) else {
        log::warn!("synced settings could not be read");
        return false;
    };
    next.scale_drag = settings.scale_drag;
    next.sanitize();
    *settings = next;
    true
}

pub fn note(settings: &mut Settings, now: u64) -> bool {
    let current = synced_values(settings);
    let defaults = synced_values(&Settings::default());
    settings
        .synced
        .note(&current, &defaults, is_synced_key, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::palette::{CustomPalette, PaletteChoice};
    use crate::app::ui_state::ui_theme::UiTheme;
    use citrine_gb::gb::ppu::types::theme::DmgTheme;

    fn walk(value: &Value, path: &str, found: &mut Vec<String>) {
        let Some(object) = value.as_object() else {
            return;
        };
        for (key, child) in object {
            let path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            let synced = SYNCED.iter().any(|(p, _)| *p == path);
            let local = LOCAL.contains(&path.as_str());
            assert!(!(synced && local), "{path} is in both lists");
            if synced || local {
                continue;
            }
            let parent = SYNCED
                .iter()
                .any(|(p, _)| p.starts_with(&format!("{path}.")));
            if parent && child.is_object() {
                walk(child, &path, found);
            } else {
                found.push(path);
            }
        }
    }

    #[test]
    fn every_setting_is_either_synced_or_kept_on_the_device() {
        let root = serde_json::to_value(Settings::default()).unwrap();
        let mut unsorted = Vec::new();
        walk(&root, "", &mut unsorted);
        assert!(
            unsorted.is_empty(),
            "put these in SYNCED or LOCAL in synced.rs: {unsorted:?}"
        );
        for (path, _) in SYNCED {
            assert!(lookup(&root, path).is_some(), "{path} is not a setting");
        }
        for path in LOCAL {
            assert!(lookup(&root, path).is_some(), "{path} is not a setting");
        }
    }

    const MARK: &str = "LOCAL-ONLY-MARK";

    fn device_settings() -> Settings {
        let mut s = Settings {
            ui_scale: 2.75,
            volume: 0.123,
            muted: true,
            audio_queue_ms: 77,
            audio_device: Some(format!("{MARK} speakers")),
            channels: [true, false, true, false],
            focus_mode: true,
            quick_slot: 7,
            ..Default::default()
        };
        s.online.address = format!("{MARK}.example:1234");
        s.online.remember(
            &crate::online::ServerAddress::parse("relay.example:7000").unwrap(),
            &citrine_gb_net::RelayId::from_bytes([9; 32]),
            &format!("{MARK} password"),
            &citrine_gb_net::Account {
                online_id: citrine_gb_net::OnlineId::random(),
                display_name: format!("{MARK} name"),
                storage_used: 0,
                quota_bytes: 0,
                two_factor: false,
                codes_left: 0,
            },
            &Default::default(),
        );
        s.touch.opacity = 0.321;
        s.touch.mirrored = true;
        s.bindings.stick_deadzone = 99;
        s
    }

    #[test]
    fn the_local_set_never_leaves_the_device() {
        let mut settings = device_settings();
        note(&mut settings, 1_000);
        let blob = settings.synced.to_bytes();
        let text = String::from_utf8(blob).unwrap();
        assert!(!text.contains(MARK), "{text}");
        for key in settings.synced.keys() {
            assert!(is_synced_key(key), "{key} left the device");
            assert!(
                !LOCAL
                    .iter()
                    .any(|l| key == *l || key.starts_with(&format!("{l}/"))),
                "{key} is local"
            );
        }
        for number in ["2.75", "0.123", "0.321", "\"gamepad\"", "99"] {
            assert!(!text.contains(number), "{number} left the device");
        }
    }

    #[test]
    fn adopting_synced_values_leaves_the_local_set_alone() {
        let mut desk = Settings {
            ui_theme: UiTheme::default(),
            ..Default::default()
        };
        desk.palettes
            .upsert(CustomPalette::parse_code("Lemon", "FFF-CC0-880-220").unwrap());
        desk.game_palettes
            .insert("abc".into(), PaletteChoice::Builtin(DmgTheme::Pocket));
        desk.matrix = true;
        let values = synced_values(&desk);

        let mut phone = device_settings();
        let before_local = serde_json::to_value(&phone).unwrap();
        assert!(adopt(&mut phone, &values));
        assert!(phone.matrix);
        assert!(phone.palettes.contains("Lemon"));
        assert_eq!(phone.game_palettes.len(), 1);
        let after = serde_json::to_value(&phone).unwrap();
        for path in LOCAL.iter().filter(|p| **p != "synced") {
            assert_eq!(lookup(&before_local, path), lookup(&after, path), "{path}");
        }
        assert!(
            !adopt(&mut phone, &values),
            "a second adopt changes nothing"
        );
    }

    #[test]
    fn palettes_keep_their_order_and_new_ones_go_last() {
        let mut settings = Settings::default();
        for name in ["Zest", "Apple"] {
            settings
                .palettes
                .upsert(CustomPalette::parse_code(name, "000-555-AAA-FFF").unwrap());
        }
        let mut values = synced_values(&settings);
        values.insert(
            "palettes.entries/Mango".into(),
            serde_json::to_value(CustomPalette::parse_code("Mango", "111-222-333-444").unwrap())
                .unwrap(),
        );
        values.remove("palettes.entries/Apple");
        adopt(&mut settings, &values);
        let names: Vec<&str> = settings
            .palettes
            .entries()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["Zest", "Mango"]);
    }
}
