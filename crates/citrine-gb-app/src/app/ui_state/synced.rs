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
    ("game_names", Kind::Map),
    ("cheats", Kind::Map),
    ("gimmicks.clock_offsets", Kind::Map),
    ("gimmicks.printers", Kind::Whole),
    ("dmg_theme", Kind::Whole),
    ("custom_palette", Kind::Whole),
    ("gbc_palette", Kind::Whole),
    ("palettes.entries", Kind::Named),
    ("per_game", Kind::Map),
    ("color_correction", Kind::Whole),
    ("gba_color_correction", Kind::Whole),
    ("scaling", Kind::Whole),
    ("same_size_pixels", Kind::Whole),
    ("ds_screens", Kind::Whole),
    ("layout_picks", Kind::Whole),
    ("matrix", Kind::Whole),
    ("ghosting", Kind::Whole),
    ("matrix_edge_darkness", Kind::Whole),
    ("matrix_corner_darkness", Kind::Whole),
    ("ghosting_strength", Kind::Whole),
    ("bindings.keyboard", Kind::Whole),
    ("bindings.shoulders.keyboard", Kind::Whole),
    ("bindings.ds.keyboard", Kind::Whole),
    ("bindings.combos.keyboard", Kind::Whole),
    ("bindings.tilt", Kind::Whole),
    ("hotkeys.bindings", Kind::Whole),
    ("hotkeys.key_combos", Kind::Whole),
    ("hotkeys.offered", Kind::Whole),
    ("hotkeys.screens.bindings", Kind::Whole),
    ("hotkeys.screens.key_combos", Kind::Whole),
    ("hotkeys.screens.offered", Kind::Whole),
    ("hotkeys.debugger.bindings", Kind::Whole),
    ("hotkeys.debugger.key_combos", Kind::Whole),
    ("hotkeys.debugger.offered", Kind::Whole),
    ("hotkeys.rewind.bindings", Kind::Whole),
    ("hotkeys.rewind.key_combos", Kind::Whole),
    ("hotkeys.rewind.offered", Kind::Whole),
    ("hotkeys.fast_forward.bindings", Kind::Whole),
    ("hotkeys.fast_forward.key_combos", Kind::Whole),
    ("hotkeys.fast_forward.offered", Kind::Whole),
    ("hotkeys.hud.bindings", Kind::Whole),
    ("hotkeys.hud.key_combos", Kind::Whole),
    ("hotkeys.hud.offered", Kind::Whole),
    ("hotkeys.window.bindings", Kind::Whole),
    ("hotkeys.window.key_combos", Kind::Whole),
    ("hotkeys.window.offered", Kind::Whole),
    ("hotkeys.talk.bindings", Kind::Whole),
    ("hotkeys.talk.key_combos", Kind::Whole),
    ("hotkeys.talk.offered", Kind::Whole),
    ("hotkeys.clip.bindings", Kind::Whole),
    ("hotkeys.clip.key_combos", Kind::Whole),
    ("hotkeys.clip.offered", Kind::Whole),
    ("online.show_playing", Kind::Whole),
    ("online.input_delay", Kind::Whole),
    ("online.direct", Kind::Whole),
    ("online.typing_indicators", Kind::Whole),
    ("online.typing_text", Kind::Whole),
    ("online.notify_friend_requests", Kind::Whole),
    ("online.notify_invites", Kind::Whole),
    ("online.notify_chats", Kind::Whole),
    ("online.focus_popups", Kind::Whole),
    ("local_link.swapped", Kind::Whole),
    ("local_link.second_sound", Kind::Whole),
    ("local_link.left_keys", Kind::Whole),
    ("local_link.right_keys", Kind::Whole),
    ("autosave_interval", Kind::Whole),
    ("autosave_keep", Kind::Whole),
    ("rewind", Kind::Whole),
    ("fast_forward_speed", Kind::Whole),
    ("fast_forward_key", Kind::Whole),
    ("quick_load", Kind::Whole),
    ("screenshot_scale", Kind::Whole),
    ("clip_length", Kind::Whole),
    ("recording_scale", Kind::Whole),
    ("render_video", Kind::Whole),
    ("randomized_ram", Kind::Whole),
    ("library_depth", Kind::Whole),
    ("dev_mode", Kind::Whole),
    ("track_pc", Kind::Whole),
    ("stats_range", Kind::Whole),
    ("ds_user.nickname", Kind::Whole),
    ("ds_user.birthday", Kind::Whole),
    ("ds_user.language", Kind::Whole),
    ("ds_user.favorite_color", Kind::Whole),
    ("ds_online.service", Kind::Whole),
    ("ds_online.dns", Kind::Whole),
    ("walker.in_game", Kind::Whole),
];

#[cfg_attr(not(test), allow(dead_code))]
const LOCAL: &[&str] = &[
    "per_game_local",
    "ui_scale",
    "ds_3d",
    "ds_jit",
    "gba_jit",
    "match_screen",
    "ui_scale_per_ratio",
    "bindings.shoulders.triggers",
    "layout",
    "volume",
    "muted",
    "audio_queue_ms",
    "audio_device",
    "microphone",
    "push_to_talk",
    "talk_button",
    "slow_sound",
    "fast_forward_sound",
    "capture_folder",
    "record_frame_times",
    "fullscreen",
    "background_pause",
    "hide_pointer",
    "channels",
    "gba_channels",
    "hotkeys.pads",
    "hotkeys.screens.pads",
    "hotkeys.debugger.pads",
    "hotkeys.rewind.pads",
    "hotkeys.fast_forward.pads",
    "hotkeys.hud.pads",
    "hotkeys.pad_combos",
    "hotkeys.screens.pad_combos",
    "hotkeys.debugger.pad_combos",
    "hotkeys.rewind.pad_combos",
    "hotkeys.fast_forward.pad_combos",
    "hotkeys.hud.pad_combos",
    "hotkeys.window.pads",
    "hotkeys.window.pad_combos",
    "hotkeys.talk.pads",
    "hotkeys.talk.pad_combos",
    "hotkeys.clip.pads",
    "hotkeys.clip.pad_combos",
    "bindings.gamepad",
    "bindings.shoulders.gamepad",
    "bindings.ds.gamepad",
    "bindings.combos.gamepad",
    "bindings.ds.adopted",
    "bindings.stick_as_dpad",
    "bindings.stick_deadzone",
    "touch",
    "layouts",
    "saved_layouts",
    "layouts_adopted",
    "clip_length_adopted",
    "gimmicks.rumble",
    "gimmicks.light_source",
    "gimmicks.light",
    "gimmicks.camera",
    "gimmicks.camera_view",
    "online.address",
    "online.joined",
    "online.relays",
    "online.cloud_saves",
    "online.sync_settings",
    "online.keep_saves",
    "online.network_saves",
    "online.network_allowed",
    "online.friend_volume",
    "online.friend_muted",
    "online.notify_asked",
    "online.device_name",
    "local_link.controls",
    "current_tab",
    "check_updates",
    "covers",
    "quick_slot",
    "focus_place",
    "hud",
    "big_screen",
    "ds_user.keep_firmware_settings",
    "ds_user.clock",
    "ds_online.direct",
    "walker.phone",
    "discord",
    "achievements",
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

pub fn syncs(path: &str) -> bool {
    SYNCED.iter().any(|(synced, kind)| {
        *kind == Kind::Whole
            && (path == *synced
                || path
                    .strip_prefix(synced)
                    .is_some_and(|rest| rest.starts_with('.')))
    })
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
    if let Some(adopted) = root.get_mut("layouts_adopted") {
        *adopted = Value::Bool(true);
    }
    let Ok(mut next) = serde_json::from_value::<Settings>(root) else {
        log::warn!("synced settings could not be read");
        return false;
    };
    next.scale_drag = settings.scale_drag;
    next.sanitize();
    if serde_json::to_value(&next).is_ok_and(|after| after == before) {
        return false;
    }
    next.resume_game(&mut *settings);
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
fn walk(
    value: &Value,
    path: &str,
    descend: &dyn Fn(&str, &Value) -> bool,
    found: &mut Vec<String>,
) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, child) in object {
        let path = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        if descend(&path, child) {
            walk(child, &path, descend, found);
        } else {
            found.push(path);
        }
    }
}

#[cfg(test)]
pub(crate) fn setting_paths() -> Vec<String> {
    let root = serde_json::to_value(Settings::default()).unwrap_or_default();
    let mut found = Vec::new();
    let descend = |path: &str, child: &Value| {
        let split_by_key = SYNCED
            .iter()
            .any(|(p, kind)| *p == path && *kind != Kind::Whole);
        !split_by_key && child.as_object().is_some_and(|object| !object.is_empty())
    };
    walk(&root, "", &descend, &mut found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::palette::CustomPalette;
    use crate::app::ui_state::ui_theme::UiTheme;
    use crate::emulator::fast_forward::FastForwardSpeed;
    use citrine_gb::gb::ppu::types::theme::DmgTheme;

    fn listed(path: &str) -> bool {
        SYNCED.iter().any(|(p, _)| *p == path) || LOCAL.contains(&path)
    }

    #[test]
    fn every_setting_is_either_synced_or_kept_on_the_device() {
        let root = serde_json::to_value(Settings::default()).unwrap();
        for (path, _) in SYNCED {
            assert!(!LOCAL.contains(path), "{path} is in both lists");
        }
        let descend = |path: &str, child: &Value| {
            let parent = SYNCED
                .iter()
                .any(|(p, _)| p.starts_with(&format!("{path}.")));
            !listed(path) && parent && child.is_object()
        };
        let mut unsorted = Vec::new();
        walk(&root, "", &descend, &mut unsorted);
        unsorted.retain(|path| !listed(path));
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
            microphone: crate::audio::mic::Microphone::Device(format!("{MARK} microphone")),
            push_to_talk: true,
            talk_button: false,
            channels: [true, false, true, false],
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
        desk.set_own("abc", "dmg_theme", serde_json::json!(DmgTheme::Pocket));
        desk.matrix = true;
        let values = synced_values(&desk);

        let mut phone = device_settings();
        let before_local = serde_json::to_value(&phone).unwrap();
        assert!(adopt(&mut phone, &values));
        assert!(phone.matrix);
        assert!(phone.palettes.contains("Lemon"));
        assert_eq!(phone.own_values("abc").count(), 1);
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

    #[test]
    fn the_fast_forward_speed_follows_the_account() {
        let phone = Settings {
            fast_forward_speed: FastForwardSpeed::Ten,
            ..Settings::default()
        };
        let mut laptop = Settings::default();
        assert!(adopt(&mut laptop, &synced_values(&phone)));
        assert_eq!(laptop.fast_forward_speed, FastForwardSpeed::Ten);
        assert!(is_synced_key("fast_forward_speed"));
    }

    #[test]
    fn the_refresh_rate_match_stays_on_the_device() {
        let phone = Settings {
            match_screen: false,
            ..Settings::default()
        };
        let mut laptop = Settings::default();
        adopt(&mut laptop, &synced_values(&phone));
        assert!(laptop.match_screen);
        assert!(!is_synced_key("match_screen"));
    }

    #[test]
    fn notification_switches_follow_the_account() {
        let mut phone = Settings::default();
        phone.online.notify_invites = false;
        phone.online.notify_chats = false;
        let mut laptop = Settings::default();
        assert!(adopt(&mut laptop, &synced_values(&phone)));
        assert_eq!(
            laptop.online.push_kinds(),
            citrine_gb_net::PushKinds {
                friend_requests: true,
                invites: false,
                chats: false,
            }
        );
        assert_eq!(
            Settings::default().online.push_kinds(),
            citrine_gb_net::PushKinds::ALL
        );
    }

    #[test]
    fn controller_shortcuts_stay_on_the_device() {
        use crate::app::ui_state::hotkeys::{Hotkey, HotkeyAction};
        use crate::app::ui_state::keybinds::PadButton;
        use strum::IntoEnumIterator;
        type InGroup = fn(HotkeyAction) -> bool;
        let groups: [(&str, InGroup); 8] = [
            ("screens", HotkeyAction::is_ds),
            ("debugger", HotkeyAction::is_debugger),
            ("rewind", HotkeyAction::is_rewind),
            ("fast_forward", HotkeyAction::is_fast_forward),
            ("hud", HotkeyAction::is_hud),
            ("window", HotkeyAction::is_window),
            ("talk", HotkeyAction::is_talk),
            ("clip", HotkeyAction::is_clip),
        ];
        let mut desk = Settings::default();
        let mut pads = PadButton::iter().skip(1);
        for (_, in_group) in groups {
            let action = HotkeyAction::iter()
                .find(|a| in_group(*a))
                .expect("an action");
            desk.hotkeys
                .bind_pad(pads.next().expect("a pad button"), action);
        }
        let plain = HotkeyAction::iter()
            .find(|a| groups.iter().all(|(_, in_group)| !in_group(*a)))
            .expect("a plain action");
        desk.hotkeys
            .bind_pad(pads.next().expect("a pad button"), plain);
        desk.hotkeys.bind(Hotkey::ctrl(egui::Key::J), plain);
        let combo = crate::combo::Combo::new([
            pads.next().expect("a pad button"),
            pads.next().expect("a pad button"),
        ])
        .expect("a combo");
        desk.hotkeys.bind_pads(combo, plain);
        let stored = serde_json::to_value(&desk).unwrap();
        let combos = lookup(&stored, "hotkeys.pad_combos").and_then(Value::as_array);
        assert!(combos.is_some_and(|c| !c.is_empty()), "no pad combo stored");
        for (group, _) in groups {
            let path = format!("hotkeys.{group}.pads");
            let bound = lookup(&stored, &path).and_then(Value::as_array);
            assert!(bound.is_some_and(|b| !b.is_empty()), "{path} holds no pad");
        }
        let values = synced_values(&desk);
        for key in values.keys().filter(|key| key.starts_with("hotkeys")) {
            assert!(
                !key.ends_with("pads") && !key.ends_with("pad_combos"),
                "{key} left the device"
            );
        }
        let first = PadButton::iter().next().expect("a pad button");
        let mut phone = Settings::default();
        phone.hotkeys.bind_pad(first, plain);
        assert!(adopt(&mut phone, &values));
        assert_eq!(
            phone
                .hotkeys
                .action_for(egui::Key::J, egui::Modifiers::CTRL),
            Some(plain)
        );
        let phone_pads: Vec<PadButton> = PadButton::iter()
            .filter(|pad| phone.hotkeys.action_for_pad(*pad).is_some())
            .collect();
        assert_eq!(phone_pads, [first]);
    }
}
