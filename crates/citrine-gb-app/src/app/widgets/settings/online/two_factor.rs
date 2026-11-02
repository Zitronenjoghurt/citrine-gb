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

const ABOUT: &str =
    "Asks for a code from an authenticator app when your relay ID signs in on another device.";
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
    ui.add_space(ui.spacing().interact_size.y.max(20.0));
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
            let text = format!("Citrine recovery codes for {name} on {host}\n\n{all}\n");
            let (tx, _rx) = std::sync::mpsc::channel();
            crate::utils::file_saver::FileSaver::new(CODES_FILE)
                .add_filter("Text", &["txt"])
                .dispatch(text.into_bytes(), tx);
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
