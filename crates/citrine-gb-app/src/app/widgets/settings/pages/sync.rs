use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab};
use crate::app::widgets::check::Check;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::info_mark::{info_mark_in_line, info_mark_width};
use crate::app::widgets::progress::ProgressBar;
use crate::app::widgets::settings::content::{SettingsContent, groups, label, rows, view};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::online::{CLOUD_HINT, SYNC_HINT, cloud_problems, no_storage};
use crate::app::widgets::settings::page::Page;
use crate::icons;
use crate::online::cloud::{Elsewhere, NetworkDevice, PeerSync};
use crate::online::{KeepSaves, Online};
use crate::progress::Progress;
use egui::{Align, Layout, Rect, Response, RichText, Ui, Widget, vec2};

pub const KEEP_HINT: &str = "Other games' saves wait under Saves elsewhere until you get them. Games you've played on this device always download theirs.";
pub const SIGN_IN: &str = "Sign in";
pub const ACCOUNT: &str = "Account";
pub const GET: &str = "Get";
pub const GET_LATER: &str =
    "This game's saves come from an older Citrine. Open the game here once to get them.";
pub const GETTING: &str = "Getting saves";
pub const NETWORK_HINT: &str = "Your saves copy straight between your devices on the same Wi-Fi, without the cloud. Devices signed in to your account do this by themselves. Other devices sync once each turns the other on under Devices.";
pub const SETTINGS_HINT: &str = if cfg!(target_arch = "wasm32") {
    SYNC_HINT
} else {
    "Your theme, palettes, key bindings and screen effects copy to your other devices, through the cloud or on this network, encrypted like your saves. The latest change wins. Volume, controllers, touch controls and the UI size stay on each device. Needs Cloud saves or Saves on this network."
};
pub const SYNC_WITH: &str = "Sync saves";
pub const SYNCING: &str = "Syncing";
pub const NOT_SYNCING: &str = "Not syncing";
pub const NO_DEVICES: &str = "No other devices on this network";
pub const DEVICES: &str = "Devices";
pub const ACCOUNT_LOCKED: &str = "This device is signed in to your account, so it always syncs.";

pub fn show(content: SettingsContent, ui: &mut Ui) {
    let s = content.settings;
    let mut online = content.online.map(|(client, _)| client);
    let mut events = content.events;
    let view = view(s, SettingsTab::Sync);
    Page::show(ui, |page| {
        for group in groups(SettingsTab::Sync, view) {
            page.tree_group(group, None, |ui| {
                Form::show(ui, group, |form| {
                    let signed_out = !s.online.is_joined();
                    for row in placed(group, rows(group, view), signed_out) {
                        row_ui(form, s, online.as_deref_mut(), row);
                    }
                    if group == GroupId::Cloud {
                        if !s.online.is_joined()
                            && form.row(ACCOUNT, |ui| {
                                let button = ui.button(format!("{} {SIGN_IN}", icons::SIGN_IN));
                                crate::app::widgets::settings::form::need(ui, button.rect.width());
                                button.clicked()
                            })
                            && let Some(events) = events.as_deref_mut()
                        {
                            events.open_settings(SettingsTab::Online);
                        }
                        for text in notes(s, online.as_deref()) {
                            form.wide_hint(text);
                        }
                    }
                })
            });
        }
    });
}

fn row_ui(form: &mut Form, s: &mut Settings, online: Option<&mut Online>, row: RowId) {
    match row {
        RowId::CloudSaves => {
            let joined = s.online.is_joined();
            let mut on = joined && s.online.cloud_saves;
            if form.toggle_when(label(row), Some(CLOUD_HINT), joined, &mut on) {
                s.online.cloud_saves = on;
                s.dirty = true;
            }
        }
        RowId::SyncSettings => {
            let can = settings_sync_on(s);
            let mut on = can && s.online.sync_settings;
            if form.toggle_when(label(row), Some(SETTINGS_HINT), can, &mut on) {
                s.online.sync_settings = on;
                s.dirty = true;
            }
        }
        RowId::KeepSaves => {
            form.row_with_hint(label(row), KEEP_HINT, |ui| {
                s.dirty |= GenericSelect::from_enum(&mut s.online.keep_saves, "enum_select_keep")
                    .default_value(KeepSaves::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::NetworkSaves => {
            if form.toggle_with_hint(label(row), NETWORK_HINT, &mut s.online.network_saves) {
                s.dirty = true;
            }
        }
        RowId::NetworkDevices => {
            if !s.online.network_saves {
                return;
            }
            let Some(online) = online else {
                return;
            };
            let devices = online.network_devices();
            if devices.is_empty() {
                form.wide_hint(NO_DEVICES);
                return;
            }
            form.column_head(DEVICES, SYNC_WITH);
            let me = online.device_name();
            let kind = this_kind(form.ctx());
            for device in &devices {
                let mut allowed = device.allowed;
                let check = |ui: &mut Ui, allowed: &mut bool| {
                    ui.horizontal(|ui| {
                        ui.add(
                            Check::new(allowed, "")
                                .spoken(spoken(device))
                                .locked(device.account),
                        );
                        if let Some(status) = status(device) {
                            ui.label(RichText::new(status).weak());
                        }
                    });
                };
                if device.account {
                    form.row_with_hint(&device.name, ACCOUNT_LOCKED, |ui| check(ui, &mut allowed));
                } else {
                    form.row(&device.name, |ui| check(ui, &mut allowed));
                }
                if allowed != device.allowed {
                    set_allowed(s, device, allowed);
                }
                if let Some(text) = turn_on_there(device, &me, kind) {
                    form.row_note(&text);
                }
            }
        }
        RowId::SavesElsewhere => {
            let Some(online) = online else {
                return;
            };
            let games = online.saves_elsewhere();
            if games.is_empty() {
                return;
            }
            form.custom(|ui| {
                for game in &games {
                    if ui
                        .push_id(("elsewhere", game.game, &game.rom), |ui| {
                            elsewhere_row(ui, game)
                        })
                        .inner
                    {
                        online.fetch_elsewhere(game);
                    }
                }
            });
        }
        _ => {}
    }
}

pub fn placed(group: GroupId, rows: impl Iterator<Item = RowId>, signed_out: bool) -> Vec<RowId> {
    let moved = signed_out && cfg!(not(target_arch = "wasm32"));
    let mut placed = Vec::new();
    for row in rows {
        if moved && row == RowId::SyncSettings {
            continue;
        }
        placed.push(row);
        if moved && group == GroupId::Network && row == RowId::NetworkSaves {
            placed.push(RowId::SyncSettings);
        }
    }
    placed
}

pub fn status(device: &NetworkDevice) -> Option<&'static str> {
    if !device.allowed {
        None
    } else if device.sync == PeerSync::Syncing {
        Some(SYNCING)
    } else {
        Some(NOT_SYNCING)
    }
}

pub fn turn_on_there(device: &NetworkDevice, me: &str, kind: &str) -> Option<String> {
    if device.sync != PeerSync::TurnOnThere {
        return None;
    }
    Some(if device.account {
        format!("On that device, turn on \"{}\"", label(RowId::NetworkSaves))
    } else {
        format!("On that device, turn on \"{me}\", {kind}")
    })
}

const TABLET_SIDE: f32 = 600.0;

pub fn this_kind(ctx: &egui::Context) -> &'static str {
    let native = ctx
        .native_pixels_per_point()
        .unwrap_or(ctx.pixels_per_point());
    kind_at(ctx.content_rect().size().min_elem() * ctx.pixels_per_point() / native)
}

fn kind_at(short_side: f32) -> &'static str {
    if crate::platform::is_tv() {
        "this TV"
    } else if cfg!(any(target_os = "android", target_os = "ios")) {
        if short_side >= TABLET_SIDE {
            "this tablet"
        } else {
            "this phone"
        }
    } else if cfg!(target_arch = "wasm32") {
        "this device"
    } else if cfg!(target_os = "macos") {
        "this Mac"
    } else {
        "this PC"
    }
}

pub fn spoken(device: &NetworkDevice) -> String {
    format!("{SYNC_WITH} with {}", device.name)
}

pub fn set_allowed(s: &mut Settings, device: &NetworkDevice, allowed: bool) {
    let key = device.key;
    s.online.network_allowed.retain(|known| *known != key);
    if allowed {
        s.online.network_allowed.push(key);
    }
    s.dirty = true;
}

pub fn cloud_on(s: &Settings) -> bool {
    s.online.is_joined() && s.online.cloud_saves
}

pub fn network_on(s: &Settings) -> bool {
    cfg!(not(target_arch = "wasm32")) && s.online.network_saves
}

pub fn settings_sync_on(s: &Settings) -> bool {
    cloud_on(s) || network_on(s)
}

pub fn notes(s: &Settings, online: Option<&Online>) -> Vec<&'static str> {
    if !s.online.is_joined() {
        return Vec::new();
    }
    online
        .map(|online| {
            no_storage(s, online)
                .into_iter()
                .chain(cloud_problems(s, online))
                .collect()
        })
        .unwrap_or_default()
}

pub fn title(game: &Elsewhere) -> &str {
    if game.title.is_empty() {
        "Unknown game"
    } else {
        &game.title
    }
}

pub fn why(game: &Elsewhere) -> Option<&'static str> {
    (game.rom.is_none() && game.fetched.is_none()).then_some(GET_LATER)
}

pub fn title_line(ui: &mut Ui, name: RichText, why: Option<&str>) -> Option<Response> {
    let mark = if why.is_some() {
        info_mark_width(ui)
    } else {
        0.0
    };
    let room = (ui.available_width() - mark).max(0.0);
    let label = ui
        .scope(|ui| {
            ui.set_max_width(room);
            ui.add(egui::Label::new(name).truncate())
        })
        .inner;
    let why = why?;
    let rect = Rect::from_min_size(
        egui::pos2(
            label.rect.right() + ui.spacing().item_spacing.x,
            label.rect.top(),
        ),
        vec2(mark, label.rect.height()),
    );
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    Some(info_mark_in_line(&mut cell, why))
}

pub fn facts(game: &Elsewhere) -> String {
    let place = match game.devices.as_slice() {
        [] => "In the cloud".to_owned(),
        [one] => format!("On {one}"),
        [one, two] => format!("On {one} and {two}"),
        [one, rest @ ..] => format!("On {one} and {} more", rest.len()),
    };
    let saves = if game.saves == 1 {
        "1 save".to_owned()
    } else {
        format!("{} saves", game.saves)
    };
    format!(
        "{place} · {saves} · {}",
        crate::media::format_size(game.bytes)
    )
}

fn elsewhere_row(ui: &mut Ui, game: &Elsewhere) -> bool {
    let side = ui.spacing().interact_size.y;
    let text = 2.0 * ui.text_style_height(&egui::TextStyle::Body) + 1.0;
    let height = side.max(text);
    let mut get = false;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::right_to_left(Align::Center),
        |ui| {
            if game.fetched.is_none() {
                let button = egui::Button::new(format!("{} {GET}", icons::DOWNLOAD_SIMPLE));
                get = ui.add_enabled(game.rom.is_some(), button).clicked();
            }
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.add_space((height - text) / 2.0);
                ui.spacing_mut().item_spacing.y = 1.0;
                title_line(ui, RichText::new(title(game)).strong(), why(game));
                ui.add(egui::Label::new(RichText::new(facts(game)).weak()).truncate());
            });
        },
    );
    if let Some((done, total)) = game.fetched {
        ui.add(ProgressBar::new(GETTING, Progress::bytes(done, total)));
    }
    ui.add_space(2.0);
    get
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::GameKey;

    fn game(devices: &[&str], saves: usize, bytes: u64) -> Elsewhere {
        Elsewhere {
            game: GameKey([0; 32]),
            rom: None,
            title: String::new(),
            devices: devices.iter().map(|d| (*d).to_owned()).collect(),
            saves,
            bytes,
            fetched: None,
        }
    }

    #[test]
    fn a_game_elsewhere_says_where_its_saves_are_how_many_and_their_size() {
        assert_eq!(facts(&game(&[], 1, 512)), "In the cloud · 1 save · 512 B");
        assert_eq!(
            facts(&game(&["Clara's PC"], 2, 65536)),
            "On Clara's PC · 2 saves · 64 KB"
        );
        assert_eq!(
            facts(&game(&["PC", "Steam Deck"], 3, 0)),
            "On PC and Steam Deck · 3 saves · 0 B"
        );
        assert_eq!(
            facts(&game(&["PC", "Deck", "Phone"], 3, 0)),
            "On PC and 2 more · 3 saves · 0 B"
        );
        assert_eq!(title(&game(&[], 1, 0)), "Unknown game");
    }

    #[test]
    fn a_device_waiting_on_the_other_side_names_the_switch_to_turn_on_there() {
        let device = |account, sync| NetworkDevice {
            key: citrine_gb_net::RelayKey([1; 32]),
            name: "Clara's laptop".to_owned(),
            account,
            allowed: true,
            sync,
        };
        assert_eq!(
            turn_on_there(&device(true, PeerSync::TurnOnThere), "Pixel", "this phone").as_deref(),
            Some("On that device, turn on \"Saves on this network\"")
        );
        assert_eq!(
            turn_on_there(&device(false, PeerSync::TurnOnThere), "Pixel", "this phone").as_deref(),
            Some("On that device, turn on \"Pixel\", this phone")
        );
        assert_eq!(
            turn_on_there(&device(false, PeerSync::Syncing), "Pixel", "this phone"),
            None
        );
        assert_eq!(
            turn_on_there(&device(false, PeerSync::Off), "Pixel", "this phone"),
            None
        );
        let desk = if cfg!(target_os = "macos") {
            "this Mac"
        } else {
            "this PC"
        };
        assert_eq!(kind_at(400.0), desk);
        assert_eq!(kind_at(900.0), desk);
        for hint in [NETWORK_HINT, ACCOUNT_LOCKED] {
            assert!(!hint.contains(SYNC_WITH), "{hint}");
        }
    }

    #[test]
    fn the_mark_after_a_game_name_sits_on_the_name_line_at_touch_height() {
        let ctx = egui::Context::default();
        let mut line = Rect::NOTHING;
        let mut mark = Rect::NOTHING;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 44.0;
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    line = ui
                        .scope(|ui| {
                            let name = RichText::new("Tetris").size(17.0);
                            mark = title_line(ui, name, Some(GET_LATER)).unwrap().rect;
                        })
                        .response
                        .rect;
                    ui.label("On Living room PC");
                });
            });
        });
        assert_eq!(mark.height(), 44.0);
        assert!(line.height() < 30.0, "{line:?}");
        assert!(
            (mark.center().y - line.center().y).abs() < 1.0,
            "{line:?} {mark:?}"
        );
    }
}
