use crate::icons;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect, Response, RichText,
    Sense, Stroke, Ui, Vec2, vec2,
};

pub const SIDE: f32 = 14.0;
pub const ROW_HEIGHT: f32 = 50.0;
pub const RADIUS: u8 = 14;
pub const VALUE_EDGE: f32 = 16.0;
const ICON_COLUMN: f32 = 50.0;
const ICON_TILE: f32 = 36.0;

pub fn page_fill(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.panel_fill
    } else {
        visuals
            .panel_fill
            .lerp_to_gamma(Color32::from_gray(232), 0.6)
    }
}

pub fn card_fill(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals.panel_fill.lerp_to_gamma(Color32::WHITE, 0.07)
    } else {
        visuals.extreme_bg_color.lerp_to_gamma(Color32::WHITE, 0.8)
    }
}

pub fn icon_tile_fill(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_white_alpha(18)
    } else {
        Color32::from_black_alpha(14)
    }
}

pub fn place<R>(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink(false)
        .show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin {
                    left: SIDE as i8,
                    right: SIDE as i8,
                    top: 0,
                    bottom: SIDE as i8,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    body(ui)
                })
                .inner
        })
        .inner
}

pub fn hairline(visuals: &egui::Visuals) -> Stroke {
    Stroke::new(
        1.0_f32,
        visuals
            .widgets
            .noninteractive
            .bg_stroke
            .color
            .gamma_multiply(0.6),
    )
}

pub fn place_title(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.set_min_height(ui.spacing().interact_size.y);
        ui.add_space(4.0);
        ui.label(RichText::new(title).size(24.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    ui.add_space(4.0);
}

pub fn page_title(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) -> bool {
    let side = ui.spacing().interact_size.y;
    let mut back = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        back = ui
            .add(header_button(icons::ARROW_LEFT, side))
            .on_hover_text("Back")
            .clicked();
        ui.add(egui::Label::new(RichText::new(title).size(20.0).strong()).truncate());
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
    ui.add_space(4.0);
    back
}

pub fn header_button(icon: &str, side: f32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(icon).size(20.0))
        .frame(false)
        .min_size(Vec2::splat(side))
}

pub fn section_color(visuals: &egui::Visuals) -> Color32 {
    visuals.text_color()
}

pub fn value_color(visuals: &egui::Visuals) -> Color32 {
    if visuals.dark_mode {
        visuals
            .text_color()
            .lerp_to_gamma(visuals.strong_text_color(), 0.08)
    } else {
        visuals.text_color().lerp_to_gamma(card_fill(visuals), 0.25)
    }
}

pub fn scrim(visuals: &egui::Visuals) -> Color32 {
    Color32::from_black_alpha(if visuals.dark_mode { 170 } else { 100 })
}

pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(text)
                .size(13.0)
                .color(section_color(ui.visuals())),
        );
    });
    ui.add_space(2.0);
}

pub fn section_label_button(ui: &mut Ui, text: &str, button: &str, hint: &str) -> bool {
    let mut clicked = false;
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(text)
                .size(13.0)
                .color(section_color(ui.visuals())),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            clicked = ui
                .add(
                    egui::Button::new(button)
                        .frame(false)
                        .min_size(egui::vec2(crate::app::mobile::touch_square(ui), 0.0)),
                )
                .on_hover_text(hint)
                .clicked();
        });
    });
    ui.add_space(2.0);
    clicked
}

#[cfg(not(target_arch = "wasm32"))]
pub fn section_label_with(ui: &mut Ui, text: &str, right: &str) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let secondary = section_color(ui.visuals());
        ui.label(RichText::new(text).size(13.0).color(secondary));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(right).size(13.0).color(secondary));
        });
    });
    ui.add_space(2.0);
}

pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(card_fill(ui.visuals()))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::symmetric(0, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui)
        })
        .inner
}

pub fn padded<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui)
        })
        .inner
}

pub fn divider(ui: &mut Ui) {
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter()
        .hline(rect.left()..=rect.right(), y, hairline(ui.visuals()));
}

pub struct Row<'a> {
    icon: Option<&'a str>,
    label: &'a str,
    detail: Option<&'a str>,
    info: Option<&'a str>,
    value: Option<String>,
    chevron: bool,
    trailing: &'a str,
    checked: Option<bool>,
    danger: bool,
    accent: bool,
}

impl<'a> Row<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            icon: None,
            label,
            detail: None,
            info: None,
            value: None,
            chevron: true,
            trailing: icons::CARET_RIGHT,
            checked: None,
            danger: false,
            accent: false,
        }
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = Some(detail).filter(|d| !d.is_empty());
        self
    }

    pub fn info(mut self, info: &'a str) -> Self {
        self.info = Some(info);
        self
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        let value = value.into();
        self.value = (!value.is_empty()).then_some(value);
        self
    }

    pub fn no_chevron(mut self) -> Self {
        self.chevron = false;
        self
    }

    pub fn trailing(mut self, icon: &'a str) -> Self {
        self.trailing = icon;
        self.chevron = true;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self.chevron = false;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let height = row_height(ui, self.detail.is_some());
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let visuals = ui.visuals();
        if response.is_pointer_button_down_on() || response.has_focus() {
            ui.painter().rect_filled(
                rect.shrink2(vec2(4.0, 1.0)),
                CornerRadius::same(10),
                visuals.widgets.hovered.weak_bg_fill.gamma_multiply(0.6),
            );
        }
        let strong = if self.danger {
            visuals.error_fg_color
        } else {
            visuals.strong_text_color()
        };
        let weak = visuals.weak_text_color();
        let secondary = value_color(visuals);
        let mut left = rect.left() + 12.0;
        if let Some(icon) = self.icon {
            let tile = Rect::from_center_size(
                egui::pos2(left + ICON_TILE / 2.0, rect.center().y),
                Vec2::splat(ICON_TILE),
            );
            ui.painter()
                .rect_filled(tile, CornerRadius::same(10), icon_tile_fill(visuals));
            ui.painter().text(
                tile.center(),
                Align2::CENTER_CENTER,
                icon,
                FontId::proportional(20.0),
                if self.danger {
                    strong
                } else {
                    visuals.text_color()
                },
            );
            left += ICON_COLUMN;
        }
        let mut right = rect.right() - 12.0;
        if let Some(checked) = self.checked {
            if checked {
                ui.painter().text(
                    egui::pos2(right, rect.center().y),
                    Align2::RIGHT_CENTER,
                    icons::CHECK,
                    FontId::proportional(18.0),
                    crate::app::ui_state::ui_theme::brand_mark(visuals),
                );
            }
            right -= 28.0;
        }
        if self.chevron {
            ui.painter().text(
                egui::pos2(right, rect.center().y),
                Align2::RIGHT_CENTER,
                self.trailing,
                FontId::proportional(15.0),
                weak,
            );
            right -= 22.0;
        }
        if !self.chevron && self.checked.is_none() {
            right = rect.right() - VALUE_EDGE;
        }
        if let Some(value) = &self.value {
            let color = if self.accent {
                crate::app::ui_state::ui_theme::brand_mark(visuals)
            } else {
                secondary
            };
            let galley = one_line(ui, value, 15.0, color, (right - left) * 0.55);
            let width = galley.size().x.min((right - left) * 0.55);
            let pos = egui::pos2(right - width, rect.center().y - galley.size().y / 2.0);
            ui.painter()
                .with_clip_rect(Rect::from_min_max(
                    egui::pos2(right - width, rect.top()),
                    egui::pos2(right, rect.bottom()),
                ))
                .galley(pos, galley, color);
            right -= width + 10.0;
        }
        let text_rect = Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(right, rect.bottom()),
        );
        let info_width = if self.info.is_some() {
            crate::app::widgets::info_mark::width(ui) + 6.0
        } else {
            0.0
        };
        let label = one_line(ui, self.label, 16.0, strong, text_rect.width() - info_width);
        let label_size = label.size();
        let top = match self.detail {
            Some(detail) => {
                let detail = one_line(ui, detail, 13.0, secondary, text_rect.width());
                let top = rect.center().y - (label.size().y + detail.size().y + 2.0) / 2.0;
                let detail_top = top + label.size().y + 2.0;
                ui.painter().galley(egui::pos2(left, top), label, strong);
                ui.painter()
                    .galley(egui::pos2(left, detail_top), detail, secondary);
                top
            }
            None => {
                let top = rect.center().y - label.size().y / 2.0;
                ui.painter().galley(egui::pos2(left, top), label, strong);
                top
            }
        };
        if let Some(info) = self.info {
            let line = top + label_size.y / 2.0;
            let mark = Rect::from_min_max(
                egui::pos2(left + label_size.x + 6.0, line - height / 2.0),
                egui::pos2(left + label_size.x + info_width, line + height / 2.0),
            );
            let mut cell = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(mark)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            crate::app::widgets::info_mark::info_mark(&mut cell, info);
        }
        response.widget_info(|| match self.checked {
            Some(checked) => egui::WidgetInfo::selected(
                egui::WidgetType::RadioButton,
                ui.is_enabled(),
                checked,
                self.label,
            ),
            None => {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), self.label)
            }
        });
        response
    }
}

fn row_height(ui: &Ui, detail: bool) -> f32 {
    let base = ROW_HEIGHT.max(ui.spacing().interact_size.y + 6.0);
    if detail { base + 8.0 } else { base }
}

pub fn one_line(
    ui: &Ui,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(FontId::proportional(size), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub fn rows(ui: &mut Ui, rows: Vec<Row<'_>>) -> Option<usize> {
    let mut clicked = None;
    card(ui, |ui| {
        let count = rows.len();
        for (index, row) in rows.into_iter().enumerate() {
            if row.show(ui).clicked() {
                clicked = Some(index);
            }
            if index + 1 < count {
                divider(ui);
            }
        }
    });
    clicked
}

pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let height = ui.spacing().interact_size.y.max(26.0);
    let (cell, mut response) = ui.allocate_exact_size(vec2(52.0, height), Sense::click());
    let rect = Rect::from_center_size(cell.center(), vec2(44.0, 26.0));
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_rect_visible(cell) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        paint_switch(ui, rect, t);
    }
    response
}

pub fn paint_switch(ui: &Ui, rect: Rect, t: f32) {
    let visuals = ui.visuals();
    let off = visuals.widgets.inactive.bg_fill;
    let fill = off.lerp_to_gamma(visuals.selection.bg_fill, t);
    let radius = rect.height() / 2.0;
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius as u8), fill);
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = Color32::WHITE;
    ui.painter().circle_filled(
        egui::pos2(x, rect.center().y),
        radius - (radius * 0.23).round(),
        knob,
    );
}

pub struct SwitchRow<'a> {
    label: &'a str,
    detail: Option<&'a str>,
    info: Option<&'a str>,
    on: &'a mut bool,
    enabled: bool,
}

impl<'a> SwitchRow<'a> {
    pub fn new(label: &'a str, on: &'a mut bool) -> Self {
        Self {
            label,
            detail: None,
            info: None,
            on,
            enabled: true,
        }
    }

    pub fn detail(mut self, detail: Option<&'a str>) -> Self {
        self.detail = detail.filter(|d| !d.is_empty());
        self
    }

    pub fn info(mut self, info: &'a str) -> Self {
        self.info = Some(info);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn show(self, ui: &mut Ui) -> bool {
        let height = row_height(ui, self.detail.is_some());
        let (rect, row) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let enabled = self.enabled && ui.is_enabled();
        row.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, *self.on, self.label)
        });
        let mut changed = false;
        if enabled && row.clicked() {
            *self.on = !*self.on;
            changed = true;
        }
        let switch_width = 52.0;
        let switch_rect = Rect::from_center_size(
            egui::pos2(rect.right() - 12.0 - switch_width / 2.0, rect.center().y),
            vec2(switch_width, height),
        );
        let mut cell = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(switch_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        cell.add_enabled_ui(self.enabled, |ui| {
            changed |= switch(ui, self.on).changed();
        });
        if !ui.is_rect_visible(rect) {
            return changed;
        }
        let visuals = ui.visuals();
        let strong = if self.enabled {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        };
        let secondary = value_color(visuals);
        let left = rect.left() + 12.0;
        let info_width = if self.info.is_some() {
            crate::app::widgets::info_mark::width(ui)
        } else {
            0.0
        };
        let room = switch_rect.left() - 8.0 - left - info_width;
        let label = one_line(ui, self.label, 16.0, strong, room);
        let detail = self
            .detail
            .map(|detail| one_line(ui, detail, 13.0, secondary, room));
        let block = label.size().y + detail.as_ref().map_or(0.0, |d| d.size().y + 2.0);
        let top = rect.center().y - block / 2.0;
        let label_width = label.size().x;
        let label_height = label.size().y;
        ui.painter().galley(egui::pos2(left, top), label, strong);
        if let Some(detail) = detail {
            ui.painter().galley(
                egui::pos2(left, top + label_height + 2.0),
                detail,
                secondary,
            );
        }
        if let Some(info) = self.info {
            let line = top + label_height / 2.0;
            let mark = Rect::from_min_max(
                egui::pos2(left + label_width + 6.0, line - height / 2.0),
                egui::pos2(left + label_width + 6.0 + info_width, line + height / 2.0),
            );
            let mut cell = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(mark)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            crate::app::widgets::info_mark::info_mark(&mut cell, info);
        }
        changed
    }
}

pub struct List<'u> {
    ui: &'u mut Ui,
    count: usize,
}

impl List<'_> {
    fn gap(&mut self) {
        if self.count > 0 {
            divider(self.ui);
        }
        self.count += 1;
    }

    pub fn row(&mut self, row: Row<'_>) -> Response {
        self.gap();
        row.show(self.ui)
    }

    pub fn switch(&mut self, row: SwitchRow<'_>) -> bool {
        self.gap();
        row.show(self.ui)
    }

    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }

    pub fn custom<R>(&mut self, body: impl FnOnce(&mut Ui) -> R) -> R {
        self.gap();
        body(self.ui)
    }
}

pub fn list<R>(ui: &mut Ui, body: impl FnOnce(&mut List<'_>) -> R) -> R {
    card(ui, |ui| body(&mut List { ui, count: 0 }))
}

fn pill_colors(ui: &Ui, primary: bool) -> (Color32, Color32) {
    let visuals = ui.visuals();
    if primary && ui.is_enabled() {
        let fill = crate::app::ui_state::ui_theme::brand(visuals);
        (fill, crate::app::ui_state::ui_theme::ink_on(fill))
    } else {
        (icon_tile_fill(visuals), visuals.strong_text_color())
    }
}

pub fn pill(ui: &mut Ui, text: &str, primary: bool) -> Response {
    let (fill, ink) = pill_colors(ui, primary);
    let height = ui.spacing().interact_size.y.max(36.0);
    ui.add(
        egui::Button::new(RichText::new(text).size(15.0).color(ink))
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same((height / 2.0) as u8))
            .min_size(vec2(height * 1.8, height)),
    )
}

pub fn wide_button(ui: &mut Ui, text: &str, primary: bool) -> Response {
    let colors = pill_colors(ui, primary);
    wide(ui, text, colors)
}

pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    let colors = if ui.is_enabled() {
        (
            crate::app::ui_state::ui_theme::danger(ui.visuals()),
            crate::app::ui_state::ui_theme::ON_DANGER,
        )
    } else {
        pill_colors(ui, false)
    };
    wide(ui, text, colors)
}

fn wide(ui: &mut Ui, text: &str, (fill, ink): (Color32, Color32)) -> Response {
    let height = ROW_HEIGHT.max(ui.spacing().interact_size.y);
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Button::new(RichText::new(text).size(16.0).color(ink))
                .fill(fill)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(RADIUS))
                .min_size(vec2(ui.available_width(), height)),
        )
    })
    .inner
}

pub fn field(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        if visuals.dark_mode {
            visuals.selection.stroke.color = crate::app::ui_state::ui_theme::focus_ring(visuals);
        }
        let widgets = &mut visuals.widgets;
        for state in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            state.corner_radius = CornerRadius::same(10);
        }
        let height = ui.spacing().interact_size.y.max(40.0);
        let width = ui.available_width();
        ui.add(
            edit.font(FontId::proportional(16.0))
                .vertical_align(Align::Center)
                .margin(Margin::symmetric(10, 0))
                .desired_width(width)
                .min_size(vec2(0.0, height)),
        )
    })
    .inner
}

pub fn primary_button(ui: &mut Ui, text: &str) -> bool {
    wide_button(ui, text, true).clicked()
}
