use super::JoinForm;
use crate::app::tabs::friends::ago;
use crate::app::widgets::avatar::ONLINE;
use crate::app::widgets::confirm;
use crate::app::widgets::info_mark::info_mark;
use crate::icons;
use crate::online::Online;
use citrine_gb_net::{DeviceId, DeviceInfo};
use egui::{Align, Layout, RichText, Sense, TextStyle, Ui, vec2};

const ABOUT: &str = "Every device signed in with your sign-in key. Signing one out removes the key and your chats from it.";
const TWO_FACTOR_OFF: &str =
    "Anyone with your sign-in key can still sign in. Two-factor sign-in stops that.";

pub(super) fn any(online: &Online) -> bool {
    !online.devices().0.is_empty()
}

pub(super) fn devices(ui: &mut Ui, online: &mut Online, form: &mut JoinForm) {
    let (list, this) = online.devices();
    if list.is_empty() {
        return;
    }
    let list = in_order(list, this);
    let connected = online.is_connected();
    let unix_now = jiff::Timestamp::now().as_second().max(0) as u64;
    ui.horizontal(|ui| {
        ui.label("Devices");
        info_mark(ui, ABOUT);
    });
    for device in &list {
        ui.push_id(("device", device.id), |ui| {
            if row(ui, device, this == Some(device.id), connected, unix_now) {
                form.signing_out = Some((device.id, device.name.clone()));
            }
        });
    }
    let Some((id, name)) = form.signing_out.clone() else {
        return;
    };
    let two_factor = online.account().is_some_and(|a| a.two_factor);
    let question = sign_out_question(&name, two_factor);
    if let Some(yes) = confirm::confirm(ui.ctx(), "online_sign_out_device", &question, "Sign out") {
        form.signing_out = None;
        if yes {
            online.sign_out_device(id);
        }
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

fn row(ui: &mut Ui, device: &DeviceInfo, this: bool, connected: bool, unix_now: u64) -> bool {
    let mut sign_out = false;
    let text = ui.text_style_height(&TextStyle::Body) + ui.text_style_height(&TextStyle::Small);
    let height = ui.spacing().interact_size.y.max(text + 2.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), height),
        Layout::right_to_left(Align::Center),
        |ui| {
            if this {
                ui.label(RichText::new("This device").weak());
            } else {
                sign_out = ui
                    .add_enabled(
                        connected,
                        egui::Button::new(format!("{} Sign out", icons::SIGN_OUT)),
                    )
                    .clicked();
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let icon = if citrine_gb_net::is_phone(&device.name) {
                    icons::DEVICE_MOBILE
                } else {
                    icons::DESKTOP
                };
                ui.label(RichText::new(icon).size(18.0));
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add(egui::Label::new(&device.name).truncate());
                    status(ui, device, unix_now);
                });
            });
        },
    );
    sign_out
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
