use super::{JoinForm, copy_button, primary};
use crate::app::widgets::confirm::{CodeAsk, confirm_with_code};
use crate::app::widgets::field::row_field;
use crate::app::widgets::info_mark::info_mark;
use crate::app::widgets::problem::problem_line;
use crate::app::widgets::qr::Qr;
use crate::app::widgets::settings::form::Form;
use crate::icons;
use crate::online::two_factor::{grouped, otpauth_url, secret_text};
use crate::online::{Online, Problem};
use citrine_gb_net::ErrorCode;
use egui::{RichText, Ui};
use web_time::Instant;

const ABOUT: &str = "Asks for a code from an authenticator app when someone uses your sign-in key on another device.";
const QR_SIDE: f32 = 184.0;
const CODES_FILE: &str = "citrine-recovery-codes.txt";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Ask {
    TurnOff,
    NewCodes,
    Delete,
}

pub(super) fn code_problem(online: &Online) -> Option<String> {
    match online.request_error()? {
        ErrorCode::CodeWrong => Some(Problem::CodeWrong.text()),
        ErrorCode::TooManyTries | ErrorCode::RateLimited => Some(Problem::TooManyTries.text()),
        _ => None,
    }
}

pub(super) fn two_factor(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    name: &str,
    host: &str,
    now: Instant,
) {
    let Some(account) = online.account() else {
        return;
    };
    let (on, left) = (account.two_factor, account.codes_left);
    Form::show(ui, "online_two_factor", |f| {
        f.text_row("Two-factor sign-in", |ui| {
            if on {
                ui.label("On");
                ui.label(RichText::new(format!("· {left} recovery codes left")).weak());
            } else {
                ui.label(RichText::new("Off").weak());
            }
            info_mark(ui, ABOUT);
        });
    });
    if let Some(codes) = online.recovery_codes().map(<[String]>::to_vec) {
        recovery_codes(ui, online, form, &codes, name, host, now);
    } else if let Some(secret) = online.two_factor_setup().map(<[u8]>::to_vec) {
        setup(ui, online, form, &secret, name, now);
    } else if on {
        ui.horizontal(|ui| {
            let connected = online.is_connected();
            if ui
                .add_enabled(connected, egui::Button::new("Turn off"))
                .clicked()
            {
                open(form, Ask::TurnOff);
            }
            if ui
                .add_enabled(connected, egui::Button::new("New recovery codes"))
                .clicked()
            {
                open(form, Ask::NewCodes);
            }
        });
    } else if ui
        .add_enabled(online.is_connected(), egui::Button::new("Set up"))
        .clicked()
    {
        form.setup_code.clear();
        online.start_two_factor();
    }
}

pub(super) fn phone(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    name: &str,
    host: &str,
    now: Instant,
) {
    use crate::app::mobile::kit::{self, Row};
    use crate::app::mobile::sheet;
    let Some(account) = online.account() else {
        return;
    };
    let (on, left) = (account.two_factor, account.codes_left);
    if let Some(codes) = online.recovery_codes().map(<[String]>::to_vec) {
        phone_codes(ui, online, form, &codes, name, host, now);
        return;
    }
    if let Some(secret) = online.two_factor_setup().map(<[u8]>::to_vec) {
        phone_setup(ui, online, form, &secret, name, now);
        return;
    }
    sheet::note(ui, ABOUT);
    let connected = online.is_connected();
    if on {
        let mut ask = None;
        ui.add_enabled_ui(connected, |ui| {
            kit::list(ui, |list| {
                list.row(
                    Row::new("Recovery codes")
                        .value(format!("{left} left"))
                        .no_chevron(),
                );
                if list
                    .row(Row::new("New recovery codes").no_chevron())
                    .clicked()
                {
                    ask = Some(Ask::NewCodes);
                }
                if list
                    .row(Row::new("Turn off").danger().no_chevron())
                    .clicked()
                {
                    ask = Some(Ask::TurnOff);
                }
            });
        });
        if let Some(ask) = ask {
            open(form, ask);
        }
    } else if ui
        .add_enabled_ui(connected, |ui| kit::primary_button(ui, "Set up"))
        .inner
    {
        form.setup_code.clear();
        online.start_two_factor();
    }
}

fn phone_setup(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    secret: &[u8],
    name: &str,
    now: Instant,
) {
    use crate::app::mobile::{kit, sheet};
    sheet::note(
        ui,
        "Copy the key into an authenticator app, or scan the QR code with it, then type the code it shows.",
    );
    ui.vertical_centered(|ui| {
        let side = QR_SIDE.min(ui.available_width());
        ui.add(Qr::new(&otpauth_url(secret, name), side));
    });
    ui.add_space(12.0);
    let copied = super::just_copied(form, "totp", now);
    let label = if copied {
        format!("{} Copied", icons::CHECK)
    } else {
        format!("{} Copy key", icons::COPY)
    };
    if kit::wide_button(ui, &label, false).clicked() {
        super::copy_now(ui.ctx(), form, "totp", &secret_text(secret), now);
    }
    ui.add_space(12.0);
    let response = kit::field(
        ui,
        row_field(ui, &mut form.setup_code)
            .hint_text("6-digit code")
            .char_limit(7),
    );
    let mut go = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if let Some(problem) = code_problem(online) {
        ui.add_space(6.0);
        sheet::inset(ui, |ui| problem_line(ui, &problem));
    }
    ui.add_space(14.0);
    let digits = form.setup_code.chars().filter(char::is_ascii_digit).count() == 6;
    go |= ui
        .add_enabled_ui(digits && online.is_connected(), |ui| {
            kit::wide_button(ui, "Turn on", true)
        })
        .inner
        .clicked();
    ui.add_space(8.0);
    if ui
        .vertical_centered(|ui| kit::pill(ui, "Cancel", false))
        .inner
        .clicked()
    {
        online.cancel_two_factor();
        form.setup_code.clear();
    }
    if go && digits {
        online.confirm_two_factor(form.setup_code.trim().to_owned());
    }
}

fn phone_codes(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    codes: &[String],
    name: &str,
    host: &str,
    now: Instant,
) {
    use crate::app::mobile::{kit, sheet};
    form.setup_code.clear();
    sheet::note(
        ui,
        "Each recovery code signs in once if you lose your phone. They won't show again.",
    );
    egui::Frame::new()
        .fill(kit::icon_tile_fill(ui.visuals()))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.columns(2, |columns| {
                for (index, code) in codes.iter().enumerate() {
                    columns[index % 2].vertical_centered(|ui| {
                        ui.label(RichText::new(code).monospace().size(15.0));
                    });
                }
            });
        });
    ui.add_space(14.0);
    let all = codes.join("\n");
    let copied = super::just_copied(form, "codes", now);
    let label = if copied {
        format!("{} Copied", icons::CHECK)
    } else {
        format!("{} Copy codes", icons::COPY)
    };
    if kit::wide_button(ui, &label, false).clicked() {
        super::copy_now(ui.ctx(), form, "codes", &all, now);
    }
    ui.add_space(8.0);
    if kit::wide_button(
        ui,
        &format!("{} Save as file", icons::DOWNLOAD_SIMPLE),
        false,
    )
    .clicked()
    {
        save_codes(name, host, &all);
    }
    ui.add_space(8.0);
    if kit::primary_button(ui, "Done") {
        online.forget_recovery_codes();
    }
}

fn save_codes(name: &str, host: &str, all: &str) {
    let text = format!("Citrine recovery codes for {name} on {host}\n\n{all}\n");
    let (tx, _rx) = std::sync::mpsc::channel();
    crate::utils::file_saver::FileSaver::new(CODES_FILE)
        .add_filter("Text", &["txt"])
        .dispatch(text.into_bytes(), tx);
}

pub(super) fn open(form: &mut JoinForm, ask: Ask) {
    form.ask = Some(ask);
    form.ask_code.clear();
    form.ask_sent = false;
}

fn setup(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    secret: &[u8],
    name: &str,
    now: Instant,
) {
    ui.label("Scan this with an authenticator app, then type the code it shows.");
    ui.add_space(4.0);
    let side = QR_SIDE.min(ui.available_width());
    ui.add(Qr::new(&otpauth_url(secret, name), side));
    ui.add_space(4.0);
    let key = secret_text(secret);
    let mut go = false;
    Form::show(ui, "online_two_factor_setup", |f| {
        f.row("Key", |ui| {
            copy_button(ui, form, "totp", &key, now);
            ui.add(egui::Label::new(RichText::new(grouped(&key)).monospace().weak()).truncate());
        });
        f.row("Code", |ui| {
            let response = ui.add(
                row_field(ui, &mut form.setup_code)
                    .hint_text("6-digit code")
                    .char_limit(7)
                    .desired_width(ui.available_width()),
            );
            go |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
    });
    if let Some(problem) = code_problem(online) {
        problem_line(ui, &problem);
    }
    ui.add_space(4.0);
    let digits = form.setup_code.chars().filter(char::is_ascii_digit).count() == 6;
    ui.horizontal(|ui| {
        if ui.button("Cancel").clicked() {
            online.cancel_two_factor();
            form.setup_code.clear();
        }
        go |= primary(ui, "Turn on", digits && online.is_connected());
    });
    if go && digits {
        online.confirm_two_factor(form.setup_code.trim().to_owned());
    }
}

fn recovery_codes(
    ui: &mut Ui,
    online: &mut Online,
    form: &mut JoinForm,
    codes: &[String],
    name: &str,
    host: &str,
    now: Instant,
) {
    form.setup_code.clear();
    ui.add_space(4.0);
    ui.label(RichText::new("Recovery codes").strong());
    ui.label("Each one signs in once if you lose your phone. They won't show again.");
    ui.add_space(4.0);
    let code_width = codes.first().map_or(0.0, |code| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(
                code.clone(),
                egui::TextStyle::Monospace.resolve(ui.style()),
                egui::Color32::PLACEHOLDER,
            )
            .size()
            .x
        })
    });
    let gap = 16.0;
    let columns = if ui.available_width() >= 2.0 * code_width + gap {
        2
    } else {
        1
    };
    egui::Grid::new("online_recovery_codes")
        .num_columns(columns)
        .spacing([gap, 2.0])
        .show(ui, |ui| {
            for row in codes.chunks(columns) {
                for code in row {
                    ui.label(RichText::new(code).monospace());
                }
                ui.end_row();
            }
        });
    ui.add_space(4.0);
    let all = codes.join("\n");
    ui.horizontal_wrapped(|ui| {
        copy_button(ui, form, "codes", &all, now);
        if ui
            .button(format!("{} Save as file", icons::DOWNLOAD_SIMPLE))
            .clicked()
        {
            save_codes(name, host, &all);
        }
        if ui.button("Done").clicked() {
            online.forget_recovery_codes();
        }
    });
}

pub(super) fn ask_dialog(ui: &mut Ui, online: &mut Online, form: &mut JoinForm, host: &str) {
    let Some(ask) = form.ask else {
        return;
    };
    let on = online.account().is_some_and(|a| a.two_factor);
    let done = form.ask_sent
        && match ask {
            Ask::TurnOff => !on,
            Ask::NewCodes => online.recovery_codes().is_some(),
            Ask::Delete => false,
        };
    if done {
        form.ask = None;
        return;
    }
    let message = match ask {
        Ask::TurnOff => "Turn off two-factor sign-in? Type a code from your authenticator app or a recovery code.".to_owned(),
        Ask::NewCodes => "New recovery codes replace the old ones. Type a code from your authenticator app.".to_owned(),
        Ask::Delete => format!(
            "Delete your account on {host}? Your friends, chats, shared files and cloud saves there are deleted for good. Saves on this device stay."
        ),
    };
    let action = match ask {
        Ask::TurnOff => "Turn off",
        Ask::NewCodes => "Make new codes",
        Ask::Delete => "Delete",
    };
    let problem = form.ask_sent.then(|| code_problem(online)).flatten();
    let needs_code = ask != Ask::Delete || on;
    let answer = confirm_with_code(
        ui.ctx(),
        "online_code_ask",
        CodeAsk {
            message: &message,
            code: needs_code.then_some(&mut form.ask_code),
            problem: problem.as_deref(),
            action,
        },
    );
    match answer {
        Some(true) => {
            let code = form.ask_code.trim().to_owned();
            form.ask_sent = match ask {
                Ask::TurnOff => online.turn_off_two_factor(code),
                Ask::NewCodes => online.new_recovery_codes(code),
                Ask::Delete => online.delete_account(needs_code.then_some(code)),
            };
        }
        Some(false) => {
            form.ask = None;
            form.ask_code.clear();
            online.clear_request_error();
        }
        None => {}
    }
}
