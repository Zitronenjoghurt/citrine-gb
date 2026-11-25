use crate::app::mobile::kit::{self, Row};
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::GroupId;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets;
use crate::app::widgets::avatar;
use crate::app::widgets::confirm;
use crate::app::widgets::field::{row_field, row_field_width};
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::with_reset;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::icons;
use crate::online::cloud::SyncState;
use crate::online::settings::InputDelay;
use crate::online::{Online, Phase, Problem, Seen};
use citrine_gb_net::{DISPLAY_NAME_MAX_CHARS, FileId, MAX_TYPING_CHARS, display_name};
use egui::{RichText, Ui, Widget};
use join::{account_step, address_step, name_step};
pub use phone_join::{
    JoinSheet, join_dialogs, join_sheet, open_sheet, server_set, signed_out_rows,
};
use std::time::Duration;
use web_time::Instant;

pub mod accounts;
mod devices;
mod join;
mod phone_join;
pub mod storage;
mod two_factor;

const KEY_ABOUT: &str = "Citrine remembers this key and warns you if the relay shows a different one. The relay's owner sees it in the admin panel.";
pub const SHOW_PLAYING: &str = "Friends see what I play";
pub const TYPING: &str = "Typing indicators";
pub const TYPING_HINT: &str = "Friends see when you're writing to them, and you see when they write to you. Off means neither.";
pub const TYPING_TEXT: &str = "Typing text";
pub const TYPING_DEFAULT: &str = crate::app::widgets::typing::DEFAULT_TEXT;
pub const FOCUS_POPUPS: &str = "Popups in focus mode";
pub const FOCUS_POPUPS_HINT: &str =
    "Shows friends' messages and play invites while focus mode is on.";
pub const ALLOW: &str = "Allow notifications";
pub const FRIEND_REQUESTS: &str = "Friend requests";
pub const INVITES: &str = "Play invites";
pub const MESSAGES: &str = "Messages";
pub const ANDROID_SETTINGS: &str = "Android notification settings";
const CODE_HINT: &str = "From your app, or a recovery code";
const RELAY_ID_WHERE: &str =
    "Find it on a signed-in device: Settings > Online on a computer, or the You page on a phone.";
const RELAY_ID_PLACEHOLDER: &str = "cgb1-…";

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
    phone_address: String,
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
    forget: Option<(String, String)>,
    forget_server: Option<String>,
    new_account: bool,
    back_to: Option<(String, String)>,
    setup_code: String,
    ask: Option<two_factor::Ask>,
    ask_code: String,
    ask_sent: bool,
    code: String,
    code_asked: bool,
    trusting: bool,
    signed_out: Option<String>,
    signing_out: Option<(citrine_gb_net::DeviceId, String)>,
    renaming: Option<String>,
    phone_connect: bool,
    codes_to_save: Option<String>,
}

impl JoinForm {
    pub(crate) fn save_asked_codes(&mut self, spawner: &citrine_gb_threads::Spawner) {
        two_factor::save_asked_codes(self, spawner);
    }

    pub fn signed_out_of(host: String) -> Self {
        Self {
            signed_out: Some(host),
            ..Self::default()
        }
    }
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

const LIST_AGAIN: Duration = Duration::from_secs(5);

pub fn show(ui: &mut Ui, s: &mut Settings, online: Option<&mut Online>, form: &mut JoinForm) {
    let mut idle = Online::offline();
    let online = match online {
        Some(online) => online,
        None => &mut idle,
    };
    let now = Instant::now();
    track(s, online, form, now);
    if s.online.is_joined() {
        joined(ui, s, online, form, now);
    } else {
        match form.step {
            Step::Address => address_step(ui, s, online, form, now),
            Step::Account => account_step(ui, s, online, form, now),
            Step::Name => name_step(ui, online, form, now),
        }
    }
}

pub fn track(s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) {
    if s.online.is_joined() {
        form.back_to = None;
    }
    let phase = online.phase();
    if form.last_phase != Some(phase) {
        form.last_phase = Some(phase);
        follow(form, s, online, phase, now);
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
            match stored.filter(|_| !form.new_account) {
                Some(relay_id) => online.use_relay_id(relay_id, None, now),
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
            Problem::CodeNeeded | Problem::CodeWrong => {
                if form.pasted.trim().is_empty()
                    && let Some(relay_id) = online
                        .address()
                        .and_then(|address| s.online.entry(address))
                        .and_then(|entry| entry.relay_id())
                {
                    form.pasted = relay_id.to_string();
                }
                form.step = Step::Account;
                form.choice = Choice::Existing;
                form.code_asked = true;
            }
            Problem::TooManyTries if form.code_asked => form.step = Step::Account,
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

fn primary(ui: &mut Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(text).min_size(egui::vec2(96.0, ui.spacing().interact_size.y)),
    )
    .clicked()
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
            crate::app::tabs::playing::server_latency(ui, ms);
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
        Phase::Stopped(problem)
            if problem.needs_password()
                || problem.needs_code()
                || matches!(problem, Problem::KeyChanged | Problem::KeyMismatch) =>
        {
            return;
        }
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
    let copied_at = form
        .copied
        .filter(|(which, _)| *which == what)
        .map(|(_, at)| at);
    if widgets::copy::copy_button(ui, text, copied_at, now) {
        form.copied = Some((what, now));
    }
}

fn key_card(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let (Some(was), Some(shown), Some(host)) = (
        online.pinned_key(),
        online.offered_key(),
        online.address().map(|a| a.key()),
    ) else {
        return;
    };
    let warn = ui.visuals().warn_fg_color;
    egui::Frame::group(ui.style())
        .fill(warn.gamma_multiply(0.08))
        .stroke(egui::Stroke::new(1.0_f32, warn.gamma_multiply(0.4)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(format!(
                "The relay at {host} now shows a different key. If its owner changed the key, accept the new one. If you don't know why it changed, ask the owner first."
            ));
            Form::show(ui, "online_key_changed", |f| {
                f.text_row("Was", |ui| {
                    ui.label(RichText::new(was.fingerprint().to_string()).monospace().weak())
                });
                f.text_row("Now", |ui| {
                    ui.label(RichText::new(shown.fingerprint().to_string()).monospace())
                });
            });
            if ui.button("Accept new key").clicked() {
                form.trusting = true;
            }
        });
    if form.trusting {
        let question = format!("Trust the new key {} for {host}?", shown.fingerprint());
        if let Some(yes) = confirm::confirm(ui.ctx(), "online_trust_key", &question, "Trust") {
            form.trusting = false;
            if yes {
                online.accept_new_key(now);
            }
        }
    }
}

fn code_row(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let mut go = false;
    Form::show(ui, "online_code", |f| {
        f.row("Code", |ui| {
            let button = 72.0;
            let width = (row_field_width(ui) - button - ui.spacing().item_spacing.x).max(60.0);
            let response = ui.add(
                row_field(ui, &mut form.code)
                    .hint_text(CODE_HINT)
                    .desired_width(width),
            );
            go |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            go |= ui
                .add_enabled(
                    !form.code.trim().is_empty(),
                    egui::Button::new("Sign in").min_size(egui::vec2(button, 0.0)),
                )
                .clicked();
        });
    });
    if go && !form.code.trim().is_empty() {
        online.sign_in_with_code(std::mem::take(&mut form.code).trim().to_owned(), now);
    }
}

struct Joined {
    relay: crate::online::JoinedRelay,
    account_name: String,
    online_id: String,
    seed: u64,
}

fn joined_facts(s: &Settings, online: &Online) -> Option<Joined> {
    let relay = s.online.joined_relay()?;
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
    Some(Joined {
        relay,
        account_name,
        online_id,
        seed,
    })
}

fn joined(ui: &mut Ui, s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) {
    let Some(facts) = joined_facts(s, online) else {
        return;
    };
    Page::show(ui, |page| {
        page.plain(|ui| {
            Form::show(ui, "online_who", |f| {
                f.custom(|ui| {
                    ui.horizontal(|ui| {
                        avatar::avatar(
                            ui,
                            &facts.account_name,
                            facts.seed,
                            44.0,
                            Some(my_seen(online)),
                        );
                        ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&facts.account_name).strong().size(16.0),
                                )
                                .truncate(),
                            );
                            ui.add(
                                egui::Label::new(RichText::new(facts.relay.address.key()).weak())
                                    .truncate(),
                            );
                            connection_line(ui, online, now);
                        });
                    });
                });
                f.custom(|ui| {
                    switcher(ui, s, online, form, &facts, now);
                    problems(ui, s, online, form, now);
                });
            });
        });
        page.section(Heading::new(icons::IDENTIFICATION_CARD, "Account"), |ui| {
            Form::show(ui, "online_joined", |f| {
                relay_id_row(f, form, &facts, now);
                online_id_row(f, form, &facts, now);
                relay_key_row(f, online, &facts);
                display_name_row(f, online, form, &facts);
            });
        });
        page.section(Heading::new(icons::USERS, "Friends"), |ui| {
            Form::show(ui, "online_play", |f| {
                s.dirty |= f.toggle(SHOW_PLAYING, &mut s.online.show_playing);
                s.dirty |= f.toggle_with_hint(TYPING, TYPING_HINT, &mut s.online.typing_indicators);
                s.dirty |= f.tree_row(RowId::TypingText, |f| typing_text_row(f, &mut s.online));
                s.dirty |=
                    f.toggle_with_hint(FOCUS_POPUPS, FOCUS_POPUPS_HINT, &mut s.online.focus_popups);
            });
        });
        if crate::platform::push::available() {
            page.section(Heading::new(icons::BELL, "Notifications"), |ui| {
                Form::show(ui, "online_push", |f| notifications(f, s));
            });
        }
        page.section(Heading::new(icons::USERS, "Play together"), |ui| {
            Form::show(ui, "online_delay", |f| play_together(f, s));
        });
        if online.account().is_some() {
            page.section(
                Heading::new(icons::SHIELD_CHECK, "Two-factor sign-in").hint(two_factor::ABOUT),
                |ui| {
                    two_factor::two_factor(
                        ui,
                        online,
                        form,
                        &facts.account_name,
                        &facts.relay.address.key(),
                        now,
                    );
                },
            );
        }
        if devices::any(online) {
            page.section(
                Heading::new(icons::DEVICES, "Devices").hint(devices::ABOUT),
                |ui| {
                    Form::show(ui, "online_devices", |f| {
                        f.custom(|ui| {
                            s.dirty |=
                                devices::devices(ui, online, form, &mut s.online.device_name);
                        })
                    })
                },
            );
        }
        if storage::has_storage(online) {
            page.section(
                Heading::new(GroupId::Storage.icon(), GroupId::Storage.title()),
                |ui| {
                    Form::show(ui, "online_storage_page", |f| {
                        f.custom(|ui| storage::storage_body(ui, online, form, now))
                    })
                },
            );
        }
        page.section(
            Heading::new(GroupId::Leave.icon(), GroupId::Leave.title()),
            |ui| {
                Form::show(ui, "online_leave", |f| {
                    f.buttons(|ui| leave_and_delete(ui, s, online, form, &facts, false));
                });
            },
        );
    });
}

fn switcher(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    facts: &Joined,
    now: Instant,
) {
    let key = facts.relay.address.key();
    let saved = s.online.accounts_on(&key);
    let mut pick = None;
    let mut add = false;
    ui.add_space(8.0);
    ui.menu_button(format!("{} Switch account", icons::USER_SWITCH), |ui| {
        ui.set_width(280.0);
        for (index, account) in saved.iter().enumerate() {
            let detail = accounts::detail(account, &key, false);
            let answer = accounts::account_row(
                ui,
                account,
                &detail,
                index == 0,
                index > 0,
                accounts::Look::PAGE,
            );
            if answer.picked {
                pick = Some(account.relay_id.clone());
                ui.close();
            }
            if answer.remove {
                accounts::ask_remove(form, &key, account);
                ui.close();
            }
        }
        ui.add_space(2.0);
        ui.separator();
        if accounts::action_row(ui, icons::USER_PLUS, "Add account", accounts::Look::PAGE) {
            add = true;
            ui.close();
        }
    });
    if let Some(relay_id) = pick {
        accounts::switch_to(s, online, &key, &relay_id, now);
    }
    if add {
        accounts::add_account(s, online, form, now);
    }
    accounts::remove_dialog(ui, s, online, form);
}

pub fn problems(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    match online.phase() {
        Phase::Stopped(problem) if problem.needs_password() => {
            ui.horizontal(|ui| {
                let response = ui.add(
                    row_field(ui, &mut form.new_password)
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
        Phase::Stopped(problem) if problem.needs_code() => code_row(ui, online, form, now),
        Phase::Stopped(Problem::KeyChanged) => {
            ui.add_space(4.0);
            key_card(ui, online, form, now);
        }
        _ => retry_row(ui, online, now),
    }
}

fn relay_id_row(f: &mut Form, form: &mut JoinForm, facts: &Joined, now: Instant) {
    let relay_text = facts.relay.relay_id.to_string();
    f.row_with_hint(
        "Relay ID",
        "Don't share it. Anyone with it can sign in as you.",
        |ui| {
            copy_button(ui, form, "relay", &relay_text, now);
            ui.add(egui::Label::new(RichText::new(&relay_text).monospace().weak()).truncate());
        },
    );
}

fn need_whole(ui: &Ui, text: &RichText) {
    let width = egui::WidgetText::from(text.clone())
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        )
        .size()
        .x;
    let used = ui.cursor().left() - ui.max_rect().left();
    super::form::need(ui, used + width);
}

fn online_id_row(f: &mut Form, form: &mut JoinForm, facts: &Joined, now: Instant) {
    f.row_with_hint("Online ID", "Friends add you with this", |ui| {
        copy_button(ui, form, "online", &facts.online_id, now);
        let text = RichText::new(&facts.online_id).monospace();
        need_whole(ui, &text);
        ui.add(egui::Label::new(text).truncate());
    });
}

fn relay_key_row(f: &mut Form, online: &Online, facts: &Joined) {
    if let Some(key) = facts
        .relay
        .trust
        .relay_key
        .or(online.pinned_key())
        .filter(|_| online.offered_key().is_none())
    {
        f.row_with_hint("Relay key", KEY_ABOUT, |ui| {
            let text = RichText::new(key.fingerprint().to_string()).monospace();
            need_whole(ui, &text);
            ui.add(egui::Label::new(text).truncate());
        });
    }
}

fn display_name_row(f: &mut Form, online: &mut Online, form: &mut JoinForm, facts: &Joined) {
    let account_name = &facts.account_name;
    f.row("Display name", |ui| {
        let editing = form.name_draft.get_or_insert_with(|| account_name.clone());
        let response = ui.add_enabled(
            online.is_connected(),
            row_field(ui, editing)
                .char_limit(DISPLAY_NAME_MAX_CHARS)
                .desired_width(row_field_width(ui)),
        );
        if response.lost_focus() {
            match display_name(editing) {
                Ok(name) if name != *account_name => {
                    online.set_display_name(&name);
                    *editing = name;
                }
                _ => form.name_draft = None,
            }
        } else if !response.has_focus() && form.name_draft.as_deref() == Some(account_name.as_str())
        {
            form.name_draft = None;
        }
    });
    if online.request_error() == Some(citrine_gb_net::ErrorCode::BadName) {
        f.hint(Problem::BadName.text());
    }
}

fn play_together(f: &mut Form, s: &mut Settings) {
    f.row_with_hint("Input delay", DELAY_HINT, |ui| {
        s.dirty |= GenericSelect::from_enum(&mut s.online.input_delay, "enum_select_input_delay")
            .default_value(InputDelay::Auto)
            .ui(ui)
            .changed();
    });
    s.dirty |= f.toggle_with_hint("Direct connection", DIRECT_HINT, &mut s.online.direct);
}

fn typing_text_row(f: &mut Form, online: &mut crate::online::settings::OnlineSettings) -> bool {
    let enabled = online.typing_indicators;
    let text = &mut online.typing_text;
    f.row(TYPING_TEXT, |ui| {
        let mut dirty = false;
        let reset = ui
            .add_enabled_ui(enabled, |ui| {
                with_reset(ui, !text.is_empty(), TYPING_DEFAULT, |ui| {
                    let field = row_field(ui, text)
                        .hint_text(TYPING_DEFAULT)
                        .char_limit(MAX_TYPING_CHARS)
                        .desired_width(f32::INFINITY);
                    dirty |= ui.add(field).changed();
                })
            })
            .inner;
        if reset {
            text.clear();
            dirty = true;
        }
        dirty
    })
}

pub const DELAY_HINT: &str = "Your presses count this many frames later, so online play rewinds less, and Auto picks the number from the ping.";
pub const DIRECT_HINT: &str = "Connects you straight to other players who turn this on too, instead of through the relay. That usually lowers the ping. Those players can see your internet address.";
pub const CLOUD_HINT: &str = "Keeps your saves on the relay so your other devices can load them. They're encrypted, so only your devices can read them.";
pub const SYNC_HINT: &str = "Your theme, palettes, key bindings and screen effects copy to your other devices on this relay ID, encrypted like your saves. The latest change wins. Volume, controllers, touch controls and the UI size stay on each device. Needs Cloud saves.";

pub fn no_storage(s: &Settings, online: &Online) -> Option<&'static str> {
    if !s.online.cloud_saves || !online.account().is_some_and(|a| a.quota_bytes == 0) {
        return None;
    }
    Some(if online.info().is_some_and(|i| i.quota_bytes > 0) {
        "Your account on this relay has no storage, so saves and settings stay on this device."
    } else {
        "This relay doesn't store files, so saves and settings stay on this device."
    })
}

pub fn cloud_problems(s: &Settings, online: &Online) -> Vec<&'static str> {
    if !s.online.cloud_saves {
        return Vec::new();
    }
    let settings = if s.online.sync_settings {
        online.settings_problem()
    } else {
        None
    };
    sync_problems(settings, online.stats_problem())
}

fn leave_and_delete(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    facts: &Joined,
    sign_out: bool,
) {
    let label = if sign_out {
        "Sign out"
    } else {
        "Leave this relay"
    };
    if ui.button(format!("{} {label}", icons::SIGN_OUT)).clicked() {
        form.leaving = true;
    }
    let delete = RichText::new(format!("{} Delete account", icons::TRASH))
        .color(ui.visuals().error_fg_color);
    if ui
        .add_enabled(online.is_connected(), egui::Button::new(delete))
        .clicked()
    {
        two_factor::open(form, two_factor::Ask::Delete);
    }
    leave_dialogs(ui, s, online, form, facts, sign_out);
}

fn leave_dialogs(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    facts: &Joined,
    sign_out: bool,
) {
    two_factor::ask_dialog(ui, online, form, &facts.relay.address.key());
    if form.leaving {
        let (question, button) = if sign_out {
            (
                "Sign out on this device? Citrine keeps your relay ID here, so you can sign in again."
                    .to_owned(),
                "Sign out",
            )
        } else {
            (
                format!(
                    "Leave {} on this device? Citrine keeps your relay ID for this server, so you can join again.",
                    facts.relay.address.key()
                ),
                "Leave",
            )
        };
        if let Some(leave) = confirm::confirm(ui.ctx(), "online_leave", &question, button) {
            form.leaving = false;
            if leave {
                online.sign_out_here();
                s.online.leave();
                s.dirty = true;
                form.step = Step::Address;
                form.name_draft = None;
                form.last_phase = None;
            }
        }
    }
}

pub struct Account {
    pub name: String,
    pub online_id: String,
    pub relay_id: String,
    pub two_factor: Option<bool>,
    pub devices: usize,
}

pub fn joined_account(s: &Settings, online: &Online) -> Option<Account> {
    let facts = joined_facts(s, online)?;
    Some(Account {
        relay_id: facts.relay.relay_id.to_string(),
        two_factor: online.account().map(|a| a.two_factor),
        devices: online.devices().0.len(),
        name: facts.account_name,
        online_id: facts.online_id,
    })
}

pub fn copy_now(
    ctx: &egui::Context,
    form: &mut JoinForm,
    what: &'static str,
    text: &str,
    now: Instant,
) {
    ctx.copy_text(text.to_owned());
    ctx.request_repaint_after(widgets::copy::COPIED_FOR);
    form.copied = Some((what, now));
}

pub fn just_copied(form: &JoinForm, what: &'static str, now: Instant) -> bool {
    widgets::copy::recently(
        form.copied
            .filter(|(which, _)| *which == what)
            .map(|(_, at)| at),
        now,
    )
}

pub fn name_editor(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, current: &str) -> bool {
    let draft = form.name_draft.get_or_insert_with(|| current.to_owned());
    let response =
        crate::app::mobile::kit::field(ui, row_field(ui, draft).char_limit(DISPLAY_NAME_MAX_CHARS));
    let checked = display_name(draft);
    if online.request_error() == Some(citrine_gb_net::ErrorCode::BadName) {
        widgets::problem::problem_line(ui, &Problem::BadName.text());
    }
    let ready = online.is_connected() && checked.is_ok();
    ui.add_space(12.0);
    let pressed = ui
        .add_enabled_ui(ready, |ui| {
            crate::app::mobile::kit::primary_button(ui, "Save")
        })
        .inner;
    let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let Ok(name) = checked else {
        return false;
    };
    if !(ready && (pressed || entered)) {
        return false;
    }
    if name != current {
        online.set_display_name(&name);
    }
    form.name_draft = None;
    true
}

pub fn forget_name_draft(form: &mut JoinForm) {
    form.name_draft = None;
}

pub fn two_factor_phone(
    ui: &mut Ui,
    s: &Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) {
    let Some(facts) = joined_facts(s, online) else {
        return;
    };
    two_factor::phone(
        ui,
        online,
        form,
        &facts.account_name,
        &facts.relay.address.key(),
        now,
    );
}

pub fn devices_body(ui: &mut Ui, s: &mut Settings, online: &mut Online, form: &mut JoinForm) {
    s.dirty |= devices::devices(ui, online, form, &mut s.online.device_name);
}

pub fn ask_sign_out(form: &mut JoinForm) {
    form.leaving = true;
}

pub fn ask_delete(form: &mut JoinForm) {
    two_factor::open(form, two_factor::Ask::Delete);
}

pub fn account_dialogs(ui: &mut Ui, s: &mut Settings, online: &mut Online, form: &mut JoinForm) {
    if let Some(facts) = joined_facts(s, online) {
        leave_dialogs(ui, s, online, form, &facts, true);
    }
    accounts::remove_dialog(ui, s, online, form);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AccountsChoice {
    Stay,
    Done,
    Add,
}

pub fn accounts_body(
    ui: &mut Ui,
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    now: Instant,
) -> AccountsChoice {
    let Some(key) = s.online.joined.clone() else {
        return AccountsChoice::Done;
    };
    let saved = s.online.accounts_on(&key);
    let mut pick = None;
    let mut add = false;
    ui.add_space(6.0);
    kit::list(ui, |list| {
        for (index, account) in saved.iter().enumerate() {
            let detail = accounts::detail(account, &key, false);
            let answer = list.custom(|ui| {
                accounts::account_row(
                    ui,
                    account,
                    &detail,
                    index == 0,
                    index > 0,
                    accounts::Look::CARD,
                )
            });
            if answer.picked {
                pick = Some(account.relay_id.clone());
            }
            if answer.remove {
                accounts::ask_remove(form, &key, account);
            }
        }
        add = list
            .row(Row::new("Add account").icon(icons::USER_PLUS))
            .clicked();
    });
    if let Some(relay_id) = pick {
        accounts::switch_to(s, online, &key, &relay_id, now);
        return AccountsChoice::Done;
    }
    if add {
        return AccountsChoice::Add;
    }
    AccountsChoice::Stay
}

pub fn add_account_body(ui: &mut Ui) -> Option<JoinSheet> {
    ui.add_space(6.0);
    match kit::rows(
        ui,
        vec![
            Row::new("Sign in").icon(icons::SIGN_IN),
            Row::new("Create account").icon(icons::USER_PLUS),
        ],
    ) {
        Some(0) => Some(JoinSheet::SignIn),
        Some(_) => Some(JoinSheet::Create),
        None => None,
    }
}

pub fn start_add(
    s: &mut Settings,
    online: &mut Online,
    form: &mut JoinForm,
    sheet: JoinSheet,
    now: Instant,
) {
    accounts::add_account(s, online, form, now);
    open_sheet(s, online, form, sheet, now);
}

pub fn back_out(s: &mut Settings, online: &mut Online, form: &mut JoinForm, now: Instant) {
    accounts::back_out(s, online, form, now);
}

fn sync_problems(settings: Option<SyncState>, stats: Option<SyncState>) -> Vec<&'static str> {
    let full = |state: Option<SyncState>| state == Some(SyncState::Full);
    let mut problems = Vec::new();
    match (full(settings), full(stats)) {
        (true, true) => problems
            .push("Your cloud storage is full, so your settings and stats stay on this device."),
        (true, false) => {
            problems.push("Your cloud storage is full, so your settings stay on this device.")
        }
        (false, true) => {
            problems.push("Your cloud storage is full, so your stats stay on this device.")
        }
        (false, false) => {}
    }
    if settings == Some(SyncState::TooLarge) {
        problems.push("Your settings are too large for the relay, so they stay on this device.");
    }
    if stats == Some(SyncState::TooLarge) {
        problems.push("Your stats are too large for the relay, so they stay on this device.");
    }
    problems
}

fn notifications(f: &mut Form, s: &mut Settings) {
    let allowed = crate::platform::push::allowed();
    f.text_row("Status", |ui| {
        if allowed {
            ui.label("On");
        } else {
            ui.label(RichText::new("Off").weak());
        }
    });
    f.buttons(|ui| {
        if allowed {
            if ui.button("Android settings").clicked() {
                crate::platform::push::open_settings();
            }
        } else if ui.button("Turn on").clicked() {
            crate::platform::push::ask();
        }
    });
    for (label, on) in [
        (FRIEND_REQUESTS, &mut s.online.notify_friend_requests),
        (INVITES, &mut s.online.notify_invites),
        (MESSAGES, &mut s.online.notify_chats),
    ] {
        if allowed {
            s.dirty |= f.toggle_when(label, None, true, on);
        } else {
            f.toggle_when(label, None, false, &mut false);
        }
    }
}
