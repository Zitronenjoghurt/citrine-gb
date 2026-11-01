use crate::app::ui_state::settings::Settings;
use crate::app::widgets::avatar;
use crate::app::widgets::confirm;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::settings::form::Form;
use crate::icons;
use crate::online::cloud::SyncState;
use crate::online::{Online, Phase, Problem, Seen, ServerAddress};
use citrine_gb_net::{
    DISPLAY_NAME_MAX_CHARS, FileId, FileInfo, InviteCode, OnlineId, RelayId, display_name,
};
use egui::{RichText, Ui};
use std::time::Duration;
use web_time::Instant;

#[derive(Default)]
pub struct JoinForm {
    step: Step,
    password: String,
    choice: Choice,
    pasted: String,
    paste_error: Option<String>,
    name: String,
    name_error: Option<String>,
    invite: String,
    invite_error: Option<String>,
    invite_needed: bool,
    address_error: Option<String>,
    last_phase: Option<Phase>,
    name_draft: Option<String>,
    new_password: String,
    leaving: bool,
    copied: Option<(&'static str, Instant)>,
    show_files: bool,
    listed_at: Option<Instant>,
    delete: Option<(FileId, String)>,
    show_cloud: bool,
    delete_cloud: Option<(citrine_gb_net::GameKey, String)>,
    forget: Option<String>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Step {
    #[default]
    Address,
    Account,
    Name,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Choice {
    #[default]
    New,
    Existing,
}

const COPIED_FOR: Duration = Duration::from_secs(2);
const LIST_AGAIN: Duration = Duration::from_secs(5);

pub fn show(ui: &mut Ui, s: &mut Settings, online: Option<&mut Online>, form: &mut JoinForm) {
    let mut idle = Online::offline();
    let online = match online {
        Some(online) => online,
        None => &mut idle,
    };
    let now = Instant::now();
    let phase = online.phase();
    if form.last_phase != Some(phase) {
        form.last_phase = Some(phase);
        follow(form, s, online, phase, now);
    }
    if s.online.is_joined() {
        joined(ui, s, online, form, now);
    } else {
        match form.step {
            Step::Address => address_step(ui, s, online, form, now),
            Step::Account => account_step(ui, online, form, now),
            Step::Name => name_step(ui, online, form, now),
        }
    }
}

fn follow(form: &mut JoinForm, s: &Settings, online: &mut Online, phase: Phase, now: Instant) {
    if s.online.is_joined() {
        return;
    }
    match phase {
        Phase::Probed if form.step == Step::Address => {
            form.invite_needed = false;
            let stored = online
                .address()
                .and_then(|address| s.online.entry(address))
                .and_then(|entry| entry.relay_id());
            match stored {
                Some(relay_id) => online.use_relay_id(relay_id, now),
                None => {
                    form.step = Step::Account;
                    form.choice = if online.info().is_some_and(|i| !i.sign_ups_open) {
                        Choice::Existing
                    } else {
                        Choice::New
                    };
                }
            }
        }
        Phase::Stopped(problem) => match problem {
            Problem::SignUpsClosed => form.step = Step::Account,
            Problem::UnknownAccount | Problem::AccountExists => {
                form.step = Step::Account;
                form.paste_error = Some(problem.text());
            }
            Problem::BadName => {
                form.step = Step::Name;
                form.name_error = Some(problem.text());
            }
            Problem::InviteNeeded | Problem::InviteWrong | Problem::InviteUsed => {
                form.step = Step::Name;
                form.invite_needed = true;
                form.invite_error = Some(problem.text());
            }
            _ => form.step = Step::Address,
        },
        _ => {}
    }
}

fn busy(phase: Phase) -> bool {
    matches!(phase, Phase::Connecting | Phase::SigningIn)
}

fn problem_line(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(format!("{} {text}", icons::WARNING)).color(ui.visuals().warn_fg_color));
}

fn primary(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(text).min_size(egui::vec2(96.0, ui.spacing().interact_size.y)),
    )
    .clicked()
}

fn address_step(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    let phase = online.phase();
    let mut connect = false;
    if past_servers(ui, s, online, form, now) {
        ui.label(RichText::new("Another server").strong());
        ui.add_space(2.0);
    }
    Form::show(ui, "online_join", |f| {
        f.row("Server", |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut s.online.address)
                    .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                    .vertical_align(egui::Align::Center)
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
        f.row("Relay password", |ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut form.password)
                    .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                    .vertical_align(egui::Align::Center)
                    .password(true)
                    .desired_width(ui.available_width()),
            );
            connect |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        connect |= primary(ui, "Connect", !busy(phase));
        if busy(phase) {
            ui.spinner();
            if let Some(address) = online.address() {
                ui.label(format!("Connecting to {address}"));
            }
        } else if let Phase::Stopped(problem) = phase {
            problem_line(ui, &problem.text());
        }
    });
    if connect && !busy(phase) {
        match ServerAddress::parse(&s.online.address) {
            Ok(address) => {
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
                online.probe(address, password, now);
            }
            Err(err) => form.address_error = Some(err.to_string()),
        }
    }
}

fn past_servers(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) -> bool {
    if s.online.relays.is_empty() {
        return false;
    }
    let busy = busy(online.phase());
    let mut join = None;
    ui.label(RichText::new("Your servers").strong());
    ui.add_space(2.0);
    for (key, entry) in &s.online.relays {
        let name = entry.display_name.clone().unwrap_or_default();
        let seed = entry
            .online_id
            .as_deref()
            .and_then(|id| id.parse::<OnlineId>().ok())
            .map(|id| id.value())
            .unwrap_or_default();
        let height = 40.0_f32.max(ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let side = ui.spacing().interact_size.y.max(36.0);
                if ui
                    .add(egui::Button::new(trash(ui, side)).min_size(egui::vec2(side, side)))
                    .on_hover_text("Forget this server")
                    .clicked()
                {
                    form.forget = Some(key.clone());
                }
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new("Join").min_size(egui::vec2(0.0, side)),
                    )
                    .clicked()
                {
                    join = Some(key.clone());
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    avatar::avatar(ui, &name, seed, 40.0, None);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        if name.is_empty() {
                            ui.add(egui::Label::new(RichText::new(key).strong()).truncate());
                        } else {
                            ui.add(egui::Label::new(RichText::new(&name).strong()).truncate());
                            ui.add(egui::Label::new(RichText::new(key).weak()).truncate());
                        }
                    });
                });
            },
        );
        ui.add_space(2.0);
    }
    ui.add_space(12.0);
    if let Some(key) = join
        && let Ok(address) = ServerAddress::parse(&key)
    {
        let password = s
            .online
            .entry(&address)
            .map(|entry| entry.password.clone())
            .unwrap_or_default();
        form.address_error = None;
        s.online.address = key;
        s.dirty = true;
        online.probe(address, password, now);
    }
    if let Some(key) = form.forget.clone() {
        let question = format!(
            "Forget {}? Citrine deletes the relay ID and password it keeps for this server. Without the relay ID you can't sign in to this account again.",
            s.online
                .relays
                .get(&key)
                .and_then(|entry| entry.display_name.as_deref())
                .filter(|name| !name.is_empty())
                .map_or_else(|| key.clone(), |name| format!("{name} on {key}"))
        );
        if let Some(yes) = confirm::confirm(ui.ctx(), "online_forget", &question, "Forget") {
            form.forget = None;
            if yes {
                s.online.forget(&key);
                s.dirty = true;
            }
        }
    }
    true
}

fn server_line(ui: &mut Ui, online: &Online) {
    let Some(info) = online.info() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(icons::GLOBE_SIMPLE).size(15.0));
        ui.add(egui::Label::new(RichText::new(&info.name).strong()).truncate());
    });
    ui.add_space(4.0);
}

fn account_step(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let phase = online.phase();
    let sign_ups = online.info().is_none_or(|i| i.sign_ups_open);
    server_line(ui, online);
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
            egui::TextEdit::singleline(&mut form.pasted)
                .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                .vertical_align(egui::Align::Center)
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
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button(format!("{} Back", icons::ARROW_LEFT)).clicked() {
            online.leave();
            form.step = Step::Address;
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
    if !go {
        return;
    }
    match form.choice {
        Choice::New => form.step = Step::Name,
        Choice::Existing => match form.pasted.trim().parse::<RelayId>() {
            Ok(relay_id) => {
                form.paste_error = None;
                online.use_relay_id(relay_id, now);
            }
            Err(_) => form.paste_error = Some("That isn't a relay ID".into()),
        },
    }
}

fn name_step(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let phase = online.phase();
    let invites = form.invite_needed || online.info().is_some_and(|i| i.invites_required);
    server_line(ui, online);
    let mut go = false;
    Form::show(ui, "online_name", |f| {
        if invites {
            f.row("Invite code", |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut form.invite)
                        .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                        .vertical_align(egui::Align::Center)
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
                egui::TextEdit::singleline(&mut form.name)
                    .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                    .vertical_align(egui::Align::Center)
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

pub fn my_seen(online: &Online) -> Seen {
    if online.is_connected() {
        Seen::Online
    } else {
        Seen::Offline { last_seen: 0 }
    }
}

fn wait_text(at: Instant, now: Instant) -> String {
    let wait = at.saturating_duration_since(now).as_secs() + 1;
    if wait >= 60 {
        format!("{} min", wait.div_ceil(60))
    } else {
        format!("{wait} s")
    }
}

pub fn connection_line(ui: &mut Ui, online: &mut Online, now: Instant) {
    if online.is_connected() {
        online.watch_round_trip(now);
        if let Some(ms) = online.round_trip_ms() {
            crate::app::tabs::playing::latency(ui, ms);
        }
        return;
    }
    let (Phase::Retrying { problem, .. } | Phase::Stopped(problem)) = online.phase() else {
        return;
    };
    let text = RichText::new(problem.text()).color(ui.visuals().warn_fg_color);
    ui.add(egui::Label::new(text).truncate());
}

pub fn retry_row(ui: &mut Ui, online: &mut Online, now: Instant) {
    let (label, wait) = match online.phase() {
        Phase::Stopped(problem) if problem.needs_password() => return,
        Phase::Stopped(Problem::SignedInElsewhere) => ("Connect here".to_owned(), None),
        Phase::Retrying { at, .. } => (
            format!("{} Try now", icons::ARROW_CLOCKWISE),
            Some(wait_text(at, now)),
        ),
        Phase::Stopped(_) => (format!("{} Try now", icons::ARROW_CLOCKWISE), None),
        _ => return,
    };
    ui.horizontal(|ui| {
        if ui.button(label).clicked() {
            online.retry_now(now);
        }
        if let Some(wait) = wait {
            let text = egui::WidgetText::from(RichText::new(format!("Next try in {wait}")).weak());
            let galley = text.into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            );
            if galley.size().x <= ui.available_width() {
                ui.label(galley);
            }
        }
    });
}

fn copy_button(ui: &mut Ui, form: &mut JoinForm, what: &'static str, text: &str, now: Instant) {
    let copied = form
        .copied
        .is_some_and(|(which, at)| which == what && now.duration_since(at) < COPIED_FOR);
    let label = if copied {
        format!("{} Copied", icons::CHECK)
    } else {
        format!("{} Copy", icons::COPY)
    };
    if ui.button(label).clicked() {
        ui.ctx().copy_text(text.to_owned());
        form.copied = Some((what, now));
    }
}

fn joined(ui: &mut Ui, s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let Some(relay) = s.online.joined_relay() else {
        return;
    };
    let entry = s.online.joined_entry().cloned();
    let account_name = online
        .account()
        .map(|a| a.display_name.clone())
        .or_else(|| entry.as_ref().and_then(|e| e.display_name.clone()))
        .unwrap_or_default();
    let online_id = online
        .account()
        .map(|a| a.online_id.to_string())
        .or_else(|| entry.as_ref().and_then(|e| e.online_id.clone()))
        .unwrap_or_default();
    let seed = online_id
        .parse::<citrine_gb_net::OnlineId>()
        .map(|id| id.value())
        .unwrap_or_default();

    ui.horizontal(|ui| {
        avatar::avatar(ui, &account_name, seed, 44.0, Some(my_seen(online)));
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(&account_name).strong().size(16.0)).truncate());
            ui.add(egui::Label::new(RichText::new(relay.address.key()).weak()).truncate());
            connection_line(ui, online, now);
        });
    });
    match online.phase() {
        Phase::Stopped(problem) if problem.needs_password() => {
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut form.new_password)
                        .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                        .vertical_align(egui::Align::Center)
                        .password(true)
                        .hint_text("Relay password")
                        .desired_width(180.0),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Connect").clicked() || enter {
                    s.online.set_password(&form.new_password);
                    s.dirty = true;
                    online.change_password(std::mem::take(&mut form.new_password), now);
                }
            });
        }
        _ => retry_row(ui, online, now),
    }

    ui.add_space(10.0);
    let relay_text = relay.relay_id.to_string();
    Form::show(ui, "online_joined", |f| {
        f.row_with_hint(
            "Relay ID",
            "Don't share it. Anyone with your relay ID can sign in as you.",
            |ui| {
                copy_button(ui, form, "relay", &relay_text, now);
                ui.add(egui::Label::new(RichText::new(&relay_text).monospace().weak()).truncate());
            },
        );
        f.row_with_hint("Online ID", "Friends add you with this", |ui| {
            copy_button(ui, form, "online", &online_id, now);
            ui.add(egui::Label::new(RichText::new(&online_id).monospace()).truncate());
        });
        f.row("Display name", |ui| {
            let editing = form.name_draft.get_or_insert_with(|| account_name.clone());
            let response = ui.add_enabled(
                online.is_connected(),
                egui::TextEdit::singleline(editing)
                    .min_size(egui::vec2(0.0, ui.spacing().interact_size.y))
                    .vertical_align(egui::Align::Center)
                    .char_limit(DISPLAY_NAME_MAX_CHARS)
                    .desired_width(ui.available_width()),
            );
            if response.lost_focus() {
                match display_name(editing) {
                    Ok(name) if name != account_name => {
                        online.set_display_name(&name);
                        *editing = name;
                    }
                    _ => form.name_draft = None,
                }
            } else if !response.has_focus()
                && form.name_draft.as_deref() == Some(account_name.as_str())
            {
                form.name_draft = None;
            }
        });
        if online.request_error() == Some(citrine_gb_net::ErrorCode::BadName) {
            f.hint(Problem::BadName.text());
        }
    });
    ui.add_space(4.0);
    s.dirty |= ui
        .checkbox(&mut s.online.show_playing, "Show friends what I'm playing")
        .changed();
    let note = |ui: &mut Ui, indent: f32, text: &str| {
        let left = indent + ui.spacing().icon_width + ui.spacing().icon_spacing;
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: left.round() as i8,
                ..egui::Margin::ZERO
            })
            .show(ui, |ui| ui.label(RichText::new(text).weak()));
    };
    ui.horizontal(|ui| {
        s.dirty |= ui.checkbox(&mut s.online.cloud_saves, "Cloud saves").changed();
        info_mark(
            ui,
            "Keeps your saves on the relay so your other devices can load them. They're encrypted, so only your devices can read them.",
        );
    });
    if s.online.cloud_saves && online.account().is_some_and(|a| a.quota_bytes == 0) {
        note(
            ui,
            0.0,
            "This relay doesn't store files, so saves and settings stay on this device.",
        );
    }
    let indent = ui.spacing().indent;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        let mut on = s.online.cloud_saves && s.online.sync_settings;
        let check = egui::Checkbox::new(&mut on, "Sync settings");
        if ui.add_enabled(s.online.cloud_saves, check).changed() {
            s.online.sync_settings = on;
            s.dirty = true;
        }
        info_mark(
            ui,
            "Your theme, palettes, key bindings and screen effects copy to your other devices on this relay ID, encrypted like your saves. The latest change wins. Volume, controllers, touch controls and the UI size stay on each device. Needs Cloud saves.",
        );
    });
    let problem = match online.settings_problem() {
        Some(SyncState::Full) => {
            Some("The relay's storage is full, so your settings stay on this device.")
        }
        Some(SyncState::TooLarge) => {
            Some("Your settings are too large for the relay, so they stay on this device.")
        }
        _ => None,
    };
    if s.online.cloud_saves
        && s.online.sync_settings
        && let Some(problem) = problem
    {
        note(ui, indent, problem);
    }

    ui.add_space(ui.spacing().interact_size.y.max(20.0));
    storage(ui, online, form, now);

    ui.add_space(ui.spacing().interact_size.y.max(20.0));
    if ui
        .button(format!("{} Leave this relay", icons::SIGN_OUT))
        .clicked()
    {
        form.leaving = true;
    }
    if form.leaving {
        let question = format!(
            "Leave {}? Friends will see you as offline. Citrine keeps your relay ID for this server, so you can join again.",
            relay.address.key()
        );
        if let Some(leave) = confirm::confirm(ui.ctx(), "online_leave", &question, "Leave") {
            form.leaving = false;
            if leave {
                online.leave();
                s.online.leave();
                s.dirty = true;
                form.step = Step::Address;
                form.name_draft = None;
                form.last_phase = None;
            }
        }
    }
}

fn storage(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let Some(account) = online.account() else {
        return;
    };
    if account.quota_bytes == 0 {
        return;
    }
    let (used, quota) = (account.storage_used, account.quota_bytes);
    Form::show(ui, "online_storage", |f| {
        f.text_row("Storage", |ui| {
            ui.label(format!(
                "{} of {}",
                crate::media::format_size(used),
                crate::media::format_size(quota)
            ));
        });
    });
    let fraction = (used as f32 / quota as f32).clamp(0.0, 1.0);
    ui.scope(|ui| {
        if !ui.visuals().dark_mode {
            ui.visuals_mut().extreme_bg_color = ui.visuals().widgets.inactive.bg_fill;
        }
        ui.add(egui::ProgressBar::new(fraction).desired_height(6.0));
    });
    ui.add_space(8.0);

    let files = online.shared_files().map(<[FileInfo]>::to_vec);
    let due = form
        .listed_at
        .is_none_or(|at| now.duration_since(at) >= LIST_AGAIN);
    if files.is_none() && online.is_connected() && due && online.list_files() {
        form.listed_at = Some(now);
    }
    let (title, bytes) = match &files {
        Some(files) => (
            format!("Shared files ({})", files.len()),
            Some(files.iter().map(|f| f.size).sum::<u64>()),
        ),
        None => ("Shared files".to_owned(), None),
    };
    if storage_row(ui, Some(form.show_files), &title, bytes) {
        form.show_files = !form.show_files;
    }
    if form.show_files {
        shared_files(ui, online, form, files);
    }
    cloud_saves(ui, online, form);
    if let Some(bytes) = online.account_bytes().filter(|bytes| *bytes > 0) {
        storage_row(ui, None, "Settings and stats", Some(bytes));
    }
}

fn caret_width(ui: &Ui) -> f32 {
    egui::WidgetText::from(format!("{} ", icons::CARET_RIGHT))
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x
}

fn item_indent(ui: &Ui) -> f32 {
    caret_width(ui) + ui.spacing().indent
}

fn stored_item(ui: &mut Ui, enabled: bool, title: &str, facts: &str) -> bool {
    let side = ui.spacing().interact_size.y;
    let indent = item_indent(ui);
    let text = 2.0 * ui.text_style_height(&egui::TextStyle::Body) + 1.0;
    let height = side.max(text);
    let mut delete = false;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().button_padding = egui::Vec2::ZERO;
                delete = ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(trash(ui, side)).min_size(egui::vec2(side, side)),
                    )
                    .on_hover_text("Delete")
                    .clicked();
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.add_space((height - text) / 2.0);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
                    ui.add(egui::Label::new(RichText::new(facts).weak()).truncate());
                });
            },
        );
    });
    ui.add_space(2.0);
    delete
}

fn trash(ui: &Ui, side: f32) -> RichText {
    let body = egui::TextStyle::Body.resolve(ui.style()).size;
    RichText::new(icons::TRASH).size((side * 0.4).max(body))
}

fn stored_note(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(item_indent(ui));
        ui.label(RichText::new(text).weak());
    });
}

fn storage_row(ui: &mut Ui, open: Option<bool>, title: &str, bytes: Option<u64>) -> bool {
    let caret = |open: bool| {
        if open {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }
    };
    let text = match open {
        Some(open) => format!("{} {title}", caret(open)),
        None => title.to_owned(),
    };
    let indent = if open.is_some() { 0.0 } else { caret_width(ui) };
    let height = ui.spacing().interact_size.y;
    let row = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.style_mut().interaction.selectable_labels = false;
            if let Some(bytes) = bytes {
                ui.label(RichText::new(crate::media::format_size(bytes)).weak());
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add_space(indent);
                ui.add(egui::Label::new(text).truncate());
            });
        },
    );
    if open.is_none() {
        return false;
    }
    row.response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn shared_files(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    files: Option<Vec<FileInfo>>,
) {
    let Some(mut files) = files else {
        stored_note(ui, "Loading…");
        ui.add_space(8.0);
        return;
    };
    if files.is_empty() {
        stored_note(ui, "Nothing shared right now");
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.created));
    for file in &files {
        let (title, game) = match online.share_of(file.id) {
            Some((friend, share)) => {
                let name = online
                    .friend(friend)
                    .map(|p| p.display_name.clone())
                    .unwrap_or_else(|| friend.to_string());
                (
                    format!(
                        "{} to {name}",
                        crate::app::tabs::chat::share_label(share.kind)
                    ),
                    Some(crate::app::tabs::media::game_name(&share.game).to_owned()),
                )
            }
            None => ("Shared file".to_owned(), None),
        };
        let mut facts = Vec::new();
        facts.extend(game);
        facts.push(crate::media::format_size(file.size));
        facts.push(crate::media::local_stamp(file.created, "%-d %b %Y"));
        if stored_item(ui, online.is_connected(), &title, &facts.join(" · ")) {
            form.delete = Some((file.id, title.clone()));
        }
    }
    ui.add_space(8.0);
    if let Some((file, title)) = form.delete.clone() {
        let question = format!("Delete \"{title}\"? Your friend can't download it after this.");
        if let Some(yes) = confirm::confirm(ui.ctx(), "online_file_delete", &question, "Delete") {
            form.delete = None;
            if yes {
                online.delete_file(file);
            }
        }
    }
}

fn cloud_saves(ui: &mut Ui, online: &mut Online, form: &mut JoinForm) {
    if !online.cloud_enabled() || !online.cloud_listed() {
        return;
    }
    let games = online.cloud_games();
    let title = format!(
        "Cloud saves ({})",
        games.iter().map(|game| game.saves).sum::<usize>()
    );
    let bytes: u64 = games.iter().map(|game| game.bytes).sum();
    if storage_row(ui, Some(form.show_cloud), &title, Some(bytes)) {
        form.show_cloud = !form.show_cloud;
    }
    if form.show_cloud {
        if games.is_empty() {
            stored_note(ui, "No saves in the cloud yet");
        }
        for game in &games {
            let title = if game.title.is_empty() {
                "Unknown game".to_owned()
            } else {
                game.title.clone()
            };
            let saves = if game.saves == 1 {
                "1 save".to_owned()
            } else {
                format!("{} saves", game.saves)
            };
            let facts = format!("{saves} · {}", crate::media::format_size(game.bytes));
            if stored_item(ui, online.is_connected(), &title, &facts) {
                form.delete_cloud = Some((game.game, title.clone()));
            }
        }
        ui.add_space(8.0);
    }
    if let Some((game, title)) = form.delete_cloud.clone() {
        let question = format!("Delete the cloud saves of {title}? The saves on this device stay.");
        if let Some(yes) = confirm::confirm(ui.ctx(), "online_cloud_delete", &question, "Delete") {
            form.delete_cloud = None;
            if yes {
                online.cloud_delete_game(game);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::storage_row;

    #[test]
    fn a_storage_header_opens_from_its_label() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let mut clicked = false;
        let mut frame = |events: Vec<egui::Event>| {
            let mut at = egui::Pos2::ZERO;
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    at = ui.cursor().min + egui::vec2(40.0, ui.spacing().interact_size.y / 2.0);
                    clicked |= storage_row(ui, Some(false), "Cloud saves (1)", Some(4096));
                });
            });
            at
        };
        let at = frame(Vec::new());
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(at)]);
        frame(vec![button(true)]);
        frame(vec![button(false)]);
        assert!(clicked, "a click on the row's label at {at:?} did nothing");
    }
}
