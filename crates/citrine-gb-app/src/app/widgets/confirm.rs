use crate::app::big_screen::scroll::Following;
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
    dialog(ctx, id, message, detail, (action, "Cancel"), None)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn confirm_with_cancel(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    detail: Option<&str>,
    action: &str,
    cancel: &str,
) -> Option<bool> {
    dialog(ctx, id, message, detail, (action, cancel), None)
}

pub fn confirm_with_option(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    action: &str,
    option: (&str, &mut bool),
) -> Option<bool> {
    dialog(ctx, id, message, None, (action, "Cancel"), Some(option))
}

fn dialog(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    detail: Option<&str>,
    (action, cancel): (&str, &str),
    option: Option<(&str, &mut bool)>,
) -> Option<bool> {
    let (answer, closed) = host(ctx, id, |ui, phone| {
        message(ui, phone, text);
        if let Some(detail) = detail {
            if phone {
                phone_note(ui, detail);
            } else {
                ui.add_space(4.0);
                let weak = ui.visuals().weak_text_color();
                ui.add(egui::Label::new(egui::RichText::new(detail).color(weak)).wrap());
            }
        }
        if let Some((label, checked)) = option {
            if phone {
                kit::list(ui, |list| list.switch(kit::SwitchRow::new(label, checked)));
                ui.add_space(14.0);
            } else {
                crate::app::widgets::check::checkbox(ui, checked, label);
            }
        }
        buttons(ui, phone, (action, true), None, cancel)
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
        ui.vertical_centered(|ui| body(ui, false)).inner
    });
    let closed = modal.should_close();
    (modal.inner, closed)
}

fn message(ui: &mut egui::Ui, phone: bool, text: &str) {
    if phone {
        sheet::inset(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(text).size(16.0)).wrap());
            });
        });
        ui.add_space(14.0);
    } else {
        ui.label(text);
    }
}

fn phone_note(ui: &mut egui::Ui, text: &str) {
    sheet::inset(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).size(14.0).weak()).wrap());
        });
    });
    ui.add_space(16.0);
}

fn buttons(
    ui: &mut egui::Ui,
    phone: bool,
    action: (&str, bool),
    extra: Option<(&str, bool)>,
    cancel: &str,
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
            .vertical_centered(|ui| kit::pill(ui, cancel, false))
            .inner
            .clicked()
        {
            answer = Some(Answer::Cancel);
        }
        return answer;
    }
    ui.add_space(8.0);
    let size = egui::vec2(80.0, 32.0);
    let left = row_indent(
        ui,
        extra
            .map(|(label, _)| label)
            .into_iter()
            .chain([action.0, cancel]),
        size.x,
    );
    ui.horizontal(|ui| {
        ui.add_space(left);
        if let Some((label, enabled)) = extra
            && ui
                .add_enabled(enabled, egui::Button::new(label).min_size(size))
                .clicked()
        {
            answer = Some(Answer::Extra);
        }
        let button = if destructive(action.0) {
            super::action::danger_button(ui, action.0)
        } else {
            super::action::main_button(ui, action.0)
        };
        if ui.add_enabled(action.1, button.min_size(size)).clicked() {
            answer = Some(Answer::Action);
        }
        if ui.add(egui::Button::new(cancel).min_size(size)).clicked() {
            answer = Some(Answer::Cancel);
        }
    });
    answer
}

fn row_indent<'a>(ui: &egui::Ui, labels: impl Iterator<Item = &'a str>, min: f32) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let pad = ui.spacing().button_padding.x * 2.0;
    let gap = ui.spacing().item_spacing.x;
    let widths: Vec<f32> = labels
        .map(|label| {
            let text = ui.ctx().fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            (text + pad).max(min)
        })
        .collect();
    let row = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    ((ui.available_width() - row) / 2.0).max(0.0)
}

fn destructive(action: &str) -> bool {
    ["Delete", "Forget", "Quit", "Remove", "Reset", "Turn off"]
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
        let answer = buttons(ui, phone, (ask.action, ready), None, "Cancel");
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
    pub load: Option<(&'a str, crate::progress::Progress)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Action,
    Extra,
    Cancel,
}

pub fn ask(ctx: &egui::Context, id: &str, ask: Ask) -> Option<Answer> {
    ask_as(ctx, id, ask, "Cancel")
}

pub fn ask_as(ctx: &egui::Context, id: &str, ask: Ask, cancel: &str) -> Option<Answer> {
    let (answer, closed) = host(ctx, id, |ui, phone| {
        message(ui, phone, ask.message);
        for note in ask.notes {
            if phone {
                phone_note(ui, note);
            } else {
                ui.label(egui::RichText::new(note).small());
            }
        }
        if let Some((label, progress)) = ask.load {
            ui.add_space(8.0);
            ui.add(super::progress::ProgressBar::new(label, progress));
        }
        buttons(
            ui,
            phone,
            (ask.action, ask.action_enabled),
            ask.extra,
            cancel,
        )
    });
    answer.or(closed.then_some(Answer::Cancel))
}

pub fn pick(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    options: &[(String, String)],
) -> Option<Option<usize>> {
    let (picked, closed) = host(ctx, id, |ui, phone| {
        message(ui, phone, text);
        if phone {
            let rows = options
                .iter()
                .map(|(name, value)| kit::Row::new(name).value(value.as_str()))
                .collect();
            let picked = kit::rows(ui, rows).map(Some);
            ui.add_space(8.0);
            let cancel = ui
                .vertical_centered(|ui| kit::pill(ui, "Cancel", false))
                .inner
                .clicked();
            return picked.or(cancel.then_some(None));
        }
        ui.add_space(8.0);
        let mut picked = egui::ScrollArea::vertical()
            .max_height(320.0)
            .show_following(ui, |ui| {
                let mut picked = None;
                for (index, (name, value)) in options.iter().enumerate() {
                    let button = egui::Button::new(name)
                        .right_text(egui::RichText::new(value).weak())
                        .min_size(egui::vec2(ui.available_width(), 32.0));
                    if ui.add(button).clicked() {
                        picked = Some(Some(index));
                    }
                }
                picked
            })
            .inner;
        ui.add_space(8.0);
        if ui
            .add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 32.0)))
            .clicked()
        {
            picked = Some(None);
        }
        picked
    });
    picked.or(closed.then_some(None))
}

pub fn is_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(OPEN_KEY))) == Some(pass)
}

#[cfg(test)]
mod tests {
    use super::{Ask, CodeAsk, ask, confirm, confirm_with_code, destructive};

    #[test]
    fn enter_in_the_code_dialog_confirms() {
        let ctx = egui::Context::default();
        let mut code = "123456".to_owned();
        let mut frame = |events: Vec<egui::Event>| {
            let mut choice = None;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 800.0),
                )),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                choice = confirm_with_code(
                    ctx,
                    "code_dialog_test",
                    CodeAsk {
                        message: "Type a code",
                        code: Some(&mut code),
                        problem: None,
                        action: "Turn off",
                    },
                );
            });
            choice
        };
        assert_eq!(frame(Vec::new()), None);
        assert_eq!(frame(Vec::new()), None);
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(frame(vec![enter]), Some(true));
    }

    #[test]
    fn only_actions_that_lose_something_are_red() {
        for action in ["Delete", "Delete everything", "Forget", "Reset", "Turn off"] {
            assert!(destructive(action), "{action} should be red");
        }
        for action in ["Sign out", "Import", "Load", "Trust", "Restart"] {
            assert!(!destructive(action), "{action} should not be red");
        }
    }

    #[test]
    fn an_ask_with_a_load_shows_its_bar() {
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, 800.0),
            )),
            ..Default::default()
        };
        let notes = Vec::new();
        let mut texts = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(input(), |ctx| {
                let _ = ask(
                    ctx,
                    "load_dialog_test",
                    Ask {
                        message: "Delete everything on this device?",
                        notes: &notes,
                        action: "Delete everything",
                        action_enabled: false,
                        extra: Some(("Export first", false)),
                        load: Some((
                            "Exporting your data",
                            crate::progress::Progress::bytes(1024 * 1024, 4 * 1024 * 1024),
                        )),
                    },
                );
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        assert!(
            texts.contains(&"Exporting your data".to_owned()),
            "{texts:?}"
        );
        assert!(texts.contains(&"1.0 of 4.0 MB".to_owned()), "{texts:?}");
    }

    #[test]
    fn a_desktop_confirm_fills_its_action_red_only_when_it_loses_something() {
        let fills = |action: &str| {
            let ctx = egui::Context::default();
            let input = |frame: u32| egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            let mut output = None;
            for frame in 0..10 {
                output = Some(ctx.run(input(frame), |ctx| {
                    let _ = confirm(ctx, "desktop_fill_test", "Go on?", action);
                }));
            }
            let fills: Vec<egui::Color32> = output
                .unwrap()
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Rect(rect) => Some(rect.fill),
                    _ => None,
                })
                .collect();
            let visuals = ctx.style().visuals.clone();
            (
                fills.contains(&crate::app::ui_state::ui_theme::danger(&visuals)),
                fills.contains(&crate::app::ui_state::ui_theme::brand(&visuals)),
            )
        };
        assert_eq!(fills("Reset"), (true, false));
        assert_eq!(fills("Load"), (false, true));
    }

    #[test]
    fn a_phone_confirm_centres_its_question_and_note_over_cancel() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(412.0, 915.0));
        let input = |frame: u32| egui::RawInput {
            screen_rect: Some(screen),
            time: Some(f64::from(frame) * 0.1),
            ..Default::default()
        };
        let mut output = None;
        for frame in 0..20 {
            output = Some(ctx.run(input(frame), |ctx| {
                crate::app::mobile::sheet::set_area(ctx, Some(screen));
                let _ = super::confirm_with_detail(
                    ctx,
                    "phone_centre_test",
                    "Reset the game?",
                    Some("Unsaved progress is lost."),
                    "Reset",
                );
            }));
        }
        let centres: Vec<(String, f32)> = output
            .unwrap()
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.visual_bounding_rect().center().x,
                )),
                _ => None,
            })
            .collect();
        let centre = |wanted: &str| {
            centres
                .iter()
                .find(|(t, _)| t == wanted)
                .map(|(_, x)| *x)
                .unwrap_or_else(|| panic!("no {wanted} in {centres:?}"))
        };
        let cancel = centre("Cancel");
        for line in ["Reset the game?", "Unsaved progress is lost.", "Reset"] {
            let x = centre(line);
            assert!(
                (x - cancel).abs() < 2.0,
                "{line} at {x}, Cancel at {cancel}"
            );
        }
    }

    #[test]
    fn a_desktop_confirm_centres_its_question_note_and_buttons() {
        let ctx = egui::Context::default();
        let input = |frame: u32| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            time: Some(f64::from(frame) * 0.1),
            ..Default::default()
        };
        let mut output = None;
        for frame in 0..10 {
            output = Some(ctx.run(input(frame), |ctx| {
                let _ = super::confirm_with_detail(
                    ctx,
                    "desktop_centre_test",
                    "Reset the game?",
                    Some("Unsaved progress is lost."),
                    "Reset",
                );
            }));
        }
        let texts: Vec<(String, egui::Rect)> = output
            .unwrap()
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect();
        let rect = |wanted: &str| {
            texts
                .iter()
                .find(|(t, _)| t == wanted)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("no {wanted} in {texts:?}"))
        };
        let question = rect("Reset the game?").center().x;
        let note = rect("Unsaved progress is lost.").center().x;
        let row = (rect("Reset").center().x + rect("Cancel").center().x) / 2.0;
        for (what, x) in [("note", note), ("buttons", row)] {
            assert!(
                (x - question).abs() < 3.0,
                "{what} at {x}, question at {question}"
            );
        }
    }
}
