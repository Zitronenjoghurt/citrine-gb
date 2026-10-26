use egui::{
    Align, Id, Label, Layout, Rect, RichText, Shape, TextStyle, TextWrapMode, Ui, UiBuilder,
    WidgetText, vec2,
};
use std::hash::Hash;

const MAX_LABEL_SHARE: f32 = 0.5;

const MAX_TEXT_LABEL_SHARE: f32 = 0.65;

#[derive(Clone, Copy, Default)]
struct LabelColumn {
    pass: u64,
    widest: f32,
    previous: f32,
}

pub struct Form<'a> {
    ui: &'a mut Ui,
    label_width: f32,
    label_room: f32,
    widest: f32,
    striped: bool,
    text_only: bool,
    body_rows: usize,
}

impl Form<'_> {
    pub fn show<R>(ui: &mut Ui, id_salt: impl Hash, add_rows: impl FnOnce(&mut Form) -> R) -> R {
        let id = Id::new(("settings_form", id_salt));
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
                    widest: 0.0,
                    striped: false,
                    text_only: false,
                    body_rows: 0,
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

    pub fn text_only(&mut self) {
        self.text_only = true;
        self.label_width = self.label_room;
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

    pub fn ctx(&self) -> &egui::Context {
        self.ui.ctx()
    }

    pub fn hint(&mut self, text: impl Into<RichText>) {
        let pull = self.ui.spacing().item_spacing.y / 2.0;
        self.ui.add_space(-pull);
        let indent = self.label_width + self.ui.spacing().item_spacing.x;
        let rect = self.ui.available_rect_before_wrap();
        self.ui.scope_builder(
            UiBuilder::new().max_rect(rect.with_min_x(rect.left() + indent)),
            |ui| ui.add(Label::new(text.into().small()).wrap()),
        );
        self.ui.add_space(pull);
    }

    pub fn separator(&mut self) {
        self.ui.separator();
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
        self.widest = self.widest.max(measured);

        let label_width = self.label_width;

        let row_height = self.ui.spacing().interact_size.y.min(height);
        self.ui
            .scope(|ui| {
                ui.spacing_mut().interact_size.y = row_height;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(label_width, height),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            ui.set_width(label_width);
                            let response = ui.add(Label::new(label).truncate());
                            if let Some(hint) = hint {
                                response.on_hover_text(hint);
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
            .inner
    }
}

fn text_height() -> f32 {
    egui::style::Spacing::default().interact_size.y
}
