use super::{CODE_HINT, JoinForm, Step, accounts, busy, key_card};
use crate::app::mobile::kit::{self, Row};
use crate::app::mobile::sheet;
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::confirm;
use crate::app::widgets::field::row_field;
use crate::app::widgets::problem::problem_line;
use crate::icons;
use crate::online::address::relay_key_in;
use crate::online::{Online, Phase, Problem, ServerAddress, Trust};
use citrine_gb_net::{DISPLAY_NAME_MAX_CHARS, InviteCode, RelayId, display_name};
use egui::{Context, Id, Response, Ui};
use web_time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinSheet {
    Server,
    SignIn,
    Create,
}

pub fn server_set(s: &Settings) -> bool {
    !s.online.address.trim().is_empty()
}

pub fn signed_out_rows(
    ui: &mut Ui,
    s: &Settings,
    form: &JoinForm,
    account_rows: bool,
) -> Option<JoinSheet> {
    if let Some(host) = &form.signed_out {
        sheet::inset(ui, |ui| {
            problem_line(
                ui,
                &format!("Another device signed this one out of {host}."),
            );
        });
        ui.add_space(6.0);
    }
    let set = server_set(s);
    let mut open = None;
    kit::list(ui, |list| {
        if set && account_rows {
            if list.row(Row::new("Sign in").icon(icons::SIGN_IN)).clicked() {
                open = Some(JoinSheet::SignIn);
            }
            if list
                .row(Row::new("Create account").icon(icons::USER_PLUS))
                .clicked()
            {
                open = Some(JoinSheet::Create);
            }
        }
        let value = if set {
            s.online.address.clone()
        } else {
            "Not set up".to_owned()
        };
        if list
            .row(Row::new("Server").icon(icons::GLOBE_SIMPLE).value(value))
            .clicked()
        {
            open = Some(JoinSheet::Server);
        }
    });
    open
}

pub fn open_sheet(
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    sheet: JoinSheet,
    now: Instant,
) {
    form.address_error = None;
    form.phone_connect = false;
    if sheet == JoinSheet::Server {
        form.phone_address = s.online.address.clone();
        return;
    }
    form.paste_error = None;
    form.name_error = None;
    form.invite_error = None;
    let live = matches!(
        online.phase(),
        Phase::Probed | Phase::Connecting | Phase::SigningIn
    );
    if !(live && on_server(s, online)) {
        reach(s, online, form, now);
    }
}

pub fn join_sheet(
    ctx: &Context,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    open: JoinSheet,
    now: Instant,
) -> bool {
    let id = Id::new("join_sheet");
    let shown = match open {
        JoinSheet::Server => sheet::show(ctx, id, "Server", |ui| {
            server_body(ui, s, online, form, now)
        }),
        JoinSheet::SignIn => sheet::show(ctx, id, "Sign in", |ui| {
            sign_in_body(ui, s, online, form, now);
            false
        }),
        JoinSheet::Create => sheet::show(ctx, id, "Create account", |ui| {
            create_body(ui, s, online, form, now);
            false
        }),
    };
    shown.closed || shown.inner || s.online.is_joined()
}

pub fn join_dialogs(ui: &mut Ui, s: &mut Settings, online: &mut Online, form: &mut JoinForm) {
    accounts::remove_dialog(ui, s, online, form);
    let Some(key) = form.forget_server.clone() else {
        return;
    };
    let question = format!(
        "Forget {key}? Citrine deletes the server password and every sign-in key it keeps for this server. Without a key you can't sign in to that account again."
    );
    if let Some(yes) = confirm::confirm(ui.ctx(), "online_forget_server", &question, "Forget") {
        form.forget_server = None;
        if yes {
            s.online.forget(&key);
            form.pasted.clear();
            s.dirty = true;
        }
    }
}

fn on_server(s: &Settings, online: &Online) -> bool {
    online.address().map(ServerAddress::key) == Some(s.online.address.clone())
}

fn reach(s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) -> bool {
    let typed = s.online.address.clone();
    let Some(key) = probe(&typed, s, online, form, now) else {
        return false;
    };
    s.online.address = key;
    s.dirty = true;
    true
}

fn probe(
    typed: &str,
    s: &Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) -> Option<String> {
    match ServerAddress::parse(typed).and_then(|a| Ok((a, relay_key_in(typed)?))) {
        Ok((address, expected)) => {
            form.address_error = None;
            form.signed_out = None;
            form.step = Step::Account;
            let key = address.key();
            let entry = s.online.entry(&address);
            let password = if form.password.is_empty() {
                entry.map(|e| e.password.clone()).unwrap_or_default()
            } else {
                form.password.clone()
            };
            let trust = Trust {
                expected,
                ..entry.map(|e| e.trust()).unwrap_or_default()
            };
            online.probe(address, password, trust, now);
            Some(key)
        }
        Err(err) => {
            form.address_error = Some(err.to_string());
            None
        }
    }
}

fn entered(ui: &Ui, response: &Response) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

fn gap(ui: &mut Ui) {
    ui.add_space(14.0);
}

fn problem(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    sheet::inset(ui, |ui| problem_line(ui, text));
}

fn phone_text(problem: Problem) -> String {
    match problem {
        Problem::UnknownAccount => "This server has no account with that sign-in key".into(),
        Problem::WrongPassword => "Wrong server password. Change it under Server.".into(),
        Problem::PasswordChanged => "The server password changed. Change it under Server.".into(),
        other => other.text(),
    }
}

fn phase_problem(phase: Phase) -> Option<Problem> {
    match phase {
        Phase::Stopped(problem) | Phase::Retrying { problem, .. } => Some(problem),
        _ => None,
    }
}

fn button_label(phase: Phase, idle: &'static str) -> &'static str {
    match phase {
        Phase::Connecting => "Connecting",
        Phase::SigningIn => "Signing in",
        _ => idle,
    }
}

fn changed_key(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    if online.phase() == Phase::Stopped(Problem::KeyChanged) {
        ui.add_space(10.0);
        key_card(ui, online, form, now);
    }
}

fn server_body(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) -> bool {
    let phase = online.phase();
    let mut go = false;
    ui.add_space(10.0);
    sheet::label(ui, "Address");
    let response = kit::field(
        ui,
        row_field(ui, &mut form.phone_address).hint_text("gb.lemon.industries"),
    );
    if response.changed() {
        form.address_error = None;
        form.phone_connect = false;
    }
    go |= entered(ui, &response);
    if let Some(error) = form.address_error.clone() {
        problem(ui, &error);
    }
    gap(ui);
    sheet::label(ui, "Password");
    let response = kit::field(
        ui,
        row_field(ui, &mut form.password)
            .password(true)
            .hint_text("Only if the server has one"),
    );
    go |= entered(ui, &response);
    if form.phone_connect {
        if let Some(found) = phase_problem(phase).filter(|p| *p != Problem::KeyChanged) {
            problem(ui, &phone_text(found));
        }
        changed_key(ui, online, form, now);
    }
    gap(ui);
    let working = form.phone_connect && busy(phase);
    let label = if working { "Connecting" } else { "Connect" };
    let typed = !form.phone_address.trim().is_empty();
    go |= ui
        .add_enabled_ui(!busy(phase) && typed, |ui| {
            kit::wide_button(ui, label, true)
        })
        .inner
        .clicked();
    if go && !busy(phase) && typed {
        let address = form.phone_address.clone();
        form.phone_connect = probe(&address, s, online, form, now).is_some();
    }
    let saved = ServerAddress::parse(&s.online.address)
        .ok()
        .filter(|address| s.online.entry(address).is_some())
        .map(|address| address.key());
    let mut forget = false;
    if let Some(key) = saved {
        gap(ui);
        kit::list(ui, |list| {
            if list
                .row(
                    Row::new("Forget this server")
                        .icon(icons::TRASH)
                        .danger()
                        .no_chevron(),
                )
                .clicked()
            {
                form.forget_server = Some(key);
                forget = true;
            }
        });
    }
    let done = form.phone_connect && phase == Phase::Probed;
    if done {
        form.phone_connect = false;
        if let Some(address) = online.address() {
            s.online.address = address.key();
            s.dirty = true;
        }
    }
    done || forget
}

fn sign_in_body(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    let phase = online.phase();
    let mut go = false;
    ui.add_space(10.0);
    if saved_accounts(ui, s, online, form, now) {
        gap(ui);
    }
    sheet::label(ui, "Sign-in key");
    let response = kit::field(
        ui,
        row_field(ui, &mut form.pasted)
            .password(true)
            .hint_text("Copy it from a device that's signed in"),
    );
    if response.changed() {
        form.paste_error = None;
    }
    go |= entered(ui, &response);
    if form.code_asked {
        gap(ui);
        sheet::label(ui, "Two-factor code");
        let response = kit::field(ui, row_field(ui, &mut form.code).hint_text(CODE_HINT));
        let entered_code = entered(ui, &response);
        if phase == Phase::Stopped(Problem::CodeNeeded)
            && !entered_code
            && ui.memory(|m| m.focused().is_none())
        {
            response.request_focus();
        }
        go |= entered_code;
    }
    let from_server = [Problem::UnknownAccount, Problem::AccountExists].map(Problem::text);
    let shown = match phase_problem(phase) {
        Some(Problem::CodeNeeded | Problem::KeyChanged) | None => form
            .paste_error
            .clone()
            .filter(|error| !from_server.contains(error)),
        Some(found) => Some(phone_text(found)),
    };
    if let Some(text) = shown {
        problem(ui, &text);
    }
    changed_key(ui, online, form, now);
    gap(ui);
    let ready =
        !form.pasted.trim().is_empty() && (!form.code_asked || !form.code.trim().is_empty());
    go |= ui
        .add_enabled_ui(ready && !busy(phase), |ui| {
            kit::wide_button(ui, button_label(phase, "Sign in"), true)
        })
        .inner
        .clicked();
    if !(go && ready) || busy(phase) {
        return;
    }
    match form.pasted.trim().parse::<RelayId>() {
        Ok(relay_id) => {
            form.paste_error = None;
            let code = Some(form.code.trim().to_owned()).filter(|c| !c.is_empty());
            form.code.clear();
            let key = s.online.address.clone();
            let id = relay_id.to_string();
            if s.online.accounts_on(&key).iter().any(|a| a.relay_id == id) {
                accounts::switch_to(s, online, &key, &id, now);
            } else if on_server(s, online) || reach(s, online, form, now) {
                online.use_relay_id(relay_id, code, now);
            }
        }
        Err(_) => form.paste_error = Some("That isn't a sign-in key".into()),
    }
}

fn saved_accounts(
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
    let many_servers = accounts::spans_servers(&saved);
    let mut pick = None;
    sheet::label(ui, "On this device");
    kit::list(ui, |list| {
        for (key, account) in &saved {
            let detail = accounts::detail(account, key, many_servers);
            let answer = list.custom(|ui| {
                accounts::account_row(ui, account, &detail, false, true, accounts::Look::CARD)
            });
            if answer.picked {
                pick = Some((key.clone(), account.relay_id.clone()));
            }
            if answer.remove {
                accounts::ask_remove(form, key, account);
            }
        }
    });
    if let Some((key, relay_id)) = pick {
        form.signed_out = None;
        accounts::switch_to(s, online, &key, &relay_id, now);
    }
    true
}

fn create_body(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    let phase = online.phase();
    let info = online.info().filter(|_| on_server(s, online));
    if info.is_some_and(|i| !i.sign_ups_open) || phase == Phase::Stopped(Problem::SignUpsClosed) {
        sheet::note(ui, "This server takes no new accounts.");
        return;
    }
    let invites = form.invite_needed || info.is_some_and(|i| i.invites_required);
    let mut go = false;
    ui.add_space(10.0);
    sheet::label(ui, "Display name");
    let response = kit::field(
        ui,
        row_field(ui, &mut form.name).char_limit(DISPLAY_NAME_MAX_CHARS),
    );
    if response.changed() {
        form.name_error = None;
    }
    go |= entered(ui, &response);
    if let Some(error) = form.name_error.clone() {
        problem(ui, &error);
    }
    if invites {
        gap(ui);
        sheet::label(ui, "Invite code");
        let response = kit::field(
            ui,
            row_field(ui, &mut form.invite)
                .char_limit(24)
                .font(egui::TextStyle::Monospace),
        );
        if response.changed() {
            form.invite_error = None;
        }
        go |= entered(ui, &response);
        if let Some(error) = form.invite_error.clone() {
            problem(ui, &error);
        }
    }
    let own = [
        Problem::BadName,
        Problem::InviteNeeded,
        Problem::InviteWrong,
        Problem::InviteUsed,
        Problem::KeyChanged,
    ];
    if let Some(found) = phase_problem(phase).filter(|p| !own.contains(p)) {
        problem(ui, &phone_text(found));
    }
    changed_key(ui, online, form, now);
    gap(ui);
    let filled = !form.name.trim().is_empty() && (!invites || !form.invite.trim().is_empty());
    go |= ui
        .add_enabled_ui(filled && !busy(phase), |ui| {
            kit::wide_button(ui, button_label(phase, "Create account"), true)
        })
        .inner
        .clicked();
    if !(go && filled) || busy(phase) {
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
                return;
            }
        }
    } else {
        None
    };
    let Ok(name) = display_name(&form.name) else {
        form.name_error = Some("Use 1 to 24 letters, digits or symbols".into());
        return;
    };
    if on_server(s, online) || reach(s, online, form, now) {
        online.create_invited_account(name, invite, now);
    }
}
