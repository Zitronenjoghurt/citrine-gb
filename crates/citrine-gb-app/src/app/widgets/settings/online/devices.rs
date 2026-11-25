use super::JoinForm;
use crate::app::mobile::{kit, touch_square};
use crate::app::tabs::friends::ago;
use crate::app::widgets::avatar::ONLINE;
use crate::app::widgets::confirm;
use crate::icons;
use crate::online::Online;
use citrine_gb_net::{DeviceId, DeviceInfo};
use egui::{Align, Layout, RichText, Sense, TextStyle, Ui, vec2};

pub(super) const ABOUT: &str = "Every device signed in with your relay ID. Signing one out clears the relay ID and your chats from that device.";
const TWO_FACTOR_OFF: &str =
    "Anyone with your relay ID can still sign in. Two-factor sign-in stops that.";

pub(super) fn any(online: &Online) -> bool {
    !online.devices().0.is_empty()
}

pub(super) fn devices(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    chosen: &mut String,
) -> bool {
    let (list, this) = online.devices();
    if list.is_empty() {
        return false;
    }
    let list = in_order(list, this);
    let connected = online.is_connected();
    let shown = online.device_name();
    let unix_now = jiff::Timestamp::now().as_second().max(0) as u64;
    let mut renamed = false;
    let gap = if touch_square(ui) > 0.0 { 6.0 } else { 0.0 };
    for (index, device) in list.iter().enumerate() {
        if index > 0 {
            ui.add_space(gap);
        }
        ui.push_id(("device", device.id), |ui| {
            if this == Some(device.id) {
                renamed |= this_device(ui, device, &shown, form, chosen, unix_now);
            } else if row(ui, device, connected, unix_now) {
                form.signing_out = Some((device.id, device.name.clone()));
            }
        });
    }
    let Some((id, name)) = form.signing_out.clone() else {
        return renamed;
    };
    let two_factor = online.account().is_some_and(|a| a.two_factor);
    let question = sign_out_question(&name, two_factor);
    if let Some(yes) = confirm::confirm(ui.ctx(), "online_sign_out_device", &question, "Sign out") {
        form.signing_out = None;
        if yes {
            online.sign_out_device(id);
        }
    }
    renamed
}

fn this_device(
    ui: &mut Ui,
    device: &DeviceInfo,
    shown: &str,
    form: &mut JoinForm,
    chosen: &mut String,
    unix_now: u64,
) -> bool {
    let field = ui.make_persistent_id("rename_device");
    let Some(draft) = form.renaming.as_mut() else {
        let rename = line(ui, device, shown, true, unix_now, |ui| {
            action(ui, &format!("{} Rename", icons::PENCIL_SIMPLE), true)
        });
        if rename {
            form.renaming = Some(shown.to_owned());
            ui.data_mut(|data| data.insert_temp(field, true));
        }
        return false;
    };
    let mut done = None;
    let height = ui.spacing().interact_size.y.max(
        ui.text_style_height(&TextStyle::Body) + ui.text_style_height(&TextStyle::Small) + 2.0,
    );
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::right_to_left(Align::Center),
        |ui| {
            if action(ui, "Save", true) {
                done = Some(true);
            }
            let edit = egui::TextEdit::singleline(draft)
                .id(field)
                .char_limit(citrine_gb_net::MAX_DEVICE_NAME_CHARS)
                .hint_text(crate::online::cloud::system_name())
                .desired_width(ui.available_width());
            let edit = if touch_square(ui) > 0.0 {
                kit::field(ui, edit)
            } else {
                ui.add(edit)
            };
            if ui.data_mut(|data| data.remove_temp::<bool>(field)) == Some(true) {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        },
    );
    let Some(save) = done else {
        return false;
    };
    let draft = form.renaming.take().unwrap_or_default();
    if !save {
        return false;
    }
    let name = chosen_name(&draft, &crate::online::cloud::system_name());
    let changed = *chosen != name;
    *chosen = name;
    changed
}

fn chosen_name(draft: &str, system: &str) -> String {
    if draft.trim().is_empty() || draft.trim() == system {
        String::new()
    } else {
        citrine_gb_net::device_name(draft)
    }
}

pub(super) fn sign_out_question(name: &str, two_factor: bool) -> String {
    let question = format!("Sign out {name}? It forgets this account and its chats here.");
    if two_factor {
        question
    } else {
        format!("{question} {TWO_FACTOR_OFF}")
    }
}

pub(super) fn in_order(list: &[DeviceInfo], this: Option<DeviceId>) -> Vec<DeviceInfo> {
    let mut list = list.to_vec();
    list.sort_by_key(|d| {
        (
            this != Some(d.id),
            !d.online,
            std::cmp::Reverse(d.last_seen),
            d.id,
        )
    });
    list
}

fn row(ui: &mut Ui, device: &DeviceInfo, connected: bool, unix_now: u64) -> bool {
    line(ui, device, &device.name, false, unix_now, |ui| {
        action(ui, &format!("{} Sign out", icons::SIGN_OUT), connected)
    })
}

fn action(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        if touch_square(ui) > 0.0 {
            kit::pill(ui, text, false)
        } else {
            ui.button(text)
        }
        .clicked()
    })
    .inner
}

fn line(
    ui: &mut Ui,
    device: &DeviceInfo,
    name: &str,
    this: bool,
    unix_now: u64,
    action: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let text = ui.text_style_height(&TextStyle::Body) + ui.text_style_height(&TextStyle::Small);
    let height = ui.spacing().interact_size.y.max(text + 2.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::left_to_right(Align::Center),
        |ui| {
            let icon = if citrine_gb_net::is_phone(name) {
                icons::DEVICE_MOBILE
            } else {
                icons::DESKTOP
            };
            ui.label(RichText::new(icon).size(18.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let acted = action(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(egui::Label::new(name).truncate());
                        if this {
                            ui.label(RichText::new("This device").small().weak());
                        } else {
                            status(ui, device, unix_now);
                        }
                    });
                });
                acted
            })
            .inner
        },
    )
    .inner
}

fn status(ui: &mut Ui, device: &DeviceInfo, unix_now: u64) {
    if !device.online {
        let seen = format!("Last seen {}", ago(device.last_seen, unix_now));
        ui.add(egui::Label::new(RichText::new(seen).small().weak()).truncate());
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, ONLINE);
        ui.label(RichText::new("Online").small().color(ONLINE));
    });
}

#[cfg(test)]
mod tests {
    fn device(id: u64, online: bool, last_seen: u64) -> citrine_gb_net::DeviceInfo {
        citrine_gb_net::DeviceInfo {
            id,
            name: format!("Device {id}"),
            added: 1,
            last_seen,
            online,
        }
    }

    #[test]
    fn this_device_comes_first_then_online_ones_then_the_latest_seen() {
        let list = [
            device(1, false, 50),
            device(2, true, 10),
            device(3, false, 90),
            device(4, true, 20),
        ];
        let order: Vec<u64> = super::in_order(&list, Some(1))
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(order, [1, 4, 2, 3]);
    }

    #[test]
    fn a_name_left_empty_or_equal_to_the_system_name_follows_the_system() {
        assert_eq!(super::chosen_name("", "Pixel 8"), "");
        assert_eq!(super::chosen_name(" Pixel 8 ", "Pixel 8"), "");
        assert_eq!(
            super::chosen_name(" Clara's Pixel ", "Pixel 8"),
            "Clara's Pixel"
        );
    }

    #[test]
    fn signing_out_warns_about_the_relay_id_only_without_two_factor() {
        let on = super::sign_out_question("Firefox on Mac", true);
        assert_eq!(
            on,
            "Sign out Firefox on Mac? It forgets this account and its chats here."
        );
        let off = super::sign_out_question("Firefox on Mac", false);
        assert!(off.starts_with(&on));
        assert!(off.ends_with("Two-factor sign-in stops that."));
    }
}
