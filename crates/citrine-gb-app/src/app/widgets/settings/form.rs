use crate::app::widgets::info_mark::info_mark;
use egui::{
    Align, Id, Label, Layout, Rect, RichText, Shape, TextStyle, TextWrapMode, Ui, UiBuilder,
    WidgetText, vec2,
};
use std::hash::Hash;

const MAX_LABEL_SHARE: f32 = 0.4;

const MAX_TEXT_LABEL_SHARE: f32 = 0.65;

#[derive(Clone, Copy, Default)]
struct LabelColumn {
    pass: u64,
    widest: f32,
    previous: f32,
}

#[derive(Clone, Copy, Default)]
struct RowNeed {
    pass: u64,
    widest: f32,
}

fn row_key() -> Id {
    Id::new("settings_form_row")
}

pub fn need(ui: &Ui, width: f32) {
    let ctx = ui.ctx();
    let Some(row) = ctx.data(|d| d.get_temp::<Option<Id>>(row_key())).flatten() else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let mut need = d.get_temp::<RowNeed>(row).unwrap_or_default();
        if need.pass != pass {
            need = RowNeed { pass, widest: 0.0 };
        }
        need.widest = need.widest.max(width);
        d.insert_temp(row, need);
    });
}

fn settled_need(ui: &Ui, row: Id) -> f32 {
    ui.data(|d| d.get_temp::<RowNeed>(row))
        .map_or(0.0, |need| need.widest)
}

fn check_need(ui: &Ui, row: Id, used: f32) {
    let pass = ui.ctx().cumulative_pass_nr();
    let Some(need) = ui.data(|d| d.get_temp::<RowNeed>(row)) else {
        return;
    };
    if need.pass == pass && (need.widest - used).abs() > 0.5 {
        ui.ctx().request_discard("settings value need changed");
    }
}

pub struct Form<'a> {
    ui: &'a mut Ui,
    label_width: f32,
    label_room: f32,
    longest: f32,
    widest: f32,
    stacked_rows: usize,
    striped: bool,
    text_only: bool,
    body_rows: usize,
    in_page: bool,
    narrow: bool,
}

impl Form<'_> {
    pub fn show<R>(ui: &mut Ui, id_salt: impl Hash, add_rows: impl FnOnce(&mut Form) -> R) -> R {
        let page = super::page::column(ui.ctx());
        let id = page.unwrap_or_else(|| Id::new(("settings_form", id_salt)));
        let pass = ui.ctx().cumulative_pass_nr();
        let width = ui.available_width();

        let mut column = ui
            .data(|d| d.get_temp::<LabelColumn>(id))
            .unwrap_or_default();
        if column.pass != pass {
            column = LabelColumn {
                pass,
                widest: 0.0,
                previous: column.widest,
            };
        }

        let inner = ui
            .with_layout(Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                let mut form = Form {
                    ui,
                    label_width: column.previous.min(width * MAX_LABEL_SHARE),
                    label_room: column.previous.min(width * MAX_TEXT_LABEL_SHARE),
                    longest: column.previous,
                    widest: 0.0,
                    stacked_rows: 0,
                    striped: false,
                    text_only: false,
                    body_rows: 0,
                    in_page: page.is_some(),
                    narrow: width < super::page::STACK_BELOW,
                };
                let inner = add_rows(&mut form);
                column.widest = column.widest.max(form.widest);
                inner
            })
            .inner;

        if column.widest > column.previous {
            ui.ctx().request_discard("settings label column grew");
        }
        ui.data_mut(|d| d.insert_temp(id, column));
        inner
    }

    pub fn striped(&mut self) {
        self.striped = true;
    }

    pub fn text_only(&mut self, widest_value_word: f32) {
        self.text_only = true;
        let value_room =
            self.ui.available_width() - self.label_room - self.ui.spacing().item_spacing.x;
        self.label_width = if value_room < widest_value_word {
            0.0
        } else {
            self.label_room
        };
    }

    pub fn header(&mut self, label: &str, value: &str) {
        self.labelled(RichText::new(label).strong(), None, |ui| {
            ui.add(Label::new(RichText::new(value).strong()).wrap());
        });
    }

    pub fn text_row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.labelled_at(label, None, text_height(), add)
    }

    pub fn row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        let stripe = self.striped && self.body_rows.is_multiple_of(2);
        self.body_rows += 1;
        if !stripe {
            return self.labelled(label, None, add);
        }
        let background = self.ui.painter().add(Shape::Noop);
        let top = self.ui.cursor().top();
        let inner = self.labelled(label, None, add);
        let gap = self.ui.spacing().item_spacing.y / 2.0;
        let rect = Rect::from_x_y_ranges(
            self.ui.max_rect().x_range(),
            (top - gap)..=(self.ui.min_rect().bottom() + gap),
        );
        let fill = self.ui.visuals().faint_bg_color;
        self.ui.painter().set(
            background,
            Shape::rect_filled(
                rect,
                self.ui.visuals().widgets.noninteractive.corner_radius,
                fill,
            ),
        );
        inner
    }

    pub fn row_with_hint<R>(
        &mut self,
        label: &str,
        hint: &str,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.labelled(label, Some(hint), add)
    }

    pub fn toggle(&mut self, label: &str, on: &mut bool) -> bool {
        self.toggle_when(label, None, true, on)
    }

    pub fn toggle_with_hint(&mut self, label: &str, hint: &str, on: &mut bool) -> bool {
        self.toggle_when(label, Some(hint), true, on)
    }

    pub fn toggle_when(
        &mut self,
        label: &str,
        hint: Option<&str>,
        enabled: bool,
        on: &mut bool,
    ) -> bool {
        self.toggle_row(label, hint, enabled, on, false)
    }

    #[cfg(ds_wifi)]
    pub fn toggle_found(&mut self, label: &str, hint: &str, on: &mut bool, find: bool) -> bool {
        self.toggle_row(label, Some(hint), true, on, find)
    }

    fn toggle_row(
        &mut self,
        label: &str,
        hint: Option<&str>,
        enabled: bool,
        on: &mut bool,
        find: bool,
    ) -> bool {
        let check = |ui: &mut Ui, on: &mut bool| {
            let response = ui.add_enabled(enabled, crate::app::widgets::check::Check::new(on, ""));
            if find {
                response.scroll_to_me(Some(Align::Center));
                response.request_focus();
            }
            response.changed()
        };
        if !self.stacked() {
            return self.labelled(label, hint, |ui| check(ui, on));
        }
        self.measure(label, hint);
        self.stacked_gap();
        let top = self.ui.cursor().top();
        let height = self.ui.spacing().interact_size.y;
        let changed = self
            .ui
            .allocate_ui_with_layout(
                vec2(self.ui.available_width(), height),
                Layout::right_to_left(Align::Center),
                |ui| {
                    let changed = check(ui, on);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(Label::new(label).wrap());
                        if let Some(hint) = hint {
                            info_mark(ui, hint);
                        }
                    });
                    changed
                },
            )
            .inner;
        self.mark_row(top);
        changed
    }

    fn measure(&mut self, label: &str, hint: Option<&str>) {
        let measured = WidgetText::from(label)
            .into_galley(
                self.ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Body,
            )
            .size()
            .x;
        let mark = if hint.is_some() {
            crate::app::widgets::info_mark::width(self.ui)
        } else {
            0.0
        };
        self.widest = self.widest.max(measured + mark);
    }

    fn stacked_gap(&mut self) {
        if !self.stacked() {
            return;
        }
        if self.stacked_rows > 0 {
            let gap = self.ui.spacing().item_spacing.y;
            self.ui.add_space(gap);
        }
        self.stacked_rows += 1;
    }

    pub fn buttons<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.stacked_gap();
        let top = self.ui.cursor().top();
        let inner = self.under(|ui| ui.horizontal_wrapped(add).inner);
        self.mark_row(top);
        inner
    }

    pub fn custom<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.stacked_gap();
        let top = self.ui.cursor().top();
        let inner = self.ui.with_layout(Layout::top_down(Align::Min), add).inner;
        self.mark_row(top);
        inner
    }

    fn mark_row(&self, top: f32) {
        super::page::mark(
            super::page::Mark::Row,
            Rect::from_x_y_ranges(
                self.ui.max_rect().x_range(),
                top..=self.ui.min_rect().bottom(),
            ),
        );
    }

    pub fn ctx(&self) -> &egui::Context {
        self.ui.ctx()
    }

    fn stacked(&self) -> bool {
        if self.in_page {
            self.narrow
        } else {
            self.label_width < self.longest
        }
    }

    pub fn hint(&mut self, text: impl Into<RichText>) {
        let pull = self.ui.spacing().item_spacing.y / 2.0;
        self.ui.add_space(-pull);
        let top = self.ui.cursor().top();
        self.under(|ui| ui.add(Label::new(text.into().weak()).wrap()));
        super::page::mark(
            super::page::Mark::Note,
            Rect::from_x_y_ranges(
                self.ui.max_rect().x_range(),
                top..=self.ui.min_rect().bottom(),
            ),
        );
        self.ui.add_space(pull);
    }

    pub fn under<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let indent = if self.stacked() || self.label_width <= 0.0 {
            0.0
        } else {
            self.label_width + self.ui.spacing().item_spacing.x
        };
        let rect = self.ui.available_rect_before_wrap();
        self.ui
            .scope_builder(
                UiBuilder::new().max_rect(rect.with_min_x(rect.left() + indent)),
                add,
            )
            .inner
    }

    pub fn full<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = self.ui.available_rect_before_wrap();
        self.ui
            .scope_builder(UiBuilder::new().max_rect(rect), add)
            .inner
    }

    fn labelled<R>(
        &mut self,
        label: impl Into<WidgetText>,
        hint: Option<&str>,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let height = if self.text_only {
            text_height()
        } else {
            self.ui.spacing().interact_size.y
        };
        self.labelled_at(label, hint, height, add)
    }

    fn labelled_at<R>(
        &mut self,
        label: impl Into<WidgetText>,
        hint: Option<&str>,
        height: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let label: WidgetText = label.into();
        let measured = label
            .clone()
            .into_galley(
                self.ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Body,
            )
            .size()
            .x;
        let mark = if hint.is_some() {
            crate::app::widgets::info_mark::width(self.ui)
        } else {
            0.0
        };
        self.widest = self.widest.max(measured + mark);

        let row = self.ui.id().with(("value_need", label.text()));
        let need = settled_need(self.ui, row);
        let width = self.ui.available_width();
        let value_room = width - need - self.ui.spacing().item_spacing.x;
        let floor = label
            .text()
            .split_whitespace()
            .map(|word| {
                WidgetText::from(word)
                    .into_galley(
                        self.ui,
                        Some(TextWrapMode::Extend),
                        f32::INFINITY,
                        TextStyle::Body,
                    )
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
            + mark;
        let column = if need > 0.0 && !self.text_only {
            self.longest
        } else {
            self.label_width
        };
        let label_width = column.min(value_room.max(floor));
        let outer = self
            .ui
            .data(|d| d.get_temp::<Option<Id>>(row_key()))
            .flatten();
        self.ui.data_mut(|d| d.insert_temp(row_key(), Some(row)));
        let inner = self.row_body(label, hint, label_width, height, add);
        self.ui.data_mut(|d| d.insert_temp(row_key(), outer));
        check_need(self.ui, row, need);
        inner
    }

    fn row_body<R>(
        &mut self,
        label: WidgetText,
        hint: Option<&str>,
        label_width: f32,
        height: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let row_height = self.ui.spacing().interact_size.y.min(height);
        let top = self.ui.cursor().top();
        let in_page = self.in_page;
        let mark = if hint.is_some() {
            crate::app::widgets::info_mark::width(self.ui)
        } else {
            0.0
        };
        if self.stacked() {
            let inner = self.stacked_row(label, hint, height, row_height, add);
            self.mark_row(top);
            return inner;
        }

        let inner = self
            .ui
            .scope(|ui| {
                ui.spacing_mut().interact_size.y = row_height;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(label_width, height),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.set_width(label_width);
                            if in_page {
                                ui.scope(|ui| {
                                    ui.set_max_width((label_width - mark).max(0.0));
                                    ui.add(Label::new(label).wrap());
                                });
                            } else {
                                ui.add(Label::new(label).truncate());
                            }
                            if let Some(hint) = hint {
                                info_mark(ui, hint);
                            }
                        },
                    );

                    let width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        vec2(width, height),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.set_max_width(width);
                            add(ui)
                        },
                    )
                    .inner
                })
                .inner
            })
            .inner;
        self.mark_row(top);
        inner
    }

    fn stacked_row<R>(
        &mut self,
        label: WidgetText,
        hint: Option<&str>,
        height: f32,
        row_height: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let gap = self.ui.spacing().item_spacing.y;
        if self.stacked_rows > 0 {
            self.ui.add_space(gap);
        }
        self.stacked_rows += 1;
        match hint {
            Some(hint) => {
                self.ui.horizontal(|ui| {
                    ui.add(Label::new(label).wrap());
                    info_mark(ui, hint);
                });
            }
            None => {
                self.ui.add(Label::new(label).wrap());
            }
        }
        self.ui.add_space(-gap / 2.0);
        self.ui
            .scope(|ui| {
                ui.spacing_mut().interact_size.y = row_height;
                let width = ui.available_width();
                ui.allocate_ui_with_layout(
                    vec2(width, height),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_max_width(width);
                        add(ui)
                    },
                )
                .inner
            })
            .inner
    }
}

fn text_height() -> f32 {
    egui::style::Spacing::default().interact_size.y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_pass(ctx: &egui::Context, widths: &[f32]) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                Form::show(ui, "need_test", |form| {
                    form.row("Birthday", |ui| {
                        for width in widths {
                            need(ui, *width);
                        }
                    });
                });
            });
        })
    }

    #[test]
    fn a_row_with_two_needs_settles_after_one_pass() {
        let ctx = egui::Context::default();
        run_pass(&ctx, &[40.0, 90.0]);
        for _ in 0..3 {
            let output = run_pass(&ctx, &[40.0, 90.0]);
            assert!(
                output.platform_output.request_discard_reasons.is_empty(),
                "{:?}",
                output.platform_output.request_discard_reasons
            );
        }
    }

    #[test]
    fn a_row_asks_again_when_its_need_changes() {
        let ctx = egui::Context::default();
        run_pass(&ctx, &[40.0, 90.0]);
        run_pass(&ctx, &[40.0, 90.0]);
        let output = run_pass(&ctx, &[40.0, 60.0]);
        assert_eq!(output.platform_output.num_completed_passes, 2);
        let output = run_pass(&ctx, &[40.0, 60.0]);
        assert!(output.platform_output.request_discard_reasons.is_empty());
    }
}
