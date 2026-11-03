use crate::app::mobile::{kit, sheet};

const OPEN_KEY: &str = "confirm_open";

pub fn confirm(ctx: &egui::Context, id: &str, message: &str, action: &str) -> Option<bool> {
    confirm_with_detail(ctx, id, message, None, action)
}

pub fn confirm_with_detail(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    detail: Option<&str>,
    action: &str,
) -> Option<bool> {
    dialog(ctx, id, message, detail, action, None)
}

pub fn confirm_with_option(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    action: &str,
    option: (&str, &mut bool),
) -> Option<bool> {
    dialog(ctx, id, message, None, action, Some(option))
}

fn dialog(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    detail: Option<&str>,
    action: &str,
    option: Option<(&str, &mut bool)>,
) -> Option<bool> {
    let (answer, closed) = host(ctx, id, |ui, phone| {
        message(ui, phone, text);
        if let Some(detail) = detail {
            if phone {
                sheet::note(ui, detail);
            } else {
                ui.add(egui::Label::new(egui::RichText::new(detail).small()).truncate());
            }
        }
        if let Some((label, checked)) = option {
            if phone {
                kit::list(ui, |list| list.switch(kit::SwitchRow::new(label, checked)));
                ui.add_space(14.0);
            } else {
                ui.checkbox(checked, label);
            }
        }
        buttons(ui, phone, (action, true), None)
    });
    answer
        .map(|answer| answer == Answer::Action)
        .or(closed.then_some(false))
}

fn host<R>(
    ctx: &egui::Context,
    id: &str,
    body: impl FnOnce(&mut egui::Ui, bool) -> R,
) -> (R, bool) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_KEY), pass));
    if sheet::area(ctx).is_some() {
        let shown = sheet::show(ctx, egui::Id::new(id), "", |ui| body(ui, true));
        return (shown.inner, shown.closed);
    }
    let modal = egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_max_width(300.0);
        body(ui, false)
    });
    let closed = modal.should_close();
    (modal.inner, closed)
}

fn message(ui: &mut egui::Ui, phone: bool, text: &str) {
    if phone {
        sheet::inset(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).size(16.0)).wrap());
        });
        ui.add_space(14.0);
    } else {
        ui.label(text);
    }
}

fn buttons(
    ui: &mut egui::Ui,
    phone: bool,
    action: (&str, bool),
    extra: Option<(&str, bool)>,
) -> Option<Answer> {
    let mut answer = None;
    if phone {
        let wide = |ui: &mut egui::Ui, (label, enabled): (&str, bool), primary: bool| {
            ui.add_enabled_ui(enabled, |ui| {
                if primary && destructive(label) {
                    kit::danger_button(ui, label)
                } else {
                    kit::wide_button(ui, label, primary)
                }
            })
            .inner
            .clicked()
        };
        if wide(ui, action, true) {
            answer = Some(Answer::Action);
        }
        if let Some(extra) = extra {
            ui.add_space(8.0);
            if wide(ui, extra, false) {
                answer = Some(Answer::Extra);
            }
        }
        ui.add_space(8.0);
        if ui
            .vertical_centered(|ui| kit::pill(ui, "Cancel", false))
            .inner
            .clicked()
        {
            answer = Some(Answer::Cancel);
        }
        return answer;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let size = egui::vec2(80.0, 32.0);
        if let Some((label, enabled)) = extra
            && ui
                .add_enabled(enabled, egui::Button::new(label).min_size(size))
                .clicked()
        {
            answer = Some(Answer::Extra);
        }
        if ui
            .add_enabled(action.1, egui::Button::new(action.0).min_size(size))
            .clicked()
        {
            answer = Some(Answer::Action);
        }
        if ui.add(egui::Button::new("Cancel").min_size(size)).clicked() {
            answer = Some(Answer::Cancel);
        }
    });
    answer
}

fn destructive(action: &str) -> bool {
    ["Delete", "Forget", "Remove", "Reset", "Turn off"]
        .iter()
        .any(|word| action.starts_with(word))
}

pub struct CodeAsk<'a> {
    pub message: &'a str,
    pub code: Option<&'a mut String>,
    pub problem: Option<&'a str>,
    pub action: &'a str,
}

pub fn confirm_with_code(ctx: &egui::Context, id: &str, ask: CodeAsk) -> Option<bool> {
    let ready = ask
        .code
        .as_deref()
        .is_none_or(|code| !code.trim().is_empty());
    let (answer, closed) = host(ctx, id, |ui, phone| {
        message(ui, phone, ask.message);
        let mut enter = false;
        if let Some(code) = ask.code {
            ui.add_space(4.0);
            let hint = "From your app, or a recovery code";
            if phone {
                let edit = crate::app::widgets::field::row_field(ui, code).hint_text(hint);
                let response = kit::field(ui, edit);
                enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if !enter && ui.memory(|m| m.focused().is_none()) {
                    response.request_focus();
                }
                ui.add_space(10.0);
            } else {
                ui.horizontal(|ui| {
                    ui.label("Code");
                    let width = ui.available_width();
                    enter = crate::app::widgets::field::code_field(ui, code, hint, width, true);
                });
            }
        }
        if let Some(problem) = ask.problem {
            if phone {
                sheet::inset(ui, |ui| {
                    crate::app::widgets::problem::problem_line(ui, problem)
                });
                ui.add_space(10.0);
            } else {
                crate::app::widgets::problem::problem_line(ui, problem);
            }
        }
        let answer = buttons(ui, phone, (ask.action, ready), None);
        if answer.is_none() && enter && ready {
            Some(Answer::Action)
        } else {
            answer
        }
    });
    answer
        .map(|answer| answer == Answer::Action)
        .or(closed.then_some(false))
}

pub struct Ask<'a> {
    pub message: &'a str,
    pub notes: &'a [String],
    pub action: &'a str,
    pub action_enabled: bool,
    pub extra: Option<(&'a str, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Action,
    Extra,
    Cancel,
}

pub fn ask(ctx: &egui::Context, id: &str, ask: Ask) -> Option<Answer> {
    let (answer, closed) = host(ctx, id, |ui, phone| {
        message(ui, phone, ask.message);
        for note in ask.notes {
            if phone {
                sheet::note(ui, note);
            } else {
                ui.label(egui::RichText::new(note).small());
            }
        }
        buttons(ui, phone, (ask.action, ask.action_enabled), ask.extra)
    });
    answer.or(closed.then_some(Answer::Cancel))
}

pub fn is_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(OPEN_KEY))) == Some(pass)
}

#[cfg(test)]
mod tests;
