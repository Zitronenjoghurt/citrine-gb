use super::accounts;
use super::{CODE_HINT, Choice, JoinForm, KEY_ABOUT, Step, busy, key_card, primary};
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::field::{code_field, row_field};
use crate::app::widgets::problem::problem_line;
use crate::app::widgets::settings::form::Form;
use crate::icons;
use crate::online::address::relay_key_in;
use crate::online::{Online, Phase, Problem, ServerAddress, Trust};
use citrine_gb_net::{DISPLAY_NAME_MAX_CHARS, InviteCode, RelayId, display_name};
use egui::{RichText, Ui};
use web_time::Instant;

pub(super) fn address_step(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    let phase = online.phase();
    let mut connect = false;
    let mut back = false;
    if let Some(host) = &form.signed_out {
        problem_line(
            ui,
            &format!("Another device signed this one out of {host}."),
        );
        ui.add_space(6.0);
    }
    if past_servers(ui, s, online, form, now) {
        ui.label(RichText::new("Another account").strong());
        ui.add_space(2.0);
    }
    Form::show(ui, "online_join", |f| {
        f.row("Server", |ui| {
            let response = ui.add(
                row_field(ui, &mut s.online.address)
                    .hint_text("gb.lemon.industries")
                    .desired_width(ui.available_width()),
            );
            if response.changed() {
                form.address_error = None;
                s.dirty = true;
            }
            connect |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        if let Some(error) = &form.address_error {
            f.hint(RichText::new(error).color(f.ctx().style().visuals.warn_fg_color));
        }
        let saved_password = ServerAddress::parse(&s.online.address)
            .ok()
            .and_then(|address| s.online.entry(&address))
            .is_some_and(|entry| !entry.password.is_empty());
        f.row("Relay password", |ui| {
            let response = ui.add(
                row_field(ui, &mut form.password)
                    .password(true)
                    .hint_text(if saved_password {
                        "Saved for this server"
                    } else {
                        ""
                    })
                    .desired_width(ui.available_width()),
            );
            connect |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        connect |= primary(ui, "Connect", !busy(phase));
        if accounts::can_back_out(s, form) && ui.button("Cancel").clicked() {
            back = true;
        }
        if busy(phase) {
            ui.spinner();
            if let Some(address) = online.address() {
                ui.label(format!("Connecting to {address}"));
            }
        } else if let Phase::Stopped(problem) = phase {
            problem_line(ui, &problem.text());
        }
    });
    if back {
        accounts::back_out(s, online, form, now);
        return;
    }
    if phase == Phase::Stopped(Problem::KeyChanged) {
        ui.add_space(4.0);
        key_card(ui, online, form, now);
    }
    if connect && !busy(phase) {
        form.signed_out = None;
        form.new_account = true;
        let typed = s.online.address.clone();
        match ServerAddress::parse(&typed).and_then(|a| Ok((a, relay_key_in(&typed)?))) {
            Ok((address, expected)) => {
                form.address_error = None;
                s.online.address = address.key();
                s.dirty = true;
                let password = if form.password.is_empty() {
                    s.online
                        .entry(&address)
                        .map(|entry| entry.password.clone())
                        .unwrap_or_default()
                } else {
                    form.password.clone()
                };
                let trust = Trust {
                    expected,
                    ..s.online
                        .entry(&address)
                        .map(|entry| entry.trust())
                        .unwrap_or_default()
                };
                online.probe(address, password, trust, now);
            }
            Err(err) => form.address_error = Some(err.to_string()),
        }
    }
}

pub(super) fn past_servers(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) -> bool {
    let saved = accounts::all_saved(s);
    if saved.is_empty() {
        return false;
    }
    let busy = busy(online.phase());
    let mut pick = None;
    ui.label(RichText::new("Your accounts").strong());
    ui.add_space(2.0);
    let many_servers = accounts::spans_servers(&saved);
    for (key, account) in &saved {
        let detail = accounts::detail(account, key, many_servers);
        let answer = ui
            .add_enabled_ui(!busy, |ui| {
                accounts::account_row(ui, account, &detail, false, true, accounts::Look::PAGE)
            })
            .inner;
        if answer.picked {
            pick = Some((key.clone(), account.relay_id.clone()));
        }
        if answer.remove {
            accounts::ask_remove(form, key, account);
        }
        ui.add_space(2.0);
    }
    ui.add_space(12.0);
    if let Some((key, relay_id)) = pick {
        form.signed_out = None;
        accounts::switch_to(s, online, &key, &relay_id, now);
    }
    accounts::remove_dialog(ui, s, online, form);
    true
}

pub(super) fn server_line(ui: &mut Ui, online: &Online) {
    let Some(info) = online.info() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::GLOBE_SIMPLE).size(15.0));
        ui.add(egui::Label::new(RichText::new(&info.name).strong()).truncate());
    });
    ui.add_space(4.0);
}

pub(super) fn account_step(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    let phase = online.phase();
    let mut back = false;
    let sign_ups = online.info().is_none_or(|i| i.sign_ups_open);
    server_line(ui, online);
    if let Some(key) = online.relay_key() {
        Form::show(ui, "online_relay_key", |f| {
            f.row_with_hint("Relay key", KEY_ABOUT, |ui| {
                let text = key.fingerprint().to_string();
                ui.add(egui::Label::new(RichText::new(text).monospace()).truncate());
            });
        });
    }
    ui.add_enabled_ui(sign_ups, |ui| {
        ui.radio_value(&mut form.choice, Choice::New, "New account");
    });
    if !sign_ups {
        ui.label(RichText::new("This server takes no new accounts.").weak());
    }
    ui.radio_value(&mut form.choice, Choice::Existing, "I have a relay ID");
    let mut go = false;
    if form.choice == Choice::Existing {
        ui.add_space(2.0);
        let response = ui.add(
            row_field(ui, &mut form.pasted)
                .hint_text("cgb1-…")
                .font(egui::TextStyle::Monospace)
                .desired_width(ui.available_width()),
        );
        if response.changed() {
            form.paste_error = None;
        }
        go |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(error) = &form.paste_error {
            problem_line(ui, error);
        }
        if form.code_asked {
            Form::show(ui, "online_sign_in_code", |f| {
                f.row("Code", |ui| {
                    let width = ui.available_width();
                    let focus = phase == Phase::Stopped(Problem::CodeNeeded);
                    go |= code_field(ui, &mut form.code, CODE_HINT, width, focus);
                });
            });
            if let Phase::Stopped(problem @ (Problem::CodeWrong | Problem::TooManyTries)) = phase {
                problem_line(ui, &problem.text());
            }
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(format!("{} Back", icons::ARROW_LEFT)).clicked() {
            if accounts::can_back_out(s, form) {
                back = true;
            } else {
                online.leave();
                form.step = Step::Address;
                form.code_asked = false;
                form.code.clear();
            }
        }
        let label = match form.choice {
            Choice::New => "Next",
            Choice::Existing => "Sign in",
        };
        let ready = form.choice == Choice::Existing || sign_ups;
        go |= primary(ui, label, ready && !busy(phase));
        if busy(phase) {
            ui.spinner();
        }
    });
    if back {
        accounts::back_out(s, online, form, now);
        return;
    }
    if !go {
        return;
    }
    match form.choice {
        Choice::New => form.step = Step::Name,
        Choice::Existing => match form.pasted.trim().parse::<RelayId>() {
            Ok(relay_id) => {
                form.paste_error = None;
                let code = Some(form.code.trim().to_owned()).filter(|c| !c.is_empty());
                form.code.clear();
                online.use_relay_id(relay_id, code, now);
            }
            Err(_) => form.paste_error = Some("That isn't a relay ID".into()),
        },
    }
}

pub(super) fn name_step(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let phase = online.phase();
    let invites = form.invite_needed || online.info().is_some_and(|i| i.invites_required);
    server_line(ui, online);
    let mut go = false;
    Form::show(ui, "online_name", |f| {
        if invites {
            f.row("Invite code", |ui| {
                let response = ui.add(
                    row_field(ui, &mut form.invite)
                        .char_limit(24)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(ui.available_width()),
                );
                if response.changed() {
                    form.invite_error = None;
                }
                go |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            });
            if let Some(error) = &form.invite_error {
                f.hint(RichText::new(error).color(f.ctx().style().visuals.warn_fg_color));
            }
        }
        f.row("Display name", |ui| {
            let response = ui.add(
                row_field(ui, &mut form.name)
                    .char_limit(DISPLAY_NAME_MAX_CHARS)
                    .desired_width(ui.available_width()),
            );
            if response.changed() {
                form.name_error = None;
            }
            go |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        if let Some(error) = &form.name_error {
            f.hint(RichText::new(error).color(f.ctx().style().visuals.warn_fg_color));
        }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(format!("{} Back", icons::ARROW_LEFT)).clicked() {
            form.step = Step::Account;
        }
        let filled = !form.name.trim().is_empty() && (!invites || !form.invite.trim().is_empty());
        go |= primary(ui, "Join", !busy(phase) && filled);
        if busy(phase) {
            ui.spinner();
        }
    });
    if !go || busy(phase) {
        return;
    }
    let invite = if invites {
        match form.invite.parse::<InviteCode>() {
            Ok(code) => {
                form.invite = code.to_string();
                Some(code)
            }
            Err(_) => {
                form.invite_error = Some("An invite code has 12 letters and digits".into());
                None
            }
        }
    } else {
        None
    };
    let name = match display_name(&form.name) {
        Ok(name) => Some(name),
        Err(_) => {
            form.name_error = Some("Use 1 to 24 letters, digits or symbols".into());
            None
        }
    };
    if let Some(name) = name
        && (invite.is_some() || !invites)
    {
        online.create_invited_account(name, invite, now);
    }
}
