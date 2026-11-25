use crate::app::big_screen::first_choice;
use crate::app::big_screen::scroll::Following;
use crate::app::mobile::{kit, sheet};
use crate::app::widgets::security_code::SecurityCode;
use crate::app::widgets::text::balanced_label;

const OPEN_KEY: &str = "confirm_open";
const WIDTH: f32 = 300.0;
const LIST_WIDTH: f32 = 380.0;
const TWO_COLUMNS_PAST: usize = 4;
const COLUMN_GAP: f32 = 24.0;
const ANSWERED_KEY: &str = "confirm_answered";
const LIT_KEY: &str = "confirm_lit";
const BACKDROP: egui::Color32 = egui::Color32::from_black_alpha(100);

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
    dialog(ctx, id, message, detail, (action, "Cancel"), None, false)
}

pub const PROGRESS_LOST: &str = "Unsaved progress is lost.";

pub fn confirm_losing_progress(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    action: &str,
) -> Option<bool> {
    dialog(
        ctx,
        id,
        message,
        Some(PROGRESS_LOST),
        (action, "Cancel"),
        None,
        true,
    )
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
    dialog(ctx, id, message, detail, (action, cancel), None, false)
}

pub fn confirm_with_option(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    action: &str,
    option: (&str, &mut bool),
) -> Option<bool> {
    dialog(
        ctx,
        id,
        message,
        None,
        (action, "Cancel"),
        Some(option),
        false,
    )
}

fn dialog(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    detail: Option<&str>,
    (action, cancel): (&str, &str),
    option: Option<(&str, &mut bool)>,
    losing: bool,
) -> Option<bool> {
    let (answer, closed) = host(ctx, id, WIDTH, |ui, phone| {
        message_with(ui, phone, text, detail.as_slice());
        if let Some((label, checked)) = option {
            if phone {
                kit::list(ui, |list| list.switch(kit::SwitchRow::new(label, checked)));
                ui.add_space(14.0);
            } else {
                ui.add_space(OPTION_ABOVE - ui.spacing().item_spacing.y);
                ringed_as_button(ui, |ui| {
                    crate::app::widgets::check::checkbox(ui, checked, label)
                });
                ui.add_space(OPTION_BELOW - BUTTONS_ABOVE - ui.spacing().item_spacing.y);
            }
        }
        let first = losing || cancel_first(action);
        buttons(ui, phone, (action, true), None, (cancel, first))
    });
    answered(
        ctx,
        answer
            .map(|answer| answer == Answer::Action)
            .or(closed.then_some(false)),
    )
}

fn answered<T>(ctx: &egui::Context, answer: Option<T>) -> Option<T> {
    if answer.is_some() {
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(ANSWERED_KEY), pass));
    }
    answer
}

fn host<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    body: impl FnOnce(&mut egui::Ui, bool) -> R,
) -> (R, bool) {
    host_wide(ctx, id, width, None, body)
}

fn host_wide<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    wide: Option<f32>,
    body: impl FnOnce(&mut egui::Ui, bool) -> R,
) -> (R, bool) {
    let pass = ctx.cumulative_pass_nr();
    let before = ctx.memory(|m| m.focused());
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::new(OPEN_KEY), pass);
        d.insert_temp(focus_before_id(), before);
    });
    if sheet::area(ctx).is_some() {
        let shown = sheet::dialog(ctx, egui::Id::new(id), |ui| body(ui, true));
        return (shown.inner, shown.closed);
    }
    let frame = egui::Frame::popup(&ctx.style()).inner_margin(egui::Margin::same(CARD_PADDING));
    let modal = egui::Modal::new(egui::Id::new(id))
        .area(
            egui::Modal::default_area(egui::Id::new(id))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        )
        .frame(egui::Frame::NONE)
        .backdrop_color(egui::Color32::TRANSPARENT)
        .show(ctx, |ui| {
            dim(ui, ui.ctx().content_rect(), BACKDROP);
            frame
                .show(ui, |ui| {
                    ui.set_max_width(wide.unwrap_or(width));
                    if let Some(width) = wide {
                        ui.set_min_width(width);
                    }
                    ui.vertical_centered(|ui| body(ui, false)).inner
                })
                .inner
        });
    let closed = modal.should_close();
    (modal.inner, closed)
}

fn focus_before_id() -> egui::Id {
    egui::Id::new(OPEN_KEY).with("focus_before")
}

const PHONE_TITLE: f32 = 20.0;
pub const PHONE_NOTE: f32 = 14.0;
const PHONE_TITLE_GAP: f32 = 4.0;
const PHONE_TEXT_GAP: f32 = 20.0;
const OPTION_ABOVE: f32 = 20.0;
const OPTION_BELOW: f32 = 17.0;
const BUTTONS_ABOVE: f32 = 8.0;
const SIDE_PADDING: f32 = 12.0;
const BUTTON_HEIGHT: f32 = 32.0;
const RING_GAP: f32 = 2.0;
const RING_WIDTH: f32 = 2.0;
const CARD_PADDING: i8 = 10;
const PHONE_BUTTON_GAP: f32 = 12.0;

fn message(ui: &mut egui::Ui, phone: bool, text: &str) {
    message_with(ui, phone, text, &[]);
}

fn message_with(ui: &mut egui::Ui, phone: bool, text: &str, notes: &[&str]) {
    let (title, rest) = title_and_rest(text);
    let notes: Vec<&str> = rest.into_iter().chain(notes.iter().copied()).collect();
    if !phone {
        balanced_label(ui, egui::RichText::new(title).heading().strong());
        for line in notes {
            balanced_label(ui, egui::RichText::new(line));
        }
        return;
    }
    sheet::inset(ui, |ui| {
        ui.vertical_centered(|ui| {
            balanced_label(ui, egui::RichText::new(title).size(PHONE_TITLE).strong());
            if !notes.is_empty() {
                ui.add_space(PHONE_TITLE_GAP);
            }
            for line in notes {
                balanced_label(ui, egui::RichText::new(line).size(PHONE_NOTE));
            }
        });
    });
    ui.add_space(PHONE_TEXT_GAP);
}

fn title_and_rest(text: &str) -> (&str, Option<&str>) {
    let end = text
        .find("? ")
        .or_else(|| text.find(". "))
        .filter(|end| !text[end + 2..].trim().is_empty());
    match end {
        Some(end) => (&text[..=end], Some(text[end + 2..].trim())),
        None => (text, None),
    }
}

fn buttons(
    ui: &mut egui::Ui,
    phone: bool,
    action: (&str, bool),
    extra: Option<(&str, bool)>,
    (cancel, first): (&str, bool),
) -> Option<Answer> {
    let mut answer = None;
    if phone {
        keep_focused_size(ui);
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
        if let Some(extra) = extra {
            if wide(ui, extra, false) {
                answer = Some(Answer::Extra);
            }
            ui.add_space(PHONE_BUTTON_GAP - ui.spacing().item_spacing.y);
        }
        if wide(ui, action, true) {
            answer = Some(Answer::Action);
        }
        ui.add_space(PHONE_BUTTON_GAP - ui.spacing().item_spacing.y);
        let cancel = kit::wide_button(ui, cancel, false);
        first_choice(&cancel);
        first_tab(ui, &cancel);
        if focus_cancel_first(ui, first, &cancel) {
            cancel.scroll_to_me(None);
        }
        if cancel.clicked() {
            answer = Some(Answer::Cancel);
        }
        return answer;
    }
    ui.add_space(BUTTONS_ABOVE);
    let left = row_indent(
        ui,
        extra
            .map(|(label, _)| label)
            .into_iter()
            .chain([action.0, cancel]),
    );
    let row = egui::vec2(ui.available_width() - 2.0 * left, BUTTON_HEIGHT);
    ui.allocate_ui_with_layout(
        row,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().button_padding.x = SIDE_PADDING;
            let size = egui::vec2(0.0, BUTTON_HEIGHT);
            if let Some((label, enabled)) = extra
                && ringed(ui, |ui| {
                    ui.add_enabled(enabled, egui::Button::new(label).min_size(size))
                })
                .clicked()
            {
                answer = Some(Answer::Extra);
            }
            let button = if destructive(action.0) {
                super::action::danger_button(ui, action.0)
            } else {
                super::action::main_button(ui, action.0)
            };
            if ringed(ui, |ui| ui.add_enabled(action.1, button.min_size(size))).clicked() {
                answer = Some(Answer::Action);
            }
            let cancel = ringed(ui, |ui| ui.add(egui::Button::new(cancel).min_size(size)));
            first_choice(&cancel);
            first_tab(ui, &cancel);
            focus_cancel_first(ui, first, &cancel);
            if cancel.clicked() {
                answer = Some(Answer::Cancel);
            }
        },
    );
    answer
}

fn focus_cancel_first(ui: &egui::Ui, first: bool, cancel: &egui::Response) -> bool {
    if !first {
        return false;
    }
    let ctx = ui.ctx();
    let outside = ctx.memory(|m| m.focused()).is_some_and(|id| {
        ctx.read_response(id)
            .is_none_or(|focused| focused.layer_id != cancel.layer_id)
    });
    if outside {
        cancel.request_focus();
    }
    outside
}

fn cancel_first(action: &str) -> bool {
    destructive(action)
        || ["Clear", "Replace"]
            .iter()
            .any(|word| action.starts_with(word))
}

fn keep_focused_size(ui: &mut egui::Ui) {
    if !ui.input(|i| i.pointer.any_down()) {
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.active.expansion = widgets.inactive.expansion;
    }
}

fn first_tab(ui: &egui::Ui, cancel: &egui::Response) {
    let tab = ui.input(|i| i.key_pressed(egui::Key::Tab) && !i.modifiers.shift);
    if !tab {
        return;
    }
    let before = ui.data(|d| d.get_temp::<Option<egui::Id>>(focus_before_id()).flatten());
    let inside = before
        .and_then(|id| ui.ctx().read_response(id))
        .is_some_and(|response| response.layer_id == cancel.layer_id);
    if !inside {
        cancel.request_focus();
    }
}

fn ringed(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> egui::Response) -> egui::Response {
    ringed_padded(ui, |_| egui::Vec2::ZERO, add)
}

fn ringed_as_button(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    ringed_padded(
        ui,
        |rect| {
            egui::vec2(
                SIDE_PADDING,
                ((BUTTON_HEIGHT - rect.height()) / 2.0).max(0.0),
            )
        },
        add,
    )
}

fn ringed_padded(
    ui: &mut egui::Ui,
    pad: impl FnOnce(egui::Rect) -> egui::Vec2,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let id = ui.next_auto_id();
    let keyed = ui.memory(|m| m.has_focus(id)) && !ui.input(|i| i.pointer.any_down());
    let active = ui.visuals().widgets.active;
    if keyed {
        let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.active = if hovered {
            widgets.hovered
        } else {
            widgets.inactive
        };
    }
    let response = add(ui);
    ui.visuals_mut().widgets.active = active;
    if response.has_focus() && !crate::app::big_screen::ring::shown(&response) {
        let visuals = ui.visuals();
        let rect = response.rect.expand2(pad(response.rect)).expand(RING_GAP);
        let radius = visuals.widgets.inactive.corner_radius + RING_GAP as u8;
        let color = crate::app::ui_state::ui_theme::focus_ring(visuals);
        ui.ctx()
            .layer_painter(response.layer_id)
            .with_clip_rect(rect.expand(RING_WIDTH + 1.0))
            .rect_stroke(
                rect,
                radius,
                egui::Stroke::new(RING_WIDTH, color),
                egui::StrokeKind::Outside,
            );
    }
    response
}

fn row_indent<'a>(ui: &egui::Ui, labels: impl Iterator<Item = &'a str>) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let pad = SIDE_PADDING * 2.0;
    let gap = ui.spacing().item_spacing.x;
    let widths: Vec<f32> = labels
        .map(|label| {
            let text = ui.ctx().fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            text + pad
        })
        .collect();
    let row = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let centre = ui.min_rect().center().x - ui.max_rect().left();
    (centre - row / 2.0).max(0.0)
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
    let (answer, closed) = host(ctx, id, WIDTH, |ui, phone| {
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
        let answer = buttons(
            ui,
            phone,
            (ask.action, ready),
            None,
            ("Cancel", cancel_first(ask.action)),
        );
        if answer.is_none() && enter && ready {
            Some(Answer::Action)
        } else {
            answer
        }
    });
    answered(
        ctx,
        answer
            .map(|answer| answer == Answer::Action)
            .or(closed.then_some(false)),
    )
}

pub fn name(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    name: &mut String,
    action: &str,
) -> Option<bool> {
    let ready = !name.trim().is_empty();
    let (answer, closed) = host(ctx, id, WIDTH, |ui, phone| {
        message(ui, phone, text);
        ui.add_space(4.0);
        let enter = if phone {
            let response = kit::field(ui, crate::app::widgets::field::row_field(ui, name));
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !enter && ui.memory(|m| m.focused().is_none()) {
                response.request_focus();
            }
            ui.add_space(10.0);
            enter
        } else {
            let width = ui.available_width();
            crate::app::widgets::field::code_field(ui, name, "", width, true)
        };
        let answer = buttons(
            ui,
            phone,
            (action, ready),
            None,
            ("Cancel", cancel_first(action)),
        );
        if answer.is_none() && enter && ready {
            Some(Answer::Action)
        } else {
            answer
        }
    });
    answered(
        ctx,
        answer
            .map(|answer| answer == Answer::Action)
            .or(closed.then_some(false)),
    )
}

pub struct Ask<'a> {
    pub message: &'a str,
    pub list: &'a [String],
    pub notes: &'a [String],
    pub action: &'a str,
    pub action_enabled: bool,
    pub extra: Option<(&'a str, bool)>,
    pub load: Option<(&'a str, crate::progress::Progress)>,
    pub code: Option<SecurityCode<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Action,
    Extra,
    Cancel,
}

const CODE_ASK_WIDTH: f32 = 336.0;

pub fn ask(ctx: &egui::Context, id: &str, ask: Ask) -> Option<Answer> {
    ask_as(ctx, id, ask, "Cancel")
}

pub fn ask_as(ctx: &egui::Context, id: &str, ask: Ask, cancel: &str) -> Option<Answer> {
    let wide = ask.code.is_some().then_some(CODE_ASK_WIDTH);
    let width = if ask.list.is_empty() {
        WIDTH
    } else {
        LIST_WIDTH
    };
    let (answer, closed) = host_wide(ctx, id, width, wide, |ui, phone| {
        if ask.list.is_empty() {
            let top = ui.cursor().top();
            let notes: Vec<&str> = ask.notes.iter().map(String::as_str).collect();
            message_with(ui, phone, ask.message, &notes);
            if let Some(code) = &ask.code {
                let block = egui::Rect::from_x_y_ranges(
                    ui.max_rect().x_range(),
                    top..=ui.min_rect().bottom(),
                );
                if phone {
                    sheet::inset(ui, |ui| {
                        ui.vertical_centered(|ui| code.show_under(ui, block))
                    });
                    ui.add_space(16.0);
                } else {
                    let room_id = egui::Id::new(id).with("hint_room");
                    let room = ui.data(|d| d.get_temp::<f32>(room_id)).unwrap_or(0.0);
                    ui.add_space(14.0 + room);
                    let block = egui::Rect::from_x_y_ranges(block.x_range(), top..=f32::INFINITY);
                    let short = code.show_under(ui, block);
                    let wanted = (room + short).max(0.0);
                    if (wanted - room).abs() > 0.5 {
                        ui.data_mut(|d| d.insert_temp(room_id, wanted));
                        ui.ctx().request_discard("room for the security code hint");
                    }
                }
            }
        } else {
            listed(ui, phone, &ask);
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
            (cancel, cancel_first(ask.action)),
        )
    });
    answered(ctx, answer.or(closed.then_some(Answer::Cancel)))
}

fn listed(ui: &mut egui::Ui, phone: bool, ask: &Ask) {
    let sized = |text: egui::RichText, size: f32| if phone { text.size(size) } else { text };
    let body = |ui: &mut egui::Ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            let title = egui::RichText::new(ask.message);
            let title = if phone {
                title.size(PHONE_TITLE)
            } else {
                title.heading()
            };
            ui.add(egui::Label::new(title.strong()).wrap());
            ui.add_space(10.0);
            list_lines(ui, ask.list, phone);
            if !ask.notes.is_empty() {
                ui.add_space(10.0);
            }
            for note in ask.notes {
                let text = sized(egui::RichText::new(note.as_str()).weak(), PHONE_NOTE);
                ui.add(egui::Label::new(text).wrap());
            }
        });
    };
    if phone {
        sheet::inset(ui, body);
        ui.add_space(16.0);
    } else {
        body(ui);
    }
}

fn list_lines(ui: &mut egui::Ui, list: &[String], phone: bool) {
    let text = |line: &String| {
        let text = egui::RichText::new(line.as_str());
        if phone { text.size(16.0) } else { text }
    };
    let column = (ui.available_width() - COLUMN_GAP) / 2.0;
    let widest = list
        .iter()
        .map(|line| {
            egui::WidgetText::from(text(line))
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Body,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max);
    if !phone || list.len() <= TWO_COLUMNS_PAST || widest > column {
        for line in list {
            ui.add(egui::Label::new(text(line)).wrap());
        }
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = COLUMN_GAP;
        for half in list.chunks(list.len().div_ceil(2)) {
            ui.allocate_ui_with_layout(
                egui::vec2(column, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(column);
                    for line in half {
                        ui.add(egui::Label::new(text(line)));
                    }
                },
            );
        }
    });
}

pub fn pick(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    options: &[(String, String)],
) -> Option<Option<usize>> {
    let (picked, closed) = host(ctx, id, WIDTH, |ui, phone| {
        message(ui, phone, text);
        if phone {
            let rows = options
                .iter()
                .map(|(name, value)| kit::Row::new(name).value(value.as_str()))
                .collect();
            let picked = kit::rows(ui, rows).map(Some);
            keep_focused_size(ui);
            ui.add_space(PHONE_BUTTON_GAP - ui.spacing().item_spacing.y);
            let cancel = kit::wide_button(ui, "Cancel", false).clicked();
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
        ui.add_space(BUTTONS_ABOVE);
        ui.spacing_mut().button_padding.x = SIDE_PADDING;
        if ringed(ui, |ui| {
            ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, BUTTON_HEIGHT)))
        })
        .clicked()
        {
            picked = Some(None);
        }
        picked
    });
    answered(ctx, picked.or(closed.then_some(None)))
}

pub fn is_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(OPEN_KEY))) == Some(pass)
}

pub fn keep_lit(ui: &egui::Ui, rect: egui::Rect) {
    let pass = ui.ctx().cumulative_pass_nr();
    let lit = rect.intersect(ui.clip_rect());
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(LIT_KEY), (pass, lit)));
}

pub fn lit(ctx: &egui::Context) -> Option<egui::Rect> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(u64, egui::Rect)>(egui::Id::new(LIT_KEY)))
        .filter(|(at, _)| pass.saturating_sub(*at) <= 1)
        .map(|(_, rect)| rect)
}

#[cfg(test)]
pub fn flat_fill(shape: &egui::Shape) -> Option<(egui::Rect, egui::Color32)> {
    match shape {
        egui::Shape::Rect(shape) => Some((shape.rect, shape.fill)),
        egui::Shape::Mesh(mesh)
            if mesh.vertices.len() == 4 && mesh.texture_id == egui::TextureId::default() =>
        {
            let color = mesh.vertices[0].color;
            let corners: Vec<egui::Pos2> = mesh.vertices.iter().map(|v| v.pos).collect();
            mesh.vertices
                .iter()
                .all(|v| v.color == color)
                .then(|| (egui::Rect::from_points(&corners), color))
        }
        _ => None,
    }
}

pub fn dim(ui: &egui::Ui, area: egui::Rect, color: egui::Color32) {
    let mut mesh = egui::Mesh::default();
    for rect in dim_rects(area, lit(ui.ctx())) {
        mesh.add_colored_rect(rect, color);
    }
    ui.ctx()
        .layer_painter(ui.layer_id())
        .add(egui::Shape::mesh(mesh));
}

fn dim_rects(area: egui::Rect, lit: Option<egui::Rect>) -> Vec<egui::Rect> {
    let Some(lit) = lit
        .map(|lit| lit.intersect(area))
        .filter(|lit| lit.is_positive())
    else {
        return vec![area];
    };
    [
        egui::Rect::from_min_max(area.min, egui::pos2(area.max.x, lit.min.y)),
        egui::Rect::from_min_max(egui::pos2(area.min.x, lit.max.y), area.max),
        egui::Rect::from_min_max(
            egui::pos2(area.min.x, lit.min.y),
            egui::pos2(lit.min.x, lit.max.y),
        ),
        egui::Rect::from_min_max(
            egui::pos2(lit.max.x, lit.min.y),
            egui::pos2(area.max.x, lit.max.y),
        ),
    ]
    .into_iter()
    .filter(|rect| rect.is_positive())
    .collect()
}

pub fn open_lately(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(OPEN_KEY)))
        .is_some_and(|shown| pass.saturating_sub(shown) <= 1)
}

pub fn answered_lately(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(ANSWERED_KEY)))
        .is_some_and(|at| pass.saturating_sub(at) <= 1)
}

#[cfg(test)]
mod tests {
    use super::{
        Answer, Ask, CODE_ASK_WIDTH, CodeAsk, PROGRESS_LOST, ask, cancel_first, confirm,
        confirm_losing_progress, confirm_with_code, destructive,
    };
    use crate::app::ui_state::ui_theme::UiTheme;

    #[test]
    fn the_dim_leaves_out_only_the_lit_rect() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 800.0));
        let lit = egui::Rect::from_min_max(egui::pos2(10.0, 100.0), egui::pos2(390.0, 180.0));
        let rects = super::dim_rects(area, Some(lit));
        let dimmed = |point: egui::Pos2| rects.iter().any(|rect| rect.contains(point));
        assert!(!dimmed(lit.center()), "{rects:?}");
        for point in [
            egui::pos2(5.0, 140.0),
            egui::pos2(395.0, 140.0),
            egui::pos2(200.0, 50.0),
            egui::pos2(200.0, 500.0),
        ] {
            assert!(dimmed(point), "{point:?} in {rects:?}");
        }
        let covered: f32 = rects.iter().map(|rect| rect.area()).sum();
        assert!(
            (covered - (area.area() - lit.area())).abs() < 0.5,
            "{rects:?}"
        );
        assert_eq!(super::dim_rects(area, None), vec![area]);
        let outside = egui::Rect::from_min_size(egui::pos2(500.0, 0.0), egui::vec2(50.0, 50.0));
        assert_eq!(super::dim_rects(area, Some(outside)), vec![area]);
    }

    #[test]
    fn an_open_confirm_dims_the_screen_but_not_the_lit_row() {
        for (width, height, phone) in [(1280.0, 800.0, false), (412.0, 915.0, true)] {
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
            let row = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(width, 180.0));
            let mut output = None;
            for frame in 0..20 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(f64::from(frame) * 0.1),
                    ..Default::default()
                };
                output = Some(ctx.run(input, |ctx| {
                    crate::app::mobile::sheet::set_area(ctx, phone.then_some(screen));
                    egui::CentralPanel::default().show(ctx, |ui| super::keep_lit(ui, row));
                    let _ = confirm(ctx, "lit_row_test", "Delete Slot 1?", "Delete");
                }));
            }
            let quads: Vec<egui::Rect> = output
                .unwrap()
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Mesh(mesh) => Some(mesh),
                    _ => None,
                })
                .flat_map(|mesh| {
                    mesh.vertices
                        .chunks(4)
                        .filter(|quad| quad.iter().all(|v| v.color.a() > 0))
                        .map(|quad| {
                            egui::Rect::from_points(&quad.iter().map(|v| v.pos).collect::<Vec<_>>())
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let dimmed = |point: egui::Pos2| quads.iter().any(|quad| quad.contains(point));
            assert!(!dimmed(row.center()), "{width}: {quads:?}");
            assert!(dimmed(egui::pos2(width / 2.0, 50.0)), "{width}: {quads:?}");
            assert!(dimmed(egui::pos2(20.0, 300.0)), "{width}: {quads:?}");
        }
    }

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
                        list: &[],
                        notes: &notes,
                        action: "Delete everything",
                        action_enabled: false,
                        extra: Some(("Export first", false)),
                        load: Some((
                            "Exporting your data",
                            crate::progress::Progress::bytes(1024 * 1024, 4 * 1024 * 1024),
                        )),
                        code: None,
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

    #[test]
    fn a_desktop_confirm_frames_a_wrapped_question_with_even_sides() {
        let question = "Delete Mika's autosave from Nov 17, 12:27:25?";
        let ctx = egui::Context::default();
        let mut output = None;
        for frame in 0..20 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                let _ = super::confirm(ctx, "even_sides_test", question, "Delete");
            }));
        }
        let texts: Vec<egui::epaint::TextShape> = output
            .expect("the confirm ran")
            .shapes
            .into_iter()
            .flat_map(|clipped| flat(clipped.shape))
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        let drawn: Vec<(String, egui::Rect, egui::Rect)> = texts
            .iter()
            .map(|text| {
                (
                    text.galley.text().to_owned(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                    text.visual_bounding_rect(),
                )
            })
            .collect();
        let text = texts
            .iter()
            .find(|text| text.galley.text() == question)
            .map(|text| text.galley.rect.translate(text.pos.to_vec2()))
            .unwrap_or_else(|| panic!("no question in {drawn:?}"));
        let popup = ctx
            .memory(|m| m.area_rect(egui::Id::new("even_sides_test")))
            .expect("the modal is open");
        assert!(text.height() > 30.0, "{text:?} wraps");
        let left = text.left() - popup.left();
        let right = popup.right() - text.right();
        assert!(
            (left - right).abs() < 2.0,
            "left {left}, right {right}, popup {popup:?}, drawn {drawn:?}"
        );
    }

    #[test]
    fn a_desktop_code_ask_holds_its_hint_in_two_centred_lines_inside_the_modal() {
        let ctx = egui::Context::default();
        let hint = crate::app::second_screens::ask_code_hint("Pixel 8");
        let lines = ["48392".to_owned()];
        let notes = ["It can see and play this game.".to_owned()];
        let mut pointer = None;
        let mut frame = 0;
        let mut run = |pointer: Option<egui::Pos2>| {
            frame += 1;
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            if let Some(pos) = pointer {
                input.events.push(egui::Event::PointerMoved(pos));
            }
            ctx.run(input, |ctx| {
                let _ = ask(
                    ctx,
                    "desktop_code_hint_test",
                    Ask {
                        message: "Pixel 8 wants to be a second screen",
                        list: &[],
                        notes: &notes,
                        action: "Allow",
                        action_enabled: true,
                        extra: None,
                        load: None,
                        code: Some(crate::app::widgets::security_code::SecurityCode {
                            title: crate::app::widgets::security_code::TITLE,
                            hint: &hint,
                            lines: &lines,
                            compare: Some("Allow if Pixel 8 shows the same digits"),
                        }),
                    },
                );
            })
        };
        let texts = |output: &egui::FullOutput| -> Vec<(String, egui::Rect, usize, Vec<f32>)> {
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.visual_bounding_rect(),
                        text.galley.rows.len(),
                        text.galley
                            .rows
                            .iter()
                            .map(|row| text.pos.x + row.rect().center().x)
                            .collect(),
                    )),
                    _ => None,
                })
                .collect()
        };
        for _ in 0..6 {
            let output = run(None);
            pointer = texts(&output)
                .into_iter()
                .find(|(text, ..)| text == crate::icons::INFO)
                .map(|(_, rect, ..)| rect.center());
        }
        let mark = pointer.expect("the info mark is drawn");
        let mut output = run(Some(mark));
        for _ in 0..6 {
            output = run(Some(mark));
        }
        let modal = ctx
            .memory(|m| m.area_rect(egui::Id::new("desktop_code_hint_test")))
            .expect("the modal is open");
        assert!(
            modal.width() >= CODE_ASK_WIDTH,
            "the modal {modal:?} is desktop wide"
        );
        let drawn = texts(&output);
        let (_, text, rows, centres) = drawn
            .iter()
            .find(|(text, ..)| *text == hint)
            .expect("the hint is open");
        assert_eq!(*rows, 2, "the hint {text:?} takes two lines in {modal:?}");
        for centre in centres {
            assert!(
                (centre - text.center().x).abs() < 2.5,
                "each line is centred: {centres:?} around {}",
                text.center().x
            );
        }
        let layer = ctx
            .layer_id_at(text.center())
            .expect("the hint has a layer");
        let card = ctx
            .memory(|m| m.area_rect(layer.id))
            .expect("the hint card has a rect");
        assert!(
            modal.shrink(1.0).contains_rect(card),
            "the hint {card:?} sits inside the modal {modal:?}"
        );
        let (_, title, ..) = drawn
            .iter()
            .find(|(text, ..)| text == crate::app::widgets::security_code::TITLE)
            .expect("the title is drawn");
        assert!(
            card.bottom() < title.top(),
            "the hint {card:?} ends above {title:?}"
        );
        let disc = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Circle(circle) if circle.fill != egui::Color32::TRANSPARENT => {
                    Some(egui::Rect::from_center_size(
                        circle.center,
                        egui::Vec2::splat(circle.radius * 2.0),
                    ))
                }
                _ => None,
            })
            .expect("the hover disc is drawn");
        assert!(
            disc.left() >= title.right() + 2.0,
            "the disc {disc:?} keeps clear of the title {title:?}"
        );
    }

    fn code_ask_texts(width: f32) -> (egui::Context, Vec<egui::epaint::TextShape>) {
        let ctx = egui::Context::default();
        let phone = width < 600.0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0));
        if phone {
            ctx.style_mut(|style| style.spacing.interact_size.y = 48.0);
        }
        let hint = crate::app::second_screens::ask_code_hint("Pixel 8");
        let lines = ["48392".to_owned()];
        let notes = ["It can see and play this game.".to_owned()];
        let mut output = None;
        for frame in 0..20 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                if phone {
                    crate::app::mobile::sheet::set_area(ctx, Some(screen));
                }
                let _ = ask(
                    ctx,
                    "code_ask_text_test",
                    Ask {
                        message: "Pixel 8 wants to be a second screen",
                        list: &[],
                        notes: &notes,
                        action: "Allow",
                        action_enabled: true,
                        extra: None,
                        load: None,
                        code: Some(crate::app::widgets::security_code::SecurityCode {
                            title: crate::app::widgets::security_code::TITLE,
                            hint: &hint,
                            lines: &lines,
                            compare: Some("Allow if Pixel 8 shows the same digits"),
                        }),
                    },
                );
            }));
        }
        let texts = output
            .expect("the ask ran")
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        (ctx, texts)
    }

    #[test]
    fn a_code_ask_ranks_title_over_note_over_compare_line_and_centres_its_label_and_mark() {
        for width in [412.0, 1280.0] {
            let (ctx, texts) = code_ask_texts(width);
            let text = |wanted: &str| {
                texts
                    .iter()
                    .find(|text| text.galley.text() == wanted)
                    .unwrap_or_else(|| panic!("{wanted} is drawn at {width}"))
            };
            let size = |wanted: &str| text(wanted).galley.job.sections[0].format.font_id.size;
            let style = ctx.style();
            let title = size("Pixel 8 wants to be a second screen");
            let note = size("It can see and play this game.");
            let compare = size("Allow if Pixel 8 shows the same digits");
            assert!(title > note, "title {title} over note {note} at {width}");
            assert!(
                note >= compare,
                "note {note} over compare {compare} at {width}"
            );
            assert_eq!(
                text("It can see and play this game.").fallback_color,
                style.visuals.text_color(),
                "the note is body colour at {width}"
            );
            if width > 600.0 {
                assert_eq!(note, egui::TextStyle::Body.resolve(&style).size);
                assert_eq!(title, egui::TextStyle::Heading.resolve(&style).size);
            }
            let digits = text("48392").visual_bounding_rect();
            let label = text(crate::app::widgets::security_code::TITLE).visual_bounding_rect();
            let mark = text(crate::icons::INFO).visual_bounding_rect();
            let unit = (label.left() + mark.right()) / 2.0;
            assert!(
                (unit - digits.center().x).abs() < 1.0,
                "at {width} the label and mark {label:?} {mark:?} centre on the digits {digits:?}"
            );
        }
    }

    #[test]
    fn a_code_ask_keeps_its_title_closer_to_its_note_than_the_note_to_the_code() {
        for width in [412.0, 1280.0] {
            let (_, texts) = code_ask_texts(width);
            let ink = |wanted: &str| {
                texts
                    .iter()
                    .find(|text| text.galley.text() == wanted)
                    .unwrap_or_else(|| panic!("{wanted} is drawn at {width}"))
                    .visual_bounding_rect()
            };
            let title = ink("Pixel 8 wants to be a second screen");
            let note = ink("It can see and play this game.");
            let label = ink(crate::app::widgets::security_code::TITLE);
            let digits = ink("48392");
            let in_title = note.top() - title.bottom();
            let before_code = label.top() - note.bottom();
            let in_code = digits.top() - label.bottom();
            assert!(
                before_code > 1.4 * in_title,
                "at {width} title to note {in_title}, note to code {before_code}"
            );
            assert!(
                before_code > in_code,
                "at {width} note to code {before_code}, label to digits {in_code}"
            );
        }
    }

    #[test]
    fn a_title_splits_after_its_question_or_first_sentence() {
        assert_eq!(
            super::title_and_rest("Reset the game?"),
            ("Reset the game?", None)
        );
        assert_eq!(
            super::title_and_rest("Delete 'Dr. Mario.sav'? It can't be undone."),
            ("Delete 'Dr. Mario.sav'?", Some("It can't be undone."))
        );
        assert_eq!(
            super::title_and_rest("Your relay's GTS holds 3 of your Pokémon. Switch anyway?"),
            (
                "Your relay's GTS holds 3 of your Pokémon.",
                Some("Switch anyway?")
            )
        );
        assert_eq!(
            super::title_and_rest("Recording needs a codec of about 600 KB."),
            ("Recording needs a codec of about 600 KB.", None)
        );
    }

    #[test]
    fn a_long_desktop_confirm_heads_with_its_question_and_wraps_the_rest_inside_the_modal() {
        let ctx = egui::Context::default();
        let question = "Forget relay.example?";
        let rest = "Citrine deletes the saved password and the relay IDs stored here for this server. Without a relay ID saved somewhere else, you can't sign in to that account again.";
        let text = format!("{question} {rest}");
        let mut output = None;
        for frame in 0..10 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                let _ = super::confirm(ctx, "long_title_test", &text, "Forget");
            }));
        }
        let texts: Vec<egui::epaint::TextShape> = output
            .expect("the confirm ran")
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        let shape = |wanted: &str| {
            texts
                .iter()
                .find(|text| text.galley.text() == wanted)
                .unwrap_or_else(|| panic!("{wanted} is drawn"))
        };
        let style = ctx.style();
        let size = |wanted: &str| shape(wanted).galley.job.sections[0].format.font_id.size;
        assert_eq!(
            size(question),
            egui::TextStyle::Heading.resolve(&style).size
        );
        assert_eq!(size(rest), egui::TextStyle::Body.resolve(&style).size);
        assert!(shape(rest).galley.rows.len() > 1, "the rest wraps");
        let buttons = shape("Forget")
            .visual_bounding_rect()
            .union(shape("Cancel").visual_bounding_rect());
        for line in [question, rest] {
            let ink = shape(line).visual_bounding_rect();
            assert!(ink.width() <= 300.0, "{line} is {} wide", ink.width());
            assert!(
                (ink.center().x - buttons.center().x).abs() < 3.0,
                "{line} centres on the buttons"
            );
        }
    }

    fn texts_at(width: f32, show: impl Fn(&egui::Context)) -> Vec<egui::epaint::TextShape> {
        shapes_at(width, show)
            .into_iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    fn shapes_at(width: f32, show: impl Fn(&egui::Context)) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0));
        let phone = width < 600.0;
        if phone {
            ctx.style_mut(|style| style.spacing.interact_size.y = 48.0);
        }
        let mut output = None;
        for frame in 0..20 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                if phone {
                    crate::app::mobile::sheet::set_area(ctx, Some(screen));
                }
                show(ctx);
            }));
        }
        output
            .expect("the confirm ran")
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    #[test]
    fn a_wrapped_confirm_line_never_leaves_a_short_last_row() {
        let notes = [
            "This deletes 2 saves and 14 screenshots.".to_owned(),
            "Your online account and cloud copies stay.".to_owned(),
        ];
        let forget = "Forget relay.example.org? Citrine deletes the saved password and the relay IDs stored here for this server. Without a relay ID saved somewhere else, you can't sign in to that account again.";
        for width in [412.0, 1280.0] {
            let everything = texts_at(width, |ctx| {
                let _ = super::ask(
                    ctx,
                    "balanced_everything_test",
                    Ask {
                        message: "Delete everything on this device?",
                        list: &[],
                        notes: &notes,
                        action: "Delete everything",
                        action_enabled: true,
                        extra: Some(("Export first", true)),
                        load: None,
                        code: None,
                    },
                );
            });
            let forgetting = texts_at(width, |ctx| {
                let _ = super::confirm(ctx, "balanced_forget_test", forget, "Forget");
            });
            for text in everything.iter().chain(&forgetting) {
                let rows = &text.galley.rows;
                let widest = rows
                    .iter()
                    .map(|row| row.rect().width())
                    .fold(0.0, f32::max);
                for row in rows {
                    assert!(
                        row.rect().width() >= 0.5 * widest,
                        "at {width} {:?} has a short row {:?}",
                        text.galley.text(),
                        row.text()
                    );
                }
            }
        }
    }

    #[test]
    fn a_desktop_confirm_sets_its_option_apart_from_its_note() {
        let texts = texts_at(1280.0, |ctx| {
            let mut stop = false;
            let _ = super::confirm_with_option(
                ctx,
                "option_gap_test",
                "Reset the game? You'll unlink from Mia.",
                "Reset",
                ("Don't ask again until Citrine closes", &mut stop),
            );
        });
        let ink = |wanted: &str| {
            texts
                .iter()
                .find(|text| text.galley.text() == wanted)
                .unwrap_or_else(|| panic!("{wanted} is drawn"))
                .visual_bounding_rect()
        };
        let title = ink("Reset the game?");
        let note = ink("You'll unlink from Mia.");
        let option = ink("Don't ask again until Citrine closes");
        let in_message = note.top() - title.bottom();
        let before_option = option.top() - note.bottom();
        assert!(
            before_option > 1.4 * in_message,
            "title to note {in_message}, note to option {before_option}"
        );
    }

    fn delete_everything(ctx: &egui::Context) -> Option<Answer> {
        let notes = [
            "This deletes 2 saves and 14 screenshots.".to_owned(),
            "Your online account and cloud copies stay.".to_owned(),
        ];
        super::ask(
            ctx,
            "everything_test",
            Ask {
                message: "Delete everything on this device?",
                list: &[],
                notes: &notes,
                action: "Delete everything",
                action_enabled: true,
                extra: Some(("Export first", true)),
                load: None,
                code: None,
            },
        )
    }

    fn delete_save(ctx: &egui::Context) -> Option<bool> {
        super::confirm(
            ctx,
            "delete_save_test",
            "Delete 'Pokémon Crystal'? It can't be undone.",
            "Delete",
        )
    }

    fn reset(ctx: &egui::Context) -> Option<bool> {
        let mut stop = false;
        super::confirm_with_option(
            ctx,
            "reset_test",
            "Reset the game? You'll unlink from Mia.",
            "Reset",
            ("Don't ask again until Citrine closes", &mut stop),
        )
    }

    type Show = (&'static str, fn(&egui::Context));

    struct Laid {
        texts: Vec<egui::epaint::TextShape>,
        fills: Vec<egui::Rect>,
    }

    impl Laid {
        fn at(width: f32, show: impl Fn(&egui::Context)) -> Self {
            let shapes = shapes_at(width, show);
            let fills = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Rect(rect) if rect.fill != egui::Color32::TRANSPARENT => {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .collect();
            let texts = shapes
                .into_iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Text(text) => Some(text),
                    _ => None,
                })
                .collect();
            Self { texts, fills }
        }

        fn text(&self, wanted: &str) -> &egui::epaint::TextShape {
            self.texts
                .iter()
                .find(|text| text.galley.text() == wanted)
                .unwrap_or_else(|| panic!("{wanted} is drawn"))
        }

        fn ink(&self, wanted: &str) -> egui::Rect {
            self.text(wanted).visual_bounding_rect()
        }

        fn line(&self, wanted: &str) -> egui::Rect {
            let text = self.text(wanted);
            text.galley.rect.translate(text.pos.to_vec2())
        }

        fn size(&self, wanted: &str) -> f32 {
            self.text(wanted).galley.job.sections[0].format.font_id.size
        }

        fn button(&self, label: &str) -> egui::Rect {
            let centre = self.ink(label).center();
            self.fills
                .iter()
                .filter(|rect| rect.contains(centre))
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .copied()
                .unwrap_or_else(|| panic!("{label} has a button"))
        }
    }

    #[test]
    fn a_phone_confirm_heads_a_step_above_its_note_and_its_buttons() {
        let style = egui::Style::default();
        let desktop = egui::TextStyle::Heading.resolve(&style).size
            / egui::TextStyle::Body.resolve(&style).size;
        let laid = Laid::at(412.0, |ctx| {
            let _ = delete_everything(ctx);
        });
        let title = laid.size("Delete everything on this device?");
        let note = laid.size("This deletes 2 saves and 14 screenshots.");
        let label = laid.size("Delete everything");
        assert!(
            title / note >= desktop - 0.05,
            "title {title} over note {note} by the desktop step {desktop}"
        );
        assert!(title > label, "title {title} over button labels {label}");
    }

    #[test]
    fn a_phone_confirm_keeps_its_heading_on_its_note_and_its_notes_one_paragraph() {
        let laid = Laid::at(412.0, |ctx| {
            let _ = delete_everything(ctx);
        });
        let title = laid.ink("Delete everything on this device?");
        let first = laid.ink("This deletes 2 saves and 14 screenshots.");
        let second = laid.ink("Your online account and cloud copies stay.");
        let button = laid.button("Export first");
        let heading = first.top() - title.bottom();
        let first_line = laid.line("This deletes 2 saves and 14 screenshots.");
        let second_line = laid.line("Your online account and cloud copies stay.");
        let paragraph = second_line.top() - first_line.bottom();
        let before_buttons = button.top() - second.bottom();
        assert!(
            paragraph < 1.0,
            "the notes {paragraph} apart read as one paragraph"
        );
        assert!(
            before_buttons > 2.0 * heading,
            "title to note {heading}, note to buttons {before_buttons}"
        );

        let laid = Laid::at(412.0, |ctx| {
            let _ = delete_save(ctx);
        });
        let title = laid.ink("Delete 'Pokémon Crystal'?");
        let note = laid.ink("It can't be undone.");
        let button = laid.button("Delete");
        let heading = note.top() - title.bottom();
        let before_buttons = button.top() - note.bottom();
        assert!(
            before_buttons > 2.0 * heading,
            "title to note {heading}, note to button {before_buttons}"
        );
    }

    #[test]
    fn delete_everything_keeps_one_button_order_on_every_width() {
        let order = ["Export first", "Delete everything", "Cancel"];
        for width in [412.0, 1280.0] {
            let laid = Laid::at(width, |ctx| {
                let _ = delete_everything(ctx);
            });
            let along = |label: &str| {
                let at = laid.ink(label).center();
                if width < 600.0 { at.y } else { at.x }
            };
            for pair in order.windows(2) {
                assert!(
                    along(pair[0]) < along(pair[1]),
                    "at {width} {} comes before {}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn every_desktop_dialog_button_pads_its_label_alike() {
        let shows: [(Show, &[&str]); 3] = [
            (
                ("delete everything", |ctx| {
                    let _ = delete_everything(ctx);
                }),
                &["Export first", "Delete everything", "Cancel"],
            ),
            (
                ("delete save", |ctx| {
                    let _ = delete_save(ctx);
                }),
                &["Delete", "Cancel"],
            ),
            (
                ("reset", |ctx| {
                    let _ = reset(ctx);
                }),
                &["Reset", "Cancel"],
            ),
        ];
        for ((name, show), labels) in shows {
            let laid = Laid::at(1280.0, show);
            for label in labels {
                let side = (laid.button(label).width() - laid.line(label).width()) / 2.0;
                assert!(
                    (side - super::SIDE_PADDING).abs() < 0.5,
                    "{name}: {label} sits {side} from its edges"
                );
            }
        }
    }

    #[test]
    fn a_desktop_option_sits_with_the_buttons() {
        let laid = Laid::at(1280.0, |ctx| {
            let _ = reset(ctx);
        });
        let note = laid.line("You'll unlink from Mia.");
        let option = laid.line("Don't ask again until Citrine closes");
        let button = laid.button("Reset");
        let above = option.top() - note.bottom();
        let below = button.top() - option.bottom();
        assert!(
            (above - super::OPTION_ABOVE).abs() < 2.0,
            "note to option {above}"
        );
        assert!(
            (below - super::OPTION_BELOW).abs() < 2.0,
            "option to buttons {below}"
        );
    }

    #[test]
    fn a_focused_desktop_option_rings_like_a_button() {
        let option = "Don't ask again until Citrine closes";
        for theme in [UiTheme::Light, UiTheme::Dark] {
            let (ctx, shapes) = keyed(1280.0, theme, 2, |ctx| {
                let _ = reset(ctx);
            });
            let ink = |wanted: &str| {
                shapes
                    .iter()
                    .find_map(|shape| match shape {
                        egui::Shape::Text(text) if text.galley.text() == wanted => {
                            Some(text.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("{wanted} is drawn"))
            };
            let text = ink(option);
            let note = ink("You'll unlink from Mia.");
            let focused = ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
                .expect("something has the focus");
            assert!(
                focused.rect.contains(text.center()),
                "{theme:?}: the option has the focus"
            );
            let colour = crate::app::ui_state::ui_theme::focus_ring(&ctx.style().visuals);
            let ring = shapes
                .iter()
                .find_map(|shape| match shape {
                    egui::Shape::Rect(rect)
                        if rect.stroke.color == colour
                            && rect.stroke.width == super::RING_WIDTH =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .expect("the option is ringed");
            let button = fill_under(&shapes, "Reset");
            assert!(
                (ring.height() - (button.height() + 2.0 * super::RING_GAP)).abs() < 0.5,
                "{theme:?}: option ring {} tall, a button ring {}",
                ring.height(),
                button.height() + 2.0 * super::RING_GAP
            );
            let inset = super::SIDE_PADDING + super::RING_GAP - 0.5;
            assert!(
                focused.rect.left() - ring.left() >= inset && ring.right() - text.right() >= inset,
                "{theme:?}: ring {ring:?} around {:?} and {text:?}",
                focused.rect
            );
            let under = button.top() - ring.bottom() - super::RING_WIDTH;
            let over = ring.top() - super::RING_WIDTH - note.bottom();
            assert!(
                under >= 5.5 && over >= 5.5,
                "{theme:?}: ring clears Reset by {under} and the note by {over}"
            );
        }
    }

    fn tabbed(theme: UiTheme, show: impl Fn(&egui::Context)) -> (egui::Context, Vec<egui::Shape>) {
        keyed(1280.0, theme, 0, show)
    }

    fn keyed(
        width: f32,
        theme: UiTheme,
        back: u32,
        show: impl Fn(&egui::Context),
    ) -> (egui::Context, Vec<egui::Shape>) {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0));
        let phone = width < 600.0;
        if phone {
            ctx.style_mut(|style| style.spacing.interact_size.y = 48.0);
        }
        let tab = |pressed, modifiers| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let mut output = None;
        for frame in 0..14 + 2 * back {
            let events = match frame {
                8 | 9 => vec![tab(frame == 8, egui::Modifiers::NONE)],
                10.. if (frame - 10) / 2 < back => {
                    vec![tab(frame.is_multiple_of(2), egui::Modifiers::SHIFT)]
                }
                _ => vec![],
            };
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(f64::from(frame) * 0.1),
                events,
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                if phone {
                    crate::app::mobile::sheet::set_area(ctx, Some(screen));
                }
                show(ctx);
            }));
        }
        let shapes = output
            .expect("the confirm ran")
            .shapes
            .into_iter()
            .flat_map(|clipped| flat(clipped.shape))
            .collect();
        (ctx, shapes)
    }

    fn flat(shape: egui::Shape) -> Vec<egui::Shape> {
        match shape {
            egui::Shape::Vec(shapes) => shapes.into_iter().flat_map(flat).collect(),
            shape => vec![shape],
        }
    }

    #[test]
    fn a_focused_desktop_button_keeps_its_fill_and_rings_outside_it() {
        for theme in [UiTheme::Light, UiTheme::Dark] {
            let (ctx, shapes) = tabbed(theme, |ctx| {
                let _ = delete_everything(ctx);
            });
            let visuals = ctx.style().visuals.clone();
            let rects: Vec<&egui::epaint::RectShape> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Rect(rect) => Some(rect),
                    _ => None,
                })
                .collect();
            let label = shapes
                .iter()
                .find_map(|shape| match shape {
                    egui::Shape::Text(text) if text.galley.text() == "Cancel" => {
                        Some(text.visual_bounding_rect().center())
                    }
                    _ => None,
                })
                .expect("Cancel is drawn");
            let cancel = rects
                .iter()
                .filter(|rect| rect.fill != egui::Color32::TRANSPARENT && rect.rect.contains(label))
                .min_by(|a, b| a.rect.area().total_cmp(&b.rect.area()))
                .expect("Cancel has a button");
            assert_eq!(
                cancel.fill, visuals.widgets.inactive.weak_bg_fill,
                "{theme:?}: focused Cancel keeps its fill"
            );
            let ring = crate::app::ui_state::ui_theme::focus_ring(&visuals);
            let ringed = rects.iter().any(|rect| {
                rect.stroke.color == ring
                    && rect.stroke.width == super::RING_WIDTH
                    && rect.rect == cancel.rect.expand(super::RING_GAP)
            });
            assert!(ringed, "{theme:?}: a ring sits outside the focused Cancel");
        }
    }

    #[test]
    fn the_first_tab_in_a_danger_confirm_lands_on_cancel() {
        let shows: [Show; 3] = [
            ("delete everything", |ctx| {
                let _ = delete_everything(ctx);
            }),
            ("delete save", |ctx| {
                let _ = delete_save(ctx);
            }),
            ("reset", |ctx| {
                let _ = reset(ctx);
            }),
        ];
        for width in [412.0, 1280.0] {
            for (name, show) in shows {
                let ctx = egui::Context::default();
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0));
                let tab = |pressed| egui::Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                for frame in 0..12 {
                    let events = match frame {
                        8 => vec![tab(true)],
                        9 => vec![tab(false)],
                        _ => vec![],
                    };
                    let input = egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(f64::from(frame) * 0.1),
                        events,
                        ..Default::default()
                    };
                    let _ = ctx.run(input, |ctx| {
                        if width < 600.0 {
                            crate::app::mobile::sheet::set_area(ctx, Some(screen));
                        }
                        show(ctx);
                    });
                }
                let cancel = ctx
                    .data(|d| {
                        d.get_temp::<(u64, egui::Id)>(egui::Id::new("big_screen_first_choice"))
                    })
                    .map(|(_, id)| id);
                assert!(cancel.is_some(), "{name} at {width} marks its Cancel");
                assert_eq!(
                    ctx.memory(|m| m.focused()),
                    cancel,
                    "{name} at {width}: the first Tab lands on Cancel"
                );
            }
        }
    }

    fn fill_under(shapes: &[egui::Shape], label: &str) -> egui::Rect {
        let centre = shapes
            .iter()
            .find_map(|shape| match shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label} is drawn"));
        shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Rect(rect)
                    if rect.fill != egui::Color32::TRANSPARENT && rect.rect.contains(centre) =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap_or_else(|| panic!("{label} has a button"))
    }

    #[test]
    fn a_desktop_confirm_pads_under_its_buttons_as_at_its_sides() {
        let shows: [(Show, &[&str]); 3] = [
            (
                ("delete everything", |ctx| {
                    let _ = delete_everything(ctx);
                }),
                &["Export first", "Delete everything", "Cancel"],
            ),
            (
                ("delete save", |ctx| {
                    let _ = delete_save(ctx);
                }),
                &["Delete", "Cancel"],
            ),
            (
                ("reset", |ctx| {
                    let _ = reset(ctx);
                }),
                &["Reset", "Cancel"],
            ),
        ];
        let padding = f32::from(super::CARD_PADDING);
        for ((name, show), labels) in shows {
            let (ctx, shapes) = keyed(1280.0, UiTheme::Light, 0, show);
            let visuals = ctx.style().visuals.clone();
            let stroke = visuals.window_stroke.width;
            let buttons: Vec<egui::Rect> = labels
                .iter()
                .map(|label| fill_under(&shapes, label))
                .collect();
            let row = buttons
                .iter()
                .fold(egui::Rect::NOTHING, |row, button| row.union(*button));
            let card = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == visuals.window_fill && rect.rect.contains_rect(row) =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("the confirm draws its card");
            let under = card.bottom() - stroke - row.bottom();
            let left = row.left() - card.left() - stroke;
            let right = card.right() - stroke - row.right();
            assert!(
                (under - padding).abs() < 0.5,
                "{name}: {under} under the buttons"
            );
            assert!(
                left >= padding - 0.5 && right >= padding - 0.5,
                "{name}: {left} and {right} at the sides, {under} under"
            );
            assert!(
                under - super::RING_GAP - super::RING_WIDTH >= super::RING_GAP * 2.0,
                "{name}: a ring under a button clears the card's edge"
            );
        }
    }

    #[test]
    fn a_focused_phone_button_keeps_its_size_and_clears_its_neighbours() {
        let cases: [(Show, u32, &str, &[&str]); 4] = [
            (
                ("delete everything", |ctx| {
                    let _ = delete_everything(ctx);
                }),
                2,
                "Export first",
                &["Delete everything"],
            ),
            (
                ("delete everything", |ctx| {
                    let _ = delete_everything(ctx);
                }),
                1,
                "Delete everything",
                &["Export first", "Cancel"],
            ),
            (
                ("delete save", |ctx| {
                    let _ = delete_save(ctx);
                }),
                0,
                "Cancel",
                &["Delete"],
            ),
            (
                ("reset", |ctx| {
                    let _ = reset(ctx);
                }),
                0,
                "Cancel",
                &["Reset"],
            ),
        ];
        let ring = crate::app::big_screen::ring::MARGIN;
        let gap = crate::app::big_screen::ring::GAP;
        for ((name, show), back, focused, neighbours) in cases {
            let (ctx, shapes) = keyed(412.0, UiTheme::Light, back, show);
            let button = fill_under(&shapes, focused);
            let on = ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
                .is_some_and(|response| response.rect.contains(button.center()));
            assert!(on, "{name}: {focused} has the focus");
            let still = Laid::at(412.0, show).button(focused);
            assert!(
                (button.height() - still.height()).abs() < 0.01
                    && (button.width() - still.width()).abs() < 0.01,
                "{name}: focused {focused} {button:?} keeps its size {still:?}"
            );
            for neighbour in neighbours {
                let other = fill_under(&shapes, neighbour);
                let apart = if other.center().y < button.center().y {
                    button.top() - other.bottom()
                } else {
                    other.top() - button.bottom()
                };
                assert!(
                    apart - ring >= 2.0 * gap,
                    "{name}: {focused}'s ring {ring} wide sits {} from {neighbour}",
                    apart - ring
                );
            }
        }
    }

    #[test]
    fn danger_asks_start_on_cancel_and_safe_ones_on_their_action() {
        for action in [
            "Delete",
            "Delete everything",
            "Clear",
            "Clear all",
            "Replace",
            "Reset",
            "Forget",
            "Remove",
        ] {
            assert!(cancel_first(action), "{action} should start on Cancel");
        }
        for action in ["Import", "Allow", "Load", "Restart", "Sign out", "Use this"] {
            assert!(!cancel_first(action), "{action} should start on itself");
        }
    }

    fn focus_on_cancel(phone: bool, losing: bool) -> (bool, bool) {
        let opened = open_ask(phone, losing, UiTheme::Light);
        (opened.on_cancel, opened.noted)
    }

    struct Opened {
        on_cancel: bool,
        noted: bool,
        ringed: bool,
        cancel_fill: egui::Color32,
    }

    fn open_ask(phone: bool, losing: bool, theme: UiTheme) -> Opened {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let ring = crate::app::ui_state::ui_theme::focus_ring(&ctx.style().visuals);
        let mut rings = Vec::new();
        let mut fills = Vec::new();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(412.0, 800.0));
        let mut cancel = None;
        let mut noted = false;
        for frame in 0..20 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                crate::app::mobile::sheet::set_area(ctx, phone.then_some(screen));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let play = ui.button("Play");
                    if frame == 0 {
                        play.request_focus();
                    }
                });
                if frame > 0 {
                    let _ = if losing {
                        confirm_losing_progress(ctx, "losing_test", "Load Slot 1?", "Load")
                    } else {
                        confirm(ctx, "losing_test", "Load Slot 1?", "Load")
                    };
                }
                if !phone {
                    crate::app::ui_state::ui_theme::paint_focus_ring(ctx);
                }
            });
            rings = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.stroke.color == ring => Some(rect.rect),
                    _ => None,
                })
                .collect();
            fills = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) if rect.fill != egui::Color32::TRANSPARENT => {
                        Some((rect.rect, rect.fill))
                    }
                    _ => None,
                })
                .collect();
            let texts: Vec<(String, egui::Pos2)> = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.visual_bounding_rect().center(),
                    )),
                    _ => None,
                })
                .collect();
            noted = texts.iter().any(|(text, _)| text == PROGRESS_LOST);
            cancel = texts
                .into_iter()
                .find_map(|(text, centre)| (text == "Cancel").then_some(centre));
        }
        let cancel = cancel.expect("no Cancel drawn");
        let on_cancel = ctx
            .memory(|m| m.focused())
            .and_then(|id| ctx.read_response(id))
            .is_some_and(|focused| focused.rect.contains(cancel));
        let ringed = rings.iter().any(|rect| rect.contains(cancel));
        let cancel_fill = fills
            .into_iter()
            .filter(|(rect, _)| rect.contains(cancel))
            .min_by(|(a, _), (b, _)| a.area().total_cmp(&b.area()))
            .map(|(_, fill)| fill)
            .expect("no fill under Cancel");
        Opened {
            on_cancel,
            noted,
            ringed,
            cancel_fill,
        }
    }

    #[test]
    fn a_focused_cancel_keeps_its_ring_and_its_resting_fill() {
        for phone in [false, true] {
            for theme in [UiTheme::Light, UiTheme::Dark] {
                let focused = open_ask(phone, true, theme);
                let resting = open_ask(phone, false, theme);
                assert!(
                    focused.on_cancel && focused.ringed,
                    "phone {phone}, {theme}"
                );
                assert!(
                    !resting.on_cancel && !resting.ringed,
                    "phone {phone}, {theme}"
                );
                assert_eq!(
                    focused.cancel_fill, resting.cancel_fill,
                    "phone {phone}, {theme}"
                );
            }
        }
    }

    #[test]
    fn asks_that_lose_unsaved_progress_say_so_and_start_on_cancel() {
        for phone in [false, true] {
            assert_eq!(focus_on_cancel(phone, true), (true, true), "phone {phone}");
            assert_eq!(
                focus_on_cancel(phone, false),
                (false, false),
                "phone {phone}"
            );
        }
    }

    #[test]
    fn the_compiled_code_ask_centres_its_buttons_in_the_dialog_as_drawn() {
        use crate::app::widgets::settings::storage::{StorageUi, ask_message, clear_ask};
        use crate::storage::kinds::Kind;
        let ctx = egui::Context::default();
        let input = |frame: u32| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            time: Some(f64::from(frame) * 0.1),
            ..Default::default()
        };
        let mut state = StorageUi::default();
        state.usage.insert(Kind::CompiledCode, 99 << 20);
        let kinds = [Kind::CompiledCode];
        let message = ask_message(&kinds);
        let clear = clear_ask(&state, &kinds);
        let mut output = None;
        for frame in 0..10 {
            output = Some(ctx.run(input(frame), |ctx| {
                let _ = super::ask(
                    ctx,
                    "compiled_code_ask_test",
                    super::Ask {
                        message: &message,
                        list: &clear.list,
                        notes: &clear.notes,
                        action: "Clear",
                        action_enabled: true,
                        extra: None,
                        load: None,
                        code: None,
                    },
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
        let left = rect(&message).left();
        let right = texts
            .iter()
            .map(|(_, r)| r.right())
            .fold(f32::MIN, f32::max);
        let row = (rect("Clear").center().x + rect("Cancel").center().x) / 2.0;
        let middle = (left + right) / 2.0;
        assert!(
            (row - middle).abs() < 3.0,
            "buttons at {row}, dialog content from {left} to {right}"
        );
    }

    #[test]
    fn a_tall_ask_scrolls_its_focused_cancel_into_view() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(412.0, 420.0));
        let list: Vec<String> = (1..=12)
            .map(|n| format!("{n} settings, back to default"))
            .collect();
        let notes = vec!["Your online account and its cloud saves stay.".to_owned()];
        let mut cancel = None;
        for frame in 0..20 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(f64::from(frame) * 0.1),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                crate::app::mobile::sheet::set_area(ctx, Some(screen));
                egui::CentralPanel::default().show(ctx, |ui| {
                    let play = ui.button("Play");
                    if frame == 0 {
                        play.request_focus();
                    }
                });
                if frame > 0 {
                    let _ = ask(
                        ctx,
                        "tall_ask_test",
                        Ask {
                            message: "Delete everything on this device?",
                            list: &list,
                            notes: &notes,
                            action: "Delete everything",
                            action_enabled: true,
                            extra: Some(("Export first", true)),
                            load: None,
                            code: None,
                        },
                    );
                }
            });
            cancel = output
                .shapes
                .into_iter()
                .find_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Cancel" => {
                        Some((text.visual_bounding_rect(), clipped.clip_rect))
                    }
                    _ => None,
                });
        }
        let (cancel, clip) = cancel.expect("no Cancel drawn");
        assert!(
            clip.contains_rect(cancel) && screen.contains_rect(cancel),
            "Cancel at {cancel:?}, shown in {clip:?}"
        );
    }

    #[test]
    fn a_long_list_takes_two_columns_where_they_fit_so_a_big_ask_needs_no_scroll() {
        let list: Vec<String> = [
            "8 saves",
            "32 save states",
            "36 screenshots",
            "2 recordings",
            "8 game names",
            "2 Pokéwalker saves",
            "1 game copy",
            "3 settings, back to default",
        ]
        .map(str::to_owned)
        .to_vec();
        let notes = vec![
            "Export leaves out game copies.".to_owned(),
            "Your online account and its cloud saves stay.".to_owned(),
        ];
        let shown = |size: egui::Vec2| {
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut texts = Vec::new();
            for frame in 0..20 {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(f64::from(frame) * 0.1),
                    ..Default::default()
                };
                let output = ctx.run(input, |ctx| {
                    crate::app::mobile::sheet::set_area(ctx, Some(screen));
                    egui::CentralPanel::default().show(ctx, |_| {});
                    let _ = ask(
                        ctx,
                        "two_columns_test",
                        Ask {
                            message: "Delete everything on this device?",
                            list: &list,
                            notes: &notes,
                            action: "Delete everything",
                            action_enabled: true,
                            extra: Some(("Export first", true)),
                            load: None,
                            code: None,
                        },
                    );
                });
                texts = output
                    .shapes
                    .into_iter()
                    .filter_map(|clipped| match clipped.shape {
                        egui::Shape::Text(text) => Some((
                            text.galley.text().to_owned(),
                            text.visual_bounding_rect(),
                            clipped.clip_rect,
                        )),
                        _ => None,
                    })
                    .collect::<Vec<(String, egui::Rect, egui::Rect)>>();
            }
            (screen, texts)
        };
        let columns = |texts: &[(String, egui::Rect, egui::Rect)]| {
            let mut lefts: Vec<i32> = texts
                .iter()
                .filter(|(text, _, _)| list.contains(text))
                .map(|(_, rect, _)| rect.left().round() as i32)
                .collect();
            lefts.sort_unstable();
            lefts.dedup_by(|a, b| (*a - *b).abs() <= 2);
            lefts.len()
        };
        let (screen, big) = shown(egui::vec2(1280.0, 720.0));
        assert_eq!(columns(&big), 2, "{big:?}");
        for whole in ["Delete everything on this device?", "Cancel"] {
            let (_, rect, clip) = big
                .iter()
                .find(|(text, _, _)| text == whole)
                .unwrap_or_else(|| panic!("no {whole} drawn"));
            assert!(
                clip.contains_rect(*rect) && screen.contains_rect(*rect),
                "{whole} at {rect:?}, shown in {clip:?}"
            );
        }
        let (_, phone) = shown(egui::vec2(412.0, 900.0));
        assert_eq!(columns(&phone), 1, "{phone:?}");
    }
}
